# Experiment 11: One Supervised Lock-Only Observation

**Date:** 2026-09-28
**Result:** PARTIAL; FEAS-A remains unproven
**Scope:** Current eDP-only Wayland session. No virtual monitor, real remote
input, physical input isolation, output disable, or remote-mode activation.

## Approval And Preflight

The operator confirmed work was saved, physical presence through manual unlock,
and a fresh key-based SSH login from a second device kept open. The operator
approved exactly one `loginctl lock-session 2` invocation followed by read-only
checks and manual local unlock, with no retries or additional live test.

Immediately before the call, login1 session `2` was UID `1000`, `seat0`,
`Type=wayland`, `Class=user`, active, `LockedHint=no`. GNOME Shell PID was
`6842`. Only `card1-eDP-1` reported kernel DRM `connected`; HDMI remained
physically unplugged. `ssh.socket` was active, GNOME remote desktop was
inactive, and no matching exp06/exp07 timer was present. The read-only lock
observer showed ScreenSaver `GetActive=false`, `LockedHint=false`, with
`screen_saver_session_verified=false` and classification `INDETERMINATE`.

## Single Run And Recovery

The approved `loginctl lock-session 2` returned successfully. The immediate
read-only check showed ScreenSaver active but login1 `LockedHint=no`. A later
read-only check showed both `GetActive=true` and `LockedHint=yes` for the same
active session and Shell PID `6842`; the observer still reported
`INDETERMINATE` because the ScreenSaver owner's PID is a per-user `gjs`
service outside login1 session scope. SSH stayed active and no experiment
timer appeared.

The operator reported the GNOME lock screen on the built-in panel with no
desktop content visible, then manually unlocked and confirmed the original
eDP desktop responsive and second-device SSH still open. Postflight login1
session `2` remained active Wayland on `seat0`, Shell PID remained `6842`,
`LockedHint=no`, ScreenSaver `GetActive=false`, only eDP was kernel-connected,
and SSH remained active with no matching timer. No Unlock method, output
mutation, input injection, or second lock call was made by the agent.

## Limits

This is one supervised lock-only transition and manual recovery, not a test of
remote unlock, ScreenCast/EIS continuity, physical isolation or unattended
rollback. The GNOME ScreenSaver service is not independently bound to the
selected login1 session, so its raw signal cannot certify same-session lock
semantics. Operator visual observation is not an independent capture of
physical privacy. FEAS-A/C/D/E/F/H and product remote mode remain unproven;
this run gives no authorization to repeat isolation or lock testing.