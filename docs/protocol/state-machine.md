# State machine protocol reference

Canonical reference for the platform-independent remote-session state
machine implemented in `crates/blackroom-core` (Phase 2,
[plan-20260905-phase2-state-machine-core.md](../plans/plan-20260905-phase2-state-machine-core.md)).
Primary source: Document 07 "Remote Session State Machine"
(`docs/plans/Detailed_Project_Plan/07 — Remote Session State Machine.md`).
This document records the *decisions and canonical shapes*; the executable
behavior and its tests live in `crates/blackroom-core/src/{state,event,
transition,lock,lease,epoch,limits,error}.rs` and
`crates/blackroom-core/tests/`.

## States (Doc 07 §4–§5; assessment C3)

Exactly 11 canonical states: `LOCAL_ACTIVE, LOCAL_LOCKED, AUTHENTICATING,
AUTHENTICATED, PREPARING_REMOTE, REMOTE_ACTIVE, REMOTE_DEGRADED,
TEARING_DOWN, RECOVERING, EMERGENCY, FAILED_SAFE`. See `state.rs` for the
one-line definition of each (mirrors Doc 07 §5.1–§5.11 verbatim).

## Transition table (Doc 07 §8) — 25 rows

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

Row 23 applies from all 11 states (including a harmless idempotent
re-entry from `EMERGENCY`/`FAILED_SAFE`) and is checked *before* per-state
dispatch (Doc 07 §48 pseudocode). `LOCAL_ACTIVE` is never a transition
*target* anywhere in this table — local unlock is always a manual GNOME
action (Invariant 8), never automatic.

Document 11 §7's abbreviated Phase 2 "Initial states" list names only 8 of
the 11 (omitting `AUTHENTICATING`, `AUTHENTICATED`, `REMOTE_DEGRADED`) —
recorded as conflict **C26** in
`docs/plans/assessment-20260904-detailed-project-plan.md` §5. Document 07's
11 states are canonical for the state machine and all are implemented.

## Priority resolver for concurrent events (Doc 07 §24)

```text
1. EMERGENCY
2. SAFETY / FAILURE
3. LEASE_EXPIRY
4. DISCONNECT
5. NORMAL STATE TRANSITION
6. RECONNECT
```

Implemented as `event::Priority` (`Ord`-derived, lower ordinal = higher
priority) and `event::resolve_priority`. Worked example: `REMOTE_ACTIVE`
receiving `reconnect + emergency` concurrently always resolves to
`EMERGENCY`.

## StateMachineLock and transition IDs (Doc 07 §25–§26)

`lock::StateMachineLock` guards exactly the fields Doc 07 §25 names:
`state, active_session, active_lease, security_epoch, transition_id`. All
mutating transitions go through `StateMachineLock::with_locked`, which
serializes access so display restoration, input isolation, and lease/
session bookkeeping never interleave.

Transition IDs use this project's `tr_<ULID>` format (architecture.md §2
correlation-ID convention) — Doc 07 §26 shows a generic `UUID` example;
the ULID format is applied instead, not re-litigated.

## Activation, rollback, and teardown transactions (Doc 07 §9, §10, §5.8; assessment C10)

**Activation (22 steps, `transition::ACTIVATION_SEQUENCE`):** acquire
state-machine lock → confirm `LOCAL_LOCKED` → confirm supported GNOME
environment → confirm target GNOME session → confirm no conflicting remote
session → validate security epoch → create remote session record → create
control lease → snapshot physical display configuration → snapshot input
state → prepare virtual monitor → verify virtual monitor → disable
physical outputs → verify physical outputs isolated → prepare remote
input → disable physical input → verify physical input isolation → verify
GNOME session usable → verify PipeWire capture → verify remote input path
→ mark `REMOTE_ACTIVE` → release lock. Steps 8–20 call into `GnomeBackend`
(mocked in Phase 2); steps 1–7 and 21–22 are pure state-machine bookkeeping.

**Rollback (10 steps, `transition::ROLLBACK_SEQUENCE`, reverse dependency
order):** stop remote input → invalidate control lease → disable remote
control → restore physical input → restore physical display topology →
destroy virtual monitor → restore original monitor configuration → lock
GNOME → clear transient state → verify safe state. Success → `LOCAL_LOCKED`;
otherwise → `FAILED_SAFE`.

**Normal teardown (9 steps, `transition::TEARDOWN_SEQUENCE`):** stop
accepting remote input → invalidate control lease → invalidate session
authority → restore physical input → restore physical display → destroy
virtual display → restore original monitor configuration → lock GNOME
session → clear transient remote state. Ordering may vary in a real
adapter, but "remote input authority must be revoked before the system is
considered safe" is a hard invariant (stop-input is always step 1).

## Idempotent operations (Doc 07 §27; Doc 16 §34–§36)

`revoke_remote_input`, `lock_session`, `restore_display`,
`destroy_virtual_monitor` (and, per Invariant 9, `revoke_remote_authority`,
`restore_physical_input`, `invalidate_sessions`) must be safe to call
repeatedly. `lock::IdempotencyGuard` deduplicates by request ID (Doc 16
§34–§35); `GnomeBackend` implementations (`blackroom-gnome`) must make the
underlying operations themselves idempotent — the in-memory
`FakeGnomeBackend` demonstrates this for all 13 operations. Stale
responses (an older transition ID / security epoch) are rejected via
`protocol::is_stale` (Doc 16 §36).

## Security epoch and control lease — scope boundary

`epoch::SecurityEpoch` and `lease::ControlLease` implement the full Doc 07
§16 / Doc 17 §23–§24 monotonic-epoch and Doc 07 §13 / assessment-C5 lease
*logic* (never decreases; a corrupted store fails closed rather than
guessing; a lease is rejected when expired, revoked, epoch-mismatched, or
the host is not `REMOTE_ACTIVE`). **Real disk-backed persistence is out of
scope for this phase** — `blackroom-core` is architecturally no-I/O
(architecture.md §1 repository layout). `epoch::EpochStore` is a trait with
an in-memory `FakeEpochStore`; a real store backed by
`/var/lib/blackroom-console/state/` lands with `blackroom-store` in
Phase 13/14 (Doc 11 §18–§19). Do not read "monotonic persisted epoch" as
implying real file I/O exists yet.

`lease::ControlLease` is signed with `ed25519-dalek` 3.0.0 (already decided,
architecture.md §4/§6). In this phase the signing key is whatever keypair a
caller generates (e.g. a test key) — real host-identity key management is
Phase 15 (Host Security Authority).

## Error catalogue

The full `err001` machine error-code catalogue (Doc 16 §51) and the
`err002` `ErrorResponse` shape are implemented in `error.rs`
(`ErrorCode`/`BlackroomError`). See that module's doc comments for the
complete list; never log a password, TOTP value, Access Key, session
secret, or stack trace through it (Doc 16 §52).

## Structured events

`events::StateTransitionEvent` (Doc 13 §6: `timestamp, event,
previous_state, new_state, transition_id, trigger, component, result,
failure_code`) and `events::StructuredLogFields` (Doc 13 §7, a superset
adding `severity, session_id, client_id, security_epoch, error_code,
duration_ms`) are emitted via `tracing` on every transition attempt.

## Message envelope (Doc 16 §32–§38)

`protocol::Envelope` / `protocol::RequestId` define the common message
shape (`protocol_version, message_type, request_id, timestamp, sender,
session_id, transition_id, security_epoch, payload`). The concrete message
catalogue (Plane A–D authentication/session/media/safety messages) is
Phase 12+ scope — this phase only needs the envelope shape plus
`protocol::is_stale` (§36) and `protocol::OperationOutcome` (§37–§38:
timeout is never success; a mandatory step timing out blocks entry to
`REMOTE_ACTIVE`).

## GnomeBackend operation mapping (Doc 05 §8, `crates/blackroom-gnome`)

Cross-checked against `docs/gnome/api-inventory.md` (Phase 1 evidence):

| `GnomeBackend` operation | Real D-Bus candidate (Phase 3+) |
|---|---|
| `discover_session` | `login1.Manager.ListSessions`/`ListSessionsEx` + `login1.Session` properties |
| `get_display_state` | `Mutter.DisplayConfig.GetCurrentState` |
| `create_virtual_monitor` | `Mutter.RemoteDesktop.CreateSession` + `Mutter.ScreenCast.CreateSession` + `DisplayConfig.ApplyMonitorsConfig` |
| `destroy_virtual_monitor` | session sub-object `Stop` + `ApplyMonitorsConfig` restore |
| `disable_physical_outputs` / `restore_physical_outputs` | `Mutter.DisplayConfig.ApplyMonitorsConfig` |
| `enable_remote_input` / `disable_remote_input` | `Mutter.InputCapture.CreateSession` (barrier-crossing model; Gate E still `UNKNOWN`) |
| `start_capture` / `stop_capture` | ScreenCast session `Start`/`Stop` (sub-object) |
| `lock_session` | `org.gnome.ScreenSaver.Lock` / `login1.Session.Lock` (emergency fallback) |
| `get_cursor_state` | **no matching interface found** — Phase 3 research gap |
| `restore_session` | restores local ownership only — **must never** call `Unlock`/`SetActive(false)` (Invariant 8) |

`blackroom-gnome::fake::FakeGnomeBackend` implements the trait in-memory
with fault injection (`fail`, `timeout`, `partial`, `duplicate`,
`concurrent`) — no real GNOME/Mutter/D-Bus call exists yet.

## Startup reconciliation (Doc 07 §28; Doc 17 §57–§59)

`transition::reconcile_startup_state` forces any persisted state other than
`LOCAL_LOCKED`/`FAILED_SAFE` to `LOCAL_LOCKED` on restart — a persisted
`REMOTE_ACTIVE` with the actual GNOME session gone must produce recovery,
not continuation ("prefer actual safe state over stale persisted state; if
reality cannot be determined, fail safe").
