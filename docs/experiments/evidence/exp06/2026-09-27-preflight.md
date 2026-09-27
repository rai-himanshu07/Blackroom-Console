# Experiment 6 eDP-only supervised preflight

**Status:** Preflight confirmed, launch pending as of 2026-09-27T09:01:53+00:00.
**Scope:** One `--pause-after-isolate --watchdog-seconds 45` run, no kill or repeat.

## Read-only baseline

- Selected login1 display session: `3`, active Wayland user session on `seat0`.
- GNOME Shell PID: `34735`.
- DisplayConfig serial: `26`; raw connectors: `eDP-1`, `HDMI-1`.
- Active logical monitor: `eDP-1` primary at `(0, 0)`, scale `1`, transform `0`.
- `PowerSaveMode=0` (ON); `ssh.socket` active.
- No pending `blackroom-exp06-watchdog-*` timer at preflight.
- `gnome-remote-desktop.service` originally inactive/disabled; now masked and
  inactive, verified at 2026-09-27T09:01:53+00:00. Restore to disabled/inactive.
- Operator previously confirmed fresh second-device key-based SSH; reconfirm
  it remains open, with work saved and observer present, immediately before launch.
- Operator confirmed saved work, SSH connected, eDP observation and approval of
  the exact one-run `--pause-after-isolate --watchdog-seconds 45` command.

## Fresh binaries

- `exp06_isolate_outputs` SHA-256: `514f0814cf84b2e6c2fad4ede5066f97d32c9edfdc2508cc5a6a63b12257433e`.
- `exp07_restore` SHA-256: `3674a329a30c35ac26808a7c0f1584510c2459c0cd4cdf1b4a4ee4ece3288930`.
- Fresh build, focused exp06/exp07 tests and `cargo fmt --check` passed.
- Exp06 now persists session ID and Shell PID; exp07 refuses a missing or
  mismatched origin before any restore write (including watchdog invocation).
- Exp06 now refuses to start unless `gnome-remote-desktop.service` is masked
  and inactive; the required state is verified above.

The full raw GetCurrentState response is intentionally not stored here because
it contains monitor serials. Exp06 will persist the exact restoration backup
in its own newly reserved evidence directory before changing the display.
If GNOME Shell restarts, never apply that backup to the new session.

## Isolated observation (run in progress)

- Exact command launched once; exp06 PID `940537` paused after isolation.
- Printed watchdog: `blackroom-exp06-watchdog-1790499779` (45 s).
- Printed backup: `docs/experiments/evidence/exp06/2026-09-27/backup.json`.
- Read-only GetCurrentState while isolated: raw `eDP-1`, `HDMI-1`, `Meta-0`;
  only `Meta-0` active logically. `PowerSaveMode=3` (OFF).
- Named timer was active/waiting, Shell PID remained `34735`, original login1
  session `3` remained active Wayland. Backup has `session_id=3`,
  `shell_pid=34735`, and eDP-1 as its saved logical output.
- At 09:03:43 UTC the watchdog service had **no execution timestamp**;
  `Result=success` while inactive is not restoration evidence. Await the
  watchdog; do not send Enter until restore is independently confirmed.