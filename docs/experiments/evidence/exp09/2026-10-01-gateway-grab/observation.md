# Exp 9: gateway -> hostd -> remote-emergencyd physical grab, supervised run (2026-10-01)

Operator present, fresh tablet SSH (192.168.1.51), runbook `docs/ops/live-grab-runbook.md`
(single-window `bk case ...`). Binaries from commit `860d41c` plus the uncommitted guided script.
Grabbed nodes: event2 AT keyboard, event3 PS/2 mouse, event4 DELL0A71 Mouse, event5 Touchpad
(ACL by the operator, removed afterwards; no dongle). Real: the grab. Simulated: session, agent,
lock, desktop. gnome-shell PID 6842 unchanged throughout. Times are local (IST); audit lines are
static codes and epochs only, no key data.

| Case | What the audit and operator showed | Verdict |
|---|---|---|
| Start with no heartbeat (14:29:20, 14:31:03, accidental, twice) | grab engaged, `grant_revoked isolation_lost` 24.65 s after each Start | heartbeat-loss release works |
| 1 Revoke (14:47:39) | operator: pointer frozen; grant revoked 14:47:51 (`revoked`); pointer back (Enter 5.4 s later, reaction time) | PASS |
| 2 Heartbeat stops (14:51:08) | last renew 14:51:19; `isolation_lost` logged 14:51:43 (+24 s); Enter 14:51:46 (+26.8 s incl. reaction), expected +23..+27 | PASS |
| 3 hostd SIGSTOP (14:55:43) | frozen 14:55:48; pointer returned by itself before Enter at +18.5 s (reaction time included; exact moment unmeasured); after SIGCONT hostd logged `isolation_lost` 14:56:08, so the daemon had already let go. First run aborted with Ctrl-C: SIGCONT plus revoke worked | PASS (timing coarse) |
| 4 Chord (15:40:07) | operator held Left Ctrl+Shift+Alt+Esc: pointer returned; `emergency-stop` marker written 15:40 (mode 0600, doctor OK), epoch bumped to 15, gateway FAILED_SAFE, no `stop marker failed` in the daemon log; hostd exited leaving `recovery-pending` (expected); no stuck key reported | PASS |

Other observations:
- The 25 min kill timer (armed 14:42:53) fired at 15:07:53 while the operator was away and killed
  the idle daemon as designed; the next Start failed closed with `INPUT_ISOLATION_FAILED` (nothing
  grabbed). A refused Start closes hostd's control stream, so the gateway restarted hostd on each
  retry (epoch +1).
- The chord path exits hostd through `emergency_required` and writes no audit line for the chord
  or for the grant that was active (the audit log ends at `grant_issued`).
- The first guided attempt was too complex (two windows, Termux); the script now runs one case per
  command and measures the operator's answer, which includes 1 to 5 s of reaction time.

Not shown: keyboard-only observation in cases 1 to 3 (the pointer was the indicator; the chord
shows the keyboard node was held); chord latch against a second isolate (unit-tested only); lease
lapse while the page heartbeat runs; hotplug; SysRq and power button under a grab; repeated
cycles. FEAS-E and FEAS-G stay unpromoted; step 6 independent review is still open.
