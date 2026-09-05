# Plan: Phase 3 — GNOME Session Discovery

**Created:** 2026-09-05
**Status:** complete
**Approved by:** user ("approved proceed", 2026-09-05)
**Task tier:** governed

## Goal

Turn Phase 0-1's read-only GNOME research (`exp01_session_discovery`,
`exp02_mutter_inventory`, `docs/gnome/{api-inventory,feasibility-research,
capability-report}.md`) into real, still non-mutating production code: real
`login1` session selection and Wayland/XWayland detection in
`crates/blackroom-gnome/src/mutter/session.rs`; real Mutter/Shell/systemd
capability detection producing the Doc 20 §8 constants in
`crates/blackroom-gnome/src/mutter/capability.rs`; a new `gnome-session-agent`
skeleton crate that starts under `systemctl --user`, discovers the session,
runs the capability gate, and reports its own `AgentState::SessionReady`; and
an `agent.sock` server with `SO_PEERCRED` peer verification. This is the first
phase making real GNOME/D-Bus calls (assessment C2); virtual display, physical
display/input isolation, and remote input stay mocked until Phases 4–7.

## Acceptance Criteria

- `mutter::session::discover()` selects the logind session by
  `UID + Seat=seat0 + Type=wayland + Class=user + Active=true`, never "the
  first session"; a unit test with fake/injected logind data proves a
  non-Wayland or ambiguous session is rejected (fails closed); a live run on
  this host (two sessions on `seat0`, Phase 0-1 finding) selects the correct
  one.
- Wayland-vs-XWayland detection is explicit (Doc 05 §14–§15), not inferred
  from `$DISPLAY` or process names (Doc 05 §12).
- `mutter::capability::detect()` produces all 16 Doc 20 §8 constants using the
  Doc 00 §35 five-tier vocabulary (`SUPPORTED, SUPPORTED_WITH_LIMITATIONS,
  EXPERIMENTAL, UNSUPPORTED, UNKNOWN`); a live run on this host reproduces
  `docs/gnome/capability-report.md`'s existing classifications; the Phase 3
  gate (`OS_SUPPORTED, GNOME_SUPPORTED, WAYLAND_SUPPORTED, SYSTEMD_SUPPORTED,
  SESSION_FOUND` all `SUPPORTED`) passes on this host.
- No GNOME mutation anywhere in this phase: only `Introspect`/`Get`/
  `ListSessions`/`GetCurrentState`-style read calls, matching the Phase 0-1
  constraint (never `CreateSession`, `RecordVirtual`, `ApplyMonitorsConfig`,
  `ConnectToEIS`, or `InputCapture.CreateSession`).
- `crates/gnome-session-agent` exists, builds, and starts as a systemd **user**
  unit (`systemd/user/gnome-session-agent.service`,
  `PartOf=graphical-session.target`); on a supported session it reaches
  `AgentState::SessionReady`; on a non-Wayland/non-GNOME session it fails
  closed to `AgentState::Failed` (never silently falls back), proven by both
  a fake-data unit test and a live check on this host.
- `agent.sock` listens on a Unix domain socket and verifies each peer via
  `SO_PEERCRED`-equivalent credentials; it never parses a password, TOTP
  value, or Access Key from any message.
- `AgentState` (Doc 05 §20's 11 values) is defined in `gnome-session-agent`,
  is not named `State` or anything importable-confusable with
  `blackroom_core::state::State`, and its doc comment states the one-sentence
  relationship, citing conflict **C3** (no new conflict number filed).
- `docs/gnome/session-discovery.md` documents the real implementation,
  cross-referencing the Phase 0-1 evidence it supersedes.
- `cargo test --workspace`, `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo check --workspace --all-targets`, `cargo deny check`, `cargo audit`
  all green; live-host checks are `#[ignore]`d and gated `BLACKROOM_SYSTEST=1`
  so plain `cargo test` never touches the real session (Doc 12 §52,
  `AGENTS.md`).

## Non-Goals

- No virtual display, capture, or any `RemoteDesktop`/`ScreenCast` session
  creation (Phase 4).
- No physical display isolation or `ApplyMonitorsConfig` mutation (Phase 5).
- No remote input, EIS, or physical input isolation (Phases 6–7).
- No GNOME lock / same-session validation (Phase 8).
- No change to `blackroom-core`'s state machine, transitions, lease, or epoch
  logic — the only revision is the evidence-cited capability-model change
  confined to `blackroom-gnome` (see Evidence And Decisions #2).
- No `remote-hostd` (does not exist yet); `agent.sock` has no real client this
  phase, only its own peer-credential-checked listener.
- No `blackroom-ipc` shared crate extraction; the socket listener lives
  directly in `gnome-session-agent` until a second real consumer exists.
- No complete concrete `GnomeBackend` implementation (e.g. a `MutterBackend`
  wiring all 13 Doc 05 §8 ops) — later phases' `mutter/*.rs` modules
  (`remote_desktop.rs`, `display_config.rs`, `eis.rs`, `lock.rs`) are what
  eventually get assembled into one.
- No polkit, authentication, TOTP, Access Key, or PAM work.
- No config-file loading (`/etc/blackroom-console/config.toml` does not exist
  yet) — Doc 05 §13's "configured remote user == session owner" ownership
  *check* is deferred to whichever phase adds config loading; this phase only
  exposes the `uid` field the future check will need.

## Evidence And Decisions

- **Sources cross-checked:** primary Doc 05 §8–§21 (read in full); Doc 00's
  own terse Phase 3 section (`00 — ...md` line 1568, "Implement: session
  detection, Wayland verification, GNOME detection, capability discovery");
  Doc 11 §8 "Phase 3 — GNOME Session Discovery" (line 223), whose embedded
  Definition of Done is "The adapter reliably identifies the intended active
  GNOME session on the supported environment"; Doc 11 §39 "Definition of Done
  — GNOME Feature" (real GNOME environment tested, Wayland tested, Mutter
  interaction verified, PipeWire behavior verified where relevant, failure
  behavior tested, restoration tested, version assumptions documented — the
  cross-cutting checklist this phase's Final Verification maps to); Doc 06
  §28–§32 (agent startup ordering/retry, agent logout invalidates remote
  sessions, user-session lifecycle states, systemd user-service rationale);
  roadmap line 150 (`docs/plans/plan-20260904-blackroom-console-master-roadmap.md`).
- **MemPalace:** `mempalace_status` loaded; `mempalace_diary_read` (agent
  `copilot`, wing `blackroom_console`) and `mempalace_search` show only Phase
  2 completion + "next: Phase 3 planning" — no prior Phase 3 synthesis exists,
  consistent with `docs/HANDOFF.md`. No contradiction, no degraded mode.
- **Code graph:** `Blackroom_Console` was indexed (3803 nodes/5654 edges) but
  `check_index_coverage` showed `metadata_changed` on
  `blackroom-core/src/{error,state}.rs` and `blackroom-gnome/src/{backend,
  fake,lib}.rs` — expected, since the Reviewer's post-index Phase 2 fixes
  touched them after that index generation. Re-indexed this session (now
  3814 nodes/5783 edges, current). A grep-based blast-radius check confirms
  `blackroom_gnome::{Capability, SessionInfo}` and `GnomeBackend` are used
  nowhere outside `blackroom-gnome` itself (blackroom-core has its own,
  unrelated `lease::Capability{View,Control}`, already a distinct enum per
  C6) — so item #2 below cannot touch already-reviewed Phase 2 logic.
- **1. Session discovery — port, don't depend on, the experiment.**
  `blackroom-experiments::session::discover()` already implements exactly the
  Doc 05 §12–14 selection rule (`zbus::blocking`, `login1.Manager
  .ListSessions` + per-candidate property `Get`, unique
  `Type=wayland ∧ Class=user ∧ Seat=seat0 ∧ User=uid ∧ Active` match, fail
  closed on 0 or >1 matches) and was proven correct on this exact host's
  two-seat0-session case (Phase 0-1, `exp01`). `blackroom-experiments` is
  explicitly "discardable" (architecture.md §1); production code must not
  depend on it. Decision: port the algorithm into
  `crates/blackroom-gnome/src/mutter/session.rs` as new production code
  returning `Result<SessionInfo, BlackroomError>`, citing the experiment as
  prior art in a doc comment.
- **2. `GnomeBackend`/`SessionInfo` signature change — required, evidence-cited,
  confined to `blackroom-gnome`.** The Phase 2 placeholder `Capability` enum
  (`VirtualDisplay, RemoteInput, PhysicalInputIsolation`) inside
  `SessionInfo.capabilities: Vec<Capability>` is explicitly documented in
  `backend.rs` as "a minimal placeholder set for Phase 2; the full Doc 20 §8
  constant list is Phase 3 research." A presence-only `Vec` cannot express
  Doc 00 §35's five tiers — it cannot distinguish `UNSUPPORTED` (confirmed
  broken) from `UNKNOWN` (not yet tested), even though both must not activate.
  Decision: remove `Capability` and `SessionInfo.capabilities` entirely; add
  `CapabilityTier` (5 values) and `CapabilityReport` (16 named Doc 20 §8
  fields) in the new `capability.rs`. Capability detection is **not** added
  as a 14th `GnomeBackend` trait method — Doc 05 §8's pseudocode interface
  lists exactly 13 ops and Doc 05 §9 treats capability detection as a
  separate startup-time concern, not a per-operation one — so it stays a free
  function (`capability::detect()`), called once by `gnome-session-agent` at
  startup, and the already-reviewed 13-op trait is untouched. Separately,
  Doc 05 §12 lists `uid` and "active state" as fields the agent must
  determine; `discover()` already computes both as part of its selection
  logic at zero extra cost, so `SessionInfo` gains `uid: u32` and
  `active: bool`. Net new shape:
  `SessionInfo{session_id, uid, seat, is_wayland, active}`. Only
  `fake.rs`'s `discover_session()` literal needs updating to match (its sole
  internal use).
- **3. `AgentState` — new, distinct type; relationship to `blackroom_core::state::State`.**
  Doc 05 §20 defines 11 values (`SESSION_UNKNOWN, SESSION_READY, PREPARING,
  VIRTUAL_DISPLAY_READY, PHYSICAL_DISPLAY_ISOLATED, PHYSICAL_INPUT_ISOLATED,
  REMOTE_READY, REMOTE_ACTIVE, RESTORING, RESTORED, FAILED`) describing the
  GNOME agent's own local-subsystem readiness — this is not
  `blackroom_core::state::State`'s 11 canonical states (Doc 07 §4–5, Phase 2,
  already implemented and reviewed) and must never be confused with it.
  Decision: define `AgentState` in `crates/gnome-session-agent`, mirroring
  `blackroom_core::state::State`'s established pattern (`ALL` const array,
  `as_str()` returning the `SCREAMING_SNAKE_CASE` wire form, `Display` impl,
  an exhaustiveness test). One-sentence relationship, to appear verbatim in
  its doc comment: *"`AgentState` is `gnome-session-agent`'s own
  local-subsystem readiness state; `blackroom_core::state::State` remains the
  sole cross-host authority (owned by `remote-hostd`, Phase 11+), and
  `AgentState` never substitutes for it."* Doc 05 §21's informal
  `LOCAL_ACTIVE → PREPARING_REMOTE → REMOTE_ACTIVE → RECOVERING → LOCKED`
  sketch is already covered by conflict **C3** (assessment §5 already lists
  Document 05 among the sources resolved to the canonical 11-state list) —
  cite C3, file no new conflict number.
- **4. Capability-detection technique.** Production `capability.rs` uses
  targeted `zbus::blocking::Proxy::new(conn, destination, path, interface)` +
  `get_property`/`call` against the exact interfaces/paths
  `docs/gnome/api-inventory.md` already confirmed exist (mirrors
  `session.rs`'s own style) — **not** `exp02`'s generic hand-rolled
  introspection-XML line-scanner, which was a Phase-1-only technique for
  discovering *unknown* interfaces. Non-D-Bus facts (`OS_SUPPORTED`,
  `GNOME_SUPPORTED`, `SYSTEMD_SUPPORTED`, `GPU_CAPABLE`) port `exp00`'s proven
  techniques: `/etc/os-release` read, `dpkg-query` for package versions
  (including its versioned-`libmutter-<abi>-0` fallback), `/sys/class/drm` +
  `/proc/modules` reads. Still strictly read-only throughout (`Get`/
  `Introspect`/the one already-proven `GetCurrentState` call only).
- **5. `uid` retrieval — no environment variables, no `unsafe`.** Doc 05 §12
  explicitly forbids identifying a session via "arbitrary environment
  variables"; reading `$UID`/`$USER` would violate this. `blackroom-gnome`
  has `#![forbid(unsafe_code)]`. Decision: promote `rustix` (already
  evaluated in `docs/security/architecture.md`, pinned `1.1.4`, safe POSIX
  wrapper, active) from evaluated→in-use for `rustix::process::getuid()` —
  same evaluated→in-use pattern as Phase 2's `ulid`/`ed25519-dalek`
  promotions — instead of `exp01`'s `Command::new("id").args(["-u"])`
  shell-out (avoids a process spawn on a security-relevant path).
- **6. `SO_PEERCRED` — verify before committing.** `doc.rust-lang.org`'s
  `UnixStream` page lists `peer_cred() -> Result<UCred>` grouped with other
  methods stabilized at `1.10.0` (well under the pinned MSRV 1.96), but the
  same page also renders one stray `#![feature(peer_credentials_unix_socket)]`
  example block for it — inconclusive from documentation text alone. Decision:
  step 9 below verifies directly against the pinned toolchain before writing
  the listener; if `UnixStream::peer_cred()` is stable, no new dependency is
  needed; if not, fall back to `rustix` (already being added per #5) rather
  than raw `libc` + `unsafe`.
- **7. New crate `blackroom-systest` — created this phase, not invented.**
  `AGENTS.md` already declares "Real-GNOME system tests live in
  `crates/blackroom-systest`, are `#[ignore]`d, and run only with
  `BLACKROOM_SYSTEST=1` on a prepared host (never from plain `cargo test`)"
  and `docs/security/architecture.md` §1 already lists it in the intended
  repository layout — but no phase has needed a live-host test until now.
  Phase 3 is explicitly "the FIRST phase making real GNOME/D-Bus calls" and
  the roadmap's own verify line requires "live check" evidence in addition to
  fake-data unit tests. Decision: instantiate the already-decided
  `blackroom-systest` crate now (not a new pattern) to house the two live
  checks (two-session selection, Phase 3 capability gate), `#[ignore]`d and
  gated by `BLACKROOM_SYSTEST=1` per Doc 12 §52.
- **8. `blackroom-core/src/error.rs` — no change.** Reviewed the full 30-code
  catalogue (the currently open file): `GnomeSessionUnavailable` (no matching
  session), `HostUnsupported` (capability-gate failure), and
  `MutterUnavailable` (D-Bus/introspection failure) already cover every
  Phase 3 failure mode. Decision: no edits to `error.rs` this phase.
- **9. systemd unit ordering.** `assessment §6.2` already decided
  `PartOf=graphical-session.target`. `feasibility-research.md` topic 8
  already shows this host's live `systemctl --user list-units` output
  includes `gnome-session-manager@ubuntu.service` and
  `graphical-session.target`; the exact `After=`/`Wants=` target is confirmed
  live in step 10, not guessed. Doc 06 §29 requires the agent to wait/retry
  (never fail permanently) if it starts before GNOME is ready — implemented
  as bounded startup retry reporting `AgentState::SessionUnknown`/`Preparing`
  while retrying, per Doc 06 §31's "do not treat `active` and `unlocked` as
  identical" style explicit-state discipline.
- **10. Agent logout (Doc 06 §30) — acknowledged, not fully implemented.** No
  remote sessions exist to invalidate yet (`remote-hostd` doesn't exist).
  `PartOf=graphical-session.target` already gives correct-by-construction
  teardown (systemd stops the agent when the graphical session stops) for
  this phase; full "invalidate remote sessions on logout" bookkeeping is
  deferred to whichever phase adds real remote-session state to the agent.

## Risks

- Doc 20 §8 constants are evidence from this exact GNOME/Mutter version; an
  OS update could change interface presence. Mitigation: keep
  `capability::detect()` unit-testable against injected/fake D-Bus-shaped
  inputs (mirroring `FakeGnomeBackend`'s pattern) so re-verification after an
  update is cheap (`feasibility-research.md` topic 1's own escalation note).
- The two-sessions-on-`seat0` condition is real hardware state unique to this
  host; a fake-data unit test alone cannot prove live selection — the live
  `blackroom-systest` check is mandatory, not optional polish.
- `SO_PEERCRED`/`peer_cred()` stability at MSRV 1.96 is unconfirmed from docs
  alone; a wrong assumption would surface mid-implementation. Mitigated by
  verifying first (Evidence #6) before writing dependent code.
- Removing `Capability`/`SessionInfo.capabilities` is a breaking
  `blackroom-gnome` API change. Blast radius is proven zero today by grep;
  re-grep after implementation in case anything changed mid-execution.
- Guessing the systemd `After=` target wrong could start the agent before
  GNOME is ready. Mitigated by Doc 06 §29's mandatory retry/backoff (never a
  hard failure) plus live verification against this host before finalizing.
- Capability detection must stay strictly read-only; an accidental mutating
  call would violate the phase's hard non-goal boundary and could disrupt the
  live desktop. Mitigated by reviewing every new zbus call against the
  explicit allow-list (`Introspect`/`Get`/`GetCurrentState`/`ListSessions`/
  session-property `Get` only).
- Two new workspace members touch the root `Cargo.toml`; must not disturb the
  existing shared `[workspace.package]`/`[workspace.lints.rust]` tables.

## Steps

- [x] 1. Revise the capability model in `blackroom-gnome`: remove the Phase-2
      placeholder `Capability` enum and `SessionInfo.capabilities`; expand
      `SessionInfo` with `uid: u32` and `active: bool` (Doc 05 §12); update
      `fake.rs`'s `discover_session()` literal to match.
  - Files: `crates/blackroom-gnome/src/backend.rs`,
    `crates/blackroom-gnome/src/fake.rs`
  - Depends on: none
  - Verify: `cargo test -p blackroom-gnome` (existing 6 tests still green)

- [x] 2. Add `zbus = "5.19.0"` and `rustix = "1.1.4"` to
      `crates/blackroom-gnome/Cargo.toml`; scaffold
      `crates/blackroom-gnome/src/mutter/mod.rs` (matches the existing
      `events/mod.rs`/`protocol/mod.rs` module-root style already used in
      `blackroom-core`, not the sibling-file style).
  - Files: `crates/blackroom-gnome/Cargo.toml`,
    `crates/blackroom-gnome/src/mutter/mod.rs`,
    `crates/blackroom-gnome/src/lib.rs`
  - Depends on: step 1
  - Verify: `cargo check -p blackroom-gnome`

- [x] 3. Implement `crates/blackroom-gnome/src/mutter/session.rs`: port
      `blackroom-experiments::session::discover()`'s algorithm as production
      code returning `Result<SessionInfo, BlackroomError>` (uid via
      `rustix::process::getuid()`); add explicit Wayland-vs-XWayland
      corroboration (Doc 05 §14–15) as a secondary signal alongside the
      primary `Type=wayland` selection criterion.
  - Files: `crates/blackroom-gnome/src/mutter/session.rs`,
    `crates/blackroom-gnome/src/mutter/mod.rs`
  - Depends on: step 2
  - Verify: unit tests with injectable fake logind data (non-Wayland session
    rejected; zero-match and ambiguous->1-match both fail closed; unique
    match selected); `cargo test -p blackroom-gnome`

- [x] 4. Implement `crates/blackroom-gnome/src/mutter/capability.rs`:
      `CapabilityTier` (Doc 00 §35, 5 values) and `CapabilityReport` (Doc 20
      §8, 16 named fields) plus `detect()`, using targeted zbus calls for
      D-Bus-derived constants and ported `exp00` techniques
      (`/etc/os-release`, `dpkg-query`, `/sys/class/drm`, `/proc/modules`) for
      the rest; add a gate method checking
      OS/GNOME/Wayland/systemd/session == `Supported`.
  - Files: `crates/blackroom-gnome/src/mutter/capability.rs`,
    `crates/blackroom-gnome/src/mutter/mod.rs`
  - Depends on: step 3
  - Verify: unit tests per constant's presence→tier mapping using
    injectable/fake inputs; `cargo test -p blackroom-gnome`

- [x] 5. Wire `lib.rs` exports (`pub mod mutter;` plus any new public
      re-exports) and update `backend.rs`'s module doc comment (no longer
      claims "no real GNOME/Mutter/D-Bus call is made anywhere in this
      crate").
  - Files: `crates/blackroom-gnome/src/lib.rs`,
    `crates/blackroom-gnome/src/backend.rs`
  - Depends on: steps 3–4
  - Verify: `cargo clippy -p blackroom-gnome --all-targets -- -D warnings`;
    `cargo doc -p blackroom-gnome --no-deps`

- [x] 6. Add `crates/gnome-session-agent` as a new workspace member (root
      `Cargo.toml`); scaffold its `Cargo.toml` (depends on `blackroom-core`,
      `blackroom-gnome`, `zbus`) with a thin `src/main.rs` over a real
      `src/lib.rs` (so internal logic stays unit-testable via
      `cargo test -p gnome-session-agent` without an integration-only
      binary).
  - Files: `Cargo.toml`, `crates/gnome-session-agent/Cargo.toml`,
    `crates/gnome-session-agent/src/main.rs`,
    `crates/gnome-session-agent/src/lib.rs`
  - Depends on: step 5
  - Verify: `cargo check --workspace`

- [x] 7. Implement `AgentState` in `crates/gnome-session-agent/src/state.rs`
      (11 Doc 05 §20 values), mirroring `blackroom_core::state::State`'s
      `ALL`/`as_str()`/`Display`/exhaustiveness-test pattern; doc comment
      states the one-sentence relationship citing C3 (Evidence #3).
  - Files: `crates/gnome-session-agent/src/state.rs`,
    `crates/gnome-session-agent/src/lib.rs`
  - Depends on: step 6
  - Verify: `cargo test -p gnome-session-agent`

- [x] 8. Implement the agent's startup sequence: call
      `mutter::session::discover()` then `mutter::capability::detect()`,
      transition `AgentState` (`SessionUnknown → Preparing → SessionReady` on
      gate pass; `→ Failed` on gate failure or non-Wayland/non-GNOME, never a
      silent fallback), with bounded retry/backoff if GNOME is not yet ready
      (Doc 06 §29).
  - Files: `crates/gnome-session-agent/src/startup.rs`,
    `crates/gnome-session-agent/src/lib.rs`
  - Depends on: step 7
  - Verify: unit test with injected fake session/capability results proving
    refusal (`Failed`) on a non-Wayland/non-GNOME session;
    `cargo test -p gnome-session-agent`

- [x] 9. Verify `std::os::unix::net::UnixStream::peer_cred()` stability at
      the pinned toolchain (1.96); implement the `agent.sock` listener
      (`SO_PEERCRED`-equivalent peer verification via `peer_cred()` or the
      `rustix` fallback), rejecting unauthorized peers; no password/TOTP/
      Access-Key field anywhere in message handling.
  - Files: `crates/gnome-session-agent/src/ipc.rs`,
    `crates/gnome-session-agent/src/lib.rs`
  - Depends on: step 8
  - Verify: unit test connecting a real `UnixStream` pair and asserting peer
    credentials are extracted and an unauthorized peer is rejected;
    `cargo test -p gnome-session-agent`

- [x] 10. Write `systemd/user/gnome-session-agent.service`
       (`PartOf=graphical-session.target`, `After=` target confirmed live
       against this host's `systemctl --user list-units` output,
       `ExecStart`, restart policy).
  - Files: `systemd/user/gnome-session-agent.service`
  - Depends on: step 9
  - Verify: `systemd-analyze --user verify
    systemd/user/gnome-session-agent.service`; manual
    `systemctl --user link/start/status` smoke test on this host (roadmap's
    "agent starts with the graphical session via `systemctl --user`,
    reports `SESSION_READY`" criterion), evidence captured in
    `docs/gnome/session-discovery.md`

- [x] 11. Create `crates/blackroom-systest` skeleton (new workspace member)
       with the two live-host checks: two-session-on-`seat0` selection, and
       the Phase 3 capability gate; `#[ignore]`d, gated `BLACKROOM_SYSTEST=1`
       plus a minimal host preflight (`XDG_SESSION_TYPE=wayland` present).
  - Files: `Cargo.toml`, `crates/blackroom-systest/Cargo.toml`,
    `crates/blackroom-systest/tests/session_discovery.rs`
  - Depends on: step 4 (needs `capability::detect()`/`session::discover()`)
  - Verify: `cargo test -p blackroom-systest -- --ignored` with
    `BLACKROOM_SYSTEST=1` set, run live on this host; plain
    `cargo test --workspace` (no env var) does not execute these

- [x] 12. Write `docs/gnome/session-discovery.md` documenting the real
       implementation, cross-referencing Phase 0-1 evidence
       (`api-inventory.md`, `feasibility-research.md`, `capability-report.md`)
       and the `AgentState`/C3 relationship.
  - Files: `docs/gnome/session-discovery.md`
  - Depends on: steps 1–11
  - Verify: manual read-through cross-checking every citation against the
    live files/output it references

- [x] 13. Update `docs/HANDOFF.md`, `/memories/repo/blackroom-console.md`, and
       a MemPalace checkpoint (wing `blackroom_console`) reflecting Phase 3
       completion.
  - Files: `docs/HANDOFF.md`
  - Depends on: steps 1–12
  - Verify: `python3 .github/skills/project-doctor/scripts/doctor.py` reports
    0 errors/0 warnings; `docs/HANDOFF.md` stays ≤ 40 lines / 3 KB

- [x] 14. Full workspace verification and independent review: `cargo test
       --workspace`, `cargo fmt --check`,
       `cargo clippy --workspace --all-targets -- -D warnings`,
       `cargo check --workspace --all-targets`, `cargo deny check`,
       `cargo audit`; re-index `Blackroom_Console` in codebase-memory
       (`name=Blackroom_Console` explicit); independent Reviewer-agent pass
       (Phase 2 showed self-review alone misses real gaps).
  - Files: none new
  - Depends on: steps 1–13
  - Verify: all commands exit green; Reviewer subagent verdict recorded in
    the Execution Log

## Final Verification

- Run the configured project checks from `AGENTS.md`
  (`cargo test --workspace`; `cargo fmt --check && cargo clippy --workspace
  --all-targets -- -D warnings`; `cargo check --workspace --all-targets`).
- Confirm every acceptance criterion above with current evidence (live-host
  output, not just green tests).
- Cross-check against Doc 11 §39's GNOME-feature Definition of Done: real
  GNOME environment tested (yes, `blackroom-systest`); Wayland tested (yes,
  `session.rs`); Mutter interaction verified (yes, `capability.rs` live
  report); PipeWire behavior verified where relevant (not applicable this
  phase — `PIPEWIRE_CAPABLE` stays `EXPERIMENTAL`, no session created);
  failure behavior tested (yes, `Failed` path); restoration tested (not
  applicable — nothing is created this phase to restore); version
  assumptions documented (yes, `session-discovery.md`).

## Blockers

- None.

## Execution Log

- 2026-09-05: Plan drafted. No code written yet.
- 2026-09-05: All 14 steps implemented in one continuous session, 8
  coherent commits. Live-verified throughout on this host (real two-
  seat0-session selection, all 16 capability constants matching
  `capability-report.md`, real `systemctl --user` smoke test, real
  `agent.sock` SO_PEERCRED connection via `socat`). `cargo test/fmt/
  clippy(-D warnings)/check --workspace`, `cargo deny check`, `cargo
  audit` all green (170 deps, 0 advisories, 123 tests across the
  workspace). `std::os::unix::net::UnixStream::peer_cred()` confirmed
  still unstable at the pinned `rustc 1.96.0` by direct compile (the
  standard library docs page was misleadingly showing nightly-only
  content); used `rustix::net::sockopt::socket_peercred` instead.
- 2026-09-05: Independent review (Reviewer subagent, read-only).
  **Verdict: FAIL initially**, 3 real gaps found (2 High, 1 Medium):
  (1) `gnome-session-agent`'s startup treated every `discover_session`
  error as transient/retryable, including definitive mismatches (an
  X11-only session, an ambiguous seat0 host) that retrying can never
  fix — fixed by splitting `session::select()` into `NoneYet` (transient) vs
  `DefinitiveMismatch`/`Ambiguous` (permanent), mapped to
  `ErrorCode::GnomeSessionUnavailable` (`Retryable::Conditional`) vs
  `ErrorCode::HostUnsupported` (`Retryable::No`) respectively, and having
  `startup::attempt` branch on `error.retryable` (reusing the project's
  existing `Retryable` mechanism, Doc 16 §53, rather than inventing new
  machinery); (2) `capability::detect()` hardcoded 4 of the 16 Doc 20 §8
  constants as literal `Unknown` (correct values, matching
  `capability-report.md`, but not computed from any evidence gathered
  this phase) while `docs/gnome/session-discovery.md` overclaimed all 16
  were live-verified — fixed by naming the 4 structurally-fixed constants
  via a documented `NOT_YET_DETERMINABLE` constant and correcting the docs
  to state precisely which 12 are live-computed vs. which 4 are
  structurally fixed pending later phases; (3) a per-candidate logind
  property-read failure was silently skipped in `fetch_candidates`, which
  could manufacture a false "unique" match if a genuinely competing
  session became transiently unreadable — fixed by propagating the
  failure as a whole-attempt (retryable) error instead. All three fixes
  applied; re-verified: 32 blackroom-gnome + 12 gnome-session-agent tests
  green, live behavior re-confirmed unchanged (still reaches
  `SESSION_READY` selecting the correct session), fmt/clippy -D warnings
  clean workspace-wide. LESSON (matches Phase 2): a fresh independent
  review pass caught real correctness gaps that direct re-reading of
  freshly-written code, and even live verification of the happy path, did
  not — the failure-path logic (not the happy path) is where self-review
  keeps missing things.
