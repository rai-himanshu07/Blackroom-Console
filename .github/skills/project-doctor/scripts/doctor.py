#!/usr/bin/env python3
"""Surface-aware, read-only checks for an installed adaptive workflow."""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any


TOKEN_PATTERN = re.compile(r"\{\{[A-Z0-9_]+\}\}")
FIELD_PATTERN = re.compile(r"^-\s+([^:]+):\s+`?([^`\n]+)`?\s*$", re.MULTILINE)
ACTIVE_PLAN_PATTERN = re.compile(r"^\*\*Active plan:\*\*\s*(.+?)\s*$", re.MULTILINE)
TASK_TIER_PATTERN = re.compile(r"^\*\*Task tier:\*\*\s*([0-3])\s*$", re.MULTILINE)
CONTRACT_PATTERN = re.compile(r"^## Installed file contract\s*$([\s\S]*?)(?=^## |\Z)", re.MULTILINE)
PLAN_BUDGETS = {"none": 0, "mini": 25, "compact": 80, "governed": 0}
VALID_SURFACES = {"minimal", "standard", "governed"}
FALLBACK_FILES = ("AGENTS.md", ".github/copilot-instructions.md", "docs/WORKFLOW_CONFIG.md")


@dataclass(frozen=True)
class Finding:
    severity: str
    code: str
    message: str
    path: str = ""


class Report:
    def __init__(self) -> None:
        self.findings: list[Finding] = []

    def add(self, severity: str, code: str, message: str, path: Path | str = "") -> None:
        self.findings.append(Finding(severity, code, message, str(path)))

    @property
    def errors(self) -> int:
        return sum(item.severity == "error" for item in self.findings)

    @property
    def warnings(self) -> int:
        return sum(item.severity == "warning" for item in self.findings)

    def to_dict(self) -> dict[str, Any]:
        return {
            "status": "error" if self.errors else "warning" if self.warnings else "ok",
            "errors": self.errors,
            "warnings": self.warnings,
            "findings": [asdict(item) for item in self.findings],
        }


def _project_path(root: Path, relative: str) -> Path | None:
    normalized = relative.replace("\\", "/")
    rel = Path(normalized)
    if (
        not normalized
        or rel.is_absolute()
        or ".." in rel.parts
        or any(part in {"", "."} for part in rel.parts)
    ):
        return None
    current = root
    for part in rel.parts:
        current = current / part
        if current.is_symlink():
            return None
    try:
        current.resolve(strict=False).relative_to(root.resolve(strict=False))
    except (OSError, ValueError):
        return None
    return current


def _project_file(root: Path, relative: str) -> Path | None:
    path = _project_path(root, relative)
    return path if path is not None and path.is_file() else None


def _read(path: Path, report: Report, *, root: Path | None = None) -> str | None:
    try:
        if path.is_symlink() or not path.is_file():
            return None
        if root is not None:
            try:
                relative = path.relative_to(root).as_posix()
            except ValueError:
                return None
            if _project_file(root, relative) != path:
                return None
        return path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        report.add("error", "read.failed", str(error), path)
        return None


def _policy(root: Path, report: Report) -> dict[str, Any]:
    path = root / "docs" / "WORKFLOW_CONFIG.md"
    text = _read(path, report, root=root)
    if text is None:
        report.add("error", "policy.missing", "generated workflow policy is missing", path)
        return {
            "installation surface": "standard",
            "plan tier": "compact",
            "required files": list(FALLBACK_FILES),
            "mcp choices": [],
        }
    fields = {name.strip().lower(): value.strip() for name, value in FIELD_PATTERN.findall(text)}
    surface = fields.get("installation surface")
    tier = fields.get("plan tier")
    if surface not in VALID_SURFACES:
        report.add("error", "policy.surface", f"invalid installation surface: {surface!r}", path)
        surface = "standard"
    if tier not in PLAN_BUDGETS:
        report.add("error", "policy.plan-tier", f"invalid plan tier: {tier!r}", path)
        tier = "compact"
    contract_match = CONTRACT_PATTERN.search(text)
    required = (
        re.findall(r"^-\s+`([^`]+)`\s*$", contract_match.group(1), re.MULTILINE)
        if contract_match
        else list(FALLBACK_FILES)
    )
    if not required:
        report.add(
            "error",
            "policy.file-contract",
            "installed file contract is empty",
            path,
        )
        required = list(FALLBACK_FILES)
    mcp_text = fields.get("mcp choices", "none")
    mcp_choices = [] if mcp_text.lower() == "none" else [
        item.strip() for item in mcp_text.split(",") if item.strip()
    ]
    return {
        "installation surface": surface,
        "plan tier": tier,
        "required files": required,
        "mcp choices": mcp_choices,
    }


def _required_files(root: Path, required: list[str], report: Report) -> None:
    for relative in required:
        path = _project_file(root, relative)
        if path is None:
            report.add("error", "file.required", "required workflow file is missing", relative)


def _tokens(root: Path, report: Report) -> None:
    for base in ("AGENTS.md", ".github", "docs"):
        path = root / base
        candidates = [path] if path.is_file() else path.rglob("*") if path.is_dir() else ()
        for candidate in candidates:
            if not candidate.is_file() or candidate.is_symlink():
                continue
            text = _read(candidate, report, root=root)
            if text and TOKEN_PATTERN.search(text):
                report.add(
                    "error",
                    "template.unresolved",
                    "unresolved template token",
                    candidate.relative_to(root),
                )


def _task_state(root: Path, tier: str, report: Report) -> None:
    handoff_path = root / "docs" / "HANDOFF.md"
    if not handoff_path.is_file():
        if tier not in {"none", "mini"}:
            report.add("error", "handoff.missing", "handoff is required by this plan tier", handoff_path)
        return
    handoff = _read(handoff_path, report, root=root)
    if handoff is None:
        return
    task_match = TASK_TIER_PATTERN.search(handoff)
    if task_match:
        tier = {
            "0": "none",
            "1": "mini",
            "2": "compact",
            "3": "governed",
        }[task_match.group(1)]
    if len(handoff.splitlines()) > 40 or len(handoff.encode("utf-8")) > 3_072:
        report.add("warning", "handoff.budget", "handoff exceeds 40 lines or 3 KB", handoff_path)
    match = ACTIVE_PLAN_PATTERN.search(handoff)
    if not match:
        report.add("error", "plan.pointer", "handoff has no active-plan field", handoff_path)
        return
    value = match.group(1).strip().strip("`")
    if value.lower() == "none":
        if tier not in {"none", "mini"}:
            report.add("error", "plan.required", f"{tier} policy requires an active plan", handoff_path)
        return
    plan = _project_path(root, value)
    if plan is None:
        report.add(
            "error",
            "plan.path",
            "active plan must be a regular project-relative file with no symlink components",
            value,
        )
        return
    if not plan.is_file():
        report.add("error", "plan.missing", "active plan does not exist", value)
        return
    text = _read(plan, report, root=root)
    if text is None:
        report.add("error", "plan.missing", "active plan does not exist", value)
        return
    budget = PLAN_BUDGETS[tier]
    if budget and len(text.splitlines()) > budget:
        report.add("warning", "plan.budget", f"{tier} plan exceeds {budget} lines", value)


def _structured_files(
    root: Path,
    report: Report,
    selected_mcp: list[str],
) -> None:
    mcp_path = root / ".vscode" / "mcp.json"
    if mcp_path.exists():
        text = _read(mcp_path, report, root=root)
        if text:
            try:
                value = json.loads(text)
            except json.JSONDecodeError as error:
                report.add("error", "mcp.json", str(error), mcp_path)
            else:
                if not isinstance(value, dict) or not isinstance(value.get("servers"), dict):
                    report.add("error", "mcp.schema", "mcp.json must contain a servers object", mcp_path)
                else:
                    missing = sorted(
                        set(selected_mcp) - set(value["servers"])
                    )
                    if missing:
                        report.add(
                            "error",
                            "mcp.selected-missing",
                            "selected MCP servers are absent: "
                            + ", ".join(missing),
                            mcp_path,
                        )
    elif selected_mcp:
        report.add(
            "error",
            "mcp.missing",
            "selected MCP configuration is missing",
            mcp_path,
        )
    for hook in (root / ".github" / "hooks").glob("*.json"):
        text = _read(hook, report, root=root)
        if not text:
            continue
        try:
            value = json.loads(text)
        except json.JSONDecodeError as error:
            report.add("error", "hook.json", str(error), hook)
            continue
        if not isinstance(value, dict) or value.get("version") != 1:
            report.add("error", "hook.schema", "hook config must use version 1", hook)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--strict", action="store_true")
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = args.root.expanduser().resolve()
    report = Report()
    if not root.is_dir():
        report.add("error", "root.invalid", "project root is not a directory", root)
    else:
        policy = _policy(root, report)
        _required_files(root, policy["required files"], report)
        _tokens(root, report)
        _task_state(root, policy["plan tier"], report)
        _structured_files(root, report, policy["mcp choices"])
    payload = report.to_dict()
    if args.json:
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        for item in report.findings:
            location = f" [{item.path}]" if item.path else ""
            print(f"{item.severity.upper()} {item.code}: {item.message}{location}")
        print(f"{payload['status'].upper()}: {report.errors} error(s), {report.warnings} warning(s)")
    return 1 if report.errors or (args.strict and report.warnings) else 0


if __name__ == "__main__":
    raise SystemExit(main())
