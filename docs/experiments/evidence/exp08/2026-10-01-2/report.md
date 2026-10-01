Experiment: Experiment 8 — Remote Input (FEAS-D)
Date: 2026-10-01T12:48:23.846681250Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1, single built-in display
Objective:
Prove a signed-lease-gated EIS Sender from RemoteDesktop.Session.ConnectToEIS delivers keyboard, pointer, click and scroll to the existing session and that no input arrives after an authorization revoke or session teardown.

Hypothesis:
Mutter accepts the Sender handshake, advertises keyboard/pointer/button/scroll devices, delivers the injected events to the focused observer page, and delivers nothing after the revoke and the session stop.

Procedure:
Preflight; serve the observer page on loopback; wait for a focused fullscreen page (5 s settle); CreateSession; Start; ConnectToEIS; bind seat; Shift tap, Shift+Right Ctrl chord, `a` tap, Left tap, pointer +10 (start), +40, -40, -10, left click, scroll 15; revoked-authorization tap (must be refused); Stop; valid-authorization tap after Stop (delivery judged by the page); stale-path Stop call; Shell PID unchanged.

Expected:
Every injected stage accepted; the revoked stage refused as LeaseRevoked; observer tally: ShiftLeft 2/2, ControlRight 1/1 (with Shift held), KeyA 1/1, ArrowLeft 1/1, one left click, a pointer path of +40, -40, -10 steps from a captured start, positive scroll, no untrusted events and nothing from the revoked or post-stop attempts.

Observed:
result=FAIL; blocked=None; failure=None; aborted=None; stages=13; tally_matches=Some(false); violations=["observer tally mismatch: [\"1 unexpected key events\"]"]; shell Some("6842")->Some("6842"); baseline=Some(Baseline { physical_key_downs: 4, physical_pointer_moves: 168 }); daemon_grab=Some(DaemonGrab { isolated_nodes: Some(4), refused: None, status_phase_at_end: Some("isolated"), reads: Some(741), active_nodes: Some(2), pushed_releases: [], restored: true }); daemon_attribution_ok=Some(true)

Evidence:
- findings.json (this directory), including the page's own tally

Result:
FAIL

Failure:
(none)

Root Cause:
(none)

Security Impact:
Input injected into the live desktop; only Shift, Ctrl, the letter A and Left (checked against gsettings and xkb) and a net-zero pointer path were sent, gated on the observer page holding focus. PASS here does not by itself promote FEAS-D.

Recommended Action:
(none)

Follow-up:
FEAS-D decision is a separate evidence review (plan Step 4).
