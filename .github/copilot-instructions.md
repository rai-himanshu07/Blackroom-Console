# Copilot Behavior for Blackroom Console

Read `AGENTS.md` and `docs/WORKFLOW_CONFIG.md`; do not duplicate their commands.

- The installed surface is `governed`, but route each task by risk: inline/mini
  by default, compact for a coupled multi-crate change, governed for a release
  only. Delivery follows the MVP fast path in `AGENTS.md`.
- Research/docs/trivial configuration require no code tests.
- Search and reuse before adding code. Keep non-goals explicit and avoid
  speculative abstractions, dependencies, configuration, or cleanup.
- Test changed behavior or a named risk with the smallest affected check after
  a coherent slice. Run broad workspace checks once only for cross-crate
  integration, release, or a named cross-cutting risk; no code tests for docs.
- Continue adjacent work without asking after each commit. No independent
  review unless the user asks, plus one at release.
- Build the MVP end to end; passed gates stay accepted. Run live experiments
  only when they unblock the MVP, under the standing approval in
  `docs/ops/experiment-safety.md`; keep unproven behavior behind a flag.
- Use MemPalace according to `on-demand`, always with
  `wing="blackroom_console"` for project reads and writes.
- Use codebase-memory according to `on-demand` with project
  `Blackroom_Console`; name only tools exposed in the active session and
  verify current behavior in live files. Skip it for known-file/literal lookups;
  use Scout for orientation, Verify for task claims, and Auditor only for
  bounded exhaustive/security claims.
- Preserve hard path, secret, sandbox, destructive-operation, preview,
  collision, and transactional safeguards regardless of expert overrides.
- For live GNOME mutation, use the standing approval and kit in
  `docs/ops/experiment-safety.md`; a new risk class gets one chat sentence
  first. Never turn an unobserved privacy or restoration property into a PASS.
- Keep plans/handoffs current and compact when this surface installs them.
- Report blockers and failed checks; never claim unobserved success.
