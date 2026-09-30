# Exp 9b run 7: stages 1 to 3 passed, built-in chord stage did not release (Phase 7 step 5)

Date: 2026-10-01 local. Full session with the release gate, focus recovery and on-page prompts. Preflight passed, kill timer armed with 1 s accuracy, `gnome-remote-desktop` masked then unmasked, no stray process, timer or temp link afterwards, Shell PID 6842 unchanged.

## Stage 1: in-process grab of the dongle: passed

| Phase | Key events | Pointer moves |
|---|---|---|
| A, before | 32 (16 downs) | 931 |
| B, grabbed | 2 (`du`, exactly the injected Shift) | 0 |
| C0, hands off | 0 (and 0 device events) | 0 |
| C, after | 84 (42 downs) | 1086 |

The probe read 38 key presses and 2255 motions from the grabbed nodes in B (counters drained and zeroed right before the grab) while the page saw only the injected tap. Release clean, no key left down, no ghost input.

## Stage 2: holder killed with SIGKILL: passed

The separate helper held the grab (read 33 key presses and 881 motions on the dongle) and the page saw 0 events. After SIGKILL (`signal 9`) the page saw 74 key events and 308 pointer moves (522 device events). The kernel releases the grab when the holding process is killed.

## Stage 3: frozen holder killed by its own timer: passed

While the helper was frozen (SIGSTOP) the page saw 0 events and a key was held down for 77 samples, so a frozen holder keeps the grab and the dongle stays dead. The helper's own timer killed it 24 s after the stop (1 s accuracy now works). Afterwards the page saw 52 key events and 556 pointer moves.

## Stage 4: built-in devices plus dongle, released by the emergency chord: failed

- With the built-in keyboard and touchpad and the dongle all grabbed, the page saw 0 events. The helper read 101 touchpad events (event5) and 79 dongle mouse events (event7) in that window, so built-in pointing isolation held. No key press was read on any keyboard in that window, so built-in keyboard isolation was not exercised.
- The chord (Left Ctrl, Right Ctrl, Left Shift, Right Shift held 2 s on the built-in keyboard) did not release the grab within 40 s, and the probe killed the helper. The probe did not record how many chord keys the helper saw, so the cause is unknown: the operator may not have completed the chord, this laptop may not have a usable Right Ctrl, or chord detection may not see the built-in keyboard events.
- After the helper was killed, the built-in keyboard and touchpad worked again (26 key events and 176 pointer moves on the page), so release by killing the holder also works for the built-in devices.

## Changes after this run

- The experiment chord now uses keys every laptop has: Left Ctrl, Left Shift, Left Alt and Esc.
- The helper reports how many chord keys it sees down at once (a count only), and the evidence keeps the maximum, so a failed chord can be diagnosed.
- A `--chord-only` mode repeats just the built-in stage.

## Not run

Exp 8 run 3. FEAS-E is not claimed; built-in keyboard isolation and the emergency chord are not shown.
