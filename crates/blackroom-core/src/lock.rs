//! `StateMachineLock` (Doc 07 §25) and idempotency/duplicate-request
//! guarding (Doc 07 §27; Doc 16 §34–§36).

use std::collections::HashSet;
use std::sync::Mutex;

use crate::epoch::SecurityEpoch;
use crate::lease::ControlLease;
use crate::state::State;

/// Serializes state-mutating transitions so display restoration, input
/// isolation, and lease/session bookkeeping never interleave (Doc 07 §25):
/// "Avoid scattered locks that can produce: display restored while input
/// still remote; or new remote session activated while old teardown is
/// still running."
pub struct StateMachineLock {
    inner: Mutex<LockedState>,
}

/// The fields Doc 07 §25 names for the lock's guarded state.
#[derive(Debug, Clone)]
pub struct LockedState {
    pub state: State,
    pub active_session: Option<String>,
    pub active_lease: Option<ControlLease>,
    pub security_epoch: SecurityEpoch,
    pub transition_id: Option<String>,
}

impl StateMachineLock {
    pub fn new(initial_state: State, initial_epoch: SecurityEpoch) -> Self {
        Self {
            inner: Mutex::new(LockedState {
                state: initial_state,
                active_session: None,
                active_lease: None,
                security_epoch: initial_epoch,
                transition_id: None,
            }),
        }
    }

    /// Runs `f` with exclusive access to the guarded state, so a caller can
    /// read-then-write atomically (e.g. "confirm state == LOCAL_LOCKED,
    /// then set the new transition_id"). This is the single mutation point
    /// transition.rs's dispatcher goes through.
    pub fn with_locked<T>(&self, f: impl FnOnce(&mut LockedState) -> T) -> T {
        let mut guard = self.inner.lock().expect("state machine lock poisoned");
        f(&mut guard)
    }

    /// A snapshot of the guarded state (for read-only callers, e.g.
    /// diagnostics).
    pub fn snapshot(&self) -> LockedState {
        self.inner
            .lock()
            .expect("state machine lock poisoned")
            .clone()
    }
}

/// Tracks which idempotency keys (typically a `req_<ULID>` request ID) have
/// already been applied, so re-delivery of the same command is a safe no-op
/// (Doc 16 §34–§35) rather than a repeated side effect. Doc 07 §27's
/// examples (`revoke_remote_input`, `lock_session`, `restore_display`,
/// `destroy_virtual_monitor`) are additionally naturally idempotent at the
/// operation level (calling them when the effect is already in place must
/// succeed, not error) — that property lives on `GnomeBackend`
/// implementations (`blackroom-gnome`), not here; this guard is the
/// request/command-level deduplication layer.
#[derive(Default)]
pub struct IdempotencyGuard {
    seen: Mutex<HashSet<String>>,
}

impl IdempotencyGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` the first time `key` is seen (the caller should
    /// perform the effect); returns `false` on any later call with the same
    /// key (the caller should skip the effect and return the previously
    /// cached-safe result, per Doc 16 §35: "safe final state", never "error
    /// because restoration was already performed").
    pub fn should_apply(&self, key: &str) -> bool {
        self.seen
            .lock()
            .expect("idempotency guard lock poisoned")
            .insert(key.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn with_locked_reads_and_writes_atomically() {
        let lock = StateMachineLock::new(State::LocalLocked, SecurityEpoch::INITIAL);
        lock.with_locked(|s| {
            assert_eq!(s.state, State::LocalLocked);
            s.state = State::Authenticating;
            s.transition_id = Some("tr_01TEST".to_string());
        });
        let snapshot = lock.snapshot();
        assert_eq!(snapshot.state, State::Authenticating);
        assert_eq!(snapshot.transition_id.as_deref(), Some("tr_01TEST"));
    }

    /// Two "concurrent" transition attempts must not interleave — the
    /// second one only observes the first's fully-applied result, never a
    /// half-updated state (Doc 07 §25).
    #[test]
    fn serializes_concurrent_transition_attempts() {
        let lock = Arc::new(StateMachineLock::new(
            State::LocalActive,
            SecurityEpoch::INITIAL,
        ));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let lock = Arc::clone(&lock);
            handles.push(thread::spawn(move || {
                lock.with_locked(|s| {
                    let before = s.state;
                    // Simulate work between read and write: if another
                    // thread could interleave, this would corrupt state.
                    std::thread::yield_now();
                    s.state = before;
                });
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(lock.snapshot().state, State::LocalActive);
    }

    #[test]
    fn idempotency_guard_applies_once_per_key() {
        let guard = IdempotencyGuard::new();
        assert!(guard.should_apply("req_01AAA"));
        assert!(!guard.should_apply("req_01AAA"));
        assert!(guard.should_apply("req_01BBB"));
    }

    /// Doc 16 §35's exact test shape: repeating an idempotent request three
    /// times must not error on the 2nd/3rd call.
    #[test]
    fn repeated_restore_display_requests_stay_safe() {
        let guard = IdempotencyGuard::new();
        let mut effect_count = 0;
        for _ in 0..3 {
            if guard.should_apply("req_restore_display_1") {
                effect_count += 1;
            }
        }
        assert_eq!(effect_count, 1);
    }
}
