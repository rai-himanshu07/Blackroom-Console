# Exp 9b run 6: page lost focus in phase A (Phase 7 step 5)

Date: 2026-10-01 local. Same setup and safeguards as run 5; nothing was grabbed. Preflight passed, kill timer armed with 1 s accuracy, `gnome-remote-desktop` masked then unmasked, no stray process or timer, Shell PID 6842 unchanged.

## Result: PARTIAL, aborted in phase A

The observer page lost focus or fullscreen during phase A (17 key events, 218 pointer moves, 4 button events before that). No grab had started, so nothing was at risk. The probe did not record why; the likely cause is the mouse reaching the top edge (Firefox shows its toolbar there) or a click taking focus off the page, and an earlier run lost the page in phase C the same way.

## Fixes made after this run

- In windows with no grab held (phases A and C, the hands-off window, the after-windows of stages 2 to 4) a lost page is now recoverable: the page shows how to refocus, waits up to 30 s, clears the tally and restarts that window. With a grab held it still aborts.
- The prompts no longer ask for mouse clicks and say to keep the mouse in the middle of the screen.
- The evidence records the page's focus and fullscreen history (times and flags only).

## Not run

Stages 2 to 4 (the release gate from run 5 was never reached) and Exp 8 run 3. FEAS-E is not claimed.
