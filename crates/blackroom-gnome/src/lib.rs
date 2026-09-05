//! `GnomeBackend` trait boundary (Doc 05 §8), real session discovery and
//! capability detection (Phase 3), real RemoteDesktop/ScreenCast session and
//! virtual-monitor mechanics (Phase 4, `mutter` module), and an in-memory
//! fake with fault injection. Display isolation and remote input remain
//! mocked until Phases 5–7; no concrete `GnomeBackend` implementation
//! assembles these modules yet (assessment C2; Phase 4 plan Evidence #2).
#![forbid(unsafe_code)]

pub mod backend;
pub mod fake;
pub mod mutter;

pub use backend::{CursorState, DisplayState, GnomeBackend, SessionInfo};
pub use fake::{FakeGnomeBackend, FaultConfig, FaultMode};
