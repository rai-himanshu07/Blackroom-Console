# Blackroom Console Agent Guide

Project using Himanshu's adaptive local agent workflow.

## Commands

| Action | Command |
|---|---|
| Focused test | `cargo test --workspace` |
| Lint | `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings` |
| Typecheck | `cargo check --workspace --all-targets` |
| Run | `Not configured for this project.` |

`Not configured` means inspect existing manifests and ask before adding tooling.
Use the project's declared environment manager; never mix Conda and uv
dependency operations.

Toolchain (decided 2026-09-04, see `docs/plans/assessment-20260904-detailed-project-plan.md` §6):
Rust stable via rustup (`rust-toolchain.toml`) for every daemon, helper, CLI and test;
TypeScript with Node 22/npm for `web/` (`npm --prefix web run test|lint|typecheck` once it
exists). No Python environment manager; `docs/ENVIRONMENT_POLICY.md` applies only if Python
tooling is ever added. Security checks: `cargo audit` and `cargo deny check`. Real-GNOME
system tests live in `crates/blackroom-systest`, are `#[ignore]`d, and run only with
`BLACKROOM_SYSTEST=1` on a prepared host (never from plain `cargo test`).
Services are systemd units; the operator CLI is `blackroom`.

## Active Policy

- Surface: `governed`
- Plan tier: `governed`
- Validation tier: `broad`
- Memory: `required` in wing `blackroom_console`
- Code intelligence: `required` for project
  `Blackroom_Console`
- Review: `independent`

Detailed resolved policy and selected optional capabilities are in
`docs/WORKFLOW_CONFIG.md`.

## Working Rules

- Start from the named behavior, file, symbol, or failure.
- Inspect nearby conventions and reuse existing code before adding a helper,
  layer, dependency, public API, or configuration.
- State non-goals; do not implement plausible future work.
- Keep the diff single-purpose and never modify unrelated user work.
- Preserve required validation, errors, security, accessibility, operations,
  and recovery even when simplifying.
- Never expose credentials or perform destructive filesystem, database,
  deployment, or Git operations without explicit approval.
- Add tests only for changed behavior, a reproduced bug, or a named risk.
- Run the smallest affected check after a coherent slice; broaden once at the
  configured checkpoint rather than after every edit.
- Report failed or unavailable checks plainly.

## Task Routing

- Questions, research, docs, and trivial configuration need no plan artifact and
  no code tests.
- Bounded low-risk edits use an inline/mini plan and the smallest affected check.
- Coupled or multi-session changes use a compact plan and concise handoff.
- Security, migration, regulated, or cross-service work uses the governed
  specification path selected in `docs/WORKFLOW_CONFIG.md`.

Use MemPalace only according to the active memory policy and always scope
project operations to `blackroom_console`. Use codebase-memory only according to
the active code-intelligence policy; live files and project checks remain the
current source of truth.
