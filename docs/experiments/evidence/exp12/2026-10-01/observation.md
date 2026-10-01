# exp12 same session through lock, logind unlock and reconnect: PASS (replacement path)

Operator-supervised on session 2 (eDP-1 only), gnome-remote-desktop masked then restored to disabled, no grab
holder, no virtual monitor, no capture, harness commit `b336f01` (clean tree). The operator kept their hands off
and never typed a password; the program did the unlock.

- Pre-lock: the injected Shift tap arrived at the page exactly once.
- Lock 1: `loginctl lock-session 2` exit 0; both signals agreed after 761 ms; `ActiveChanged` true/false events
  were recorded for both cycles. The old EIS connection was already gone when the signals agreed
  (`DeviceRemoved` x2, `SeatRemoved`, `Disconnected`, socket closed), as in exp11.
- `CreateSession` while locked: refused, `org.freedesktop.DBus.Error.Failed: Session creation inhibited`. This
  confirms the gnome-shell 50.1 source reading (remote access is inhibited in the locked session mode).
- Locked tally: 0 key, 0 button, 0 pointer, 0 wheel events with 79 page heartbeats in the 20 s hold. Because the
  locked `CreateSession` was refused, nothing was injected while locked, so this only shows that nothing reached
  the page and that the page stayed alive; it is not a leak test of an accepted remote session.
- Unlock 1: `loginctl unlock-session 2` from the program, exit 0, no stderr, session reported unlocked after
  976 ms (method `loginctl`, not manual).
- Fresh RemoteDesktop + EIS session right after the unlock: accepted; Shift, `a`, Left each arrived exactly once,
  plain, no repeat, nothing untrusted.
- Disconnect, lock 2 (749 ms), 8 s hold, unlock 2 by logind (984 ms, exit 0).
- Same login session id (2), same GNOME Shell PID (6842), observer page served once and never reloaded.

Limits: one run, one layout (built-in eDP-1, one window checked: the observer page); the lock signals are not
provenance-verified (the ScreenSaver owner is not mapped to the login1 session); the unlock was by the session's
own uid, which logind authorises without polkit, so the lock screen is not a boundary against a same-user
process (an expected property, now observed); windows and workspaces other than the page were not inspected;
physical outputs and input isolation were not part of this run; the time for remote access to be allowed again
after an unlock was not measured beyond "accepted immediately".
