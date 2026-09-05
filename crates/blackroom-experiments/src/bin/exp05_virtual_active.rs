//! Experiment 5 — Virtual Monitor as Active Display (Document 10 §12 /
//! Document 02 §11).
//!
//! Adds the virtual monitor (created via `RecordVirtual`, Experiment 4's
//! proven mechanics) as an **additional** enabled logical monitor via
//! `ApplyMonitorsConfig` (method = Temporary) alongside the existing
//! physical monitor(s) — never removing/disabling any physical monitor from
//! the topology (Phase 4 plan Decision #3: the narrower
//! "zero-physical-monitor" question is Phase 5's Experiment 6, under its own
//! full safety procedure). The original topology is captured before any
//! change and restored via an RAII guard that runs even on an early error
//! return (Rust drops locals on `?`-propagated returns, no panic needed).
//!
//! Requires `gnome-remote-desktop.service` masked
//! (`docs/ops/experiment-safety.md` §5).

use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, evidence_dir, redact, write_evidence,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Type, Value};

const EXP_ID: &str = "exp05";
const SIGNAL_WAIT: Duration = Duration::from_secs(10);
/// Mutter's `ApplyMonitorsConfig` method enum: 0 = Verify, 1 = Temporary,
/// 2 = Persistent. This project never uses Persistent (Doc 02 §11: "do not
/// permanently modify the user's display configuration").
const METHOD_TEMPORARY: u32 = 1;

#[derive(Parser, Debug)]
#[command(about = "Experiment 5: make a virtual monitor an additional active display")]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
}

// ---------------------------------------------------------------------
// DisplayConfig.GetCurrentState read-side types (established per-file
// duplication pattern — see capability.rs/exp02/exp03/exp04/virtual_monitor.rs).
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Type, Deserialize)]
struct ConnectorInfo {
    connector: String,
    #[allow(dead_code)]
    vendor: String,
    #[allow(dead_code)]
    product: String,
    #[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Clone, Type, Deserialize)]
struct MonitorEntry {
    connector_info: ConnectorInfo,
    modes: Vec<ModeInfo>,
    properties: HashMap<String, OwnedValue>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Type, Deserialize)]
struct LogicalMonitorEntryRead {
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
    Vec<LogicalMonitorEntryRead>,
    HashMap<String, OwnedValue>,
);

fn is_current_mode(properties: &HashMap<String, OwnedValue>) -> bool {
    properties
        .get("is-current")
        .and_then(|value| bool::try_from(value.clone()).ok())
        .unwrap_or(false)
}

fn read_state(
    conn: &Connection,
) -> anyhow::Result<(u32, Vec<MonitorEntry>, Vec<LogicalMonitorEntryRead>)> {
    let proxy = Proxy::new(
        conn,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )?;
    let (serial, monitors, logical_monitors, _props): GetCurrentStateResult =
        proxy.call("GetCurrentState", &())?;
    Ok((serial, monitors, logical_monitors))
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

fn mode_width(monitors: &[MonitorEntry], connector: &str) -> Option<i32> {
    monitors
        .iter()
        .find(|m| m.connector_info.connector == connector)
        .and_then(|m| {
            m.modes
                .iter()
                .find(|mode| is_current_mode(&mode.properties))
        })
        .map(|mode| mode.width)
}

// ---------------------------------------------------------------------
// ApplyMonitorsConfig write-side types (`a(iiduba(ssa{sv}))`).
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

/// Reconstructs the write-side config exactly matching the current
/// read-side topology (same connectors/positions/scale), resolving each
/// connector's write-side `mode_id` from its current mode. Used both to
/// build the "keep physical unchanged" baseline and to restore afterward.
fn to_write_side(
    monitors: &[MonitorEntry],
    logical: &[LogicalMonitorEntryRead],
) -> anyhow::Result<Vec<LogicalMonitorConfig>> {
    logical
        .iter()
        .map(|lm| {
            let monitor_refs = lm
                .monitors
                .iter()
                .map(|connector_info| {
                    let mode_id =
                        current_mode_id(monitors, &connector_info.connector).ok_or_else(|| {
                            anyhow::anyhow!(
                                "no current mode for connector {}",
                                connector_info.connector
                            )
                        })?;
                    Ok(MonitorRef {
                        connector: connector_info.connector.clone(),
                        mode_id,
                        properties: HashMap::new(),
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            Ok(LogicalMonitorConfig {
                x: lm.x,
                y: lm.y,
                scale: lm.scale,
                transform: lm.transform,
                primary: lm.primary,
                monitors: monitor_refs,
            })
        })
        .collect()
}

fn apply_monitors_config(
    conn: &Connection,
    serial: u32,
    logical_monitors: &[LogicalMonitorConfig],
) -> anyhow::Result<()> {
    let proxy = Proxy::new(
        conn,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )?;
    let empty_props: HashMap<&str, Value<'_>> = HashMap::new();
    proxy.call::<_, _, ()>(
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

/// Restores `original` on drop unless [`RestoreGuard::disarm`] was already
/// called after a verified successful explicit restore. Runs even on an
/// early `?`-propagated error return (no panic/unwind needed for that case).
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

/// Stops the ScreenCast session at `path` on drop unless [`SessionStopGuard::disarm`]
/// was called first. Armed immediately after `CreateSession` succeeds
/// (independent-review finding: arming cleanup only after later fallible
/// steps such as `RecordVirtual`/waiting for the PipeWire node/polling for
/// the new connector left an early-error path with no cleanup coverage at
/// all for the created session).
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
// Virtual monitor creation (Experiment 4's proven mechanics, single-shot).
// ---------------------------------------------------------------------

mod pipewire_probe {
    // Identical to Experiment 4's probe (`exp04_virtual_monitor.rs`) —
    // duplicated per this project's established per-experiment-binary
    // pattern rather than shared, since experiments are discardable
    // scaffolding (architecture.md §1).
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
            "blackroom-exp05-probe",
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

/// Polls `GetCurrentState` until a connector absent from `before` appears
/// (Experiment 4 finding: not synchronous with `Start()`).
fn poll_for_new_connector(
    conn: &Connection,
    before: &[String],
) -> anyhow::Result<(String, Vec<MonitorEntry>)> {
    let deadline = std::time::Instant::now() + SIGNAL_WAIT;
    loop {
        let (_serial, monitors, _logical) = read_state(conn)?;
        if let Some(connector) = monitors
            .iter()
            .map(|m| m.connector_info.connector.clone())
            .find(|c| !before.contains(c))
        {
            return Ok((connector, monitors));
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("no new connector appeared in GetCurrentState within timeout");
        }
        thread::sleep(Duration::from_millis(150));
    }
}

/// Polls `GetCurrentState` until `connector` is absent from the top-level
/// monitors inventory, or `SIGNAL_WAIT` elapses (whichever comes first;
/// times out silently rather than erroring, since the caller records
/// whatever the final observed state is either way).
fn poll_for_connector_gone(conn: &Connection, connector: &str) -> anyhow::Result<Vec<String>> {
    let deadline = std::time::Instant::now() + SIGNAL_WAIT;
    loop {
        let (_serial, monitors, _logical) = read_state(conn)?;
        let names: Vec<String> = monitors
            .iter()
            .map(|m| m.connector_info.connector.clone())
            .collect();
        if !names.iter().any(|c| c == connector) || std::time::Instant::now() >= deadline {
            return Ok(names);
        }
        thread::sleep(Duration::from_millis(150));
    }
}

#[derive(Debug, Serialize)]
struct Findings {
    physical_connectors_before: Vec<String>,
    virtual_connector: Option<String>,
    logical_monitor_count_before: usize,
    logical_monitor_count_after_apply: Option<usize>,
    apply_error: Option<String>,
    virtual_logical_monitor_present: bool,
    physical_connectors_still_present_after_apply: bool,
    render_frames_received: u32,
    restore_error: Option<String>,
    topology_restored: bool,
    physical_connectors_after_restore: Vec<String>,
    virtual_connector_fully_gone: bool,
    stop_error: Option<String>,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    let redact_on = args.common.redact_enabled();
    let now = OffsetDateTime::now_utc();

    let conn = Connection::session()?;

    // 1. Snapshot original topology.
    let (_serial0, monitors0, logical0) = read_state(&conn)?;
    let original_write_side = to_write_side(&monitors0, &logical0)?;
    let physical_connectors_before: Vec<String> = monitors0
        .iter()
        .map(|m| m.connector_info.connector.clone())
        .collect();

    // 2. Create the virtual monitor (single-shot, 1920x1080@60 — reliability
    //    across resolutions/cycles was already proven by Experiment 4).
    let (width, height, refresh_rate) = (1920_i32, 1080_i32, 60.0_f64);
    let screencast_proxy = Proxy::new(
        &conn,
        "org.gnome.Mutter.ScreenCast",
        "/org/gnome/Mutter/ScreenCast",
        "org.gnome.Mutter.ScreenCast",
    )?;
    let session_path: OwnedObjectPath =
        screencast_proxy.call("CreateSession", &(HashMap::<&str, Value<'_>>::new(),))?;
    // Armed immediately (Reviewer finding): every fallible step from here on
    // (RecordVirtual, waiting for the PipeWire node, polling for the new
    // connector) is now covered by session cleanup even on early error.
    let mut session_guard = SessionStopGuard {
        conn: &conn,
        path: session_path.clone(),
        disarmed: false,
    };
    let session_proxy = Proxy::new(
        &conn,
        "org.gnome.Mutter.ScreenCast",
        session_path.as_ref(),
        "org.gnome.Mutter.ScreenCast.Session",
    )?;
    let record_props: HashMap<&str, Value<'_>> = HashMap::from([
        ("width", Value::from(width)),
        ("height", Value::from(height)),
        ("framerate", Value::from(refresh_rate)),
    ]);
    let stream_path: OwnedObjectPath = session_proxy.call("RecordVirtual", &(record_props,))?;
    let stream_proxy = Proxy::new(
        &conn,
        "org.gnome.Mutter.ScreenCast",
        stream_path.as_ref(),
        "org.gnome.Mutter.ScreenCast.Stream",
    )?;
    let node_id = wait_for_pipewire_node(&session_proxy, &stream_proxy)?;
    pipewire_probe::receive_a_few_frames(node_id, width, height).ok();
    let (virtual_connector, monitors1) =
        poll_for_new_connector(&conn, &physical_connectors_before)?;

    // 3. Build the new logical-monitors config: original entries unchanged,
    //    plus one new entry for the virtual connector placed to the right
    //    of the existing layout's rightmost extent.
    let offset_x = logical0
        .iter()
        .flat_map(|lm| {
            lm.monitors
                .iter()
                .map(|c| lm.x + mode_width(&monitors0, &c.connector).unwrap_or(0))
        })
        .max()
        .unwrap_or(0);
    let virtual_mode_id = current_mode_id(&monitors1, &virtual_connector)
        .ok_or_else(|| anyhow::anyhow!("no current mode reported for the virtual connector"))?;
    let mut new_logical = original_write_side.clone();
    new_logical.push(LogicalMonitorConfig {
        x: offset_x,
        y: 0,
        scale: 1.0,
        transform: 0,
        primary: false,
        monitors: vec![MonitorRef {
            connector: virtual_connector.clone(),
            mode_id: virtual_mode_id,
            properties: HashMap::new(),
        }],
    });

    // 4. Apply — arm the restore guard first so any failure below still
    //    restores the original topology.
    let mut guard = RestoreGuard {
        conn: &conn,
        original: original_write_side.clone(),
        disarmed: false,
    };
    let (serial1, ..) = read_state(&conn)?;
    let apply_error = apply_monitors_config(&conn, serial1, &new_logical)
        .err()
        .map(|e| e.to_string());

    // 5. Verify the applied topology.
    let (_serial2, monitors2, logical2) = read_state(&conn)?;
    let virtual_logical_monitor_present = logical2
        .iter()
        .any(|lm| lm.monitors.iter().any(|m| m.connector == virtual_connector));
    let physical_connectors_still_present_after_apply = physical_connectors_before
        .iter()
        .all(|c| monitors2.iter().any(|m| &m.connector_info.connector == c));

    // 6. Prove rendering: capture the virtual connector now that it is an
    //    active logical monitor (not just a raw capture stream).
    let render_frames_received = (|| -> anyhow::Result<u32> {
        let render_session_path: OwnedObjectPath =
            screencast_proxy.call("CreateSession", &(HashMap::<&str, Value<'_>>::new(),))?;
        let render_session_proxy = Proxy::new(
            &conn,
            "org.gnome.Mutter.ScreenCast",
            render_session_path.as_ref(),
            "org.gnome.Mutter.ScreenCast.Session",
        )?;
        let render_stream_path: OwnedObjectPath = render_session_proxy.call(
            "RecordMonitor",
            &(
                virtual_connector.as_str(),
                HashMap::<&str, Value<'_>>::new(),
            ),
        )?;
        let render_stream_proxy = Proxy::new(
            &conn,
            "org.gnome.Mutter.ScreenCast",
            render_stream_path.as_ref(),
            "org.gnome.Mutter.ScreenCast.Stream",
        )?;
        let render_node_id = wait_for_pipewire_node(&render_session_proxy, &render_stream_proxy)?;
        let frames = pipewire_probe::receive_a_few_frames(render_node_id, width, height)?;
        render_session_proxy.call::<_, _, ()>("Stop", &()).ok();
        Ok(frames)
    })()
    .unwrap_or(0);

    // 7. Restore the original topology explicitly (captures success/failure
    //    as evidence); the guard remains armed as a last-resort safety net
    //    if this explicit restore does not fully verify.
    let (serial3, ..) = read_state(&conn)?;
    let restore_error = apply_monitors_config(&conn, serial3, &original_write_side)
        .err()
        .map(|e| e.to_string());
    let (_serial3b, _monitors3b, logical3b) = read_state(&conn)?;
    let topology_restored = restore_error.is_none()
        && logical3b.len() == logical0.len()
        && !logical3b
            .iter()
            .any(|lm| lm.monitors.iter().any(|m| m.connector == virtual_connector));
    if topology_restored {
        guard.disarm();
    }
    drop(guard);

    // 8. Tear down the virtual monitor's ScreenCast session (Reviewer
    //    finding: this must happen, and be checked, *before* recording the
    //    final connector snapshot below — otherwise the evidence shows the
    //    virtual connector still present with no explanation, and a Stop()
    //    failure would go unnoticed by the pass/fail predicate).
    let stop_error = session_proxy
        .call::<_, _, ()>("Stop", &())
        .err()
        .map(|e| e.to_string());
    if stop_error.is_none() {
        session_guard.disarm();
    }
    drop(session_guard);

    // 9. Final snapshot, polled until the virtual connector disappears from
    //    the raw inventory (its removal is not synchronous with `Stop()`
    //    returning, symmetric to Experiment 4's "appears only after a real
    //    PipeWire client connects" finding — confirmed live: an immediate
    //    post-`Stop()` snapshot still showed the connector present).
    let physical_connectors_after_restore = poll_for_connector_gone(&conn, &virtual_connector)?;
    let virtual_connector_fully_gone =
        !physical_connectors_after_restore.contains(&virtual_connector);

    let findings = Findings {
        physical_connectors_before: physical_connectors_before.clone(),
        virtual_connector: Some(virtual_connector.clone()),
        logical_monitor_count_before: logical0.len(),
        logical_monitor_count_after_apply: Some(logical2.len()),
        apply_error,
        virtual_logical_monitor_present,
        physical_connectors_still_present_after_apply,
        render_frames_received,
        restore_error,
        topology_restored,
        physical_connectors_after_restore,
        virtual_connector_fully_gone,
        stop_error,
    };

    let result = if findings.apply_error.is_none()
        && findings.virtual_logical_monitor_present
        && findings.physical_connectors_still_present_after_apply
        && findings.render_frames_received > 0
        && findings.topology_restored
        && findings.stop_error.is_none()
        && findings.virtual_connector_fully_gone
    {
        ExperimentResult::Pass
    } else if findings.topology_restored && findings.stop_error.is_none() {
        ExperimentResult::Partial
    } else {
        ExperimentResult::Fail
    };

    let observed = redact(&format!("{findings:#?}"), redact_on);

    let report = ExperimentReport {
        experiment: "Experiment 5 — Virtual Monitor as Active Display".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1"
            .to_string(),
        objective: "Make the virtual monitor an additional active display alongside the \
                    existing physical monitor(s), without disabling any physical monitor \
                    (Document 10 §12, Document 02 §11)."
            .to_string(),
        hypothesis: "ApplyMonitorsConfig(method=Temporary) can add the virtual monitor's \
                     connector as a new logical monitor while leaving existing physical \
                     logical monitors unchanged; the compositor then renders the desktop on \
                     it (provable by capturing it via RecordMonitor); the original topology \
                     is fully restorable."
            .to_string(),
        procedure: "Snapshot GetCurrentState; create a virtual monitor (Experiment 4 \
                    mechanics); build a new logical-monitors array = original entries + one \
                    new entry for the virtual connector; ApplyMonitorsConfig(Temporary); \
                    verify; RecordMonitor the virtual connector and receive frames; restore \
                    the original array; verify restoration; tear down."
            .to_string(),
        expected: "The virtual monitor becomes a second active logical monitor, physical \
                   monitors remain enabled and unchanged, real frames are captured from the \
                   virtual connector, and the original topology is exactly restored."
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
            "gnome-remote-desktop.service must stay masked for this run (docs/ops/\
             experiment-safety.md §5); ApplyMonitorsConfig(Temporary) never persists to \
             monitors.xml, and this experiment explicitly restores the original topology \
             before exiting (RAII guard covers early-error paths too)."
                .to_string(),
        ),
        recommended_action: None,
        follow_up: Some(
            "Physical-monitor removal (Doc 02 §11's 'can be removed from the active \
             topology') is deferred to Phase 5 Experiment 6 under its full safety procedure \
             (Phase 4 plan Decision #3) — not attempted here."
                .to_string(),
        ),
    };

    let dir = evidence_dir(EXP_ID, now)?;
    write_evidence(
        &dir,
        &redact(&report.render(now), redact_on),
        "virtual_active.json",
        &findings,
    )?;
    println!("Wrote evidence to {}", dir.display());
    println!("Result: {result}");

    Ok(())
}
