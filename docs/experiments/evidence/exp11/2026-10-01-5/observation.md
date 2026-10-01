# exp11 lock semantics, attempt 5: FEAS-A criterion not met (EIS does not survive the lock)

Operator-supervised, session 2 (eDP-1 only), gnome-remote-desktop masked, no grab holder, no virtual monitor.
Attempts 1 and 2 were BLOCKED (page never opened in time), attempt 3 and 4 stopped before the lock by the
pre-lock capture gate (the stream delivers 0 frames while a fullscreen page is up; see the plan log). Attempt 5
(this directory, harness commit `6e08689`) is the first run that locked the session.

- Pre-lock: the injected Shift tap arrived at the page exactly once.
- `loginctl lock-session 2`: both signals (ScreenSaver `GetActive`, logind `LockedHint`) reported the lock after
  776 ms; `ActiveChanged(true)` arrived, `ActiveChanged(false)` after the operator's unlock.
- At the lock Mutter ended the EIS connection: the next drain showed `DeviceRemoved` twice, `SeatRemoved`,
  `Disconnected`, then "EIS device socket closed". Every injection after that (7 while locked, 3 after the
  unlock) was refused `MutterUnavailable`; the connection was not ready after the unlock either. Nothing was
  typed into the lock screen, so the operator's curtain/dots observation does not apply to this run.
- The page kept beating while locked (18 heartbeats) and its tally had no key, button, pointer or wheel event;
  that silence is vacuous because nothing was injected.
- Capture: the monitor stream delivered 0 frames in every phase (fullscreen page; the same code gave about 90
  frames/s on a busy desktop), so capture continuity through the lock is unmeasured.
- Shell PID 6842 before and after; the operator unlocked with their own password (no program ever touched it).

Cause (source-read, consistent with the observation): gnome-shell 50.1 `js/ui/main.js` `_sessionUpdated()` calls
`remoteAccessController.inhibit_remote_access()` whenever the session mode does not allow screencast, and the
`unlock-dialog` mode (locked) does not (`js/ui/sessionMode.js`; only the `user` mode sets `allowScreencast`).
Mutter documents that call as "Inhibits remote access sessions from being created and running. Any active remote
access session will be terminated." So on this GNOME every RemoteDesktop and ScreenCast session ends when the
screen locks and none can be created while it is locked (the refusal of a new `CreateSession` while locked is
not yet observed).

Result: the roadmap Phase 8 criteria "session locked before activation stays attached through lock" and "remote
input can drive the unlock dialog" cannot be met through Mutter RemoteDesktop on GNOME 50.1. This is a design
finding for Architecture Review #1, not a harness defect. The recovery note given before the run (SSH
`loginctl unlock-session 2`) is unverified and probably needs polkit authentication from an SSH session; use
`sudo loginctl unlock-session 2`.
