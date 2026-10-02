# Integrated probe with the physical-input check, 2026-10-02 13:35-13:36 (fourth run): PASS

Operator-run `docs/ops/live-integrated-run.sh` with `BLACKROOM_PHYSICAL_CHECK=1` (commit `ea9fbbb`; `--physical-check`, 35 s
hold, audible cues because the panel is black while isolated). Same flow as the earlier runs: remote session, virtual
monitor with a streaming consumer, isolate eDP-1, grab on event2-5, three remote taps judged by the observer page, a
typing and swiping window for the operator, restore with the kept virtual monitor, capture stop, ScreenCast Stop, lock,
logind unlock. The operator closed other windows, pressed F11 and typed letters and swiped the built-in touchpad after the
chime until the bells (the cue sounds were played by the harness; which cues the operator heard was not recorded, the read
counts below show the typing happened).

Observed (`integrated.json`, `findings.json`, journal):
- Physical input: the daemon read 538 events from the grabbed nodes by mid-hold and 1390 by the end, while the page saw
  exactly the three injected keys (Shift, A, Left; six events), no other key, no pointer move, button or wheel event, no
  untrusted event, no judge notes. The grab held for the whole hold and was handed back (`grab_restored`, phase `idle`,
  no early release). `pass` true, including the new rule (reads >= 20).
- Focus: the page lost focus at 7.8 s (page clock, the isolation) and the gated focus click was sent after the 3 s wait
  (`focus_click` accepted); focus returned at 11.5 s and the taps followed. This is the first live use of the click path;
  it worked once, so it is observed, not proven reliable. (Runs 1-3: never regained, regained by itself, never lost.)
- Capture: 2114 frames in the hold (about 60 fps) with the consumer streaming through isolation and restore.
- Display: 13:35:37 `Added virtual monitor Meta-0`, 13:36:13 EIS socket closed, 13:36:14 `Removed virtual monitor Meta-0`.
  No SIGSEGV and no new apport report; Shell PID 3828099 and session 182 unchanged. Final state: eDP-1 only,
  `topology_matches_original` and `configuration_hash_matches` true, PowerSaveMode 0, no post-Stop repair.
- Lock teardown: lock after 694 ms, `loginctl unlock-session` exit 0, unlocked after 954 ms, same Shell.
- Cleanup: watchdog and kill timers stopped at 13:36:18, daemon and live directory gone, gnome-remote-desktop
  disabled/inactive, ACLs removed by the operator.

Limits: one passing run of this check (an earlier attempt, `../2026-10-02-3/`, was inconclusive because the operator could
not see the page prompts; reads 0). Only the four grabbed built-in nodes were exercised: the LITEON dongle devices are not
grabbed and were not used, no key was held across the grab start, no emergency-chord or hotplug case, no repeat cycles
or soak, one topology (eDP-only, scale 1.0). Events typed before the tally reset would not show on the page, but the
operator typed only after the chime, several seconds after isolation.
