# Supervised eDP-only isolation, HDMI unplugged (2026-09-27)

**Scope:** One separately approved 45-second no-kill pause diagnostic with
HDMI-1 physically disconnected. This is not a Gate FEAS-C pass or a repeat
authorization for the connected-HDMI layout.

## Sequence

1. The unique `../2026-09-27-2-preflight.md` records raw and logical eDP-1
   only, GNOME Shell PID `34735`, active login1 session `3`, power ON, fresh
   checked binary hashes, saved work, operator presence, second-device SSH,
   and the temporary remote-desktop mask.
2. Exp06 PID `967321` armed `blackroom-exp06-watchdog-1790501585` for 45 s.
   During isolation raw connectors were eDP-1 and Meta-0, with only Meta-0
   logically active and `PowerSaveMode=3`. The original Shell/session survived.
3. The first timer invoked exp07 at 09:34:38 UTC. Its
   `../../exp07/2026-09-27-3/report.md` reports PASS, no retry or apply error,
   and a matching original eDP-only logical topology. A separate D-Bus read
   showed eDP-1 active and power ON; the operator confirmed the built-in
   desktop was visible and responsive. Exp06 was still paused.
4. Only after this confirmation, one Enter resumed exp06. It verified the
   original Shell/session and armed a *second* 45-second timer named
   `blackroom-exp06-watchdog-1790501585-cleanup` before local restore and
   ScreenCast Stop. Its generated `report.md` is PARTIAL by pause-mode design,
   with no Stop error and Meta-0 fully gone. Its persisted `final_state`
   captured eDP-1 as the only raw and logical connector, power ON, PID
   `34735`, session `3`, no probe/read errors, and a matching original
   topology. The cleanup timer was active at that final snapshot, then
   independently observed dead/disarmed with no pending exp06 timer.
5. Final independent GetCurrentState still showed eDP-1 only and power ON;
   Shell PID/session were unchanged. The remote-desktop unit was returned to
   its original disabled/inactive state. The operator confirmed the built-in
   desktop was normal after cleanup. No second exp07 run was needed.

## Interpretation

The disconnected-HDMI run did not reproduce the earlier connected-but-disabled
HDMI-1 reactivation on owner cleanup. This narrows that earlier failure's
conditions; it does **not** prove its Mutter cause or show that the same path
is safe while HDMI remains physically connected. The first watchdog PASS
applies at the time exp07 checked it; the new exp06 final-state fields supply
the separate post-Stop observation. Generated reports and backup remain
unchanged. The operator's accepted built-in flicker risk is not independent
zero-content proof. Gate FEAS-C remains stopped on the connected-HDMI
restoration failure and unfinished matrix; no new isolation is authorized by
this observation.