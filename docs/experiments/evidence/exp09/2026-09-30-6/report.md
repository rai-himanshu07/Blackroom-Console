Experiment: Experiment 9b — Exclusive input grab probe (FEAS-E)
Date: 2026-09-30T19:55:07.087045813Z
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
result=FAIL; blocked=None; failure=None; aborted=None; A=Some(PhaseTally { key_events: 64, key_downs: 32, key_shape: "dduudduududduudduududududduududduudduudd", shift_down: 0, pointer_moves: 531, buttons: 0, wheel_events: 0 }); B=Some(PhaseTally { key_events: 2, key_downs: 1, key_shape: "du", shift_down: 1, pointer_moves: 0, buttons: 0, wheel_events: 0 }); C0=Some(PhaseTally { key_events: 0, key_downs: 0, key_shape: "", shift_down: 0, pointer_moves: 43, buttons: 0, wheel_events: 0 }); C=Some(PhaseTally { key_events: 64, key_downs: 33, key_shape: "dduduudududduudduduudduududduudduududduu", shift_down: 0, pointer_moves: 287, buttons: 3, wheel_events: 0 }); probe read 58 key presses and 1880 motions in B; injected Shift=Some("accepted"); release failures=[]; keys still down after release=Some(1); violations=["helper killed with SIGKILL: 347 physical-looking events reached the page while grabbed"]; notes=["later stages were skipped because an earlier stage did not pass", "a device was touched in the hands-off window, so ghost input was not checked"]; stages=[helper killed with SIGKILL: held=Some(PhaseTally { key_events: 347, key_downs: 347, key_shape: "rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr", shift_down: 0, pointer_moves: 0, buttons: 0, wheel_events: 0 }) after=Some(PhaseTally { key_events: 57, key_downs: 28, key_shape: "udduudduduududduduuududduudduudududduudu", shift_down: 0, pointer_moves: 518, buttons: 0, wheel_events: 0 }) reads=`READ e6=16 e7=576` end=`signal 9` failures=["347 physical-looking events reached the page while grabbed"] gaps=[]]

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
