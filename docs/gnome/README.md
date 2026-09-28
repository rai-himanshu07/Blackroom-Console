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

For a current, one-shot host observation, run `cargo run -p gnome-session-agent
-- --compatibility-report`. This queries the unique active Wayland user session
through logind and the existing read-only GNOME capability detector, prints
JSON, and exits without binding an agent socket. `phase3_session_gate_passed`
only describes the basic session gate; `remote_mode_allowed` remains `false`
regardless of the capability tiers. Some tiers encode earlier limited evidence,
not a fresh physical privacy or recovery test. A missing/ambiguous session
fails closed; this command neither activates remote mode nor proves FEAS-C.

`cargo run -p gnome-session-agent -- --lock-observation` reads GNOME
ScreenSaver `GetActive` and login1 `LockedHint` for the selected session,
without calling `Lock`. It checks the ScreenSaver owner's login1 session;
an owner outside login1 or mismatched lock signals yields `INDETERMINATE`,
while a known different session is refused. Even verified agreement is only
the current observed state, not FEAS-A or same-session remote-unlock proof.
