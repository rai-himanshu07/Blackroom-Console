# Blackroom Console Agent Guide

Project using Himanshu's adaptive local agent workflow.

## Commands

| Action | Command |
|---|---|
| Affected test | `cargo test -p <affected-crate> [test-filter]` |
| Workspace checkpoint | `cargo test --workspace` |
| Lint checkpoint | `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings` |
| Typecheck checkpoint | `cargo check --workspace --all-targets` |
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
- Delivery: MVP fast path (2026-10-02, `docs/plans/plan-20261002-mvp-fast-path.md`)
  overrides the roadmap order: build the working remote console end to end for
  the owner's own use. Gates already passed with limits (FEAS-A/C/D/E, integrated
  probe) are accepted; do not reopen them. Phases not on the MVP list stay
  deferred until the MVP works.
- Default plan tier: `mini` (inline); `compact` only for a coupled multi-crate
  change; `governed` only for a release.
- Validation: `focused` for each coherent change; one relevant broad checkpoint
  only for cross-crate integration, release, or a named cross-cutting risk.
- Memory: `on-demand` in wing `blackroom_console`; retrieve on resume or when
  a past decision controls the task, checkpoint durable decisions once.
- Code intelligence: `on-demand` for project `Blackroom_Console`; use for
  unknown structural impact, not known-file or literal lookups.
- Review: none unless the user asks for one, plus one at release. No review per
  mechanism, recovery change, or live run.

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
- Run the smallest affected check after a coherent slice. Do not rerun the
  workspace suite, dependency audit, doctor, and independent review for every
  small offline commit. Finish adjacent approved offline work without asking
  for step-by-step permission.
- Report failed or unavailable checks plainly.
- Live experiments only when they unblock the MVP and cannot be answered by
  code or an existing result. Standing approval and light evidence rules are in
  `docs/ops/experiment-safety.md` ("Standing approval"). Never equate an untested
  capability with a PASS, and say plainly what a run did not cover.

## Task Routing

- Questions, research, docs, and trivial configuration need no plan artifact and
  no code tests.
- Bounded low-risk edits use an inline/mini plan and the smallest affected check.
- Coupled or multi-session changes use a compact plan and concise handoff;
  update them at meaningful checkpoints, not after every step.
- Release work uses the governed path in `docs/WORKFLOW_CONFIG.md`. Everything
  else is mini or compact; live display/input work follows the standing approval
  rules, not a per-run governed path.
- Keep unproven live behavior behind a flag and name its gap in one line of the
  handoff. Fix defects in authorization, rollback, or recovery before relying on
  those paths; defer other gaps.

Use MemPalace only according to the active memory policy and always scope
project operations to `blackroom_console`. Use codebase-memory only according to
the active code-intelligence policy; live files and project checks remain the
current source of truth.
