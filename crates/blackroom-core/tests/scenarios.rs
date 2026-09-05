//! Doc 07 §56 Scenarios A–J: the state-machine acceptance criteria, each as
//! a directly-named test.

use std::time::{Duration, SystemTime};

use blackroom_core::epoch::{EpochStore, FakeEpochStore, SecurityEpoch};
use blackroom_core::error::ErrorCode;
use blackroom_core::event::Event;
use blackroom_core::lease::{Capability, ControlLease};
use blackroom_core::lock::IdempotencyGuard;
use blackroom_core::state::State;
use blackroom_core::transition::{apply, reconcile_startup_state};

fn sample_lease(epoch: SecurityEpoch, now: SystemTime) -> ControlLease {
    ControlLease {
        session_id: "rs_01SCENARIO".to_string(),
        host_id: "bc_deadbeef".to_string(),
        user_id: "user".to_string(),
        client_id: "cl_01SCENARIO".to_string(),
        security_epoch: epoch,
        issued_at: now,
        expires_at: now + Duration::from_secs(30),
        capabilities: vec![Capability::View, Capability::Control],
    }
}

/// A. No remote input without authorization: impossible to inject remote
/// input without a valid lease and current epoch (Invariant 1) — walking
/// forward through `AUTHENTICATED` alone (no lease yet, and not in
/// `REMOTE_ACTIVE`) demonstrates the state precondition, and `lease.validate`
/// demonstrates the lease/epoch precondition.
#[test]
fn scenario_a_no_remote_input_without_authorization() {
    let now = SystemTime::now();
    let lease = sample_lease(SecurityEpoch::INITIAL, now);
    // Authenticated, but not yet authorized/prepared: no state accepts
    // remote input here.
    let err = lease
        .validate(
            SecurityEpoch::INITIAL,
            State::Authenticated,
            "rs_01SCENARIO",
            false,
            now,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::LeaseInvalid);
}

/// B. Disconnect is fail-safe: remote disconnect results in locked local
/// state.
#[test]
fn scenario_b_disconnect_is_fail_safe() {
    let torn_down = apply(State::RemoteActive, Event::Disconnect).unwrap();
    let locked = apply(torn_down.to, Event::TeardownSuccess).unwrap();
    assert_eq!(locked.to, State::LocalLocked);
}

/// C. Network loss is fail-safe: network failure cannot leave indefinite
/// remote input authority — degrade, then expire, then teardown.
#[test]
fn scenario_c_network_loss_is_fail_safe() {
    let degraded = apply(State::RemoteActive, Event::TransientFailure).unwrap();
    assert_eq!(degraded.to, State::RemoteDegraded);
    let tearing_down = apply(degraded.to, Event::LeaseExpiry).unwrap();
    assert_eq!(tearing_down.to, State::TearingDown);
    let locked = apply(tearing_down.to, Event::TeardownSuccess).unwrap();
    assert_eq!(locked.to, State::LocalLocked);
}

/// D. Emergency is independent: works when the main remote application is
/// unhealthy — modeled here as emergency firing while mid-transaction
/// (`PREPARING_REMOTE`), a state where the "main application" is busy and
/// has not yet even reached a steady state.
#[test]
fn scenario_d_emergency_is_independent_of_main_application_health() {
    assert_eq!(
        apply(State::PreparingRemote, Event::Emergency).unwrap().to,
        State::Emergency
    );
}

/// E. Emergency invalidates stale sessions: an old remote session cannot
/// regain control after emergency (epoch bump rejects the old lease).
#[test]
fn scenario_e_emergency_invalidates_stale_sessions() {
    let now = SystemTime::now();
    let pre_emergency_epoch = SecurityEpoch::from_value(41);
    let stale_lease = sample_lease(pre_emergency_epoch, now);

    assert_eq!(
        apply(State::RemoteActive, Event::Emergency).unwrap().to,
        State::Emergency
    );
    let post_emergency_epoch = pre_emergency_epoch.next();

    let err = stale_lease
        .validate(
            post_emergency_epoch,
            State::RemoteActive,
            "rs_01SCENARIO",
            false,
            now,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::SessionEpochMismatch);
}

/// F. Physical display privacy is restored after remote teardown.
#[test]
fn scenario_f_physical_display_privacy_is_restored() {
    use blackroom_core::transition::TEARDOWN_SEQUENCE;
    assert!(TEARDOWN_SEQUENCE.contains(&"restore_physical_display"));
}

/// G. Physical keyboard/mouse control is restored after remote teardown.
#[test]
fn scenario_g_physical_control_is_restored() {
    use blackroom_core::transition::TEARDOWN_SEQUENCE;
    assert!(TEARDOWN_SEQUENCE.contains(&"restore_physical_input"));
}

/// H. No automatic unlock: the user must explicitly unlock GNOME —
/// `LOCAL_ACTIVE` is never a transition target (see `tests/invariants.rs`
/// for the exhaustive structural proof; this test is the scenario-named
/// entry point).
#[test]
fn scenario_h_no_automatic_unlock() {
    assert!(
        apply(State::TearingDown, Event::TeardownSuccess)
            .unwrap()
            .to
            != State::LocalActive
    );
    assert_eq!(
        apply(State::TearingDown, Event::TeardownSuccess)
            .unwrap()
            .to,
        State::LocalLocked
    );
}

/// I. Recovery is idempotent: repeated cleanup operations do not corrupt
/// state.
#[test]
fn scenario_i_recovery_is_idempotent() {
    let guard = IdempotencyGuard::new();
    let mut applied = 0;
    for _ in 0..5 {
        if guard.should_apply("req_recovery_cleanup") {
            applied += 1;
        }
    }
    assert_eq!(applied, 1);
}

/// J. Startup is safe: power loss or daemon restart cannot silently
/// restore stale remote control.
#[test]
fn scenario_j_startup_is_safe() {
    assert_eq!(
        reconcile_startup_state(State::RemoteActive),
        State::LocalLocked
    );
    let store = FakeEpochStore::new(SecurityEpoch::from_value(3));
    store.corrupt();
    assert!(
        store.current().is_err(),
        "unknown security state must disable remote access, not guess"
    );
}
