//! Blackroom Console: see and drive this laptop's desktop from a browser while the local panel is
//! blank and the local keyboard and touchpad are grabbed.

pub mod console;
pub mod display;
pub mod eis_support;

pub use console::{ConsoleConfig, InputEvent, Phase, RemoteConsole, Status, StopReport};
