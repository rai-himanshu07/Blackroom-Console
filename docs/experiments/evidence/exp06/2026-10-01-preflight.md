# Experiment 6 eDP-only privacy and hash run: supervised preflight (2026-10-01)

**Status:** run completed 2026-10-01 (see `2026-10-01/observation.md`); original eDP-only state restored.
**Scope:** one `--pause-after-isolate --watchdog-seconds 45` run on the
declared single-display layout (built-in eDP-1; HDMI is not connected and kernel DRM reports HDMI-A-1
disconnected), no kill, no repeat. Goals: (1) an independent photo of the blank panel during isolation,
(2) the first live check of the version-one backup hash plus exact logical fields on this layout.
This run alone does not close Gate FEAS-C.

## Baseline (read-only, names only; monitor serials are not printed or stored)

- GetCurrentState serial `1`: raw connectors `eDP-1` only; one logical monitor containing `eDP-1`;
  `PowerSaveMode=0`; GNOME Shell PID `6842`; login1 session `2` on seat0 (tty2).
- `ssh.socket` active; tablet connected (two established connections from 192.168.1.51); no pending
  `blackroom-*` timer; no exp06/exp07 process.
- `gnome-remote-desktop.service` inactive/disabled; to be masked for the run and restored to
  disabled/inactive.
- Binaries built from the working tree at `53198a3`; `exp06_isolate_outputs` SHA-256
  `503a2805e8a181ac5304cc51cfc9e0d6bcd685f6022415f2d0d535f9ecdd1f9a`, `exp07_restore` SHA-256
  `fb274ebc8a1b3220c5fd236c24bd40ef9a430ce3d74b93241c28063e2df3ca38`. Since the 2026-09-27 runs exp06 changed
  only by the 1 s watchdog timer accuracy (`d36f201`, 2026-10-01); exp07 is unchanged.

## Procedure (per `docs/ops/experiment-safety.md` section 7)

Operator saves work, keeps the tablet SSH open and photographs the laptop panel with a phone during
the isolation window; the agent never expects a chat reply while the panel is blank. Exp06 prints the
backup path, PID and the 45 s timer; the watchdog invokes exp07, which must report PASS (hash plus fields).
Only after the desktop is back and the operator confirms it, one Enter arms the separate cleanup timer
before local restore and ScreenCast Stop. Final raw and logical state, power, Shell PID and session are
checked independently, then the remote-desktop unit is restored.

Accepted risk (operator, for this exact command and layout): a GNOME Shell crash or logout, brief panel
flicker, and a restore that needs the watchdog or SSH. Not a repeat authorization.
