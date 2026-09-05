//! `GnomeBackend` trait (Doc 05 §8), cross-checked against
//! `docs/gnome/api-inventory.md` (Phase 1 evidence: every Mutter/Shell/
//! logind D-Bus interface actually present on GNOME 50.1). This trait
//! itself still has no implementation making real calls — `mutter::session`
//! and `mutter::capability` (Phase 3) provide the real session-discovery
//! and capability-detection logic that a future concrete `GnomeBackend`
//! will assemble alongside later phases' virtual-display/display-isolation/
//! remote-input modules (assessment C2).
#![allow(clippy::doc_markdown)]

use blackroom_core::error::BlackroomError;

/// A GNOME/logind session as `discover_session` reports it (Doc 05 §12–§15
/// selection rules: UID + seat + `Type=wayland` + `Class=user`, never "the
/// first session" — this host has two sessions on seat0, per Phase 1
/// evidence). Host capability bits are a separate concern (Phase 3
/// `mutter::capability::CapabilityReport`, assessment C6: distinct from
/// lease capabilities `VIEW`/`CONTROL`), not a field on this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: String,
    pub uid: u32,
    pub seat: String,
    pub is_wayland: bool,
    pub active: bool,
}

/// Display topology as `get_display_state` reports it (maps conceptually
/// to `Mutter.DisplayConfig.GetCurrentState`, `docs/gnome/api-inventory.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayState {
    pub connectors: Vec<String>,
    pub virtual_monitor_active: bool,
}

/// Cursor position/visibility as `get_cursor_state` would report it.
/// **No matching D-Bus interface was found in the Phase 1
/// `docs/gnome/api-inventory.md` evidence** — flagged here as a Phase 3
/// research gap, not resolved by this trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorState {
    pub x: i32,
    pub y: i32,
    pub visible: bool,
}

/// The GNOME/Mutter/PipeWire operation boundary (Doc 05 §8). Every method
/// maps to a real interface candidate per the mapping table in
/// `docs/plans/plan-20260905-phase2-state-machine-core.md` Evidence And
/// Decisions. `restore_session` must NEVER unlock GNOME — it restores
/// local display/input ownership only; GNOME unlock stays a manual local
/// user action (Invariant 8).
pub trait GnomeBackend {
    /// Real candidate: `login1.Manager.ListSessions`/`ListSessionsEx` +
    /// `login1.Session` properties (`Type`, `Class`, `Active`).
    fn discover_session(&mut self) -> Result<SessionInfo, BlackroomError>;
    /// Real candidate: `Mutter.DisplayConfig.GetCurrentState`.
    fn get_display_state(&mut self) -> Result<DisplayState, BlackroomError>;
    /// Real candidate: `Mutter.RemoteDesktop.CreateSession` +
    /// `Mutter.ScreenCast.CreateSession` + `DisplayConfig.ApplyMonitorsConfig`.
    fn create_virtual_monitor(&mut self) -> Result<(), BlackroomError>;
    /// Idempotent (Doc 07 §27): safe when the monitor no longer exists.
    fn destroy_virtual_monitor(&mut self) -> Result<(), BlackroomError>;
    /// Real candidate: `Mutter.DisplayConfig.ApplyMonitorsConfig`.
    fn disable_physical_outputs(&mut self) -> Result<(), BlackroomError>;
    /// Idempotent (Doc 07 §27): safe when already restored.
    fn restore_physical_outputs(&mut self) -> Result<(), BlackroomError>;
    /// Real candidate: `Mutter.InputCapture.CreateSession` (barrier-crossing
    /// model — Phase 0-1 finding; Gate E still `UNKNOWN`).
    fn enable_remote_input(&mut self) -> Result<(), BlackroomError>;
    /// Idempotent (Doc 07 §27): safe when already disabled.
    fn disable_remote_input(&mut self) -> Result<(), BlackroomError>;
    /// Real candidate: ScreenCast session `Start` (sub-object, not
    /// enumerable from top-level introspection).
    fn start_capture(&mut self) -> Result<(), BlackroomError>;
    fn stop_capture(&mut self) -> Result<(), BlackroomError>;
    /// Idempotent (Doc 07 §27): safe when GNOME is already locked. Real
    /// candidate: `org.gnome.ScreenSaver.Lock` (`ScreenShield` aliases to
    /// it on this GNOME version) or `login1.Session.Lock` (emergency
    /// fallback).
    fn lock_session(&mut self) -> Result<(), BlackroomError>;
    /// No known backing D-Bus interface yet (Phase 3 research gap).
    fn get_cursor_state(&mut self) -> Result<CursorState, BlackroomError>;
    /// Restores local display/input ownership ONLY — must not call
    /// `Unlock`/`SetActive(false)` (Invariant 8).
    fn restore_session(&mut self) -> Result<(), BlackroomError>;
}
