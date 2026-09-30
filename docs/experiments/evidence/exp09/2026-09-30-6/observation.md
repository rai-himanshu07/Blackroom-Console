# Exp 9b run 5: stage 2 found the stuck-key hazard (Phase 7 step 5)

Date: 2026-10-01 local. Same setup and safeguards as run 4 (kill timer armed with 1 s accuracy, all preflight checks passed, `gnome-remote-desktop` masked then unmasked, no stray process, timer or temp link afterwards, Shell PID 6842 unchanged).

## Result: FAIL by rule, with a real finding

Stage 1 (in-process grab of the dongle) held: phase B on the page was exactly the injected Shift (`du`, 2 events) while the probe read 58 key presses and 1880 motions from the grabbed nodes; phase A and C had keyboard and pointer activity; release was clean. One key was still physically down at release (`keys_still_down_after_release` 1), which is not a leak. In the 3 s hands-off window the operator moved the mouse (72 device events, 43 page moves), so ghost input could not be judged (partial, not a failure).

Stage 2 (separate helper process holds the grab, then SIGKILL) failed: the page saw **347 key-down events, every one an auto-repeat (`r`), and no pointer or other input** while the helper held the grab and read only 16 real key presses and 576 motions. After the SIGKILL the page saw 57 key events and 518 pointer moves (985 device events), so release on SIGKILL worked again.

## Finding: a key held when the grab starts auto-repeats into the session

A key that is physically down (and already seen by the session as down) when the grab starts never delivers its release to the session, so the browser's own repeat timer keeps firing the key for as long as the grab lasts (about 35 per second here). Nothing new was typed into the page; the session was fed a stuck key. For a real remote session this is a real hazard (for example a held Enter), and a mouse button held at grab start would likewise stay pressed.

Requirements this adds to the emergency helper design (not yet implemented):
1. Before grabbing, wait until no key or button is down on the nodes to be grabbed (bounded), or refuse.
2. After grabbing, inject a release for every key or button that was down at any point between the last check and the grab (through the authorized input path), since a press and release can straddle the grab.

## Fixes made to the probe after this run

- Every grab now waits until all keys and buttons are up and stay up for 0.5 s: the in-process phase (with a "LIFT ALL fingers" prompt on the page), and each helper (parent prompt plus a check inside the helper, which refuses with `KEYS-HELD` after 20 s).
- Leak messages include the key shape, and a shape of only `r` is labelled as the stuck-key case.

## Not run

Stages 3 and 4 (frozen helper with a precise timer, built-in devices with the emergency chord) and Exp 8 run 3. FEAS-E is not claimed.
