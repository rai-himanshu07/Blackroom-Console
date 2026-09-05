//! Doc 12 §54 property-based testing over random event sequences.

use std::collections::{HashSet, VecDeque};

use blackroom_core::event::Event;
use blackroom_core::state::State;
use blackroom_core::transition::apply;
use proptest::prelude::*;

const ALL_NON_EMERGENCY_EVENTS: [Event; 21] = [
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

/// Doc 07 §7 Invariant 10 / the roadmap's Phase 2 acceptance line, checked
/// as a deterministic GRAPH property rather than by sampling: from every
/// one of the 11 canonical states, a safe terminus (`LOCAL_LOCKED` or
/// `FAILED_SAFE`) is reachable via some sequence of legal transitions
/// (Doc 07 §8's 25 rows). No state is a dead end that can only lead to an
/// unsafe or unknown state.
#[test]
fn every_state_can_reach_a_safe_terminus() {
    for start in State::ALL {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(start);
        queue.push_back(start);
        let mut reached_safe = matches!(start, State::LocalLocked | State::FailedSafe);

        while let Some(state) = queue.pop_front() {
            if reached_safe {
                break;
            }
            for event in ALL_NON_EMERGENCY_EVENTS
                .into_iter()
                .chain([Event::Emergency])
            {
                if let Ok(t) = apply(state, event) {
                    if matches!(t.to, State::LocalLocked | State::FailedSafe) {
                        reached_safe = true;
                        break;
                    }
                    if visited.insert(t.to) {
                        queue.push_back(t.to);
                    }
                }
            }
        }
        assert!(reached_safe, "state {start} cannot reach a safe terminus");
    }
}

fn any_event() -> impl Strategy<Value = Event> {
    prop_oneof![
        Just(Event::Lock),
        Just(Event::Connect),
        Just(Event::AuthSuccess),
        Just(Event::AuthFailure),
        Just(Event::AuthRetryLimitExceeded),
        Just(Event::AuthorizationSuccess),
        Just(Event::AuthorizationTimeout),
        Just(Event::PreparationSuccess),
        Just(Event::PreparationFailureRecoverable),
        Just(Event::PreparationFailureUnrecoverable),
        Just(Event::LeaseRenewed),
        Just(Event::TransientFailure),
        Just(Event::Disconnect),
        Just(Event::LeaseExpiry),
        Just(Event::TeardownSuccess),
        Just(Event::TeardownPartialFailure),
        Just(Event::RecoverySuccess),
        Just(Event::RecoveryFailure),
        Just(Event::Emergency),
        Just(Event::EmergencyCompleted),
        Just(Event::EmergencyRestorationFailure),
        Just(Event::Reconnect),
    ]
}

proptest! {
    /// Doc 12 §54: random event sequences must never leave `REMOTE_ACTIVE`
    /// reachable except via the full guard chain `Connect -> AuthSuccess ->
    /// AuthorizationSuccess -> PreparationSuccess` (a subsequent
    /// `LeaseRenewed` from `REMOTE_ACTIVE`/`REMOTE_DEGRADED` is the only
    /// other legal way to (re)land on `REMOTE_ACTIVE`, and it requires
    /// already being there). Any disconnect/teardown/emergency/timeout
    /// resets the accumulated guard progress, since a fresh remote session
    /// must re-earn it.
    #[test]
    fn random_event_sequences_never_reach_remote_active_without_the_guard_chain(
        events in prop::collection::vec(any_event(), 0..40)
    ) {
        let mut state = State::LocalLocked;
        let mut passed_auth_success = false;
        let mut passed_authorization_success = false;

        for event in events {
            if let Ok(t) = apply(state, event) {
                state = t.to;
                match event {
                    Event::AuthSuccess => passed_auth_success = true,
                    Event::AuthorizationSuccess => passed_authorization_success = true,
                    Event::Disconnect
                    | Event::LeaseExpiry
                    | Event::TeardownSuccess
                    | Event::Emergency
                    | Event::EmergencyCompleted
                    | Event::AuthRetryLimitExceeded
                    | Event::AuthorizationTimeout => {
                        passed_auth_success = false;
                        passed_authorization_success = false;
                    }
                    _ => {}
                }
                if state == State::RemoteActive {
                    prop_assert!(
                        passed_auth_success && passed_authorization_success,
                        "reached REMOTE_ACTIVE without the full guard chain (last event {event:?})"
                    );
                }
            }
        }
    }

    /// Every random sequence that legally applies an emergency event ends
    /// the walk in `EMERGENCY` immediately (before any further event is
    /// considered) — emergency preempts whatever was in progress.
    #[test]
    fn emergency_always_wins_immediately_when_applied(
        prefix in prop::collection::vec(any_event(), 0..10)
    ) {
        let mut state = State::LocalLocked;
        for event in prefix {
            if let Ok(t) = apply(state, event) {
                state = t.to;
            }
        }
        let after_emergency = apply(state, Event::Emergency).unwrap();
        prop_assert_eq!(after_emergency.to, State::Emergency);
    }
}
