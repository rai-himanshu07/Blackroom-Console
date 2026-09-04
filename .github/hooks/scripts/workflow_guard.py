#!/usr/bin/env python3
"""Fast Stop guard for configured local plan/handoff budgets."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any


PLAN_TIER = re.compile(r"^-\s+Plan tier:\s+`?([^`\n]+)`?\s*$", re.MULTILINE)
ACTIVE_PLAN = re.compile(r"^\*\*Active plan:\*\*\s*(.+?)\s*$", re.MULTILINE)
TASK_TIER = re.compile(r"^\*\*Task tier:\*\*\s*([0-3])\s*$", re.MULTILINE)
BUDGETS = {"none": 0, "mini": 25, "compact": 80, "governed": 0}


def _payload() -> dict[str, Any]:
    try:
        value = json.load(sys.stdin)
    except (json.JSONDecodeError, OSError):
        return {}
    return value if isinstance(value, dict) else {}


def _root(payload: dict[str, Any]) -> Path:
    value = payload.get("cwd") or payload.get("working_directory") or os.getcwd()
    return Path(str(value)).expanduser().resolve(strict=False)


def _read(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8") if path.is_file() and not path.is_symlink() else None
    except (OSError, UnicodeError):
        return None


def _project_file(root: Path, relative: str) -> Path | None:
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
    return current if current.is_file() else None


def _reason(root: Path) -> str | None:
    policy_path = _project_file(root, "docs/WORKFLOW_CONFIG.md")
    policy = _read(policy_path) if policy_path is not None else None
    match = PLAN_TIER.search(policy or "")
    tier = match.group(1).strip() if match else ""
    if tier not in BUDGETS:
        return "generated workflow policy is missing a valid plan tier"

    handoff_path = _project_file(root, "docs/HANDOFF.md")
    handoff = _read(handoff_path) if handoff_path is not None else None
    if handoff is None:
        return None if tier in {"none", "mini"} else "handoff is required by the active plan tier"
    if len(handoff.splitlines()) > 40 or len(handoff.encode("utf-8")) > 3_072:
        return "handoff exceeds the configured 40-line/3 KB current-state budget"
    task = TASK_TIER.search(handoff)
    if task:
        tier = {
            "0": "none",
            "1": "mini",
            "2": "compact",
            "3": "governed",
        }[task.group(1)]
    pointer = ACTIVE_PLAN.search(handoff)
    if pointer is None:
        return "handoff active-plan field is missing or ambiguous"
    value = pointer.group(1).strip().strip("`")
    if value.lower() == "none":
        return None if tier in {"none", "mini"} else f"{tier} policy requires an active plan"
    plan_path = _project_file(root, value)
    if plan_path is None:
        return (
            "handoff active plan must be a regular project-relative file with "
            "no symlink components"
        )
    plan = _read(plan_path)
    if plan is None:
        return "handoff active plan does not exist or is unreadable"
    budget = BUDGETS[tier]
    if budget and len(plan.splitlines()) > budget:
        return f"{tier} plan exceeds its {budget}-line budget"
    return None


def _workflow_changes(root: Path) -> bool:
    if not (root / ".git").exists():
        return False
    try:
        result = subprocess.run(
            [
                "git",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--",
                "AGENTS.md",
                ".github",
                "docs/WORKFLOW_CONFIG.md",
                "docs/HANDOFF.md",
                "docs/plans",
            ],
            cwd=root,
            capture_output=True,
            text=True,
            timeout=1,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return False
    return result.returncode == 0 and bool(result.stdout.strip())


def evaluate(payload: dict[str, Any]) -> dict[str, Any]:
    event = str(payload.get("hook_event_name") or payload.get("hookEventName") or "")
    if event and event not in {"Stop", "agentStop"}:
        return {}
    reason = _reason(_root(payload))
    if reason is None and _workflow_changes(_root(payload)):
        reason = (
            "workflow-control files changed; review scope and current task state "
            "before stopping"
        )
    if reason is None:
        return {}
    return {
        "permissionDecision": "ask",
        "permissionDecisionReason": reason,
        "hookSpecificOutput": {
            "hookEventName": event or "Stop",
            "permissionDecision": "ask",
            "permissionDecisionReason": reason,
        },
    }


def main() -> int:
    print(json.dumps(evaluate(_payload())))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
