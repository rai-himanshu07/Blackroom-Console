//! Experiment 11 — Lock semantics (Document 10 Experiment 11; Phase 8; Gate FEAS-A).
//!
//! MUTATING and supervised: locks the selected existing GNOME session once (`loginctl
//! lock-session`) while a RemoteDesktop/EIS sender and, unless `--skip-capture`, a ScreenCast
//! monitor capture are attached, injects a few harmless events into the lock screen, and waits
//! for the operator to unlock with their own password. Nothing here touches displays, grabs
//! physical input, or ever handles a password.
//!
//! The observer page (loopback, one-time token) is the independent witness. Its tally is judged
//! at three points: before the lock the injected Shift tap must arrive, while locked NOTHING
//! injected may reach the page (the lock screen must take it), and after the unlock the same
//! EIS connection must still deliver Shift, `a` and Left. The page's heartbeat must keep running
//! during the lock, otherwise "saw nothing" proves nothing and the run is PARTIAL.
//!
//! The ScreenSaver owner is not mapped to the login1 session on this host, so the lock signals
//! (`GetActive`, `LockedHint`, `ActiveChanged`) are recorded but not provenance-verified; the
//! operator's own look at the lock screen goes into the observation, not into the verdict.

use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::Context;
use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_experiments::eis_support::{
    Authority, DeviceSeen, Devices, bind_devices, command_line, pump,
};
use blackroom_experiments::observer::{FRESH, Observer};
use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, evidence_dir, write_evidence,
};
use blackroom_gnome::backend::SessionInfo;
use blackroom_gnome::mutter::display_config;
use blackroom_gnome::mutter::lock::{self, LockObservation};
use blackroom_gnome::mutter::pipewire_capture::{CaptureOutcome, capture_until_stopped};
use blackroom_gnome::mutter::remote_desktop::RemoteDesktopSession;
use blackroom_gnome::mutter::screencast::{ScreenCastSession, ScreenCastStream};
use blackroom_gnome::mutter::session::discover_session;
use clap::Parser;
use reis::event::EiEvent;
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;
use zbus::blocking::{Connection, Proxy};

const EXP_ID: &str = "exp11";
const OBSERVER_PAGE: &str = include_str!("../../assets/exp08_observer.html");
// Checked 2026-10-01 against gsettings and xkb: no bare binding for these evdev keys.
const KEY_ESC: u32 = 1;
const KEY_A: u32 = 30;
const KEY_X: u32 = 45;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_LEFT: u32 = 105;
const LOCK_WAIT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(400);
/// Frames each phase must deliver; the consumer stays attached the whole run.
const CAPTURE_MIN_FRAMES: u32 = 1;
const CAPTURE_MAX: Duration = Duration::from_secs(900);
/// The page must keep beating through the lock for its silence to count as evidence (a page
/// throttled to one beat a second still gives about a dozen in the injection window).
const MIN_BEATS_DURING_LOCK: u64 = 8;
const MAX_BEAT_AGE: Duration = Duration::from_secs(3);
/// Grab holders and earlier experiment binaries (kernel `comm` is cut at 15 characters).
const GRAB_HOLDERS: [&str; 6] = [
    "remote-emergenc",
    "remote-hostd",
    "remote-gateway",
    "exp09_grab_prob",
    "exp09_freeze",
    "exp08_remote_in",
];

#[derive(Parser, Debug)]
#[command(about = "Experiment 11: lock the live session with EIS and capture attached (MUTATING)")]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Required: the operator is present, work is saved, a second-device SSH session is open and
    /// the operator will unlock with their own password (docs/ops/experiment-safety.md §7).
    #[arg(long, default_value_t = false)]
    operator_present: bool,
    /// Seconds the observer page must stay focused and fullscreen before the run starts.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(3..=30))]
    settle_secs: u64,
    /// Seconds to wait for the operator to focus the observer page.
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(30..=600))]
    ready_timeout_secs: u64,
    /// Seconds to wait for the operator to unlock.
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(30..=600))]
    unlock_wait_secs: u64,
    /// Do not attach a ScreenCast monitor capture.
    #[arg(long, default_value_t = false)]
    skip_capture: bool,
}

#[derive(Debug, Serialize)]
struct Stage {
    name: String,
    outcome: String,
    at_ms: u64,
    violation: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct LockSnap {
    screen_saver_active: bool,
    logind_locked_hint: bool,
    /// False on this host: the ScreenSaver owner is not in the login1 session.
    owner_session_verified: bool,
}

impl From<LockObservation> for LockSnap {
    fn from(value: LockObservation) -> Self {
        Self {
            screen_saver_active: value.screen_saver_active,
            logind_locked_hint: value.logind_locked_hint,
            owner_session_verified: value.screen_saver_session_verified,
        }
    }
}

impl LockSnap {
    fn locked(self) -> bool {
        self.screen_saver_active && self.logind_locked_hint
    }

    fn unlocked(self) -> bool {
        !self.screen_saver_active && !self.logind_locked_hint
    }
}

#[derive(Debug, Serialize)]
struct CaptureWindow {
    phase: &'static str,
    frames: u32,
}

/// Counts only: a leak while locked must not persist whatever the operator typed.
#[derive(Debug, Serialize)]
struct LockedCounts {
    keys: u64,
    buttons: u64,
    pointer_moves: u64,
    wheel_events: u64,
}

fn locked_counts(tally: &Value) -> LockedCounts {
    let length = |value: &Value| {
        value
            .as_array()
            .map_or(u64::MAX, |items| items.len() as u64)
    };
    LockedCounts {
        keys: length(&tally["keys"]),
        buttons: length(&tally["buttons"]),
        pointer_moves: tally["pointer"]["moves"].as_u64().unwrap_or(u64::MAX),
        wheel_events: tally["wheel"]["events"].as_u64().unwrap_or(u64::MAX),
    }
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
    /// Reasons the page's silence or the lock state cannot be trusted.
    inconclusive: Vec<String>,
    devices_seen: Vec<DeviceSeen>,
    stages: Vec<Stage>,
    initial_lock: Option<LockSnap>,
    lock_engaged_after_ms: Option<u64>,
    unlock_observed_after_ms: Option<u64>,
    locked_at_snapshot: Option<LockSnap>,
    active_changed: Vec<(u64, bool)>,
    eis_events_during_lock: Vec<String>,
    devices_missing_after_lock: Vec<&'static str>,
    eis_ready_after_unlock: Option<bool>,
    beats_during_lock: u64,
    beats_total: u64,
    tally_pre: Option<Value>,
    tally_locked: Option<LockedCounts>,
    tally_unlocked: Option<Value>,
    notes_pre: Vec<String>,
    notes_locked: Vec<String>,
    notes_unlocked: Vec<String>,
    capture: Vec<CaptureWindow>,
    violations: Vec<String>,
}

impl Run {
    fn record(&mut self, name: &str, result: Result<(), BlackroomError>, t0: Instant) {
        let (outcome, violation) = match &result {
            Ok(()) => ("accepted".to_string(), false),
            Err(error) => (format!("refused:{:?}", error.code), true),
        };
        if violation {
            self.violations.push(format!("{name}: {outcome}"));
        }
        self.stages.push(Stage {
            name: name.to_string(),
            outcome,
            at_ms: elapsed_ms(t0),
            violation,
        });
    }

    fn complete(&self) -> bool {
        self.tally_pre.is_some()
            && self.tally_locked.is_some()
            && self.tally_unlocked.is_some()
            && self.lock_engaged_after_ms.is_some()
            && self.unlock_observed_after_ms.is_some()
    }
}

fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn classify(run: &Run) -> ExperimentResult {
    if run.blocked.is_some() {
        ExperimentResult::Blocked
    } else if run.failure.is_some() || !run.violations.is_empty() {
        ExperimentResult::Fail
    } else if run.aborted.is_some() || !run.inconclusive.is_empty() || !run.complete() {
        ExperimentResult::Partial
    } else {
        ExperimentResult::Pass
    }
}

fn require_operator(args: &Args) -> anyhow::Result<()> {
    anyhow::ensure!(
        args.operator_present,
        "refusing to lock the session: pass --operator-present only after the safety preflight in \
         docs/ops/experiment-safety.md §7 (work saved, second-device SSH open, you will unlock)"
    );
    Ok(())
}

fn key_events(tally: &Value, kind: &str, code: &str) -> usize {
    tally["keys"].as_array().map_or(0, |keys| {
        keys.iter()
            .filter(|key| key["type"] == kind && key["code"] == code)
            .count()
    })
}

fn untrusted(tally: &Value) -> u64 {
    tally["untrusted"].as_u64().unwrap_or(u64::MAX)
}

/// Before the lock only the injected Shift tap may have arrived.
fn judge_pre(tally: &Value) -> Vec<String> {
    let mut notes = Vec::new();
    let total = tally["keys"].as_array().map_or(0, Vec::len);
    if key_events(tally, "down", "ShiftLeft") != 1 || key_events(tally, "up", "ShiftLeft") != 1 {
        notes.push("pre-lock Shift tap did not arrive exactly once".to_string());
    }
    if total != 2 {
        notes.push(format!("{total} key events before the lock (expected 2)"));
    }
    if untrusted(tally) != 0 {
        notes.push("page saw untrusted events before the lock".to_string());
    }
    notes
}

/// While locked nothing injected (or physical) may reach a window behind the lock screen. The
/// pointer and wheel counts are the real witness: the page has no keyboard focus while locked.
fn judge_locked(counts: &LockedCounts) -> Vec<String> {
    let total = [
        counts.keys,
        counts.buttons,
        counts.pointer_moves,
        counts.wheel_events,
    ]
    .into_iter()
    .fold(0_u64, u64::saturating_add);
    if total == 0 {
        return Vec::new();
    }
    vec![format!(
        "input reached the page while locked: {} key, {} button, {} pointer, {} wheel events",
        counts.keys, counts.buttons, counts.pointer_moves, counts.wheel_events
    )]
}

/// After the unlock the same EIS connection must deliver Shift, `a` and Left, plain.
fn judge_unlocked(tally: &Value) -> Vec<String> {
    let mut notes = Vec::new();
    let keys = tally["keys"].as_array().cloned().unwrap_or_default();
    for code in ["ShiftLeft", "KeyA", "ArrowLeft"] {
        if key_events(tally, "down", code) != 1 || key_events(tally, "up", code) != 1 {
            notes.push(format!("{code} not exactly one down/up after the unlock"));
        }
    }
    let other = keys
        .iter()
        .filter(|key| {
            !matches!(
                key["code"].as_str(),
                Some("ShiftLeft" | "KeyA" | "ArrowLeft")
            )
        })
        .count();
    if other != 0 {
        notes.push(format!("{other} unexpected key events after the unlock"));
    }
    if keys.iter().any(|key| key["repeat"] == true) {
        notes.push("a repeated key event after the unlock".to_string());
    }
    if untrusted(tally) != 0 {
        notes.push("page saw untrusted events after the unlock".to_string());
    }
    notes
}

fn eis_label(event: &EiEvent) -> &'static str {
    match event {
        EiEvent::DeviceAdded(_) => "DeviceAdded",
        EiEvent::DeviceResumed(_) => "DeviceResumed",
        EiEvent::DevicePaused(_) => "DevicePaused",
        EiEvent::DeviceRemoved(_) => "DeviceRemoved",
        EiEvent::SeatRemoved(_) => "SeatRemoved",
        EiEvent::Disconnected(_) => "Disconnected",
        _ => "Other",
    }
}

fn lock_state(info: &SessionInfo) -> Option<LockSnap> {
    lock::observe(info).ok().map(LockSnap::from)
}

/// Polls until `matches` holds; returns the milliseconds since `since`.
fn wait_lock_state(
    info: &SessionInfo,
    matches: fn(LockSnap) -> bool,
    timeout: Duration,
    since: Instant,
) -> Option<u64> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if lock_state(info).is_some_and(matches) {
            return Some(elapsed_ms(since));
        }
        sleep(POLL);
    }
    None
}

/// Records every `ActiveChanged` the ScreenSaver emits, with milliseconds since `t0`.
fn watch_active_changed(t0: Instant) -> Arc<Mutex<Vec<(u64, bool)>>> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    std::thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            return;
        };
        let Ok(proxy) = Proxy::new(
            &connection,
            "org.gnome.ScreenSaver",
            "/org/gnome/ScreenSaver",
            "org.gnome.ScreenSaver",
        ) else {
            return;
        };
        let Ok(signals) = proxy.receive_signal("ActiveChanged") else {
            return;
        };
        for message in signals {
            if let Ok((active,)) = message.body().deserialize::<(bool,)>() {
                sink.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((elapsed_ms(t0), active));
            }
        }
    });
    log
}

struct Cast<'a> {
    _session: ScreenCastSession<'a>,
    _stream: ScreenCastStream<'a>,
    node: u32,
    width: i32,
    height: i32,
}

fn start_capture<'a>(conn: &'a Connection, session_id: &str) -> Result<Cast<'a>, BlackroomError> {
    let backup = display_config::snapshot(conn, session_id)?;
    let output = backup
        .outputs
        .iter()
        .find(|output| output.enabled)
        .ok_or_else(|| {
            BlackroomError::new(ErrorCode::MutterUnavailable, "no enabled output to capture")
        })?;
    let session = ScreenCastSession::create(conn)?;
    let stream = session.record_monitor(&output.connector)?;
    let node = stream.start_and_wait_for_pipewire_node(&session)?;
    Ok(Cast {
        _session: session,
        _stream: stream,
        node,
        width: output.width,
        height: output.height,
    })
}

/// One PipeWire consumer that stays attached through lock and unlock; the running frame count is
/// read at phase boundaries.
struct Capture {
    stop: Arc<AtomicBool>,
    frames: Arc<AtomicU32>,
    mark: u32,
    handle: Option<JoinHandle<Result<CaptureOutcome, BlackroomError>>>,
}

impl Capture {
    fn spawn(cast: &Cast<'_>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let frames = Arc::new(AtomicU32::new(0));
        let (node, width, height) = (cast.node, cast.width, cast.height);
        let handle = {
            let (stop, frames) = (Arc::clone(&stop), Arc::clone(&frames));
            std::thread::spawn(move || {
                capture_until_stopped(node, width, height, stop, frames, CAPTURE_MAX)
            })
        };
        Self {
            stop,
            frames,
            mark: 0,
            handle: Some(handle),
        }
    }

    /// Starts a new phase without judging the one before.
    fn reset(&mut self) {
        self.mark = self.frames.load(Ordering::Relaxed);
    }

    /// Frames delivered since the last mark or reset.
    fn take(&mut self) -> u32 {
        let total = self.frames.load(Ordering::Relaxed);
        let frames = total.saturating_sub(self.mark);
        self.mark = total;
        frames
    }

    /// Stops the consumer and returns its error text, if it ended with one.
    fn finish(&mut self) -> Option<String> {
        self.stop.store(true, Ordering::Relaxed);
        match self.handle.take()?.join() {
            Ok(Ok(_)) => None,
            Ok(Err(error)) => Some(error.to_string()),
            Err(_) => Some("capture thread panicked".to_string()),
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// `strict`: too few frames is a violation. The lock screen is mostly static and the stream is
/// damage-driven, so the locked window is only inconclusive when it is quiet.
fn capture_window(run: &mut Run, capture: Option<&mut Capture>, phase: &'static str, strict: bool) {
    let Some(capture) = capture else {
        return;
    };
    let frames = capture.take();
    if frames < CAPTURE_MIN_FRAMES {
        let note = format!("capture {phase}: {frames} frames");
        if strict {
            run.violations.push(note);
        } else {
            run.inconclusive.push(note);
        }
    }
    run.capture.push(CaptureWindow { phase, frames });
}

fn block(run: &mut Run, step: &str, error: impl std::fmt::Display) -> anyhow::Error {
    let message = format!("{step}: {error}");
    run.blocked = Some(message.clone());
    anyhow::anyhow!(message)
}

fn preflight(run: &mut Run) -> anyhow::Result<SessionInfo> {
    anyhow::ensure!(
        std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value == "wayland"),
        "XDG_SESSION_TYPE is not wayland"
    );
    let info = discover_session().map_err(|error| anyhow::anyhow!("session discovery: {error}"))?;
    let state = lock::observe(&info).map_err(|error| anyhow::anyhow!("lock state: {error}"))?;
    run.initial_lock = Some(state.into());
    anyhow::ensure!(
        LockSnap::from(state).unlocked(),
        "the session already reports a lock; unlock it first"
    );
    run.selected_session_id = Some(info.session_id.clone());
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
    for holder in GRAB_HOLDERS {
        anyhow::ensure!(
            command_line("pgrep", &["-x", holder]).is_none(),
            "{holder} is running: a physical-input grab would leave only SSH to unlock"
        );
    }
    run.git_head = command_line("git", &["rev-parse", "--short", "HEAD"]);
    run.git_dirty = Some(command_line("git", &["status", "--porcelain", "--", "crates"]).is_some());
    Ok(info)
}

fn execute(
    args: &Args,
    info: &SessionInfo,
    run: &mut Run,
    observer: &Observer,
    t0: Instant,
) -> anyhow::Result<()> {
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

    let conn = Connection::session().context("session bus")?;
    let authority = Authority::new(Duration::from_secs(600));
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
    bind_devices(&mut eis, &mut devices, &mut run.devices_seen)
        .map_err(|(step, detail)| block(run, step, detail))?;

    let cast = if args.skip_capture {
        None
    } else {
        match start_capture(&conn, &info.session_id) {
            Ok(cast) => Some(cast),
            Err(error) => return Err(block(run, "ScreenCast monitor capture", error)),
        }
    };

    let mut capture = cast.as_ref().map(Capture::spawn);
    sleep(Duration::from_secs(2));
    if let Some(capture) = capture.as_mut() {
        capture.reset();
    }

    // Phase P: the injection path works before the lock.
    if !observer.arm(Duration::from_secs(10)) {
        run.aborted = Some("observer did not acknowledge the tally reset while focused".into());
        return Ok(());
    }
    macro_rules! tap {
        ($name:expr, $key:expr) => {{
            let _ = pump(
                &mut eis,
                &mut devices,
                &mut run.devices_seen,
                Duration::from_millis(30),
                |_| {},
            );
            let result = match devices.keyboard.clone() {
                Some(device) => eis.send_key_tap(&authority.authorization(false), &device, $key),
                None => Err(BlackroomError::new(
                    ErrorCode::MutterUnavailable,
                    "no active keyboard device",
                )),
            };
            run.record($name, result, t0);
            sleep(Duration::from_millis(300));
        }};
    }
    tap!("pre_lock_key_tap_shift", KEY_LEFTSHIFT);
    sleep(Duration::from_secs(1));
    capture_window(run, capture.as_mut(), "before_lock", true);
    observer.wait_beats(2, Duration::from_secs(3));
    run.tally_pre = observer.snapshot(|state| state.tally.clone());
    let pre_notes = judge_pre(run.tally_pre.as_ref().unwrap_or(&Value::Null));
    if !pre_notes.is_empty() || !run.violations.is_empty() || !devices.missing().is_empty() {
        run.aborted = Some(format!(
            "pre-lock checks failed, not locking: {pre_notes:?}"
        ));
        return Ok(());
    }

    // Phase L: lock, then inject into the lock screen.
    observer.set_prompt(
        "LOCKING in 8 s. When the lock screen appears: HANDS OFF for 20 s (the program types into it), \
         then press Esc once, then type your password to unlock.",
    );
    sleep(Duration::from_secs(8));
    if !observer.arm(Duration::from_secs(10)) {
        run.aborted = Some("observer lost focus before the lock".into());
        return Ok(());
    }
    let lock_started = Instant::now();
    let locked = Command::new("loginctl")
        .args(["lock-session", &info.session_id])
        .status();
    if !locked.is_ok_and(|status| status.success()) {
        run.aborted = Some("loginctl lock-session failed".into());
        return Ok(());
    }
    let Some(engaged) = wait_lock_state(info, LockSnap::locked, LOCK_WAIT, lock_started) else {
        run.aborted = Some("lock signals never both reported a lock within 15 s".into());
        return Ok(());
    };
    run.lock_engaged_after_ms = Some(engaged);
    if let Some(capture) = capture.as_mut() {
        capture.reset();
    }
    let beats_at_lock = observer.snapshot(|state| state.beats);
    println!(
        "LOCKED after {engaged} ms. Hands off for 20 s: the program types into the lock screen, then \
         waits up to {} s for YOU to unlock (Esc once first if dots show).",
        args.unlock_wait_secs
    );
    sleep(Duration::from_secs(2));

    let still_locked = |run: &mut Run, step: &str| -> bool {
        let ok = lock_state(info).is_some_and(LockSnap::locked);
        if !ok {
            run.aborted = Some(format!("lock state not confirmed before {step}"));
        }
        ok
    };
    let mut lock_events: Vec<String> = Vec::new();
    macro_rules! locked_drain {
        ($duration:expr) => {
            if let Some(error) = pump(
                &mut eis,
                &mut devices,
                &mut run.devices_seen,
                $duration,
                |event| lock_events.push(eis_label(event).to_string()),
            ) {
                lock_events.push(format!("transport_closed: {error}"));
            }
        };
    }
    macro_rules! locked_pointer {
        ($name:expr, $dx:expr) => {{
            if run.aborted.is_none() && still_locked(run, $name) {
                locked_drain!(Duration::from_millis(30));
                let result = match devices.pointer.clone() {
                    Some(device) => {
                        eis.send_pointer_motion(&authority.authorization(false), &device, $dx, 0.0)
                    }
                    None => Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "no active pointer device",
                    )),
                };
                run.record($name, result, t0);
                sleep(Duration::from_millis(300));
            }
        }};
    }
    macro_rules! locked_key {
        ($name:expr, $key:expr) => {{
            if run.aborted.is_none() && still_locked(run, $name) {
                locked_drain!(Duration::from_millis(30));
                let result = match devices.keyboard.clone() {
                    Some(device) => {
                        eis.send_key_tap(&authority.authorization(false), &device, $key)
                    }
                    None => Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "no active keyboard device",
                    )),
                };
                run.record($name, result, t0);
                sleep(Duration::from_millis(300));
            }
        }};
    }
    locked_pointer!("locked_pointer_right_5", 5.0);
    locked_pointer!("locked_pointer_left_5", -5.0);
    locked_key!("locked_key_tap_shift", KEY_LEFTSHIFT);
    locked_key!("locked_key_tap_x_1", KEY_X);
    locked_key!("locked_key_tap_x_2", KEY_X);
    locked_key!("locked_key_tap_x_3", KEY_X);
    // Hold the dots on screen long enough to see them, then clear the entry.
    if run.aborted.is_none() {
        locked_drain!(Duration::from_secs(6));
    }
    locked_key!("locked_key_tap_escape", KEY_ESC);
    if run.aborted.is_none() {
        capture_window(run, capture.as_mut(), "during_lock", false);
    }
    run.eis_events_during_lock = lock_events;
    run.devices_missing_after_lock = devices.missing();
    observer.wait_beats(2, Duration::from_secs(5));
    let snapshot_lock = lock_state(info);
    run.locked_at_snapshot = snapshot_lock;
    run.beats_during_lock = observer
        .snapshot(|state| state.beats)
        .saturating_sub(beats_at_lock);
    run.tally_locked = observer
        .snapshot(|state| state.tally.clone())
        .map(|tally| locked_counts(&tally));
    if !observer.snapshot(|state| state.beat_at.is_some_and(|at| at.elapsed() <= MAX_BEAT_AGE)) {
        run.inconclusive.push(
            "the page's last heartbeat was stale when the locked tally was taken".to_string(),
        );
    }
    if !snapshot_lock.is_some_and(LockSnap::locked) {
        run.inconclusive
            .push("the session was no longer locked when the locked tally was taken".to_string());
    }
    if run.beats_during_lock < MIN_BEATS_DURING_LOCK {
        run.inconclusive.push(format!(
            "page sent only {} heartbeats during the lock (needs {MIN_BEATS_DURING_LOCK})",
            run.beats_during_lock
        ));
    }

    // Phase U: the operator unlocks; the same connection must still work.
    let unlock_started = Instant::now();
    let Some(unlocked) = wait_lock_state(
        info,
        LockSnap::unlocked,
        Duration::from_secs(args.unlock_wait_secs),
        unlock_started,
    ) else {
        run.aborted = Some("no unlock observed in time (unlock manually; SSH `loginctl unlock-session` is the fallback)".into());
        return Ok(());
    };
    run.unlock_observed_after_ms = Some(unlocked);
    observer.set_prompt("UNLOCKED. Hands off until the program finishes.");
    if let Some(capture) = capture.as_mut() {
        capture.reset();
    }
    if !observer.wait_ready(Duration::from_secs(90), Duration::from_secs(3)) {
        run.aborted =
            Some("observer page not focused and fullscreen again after the unlock".into());
        return Ok(());
    }
    if !observer.arm(Duration::from_secs(10)) {
        run.aborted = Some("observer did not acknowledge the tally reset after the unlock".into());
        return Ok(());
    }
    run.eis_ready_after_unlock = Some(eis.is_ready());
    tap!("unlocked_key_tap_shift", KEY_LEFTSHIFT);
    tap!("unlocked_key_tap_a", KEY_A);
    tap!("unlocked_key_tap_left", KEY_LEFT);
    sleep(Duration::from_secs(1));
    capture_window(run, capture.as_mut(), "after_unlock", true);
    observer.wait_beats(2, Duration::from_secs(3));
    run.tally_unlocked = observer.snapshot(|state| state.tally.clone());
    if let Some(error) = capture.as_mut().and_then(Capture::finish) {
        run.violations
            .push(format!("capture consumer ended with an error: {error}"));
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    require_operator(&args)?;
    let now = OffsetDateTime::now_utc();
    let mut run = Run::default();

    match preflight(&mut run) {
        Err(error) => run.blocked = Some(format!("preflight: {error}")),
        Ok(info) => {
            let observer = Observer::start(OBSERVER_PAGE)?;
            let t0 = Instant::now();
            let active_changed = watch_active_changed(t0);
            if let Err(error) = execute(&args, &info, &mut run, &observer, t0)
                && run.blocked.is_none()
            {
                run.failure = Some(error.to_string());
            }
            run.active_changed = active_changed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            observer.wait_beats(2, Duration::from_secs(3));
            run.beats_total = observer.snapshot(|state| state.beats);
            if !observer.ready_now(FRESH) {
                run.inconclusive
                    .push("observer page not focused at the end of the run".to_string());
            }
        }
    }

    run.shell_pid_after = command_line("pidof", &["gnome-shell"]);
    run.gnome_remote_desktop_after = command_line(
        "systemctl",
        &["--user", "is-active", "gnome-remote-desktop.service"],
    );
    if run.shell_pid_before.is_some() && run.shell_pid_before != run.shell_pid_after {
        run.violations.push("gnome-shell PID changed".to_string());
    }
    if let Some(tally) = run.tally_pre.clone() {
        run.notes_pre = judge_pre(&tally);
    }
    if let Some(counts) = &run.tally_locked {
        run.notes_locked = judge_locked(counts);
    }
    if let Some(tally) = run.tally_unlocked.clone() {
        run.notes_unlocked = judge_unlocked(&tally);
    }
    for (label, notes) in [
        ("pre-lock", &run.notes_pre),
        ("locked", &run.notes_locked),
        ("unlocked", &run.notes_unlocked),
    ] {
        if !notes.is_empty() {
            run.violations.push(format!("{label} tally: {notes:?}"));
        }
    }
    if run.eis_ready_after_unlock == Some(false) {
        run.violations
            .push("EIS connection not ready after the unlock".to_string());
    }

    let result = classify(&run);
    let observed = format!(
        "result={result}; blocked={:?}; failure={:?}; aborted={:?}; inconclusive={:?}; stages={}; \
         lock_after_ms={:?}; unlock_after_ms={:?}; beats_during_lock={}; capture={:?}; \
         violations={:?}; shell {:?}->{:?}",
        run.blocked,
        run.failure,
        run.aborted,
        run.inconclusive,
        run.stages.len(),
        run.lock_engaged_after_ms,
        run.unlock_observed_after_ms,
        run.beats_during_lock,
        run.capture,
        run.violations,
        run.shell_pid_before,
        run.shell_pid_after
    );
    println!("{observed}");
    for stage in &run.stages {
        println!(
            "  {:>6} ms {:<32} {}",
            stage.at_ms, stage.name, stage.outcome
        );
    }

    let report = ExperimentReport {
        experiment: "Experiment 11 — Lock semantics (FEAS-A)".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1, single built-in \
                      display"
            .to_string(),
        objective: "Show that a RemoteDesktop/EIS sender and a ScreenCast monitor capture attached \
                    before a session lock survive the lock and the operator's unlock, and that input \
                    injected while locked reaches no window behind the lock screen (that it reaches the \
                    lock screen itself is only the operator's observation, recorded separately)."
            .to_string(),
        hypothesis: "The EIS connection and capture stay attached through lock and unlock; while locked \
                     the observer page sees no injected event; after the unlock the same connection \
                     delivers Shift, `a` and Left."
            .to_string(),
        procedure: format!(
            "Preflight (unlocked session, remote desktop service inactive); observer page focused and \
             fullscreen ({} s settle); CreateSession, Start, ConnectToEIS, bind; ScreenCast monitor \
             capture{}; pre-lock Shift tap; `loginctl lock-session`; while locked: pointer +5/-5, Shift, \
             three `x`, six seconds hold, Esc; operator unlocks with their own password; post-unlock \
             Shift, `a`, Left; capture windows before, during and after.",
            args.settle_secs,
            if args.skip_capture { " skipped" } else { " attached" }
        ),
        expected: "Lock observed by GetActive and LockedHint agreeing (the ScreenSaver owner is not mapped \
                   to the login1 session on this host, so this is not provenance-verified); page \
                   heartbeat continues while locked; the page tally while locked has no key, button, \
                   pointer or wheel event (keyboard silence is weak evidence because the page has no \
                   keyboard focus while locked; pointer and wheel are the witnesses); the post-unlock \
                   tally is exactly Shift, A and Left, plain; the capture delivers frames before the \
                   lock and after the unlock (a quiet locked screen only makes that window \
                   inconclusive); Shell PID unchanged."
            .to_string(),
        observed,
        evidence: vec![
            "findings.json (this directory), including the page's tally at three points".to_string(),
        ],
        result,
        failure: run.failure.clone().or_else(|| run.blocked.clone()),
        root_cause: None,
        security_impact: Some(
            "The live session is locked once; only Shift, `x`, Esc, `a`, Left and a net-zero 5 px pointer \
             move are injected; no password is ever typed or read by the program. PASS here does not \
             promote FEAS-A by itself."
                .to_string(),
        ),
        recommended_action: None,
        follow_up: Some(
            "FEAS-A decision and Architecture Review #1 are separate evidence reviews.".to_string(),
        ),
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

    fn key(kind: &str, code: &str) -> Value {
        json!({"type": kind, "code": code, "repeat": false, "shift": false})
    }

    fn tally(keys: Vec<Value>) -> Value {
        json!({
            "keys": keys,
            "buttons": [],
            "pointer": {"moves": 0, "positions": []},
            "wheel": {"events": 0, "deltaY": 0, "scrollY": 0},
            "untrusted": 0,
        })
    }

    fn tap(code: &str) -> Vec<Value> {
        vec![key("down", code), key("up", code)]
    }

    #[test]
    fn lock_requires_the_operator_flag_and_bounded_waits() {
        assert!(require_operator(&Args::parse_from(["exp11"])).is_err());
        assert!(require_operator(&Args::parse_from(["exp11", "--operator-present"])).is_ok());
        assert!(Args::try_parse_from(["exp11", "--unlock-wait-secs", "5"]).is_err());
        assert!(Args::try_parse_from(["exp11", "--settle-secs", "1"]).is_err());
    }

    #[test]
    fn pre_lock_tally_is_exactly_one_shift_tap() {
        assert!(judge_pre(&tally(tap("ShiftLeft"))).is_empty());
        assert!(!judge_pre(&tally(vec![])).is_empty());
        let mut extra = tap("ShiftLeft");
        extra.extend(tap("KeyX"));
        assert!(!judge_pre(&tally(extra)).is_empty());
    }

    #[test]
    fn nothing_may_reach_the_page_while_locked() {
        assert!(judge_locked(&locked_counts(&tally(vec![]))).is_empty());
        assert!(!judge_locked(&locked_counts(&tally(tap("KeyX")))).is_empty());
        let mut moved = tally(vec![]);
        moved["pointer"]["moves"] = json!(1);
        assert!(!judge_locked(&locked_counts(&moved)).is_empty());
        let mut scrolled = tally(vec![]);
        scrolled["wheel"]["events"] = json!(1);
        assert!(!judge_locked(&locked_counts(&scrolled)).is_empty());
        let mut clicked = tally(vec![]);
        clicked["buttons"] = json!([{"type": "click", "button": 0}]);
        assert!(!judge_locked(&locked_counts(&clicked)).is_empty());
        // A malformed tally never passes as silence.
        assert!(!judge_locked(&locked_counts(&json!({}))).is_empty());
    }

    #[test]
    fn the_locked_record_keeps_counts_and_no_key_codes() {
        let leaked = locked_counts(&tally(tap("KeyP")));
        let text = serde_json::to_string(&leaked).expect("serialises");
        assert!(!text.contains("KeyP"), "{text}");
        assert_eq!(leaked.keys, 2);
    }

    #[test]
    fn the_unlocked_tally_is_exactly_shift_a_left_plain() {
        let mut good = tap("ShiftLeft");
        good.extend(tap("KeyA"));
        good.extend(tap("ArrowLeft"));
        assert!(judge_unlocked(&tally(good.clone())).is_empty());

        let mut missing = tap("ShiftLeft");
        missing.extend(tap("KeyA"));
        assert!(!judge_unlocked(&tally(missing)).is_empty());

        let mut extra = good.clone();
        extra.extend(tap("KeyX"));
        assert!(!judge_unlocked(&tally(extra)).is_empty());

        let mut repeated = good.clone();
        repeated[2]["repeat"] = json!(true);
        assert!(!judge_unlocked(&tally(repeated)).is_empty());

        let mut synthetic = tally(good);
        synthetic["untrusted"] = json!(1);
        assert!(!judge_unlocked(&synthetic).is_empty());
    }

    #[test]
    fn only_a_complete_clean_run_passes() {
        let t0 = Instant::now();
        let mut run = Run::default();
        assert_eq!(classify(&run), ExperimentResult::Partial);
        run.tally_pre = Some(json!({}));
        run.tally_locked = Some(locked_counts(&tally(vec![])));
        run.tally_unlocked = Some(json!({}));
        run.lock_engaged_after_ms = Some(900);
        run.unlock_observed_after_ms = Some(30_000);
        assert_eq!(classify(&run), ExperimentResult::Pass);

        run.inconclusive.push("page silent".to_string());
        assert_eq!(classify(&run), ExperimentResult::Partial);
        run.inconclusive.clear();
        run.aborted = Some("unlock timed out".to_string());
        assert_eq!(classify(&run), ExperimentResult::Partial);
        run.aborted = None;

        run.record(
            "locked_key",
            Err(BlackroomError::new(ErrorCode::MutterUnavailable, "t")),
            t0,
        );
        assert_eq!(classify(&run), ExperimentResult::Fail);
        run.blocked = Some("ConnectToEIS".to_string());
        assert_eq!(classify(&run), ExperimentResult::Blocked);
    }

    #[test]
    fn lock_snapshots_require_both_signals_to_agree() {
        let snap = |active, hint| LockSnap {
            screen_saver_active: active,
            logind_locked_hint: hint,
            owner_session_verified: false,
        };
        assert!(snap(true, true).locked() && !snap(true, true).unlocked());
        assert!(snap(false, false).unlocked() && !snap(false, false).locked());
        for mixed in [snap(true, false), snap(false, true)] {
            assert!(!mixed.locked() && !mixed.unlocked());
        }
    }
}
