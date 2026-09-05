//! Structured logging fields (Doc 13 §7): a superset used for general
//! operational logging beyond a single transition.

use time::OffsetDateTime;

use crate::error::ErrorCode;
use crate::state::State;

/// Doc 13 §8 log-severity levels (`DEBUG, INFO, NOTICE/WARNING, ERROR,
/// CRITICAL`; `NOTICE`/`WARNING` are treated as one level here, matching
/// the source document's single heading for both).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Debug,
    Info,
    Notice,
    Error,
    Critical,
}

/// Doc 13 §7's recommended field list. `gnome_version`/`mutter_version`/
/// `pipewire_version`/etc. ("where appropriate") are Phase 3+ fields (no
/// real GNOME backend exists yet) and are intentionally omitted here.
#[derive(Debug, Clone)]
pub struct StructuredLogFields {
    pub timestamp: OffsetDateTime,
    pub severity: Severity,
    pub component: String,
    pub event: String,
    pub state: Option<State>,
    pub transition_id: Option<String>,
    pub session_id: Option<String>,
    pub client_id: Option<String>,
    pub security_epoch: Option<u64>,
    pub result: String,
    pub error_code: Option<ErrorCode>,
    pub duration_ms: Option<u64>,
}

impl StructuredLogFields {
    /// Emits via `tracing` at the level matching [`Severity`]. Never logs a
    /// secret (Doc 13 §7's exclusion list) — none of this struct's fields
    /// can hold one.
    pub fn emit(&self) {
        macro_rules! log_at {
            ($level:ident) => {{
                tracing::$level!(
                    component = %self.component,
                    event = %self.event,
                    state = %self.state.map(State::as_str).unwrap_or("-"),
                    transition_id = %self.transition_id.as_deref().unwrap_or("-"),
                    session_id = %self.session_id.as_deref().unwrap_or("-"),
                    client_id = %self.client_id.as_deref().unwrap_or("-"),
                    security_epoch = self.security_epoch.unwrap_or_default(),
                    result = %self.result,
                    error_code = %self.error_code.map(ErrorCode::as_str).unwrap_or("-"),
                    duration_ms = self.duration_ms.unwrap_or_default(),
                    "{}",
                    self.event,
                )
            }};
        }
        match self.severity {
            Severity::Debug => log_at!(debug),
            Severity::Info => log_at!(info),
            Severity::Notice => log_at!(warn),
            Severity::Error => log_at!(error),
            Severity::Critical => log_at!(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_ordering_is_debug_lowest_critical_highest() {
        assert!(Severity::Debug < Severity::Info);
        assert!(Severity::Info < Severity::Notice);
        assert!(Severity::Notice < Severity::Error);
        assert!(Severity::Error < Severity::Critical);
    }

    #[test]
    fn emit_does_not_panic_with_minimal_fields() {
        let fields = StructuredLogFields {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            severity: Severity::Info,
            component: "blackroom-core".to_string(),
            event: "test".to_string(),
            state: None,
            transition_id: None,
            session_id: None,
            client_id: None,
            security_epoch: None,
            result: "ok".to_string(),
            error_code: None,
            duration_ms: None,
        };
        fields.emit();
    }
}
