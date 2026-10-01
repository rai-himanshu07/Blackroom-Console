# exp06 owner-loss probe on the sole eDP-1 panel, with the real input grab: Gate F eDP-only observation (one run)

Operator present, tablet SSH open, work saved, temporary ACLs on event2-5, gnome-remote-desktop masked, a 7 min
kill timer (1 s accuracy) and the 45 s exp07 restore watchdog armed. Harness commit `fcb7411`. Run:
`exp06_isolate_outputs --pause-after-isolate --watchdog-seconds 45 --auto-kill-after-isolate --grab-socket <sock>`
with `remote-emergencyd --client-uid 1000 --enable-grabs` as the grab holder.

Machine record:
- Pre-kill (`pre_kill.json`): original topology `eDP-1` only; virtual monitor `Meta-0` the only logical monitor;
  DPMS 3; Shell PID 6842 unchanged; watchdog timer active (671 ms after arming); the daemon isolated 4 nodes
  (event2-5).
- 22:34:17 `Added virtual monitor Meta-0`; 22:34:18 exp06 SIGKILLed itself; in the same second gnome-shell logged
  `D-Bus client with active sessions vanished` and `Removed virtual monitor Meta-0`; eDP-1 was back in the logical
  topology at the first check (22:34:33) while `PowerSaveMode` stayed 3.
- The daemon reported `idle`, `held 0` at 22:34:33, 15 s after the kill and well inside the 60 s lease: the grab was
  released by the owner's socket closing (the exact release time was not recorded).
- 22:35:03 the watchdog ran `exp07_restore`: PASS, monitors before restore `[eDP-1]`, no unexpected connectors,
  topology and configuration hash match, `PowerSaveMode` 0.
- Shell PID 6842 before and after; no crash, SIGSEGV or abort line in the user journal for the window.

Operator report (a summary choice, not timed): the panel showed no desktop content while blank, the picture came
back within about a minute, keyboard and touchpad worked, the desktop was intact.

Limits: one run, built-in eDP-1 only, same uid; the owner was the probe, not the product agent; no remote input,
capture frames or lock were part of the run; the blank interval before the watchdog (about 45 s with DPMS stuck at 3
until `exp07_restore`) is the observed recovery time, so the panel stays dark about that long after an owner death;
the exact input-release time and the picture-return time were not recorded by the machine; no repeat. The temporary
ACLs on event2-5 were still present when the cleanup was checked.
