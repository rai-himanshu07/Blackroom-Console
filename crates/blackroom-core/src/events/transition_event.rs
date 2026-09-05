//! Structured state-transition event (Doc 13 §6).

use time::OffsetDateTime;

use crate::error::ErrorCode;
use crate::state::State;

/// Result of a single transition attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionResult {
    Started,
    Success,
    Rejected,
    Failed,
}

/// One structured event per transition attempt, success or failure
/// (Doc 13 §6 field list: `timestamp, event, previous_state, new_state,
/// transition_id, trigger, component, result, failure_code`).
#[derive(Debug, Clone)]
pub struct StateTransitionEvent {
    pub timestamp: OffsetDateTime,
    pub event: String,
    pub previous_state: State,
    pub new_state: Option<State>,
    pub transition_id: String,
    pub trigger: String,
    pub component: String,
    pub result: TransitionResult,
    pub failure_code: Option<ErrorCode>,
}

impl StateTransitionEvent {
    /// Emits this event via `tracing`. Never logs a secret (Doc 13 §7's
    /// exclusion list: session tokens, access keys, TOTP values, passwords,
    /// raw auth headers) — none of this struct's fields can hold one.
    pub fn emit(&self) {
        tracing::info!(
            event = %self.event,
            previous_state = %self.previous_state,
            new_state = %self.new_state.map(State::as_str).unwrap_or("-"),
            transition_id = %self.transition_id,
            trigger = %self.trigger,
            component = %self.component,
            result = ?self.result,
            failure_code = %self.failure_code.map(ErrorCode::as_str).unwrap_or("-"),
            "state transition",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_does_not_panic_on_a_rejected_transition_with_no_new_state() {
        let event = StateTransitionEvent {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            event: "AUTH_FAILURE".to_string(),
            previous_state: State::Authenticating,
            new_state: None,
            transition_id: "tr_01TEST".to_string(),
            trigger: "client".to_string(),
            component: "remote-hostd".to_string(),
            result: TransitionResult::Rejected,
            failure_code: Some(ErrorCode::AuthInvalid),
        };
        event.emit();
    }
}
