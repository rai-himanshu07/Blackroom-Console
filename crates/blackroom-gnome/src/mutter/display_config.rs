//! Physical display isolation (Doc 05 §28–40, §60–62; Doc 10 Exp 6–7; Doc 02
//! §12–13, §37–38; Doc 20 §20–24) — `DisplayBackup` snapshot/disable/restore,
//! Phase 5's hard gate FEAS-C. Every `ApplyMonitorsConfig` call uses
//! `method=Temporary`; this module never writes `monitors.xml`.
//!
//! KEY DISTINCTION (Doc 05 §33–34): a "disabled" physical output stays
//! present in `GetCurrentState`'s top-level `monitors` inventory (the
//! hardware connector reference persists) but has no entry in any
//! `logical_monitors[].monitors` — that absence from the active-desktop
//! topology, not any claim about panel electronics, is what this module
//! verifies. Restore reuses the exact mode IDs captured at snapshot time
//! (Experiment 5 precedent: only the `ApplyMonitorsConfig` `serial` is
//! re-read fresh on every call, not the mode IDs).

use std::collections::HashMap;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Type, Value};

use blackroom_core::error::{BlackroomError, ErrorCode};

const METHOD_TEMPORARY: u32 = 1;

fn mutter_unavailable(detail: impl std::fmt::Display) -> BlackroomError {
    BlackroomError::new(ErrorCode::MutterUnavailable, detail.to_string())
}

fn display_isolation_failed(detail: impl std::fmt::Display) -> BlackroomError {
    BlackroomError::new(ErrorCode::DisplayIsolationFailed, detail.to_string())
}

fn display_restore_failed(detail: impl std::fmt::Display) -> BlackroomError {
    BlackroomError::new(ErrorCode::DisplayRestoreFailed, detail.to_string())
}

// ---------------------------------------------------------------------
// DisplayConfig.GetCurrentState/ApplyMonitorsConfig wire types (established
// per-file duplication pattern — see capability.rs/virtual_monitor.rs/
// exp0{2,4,5}; Phase 5 plan Decision 10: not deduplicated across files).
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Type, serde::Deserialize)]
struct ConnectorInfo {
    connector: String,
    vendor: String,
    product: String,
    serial: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Type, serde::Deserialize)]
struct ModeInfo {
    id: String,
    width: i32,
    height: i32,
    refresh_rate: f64,
    preferred_scale: f64,
    supported_scales: Vec<f64>,
    properties: HashMap<String, OwnedValue>,
}

#[derive(Debug, Clone, Type, serde::Deserialize)]
struct MonitorEntry {
    connector_info: ConnectorInfo,
    modes: Vec<ModeInfo>,
    #[allow(dead_code)]
    properties: HashMap<String, OwnedValue>,
}

#[derive(Debug, Clone, Type, serde::Deserialize)]
struct LogicalMonitorEntry {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: Vec<ConnectorInfo>,
    #[allow(dead_code)]
    properties: HashMap<String, OwnedValue>,
}

type GetCurrentStateResult = (
    u32,
    Vec<MonitorEntry>,
    Vec<LogicalMonitorEntry>,
    HashMap<String, OwnedValue>,
);

#[derive(Debug, Clone, Type, serde::Serialize)]
struct MonitorRef {
    connector: String,
    mode_id: String,
    properties: HashMap<String, OwnedValue>,
}

#[derive(Debug, Clone, Type, serde::Serialize)]
struct LogicalMonitorConfig {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: Vec<MonitorRef>,
}

fn is_current_mode(properties: &HashMap<String, OwnedValue>) -> bool {
    properties
        .get("is-current")
        .and_then(|value| bool::try_from(value.clone()).ok())
        .unwrap_or(false)
}

fn display_config_proxy(conn: &Connection) -> Result<Proxy<'_>, BlackroomError> {
    Proxy::new(
        conn,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )
    .map_err(mutter_unavailable)
}

fn read_state(
    conn: &Connection,
) -> Result<(u32, Vec<MonitorEntry>, Vec<LogicalMonitorEntry>), BlackroomError> {
    let (serial, monitors, logical_monitors, _props): GetCurrentStateResult =
        display_config_proxy(conn)?
            .call("GetCurrentState", &())
            .map_err(mutter_unavailable)?;
    Ok((serial, monitors, logical_monitors))
}

/// Doc 05 §30: validate before disabling, don't discover incompatibility
/// afterward.
fn apply_monitors_config_allowed(conn: &Connection) -> Result<bool, BlackroomError> {
    display_config_proxy(conn)?
        .get_property::<bool>("ApplyMonitorsConfigAllowed")
        .map_err(mutter_unavailable)
}

/// `PowerSaveMode` DPMS levels (standard: 0=ON, 1=STANDBY, 2=SUSPEND,
/// 3=OFF; confirmed live 2026-09-05 — reading back `0` matched the panel
/// being visibly on, and setting `3` visibly blanked it).
const POWER_SAVE_ON: i32 = 0;
const POWER_SAVE_OFF: i32 = 3;

/// Forces a real hardware blank/wake, global across every output (not
/// per-connector — `docs/gnome/api-inventory.md`). Doc 05 §34's caveat that
/// "disabled" does not guarantee the panel stops rendering is not
/// hypothetical here: confirmed live 2026-09-05 that a bare
/// `ApplyMonitorsConfig` zero-physical disable left `eDP-1` showing a
/// frozen, fully visible mid-reflow frame (GNOME reflowing windows onto the
/// sole remaining virtual monitor) rather than blanking — a real,
/// undetectable-by-`GetCurrentState` privacy exposure. `PowerSaveMode`
/// already proved it can force a genuine blank independent of logical
/// topology (the earlier probe blanked both panels with topology
/// unchanged), so `disable_physical_outputs`/`restore_physical_outputs`
/// pair it with the `ApplyMonitorsConfig` call.
fn set_power_save_mode(conn: &Connection, mode: i32) -> Result<(), BlackroomError> {
    display_config_proxy(conn)?
        .set_property("PowerSaveMode", mode)
        .map_err(mutter_unavailable)
}

fn apply(
    conn: &Connection,
    serial: u32,
    logical_monitors: &[LogicalMonitorConfig],
) -> Result<(), BlackroomError> {
    let empty_props: HashMap<&str, Value<'_>> = HashMap::new();
    display_config_proxy(conn)?
        .call::<_, _, ()>(
            "ApplyMonitorsConfig",
            &(
                serial,
                METHOD_TEMPORARY,
                logical_monitors.to_vec(),
                empty_props,
            ),
        )
        .map_err(mutter_unavailable)
}

// ---------------------------------------------------------------------
// DisplayBackup (Doc 05 §29 schema; Phase 5 plan Decision 3: keyed by
// connector+EDID serial, not array index — Doc 02 §37).
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct OutputBackup {
    pub connector: String,
    pub vendor: String,
    pub product: String,
    pub serial: String,
    pub mode_id: String,
    pub width: i32,
    pub height: i32,
    pub refresh_rate: f64,
    /// Present in `outputs[]` regardless; `enabled` iff it appears in some
    /// `topology[].monitors` entry (Doc 05 §33–34's distinction).
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogicalMonitorBackup {
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: u32,
    pub primary: bool,
    pub monitors: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayBackup {
    pub timestamp_unix: i64,
    pub session_id: String,
    pub outputs: Vec<OutputBackup>,
    pub topology: Vec<LogicalMonitorBackup>,
    pub primary_output: Option<(String, String)>,
    /// Versioned output equality check (Doc 05 §29), not a security control.
    /// Full logical-topology fields must also match.
    pub configuration_hash: u64,
}

pub const CONFIGURATION_HASH_VERSION: u32 = 1;

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn compute_hash(outputs: &[OutputBackup]) -> u64 {
    let mut sorted: Vec<&OutputBackup> = outputs.iter().collect();
    sorted.sort_by(|a, b| (&a.connector, &a.serial).cmp(&(&b.connector, &b.serial)));
    let mut hasher = Sha256::new();
    hasher.update(b"blackroom-output-hash-v1");
    hasher.update((sorted.len() as u64).to_be_bytes());
    for output in sorted {
        for field in [
            &output.connector,
            &output.vendor,
            &output.product,
            &output.serial,
            &output.mode_id,
        ] {
            hasher.update((field.len() as u64).to_be_bytes());
            hasher.update(field.as_bytes());
        }
        hasher.update(output.width.to_be_bytes());
        hasher.update(output.height.to_be_bytes());
        hasher.update(output.refresh_rate.to_bits().to_be_bytes());
        hasher.update([u8::from(output.enabled)]);
    }
    let digest = hasher.finalize();
    u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("SHA-256 digest is at least 8 bytes"),
    )
}

/// One output's identity + currently-active mode, extracted from the raw
/// zvariant shape into plain fields so [`build_backup`] stays unit-testable
/// without constructing zvariant values.
#[derive(Debug, Clone, PartialEq)]
struct CurrentOutput {
    connector: String,
    vendor: String,
    product: String,
    serial: String,
    mode_id: String,
    width: i32,
    height: i32,
    refresh_rate: f64,
}

fn current_outputs(monitors: &[MonitorEntry]) -> Vec<CurrentOutput> {
    monitors
        .iter()
        .filter_map(|monitor| {
            let mode = monitor
                .modes
                .iter()
                .find(|mode| is_current_mode(&mode.properties))?;
            Some(CurrentOutput {
                connector: monitor.connector_info.connector.clone(),
                vendor: monitor.connector_info.vendor.clone(),
                product: monitor.connector_info.product.clone(),
                serial: monitor.connector_info.serial.clone(),
                mode_id: mode.id.clone(),
                width: mode.width,
                height: mode.height,
                refresh_rate: mode.refresh_rate,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
struct CurrentLogicalMonitor {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: Vec<(String, String)>,
}

fn current_topology(logical: &[LogicalMonitorEntry]) -> Vec<CurrentLogicalMonitor> {
    logical
        .iter()
        .map(|lm| CurrentLogicalMonitor {
            x: lm.x,
            y: lm.y,
            scale: lm.scale,
            transform: lm.transform,
            primary: lm.primary,
            monitors: lm
                .monitors
                .iter()
                .map(|c| (c.connector.clone(), c.serial.clone()))
                .collect(),
        })
        .collect()
}

/// Pure construction (no D-Bus) — unit-tested directly with fixtures.
fn build_backup(
    session_id: &str,
    timestamp_unix: i64,
    outputs: Vec<CurrentOutput>,
    topology: Vec<CurrentLogicalMonitor>,
) -> DisplayBackup {
    let enabled_connectors: std::collections::HashSet<(String, String)> = topology
        .iter()
        .flat_map(|lm| lm.monitors.iter().cloned())
        .collect();
    let primary_output = topology
        .iter()
        .find(|lm| lm.primary)
        .and_then(|lm| lm.monitors.first().cloned());
    let output_backups: Vec<OutputBackup> = outputs
        .into_iter()
        .map(|o| {
            let enabled = enabled_connectors.contains(&(o.connector.clone(), o.serial.clone()));
            OutputBackup {
                connector: o.connector,
                vendor: o.vendor,
                product: o.product,
                serial: o.serial,
                mode_id: o.mode_id,
                width: o.width,
                height: o.height,
                refresh_rate: o.refresh_rate,
                enabled,
            }
        })
        .collect();
    let configuration_hash = compute_hash(&output_backups);
    DisplayBackup {
        timestamp_unix,
        session_id: session_id.to_string(),
        outputs: output_backups,
        topology: topology
            .into_iter()
            .map(|lm| LogicalMonitorBackup {
                x: lm.x,
                y: lm.y,
                scale: lm.scale,
                transform: lm.transform,
                primary: lm.primary,
                monitors: lm.monitors,
            })
            .collect(),
        primary_output,
        configuration_hash,
    }
}

/// Reconstructs the `ApplyMonitorsConfig` write-side shape from a
/// `DisplayBackup`, reusing the mode IDs captured at snapshot time rather
/// than re-resolving them (Experiment 5 precedent). Fails closed if the
/// backup's own `topology` references a connector/serial absent from its
/// `outputs[]` (an internal-consistency invariant, not an environmental
/// failure).
fn to_write_side(backup: &DisplayBackup) -> Result<Vec<LogicalMonitorConfig>, BlackroomError> {
    backup
        .topology
        .iter()
        .map(|lm| {
            let monitors = lm
                .monitors
                .iter()
                .map(|(connector, serial)| {
                    backup
                        .outputs
                        .iter()
                        .find(|o| &o.connector == connector && &o.serial == serial)
                        .map(|o| MonitorRef {
                            connector: connector.clone(),
                            mode_id: o.mode_id.clone(),
                            properties: HashMap::new(),
                        })
                        .ok_or_else(|| {
                            display_restore_failed(format!(
                                "DisplayBackup inconsistent: topology references connector \
                                 {connector} serial {serial} not present in outputs[]"
                            ))
                        })
                })
                .collect::<Result<Vec<_>, BlackroomError>>()?;
            Ok(LogicalMonitorConfig {
                x: lm.x,
                y: lm.y,
                scale: lm.scale,
                transform: lm.transform,
                primary: lm.primary,
                monitors,
            })
        })
        .collect()
}

fn zero_physical_config(
    virtual_connector: &str,
    virtual_mode_id: &str,
) -> Vec<LogicalMonitorConfig> {
    vec![LogicalMonitorConfig {
        x: 0,
        y: 0,
        scale: 1.0,
        transform: 0,
        primary: true,
        monitors: vec![MonitorRef {
            connector: virtual_connector.to_string(),
            mode_id: virtual_mode_id.to_string(),
            properties: HashMap::new(),
        }],
    }]
}

/// Real candidate for `GnomeBackend::get_display_state`'s snapshot half
/// (Doc 05 §8) and for `disable_physical_outputs`/`restore_physical_outputs`'s
/// "capture before mutating" precondition (Doc 05 §28–30). No concrete
/// `GnomeBackend` impl exists yet (Phase 5 plan Decision 2) — this is a free
/// function, not a trait method body.
pub fn snapshot(conn: &Connection, session_id: &str) -> Result<DisplayBackup, BlackroomError> {
    let (_serial, monitors, logical) = read_state(conn)?;
    Ok(build_backup(
        session_id,
        now_unix(),
        current_outputs(&monitors),
        current_topology(&logical),
    ))
}

/// Disables every physical output by applying a logical-monitors config
/// containing **only** the already-created virtual monitor's entry (Doc 05
/// §35: "All physical outputs should be disabled" — assessment §7.3's open
/// "does Mutter accept zero physical monitors" question). Caller must have
/// already created and confirmed `virtual_connector` (Phase 4's
/// `virtual_monitor::VirtualMonitor::create`) before calling this — Doc 05
/// §30 order: validate the virtual configuration works before disabling
/// anything physical.
///
/// Blanks via `PowerSaveMode` **before** changing the topology (not after):
/// this avoids ever exposing the brief window-reflow transition a bare
/// `ApplyMonitorsConfig` call leaves frozen on the panel (found live
/// 2026-09-05 — see [`set_power_save_mode`]). If the topology change itself
/// fails, the panel is un-blanked again on a best-effort basis rather than
/// left dark with the original, fully-active topology still intact.
pub fn disable_physical_outputs(
    conn: &Connection,
    virtual_connector: &str,
) -> Result<(), BlackroomError> {
    if !apply_monitors_config_allowed(conn)? {
        return Err(display_isolation_failed(
            "ApplyMonitorsConfigAllowed is false; refusing to attempt a zero-physical config",
        ));
    }
    set_power_save_mode(conn, POWER_SAVE_OFF)?;
    let result = (|| -> Result<(), BlackroomError> {
        let (serial, monitors, _logical) = read_state(conn)?;
        let virtual_mode_id = virtual_mode_id(&monitors, virtual_connector)?;
        apply(
            conn,
            serial,
            &zero_physical_config(virtual_connector, &virtual_mode_id),
        )
    })();
    if result.is_err() {
        let _ = set_power_save_mode(conn, POWER_SAVE_ON);
    }
    result
}

fn virtual_mode_id(
    monitors: &[MonitorEntry],
    virtual_connector: &str,
) -> Result<String, BlackroomError> {
    monitors
        .iter()
        .find(|m| m.connector_info.connector == virtual_connector)
        .and_then(|m| {
            m.modes
                .iter()
                .find(|mode| is_current_mode(&mode.properties))
        })
        .map(|mode| mode.id.clone())
        .ok_or_else(|| {
            display_isolation_failed(format!(
                "no current mode reported for virtual connector {virtual_connector}"
            ))
        })
}

/// Idempotent (Doc 07 §27 convention): re-applying an already-restored
/// topology is a harmless no-op from Mutter's perspective. Doc 05 §62's
/// failure handling (retry safely, never claim success on unknown state) is
/// the caller's responsibility — this function reports failure plainly and
/// does not retry internally.
///
/// Un-blanks via `PowerSaveMode` only **after** the real topology is back
/// (mirrors `disable_physical_outputs`'s blank-before-topology-change
/// ordering — see [`set_power_save_mode`]) so nothing transitional is ever
/// revealed in either direction. If the topology restore itself fails, the
/// panel is deliberately left blanked rather than un-blanked onto an
/// unknown/partial state (Doc 05 §62: never claim a safe state you have not
/// verified).
pub fn restore_physical_outputs(
    conn: &Connection,
    backup: &DisplayBackup,
) -> Result<(), BlackroomError> {
    let write_side = to_write_side(backup)?;
    let (serial, ..) = read_state(conn)?;
    apply(conn, serial, &write_side)?;
    set_power_save_mode(conn, POWER_SAVE_ON)
}

/// [`restore_physical_outputs`] that keeps `virtual_connector` as an extra logical monitor
/// right of the restored ones. Mutter 50.1 dereferences a NULL view in the ScreenCast
/// virtual-stream `monitors-changed` handler when an enabled stream's virtual monitor has no
/// logical monitor (Shell SIGSEGV reproduced by exp13), so restore this way while any consumer may
/// stream and stop the ScreenCast session afterwards.
pub fn restore_physical_outputs_keeping_virtual(
    conn: &Connection,
    backup: &DisplayBackup,
    virtual_connector: &str,
) -> Result<(), BlackroomError> {
    let mut config = to_write_side(backup)?;
    let (serial, monitors, _logical) = read_state(conn)?;
    let virtual_mode_id = virtual_mode_id(&monitors, virtual_connector)?;
    let (x, y) = config
        .iter()
        .map(|lm| {
            let width = lm
                .monitors
                .first()
                .and_then(|m| backup.outputs.iter().find(|o| o.connector == m.connector))
                .map_or(0, |o| o.width);
            (lm.x + (f64::from(width) / lm.scale).ceil() as i32, lm.y)
        })
        .max_by_key(|(right_edge, _)| *right_edge)
        .unwrap_or((0, 0));
    config.push(LogicalMonitorConfig {
        x,
        y,
        scale: 1.0,
        transform: 0,
        primary: false,
        monitors: vec![MonitorRef {
            connector: virtual_connector.to_string(),
            mode_id: virtual_mode_id,
            properties: HashMap::new(),
        }],
    });
    apply(conn, serial, &config)?;
    set_power_save_mode(conn, POWER_SAVE_ON)
}

/// Compares a fresh snapshot's `configuration_hash` against `backup`'s (Doc
/// 05 §61: "where possible, compare against the original configuration
/// snapshot"). `Ok(true)` only when every output's identity, mode, and
/// enabled/disabled state matches exactly.
pub fn verify_restored(conn: &Connection, backup: &DisplayBackup) -> Result<bool, BlackroomError> {
    let (_serial, monitors, logical) = read_state(conn)?;
    let current = build_backup(
        &backup.session_id,
        now_unix(),
        current_outputs(&monitors),
        current_topology(&logical),
    );
    Ok(current.configuration_hash == backup.configuration_hash)
}

/// Blocks up to `timeout` for `MonitorsChanged` (confirmed present, no args,
/// `docs/gnome/api-inventory.md` — Doc 20 §7 "No API Guessing"). Caller must
/// arm this **before** the change that might trigger it, mirroring
/// `screencast.rs`'s proven `PipeWireStreamAdded`-before-`Start()` ordering.
/// Returns `Ok(true)` if the signal fired within the window, `Ok(false)` on
/// timeout with no signal.
pub fn wait_for_monitors_changed(
    conn: &Connection,
    timeout: Duration,
) -> Result<bool, BlackroomError> {
    let mut signal_iter = display_config_proxy(conn)?
        .receive_signal("MonitorsChanged")
        .map_err(mutter_unavailable)?;
    let fired = thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel();
        scope.spawn(move || {
            if signal_iter.next().is_some() {
                let _ = tx.send(());
            }
        });
        rx.recv_timeout(timeout).is_ok()
    });
    Ok(fired)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(connector: &str, serial: &str, mode_id: &str, enabled: bool) -> OutputBackup {
        OutputBackup {
            connector: connector.to_string(),
            vendor: "VEN".to_string(),
            product: "PROD".to_string(),
            serial: serial.to_string(),
            mode_id: mode_id.to_string(),
            width: 1920,
            height: 1080,
            refresh_rate: 60.0,
            enabled,
        }
    }

    fn current_output(connector: &str, serial: &str, mode_id: &str) -> CurrentOutput {
        CurrentOutput {
            connector: connector.to_string(),
            vendor: "VEN".to_string(),
            product: "PROD".to_string(),
            serial: serial.to_string(),
            mode_id: mode_id.to_string(),
            width: 1920,
            height: 1080,
            refresh_rate: 60.0,
        }
    }

    fn logical(primary: bool, monitors: Vec<(&str, &str)>) -> CurrentLogicalMonitor {
        CurrentLogicalMonitor {
            x: 0,
            y: 0,
            scale: 1.0,
            transform: 0,
            primary,
            monitors: monitors
                .into_iter()
                .map(|(c, s)| (c.to_string(), s.to_string()))
                .collect(),
        }
    }

    #[test]
    fn compute_hash_is_order_independent() {
        let a = vec![
            output("eDP-1", "S1", "m1", true),
            output("HDMI-1", "S2", "m2", true),
        ];
        let b = vec![
            output("HDMI-1", "S2", "m2", true),
            output("eDP-1", "S1", "m1", true),
        ];
        assert_eq!(compute_hash(&a), compute_hash(&b));
    }

    #[test]
    fn compute_hash_differs_when_an_output_differs() {
        let a = vec![output("eDP-1", "S1", "m1", true)];
        let mut b = a.clone();
        b[0].width = 2560;
        assert_ne!(compute_hash(&a), compute_hash(&b));
    }

    #[test]
    fn compute_hash_has_a_stable_version_one_fixture() {
        assert_eq!(CONFIGURATION_HASH_VERSION, 1);
        assert_eq!(
            compute_hash(&[output("eDP-1", "S1", "m1", true)]),
            130760377717090583
        );
    }

    #[test]
    fn build_backup_marks_outputs_enabled_iff_present_in_topology() {
        let outputs = vec![
            current_output("eDP-1", "S1", "m1"),
            current_output("HDMI-1", "S2", "m2"),
        ];
        let topology = vec![logical(true, vec![("eDP-1", "S1")])];
        let backup = build_backup("session-1", 0, outputs, topology);
        let edp = backup
            .outputs
            .iter()
            .find(|o| o.connector == "eDP-1")
            .unwrap();
        let hdmi = backup
            .outputs
            .iter()
            .find(|o| o.connector == "HDMI-1")
            .unwrap();
        assert!(edp.enabled);
        assert!(!hdmi.enabled);
    }

    #[test]
    fn build_backup_finds_primary_output_from_primary_logical_monitor() {
        let outputs = vec![current_output("HDMI-1", "S2", "m2")];
        let topology = vec![logical(true, vec![("HDMI-1", "S2")])];
        let backup = build_backup("session-1", 0, outputs, topology);
        assert_eq!(
            backup.primary_output,
            Some(("HDMI-1".to_string(), "S2".to_string()))
        );
    }

    #[test]
    fn build_backup_hash_excludes_timestamp_and_session() {
        let outputs = vec![current_output("eDP-1", "S1", "m1")];
        let topology = vec![logical(true, vec![("eDP-1", "S1")])];
        let a = build_backup("session-1", 100, outputs.clone(), topology.clone());
        let b = build_backup("session-2", 200, outputs, topology);
        assert_eq!(a.configuration_hash, b.configuration_hash);
    }

    #[test]
    fn to_write_side_reconstructs_monitor_refs_from_outputs() {
        let backup = DisplayBackup {
            timestamp_unix: 0,
            session_id: "session-1".to_string(),
            outputs: vec![output("eDP-1", "S1", "m1", true)],
            topology: vec![LogicalMonitorBackup {
                x: 0,
                y: 0,
                scale: 1.0,
                transform: 0,
                primary: true,
                monitors: vec![("eDP-1".to_string(), "S1".to_string())],
            }],
            primary_output: Some(("eDP-1".to_string(), "S1".to_string())),
            configuration_hash: 0,
        };
        let write_side = to_write_side(&backup).unwrap();
        assert_eq!(write_side.len(), 1);
        assert_eq!(write_side[0].monitors[0].connector, "eDP-1");
        assert_eq!(write_side[0].monitors[0].mode_id, "m1");
    }

    #[test]
    fn to_write_side_fails_closed_on_inconsistent_backup() {
        let backup = DisplayBackup {
            timestamp_unix: 0,
            session_id: "session-1".to_string(),
            outputs: vec![],
            topology: vec![LogicalMonitorBackup {
                x: 0,
                y: 0,
                scale: 1.0,
                transform: 0,
                primary: true,
                monitors: vec![("eDP-1".to_string(), "S1".to_string())],
            }],
            primary_output: None,
            configuration_hash: 0,
        };
        assert!(to_write_side(&backup).is_err());
    }

    #[test]
    fn zero_physical_config_has_exactly_one_logical_monitor_for_the_virtual_connector() {
        let config = zero_physical_config("REMOTE-0", "m9");
        assert_eq!(config.len(), 1);
        assert_eq!(config[0].monitors.len(), 1);
        assert_eq!(config[0].monitors[0].connector, "REMOTE-0");
        assert_eq!(config[0].monitors[0].mode_id, "m9");
    }
}
