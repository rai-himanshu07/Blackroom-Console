//! Shared CLI flags for Document 10 Phase 1 experiment binaries.

use clap::Parser;

/// Flags common to every `expNN_*` binary.
#[derive(Debug, Parser)]
pub struct CommonArgs {
    /// Write real username/hostname into evidence instead of `[USER]`/`[HOST]`.
    #[arg(long, default_value_t = false)]
    pub no_redact: bool,
}

impl CommonArgs {
    /// `true` when values should be redacted (the default).
    pub fn redact_enabled(&self) -> bool {
        !self.no_redact
    }
}
