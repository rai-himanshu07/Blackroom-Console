//! Virtual monitor lifecycle (Doc 05 §25–27; Experiment 4 evidence,
//! `docs/experiments/evidence/exp04/`). A virtual monitor is created via
//! `ScreenCastSession::record_virtual` and is not a real Mutter monitor
//! until confirmed via `DisplayConfig.GetCurrentState` — a mere capture
//! stream is not sufficient (Doc 10 §11 acceptance).
//!
//! KEY FINDING (Experiment 4): the new connector does **not** appear in
//! `GetCurrentState` immediately after `Start()`/`PipeWireStreamAdded` — it
//! only appears once a real PipeWire client actually consumes the stream.
//! `create` therefore drives a bounded [`pipewire_capture::capture_frames`]
//! call before polling for confirmation, rather than polling first.

use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Type};

use blackroom_core::error::{BlackroomError, ErrorCode};

use super::pipewire_capture;
use super::screencast::{ScreenCastSession, ScreenCastStream};

/// Experiment 4's proven bound for both the `PipeWireStreamAdded` wait and
/// the post-capture `GetCurrentState` confirmation poll.
const CONFIRM_WAIT: Duration = Duration::from_secs(10);
/// Experiment 4's proven frame target for confirmation (2 frames).
const CONFIRM_FRAME_TARGET: u32 = 2;

fn mutter_unavailable(detail: impl std::fmt::Display) -> BlackroomError {
    BlackroomError::new(ErrorCode::MutterUnavailable, detail.to_string())
}

fn virtual_display_failed(detail: impl std::fmt::Display) -> BlackroomError {
    BlackroomError::new(ErrorCode::VirtualDisplayFailed, detail.to_string())
}

// ---------------------------------------------------------------------
// DisplayConfig.GetCurrentState (read-only confirmation; same wire schema
// as `capability.rs`/Experiment 4 — duplicated per-file by established
// project precedent rather than shared, since each is a simple zvariant
// mapping of a stable D-Bus signature).
// ---------------------------------------------------------------------

#[derive(Debug, Type, serde::Deserialize)]
struct ConnectorInfo {
    connector: String,
    #[allow(dead_code)]
    vendor: String,
    #[allow(dead_code)]
    product: String,
    #[allow(dead_code)]
    serial: String,
}

// Trailing fields are unread but must stay in this exact order: zvariant
// matches D-Bus structures by field position, not name.
#[allow(dead_code)]
#[derive(Debug, Type, serde::Deserialize)]
struct ModeInfo {
    id: String,
    width: i32,
    height: i32,
    refresh_rate: f64,
    preferred_scale: f64,
    supported_scales: Vec<f64>,
    properties: HashMap<String, OwnedValue>,
}

#[allow(dead_code)]
#[derive(Debug, Type, serde::Deserialize)]
struct MonitorEntry {
    connector_info: ConnectorInfo,
    modes: Vec<ModeInfo>,
    properties: HashMap<String, OwnedValue>,
}

#[allow(dead_code)]
#[derive(Debug, Type, serde::Deserialize)]
struct LogicalMonitorEntry {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: Vec<ConnectorInfo>,
    properties: HashMap<String, OwnedValue>,
}

type GetCurrentStateResult = (
    u32,
    Vec<MonitorEntry>,
    Vec<LogicalMonitorEntry>,
    HashMap<String, OwnedValue>,
);

fn connectors(conn: &Connection) -> Result<Vec<String>, BlackroomError> {
    let proxy = Proxy::new(
        conn,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )
    .map_err(mutter_unavailable)?;
    let (_serial, monitors, _logical, _props): GetCurrentStateResult = proxy
        .call("GetCurrentState", &())
        .map_err(mutter_unavailable)?;
    Ok(monitors
        .into_iter()
        .map(|monitor| monitor.connector_info.connector)
        .collect())
}

/// A real, `DisplayConfig`-confirmed virtual monitor (Doc 10 §11
/// acceptance: "A real virtual monitor appears in GNOME/Mutter state").
/// Owns the underlying `ScreenCastSession`/`ScreenCastStream`, so dropping
/// it (or calling [`VirtualMonitor::destroy`]) tears the monitor down.
pub struct VirtualMonitor<'a> {
    session: ScreenCastSession<'a>,
    #[allow(dead_code)]
    stream: ScreenCastStream<'a>,
    pub connector: String,
    pub width: i32,
    pub height: i32,
    pub refresh_rate: f64,
}

impl<'a> VirtualMonitor<'a> {
    /// Creates a session, calls `RecordVirtual`, drives a bounded PipeWire
    /// capture to make Mutter register the connector, then confirms it via
    /// `GetCurrentState`. Returns `VirtualDisplayFailed` if the monitor
    /// cannot be confirmed within [`CONFIRM_WAIT`] of capture completing.
    pub fn create(
        conn: &'a Connection,
        width: i32,
        height: i32,
        refresh_rate: f64,
    ) -> Result<Self, BlackroomError> {
        let before = connectors(conn)?;
        let session = ScreenCastSession::create(conn)?;
        let stream = session.record_virtual(width, height, refresh_rate)?;
        let node_id = stream.start_and_wait_for_pipewire_node(&session)?;

        pipewire_capture::capture_frames(
            node_id,
            width,
            height,
            CONFIRM_FRAME_TARGET,
            CONFIRM_WAIT,
        )?;

        let deadline = Instant::now() + CONFIRM_WAIT;
        let connector = loop {
            let after = connectors(conn)?;
            if let Some(connector) = after.into_iter().find(|c| !before.contains(c)) {
                break connector;
            }
            if Instant::now() >= deadline {
                return Err(virtual_display_failed(
                    "virtual monitor did not appear in DisplayConfig.GetCurrentState \
                     within the confirmation window",
                ));
            }
            thread::sleep(Duration::from_millis(150));
        };

        Ok(Self {
            session,
            stream,
            connector,
            width,
            height,
            refresh_rate,
        })
    }

    /// Idempotent (Doc 07 §27): stops the underlying `ScreenCast` session,
    /// safe to call even if already stopped.
    pub fn destroy(mut self) -> Result<(), BlackroomError> {
        self.session.stop()
    }
}
