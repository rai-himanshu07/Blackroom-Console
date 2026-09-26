# Plan: Phase 6 Remote Input Readiness

**Created:** 2026-09-26
**Status:** in progress; isolated offline PoC exception, live input blocked
**Approved by:** user (2026-09-27, narrow Phase 6 PoC exception)
**Task tier:** governed

## Goal

Build and test the minimal Phase 6 remote-input path offline, without creating
an EIS session or injecting input on the daily-driver host. Gate FEAS-C and
the Phase 5 Mutter-instability stop remain in force for product activation;
live FEAS-D proof requires a separate supervised approval.

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

- No live `CreateSession`, `ConnectToEIS`, input injection, or physical input
  isolation without a separate supervised test approval.
- No Gate FEAS-C promotion, product remote-mode activation, Phase 7 work, or
  relaxation of the Phase 5 stop for live display experiments.
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
- Mutter 50.1 source (`src/backends/meta-remote-desktop-session.c`) confirms
  lazy `MetaEis` creation, optional `device-types: u32` (default keyboard,
  pointer, touch), Unix-FD-list return, viewport initialization when started,
  and EIS teardown when the session closes. Real negotiation and event delivery
  remain unknown; the offline PoC must not infer host compatibility.
- Decision: finish session-contract and crate research offline first. Keep
  remote input (Gate D) separate from physical input isolation (Gate E); do
  not use an InputCapture finding as proof of RemoteDesktop input or vice versa.
- 2026-09-27 exception: the user explicitly authorized an isolated Phase 6
  PoC to avoid blocking all offline work on Gate C. Only research, minimal
  implementation and fake/synthetic tests are authorized; `reis` may be added
  once its exact API and license are reviewed. Do not wire an input listener
  into agent startup or enable remote mode. This does not amend Doc 00 §49's
  live safety stop or certify any feasibility gate.

## Risks

- A session-scoped D-Bus FD method may differ from the manager's introspected
  API; a plausible signature is not evidence of a working EIS connection.
- Input injected into a real desktop can type commands or change active
  windows. A future live test needs operator presence, a prepared host, explicit
  target windows, bounded event delivery, and independent recovery.
- An agent that accepts input before lease/epoch/state checks would violate
  Doc 05 §43 even if Experiment 8's happy path succeeds.
- The offline gate accepts a verifying key and authentication/authorization
  snapshot from its caller. Until the agent binds those to trusted hostd
  state, synthetic tests cannot establish the real trust boundary.
- Intel/NVIDIA cursor and absolute-coordinate mapping must be measured, not
  inferred from a pointer moving on one GPU.
- `reis::event::EiConvertEventIterator::next` blocks while waiting for
  seat/device events. The offline EI owner now uses a deadline-driven poll,
  public converter and bounded handshake/event read instead; live seat/device
  readiness and teardown still need independent verification on Mutter.

## Steps

- [x] 1. Read GNOME 50.1 session-interface source and the selected `reis`
      version's documentation; write down the FD ownership, seat/device
      negotiation, event lifecycle, and teardown contract without opening a
      live session.
  - Files: `docs/gnome/feasibility-research.md`
  - Depends on: none (research-only exception to the implementation stop)
  - Verify: every contract claim cites a current source or is marked unknown.
- [ ] 2. Under the approved offline exception, implement the minimal `eis.rs`
      owner and an explicit per-event authorization boundary behind fake
      transports, without connecting to Mutter or agent startup.
  - Files: `crates/blackroom-core/src/lease.rs`,
    `crates/blackroom-gnome/src/mutter/{eis,remote_desktop}.rs`,
    `crates/blackroom-gnome/Cargo.toml`
  - Depends on: step 1 and the 2026-09-27 offline-only exception
  - Verify: focused fake-hostd negative tests for all five authority terms;
    no input after revoke, teardown, or stale epoch.
- [ ] 3. Only after a separate live-test approval on a prepared host with an
  operator and recovery path,
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

- Run focused synthetic tests after each offline implementation slice, then
  `AGENTS.md`'s Rust workspace gates and dependency checks if `reis` is added.
- No GNOME input session or gate promotion can be validated by unit tests;
  these require separate operator evidence and independent review.

## Blockers

- Phase 5 stop-and-report and unproven FEAS-C block product activation and
  live input experiments. The exception authorizes offline Phase 6 PoC code
  only; its live test still requires fresh, explicit approval.

## Execution Log

- 2026-09-26: drafted readiness criteria from the existing roadmap,
  feasibility research, and Doc 02/05/10 requirements. Read Experiment 3's
  persisted session XML and `reis` 0.7.1 documentation: signature and client
  entry points verified, but no EIS fd was requested, no handshake was run,
  and step 1's runtime/ownership questions remain open. No implementation
  or GNOME mutation.
- 2026-09-27: user approved a narrow offline Phase 6 implementation
  exception while leaving Gate C and live test authorization unchanged.
- 2026-09-27: read Mutter 50.1 `handle_connect_to_eis` and `reis` 0.7.1
  `ei::Context`/Sender docs; filed confirmed fd/options/lifecycle facts in
  feasibility research and kept negotiation/event delivery unknown. Core
  `ControlLease::validate` now rejects VIEW-only remote input; focused
  regression and formatting checks pass. No GNOME session was created.
- 2026-09-27 (Step 2 partial, offline only): added `reis` 0.7.1 to
  `blackroom-gnome` (cargo check/deny green). `EiConnection` owns the
  D-Bus-returned Unix FD; a synthetic socket-pair test confirms peer EOF
  after drop. `RemoteDesktopSession::connect_to_eis` requires a started
  session and `InputAuthorization::validate` before making any D-Bus call.
  The core gate checks authentication, authorization, the signed lease,
  current session/epoch/state/revocation/expiry and CONTROL capability;
  a fake-sink test rejects events after revoke or leaving REMOTE_ACTIVE.
  No code calls ConnectToEIS, starts a new session, handshakes with Mutter,
  or sends an input event. Sender device negotiation and an actual
  auth-checked EI event path remain to implement before Step 2 is complete.
- 2026-09-27 (offline Sender handshake and fake event gate): `EiConnection`
  now negotiates `ContextType::Sender` over a synthetic Unix socket pair
  using `reis`'s server-side handshaker; the peer asserts Sender and four
  consecutive focused runs passed. Negotiated connection/event objects
  remain private. `InputAuthorization::dispatch` validates immediately
  before calling a fake event sink; revocation or leaving REMOTE_ACTIVE
  leaves the sink unchanged. Mutter 50.1 source confirms FD options/cleanup;
  `reis` examples show keyboard events need seat binding and a resumed
  device with a serial before framing. `ei_text` requires libei >=1.6,
  while this host has 1.5; use keyboard/pointer capability paths instead.
  No real handshake or input event was sent. Step 2 stays open pending
  fake-tested seat/device negotiation and keyboard/pointer dispatch.
- 2026-09-27 (bounded next slice): reviewed `reis` 0.7.1's `SeatAdded`,
  `DeviceAdded`, `DeviceResumed` and `EisRequestConverter` APIs. Sender
  keyboard delivery requires advertised/bound keyboard capability, a
  resumed device serial, start/frame/stop emulation and flush. The current
  `EiConnection` deliberately exposes no event-send operation; its private
  event iterator can block indefinitely and must gain a bounded wait before
  live use. This is an open Step 2 requirement, not a FEAS-D result.
- 2026-09-27 (bounded offline EI readiness): the prior high-level iterator
  calls an unbounded internal poll, so the GNOME wrapper now drives public
  `EiHandshaker`/`EiEventConverter` with `rustix` poll deadlines instead.
  Silent synthetic peers time out during the Sender handshake and after
  handshake; a fake EIS peer advertises a keyboard-capable seat and the
  bounded client receives `SeatAdded`. The fake seat test passed four
  consecutive runs. No seat was bound, no device was resumed, and no key or
  pointer event was sent. Step 2 remains open for bounded device readiness
  and authorized event dispatch; no live EIS permission or FEAS-D claim.
- 2026-09-27 (fake keyboard device and one framed key request): the
  synthetic EIS peer now observes a keyboard-capability seat bind,
  advertises a virtual keyboard device and resumes it. The bounded EI
  reader receives `DeviceAdded` and `DeviceResumed`; a test-only Sender
  starts emulating with the resumed serial, sends keycode 30 in a frame
  timestamped with CLOCK_MONOTONIC, and the fake server observes its
  pressed-key request. The focused test, formatting and GNOME Clippy pass.
  This is not yet a production EI input method: a safe press/release and
  stop-emulation sequence must invoke `InputAuthorization::dispatch` at
  each event boundary and be fake-tested under revocation/state changes.
  No real Mutter handshake or live key was sent; Step 2 remains open.
- 2026-09-27 (authorized synthetic key tap, Step 2 still partial): the
  EI owner now accepts an `InputAuthorization` for each key-tap command
  and validates the signed CONTROL lease before any EI write. It sends
  press and release in separate CLOCK_MONOTONIC frames, followed by
  stop-emulation; release is required cleanup even if authority changes
  after the command begins. A fake EIS peer observes keycode 30 pressed,
  then released with a later timestamp and stop-emulation. Revocation or
  leaving `REMOTE_ACTIVE` before a new command prevents another send;
  a failed socket flush marks the EI sender not-ready. Core tests cover
  invalid authentication, authorization, signature, capability, expiry,
  epoch, session and state. This **does not** establish live typing:
  the trusted agent has not supplied/bound the verifier and state snapshot,
  device pause/removal is not tracked, pointer/scroll remain unimplemented,
  and no Mutter EIS connection or input event has been opened. Gate FEAS-D
  and Step 2 remain open; no live test is authorized.