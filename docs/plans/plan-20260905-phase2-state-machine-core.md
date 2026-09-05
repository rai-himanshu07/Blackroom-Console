# Plan: Phase 2 — State Machine Core

**Created:** 2026-09-05
**Status:** complete
**Approved by:** user (Himanshu), 2026-09-05 — "approved proceed"
**Task tier:** governed

Elaborates the master roadmap's Phase 2 step
([plan-20260904-blackroom-console-master-roadmap.md](plan-20260904-blackroom-console-master-roadmap.md)
§Steps, "Phase 2 — State machine core", `11.P2`/`11.P13`/`11.P14` logic-only).
Primary source: Document 07 "Remote Session State Machine" (canonical for the
state machine specifically per assessment §4.1 authority ordering — layer 2,
"externally observable safety properties … must remain true"). Cross-checked
against Document 00 §47 Phase 2 and Document 11 §7 Phase 2 + §37 state-transition
Definition of Done.

## Goal

`blackroom-core` and `blackroom-gnome` implement the platform-independent
remote-session state machine — 11 canonical states, the full 25-transition
table, invariants, transition IDs, a `StateMachineLock`, idempotent operation
wrappers, a signed `ControlLease`, a monotonic security epoch, the `err001`
error catalogue, and the structured event/protocol envelope — validated entirely
against an in-memory `GnomeBackend` fake with fault injection. No real
GNOME/Mutter/D-Bus call is made. `cargo test -p blackroom-core -p
blackroom-gnome` is green, `Blackroom_Console` is indexed in codebase-memory,
and Document 11 §37's per-transition Definition of Done is satisfied for all
25 transitions.

## Acceptance Criteria

- `state.rs` defines exactly the 11 canonical states (Doc 07 §4, assessment
  C3): `LOCAL_ACTIVE, LOCAL_LOCKED, AUTHENTICATING, AUTHENTICATED,
  PREPARING_REMOTE, REMOTE_ACTIVE, REMOTE_DEGRADED, TEARING_DOWN, RECOVERING,
  EMERGENCY, FAILED_SAFE`.
- `transition.rs` implements every row of the Doc 07 §8 transition table
  (25 transitions, counted and reproduced below) — every legal transition has
  a test, every illegal transition is rejected and tested, and Document 11
  §37's 7-point per-transition Definition of Done (legal works, illegal
  rejected, failure path tested, rollback tested, concurrent-event behavior
  tested, logging exists, final state verified) is satisfied for all 25 rows.
- The priority resolver (Doc 07 §24) is implemented and tested: `EMERGENCY >
  SAFETY/FAILURE > LEASE_EXPIRY > DISCONNECT > NORMAL STATE TRANSITION >
  RECONNECT`; a concurrent `reconnect + emergency` on `REMOTE_ACTIVE` always
  resolves to `EMERGENCY`, never `REMOTE_ACTIVE`.
- `StateMachineLock` (state, active_session, active_lease, security_epoch,
  transition_id) serializes transitions; idempotent wrappers exist for every
  Doc 07 §27 operation (`revoke_remote_input`, `lock_session`,
  `restore_display`, `destroy_virtual_monitor`, …) and calling each twice is
  tested to be safe (Invariant 9, Doc 16 §34–§36).
- Transition IDs use the project's `tr_<ULID>` format (architecture.md §2),
  not the generic UUID example in Doc 07 §26.
- `lease.rs`'s `ControlLease` has exactly the 8 assessment-C5 fields
  (`session_id, host_id, user_id, client_id, security_epoch, issued_at,
  expires_at, capabilities`), is signed/verified with `ed25519-dalek` 3.0.0
  (already decided, architecture.md §4/§6 — reused, not re-litigated), and
  rejects expired/revoked/epoch-mismatched/wrong-session leases (Invariant 1).
- `epoch.rs` implements a monotonically increasing `SecurityEpoch` that never
  decreases and an `EpochStore` abstraction whose in-memory fake demonstrates
  the Doc 17 §23–§24 contract (survives a simulated restart at/above its prior
  value; a corrupted/inconsistent store yields fail-safe, never a guess).
  Real disk-backed persistence is explicitly out of scope (see Non-Goals).
- `error.rs` defines the full Doc 16 §51 `err001` code set and the §52
  `ErrorResponse` shape (`code, category, retryable, user_message,
  diagnostic_id`); no `Display`/logging path ever emits a password, TOTP
  value, Access Key, session secret, or stack trace.
- `events/*.rs` emits the Doc 13 §6 state-transition event shape
  (`timestamp, event, previous_state, new_state, transition_id, trigger,
  component, result, failure_code`) and the §7 structured-log field set on
  every transition attempt (success or failure).
- `protocol/*.rs` defines the Doc 16 §32 message envelope, §33 request IDs,
  and the §34–§37 idempotency/duplicate/stale-request/timeout primitives used
  by `lock.rs`'s wrappers and `transition.rs`'s stale-event rejection.
- `blackroom-gnome`'s `GnomeBackend` trait has the 13 Doc 05 §8 operations;
  `fake.rs`'s `FakeGnomeBackend` supports fault injection modes fail, timeout,
  partial, duplicate, and concurrent, cross-checked against
  `docs/gnome/api-inventory.md` (mapping table in Evidence And Decisions).
- `crates/blackroom-core/tests/` covers: all 25 legal transitions, a
  representative illegal transition per state, the Doc 07 §9 activation
  transaction / §10 rollback / §5.8 teardown sequences, idempotency, the
  concurrent-event priority resolver, Doc 07 §28 / Doc 17 §58 startup
  reconciliation (persisted `REMOTE_ACTIVE` + actual GNOME session gone →
  recovery, not continuation), the Doc 07 §7 Invariants 1–10, and the Doc 07
  §56 Scenarios A–J as named tests.
- A property test (Doc 12 §54, via `proptest`) generates random event
  sequences and asserts: remote input is allowed only with authentication +
  authorization + valid session + valid epoch + valid lease + valid GNOME
  state; `REMOTE_ACTIVE` is never reached without the full guard set; every
  sequence that includes a failure event ends the run in `LOCAL_LOCKED` or
  `FAILED_SAFE`.
- `cargo test -p blackroom-core -p blackroom-gnome`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo fmt --check`, `cargo check --workspace
  --all-targets`, `cargo deny check`, and `cargo audit` are all green.
- `Blackroom_Console` is indexed in codebase-memory once the source above
  exists (assessment §9 workflow-alignment row; roadmap Phase 2 Verify line).
- Conflict C26 (Doc 11 §7's abbreviated 8-state Phase 2 list vs Doc 07's
  canonical 11 states) is recorded in the assessment doc's conflict register
  rather than silently resolved.

## Non-Goals

- No real GNOME/Mutter/PipeWire/D-Bus calls. `blackroom-gnome` in this phase
  is the trait + in-memory fake only; a real Mutter-backed implementation is
  Phase 3+ (assessment C2, mock-first).
- No `remote-hostd`, `remote-gateway`, PAM, TLS, or browser/web work. No
  authentication or session-creation wire messages beyond the generic
  envelope/error/lease/transition-event types already required for the state
  machine itself (full Plane A–D protocol is Stage III, Phases 11–14).
- No re-deciding crates already fixed in `docs/security/architecture.md`:
  `ed25519-dalek`, `ulid`, `rand`/`getrandom`, `zeroize` are reused as
  specified, not re-evaluated.
- No real disk-backed persistence for the security epoch, control lease, or
  session state. `blackroom-core` remains **no I/O** by architectural
  decision (architecture.md §1 repository layout); `epoch.rs`/`lease.rs` in
  this phase define the value types, invalidation logic, and store *traits*
  with in-memory fakes only. Durable storage is `blackroom-store` /
  Phase 13–14 (Doc 11 §18–§19) work, not created here.
- No `tokio` or other async runtime in `blackroom-core` or `blackroom-gnome`;
  both stay synchronous (`std::sync` primitives only). `tokio` remains
  reserved for `remote-gateway` (architecture.md crate table).
- No `schemars`-generated JSON Schema yet — wire types use `serde` derive
  only; schema generation is deferred to when real IPC parsing lands
  (Phase 12, Doc 16 §62).
- No `thiserror` or other error-derive dependency — `error.rs` is a
  hand-rolled enum + manual `Display`/`Error` impl (Doc 00 §51: "do not add a
  dependency merely to avoid a small amount of implementation work"; ~30
  match arms is not a significant burden).
- No changes to `remote-hostd`/`gnome-session-agent`/other not-yet-created
  crates, and no edits to files outside the list in Steps.

## Evidence And Decisions

**Memory:** `mempalace_status` OK (v3.9.0, no integrity errors). `wing
blackroom_console` diary (`copilot`, 6 entries) and `mempalace_search` both
confirm the stored decisions below are current and uncontradicted — no
`MEMORY DEGRADED`.

**Code intelligence:** `mcp_codebase-memo_list_projects` does not list
`Blackroom_Console` (only `AI_TOOL`, `adaptive-workflow-configurator`,
`media-user-Playground-Playground_Sys-QC_Tool` exist). This matches
assessment §2 ("not indexed and there is no code to index … indexing should
be added to the Phase 2 plan") — **CODE GRAPH DEGRADED, expected, not a fresh
failure.** Per `docs/CODE_INTELLIGENCE.md` degraded-mode convention, structure
was discovered by direct file reads/grep instead of graph queries; Step 12
below indexes the project once source exists.

**States (Doc 07 §4–§5, lines 78–366; assessment C3):** the 11 states listed
in Acceptance Criteria, each with the entry/exit conditions quoted in
§5.1–§5.11. `LOCAL_LOCKED` is both the disconnect target and the pre-remote
staging state; `AUTHENTICATED != REMOTE_ACTIVE` (Invariant 2) — authorization
and preparation are still required after authentication.

**Transition table — 25 transitions (Doc 07 §8, lines 582–613; verified by a
direct read, not only the extraction pass):**

| # | Current State | Event | Guard | Action | Next State |
|---|---|---|---|---|---|
| 1 | LOCAL_ACTIVE | lock | session available | lock GNOME | LOCAL_LOCKED |
| 2 | LOCAL_LOCKED | connection | remote access enabled | begin authentication | AUTHENTICATING |
| 3 | AUTHENTICATING | auth success | all required factors valid | issue session credential | AUTHENTICATED |
| 4 | AUTHENTICATING | auth failure | retry limit not exceeded | reject | AUTHENTICATING |
| 5 | AUTHENTICATING | retry limit | limit exceeded | temporary block | LOCAL_LOCKED |
| 6 | AUTHENTICATED | authorization success | policy allows access | create lease | PREPARING_REMOTE |
| 7 | AUTHENTICATED | timeout | timeout exceeded | invalidate credential | LOCAL_LOCKED |
| 8 | PREPARING_REMOTE | success | all safety checks pass | activate remote control | REMOTE_ACTIVE |
| 9 | PREPARING_REMOTE | failure | rollback possible | rollback | LOCAL_LOCKED |
| 10 | PREPARING_REMOTE | failure | rollback uncertain | fail-safe recovery | FAILED_SAFE |
| 11 | REMOTE_ACTIVE | lease renewal | valid | continue | REMOTE_ACTIVE |
| 12 | REMOTE_ACTIVE | transient failure | lease remains valid | suspend/recover | REMOTE_DEGRADED |
| 13 | REMOTE_ACTIVE | disconnect | any | teardown | TEARING_DOWN |
| 14 | REMOTE_ACTIVE | lease expiry | no renewal | teardown | TEARING_DOWN |
| 15 | REMOTE_DEGRADED | lease renewal | valid | restore connection | REMOTE_ACTIVE |
| 16 | REMOTE_DEGRADED | lease expiry | invalid | teardown | TEARING_DOWN |
| 17 | REMOTE_DEGRADED | client disconnect | any | teardown | TEARING_DOWN |
| 18 | TEARING_DOWN | success | restoration verified | lock | LOCAL_LOCKED |
| 19 | TEARING_DOWN | partial failure | recovery possible | retry recovery | RECOVERING |
| 20 | RECOVERING | success | safe state verified | lock | LOCAL_LOCKED |
| 21 | RECOVERING | failure | safety uncertain | remain conservative | FAILED_SAFE |
| 22 | FAILED_SAFE | recovery success | all checks pass | lock | LOCAL_LOCKED |
| 23 | ANY | emergency | emergency trigger | revoke + invalidate + restore | EMERGENCY |
| 24 | EMERGENCY | completed | safety verified | remain locked | LOCAL_LOCKED |
| 25 | EMERGENCY | restoration failure | uncertain | conservative recovery | FAILED_SAFE |

Row 23 ("ANY") applies from all 11 states including `EMERGENCY`/`FAILED_SAFE`
themselves (idempotent re-entry, harmless per §27) and is checked *first* in
the dispatcher, before per-state handling (Doc 07 §48 pseudocode: `if
event.type == EMERGENCY: return handle_emergency()` precedes the per-state
`if` chain). Doc 12 §5's illustrative per-state list ("At minimum, test every
valid transition. Example: …") enumerates 23 of these 25 rows directly
per-source-state and is a non-exhaustive worked example, not a competing
table — no conflict; Doc 07 §8 remains the single canonical table, and Doc 12
§5's extra per-transition test dimensions (invalid input, missing
prerequisite, duplicate event, stale event/transition ID, event arriving
during another transition, failure halfway through, retry, process restart)
are folded into Step 9's test design.

**Priority resolver (Doc 07 §24, lines 1141–1197):** `1. EMERGENCY, 2.
SAFETY/FAILURE, 3. LEASE_EXPIRY, 4. DISCONNECT, 5. NORMAL STATE TRANSITION,
6. RECONNECT`. Worked example in the source: `REMOTE_ACTIVE` receiving
`reconnect + emergency` simultaneously must resolve to `EMERGENCY`, never
`REMOTE_ACTIVE`.

**StateMachineLock & transition IDs (Doc 07 §25–§26, lines 1198–1258;
architecture.md §2):** lock fields `state, active_session, active_lease,
security_epoch, transition_id`. Doc 07 §26 shows a generic `UUID` example;
the project's already-decided correlation-ID format
(architecture.md §2, "Correlation IDs") is `tr_<ULID>` — applied here, not
re-litigated. `blackroom-core` stays synchronous (`std::sync::Mutex` +
thread-based tests for the concurrency/priority tests), matching the no-I/O,
no-tokio decision above.

**Activation transaction — 22 steps (Doc 07 §9, lines 614–638; assessment
C10 "canonical"):** acquire lock → confirm `LOCAL_LOCKED` → confirm supported
GNOME env → confirm target session → confirm no conflicting remote session →
validate epoch → create session record → create lease → snapshot display →
snapshot input → prepare virtual monitor → verify virtual monitor → disable
physical outputs → verify isolation → prepare remote input → disable
physical input → verify input isolation → verify GNOME session usable →
verify PipeWire capture → verify remote input path → mark `REMOTE_ACTIVE` →
release lock, else `ROLLBACK`. Steps 1–7 and 21–22 (lock/record/verify) are
implementable now against the fake; steps 8–20 call `GnomeBackend` methods
(mocked in this phase).

**Rollback — 10 steps (Doc 07 §10, lines 657–681; assessment C10):** stop
remote input → invalidate lease → disable remote control → restore physical
input → restore physical display topology → destroy virtual monitor →
restore original monitor configuration → lock GNOME → clear transient state
→ verify safe state; success → `LOCAL_LOCKED`, else `FAILED_SAFE`.

**Teardown — 9 steps (Doc 07 §5.8):** stop accepting remote input →
invalidate lease → invalidate session authority → restore physical input →
restore physical display → destroy virtual display → restore original
monitor configuration → lock GNOME session → clear transient remote state.
Ordering may vary in a real adapter but "remote input authority must be
revoked before the system is considered safe" is a hard invariant.

**Idempotent operations (Doc 07 §27, lines 1259–1292; Doc 16 §34–§36, lines
838–957):** `revoke_remote_input`, `lock_session`, `restore_display`,
`destroy_virtual_monitor` (and by Invariant 9: `revoke_remote_authority`,
`restore_physical_input`, `invalidate_sessions`) must be safe to call
repeatedly — required because crash recovery may repeat cleanup. Doc 16 §35's
test shape (`restore_display; restore_display; restore_display` →
"safe final state", never "error because restoration was already performed")
is the acceptance shape for `lock.rs`'s wrapper. Doc 16 §36 stale-request
rejection uses transition IDs + request IDs + generation counters + security
epoch — implemented via `protocol/*.rs` types consumed by `transition.rs`.

**Startup recovery (Doc 07 §28–§30, lines 1293–1349; Doc 17 §23–§24,
§57–§59, lines 518–541, 1234–1320):** on start, never assume persisted state
is reality — reconcile `persistent state + actual GNOME state + actual
display + actual input + service state`; a persisted `REMOTE_ACTIVE` with the
actual GNOME session gone must produce recovery, not continuation ("prefer
actual safe state over stale persisted state; if reality cannot be
determined, FAIL SAFE"). The security epoch must never be reset backward on
restart (§23) and a corrupted/inconsistent epoch store must disable remote
access rather than guess (§24) — exactly the contract `epoch.rs`'s
`EpochStore` fake must demonstrate.

**ControlLease (Doc 07 §13, lines 767–801; assessment C5):** fields
`session_id, host_id, user_id, client_id, security_epoch, issued_at,
expires_at, capabilities` (Doc 07 itself says `user`; C5 already normalizes
this to `user_id` — not a new conflict). Rejected when expired, revoked,
epoch-mismatched, session-ID-invalid, or host state `!= REMOTE_ACTIVE`
(Invariant 1). Signed by the host identity key
(`ed25519-dalek` 3.0.0, BSD-3-Clause, already allow-listed in `deny.toml`,
architecture.md §4/§6) so a verifier can check a lease without a round trip;
in this phase the "host identity key" is a locally generated test keypair
(no real key management — that is Phase 15, Host Security Authority).

**Security epoch (Doc 07 §16, lines 866–901; Doc 17 §23–§24; assessment
C7):** monotonically increasing `u64`; incremented on every
authority-invalidating event (assessment C7's list); old epoch holders become
invalid via `security_epoch == current_security_epoch` (Invariant 1, 6).

**Error catalogue — `err001`/`err002` (Doc 16 §51–§53, lines 1319–1420;
assessment C17 "canonical machine catalogue"):**

```text
AUTH_INVALID, AUTH_RATE_LIMITED, AUTH_TOTP_REQUIRED,
AUTH_ACCESS_KEY_REQUIRED, AUTH_DEVICE_REVOKED,
HOST_UNAVAILABLE, HOST_UNSUPPORTED,
SESSION_NOT_FOUND, SESSION_REVOKED, SESSION_EPOCH_MISMATCH,
LEASE_EXPIRED, LEASE_REVOKED, LEASE_INVALID,
GNOME_SESSION_UNAVAILABLE, MUTTER_UNAVAILABLE, VIRTUAL_DISPLAY_FAILED,
DISPLAY_ISOLATION_FAILED, INPUT_ISOLATION_FAILED, SESSION_LOCK_FAILED,
PIPEWIRE_UNAVAILABLE, WEBRTC_FAILED, NETWORK_FAILED,
IPC_UNAUTHORIZED, IPC_INVALID_MESSAGE, IPC_TIMEOUT,
EMERGENCY_TRIGGERED, EMERGENCY_RECOVERY_FAILED,
DISPLAY_RESTORE_FAILED, INPUT_RESTORE_FAILED, RECOVERY_FAILED
```

`ErrorResponse{code, category, retryable, user_message, diagnostic_id}`
(§52); never include password/TOTP/Access Key/session secret/stack trace in
a normal error (§52). §53 retry semantics (e.g. `NETWORK_FAILED` retryable,
`AUTH_INVALID`/`SESSION_REVOKED` not, `DISPLAY_ISOLATION_FAILED` no automatic
retry) become a `retryable()` method on the error type. Only the subset
relevant to the state machine (not the full auth/session wire protocol) is
implemented now; the remainder is available for reuse, unmodified, when
Phase 12/16 build the wire protocol.

**Structured events (Doc 13 §6–§7, lines 170–244):** transition event fields
`timestamp, event, previous_state, new_state, transition_id, trigger,
component, result, failure_code`; structured-log fields additionally
`severity, session_id, client_id, security_epoch, error_code, duration_ms`.
Never log secrets (same list as §52). Emitted via the already-adopted
`tracing`/`tracing-subscriber` crates (architecture.md, in use).

**Protocol envelope (Doc 16 §32–§33, lines 838–884):** `Message{
protocol_version, message_type, request_id, timestamp, sender, session_id,
transition_id, security_epoch, payload }`; `request_id` format follows the
project's `req_<ULID>`-style convention (consistent with `rs_/tr_/cl_/bc_`,
architecture.md §2). Timeout semantics (§37, lines 958–975): a timeout is
never interpreted as success — it enters recovery/fail-safe. Partial failure
(§38, lines 976–1000): a `TIMEOUT` on any mandatory step blocks entry to
`REMOTE_ACTIVE` and forces the rollback/recovery path.

**GnomeBackend operations (Doc 05 §8, lines 172–192) cross-checked against
`docs/gnome/api-inventory.md` (Phase 1 evidence, 328 lines, all D-Bus
interfaces actually present on this host's GNOME 50.1):**

| `GnomeBackend` operation | Phase 1 D-Bus evidence (real impl candidate, Phase 3+) |
|---|---|
| `discover_session()` | `login1.Manager.ListSessions`/`ListSessionsEx` + `login1.Session` properties (`Type`, `Class`, `Active`) |
| `get_display_state()` | `Mutter.DisplayConfig.GetCurrentState` |
| `create_virtual_monitor()` | `Mutter.RemoteDesktop.CreateSession` + `Mutter.ScreenCast.CreateSession` + `DisplayConfig.ApplyMonitorsConfig` |
| `destroy_virtual_monitor()` | session sub-object `Stop` (not enumerable from top-level introspection) + `ApplyMonitorsConfig` restore |
| `disable_physical_outputs()` | `Mutter.DisplayConfig.ApplyMonitorsConfig` |
| `restore_physical_outputs()` | `Mutter.DisplayConfig.ApplyMonitorsConfig` (restore snapshot) |
| `enable_remote_input()` | `Mutter.InputCapture.CreateSession` (barrier-crossing model — Phase 0-1 finding; Gate E still `UNKNOWN`) |
| `disable_remote_input()` | InputCapture session release (sub-object) |
| `start_capture()` | ScreenCast session `Start` (sub-object) |
| `stop_capture()` | ScreenCast session `Stop` (sub-object) |
| `lock_session()` | `org.gnome.ScreenSaver.Lock` (session bus; `ScreenShield` aliases to it) or `login1.Session.Lock` (emergency fallback) |
| `get_cursor_state()` | **no matching interface found** in the Phase 1 inventory — flagged as a Phase 3 research gap, not resolved here |
| `restore_session()` | restores local display/input ownership only — **must not** call `Unlock`/`SetActive(false)`; GNOME unlock stays a manual user action (Invariant 8) |

The `get_cursor_state` gap and the `restore_session`-must-not-unlock
constraint are recorded as doc comments on the trait so Phase 3's real
adapter doesn't silently violate Invariant 8 or invent an interface that
doesn't exist.

**Invariants (Doc 07 §7, lines 448–580; assessment C9 — these are the
product-level `INV-001…014` restated for the state machine, not a competing
numbering):**

1. No valid lease, no remote input (`remote_input_enabled` only if valid
   lease + current epoch + `REMOTE_ACTIVE`).
2. Authentication ≠ control (`AUTHENTICATED != REMOTE_ACTIVE`).
3. TOTP always mandatory, even for trusted clients.
4. New/untrusted clients require username + password + TOTP + Remote Access Key.
5. Emergency always revokes remote authority regardless of network state.
6. Old security epochs are invalid after emergency (`epoch := epoch + 1`).
7. Disconnect → revoke → lock → restore display → restore input → `LOCAL_LOCKED`.
8. No automatic local unlock after remote teardown.
9. Recovery operations are idempotent (safe to call repeatedly).
10. Fail closed when uncertain — never "probably okay, leave remote control enabled".

**Scenarios A–J (Doc 07 §56, lines 2065–2112 — full acceptance list, used as
named tests in Step 9):** A. no remote input without authorization; B.
disconnect is fail-safe; C. network loss is fail-safe; D. emergency is
independent of main app health; E. emergency invalidates stale sessions; F.
physical display privacy restored after teardown; G. physical
keyboard/mouse restored after teardown; H. no automatic unlock; I. recovery
is idempotent; J. startup is safe (power loss/daemon restart cannot silently
restore stale remote control).

**Numeric defaults (`docs/security/architecture.md` §5 — reused verbatim,
none invented):** auth session TTL 5 min; session credential max lifetime
12 h; control lease TTL/renewal/heartbeat 30 s/10 s/5 s; `REMOTE_DEGRADED`
max ≤30 s then `TEARING_DOWN`; `PREPARING_REMOTE` per-step/total 10 s/60 s;
`TEARING_DOWN`/`RECOVERING` per-step/total 10 s/60 s then `FAILED_SAFE`;
emergency chord 2000 ms; auth rate limit 5/15 min per (client_id, account) +
20/min per source IP; max concurrent auth sessions 5; max message size
64 KiB (input events ≤512 B); input event rate cap 2000/s. (Doc 12/19 cycle
and soak counts are test-harness/CI parameters, not `limits.rs` runtime
constants, and are out of scope here.)

**Test-matrix cross-check (Doc 12 §5–§6, lines 123–300; §54, lines
1872–1908):** §5's illustrative per-state transition list and its extra test
dimensions (invalid input, missing prerequisite, duplicate/stale event,
event during another transition, failure halfway, retry, process restart)
are folded into Step 9. §6's 5 state-machine invariant tests are the
`INV-SEC-*`-style assertions (assessment C9) layered on top of Doc 07's
10 narrative invariants — both are implemented, not one substituted for the
other. §54's property-test event vocabulary (`CONNECT, DISCONNECT,
LEASE_EXPIRE, RECONNECT, EMERGENCY, NETWORK_LOSS, NETWORK_RESTORE,
HOSTD_RESTART, GNOME_FAILURE, DISPLAY_FAILURE, INPUT_FAILURE`) is the
generator alphabet for Step 9's `proptest`.

**New dependency decisions (Doc 00 §51 criteria: maturity, maintenance,
licence, privilege, attack surface, compatibility, failure behaviour):**
- `ulid` 3.0.0 (MIT OR Apache-2.0), `ed25519-dalek` 3.0.0 (BSD-3-Clause),
  `rand` 0.10.2 / `getrandom` 0.4.3 (MIT OR Apache-2.0), `zeroize` 1.9.0
  (MIT OR Apache-2.0) — already evaluated in architecture.md §6, moving from
  "evaluated" to "in use" here; not re-litigated.
- `proptest` — new evaluation this phase, `[dev-dependencies]` only (test
  code, not shipped, no privilege/attack-surface concern). crates.io checked
  2026-09-05: version `1.11.0`, 48 published versions since 2017-06-18,
  181M+ total downloads, updated `2026-03-24` — actively maintained,
  MIT OR Apache-2.0 (both already in `deny.toml`'s allow list). Justified
  because hand-rolling a shrinking random-sequence generator is substantial
  implementation work, unlike the `thiserror` case below.
- Rejected for this phase: `thiserror` (hand-rolled `error.rs` instead —
  Doc 00 §51 "do not add a dependency merely to avoid a small amount of
  implementation work"), `schemars` (no real IPC parsing yet), `tokio` (no
  async I/O in `blackroom-core`/`blackroom-gnome` yet).
- Step 11 records all of the above in `docs/security/architecture.md`'s
  crate inventory table (§6) so it remains the single source of truth.

**Conflict C26 (new — recorded here, filed into the assessment doc's
register by Step 11 per the plan-task skill's edit restriction during
planning):** Document 11 §7 "Phase 2 — State Machine Core" (lines 117–133)
lists only 8 "Initial states" (`LOCAL_ACTIVE, LOCAL_LOCKED,
PREPARING_REMOTE, REMOTE_ACTIVE, TEARING_DOWN, RECOVERING, EMERGENCY,
FAILED_SAFE`), omitting `AUTHENTICATING`, `AUTHENTICATED`, and
`REMOTE_DEGRADED`. Document 00 §47's Phase 2 entry is silent on the state
list (no conflict there). Document 07 §4 and the master roadmap's own Phase
2 step ("11 canonical states") already require all 11. **Resolution:**
Document 07 is canonical for the state machine specifically (assessment
§4.1, and this project's explicit instruction); Doc 11 §7's list is an
abbreviated/incomplete restatement, not an intentionally smaller Phase 2
scope. All 11 states are implemented in this phase; nothing is deferred to a
later phase on the strength of Doc 11 §7 alone.

## Risks

- `proptest`-generated sequences could be non-deterministic/flaky in CI if
  unbounded. Mitigation: fixed default case count, no dependence on wall-clock
  time, documented seed override for reproducing a failure.
- `ed25519-dalek` 3.0.0's API differs from earlier major versions.
  Mitigation: verify the exact 3.0.0 signing/verification API against
  docs.rs while implementing `lease.rs` (Step 5); not a planning blocker.
- Treating "monotonic persisted epoch"/"ControlLease" as logic-only in this
  phase (no real disk I/O; see Non-Goals) could be misread later as full
  delivery. Mitigation: the scope boundary is stated in this plan and
  repeated in `docs/protocol/state-machine.md` (Step 10).
- The `GnomeBackend` 13-operation surface is a v1 abstraction guessed from
  Doc 05 §8 and cross-checked only against read-only Phase 1 introspection;
  real Mutter/PipeWire session sub-object semantics (`ScreenCast.Start`,
  `RemoteDesktop.Start`, virtual-monitor teardown) were not independently
  enumerable and may force a trait signature change in Phase 3–4. Acceptable
  under the mock-first decision (assessment C2); flagged so the trait isn't
  mistaken for a finalized real-adapter contract. `get_cursor_state` has no
  known backing interface at all yet (see mapping table).
- Five new non-dev crates plus one dev-only crate enter the dependency
  graph. Mitigation: `cargo deny check`/`cargo audit` re-run in Step 12/13;
  architecture.md updated in the same step so it stays authoritative.
- Concurrency in this phase is exercised only at the synchronous
  `std::sync::Mutex`/thread level (no `tokio`); real async races under
  `remote-gateway`'s runtime (Phase 12+) are not exercised here. Accepted
  scope boundary for a logic-only, mock-first phase.

## Steps

- [x] 1. Scaffold `blackroom-gnome` and declare the new dependencies.
  - Files: `crates/blackroom-gnome/Cargo.toml`, `crates/blackroom-gnome/src/lib.rs`
    (empty `backend`/`fake` module stubs), `Cargo.toml` (add
    `crates/blackroom-gnome` to `[workspace] members`),
    `crates/blackroom-core/Cargo.toml` (add `serde`, `serde_json`, `time`,
    `tracing`, `ulid = "3.0.0"`, `ed25519-dalek = "3.0.0"` (features
    `rand_core`, `zeroize`), `getrandom = "0.4.3"`;
    `[dev-dependencies] proptest = "1.11.0"`; `rand` was evaluated but not
    added — `SigningKey::generate` uses `getrandom::SysRng` directly).
  - Depends on: none.
  - Verify: `cargo check --workspace --all-targets` passes with the new
    (still-empty) crate and dependencies resolved; `cargo deny check` clean.

- [x] 2. `error.rs` and `limits.rs` (foundational; no dependency on other new
  modules).
  - Files: `crates/blackroom-core/src/error.rs`, `crates/blackroom-core/src/limits.rs`,
    `crates/blackroom-core/src/lib.rs` (module declarations).
  - Depends on: step 1.
  - Verify: `cargo test -p blackroom-core` (unit tests: every `err001` code
    has a `Display` string; no variant's `Display`/`Debug` output can embed a
    secret; `limits.rs` constants match architecture.md §5 exactly, checked
    by a test that asserts the literal values).

- [x] 3. `state.rs` and `event.rs`.
  - Files: `crates/blackroom-core/src/state.rs`, `crates/blackroom-core/src/event.rs`.
  - Depends on: step 2 (events carry `ErrorCode` on failure variants).
  - Verify: `cargo test -p blackroom-core` (11 states enumerable/exhaustive
    match compiles; events use distinct variants rather than booleans for
    guard-dependent outcomes, e.g. separate rollback-possible /
    rollback-uncertain variants; `Event::priority()` returns the Doc 07 §24
    6-level order and is `Ord`).

- [x] 4. `epoch.rs` and `lease.rs`.
  - Files: `crates/blackroom-core/src/epoch.rs`, `crates/blackroom-core/src/lease.rs`.
  - Depends on: step 2, step 3.
  - Verify: `cargo test -p blackroom-core` (epoch never decreases across a
    simulated restart of the fake store; corrupted-store fake path returns
    fail-safe, not a guess; `ControlLease` sign/verify round-trips with
    `ed25519-dalek`; expired/revoked/epoch-mismatched/wrong-session leases
    are rejected — Invariant 1).

- [x] 5. `lock.rs` (`StateMachineLock` + idempotent operation wrapper).
  - Files: `crates/blackroom-core/src/lock.rs`.
  - Depends on: step 3, step 4.
  - Verify: `cargo test -p blackroom-core` (each Doc 07 §27 operation wrapped
    idempotently; calling twice yields the same observable state and does
    not double-invoke the underlying effect; a thread-based test confirms
    the lock serializes two concurrent transition attempts without a torn
    state).

- [x] 6. `transition.rs` (25-row table, dispatcher, rollback, emergency,
  fail-safe).
  - Files: `crates/blackroom-core/src/transition.rs`.
  - Depends on: steps 2–5.
  - Verify: `cargo test -p blackroom-core` (all 25 rows reachable and
    produce the documented next state; illegal (state, event) pairs are
    rejected with a typed error, not a panic; emergency is checked before
    per-state dispatch, matching Doc 07 §48's pseudocode order; rollback
    follows the Doc 07 §10 order).

- [x] 7. `protocol/*.rs` and `events/*.rs`.
  - Files: `crates/blackroom-core/src/protocol/mod.rs` (+ envelope,
    idempotency/duplicate/stale-request, timeout submodules),
    `crates/blackroom-core/src/events/mod.rs` (+ transition-event,
    structured-log-fields submodules), `crates/blackroom-core/src/lib.rs`
    (module declarations).
  - Depends on: steps 2, 3, 6 (needs `ErrorCode`, `State`/`Event`,
    `transition_id`).
  - Verify: `cargo test -p blackroom-core` (envelope round-trips through
    `serde_json`; a duplicate `request_id` is detected; a stale
    `transition_id`/lower `security_epoch` response is rejected; a
    transition emits the full Doc 13 §6/§7 field set, captured in a test via
    `tracing`'s test subscriber).

- [x] 8. `blackroom-gnome`: `backend.rs` and `fake.rs`.
  - Files: `crates/blackroom-gnome/src/backend.rs`, `crates/blackroom-gnome/src/fake.rs`.
  - Depends on: step 2 (error types), step 1 (crate scaffold).
  - Verify: `cargo test -p blackroom-gnome` (all 13 operations present on
    the trait per the Evidence mapping table, each with a doc comment citing
    its api-inventory.md candidate or the `get_cursor_state`/`restore_session`
    caveats; `FakeGnomeBackend` supports fail/timeout/partial/duplicate/
    concurrent fault injection, one test per mode).

- [x] 9. `crates/blackroom-core/tests/` full suite.
  - Files: `crates/blackroom-core/tests/transitions.rs`,
    `tests/rollback.rs`, `tests/idempotency.rs`, `tests/concurrency.rs`,
    `tests/startup_reconciliation.rs`, `tests/invariants.rs`,
    `tests/scenarios.rs`, `tests/property.rs`.
  - Depends on: steps 3–8.
  - Verify: `cargo test -p blackroom-core -p blackroom-gnome` green,
    covering all 25 transitions + representative illegal ones, the §9/§10/
    §5.8 transaction sequences, idempotency, the priority resolver,
    Doc 07 §28/Doc 17 §58 startup reconciliation, Invariants 1–10, Scenarios
    A–J as named tests, and the `proptest` property test (never reaches
    `REMOTE_ACTIVE` without the full guard set; always ends in
    `LOCAL_LOCKED`/`FAILED_SAFE` after a failure event).

- [x] 10. `docs/protocol/state-machine.md` (+ `docs/protocol/README.md`
  note).
  - Files: `docs/protocol/state-machine.md` (new), `docs/protocol/README.md`
    (amend "empty in Phase 0–1 by design" to note Phase 2 populates the
    state-machine transaction/table content; full wire protocol remains
    Phase 11–14).
  - Depends on: steps 3–9 (documents the implemented, tested behavior).
  - Verify: manual review — contains the 11 states, the 25-row table, the
    priority order, `StateMachineLock` fields, `tr_<ULID>` format, the
    22-step activation / 10-step rollback / 9-step teardown transactions
    (Doc 07 §9/§10/§5.8, assessment C10), the epoch/lease persistence
    scope boundary, and a link to `error.rs` for the `err001` catalogue; all
    intra-repo links resolve.

- [x] 11. Record C26 and update the crate inventory.
  - Files: `docs/plans/assessment-20260904-detailed-project-plan.md`
    (§5 conflict register: add C26 as specified above), `docs/security/architecture.md`
    (§6 crate table: move `ulid`/`ed25519-dalek`/`getrandom` to "in use";
    `zeroize` in use transitively via `ed25519-dalek`'s own feature flag;
    `rand` recorded as evaluated-only, not a direct dependency; add
    `proptest` as a new dev-dependency row with the 2026-09-05 crates.io
    data above).
  - Depends on: step 1 (dependencies actually added), any step (C26 is
    independent of code).
  - Verify: manual review; `cargo deny check` and `cargo audit` still clean
    after the table update.

- [x] 12. Index `Blackroom_Console` in codebase-memory.
  - Files: none (tool call only).
  - Depends on: steps 1–9 (real source must exist).
  - Verify: `mcp_codebase-memo_index_repository` on the workspace root
    succeeds; `mcp_codebase-memo_index_status` for project
    `Blackroom_Console` reports non-zero nodes/edges.

- [x] 13. Full verification sweep and handoff update.
  - Files: `docs/HANDOFF.md` (Phase 2 complete summary; Active plan stays
    this file until Phase 3 planning starts; next actions point to Phase 3
    GNOME session discovery after independent review).
  - Depends on: steps 1–12.
  - Verify: `cargo test --workspace`, `cargo fmt --check`, `cargo clippy
    --workspace --all-targets -- -D warnings`, `cargo check --workspace
    --all-targets`, `cargo deny check`, `cargo audit` all green; project
    doctor (`.github/skills/project-doctor/scripts/doctor.py`) passes;
    `docs/HANDOFF.md` stays ≤40 lines/3 KB per repo convention.

## Final Verification

- Run the configured project checks from `AGENTS.md`: `cargo test
  --workspace`, `cargo fmt --check && cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo check --workspace --all-targets`; plus `cargo deny
  check` and `cargo audit` (new dependencies).
- Confirm every Acceptance Criterion above against the actual test output
  (25 transitions, priority resolver, idempotency, epoch/lease behavior,
  error catalogue, structured events, Scenarios A–J, property test) — not
  merely that the files exist.
- Confirm Document 11 §37's 7-point per-transition Definition of Done for
  all 25 rows, and that conflict C26 is present in the assessment doc.
- Confirm `Blackroom_Console` is indexed in codebase-memory.

## Blockers

- None. Phase 0-1 is fully complete (repo memory, 2026-09-05) and this plan
  depends on nothing else pending.

## Execution Log

- 2026-09-05: Plan drafted via `/plan-task` from the master roadmap's Phase 2
  step, Document 07 (primary), cross-checked against Document 00 §47,
  Document 11 §7/§37, Document 05 §8, Document 12 §5/§6/§54, Document 13
  §6/§7, Document 16 §32–§38/§51–§53, Document 17 §3–§5/§11/§23–§24/§57–§59,
  `docs/security/architecture.md`, and `docs/gnome/api-inventory.md`. No code
  written yet; awaiting approval.
- 2026-09-05: User approved ("approved proceed"). Executed steps 1–13 in one
  session. `blackroom-core` implements all 11 states, all 25 Doc 07 §8
  transitions (verified both as inline unit tests and as an external-API
  integration suite), the §24 priority resolver, `StateMachineLock` +
  `IdempotencyGuard`, `ControlLease` signed with `ed25519-dalek` 3.0.0
  (API confirmed live via docs.rs — `SigningKey::generate` needs
  `getrandom::SysRng` + `rand_core::UnwrapErr`, not `rand::rngs::OsRng`),
  `SecurityEpoch`/`FakeEpochStore` (never decreases; corrupted → fail
  closed), the full 30-entry `err001` catalogue + `err002` shape, Doc 13
  §6/§7 structured events via `tracing`, the Doc 16 §32–§38 envelope/
  staleness/timeout primitives, and `transition::reconcile_startup_state`
  (Doc 07 §28/Doc 17 §58). `blackroom-gnome`'s `GnomeBackend` trait (13
  Doc 05 §8 operations, cross-checked against `docs/gnome/api-inventory.md`
  — found `get_cursor_state` has no known backing interface, and confirmed
  `restore_session` must never unlock per Invariant 8) and
  `FakeGnomeBackend` (fail/timeout/partial/duplicate/concurrent fault
  injection) are implemented and tested.
  `crates/blackroom-core/tests/` covers transitions, rollback, idempotency,
  concurrency/priority, startup reconciliation, all 10 invariants, Scenarios
  A–J, and a `proptest` property test plus a deterministic graph-reachability
  check that every state can reach a safe terminus. Total: 42 unit + 41
  integration tests in `blackroom-core`, 6 in `blackroom-gnome`, all green.
  `cargo test/fmt/clippy(-D warnings)/check --workspace`, `cargo deny check`,
  `cargo audit`: all green (168 deps, 0 advisories). One real dependency
  adjustment versus the plan: `rand` was evaluated but not added as a direct
  dependency (`getrandom::SysRng` suffices for `ed25519-dalek`'s
  `SigningKey::generate`), and `zeroize` ended up in use transitively via
  `ed25519-dalek`'s own `zeroize` feature flag rather than as a direct
  `blackroom-core` dependency — both corrected in `docs/security/architecture.md`
  §6 and this plan's Step 1/11 text. `docs/protocol/state-machine.md`
  written; conflict C26 filed into the assessment doc's §5 register.
  `Blackroom_Console` indexed in codebase-memory (3803 nodes, 5654 edges, no
  skipped/parse-partial files).
- 2026-09-05: Independent review (Reviewer agent) found five real gaps and
  fixed all of them: (1) `ControlLease::validate` was missing the Doc 07 §13
  session-ID check (added `current_session_id` parameter + `SESSION_NOT_FOUND`
  rejection, tested); (2) `transition::apply` never actually generated a
  `tr_<ULID>` ID or emitted `StateTransitionEvent` — added
  `transition::apply_and_record(lock, event, trigger, component)` wiring
  ID generation + `StateMachineLock` update + event emission into the real
  dispatch path, with tests; (3) `FakeGnomeBackend`'s Duplicate/Concurrent
  fault modes had no distinguishing test — added explicit call-twice
  idempotency tests for all four Doc 07 §27 named operations plus a real
  multi-thread test through a shared `Mutex<FakeGnomeBackend>`; (4) the
  `proptest` property test only modeled the auth/authorization guard chain,
  not lease/epoch validity — added a second property test tracking a
  synthetic `ControlLease`/`SecurityEpoch` through the random walk and
  asserting `ControlLease::validate` actually succeeds whenever
  `REMOTE_ACTIVE` is reached; (5) Scenario F/G were one combined test, not
  two — split into `scenario_f_...`/`scenario_g_...` (scenarios.rs now has
  10 tests, one per letter). Also corrected a stale `architecture.md` §7
  note claiming no Phase 2 crates were dependencies yet. Full re-verification
  after fixes: 100 tests green (was 89), `cargo fmt --check`/`clippy -D
  warnings`/`deny check`/`audit`/doctor all clean.
