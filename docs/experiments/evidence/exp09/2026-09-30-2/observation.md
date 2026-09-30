# Exp 9b run 1: EVIOCGRAB of the external dongle (Phase 7 step 5a)

Date: 2026-10-01 local (evidence directory is UTC-dated 2026-09-30-2). Operator present.

## Setup

- Operator confirmed a second-device SSH session (an established connection on port 22 was seen), saved work, and ran the temporary `setfacl` for `event6` and `event7` themselves.
- `gnome-remote-desktop` was masked for the run, then unmasked (back to inactive/disabled).
- External kill timer `blackroom-exp09-kill` armed for 300 s, then stopped after the run. No timer or probe process remained.
- Command: `exp09_grab_probe --operator-present --nodes 6,7 --expect-phys-prefix usb-0000:00:14.0-1.1/ --ready-timeout-secs 120`.
- Nodes: `event6` LITEON Dell Wireless Device and `event7` its Mouse node, both `usb-0000:00:14.0-1.1/`. Built-in keyboard and touchpad were not grabbed.
- Shell PID 6842 before and after.

## Observed

The probe's own rule gives PASS (`grab-findings.json`, counts only, no key codes).

| Phase | Page key events | Shift down | Pointer moves | Buttons | Wheel |
|---|---|---|---|---|---|
| A, before the grab | 0 | 0 | 3 | 3 | 2 |
| B, grabbed | 2 | 1 | 0 | 0 | 0 |
| C, after release | 8 | 1 | 41 | 0 | 0 |

- During B the probe itself read 7 physical key presses (and 327 pointer motions, see the correction below) from the grabbed nodes, yet the page saw none of them. Its only input was the injected Shift (press and release).
- The injected EIS Shift was accepted while the grab was held, so a remote tap and the physical grab coexisted.
- Release of both nodes succeeded, no release failures, and no key was still down afterwards.
- No violations, no abort, no failure.

## Correction (2026-10-01, found after run 2)

The probe did not drain the grabbed nodes' queues before grabbing, so its phase B read counts can include events the operator produced in phase A. The 7 key presses are still B-only, because phase A had no key events. The 327 pointer motions are not attributable to B. Keyboard isolation in B therefore stands; pointer isolation rests only on the page seeing 0 moves, buttons and wheel events in B.

## Not shown by this run

- Phase A had no keyboard activity (0 key events), so there is no keyboard baseline before the grab. The keyboard is shown working only after release (phase C).
- One run only. No SIGKILL release, no stalled-helper (`kill -STOP`) case, no dongle hotplug, no test of keys held across the grab start, no LED or repeat state check, and no built-in devices (5b).
- Operator confirmation that the desktop and devices were responsive afterwards was not recorded at the time of writing.
- Lease expiry is covered by unit tests only, not observed live.

## Found during the run

`pkill -x exp09_grab_probe` matches nothing because the kernel truncates process names to 15 bytes. The kill timer therefore uses `exp09_grab_prob` (verified against a stand-in process first). The probe now refuses unless the armed timer runs exactly `pkill -KILL -x exp09_grab_prob`.

## Status

FEAS-E is not claimed. This is one supervised observation of the grab mechanism on the external devices only.
