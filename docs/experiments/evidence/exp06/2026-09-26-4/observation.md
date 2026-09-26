# HDMI-only owner-loss observation (2026-09-27 local time)

This addendum records independent observations after the exp06 process
self-signaled. It does not change the generated PARTIAL exp06 report or PASS
exp07 report. Times below are local IST (UTC+05:30).

- 00:10:40: `pre_kill.json`/`report.md` recorded HDMI-1 as the original sole
  logical output, Meta-0 as the active logical output, `PowerSaveMode=3`, an
  active restore timer, and ScreenCast owner PID 34735. The final checks
  preceded exp06's SIGKILL; the process did not write post-kill findings.
- 00:10:41: the retained user journal recorded `D-Bus client with active
  sessions vanished` and `Removed virtual monitor Meta-0` for GNOME Shell
  PID 34735. This, with the pre-kill state, establishes owner disappearance;
  it does not by itself prove physical display privacy.
- 00:11:07: the independent read-only exp02 inventory at
  `docs/experiments/evidence/exp02/2026-09-26-3/inventory.json` recorded
  HDMI-1 as the sole active logical output, before the watchdog service
  started. Both eDP-1 and HDMI-1 remained in the raw connector inventory.
- Between owner disappearance and the watchdog firing, a separate read-only
  `busctl get-property` returned `PowerSaveMode: i 3` and a D-Bus owner PID
  query still returned 34735. These were observed in the session's terminal
  output, not saved in exp02's JSON; this addendum preserves their provenance
  and the bounded time interval, not an invented exact timestamp.
- 00:11:32: the user journal recorded the named watchdog service starting
  exp07 with this run's exact `backup.json` path. At 00:11:33, exp07 logged
  `Result: PASS`. Its saved findings report `topology_matches=true`, no
  apply error, and no retry. A subsequent read-only power check returned
  `PowerSaveMode: i 0`; Shell PID stayed 34735, and Meta-0/timer were gone.

The operator watched without interacting: HDMI went blank and showed no
desktop content before its UI returned. The disabled built-in panel flickered
and then blanked. The flicker was too brief to determine whether any readable
desktop content appeared. The operator subsequently stated, "Consider the
flicker to be privacy approved." This is acceptance of that observed
behavior for this run, **not** a retrospective observation that no content
was visible. No photo/video of the flicker was captured.

Mutter restored HDMI-1 to the logical topology after owner loss, but the
contemporaneous power reading shows physical unblanking still depended on
the watchdog/exp07. This single run satisfies the HDMI-only process-death
diagnostic, not Gate FEAS-C's full physical-privacy and repeatability proof.