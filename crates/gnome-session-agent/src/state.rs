//! `gnome-session-agent`'s own local-subsystem readiness state (Doc 05 §20,
//! 11 values). `AgentState` is `gnome-session-agent`'s own local-subsystem
//! readiness state; `blackroom_core::state::State` remains the sole
//! cross-host authority (owned by `remote-hostd`, Phase 11+), and
//! `AgentState` never substitutes for it. Doc 05 §21's informal global-state
//! sketch is already covered by conflict C3
//! (`docs/plans/assessment-20260904-detailed-project-plan.md` §5).

use std::fmt;

/// The agent's own local-subsystem readiness (Doc 05 §20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentState {
    /// Startup has not yet determined anything (Doc 05 §20).
    SessionUnknown,
    /// The session was discovered and the Phase 3 capability gate passed.
    SessionReady,
    /// Running the Doc 07 §9-style activation transaction (Phase 4+).
    Preparing,
    VirtualDisplayReady,
    PhysicalDisplayIsolated,
    PhysicalInputIsolated,
    RemoteReady,
    RemoteActive,
    Restoring,
    Restored,
    /// Startup, the capability gate, or a later transaction failed; never
    /// a silent fallback to `SessionReady` (Doc 00 §35 `UNKNOWN`-never-
    /// activates discipline applied to the agent's own state).
    Failed,
}

impl AgentState {
    /// All 11 states, in Doc 05 §20 order.
    pub const ALL: [AgentState; 11] = [
        AgentState::SessionUnknown,
        AgentState::SessionReady,
        AgentState::Preparing,
        AgentState::VirtualDisplayReady,
        AgentState::PhysicalDisplayIsolated,
        AgentState::PhysicalInputIsolated,
        AgentState::RemoteReady,
        AgentState::RemoteActive,
        AgentState::Restoring,
        AgentState::Restored,
        AgentState::Failed,
    ];

    /// The wire/log identifier (Doc 05 §20's own `SCREAMING_SNAKE_CASE`
    /// spelling).
    pub const fn as_str(self) -> &'static str {
        match self {
            AgentState::SessionUnknown => "SESSION_UNKNOWN",
            AgentState::SessionReady => "SESSION_READY",
            AgentState::Preparing => "PREPARING",
            AgentState::VirtualDisplayReady => "VIRTUAL_DISPLAY_READY",
            AgentState::PhysicalDisplayIsolated => "PHYSICAL_DISPLAY_ISOLATED",
            AgentState::PhysicalInputIsolated => "PHYSICAL_INPUT_ISOLATED",
            AgentState::RemoteReady => "REMOTE_READY",
            AgentState::RemoteActive => "REMOTE_ACTIVE",
            AgentState::Restoring => "RESTORING",
            AgentState::Restored => "RESTORED",
            AgentState::Failed => "FAILED",
        }
    }
}

impl fmt::Display for AgentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_eleven_states() {
        assert_eq!(AgentState::ALL.len(), 11);
    }

    #[test]
    fn every_state_has_a_screaming_snake_case_name() {
        for state in AgentState::ALL {
            assert!(
                state
                    .as_str()
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_')
            );
        }
    }

    #[test]
    fn all_names_are_unique() {
        let mut names: Vec<&str> = AgentState::ALL.iter().map(|s| s.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), AgentState::ALL.len());
    }
}
