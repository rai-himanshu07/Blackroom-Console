# Plan: Blackroom Console — Phase-wise Implementation Roadmap (Phases 0–20)

**Created:** 2026-09-04
**Status:** approved
**Approved by:** user (Himanshu), 2026-09-05 — licence GPL-3.0 (SPDX `GPL-3.0-or-later`); AMD hardware `UNKNOWN` for v1
**Task tier:** governed
**Companion documents:** [assessment-20260904-detailed-project-plan.md](assessment-20260904-detailed-project-plan.md)
(decisions, conflict resolutions, risk register); per-phase execution plans are created
with `/plan-task` when a phase starts — the first one is
[plan-20260904-phase0-1-discovery-and-environment.md](plan-20260904-phase0-1-discovery-and-environment.md).

**2026-09-27 execution amendment:** Build independent host, gateway, and browser
pieces offline before closing every live feasibility gate. The phase sequence
below describes product dependencies and the original comprehensive experiment
scope, not a requirement to run all experiments or finish each phase before
starting the next offline slice. Choose a bounded live test only for a specific
unknown that blocks the supported core workflow; defer optional hardware
variants, large matrices, repeat counts, and soak until a failure or support
claim justifies them. Keep unproven features disabled and gaps explicit. Do not
activate remote mode or call the product verified until its privacy, input,
lock, rollback, and emergency claims have evidence on the supported setup.
This user-approved execution amendment supersedes the earlier phase-order and
stop-on-instability rules for **independent offline code only**; it does not
override the live experiment recovery procedure or any product gate.

## Goal

Deliver the product defined by Document 00: a Wayland-native remote console for
Ubuntu 26.04 / GNOME 50+ that lets an authenticated browser use the existing GNOME session
while the physical display is disabled and physical input is blocked, and that returns the
workstation to `LOCAL_LOCKED` (or a defined `FAILED_SAFE`) on disconnect, failure, or
emergency — implemented incrementally offline, with real feasibility evidence before
enabling or claiming each dependent live capability.

## Acceptance Criteria

- Independent networking and browser implementation may start with synthetic/local
  backends. Every feasibility claim needed for the supported live product must have
  target-host evidence before enabling that capability or declaring release readiness.
- Every product invariant INV-001…INV-014 has at least one automated test
  (`INV-SEC-*`/`INV-PERF-*` assertions) and, where physical, a recorded manual
  verification.
- The Document 00 §73 final integration scenario and §74 security review complete with no
  `CRITICAL` finding, no unaccepted `HIGH`, and no `UNVERIFIED` P0 invariant.
- Each phase ends with the Document 00 §68 report and the Document 00 §69 status
  classification; nothing is reported `VERIFIED` without evidence in `docs/experiments/`
  or the test suite.

## Non-Goals

- KDE, wlroots, X11, other distributions, multi-user, headless second sessions, clipboard,
  file transfer, audio, printing, camera, mobile-native clients, passkeys/WebAuthn as a
  replacement for the agreed model, enterprise identity (Doc 00 §72).
- Shipping a "best-effort" isolation, lock, or emergency fallback. A disabled internal
  implementation for offline development is not a supported product capability.
- Rewriting the generated workflow scaffolding beyond the command-table alignment already
  approved.

## Evidence And Decisions

- Evidence: Document 00 (master prompt) — phases §47, gates §48, stop conditions §49,
  DoD §62–67, reporting §68–69. Document 10 — Experiments 0–38, Gates A–H, result format
  §47, mock-first §55. Document 11 — Phases 0–30 and Architecture Review points §47.
  Document 07 §9/§10/§5.8 — canonical activation, rollback and teardown transactions.
  Digests: [docs/plans/assessment/digests/](assessment/digests/).
- Evidence: host inventory (assessment §3) — target platform present; hybrid NVIDIA+Intel
  GPU; Mutter `InputCapture` available; dev headers missing; no Git repo.
- Decision: Document 00 phase numbers are the spine; Document 11 phases are sub-phases
  (`11.Pn` below); Document 10 experiments (`Exp n`) are the executable content of Phases
  1 and 3–10 (assessment C1, C2).
- Decision: Rust for all host components and tests, TypeScript for the browser, no Python;
  names `blackroom-console` / `blackroom` / spec component names (assessment §6.1–6.2).
- Decision: canonical vocabularies, lease schema, epoch triggers, error catalogue, IPC and
  crypto choices as in assessment §5–§6; these are treated as spec amendments and are
  recorded in `docs/security/architecture.md` and `docs/protocol/` during Phases 2 and 12.
- Decision: five stages with a formal Go/No-Go after Stage II; Architecture Reviews
  (Doc 11 §47) after Phases 8, 10, 13, 15, 16, 19.

## Risks

- FEAS-E physical input isolation may be unachievable without a global grab → project
  stop (assessment §7.1). Mitigation: research `InputCapture` first; Phase 7 is time-boxed
  and cannot be bypassed.
- FEAS-A lock semantics may terminate remote sessions → architecture change or stop
  (assessment §7.2). Mitigation: Phase 8 precedes any lifecycle/emergency build-out.
- Mutter may refuse a zero-physical-monitor configuration or restore inconsistently on
  hybrid GPU → FEAS-C at risk. Mitigation: Exp 5/6 on this host first; hybrid row in every
  matrix.
- Experiments run on the only available machine → operator lock-out. Mitigation:
  assessment §8 safety plan is a hard prerequisite of Phases 5, 7, 10.
- Private Mutter APIs (`RecordVirtual`) change across 50.x point releases → all Mutter use
  isolated behind `GnomeBackend`, versioned capability detection (Doc 05 §159, Doc 20).
- Spec silence on numbers/algorithms invites inconsistent ad-hoc choices → single
  `blackroom_core::limits` and `docs/security/architecture.md` own them (assessment §6.4–6.5).
- Single developer + agent, large surface → prioritize a working offline core and keep
  gaps visible in `docs/HANDOFF.md`; a failed live gate blocks dependent activation,
  not independent offline work in a later phase.

## Steps

Numbering: `Phase N` = Document 00 §47 phase. Each step lists sub-phases (`11.Pn` =
Document 11 phase), controlling documents, deliverables/files, dependencies, verification,
and the gate that closes it. "Report" always means the Document 00 §68 phase report
appended to the phase plan's Execution Log and summarised in `docs/HANDOFF.md`.

### Stage I — Foundations

- [ ] **Phase 0 — Repository discovery and bootstrap** (`11.P0`; Docs 00 §47, 01 §53, 11 §5)
  - Files: `.gitignore`, `Cargo.toml` (workspace), `rust-toolchain.toml`, `deny.toml`,
    `crates/*/Cargo.toml` skeletons, `web/` placeholder, `docs/gnome/`, `docs/security/`,
    `docs/experiments/`, `docs/protocol/`, `docs/ops/` (empty README stubs), `AGENTS.md`
    (already aligned), `docs/HANDOFF.md`.
  - Depends on: user approval of this roadmap.
  - Verify: `git log` shows the initial commit; `cargo check --workspace` passes on the
    empty workspace; `cargo fmt --check`, `cargo clippy` clean; project doctor
    (`.github/skills/project-doctor/scripts/doctor.py`) passes; `docs/HANDOFF.md`
    updated.
  - Gate: none (bookkeeping). Report.

- [ ] **Phase 1 — Environment and research** (`11.P1`; Docs 10 Exp 0–2, 02 §7, 05 §165 steps 1–2, 06 §160, 03 §124, 20 §59)
  - Files: `crates/blackroom-experiments/src/bin/exp00_environment.rs`,
    `exp01_session_discovery.rs`, `exp02_mutter_inventory.rs` (read-only D-Bus
    introspection via `zbus`); `docs/gnome/api-inventory.md` (every Mutter/Shell/logind/
    portal interface, method, signal and property actually present on GNOME 50.1, with
    introspection XML captured); `docs/gnome/feasibility-research.md` (questions from
    Doc 00 §50 answered from source/docs: `RecordVirtual` lifecycle, `ApplyMonitorsConfig`
    constraints, `InputCapture` semantics, `ConnectToEIS`, ScreenShield behaviour with
    RemoteDesktop sessions, logind `Lock`, systemd user-session targets on Ubuntu 26.04,
    `pam_unix`/`unix_chkpwd` privilege needs, PipeWire/GStreamer encoder availability on
    NVIDIA+Intel); `docs/gnome/capability-report.md` (first `blackroom compatibility`-shaped
    report, classification per Doc 20 §8 constants); `docs/ops/experiment-safety.md`
    (assessment §8 procedure); `docs/security/architecture.md` (decisions from assessment
    §6, crate inventory with versions and licence check).
  - Depends on: Phase 0; user installs missing `-dev` packages and `openssh-server`.
  - Verify: the three experiment binaries run without changing system state and their
    output is committed under `docs/experiments/evidence/exp00-02/`; `cargo deny check`
    passes for the selected crate set; SSH from the second device to the host succeeds;
    every research question carries a `CONFIRMED/LIKELY/UNVERIFIED/UNSUPPORTED` label.
  - Gate: feasibility blockers list (Doc 00 §47 Phase 1 deliverable) is empty or explicitly
    accepted. Report.

- [ ] **Phase 2 — State machine core** (`11.P2`, `11.P13`, `11.P14` logic-only; Docs 07 §4–§30, §59 steps 1–7; 16 §32–§38, §51; 17 §3–§5, §11; 13 §6–§7)
  - Files: `crates/blackroom-core/src/{state.rs, event.rs, transition.rs, lock.rs,
    lease.rs, epoch.rs, limits.rs, error.rs, protocol/*.rs, events/*.rs}` — 11 canonical
    states, typed events (one per transaction step outcome), the Doc 07 §8 transition
    table, priority resolver `EMERGENCY > SAFETY > LEASE_EXPIRY > DISCONNECT > NORMAL >
    RECONNECT`, `StateMachineLock`, idempotent operation wrappers, transition IDs
    (`tr_<ULID>`), timeouts from `limits.rs`, `ControlLease` (assessment C5) with
    Ed25519 signing, monotonic persisted epoch, `err001` error catalogue, structured event
    schema (Doc 13 §6–7); `crates/blackroom-gnome/src/backend.rs` (`GnomeBackend` trait,
    Doc 05 §8 operations) + `fake.rs` (in-memory fake with fault injection: fail, timeout,
    partial, duplicate, concurrent); `crates/blackroom-core/tests/` (every legal/illegal
    transition, rollback, idempotency, concurrency priority, startup reconciliation,
    Doc 07 §56 A–J as unit tests, `INV-SEC-001/002/003/009` assertions); `docs/protocol/
    state-machine.md` (canonical transaction lists from assessment C10).
  - Depends on: Phase 1 (research confirms `GnomeBackend` operation set).
  - Verify: `cargo test -p blackroom-core -p blackroom-gnome` green; property test over
    random event sequences (Doc 12 §54) never reaches `REMOTE_ACTIVE` without the full
    guard set and always reaches `LOCAL_LOCKED`/`FAILED_SAFE` after any failure event;
    `cargo clippy -D warnings`; codebase-memory index of `Blackroom_Console` created.
  - Gate: Doc 11 §37 state-transition DoD for all 25 transitions. Report.

### Stage II — GNOME feasibility PoC (all on this host; Doc 10 order; safety plan mandatory)

- [ ] **Phase 3 — GNOME session discovery** (`11.P3`; Docs 05 §12–§15, 10 Exp 1–2, 06 §28–§32)
  - Files: `crates/blackroom-gnome/src/mutter/{session.rs, capability.rs}` (logind session
    selection by UID+seat+Type=wayland+Class=user — never "first session"; Wayland check;
    Mutter/Shell version and interface capability detection → Doc 20 §8 constants);
    `crates/gnome-session-agent/` skeleton with systemd user unit `systemd/user/
    gnome-session-agent.service`, agent-side state (Doc 05 §20), `agent.sock` server with
    peer-credential checks; `docs/gnome/session-discovery.md`.
  - Depends on: Phase 2.
  - Verify: agent starts with the graphical session via `systemctl --user`, reports
    `SESSION_READY`, refuses to run on a non-Wayland/non-GNOME session (unit test with fake
    logind data + live check); two-session host case selects the correct session.
  - Gate: capability report `SUPPORTED` for OS/GNOME/Wayland/systemd/session. Report.

- [ ] **Phase 4 — Virtual display PoC** (`11.P4`; Docs 10 Exp 3–5, 02 §8–§11, 05 §25–§27, 19 §16–§17)
  - Files: `crates/blackroom-gnome/src/mutter/{remote_desktop.rs, screencast.rs,
    virtual_monitor.rs, pipewire_capture.rs}`; experiments `exp03_capture.rs`,
    `exp04_virtual_monitor.rs`, `exp05_virtual_active.rs`; evidence under
    `docs/experiments/evidence/exp03-05/`.
  - Depends on: Phase 3; `gnome-remote-desktop` masked during runs.
  - Verify: RemoteDesktop+ScreenCast session create/start/stop with verified cleanup;
    `RecordVirtual` at 1280×720, 1920×1080, 2560×1440 (60 Hz) produces PipeWire frames;
    desktop/apps/workspaces usable on the virtual monitor; 50 create/destroy cycles with
    no leaked monitors/PipeWire nodes (`pw-dump` diff) and no Shell crash (journal).
  - Gate: **FEAS-B** PASS. Report.

- [x] **Phase 5 — Physical display isolation** (`11.P5`; Docs 10 Exp 6–7, 37, 26–27, 29; 02 §12–§13, §37–§38; 05 §28–§40, §60–§62; 20 §20–§24) — **hard gate**
  - Supported-layout decision (2026-09-28): one physical display is a
    first-class configuration, not a fallback requiring an optional HDMI
    monitor. The matrix below records the original comprehensive targets;
    connected-HDMI and hotplug claims are deferred until separately proven.
    Every connected output in a declared layout must be isolated and restored;
    an unexpected/unsupported output blocks activation or triggers safe
    teardown. Single-display recovery needs verified out-of-band SSH and a
    watchdog/emergency path, not a second monitor. 2026-10-01: FEAS-C PASS for
    the declared single built-in eDP layout only (`docs/gnome/display-isolation.md`);
    connected HDMI, hotplug, modes, 50 cycles and abnormal termination (Gate F)
    are not claimed, and the connected-HDMI failure and a single-monitor
    desktop GPU stay open.
  - Files: `crates/blackroom-gnome/src/mutter/display_config.rs` (`GetCurrentState`
    snapshot → `DisplayBackup`, `ApplyMonitorsConfig` temporary, verify, restore, hotplug
    signal handling); experiments `exp06_isolate_outputs.rs` (with armed restore
    watchdog), `exp07_restore.rs`, `exp26_hotplug.rs`, `exp27_modes.rs`,
    `exp37_privacy_check.rs`; evidence incl. photos of the physical panels.
  - Depends on: Phase 4; assessment §8 safety plan in place (SSH verified, watchdog).
  - Verify: with only the virtual monitor enabled, every physical output reports disabled in
    `GetCurrentState` **and** the panels show no desktop content (photo evidence; note
    standby/no-signal behaviour per output); exact topology restored (connector, mode,
    scale, transform, position, primary) and compared by hash; hotplug during isolation
    keeps the new output isolated or triggers safe teardown; matrix rows: internal panel
    (Intel), HDMI 4K (NVIDIA), both; 50 isolate/restore cycles clean.
  - Gate: **FEAS-C** PASS; stop if Mutter refuses a zero-physical configuration or restore
    is unreliable (Doc 00 §49). Report.

- [x] **Phase 6 — Remote input** (`11.P6`; Docs 10 Exp 8, 28; 05 §41–§43; 02 §14, §29)
  - Closed 2026-10-01: FEAS-D PASS with documented limits (Exp 8 run 4; input-only session on the
    built-in display; virtual-monitor routing, live hostd authority, owner-loss and Exp 28 deferred).
  - Files: `crates/blackroom-gnome/src/mutter/eis.rs` (`ConnectToEIS`, `reis` devices,
    keyboard/pointer/scroll, absolute mapping to virtual-monitor region, cursor state);
    experiments `exp08_remote_input.rs`, `exp28_cursor.rs`.
  - Depends on: Phase 4.
  - Verify: synthetic events reach terminal/editor/browser/window-move/modifiers on the
    virtual monitor; cursor position/shape consistent (hardware and software cursor
    paths, NVIDIA and Intel); input starts/stops exactly with `EnableRemoteInput/
    DisableRemoteInput`; no input accepted when the agent holds no valid signed lease
    (fake-hostd test).
  - Gate: **FEAS-D** PASS. Report.

- [x] **Phase 7 — Physical input isolation** (`11.P7`; Docs 10 Exp 9–10, 38; 02 §15–§16, §40; 05 §44–§47; 06 §41–§45, §88; 20 §25–§27) — **hard gate**
  - Files: research note `docs/gnome/input-isolation-research.md` evaluating, in order,
    (1) Mutter `InputCapture`, (2) RemoteDesktop/InputMapping options, (3) minimal
    `EVIOCGRAB` helper, each with evidence and a privilege/hotplug/emergency analysis;
    implementation of the chosen mechanism in `crates/blackroom-gnome/src/mutter/
    input_isolation.rs` or `crates/remote-input-helper/` (only if (3) is chosen, with
    `ISOLATE_INPUT`/`RESTORE_INPUT` only, allow-listed devices, udev hotplug); experiments
    `exp09_isolate_input.rs` (armed watchdog, ≤ 45 s windows), `exp10_restore_input.rs`,
    `exp38_physical_verification.rs`; `docs/security/input-isolation-decision.md`.
  - Depends on: Phases 5–6; safety plan.
  - Status 2026-10-01: **FEAS-E PASS-WITH-LIMITS for the built-in keyboard, PS/2 mouse and touchpad
    (event2-5) only**, after runs A and B and a second independent review (the first review had
    stayed UNPROVEN); `PHYSICAL_INPUT_ISOLATION_CAPABLE` is `SUPPORTED_WITH_LIMITATIONS`. Limits: same-uid
    privilege accepted by the operator, nothing installed, dongle/USB keyboards/hotplug uncovered, no
    key-held Start test, no cycles, the page observer and the gateway chain never one run (`docs/plans/
    plan-20260930-phase7-physical-input-isolation.md`, `docs/security/input-isolation-decision.md`).
    Activation stays disabled.
  - Verify: while isolated, physical keyboard and mouse (USB combo + internal
    keyboard/touchpad) produce no effect in the session while remote input works;
    hotplugged keyboard is covered within one second; emergency chord still observable by
    the emergency observer path; restoration verified for every device; 50 cycles.
  - Gate: **FEAS-E** PASS with acceptable privilege; otherwise **STOP and report**
    (Doc 00 §49, Doc 11 §12). Report.

- [ ] **Phase 8 — Lock and same-session semantics** (`11.P8`; Docs 10 Exp 11–12; 02 §17–§19; 05 §5, §56–§58; 20 §29–§30; 12 §16–§17)
  - Files: `crates/blackroom-gnome/src/mutter/lock.rs` (`ScreenShield.Lock`,
    `GetActive`/`ActiveChanged`, logind `LockedHint`; never trust `lock()` return alone);
    experiments `exp11_lock_semantics.rs`, `exp12_same_session.rs`;
    `docs/gnome/lock-semantics.md`.
  - Depends on: Phases 4–7.
  - Status 2026-10-01: exp11 live (`docs/experiments/evidence/exp11/2026-10-01-5/`): the lock engaged in 776 ms
    and Mutter ended the EIS connection at once; gnome-shell 50.1 inhibits remote access in the locked
    session mode, so the two Verify items below that need EIS or capture to stay attached through the lock, and
    remote input driving the unlock dialog, are **not achievable through Mutter RemoteDesktop**. exp12 (PASS)
    observed the replacement path: new sessions are refused while locked, `loginctl unlock-session` from a same-user
    process unlocks in under a second, fresh RemoteDesktop/EIS works right after, and the login session, Shell
    and page survive two lock cycles. FEAS-A decision and Architecture Review #1 (MODIFY expected) are the
    operator's; see `docs/gnome/lock-semantics.md`.
  - Verify: session locked before remote activation stays attached (virtual monitor,
    capture, EIS) through lock; remote input can drive the unlock dialog; after unlock the
    physical outputs remain disabled and physical input remains isolated; identifiable
    session state (open windows, workspaces) survives lock → remote → disconnect → lock;
    lock on teardown verified via `GetActive`.
  - Gate: **FEAS-A** PASS; **Architecture Review #1** (Doc 11 §47): PROCEED / MODIFY /
    STOP recorded in `docs/HANDOFF.md`. Report.

- [ ] **Phase 9 — Complete local lifecycle** (`11.P9`, `11.P10`, `11.P11`; Docs 10 Exp 13–20, 25, 30–36; 07 §9–§11, §18–§30; 05 §63–§72, §84–§99; 19 §30–§31 dev subset)
  - Files: `crates/gnome-session-agent/src/{transaction.rs, teardown.rs, recovery.rs}`
    wiring the real `GnomeBackend` into the Doc 07 §9 22-step activation, §10 rollback and
    §5.8 teardown; `crates/remote-hostd/` PoC controller (local only: no network, mocked
    auth adapter, real lease/epoch); `RecoveryMarker` persistence; suspend inhibitor;
    experiments `exp13_teardown.rs`, `exp14_abrupt_disconnect.rs`, `exp15_hostd_crash.rs`,
    `exp16_agent_crash.rs`, `exp17_pipewire_failure.rs`, `exp18_mutter_failure.rs`,
    `exp19_display_restore_failure.rs`, `exp20_input_restore_failure.rs`,
    `exp25_reconnect.rs`, `exp30_suspend.rs`, `exp31_logout.rs`, `exp32_power_loss.rs`,
    `exp33_fault_injection.rs`, `exp34_races.rs`, `exp35_cycles.rs`, `exp36_soak.rs`;
    `crates/blackroom-systest/` harness (kill/restart units, inject PipeWire/Mutter
    failures, verify safe state; `#[ignore]` + `BLACKROOM_SYSTEST=1`).
  - Depends on: Phase 8 review = PROCEED.
  - Verify: every experiment converges to `LOCAL_LOCKED` or a documented `FAILED_SAFE`
    with authority revoked and physical state restored; `kill -9` of hostd/agent and
    PipeWire restart recover without manual steps or reach `FAILED_SAFE` explicitly;
    100 activation/teardown cycles and a 1 h soak show no leaks (RSS, fds, threads,
    PipeWire objects, virtual monitors); race matrix (Doc 10 §41 priorities) passes;
    reboot invalidates prior sessions.
  - Gate: **FEAS-F** (fail-safe) and **FEAS-H** (same-session recovery) PASS. Report.

- [ ] **Phase 10 — Emergency** (`11.P12`; Docs 10 Exp 21–24; 06 §14–§19, §84–§105, §126–§129, §139–§141; 07 §17–§19, §49; 21 §18–§19)
  - Files: `crates/remote-emergencyd/` (evdev chord observer with 2000 ms hold, epoch-bump
    marker, logind `Session.Lock`, session-agent stop path, `emergency.sock` notify,
    `EMERGENCY_*` events only, full Doc 06 §97 hardening, `PrivateNetwork=yes`);
    `systemd/system/remote-emergencyd.service`; user fallback unit
    `systemd/user/remote-emergency-restore.service`; `docs/security/emergency-matrix.md`
    (Doc 06 §19 per-operation matrix: emergencyd alone / needs agent / needs hostd);
    experiments `exp21_emergency.rs`, `exp22_emergency_network_failure.rs`,
    `exp23_emergency_hostd_dead.rs`, `exp24_stale_after_emergency.rs`.
  - Depends on: Phase 9.
  - Verify: chord during `REMOTE_ACTIVE` → authority revoked, epoch incremented and
    persisted, session terminated, GNOME locked, display and input restored, stays locked;
    works with network down; works with `remote-hostd` SIGSTOPped/killed (Doc 06 §140,
    `RT-EMERGENCY-004`); stale PoC client cannot reconnect; repeated emergencies safe;
    daemon binary links no network/media/codec stack (`ldd` evidence).
  - Gate: **FEAS-G** PASS; **Go/No-Go decision** per Doc 10 §60 (all FEAS-A…H PASS,
    privileges acceptable, Mutter stable, recovery deterministic) and **Architecture
    Review #2**. Feasibility report `docs/gnome/feasibility-report.md` (Doc 02 §44) and
    `docs/security/poc-findings.md` (Doc 02 §46) written. Report.

### Stage III — Security authority and networking (only after Go)

- [ ] **Phase 11 — Security authority** (`11.P13`–`11.P17`; Docs 03 §123 order, 17 §75 order, 09, 06 §36–§38, §106–§112)
  - Files: `crates/blackroom-store/` (`SecretStore` with atomic writes, schema versions,
    migration, corruption → `REMOTE ACCESS DISABLED`); `crates/remote-hostd/src/
    {identity.rs, totp.rs, access_key.rs, recovery_codes.rs, trusted_devices.rs,
    auth_session.rs, session.rs, lease_issuer.rs, epoch.rs, revocation.rs, ratelimit.rs,
    audit.rs}`; `crates/pam-auth-helper/`; `/etc/pam.d/blackroom-console`;
    `systemd/system/remote-hostd.service` (hardened, watchdog); `admin.sock` + polkit
    policy `polkit/org.blackroom.console.policy`; `crates/blackroom-cli/` first verbs
    (`status`, `doctor`, `sessions`, `revoke-session`, `revoke-all`, `disable`, `enable`,
    `emergency-status`, `compatibility`, `diagnostics`); `docs/security/{authentication.md,
    credential-lifecycle.md, threat-model.md}` (Doc 03 §107).
  - Depends on: Phase 10 Go.
  - Verify: Doc 03 §113–§121 acceptance tests as `cargo test` (fake PAM) plus live PAM
    check; Doc 12 §7 authentication matrix (10 rows) automated; `RT-AUTH-001…015`,
    `RT-SESSION-001…005`, `RT-LEASE-001…007`, `RT-EPOCH-001…004`, `RT-FILE-001…005`
    automated; crash-consistency tests (Doc 17 §42) with kill during rotation/revocation/
    epoch increment; Doc 17 §74 data-security tests (gateway/agent/emergency cannot read
    secrets — as separate users); no secret in logs (grep-based redaction test).
  - Gate: **SEC-A…E, SEC-J** (Doc 09 §100) unit/integration PASS. Report.

- [ ] **Phase 12 — Gateway and protocol** (`11.P18` part, `11.P20`; Docs 16 §76 pipeline, 04 §11–§15, §34–§36, §69–§73, §104–§109; 06 §5, §41, §58, §72)
  - Files: `docs/protocol/{messages.md, ipc.md, errors.md}` (every message per Doc 16 §75,
    the C18 allow-list, casing table C19, limits); `crates/blackroom-ipc/` framing +
    schema validation + fuzz targets (`cargo fuzz`); `crates/remote-gateway/` (axum +
    rustls HTTPS, WebSocket signalling, origin/CSRF checks, rate limits, `hostd.sock`
    client, zero D-Bus, `User=remote-gateway`, hardened unit); `remote-hostd` gateway
    endpoint; mDNS `_blackroom._tcp` (LAN only); credential-envelope design
    (assessment §7.7) reviewed and implemented.
  - Depends on: Phase 11.
  - Verify: Doc 16 §69 15-category matrix per message type; `ptest001–007`,
    `timeout002–004`, `compat001`; `RT-PRIV-001…006` (gateway user cannot reach secrets,
    D-Bus, `/dev/input`, privileged verbs); malformed/oversized/replayed/stale rejected
    without state change; browser-less WebSocket client can authenticate and create a
    session on LAN.
  - Gate: Doc 16 §77 DoD items for authentication/session/IPC/errors/validation. Report.

- [ ] **Phase 13 — Browser client** (`11.P18`; Docs 08 §65 order, 04 §7–§9, §37–§39, §45–§47, §60; 16 §39–§40, §56–§58)
  - Files: `web/` (Vite + TypeScript; Preact or Lit chosen here; client state machine per
    Doc 08 §52; screens per Doc 08 §5–§36; WebCrypto device credential; capability
    detection; pointer lock for relative motion; IME/composition handling; accessibility
    Doc 08 §57; diagnostics panel; no secrets in storage/URLs); `web/tests` (vitest state
    machine + Playwright flows); static bundle served by `remote-gateway`.
  - Depends on: Phase 12.
  - Verify: Doc 08 §63 client criteria A–J automated where possible; `RT-BROWSER-001…007`;
    Chromium + Firefox pass the auth/new-device/trusted-device/disconnect/refresh flows
    against the local gateway; UI never shows the desktop before `REMOTE_ACTIVE` is
    received.
  - Gate: **Architecture Review #3** (Doc 11 after P18). Report.

- [ ] **Phase 14 — WebRTC** (`11.P19`–`11.P21`; Docs 04 §40–§44, §50–§56, §74–§86, §120–§122, 142 phases 3–5,8; 06 §75–§78; 19 §13–§15)
  - Files: `crates/remote-media/` (user-session child of the agent: GStreamer
    `pipewiresrc → encoder (vah264enc/nvh264enc/x264enc negotiated) → webrtcsink`, input
    data channel with sequence numbers bound to session+lease, lease-checked per event);
    signalling relay through gateway↔hostd (`signal_webrtc`); STUN/TURN config (`coturn`
    documented for self-hosting, short-lived credentials); reconnect semantics
    (re-authentication default); `PERF-LAN-1080P` baseline instrumentation.
  - Depends on: Phases 12–13, Phase 6 (EIS path).
  - Verify: LAN direct (IPv4/IPv6/mDNS), NAT with STUN, TURN fallback; media stops and
    input is dropped within lease TTL after abrupt browser close; `RT-NET-001…007`;
    WebRTC peer/PipeWire object counts return to baseline after 50 sessions; input latency
    measured (target ≈ 50–100 ms LAN, Doc 19 §4) and recorded as baseline, not asserted.
  - Gate: Doc 04 §144 items for connectivity/media/input; `WebRTC connected ≠ authorized`
    enforced by a single host-side gate function with tests. Report.

### Stage IV — Integration and hardening

- [ ] **Phase 15 — Full integration** (`11.P22`; Docs 00 §15–§16, §73; 04 §110–§112; 07; 13)
  - Files: end-to-end wiring of `network → auth → authz → session → lease → GNOME
    preparation → display isolation → input isolation → media → REMOTE_ACTIVE` and
    `failure → revoke → lock → restore → verify`; observability completion (Doc 13 event
    taxonomy, `blackroom show …`, `blackroom logs`, diagnostic bundle without secrets);
    `crates/blackroom-systest/tests/e2e_*.rs`.
  - Depends on: Phases 9–14.
  - Verify: Doc 00 §73 scenario end-to-end on this host (connect, operate with Wayland and
    XWayland apps and multiple workspaces, disconnect, failures, emergency); Doc 12 §47
    BLOCKER rows PASS; Doc 13 §55 observability acceptance list.
  - Gate: **Architecture Review #4**. Report.

- [ ] **Phase 16 — Adversarial testing** (`11.P23`, `11.P25`; Docs 18 §39 order, 09 §99–§103, 12 §19–§25, §36–§41)
  - Files: `crates/blackroom-systest/tests/rt_*.rs` covering all 82 `RT-*` attacks
    (automated / system-automated / manual with recorded evidence), `INV-SEC-001…010`
    assertions, Race A–F, fault injection in every state (Doc 00 §39), fuzzing campaign
    results; `docs/security/red-team-report.md` (Doc 18 §33 format); regression tests for
    every finding.
  - Depends on: Phase 15.
  - Verify: no `CRITICAL`; every `HIGH` fixed or accepted with written review; Doc 09
    §103 checklist answered from code and tests; Doc 00 §74 questions answered.
  - Gate: **SEC-A…J PASS**; **Architecture Review #5**. Report.

- [ ] **Phase 17 — Performance and reliability** (`11.P26`; Docs 19 §49 order, §42 profiles, §51 gates)
  - Files: metrics exposure (`blackroom status --metrics`, journald structured fields),
    `PERF-LAN-1080P/4K/WAN/DEGRADED/SOAK/CYCLE/FAILURE` runners in `blackroom-systest`,
    baselines and thresholds in `docs/ops/performance-baselines.md`.
  - Depends on: Phase 15.
  - Verify: 100/500/1000 cycles and 1/6/12/24 h soaks (48 h where practical) with flat
    RSS/fd/thread/PipeWire/WebRTC counts; degraded-network profiles reach safe states;
    `INV-PERF-001…010` hold under CPU/memory/disk/GPU/network pressure; Doc 19 §51 has no
    tripped gate.
  - Gate: Doc 19 §50 DoD. Report.

- [ ] **Phase 18 — Compatibility** (`11.P24`; Docs 20 §49–§63, 12 §28–§31, §43, 05 §79–§83)
  - Files: `crates/remote-hostd/src/compat/` (version + capability gating, cached
    validation invalidated on GNOME/Mutter/kernel/driver change, `UNKNOWN` never
    activates), `docs/ops/compatibility-matrix.md` (Ubuntu × GNOME × GPU incl. **hybrid
    NVIDIA+Intel** × display × input × browser × network × power).
  - Depends on: Phase 15.
  - Verify: Doc 20 §63 13-stage release compatibility gate on every available
    configuration; GNOME point-update re-validation flow tested by simulating a version
    change; matrix cells classified with evidence, untested cells `UNKNOWN`. **AMD is
    `UNKNOWN` for v1 by decision (no AMD hardware available, 2026-09-05); the supported-scope
    statement must say so explicitly.**
  - Gate: Doc 20 §62 DoD. Report.

- [ ] **Phase 19 — Packaging, installation and operations** (`11.P27`–`11.P29`; Docs 14 §58 order, 15 §57 order, 21 §56 order, 13 §56; 06 §146–§155)
  - Files: `packaging/debian/` (`blackroom-console` `.deb` via `cargo-deb` for development,
    `debian/` + `dh-cargo` for release; users/groups, directories and modes from assessment
    §6.2; minimal maintainer scripts; upgrade sequence Doc 15 §15; remote access disabled
    after install/upgrade); `blackroom setup` first-run wizard (Doc 14 §11 steps, gates
    SETUP-A…L, TOTP QR, Remote Access Key display, recovery codes, emergency test,
    explicit enable); `blackroom reset {soft,security,full}`, `repair`, uninstall path
    (Doc 14 §44–§47); runbook `docs/ops/runbook.md` (Doc 21 procedures, incident flow,
    credential-compromise procedures); signing and provenance (Doc 15 §26–§31: signed
    repo metadata + packages, SBOM via `cargo cyclonedx`).
  - Depends on: Phases 15–18.
  - Verify: Doc 15 `clean001` clean-machine install (VM acceptable for install mechanics,
    hardware for safety steps), `upg003` upgrade security test, `uninstall01` removal test,
    `matrix01` (N-1→N, N-2→N, clean, N→dev); Doc 14 §56 26-step first-run acceptance on
    this host; Doc 21 §49 recovery verification checklist after each runbook procedure.
  - Gate: Doc 15 `rc001` checklist; **Architecture Review #6**. Report.

### Stage V — Release

- [ ] **Phase 20 — Release candidate** (`11.P30`; Docs 00 §67, §73–§74; 12 §59; 15 §45–§46; 11 §56)
  - Files: release notes, `docs/` synchronised with implemented behaviour only (Doc 00 §46),
    known-limitations list, supported-scope statement (Doc 20 §58), signed artefacts.
  - Depends on: Phases 16–19 gates recorded.
  - Verify: full pipeline FEASIBILITY → FUNCTIONAL → SECURITY → RECOVERY → RED TEAM →
    PERFORMANCE → COMPATIBILITY → UPGRADE → OPERATIONAL → DOCUMENTATION (Doc 00 §67);
    Doc 12 §59 26-step release-gate scenario; Doc 00 §73 final integration and §74
    security review answered; independent read-only review (workflow policy).
  - Gate: no unresolved `CRITICAL`, no unaccepted `HIGH`, no `UNVERIFIED` P0 invariant.
    Report.

## Traceability (invariants → phases that prove them)

| Invariant | Proven in | Primary tests |
|---|---|---|
| INV-001 no auth → no control | 2, 11, 15 | `INV-SEC-001`, `RT-AUTH-*` |
| INV-002 no lease → no input | 2, 6, 11, 14 | `INV-SEC-002`, `RT-LEASE-001` |
| INV-003 lease binding | 2, 11 | `RT-LEASE-003…005`, `RT-SESSION-002/003` |
| INV-004 epoch revocation | 2, 10, 11 | `INV-SEC-003`, `RT-EPOCH-001…004` |
| INV-005 emergency wins | 9, 10, 16 | Race A/E, `RT-EPOCH-003`, `RT-EMERGENCY-001…007` |
| INV-006 remote failure ≠ unlock | 9, 15 | Doc 07 §56 H, `exp13–20` |
| INV-007 ends `LOCAL_LOCKED`/`FAILED_SAFE` | 2, 9, 17 | property test, `INV-PERF-007`, Race F |
| INV-008 physical input isolation | 7, 16 | `RT-INPUT-001…007`, `INV-SEC-006`, `exp09/38` |
| INV-009 physical display privacy | 5, 16 | `RT-DISPLAY-001…005`, `INV-SEC-007`, `exp06/37` |
| INV-010 restoration verified | 5, 7, 9 | `exp07/10/19/20`, `INV-PERF-009/010` |
| INV-011 emergency does not unlock | 10 | `exp21`, `RT-EMERGENCY-001` |
| INV-012 stale clients cannot reconnect | 10, 11, 14 | `exp24`, `RT-EMERGENCY-006`, `INV-SEC-008`, Race C |
| INV-013 no arbitrary privileged execution | 11, 12, 16 | `RT-PRIV-001…006`, `SEC-J` |
| INV-014 fail closed | 2, 9, 18 | `UNKNOWN` never activates; `exp19/20`; Doc 17 §46 corruption tests |

## Final Verification

- Run the configured project checks from `AGENTS.md` (`cargo test --workspace`,
  `cargo fmt --check`, `cargo clippy … -D warnings`, `cargo check`, plus `cargo audit`,
  `cargo deny check`, and `npm --prefix web run test|lint|typecheck` once `web/` exists).
- Confirm every acceptance criterion with current evidence in `docs/experiments/`,
  `docs/security/`, `docs/ops/` and the test suite; every phase report recorded.

## Blockers

- None for Phase 0. Phase 1 step 5 still needs the user-run `apt install` of development
  headers (`openssh-server` already installed 2026-09-05) and a verified SSH login from the
  second device.

## Execution Log

- 2026-09-04: roadmap drafted from the assessment; awaiting approval. No code written.
- 2026-09-05: approved by user. Licence GPL-3.0 (`GPL-3.0-or-later`). AMD `UNKNOWN` for v1.
  `openssh-server` installed (socket-activated `ssh.socket`). Execution starts with
  `plan-20260904-phase0-1-discovery-and-environment.md`.
