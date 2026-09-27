# Connected-HDMI post-Stop restoration diagnostic (2026-09-27)

**Result:** exp06 FAIL; both independent exp07 restorations PASS. The original
eDP-only active topology and usable desktop were recovered. Gate FEAS-C STOP.

## One-run sequence

1. The unique `../2026-09-27-3-preflight.md` records operator approval, saved
   work, connected but inactive HDMI-1, sole active eDP-1, power ON, Shell PID
   `34735`, Wayland session `3`, fresh second-device SSH, zero prior timers,
   checked binary hashes and the temporary runtime remote-desktop mask.
2. The one approved no-kill `--pause-after-isolate --watchdog-seconds 45` run
   reserved `backup.json`, armed `blackroom-exp06-watchdog-1790521856` and
   reached virtual-only Meta-0 with `PowerSaveMode=3`. Shell/session survived.
3. The first watchdog invoked exp07 and wrote `../../exp07/2026-09-27-4/`:
   PASS. Independent GetCurrentState showed eDP-1 as the sole active output,
   HDMI-1 still raw, Meta-0 raw, power ON and Shell/session unchanged. The
   operator confirmed the built-in desktop was responsive and SSH worked.
4. After that confirmation, one Enter armed
   `blackroom-exp06-watchdog-1790521856-cleanup` for 45 seconds *before*
   local restore and ScreenCast Stop. Exp06 removed Meta-0 but reported FAIL:
   `findings.json` records raw eDP-1 and HDMI-1, **both logically active**,
   `topology_matches_original=false`, power ON, Shell PID `34735`, session
   `3`, no Stop or probe error, and the cleanup timer still active. This
   independently reproduces the earlier connected-HDMI reactivation.
5. The cleanup timer invoked exp07 and wrote `../../exp07/2026-09-27-5/`:
   PASS. A separate read confirmed only eDP-1 logically active, both physical
   connectors raw, power ON, Shell PID `34735`, active session `3`, Meta-0
   absent and no pending exp06/exp07 timer. Exp06 exited; the runtime mask
   was removed and remote desktop returned to disabled/inactive. The operator
   confirmed a normal responsive desktop and working second-device SSH.

## Interpretation

An exp07 PASS before owner cleanup was not a stable final restore with HDMI
connected. The second, independent watchdog recovered the original session;
this does **not** erase exp06's post-Stop FAIL, prove the cause inside Mutter,
or establish zero visible-content exposure. The previous unplugged-HDMI run
did not show this reactivation; the difference is diagnostic, not proof of
causality. The backup hash/primary-output acceptance gap and historical Shell
crash remain open. No repeat, other matrix row, or product activation is
authorized by this result. Generated backup, findings and reports are unchanged.