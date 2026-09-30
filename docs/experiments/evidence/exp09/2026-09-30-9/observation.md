# Exp 9b run 8: built-in devices released by the emergency chord: passed (Phase 7 step 5b)

Date: 2026-10-01 local. `--chord-only` mode, nodes 6,7 (dongle) plus 2,3,4,5 (built-in keyboard, PS/2 mouse, touchpad mouse and touchpad). Preflight passed, kill timer armed with 1 s accuracy, SSH open, `gnome-remote-desktop` masked then unmasked, no stray process or timer afterwards, Shell PID 6842 unchanged.

## Result: PASS

- A separate helper process grabbed all six nodes after every key and button had been up for 0.5 s.
- While grabbed, the operator used the built-in keyboard and touchpad: the helper read 13 key presses on the built-in keyboard (event2) and 806 touchpad events (event5), and the page saw 0 events. So built-in keyboard and touchpad isolation held.
- The experiment chord (Left Ctrl + Left Shift + Left Alt + Esc, held 2 s, detected inside the helper from the built-in nodes only) released the grab: the helper reported `RELEASED chord clean=true` and exited 0, 9 s after the chord prompt. It saw all four chord keys down at once (`chord=4`).
- After the release the built-in keyboard and touchpad worked: 18 key events and 145 pointer moves on the page. A focus loss on the page in that window was recovered and the window restarted.

## Notes

- The earlier chord (Left Ctrl + Right Ctrl + Left Shift + Right Shift) did not release in run 7 although the operator reports pressing all four keys; the probe then kept no chord diagnostics, so the cause is unknown. The new chord uses keys every laptop has.
- The dongle was not used in the held window of this run (its reads appear only after the chord prompt); dongle isolation was observed in runs 4 and 7.
- This chord is an experiment choice, not a product decision.
