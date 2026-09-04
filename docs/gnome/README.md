# docs/gnome/

GNOME/Mutter/Wayland research artifacts for the feasibility PoC (Document 10).

- `api-inventory.md` — introspected `org.gnome.Mutter.*`, `ScreenShield`, `ScreenSaver`,
  and `login1` interfaces actually exposed on the target host (Phase 1 step 8).
- `feasibility-research.md` — Document 00 §50 research topics for GNOME 50.1 / Mutter 50.1,
  with sources and a `CONFIRMED / LIKELY / UNVERIFIED / UNSUPPORTED` label per finding
  (Phase 1 step 9).
- `capability-report.md` — Document 20 §8 capability constants classified with the
  Document 00 §35 compatibility tiers (Phase 1 step 10).
- `introspection/` — raw D-Bus `Introspect()` XML captured by Experiment 2, one file per
  interface/object path.

All content here is produced by **read-only** introspection; see
`docs/ops/experiment-safety.md` before any experiment that changes display or input state.
