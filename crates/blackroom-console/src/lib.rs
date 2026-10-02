//! Blackroom Console: see and drive this laptop's desktop from a browser while the local panel is
//! blank and the local keyboard and touchpad are grabbed.

pub mod console;
pub mod display;
pub mod eis_support;
pub mod server;
pub mod tls;
pub mod webrtc;

pub use console::{ConsoleConfig, InputEvent, Phase, Quality, RemoteConsole, Status, StopReport};
