# Plan: Phase 6 Remote Input Readiness

**Created:** 2026-09-26
**Status:** in progress; isolated offline PoC exception, live input blocked
**Approved by:** user (2026-09-27, narrow Phase 6 PoC exception)
**Task tier:** governed

## Goal

Build and test the minimal Phase 6 remote-input path offline, without creating
an EIS session or injecting input on the daily-driver host. Independent later-phase
host, gateway, and browser implementation may proceed offline in parallel. Gate
FEAS-C and the Phase 5 Mutter-instability stop remain in force for product
activation; live FEAS-D proof requires a separate supervised approval.

## Acceptance Criteria

- The exact GNOME 50.1 `RemoteDesktop.Session.ConnectToEIS` contract and the
  selected `reis` API/version are verified from current source or documentation;
  record unknowns instead of inferring the contract from InputCapture.
- A future bounded Experiment 8 can prove the input modes required by the
  supported core workflow reach the *selected existing* GNOME session,
  including refusal after revoke/teardown; expand the app/event matrix only
  when a specific use case or failure requires it.
- A future authorization check rejects input unless authentication,
  authorization, current epoch, valid control lease, and `REMOTE_ACTIVE` all
  hold; fake-hostd tests must cover each missing condition and teardown.
- Cursor position/shape and absolute coordinates on the supported virtual
  monitor have a defined observation method if needed for core usability;
  unobserved GPU or hardware-cursor variants remain unknown.
- No FEAS-D PASS or Phase 7 readiness claim is made from introspection or
  static tests alone.

## Non-Goals

- No live `CreateSession`, `ConnectToEIS`, input injection, or physical input
  isolation without a separate supervised test approval.
- No Gate FEAS-C promotion, product remote-mode activation, or live Phase 7
  experiment without its own recovery and operator approval.
- This plan does not implement browser transport, product networking, or hostd
  deployment; separate offline slices may implement them without waiting for
  a live FEAS-C or FEAS-D result. Do not expose a remote-mode activation path.

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
- 2026-09-27 implementation-first amendment: that earlier narrow exception
  no longer prevents independent offline host, gateway, UI, or input-isolation
  development. This supersedes the earlier Doc 00/10 §49 stop interpretation
  for independent offline code, not for live tests or product activation. It
  gives no permission to inject input or change physical state. Track unresolved
  gates; do not present offline implementation as live feasibility evidence.

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
- The new agent-owned offline authority builds that snapshot per event from
  private state and a host verifier; its simulated grant is not a production
  hostd identity, authentication, or state-update channel.
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
- [ ] 3. Only after a separate live-test approval with an operator and recovery
      path, run one bounded Experiment 8 proof for keyboard, pointer, click,
      scroll, revoke, and teardown in the selected existing session. Add the
      Experiment 28 cursor/absolute-coordinate check only if that unknown
      blocks usability on the supported display layout.
  - Files: `crates/blackroom-experiments/src/bin/exp08_remote_input.rs`;
    `exp28_cursor.rs` only if required by the supported core path
  - Depends on: step 2 and a new live-test authorization
  - Verify: representative target-window input, session identity, teardown,
    and no-input-when-unauthorized; expand the app/hardware matrix only for a
    specific failure or additional support claim.
- [ ] 4. Review recorded evidence for the supported configuration and determine FEAS-D PASS or
      stop-and-report; only then consider Phase 7's separate Gate E plan.
  - Files: `docs/gnome/capability-report.md`, `docs/HANDOFF.md`
  - Depends on: step 3
  - Verify: no gate promotion on source reading or unobserved assertions.

## Final Verification

- Run focused synthetic tests after each offline implementation slice; use one
  workspace/dependency checkpoint for integration or release, not every slice.
- No GNOME input session or gate promotion can be validated by unit tests;
  these require separate operator evidence and independent review.

## Blockers

- Phase 5 stop-and-report and unproven FEAS-C block product activation and
  unapproved live input experiments, not independent offline work in later
  phases. A live input test still requires fresh, explicit approval.

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
- 2026-09-27 (offline device lifecycle gate): `EiConnection` now records
  device identity and current resume serial when bounded EI reads deliver
  `DeviceResumed`; it invalidates the device on `DevicePaused`, removal,
  seat removal, disconnection, and socket EOF. An authorized tap first
  drains queued lifecycle events within 10ms, then validates authority
  again immediately before writes and refuses a stale resume serial.
  The fake EIS peer sent real protocol pause/resume events after the first
  tap; a stale handle was refused and the new serial was tracked. A
  constructed `DeviceRemoved` event tests removal invalidation, and a
  closed socket leaves the sender not-ready. The focused EI suite passes.
  This does not solve the unavoidable pause-vs-send race, trusted agent
  authority binding, pointer/scroll breadth, or live Mutter behavior;
  Step 2/FEAS-D remain open and no live input is authorized.
- 2026-09-27 (offline relative pointer and scroll, Step 2 still partial):
  `EiConnection` now accepts finite relative pointer motion and scroll
  deltas only with a signed current CONTROL lease and an active resumed
  device serial advertising the respective interface. Each command uses
  start/frame/stop and invalidates the connection on flush failure. One
  synthetic EIS peer advertises keyboard, pointer and scroll, receives
  the exact motion and scroll deltas, and asserts start/event/timestamped
  frame/stop order with matching serials and sequence numbers. Fake checks
  refuse revoked leases, non-finite deltas, stale serials, paused and
  removed devices without advancing the send sequence. Focused GNOME tests/Clippy and the
  workspace test, format, Clippy, check, deny and audit gates passed. No
  live input was sent; trusted agent-sourced verification and state remain
  unimplemented. Absolute positioning, clicks, modifier/special-key
  behavior, FEAS-D and Gate C remain unproven; no live test is authorized.
- 2026-09-27 (offline click and one-modifier chord, Step 2 still partial):
  `EiConnection` now sends a signed button press/release and a bounded
  modifier-down/key-down/key-up/modifier-up chord, each with framed,
  increasing timestamps and stop-emulation. The synthetic EIS peer checks
  button capability, exact event order, frame/serial/sequence matching,
  and release; revoked, stale-device, and malformed chord requests are
  refused without advancing the sequence. Focused EIS tests and GNOME
  Clippy pass. Keycodes are synthetic; actual GNOME keymap, cursor/button
  behavior and input safety remain unverified. No remote-hostd authority
  owner exists yet, so the verifying key and current state are still
  caller-supplied snapshots; Step 2, FEAS-D and Gate C remain open.
- 2026-09-27 (offline lease-expiry hardening): a signed lease with an
  already-expired real deadline was accepted for input when the caller
  reused an older `InputAuthorization.now` snapshot. A core regression
  reproduces the bypass without sleeping and verifies that dispatch
  never calls the sink. `InputAuthorization::validate` now compares lease
  expiry against the later of the supplied time and `SystemTime::now()`,
  while the pure `ControlLease::validate` contract remains unchanged.
  Focused core regression and Clippy pass. The agent still lacks a trusted
  hostd authority/verifier source; no live EIS or input was attempted.
- 2026-09-27 (offline cross-component slice): the user-session agent now owns
  its verifier, epoch, session, state and credential; it validates grants,
  clears failed replacements, refuses revoked credentials within an epoch and
  dispatches all implemented EI event types through its authority. A fake
  `reis` peer observes one key tap through this path and no post-revoke input.
  An offline `remote-hostd` library owns a fixed synthetic signing key,
  epoch and state; `remote-gateway --offline-sim` links it and the agent into
  one loopback-only process with an in-memory input journal. The Vite/TypeScript
  local control surface starts, sends and revokes that fake session. API
  requests cannot supply authority fields. The normal agent startup and
  live GNOME paths are unchanged. The CLI/UI are **simulation only**: no real
  hostd identity, persisted epoch, trusted IPC update stream, real desktop
  capture, input injection or FEAS-C/FEAS-D/FEAS-E proof exists yet. Step 2
  remains open for real hostd trust binding and further live-independent
  hardening; activation remains disabled.
- 2026-09-27 (offline hostd-agent update socket): the core protocol now
  carries structured Grant/Revoke updates with a signed lease. Hostd writes
  length-prefixed bounded frames; the agent checks `SO_PEERCRED` before a
  deadline-limited read, validates signature/session/epoch, and closes input
  on invalid messages, EOF or timeout. The offline gateway sends updates
  through a persistent Unix socket pair and drains queued revocations before
  fake input; tests reproduced and fixed input-after-host-loss and epoch drift
  on repeated Start attempts. All peers still share one process and UID;
  no installed hostd socket ACL, durable identity/epoch, separate-service
  recovery, live agent activation, Mutter EIS send or feasibility proof exists.
- 2026-09-27 (offline persisted-host and process test): added an opt-in
  file-backed host authority scoped to a pre-opened owner-only directory fd.
  It creates a `0600` Ed25519 key and epoch, holds an exclusive lock, refuses
  partial/symlinked/corrupt state, fsyncs atomic epoch replacement before
  returning from revoke or restart, and blocks grants on epoch exhaustion.
  A synthetic child process reopened the state, advanced the epoch and sent
  revoke/grant updates over a Unix socket; the parent checked peer UID/PID,
  dispatched fake input, then refused it on EOF. The loopback browser demo
  still uses the original ephemeral in-process key and socket pair. No
  production path/ACL, deployed hostd and agent services, live GNOME input,
  or FEAS-C/D/E evidence was exercised; activation remains disabled.
- 2026-09-27 (opt-in persisted offline console): the loopback gateway now
  accepts an explicit `--state-dir` absolute path, opens it without following
  symlinks, and refuses unsafe ownership/mode or incomplete state. Its HTTP
  flow verifies persisted Start/Revoke, locked restart at a higher epoch,
  storage-failure refusal and no state writes when the port is occupied.
  The browser displays EPHEMERAL or PERSISTED; the default remains volatile.
  This is still a same-UID, one-process simulation. No service installation,
  physical state change, live input, or feasibility-gate promotion occurred.
- 2026-09-27 (separate offline executables): an explicit
  `gnome-session-agent --offline-sim-agent` binds only a private socket,
  verifies the configured public key and peer UID, accepts bounded host
  updates and reports fake control/EOF transitions without GNOME discovery.
  `remote-hostd --offline-sim-host` opens persisted state and sends signed
  Grant/Revoke updates over an agent socket inside that private directory.
  Process tests verify correct delivery, refusal of wrong UID and old epoch,
  and refusal of external socket paths. These are offline test tools, not
  installed services or the browser's in-process authority. Production
  verifier/UID provisioning, process recovery and live gates remain open.
- 2026-09-27 (separated offline console path): hostd gained bounded,
  strict Start/Revoke/Input control frames, with signature-checked agent
  acknowledgment before accepting fake input. A separate private runtime
  directory holds transient sockets. The gateway now supervises real offline
  hostd/agent child processes under an explicit `--separate` option; its
  HTTP test covers Start/input/revoke, persisted restart, and active-shutdown
  rollback (caught and fixed a lost-revoke race). The web console reports
  SEPARATE while continuing to label all content synthetic. This remains
  same-UID local simulation, not installed hostd/agent privilege separation,
  GNOME input, or FEAS-C/D/E evidence.
- 2026-09-27 (idle and scratch recovery): a real browser launch exposed that
  the offline agent exited after its 3s initial accept timeout while hostd
  waited for Start. Hostd now connects at service startup; a bounded
  beyond-3s idle test and browser Start succeed. Separate child-death tests
  refuse further input, report `LOCAL_LOCKED` even on idle status reads, and
  restart locked. Added opt-in `--separate-scratch`
  with a private temporary identity and explicit SIGTERM/SIGINT shutdown;
  graceful signal cleanup was observed. The terminal tool's hard stop left
  four owner-only scratch directories in `/tmp` from two earlier runs; they
  were not removed. No GNOME mutation or feasibility-gate promotion occurred.
- 2026-09-27 (offline child recovery): status now checks supervised hostd
  and agent processes and reports `LOCAL_LOCKED` after either dies. Only an
  explicit subsequent Start relaunches both children from persisted state,
  advances the epoch and requires a new signed grant before recording fake
  input. Failed relaunch remains locked and logs no input; real-process tests
  cover host death, agent death and missing replacement binary. No automatic
  product recovery or live input was enabled.