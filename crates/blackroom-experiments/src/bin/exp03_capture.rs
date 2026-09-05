//! Experiment 3 — Basic Screen Capture (Document 10 §10 / Document 02 §9).
//!
//! **First experiment that mutates real Mutter/PipeWire state** (Phase 4;
//! Phase 0-3 were read-only). Creates a `ScreenCast` session (paired with a
//! `RemoteDesktop` session only if the live `ScreenCast.CreateSession` call
//! actually requires it — Doc 02 §9 step 2 says "if required", not assumed),
//! captures the **existing** desktop (no virtual monitor — that is
//! Experiment 4), receives real PipeWire frames, and verifies clean
//! teardown. Session sub-object method names are not assumed: this
//! experiment introspects each live session/stream object before calling
//! any of its methods (`feasibility-research.md` topic 2;
//! `blackroom_experiments::introspect`).
//!
//! Requires `gnome-remote-desktop.service` masked first
//! (`docs/ops/experiment-safety.md` §5).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use blackroom_experiments::{
    BusKind, CommonArgs, ExperimentReport, ExperimentResult, Target, evidence_dir,
    introspect_target, redact, write_evidence,
};
use clap::Parser;
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::pod::Pod;
use serde::Serialize;
use time::OffsetDateTime;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Type, Value};

const EXP_ID: &str = "exp03";
const INTROSPECTION_DIR: &str = "docs/gnome/introspection";
/// How long to wait for the `PipeWireStreamAdded` signal after `Start()`.
const SIGNAL_WAIT: Duration = Duration::from_secs(10);
/// Hard bound on the PipeWire capture window (Doc 19 §16: never assume an
/// operation completes; always bound it).
const CAPTURE_WINDOW: Duration = Duration::from_secs(8);
/// Stop early once this many frames are observed.
const FRAMES_WANTED: u32 = 5;

#[derive(Parser, Debug)]
#[command(about = "Experiment 3: capture the existing desktop via Mutter ScreenCast + PipeWire")]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
}

// ---------------------------------------------------------------------
// DisplayConfig.GetCurrentState (read-only; reused only to pick a real
// connector name to capture — same typed signature as capability.rs/exp02).
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

/// The primary connector's name (falls back to the first monitor listed).
fn primary_connector(conn: &Connection) -> anyhow::Result<String> {
    let proxy = Proxy::new(
        conn,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )?;
    let (_serial, monitors, logical_monitors, _props): GetCurrentStateResult =
        proxy.call("GetCurrentState", &())?;
    if let Some(primary) = logical_monitors.iter().find(|l| l.primary)
        && let Some(connector) = primary.monitors.first()
    {
        return Ok(connector.connector.clone());
    }
    monitors
        .into_iter()
        .next()
        .map(|m| m.connector_info.connector)
        .ok_or_else(|| anyhow::anyhow!("GetCurrentState reported zero monitors"))
}

// ---------------------------------------------------------------------
// ScreenCast session creation + capture-method discovery.
// ---------------------------------------------------------------------

/// Session sub-object methods this experiment tries, in order, to capture
/// the existing desktop (Doc 10 §10: "start with the simplest supported
/// capture mechanism"). Real availability is discovered live via
/// introspection, not assumed.
const CAPTURE_METHOD_CANDIDATES: &[&str] = &["RecordMonitor", "RecordWindow", "RecordArea"];

#[derive(Debug, Serialize)]
struct CaptureAttempt {
    method_tried: String,
    connector_argument: Option<String>,
    succeeded: bool,
    error: Option<String>,
}

/// Calls the first present candidate method able to capture the existing
/// desktop. Returns the stream object path and which method worked.
fn record_existing_desktop(
    session_proxy: &Proxy<'_>,
    session_interfaces_have: impl Fn(&str) -> bool,
    connector: &str,
) -> (Option<OwnedObjectPath>, Vec<CaptureAttempt>) {
    let mut attempts = Vec::new();
    for method in CAPTURE_METHOD_CANDIDATES {
        if !session_interfaces_have(method) {
            attempts.push(CaptureAttempt {
                method_tried: (*method).to_string(),
                connector_argument: None,
                succeeded: false,
                error: Some("not present on the live session object".to_string()),
            });
            continue;
        }
        let empty_props: HashMap<&str, Value<'_>> = HashMap::new();
        let result: zbus::Result<OwnedObjectPath> = match *method {
            "RecordMonitor" => session_proxy.call("RecordMonitor", &(connector, empty_props)),
            "RecordWindow" => session_proxy.call("RecordWindow", &(empty_props,)),
            "RecordArea" => {
                session_proxy.call("RecordArea", &(0_i32, 0_i32, 1_i32, 1_i32, empty_props))
            }
            _ => unreachable!(),
        };
        match result {
            Ok(stream_path) => {
                attempts.push(CaptureAttempt {
                    method_tried: (*method).to_string(),
                    connector_argument: Some(connector.to_string()),
                    succeeded: true,
                    error: None,
                });
                return (Some(stream_path), attempts);
            }
            Err(error) => attempts.push(CaptureAttempt {
                method_tried: (*method).to_string(),
                connector_argument: Some(connector.to_string()),
                succeeded: false,
                error: Some(error.to_string()),
            }),
        }
    }
    (None, attempts)
}

// ---------------------------------------------------------------------
// PipeWire frame receipt.
// ---------------------------------------------------------------------

#[derive(Debug, Serialize, Clone, Default)]
struct CaptureFormat {
    format: String,
    width: u32,
    height: u32,
    framerate_num: u32,
    framerate_denom: u32,
}

struct StreamUserData {
    mainloop: pw::main_loop::MainLoopRc,
    frame_count: Rc<std::cell::Cell<u32>>,
    format: spa::param::video::VideoInfoRaw,
    observed_format: Rc<RefCell<Option<CaptureFormat>>>,
}

/// Connects to the local PipeWire instance (the agent runs unsandboxed in
/// the user session, so it reaches `$XDG_RUNTIME_DIR/pipewire-0` directly —
/// no portal `OpenPipeWireRemote` fd hand-off is needed, unlike a sandboxed
/// Flatpak client) and receives frames from `node_id` for up to
/// `CAPTURE_WINDOW`, stopping early once `FRAMES_WANTED` arrive.
fn receive_frames(node_id: u32) -> anyhow::Result<(u32, Option<CaptureFormat>)> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    let frame_count = Rc::new(std::cell::Cell::new(0_u32));
    let observed_format = Rc::new(RefCell::new(None));
    let data = StreamUserData {
        mainloop: mainloop.clone(),
        frame_count: frame_count.clone(),
        format: Default::default(),
        observed_format: observed_format.clone(),
    };

    let stream = pw::stream::StreamBox::new(
        &core,
        "blackroom-exp03-capture",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )?;

    let _listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else { return };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param)
            else {
                return;
            };
            if media_type != spa::param::format::MediaType::Video
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                return;
            }
            if user_data.format.parse(param).is_ok() {
                *user_data.observed_format.borrow_mut() = Some(CaptureFormat {
                    format: format!("{:?}", user_data.format.format()),
                    width: user_data.format.size().width,
                    height: user_data.format.size().height,
                    framerate_num: user_data.format.framerate().num,
                    framerate_denom: user_data.format.framerate().denom,
                });
            }
        })
        .process(|stream, user_data| {
            if let Some(_buffer) = stream.dequeue_buffer() {
                let count = user_data.frame_count.get() + 1;
                user_data.frame_count.set(count);
                if count >= FRAMES_WANTED {
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
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::BGRA,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
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
    let mut params = [Pod::from_bytes(&values).ok_or_else(|| anyhow::anyhow!("bad format pod"))?];

    stream.connect(
        spa::utils::Direction::Input,
        Some(node_id),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;

    let quit_on_timeout = mainloop.clone();
    let timer = mainloop.loop_().add_timer(move |_| quit_on_timeout.quit());
    timer
        .update_timer(Some(CAPTURE_WINDOW), None)
        .into_result()?;

    mainloop.run();
    stream.disconnect()?;

    Ok((frame_count.get(), observed_format.borrow().clone()))
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    let redact_on = args.common.redact_enabled();
    let now = OffsetDateTime::now_utc();

    let session_conn = Connection::session()?;
    let connector = primary_connector(&session_conn)?;

    // Supplementary evidence for Doc 02 §9 step 2 ("associate it with the
    // RemoteDesktop session if required") and Phase 4 step 4's
    // `remote_desktop.rs`: RemoteDesktop.Session's real sub-object interface
    // has never been introspected (its object does not exist until created;
    // Phase 0-1 never called CreateSession). Create one, introspect it, and
    // stop it — not used to pair with the ScreenCast capture below, since
    // that turns out not to be required for basic monitor capture.
    let mut remote_desktop_notes = Vec::new();
    {
        let rd_top_proxy = Proxy::new(
            &session_conn,
            "org.gnome.Mutter.RemoteDesktop",
            "/org/gnome/Mutter/RemoteDesktop",
            "org.gnome.Mutter.RemoteDesktop",
        )?;
        let rd_session_path: OwnedObjectPath = rd_top_proxy.call("CreateSession", &())?;
        let rd_target = Target {
            label: "RemoteDesktop.Session (exp03)",
            bus: BusKind::Session,
            destination: "org.gnome.Mutter.RemoteDesktop",
            path: rd_session_path.to_string(),
        };
        let inspected_rd =
            introspect_target(&session_conn, &session_conn, &rd_target, INTROSPECTION_DIR);
        remote_desktop_notes.push(format!(
            "RemoteDesktop.Session methods: {}",
            inspected_rd
                .interfaces
                .iter()
                .flat_map(|i| i.methods.iter().map(|m| m.name.clone()))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let rd_session_proxy = Proxy::new(
            &session_conn,
            "org.gnome.Mutter.RemoteDesktop",
            rd_session_path.as_ref(),
            "org.gnome.Mutter.RemoteDesktop.Session",
        )?;
        let rd_stop_error = rd_session_proxy.call::<_, _, ()>("Stop", &()).err();
        remote_desktop_notes.push(format!(
            "RemoteDesktop.Session Stop() (without Start()): {}",
            rd_stop_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "ok".to_string())
        ));
    }

    let screencast_session_proxy = Proxy::new(
        &session_conn,
        "org.gnome.Mutter.ScreenCast",
        "/org/gnome/Mutter/ScreenCast",
        "org.gnome.Mutter.ScreenCast",
    )?;
    let empty_props: HashMap<&str, Value<'_>> = HashMap::new();
    let session_path: OwnedObjectPath =
        screencast_session_proxy.call("CreateSession", &(empty_props,))?;

    let session_target = Target {
        label: "ScreenCast.Session (exp03)",
        bus: BusKind::Session,
        destination: "org.gnome.Mutter.ScreenCast",
        path: session_path.to_string(),
    };
    let inspected_session = introspect_target(
        &session_conn,
        &session_conn,
        &session_target,
        INTROSPECTION_DIR,
    );
    let has_method = |name: &str| {
        inspected_session.interfaces.iter().any(|iface| {
            iface.name == "org.gnome.Mutter.ScreenCast.Session"
                && iface.methods.iter().any(|m| m.name == name)
        })
    };

    let session_proxy = Proxy::new(
        &session_conn,
        "org.gnome.Mutter.ScreenCast",
        session_path.as_ref(),
        "org.gnome.Mutter.ScreenCast.Session",
    )?;

    let (stream_path, capture_attempts) =
        record_existing_desktop(&session_proxy, has_method, &connector);

    let mut evidence_notes = remote_desktop_notes;
    evidence_notes.push(format!(
        "Introspected ScreenCast.Session methods: {}",
        inspected_session
            .interfaces
            .iter()
            .flat_map(|i| i.methods.iter().map(|m| m.name.clone()))
            .collect::<Vec<_>>()
            .join(", ")
    ));

    let (frame_count, observed_format, cleanup_verified, stop_error) =
        if let Some(stream_path) = &stream_path {
            let stream_target = Target {
                label: "ScreenCast.Stream (exp03)",
                bus: BusKind::Session,
                destination: "org.gnome.Mutter.ScreenCast",
                path: stream_path.to_string(),
            };
            let inspected_stream = introspect_target(
                &session_conn,
                &session_conn,
                &stream_target,
                INTROSPECTION_DIR,
            );
            evidence_notes.push(format!(
                "ScreenCast.Stream signals: {}",
                inspected_stream
                    .interfaces
                    .iter()
                    .flat_map(|i| i.signals.iter().map(|s| s.name.clone()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));

            let stream_signal_proxy = Proxy::new(
                &session_conn,
                "org.gnome.Mutter.ScreenCast",
                stream_path.as_ref(),
                "org.gnome.Mutter.ScreenCast.Stream",
            )?;
            let mut signal_iter = stream_signal_proxy.receive_signal("PipeWireStreamAdded")?;

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

            let (frame_count, observed_format) = match node_id {
                Some((node_id,)) => {
                    evidence_notes.push(format!("PipeWireStreamAdded node_id={node_id}"));
                    match receive_frames(node_id) {
                        Ok((count, format)) => (count, format),
                        Err(error) => {
                            evidence_notes.push(format!("PipeWire capture error: {error}"));
                            (0, None)
                        }
                    }
                }
                None => {
                    evidence_notes
                        .push("PipeWireStreamAdded signal not received within timeout".to_string());
                    (0, None)
                }
            };

            let stop_error = session_proxy
                .call::<_, _, ()>("Stop", &())
                .err()
                .map(|e| e.to_string());

            // KEY FINDING: a bare `Introspect()` succeeds generically for
            // ANY object path under this service (GDBus subtree dispatch
            // returns empty introspection data rather than an error for a
            // nonexistent child) — confirmed live via `busctl introspect`
            // on both a real and a made-up path, both "succeeding" with
            // empty output. A real method call (e.g. a second `Stop()`)
            // reliably fails with `UnknownObject`/"Object does not exist"
            // for a truly gone object (confirmed live via `busctl call`),
            // so cleanup verification calls `Stop()` again instead of
            // introspecting.
            let same_conn_gone = session_proxy.call::<_, _, ()>("Stop", &()).is_err();
            drop(session_proxy);
            drop(session_conn);
            let fresh_conn = Connection::session()?;
            let fresh_session_proxy = Proxy::new(
                &fresh_conn,
                "org.gnome.Mutter.ScreenCast",
                session_path.as_ref(),
                "org.gnome.Mutter.ScreenCast.Session",
            )?;
            let fresh_conn_gone = fresh_session_proxy.call::<_, _, ()>("Stop", &()).is_err();
            let cleanup_verified = same_conn_gone || fresh_conn_gone;
            evidence_notes.push(format!(
                "Post-Stop() second-Stop()-call reachability probe: same-connection \
                 gone={same_conn_gone}, fresh-connection gone={fresh_conn_gone} — if both \
                 are false, the session object outlives Stop() and this process's own \
                 connection (a Phase 9 crash-recovery question, not a Phase 4 defect)."
            ));

            (frame_count, observed_format, cleanup_verified, stop_error)
        } else {
            (0, None, false, None)
        };

    let result = if stream_path.is_some() && frame_count > 0 && cleanup_verified {
        ExperimentResult::Pass
    } else if stream_path.is_some() && (frame_count > 0 || cleanup_verified) {
        ExperimentResult::Partial
    } else {
        ExperimentResult::Fail
    };

    let observed = redact(
        &format!(
            "connector={connector}\ncapture attempts: {capture_attempts:?}\n\
             stream_path={stream_path:?}\nframes_received={frame_count}\n\
             observed_format={observed_format:?}\ncleanup_verified={cleanup_verified}\n\
             stop_error={stop_error:?}\nnotes:\n{}",
            evidence_notes.join("\n")
        ),
        redact_on,
    );

    let report = ExperimentReport {
        experiment: "Experiment 3 — Basic Screen Capture".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1"
            .to_string(),
        objective: "Prove that the existing GNOME session can be captured over ScreenCast + \
                    PipeWire, before touching display topology (Document 10 §10)."
            .to_string(),
        hypothesis: "org.gnome.Mutter.ScreenCast.CreateSession plus one of RecordMonitor/\
                     RecordWindow/RecordArea on the returned session object starts a capture \
                     of the existing desktop that PipeWire delivers as real video frames."
            .to_string(),
        procedure: "CreateSession(); introspect the session object; call the first available \
                    capture method for the primary connector; introspect the stream object; \
                    subscribe to PipeWireStreamAdded; Start(); connect a PipeWire stream to the \
                    reported node id and receive frames; Stop(); verify the session object is \
                    gone."
            .to_string(),
        expected: format!(
            "A stream object is created, PipeWireStreamAdded reports a node id within \
             {SIGNAL_WAIT:?}, at least one real frame arrives within {CAPTURE_WINDOW:?}, and \
             Stop() leaves the session object unreachable."
        ),
        observed,
        evidence: vec![
            format!("docs/experiments/evidence/{EXP_ID}/<date>/report.md"),
            format!("{INTROSPECTION_DIR}/screencast-session-exp03.xml"),
        ],
        result,
        failure: stop_error.clone(),
        root_cause: None,
        security_impact: Some(
            "gnome-remote-desktop.service must stay masked for this run (docs/ops/\
             experiment-safety.md §5) to avoid two session owners on the same Mutter \
             interfaces."
                .to_string(),
        ),
        recommended_action: None,
        follow_up: Some(
            "Port the proven mechanics into crates/blackroom-gnome/src/mutter/\
             {remote_desktop.rs,screencast.rs} (Phase 4 plan step 4)."
                .to_string(),
        ),
    };

    let dir = evidence_dir(EXP_ID, now)?;
    write_evidence(
        &dir,
        &redact(&report.render(now), redact_on),
        "capture.json",
        &serde_json::json!({
            "connector": connector,
            "capture_attempts": capture_attempts,
            "stream_path": stream_path.map(|p| p.to_string()),
            "frames_received": frame_count,
            "observed_format": observed_format,
            "cleanup_verified": cleanup_verified,
        }),
    )?;
    println!("Wrote evidence to {}", dir.display());
    println!("Result: {result}");

    Ok(())
}
