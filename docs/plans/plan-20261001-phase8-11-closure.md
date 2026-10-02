# Plan: Close Phases 8 to 11 (lock, lifecycle, emergency, security authority)

**Created:** 2026-10-01
**Status:** in progress (Phase 8 step 1 built offline; live run awaits approval)
**Approved by:** operator ("Lets close every item till phase 11", 2026-10-01); each live step needs its own approval
**Task tier:** governed (live lock, display and input mutation, emergency path)

## Goal

Close every Phase 8 to 11 roadmap item that evidence can close, in order, and stop at each gate that
needs a decision or a live observation only the operator can give. Gates: FEAS-A (Phase 8) and
Architecture Review #1, FEAS-F and FEAS-H (Phase 9), FEAS-G, Go/No-Go and Architecture Review #2
(Phase 10), SEC-A to SEC-E and SEC-J (Phase 11, only after Go).

## Acceptance Criteria

- Each phase's roadmap Verify list is either demonstrated with current evidence or listed in its
  Gate section as an open limit with a reason; no unobserved claim is promoted.
- Every live step: operator present, fresh second-device SSH, named recovery, exact command approved
  first; one bounded observation per distinct question.
- Phase 10 Go/No-Go is recorded by the operator from the evidence, not inferred.

## Non-Goals

- No installed service, dedicated uid, PAM change or polkit file on this host without separate approval.
- No repeat, cycle or soak run beyond what a named failure or a roadmap Verify line requires.
- No claim for HDMI, hotplug, dongle or any layout other than built-in eDP-1 and event2-5.

## Evidence And Decisions

- Evidence: `docs/plans/plan-20260904-blackroom-console-master-roadmap.md` Phases 8 to 11;
  `docs/gnome/lock-semantics.md`; `docs/experiments/evidence/exp11/2026-09-28-lock-only/`; FEAS-C/D/E
  decisions of 2026-10-01.
- Decision: Phase 8 is staged. Stage 1 (exp11) attaches EIS and a monitor capture, locks once, injects
  into the lock screen and has the operator unlock with their own password; the observer page is the
  witness. No physical isolation and no virtual monitor in the first lock run (Mutter-instability risk,
  unexplained Shell SIGSEGV). Isolation-after-unlock and virtual-monitor continuity move to the Phase 9
  activation transaction, which owns lock-before-activation, and are named as limits in the FEAS-A gate.
- Decision: no password is ever typed by the program or passed through chat; remote password entry is
  a Phase 11 browser-path item.

## Risks

- Lock plus EIS plus capture is untested together; the session may need the operator's manual unlock
  (known path; `loginctl unlock-session 2` over SSH is the unverified fallback (logind authorises the session's own uid, per a source review; observed only from inside the session), `sudo` otherwise).
- Locking while the physical grab is on would lock the operator out; the grab stays off in Phase 8.
- Phase 9 and 10 touch recovery of the only daily-driver display; every run needs the kill timer
  re-armed immediately before it (it expired during a 40 min approval gap on 2026-10-01).

## Steps

- [x] 1. Phase 8 stage 1: exp11 harness (lock + EIS + capture, page tally at three points)
  - Files: `crates/blackroom-experiments/src/bin/exp11_lock_semantics.rs`, `src/eis_support.rs`,
    `crates/blackroom-gnome/src/mutter/pipewire_capture.rs`
  - Verify: `cargo test -p blackroom-experiments -p blackroom-gnome`; one supervised live run PASS
- [x] 2. Phase 8 stage 2: exp12 same session (lock, remote, disconnect, relock)
  - Files: `crates/blackroom-experiments/src/bin/exp12_same_session.rs`
  - Verify: Shell PID, session id and observer page survive disconnect and a second lock
- [x] 3. Phase 8 gate: FEAS-A evidence review and Architecture Review #1 (operator decision in HANDOFF)
  - Files: `docs/gnome/lock-semantics.md`, `docs/HANDOFF.md`, capability report
  - Verify: independent read-only review before the operator accepts
- [ ] 4. Phase 9 offline: real `GnomeBackend` assembly, agent `transaction.rs`/`teardown.rs`/`recovery.rs`,
  recovery marker persistence, suspend inhibitor, systest harness
- [ ] 5. Phase 9 live: exp13 to exp20, exp25, exp30 to exp36 (FEAS-F, FEAS-H), eDP-only owner-kill (Gate F)
- [ ] 6. Phase 10: emergency hardening and wiring, exp21 to exp24, FEAS-G, feasibility report,
  Go/No-Go, Architecture Review #2
- [ ] 7. Phase 11 (after Go): store, identity, PAM helper, polkit, CLI verbs, security docs, RT suites

## Final Verification

- Focused tests per slice; one broad `cargo test --workspace --exclude gnome-session-agent` plus
  `-p gnome-session-agent` at each phase gate; one independent review per gate.

## Blockers

- Live steps need the operator present with a second-device SSH session.
- Phase 11 certification waits for the Phase 10 Go decision.
- Soak (1 h) and 100-cycle runs, PAM live checks and installed hardened units need approvals and
  wall-clock time; they are scheduled, not skipped.

## Execution Log

- 2026-10-01: plan drafted; step 1 harness built and tested offline (exp11 refuses without
  `--operator-present`; shared EIS plumbing moved to `eis_support`, exp08 re-verified by clippy and tests).
- 2026-10-01: independent read-only review of the exp11 harness: READY after fixes. Fixed: heartbeat
  baseline at lock engagement (at least 8 beats, last beat fresh), abort before locking when pre-lock
  checks fail, device handles re-read after each pump, locked tally stored as counts only, grab-holder
  preflight, a quiet locked capture window is inconclusive not a violation, accurate on-page operator
  prompt, report text no longer claims the lock screen received the input. A malformed-tally overflow
  in the locked judge was caught by its own test and fixed (saturating sum).
- 2026-10-01 (exp11 live attempts, evidence exp11/2026-10-01, -2, -3; nothing was locked in any): attempts 1 and 2
  BLOCKED, zero page heartbeats (the page was opened after the program's 180 s and 600 s waits; start only
  when the operator is at the machine). Attempt 3 FAIL at the pre-lock gate by design: the pre-lock Shift tap
  arrived, but the capture window counted 0 frames. `--capture-probe` showed the monitor stream delivers about
  90 frames/s on a busy desktop; a static fullscreen page repaints nothing and the stream is damage-driven,
  so the criterion was unfair. Fixed (frames counted since attaching, a deliberate prompt repaint before the
  pre-lock check, locked and unlocked windows inconclusive when quiet, consumer errors recorded), commit
  `fe50936`. The pre-lock gate did its job: it stopped the run before the lock.
- 2026-10-01 (exp11 attempt 4, evidence exp11/2026-10-01-4, nothing locked): the deliberate repaint did not help: the
  consumer stayed attached with no error and received 0 frames at all while the fullscreen observer page was
  up (the same code gave about 90 frames/s on a busy windowed desktop). So a fullscreen client starves the
  monitor stream here (direct scanout is the likely cause, unverified). Capture windows are now inconclusive
  when quiet and no longer block the lock; capture continuity under a fullscreen page stays a named limit
  for the FEAS-A gate unless the locked or unlocked windows deliver frames.
- 2026-10-01 (exp11 attempt 5, evidence exp11/2026-10-01-5): FEAS-A original wording NOT MET. The lock engaged in 776 ms
  and Mutter ended the EIS connection at once; gnome-shell 50.1 inhibits remote access in the locked session mode
  (`main.js _sessionUpdated`, `sessionMode.js`), which terminates every RemoteDesktop/ScreenCast session and
  refuses new ones. Remote input cannot drive the unlock dialog through Mutter. Replacement path to test next
  (exp12): refusal of `CreateSession` while locked, unlock by logind `Unlock` from a user process (the product
  mechanism after hostd-side authentication), fresh RemoteDesktop/EIS after the unlock, same Shell and page.
  Recovery note: `loginctl unlock-session 2` from SSH (same uid), `sudo` as the fallback; neither observed over SSH.
- 2026-10-01: exp12 harness built (`exp12_same_session`) on shared `lock_support`/`eis_support`; independent read-only review
  READY after fixes. Fixed: a page-load counter replaces the first-transition check (a reload could match), the
  unlock counts as logind's only if the call exited 0 on a still-locked session, loginctl gets a null stdin and a
  10 s timeout, focus is re-checked before each post-unlock tap, an uncertain lock triggers a best-effort unlock,
  the second cycle is skipped after a manual first unlock, the selected session id is compared at the end, the
  report text no longer claims pointer/wheel witnesses unless a locked CreateSession is accepted.
- 2026-10-01 (exp12 live, evidence exp12/2026-10-01, PASS, harness `b336f01`): CreateSession while locked refused (`Session creation
  inhibited`); `loginctl unlock-session` from a same-user process unlocked the session in under a second, twice;
  a fresh RemoteDesktop/EIS session after the unlock delivered Shift, `a`, Left once each; session id,
  Shell PID and the observer page instance survived two lock cycles. FEAS-A original wording is not met by
  design; the replacement path is observed on the built-in layout. Step 3 (gate and Architecture Review #1)
  is next, with an independent review before the operator decides.
- 2026-10-01 (Phase 8 gate): independent review of the evidence (FEAS-A original not met by design; replacement
  design PASS-WITH-LIMITS; recommended MODIFY) and eleven wording corrections applied. The operator chose MODIFY:
  FEAS-A recorded PASS-WITH-LIMITS under the replacement design, Architecture Review #1 = MODIFY. Conflict C27 filed in
  the assessment register and `docs/plans/amendment-20261001-lock-inhibits-remote-access.md` written (option A:
  no remote unlock by default; option B needs a separate decision). Phase 9 offline work may start.
- 2026-10-01 (Phase 9, shortest path to a real live test): the open Gate F observation is the eDP-only owner-kill.
  exp06's `--auto-kill-after-isolate` was hard-wired to HDMI-1; it now accepts a sole eDP-1 or HDMI-1 output and an
  optional `--grab-socket` takes the real physical-input grab (remote-emergencyd, 60 s lease) before the self-SIGKILL, so the
  killed owner holds the display configuration and the daemon connection at once. Offline: tests, clippy green.
  Full real-backend wiring of the agent transaction is NOT built; this live run is an experiment-level composition
  of existing modules, named as such in the Phase 9 gate.
- 2026-10-01 (Phase 9 live, Gate F eDP-only owner-kill with the real input grab, evidence exp06/2026-10-01-2, harness
  `fcb7411`): the probe isolated eDP-1 and the daemon grabbed event2-5, then the probe SIGKILLed itself. Mutter removed the
  virtual monitor and restored eDP-1 logically within a second, the daemon had released the grab by the first check
  (15 s, before the 60 s lease), the 45 s watchdog restored the panel (`exp07_restore` PASS, power-save 0), Shell PID
  unchanged, no crash line; the operator reported no desktop content, picture back within about a minute, input
  and desktop fine. One run, same uid, exp06 as the owner (not the product agent), no remote input/capture/lock.
  Temporary ACLs on event2-5 were still present at the cleanup check (operator action pending).
- 2026-10-01 (Phase 9, integrated core-path live test built, not yet run): `exp06_isolate_outputs --pause-after-isolate
  --watchdog-seconds 120 --integrated-probe --grab-socket <sock>` composes the proven pieces in the amended order:
  RemoteDesktop/EIS and the observer page first, virtual monitor with a capture consumer, isolate eDP-1, real input
  grab, remote Shift/a/Left judged by the page at the end of a 25 s hold, grab release, session stop, orderly restore,
  ScreenCast stop, lock and unlock through logind; one `pass` flag in `integrated.json`. An independent read-only
  review found no lock-out path and nine defects, fixed (eDP-only guard, watchdog 120, no hold without a grab, end-of-hold
  judging, capture join after the restore, no teardown lock after an early grab end, original watchdog disarmed, pass flag).
  Helpers moved into `eis_support`/`lock_support`. The run is `docs/ops/live-integrated-run.sh` (`--check` is read-only).
  This is an experiment-level composition; the production agent is still on the fake backend.
- 2026-10-01 23:02 (Phase 9 integrated live run, evidence exp06/2026-10-01-3): **GNOME Shell crashed (SIGSEGV) during the restore
  `ApplyMonitorsConfig`, the session was lost** (details and stack summary in the observation). Stop condition (Doc 00
  section 49): all live display/input/session experiments are stopped; `docs/ops/live-integrated-run.sh` is guarded
  (`BLACKROOM_STOP_LIFTED=1`). FEAS-F/H are not met for the integrated composition.
- 2026-10-02 00:10 (offline + throwaway headless Shell, evidence exp06/2026-10-01-3 "Headless reproduction"): **root cause
  reproduced 5/5, high confidence**: the ScreenCast virtual-stream `monitors-changed-internal` handler dereferences a NULL
  view when the stream is enabled (consumer streaming) and the applied config omits the virtual monitor; the probe's
  physical-only restore did exactly that while its capture thread was still streaming. Same libmutter frame offsets as
  the real crash. Fix (operator chose design A): restore with the virtual monitor kept as an extra logical monitor
  (scale 1.0 only, else the probe refuses), then Stop (`restore_physical_outputs_keeping_virtual`, exp06 integrated path
  and its `RestoreGuard`); verified safe 8/8 on the headless Shell. One independent review done (FIX_FIRST, fixes applied:
  guard fallback after Stop, scale rule, wording). Open before any live retest: operator approval for one real-session
  run (the stop stays in force); upstream report.
- 2026-10-02 (exp07 hardening, offline, one independent review FIX_FIRST, fixes applied): `exp07_restore
  --keep-live-virtual` keeps every live `Meta-N` virtual monitor as an extra logical monitor right of the restored ones,
  falls back to the plain restore when a restored monitor is not scale 1.0 or rotated, never keeps them on its single
  retry (recovery first), and its topology check ignores virtual monitors; `exp06 --integrated-probe` passes the flag to
  both watchdogs only (earlier flows behave the same; evidence gains one field) and the integrated run script's recovery
  text includes it. Unit-tested only: exp07's apply path needs the real login1 session identity and was not run headless.
  A manual `exp07_restore` without the flag with a live owner and a streaming consumer is still unsafe. Side effect to
  expect: windows may stay on the kept virtual monitor until the session is stopped.
- 2026-10-02 12:50 (operator-approved real-session integrated run, evidence exp06/2026-10-02): **no Shell crash**; the
  restore with the kept virtual monitor passed with the consumer streaming (1369 frames in the hold), original topology
  and hash restored, lock 674 ms and logind unlock fine, daemon/timers/ACLs cleaned up. Probe NOT PASS: the observer page
  lost focus when the panel was isolated and never regained it, so no input was injected; remote input under isolation and
  physical-input blocking during the integrated hold (nobody touched the machine) remain unproven (FEAS-F/H integrated part).
  Harness defects: pre-isolation F11 left in the end-of-hold tally; `devices_seen_before_isolation` 0. One observation only.
- 2026-10-02 13:16 (second operator-approved real-session integrated run, evidence exp06/2026-10-02-2; commit `508e198`
  added a gated focus click and judged the tally only after a reset): **PASS by the probe rule**. Remote Shift/A/Left were
  accepted under isolation and the page saw exactly them (no pointer, button or wheel event), the daemon isolated 4 nodes,
  held and released the grab, capture streamed 1408 frames through isolation and restore, no Shell crash (same PID), original
  topology and hash restored, lock 687 ms and logind unlock fine, cleanup complete. The page regained focus by itself after
  1 s, so the focus click was not exercised (the first run, with other windows open, never regained focus; cause unverified).
  Still unproven: physical-input blocking during the integrated hold (nobody touched the machine; the dongle is not grabbed),
  the focus-click path, other topologies, repeats and soak. Phase 9 real-backend wiring, lifecycle experiments and Phase
  10-11 remain open.
- 2026-10-02 13:25 (third integrated run, evidence exp06/2026-10-02-3, `--physical-check`): **inconclusive, nothing wrong**.
  The operator could not see the page prompts because the physical panel is black while isolated (my design flaw: the
  prompt lives on the page, which is on the virtual monitor), so nobody typed and the daemon read 0 events (< 20). The
  rest matched run 2: 3 remote taps accepted and seen exactly, no other page event, 4 nodes grabbed and released, 2130
  frames in the 35 s hold, no Shell crash, topology and hash restored, lock 680 ms and logind unlock. The page never lost
  focus in this run (runs 1-3: lost for good, lost 1 s, never lost). Fix: audible cues (one chime to start, two bells to
  stop) plus a countdown in the script text; the page prompt stays as a secondary cue.
- 2026-10-02 13:36 (fourth integrated run with audible cues, evidence exp06/2026-10-02-4, commit `ea9fbbb`): **PASS with the
  physical-input check**. The operator typed and swiped after the chime: the daemon read 538 events by mid-hold and 1390
  by the end from the four grabbed nodes, while the page saw exactly the three injected keys and no other key, pointer,
  button or wheel event; grab held and released, 2114 frames captured, no Shell crash, topology and hash restored, lock
  694 ms and logind unlock fine. The gated focus click fired once (page lost focus at isolation, refocused after the
  click). Limits: one passing run, built-in nodes only (the dongle is not grabbed), no held key at grab start, no chord or
  hotplug case, no repeats or soak. Remaining Phase 9: real `GnomeBackend`/agent transaction wiring, lifecycle
  experiments, cycles; Phase 10 emergency, FEAS-G, Go/No-Go and Review #2; Phase 11 after Go.
