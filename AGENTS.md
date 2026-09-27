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
- Delivery: implement the core app offline first. A blocked live gate blocks
  activation and claims of support, not independent host, gateway, or UI code.
- Default plan tier: `compact` for coupled work, `mini` for bounded work;
  `governed` only for security, migrations, live host mutation, or releases.
- Validation: `focused` for each coherent change; one relevant broad checkpoint
  only for cross-crate integration, release, or a named cross-cutting risk.
- Memory: `on-demand` in wing `blackroom_console`; retrieve on resume or when
  a past decision controls the task, checkpoint durable decisions once.
- Code intelligence: `on-demand` for project `Blackroom_Console`; use for
  unknown structural impact, not known-file or literal lookups.
- Review: self-check routine edits; one independent review for a new high-risk
  mechanism, changed recovery contract, or release, not each offline increment
  or same-mechanism supervised diagnostic.

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
- Select only live experiments needed to resolve an unknown that blocks the
  supported core workflow: physical display privacy/final restoration, real
  EIS input and revocation, physical input isolation and emergency recovery,
  same-session lock and disconnect recovery. Use one bounded observation per
  distinct question; expand only after a specific failure or support decision.
  Defer optional matrices, repeated cycles, soak, cursor/GPU variants, and
  unrelated experiments. Never equate an untested capability with a PASS.

## Task Routing

- Questions, research, docs, and trivial configuration need no plan artifact and
  no code tests.
- Bounded low-risk edits use an inline/mini plan and the smallest affected check.
- Coupled or multi-session changes use a compact plan and concise handoff;
  update them at meaningful checkpoints, not after every step.
- Security, migration, release, or live display/input changes use the governed
  path in `docs/WORKFLOW_CONFIG.md`; operator approval of one diagnostic does
  not certify a product gate or authorize repeats.
- Keep unproven live behavior behind disabled activation and name its gap in
  the handoff. Fix defects in authorization, rollback, or recovery before
  relying on those paths; defer other gaps until the core path exists.

Use MemPalace only according to the active memory policy and always scope
project operations to `blackroom_console`. Use codebase-memory only according to
the active code-intelligence policy; live files and project checks remain the
current source of truth.
