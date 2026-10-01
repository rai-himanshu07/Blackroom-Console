# Supervised eDP-only isolation with operator photo and version-one hash check (2026-10-01)

**Scope:** one separately approved 45-second no-kill `--pause-after-isolate` run on the declared single-display
layout (built-in eDP-1; HDMI not connected). Preflight: `../2026-10-01-preflight.md`. The operator accepted the
risk of a Shell crash or logout, brief flicker and an assisted restore for this exact command only.

## Sequence

1. 16:58:28 exp06 PID `3470197` armed `blackroom-exp06-watchdog-1790854108` (45 s), persisted the backup (new
   schema: `configuration_hash`, `hash_version`, `outputs`, `primary_output`, `session_id`, `shell_pid`,
   `topology`) and isolated. Read-only checks afterwards showed raw `eDP-1` and `Meta-0`, only the virtual
   monitor active logically, `PowerSaveMode` OFF during isolation.
2. The operator photographed the panel from a phone (not copied to this repository) and reports it **fully
   black/blank**, no frozen frame, no desktop content. Whether the backlight stayed on was not recorded.
3. 16:59:14 the first watchdog invoked exp07 unattended: `Result: PASS`, `topology_matches: true`,
   `configuration_hash_matches: true` (version-one hash plus exact logical fields). Independent read at 17:01:06:
   logical `eDP-1` only, `PowerSaveMode` 0, Shell PID `6842`, session `2`; the operator confirmed the desktop was
   back. Exp06 was still paused (raw `Meta-0` present).
4. One Enter armed the separate cleanup timer `...-cleanup` (45 s) before local restore and ScreenCast Stop. Exp06
   finished `PARTIAL` by pause-mode design. Its `findings.json`: `stop_error` null,
   `virtual_connector_fully_gone` true, `post_stop_restore_attempted` false, `final_state` raw and logical
   `eDP-1` only, power 0, Shell PID `6842`, session `2`, cleanup timer active at the snapshot,
   `topology_matches_original` true. Afterwards: zero blackroom timers, no exp06/exp07 process,
   `gnome-remote-desktop` returned to inactive/disabled.

## Doc 02 section 13 states (per output, eDP-1)

1. Desktop pixels no longer routed: yes (only Meta-0 active logically).
2. Panel standby or no signal: panel appeared fully black; a DPMS-off state, not a frozen frame.
3. Hardware-generated messages: none reported.

## Interpretation and limits

- On this layout the earlier results are reproduced a third time: Mutter accepts a zero-physical topology,
  the watchdog restores it, final topology and the hash match, and the original Shell and session survive.
- This is the first live check of the version-one hash on the eDP-only layout; it matched.
- The photo exists only on the operator's phone; the privacy observation is the operator's statement.
- Not shown here: abnormal termination (Gate F, Phase 9), connected HDMI, hotplug during isolation, other modes,
  repeated cycles, the historical Shell SIGSEGV cause. This is not a repeat authorization.
