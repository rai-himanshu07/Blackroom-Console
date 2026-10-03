//! Blackroom Console: see and drive this laptop's desktop from a browser while the local panel is
//! blank and the local keyboard and touchpad are grabbed.

pub mod clipboard;
pub mod console;
pub mod display;
pub mod eis_support;
pub mod exposure;
pub mod hostd_auth;
pub mod ice;
pub mod login;
pub mod server;
pub mod tls;
pub mod webrtc;

pub use console::{ConsoleConfig, InputEvent, Phase, Quality, RemoteConsole, Status, StopReport};
