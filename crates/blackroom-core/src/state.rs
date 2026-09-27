//! The 11 canonical remote-session states (Doc 07 §4–§5; assessment C3).
//!
//! Exactly 11 variants. Document 11 §7's abbreviated "Initial states" list
//! for Phase 2 names only 8 of these (omitting `Authenticating`,
//! `Authenticated`, `RemoteDegraded`) — recorded as conflict C26
//! (`docs/plans/plan-20260905-phase2-state-machine-core.md` Evidence And
//! Decisions); Document 07's 11 states are canonical and all are
//! implemented here.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Canonical host state (Doc 07 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum State {
    /// Normal local workstation state (Doc 07 §5.1).
    LocalActive,
    /// GNOME session locked; the safe state after disconnect and the
    /// staging state before authentication (Doc 07 §5.2).
    LocalLocked,
    /// A remote client is attempting to authenticate; no control granted
    /// (Doc 07 §5.3).
    Authenticating,
    /// Authenticated but not yet authorized/prepared — `Authenticated`
    /// never implies `RemoteActive` (Invariant 2, Doc 07 §5.4).
    Authenticated,
    /// Host is running the Doc 07 §9 activation transaction (Doc 07 §5.5).
    PreparingRemote,
    /// Remote control fully established; the only state in which remote
    /// input is permitted (Doc 07 §5.6, Invariant 1).
    RemoteActive,
    /// Transient problem while the lease remains valid; must fail closed
    /// to `TearingDown` if the lease cannot be renewed in time
    /// (Doc 07 §5.7).
    RemoteDegraded,
    /// Remote session being terminated (Doc 07 §5.8).
    TearingDown,
    /// Recovering from an error during preparation/teardown; recovery must
    /// be idempotent (Doc 07 §5.9, Invariant 9).
    Recovering,
    /// The independent emergency controller has taken over; highest
    /// priority, reachable from any state, does not depend on the
    /// network/browser/gateway (Doc 07 §5.10).
    Emergency,
    /// Restoration could not be verified; conservative — never silently
    /// falls back to `LocalActive` (Doc 07 §5.11, Invariant 10).
    FailedSafe,
}

impl State {
    /// All 11 canonical states, in Doc 07 §4 order.
    pub const ALL: [State; 11] = [
        State::LocalActive,
        State::LocalLocked,
        State::Authenticating,
        State::Authenticated,
        State::PreparingRemote,
        State::RemoteActive,
        State::RemoteDegraded,
        State::TearingDown,
        State::Recovering,
        State::Emergency,
        State::FailedSafe,
    ];

    /// The wire/log identifier used throughout the specification set.
    pub const fn as_str(self) -> &'static str {
        match self {
            State::LocalActive => "LOCAL_ACTIVE",
            State::LocalLocked => "LOCAL_LOCKED",
            State::Authenticating => "AUTHENTICATING",
            State::Authenticated => "AUTHENTICATED",
            State::PreparingRemote => "PREPARING_REMOTE",
            State::RemoteActive => "REMOTE_ACTIVE",
            State::RemoteDegraded => "REMOTE_DEGRADED",
            State::TearingDown => "TEARING_DOWN",
            State::Recovering => "RECOVERING",
            State::Emergency => "EMERGENCY",
            State::FailedSafe => "FAILED_SAFE",
        }
    }

    /// `true` for the two states in which the workstation is safe for the
    /// local user regardless of remote-authority bookkeeping (used by the
    /// property test in `tests/property.rs`, Doc 12 §54).
    pub const fn is_safe_terminus(self) -> bool {
        matches!(self, State::LocalLocked | State::FailedSafe)
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_eleven_canonical_states() {
        assert_eq!(State::ALL.len(), 11);
    }

    #[test]
    fn every_state_has_a_screaming_snake_case_name() {
        for state in State::ALL {
            assert!(
                state
                    .as_str()
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_')
            );
        }
    }

    #[test]
    fn only_local_locked_and_failed_safe_are_safe_termini() {
        for state in State::ALL {
            let expected = matches!(state, State::LocalLocked | State::FailedSafe);
            assert_eq!(state.is_safe_terminus(), expected, "state {state}");
        }
    }
}
