# Connected-HDMI pre-repair evidence and connector loss (2026-09-27)

**Result:** exp06 FAIL; first independent exp07 PASS; second exp07 FAIL.
The laptop's eDP desktop and SSH remained usable, but HDMI-1 disappeared from
kernel and Mutter raw inventory while its cable remained plugged in.
Gate FEAS-C STOP; this was one separately approved no-kill run with no repeat.

## Sequence

1. `../2026-09-27-5-preflight.md` records the exact command approval,
   original raw eDP-1/HDMI-1 with only eDP-1 active, power ON, Shell PID
   `34735`/Wayland session `3`, saved work, second-device SSH, fresh checked
   binaries, zero timers and temporary runtime remote-desktop mask.
2. Exp06 PID `1376190` reserved a version-one `backup.json` containing
   primary eDP-1 and connected-but-disabled HDMI-1 with no current mode. It
   armed `blackroom-exp06-watchdog-1790526436` for 45 seconds. During
   isolation, only virtual Meta-0 was logically active and power was OFF;
   the original Shell/session stayed active.
3. The first timer invoked exp07 and wrote `../../exp07/2026-09-27-8/`:
   PASS. An independent read showed eDP-1 alone active, HDMI-1 and Meta-0
   still raw, power ON and the same Shell/session. The operator confirmed a
   responsive laptop desktop and working second-device SSH before Enter.
4. One Enter armed `blackroom-exp06-watchdog-1790526436-cleanup` for 45
   seconds before ScreenCast Stop. Exp06's `findings.json` records FAIL,
   `post_stop_restore_attempted=true`, no local restore/Stop error, and a
   saved **pre-repair** snapshot with both eDP-1 and HDMI-1 logically active,
   `configuration_hash_matches=false`, power ON and unchanged Shell/session.
   Its recaptured final snapshot has only eDP-1 active, matching topology
   and version-one hash, and the second timer still active. Thus the local
   identity-checked reapply corrected the logical mismatch, but did not make
   this a clean final restoration.
5. Subsequent read-only GetCurrentState returned **only eDP-1 in raw
   inventory**, while eDP remained active and power ON. The second timer
   independently invoked exp07 and wrote `../../exp07/2026-09-27-9/`:
   **FAIL**. Its findings record no apply error and matching logical fields,
   but `configuration_hash_matches=false` because the saved raw HDMI-1
   output could not be found. The systemd service exited successfully even
   though the binary printed FAIL. A later synthetic-tested code fix makes
   future exp07 verification FAILs exit nonzero; this run's report is unchanged.
6. The operator confirmed the HDMI cable remained plugged in and the monitor
   was powered with no signal. One operator-controlled cable re-seat did not
   recover it: `/sys/class/drm/card0-HDMI-A-1/status` remained `disconnected`
   and Mutter still showed raw eDP-1 only. GNOME Shell PID `34735`, active
   Wayland session `3`, eDP desktop/power and second-device SSH remained
   usable. No experiment timer remains; the runtime mask was removed and
   remote desktop restored to disabled/inactive.

## Interpretation

This run proves the earlier connected-HDMI post-Stop logical reactivation
occurred again, and the local reapply corrected it once. It does **not**
explain why the physically connected HDMI output then disappeared at the
kernel level, restore its original raw inventory, establish physical display
privacy, or clear Gate FEAS-C. The bounded user journal logged monitor/work
area assertions at ScreenCast Stop, but that does not establish root cause.
No GPU reset, extra cable cycles, manual exp07 retry, or further experiment
was performed. Generated backup/findings/reports remain unchanged.