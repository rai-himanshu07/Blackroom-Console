Experiment: Experiment 9b — Exclusive input grab probe (FEAS-E)
Date: 2026-09-30T19:03:44.527495134Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1; named USB nodes only unless --include-builtin
Objective:
Observe that an exclusive EVIOCGRAB on physical nodes stops the operator's physical input from reaching the session while a remote EIS tap still arrives, and that release restores it.

Hypothesis:
Phase A the page sees physical input; phase B it sees none but the injected Shift while the grabbed nodes still produce events; phase C it sees physical input again.

Procedure:
Preflight; observer page focused; open the nodes; open an EIS keyboard; phase A; isolate (grab all or none); phase B with one injected Shift tap; release; phase C. External kill timer armed.

Expected:
A and C show physical activity, B shows only the injected Shift, probe read events in B, release clean.

Observed:
result=PASS; blocked=None; failure=None; aborted=None; A=Some(PhaseTally { key_events: 0, shift_down: 0, pointer_moves: 3, buttons: 3, wheel_events: 2 }); B=Some(PhaseTally { key_events: 2, shift_down: 1, pointer_moves: 0, buttons: 0, wheel_events: 0 }); C=Some(PhaseTally { key_events: 8, shift_down: 1, pointer_moves: 41, buttons: 0, wheel_events: 0 }); probe read 7 key presses and 327 motions in B; injected Shift=Some("accepted"); release failures=[]; keys still down after release=Some(0); violations=[]

Evidence:
- findings.json (this directory)

Result:
PASS

Failure:
(none)

Root Cause:
(none)

Security Impact:
Holds an exclusive grab on the named input nodes for one bounded window; counts events only, never key codes.

Recommended Action:
(none)

Follow-up:
A PASS here is one observation, not FEAS-E; release on SIGKILL is tested separately.
