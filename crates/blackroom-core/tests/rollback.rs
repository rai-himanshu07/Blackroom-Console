//! Doc 07 §9 activation / §10 rollback / §5.8 teardown sequence checks,
//! and the state outcomes they correspond to.

use blackroom_core::event::Event;
use blackroom_core::state::State;
use blackroom_core::transition::{
    ACTIVATION_SEQUENCE, ROLLBACK_SEQUENCE, TEARDOWN_SEQUENCE, apply,
};

#[test]
fn activation_sequence_ends_with_marking_remote_active_then_releasing_the_lock() {
    assert_eq!(ACTIVATION_SEQUENCE[20], "mark_remote_active");
    assert_eq!(ACTIVATION_SEQUENCE[21], "release_state_machine_lock");
}

/// Doc 07 §10: rollback undoes activation in reverse dependency order —
/// remote input first, physical display topology last before verification.
#[test]
fn rollback_order_is_remote_input_first_display_topology_last() {
    let remote_input_index = ROLLBACK_SEQUENCE
        .iter()
        .position(|&s| s == "stop_remote_input")
        .unwrap();
    let lease_index = ROLLBACK_SEQUENCE
        .iter()
        .position(|&s| s == "invalidate_control_lease")
        .unwrap();
    let display_index = ROLLBACK_SEQUENCE
        .iter()
        .position(|&s| s == "restore_physical_display_topology")
        .unwrap();
    let monitor_index = ROLLBACK_SEQUENCE
        .iter()
        .position(|&s| s == "destroy_virtual_monitor")
        .unwrap();
    let lock_index = ROLLBACK_SEQUENCE
        .iter()
        .position(|&s| s == "lock_gnome")
        .unwrap();
    let verify_index = ROLLBACK_SEQUENCE
        .iter()
        .position(|&s| s == "verify_safe_state")
        .unwrap();

    assert!(remote_input_index < lease_index);
    assert!(lease_index < display_index);
    assert!(display_index < monitor_index);
    assert!(monitor_index < lock_index);
    assert!(
        lock_index < verify_index,
        "verification must be the final step"
    );
}

/// A `PreparationFailureRecoverable` event (rollback possible, Doc 07 §8
/// row 9) lands the state machine at `LOCAL_LOCKED`, matching the Doc 07
/// §10 rollback's success outcome.
#[test]
fn recoverable_preparation_failure_reaches_local_locked() {
    let outcome = apply(State::PreparingRemote, Event::PreparationFailureRecoverable).unwrap();
    assert_eq!(outcome.to, State::LocalLocked);
}

/// An unrecoverable failure (Doc 07 §8 row 10) lands at `FAILED_SAFE`,
/// matching Doc 07 §10's "otherwise: FAILED_SAFE".
#[test]
fn unrecoverable_preparation_failure_reaches_failed_safe() {
    let outcome = apply(
        State::PreparingRemote,
        Event::PreparationFailureUnrecoverable,
    )
    .unwrap();
    assert_eq!(outcome.to, State::FailedSafe);
}

/// Doc 07 §5.8: "remote input authority must be revoked before the system
/// is considered safe" — the teardown sequence stops remote input first
/// and locks the session only near the end.
#[test]
fn teardown_sequence_revokes_input_authority_before_locking() {
    let stop_input_index = TEARDOWN_SEQUENCE
        .iter()
        .position(|&s| s == "stop_accepting_remote_input")
        .unwrap();
    let lock_index = TEARDOWN_SEQUENCE
        .iter()
        .position(|&s| s == "lock_gnome_session")
        .unwrap();
    assert!(stop_input_index < lock_index);
    assert_eq!(
        stop_input_index, 0,
        "revoking input authority must be the very first teardown step"
    );
}

/// Full happy-path walk: `REMOTE_ACTIVE` disconnects, tears down
/// successfully, and lands `LOCAL_LOCKED` (Doc 07 §11 / Invariant 7).
#[test]
fn full_teardown_walk_reaches_local_locked() {
    let after_disconnect = apply(State::RemoteActive, Event::Disconnect).unwrap();
    assert_eq!(after_disconnect.to, State::TearingDown);
    let after_teardown = apply(after_disconnect.to, Event::TeardownSuccess).unwrap();
    assert_eq!(after_teardown.to, State::LocalLocked);
}

/// A partial teardown failure routes through `RECOVERING` rather than
/// jumping straight to a terminus, and recovery itself resolves to one of
/// the two safe states.
#[test]
fn partial_teardown_failure_routes_through_recovering() {
    let after_disconnect = apply(State::RemoteActive, Event::Disconnect).unwrap();
    let after_partial_failure = apply(after_disconnect.to, Event::TeardownPartialFailure).unwrap();
    assert_eq!(after_partial_failure.to, State::Recovering);

    let recovered = apply(State::Recovering, Event::RecoverySuccess).unwrap();
    assert_eq!(recovered.to, State::LocalLocked);
    let recovery_failed = apply(State::Recovering, Event::RecoveryFailure).unwrap();
    assert_eq!(recovery_failed.to, State::FailedSafe);
}
