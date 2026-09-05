//! Startup sequence: discover the session, run the Phase 3 capability
//! gate, and transition [`AgentState`] accordingly. Doc 06 §29: if GNOME is
//! not yet ready, wait/retry — never fail permanently on transient
//! unavailability. A definitive failure (non-Wayland/non-GNOME session, or
//! the capability gate not passing) is never retried and never silently
//! falls back to `SessionReady`.

use std::thread;
use std::time::Duration;

use blackroom_gnome::mutter::{capability, session};

use crate::state::AgentState;

/// Pure decision (Doc 00 §35's `UNKNOWN`-never-activates discipline,
/// applied to the agent's own state): given a discovered session's Wayland
/// flag and its capability report, decide the resulting state.
fn decide(session_is_wayland: bool, report: &capability::CapabilityReport) -> AgentState {
    if session_is_wayland && report.phase3_gate_passed() {
        AgentState::SessionReady
    } else {
        AgentState::Failed
    }
}

/// One real attempt: discover the session and run the capability gate.
fn attempt() -> AgentState {
    let session_info = match session::discover_session() {
        Ok(info) => info,
        Err(error) => {
            tracing::warn!(%error, "session discovery failed; will retry");
            return AgentState::SessionUnknown;
        }
    };
    let report = capability::detect(&session_info);
    let state = decide(session_info.is_wayland, &report);
    if matches!(state, AgentState::Failed) {
        tracing::error!(
            ?report,
            is_wayland = session_info.is_wayland,
            "non-Wayland/non-GNOME session or failed capability gate; refusing to activate"
        );
    } else {
        tracing::info!(session_id = %session_info.session_id, "capability gate passed");
    }
    state
}

fn start_with(
    max_attempts: u32,
    retry_delay: Duration,
    mut attempt_fn: impl FnMut() -> AgentState,
) -> AgentState {
    let mut state = AgentState::SessionUnknown;
    for attempt_number in 1..=max_attempts.max(1) {
        state = attempt_fn();
        match state {
            AgentState::SessionReady | AgentState::Failed => return state,
            AgentState::SessionUnknown if attempt_number < max_attempts => {
                thread::sleep(retry_delay);
            }
            _ => {}
        }
    }
    if matches!(state, AgentState::SessionUnknown) {
        tracing::error!("GNOME session never became available after retrying; failing closed");
        state = AgentState::Failed;
    }
    state
}

/// Bounded retry/backoff startup (Doc 06 §29).
pub fn start(max_attempts: u32, retry_delay: Duration) -> AgentState {
    start_with(max_attempts, retry_delay, attempt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackroom_gnome::mutter::capability::{CapabilityReport, CapabilityTier};

    fn report_with_gate(passed: bool) -> CapabilityReport {
        let tier = if passed {
            CapabilityTier::Supported
        } else {
            CapabilityTier::Unknown
        };
        CapabilityReport {
            os_supported: tier,
            gnome_supported: tier,
            wayland_supported: tier,
            systemd_supported: tier,
            session_found: tier,
            mutter_capable: CapabilityTier::Unknown,
            remote_desktop_capable: CapabilityTier::Unknown,
            screencast_capable: CapabilityTier::Unknown,
            pipewire_capable: CapabilityTier::Unknown,
            virtual_display_capable: CapabilityTier::Unknown,
            display_config_capable: CapabilityTier::Unknown,
            remote_input_capable: CapabilityTier::Unknown,
            physical_input_isolation_capable: CapabilityTier::Unknown,
            session_lock_capable: CapabilityTier::Unknown,
            emergency_capable: CapabilityTier::Unknown,
            gpu_capable: CapabilityTier::Unknown,
        }
    }

    #[test]
    fn decide_ready_when_wayland_and_gate_passed() {
        assert_eq!(
            decide(true, &report_with_gate(true)),
            AgentState::SessionReady
        );
    }

    #[test]
    fn decide_fails_closed_on_non_wayland_even_if_gate_passed() {
        assert_eq!(decide(false, &report_with_gate(true)), AgentState::Failed);
    }

    #[test]
    fn decide_fails_closed_when_gate_not_passed() {
        assert_eq!(decide(true, &report_with_gate(false)), AgentState::Failed);
    }

    #[test]
    fn start_with_retries_until_ready() {
        let mut calls = 0;
        let state = start_with(5, Duration::ZERO, || {
            calls += 1;
            if calls < 3 {
                AgentState::SessionUnknown
            } else {
                AgentState::SessionReady
            }
        });
        assert_eq!(state, AgentState::SessionReady);
        assert_eq!(calls, 3);
    }

    #[test]
    fn start_with_fails_closed_after_exhausting_retries() {
        let state = start_with(3, Duration::ZERO, || AgentState::SessionUnknown);
        assert_eq!(state, AgentState::Failed);
    }

    #[test]
    fn start_with_stops_immediately_on_definitive_failed_without_retrying() {
        let mut calls = 0;
        let state = start_with(5, Duration::ZERO, || {
            calls += 1;
            AgentState::Failed
        });
        assert_eq!(state, AgentState::Failed);
        assert_eq!(calls, 1, "a definitive Failed must not be retried");
    }
}
