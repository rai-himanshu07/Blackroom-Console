# Experiment 9a observation (2026-09-30): read-only input inventory

Generated `report.md` and `inventory.json` are unchanged. Nothing was grabbed,
no `/dev/input` node was opened, no input was sent (procfs, sysfs and the udev
database only; unique ids are not recorded). This is not a FEAS-E result.

## Result
23 event nodes; the Phase 7 classification (capability bits, `remote-input-helper`)
would grab 6 and skip 17, with **0 differences** from libinput's udev tags.

Would grab: `event2` AT Translated Set 2 keyboard (built-in), `event3` PS/2 Generic
Mouse, `event4` DELL0A71 Mouse and `event5` DELL0A71 Touchpad (the built-in touchpad
exposes two nodes), `event6` LITEON Dell Wireless Device (keyboard) and `event7` its
Mouse node (an external wireless keyboard and mouse receiver).

Would skip: lid switch, power button, Intel HID events and 5-button array, Dell WMI
hotkeys, Dell Privacy Driver, both Video Bus nodes, the wireless device's Consumer
Control node, and eight audio jack/HDMI switch nodes.

## What it settles
- The built-in Fn-row hotkeys are on separate nodes (Dell WMI hotkeys, Intel HID),
  so they would keep working while the typing keyboard is grabbed.
- The laptop has an external wireless keyboard/mouse dongle that also has to be
  grabbed; unplugging it or plugging in another device is the hotplug case.
- The built-in keyboard node carries the kernel `sysrq` handler. The input core
  hands events only to the grabbing handle, so SysRq and the kernel VT key
  handling are expected to stop while that node is grabbed. Expected, not observed.

## Still open (plan step 5, live)
Release on fd close and on SIGKILL; queued-event leakage; stuck-modifier, LED and
repeat state after release; SysRq and power-button behavior; a stalled helper;
hotplug latency; that the observer page sees no physical input while remote input
still works; the privilege path (user is not in group `input`).
