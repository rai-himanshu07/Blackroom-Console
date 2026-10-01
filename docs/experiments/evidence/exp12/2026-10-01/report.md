Experiment: Experiment 12 — Same session through lock, unlock and reconnect (FEAS-A)
Date: 2026-10-01T16:20:52.718535017Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1, single built-in display
Objective:
Show that, with remote access inhibited while the screen is locked (exp11), the session can still be locked, unlocked by logind from a user process, and driven by a fresh RemoteDesktop/EIS session after the unlock, with the same login session, Shell and observer page instance throughout.

Hypothesis:
Nothing injected before the lock reaches the page while locked; `loginctl unlock-session` from the session's own user unlocks it without a password; a new RemoteDesktop session after the unlock delivers Shift, `a` and Left once each; the Shell PID and the observer page instance survive two lock cycles.

Procedure:
Preflight (unlocked session, no grab holder, remote desktop service inactive); observer page focused and fullscreen (5 s settle); RemoteDesktop session and EIS; pre-lock Shift tap; `loginctl lock-session`; observe the old EIS connection, try a new CreateSession while locked, hold 20 s; `loginctl unlock-session` (manual unlock only as the fallback); fresh session: Shift, `a`, Left; disconnect; lock, hold 8 s, `loginctl unlock-session` again.

Expected:
Lock observed by GetActive and LockedHint agreeing (the ScreenSaver owner is not mapped to the login1 session on this host: not provenance-verified); the page tally while locked has no key, button, pointer or wheel event and the page keeps beating (a leak can only show if a CreateSession while locked is accepted and its pointer and Shift events are sent; otherwise the locked tally only shows that the operator kept their hands off, and keyboard silence is weak anyway because the page has no keyboard focus); logind unlocks both times; the post-unlock tally is exactly Shift, A and Left, plain; login session id, Shell PID and page instance (page-load count) unchanged. How the old EIS connection ends and what a CreateSession while locked does are observations, not criteria.

Observed:
result=PASS; blocked=None; failure=None; aborted=None; inconclusive=[]; lock1_ms=Some(761); old_eis_ended_after_ms=Some(761); locked_create_session=Some("refused at CreateSession: MUTTER_UNAVAILABLE (diag_01M3W4AMKC2H3J7A6DSZ64MW0V): org.freedesktop.DBus.Error.Failed: Session creation inhibited"); beats_during_lock=79; unlock1=Some(Unlock { method: Some("loginctl"), logind_exit: Some(0), logind_stderr: None, observed_after_ms: Some(976) }); fresh_eis_after_unlock=Some(true); lock2_ms=Some(749); unlock2=Some(Unlock { method: Some("loginctl"), logind_exit: Some(0), logind_stderr: None, observed_after_ms: Some(984) }); page_same=Some(true); violations=[]; shell Some("6842")->Some("6842")

Evidence:
- findings.json (this directory), including the page's tally at three points and the lock/EIS timings

Result:
PASS

Failure:
(none)

Root Cause:
(none)

Security Impact:
The live session is locked up to twice. The program unlocked the session through logind; on this host, per this run, a process of the session's own user can do that, so the lock screen is not a boundary against it. No password is typed or read by the program. Only Shift, `a`, Left and a net-zero 5 px pointer move (only if a session could be created while locked) are injected. PASS here does not promote FEAS-A by itself.

Recommended Action:
(none)

Follow-up:
FEAS-A decision and Architecture Review #1 are separate evidence reviews.
