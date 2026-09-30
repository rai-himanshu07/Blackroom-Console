# Exp 9b run 2: full session stopped at stage 1 (Phase 7 step 5)

Date: 2026-10-01 local. Operator present, SSH open, ACLs on event2..7, `gnome-remote-desktop` masked then unmasked, 720 s kill timer armed then stopped, no stray process or timer afterwards. Shell PID 6842 unchanged.

Command: `exp09_grab_probe --operator-present --full --nodes 6,7 --expect-phys-prefix usb-0000:00:14.0-1.1/ --builtin-nodes 2,3,4,5 --ready-timeout-secs 90`. All preflight checks passed.

## Result: FAIL by the rule as written, and inconclusive

| Phase | Key events | Pointer moves | Buttons | Wheel |
|---|---|---|---|---|
| A | 6 (3 down) | 8 | 0 | 1 |
| B, grabbed | 2 (the injected Shift only) | 0 | 0 | 0 |
| C0, hands off | 0 | 0 | 0 | 2 |
| C | 0 | 35 | 2 | 5 |

- The grab again hid all physical input from the page in B; the injected Shift was accepted and release was clean with no key left down.
- The only violation was 2 wheel events on the page in the 3 s hands-off window. Nothing could attribute them: the page cannot tell a touched device from ghost input, so this is not evidence of a leak.
- The gating worked as designed: no later stage (SIGKILL, stalled helper, built-in chord) started, and no built-in device was grabbed.

## Flaws found in the probe (fixed before any re-run)

- The probe did not drain the grabbed nodes' queues before the grab, so its phase B counts (3 key presses, 503 motions) may include phase A events; phase A had 3 key presses. B activity on the grabbed nodes is therefore not proven by this run.
- The hands-off check now counts events from the un-grabbed devices in the same window (the dongle and, in a full run, the built-in nodes): page events with no device events are ghost input and fail; page events with device activity are an inconclusive touch and make the run partial.
- Phase C now needs a keyboard and pointer baseline (this run had no key events in C), and phase B needs both keyboard and pointing activity.

## Status

No stage 2 to 4 evidence exists. FEAS-E is not claimed.
