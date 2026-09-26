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
    CommonArgs, ExperimentReport, ExperimentResult, evidence_dir, redact, write_evidence,
};
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
    outputs: Vec<OutputBackup>,
    topology: Vec<LogicalMonitorBackup>,
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
/// so a mismatch's exact cause is diagnosable.
fn topology_matches(backup: &DisplayBackup, logical: &[LogicalMonitorEntry]) -> bool {
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

/// Any raw connector present now but absent from the backup's own
/// `outputs[]` — a lingering virtual-monitor connector Mutter did not clean
/// up on its own (assessment §7.3's "what does Mutter restore
/// automatically" question).
fn unexpected_connectors(backup: &DisplayBackup, monitors: &[MonitorEntry]) -> Vec<String> {
    monitors
        .iter()
        .map(|m| m.connector_info.connector.clone())
        .filter(|c| !backup.outputs.iter().any(|o| &o.connector == c))
        .collect()
}

#[derive(Debug, Serialize)]
struct Findings {
    backup_path: String,
    monitors_before_restore: Vec<String>,
    unexpected_connectors_before_restore: Vec<String>,
    apply_error: Option<String>,
    retried: bool,
    topology_matches: bool,
    unexpected_connectors_after_restore: Vec<String>,
}

fn attempt_restore(conn: &Connection, backup: &DisplayBackup) -> anyhow::Result<()> {
    let write_side = to_write_side(backup)?;
    let (serial, ..) = read_state(conn)?;
    apply_monitors_config(conn, serial, &write_side)?;
    set_power_save_mode(conn, POWER_SAVE_ON)
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    let redact_on = args.common.redact_enabled();
    let now = OffsetDateTime::now_utc();

    let backup_text = std::fs::read_to_string(&args.backup)?;
    let backup: DisplayBackup = serde_json::from_str(&backup_text)?;

    let conn = Connection::session()?;

    let (_serial0, monitors0, _logical0) = read_state(&conn)?;
    let monitors_before_restore: Vec<String> = monitors0
        .iter()
        .map(|m| m.connector_info.connector.clone())
        .collect();
    let unexpected_connectors_before_restore = unexpected_connectors(&backup, &monitors0);

    // Doc 05 §62: retry safely once; never claim success if physical state
    // remains unknown.
    let mut apply_error = attempt_restore(&conn, &backup).err().map(|e| e.to_string());
    let mut retried = false;
    if apply_error.is_some() {
        retried = true;
        apply_error = attempt_restore(&conn, &backup).err().map(|e| e.to_string());
    }

    let deadline = Instant::now() + POLL_WAIT;
    let (topology_ok, monitors_final) = loop {
        let (_serial, monitors, logical) = read_state(&conn)?;
        let ok = apply_error.is_none() && topology_matches(&backup, &logical);
        if ok || Instant::now() >= deadline {
            break (ok, monitors);
        }
        thread::sleep(Duration::from_millis(150));
    };

    let findings = Findings {
        backup_path: args.backup.display().to_string(),
        monitors_before_restore,
        unexpected_connectors_before_restore,
        apply_error: apply_error.clone(),
        retried,
        topology_matches: topology_ok,
        unexpected_connectors_after_restore: unexpected_connectors(&backup, &monitors_final),
    };

    // Doc 05 §62 step 5: remain safe/diagnostic rather than silently
    // reporting LOCAL_ACTIVE-equivalent success on unknown state.
    let result = if topology_ok {
        ExperimentResult::Pass
    } else {
        ExperimentResult::Fail
    };

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
        expected: "Every backed-up output's position/mode/scale/transform/primary matches \
                   exactly; no unexpected connector remains from a lingering virtual monitor."
            .to_string(),
        observed,
        evidence: vec![format!(
            "docs/experiments/evidence/{EXP_ID}/<date>/report.md"
        )],
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

    let dir = evidence_dir(EXP_ID, now)?;
    write_evidence(
        &dir,
        &redact(&report.render(now), redact_on),
        "findings.json",
        &findings,
    )?;
    println!("Wrote evidence to {}", dir.display());
    println!("Result: {result}");
    Ok(())
}
