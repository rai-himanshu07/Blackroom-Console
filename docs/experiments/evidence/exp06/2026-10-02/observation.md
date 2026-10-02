# Integrated probe, 2026-10-02 12:49-12:51: no Shell crash; restore with the kept virtual monitor passed; remote input not tested

Operator-run `docs/ops/live-integrated-run.sh` (commit `aa0d524`, binaries built from `fa7efa0`): remote session, virtual
monitor with a streaming PipeWire consumer, isolate eDP-1, input grab on event2-5, restore with the virtual monitor kept
(`with_kept_virtual`, both watchdogs with `--keep-live-virtual`), capture stop, ScreenCast Stop, lock, logind unlock. The
operator opened the observer page, pressed F11 and kept hands off. Tablet SSH was connected.

Observed (journal, `findings.json`, `integrated.json`):
- 12:50:09 `Added virtual monitor Meta-0`, restore watchdog armed, isolation applied. 12:50:35 the EIS socket closed and
  `Removed virtual monitor Meta-0`. No SIGSEGV and no new apport report; Shell PID 3828099 and session 182 unchanged.
  This is the composition that crashed the Shell on 2026-10-01 (consumer streaming through the restore); it now survived
  once. One run is one observation, not a reliability claim.
- Capture: 1369 frames during the 25 s hold (about 55 fps) with the consumer streaming through isolation and restore, no
  capture error.
- Final state after Stop: raw and logical connectors `eDP-1` only, `topology_matches_original` and
  `configuration_hash_matches` true, PowerSaveMode 0, no post-Stop repair needed, `stop_error` null.
- Lock teardown: lock engaged after 674 ms, `loginctl unlock-session` exit 0, unlocked after 936 ms, same Shell. Both
  watchdog timers were stopped at 12:50:40. Daemon, live directory and kill timer were gone afterwards, the
  gnome-remote-desktop service was back to disabled/inactive, and the operator removed the input ACLs.
- Grab: the daemon isolated 4 nodes, reported phase `isolated` mid-hold and `idle` after, `grab_restored` true, no early
  release. Its read counter stayed 0 and the page saw no pointer, button or wheel events: nobody touched the machine, so
  this run says nothing about physical input being blocked.

Not met: `pass` is false. The observer page lost focus at 8.0 s after its load (transition focus=false, fullscreen=true,
visible), which matches the isolation time (12:50:09, with `meta_window_set_stack_position_no_sync` assertions at that
moment; page-clock to wall-clock alignment is inferred), and never regained it within the 15 s wait. The harness
therefore injected nothing: Shift, A and Left were not sent, so remote input under isolation is untested (FEAS-F/H
integrated part still open).
Harness defects seen: the end-of-hold tally still held the operator's pre-isolation F11 down/up (the epoch reset is
skipped when the page is not ready), producing 4 misleading `tally_notes`; `devices_seen_before_isolation` was 0 although
the remote session reported 4 devices.

Interpretation: the crash fix works on the real session once; whether the focus loss is caused by the isolation (windows
move to the virtual monitor and focus is dropped) was not isolated.
