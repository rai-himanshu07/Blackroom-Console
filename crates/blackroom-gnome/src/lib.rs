//! `GnomeBackend` trait boundary (Doc 05 §8), real session discovery and
//! capability detection (Phase 3, `mutter` module), and an in-memory fake
//! with fault injection. Real Mutter/RemoteDesktop/ScreenCast session
//! creation, display isolation, and remote input remain mocked until
//! Phases 4–7 (assessment C2).
#![forbid(unsafe_code)]

pub mod backend;
pub mod fake;
pub mod mutter;

pub use backend::{CursorState, DisplayState, GnomeBackend, SessionInfo};
pub use fake::{FakeGnomeBackend, FaultConfig, FaultMode};
