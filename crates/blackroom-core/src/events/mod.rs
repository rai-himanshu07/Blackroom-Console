//! Structured event schema (Doc 13 §6–§7): the state-transition event and
//! the general structured-log field set.

pub mod structured_log;
pub mod transition_event;

pub use structured_log::{Severity, StructuredLogFields};
pub use transition_event::{StateTransitionEvent, TransitionResult};
