//! Doc 07 §7's ten named invariants, each as a directly-named test.

use std::time::{Duration, SystemTime};

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::error::ErrorCode;
use blackroom_core::event::Event;
use blackroom_core::lease::{Capability, ControlLease};
use blackroom_core::state::State;
use blackroom_core::transition::apply;

fn sample_lease(epoch: SecurityEpoch, now: SystemTime) -> ControlLease {
    ControlLease {
        session_id: "rs_01INVARIANT".to_string(),
        host_id: "bc_deadbeef".to_string(),
        user_id: "user".to_string(),
        client_id: "cl_01INVARIANT".to_string(),
        security_epoch: epoch,
        issued_at: now,
        expires_at: now + Duration::from_secs(30),
        capabilities: vec![Capability::View, Capability::Control],
    }
}

/// Invariant 1 — no valid lease, no remote input: every one of the four
/// guard conditions independently gates validity.
#[test]
fn invariant_1_no_valid_lease_no_remote_input() {
    let now = SystemTime::now();
    let lease = sample_lease(SecurityEpoch::INITIAL, now);

    assert!(
        lease
            .validate(SecurityEpoch::INITIAL, State::RemoteActive, false, now)
            .is_ok()
    );
    assert_eq!(
        lease
            .validate(SecurityEpoch::INITIAL, State::RemoteActive, true, now)
            .unwrap_err()
            .code,
        ErrorCode::LeaseRevoked
    );
    assert_eq!(
        lease
            .validate(
                SecurityEpoch::INITIAL,
                State::RemoteActive,
                false,
                now + Duration::from_secs(31)
            )
            .unwrap_err()
            .code,
        ErrorCode::LeaseExpired
    );
    assert_eq!(
        lease
            .validate(
                SecurityEpoch::INITIAL.next(),
                State::RemoteActive,
                false,
                now
            )
            .unwrap_err()
            .code,
        ErrorCode::SessionEpochMismatch
    );
    assert_eq!(
        lease
            .validate(SecurityEpoch::INITIAL, State::RemoteDegraded, false, now)
            .unwrap_err()
            .code,
        ErrorCode::LeaseInvalid
    );
}

/// Invariant 2 — authentication does not equal control: `AUTHENTICATED` is
/// a distinct value from `REMOTE_ACTIVE`, and no legal transition maps
/// `AUTHENTICATED` directly to `REMOTE_ACTIVE` (only to `PREPARING_REMOTE`
/// or back to `LOCAL_LOCKED`).
#[test]
fn invariant_2_authenticated_does_not_equal_remote_active() {
    assert_ne!(State::Authenticated, State::RemoteActive);
    assert!(apply(State::Authenticated, Event::AuthorizationSuccess).is_ok());
    for event in [Event::PreparationSuccess, Event::LeaseRenewed] {
        assert!(
            apply(State::Authenticated, event).is_err(),
            "AUTHENTICATED must not reach REMOTE_ACTIVE directly via {event:?}"
        );
    }
}

/// Invariants 3/4 concern authentication *factors* (TOTP always mandatory;
/// Remote Access Key for untrusted clients) which are not yet modeled —
/// that is Phase 16 (Doc 11 §21). What Phase 2 *can* and does enforce is
/// the structural choke point every factor-check must pass through: there
/// is exactly one legal event, from exactly one state, that can ever
/// produce `AUTHENTICATED` — so whatever factor validation Phase 16 adds
/// gates this single edge, not a bypassable alternate path.
#[test]
fn invariants_3_and_4_structural_choke_point_for_future_factor_checks() {
    let mut producers = 0;
    for from in State::ALL {
        for event in [
            Event::AuthSuccess,
            Event::AuthFailure,
            Event::AuthRetryLimitExceeded,
        ] {
            if let Ok(t) = apply(from, event)
                && t.to == State::Authenticated
            {
                producers += 1;
                assert_eq!(from, State::Authenticating);
                assert_eq!(event, Event::AuthSuccess);
            }
        }
    }
    assert_eq!(
        producers, 1,
        "exactly one edge may ever produce AUTHENTICATED"
    );
}

/// Invariant 5 — emergency always revokes remote authority: legal from
/// every state, unconditionally (no guard parameter exists to refuse it).
#[test]
fn invariant_5_emergency_always_available() {
    for state in State::ALL {
        assert!(
            apply(state, Event::Emergency).is_ok(),
            "emergency must work from {state}"
        );
    }
}

/// Invariant 6 — old security epochs are invalid: after an epoch bump
/// (simulating the emergency-triggered increment), a lease signed under
/// the old epoch is rejected.
#[test]
fn invariant_6_old_epoch_leases_are_invalid_after_bump() {
    let now = SystemTime::now();
    let old_epoch = SecurityEpoch::from_value(41);
    let lease = sample_lease(old_epoch, now);
    let new_epoch = old_epoch.next();
    assert_eq!(new_epoch.value(), 42);
    let err = lease
        .validate(new_epoch, State::RemoteActive, false, now)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::SessionEpochMismatch);
}

/// Invariant 7 — disconnect returns to locked local console: the full walk
/// `REMOTE_ACTIVE -> disconnect -> TEARING_DOWN -> teardown success ->
/// LOCAL_LOCKED`.
#[test]
fn invariant_7_disconnect_reaches_local_locked() {
    let torn_down = apply(State::RemoteActive, Event::Disconnect).unwrap();
    assert_eq!(torn_down.to, State::TearingDown);
    let locked = apply(torn_down.to, Event::TeardownSuccess).unwrap();
    assert_eq!(locked.to, State::LocalLocked);
}

/// Invariant 8 — no automatic local unlock: `LOCAL_ACTIVE` is never the
/// target of any legal transition (structural proof).
#[test]
fn invariant_8_local_active_is_never_a_transition_target() {
    const ALL_EVENTS: [Event; 22] = [
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
        Event::Emergency,
        Event::EmergencyCompleted,
        Event::EmergencyRestorationFailure,
        Event::Reconnect,
    ];
    for from in State::ALL {
        for event in ALL_EVENTS {
            if let Ok(t) = apply(from, event) {
                assert_ne!(t.to, State::LocalActive);
            }
        }
    }
}

/// Invariant 9 — recovery is idempotent: applying the same successful
/// recovery transition repeatedly is safe (pure function, same result
/// every time — the underlying operations' idempotency is covered in
/// `tests/idempotency.rs`).
#[test]
fn invariant_9_recovery_transition_is_repeatable_without_changing_the_outcome() {
    let first = apply(State::Recovering, Event::RecoverySuccess).unwrap();
    let second = apply(State::Recovering, Event::RecoverySuccess).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.to, State::LocalLocked);
}

/// Invariant 10 — fail closed: every "uncertain" outcome in the table
/// lands on `FAILED_SAFE`, never on an active/permissive state.
#[test]
fn invariant_10_uncertain_outcomes_fail_closed() {
    assert_eq!(
        apply(
            State::PreparingRemote,
            Event::PreparationFailureUnrecoverable
        )
        .unwrap()
        .to,
        State::FailedSafe
    );
    assert_eq!(
        apply(State::Recovering, Event::RecoveryFailure).unwrap().to,
        State::FailedSafe
    );
    assert_eq!(
        apply(State::Emergency, Event::EmergencyRestorationFailure)
            .unwrap()
            .to,
        State::FailedSafe
    );
}
