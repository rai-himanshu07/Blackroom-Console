//! Experiment 8 — Remote Input (Document 10 Experiment 8; Phase 6 Step 3; Gate FEAS-D).
//!
//! MUTATING and supervised: injects a short, harmless input sequence into the
//! *selected existing* GNOME session through `RemoteDesktop.Session.ConnectToEIS`,
//! then checks input stops after an authorization revoke and after session
//! stop. It never touches displays, grabs physical input, or opens a ScreenCast.
//!
//! Only keys with no default effect are sent (F13, Shift) and pointer motion
//! nets to zero. The run serves a local observer page (loopback, one-time
//! token); the page reports focus/fullscreen every 250 ms and tallies the
//! events that actually reach it. Input is sent only while the page is focused
//! and fullscreen, and the recorded result comes from the page's own tally.
//!
//! Abort by defocusing the page (injection stops at the next stage). A signal
//! kill skips `Drop`; owner-death teardown of a RemoteDesktop session is unobserved.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{JoinHandle, sleep};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::lease::{Capability, ControlLease, InputAuthorization};
use blackroom_core::state::State;
use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, current_uid, discover, evidence_dir,
    write_evidence,
};
use blackroom_gnome::mutter::eis::EiConnection;
use blackroom_gnome::mutter::remote_desktop::RemoteDesktopSession;
use clap::Parser;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use reis::event::{Device, DeviceCapability, DeviceResumed, EiEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

const EXP_ID: &str = "exp08";
const KEY_F13: u32 = 183;
const KEY_LEFTSHIFT: u32 = 42;
const BTN_LEFT: u32 = 272;
const OBSERVER_PAGE: &str = include_str!("../../assets/exp08_observer.html");
/// A heartbeat older than this no longer proves the page is focused.
const FRESH: Duration = Duration::from_millis(1200);
const MAX_HEADER: usize = 16 * 1024;
const MAX_BODY: usize = 256 * 1024;

#[derive(Parser, Debug)]
#[command(about = "Experiment 8: bounded remote input into the live GNOME session (MUTATING)")]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Required: the operator is present, work is saved, and a second-device
    /// SSH session is open (docs/ops/experiment-safety.md §7).
    #[arg(long, default_value_t = false)]
    operator_present: bool,
    /// Rehearsal: serve the observer page and report focus/fullscreen and the
    /// live tally for 15 s. No D-Bus, no session, no input, no evidence file.
    #[arg(long, default_value_t = false)]
    observer_only: bool,
    /// Seconds the observer page must stay focused and fullscreen before input.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(3..=30))]
    settle_secs: u64,
    /// Seconds to wait for the operator to focus the observer page.
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(30..=600))]
    ready_timeout_secs: u64,
    /// Pause between injected stages.
    #[arg(long, default_value_t = 400, value_parser = clap::value_parser!(u64).range(100..=2000))]
    pace_ms: u64,
}

#[derive(Debug, Clone, Copy)]
enum Expect {
    Accept,
    Refuse(&'static [ErrorCode]),
    /// Outcome is recorded; delivery is judged by the observer tally.
    Observe,
}

#[derive(Debug, Serialize)]
struct Stage {
    name: &'static str,
    expected: &'static str,
    outcome: String,
    at_ms: u64,
    violation: bool,
}

#[derive(Debug, Serialize)]
struct DeviceSeen {
    event: &'static str,
    name: Option<String>,
    capabilities: Vec<&'static str>,
}

#[derive(Debug, Default, Serialize)]
struct Run {
    selected_session_id: Option<String>,
    git_head: Option<String>,
    git_dirty: Option<bool>,
    shell_pid_before: Option<String>,
    shell_pid_after: Option<String>,
    gnome_remote_desktop_before: Option<String>,
    gnome_remote_desktop_after: Option<String>,
    remote_desktop_session_path: Option<String>,
    blocked: Option<String>,
    failure: Option<String>,
    aborted: Option<String>,
    devices_seen: Vec<DeviceSeen>,
    stages: Vec<Stage>,
    post_teardown_events: Vec<String>,
    eis_ready_after_stop: Option<bool>,
    stuck_input_suspect: bool,
    stale_session_call: Option<String>,
    observer_beats: u64,
    observer_tally: Option<Value>,
    tally_matches: Option<bool>,
    tally_notes: Vec<String>,
    violations: Vec<String>,
}

impl Run {
    fn record(
        &mut self,
        name: &'static str,
        expect: Expect,
        result: Result<(), BlackroomError>,
        t0: Instant,
    ) {
        let (outcome, violation) = match (&result, expect) {
            (Ok(()), Expect::Accept | Expect::Observe) => ("accepted".to_string(), false),
            (Ok(()), Expect::Refuse(_)) => ("accepted".to_string(), true),
            (Err(error), Expect::Accept) => (format!("refused:{:?}", error.code), true),
            (Err(error), Expect::Observe) => (format!("refused:{:?}", error.code), false),
            (Err(error), Expect::Refuse(codes)) => (
                format!("refused:{:?}", error.code),
                !codes.contains(&error.code),
            ),
        };
        if matches!(expect, Expect::Accept) && violation {
            self.stuck_input_suspect = true;
        }
        if violation {
            self.violations.push(format!("{name}: {outcome}"));
        }
        self.stages.push(Stage {
            name,
            expected: match expect {
                Expect::Accept => "accepted",
                Expect::Refuse(_) => "refused",
                Expect::Observe => "observe",
            },
            outcome,
            at_ms: u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX),
            violation,
        });
    }
}

fn classify(run: &Run) -> ExperimentResult {
    if run.blocked.is_some() {
        ExperimentResult::Blocked
    } else if run.failure.is_some() || !run.violations.is_empty() {
        ExperimentResult::Fail
    } else if run.aborted.is_some() || run.tally_matches != Some(true) {
        ExperimentResult::Partial
    } else {
        ExperimentResult::Pass
    }
}

fn require_operator(args: &Args) -> anyhow::Result<()> {
    anyhow::ensure!(
        args.operator_present,
        "refusing to inject input: pass --operator-present only after the safety preflight in \
         docs/ops/experiment-safety.md §7 (work saved, second-device SSH open)"
    );
    Ok(())
}

/// Judges the page's own tally against the exact sequence sent: F13 twice
/// (tap, then inside Shift), one left click, net-zero pointer motion, a
/// positive scroll, nothing from the revoked or post-stop attempts.
fn evaluate_tally(tally: &Value) -> (bool, Vec<String>) {
    let mut notes = Vec::new();
    let keys = tally["keys"].as_array().cloned().unwrap_or_default();
    let key_count = |kind: &str, code: &str| {
        keys.iter()
            .filter(|key| key["type"] == kind && key["code"] == code)
            .count()
    };
    let mut check = |ok: bool, note: String| {
        if !ok {
            notes.push(note);
        }
    };
    check(
        key_count("down", "F13") == 2 && key_count("up", "F13") == 2,
        format!(
            "F13 down/up {}/{} (expected 2/2)",
            key_count("down", "F13"),
            key_count("up", "F13")
        ),
    );
    check(
        key_count("down", "ShiftLeft") == 1 && key_count("up", "ShiftLeft") == 1,
        "ShiftLeft not 1/1".to_string(),
    );
    let other = keys
        .iter()
        .filter(|key| key["code"] != "F13" && key["code"] != "ShiftLeft")
        .count();
    check(other == 0, format!("{other} unexpected key events"));
    let buttons = tally["buttons"].as_array().cloned().unwrap_or_default();
    let clicks = buttons
        .iter()
        .filter(|button| button["type"] == "click" && button["button"] == 0)
        .count();
    check(
        clicks == 1 && buttons.iter().all(|button| button["button"] == 0),
        format!("{clicks} left clicks or a non-left button seen"),
    );
    let pointer = &tally["pointer"];
    let number = |value: &Value| value.as_f64().unwrap_or(f64::NAN);
    let (net_x, net_y, abs_x) = (
        number(&pointer["netX"]),
        number(&pointer["netY"]),
        number(&pointer["absX"]),
    );
    check(
        pointer["moves"].as_u64().unwrap_or(0) > 0
            && net_x.abs() <= 2.0
            && net_y.abs() <= 2.0
            && (20.0..=200.0).contains(&abs_x),
        format!("pointer motion not net-zero or missing (net {net_x},{net_y} abs {abs_x})"),
    );
    let wheel = &tally["wheel"];
    let delta = number(&wheel["deltaY"]);
    check(
        wheel["events"].as_u64().unwrap_or(0) > 0
            && delta > 0.0
            && delta <= 500.0
            && number(&wheel["scrollY"]) > 0.0,
        format!("scroll missing or out of range (deltaY {delta})"),
    );
    check(
        tally["untrusted"].as_u64() == Some(0),
        "page saw untrusted (synthetic) events".to_string(),
    );
    (notes.is_empty(), notes)
}

#[derive(Default)]
struct ObserverState {
    beat_at: Option<Instant>,
    focus: bool,
    fullscreen: bool,
    tally: Option<Value>,
    beats: u64,
}

#[derive(Deserialize)]
struct Beat {
    focus: bool,
    fullscreen: bool,
    tally: Value,
}

struct Request {
    method: String,
    path: String,
    host: Option<String>,
    body: Vec<u8>,
}

/// Loopback-only observer server: serves the page and receives its heartbeats.
struct Observer {
    state: Arc<Mutex<ObserverState>>,
    stop: Arc<AtomicBool>,
    port: u16,
    token: String,
    thread: Option<JoinHandle<()>>,
}

impl Observer {
    fn start() -> anyhow::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).context("observer listener")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let mut raw = [0_u8; 16];
        getrandom::fill(&mut raw).context("observer token")?;
        let token: String = raw.iter().map(|byte| format!("{byte:02x}")).collect();
        let state = Arc::new(Mutex::new(ObserverState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (state, stop, token) = (Arc::clone(&state), Arc::clone(&stop), token.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => handle(stream, &state, &token, port),
                        Err(_) => sleep(Duration::from_millis(20)),
                    }
                }
            })
        };
        Ok(Self {
            state,
            stop,
            port,
            token,
            thread: Some(thread),
        })
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/{}/", self.port, self.token)
    }

    fn snapshot<T>(&self, read: impl FnOnce(&ObserverState) -> T) -> T {
        read(&self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn ready_now(&self, max_age: Duration) -> bool {
        self.snapshot(|state| {
            state.focus
                && state.fullscreen
                && state.beat_at.is_some_and(|at| at.elapsed() <= max_age)
        })
    }

    /// True once the page has been focused and fullscreen continuously for `settle`.
    fn wait_ready(&self, timeout: Duration, settle: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut since: Option<Instant> = None;
        let mut last_hint = Instant::now();
        while Instant::now() < deadline {
            if self.ready_now(FRESH) {
                let started = *since.get_or_insert_with(Instant::now);
                if started.elapsed() >= settle {
                    return true;
                }
            } else {
                since = None;
                if last_hint.elapsed() >= Duration::from_secs(15) {
                    println!("  still waiting for the observer page: focused and fullscreen (F11)");
                    last_hint = Instant::now();
                }
            }
            sleep(Duration::from_millis(100));
        }
        false
    }

    fn wait_beats(&self, extra: u64, timeout: Duration) {
        let target = self.snapshot(|state| state.beats) + extra;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline && self.snapshot(|state| state.beats) < target {
            sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > MAX_HEADER {
            return None;
        }
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = std::str::from_utf8(&buffer[..header_end]).ok()?;
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_string();
    let path = request_line.next()?.to_string();
    let (mut host, mut length) = (None, 0_usize);
    for line in lines {
        let (name, value) = line.split_once(':')?;
        match name.trim().to_ascii_lowercase().as_str() {
            "host" => host = Some(value.trim().to_string()),
            "content-length" => length = value.trim().parse().ok()?,
            _ => {}
        }
    }
    if length > MAX_BODY {
        return None;
    }
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    Some(Request {
        method,
        path,
        host,
        body,
    })
}

fn response(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

fn route(request: &Request, state: &Mutex<ObserverState>, token: &str, port: u16) -> Vec<u8> {
    // The Host check blocks DNS rebinding; the token blocks other local pages.
    if request.host.as_deref() != Some(&format!("127.0.0.1:{port}")) {
        return response("403 Forbidden", "text/plain", b"");
    }
    let base = format!("/{token}/");
    match request.method.as_str() {
        "GET" if request.path == base => response(
            "200 OK",
            "text/html; charset=utf-8",
            OBSERVER_PAGE.as_bytes(),
        ),
        "POST" if request.path == format!("{base}beat") => {
            match serde_json::from_slice::<Beat>(&request.body) {
                Ok(beat) => {
                    let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
                    state.beat_at = Some(Instant::now());
                    state.focus = beat.focus;
                    state.fullscreen = beat.fullscreen;
                    state.tally = Some(beat.tally);
                    state.beats += 1;
                    response("204 No Content", "text/plain", b"")
                }
                Err(_) => response("400 Bad Request", "text/plain", b""),
            }
        }
        _ => response("404 Not Found", "text/plain", b""),
    }
}

fn handle(mut stream: TcpStream, state: &Mutex<ObserverState>, token: &str, port: u16) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    if let Some(request) = read_request(&mut stream) {
        let _ = stream.write_all(&route(&request, state, token, port));
    }
}

struct Authority {
    lease: ControlLease,
    signature: Signature,
    verifying_key: VerifyingKey,
}

impl Authority {
    fn new(ttl: Duration) -> Self {
        use getrandom::rand_core::UnwrapErr;
        let now = SystemTime::now();
        let signing_key = SigningKey::generate(&mut UnwrapErr(getrandom::SysRng));
        let lease = ControlLease {
            session_id: "rs_EXP08".to_string(),
            host_id: "bc_EXP08".to_string(),
            user_id: "exp08".to_string(),
            client_id: "cl_EXP08".to_string(),
            security_epoch: SecurityEpoch::INITIAL,
            issued_at: now,
            expires_at: now + ttl,
            capabilities: vec![Capability::View, Capability::Control],
        };
        Self {
            signature: lease.sign(&signing_key),
            verifying_key: signing_key.verifying_key(),
            lease,
        }
    }

    fn authorization(&self, revoked: bool) -> InputAuthorization<'_> {
        InputAuthorization {
            lease: &self.lease,
            signature: &self.signature,
            verifying_key: &self.verifying_key,
            authenticated: true,
            authorized: true,
            current_epoch: SecurityEpoch::INITIAL,
            current_state: State::RemoteActive,
            current_session_id: &self.lease.session_id,
            revoked,
            now: SystemTime::now(),
        }
    }
}

const ALL_CAPABILITIES: [(DeviceCapability, &str); 7] = [
    (DeviceCapability::Pointer, "pointer"),
    (DeviceCapability::PointerAbsolute, "pointer_absolute"),
    (DeviceCapability::Keyboard, "keyboard"),
    (DeviceCapability::Touch, "touch"),
    (DeviceCapability::Scroll, "scroll"),
    (DeviceCapability::Button, "button"),
    (DeviceCapability::Text, "text"),
];

fn describe(event: &'static str, device: &Device) -> DeviceSeen {
    DeviceSeen {
        event,
        name: device.name().map(str::to_owned),
        capabilities: ALL_CAPABILITIES
            .iter()
            .filter(|(capability, _)| device.has_capability(*capability))
            .map(|(_, label)| *label)
            .collect(),
    }
}

/// Latest resumed device per input kind the run needs.
#[derive(Default)]
struct Devices {
    keyboard: Option<DeviceResumed>,
    pointer: Option<DeviceResumed>,
    button: Option<DeviceResumed>,
    scroll: Option<DeviceResumed>,
}

impl Devices {
    fn slots(&mut self) -> [(&mut Option<DeviceResumed>, DeviceCapability); 4] {
        [
            (&mut self.keyboard, DeviceCapability::Keyboard),
            (&mut self.pointer, DeviceCapability::Pointer),
            (&mut self.button, DeviceCapability::Button),
            (&mut self.scroll, DeviceCapability::Scroll),
        ]
    }

    fn missing(&self) -> Vec<&'static str> {
        [
            (self.keyboard.is_none(), "keyboard"),
            (self.pointer.is_none(), "pointer"),
            (self.button.is_none(), "button"),
            (self.scroll.is_none(), "scroll"),
        ]
        .into_iter()
        .filter_map(|(absent, label)| absent.then_some(label))
        .collect()
    }

    fn clear(&mut self, device: &Device) {
        for (slot, _) in self.slots() {
            if slot
                .as_ref()
                .is_some_and(|resumed| &resumed.device == device)
            {
                *slot = None;
            }
        }
    }

    fn observe(&mut self, event: &EiEvent, seen: &mut Vec<DeviceSeen>) {
        match event {
            EiEvent::DeviceAdded(added) => seen.push(describe("added", &added.device)),
            EiEvent::DeviceResumed(resumed) => {
                seen.push(describe("resumed", &resumed.device));
                for (slot, capability) in self.slots() {
                    if resumed.device.has_capability(capability) {
                        *slot = Some(resumed.clone());
                    }
                }
            }
            EiEvent::DevicePaused(paused) => self.clear(&paused.device),
            EiEvent::DeviceRemoved(removed) => {
                seen.push(describe("removed", &removed.device));
                self.clear(&removed.device);
            }
            EiEvent::SeatRemoved(_) | EiEvent::Disconnected(_) => *self = Self::default(),
            _ => {}
        }
    }
}

/// Drains events for `duration`, keeping `devices` current. Returns the
/// transport error text if the socket closed while draining.
fn pump(
    eis: &mut EiConnection,
    devices: &mut Devices,
    seen: &mut Vec<DeviceSeen>,
    duration: Duration,
    mut on_event: impl FnMut(&EiEvent),
) -> Option<String> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        match eis.next_event_until(deadline.saturating_duration_since(Instant::now())) {
            Ok(Some(event)) => {
                devices.observe(&event, seen);
                on_event(&event);
            }
            Ok(None) => {}
            Err(error) => return Some(error.to_string()),
        }
    }
    None
}

fn command_line(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn preflight(run: &mut Run) -> anyhow::Result<()> {
    anyhow::ensure!(
        std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value == "wayland"),
        "XDG_SESSION_TYPE is not wayland"
    );
    let (candidates, selected) = discover(current_uid()?)?;
    let selected = selected.context("no unique active Wayland session")?;
    let locked = candidates
        .iter()
        .find(|candidate| candidate.session_id == selected)
        .and_then(|candidate| candidate.properties.as_ref())
        .map(|properties| properties.locked_hint);
    anyhow::ensure!(
        locked == Some(false),
        "selected session is locked or unreadable"
    );
    run.selected_session_id = Some(selected);
    run.shell_pid_before = command_line("pidof", &["gnome-shell"]);
    anyhow::ensure!(run.shell_pid_before.is_some(), "gnome-shell PID not found");
    let rdp = command_line(
        "systemctl",
        &["--user", "is-active", "gnome-remote-desktop.service"],
    );
    anyhow::ensure!(
        rdp.as_deref() != Some("active"),
        "gnome-remote-desktop is active; mask it first (safety §5)"
    );
    run.gnome_remote_desktop_before = rdp;
    let timers = command_line(
        "systemctl",
        &["--user", "list-timers", "--all", "--no-legend"],
    );
    anyhow::ensure!(
        !timers.is_some_and(|text| text.contains("blackroom-exp")),
        "a blackroom-exp watchdog timer is pending"
    );
    run.git_head = command_line("git", &["rev-parse", "--short", "HEAD"]);
    run.git_dirty = Some(command_line("git", &["status", "--porcelain", "--", "crates"]).is_some());
    Ok(())
}

fn block(run: &mut Run, step: &str, error: impl std::fmt::Display) -> anyhow::Error {
    let message = format!("{step}: {error}");
    run.blocked = Some(message.clone());
    anyhow::anyhow!(message)
}

fn execute(args: &Args, run: &mut Run, observer: &Observer) -> anyhow::Result<()> {
    println!("Open this in a browser on the desktop, press F11, keep it focused, touch nothing:");
    println!("  {}", observer.url());
    if !observer.wait_ready(
        Duration::from_secs(args.ready_timeout_secs),
        Duration::from_secs(args.settle_secs),
    ) {
        return Err(block(
            run,
            "observer",
            "page never reported focused and fullscreen",
        ));
    }

    let conn = zbus::blocking::Connection::session().context("session bus")?;
    let authority = Authority::new(Duration::from_secs(120));
    let pace = Duration::from_millis(args.pace_ms);

    let mut session =
        RemoteDesktopSession::create(&conn).map_err(|error| block(run, "CreateSession", error))?;
    run.remote_desktop_session_path = Some(session.object_path().to_string());
    session
        .start()
        .map_err(|error| block(run, "Start", error))?;
    let mut eis = session
        .connect_to_eis(&authority.authorization(false))
        .map_err(|error| block(run, "ConnectToEIS", error))?;
    eis.handshake_sender(Duration::from_secs(5))
        .map_err(|error| block(run, "EIS handshake", error))?;

    let mut devices = Devices::default();
    let mut bound = false;
    let negotiation_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < negotiation_deadline && !(bound && devices.missing().is_empty()) {
        match eis
            .next_event_until(Duration::from_millis(250))
            .map_err(|error| block(run, "EIS negotiation", error))?
        {
            Some(EiEvent::SeatAdded(added)) if !bound => {
                eis.bind_seat(
                    &added,
                    DeviceCapability::Keyboard
                        | DeviceCapability::Pointer
                        | DeviceCapability::Button
                        | DeviceCapability::Scroll,
                )
                .map_err(|error| block(run, "EIS seat bind", error))?;
                bound = true;
            }
            Some(event) => devices.observe(&event, &mut run.devices_seen),
            None => {}
        }
    }
    if !bound || !devices.missing().is_empty() {
        let missing = devices.missing();
        return Err(block(
            run,
            "EIS negotiation",
            format!("seat bound={bound}, missing resumed devices: {missing:?}"),
        ));
    }

    // Focus must still hold after setup, before the first injected event.
    if !observer.wait_ready(Duration::from_secs(30), Duration::from_secs(2)) {
        run.aborted = Some("observer lost focus or fullscreen before injection".to_string());
        return Ok(());
    }

    let t0 = Instant::now();
    let auth = authority.authorization(false);
    macro_rules! stage {
        ($name:literal, $slot:ident, $expect:expr, |$device:ident| $call:expr) => {{
            if run.aborted.is_none() {
                if observer.ready_now(FRESH) {
                    let _ = pump(
                        &mut eis,
                        &mut devices,
                        &mut run.devices_seen,
                        Duration::from_millis(30),
                        |_| {},
                    );
                    let result = match devices.$slot.clone() {
                        Some($device) => $call,
                        None => Err(BlackroomError::new(
                            ErrorCode::MutterUnavailable,
                            "no active device for this input kind",
                        )),
                    };
                    run.record($name, $expect, result, t0);
                    sleep(pace);
                } else {
                    run.aborted = Some(format!(
                        "observer not focused and fullscreen before {}",
                        $name
                    ));
                }
            }
        }};
    }
    stage!("key_tap_f13", keyboard, Expect::Accept, |d| eis
        .send_key_tap(&auth, &d, KEY_F13));
    stage!("key_chord_shift_f13", keyboard, Expect::Accept, |d| {
        eis.send_key_chord(&auth, &d, KEY_LEFTSHIFT, KEY_F13)
    });
    stage!("pointer_right_40", pointer, Expect::Accept, |d| {
        eis.send_pointer_motion(&auth, &d, 40.0, 0.0)
    });
    stage!("pointer_left_40", pointer, Expect::Accept, |d| {
        eis.send_pointer_motion(&auth, &d, -40.0, 0.0)
    });
    stage!("button_left_click", button, Expect::Accept, |d| {
        eis.send_button_click(&auth, &d, BTN_LEFT)
    });
    stage!("scroll_down_15", scroll, Expect::Accept, |d| {
        eis.send_scroll_delta(&auth, &d, 0.0, 15.0)
    });

    let revoked = authority.authorization(true);
    stage!(
        "key_tap_after_authorization_revoked",
        keyboard,
        Expect::Refuse(&[ErrorCode::LeaseRevoked]),
        |d| eis.send_key_tap(&revoked, &d, KEY_F13)
    );

    let keyboard_before_teardown = devices.keyboard.clone();
    let stop = session
        .stop()
        .map_err(|error| BlackroomError::new(ErrorCode::MutterUnavailable, error.to_string()));
    run.record("session_stop", Expect::Accept, stop, t0);
    let mut events = Vec::new();
    let closed = pump(
        &mut eis,
        &mut devices,
        &mut run.devices_seen,
        Duration::from_secs(3),
        |event| events.push(format!("{event:?}").chars().take(80).collect::<String>()),
    );
    run.post_teardown_events = events;
    if let Some(error) = closed {
        run.post_teardown_events
            .push(format!("socket_closed: {error}"));
    }
    run.eis_ready_after_stop = Some(eis.is_ready());

    // Valid authorization: delivery after Stop is judged by the observer tally,
    // so a local not-ready refusal and a Mutter refusal both count as no delivery.
    if run.aborted.is_none() && observer.ready_now(FRESH) {
        let valid = authority.authorization(false);
        let result = match keyboard_before_teardown {
            Some(device) => eis.send_key_tap(&valid, &device, KEY_F13),
            None => Err(BlackroomError::new(
                ErrorCode::MutterUnavailable,
                "no keyboard device was active before teardown",
            )),
        };
        run.record(
            "key_tap_after_session_stop_valid_authorization",
            Expect::Observe,
            result,
            t0,
        );
    }
    Ok(())
}

fn rehearse(args: &Args) -> anyhow::Result<()> {
    let observer = Observer::start()?;
    println!(
        "Rehearsal (no input is sent). Open in a browser, press F11:\n  {}",
        observer.url()
    );
    let ready = observer.wait_ready(
        Duration::from_secs(args.ready_timeout_secs),
        Duration::from_secs(args.settle_secs),
    );
    println!("focused and fullscreen for {} s: {ready}", args.settle_secs);
    for _ in 0..15 {
        sleep(Duration::from_secs(1));
        let (focus, fullscreen, tally) =
            observer.snapshot(|state| (state.focus, state.fullscreen, state.tally.clone()));
        let notes = tally.as_ref().map(|tally| evaluate_tally(tally).1);
        println!("  focus={focus} fullscreen={fullscreen} tally_gaps={notes:?}");
    }
    anyhow::ensure!(ready, "observer page never reported focused and fullscreen");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    if args.observer_only {
        return rehearse(&args);
    }
    require_operator(&args)?;
    let now = OffsetDateTime::now_utc();
    let mut run = Run::default();

    if let Err(error) = preflight(&mut run) {
        run.blocked = Some(format!("preflight: {error}"));
    } else {
        let observer = Observer::start()?;
        if let Err(error) = execute(&args, &mut run, &observer)
            && run.blocked.is_none()
        {
            run.failure = Some(error.to_string());
        }
        observer.wait_beats(2, Duration::from_secs(3));
        run.observer_beats = observer.snapshot(|state| state.beats);
        run.observer_tally = observer.snapshot(|state| state.tally.clone());
    }

    run.shell_pid_after = command_line("pidof", &["gnome-shell"]);
    run.gnome_remote_desktop_after = command_line(
        "systemctl",
        &["--user", "is-active", "gnome-remote-desktop.service"],
    );
    if run.shell_pid_before.is_some() && run.shell_pid_before != run.shell_pid_after {
        run.violations.push("gnome-shell PID changed".to_string());
    }
    if let Some(path) = run.remote_desktop_session_path.clone() {
        // A real method call, not Introspect: Mutter answers Introspect for dead paths.
        let stale = zbus::blocking::Connection::session().and_then(|conn| {
            zbus::blocking::Proxy::new(
                &conn,
                "org.gnome.Mutter.RemoteDesktop",
                path,
                "org.gnome.Mutter.RemoteDesktop.Session",
            )?
            .call::<_, _, ()>("Stop", &())
        });
        match stale {
            Ok(()) => run
                .violations
                .push("session object still alive after Stop".to_string()),
            Err(error) => run.stale_session_call = Some(error.to_string()),
        }
    }
    if let Some(tally) = &run.observer_tally {
        let (matches, notes) = evaluate_tally(tally);
        run.tally_matches = Some(matches);
        if !matches && run.aborted.is_none() && run.blocked.is_none() {
            run.violations
                .push(format!("observer tally mismatch: {notes:?}"));
        }
        run.tally_notes = notes;
    }

    let result = classify(&run);
    let observed = format!(
        "result={result}; blocked={:?}; failure={:?}; aborted={:?}; stages={}; tally_matches={:?}; \
         violations={:?}; shell {:?}->{:?}",
        run.blocked,
        run.failure,
        run.aborted,
        run.stages.len(),
        run.tally_matches,
        run.violations,
        run.shell_pid_before,
        run.shell_pid_after
    );
    println!("{observed}");
    for stage in &run.stages {
        println!(
            "  {:>5} ms {:<52} {}",
            stage.at_ms, stage.name, stage.outcome
        );
    }
    if run.stuck_input_suspect {
        println!(
            "A stage failed while accepting input: tap Shift and the left mouse button once physically."
        );
    }

    let report = ExperimentReport {
        experiment: "Experiment 8 — Remote Input (FEAS-D)".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1, single built-in display"
            .to_string(),
        objective: "Prove a signed-lease-gated EIS Sender from RemoteDesktop.Session.ConnectToEIS delivers \
                    keyboard, pointer, click and scroll to the existing session and that no input arrives \
                    after an authorization revoke or session teardown."
            .to_string(),
        hypothesis: "Mutter accepts the Sender handshake, advertises keyboard/pointer/button/scroll devices, \
                     delivers the injected events to the focused observer page, and delivers nothing after \
                     the revoke and the session stop."
            .to_string(),
        procedure: format!(
            "Preflight; serve the observer page on loopback; wait for a focused fullscreen page ({} s settle); \
             CreateSession; Start; ConnectToEIS; bind seat; F13 tap, Shift+F13 chord, pointer +40/-40, left \
             click, scroll 15; revoked-authorization tap (must be refused); Stop; valid-authorization tap \
             after Stop (delivery judged by the page); stale-path Stop call; Shell PID unchanged.",
            args.settle_secs
        ),
        expected: "Every injected stage accepted; the revoked stage refused as LeaseRevoked; observer tally: \
                   F13 2/2, ShiftLeft 1/1, one left click, net-zero pointer motion, positive scroll, no \
                   untrusted events and nothing from the revoked or post-stop attempts."
            .to_string(),
        observed,
        evidence: vec!["findings.json (this directory), including the page's own tally".to_string()],
        result,
        failure: run.failure.clone().or_else(|| run.blocked.clone()),
        root_cause: None,
        security_impact: Some(
            "Input injected into the live desktop; only inert keys and a net-zero pointer move were sent, \
             gated on the observer page holding focus. PASS here does not by itself promote FEAS-D."
                .to_string(),
        ),
        recommended_action: None,
        follow_up: Some("FEAS-D decision is a separate evidence review (plan Step 4).".to_string()),
    };
    let dir = evidence_dir(EXP_ID, now)?;
    write_evidence(&dir, &report.render(now), "findings.json", &run)?;
    println!("Wrote evidence to {}", dir.display());
    std::process::exit(match result {
        ExperimentResult::Pass => 0,
        ExperimentResult::Fail => 1,
        ExperimentResult::Blocked => 2,
        _ => 3,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn refusal(code: ErrorCode) -> Result<(), BlackroomError> {
        Err(BlackroomError::new(code, "test"))
    }

    fn good_tally() -> Value {
        json!({
            "keys": [
                {"type": "down", "code": "F13"}, {"type": "up", "code": "F13"},
                {"type": "down", "code": "ShiftLeft"}, {"type": "down", "code": "F13"},
                {"type": "up", "code": "F13"}, {"type": "up", "code": "ShiftLeft"},
            ],
            "buttons": [
                {"type": "down", "button": 0}, {"type": "up", "button": 0}, {"type": "click", "button": 0},
            ],
            "pointer": {"moves": 2, "netX": 0, "netY": 0, "absX": 80, "absY": 0},
            "wheel": {"events": 1, "deltaY": 15, "scrollY": 15},
            "untrusted": 0,
        })
    }

    fn http(port: u16, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("timeout");
        stream.write_all(request.as_bytes()).expect("write");
        let mut out = String::new();
        let _ = stream.read_to_string(&mut out);
        out
    }

    #[test]
    fn injection_requires_the_operator_flag_and_bounded_timing() {
        assert!(require_operator(&Args::parse_from(["exp08"])).is_err());
        assert!(require_operator(&Args::parse_from(["exp08", "--operator-present"])).is_ok());
        assert!(Args::try_parse_from(["exp08", "--settle-secs", "1"]).is_err());
        assert!(Args::try_parse_from(["exp08", "--pace-ms", "5000"]).is_err());
    }

    #[test]
    fn stages_flag_unexpected_outcomes_and_only_a_matching_tally_passes() {
        let t0 = Instant::now();
        let mut run = Run::default();
        run.record("ok", Expect::Accept, Ok(()), t0);
        run.record(
            "revoked",
            Expect::Refuse(&[ErrorCode::LeaseRevoked]),
            refusal(ErrorCode::LeaseRevoked),
            t0,
        );
        run.record(
            "post_stop",
            Expect::Observe,
            refusal(ErrorCode::MutterUnavailable),
            t0,
        );
        assert_eq!(classify(&run), ExperimentResult::Partial);
        run.tally_matches = Some(true);
        assert_eq!(classify(&run), ExperimentResult::Pass);
        run.aborted = Some("focus lost".to_string());
        assert_eq!(classify(&run), ExperimentResult::Partial);
        run.aborted = None;

        run.record(
            "wrong_cause",
            Expect::Refuse(&[ErrorCode::LeaseRevoked]),
            refusal(ErrorCode::LeaseInvalid),
            t0,
        );
        run.record(
            "leaked",
            Expect::Refuse(&[ErrorCode::LeaseRevoked]),
            Ok(()),
            t0,
        );
        run.record(
            "stalled",
            Expect::Accept,
            refusal(ErrorCode::MutterUnavailable),
            t0,
        );
        assert_eq!(run.violations.len(), 3);
        assert!(run.stuck_input_suspect);
        assert_eq!(classify(&run), ExperimentResult::Fail);

        run.blocked = Some("ConnectToEIS".to_string());
        assert_eq!(classify(&run), ExperimentResult::Blocked);
    }

    #[test]
    fn tally_accepts_the_exact_sequence_and_rejects_leaks_and_drift() {
        assert_eq!(evaluate_tally(&good_tally()), (true, vec![]));

        let mut leaked = good_tally();
        leaked["keys"]
            .as_array_mut()
            .expect("keys")
            .push(json!({"type": "down", "code": "F13"}));
        assert!(!evaluate_tally(&leaked).0);

        let mut drifted = good_tally();
        drifted["pointer"]["netX"] = json!(40);
        assert!(!evaluate_tally(&drifted).0);

        let mut synthetic = good_tally();
        synthetic["untrusted"] = json!(1);
        assert!(!evaluate_tally(&synthetic).0);
        assert!(!evaluate_tally(&json!({})).0);
    }

    #[test]
    fn authority_signs_a_lease_that_validates_and_revokes() {
        let authority = Authority::new(Duration::from_secs(30));
        assert!(authority.authorization(false).validate().is_ok());
        assert_eq!(
            authority
                .authorization(true)
                .validate()
                .map_err(|error| error.code),
            Err(ErrorCode::LeaseRevoked)
        );
    }

    #[test]
    fn observer_serves_only_its_token_and_host_and_tracks_focus() {
        let observer = Observer::start().expect("observer");
        let (port, token) = (observer.port, observer.token.clone());
        let host = format!("Host: 127.0.0.1:{port}\r\n");
        let get = |path: &str, host: &str| format!("GET {path} HTTP/1.1\r\n{host}\r\n");
        assert!(http(port, &get(&format!("/{token}/"), &host)).contains("exp08 observer"));
        assert!(http(port, &get("/wrong/", &host)).starts_with("HTTP/1.1 404"));
        assert!(
            http(port, &get(&format!("/{token}/"), "Host: evil.example\r\n"))
                .starts_with("HTTP/1.1 403")
        );

        assert!(!observer.ready_now(FRESH));
        let body = json!({"focus": true, "fullscreen": true, "tally": {}}).to_string();
        let post = |body: &str| {
            format!(
                "POST /{token}/beat HTTP/1.1\r\n{host}Content-Length: {}\r\n\r\n{body}",
                body.len()
            )
        };
        assert!(http(port, &post(&body)).starts_with("HTTP/1.1 204"));
        assert!(observer.ready_now(FRESH));
        assert!(http(port, &post("not json")).starts_with("HTTP/1.1 400"));
        let blurred = json!({"focus": false, "fullscreen": true, "tally": {}}).to_string();
        assert!(http(port, &post(&blurred)).starts_with("HTTP/1.1 204"));
        assert!(!observer.ready_now(FRESH));
        let oversized = format!(
            "POST /{token}/beat HTTP/1.1\r\n{host}Content-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(http(port, &oversized), "");
    }
}
