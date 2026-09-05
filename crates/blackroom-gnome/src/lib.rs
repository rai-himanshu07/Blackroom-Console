//! `GnomeBackend` trait boundary (Doc 05 §8) and in-memory fake with fault
//! injection. No real GNOME/Mutter/D-Bus call is made from this crate yet
//! (mock-first, assessment C2); a real Mutter-backed implementation is
//! Phase 3+.
#![forbid(unsafe_code)]

pub mod backend;
pub mod fake;

pub use backend::{CursorState, DisplayState, GnomeBackend, SessionInfo};
pub use fake::{FakeGnomeBackend, FaultConfig, FaultMode};
