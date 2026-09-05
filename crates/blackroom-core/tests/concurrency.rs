//! Doc 07 §24 priority resolver + `StateMachineLock` concurrency
//! integration tests, spanning `event`, `lock`, and `transition`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::event::{Event, resolve_priority};
use blackroom_core::lock::StateMachineLock;
use blackroom_core::state::State;
use blackroom_core::transition::apply;

/// Doc 07 §24's worked example, driven through the real dispatcher (not
/// just the priority comparison in isolation): `REMOTE_ACTIVE` receiving
/// `reconnect + emergency` concurrently must resolve to `EMERGENCY`.
#[test]
fn concurrent_reconnect_and_emergency_resolve_and_dispatch_to_emergency() {
    let batch = [Event::Reconnect, Event::LeaseRenewed, Event::Emergency];
    let winner = resolve_priority(&batch).expect("non-empty batch");
    assert_eq!(winner, Event::Emergency);
    let outcome = apply(State::RemoteActive, winner).unwrap();
    assert_eq!(outcome.to, State::Emergency);
}

/// A batch with no emergency/failure/lease-expiry/disconnect still applies
/// the (single) normal-tier event correctly.
#[test]
fn a_batch_of_only_normal_events_dispatches_the_normal_event() {
    let batch = [Event::LeaseRenewed];
    let winner = resolve_priority(&batch).unwrap();
    assert_eq!(winner, Event::LeaseRenewed);
    assert_eq!(
        apply(State::RemoteActive, winner).unwrap().to,
        State::RemoteActive
    );
}

/// Many threads race to mutate a shared `StateMachineLock`; the final
/// state must be exactly what the last successfully-applied legal
/// transition produced — never a torn read of a half-updated struct
/// (Doc 07 §25: "avoid scattered locks that can produce ... new remote
/// session activated while old teardown is still running").
#[test]
fn many_threads_racing_on_the_same_lock_never_observe_a_torn_state() {
    let lock = Arc::new(StateMachineLock::new(
        State::RemoteActive,
        SecurityEpoch::INITIAL,
    ));
    let barrier = Arc::new(Barrier::new(8));
    let successful_transitions = Arc::new(AtomicU32::new(0));

    let mut handles = Vec::new();
    for _ in 0..8 {
        let lock = Arc::clone(&lock);
        let barrier = Arc::clone(&barrier);
        let successful_transitions = Arc::clone(&successful_transitions);
        handles.push(thread::spawn(move || {
            barrier.wait();
            lock.with_locked(|locked| {
                if let Ok(transition) = apply(locked.state, Event::TransientFailure) {
                    locked.state = transition.to;
                    successful_transitions.fetch_add(1, Ordering::SeqCst);
                }
            });
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }

    // Only the first thread to run the lock body sees a legal
    // (REMOTE_ACTIVE, TransientFailure) pair; every later thread sees
    // REMOTE_DEGRADED and TransientFailure is illegal from there, so
    // exactly one transition succeeds and the final state reflects it.
    assert_eq!(successful_transitions.load(Ordering::SeqCst), 1);
    assert_eq!(lock.snapshot().state, State::RemoteDegraded);
}
