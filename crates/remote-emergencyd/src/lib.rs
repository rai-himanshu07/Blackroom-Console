//! Independent holder of the exclusive physical-input grab and observer of the emergency chord
//! (Phase 7 step 5 findings; Doc 06 emergency matrix). The logic in [`core`] is device-free and
//! tested with fakes; [`nodes`] is the evdev implementation and [`server`] the control socket.
//!
//! Nothing here is installed or enabled, and the binary refuses to grab unless started with
//! `--enable-grabs`. Key codes are never logged or forwarded: only the chord keys are tracked,
//! only as a count, and only in memory.
#![forbid(unsafe_code)]

pub use remote_emergency_client::{client, proto};

pub mod core;
pub mod nodes;
pub mod server;
