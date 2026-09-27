# Connected-HDMI guarded post-Stop repair (2026-09-27)

**Result:** exp06 FAIL because a post-Stop local reapply was required. Both
independent exp07 watchdog restorations PASS; the original eDP-only active
topology and usable desktop were recovered. Gate FEAS-C remains STOP.

## One-run sequence

1. The unique `../2026-09-27-4-preflight.md` records the exact one-run
   operator approval, saved work, second-device SSH, eDP observation, original
   Shell PID `34735`/Wayland session `3`, connected but inactive HDMI-1,
   zero old timers, original remote-desktop disabled/inactive state, and fresh
   binary hashes. The user service was temporarily runtime-masked.
2. The no-kill exp06 run used PID `1366692` and armed the first 45-second
   watchdog `blackroom-exp06-watchdog-1790525785`. The version-one backup
   persisted primary eDP-1 and raw HDMI-1 as disabled with no current mode.
   During isolation only virtual Meta-0 was active and power mode was OFF;
   the original Shell/session survived.
3. The first watchdog invoked exp07 and wrote `../../exp07/2026-09-27-6/`:
   PASS. An independent read showed eDP-1 alone active, HDMI-1 still raw,
   virtual Meta-0 still raw, power ON and the same Shell/session. The operator
   confirmed a responsive desktop and working second-device SSH before Enter.
4. One Enter armed `blackroom-exp06-watchdog-1790525785-cleanup` for 45
   seconds before ScreenCast Stop. Exp06 removed Meta-0 and reported FAIL.
   Its generated findings record `post_stop_restore_attempted=true`, no
   local restore/Stop error, and a recaptured final eDP-only topology with
   matching version-one hash, power ON and unchanged Shell/session. The
   cleanup timer was still active. The **initial mismatch that triggered**
   the local reapply was not persisted separately, so this run does not
   prove which field changed after Stop. The earlier connected-HDMI run
   separately documented HDMI reactivation at that point.
5. The still-armed cleanup timer independently invoked exp07 and wrote
   `../../exp07/2026-09-27-7/`: PASS. A separate check found only eDP-1
   logically active with eDP-1/HDMI-1 raw, power ON, PID `34735`, active
   session `3`, no virtual connector and zero pending experiment timers.
   Exp06 exited; the runtime mask was removed and remote desktop returned to
   disabled/inactive. The operator confirmed a normal responsive desktop and
   working second-device SSH.

## Interpretation

One identity-checked local reapply was observed to return the host to its
saved topology before the independent cleanup watchdog ran. It is not an
unassisted stable post-Stop restoration, a visual privacy observation, proof
of Mutter's cause, or Gate FEAS-C PASS. This run's approval is consumed;
generated backup/findings/reports remain unchanged and authorize no repeat.