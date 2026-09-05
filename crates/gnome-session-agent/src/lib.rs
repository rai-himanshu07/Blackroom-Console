//! `gnome-session-agent`: runs as the logged-in user via systemd `--user`
//! (Doc 05 §16-17, Doc 06 §28-32), discovers the GNOME session, runs the
//! Phase 3 capability gate, and reports its own [`state::AgentState`] over
//! `agent.sock`. Real logic lives here (not in `main.rs`) so it stays
//! unit-testable via `cargo test -p gnome-session-agent`.
#![forbid(unsafe_code)]

pub mod startup;
pub mod state;

pub use state::AgentState;
