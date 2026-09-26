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
use std::io::BufRead;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, evidence_dir, redact, write_evidence,
};
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

#[cfg(test)]
mod tests {
    use super::*;

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

fn build_backup(monitors: &[MonitorEntry], logical: &[LogicalMonitorEntry]) -> DisplayBackup {
    let outputs = monitors
        .iter()
        .filter_map(|m| {
            let mode = m
                .modes
                .iter()
                .find(|mode| is_current_mode(&mode.properties))?;
            Some(OutputBackup {
                connector: m.connector_info.connector.clone(),
                vendor: m.connector_info.vendor.clone(),
                product: m.connector_info.product.clone(),
                serial: m.connector_info.serial.clone(),
                mode_id: mode.id.clone(),
                width: mode.width,
                height: mode.height,
                refresh_rate: mode.refresh_rate,
            })
        })
        .collect();
    let topology = logical
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
    DisplayBackup { outputs, topology }
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
        match read_state(self.conn) {
            Ok((serial, ..)) => {
                if let Err(error) = apply_monitors_config(self.conn, serial, &self.original) {
                    eprintln!(
                        "CRITICAL: RestoreGuard failed to restore original display topology: {error}"
                    );
                }
            }
            Err(error) => {
                eprintln!(
                    "CRITICAL: RestoreGuard failed to read current state before restoring: {error}"
                );
            }
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
    cycles: Vec<CycleFindings>,
    stop_error: Option<String>,
    virtual_connector_fully_gone: bool,
    paused_for_manual_kill_test: bool,
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

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    let redact_on = args.common.redact_enabled();
    let now = OffsetDateTime::now_utc();
    let dir = evidence_dir(EXP_ID, now)?;

    let conn = Connection::session()?;
    let host = host_info();

    // 1. Snapshot + persist (Decision 1/3: a separate process, exp07, must
    //    be able to restore from this file alone).
    let (_serial0, monitors0, logical0) = read_state(&conn)?;
    let backup = build_backup(&monitors0, &logical0);
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
            cycles: vec![],
            stop_error: None,
            virtual_connector_fully_gone: false,
            paused_for_manual_kill_test: false,
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
    arm_watchdog(&watchdog_unit, watchdog_seconds, &backup_path)?;
    let watchdog_armed = true;

    let mut restore_guard = RestoreGuard {
        conn: &conn,
        original: original_write_side.clone(),
        disarmed: false,
    };

    let (mut cycles, mut all_restored, mut paused) = (Vec::new(), true, false);

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
        println!(
            "Isolated. PID={}, backup={}",
            std::process::id(),
            backup_path.display()
        );
        println!("Press Enter to restore gracefully and exit, OR from a separate");
        println!(
            "terminal run: kill -9 {} ; then run exp07_restore --backup {}",
            std::process::id(),
            backup_path.display()
        );
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        // Actually restore now (bug found live 2026-09-05: this branch
        // previously fell through without restoring at all, and
        // `all_restored`'s unmutated `true` default wrongly disarmed
        // RestoreGuard — the operator had to restore manually over SSH).
        let restore_error = (|| -> anyhow::Result<()> {
            let (serial, ..) = read_state(&conn)?;
            apply_monitors_config(&conn, serial, &original_write_side)?;
            set_power_save_mode(&conn, POWER_SAVE_ON)
        })()
        .err();
        if let Some(error) = restore_error {
            eprintln!("Restore after pause failed: {error}");
            all_restored = false;
        } else {
            let expected_physical_count: usize =
                original_write_side.iter().map(|lm| lm.monitors.len()).sum();
            all_restored = read_state(&conn)
                .map(|(_serial, _monitors, logical)| {
                    physical_connectors_active(&logical, &virtual_connector).len()
                        == expected_physical_count
                        && logical.len() == original_write_side.len()
                })
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

    if all_restored {
        restore_guard.disarm();
        disarm_watchdog(&watchdog_unit);
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

    let findings = Findings {
        host,
        physical_connectors_before,
        virtual_connector: Some(virtual_connector),
        apply_monitors_config_allowed: apply_allowed,
        watchdog_armed,
        watchdog_unit: Some(watchdog_unit),
        cycles,
        stop_error,
        virtual_connector_fully_gone,
        paused_for_manual_kill_test: paused,
    };

    let result = if paused {
        ExperimentResult::Partial // isolate proven; restore path exercised separately (exp07)
    } else if all_restored
        && findings.stop_error.is_none()
        && findings.virtual_connector_fully_gone
        && findings
            .cycles
            .iter()
            .all(|c| c.apply_error.is_none() && c.physical_connectors_absent_from_logical_monitors)
    {
        ExperimentResult::Pass
    } else {
        ExperimentResult::Fail
    };

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
