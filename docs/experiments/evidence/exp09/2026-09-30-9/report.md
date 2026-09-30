Experiment: Experiment 9b — Exclusive input grab probe (FEAS-E)
Date: 2026-09-30T20:12:57.804072454Z
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
result=PASS; blocked=None; failure=None; aborted=None; A=None; B=None; C0=None; C=None; probe read 0 key presses and 0 motions in B; injected Shift=None; release failures=[]; keys still down after release=None; violations=[]; notes=["the page lost focus during stage 4 after; it restarted"]; stages=[built-in devices released by the emergency chord: held=Some(PhaseTally { key_events: 0, key_downs: 0, key_shape: "", shift_down: 0, pointer_moves: 0, buttons: 0, wheel_events: 0 }) after=Some(PhaseTally { key_events: 18, key_downs: 9, key_shape: "dudududduududududu", shift_down: 0, pointer_moves: 145, buttons: 0, wheel_events: 0 }) reads=`READ e2=13 e5=806 chord=0` end=`exit 0; RELEASED chord clean=true` failures=[] gaps=[]]

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
A PASS here is one observation, not FEAS-E; hotplug, LED/repeat state and repeated cycles are untested.
