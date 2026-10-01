Experiment: Experiment 11 — Lock semantics (FEAS-A)
Date: 2026-10-01T15:45:16.595982355Z
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
result=FAIL; blocked=None; failure=None; aborted=Some("pre-lock checks failed, not locking: []"); inconclusive=[]; stages=1; lock_after_ms=None; unlock_after_ms=None; beats_during_lock=0; capture=[CaptureWindow { phase: "before_lock", frames: 0 }]; violations=["capture before_lock: 0 frames"]; shell Some("6842")->Some("6842")

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
