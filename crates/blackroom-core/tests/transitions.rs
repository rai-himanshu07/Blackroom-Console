//! Integration-level re-verification of every Doc 07 §8 transition using
//! only `blackroom_core`'s public API (distinct from the inline unit tests
//! in `src/transition.rs`, which exercise private-module details).

use blackroom_core::event::Event;
use blackroom_core::state::State;
use blackroom_core::transition::apply;

/// The full Doc 07 §8 table (25 rows; row 23 "ANY -> EMERGENCY" is tested
/// separately in `emergency_row_covers_every_state`).
const NON_EMERGENCY_ROWS: [(State, Event, State); 24] = [
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
fn every_legal_row_produces_its_documented_next_state() {
    assert_eq!(NON_EMERGENCY_ROWS.len(), 24);
    for (from, event, expected_to) in NON_EMERGENCY_ROWS {
        let transition = apply(from, event).unwrap_or_else(|e| panic!("expected legal: {e}"));
        assert_eq!(transition.to, expected_to, "{from} + {event:?}");
    }
}

#[test]
fn emergency_row_covers_every_state() {
    for state in State::ALL {
        assert_eq!(apply(state, Event::Emergency).unwrap().to, State::Emergency);
    }
}

/// Any `(state, event)` pair not in the 24 non-emergency rows and not the
/// emergency row must be rejected.
#[test]
fn every_other_combination_is_illegal() {
    let legal: Vec<(State, Event)> = NON_EMERGENCY_ROWS
        .iter()
        .map(|&(from, event, _)| (from, event))
        .collect();

    const ALL_EVENTS: [Event; 21] = [
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
    ];

    let mut illegal_checked = 0;
    for state in State::ALL {
        for event in ALL_EVENTS {
            if !legal.contains(&(state, event)) {
                assert!(
                    apply(state, event).is_err(),
                    "{state} + {event:?} should be illegal"
                );
                illegal_checked += 1;
            }
        }
    }
    // 11 states * 21 non-emergency events = 231 pairs; 24 are legal.
    assert_eq!(illegal_checked, 11 * 21 - 24);
}
