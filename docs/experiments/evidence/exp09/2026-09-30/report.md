Experiment: Experiment 9a — Input device inventory (read-only)
Date: 2026-09-30T17:55:34.400276669Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1
Objective:
Apply the Phase 7 grab classification to this host's real input nodes without opening any device.

Hypothesis:
Capability-bit classification selects the built-in keyboard and touchpad nodes and skips power, lid and hotkey nodes, agreeing with libinput's udev tags.

Procedure:
Parse /proc/bus/input/devices, read /sys/class/input/event*/dev and /run/udev/data/c13:*, classify each node, compare with udev ID_INPUT_* tags. No /dev/input access, no grab, no input.

Expected:
Keyboard and pointing roles present, zero mismatches with udev.

Observed:
23 event nodes, 6 would be grabbed, 0 differ from udev tags; keyboard=true, pointing=true; a grabbed node carries the kernel `sysrq` handler: true

Evidence:
- inventory.json (this directory)

Result:
PASS

Failure:
(none)

Root Cause:
(none)

Security Impact:
None: read-only procfs, sysfs and udev-database reads; unique ids are not recorded.

Recommended Action:
(none)

Follow-up:
Phase 7 step 5 live grab needs its own approval; this does not prove FEAS-E.
