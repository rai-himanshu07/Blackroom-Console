Experiment: Experiment 11 — Lock semantics (FEAS-A)
Date: 2026-10-01T15:51:16.998018487Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1, single built-in display
Objective:
Show that a RemoteDesktop/EIS sender and a ScreenCast monitor capture attached before a session lock survive the lock and the operator's unlock, and that input injected while locked reaches no window behind the lock screen (that it reaches the lock screen itself is only the operator's observation, recorded separately).

Hypothesis:
The EIS connection and capture stay attached through lock and unlock; while locked the observer page sees no injected event; after the unlock the same connection delivers Shift, `a` and Left.

Procedure:
Preflight (unlocked session, remote desktop service inactive); observer page focused and fullscreen (5 s settle); CreateSession, Start, ConnectToEIS, bind; ScreenCast monitor capture attached; pre-lock Shift tap; `loginctl lock-session`; while locked: pointer +5/-5, Shift, three `x`, six seconds hold, Esc; operator unlocks with their own password; post-unlock Shift, `a`, Left; capture windows before, during and after.

Expected:
Lock observed by GetActive and LockedHint agreeing (the ScreenSaver owner is not mapped to the login1 session on this host, so this is not provenance-verified); page heartbeat continues while locked; the page tally while locked has no key, button, pointer or wheel event (keyboard silence is weak evidence because the page has no keyboard focus while locked; pointer and wheel are the witnesses); the post-unlock tally is exactly Shift, A and Left, plain; the capture delivers frames before the lock and after the unlock (a quiet locked screen only makes that window inconclusive); Shell PID unchanged.

Observed:
result=FAIL; blocked=None; failure=None; aborted=None; inconclusive=["capture before_lock: 0 frames in the phase, 0 since attaching (consumer_ended=false, error=None)", "capture during_lock: 0 frames in the phase, 0 since attaching (consumer_ended=false, error=None)", "capture after_unlock: 0 frames in the phase, 0 since attaching (consumer_ended=false, error=None)"]; stages=11; lock_after_ms=Some(776); unlock_after_ms=Some(34618); beats_during_lock=18; capture=[CaptureWindow { phase: "before_lock", frames: 0, total: 0 }, CaptureWindow { phase: "during_lock", frames: 0, total: 0 }, CaptureWindow { phase: "after_unlock", frames: 0, total: 0 }]; violations=["locked_pointer_right_5: refused:MutterUnavailable", "locked_pointer_left_5: refused:MutterUnavailable", "locked_key_tap_shift: refused:MutterUnavailable", "locked_key_tap_x_1: refused:MutterUnavailable", "locked_key_tap_x_2: refused:MutterUnavailable", "locked_key_tap_x_3: refused:MutterUnavailable", "locked_key_tap_escape: refused:MutterUnavailable", "unlocked_key_tap_shift: refused:MutterUnavailable", "unlocked_key_tap_a: refused:MutterUnavailable", "unlocked_key_tap_left: refused:MutterUnavailable", "unlocked tally: [\"ShiftLeft not exactly one down/up after the unlock\", \"KeyA not exactly one down/up after the unlock\", \"ArrowLeft not exactly one down/up after the unlock\"]", "EIS connection not ready after the unlock"]; shell Some("6842")->Some("6842")

Evidence:
- findings.json (this directory), including the page's tally at three points

Result:
FAIL

Failure:
(none)

Root Cause:
(none)

Security Impact:
The live session is locked once; only Shift, `x`, Esc, `a`, Left and a net-zero 5 px pointer move are injected; no password is ever typed or read by the program. PASS here does not promote FEAS-A by itself.

Recommended Action:
(none)

Follow-up:
FEAS-A decision and Architecture Review #1 are separate evidence reviews.
