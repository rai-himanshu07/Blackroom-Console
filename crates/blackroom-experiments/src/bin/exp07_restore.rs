//! Experiment 7 — Display Restoration (Document 10 §14 / Document 02 §12
//! step 7-8): a standalone binary, deliberately decoupled from
//! `exp06_isolate_outputs`'s own process — it loads a persisted
//! `DisplayBackup` JSON and independently restores the original physical
//! topology, whether or not the process that isolated it is still alive
//! (Phase 5 plan Decision 1: this is what actually exercises assessment
//! §7.3's crash-recovery question, and what `exp06 --pause-after-isolate`'s
//! watchdog invokes on ungraceful termination).
//!
//! Implements Doc 05 §62's restoration-failure handling explicitly: retries
//! once safely, never claims success while physical state is unknown, and
//! reports diagnostic state either way.

use std::collections::HashMap;
use std::path::PathBuf;
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
use zbus::zvariant::{OwnedValue, Type, Value};

const EXP_ID: &str = "exp07";
const METHOD_TEMPORARY: u32 = 1;
const POLL_WAIT: Duration = Duration::from_secs(10);

#[derive(Parser, Debug)]
#[command(
    about = "Experiment 7: restore the original physical display topology from a persisted DisplayBackup"
)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Path to a `backup.json` written by `exp06_isolate_outputs` — required
    /// (deliberately no "most recent" auto-discovery: a restore tool should
    /// never guess which snapshot to apply).
    #[arg(long)]
    backup: PathBuf,
    /// Keep every live ScreenCast virtual monitor as an extra logical monitor right of the
    /// restored ones (Mutter 50.1 crashes if an enabled stream's virtual monitor has none, exp13).
    /// Set by `exp06 --integrated-probe`'s watchdogs, where a consumer may be streaming.
    #[arg(long)]
    keep_live_virtual: bool,
    /// Lock the backed-up session after the restore attempt, pass or fail (dead-man restores:
    /// the owner is gone, so nothing may be left unlocked). Never unlocks.
    #[arg(long)]
    lock_after: bool,
    /// Guard mode: wait until this process (the console) is gone, then lock the session at once and exit
    /// WITHOUT touching the display: an ApplyMonitorsConfig right after the owner vanished crashed the
    /// Shell (2026-10-03 13:52, libmutter apply_monitors_config); the dead-man timers restore later.
    #[arg(long)]
    after_pid: Option<u32>,
}

// ---------------------------------------------------------------------
// DisplayConfig read/write wire types (per-file duplication convention —
// see exp05_virtual_active.rs / exp06_isolate_outputs.rs).
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Type, Deserialize)]
struct ConnectorInfo {
    connector: String,
    #[allow(dead_code)]
    vendor: String,
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

fn is_current_mode(properties: &HashMap<String, OwnedValue>) -> bool {
    properties
        .get("is-current")
        .and_then(|value| bool::try_from(value.clone()).ok())
        .unwrap_or(false)
}

/// DPMS levels (standard: 0=ON, 3=OFF; confirmed live 2026-09-05). Doc 05
/// §34: a bare `ApplyMonitorsConfig` restore does not guarantee the panel
/// actually lights back up (found live 2026-09-05 that the isolating
/// mechanism needs this paired with `ApplyMonitorsConfig`, not as a
/// substitute for it) — restore un-blanks explicitly rather than assuming.
const POWER_SAVE_ON: i32 = 0;

fn set_power_save_mode(conn: &Connection, mode: i32) -> anyhow::Result<()> {
    Ok(display_config_proxy(conn)?.set_property("PowerSaveMode", mode)?)
}

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

// ---------------------------------------------------------------------
// DisplayBackup (Doc 05 §29) — same shape `exp06_isolate_outputs` writes.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OutputBackup {
    connector: String,
    #[allow(dead_code)]
    vendor: String,
    #[allow(dead_code)]
    product: String,
    serial: String,
    mode_id: String,
    width: i32,
    height: i32,
    #[allow(dead_code)]
    refresh_rate: f64,
    #[serde(default)]
    enabled: Option<bool>,
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
    #[serde(default)]
    primary_output: Option<(String, String)>,
    #[serde(default)]
    hash_version: Option<u32>,
    #[serde(default)]
    configuration_hash: Option<u64>,
}

fn validated_hash(backup: &DisplayBackup) -> anyhow::Result<Option<u64>> {
    let Some(expected_hash) = backup.configuration_hash else {
        anyhow::ensure!(
            backup.primary_output.is_none()
                && backup.hash_version.is_none()
                && backup.outputs.iter().all(|output| output.enabled.is_none()),
            "incomplete display backup metadata"
        );
        return Ok(None);
    };
    let primary_output = backup
        .topology
        .iter()
        .find(|monitor| monitor.primary)
        .and_then(|monitor| monitor.monitors.first().cloned());
    anyhow::ensure!(
        primary_output.is_some() && backup.primary_output == primary_output,
        "display backup primary output does not match its topology"
    );
    let canonical: Vec<_> = backup
        .outputs
        .iter()
        .map(|output| {
            let enabled = output
                .enabled
                .ok_or_else(|| anyhow::anyhow!("display backup enabled state is missing"))?;
            anyhow::ensure!(
                enabled
                    == backup.topology.iter().any(|monitor| {
                        monitor.monitors.iter().any(|(connector, serial)| {
                            connector == &output.connector && serial == &output.serial
                        })
                    }),
                "display backup enabled state does not match its topology"
            );
            Ok(CanonicalOutputBackup {
                connector: output.connector.clone(),
                vendor: output.vendor.clone(),
                product: output.product.clone(),
                serial: output.serial.clone(),
                mode_id: output.mode_id.clone(),
                width: output.width,
                height: output.height,
                refresh_rate: output.refresh_rate,
                enabled,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    if backup.hash_version == Some(display_config::CONFIGURATION_HASH_VERSION) {
        anyhow::ensure!(
            display_config::compute_hash(&canonical) == expected_hash,
            "display backup configuration hash mismatch"
        );
        Ok(Some(expected_hash))
    } else {
        Ok(None)
    }
}

fn verify_identity(
    backup: &DisplayBackup,
    current_session_id: &str,
    current_shell_pid: u32,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !backup.session_id.is_empty()
            && backup.shell_pid != 0
            && backup.session_id == current_session_id
            && backup.shell_pid == current_shell_pid,
        "refusing display backup from a different GNOME session or Shell process"
    );
    Ok(())
}

fn verify_live_identity(conn: &Connection, backup: &DisplayBackup) -> anyhow::Result<()> {
    let session_id = discover(current_uid()?)?
        .1
        .ok_or_else(|| anyhow::anyhow!("no unique active Wayland user session"))?;
    let proxy = zbus::blocking::fdo::DBusProxy::new(conn)?;
    let shell_pid = proxy.get_connection_unix_process_id(zbus::names::BusName::try_from(
        "org.gnome.Mutter.ScreenCast",
    )?)?;
    verify_identity(backup, &session_id, shell_pid)
}

/// Reuses the mode IDs captured at snapshot time (Experiment 5 precedent),
/// not re-resolved from current state. Fails closed on an inconsistent
/// backup rather than guessing.
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

/// Doc 05 §61: verify every expected physical output is restored with the
/// correct mode/position/scale/primary — field comparison, not just a hash,
/// so a mismatch's exact cause is diagnosable. Logical monitors holding a
/// `kept` virtual connector are ignored.
fn topology_matches(
    backup: &DisplayBackup,
    logical: &[LogicalMonitorEntry],
    kept: &[String],
) -> bool {
    let logical: Vec<&LogicalMonitorEntry> = logical
        .iter()
        .filter(|entry| !entry.monitors.iter().any(|m| kept.contains(&m.connector)))
        .collect();
    if logical.len() != backup.topology.len() {
        return false;
    }
    backup.topology.iter().all(|expected| {
        logical.iter().any(|actual| {
            actual.x == expected.x
                && actual.y == expected.y
                && (actual.scale - expected.scale).abs() < f64::EPSILON
                && actual.transform == expected.transform
                && actual.primary == expected.primary
                && actual.monitors.len() == expected.monitors.len()
                && expected.monitors.iter().all(|(c, s)| {
                    actual
                        .monitors
                        .iter()
                        .any(|m| &m.connector == c && &m.serial == s)
                })
        })
    })
}

fn restored_output_hash(
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
                None if !enabled && saved.enabled == Some(false) => (
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

/// Raw connector absent from `outputs[]`. This also includes physical outputs
/// without a current mode when the backup was taken, so it is diagnostic only.
fn unexpected_connectors(backup: &DisplayBackup, monitors: &[MonitorEntry]) -> Vec<String> {
    monitors
        .iter()
        .map(|m| m.connector_info.connector.clone())
        .filter(|c| !backup.outputs.iter().any(|o| &o.connector == c))
        .collect()
}

/// Mutter names ScreenCast virtual outputs `Meta-N`; no physical connector does.
fn is_virtual_monitor(info: &ConnectorInfo) -> bool {
    info.connector.starts_with("Meta-")
}

/// Restore config for `--keep-live-virtual`: the backup's topology plus every live virtual
/// monitor absent from it, placed right of the restored ones. Falls back to the plain config
/// (nothing kept) when the restored monitors are not all scale 1.0 without a transform, because
/// recovery matters more than keeping a virtual monitor.
fn restore_config(
    backup: &DisplayBackup,
    monitors: &[MonitorEntry],
    keep_live_virtual: bool,
) -> anyhow::Result<(Vec<LogicalMonitorConfig>, Vec<String>)> {
    let mut config = to_write_side(backup)?;
    if !keep_live_virtual {
        return Ok((config, Vec::new()));
    }
    let live: Vec<(String, String, i32)> = monitors
        .iter()
        .filter(|m| {
            is_virtual_monitor(&m.connector_info)
                && !backup
                    .outputs
                    .iter()
                    .any(|o| o.connector == m.connector_info.connector)
        })
        .filter_map(|m| {
            let Some(mode) = m
                .modes
                .iter()
                .find(|mode| is_current_mode(&mode.properties))
            else {
                eprintln!(
                    "Not keeping {}: no current mode",
                    m.connector_info.connector
                );
                return None;
            };
            Some((
                m.connector_info.connector.clone(),
                mode.id.clone(),
                mode.width,
            ))
        })
        .collect();
    if live.is_empty() {
        return Ok((config, Vec::new()));
    }
    let restored: Vec<_> = config
        .iter()
        .map(|lm| {
            let width = lm
                .monitors
                .first()
                .and_then(|m| backup.outputs.iter().find(|o| o.connector == m.connector))
                .map_or(0, |o| o.width);
            (lm.x, lm.y, width, lm.scale, lm.transform)
        })
        .collect();
    let (mut x, y) = match display_config::kept_virtual_origin(&restored) {
        Ok(origin) => origin,
        Err(error) => {
            eprintln!("Not keeping the live virtual monitor: {error}");
            return Ok((config, Vec::new()));
        }
    };
    let mut kept = Vec::new();
    for (connector, mode_id, width) in live {
        config.push(LogicalMonitorConfig {
            x,
            y,
            scale: 1.0,
            transform: 0,
            primary: false,
            monitors: vec![MonitorRef {
                connector: connector.clone(),
                mode_id,
                properties: HashMap::new(),
            }],
        });
        x += width;
        kept.push(connector);
    }
    Ok((config, kept))
}

#[derive(Debug, Serialize)]
struct Findings {
    backup_path: String,
    monitors_before_restore: Vec<String>,
    unexpected_connectors_before_restore: Vec<String>,
    kept_virtual_connectors: Vec<String>,
    apply_error: Option<String>,
    retried: bool,
    topology_matches: bool,
    configuration_hash_matches: Option<bool>,
    unexpected_connectors_after_restore: Vec<String>,
}

fn require_verified_restore(result: ExperimentResult) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(result, ExperimentResult::Pass),
        "display restoration did not verify"
    );
    Ok(())
}

/// Returns the virtual connectors kept in the applied config.
fn attempt_restore(
    conn: &Connection,
    backup: &DisplayBackup,
    keep_live_virtual: bool,
) -> anyhow::Result<Vec<String>> {
    verify_live_identity(conn, backup)?;
    let (serial, monitors, _logical) = read_state(conn)?;
    let (config, kept) = restore_config(backup, &monitors, keep_live_virtual)?;
    verify_live_identity(conn, backup)?;
    apply_monitors_config(conn, serial, &config)?;
    verify_live_identity(conn, backup)?;
    set_power_save_mode(conn, POWER_SAVE_ON)?;
    Ok(kept)
}

/// A process that no longer exists or is only a zombie awaiting its parent counts as gone.
fn process_gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat
            .rsplit_once(") ")
            .is_some_and(|(_, rest)| rest.starts_with('Z')),
    }
}

fn print_lock(session_id: &str) -> bool {
    let locked = std::process::Command::new("loginctl")
        .args(["lock-session", session_id])
        .stdin(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    println!(
        "lock-session {session_id}: {}",
        if locked { "ok" } else { "FAILED" }
    );
    locked
}

/// The process result: a restore that did not verify, or a lock that was asked for and failed, is an error.
fn require_outcome(result: ExperimentResult, locked: Option<bool>) -> anyhow::Result<()> {
    let verified = require_verified_restore(result);
    if locked == Some(false) {
        anyhow::bail!("the session could not be locked");
    }
    verified
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    if let Some(pid) = args.after_pid {
        while !process_gone(pid) {
            thread::sleep(Duration::from_millis(100));
        }
        println!("guarded process {pid} is gone");
    }
    let redact_on = args.common.redact_enabled();
    let now = OffsetDateTime::now_utc();

    let backup_text = std::fs::read_to_string(&args.backup)?;
    let backup: DisplayBackup = serde_json::from_str(&backup_text)?;
    let expected_hash = validated_hash(&backup)?;
    if backup.configuration_hash.is_some() && expected_hash.is_none() {
        eprintln!(
            "Unsupported or unversioned display hash: verifying session and exact topology fields only"
        );
    }

    let conn = Connection::session()?;
    verify_live_identity(&conn, &backup)?;
    if args.after_pid.is_some() {
        if !print_lock(&backup.session_id) {
            anyhow::bail!("the session could not be locked");
        }
        return Ok(());
    }

    let (_serial0, monitors0, _logical0) = read_state(&conn)?;
    let monitors_before_restore: Vec<String> = monitors0
        .iter()
        .map(|m| m.connector_info.connector.clone())
        .collect();
    let unexpected_connectors_before_restore = unexpected_connectors(&backup, &monitors0);

    // Doc 05 §62: retry safely once; never claim success if physical state
    // remains unknown. The retry never keeps the virtual monitor: recovering the
    // physical display matters more than avoiding a Mutter crash with a live consumer.
    let mut kept_virtual_connectors = Vec::new();
    let mut attempt =
        |keep_live_virtual: bool| match attempt_restore(&conn, &backup, keep_live_virtual) {
            Ok(kept) => {
                kept_virtual_connectors = kept;
                None
            }
            Err(error) => Some(error.to_string()),
        };
    let mut apply_error = attempt(args.keep_live_virtual);
    let mut retried = false;
    if apply_error.is_some() {
        retried = true;
        apply_error = attempt(false);
    }

    // Locked before the verification poll: a failing state read there must not skip the lock.
    let locked = args.lock_after.then(|| print_lock(&backup.session_id));

    let deadline = Instant::now() + POLL_WAIT;
    let (restored, topology_ok, configuration_hash_matches, monitors_final) = loop {
        let (_serial, monitors, logical) = read_state(&conn)?;
        // Virtual monitors are not physical outputs; with the flag they may hold logical monitors.
        let ignored_virtual: Vec<String> = if args.keep_live_virtual {
            monitors
                .iter()
                .filter(|m| {
                    is_virtual_monitor(&m.connector_info)
                        && !backup
                            .outputs
                            .iter()
                            .any(|o| o.connector == m.connector_info.connector)
                })
                .map(|m| m.connector_info.connector.clone())
                .collect()
        } else {
            Vec::new()
        };
        let topology_ok =
            apply_error.is_none() && topology_matches(&backup, &logical, &ignored_virtual);
        let configuration_hash_matches = expected_hash
            .map(|hash| restored_output_hash(&backup, &monitors, &logical) == Some(hash));
        let restored = topology_ok && configuration_hash_matches.unwrap_or(true);
        if restored || Instant::now() >= deadline {
            break (restored, topology_ok, configuration_hash_matches, monitors);
        }
        thread::sleep(Duration::from_millis(150));
    };

    let findings = Findings {
        backup_path: args.backup.display().to_string(),
        monitors_before_restore,
        unexpected_connectors_before_restore,
        kept_virtual_connectors,
        apply_error: apply_error.clone(),
        retried,
        topology_matches: topology_ok,
        configuration_hash_matches,
        unexpected_connectors_after_restore: unexpected_connectors(&backup, &monitors_final),
    };

    // Doc 05 §62 step 5: remain safe/diagnostic rather than silently
    // reporting LOCAL_ACTIVE-equivalent success on unknown state.
    let result = if restored {
        ExperimentResult::Pass
    } else {
        ExperimentResult::Fail
    };

    let dir = evidence_dir(EXP_ID, now)?;
    let observed = redact(&format!("{findings:#?}"), redact_on);
    let report = ExperimentReport {
        experiment: "Experiment 7 — Display Restoration".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1"
            .to_string(),
        objective: "Restore the exact original physical display topology from a persisted \
                    DisplayBackup, independently of whether the isolating process is still \
                    alive (Document 10 §14)."
            .to_string(),
        hypothesis: "Re-applying the backup's write-side config (reusing its captured mode \
                     IDs, only the ApplyMonitorsConfig serial re-read fresh) restores the \
                     exact original topology regardless of which process created the backup."
            .to_string(),
        procedure: "Load the DisplayBackup JSON; read current state (diagnostic); apply the \
                    reconstructed write-side config (retry once on failure per Doc 05 §62); \
                    poll GetCurrentState until the topology matches or the wait bound elapses."
            .to_string(),
        expected: "Every backed-up logical output's position/mode/scale/transform/primary \
               matches exactly; extra raw connectors are diagnostic only."
            .to_string(),
        observed,
        evidence: vec![dir.join("report.md").display().to_string()],
        result,
        failure: if matches!(result, ExperimentResult::Pass) {
            None
        } else {
            Some(format!("{findings:#?}"))
        },
        root_cause: None,
        security_impact: Some(
            "ApplyMonitorsConfig uses method=Temporary; this binary never writes monitors.xml \
             and never enables remote input regardless of outcome (Doc 05 §62 step 2)."
                .to_string(),
        ),
        recommended_action: if matches!(result, ExperimentResult::Pass) {
            None
        } else {
            Some(
                "Do not enable remote input or claim LOCAL_ACTIVE while physical state is \
                 unknown; escalate per Doc 00 §49."
                    .to_string(),
            )
        },
        follow_up: None,
    };

    write_evidence(
        &dir,
        &redact(&report.render(now), redact_on),
        "findings.json",
        &findings,
    )?;
    println!("Wrote evidence to {}", dir.display());
    println!("Result: {result}");
    require_outcome(result, locked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_process_is_not_gone_and_a_missing_or_zombie_one_is() {
        assert!(!process_gone(std::process::id()));
        assert!(process_gone(u32::MAX - 1));
        let child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        // Unreaped, the exited child stays a zombie: still "gone".
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !process_gone(pid) && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(process_gone(pid));
        drop(child);
    }

    #[test]
    fn failed_restore_reports_a_nonzero_process_result() {
        assert!(require_verified_restore(ExperimentResult::Pass).is_ok());
        assert!(require_verified_restore(ExperimentResult::Fail).is_err());
    }

    #[test]
    fn a_lock_that_was_asked_for_and_failed_is_a_nonzero_result() {
        assert!(require_outcome(ExperimentResult::Pass, None).is_ok());
        assert!(require_outcome(ExperimentResult::Pass, Some(true)).is_ok());
        assert!(require_outcome(ExperimentResult::Pass, Some(false)).is_err());
        assert!(require_outcome(ExperimentResult::Fail, Some(true)).is_err());
    }

    #[test]
    fn legacy_backup_without_metadata_remains_readable() {
        let backup: DisplayBackup = serde_json::from_str(
            r#"{"session_id":"3","shell_pid":34735,"outputs":[{"connector":"eDP-1","vendor":"vendor","product":"panel","serial":"internal","mode_id":"mode-1","width":1920,"height":1080,"refresh_rate":60.0}],"topology":[{"x":0,"y":0,"scale":1.0,"transform":0,"primary":true,"monitors":[["eDP-1","internal"]]}]}"#,
        )
        .unwrap();
        assert_eq!(backup.primary_output, None);
        assert_eq!(backup.hash_version, None);
        assert_eq!(backup.configuration_hash, None);
        assert_eq!(backup.outputs[0].enabled, None);
        assert_eq!(validated_hash(&backup).unwrap(), None);
        assert!(to_write_side(&backup).is_ok());
    }

    #[test]
    fn committed_pre_hash_backup_keeps_its_e_dp_restore_path() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/experiments/evidence/exp06/2026-09-27-3/backup.json");
        let backup: DisplayBackup =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(validated_hash(&backup).unwrap(), None);
        let write_side = to_write_side(&backup).unwrap();
        assert_eq!(write_side.len(), 1);
        assert_eq!(write_side[0].monitors.len(), 1);
        assert_eq!(write_side[0].monitors[0].connector, "eDP-1");
    }

    #[test]
    fn incomplete_or_corrupt_new_backup_metadata_is_refused() {
        let mut backup: DisplayBackup = serde_json::from_str(
            r#"{"session_id":"3","shell_pid":34735,"outputs":[{"connector":"eDP-1","vendor":"vendor","product":"panel","serial":"internal","mode_id":"mode-1","width":1920,"height":1080,"refresh_rate":60.0}],"topology":[{"x":0,"y":0,"scale":1.0,"transform":0,"primary":true,"monitors":[["eDP-1","internal"]]}]}"#,
        )
        .unwrap();
        backup.outputs[0].enabled = Some(true);
        backup.primary_output = Some(("eDP-1".into(), "internal".into()));
        backup.hash_version = Some(display_config::CONFIGURATION_HASH_VERSION);
        backup.configuration_hash = Some(display_config::compute_hash(&[CanonicalOutputBackup {
            connector: "eDP-1".into(),
            vendor: "vendor".into(),
            product: "panel".into(),
            serial: "internal".into(),
            mode_id: "mode-1".into(),
            width: 1920,
            height: 1080,
            refresh_rate: 60.0,
            enabled: true,
        }]));
        assert!(validated_hash(&backup).unwrap().is_some());
        backup.outputs[0].enabled = None;
        assert!(validated_hash(&backup).is_err());
        backup.outputs[0].enabled = Some(false);
        assert!(validated_hash(&backup).is_err());
        backup.outputs[0].enabled = Some(true);
        backup.configuration_hash = Some(0);
        assert!(validated_hash(&backup).is_err());
        backup.configuration_hash = None;
        assert!(validated_hash(&backup).is_err());
        backup.configuration_hash = Some(0);
        backup.hash_version = None;
        assert_eq!(validated_hash(&backup).unwrap(), None);
        backup.hash_version = Some(999);
        assert_eq!(validated_hash(&backup).unwrap(), None);
    }

    #[test]
    fn new_backup_restoration_checks_mode_and_topology_with_virtual_raw_monitor() {
        let identity = ConnectorInfo {
            connector: "eDP-1".into(),
            vendor: "vendor".into(),
            product: "panel".into(),
            serial: "internal".into(),
        };
        let expected = CanonicalOutputBackup {
            connector: identity.connector.clone(),
            vendor: identity.vendor.clone(),
            product: identity.product.clone(),
            serial: identity.serial.clone(),
            mode_id: "mode-1".into(),
            width: 1920,
            height: 1080,
            refresh_rate: 60.0,
            enabled: true,
        };
        let backup = DisplayBackup {
            session_id: "3".into(),
            shell_pid: 34735,
            outputs: vec![OutputBackup {
                connector: expected.connector.clone(),
                vendor: expected.vendor.clone(),
                product: expected.product.clone(),
                serial: expected.serial.clone(),
                mode_id: expected.mode_id.clone(),
                width: expected.width,
                height: expected.height,
                refresh_rate: expected.refresh_rate,
                enabled: Some(true),
            }],
            topology: vec![LogicalMonitorBackup {
                x: 0,
                y: 0,
                scale: 1.0,
                transform: 0,
                primary: true,
                monitors: vec![(identity.connector.clone(), identity.serial.clone())],
            }],
            primary_output: Some((identity.connector.clone(), identity.serial.clone())),
            hash_version: Some(display_config::CONFIGURATION_HASH_VERSION),
            configuration_hash: Some(display_config::compute_hash(std::slice::from_ref(
                &expected,
            ))),
        };
        let mut monitors = vec![MonitorEntry {
            connector_info: identity.clone(),
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
        }];
        let logical = LogicalMonitorEntry {
            x: 0,
            y: 0,
            scale: 1.0,
            transform: 0,
            primary: true,
            monitors: vec![identity],
            properties: HashMap::new(),
        };
        monitors.push(MonitorEntry {
            connector_info: ConnectorInfo {
                connector: "Meta-0".into(),
                vendor: "virtual".into(),
                product: "virtual".into(),
                serial: "virtual".into(),
            },
            modes: vec![],
            properties: HashMap::new(),
        });
        assert!(validated_hash(&backup).is_ok());
        assert!(topology_matches(
            &backup,
            std::slice::from_ref(&logical),
            &[]
        ));
        assert_eq!(
            restored_output_hash(&backup, &monitors, std::slice::from_ref(&logical)),
            backup.configuration_hash
        );
        let mut with_disabled_hdmi = backup.clone();
        let disabled_hdmi = CanonicalOutputBackup {
            connector: "HDMI-1".into(),
            vendor: "vendor".into(),
            product: "screen".into(),
            serial: "external".into(),
            mode_id: "mode-2".into(),
            width: 1920,
            height: 1080,
            refresh_rate: 60.0,
            enabled: false,
        };
        with_disabled_hdmi.outputs.push(OutputBackup {
            connector: disabled_hdmi.connector.clone(),
            vendor: disabled_hdmi.vendor.clone(),
            product: disabled_hdmi.product.clone(),
            serial: disabled_hdmi.serial.clone(),
            mode_id: disabled_hdmi.mode_id.clone(),
            width: disabled_hdmi.width,
            height: disabled_hdmi.height,
            refresh_rate: disabled_hdmi.refresh_rate,
            enabled: Some(false),
        });
        with_disabled_hdmi.configuration_hash =
            Some(display_config::compute_hash(&[expected, disabled_hdmi]));
        monitors.push(MonitorEntry {
            connector_info: ConnectorInfo {
                connector: "HDMI-1".into(),
                vendor: "vendor".into(),
                product: "screen".into(),
                serial: "external".into(),
            },
            modes: vec![],
            properties: HashMap::new(),
        });
        assert!(validated_hash(&with_disabled_hdmi).is_ok());
        let serialized = serde_json::to_vec(&with_disabled_hdmi).unwrap();
        let parsed: DisplayBackup = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(validated_hash(&parsed).unwrap(), parsed.configuration_hash);
        assert_eq!(
            restored_output_hash(&parsed, &monitors, std::slice::from_ref(&logical)),
            parsed.configuration_hash
        );
        monitors[0].modes[0].width = 2560;
        assert_ne!(
            restored_output_hash(&backup, &monitors, std::slice::from_ref(&logical)),
            backup.configuration_hash
        );
        let hdmi = LogicalMonitorEntry {
            x: 1920,
            y: 0,
            scale: 1.0,
            transform: 0,
            primary: false,
            monitors: vec![ConnectorInfo {
                connector: "HDMI-1".into(),
                vendor: "vendor".into(),
                product: "screen".into(),
                serial: "external".into(),
            }],
            properties: HashMap::new(),
        };
        assert!(!topology_matches(&backup, &[logical, hdmi], &[]));
    }

    #[test]
    fn keep_live_virtual_adds_the_virtual_monitor_and_falls_back_on_scaling() {
        let panel = ConnectorInfo {
            connector: "eDP-1".into(),
            vendor: "vendor".into(),
            product: "panel".into(),
            serial: "internal".into(),
        };
        let backup = DisplayBackup {
            session_id: "3".into(),
            shell_pid: 1,
            outputs: vec![OutputBackup {
                connector: "eDP-1".into(),
                vendor: "vendor".into(),
                product: "panel".into(),
                serial: "internal".into(),
                mode_id: "mode-1".into(),
                width: 1920,
                height: 1080,
                refresh_rate: 60.0,
                enabled: Some(true),
            }],
            topology: vec![LogicalMonitorBackup {
                x: 0,
                y: 0,
                scale: 1.0,
                transform: 0,
                primary: true,
                monitors: vec![("eDP-1".into(), "internal".into())],
            }],
            primary_output: None,
            hash_version: None,
            configuration_hash: None,
        };
        let virtual_monitor = |connector: &str, vendor: &str| MonitorEntry {
            connector_info: ConnectorInfo {
                connector: connector.into(),
                vendor: vendor.into(),
                product: "Virtual remote monitor".into(),
                serial: "0x1".into(),
            },
            modes: vec![ModeInfo {
                id: "v-mode".into(),
                width: 1280,
                height: 720,
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
        let monitors = vec![virtual_monitor("Meta-0", "MetaVendor")];

        let (plain, kept) = restore_config(&backup, &monitors, false).unwrap();
        assert_eq!((plain.len(), kept.len()), (1, 0));
        let (config, kept) = restore_config(&backup, &monitors, true).unwrap();
        assert_eq!(kept, ["Meta-0"]);
        assert_eq!((config.len(), config[1].x, config[1].y), (2, 1920, 0));
        assert!(!config[1].primary);
        assert_eq!(config[1].monitors[0].mode_id, "v-mode");
        let lookalike = vec![virtual_monitor("HDMI-1", "MetaVendor")];
        assert!(
            restore_config(&backup, &lookalike, true)
                .unwrap()
                .1
                .is_empty()
        );

        let mut scaled = backup.clone();
        scaled.topology[0].scale = 1.25;
        let (fallback, kept) = restore_config(&scaled, &monitors, true).unwrap();
        assert_eq!((fallback.len(), kept.len()), (1, 0));
        let mut rotated = backup.clone();
        rotated.topology[0].transform = 1;
        assert!(
            restore_config(&rotated, &monitors, true)
                .unwrap()
                .1
                .is_empty()
        );

        let two = vec![
            virtual_monitor("Meta-0", "MetaVendor"),
            virtual_monitor("Meta-1", "MetaVendor"),
        ];
        let (config, kept) = restore_config(&backup, &two, true).unwrap();
        assert_eq!(kept, ["Meta-0", "Meta-1"]);
        assert_eq!((config[1].x, config[2].x), (1920, 3200));
        let mut known = backup.clone();
        known.outputs.push(OutputBackup {
            connector: "Meta-0".into(),
            vendor: "MetaVendor".into(),
            product: "Virtual remote monitor".into(),
            serial: "0x1".into(),
            mode_id: "v-mode".into(),
            width: 1280,
            height: 720,
            refresh_rate: 60.0,
            enabled: Some(false),
        });
        assert!(
            restore_config(&known, &monitors, true)
                .unwrap()
                .1
                .is_empty()
        );
        let mut idle = virtual_monitor("Meta-0", "MetaVendor");
        idle.modes[0].properties.clear();
        assert!(restore_config(&backup, &[idle], true).unwrap().1.is_empty());

        let logical = |identity: ConnectorInfo, x: i32| LogicalMonitorEntry {
            x,
            y: 0,
            scale: 1.0,
            transform: 0,
            primary: x == 0,
            monitors: vec![identity],
            properties: HashMap::new(),
        };
        let live = [
            logical(panel, 0),
            logical(monitors[0].connector_info.clone(), 1920),
        ];
        assert!(!topology_matches(&backup, &live, &[]));
        assert!(topology_matches(&backup, &live, &["Meta-0".to_string()]));
        assert!(!topology_matches(
            &backup,
            &live[1..],
            &["Meta-0".to_string()]
        ));
    }

    #[test]
    fn restore_refuses_missing_or_changed_origin() {
        let mut backup = DisplayBackup {
            session_id: "3".to_string(),
            shell_pid: 34735,
            outputs: Vec::new(),
            topology: Vec::new(),
            primary_output: None,
            hash_version: None,
            configuration_hash: None,
        };
        assert!(verify_identity(&backup, "3", 34735).is_ok());
        assert!(verify_identity(&backup, "4", 34735).is_err());
        assert!(verify_identity(&backup, "3", 34736).is_err());
        backup.session_id.clear();
        assert!(verify_identity(&backup, "", 34735).is_err());
        backup.session_id = "3".to_string();
        backup.shell_pid = 0;
        assert!(verify_identity(&backup, "3", 0).is_err());
        assert!(serde_json::from_str::<DisplayBackup>(r#"{"outputs":[],"topology":[]}"#).is_err());
    }
}
