# Exp 9b run 3: full session stopped at stage 1, instructions were not visible (Phase 7 step 5)

Date: 2026-10-01 local. Same setup, safeguards and command as run 2 (all preflight checks passed, 720 s kill timer armed and stopped, `gnome-remote-desktop` masked then unmasked, no stray process or timer, Shell PID 6842 unchanged).

## Result: PARTIAL, no stage 2 to 4 evidence

| Phase | Key events (downs) | Pointer moves | Buttons | Wheel |
|---|---|---|---|---|
| A | 0 | 119 | 23 | 1 |
| B, grabbed | 2 (2 downs, Shift down 1) | 0 | 0 | 0 |
| C0, hands off | 0 | 0 | 0 | 0 |
| C (aborted: page lost focus or fullscreen) | 15 (7) | 0 | 0 | 0 |

- Phase A had no typing, so there is no keyboard baseline; the operator only used the mouse.
- Phase C was aborted because the page lost focus or fullscreen; it had keys and no pointer movement.
- Phase B saw two key-down events and no key-up. A normal injected Shift tap is one down and one up (runs 1 and 2). The extra down is most likely a physical key pressed in the gap between the page's tally reset and the grab taking effect (the reset happened before the grab), but the probe kept only counts, so this is unproven. The old rule (`events - 2 == 0`) did not flag it.
- Hands-off window clean (no page events, so ghost input was not seen).

## Root cause of the confusing run

The stage and phase instructions were printed only in the terminal, which is hidden behind the fullscreen observer page, so the operator could not read them. This also explains the missing typing in A and the focus loss in C.

## Fixes made before any further run

- Every phase and stage instruction is now shown large on the observer page itself (a prompt field in the heartbeat reply, rendered as text).
- Phase B: the tally reset now happens after the grab, and B must contain exactly the injected tap (2 events, 1 down, 1 Shift down) or the run fails. The tally also records the order of key down, repeat and up letters (no key codes) for diagnosis.

## Status

FEAS-E is not claimed. Stages 2 to 4 have not run.
