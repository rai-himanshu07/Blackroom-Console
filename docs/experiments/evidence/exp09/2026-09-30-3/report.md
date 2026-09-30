Experiment: Experiment 9b — Exclusive input grab probe (FEAS-E)
Date: 2026-09-30T19:36:43.640277788Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1; named USB nodes only unless --include-builtin
Objective:
Observe that an exclusive EVIOCGRAB on physical nodes stops the operator's physical input from reaching the session while a remote EIS tap still arrives, and that release restores it.

Hypothesis:
Phase A the page sees physical input; phase B it sees none but the injected Shift while the grabbed nodes still produce events; phase C it sees physical input again.

Procedure:
Preflight; observer page focused; open the nodes; open an EIS keyboard; phase A; isolate (grab all or none); phase B with one injected Shift tap; release; phase C. External kill timer armed. --full then runs: a separate helper process holds the grab and is SIGKILLed; a SIGSTOPped helper is killed by its own timer; with --builtin-nodes the built-in devices are grabbed too and released by the emergency chord.

Expected:
A and C show physical activity, B shows only the injected Shift, probe read events in B, release clean.

Observed:
result=FAIL; blocked=None; failure=None; aborted=None; A=Some(PhaseTally { key_events: 6, key_downs: 3, shift_down: 1, pointer_moves: 8, buttons: 0, wheel_events: 1 }); B=Some(PhaseTally { key_events: 2, key_downs: 1, shift_down: 1, pointer_moves: 0, buttons: 0, wheel_events: 0 }); C0=Some(PhaseTally { key_events: 0, key_downs: 0, shift_down: 0, pointer_moves: 0, buttons: 0, wheel_events: 2 }); C=Some(PhaseTally { key_events: 0, key_downs: 0, shift_down: 0, pointer_moves: 35, buttons: 2, wheel_events: 5 }); probe read 3 key presses and 503 motions in B; injected Shift=Some("accepted"); release failures=[]; keys still down after release=Some(0); violations=["2 events reached the page in the hands-off window after release"]; notes=["later stages were skipped because an earlier stage did not pass"]; stages=

Evidence:
- findings.json (this directory)

Result:
FAIL

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
