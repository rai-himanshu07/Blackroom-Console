//! Real (non-mutating) Mutter/logind integration (Phase 3, assessment C2:
//! mock-first ends here for session discovery and capability detection).
//! Every call in this module tree is read-only (`Introspect`/`Get`/
//! `ListSessions`/`GetCurrentState`-style) — no `CreateSession`,
//! `RecordVirtual`, `ApplyMonitorsConfig`, or `ConnectToEIS` call is made
//! anywhere here; those remain Phase 4+.

pub mod capability;
pub mod session;
