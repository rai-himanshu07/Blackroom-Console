# Plan: Phase 6 Remote Input Readiness

**Created:** 2026-09-26
**Status:** draft, inactive; research only while Phase 5 is stopped
**Approved by:** not yet approved
**Task tier:** governed

## Goal

Prepare a verifiable path to Gate FEAS-D (remote input to the existing GNOME
session) without creating an EIS session or injecting input on the daily-driver
host while the Phase 5 Mutter-instability stop remains in force.

## Acceptance Criteria

- The exact GNOME 50.1 `RemoteDesktop.Session.ConnectToEIS` contract and the
  selected `reis` API/version are verified from current source or documentation;
  record unknowns instead of inferring the contract from InputCapture.
- A future Experiment 8 can prove pointer, click, scroll, keyboard, modifiers,
  and special keys reach the *selected existing* GNOME session in a terminal,
  editor, browser, and window movement, without arbitrary shell execution.
- A future authorization check rejects input unless authentication,
  authorization, current epoch, valid control lease, and `REMOTE_ACTIVE` all
  hold; fake-hostd tests must cover each missing condition and teardown.
- Cursor position/shape and absolute coordinates on the virtual-monitor
  region have a defined observation method for Experiment 28, including the
  Intel/NVIDIA paths; unobserved hardware-cursor claims remain unknown.
- No FEAS-D PASS or Phase 7 readiness claim is made from introspection or
  static tests alone.

## Non-Goals

- No source-code implementation, dependency installation, `CreateSession`,
  `ConnectToEIS`, input injection, or physical input isolation in this draft.
- No bypass of the Phase 5 stop condition, no Gate FEAS-C promotion, and no
  change to the active Phase 5 plan or its operator approval boundary.
- No browser transport, product networking, or hostd deployment (later phases).

## Evidence And Decisions

- Roadmap Phase 6 depends on Phase 4 and calls for `eis.rs`, `exp08`, and
  `exp28`; Phase 7 depends on Phases 5-6. Doc 00 §49 and Doc 10 §49 separately
  require stopping product implementation after Mutter instability. Phase 5's
  active plan records the reported compositor crash with unknown trigger and
  Gate FEAS-C unproven. The roadmap dependency is not permission to override
  the stop condition.
- Doc 10 §15 / Doc 02 §14 specify the Experiment 8 input and app matrix;
  Doc 05 §41-43 requires native EIS and the five-part input authority check.
  Assessment §7.4 identifies the `RemoteDesktop.Session.ConnectToEIS` EIS fd
  and `reis` as the candidate path, not a completed compatibility proof.
- `docs/gnome/feasibility-research.md` §5 confirms installed libei/libeis
  1.5.0 and an InputCapture-side EIS pattern. Experiment 3's saved *session*
  introspection (`docs/gnome/introspection/remotedesktop-session-exp03.xml`)
  confirms `ConnectToEIS(a{sv} options) -> h fd`; it did not call the method.
  §6 finds InputCapture barrier-triggered, not a proven keyboard grab.
- `reis` 0.7.1 docs (`https://docs.rs/reis/0.7.1/reis/ei/struct.Context.html`
  and `https://docs.rs/reis/0.7.1/reis/event/index.html`) confirm
  `ei::Context::new(UnixStream)`, `handshake_blocking`, and client-side
  seat/device wrappers. The crate itself warns its API is incomplete and
  subject to change. Options, D-Bus fd ownership, successful negotiation,
  actual capabilities, and event delivery against Mutter remain unknown.
  `https://docs.rs/reis/0.7.1/reis/ei/handshake/enum.ContextType.html`
  defines `Sender` (EI client sends input) versus `Receiver` (client receives
  captured input); Phase 6 targets the former, Phase 7 evaluates the latter.
- Decision: finish session-contract and crate research offline first. Keep
  remote input (Gate D) separate from physical input isolation (Gate E); do
  not use an InputCapture finding as proof of RemoteDesktop input or vice versa.

## Risks

- A session-scoped D-Bus FD method may differ from the manager's introspected
  API; a plausible signature is not evidence of a working EIS connection.
- Input injected into a real desktop can type commands or change active
  windows. A future live test needs operator presence, a prepared host, explicit
  target windows, bounded event delivery, and independent recovery.
- An agent that accepts input before lease/epoch/state checks would violate
  Doc 05 §43 even if Experiment 8's happy path succeeds.
- Intel/NVIDIA cursor and absolute-coordinate mapping must be measured, not
  inferred from a pointer moving on one GPU.

## Steps

- [ ] 1. Read GNOME 50.1 session-interface source and the selected `reis`
      version's documentation; write down the FD ownership, seat/device
      negotiation, event lifecycle, and teardown contract without opening a
      live session.
  - Files: `docs/gnome/feasibility-research.md`
  - Depends on: none (research-only exception to the implementation stop)
  - Verify: every contract claim cites a current source or is marked unknown.
- [ ] 2. After the Phase 5 stop is resolved and a new plan is approved, design
      the minimal `eis.rs` owner and the per-event authorization boundary.
  - Files: `crates/blackroom-gnome/src/mutter/eis.rs`
  - Depends on: step 1 and explicit safety/architecture approval
  - Verify: focused fake-hostd negative tests for all five authority terms;
    no input after revoke, teardown, or stale epoch.
- [ ] 3. On a separately prepared host with an operator and recovery path,
      implement and run Experiments 8 and 28 using bounded input events.
  - Files: `crates/blackroom-experiments/src/bin/exp08_remote_input.rs`,
    `crates/blackroom-experiments/src/bin/exp28_cursor.rs`
  - Depends on: step 2 and a new live-test authorization
  - Verify: Doc 10 §15 / Doc 02 §14 app-and-event matrix plus cursor/mapping
    observations; session identity, teardown, and no-input-when-unauthorized.
- [ ] 4. Review recorded evidence independently and determine FEAS-D PASS or
      stop-and-report; only then consider Phase 7's separate Gate E plan.
  - Files: `docs/gnome/capability-report.md`, `docs/HANDOFF.md`
  - Depends on: step 3
  - Verify: no gate promotion on source reading or unobserved assertions.

## Final Verification

- Research-only work: review current source and project-doctor diagnostics;
  no code tests or live GNOME operation required.
- If implementation is later authorized, follow `AGENTS.md`'s focused and
  broad Rust gates and the active plan's operator-evidence requirements.

## Blockers

- Phase 5 stop-and-report and unproven FEAS-C; operator absent from host.
  No Phase 6 implementation or live input experiment is authorized here.

## Execution Log

- 2026-09-26: drafted readiness criteria from the existing roadmap,
  feasibility research, and Doc 02/05/10 requirements. Read Experiment 3's
  persisted session XML and `reis` 0.7.1 documentation: signature and client
  entry points verified, but no EIS fd was requested, no handshake was run,
  and step 1's runtime/ownership questions remain open. No implementation
  or GNOME mutation.