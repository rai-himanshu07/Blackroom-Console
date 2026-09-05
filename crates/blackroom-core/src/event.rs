//! Typed transition events (Doc 07 §8's "Event" column, one variant per
//! distinct guard-dependent outcome so illegal/ambiguous combinations are
//! unrepresentable) and the Doc 07 §24 concurrent-event priority resolver.

use std::fmt;

/// An event that may trigger a state transition (Doc 07 §8). Where the
/// transition table distinguishes two guard outcomes for what the prose
/// calls the same word (e.g. "failure" with "rollback possible" vs.
/// "rollback uncertain"), this enum uses two distinct variants rather than
/// a boolean flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Event {
    /// Row 1: `LOCAL_ACTIVE` → `LOCAL_LOCKED`.
    Lock,
    /// Row 2: `LOCAL_LOCKED` → `AUTHENTICATING`.
    Connect,
    /// Row 3: `AUTHENTICATING` → `AUTHENTICATED`.
    AuthSuccess,
    /// Row 4: `AUTHENTICATING` → `AUTHENTICATING` (retry limit not
    /// exceeded).
    AuthFailure,
    /// Row 5: `AUTHENTICATING` → `LOCAL_LOCKED` (retry limit exceeded).
    AuthRetryLimitExceeded,
    /// Row 6: `AUTHENTICATED` → `PREPARING_REMOTE`.
    AuthorizationSuccess,
    /// Row 7: `AUTHENTICATED` → `LOCAL_LOCKED` (authorization timeout).
    AuthorizationTimeout,
    /// Row 8: `PREPARING_REMOTE` → `REMOTE_ACTIVE`.
    PreparationSuccess,
    /// Row 9: `PREPARING_REMOTE` → `LOCAL_LOCKED` (rollback possible).
    PreparationFailureRecoverable,
    /// Row 10: `PREPARING_REMOTE` → `FAILED_SAFE` (rollback uncertain).
    PreparationFailureUnrecoverable,
    /// Rows 11, 15: lease renewed while valid — `REMOTE_ACTIVE` stays
    /// `REMOTE_ACTIVE`; `REMOTE_DEGRADED` recovers to `REMOTE_ACTIVE`.
    LeaseRenewed,
    /// Row 12: `REMOTE_ACTIVE` → `REMOTE_DEGRADED` (transient failure,
    /// lease remains valid).
    TransientFailure,
    /// Rows 13, 17: explicit client disconnect from `REMOTE_ACTIVE` or
    /// `REMOTE_DEGRADED` → `TEARING_DOWN`.
    Disconnect,
    /// Rows 14, 16: lease expiry from `REMOTE_ACTIVE` or
    /// `REMOTE_DEGRADED` → `TEARING_DOWN`.
    LeaseExpiry,
    /// Row 18: `TEARING_DOWN` → `LOCAL_LOCKED` (restoration verified).
    TeardownSuccess,
    /// Row 19: `TEARING_DOWN` → `RECOVERING` (partial failure, recovery
    /// possible).
    TeardownPartialFailure,
    /// Rows 20, 22: recovery succeeds — `RECOVERING` or `FAILED_SAFE` →
    /// `LOCAL_LOCKED`.
    RecoverySuccess,
    /// Row 21: `RECOVERING` → `FAILED_SAFE` (safety uncertain).
    RecoveryFailure,
    /// Row 23: emergency trigger, legal from `ANY` state → `EMERGENCY`.
    /// Checked before per-state dispatch (Doc 07 §48).
    Emergency,
    /// Row 24: `EMERGENCY` → `LOCAL_LOCKED` (safety verified).
    EmergencyCompleted,
    /// Row 25: `EMERGENCY` → `FAILED_SAFE` (restoration failure, uncertain).
    EmergencyRestorationFailure,
    /// A client attempting to reconnect. Not a row of the §8 table by
    /// itself (reconnect policy — re-authenticate vs. resume — is a later
    /// phase's decision, assessment §7.7); included so the Doc 07 §24
    /// priority-resolver example ("`REMOTE_ACTIVE` receiving `reconnect +
    /// emergency` simultaneously must resolve to `EMERGENCY`") and the
    /// Doc 12 §54 property-test event vocabulary are representable and
    /// testable now.
    Reconnect,
}

/// Doc 07 §24 concurrent-event priority tiers, most urgent first. Lower
/// ordinal = higher priority; pick the highest-priority event from a
/// concurrent batch with `Iterator::min`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Emergency,
    SafetyFailure,
    LeaseExpiry,
    Disconnect,
    Normal,
    Reconnect,
}

impl Event {
    /// The Doc 07 §24 priority tier for this event.
    pub const fn priority(self) -> Priority {
        match self {
            Event::Emergency => Priority::Emergency,

            Event::TransientFailure
            | Event::PreparationFailureRecoverable
            | Event::PreparationFailureUnrecoverable
            | Event::TeardownPartialFailure
            | Event::RecoveryFailure
            | Event::EmergencyRestorationFailure => Priority::SafetyFailure,

            Event::LeaseExpiry => Priority::LeaseExpiry,

            Event::Disconnect => Priority::Disconnect,

            Event::Lock
            | Event::Connect
            | Event::AuthSuccess
            | Event::AuthFailure
            | Event::AuthRetryLimitExceeded
            | Event::AuthorizationSuccess
            | Event::AuthorizationTimeout
            | Event::PreparationSuccess
            | Event::LeaseRenewed
            | Event::TeardownSuccess
            | Event::RecoverySuccess
            | Event::EmergencyCompleted => Priority::Normal,

            Event::Reconnect => Priority::Reconnect,
        }
    }
}

/// Resolves a batch of concurrently-arrived events to the single
/// highest-priority one (Doc 07 §24). Returns `None` for an empty batch.
/// On a priority tie, the first-encountered event of that tier wins
/// (stable), matching "serialize conflicting state transitions" (§24) —
/// ties are not expected in practice (each source produces at most one
/// event per batch) but must not panic if they occur.
pub fn resolve_priority(events: &[Event]) -> Option<Event> {
    events.iter().copied().min_by_key(|e| e.priority())
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emergency_outranks_every_other_event() {
        for event in [
            Event::Lock,
            Event::Connect,
            Event::TransientFailure,
            Event::LeaseExpiry,
            Event::Disconnect,
            Event::Reconnect,
        ] {
            assert!(Event::Emergency.priority() < event.priority());
        }
    }

    /// Doc 07 §24 worked example: `REMOTE_ACTIVE` receiving `reconnect +
    /// emergency` simultaneously must resolve to `EMERGENCY`, never stay on
    /// the reconnect/normal path.
    #[test]
    fn reconnect_plus_emergency_resolves_to_emergency() {
        let batch = [Event::Reconnect, Event::Emergency];
        assert_eq!(resolve_priority(&batch), Some(Event::Emergency));
    }

    #[test]
    fn priority_ordering_matches_doc07_section24() {
        assert!(Priority::Emergency < Priority::SafetyFailure);
        assert!(Priority::SafetyFailure < Priority::LeaseExpiry);
        assert!(Priority::LeaseExpiry < Priority::Disconnect);
        assert!(Priority::Disconnect < Priority::Normal);
        assert!(Priority::Normal < Priority::Reconnect);
    }

    #[test]
    fn resolve_priority_of_empty_batch_is_none() {
        assert_eq!(resolve_priority(&[]), None);
    }
}
