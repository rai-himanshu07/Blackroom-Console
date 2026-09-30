Experiment: Experiment 8 — Remote Input (FEAS-D)
Date: 2026-09-30T16:35:47.481954059Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1, single built-in display
Objective:
Prove a signed-lease-gated EIS Sender from RemoteDesktop.Session.ConnectToEIS delivers keyboard, pointer, click and scroll to the existing session and that no input arrives after an authorization revoke or session teardown.

Hypothesis:
Mutter accepts the Sender handshake, advertises keyboard/pointer/button/scroll devices, delivers the injected events to the focused observer page, and delivers nothing after the revoke and the session stop.

Procedure:
Preflight; serve the observer page on loopback; wait for a focused fullscreen page (5 s settle); CreateSession; Start; ConnectToEIS; bind seat; F13 tap, Shift+F13 chord, pointer +40/-40, left click, scroll 15; revoked-authorization tap (must be refused); Stop; valid-authorization tap after Stop (delivery judged by the page); stale-path Stop call; Shell PID unchanged.

Expected:
Every injected stage accepted; the revoked stage refused as LeaseRevoked; observer tally: F13 2/2, ShiftLeft 1/1, one left click, net-zero pointer motion, positive scroll, no untrusted events and nothing from the revoked or post-stop attempts.

Observed:
result=PARTIAL; blocked=None; failure=None; aborted=Some("observer not focused and fullscreen before pointer_right_40"); stages=3; tally_matches=Some(false); violations=[]; shell Some("6842")->Some("6842")

Evidence:
- findings.json (this directory), including the page's own tally

Result:
PARTIAL

Failure:
(none)

Root Cause:
(none)

Security Impact:
Input injected into the live desktop; only inert keys and a net-zero pointer move were sent, gated on the observer page holding focus. PASS here does not by itself promote FEAS-D.

Recommended Action:
(none)

Follow-up:
FEAS-D decision is a separate evidence review (plan Step 4).
