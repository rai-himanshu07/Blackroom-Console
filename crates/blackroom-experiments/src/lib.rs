//! Shared helpers for Document 10 Phase 1 experiments (`exp00`..`exp02`):
//! evidence-directory layout, the Document 10 §47 result-format writer,
//! username/hostname redaction, and ISO-8601 UTC timestamps.
//!
//! This crate is discardable scaffolding for the feasibility PoC, not
//! production code (assessment §6.1 repository layout).
pub mod cli;
pub mod evidence;
pub mod introspect;
pub mod session;

pub use cli::CommonArgs;
pub use evidence::{ExperimentReport, ExperimentResult, evidence_dir, redact, write_evidence};
pub use introspect::{
    BusKind, InspectedTarget, ParsedInterface, ParsedMethod, ParsedProperty, ParsedSignal, Target,
    introspect_target, method_present, parse_introspection_xml, slug,
};
pub use session::{SessionCandidate, SessionProperties, current_uid, discover, render_rationale};
