//! Experiment 6 — Physical Output Isolation (Document 10 §13 / Document 02
//! §12): with a virtual monitor already active, disable every physical
//! output by applying a logical-monitors array containing **only** the
//! virtual connector's entry, and verify no physical connector remains part
//! of the active desktop topology (Doc 05 §33–34: the connector can still
//! appear in the raw `monitors[]` inventory — only its `logical_monitors[]`
//! membership is checked here).
//!
//! Persists a `DisplayBackup` JSON so a separate process (`exp07_restore`)
//! can independently restore — `--pause-after-isolate` exists specifically
//! so an operator can `kill -9` this process and observe what Mutter does
//! on its own before `exp07` runs (assessment §7.3's crash-recovery
//! question; Phase 5 plan Decision 1). An external `systemd-run` watchdog
//! (`docs/ops/experiment-safety.md` §2) is armed before the first real
//! apply and is the only mechanism that protects the operator if this
//! process is killed ungracefully — `RestoreGuard`'s `Drop` does not run on
//! `SIGKILL`.
//!
//! Requires `gnome-remote-desktop.service` masked (`experiment-safety.md`
//! §5) and the operator physically present with the SSH out-of-band
//! channel (§1) ready before any real (non-`--pause-after-isolate`-probe)
//! run.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, current_uid, discover, evidence_dir, redact,
    write_evidence,
};
use blackroom_gnome::mutter::display_config::{self, OutputBackup as CanonicalOutputBackup};
use clap::Parser;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Type, Value};

const EXP_ID: &str = "exp06";
const SIGNAL_WAIT: Duration = Duration::from_secs(10);
const METHOD_TEMPORARY: u32 = 1;
/// Doc 19 §16–17 / `experiment-safety.md` §6: bound each reliability-loop
/// cycle so a hang fails the run instead of blocking indefinitely.
const PER_CYCLE_TIMEOUT: Duration = Duration::from_secs(10);
/// `experiment-safety.md` §2 default watchdog window.
const WATCHDOG_SECONDS_DEFAULT: u64 = 45;

#[derive(Parser, Debug)]
#[command(
    about = "Experiment 6: disable every physical output, leaving only the virtual monitor active"
)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Reliability-loop repetitions (Phase 5 plan step 10 uses 50 on one row).
    #[arg(long, default_value_t = 1)]
    cycles: u32,
    /// After the first successful isolate, print this process's PID and the
    /// backup path, then block on stdin instead of restoring — for the
    /// deliberate ungraceful-termination scenario. Ignores `--cycles`.
    #[arg(long, default_value_t = false)]
    pause_after_isolate: bool,
    /// Pause-only restore timer in seconds; default 45, maximum 120.
    #[arg(long, value_parser = clap::value_parser!(u64).range(45..=120), requires = "pause_after_isolate")]
    watchdog_seconds: Option<u64>,
    /// Kill only this process after verified HDMI-only isolation and persisted
    /// pre-kill evidence. May crash GNOME; requires separate live approval.
    #[arg(long, requires_all = ["pause_after_isolate", "watchdog_seconds"])]
    auto_kill_after_isolate: bool,
}

impl Args {
    fn watchdog_duration(&self) -> u64 {
        if self.pause_after_isolate {
            self.watchdog_seconds.unwrap_or(WATCHDOG_SECONDS_DEFAULT)
        } else {
            WATCHDOG_SECONDS_DEFAULT.max(u64::from(self.cycles) * 15 + 30)
        }
    }
}

fn require_remote_desktop_masked() -> anyhow::Result<()> {
    let output = Command::new("systemctl")
        .args(["--user", "is-enabled", "gnome-remote-desktop.service"])
        .output()?;
    let state = String::from_utf8(output.stdout)?;
    anyhow::ensure!(
        matches!(state.trim(), "masked" | "masked-runtime"),
        "gnome-remote-desktop.service must be masked before exp06"
    );
    let active = Command::new("systemctl")
        .args([
            "--user",
            "is-active",
            "--quiet",
            "gnome-remote-desktop.service",
        ])
        .status()?;
    anyhow::ensure!(
        !active.success(),
        "gnome-remote-desktop.service must be inactive before exp06"
    );
    Ok(())
}

#[derive(Debug, Serialize)]
struct AutoKillPreflight {
    original_logical_connectors: Vec<String>,
    raw_connectors: Vec<String>,
    active_logical_connectors: Vec<String>,
    logical_monitor_count: usize,
    virtual_connector: String,
    power_save_mode: i32,
    shell_pid_before: u32,
    shell_pid_now: u32,
    timer_active: bool,
    arm_elapsed_ms: u128,
}

impl AutoKillPreflight {
    fn ready(&self) -> bool {
        self.original_logical_connectors == ["HDMI-1"]
            && self.raw_connectors.iter().any(|name| name == "HDMI-1")
            && self.logical_monitor_count == 1
            && self.active_logical_connectors == [self.virtual_connector.as_str()]
            && self.power_save_mode == POWER_SAVE_OFF
            && self.shell_pid_before == self.shell_pid_now
            && self.timer_active
            && self.arm_elapsed_ms < 10_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_preflight() -> AutoKillPreflight {
        AutoKillPreflight {
            original_logical_connectors: vec!["HDMI-1".to_string()],
            raw_connectors: vec![
                "eDP-1".to_string(),
                "HDMI-1".to_string(),
                "Meta-0".to_string(),
            ],
            active_logical_connectors: vec!["Meta-0".to_string()],
            logical_monitor_count: 1,
            virtual_connector: "Meta-0".to_string(),
            power_save_mode: POWER_SAVE_OFF,
            shell_pid_before: 1000,
            shell_pid_now: 1000,
            timer_active: true,
            arm_elapsed_ms: 500,
        }
    }

    #[test]
    fn automatic_kill_requires_an_explicit_bounded_pause_mode() {
        assert!(Args::try_parse_from(["exp06", "--auto-kill-after-isolate"]).is_err());
        assert!(
            Args::try_parse_from([
                "exp06",
                "--pause-after-isolate",
                "--auto-kill-after-isolate"
            ])
            .is_err()
        );
        let args = Args::try_parse_from([
            "exp06",
            "--pause-after-isolate",
            "--watchdog-seconds",
            "90",
            "--auto-kill-after-isolate",
        ])
        .unwrap();
        assert!(args.auto_kill_after_isolate);
    }

    #[test]
    fn automatic_kill_preflight_fails_closed_on_each_missing_condition() {
        assert!(valid_preflight().ready());
        let mut preflight = valid_preflight();
        preflight
            .original_logical_connectors
            .push("eDP-1".to_string());
        assert!(!preflight.ready());
        let mut preflight = valid_preflight();
        preflight.raw_connectors.retain(|name| name != "HDMI-1");
        assert!(!preflight.ready());
        let mut preflight = valid_preflight();
        preflight.logical_monitor_count = 2;
        assert!(!preflight.ready());
        let mut preflight = valid_preflight();
        preflight
            .active_logical_connectors
            .push("HDMI-1".to_string());
        assert!(!preflight.ready());
        let mut preflight = valid_preflight();
        preflight.power_save_mode = POWER_SAVE_ON;
        assert!(!preflight.ready());
        let mut preflight = valid_preflight();
        preflight.shell_pid_now = 1001;
        assert!(!preflight.ready());
        let mut preflight = valid_preflight();
        preflight.timer_active = false;
        assert!(!preflight.ready());
        let mut preflight = valid_preflight();
        preflight.arm_elapsed_ms = 10_000;
        assert!(!preflight.ready());
    }

    #[test]
    fn pause_watchdog_override_is_explicit_and_bounded() {
        let pause =
            Args::try_parse_from(["exp06", "--pause-after-isolate", "--cycles", "50"]).unwrap();
        assert_eq!(pause.watchdog_duration(), 45);

        let extended =
            Args::try_parse_from(["exp06", "--pause-after-isolate", "--watchdog-seconds", "90"])
                .unwrap();
        assert_eq!(extended.watchdog_duration(), 90);

        let repeated = Args::try_parse_from(["exp06", "--cycles", "50"]).unwrap();
        assert_eq!(repeated.watchdog_duration(), 780);

        for seconds in ["44", "121"] {
            assert!(
                Args::try_parse_from([
                    "exp06",
                    "--pause-after-isolate",
                    "--watchdog-seconds",
                    seconds,
                ])
                .is_err()
            );
        }
        assert!(Args::try_parse_from(["exp06", "--watchdog-seconds", "90"]).is_err());
    }

    #[test]
    fn final_topology_rejects_extra_hdmi_after_virtual_monitor_stop() {
        let backup = DisplayBackup {
            session_id: "3".to_string(),
            shell_pid: 34735,
            outputs: Vec::new(),
            topology: vec![LogicalMonitorBackup {
                x: 0,
                y: 0,
                scale: 1.0,
                transform: 0,
                primary: true,
                monitors: vec![("eDP-1".to_string(), "internal".to_string())],
            }],
            primary_output: Some(("eDP-1".to_string(), "internal".to_string())),
            hash_version: display_config::CONFIGURATION_HASH_VERSION,
            configuration_hash: output_hash(&[]),
        };
        let original = LogicalMonitorEntry {
            x: 0,
            y: 0,
            scale: 1.0,
            transform: 0,
            primary: true,
            monitors: vec![ConnectorInfo {
                connector: "eDP-1".to_string(),
                vendor: String::new(),
                product: String::new(),
                serial: "internal".to_string(),
            }],
            properties: HashMap::new(),
        };
        assert!(original_topology_matches(
            &backup,
            std::slice::from_ref(&original)
        ));
        let hdmi = LogicalMonitorEntry {
            x: 1920,
            y: 0,
            scale: 1.0,
            transform: 0,
            primary: false,
            monitors: vec![ConnectorInfo {
                connector: "HDMI-1".to_string(),
                vendor: String::new(),
                product: String::new(),
                serial: "external".to_string(),
            }],
            properties: HashMap::new(),
        };
        assert!(!original_topology_matches(
            &backup,
            std::slice::from_ref(&hdmi)
        ));
        assert!(!original_topology_matches(&backup, &[original, hdmi]));
        assert!(!cleanup_verified(true, false, true, true));
    }

    #[test]
    fn new_backup_hash_tracks_hdmi_enabled() {
        let monitor = |connector: &str, serial: &str| MonitorEntry {
            connector_info: ConnectorInfo {
                connector: connector.into(),
                vendor: "vendor".into(),
                product: "screen".into(),
                serial: serial.into(),
            },
            modes: vec![ModeInfo {
                id: "mode-1".into(),
                width: 1920,
                height: 1080,
                refresh_rate: 60.0,
                preferred_scale: 1.0,
                supported_scales: vec![1.0],
                properties: HashMap::from([(
                    "is-current".into(),
                    OwnedValue::try_from(Value::from(true)).unwrap(),
                )]),
            }],
            properties: HashMap::new(),
        };
        let monitors = vec![monitor("eDP-1", "internal"), monitor("HDMI-1", "external")];
        let original = LogicalMonitorEntry {
            x: 0,
            y: 0,
            scale: 1.0,
            transform: 0,
            primary: true,
            monitors: vec![monitors[0].connector_info.clone()],
            properties: HashMap::new(),
        };
        let backup = build_backup(&monitors, std::slice::from_ref(&original), "3", 34735);
        assert_eq!(
            backup.primary_output,
            Some(("eDP-1".into(), "internal".into()))
        );
        assert_eq!(backup.outputs.len(), 2);
        assert!(backup.outputs[0].enabled);
        assert!(!backup.outputs[1].enabled);
        let json = serde_json::to_value(&backup).unwrap();
        assert!(json["configuration_hash"].is_u64());
        assert_eq!(json["outputs"][1]["enabled"], false);
        assert_eq!(
            current_output_hash(&backup, &monitors, std::slice::from_ref(&original)),
            Some(backup.configuration_hash)
        );
        let mut wrong_mode = monitors.clone();
        wrong_mode[0].modes[0].width = 2560;
        assert_ne!(
            current_output_hash(&backup, &wrong_mode, std::slice::from_ref(&original)),
            Some(backup.configuration_hash)
        );
        let mut with_hdmi = vec![original];
        with_hdmi.push(LogicalMonitorEntry {
            x: 1920,
            y: 0,
            scale: 1.0,
            transform: 0,
            primary: false,
            monitors: vec![monitors[1].connector_info.clone()],
            properties: HashMap::new(),
        });
        assert!(!original_topology_matches(&backup, &with_hdmi));
        assert_ne!(
            current_output_hash(&backup, &monitors, &with_hdmi),
            Some(backup.configuration_hash)
        );
        let mut no_hdmi_mode = monitors.clone();
        no_hdmi_mode[1].modes.clear();
        let disabled_backup = build_backup(
            &no_hdmi_mode,
            std::slice::from_ref(&with_hdmi[0]),
            "3",
            34735,
        );
        assert_eq!(disabled_backup.outputs.len(), 2);
        assert_eq!(disabled_backup.outputs[1].mode_id, "");
        assert!(!disabled_backup.outputs[1].enabled);
        assert_eq!(
            current_output_hash(&disabled_backup, &no_hdmi_mode, &with_hdmi[..1]),
            Some(disabled_backup.configuration_hash)
        );
        assert_ne!(
            current_output_hash(&disabled_backup, &no_hdmi_mode, &with_hdmi),
            Some(disabled_backup.configuration_hash)
        );
    }

    #[test]
    fn restore_guard_disarms_only_after_verified_owner_stop() {
        assert!(cleanup_verified(true, true, true, true));
        for (prestop_restored, final_topology_restored, stop_succeeded, virtual_gone) in [
            (false, true, true, true),
            (true, false, true, true),
            (true, true, false, true),
            (true, true, true, false),
        ] {
            assert!(!cleanup_verified(
                prestop_restored,
                final_topology_restored,
                stop_succeeded,
                virtual_gone
            ));
        }
    }

    #[test]
    fn post_stop_repair_requires_original_identity_and_watchdog() {
        let backup = DisplayBackup {
            session_id: "3".into(),
            shell_pid: 34735,
            outputs: Vec::new(),
            topology: Vec::new(),
            primary_output: None,
            hash_version: display_config::CONFIGURATION_HASH_VERSION,
            configuration_hash: output_hash(&[]),
        };
        let mut observed = FinalState {
            shell_pid: Some(34735),
            session_id: Some("3".into()),
            watchdog_timer_active: Some(true),
            watchdog_service_state: Some("inactive".into()),
            ..FinalState::default()
        };
        assert!(should_repair_post_stop(&observed, &backup, true, true));
        assert!(!should_repair_post_stop(&observed, &backup, false, true));
        assert!(!should_repair_post_stop(&observed, &backup, true, false));
        observed.watchdog_timer_active = Some(false);
        assert!(!should_repair_post_stop(&observed, &backup, true, true));
        observed.watchdog_timer_active = Some(true);
        for state in ["activating", "active", "failed"] {
            observed.watchdog_service_state = Some(state.into());
            assert!(!should_repair_post_stop(&observed, &backup, true, true));
        }
        observed.watchdog_service_state = None;
        assert!(!should_repair_post_stop(&observed, &backup, true, true));
        observed.watchdog_service_state = Some("inactive".into());
        observed.shell_pid = Some(34736);
        assert!(!should_repair_post_stop(&observed, &backup, true, true));
        observed.shell_pid = Some(34735);
        observed.session_id = Some("4".into());
        assert!(!should_repair_post_stop(&observed, &backup, true, true));
        observed.session_id = Some("3".into());
        observed.topology_matches_original = true;
        assert!(!should_repair_post_stop(&observed, &backup, true, true));
        observed.topology_matches_original = false;
        observed.read_error = Some("unreadable".into());
        assert!(!should_repair_post_stop(&observed, &backup, true, true));
    }

    #[test]
    fn post_stop_repair_still_reports_failure_after_a_verified_restore() {
        assert!(matches!(
            diagnostic_result(true, true, true, true),
            ExperimentResult::Fail
        ));
        assert!(matches!(
            diagnostic_result(true, false, true, true),
            ExperimentResult::Partial
        ));
        assert!(matches!(
            diagnostic_result(false, false, true, true),
            ExperimentResult::Pass
        ));
    }

    #[test]
    fn post_stop_repair_preserves_the_observed_mismatch() {
        let observed = FinalState {
            raw_connectors: vec!["eDP-1".into(), "HDMI-1".into()],
            logical_connectors: vec!["eDP-1".into(), "HDMI-1".into()],
            configuration_hash_matches: Some(false),
            ..FinalState::default()
        };
        let captured = pre_repair_state(&observed, true).unwrap();
        let fields = serde_json::to_value(captured).unwrap();
        assert_eq!(fields["logical_connectors"][1], "HDMI-1");
        assert_eq!(fields["configuration_hash_matches"], false);
        assert!(pre_repair_state(&observed, false).is_none());
        assert_eq!(observed.logical_connectors.len(), 2);
    }

    #[test]
    fn local_restore_refuses_changed_session_or_shell() {
        assert!(verify_restore_identity("3", 34735, "3", 34735).is_ok());
        assert!(verify_restore_identity("3", 34735, "4", 34735).is_err());
        assert!(verify_restore_identity("3", 34735, "3", 34736).is_err());
        assert!(verify_restore_identity("", 34735, "", 34735).is_err());
    }
}

// ---------------------------------------------------------------------
// DisplayConfig read/write wire types (per-file duplication convention —
// see exp05_virtual_active.rs; Phase 5 plan Decision 10).
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Type, Deserialize)]
struct ConnectorInfo {
    connector: String,
    vendor: String,
    product: String,
    serial: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Type, Deserialize)]
struct ModeInfo {
    id: String,
    width: i32,
    height: i32,
    refresh_rate: f64,
    preferred_scale: f64,
    supported_scales: Vec<f64>,
    properties: HashMap<String, OwnedValue>,
}

#[derive(Debug, Clone, Type, Deserialize)]
struct MonitorEntry {
    connector_info: ConnectorInfo,
    modes: Vec<ModeInfo>,
    #[allow(dead_code)]
    properties: HashMap<String, OwnedValue>,
}

#[derive(Debug, Clone, Type, Deserialize)]
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

fn is_current_mode(properties: &HashMap<String, OwnedValue>) -> bool {
    properties
        .get("is-current")
        .and_then(|value| bool::try_from(value.clone()).ok())
        .unwrap_or(false)
}

fn display_config_proxy(conn: &Connection) -> anyhow::Result<Proxy<'_>> {
    Ok(Proxy::new(
        conn,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )?)
}

fn read_state(
    conn: &Connection,
) -> anyhow::Result<(u32, Vec<MonitorEntry>, Vec<LogicalMonitorEntry>)> {
    let (serial, monitors, logical_monitors, _props): GetCurrentStateResult =
        display_config_proxy(conn)?.call("GetCurrentState", &())?;
    Ok((serial, monitors, logical_monitors))
}

/// Doc 05 §30: validate before disabling, don't discover incompatibility
/// afterward.
fn apply_monitors_config_allowed(conn: &Connection) -> anyhow::Result<bool> {
    Ok(display_config_proxy(conn)?.get_property::<bool>("ApplyMonitorsConfigAllowed")?)
}

/// DPMS levels (standard: 0=ON, 3=OFF; confirmed live 2026-09-05).
const POWER_SAVE_ON: i32 = 0;
const POWER_SAVE_OFF: i32 = 3;

/// Forces a real hardware blank/wake, global across every output (Doc 05
/// §34: found live 2026-09-05 that a bare `ApplyMonitorsConfig`
/// zero-physical disable left `eDP-1` showing a frozen, fully visible
/// mid-reflow frame rather than blanking — undetectable by `GetCurrentState`
/// alone).
fn set_power_save_mode(conn: &Connection, mode: i32) -> anyhow::Result<()> {
    Ok(display_config_proxy(conn)?.set_property("PowerSaveMode", mode)?)
}

// ---------------------------------------------------------------------
// ApplyMonitorsConfig write-side types.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Type, Serialize)]
struct MonitorRef {
    connector: String,
    mode_id: String,
    properties: HashMap<String, OwnedValue>,
}

#[derive(Debug, Clone, Type, Serialize)]
struct LogicalMonitorConfig {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: Vec<MonitorRef>,
}

fn apply_monitors_config(
    conn: &Connection,
    serial: u32,
    logical_monitors: &[LogicalMonitorConfig],
) -> anyhow::Result<()> {
    let empty_props: HashMap<&str, Value<'_>> = HashMap::new();
    display_config_proxy(conn)?.call::<_, _, ()>(
        "ApplyMonitorsConfig",
        &(
            serial,
            METHOD_TEMPORARY,
            logical_monitors.to_vec(),
            empty_props,
        ),
    )?;
    Ok(())
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

fn physical_connectors_active(
    logical: &[LogicalMonitorEntry],
    virtual_connector: &str,
) -> Vec<String> {
    logical
        .iter()
        .flat_map(|lm| lm.monitors.iter().map(|c| c.connector.clone()))
        .filter(|c| c != virtual_connector)
        .collect()
}

// ---------------------------------------------------------------------
// DisplayBackup — persisted JSON (Doc 05 §29). Local to this binary by
// established convention (Phase 5 plan Decision 10) — not imported from
// `blackroom_gnome::mutter::display_config`.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OutputBackup {
    connector: String,
    vendor: String,
    product: String,
    serial: String,
    mode_id: String,
    width: i32,
    height: i32,
    refresh_rate: f64,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogicalMonitorBackup {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DisplayBackup {
    session_id: String,
    shell_pid: u32,
    outputs: Vec<OutputBackup>,
    topology: Vec<LogicalMonitorBackup>,
    primary_output: Option<(String, String)>,
    hash_version: u32,
    configuration_hash: u64,
}

fn output_hash(outputs: &[OutputBackup]) -> u64 {
    let canonical: Vec<_> = outputs
        .iter()
        .map(|output| CanonicalOutputBackup {
            connector: output.connector.clone(),
            vendor: output.vendor.clone(),
            product: output.product.clone(),
            serial: output.serial.clone(),
            mode_id: output.mode_id.clone(),
            width: output.width,
            height: output.height,
            refresh_rate: output.refresh_rate,
            enabled: output.enabled,
        })
        .collect();
    display_config::compute_hash(&canonical)
}

fn current_output_hash(
    backup: &DisplayBackup,
    monitors: &[MonitorEntry],
    logical: &[LogicalMonitorEntry],
) -> Option<u64> {
    let outputs: Vec<_> = backup
        .outputs
        .iter()
        .map(|saved| {
            let monitor = monitors.iter().find(|monitor| {
                monitor.connector_info.connector == saved.connector
                    && monitor.connector_info.serial == saved.serial
            })?;
            let enabled = logical.iter().any(|entry| {
                entry.monitors.iter().any(|identity| {
                    identity.connector == saved.connector && identity.serial == saved.serial
                })
            });
            let current_mode = monitor
                .modes
                .iter()
                .find(|mode| is_current_mode(&mode.properties));
            let (mode_id, width, height, refresh_rate) = match current_mode {
                Some(mode) => (mode.id.clone(), mode.width, mode.height, mode.refresh_rate),
                None if !enabled && !saved.enabled => (
                    saved.mode_id.clone(),
                    saved.width,
                    saved.height,
                    saved.refresh_rate,
                ),
                None => return None,
            };
            Some(CanonicalOutputBackup {
                connector: monitor.connector_info.connector.clone(),
                vendor: monitor.connector_info.vendor.clone(),
                product: monitor.connector_info.product.clone(),
                serial: monitor.connector_info.serial.clone(),
                mode_id,
                width,
                height,
                refresh_rate,
                enabled,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(display_config::compute_hash(&outputs))
}

fn build_backup(
    monitors: &[MonitorEntry],
    logical: &[LogicalMonitorEntry],
    session_id: &str,
    shell_pid: u32,
) -> DisplayBackup {
    let topology: Vec<LogicalMonitorBackup> = logical
        .iter()
        .map(|lm| LogicalMonitorBackup {
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
        .collect();
    let outputs = monitors
        .iter()
        .filter_map(|m| {
            let enabled = topology.iter().any(|lm| {
                lm.monitors.iter().any(|(connector, serial)| {
                    connector == &m.connector_info.connector && serial == &m.connector_info.serial
                })
            });
            let (mode_id, width, height, refresh_rate) = match m
                .modes
                .iter()
                .find(|mode| is_current_mode(&mode.properties))
            {
                Some(mode) => (mode.id.clone(), mode.width, mode.height, mode.refresh_rate),
                None if !enabled => (String::new(), 0, 0, 0.0),
                None => return None,
            };
            Some(OutputBackup {
                connector: m.connector_info.connector.clone(),
                vendor: m.connector_info.vendor.clone(),
                product: m.connector_info.product.clone(),
                serial: m.connector_info.serial.clone(),
                mode_id,
                width,
                height,
                refresh_rate,
                enabled,
            })
        })
        .collect::<Vec<_>>();
    let primary_output = topology
        .iter()
        .find(|lm| lm.primary)
        .and_then(|lm| lm.monitors.first().cloned());
    let configuration_hash = output_hash(&outputs);
    DisplayBackup {
        session_id: session_id.to_string(),
        shell_pid,
        outputs,
        topology,
        primary_output,
        hash_version: display_config::CONFIGURATION_HASH_VERSION,
        configuration_hash,
    }
}

fn original_topology_matches(backup: &DisplayBackup, logical: &[LogicalMonitorEntry]) -> bool {
    logical.len() == backup.topology.len()
        && backup.topology.iter().all(|expected| {
            logical.iter().any(|actual| {
                actual.x == expected.x
                    && actual.y == expected.y
                    && (actual.scale - expected.scale).abs() < f64::EPSILON
                    && actual.transform == expected.transform
                    && actual.primary == expected.primary
                    && actual.monitors.len() == expected.monitors.len()
                    && expected.monitors.iter().all(|(connector, serial)| {
                        actual.monitors.iter().any(|current| {
                            &current.connector == connector && &current.serial == serial
                        })
                    })
            })
        })
}

/// Reuses the mode IDs captured at snapshot time rather than re-resolving
/// them (Experiment 5 precedent: only the `serial` is re-read fresh).
fn to_write_side(backup: &DisplayBackup) -> anyhow::Result<Vec<LogicalMonitorConfig>> {
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
                            anyhow::anyhow!(
                                "backup inconsistent: {connector}/{serial} missing from outputs[]"
                            )
                        })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
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

// ---------------------------------------------------------------------
// RAII cleanup — identical pattern to exp05_virtual_active.rs. Provides no
// protection against SIGKILL (Drop does not run); the external watchdog
// covers that case.
// ---------------------------------------------------------------------

struct RestoreGuard<'a> {
    conn: &'a Connection,
    original: Vec<LogicalMonitorConfig>,
    session_id: String,
    shell_pid: u32,
    disarmed: bool,
}

impl RestoreGuard<'_> {
    fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for RestoreGuard<'_> {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }
        if let Err(error) =
            restore_original(self.conn, &self.session_id, self.shell_pid, &self.original)
        {
            eprintln!("CRITICAL: RestoreGuard failed to restore original display state: {error}");
        }
    }
}

struct SessionStopGuard<'a> {
    conn: &'a Connection,
    path: OwnedObjectPath,
    disarmed: bool,
}

impl SessionStopGuard<'_> {
    fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for SessionStopGuard<'_> {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }
        let result = Proxy::new(
            self.conn,
            "org.gnome.Mutter.ScreenCast",
            self.path.clone(),
            "org.gnome.Mutter.ScreenCast.Session",
        )
        .and_then(|proxy| proxy.call::<_, _, ()>("Stop", &()));
        if let Err(error) = result {
            eprintln!(
                "CRITICAL: SessionStopGuard failed to stop ScreenCast session {}: {error}",
                self.path
            );
        }
    }
}

// ---------------------------------------------------------------------
// Virtual monitor creation (Experiment 4/5's proven mechanics, duplicated
// per this project's established per-experiment-binary pattern).
// ---------------------------------------------------------------------

mod pipewire_probe {
    // Identical to Experiment 4/5's probe — duplicated per this project's
    // established per-experiment-binary pattern (architecture.md §1).
    use pipewire as pw;
    use pw::properties::properties;
    use pw::spa;
    use pw::spa::pod::Pod;

    pub fn receive_a_few_frames(
        node_id: u32,
        preferred_width: i32,
        preferred_height: i32,
    ) -> anyhow::Result<u32> {
        pw::init();
        let mainloop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;

        let frame_count = std::rc::Rc::new(std::cell::Cell::new(0_u32));
        struct UserData {
            mainloop: pw::main_loop::MainLoopRc,
            frame_count: std::rc::Rc<std::cell::Cell<u32>>,
        }
        let data = UserData {
            mainloop: mainloop.clone(),
            frame_count: frame_count.clone(),
        };
        let stream = pw::stream::StreamBox::new(
            &core,
            "blackroom-exp06-probe",
            properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
            },
        )?;
        let _listener = stream
            .add_local_listener_with_user_data(data)
            .process(|stream, user_data| {
                if stream.dequeue_buffer().is_some() {
                    let count = user_data.frame_count.get() + 1;
                    user_data.frame_count.set(count);
                    if count >= 2 {
                        user_data.mainloop.quit();
                    }
                }
            })
            .register()?;

        let obj = spa::pod::object!(
            spa::utils::SpaTypes::ObjectParamFormat,
            spa::param::ParamType::EnumFormat,
            spa::pod::property!(
                spa::param::format::FormatProperties::MediaType,
                Id,
                spa::param::format::MediaType::Video
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::MediaSubtype,
                Id,
                spa::param::format::MediaSubtype::Raw
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::VideoFormat,
                Choice,
                Enum,
                Id,
                spa::param::video::VideoFormat::RGBx,
                spa::param::video::VideoFormat::RGBx,
                spa::param::video::VideoFormat::BGRx,
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::VideoSize,
                Choice,
                Range,
                Rectangle,
                spa::utils::Rectangle {
                    width: preferred_width as u32,
                    height: preferred_height as u32
                },
                spa::utils::Rectangle {
                    width: 1,
                    height: 1
                },
                spa::utils::Rectangle {
                    width: 7680,
                    height: 4320
                }
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::VideoFramerate,
                Choice,
                Range,
                Fraction,
                spa::utils::Fraction { num: 60, denom: 1 },
                spa::utils::Fraction { num: 0, denom: 1 },
                spa::utils::Fraction {
                    num: 1000,
                    denom: 1
                }
            ),
        );
        let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &spa::pod::Value::Object(obj),
        )?
        .0
        .into_inner();
        let mut params =
            [Pod::from_bytes(&values).ok_or_else(|| anyhow::anyhow!("bad format pod"))?];
        stream.connect(
            spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )?;

        let quit_on_timeout = mainloop.clone();
        let timer = mainloop.loop_().add_timer(move |_| quit_on_timeout.quit());
        timer
            .update_timer(Some(std::time::Duration::from_secs(5)), None)
            .into_result()?;

        mainloop.run();
        stream.disconnect()?;
        Ok(frame_count.get())
    }
}

fn wait_for_pipewire_node(
    session_proxy: &Proxy<'_>,
    stream_proxy: &Proxy<'_>,
) -> anyhow::Result<u32> {
    let mut signal_iter = stream_proxy.receive_signal("PipeWireStreamAdded")?;
    session_proxy.call::<_, _, ()>("Start", &())?;
    let node_id = thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel();
        scope.spawn(move || {
            if let Some(msg) = signal_iter.next() {
                let _ = tx.send(msg.body().deserialize::<(u32,)>().ok());
            }
        });
        rx.recv_timeout(SIGNAL_WAIT).ok().flatten()
    });
    node_id
        .map(|(id,)| id)
        .ok_or_else(|| anyhow::anyhow!("PipeWireStreamAdded not received within timeout"))
}

fn poll_for_new_connector(
    conn: &Connection,
    before: &[String],
) -> anyhow::Result<(String, Vec<MonitorEntry>)> {
    let deadline = Instant::now() + SIGNAL_WAIT;
    loop {
        let (_serial, monitors, _logical) = read_state(conn)?;
        if let Some(connector) = monitors
            .iter()
            .map(|m| m.connector_info.connector.clone())
            .find(|c| !before.contains(c))
        {
            return Ok((connector, monitors));
        }
        if Instant::now() >= deadline {
            anyhow::bail!("no new connector appeared in GetCurrentState within timeout");
        }
        thread::sleep(Duration::from_millis(150));
    }
}

fn poll_for_connector_gone(conn: &Connection, connector: &str) -> anyhow::Result<Vec<String>> {
    let deadline = Instant::now() + SIGNAL_WAIT;
    loop {
        let (_serial, monitors, _logical) = read_state(conn)?;
        let names: Vec<String> = monitors
            .iter()
            .map(|m| m.connector_info.connector.clone())
            .collect();
        if !names.iter().any(|c| c == connector) || Instant::now() >= deadline {
            return Ok(names);
        }
        thread::sleep(Duration::from_millis(150));
    }
}

fn current_mode_id(monitors: &[MonitorEntry], connector: &str) -> Option<String> {
    monitors
        .iter()
        .find(|m| m.connector_info.connector == connector)
        .and_then(|m| {
            m.modes
                .iter()
                .find(|mode| is_current_mode(&mode.properties))
        })
        .map(|mode| mode.id.clone())
}

/// Creates and confirms a virtual monitor (Experiment 4/5 mechanics),
/// returning its connector name plus the guards the caller must keep alive
/// (and eventually disarm/drop in the right order).
fn create_virtual_monitor<'a>(
    conn: &'a Connection,
    physical_connectors_before: &[String],
) -> anyhow::Result<(String, SessionStopGuard<'a>)> {
    let screencast_proxy = Proxy::new(
        conn,
        "org.gnome.Mutter.ScreenCast",
        "/org/gnome/Mutter/ScreenCast",
        "org.gnome.Mutter.ScreenCast",
    )?;
    let session_path: OwnedObjectPath =
        screencast_proxy.call("CreateSession", &(HashMap::<&str, Value<'_>>::new(),))?;
    let mut session_guard = SessionStopGuard {
        conn,
        path: session_path.clone(),
        disarmed: false,
    };
    let session_proxy = Proxy::new(
        conn,
        "org.gnome.Mutter.ScreenCast",
        session_path.as_ref(),
        "org.gnome.Mutter.ScreenCast.Session",
    )?;
    let (width, height, refresh_rate) = (1920_i32, 1080_i32, 60.0_f64);
    let record_props: HashMap<&str, Value<'_>> = HashMap::from([
        ("width", Value::from(width)),
        ("height", Value::from(height)),
        ("framerate", Value::from(refresh_rate)),
    ]);
    let stream_path: OwnedObjectPath = session_proxy.call("RecordVirtual", &(record_props,))?;
    let stream_proxy = Proxy::new(
        conn,
        "org.gnome.Mutter.ScreenCast",
        stream_path.as_ref(),
        "org.gnome.Mutter.ScreenCast.Stream",
    )?;
    let node_id = wait_for_pipewire_node(&session_proxy, &stream_proxy)?;
    pipewire_probe::receive_a_few_frames(node_id, width, height).ok();
    let (virtual_connector, _monitors) = poll_for_new_connector(conn, physical_connectors_before)?;
    session_guard.disarmed = false; // stays armed; caller owns disarm/drop ordering
    Ok((virtual_connector, session_guard))
}

// ---------------------------------------------------------------------
// Watchdog (experiment-safety.md §2) — armed via `systemd-run --user`
// before any real physical-output disable, disarmed only after the
// operator/caller has verified recovery.
// ---------------------------------------------------------------------

fn exp07_restore_path() -> anyhow::Result<PathBuf> {
    let mut path = std::env::current_exe()?;
    path.set_file_name(if cfg!(windows) {
        "exp07_restore.exe"
    } else {
        "exp07_restore"
    });
    Ok(path)
}

fn arm_watchdog(
    unit_name: &str,
    seconds: u64,
    backup_path: &std::path::Path,
) -> anyhow::Result<()> {
    let restore_bin = exp07_restore_path()?;
    // Absolute: without an explicit --working-directory, the transient
    // unit's cwd defaults to $HOME, which silently misdirects exp07_restore's
    // own relative evidence_dir() writes (found live 2026-09-05 — watchdog-
    // triggered restores were writing evidence outside the repo entirely).
    let cwd = std::env::current_dir()?;
    let status = Command::new("systemd-run")
        .args([
            "--user".to_string(),
            // systemd's default 1 min accuracy lets this timer fire up to a minute late.
            "--timer-property=AccuracySec=1s".to_string(),
            format!("--unit={unit_name}"),
            format!("--on-active={seconds}s"),
            format!("--working-directory={}", cwd.display()),
            "--".to_string(),
            restore_bin.display().to_string(),
            "--backup".to_string(),
            backup_path.display().to_string(),
        ])
        .status()?;
    anyhow::ensure!(
        status.success(),
        "systemd-run failed to arm the restore watchdog"
    );
    Ok(())
}

/// Best-effort: the timer may have already fired and self-cleaned.
fn disarm_watchdog(unit_name: &str) {
    let _ = Command::new("systemctl")
        .args(["--user", "stop", &format!("{unit_name}.timer")])
        .status();
}

fn gnome_shell_pid(conn: &Connection) -> anyhow::Result<u32> {
    let proxy = zbus::blocking::fdo::DBusProxy::new(conn)?;
    Ok(
        proxy.get_connection_unix_process_id(zbus::names::BusName::try_from(
            "org.gnome.Mutter.ScreenCast",
        )?)?,
    )
}

fn verify_restore_identity(
    expected_session_id: &str,
    expected_shell_pid: u32,
    current_session_id: &str,
    current_shell_pid: u32,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !expected_session_id.is_empty()
            && expected_shell_pid != 0
            && expected_session_id == current_session_id
            && expected_shell_pid == current_shell_pid,
        "refusing display backup from a different GNOME session or Shell process"
    );
    Ok(())
}

fn verify_live_restore_identity(
    conn: &Connection,
    expected_session_id: &str,
    expected_shell_pid: u32,
) -> anyhow::Result<()> {
    let session_id = discover(current_uid()?)?
        .1
        .ok_or_else(|| anyhow::anyhow!("no unique active Wayland user session"))?;
    verify_restore_identity(
        expected_session_id,
        expected_shell_pid,
        &session_id,
        gnome_shell_pid(conn)?,
    )
}

fn restore_original(
    conn: &Connection,
    session_id: &str,
    shell_pid: u32,
    original: &[LogicalMonitorConfig],
) -> anyhow::Result<()> {
    verify_live_restore_identity(conn, session_id, shell_pid)?;
    let (serial, ..) = read_state(conn)?;
    verify_live_restore_identity(conn, session_id, shell_pid)?;
    apply_monitors_config(conn, serial, original)?;
    verify_live_restore_identity(conn, session_id, shell_pid)?;
    set_power_save_mode(conn, POWER_SAVE_ON)
}

fn auto_kill_preflight(
    conn: &Connection,
    original_write_side: &[LogicalMonitorConfig],
    virtual_connector: &str,
    watchdog_unit: &str,
    shell_pid_before: u32,
    watchdog_started: Instant,
) -> anyhow::Result<AutoKillPreflight> {
    let (_serial, monitors, logical) = read_state(conn)?;
    let timer_active = Command::new("systemctl")
        .args([
            "--user",
            "is-active",
            "--quiet",
            &format!("{watchdog_unit}.timer"),
        ])
        .status()?
        .success();
    Ok(AutoKillPreflight {
        original_logical_connectors: original_write_side
            .iter()
            .flat_map(|monitor| monitor.monitors.iter().map(|item| item.connector.clone()))
            .collect(),
        raw_connectors: monitors
            .iter()
            .map(|monitor| monitor.connector_info.connector.clone())
            .collect(),
        active_logical_connectors: logical
            .iter()
            .flat_map(|monitor| monitor.monitors.iter().map(|item| item.connector.clone()))
            .collect(),
        logical_monitor_count: logical.len(),
        virtual_connector: virtual_connector.to_string(),
        power_save_mode: display_config_proxy(conn)?.get_property("PowerSaveMode")?,
        shell_pid_before,
        shell_pid_now: gnome_shell_pid(conn)?,
        timer_active,
        arm_elapsed_ms: watchdog_started.elapsed().as_millis(),
    })
}

// ---------------------------------------------------------------------
// Host info (Phase 5 plan Decision 5 — GPU-matrix fields folded in here
// rather than a dedicated Experiment 29 binary).
// ---------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct HostInfo {
    kernel: String,
    gpu_modules: Vec<String>,
}

fn host_info() -> HostInfo {
    const KNOWN_GPU_MODULES: [&str; 6] = ["nvidia", "nouveau", "i915", "amdgpu", "xe", "radeon"];
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_default()
        .trim()
        .to_string();
    let gpu_modules = std::fs::read_to_string("/proc/modules")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|module| KNOWN_GPU_MODULES.contains(module))
        .map(str::to_string)
        .collect();
    HostInfo {
        kernel,
        gpu_modules,
    }
}

#[derive(Debug, Serialize)]
struct CycleFindings {
    cycle: u32,
    duration_ms: u128,
    apply_error: Option<String>,
    physical_connectors_absent_from_logical_monitors: bool,
    restore_error: Option<String>,
    topology_restored: bool,
}

#[derive(Debug, Serialize)]
struct Findings {
    host: HostInfo,
    physical_connectors_before: Vec<String>,
    virtual_connector: Option<String>,
    apply_monitors_config_allowed: bool,
    watchdog_armed: bool,
    watchdog_unit: Option<String>,
    cleanup_watchdog_unit: Option<String>,
    cycles: Vec<CycleFindings>,
    stop_error: Option<String>,
    virtual_connector_fully_gone: bool,
    post_stop_restore_attempted: bool,
    post_stop_restore_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_stop_pre_repair_state: Option<FinalState>,
    paused_for_manual_kill_test: bool,
    final_state: Option<FinalState>,
}

#[derive(Clone, Debug, Default, Serialize)]
struct FinalState {
    raw_connectors: Vec<String>,
    logical_connectors: Vec<String>,
    power_save_mode: Option<i32>,
    shell_pid: Option<u32>,
    session_id: Option<String>,
    watchdog_timer_active: Option<bool>,
    watchdog_timer_state: Option<String>,
    watchdog_service_state: Option<String>,
    topology_matches_original: bool,
    configuration_hash_matches: Option<bool>,
    read_error: Option<String>,
    probe_errors: Vec<&'static str>,
}

fn pre_repair_state(observed: &FinalState, attempted: bool) -> Option<FinalState> {
    attempted.then(|| observed.clone())
}

fn should_repair_post_stop(
    observed: &FinalState,
    backup: &DisplayBackup,
    stop_succeeded: bool,
    virtual_gone: bool,
) -> bool {
    stop_succeeded
        && virtual_gone
        && !observed.topology_matches_original
        && observed.read_error.is_none()
        && observed.watchdog_timer_active == Some(true)
        && observed.watchdog_service_state.as_deref() == Some("inactive")
        && observed.shell_pid == Some(backup.shell_pid)
        && observed.session_id.as_deref() == Some(backup.session_id.as_str())
}

fn capture_final_state(
    conn: &Connection,
    backup: &DisplayBackup,
    watchdog_unit: &str,
) -> FinalState {
    let mut probe_errors = Vec::new();
    let power_save_mode = display_config_proxy(conn)
        .and_then(|proxy| {
            proxy
                .get_property::<i32>("PowerSaveMode")
                .map_err(anyhow::Error::from)
        })
        .ok();
    if power_save_mode.is_none() {
        probe_errors.push("PowerSaveMode unavailable");
    }
    let shell_pid = gnome_shell_pid(conn).ok();
    if shell_pid.is_none() {
        probe_errors.push("ScreenCast owner PID unavailable");
    }
    let session_id = current_uid()
        .and_then(|uid| discover(uid).map(|(_, selected)| selected))
        .ok()
        .flatten();
    if session_id.is_none() {
        probe_errors.push("active Wayland session unavailable");
    }
    let unit_state = |suffix: &str| {
        Command::new("systemctl")
            .args([
                "--user",
                "show",
                &format!("{watchdog_unit}.{suffix}"),
                "--property=ActiveState",
                "--value",
            ])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|state| state.trim().to_string())
            .filter(|state| !state.is_empty())
    };
    let watchdog_timer_state = unit_state("timer");
    let watchdog_service_state = unit_state("service");
    if watchdog_timer_state.is_none() {
        probe_errors.push("watchdog timer state unavailable");
    }
    if watchdog_service_state.is_none() {
        probe_errors.push("watchdog service state unavailable");
    }
    let watchdog_timer_active = watchdog_timer_state
        .as_deref()
        .map(|state| state == "active");
    match read_state(conn) {
        Ok((_serial, monitors, logical)) => {
            let configuration_hash_matches = current_output_hash(backup, &monitors, &logical)
                .map(|hash| hash == backup.configuration_hash);
            FinalState {
                raw_connectors: monitors
                    .iter()
                    .map(|monitor| monitor.connector_info.connector.clone())
                    .collect(),
                logical_connectors: logical
                    .iter()
                    .flat_map(|monitor| {
                        monitor
                            .monitors
                            .iter()
                            .map(|connector| connector.connector.clone())
                    })
                    .collect(),
                power_save_mode,
                shell_pid,
                session_id: session_id.clone(),
                watchdog_timer_active,
                watchdog_timer_state,
                watchdog_service_state,
                topology_matches_original: original_topology_matches(backup, &logical)
                    && configuration_hash_matches == Some(true)
                    && power_save_mode == Some(POWER_SAVE_ON)
                    && shell_pid == Some(backup.shell_pid)
                    && session_id.as_deref() == Some(backup.session_id.as_str()),
                configuration_hash_matches,
                read_error: None,
                probe_errors,
            }
        }
        Err(error) => FinalState {
            raw_connectors: Vec::new(),
            logical_connectors: Vec::new(),
            power_save_mode,
            shell_pid,
            session_id,
            watchdog_timer_active,
            watchdog_timer_state,
            watchdog_service_state,
            topology_matches_original: false,
            configuration_hash_matches: None,
            read_error: Some(error.to_string()),
            probe_errors,
        },
    }
}

fn run_one_cycle(
    conn: &Connection,
    cycle: u32,
    virtual_connector: &str,
    virtual_mode_id: &str,
    original_write_side: &[LogicalMonitorConfig],
) -> CycleFindings {
    let start = Instant::now();
    let apply_error = (|| -> anyhow::Result<()> {
        set_power_save_mode(conn, POWER_SAVE_OFF)?;
        let (serial, ..) = read_state(conn)?;
        apply_monitors_config(
            conn,
            serial,
            &zero_physical_config(virtual_connector, virtual_mode_id),
        )
    })()
    .err()
    .map(|e| e.to_string());

    let physical_connectors_absent_from_logical_monitors = read_state(conn)
        .map(|(_serial, _monitors, logical)| {
            physical_connectors_active(&logical, virtual_connector).is_empty()
        })
        .unwrap_or(false);

    let restore_error = (|| -> anyhow::Result<()> {
        let (serial, ..) = read_state(conn)?;
        apply_monitors_config(conn, serial, original_write_side)?;
        set_power_save_mode(conn, POWER_SAVE_ON)
    })()
    .err()
    .map(|e| e.to_string());

    let topology_restored = restore_error.is_none()
        && read_state(conn)
            .map(|(_serial, _monitors, logical)| {
                physical_connectors_active(&logical, virtual_connector).len()
                    == original_write_side
                        .iter()
                        .map(|lm| lm.monitors.len())
                        .sum::<usize>()
            })
            .unwrap_or(false);

    CycleFindings {
        cycle,
        duration_ms: start.elapsed().as_millis(),
        apply_error,
        physical_connectors_absent_from_logical_monitors,
        restore_error,
        topology_restored,
    }
}

fn cleanup_verified(
    prestop_restored: bool,
    final_topology_restored: bool,
    stop_succeeded: bool,
    virtual_gone: bool,
) -> bool {
    prestop_restored && final_topology_restored && stop_succeeded && virtual_gone
}

fn diagnostic_result(
    paused: bool,
    post_stop_restore_attempted: bool,
    verified_cleanup: bool,
    cycles_clean: bool,
) -> ExperimentResult {
    if post_stop_restore_attempted {
        ExperimentResult::Fail
    } else if paused {
        if verified_cleanup {
            ExperimentResult::Partial
        } else {
            ExperimentResult::Fail
        }
    } else if verified_cleanup && cycles_clean {
        ExperimentResult::Pass
    } else {
        ExperimentResult::Fail
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    require_remote_desktop_masked()?;
    let redact_on = args.common.redact_enabled();
    let now = OffsetDateTime::now_utc();
    let dir = evidence_dir(EXP_ID, now)?;

    let conn = Connection::session()?;
    let host = host_info();

    // 1. Snapshot + persist (Decision 1/3: a separate process, exp07, must
    //    be able to restore from this file alone).
    let (_serial0, monitors0, logical0) = read_state(&conn)?;
    let original_session_id = discover(current_uid()?)?
        .1
        .ok_or_else(|| anyhow::anyhow!("no unique active Wayland user session"))?;
    let original_shell_pid = gnome_shell_pid(&conn)?;
    let backup = build_backup(
        &monitors0,
        &logical0,
        &original_session_id,
        original_shell_pid,
    );
    anyhow::ensure!(
        backup.primary_output.is_some(),
        "refusing isolation without an identifiable primary output"
    );
    let shell_pid_before = if args.auto_kill_after_isolate {
        anyhow::ensure!(
            logical0.len() == 1
                && logical0[0].monitors.len() == 1
                && logical0[0].monitors[0].connector == "HDMI-1",
            "automatic owner-loss probe requires HDMI-1 as the sole active output"
        );
        anyhow::ensure!(
            std::path::Path::new("/usr/bin/kill").is_file(),
            "external kill command unavailable"
        );
        Some(original_shell_pid)
    } else {
        None
    };
    // Absolute: `systemd-run`'s transient watchdog unit does not share this
    // process's working directory (found live 2026-09-05 — a relative path
    // here made the watchdog's restore command fail with "No such file or
    // directory", requiring manual operator recovery over SSH).
    let backup_path = std::path::absolute(dir.join("backup.json"))?;
    std::fs::write(&backup_path, serde_json::to_string_pretty(&backup)?)?;
    let original_write_side = to_write_side(&backup)?;
    let physical_connectors_before: Vec<String> = monitors0
        .iter()
        .map(|m| m.connector_info.connector.clone())
        .collect();

    let apply_allowed = apply_monitors_config_allowed(&conn)?;
    if !apply_allowed {
        // Doc 05 §30 / Doc 00 §49: report, do not attempt a workaround.
        let findings = Findings {
            host,
            physical_connectors_before,
            virtual_connector: None,
            apply_monitors_config_allowed: false,
            watchdog_armed: false,
            watchdog_unit: None,
            cleanup_watchdog_unit: None,
            cycles: vec![],
            stop_error: None,
            virtual_connector_fully_gone: false,
            post_stop_restore_attempted: false,
            post_stop_restore_error: None,
            post_stop_pre_repair_state: None,
            paused_for_manual_kill_test: false,
            final_state: None,
        };
        write_experiment_report(&dir, now, redact_on, &findings, ExperimentResult::Blocked)?;
        println!(
            "BLOCKED: ApplyMonitorsConfigAllowed is false — Doc 00 §49 stop condition, see {}",
            dir.display()
        );
        return Ok(());
    }

    // 2. Create + confirm the virtual monitor.
    let (virtual_connector, mut session_guard) =
        create_virtual_monitor(&conn, &physical_connectors_before)?;
    let (_serial1, monitors1, _logical1) = read_state(&conn)?;
    let virtual_mode_id = current_mode_id(&monitors1, &virtual_connector)
        .ok_or_else(|| anyhow::anyhow!("no current mode reported for the virtual connector"))?;

    // 3. Arm the restore watchdog before the first real disable.
    let watchdog_unit = format!("blackroom-exp06-watchdog-{}", now.unix_timestamp());
    let watchdog_seconds = args.watchdog_duration();
    let watchdog_started = Instant::now();
    arm_watchdog(&watchdog_unit, watchdog_seconds, &backup_path)?;
    let watchdog_armed = true;

    let mut restore_guard = RestoreGuard {
        conn: &conn,
        original: original_write_side.clone(),
        session_id: original_session_id.clone(),
        shell_pid: original_shell_pid,
        disarmed: false,
    };

    let (mut cycles, mut all_restored, mut paused) = (Vec::new(), true, false);
    let mut cleanup_watchdog_unit = None;

    if args.pause_after_isolate {
        // First isolate only, then block — the deliberate ungraceful-
        // termination scenario (assessment §7.3, Phase 5 plan Decision 1).
        set_power_save_mode(&conn, POWER_SAVE_OFF)?;
        let (serial, ..) = read_state(&conn)?;
        apply_monitors_config(
            &conn,
            serial,
            &zero_physical_config(&virtual_connector, &virtual_mode_id),
        )?;
        if args.auto_kill_after_isolate {
            let shell_pid_before = shell_pid_before.expect("automatic mode captured Shell PID");
            let preflight = auto_kill_preflight(
                &conn,
                &original_write_side,
                &virtual_connector,
                &watchdog_unit,
                shell_pid_before,
                watchdog_started,
            )?;
            anyhow::ensure!(preflight.ready(), "automatic owner-loss preflight failed");
            let report = ExperimentReport {
                experiment: "Experiment 6 — Automatic HDMI-only Owner-Loss Probe".to_string(),
                environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1"
                    .to_string(),
                objective: "Observe process death while a virtual-only display configuration is active."
                    .to_string(),
                hypothesis: "Mutter removes the owning virtual monitor and restores physical outputs when exp06 is killed."
                    .to_string(),
                procedure: "Persist the original HDMI-only backup, arm the watchdog, isolate outputs, verify timer/topology/Shell/power twice, persist pre-kill evidence, then signal only this exp06 PID. Inspect restoration independently."
                    .to_string(),
                expected: "Physical panels show no desktop during isolation; watchdog or Mutter restores the original topology without a Shell crash."
                    .to_string(),
                observed: redact(&format!("pre_kill={preflight:#?}"), redact_on),
                evidence: vec![
                    dir.join("backup.json").display().to_string(),
                    dir.join("pre_kill.json").display().to_string(),
                ],
                result: ExperimentResult::Partial,
                failure: None,
                root_cause: None,
                security_impact: Some("Physical desktop privacy cannot be verified by D-Bus; the independent observer reports after recovery. GNOME Shell may crash.".to_string()),
                recommended_action: Some("Verify Shell PID, journal, physical screen and original topology from independent SSH before any further experiment.".to_string()),
                follow_up: None,
            };
            write_evidence(
                &dir,
                &redact(&report.render(now), redact_on),
                "pre_kill.json",
                &preflight,
            )?;
            println!(
                "Pre-kill evidence at {}; backup={}; watchdog={} ({}s); PID={}",
                dir.display(),
                backup_path.display(),
                watchdog_unit,
                watchdog_seconds,
                std::process::id()
            );
            std::io::stdout().flush()?;
            let final_preflight = auto_kill_preflight(
                &conn,
                &original_write_side,
                &virtual_connector,
                &watchdog_unit,
                shell_pid_before,
                watchdog_started,
            )?;
            anyhow::ensure!(
                final_preflight.ready(),
                "automatic owner-loss final check failed"
            );
            let status = Command::new("/usr/bin/kill")
                .args(["-s", "KILL", &std::process::id().to_string()])
                .status()?;
            anyhow::bail!("self-SIGKILL unexpectedly returned with status {status}");
        }
        println!(
            "Isolated. PID={}, backup={}, watchdog={} ({}s)",
            std::process::id(),
            backup_path.display(),
            watchdog_unit,
            watchdog_seconds
        );
        println!("Press Enter to restore gracefully and exit, OR from a separate");
        println!(
            "terminal run: kill -9 {} ; then run exp07_restore --backup {}",
            std::process::id(),
            backup_path.display()
        );
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        verify_live_restore_identity(&conn, &backup.session_id, backup.shell_pid)?;
        let cleanup_unit = format!("{watchdog_unit}-cleanup");
        arm_watchdog(&cleanup_unit, WATCHDOG_SECONDS_DEFAULT, &backup_path)?;
        println!(
            "Cleanup watchdog={} ({}s)",
            cleanup_unit, WATCHDOG_SECONDS_DEFAULT
        );
        std::io::stdout().flush()?;
        cleanup_watchdog_unit = Some(cleanup_unit);
        // Actually restore now (bug found live 2026-09-05: this branch
        // previously fell through without restoring at all, and
        // `all_restored`'s unmutated `true` default wrongly disarmed
        // RestoreGuard — the operator had to restore manually over SSH).
        let restore_error = restore_original(
            &conn,
            &backup.session_id,
            backup.shell_pid,
            &original_write_side,
        )
        .err();
        if let Some(error) = restore_error {
            eprintln!("Restore after pause failed: {error}");
            all_restored = false;
        } else {
            all_restored = read_state(&conn)
                .map(|(_serial, _monitors, logical)| original_topology_matches(&backup, &logical))
                .unwrap_or(false);
        }
        paused = true;
    } else {
        for cycle in 1..=args.cycles {
            let cycle_start = Instant::now();
            let result = run_one_cycle(
                &conn,
                cycle,
                &virtual_connector,
                &virtual_mode_id,
                &original_write_side,
            );
            all_restored &= result.topology_restored;
            let hung = cycle_start.elapsed() > PER_CYCLE_TIMEOUT;
            cycles.push(result);
            if hung {
                eprintln!("cycle {cycle} exceeded the {PER_CYCLE_TIMEOUT:?} bound — aborting run");
                break;
            }
        }
    }

    let stop_error = Proxy::new(
        &conn,
        "org.gnome.Mutter.ScreenCast",
        session_guard.path.clone(),
        "org.gnome.Mutter.ScreenCast.Session",
    )
    .and_then(|p| p.call::<_, _, ()>("Stop", &()))
    .err()
    .map(|e| e.to_string());
    if stop_error.is_none() {
        session_guard.disarm();
    }
    drop(session_guard);

    let remaining = poll_for_connector_gone(&conn, &virtual_connector)?;
    let virtual_connector_fully_gone = !remaining.contains(&virtual_connector);
    let monitored_unit = cleanup_watchdog_unit.as_deref().unwrap_or(&watchdog_unit);
    let observed = capture_final_state(&conn, &backup, monitored_unit);
    let post_stop_restore_attempted = should_repair_post_stop(
        &observed,
        &backup,
        stop_error.is_none(),
        virtual_connector_fully_gone,
    );
    let post_stop_pre_repair_state = pre_repair_state(&observed, post_stop_restore_attempted);
    let post_stop_restore_error = if post_stop_restore_attempted {
        restore_original(
            &conn,
            &backup.session_id,
            backup.shell_pid,
            &original_write_side,
        )
        .err()
        .map(|error| error.to_string())
    } else {
        None
    };
    let final_state = if post_stop_restore_attempted {
        capture_final_state(&conn, &backup, monitored_unit)
    } else {
        observed
    };
    let final_topology_restored = final_state.topology_matches_original;

    let findings = Findings {
        host,
        physical_connectors_before,
        virtual_connector: Some(virtual_connector),
        apply_monitors_config_allowed: apply_allowed,
        watchdog_armed,
        watchdog_unit: Some(watchdog_unit.clone()),
        cleanup_watchdog_unit: cleanup_watchdog_unit.clone(),
        cycles,
        stop_error,
        virtual_connector_fully_gone,
        post_stop_restore_attempted,
        post_stop_restore_error,
        post_stop_pre_repair_state,
        paused_for_manual_kill_test: paused,
        final_state: Some(final_state),
    };

    let verified_cleanup = cleanup_verified(
        all_restored,
        final_topology_restored,
        findings.stop_error.is_none(),
        findings.virtual_connector_fully_gone,
    ) && findings.post_stop_restore_error.is_none();

    let cycles_clean = findings
        .cycles
        .iter()
        .all(|c| c.apply_error.is_none() && c.physical_connectors_absent_from_logical_monitors);
    let result = diagnostic_result(
        paused,
        post_stop_restore_attempted,
        verified_cleanup,
        cycles_clean,
    );

    if verified_cleanup {
        restore_guard.disarm();
        if !post_stop_restore_attempted {
            if let Some(unit) = &cleanup_watchdog_unit {
                disarm_watchdog(unit);
            } else {
                disarm_watchdog(&watchdog_unit);
            }
        }
    }

    write_experiment_report(&dir, now, redact_on, &findings, result)?;
    println!("Wrote evidence to {}", dir.display());
    println!("Result: {result}");
    Ok(())
}

fn write_experiment_report(
    dir: &std::path::Path,
    now: OffsetDateTime,
    redact_on: bool,
    findings: &Findings,
    result: ExperimentResult,
) -> anyhow::Result<()> {
    let observed = redact(&format!("{findings:#?}"), redact_on);
    let report = ExperimentReport {
        experiment: "Experiment 6 — Physical Output Isolation".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1"
            .to_string(),
        objective: "Disable every physical output while the virtual monitor stays active \
                    (Document 10 §13, Document 02 §12)."
            .to_string(),
        hypothesis: "ApplyMonitorsConfig(method=Temporary) accepts a logical-monitors array \
                     containing only the virtual monitor's entry (zero physical monitors \
                     enabled); every physical connector then disappears from \
                     logical_monitors[] while remaining in the raw monitors[] inventory."
            .to_string(),
        procedure: "Snapshot + persist DisplayBackup; create and confirm a virtual monitor; \
                    check ApplyMonitorsConfigAllowed; arm the restore watchdog; apply a \
                    zero-physical logical-monitors config; verify; restore; verify \
                    restoration; tear down."
            .to_string(),
        expected: "Every physical connector is absent from logical_monitors[] while isolated, \
                   and the exact original topology is restored afterward."
            .to_string(),
        observed,
        evidence: vec![
            dir.join("report.md").display().to_string(),
            dir.join("backup.json").display().to_string(),
        ],
        result,
        failure: if matches!(result, ExperimentResult::Pass) {
            None
        } else {
            Some(format!("{findings:#?}"))
        },
        root_cause: None,
        security_impact: Some(
            "gnome-remote-desktop.service must stay masked (experiment-safety.md §5); every \
             ApplyMonitorsConfig call uses method=Temporary and never writes monitors.xml; a \
             systemd-run watchdog is armed before the first real disable."
                .to_string(),
        ),
        recommended_action: None,
        follow_up: Some(
            "exp07_restore.rs independently restores from the persisted DisplayBackup JSON, \
             decoupled from whether this process is still alive (Phase 5 plan Decision 1)."
                .to_string(),
        ),
    };
    write_evidence(
        dir,
        &redact(&report.render(now), redact_on),
        "findings.json",
        findings,
    )
}
