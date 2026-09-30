# Exp 9b run 4: stages 1 and 2 passed, stage 3 inconclusive (Phase 7 step 5)

Date: 2026-10-01 local. Same setup and safeguards as run 2 (preflight passed, 720 s kill timer armed and stopped, `gnome-remote-desktop` masked then unmasked, no stray process, timer or temp link afterwards, Shell PID 6842 unchanged). Instructions were shown on the observer page this time and the operator followed them.

## Stage 1: in-process grab of the dongle (external keyboard and mouse)

| Phase | Key events (shape) | Pointer moves | Buttons | Wheel |
|---|---|---|---|---|
| A, before | 8 (`dudududu`) | 96 | 3 | 0 |
| B, grabbed | 2 (`du`, exactly the injected Shift) | 0 | 0 | 0 |
| C0, hands off | 0 | 0 | 0 | 0 |
| C, after | 56 | 692 | 0 | 0 |

- The probe read 32 key presses and 926 motions from the grabbed nodes in B (counters drained and zeroed before the grab) while the page saw only the injected tap.
- Hands-off window: no page events and no device events (`c0_device_events` 0), so no ghost input.
- Release clean, no key left down. This is the first run with a keyboard and pointer baseline, B activity on both kinds of device, and a sound ghost-input check.

## Stage 2: separate helper process killed with SIGKILL

While the helper held the grab it read 13 key presses and 1216 motions; the page saw 0 events. The helper ended with `signal 9`. In the next 10 s the page saw 33 key events and 840 pointer moves. So the kernel released the grab when the holder was SIGKILLed and input returned, observed on the dongle.

## Stage 3: frozen helper killed by its own timer, inconclusive

- While the helper was frozen (SIGSTOP, confirmed stopped) the page saw 0 events and the parent saw a key held down for 35 samples (1.75 s), so the grab persisted through a frozen holder; a lease thread cannot help in that case.
- The helper died with `signal 9`, but 47 s after the stop instead of the intended 24 s. The transient timer used systemd's default 1 min accuracy (journal: timer started 01:18:40, service ran 01:19:28). The kill worked, late.
- In the 10 s after the kill the page saw no events, and the probe could not tell whether the operator used the dongle, so it failed the stage by the rule as written. Whether input returned after this kill is not established.

## Finding: systemd timers default to 1 min accuracy

`systemd-run --on-active=N` fires anywhere from N to N+60 s unless `--timer-property=AccuracySec=1s` is given. This applies to every kill and watchdog timer used so far, including the display restore watchdog in exp06 (45 to 120 s). All are now armed with 1 s accuracy, and the probe refuses a main kill timer whose accuracy is coarser than 5 s.

## Fixes made after this run

- Timers: 1 s accuracy for the stall timer and exp06's watchdog; the probe checks the main timer's accuracy.
- The after-window of stages 2 and 3 now counts events from the un-grabbed devices: page silence with device activity fails (input did not return), page silence without device activity is a gap (operator idle).
- Stage 3's waiting prompt tells the operator to keep holding until the text changes.

## Not run

Stage 4 (built-in devices released by the emergency chord) and Exp 8 run 3. FEAS-E is not claimed.
