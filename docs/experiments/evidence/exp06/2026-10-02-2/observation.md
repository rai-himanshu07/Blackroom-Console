# Integrated probe, 2026-10-02 13:15-13:16 (second run): PASS by the probe rule

Operator-run `docs/ops/live-integrated-run.sh` (commit `508e198`): remote session, virtual monitor with a streaming
consumer, isolate eDP-1, input grab on event2-5, remote Shift/A/Left judged by the observer page, restore with the kept
virtual monitor, capture stop, ScreenCast Stop, lock, logind unlock. The operator closed all other windows, left the
pointer mid-screen, pressed F11 and kept hands off. Tablet SSH was connected. This run followed the 12:50 run
(`../2026-10-02/`), which injected nothing because the page never regained focus.

Observed (`integrated.json`, `findings.json`, journal):
- Remote input under isolation: Shift, A and Left were accepted and the page tallied exactly those six key events (down and
  up each), with no pointer move, button or wheel event, no untrusted event and no judge notes. `integrated_pass` true.
- Focus: the page lost focus at 7.3 s (page clock, the isolation) and regained it by itself at 8.3 s, so the gated focus
  click was not needed and `focus_click` is None. The click path is therefore unexercised live. In the first run the page
  never regained focus; that run had other windows open and this one did not, which is a possible but unverified cause.
- Grab: 4 nodes isolated, phase `isolated` mid-hold and `idle` after, `grab_restored` true, no early release. Its read
  counter stayed 0: nobody touched the machine, so physical-input blocking was not exercised by this run. The LITEON
  dongle devices are not grabbed.
- Capture: 1408 frames in the 25 s hold with the consumer streaming through isolation and restore (0 frames had arrived at
  the isolation instant), no capture error.
- Display: 13:15:54 `Added virtual monitor Meta-0`, 13:16:21 EIS socket closed, 13:16:22 `Removed virtual monitor Meta-0`.
  No SIGSEGV and no new apport report (the /var/crash files date from before this run); Shell PID 3828099 and session 182
  unchanged. Final state: eDP-1 only, `topology_matches_original` and `configuration_hash_matches` true, PowerSaveMode 0,
  no post-Stop repair.
- Lock teardown: lock engaged after 687 ms, `loginctl unlock-session` exit 0, unlocked after 930 ms, same Shell.
- Cleanup: both watchdog timers and the kill timer stopped at 13:16:27, daemon and live directory gone,
  gnome-remote-desktop disabled/inactive, ACLs removed by the operator.

Limits: two real-session runs, one with injection and one pass; the crash fix survived both. Not shown: physical input
being blocked during the integrated hold, behaviour with other windows open, the focus-click path, the dongle devices,
repeat cycles, lock/disconnect recovery beyond this single teardown, and any other topology (eDP-only, scale 1.0).
