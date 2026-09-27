# Copilot Behavior for Blackroom Console

Read `AGENTS.md` and `docs/WORKFLOW_CONFIG.md`; do not duplicate their commands.

- The installed surface is `governed`, but route each task by risk: inline/mini
  for bounded work, compact for coupled work, governed for security, migration,
  release, or live display/input changes. Existing governed plans keep their tier.
- Research/docs/trivial configuration require no code tests.
- Search and reuse before adding code. Keep non-goals explicit and avoid
  speculative abstractions, dependencies, configuration, or cleanup.
- Test changed behavior or a named risk with the smallest affected check after
  a coherent slice. Run broad workspace checks once only for cross-crate
  integration, release, or a named cross-cutting risk; no code tests for docs.
- Continue adjacent approved offline work without asking after each commit.
  Independent review is one high-risk/release checkpoint, not a per-step gate.
- Build host, gateway, and UI paths offline despite unproven live gates; keep
  live activation disabled until the specific physical/privacy/input/recovery
  claim has evidence. Run only experiments needed for the supported core path,
  not every historical matrix, repeat, or optional hardware variant.
- Use MemPalace according to `on-demand`, always with
  `wing="blackroom_console"` for project reads and writes.
- Use codebase-memory according to `on-demand` with project
  `Blackroom_Console`; name only tools exposed in the active session and
  verify current behavior in live files. Skip it for known-file/literal lookups;
  use Scout for orientation, Verify for task claims, and Auditor only for
  bounded exhaustive/security claims.
- Preserve hard path, secret, sandbox, destructive-operation, preview,
  collision, and transactional safeguards regardless of expert overrides.
- For live GNOME mutation, use `docs/ops/experiment-safety.md` §7's fresh
  preflight and run-specific operator approval. Diagnostic risk acceptance
  never turns an unobserved privacy or restoration property into a PASS.
- Keep plans/handoffs current and compact when this surface installs them.
- Report blockers and failed checks; never claim unobserved success.
