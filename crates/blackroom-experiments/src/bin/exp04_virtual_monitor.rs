//! Experiment 4 — Virtual Monitor Creation (Document 10 §11 / Document 02 §10).
//!
//! Creates a real virtual monitor via `RecordVirtual` at 1280×720,
//! 1920×1080, and 2560×1440 (60 Hz), confirms each via
//! `DisplayConfig.GetCurrentState` (a real monitor, not just a capture
//! stream), destroys it cleanly, then runs a bounded 50-cycle create/destroy
//! reliability loop (Doc 19 §16–17) diffing `pw-dump` output and checking
//! the journal for a GNOME Shell crash/restart.
//!
//! `RecordVirtual`'s properties-dict schema is not documented anywhere in
//! this project's evidence (`feasibility-research.md` topic 2: the method
//! is session-scoped, never introspectable before Phase 4) — this
//! experiment discovers it empirically rather than assuming it.
//!
//! Requires `gnome-remote-desktop.service` masked first
//! (`docs/ops/experiment-safety.md` §5).

use std::collections::HashMap;
use std::process::Command;
use std::thread;
use std::time::Duration;

use blackroom_experiments::{
    ExperimentReport, ExperimentResult, evidence_dir, redact, write_evidence,
};
use clap::Parser;
use serde::Serialize;
use time::OffsetDateTime;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Type, Value};

use blackroom_experiments::CommonArgs;

const EXP_ID: &str = "exp04";
const SIGNAL_WAIT: Duration = Duration::from_secs(10);
/// Doc 19 §16-17 / experiment-safety.md §6 bounded per-cycle timeout.
const CYCLE_TIMEOUT: Duration = Duration::from_secs(10);
const CYCLE_COUNT: u32 = 50;

#[derive(Parser, Debug)]
#[command(about = "Experiment 4: create/detect/destroy a real virtual monitor via RecordVirtual")]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Skip the 50-cycle reliability loop (for fast iteration while
    /// discovering RecordVirtual's real property schema).
    #[arg(long, default_value_t = false)]
    skip_cycles: bool,
}

// ---------------------------------------------------------------------
// DisplayConfig.GetCurrentState (read-only snapshot/confirmation).
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

#[derive(Debug, Clone, Serialize)]
struct ConnectorMode {
    connector: String,
    width: i32,
    height: i32,
    refresh_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
struct StateSnapshot {
    connectors: Vec<String>,
    logical_monitor_count: usize,
    modes: Vec<ConnectorMode>,
}

fn is_current_mode(properties: &HashMap<String, OwnedValue>) -> bool {
    properties
        .get("is-current")
        .and_then(|value| bool::try_from(value.clone()).ok())
        .unwrap_or(false)
}

fn snapshot_state(conn: &Connection) -> anyhow::Result<StateSnapshot> {
    let proxy = Proxy::new(
        conn,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )?;
    let (_serial, monitors, logical_monitors, _props): GetCurrentStateResult =
        proxy.call("GetCurrentState", &())?;
    let modes = monitors
        .iter()
        .filter_map(|m| {
            m.modes
                .iter()
                .find(|mode| is_current_mode(&mode.properties))
                .map(|mode| ConnectorMode {
                    connector: m.connector_info.connector.clone(),
                    width: mode.width,
                    height: mode.height,
                    refresh_rate: mode.refresh_rate,
                })
        })
        .collect();
    Ok(StateSnapshot {
        connectors: monitors
            .into_iter()
            .map(|m| m.connector_info.connector)
            .collect(),
        logical_monitor_count: logical_monitors.len(),
        modes,
    })
}

/// The new connector name(s) present in `after` but not `before`.
fn new_connectors(before: &StateSnapshot, after: &StateSnapshot) -> Vec<String> {
    after
        .connectors
        .iter()
        .filter(|c| !before.connectors.contains(c))
        .cloned()
        .collect()
}

/// Registering/removing a virtual monitor's logical-monitor entry is not
/// synchronous with `Start()`/`Stop()` returning (empirically confirmed: a
/// snapshot taken immediately after `Start()` missed the new connector that
/// a later snapshot then showed). Polls `GetCurrentState` up to
/// `SIGNAL_WAIT` rather than listening for `DisplayConfig.MonitorsChanged`
/// concurrently with the `PipeWireStreamAdded` wait — running two blocking
/// signal iterators on the same `zbus::blocking::Connection` from separate
/// threads at once deadlocked in practice (confirmed live: the process hung
/// past a 90s bound with `SIGNAL_WAIT = 10s`, so this is a real, not merely
/// slow, contention issue, not a documented zbus limitation found in
/// advance).
fn poll_until_connector_count_changes(
    conn: &Connection,
    before_count: usize,
) -> anyhow::Result<StateSnapshot> {
    let deadline = std::time::Instant::now() + SIGNAL_WAIT;
    loop {
        let snapshot = snapshot_state(conn)?;
        if snapshot.connectors.len() != before_count || std::time::Instant::now() >= deadline {
            return Ok(snapshot);
        }
        thread::sleep(Duration::from_millis(150));
    }
}

// ---------------------------------------------------------------------
// Virtual monitor create/confirm/destroy for one resolution.
// ---------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ResolutionResult {
    width: i32,
    height: i32,
    refresh_rate: f64,
    record_virtual_error: Option<String>,
    new_connectors_after_create: Vec<String>,
    observed_mode: Option<ConnectorMode>,
    resolution_honored: bool,
    frames_received: u32,
    stop_error: Option<String>,
    new_connectors_after_destroy: Vec<String>,
    monitor_confirmed: bool,
    teardown_confirmed: bool,
}

struct SessionStopGuard<'a> {
    conn: &'a Connection,
    path: OwnedObjectPath,
    disarmed: bool,
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
            eprintln!("Failed to stop ScreenCast session {}: {error}", self.path);
        }
    }
}

/// Tries a small set of candidate property-key spellings for `width`/
/// `height`/`framerate` in one call each, in order, since Mutter's
/// `RecordVirtual` properties-dict schema is not documented anywhere in
/// this project's evidence. Returns the first that does not error.
fn try_record_virtual(
    session_proxy: &Proxy<'_>,
    width: i32,
    height: i32,
    refresh_rate: f64,
) -> (Option<OwnedObjectPath>, Vec<(String, String)>) {
    let candidates: Vec<HashMap<&str, Value<'_>>> = vec![
        HashMap::from([
            ("width", Value::from(width)),
            ("height", Value::from(height)),
            ("framerate", Value::from(refresh_rate)),
        ]),
        HashMap::from([
            ("width", Value::from(width)),
            ("height", Value::from(height)),
        ]),
        HashMap::new(),
    ];
    let mut attempts = Vec::new();
    for props in candidates {
        let label = format!("{props:?}");
        let result: zbus::Result<OwnedObjectPath> = session_proxy.call("RecordVirtual", &(props,));
        match result {
            Ok(path) => {
                attempts.push((label, "ok".to_string()));
                return (Some(path), attempts);
            }
            Err(error) => attempts.push((label, error.to_string())),
        }
    }
    (None, attempts)
}

fn one_cycle(
    conn: &Connection,
    width: i32,
    height: i32,
    refresh_rate: f64,
) -> anyhow::Result<ResolutionResult> {
    let before = snapshot_state(conn)?;

    let screencast_proxy = Proxy::new(
        conn,
        "org.gnome.Mutter.ScreenCast",
        "/org/gnome/Mutter/ScreenCast",
        "org.gnome.Mutter.ScreenCast",
    )?;
    let empty_props: HashMap<&str, Value<'_>> = HashMap::new();
    let session_path: OwnedObjectPath = screencast_proxy.call("CreateSession", &(empty_props,))?;
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

    let (stream_path, attempts) = try_record_virtual(&session_proxy, width, height, refresh_rate);
    let record_virtual_error = if stream_path.is_some() {
        None
    } else {
        Some(format!("all candidates failed: {attempts:?}"))
    };

    let mut frames_received = 0_u32;
    let mut new_after_create = Vec::new();
    let mut observed_mode = None;
    if let Some(stream_path) = &stream_path {
        let stream_proxy = Proxy::new(
            conn,
            "org.gnome.Mutter.ScreenCast",
            stream_path.as_ref(),
            "org.gnome.Mutter.ScreenCast.Stream",
        )?;
        let mut pipewire_signal_iter = stream_proxy.receive_signal("PipeWireStreamAdded")?;

        session_proxy.call::<_, _, ()>("Start", &())?;

        let node_id = thread::scope(|scope| {
            let (tx_node, rx_node) = std::sync::mpsc::channel();
            scope.spawn(move || {
                if let Some(msg) = pipewire_signal_iter.next() {
                    let _ = tx_node.send(msg.body().deserialize::<(u32,)>().ok());
                }
            });
            rx_node.recv_timeout(SIGNAL_WAIT).ok().flatten()
        });

        // KEY FINDING: the virtual monitor did not appear in
        // `GetCurrentState` when polled immediately after `Start()`/
        // `PipeWireStreamAdded` (even with a 10s poll) — it only appeared
        // once a real PipeWire client actually connected and consumed the
        // stream. Receive frames FIRST, then poll for the connector.
        if let Some((node_id,)) = node_id {
            frames_received =
                crate::pipewire_probe::receive_a_few_frames(node_id, width, height).unwrap_or(0);
        }

        let after_create = poll_until_connector_count_changes(conn, before.connectors.len())?;
        new_after_create = new_connectors(&before, &after_create);
        observed_mode = new_after_create
            .first()
            .and_then(|connector| {
                after_create
                    .modes
                    .iter()
                    .find(|m| &m.connector == connector)
            })
            .cloned();
    }

    let stop_error = session_proxy.call::<_, _, ()>("Stop", &()).err();
    if stop_error.is_none() {
        session_guard.disarmed = true;
    }
    let stop_error = stop_error.map(|error| error.to_string());
    let after_create_count = before.connectors.len() + new_after_create.len();

    let after_destroy = poll_until_connector_count_changes(conn, after_create_count)?;
    let new_after_destroy = new_connectors(&before, &after_destroy);

    let resolution_honored = observed_mode
        .as_ref()
        .is_some_and(|m| m.width == width && m.height == height);

    Ok(ResolutionResult {
        width,
        height,
        refresh_rate,
        record_virtual_error,
        monitor_confirmed: !new_after_create.is_empty(),
        new_connectors_after_create: new_after_create,
        observed_mode,
        resolution_honored,
        frames_received,
        stop_error,
        teardown_confirmed: new_after_destroy.is_empty(),
        new_connectors_after_destroy: new_after_destroy,
    })
}

// ---------------------------------------------------------------------
// Doc 19 §16-17 reliability loop.
// ---------------------------------------------------------------------

/// KEY FINDING: a raw text diff of `pw-dump` output is unreliable — two
/// consecutive `pw-dump` invocations with zero experiment activity in
/// between already differ (confirmed live: `pw-dump`'s own diagnostic
/// client registers a fresh `application.process.id`/`object.serial`/
/// `core.name` every invocation). Instead, count only screencast-shaped
/// nodes (`media.class` starting with `"Stream/"` and containing `"Video"`
/// — this project's own webcam shows `"Video/Source"`, a different class,
/// so it does not false-positive) and compare the count, not raw text.
fn screencast_video_node_count() -> Option<usize> {
    let output = Command::new("pw-dump").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let count = parsed
        .as_array()?
        .iter()
        .filter(|object| {
            object.get("type").and_then(|t| t.as_str()) == Some("PipeWire:Interface:Node")
                && object
                    .get("info")
                    .and_then(|i| i.get("props"))
                    .and_then(|p| p.get("media.class"))
                    .and_then(|c| c.as_str())
                    .is_some_and(|class| class.starts_with("Stream/") && class.contains("Video"))
        })
        .count();
    Some(count)
}

/// GNOME Shell's own process start time (via `systemctl --user show`-style
/// PID lookup), to detect a crash/restart during the loop without assuming
/// a specific systemd unit name manages it (verified live: GNOME Shell is
/// not necessarily wrapped in its own distinct systemd user unit).
fn gnome_shell_pid() -> Option<String> {
    Command::new("pgrep")
        .args(["-x", "gnome-shell"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

#[derive(Debug, Serialize)]
struct CycleOutcome {
    cycle: u32,
    duration_ms: u128,
    timed_out: bool,
    error: Option<String>,
}

fn run_cycles(conn: &Connection, width: i32, height: i32, refresh_rate: f64) -> Vec<CycleOutcome> {
    let mut outcomes = Vec::with_capacity(CYCLE_COUNT as usize);
    for cycle in 0..CYCLE_COUNT {
        let start = std::time::Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::scope(|scope| {
            scope.spawn(|| {
                let result = one_cycle(conn, width, height, refresh_rate)
                    .map(|_| ())
                    .map_err(|e| e.to_string());
                let _ = tx.send(result);
            });
            match rx.recv_timeout(CYCLE_TIMEOUT) {
                Ok(result) => outcomes.push(CycleOutcome {
                    cycle,
                    duration_ms: start.elapsed().as_millis(),
                    timed_out: false,
                    error: result.err(),
                }),
                Err(_) => outcomes.push(CycleOutcome {
                    cycle,
                    duration_ms: start.elapsed().as_millis(),
                    timed_out: true,
                    error: Some(format!("cycle exceeded {CYCLE_TIMEOUT:?}")),
                }),
            }
        });
    }
    outcomes
}

fn classify_result(
    all_confirmed: bool,
    all_torn_down: bool,
    reliability_proven: Option<bool>,
) -> ExperimentResult {
    if all_confirmed && all_torn_down && reliability_proven == Some(true) {
        ExperimentResult::Pass
    } else if all_confirmed && all_torn_down {
        ExperimentResult::Partial
    } else {
        ExperimentResult::Fail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmeasured_reliability_cannot_pass() {
        assert_eq!(classify_result(true, true, None), ExperimentResult::Partial);
        assert_eq!(
            classify_result(true, true, Some(false)),
            ExperimentResult::Partial
        );
        assert_eq!(
            classify_result(true, true, Some(true)),
            ExperimentResult::Pass
        );
        assert_eq!(
            classify_result(false, true, Some(true)),
            ExperimentResult::Fail
        );
    }
}

mod pipewire_probe {
    use pipewire as pw;
    use pw::properties::properties;
    use pw::spa;
    use pw::spa::pod::Pod;

    /// Connects to `node_id` and returns once 2 frames arrive or 5s elapse
    /// (Experiment 3's proven pattern, minimised for the reliability loop).
    /// KEY FINDING: the virtual monitor's actual resolution is driven by
    /// the negotiated PipeWire video format, not by `RecordVirtual`'s
    /// properties dict — `preferred_width`/`preferred_height` are proposed
    /// here as the Choice::Range default (see Phase 4 plan Evidence).
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
            "blackroom-exp04-probe",
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

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    let redact_on = args.common.redact_enabled();
    let now = OffsetDateTime::now_utc();

    let conn = Connection::session()?;

    let resolutions = [(1280, 720, 60.0), (1920, 1080, 60.0), (2560, 1440, 60.0)];
    let mut results = Vec::new();
    for (width, height, refresh_rate) in resolutions {
        results.push(one_cycle(&conn, width, height, refresh_rate)?);
    }

    let all_confirmed = results.iter().all(|r| r.monitor_confirmed);
    let all_torn_down = results.iter().all(|r| r.teardown_confirmed);

    let (cycle_outcomes, node_count_before, node_count_after, shell_pid_before, shell_pid_after) =
        if args.skip_cycles {
            (Vec::new(), None, None, None, None)
        } else {
            let shell_before = gnome_shell_pid();
            let count_before = screencast_video_node_count();
            let (width, height, refresh_rate) = (1920, 1080, 60.0);
            let outcomes = run_cycles(&conn, width, height, refresh_rate);
            let count_after = screencast_video_node_count();
            let shell_after = gnome_shell_pid();
            (
                outcomes,
                count_before,
                count_after,
                shell_before,
                shell_after,
            )
        };

    let cycles_clean = (!args.skip_cycles).then(|| {
        cycle_outcomes.len() == CYCLE_COUNT as usize
            && cycle_outcomes
                .iter()
                .all(|cycle| !cycle.timed_out && cycle.error.is_none())
    });
    let no_leaked_nodes = node_count_before
        .zip(node_count_after)
        .map(|(before, after)| after <= before);
    let shell_survived = shell_pid_before
        .as_ref()
        .zip(shell_pid_after.as_ref())
        .map(|(before, after)| before == after);

    let reliability_proven = cycles_clean
        .zip(no_leaked_nodes)
        .zip(shell_survived)
        .map(|((cycles, nodes), shell)| cycles && nodes && shell);
    let result = classify_result(all_confirmed, all_torn_down, reliability_proven);

    let observed = redact(
        &format!(
            "resolution_results={results:#?}\n\
             all_confirmed={all_confirmed}\nall_torn_down={all_torn_down}\n\
             cycles_run={}\ncycles_clean={cycles_clean:?}\n\
             screencast_video_nodes_before={node_count_before:?}\n\
             screencast_video_nodes_after={node_count_after:?}\nno_leaked_nodes={no_leaked_nodes:?}\n\
             shell_pid_before={shell_pid_before:?}\nshell_pid_after={shell_pid_after:?}\n\
             shell_survived={shell_survived:?}",
            cycle_outcomes.len(),
        ),
        redact_on,
    );

    let report = ExperimentReport {
        experiment: "Experiment 4 — Virtual Monitor Creation".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1"
            .to_string(),
        objective: "Create a real virtual monitor via RecordVirtual at 3 resolutions, confirm \
                    via DisplayConfig.GetCurrentState, destroy cleanly, and verify 50-cycle \
                    reliability (Document 10 §11, Doc 19 §16-17)."
            .to_string(),
        hypothesis: "ScreenCast.Session.RecordVirtual with a width/height/framerate properties \
                     dict creates a real logical monitor visible in GetCurrentState; Stop() \
                     removes it; repeated cycles leak no PipeWire nodes and do not crash GNOME \
                     Shell."
            .to_string(),
        procedure: "For each resolution: CreateSession, RecordVirtual, Start, wait for \
                    PipeWireStreamAdded, receive frames, snapshot GetCurrentState, Stop, \
                    snapshot again. Then repeat the 1920x1080 cycle 50 times with a 10s/cycle \
                    bound, diffing `pw-dump` and comparing the GNOME Shell PID before/after."
            .to_string(),
        expected: "All 3 resolutions produce a confirmed, then cleanly torn-down, virtual \
                   monitor; 50/50 cycles complete within the bound with no pw-dump diff and no \
                   GNOME Shell PID change."
            .to_string(),
        observed,
        evidence: vec![format!(
            "docs/experiments/evidence/{EXP_ID}/<date>/report.md"
        )],
        result,
        failure: None,
        root_cause: None,
        security_impact: Some(
            "gnome-remote-desktop.service must stay masked for this run (docs/ops/\
             experiment-safety.md §5)."
                .to_string(),
        ),
        recommended_action: None,
        follow_up: Some(
            "Port the proven mechanics into crates/blackroom-gnome/src/mutter/\
             {virtual_monitor.rs,pipewire_capture.rs} (Phase 4 plan step 6)."
                .to_string(),
        ),
    };

    let dir = evidence_dir(EXP_ID, now)?;
    write_evidence(
        &dir,
        &redact(&report.render(now), redact_on),
        "virtual_monitor.json",
        &serde_json::json!({
            "resolution_results": results,
            "cycle_outcomes": cycle_outcomes,
        }),
    )?;
    println!("Wrote evidence to {}", dir.display());
    println!("Result: {result}");

    Ok(())
}
