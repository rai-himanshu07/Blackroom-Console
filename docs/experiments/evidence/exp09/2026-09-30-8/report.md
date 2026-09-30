Experiment: Experiment 9b — Exclusive input grab probe (FEAS-E)
Date: 2026-09-30T20:04:20.011051594Z
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
result=FAIL; blocked=None; failure=None; aborted=None; A=Some(PhaseTally { key_events: 32, key_downs: 16, key_shape: "dduudduududduudududduudduudduudu", shift_down: 0, pointer_moves: 931, buttons: 0, wheel_events: 0 }); B=Some(PhaseTally { key_events: 2, key_downs: 1, key_shape: "du", shift_down: 1, pointer_moves: 0, buttons: 0, wheel_events: 0 }); C0=Some(PhaseTally { key_events: 0, key_downs: 0, key_shape: "", shift_down: 0, pointer_moves: 0, buttons: 0, wheel_events: 0 }); C=Some(PhaseTally { key_events: 84, key_downs: 42, key_shape: "dduduudududduudduduudduudduuddududuuddud", shift_down: 0, pointer_moves: 1086, buttons: 0, wheel_events: 0 }); probe read 38 key presses and 2255 motions in B; injected Shift=Some("accepted"); release failures=[]; keys still down after release=Some(0); violations=["built-in devices released by the emergency chord: the grab was not released cleanly by the emergency chord (no release within 40 s; killed by the probe)"]; notes=[]; stages=[helper killed with SIGKILL: held=Some(PhaseTally { key_events: 0, key_downs: 0, key_shape: "", shift_down: 0, pointer_moves: 0, buttons: 0, wheel_events: 0 }) after=Some(PhaseTally { key_events: 74, key_downs: 36, key_shape: "uudduduududduduudduudduudduduudduduududu", shift_down: 0, pointer_moves: 308, buttons: 0, wheel_events: 0 }) reads=`READ e6=33 e7=881` end=`signal 9` failures=[] gaps=[]] [stalled helper killed by its own timer: held=Some(PhaseTally { key_events: 0, key_downs: 0, key_shape: "", shift_down: 0, pointer_moves: 0, buttons: 0, wheel_events: 0 }) after=Some(PhaseTally { key_events: 52, key_downs: 26, key_shape: "dudduudduudduudududduudududduduududududd", shift_down: 0, pointer_moves: 556, buttons: 0, wheel_events: 0 }) reads=`` end=`signal 9` failures=[] gaps=[]] [built-in devices released by the emergency chord: held=Some(PhaseTally { key_events: 0, key_downs: 0, key_shape: "", shift_down: 0, pointer_moves: 0, buttons: 0, wheel_events: 0 }) after=Some(PhaseTally { key_events: 26, key_downs: 13, key_shape: "dududududududududududududu", shift_down: 0, pointer_moves: 176, buttons: 0, wheel_events: 0 }) reads=`READ e5=101 e7=79` end=`no release within 40 s; killed by the probe` failures=["the grab was not released cleanly by the emergency chord (no release within 40 s; killed by the probe)"] gaps=[]]

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
