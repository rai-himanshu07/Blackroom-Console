//! Pure logic for exclusive physical-input grabs (Phase 7, Gate FEAS-E).
//!
//! No device access, no I/O, no key data: a caller (the emergency binary) maps
//! evdev nodes to [`Caps`], implements [`DeviceGrab`] with `EVIOCGRAB`, and feeds
//! chord-key events here. Design: `docs/security/input-isolation-decision.md`.
//! Nothing here is evidence that a grab works on real hardware.
#![forbid(unsafe_code)]

mod chord;
mod classify;
mod isolation;

pub use chord::{Chord, ChordDetector};
pub use classify::{Caps, Classification, Exclusion, Role, classify};
pub use isolation::{
    DeviceGrab, DeviceId, GrabError, HotplugOutcome, IsolateError, Isolation, RestoreOutcome,
    State, TickEvent,
};
