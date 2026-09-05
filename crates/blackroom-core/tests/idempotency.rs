//! Doc 07 §27 idempotent-operation and Doc 16 §34–§36 duplicate/stale
//! request integration tests, spanning `lock`, `epoch`, and `protocol`.

use blackroom_core::epoch::{EpochStore, FakeEpochStore, SecurityEpoch};
use blackroom_core::lock::IdempotencyGuard;
use blackroom_core::protocol::is_stale;

/// Doc 16 §35's exact test shape: three identical requests to an
/// idempotent operation must produce one applied effect and two safe
/// no-ops — never an error on the repeats.
#[test]
fn three_identical_restore_display_requests_apply_once() {
    let guard = IdempotencyGuard::new();
    let mut applied = 0;
    for _ in 0..3 {
        if guard.should_apply("req_restore_display_001") {
            applied += 1;
        }
    }
    assert_eq!(
        applied, 1,
        "restoration must be applied exactly once, never re-applied or errored"
    );
}

/// Doc 07 §16 / Invariant 9 extended to the epoch itself: advancing to the
/// *same* epoch value twice is safe (not a decrease, so not rejected),
/// matching "recovery must be idempotent — calling it twice must not make
/// the state worse".
#[test]
fn advancing_the_epoch_to_the_same_value_twice_is_safe() {
    let store = FakeEpochStore::new(SecurityEpoch::from_value(3));
    store.advance_to(SecurityEpoch::from_value(4)).unwrap();
    store.advance_to(SecurityEpoch::from_value(4)).unwrap();
    assert_eq!(store.current().unwrap().value(), 4);
}

/// Doc 16 §36: a message referencing an epoch older than the current one
/// must not be allowed to overwrite state — different request IDs for the
/// same stale generation must all be rejected identically.
#[test]
fn every_request_from_a_superseded_epoch_is_stale() {
    let current = SecurityEpoch::from_value(10);
    for message_epoch in [0, 5, 9] {
        assert!(is_stale(current, Some(message_epoch)));
    }
    for message_epoch in [10, 11, 100] {
        assert!(!is_stale(current, Some(message_epoch)));
    }
}
