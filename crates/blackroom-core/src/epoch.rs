//! Monotonic security epoch (Doc 07 §16; Doc 17 §23–§24; assessment C7)
//! and its store abstraction.
//!
//! Real disk-backed persistence is Phase 13 (Doc 11 §18); this crate stays
//! no-I/O (architecture.md §1). `FakeEpochStore` is an in-memory stand-in
//! that demonstrates the full Doc 17 §23–§24 contract for tests: the epoch
//! never decreases, and a corrupted/inconsistent store fails closed instead
//! of guessing.

use std::sync::Mutex;

/// Monotonically increasing security epoch (Doc 07 §16). Incremented on
/// every authority-invalidating event (assessment C7's list: emergency,
/// revoke-all, remote-access disable, security reset, host recovery,
/// unclean-shutdown restart, TOTP reset, password change, Access Key
/// rotation, trusted-device revocation, uninstall/upgrade teardown). A
/// lease/session bound to an older epoch is invalid (Invariant 1, 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SecurityEpoch(u64);

impl SecurityEpoch {
    /// The initial epoch for a freshly provisioned host.
    pub const INITIAL: SecurityEpoch = SecurityEpoch(0);

    /// Wraps an already-known epoch value (e.g. loaded from a real store in
    /// a later phase).
    pub const fn from_value(value: u64) -> Self {
        SecurityEpoch(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    /// The next epoch after an authority-invalidating event (Doc 07 §16:
    /// `security_epoch := security_epoch + 1`).
    #[must_use]
    pub const fn next(self) -> SecurityEpoch {
        SecurityEpoch(self.0 + 1)
    }
}

/// Reason an [`EpochStore`] refuses to report/accept a value (Doc 17 §24:
/// prefer "REMOTE ACCESS DISABLED" over guessing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpochStoreError {
    /// The persisted epoch could not be read/parsed/trusted.
    Corrupted,
    /// A write attempted to move the epoch backward (Doc 17 §23: "never
    /// reset the epoch to an old value merely because the daemon
    /// restarted").
    WouldDecrease {
        current: SecurityEpoch,
        attempted: SecurityEpoch,
    },
}

/// Abstraction over epoch persistence (Doc 17 §23–§24). A real
/// disk-backed implementation is Phase 13/14 work, not this crate.
pub trait EpochStore {
    fn current(&self) -> Result<SecurityEpoch, EpochStoreError>;

    /// Advances the stored epoch to `new_epoch`. Rejects any value less
    /// than the current one.
    fn advance_to(&self, new_epoch: SecurityEpoch) -> Result<(), EpochStoreError>;
}

struct FakeEpochState {
    epoch: SecurityEpoch,
    corrupted: bool,
}

/// In-memory [`EpochStore`] fake for tests. `corrupt()` simulates the
/// Doc 17 §24 corrupted/unreadable/inconsistent case.
pub struct FakeEpochStore {
    state: Mutex<FakeEpochState>,
}

impl FakeEpochStore {
    pub fn new(initial: SecurityEpoch) -> Self {
        Self {
            state: Mutex::new(FakeEpochState {
                epoch: initial,
                corrupted: false,
            }),
        }
    }

    /// Simulates the store becoming corrupted/unreadable/inconsistent.
    /// Subsequent `current()`/`advance_to()` calls fail closed.
    pub fn corrupt(&self) {
        self.state
            .lock()
            .expect("fake epoch store lock poisoned")
            .corrupted = true;
    }
}

impl Default for FakeEpochStore {
    fn default() -> Self {
        Self::new(SecurityEpoch::INITIAL)
    }
}

impl EpochStore for FakeEpochStore {
    fn current(&self) -> Result<SecurityEpoch, EpochStoreError> {
        let state = self.state.lock().expect("fake epoch store lock poisoned");
        if state.corrupted {
            return Err(EpochStoreError::Corrupted);
        }
        Ok(state.epoch)
    }

    fn advance_to(&self, new_epoch: SecurityEpoch) -> Result<(), EpochStoreError> {
        let mut state = self.state.lock().expect("fake epoch store lock poisoned");
        if state.corrupted {
            return Err(EpochStoreError::Corrupted);
        }
        if new_epoch < state.epoch {
            return Err(EpochStoreError::WouldDecrease {
                current: state.epoch,
                attempted: new_epoch,
            });
        }
        state.epoch = new_epoch;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_increments_by_one() {
        assert_eq!(SecurityEpoch::INITIAL.next().value(), 1);
        assert_eq!(SecurityEpoch::from_value(41).next().value(), 42);
    }

    #[test]
    fn store_survives_a_simulated_restart_at_or_above_prior_value() {
        let store = FakeEpochStore::new(SecurityEpoch::INITIAL);
        store.advance_to(SecurityEpoch::from_value(5)).unwrap();
        // "Restart": re-reading the same durable handle must never see a
        // value below what was last durably advanced to (Doc 17 §23).
        assert_eq!(store.current().unwrap().value(), 5);
    }

    #[test]
    fn advance_to_rejects_moving_backward() {
        let store = FakeEpochStore::new(SecurityEpoch::from_value(10));
        let err = store.advance_to(SecurityEpoch::from_value(3)).unwrap_err();
        assert_eq!(
            err,
            EpochStoreError::WouldDecrease {
                current: SecurityEpoch::from_value(10),
                attempted: SecurityEpoch::from_value(3),
            }
        );
        assert_eq!(store.current().unwrap().value(), 10);
    }

    #[test]
    fn corrupted_store_fails_closed_instead_of_guessing() {
        let store = FakeEpochStore::new(SecurityEpoch::from_value(7));
        store.corrupt();
        assert_eq!(store.current().unwrap_err(), EpochStoreError::Corrupted);
        assert_eq!(
            store.advance_to(SecurityEpoch::from_value(8)).unwrap_err(),
            EpochStoreError::Corrupted
        );
    }
}
