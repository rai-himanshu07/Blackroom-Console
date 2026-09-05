//! Core domain types for Blackroom Console: state machine, control lease,
//! security epoch, protocol types, error codes, and event schema.
//!
//! No I/O: real GNOME calls, disk persistence, and networking live in
//! other crates (`blackroom-gnome`, and the not-yet-created
//! `blackroom-store`/`remote-hostd`). See
//! `docs/plans/plan-20260905-phase2-state-machine-core.md`.
#![forbid(unsafe_code)]

pub mod epoch;
pub mod error;
pub mod event;
pub mod events;
pub mod lease;
pub mod limits;
pub mod lock;
pub mod protocol;
pub mod state;
pub mod transition;
