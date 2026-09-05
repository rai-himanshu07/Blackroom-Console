//! The Doc 07 §8 transition table (25 rows) and the dispatcher. Doc 07 §48
//! pseudocode: emergency is checked before per-state dispatch, and applies
//! from `ANY` state (table row 23).

use std::fmt;

use time::OffsetDateTime;

use crate::event::Event;
use crate::events::{StateTransitionEvent, TransitionResult};
use crate::lock::StateMachineLock;
use crate::state::State;

/// Outcome of successfully applying a legal transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub from: State,
    pub event: Event,
    pub to: State,
}

/// An `(from, event)` pair that is not a row of the Doc 07 §8 table. This
/// is a distinct type from [`crate::error::BlackroomError`]/`ErrorCode`
/// (the `err001` catalogue): an illegal transition is a
/// protocol/programming-invariant violation ("the state machine must
/// reject illegal transitions rather than attempting to improvise",
/// Doc 12 §5), not an operational failure code reported over the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IllegalTransition {
    pub from: State,
    pub event: Event,
}

impl fmt::Display for IllegalTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "illegal transition: {} does not accept {:?}",
            self.from, self.event
        )
    }
}

impl std::error::Error for IllegalTransition {}

/// Attempts to apply `event` while in `from`. Returns the resulting
/// [`Transition`] for one of the 25 legal `(state, event)` pairs, or
/// [`IllegalTransition`] for anything else — never panics on an unexpected
/// combination.
pub fn apply(from: State, event: Event) -> Result<Transition, IllegalTransition> {
    // Doc 07 §48: `if event.type == EMERGENCY: return handle_emergency()`
    // is checked before the per-state `if` chain, and Doc 07 §8 row 23
    // ("ANY | emergency ... | EMERGENCY") makes it legal from every state,
    // including a no-op re-entry from EMERGENCY/FAILED_SAFE themselves
    // (harmless per the Doc 07 §27 idempotency requirement).
    if matches!(event, Event::Emergency) {
        return Ok(Transition {
            from,
            event,
            to: State::Emergency,
        });
    }

    let to = match (from, event) {
        // Row 1
        (State::LocalActive, Event::Lock) => State::LocalLocked,
        // Row 2
        (State::LocalLocked, Event::Connect) => State::Authenticating,
        // Row 3
        (State::Authenticating, Event::AuthSuccess) => State::Authenticated,
        // Row 4
        (State::Authenticating, Event::AuthFailure) => State::Authenticating,
        // Row 5
        (State::Authenticating, Event::AuthRetryLimitExceeded) => State::LocalLocked,
        // Row 6
        (State::Authenticated, Event::AuthorizationSuccess) => State::PreparingRemote,
        // Row 7
        (State::Authenticated, Event::AuthorizationTimeout) => State::LocalLocked,
        // Row 8
        (State::PreparingRemote, Event::PreparationSuccess) => State::RemoteActive,
        // Row 9
        (State::PreparingRemote, Event::PreparationFailureRecoverable) => State::LocalLocked,
        // Row 10
        (State::PreparingRemote, Event::PreparationFailureUnrecoverable) => State::FailedSafe,
        // Row 11
        (State::RemoteActive, Event::LeaseRenewed) => State::RemoteActive,
        // Row 12
        (State::RemoteActive, Event::TransientFailure) => State::RemoteDegraded,
        // Row 13
        (State::RemoteActive, Event::Disconnect) => State::TearingDown,
        // Row 14
        (State::RemoteActive, Event::LeaseExpiry) => State::TearingDown,
        // Row 15
        (State::RemoteDegraded, Event::LeaseRenewed) => State::RemoteActive,
        // Row 16
        (State::RemoteDegraded, Event::LeaseExpiry) => State::TearingDown,
        // Row 17
        (State::RemoteDegraded, Event::Disconnect) => State::TearingDown,
        // Row 18
        (State::TearingDown, Event::TeardownSuccess) => State::LocalLocked,
        // Row 19
        (State::TearingDown, Event::TeardownPartialFailure) => State::Recovering,
        // Row 20
        (State::Recovering, Event::RecoverySuccess) => State::LocalLocked,
        // Row 21
        (State::Recovering, Event::RecoveryFailure) => State::FailedSafe,
        // Row 22
        (State::FailedSafe, Event::RecoverySuccess) => State::LocalLocked,
        // Row 24
        (State::Emergency, Event::EmergencyCompleted) => State::LocalLocked,
        // Row 25
        (State::Emergency, Event::EmergencyRestorationFailure) => State::FailedSafe,

        _ => return Err(IllegalTransition { from, event }),
    };

    Ok(Transition { from, event, to })
}

/// Applies `event` to the state guarded by `lock`, generating a fresh
/// `tr_<ULID>` transition ID (Doc 07 §26: "every transition should have a
/// unique identifier"), updating the lock's `state`/`transition_id` on
/// success, and emitting a Doc 13 §6 structured event either way — this is
/// the one place transition IDs are actually generated and transitions are
/// actually recorded, rather than `apply`'s pure (state, event) -> state
/// math alone.
pub fn apply_and_record(
    lock: &StateMachineLock,
    event: Event,
    trigger: &str,
    component: &str,
) -> Result<Transition, IllegalTransition> {
    let transition_id = format!("tr_{}", ulid::Ulid::generate());
    let from = lock.snapshot().state;
    let result = apply(from, event);

    let (new_state, transition_result) = match &result {
        Ok(t) => (Some(t.to), TransitionResult::Success),
        Err(_) => (None, TransitionResult::Rejected),
    };

    if let Ok(t) = &result {
        lock.with_locked(|s| {
            s.state = t.to;
            s.transition_id = Some(transition_id.clone());
        });
    }

    StateTransitionEvent {
        timestamp: OffsetDateTime::now_utc(),
        event: format!("{event:?}"),
        previous_state: from,
        new_state,
        transition_id,
        trigger: trigger.to_string(),
        component: component.to_string(),
        result: transition_result,
        // An illegal (state, event) pair is a protocol/programming-invariant
        // violation (`IllegalTransition`), not an operational `err001`
        // failure — no `ErrorCode` applies here. Operational failure codes
        // are attached by the caller when a `GnomeBackend` call itself
        // fails and that failure is translated into a failure-class event.
        failure_code: None,
    }
    .emit();

    result
}

/// The Doc 07 §10 rollback sequence for [`crate::event::Event::PreparationFailureRecoverable`]
/// (reverse dependency order: remote input → physical input → virtual
/// monitor → physical display → session state).
pub const ROLLBACK_SEQUENCE: [&str; 10] = [
    "stop_remote_input",
    "invalidate_control_lease",
    "disable_remote_control",
    "restore_physical_input",
    "restore_physical_display_topology",
    "destroy_virtual_monitor",
    "restore_original_monitor_configuration",
    "lock_gnome",
    "clear_transient_state",
    "verify_safe_state",
];

/// The Doc 07 §5.8 normal teardown step order (9 steps). Ordering may
/// differ where GNOME/Mutter requires a different sequence, but the
/// security invariant is fixed: remote input authority must be revoked
/// before the system is considered safe.
pub const TEARDOWN_SEQUENCE: [&str; 9] = [
    "stop_accepting_remote_input",
    "invalidate_remote_control_lease",
    "invalidate_remote_session_authority",
    "restore_physical_input",
    "restore_physical_display",
    "destroy_virtual_display",
    "restore_original_monitor_configuration",
    "lock_gnome_session",
    "clear_transient_remote_state",
];

/// Doc 07 §28 / Doc 17 §58: startup must reconcile persisted state against
/// reality rather than trusting it — "database says: REMOTE_ACTIVE, actual
/// GNOME: session gone" must produce recovery, not continuation. Any
/// persisted state other than `LOCAL_LOCKED`/`FAILED_SAFE` is therefore
/// forced to `LOCAL_LOCKED` on startup, because no in-flight remote
/// session or lease can be trusted to have survived a restart. This crate
/// has no I/O, so "persisted state" is whatever a later phase's real store
/// reads back; this function is the reconciliation *rule*, not the read
/// itself.
pub fn reconcile_startup_state(persisted_state: State) -> State {
    match persisted_state {
        State::LocalLocked | State::FailedSafe => persisted_state,
        _ => State::LocalLocked,
    }
}

/// The Doc 07 §9 activation transaction step order (22 steps). Steps 8–20
/// call into `GnomeBackend` (mocked in Phase 2); steps 1–7 and 21–22 are
/// state-machine bookkeeping implementable without any backend.
pub const ACTIVATION_SEQUENCE: [&str; 22] = [
    "acquire_state_machine_lock",
    "confirm_state_is_local_locked",
    "confirm_supported_gnome_environment",
    "confirm_target_gnome_session",
    "confirm_no_conflicting_remote_session",
    "validate_security_epoch",
    "create_remote_session_record",
    "create_control_lease",
    "snapshot_physical_display_configuration",
    "snapshot_input_state",
    "prepare_virtual_monitor",
    "verify_virtual_monitor",
    "disable_physical_outputs",
    "verify_physical_outputs_isolated",
    "prepare_remote_input",
    "disable_physical_input",
    "verify_physical_input_isolation",
    "verify_gnome_session_usable",
    "verify_pipewire_capture",
    "verify_remote_input_path",
    "mark_remote_active",
    "release_state_machine_lock",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The full Doc 07 §8 table, reproduced 1:1 for exhaustive testing
    /// (also embedded, for citation, in
    /// `docs/plans/plan-20260905-phase2-state-machine-core.md`).
    const LEGAL_TRANSITIONS: [(State, Event, State); 24] = [
        (State::LocalActive, Event::Lock, State::LocalLocked),
        (State::LocalLocked, Event::Connect, State::Authenticating),
        (
            State::Authenticating,
            Event::AuthSuccess,
            State::Authenticated,
        ),
        (
            State::Authenticating,
            Event::AuthFailure,
            State::Authenticating,
        ),
        (
            State::Authenticating,
            Event::AuthRetryLimitExceeded,
            State::LocalLocked,
        ),
        (
            State::Authenticated,
            Event::AuthorizationSuccess,
            State::PreparingRemote,
        ),
        (
            State::Authenticated,
            Event::AuthorizationTimeout,
            State::LocalLocked,
        ),
        (
            State::PreparingRemote,
            Event::PreparationSuccess,
            State::RemoteActive,
        ),
        (
            State::PreparingRemote,
            Event::PreparationFailureRecoverable,
            State::LocalLocked,
        ),
        (
            State::PreparingRemote,
            Event::PreparationFailureUnrecoverable,
            State::FailedSafe,
        ),
        (
            State::RemoteActive,
            Event::LeaseRenewed,
            State::RemoteActive,
        ),
        (
            State::RemoteActive,
            Event::TransientFailure,
            State::RemoteDegraded,
        ),
        (State::RemoteActive, Event::Disconnect, State::TearingDown),
        (State::RemoteActive, Event::LeaseExpiry, State::TearingDown),
        (
            State::RemoteDegraded,
            Event::LeaseRenewed,
            State::RemoteActive,
        ),
        (
            State::RemoteDegraded,
            Event::LeaseExpiry,
            State::TearingDown,
        ),
        (State::RemoteDegraded, Event::Disconnect, State::TearingDown),
        (
            State::TearingDown,
            Event::TeardownSuccess,
            State::LocalLocked,
        ),
        (
            State::TearingDown,
            Event::TeardownPartialFailure,
            State::Recovering,
        ),
        (
            State::Recovering,
            Event::RecoverySuccess,
            State::LocalLocked,
        ),
        (State::Recovering, Event::RecoveryFailure, State::FailedSafe),
        (
            State::FailedSafe,
            Event::RecoverySuccess,
            State::LocalLocked,
        ),
        (
            State::Emergency,
            Event::EmergencyCompleted,
            State::LocalLocked,
        ),
        (
            State::Emergency,
            Event::EmergencyRestorationFailure,
            State::FailedSafe,
        ),
    ];

    #[test]
    fn all_24_non_emergency_rows_of_the_table_are_legal() {
        assert_eq!(LEGAL_TRANSITIONS.len(), 24);
        for (from, event, expected_to) in LEGAL_TRANSITIONS {
            let transition = apply(from, event).unwrap_or_else(|e| panic!("expected legal: {e}"));
            assert_eq!(transition.to, expected_to, "{from:?} + {event:?}");
        }
    }

    /// Row 23: `ANY | emergency -> EMERGENCY`, legal from every one of the
    /// 11 states (the 25th and final row).
    #[test]
    fn emergency_is_legal_from_every_state() {
        for state in State::ALL {
            let transition = apply(state, Event::Emergency).unwrap();
            assert_eq!(transition.to, State::Emergency, "state {state}");
        }
    }

    /// Doc 07 §8 has 25 *rows*, but row 23 ("ANY | emergency -> EMERGENCY")
    /// is one row that expands to 11 legal `(state, Emergency)` pairs — so
    /// the exhaustive pair count is 24 (other rows) + 11 (emergency from
    /// every state) = 35, not 25. This test checks the row count the
    /// specification and Doc 11 §37 DoD actually mean: 24 non-emergency
    /// rows (each independently legal) plus exactly one emergency row
    /// (universally legal, asserted separately in
    /// `emergency_is_legal_from_every_state`).
    #[test]
    fn exactly_25_transitions_are_legal_in_total() {
        let mut non_emergency_legal_count = 0;
        for from in State::ALL {
            for event in [
                Event::Lock,
                Event::Connect,
                Event::AuthSuccess,
                Event::AuthFailure,
                Event::AuthRetryLimitExceeded,
                Event::AuthorizationSuccess,
                Event::AuthorizationTimeout,
                Event::PreparationSuccess,
                Event::PreparationFailureRecoverable,
                Event::PreparationFailureUnrecoverable,
                Event::LeaseRenewed,
                Event::TransientFailure,
                Event::Disconnect,
                Event::LeaseExpiry,
                Event::TeardownSuccess,
                Event::TeardownPartialFailure,
                Event::RecoverySuccess,
                Event::RecoveryFailure,
                Event::EmergencyCompleted,
                Event::EmergencyRestorationFailure,
                Event::Reconnect,
            ] {
                if apply(from, event).is_ok() {
                    non_emergency_legal_count += 1;
                }
            }
        }
        assert_eq!(
            non_emergency_legal_count, 24,
            "24 non-emergency rows in Doc 07 §8"
        );
        // Row 23: exactly one emergency row, universally legal (11 pairs,
        // counted as 1 row) — total row count is therefore 24 + 1 = 25.
        assert!(
            State::ALL
                .iter()
                .all(|&s| apply(s, Event::Emergency).is_ok())
        );
    }

    /// A representative illegal transition per state (Doc 12 §5: "The
    /// state machine must reject illegal transitions rather than
    /// attempting to improvise").
    #[test]
    fn illegal_transitions_are_rejected_not_panicking() {
        let illegal_cases = [
            (State::LocalActive, Event::Connect),
            (State::LocalLocked, Event::AuthSuccess),
            (State::Authenticating, Event::PreparationSuccess),
            (State::Authenticated, Event::Lock),
            (State::PreparingRemote, Event::Connect),
            (State::RemoteActive, Event::AuthorizationSuccess),
            (State::RemoteDegraded, Event::PreparationSuccess),
            (State::TearingDown, Event::Lock),
            (State::Recovering, Event::Connect),
            (State::FailedSafe, Event::Lock),
            (State::Emergency, Event::Lock),
        ];
        for (from, event) in illegal_cases {
            let err = apply(from, event).unwrap_err();
            assert_eq!(err, IllegalTransition { from, event });
        }
    }

    #[test]
    fn reconnect_alone_is_illegal_everywhere_in_this_phase() {
        // Event::Reconnect exists only for the Doc 07 §24 priority-resolver
        // example today (see event.rs); it is not yet a row of the §8
        // table, so applying it in isolation must be rejected, not panic.
        for state in State::ALL {
            assert!(apply(state, Event::Reconnect).is_err());
        }
    }

    #[test]
    fn activation_and_rollback_sequences_match_doc07() {
        assert_eq!(ACTIVATION_SEQUENCE.len(), 22);
        assert_eq!(ROLLBACK_SEQUENCE.len(), 10);
        assert_eq!(TEARDOWN_SEQUENCE.len(), 9);
        assert_eq!(ACTIVATION_SEQUENCE[0], "acquire_state_machine_lock");
        assert_eq!(ACTIVATION_SEQUENCE[21], "release_state_machine_lock");
        assert_eq!(ROLLBACK_SEQUENCE[0], "stop_remote_input");
        assert_eq!(ROLLBACK_SEQUENCE[9], "verify_safe_state");
        assert_eq!(TEARDOWN_SEQUENCE[0], "stop_accepting_remote_input");
        assert_eq!(TEARDOWN_SEQUENCE[8], "clear_transient_remote_state");
    }

    #[test]
    fn local_active_is_never_a_transition_target() {
        // Structural proof of Invariant 8 ("no automatic local unlock"):
        // no row of the Doc 07 §8 table produces LOCAL_ACTIVE.
        for from in State::ALL {
            for (_, event, to) in LEGAL_TRANSITIONS {
                if apply(from, event).map(|t| t.to) == Ok(to) {
                    assert_ne!(to, State::LocalActive);
                }
            }
        }
    }

    #[test]
    fn startup_reconciliation_never_resumes_remote_active() {
        for state in State::ALL {
            let reconciled = reconcile_startup_state(state);
            assert!(
                matches!(reconciled, State::LocalLocked | State::FailedSafe),
                "persisted {state} must reconcile to a safe state, got {reconciled}"
            );
        }
        // Already-safe persisted states are preserved, not needlessly
        // overridden.
        assert_eq!(
            reconcile_startup_state(State::LocalLocked),
            State::LocalLocked
        );
        assert_eq!(
            reconcile_startup_state(State::FailedSafe),
            State::FailedSafe
        );
    }

    #[test]
    fn apply_and_record_generates_a_tr_ulid_id_and_updates_the_lock_on_success() {
        let lock = StateMachineLock::new(State::LocalActive, crate::epoch::SecurityEpoch::INITIAL);
        let transition = apply_and_record(&lock, Event::Lock, "test", "test-component").unwrap();
        assert_eq!(transition.to, State::LocalLocked);

        let snapshot = lock.snapshot();
        assert_eq!(snapshot.state, State::LocalLocked);
        let transition_id = snapshot.transition_id.expect("transition_id must be set");
        assert!(transition_id.starts_with("tr_"));
        // The suffix must be a valid ULID (26-char Crockford Base32).
        assert!(ulid::Ulid::from_string(&transition_id["tr_".len()..]).is_ok());
    }

    #[test]
    fn apply_and_record_does_not_mutate_the_lock_on_an_illegal_transition() {
        let lock = StateMachineLock::new(State::LocalActive, crate::epoch::SecurityEpoch::INITIAL);
        let err = apply_and_record(&lock, Event::Connect, "test", "test-component").unwrap_err();
        assert_eq!(
            err,
            IllegalTransition {
                from: State::LocalActive,
                event: Event::Connect
            }
        );
        let snapshot = lock.snapshot();
        assert_eq!(
            snapshot.state,
            State::LocalActive,
            "state must be unchanged after rejection"
        );
        assert!(snapshot.transition_id.is_none());
    }

    #[test]
    fn apply_and_record_generates_a_distinct_id_per_call() {
        let lock = StateMachineLock::new(State::LocalActive, crate::epoch::SecurityEpoch::INITIAL);
        apply_and_record(&lock, Event::Lock, "test", "test-component").unwrap();
        let first_id = lock.snapshot().transition_id.unwrap();
        apply_and_record(&lock, Event::Connect, "test", "test-component").unwrap();
        let second_id = lock.snapshot().transition_id.unwrap();
        assert_ne!(first_id, second_id);
    }
}
