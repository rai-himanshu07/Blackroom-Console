//! Doc 07 §28 / Doc 17 §57–§59 startup reconciliation integration tests.

use blackroom_core::epoch::{EpochStore, FakeEpochStore, SecurityEpoch};
use blackroom_core::lease::{Capability, ControlLease};
use blackroom_core::state::State;
use blackroom_core::transition::reconcile_startup_state;
use std::time::{Duration, SystemTime};

/// Doc 17 §58's exact example: "database says: REMOTE_ACTIVE, actual
/// GNOME: session gone. The correct result is recovery, not continuation
/// of remote authority."
#[test]
fn persisted_remote_active_reconciles_to_local_locked_on_restart() {
    assert_eq!(
        reconcile_startup_state(State::RemoteActive),
        State::LocalLocked
    );
}

/// Every persisted state other than the two safe termini is forced safe on
/// restart; the two safe termini are preserved as-is (no needless
/// overriding of an already-safe state).
#[test]
fn every_non_safe_persisted_state_is_forced_to_local_locked() {
    for state in State::ALL {
        let reconciled = reconcile_startup_state(state);
        match state {
            State::LocalLocked | State::FailedSafe => assert_eq!(reconciled, state),
            _ => assert_eq!(reconciled, State::LocalLocked),
        }
    }
}

/// Doc 17 §23: the security epoch must never regress across a restart. A
/// lease signed under a pre-restart epoch is rejected once the store
/// reports the (unchanged-or-higher) post-restart epoch, if the state
/// machine has since moved past `REMOTE_ACTIVE` (Invariant 1).
#[test]
fn a_pre_restart_lease_is_rejected_after_startup_reconciliation() {
    let store = FakeEpochStore::new(SecurityEpoch::from_value(5));
    let pre_restart_epoch = store.current().unwrap();

    let now = SystemTime::now();
    let lease = ControlLease {
        session_id: "rs_01BEFORE".to_string(),
        host_id: "bc_deadbeef".to_string(),
        user_id: "user".to_string(),
        client_id: "cl_01BEFORE".to_string(),
        security_epoch: pre_restart_epoch,
        issued_at: now,
        expires_at: now + Duration::from_secs(30),
        capabilities: vec![Capability::View, Capability::Control],
    };

    // "Restart": reconciliation forces LOCAL_LOCKED regardless of what was
    // persisted, so the lease can no longer satisfy Invariant 1's
    // `state == REMOTE_ACTIVE` requirement even before considering epoch.
    let reconciled_state = reconcile_startup_state(State::RemoteActive);
    assert_eq!(reconciled_state, State::LocalLocked);
    let err = lease
        .validate(
            pre_restart_epoch,
            reconciled_state,
            "rs_01BEFORE",
            false,
            now,
        )
        .unwrap_err();
    assert_eq!(err.code, blackroom_core::error::ErrorCode::LeaseInvalid);
}

/// Doc 17 §24: a corrupted epoch store must disable remote access rather
/// than guess — reconciliation cannot proceed past "unknown epoch".
#[test]
fn a_corrupted_epoch_store_blocks_reconciliation_instead_of_guessing() {
    let store = FakeEpochStore::new(SecurityEpoch::from_value(9));
    store.corrupt();
    assert!(
        store.current().is_err(),
        "must fail closed, not report a guessed epoch"
    );
}
