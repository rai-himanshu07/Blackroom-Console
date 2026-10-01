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
  (known path; `sudo loginctl unlock-session 2` over SSH is the unverified fallback).
- Locking while the physical grab is on would lock the operator out; the grab stays off in Phase 8.
- Phase 9 and 10 touch recovery of the only daily-driver display; every run needs the kill timer
  re-armed immediately before it (it expired during a 40 min approval gap on 2026-10-01).

## Steps

- [ ] 1. Phase 8 stage 1: exp11 harness (lock + EIS + capture, page tally at three points)
  - Files: `crates/blackroom-experiments/src/bin/exp11_lock_semantics.rs`, `src/eis_support.rs`,
    `crates/blackroom-gnome/src/mutter/pipewire_capture.rs`
  - Verify: `cargo test -p blackroom-experiments -p blackroom-gnome`; one supervised live run PASS
- [ ] 2. Phase 8 stage 2: exp12 same session (lock, remote, disconnect, relock)
  - Files: `crates/blackroom-experiments/src/bin/exp12_same_session.rs`
  - Verify: Shell PID, session id and observer page survive disconnect and a second lock
- [ ] 3. Phase 8 gate: FEAS-A evidence review and Architecture Review #1 (operator decision in HANDOFF)
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
  Recovery note corrected: use `sudo loginctl unlock-session 2` from SSH.
