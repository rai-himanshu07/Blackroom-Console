//! `RemoteConsole`: start, stop and drive one isolated remote session. The flow is lifted from
//! `exp06 --integrated-probe`: RemoteDesktop/EIS, a virtual monitor with a video consumer, the
//! panel isolated, the physical input grab; Stop reverses it keeping the virtual monitor until the
//! ScreenCast session is stopped (Mutter 50.1 crashes otherwise, exp13), locks the session and only
//! then releases the input grab.
//!
//! All GNOME objects live on one actor thread, so the handle is a cheap, thread-safe `Clone`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use blackroom_core::error::BlackroomError;
use blackroom_gnome::backend::SessionInfo;
use blackroom_gnome::mutter::display_config::{self, DisplayBackup};
use blackroom_gnome::mutter::lock;
use blackroom_gnome::mutter::remote_desktop::{SelectionEvent, listen_selection_events};
use blackroom_gnome::mutter::screencast::ScreenCastSession;
use blackroom_gnome::mutter::session::discover_session;
use blackroom_gnome::mutter::video::{JpegSlot, VideoOptions, VideoTuning, stream_jpeg_tuned};
use remote_emergency_client::client::{Client, Outcome};
use serde::{Deserialize, Serialize};
use zbus::blocking::Connection;

use crate::clipboard::{self, ClipboardError};
use crate::display::{self, Watchdog};
use crate::eis_support::{Authority, DeviceSeen, Remote, open_remote_with};
use crate::webrtc::{InputSink, WebRtcSession, pick_h264};

const TICK: Duration = Duration::from_millis(25);
const MAINTENANCE_EVERY: Duration = Duration::from_millis(100);
/// The daemon releases the grab by itself if the lease lapses; renewed well inside it.
const GRAB_LEASE_MS: u64 = 10_000;
const LEASE_EVERY: Duration = Duration::from_secs(2);
const DAEMON_REPLY: Duration = Duration::from_secs(5);
const DAEMON_ISOLATE_REPLY: Duration = Duration::from_secs(25);
const WATCHDOG_SECONDS: u64 = 60;
const WATCHDOG_EVERY: Duration = Duration::from_secs(20);
const SETTLE: Duration = Duration::from_secs(10);
const SESSION_TTL: Duration = Duration::from_secs(24 * 3600);
/// Same cap as the `POST /input` body.
const MAX_INPUT_MESSAGE: usize = 64 * 1024;
const HEADLESS_SESSION: &str = "headless";
/// GNOME refuses remote sessions on a locked screen unless the Blackroom extension lifts that.
const LOCKED_HINT: &str = " (the screen is locked: enable the blackroom-locked-remote extension, see docs/ops/README.md, or unlock locally)";

#[derive(Debug, Clone)]
pub struct ConsoleConfig {
    /// The `remote-emergencyd --enable-grabs` control socket; `None` only with `headless`.
    pub grab_socket: Option<PathBuf>,
    /// Private directory for `backup.json`.
    pub state_dir: PathBuf,
    /// Throwaway `--headless` Shell on a private bus: no grab, watchdog or lock, any single output.
    pub headless: bool,
    /// Starting quality level; the browser can change it while the session runs.
    pub quality: Quality,
    /// No browser heartbeat for this long runs Stop (the panel is blank, so a lost client must restore it).
    pub heartbeat_timeout: Duration,
    /// `exp07_restore`, armed as the dead-man restore.
    pub restore_bin: PathBuf,
}

/// One knob for both transports: JPEG quality and frame cap for MJPEG, bitrate for WebRTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Low,
    Medium,
    High,
}

impl Quality {
    pub fn jpeg_quality(self) -> u8 {
        match self {
            Self::Low => 40,
            Self::Medium => 70,
            Self::High => 85,
        }
    }

    pub fn max_fps(self) -> u32 {
        match self {
            Self::Low => 15,
            Self::Medium | Self::High => 30,
        }
    }

    pub fn bitrate_kbps(self) -> u32 {
        match self {
            Self::Low => 2_500,
            Self::Medium => 6_000,
            Self::High => 12_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Starting,
    Running,
    Stopping,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct StopReport {
    pub reason: String,
    pub grab_released: Option<bool>,
    /// Releases the daemon pushed on its own (chord, lease, node loss) before ours.
    pub released_early: Vec<String>,
    pub virtual_gone: Option<bool>,
    pub topology_restored: Option<bool>,
    pub locked: Option<bool>,
    pub errors: Vec<String>,
}

/// What the console process itself uses, from `/proc/self`; a rising line over many sessions is a leak.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Resources {
    pub rss_kb: u64,
    pub open_fds: u64,
    pub threads: u64,
    pub uptime_secs: u64,
    pub sessions_started: u64,
    pub sessions_stopped: u64,
}

fn proc_status_value(text: &str, key: &str) -> u64 {
    text.lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse().ok())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub phase: Phase,
    pub width: u32,
    pub height: u32,
    pub notes: Vec<String>,
    pub input_accepted: u64,
    pub input_refused: u64,
    pub quality: Quality,
    pub webrtc_encoder: Option<&'static str>,
    pub webrtc_error: Option<String>,
    pub webrtc_frames: Option<u64>,
    pub last_stop: Option<StopReport>,
    /// Whether the page may offer the clipboard buttons.
    pub clipboard: bool,
    pub resources: Resources,
}

/// One input event from the browser. Pointer positions are fractions of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum InputEvent {
    Key { code: u32, down: bool },
    Button { code: u32, down: bool },
    Move { x: f32, y: f32 },
    Scroll { dx: f32, dy: f32 },
}

const KEY_MAX: u32 = 0x2ff;
const BTN_FIRST: u32 = 0x110;
const BTN_LAST: u32 = 0x117;
/// Power, sleep, suspend and wake keys would end the session from the browser.
const BLOCKED_KEYS: [u32; 4] = [116, 142, 143, 205];
const SCROLL_LIMIT: f32 = 1000.0;

impl InputEvent {
    pub fn validate(self) -> Result<Self, &'static str> {
        match self {
            Self::Key { code, .. } if code == 0 || code > KEY_MAX => Err("key code out of range"),
            Self::Key { code, .. } if BLOCKED_KEYS.contains(&code) => Err("key not allowed"),
            Self::Button { code, .. } if !(BTN_FIRST..=BTN_LAST).contains(&code) => {
                Err("button code out of range")
            }
            Self::Move { x, y } if !x.is_finite() || !y.is_finite() => Err("position not finite"),
            Self::Scroll { dx, dy }
                if !dx.is_finite()
                    || !dy.is_finite()
                    || dx.abs() > SCROLL_LIMIT
                    || dy.abs() > SCROLL_LIMIT =>
            {
                Err("scroll out of range")
            }
            other => Ok(other),
        }
    }
}

struct Shared {
    phase: Mutex<Phase>,
    slot: Mutex<Option<Arc<JpegSlot>>>,
    last_stop: Mutex<Option<StopReport>>,
    /// Set when the start-up recovery of an unclean session failed; Start is refused meanwhile.
    recovery_pending: Mutex<Option<String>>,
    /// Emergency chords seen; a TOTP session opened before one is no longer valid.
    emergencies: AtomicU64,
    beat: Mutex<Instant>,
    notes: Mutex<Vec<String>>,
    size: Mutex<(u32, u32)>,
    quality: Mutex<Quality>,
    tuning: Mutex<Option<Arc<VideoTuning>>>,
    webrtc: Mutex<Option<WebRtcSession>>,
    input_accepted: AtomicU64,
    input_refused: AtomicU64,
    clipboard_enabled: AtomicBool,
    /// Mutter accepted the clipboard for the running session.
    clipboard_live: AtomicBool,
    started_at: Instant,
    sessions_started: AtomicU64,
    sessions_stopped: AtomicU64,
    last_clipboard: Mutex<Option<Instant>>,
}

fn lock_ok<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

enum Command {
    Start(Sender<Result<(), String>>),
    Stop(Sender<StopReport>),
    Input(Vec<InputEvent>),
    ClipboardSet(String, Sender<Result<(), ClipboardError>>),
    ClipboardGet(Sender<Result<String, ClipboardError>>),
}

#[derive(Clone)]
pub struct RemoteConsole {
    commands: Sender<Command>,
    shared: Arc<Shared>,
}

impl RemoteConsole {
    pub fn spawn(config: ConsoleConfig) -> Self {
        let (commands, receiver) = mpsc::channel();
        let shared = Arc::new(Shared {
            phase: Mutex::new(Phase::Idle),
            slot: Mutex::new(None),
            last_stop: Mutex::new(None),
            recovery_pending: Mutex::new(None),
            emergencies: AtomicU64::new(0),
            beat: Mutex::new(Instant::now()),
            notes: Mutex::new(Vec::new()),
            size: Mutex::new((0, 0)),
            quality: Mutex::new(config.quality),
            tuning: Mutex::new(None),
            webrtc: Mutex::new(None),
            input_accepted: AtomicU64::new(0),
            input_refused: AtomicU64::new(0),
            clipboard_enabled: AtomicBool::new(false),
            clipboard_live: AtomicBool::new(false),
            started_at: Instant::now(),
            sessions_started: AtomicU64::new(0),
            sessions_stopped: AtomicU64::new(0),
            last_clipboard: Mutex::new(None),
        });
        let actor_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("console-actor".into())
            .spawn(move || run_actor(&receiver, &actor_shared, &Arc::new(config)))
            .expect("spawn the console actor thread");
        Self { commands, shared }
    }

    /// Blocks until the session runs or setup failed (everything already undone).
    pub fn start(&self) -> Result<Status, String> {
        {
            let mut phase = lock_ok(&self.shared.phase);
            if *phase != Phase::Idle {
                return Err(format!("not idle ({:?})", *phase));
            }
            *phase = Phase::Starting;
        }
        let (reply, answer) = mpsc::channel();
        self.commands
            .send(Command::Start(reply))
            .map_err(|_| "console actor is gone".to_string())?;
        answer
            .recv()
            .map_err(|_| "console actor is gone".to_string())??;
        Ok(self.status())
    }

    /// Blocks until the display is restored and the session locked.
    pub fn stop(&self) -> StopReport {
        let (reply, answer) = mpsc::channel();
        if self.commands.send(Command::Stop(reply)).is_err() {
            return StopReport {
                reason: "console actor is gone".into(),
                ..StopReport::default()
            };
        }
        answer.recv().unwrap_or_else(|_| StopReport {
            reason: "console actor is gone".into(),
            ..StopReport::default()
        })
    }

    /// Queues events; counts as a heartbeat.
    pub fn input(&self, events: Vec<InputEvent>) -> Result<(), String> {
        let events = events
            .into_iter()
            .map(|event| event.validate().map_err(str::to_string))
            .collect::<Result<Vec<_>, _>>()?;
        if *lock_ok(&self.shared.phase) != Phase::Running {
            return Err("not running".into());
        }
        self.beat();
        if events.is_empty() {
            return Ok(());
        }
        self.commands
            .send(Command::Input(events))
            .map_err(|_| "console actor is gone".to_string())
    }

    /// One data-channel message: the same JSON array as `POST /input`; bad messages are dropped.
    fn input_json(&self, text: &str) {
        if text.len() > MAX_INPUT_MESSAGE {
            return;
        }
        match serde_json::from_str::<Vec<InputEvent>>(text) {
            Ok(events) => {
                if let Err(error) = self.input(events) {
                    tracing::debug!(error, "data-channel input refused");
                }
            }
            Err(_) => tracing::debug!("data-channel input malformed"),
        }
    }

    /// Turns the clipboard on for sessions started afterwards (off by default).
    pub fn set_clipboard_enabled(&self, enabled: bool) {
        self.shared
            .clipboard_enabled
            .store(enabled, Ordering::Relaxed);
    }

    fn clipboard_gate(&self) -> Result<(), ClipboardError> {
        if !self.shared.clipboard_enabled.load(Ordering::Relaxed) {
            return Err(ClipboardError::Disabled);
        }
        if *lock_ok(&self.shared.phase) != Phase::Running {
            return Err(ClipboardError::NotRunning);
        }
        if !self.shared.clipboard_live.load(Ordering::Relaxed) {
            return Err(ClipboardError::Failed(
                "Mutter did not enable the clipboard for this session".into(),
            ));
        }
        let mut last = lock_ok(&self.shared.last_clipboard);
        let now = Instant::now();
        if last.is_some_and(|at| now.duration_since(at) < clipboard::MIN_INTERVAL) {
            return Err(ClipboardError::TooFast);
        }
        *last = Some(now);
        drop(last);
        self.beat();
        Ok(())
    }

    /// Puts `text` on the laptop clipboard; blocks until Mutter took it.
    pub fn clipboard_set(&self, text: String) -> Result<(), ClipboardError> {
        clipboard::validate(&text)?;
        self.clipboard_gate()?;
        let (reply, answer) = mpsc::channel();
        self.commands
            .send(Command::ClipboardSet(text, reply))
            .map_err(|_| ClipboardError::Failed("console actor is gone".into()))?;
        answer
            .recv()
            .map_err(|_| ClipboardError::Failed("console actor is gone".into()))?
    }

    /// The text currently on the laptop clipboard.
    pub fn clipboard_get(&self) -> Result<String, ClipboardError> {
        self.clipboard_gate()?;
        let (reply, answer) = mpsc::channel();
        self.commands
            .send(Command::ClipboardGet(reply))
            .map_err(|_| ClipboardError::Failed("console actor is gone".into()))?;
        answer
            .recv()
            .map_err(|_| ClipboardError::Failed("console actor is gone".into()))?
    }

    /// Emergency chords so far; sessions opened before a change are void.
    pub fn emergency_count(&self) -> u64 {
        self.shared.emergencies.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn note_emergency(&self) {
        self.shared.emergencies.fetch_add(1, Ordering::Relaxed);
    }

    pub fn beat(&self) {
        *lock_ok(&self.shared.beat) = Instant::now();
    }

    /// The newest JPEG frames of the running session.
    pub fn video(&self) -> Option<Arc<JpegSlot>> {
        lock_ok(&self.shared.slot).clone()
    }

    /// Applies at once to a running session and becomes the level of the next one.
    pub fn set_quality(&self, quality: Quality) {
        *lock_ok(&self.shared.quality) = quality;
        if let Some(tuning) = lock_ok(&self.shared.tuning).as_ref() {
            tuning.set(quality.jpeg_quality(), quality.max_fps());
        }
        if let Some(session) = lock_ok(&self.shared.webrtc).as_ref() {
            session.set_bitrate(quality.bitrate_kbps());
        }
    }

    /// Answers a browser's WebRTC offer with the live desktop as H.264; replaces any earlier peer.
    /// Blocks while ICE gathers, so call it from a blocking context.
    pub fn webrtc_answer(&self, offer_sdp: &str) -> Result<String, String> {
        if *lock_ok(&self.shared.phase) != Phase::Running {
            return Err("not running".into());
        }
        let tuning = lock_ok(&self.shared.tuning)
            .clone()
            .ok_or("no video stream yet")?;
        if let Some(old) = lock_ok(&self.shared.webrtc).take() {
            old.close();
        }
        let quality = *lock_ok(&self.shared.quality);
        let h264 = pick_h264(offer_sdp).ok_or("the browser offers no usable H.264")?;
        let console = self.clone();
        let input: InputSink = Arc::new(move |text| console.input_json(text));
        let session = WebRtcSession::start(&tuning, quality.bitrate_kbps(), &h264, input)?;
        let answer = session.answer(offer_sdp);
        let mut slot = lock_ok(&self.shared.webrtc);
        // Stop may have run while negotiating: its teardown has already passed.
        if answer.is_err() || *lock_ok(&self.shared.phase) != Phase::Running {
            session.close();
            return answer.and(Err("stopped while negotiating".into()));
        }
        *slot = Some(session);
        answer
    }

    fn resources(&self) -> Resources {
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        Resources {
            rss_kb: proc_status_value(&status, "VmRSS:"),
            open_fds: std::fs::read_dir("/proc/self/fd")
                .map_or(0, |entries| entries.count() as u64),
            threads: proc_status_value(&status, "Threads:"),
            uptime_secs: self.shared.started_at.elapsed().as_secs(),
            sessions_started: self.shared.sessions_started.load(Ordering::Relaxed),
            sessions_stopped: self.shared.sessions_stopped.load(Ordering::Relaxed),
        }
    }

    pub fn status(&self) -> Status {
        let phase = *lock_ok(&self.shared.phase);
        let (width, height) = *lock_ok(&self.shared.size);
        let (webrtc_encoder, webrtc_error, webrtc_frames) =
            match lock_ok(&self.shared.webrtc).as_ref() {
                Some(session) => (
                    Some(session.encoder().label()),
                    session.failure(),
                    Some(session.frames()),
                ),
                None => (None, None, None),
            };
        Status {
            phase,
            width,
            height,
            notes: lock_ok(&self.shared.notes).clone(),
            input_accepted: self.shared.input_accepted.load(Ordering::Relaxed),
            input_refused: self.shared.input_refused.load(Ordering::Relaxed),
            quality: *lock_ok(&self.shared.quality),
            webrtc_encoder,
            webrtc_error,
            webrtc_frames,
            last_stop: lock_ok(&self.shared.last_stop).clone(),
            clipboard: if phase == Phase::Running {
                self.shared.clipboard_live.load(Ordering::Relaxed)
            } else {
                self.shared.clipboard_enabled.load(Ordering::Relaxed)
            },
            resources: self.resources(),
        }
    }
}

struct VideoHandle {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<Result<(), BlackroomError>>,
}

struct BackupCtx {
    canonical: DisplayBackup,
    shell_pid: u32,
}

struct Active<'c> {
    conn: &'c Connection,
    config: Arc<ConsoleConfig>,
    shared: Arc<Shared>,
    authority: Authority,
    session: SessionInfo,
    backup: Option<BackupCtx>,
    remote: Option<Remote<'c>>,
    sc: Option<ScreenCastSession<'c>>,
    video: Option<VideoHandle>,
    virtual_connector: Option<String>,
    watchdog: Option<Watchdog>,
    isolated: bool,
    grab: Option<Client>,
    held_keys: BTreeSet<u32>,
    held_buttons: BTreeSet<u32>,
    size: (u32, u32),
    last_maintenance: Instant,
    last_lease: Instant,
    last_watchdog: Instant,
    released_early: Vec<String>,
    /// Dropping it ends the logind inhibitor that stops suspend and idle sleep mid-session.
    sleep_inhibitor: Option<display::SleepLock>,
    /// Text the browser put on the laptop clipboard, served to every paste; dropped with the session.
    clipboard_text: Option<String>,
    /// Whether this session currently owns the laptop clipboard.
    clipboard_owned: bool,
}

/// Restores the display and locks the screen when a previous console died mid-session.
/// Returns the refusal text when that failed.
fn recover_on_start(conn: &Connection, config: &ConsoleConfig) -> Option<String> {
    let outcome = display::recover(
        &config.state_dir,
        |pid| {
            std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .is_ok_and(|comm| comm.trim() == "blackroom-conso")
        },
        |session_id, shell_pid| {
            discover_session().is_ok_and(|s| s.session_id == session_id)
                && display::gnome_shell_pid(conn).is_ok_and(|pid| pid == shell_pid)
        },
        |backup| {
            std::process::Command::new(&config.restore_bin)
                .arg("--backup")
                .arg(backup)
                .args(["--keep-live-virtual", "--lock-after"])
                .stdin(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        },
    );
    tracing::info!(?outcome, "start-up recovery check");
    match outcome {
        display::Recovery::Pending(text) => Some(text),
        _ => None,
    }
}

fn run_actor(receiver: &Receiver<Command>, shared: &Arc<Shared>, config: &Arc<ConsoleConfig>) {
    let conn = Connection::session();
    if let Ok(conn) = &conn
        && !config.headless
        && let Some(pending) = recover_on_start(conn, config)
    {
        *lock_ok(&shared.recovery_pending) = Some(pending);
    }
    let mut active: Option<Active<'_>> = None;
    let mut selection: Option<Receiver<SelectionEvent>> = None;
    loop {
        match receiver.recv_timeout(TICK) {
            Ok(Command::Start(reply)) => {
                if selection.is_none()
                    && shared.clipboard_enabled.load(Ordering::Relaxed)
                    && let Ok(conn) = &conn
                {
                    selection = listen_selection_events(conn).ok();
                }
                let result = match (&conn, active.is_some()) {
                    (_, true) => Err("already running".to_string()),
                    (Err(error), _) => Err(format!("session bus: {error}")),
                    (Ok(conn), false) => match begin(conn, config, shared) {
                        Ok(started) => {
                            active = Some(started);
                            Ok(())
                        }
                        Err(message) => Err(message),
                    },
                };
                if result.is_err() {
                    *lock_ok(&shared.phase) = Phase::Idle;
                }
                let _ = reply.send(result);
            }
            Ok(Command::Stop(reply)) => {
                let report = match active.take() {
                    Some(running) => finish(running, "stop requested"),
                    None => StopReport {
                        reason: "not running".into(),
                        ..StopReport::default()
                    },
                };
                let _ = reply.send(report);
            }
            Ok(Command::Input(events)) => {
                if let Some(running) = active.as_mut() {
                    running.apply_input(events);
                }
            }
            Ok(Command::ClipboardSet(text, reply)) => {
                let _ = reply.send(match active.as_mut() {
                    Some(running) => running.clipboard_set(text),
                    None => Err(ClipboardError::NotRunning),
                });
            }
            Ok(Command::ClipboardGet(reply)) => {
                let _ = reply.send(match active.as_mut() {
                    Some(running) => running.clipboard_get(),
                    None => Err(ClipboardError::NotRunning),
                });
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                if let Some(running) = active.take() {
                    finish(running, "console handle dropped");
                }
                return;
            }
        }
        if let Some(events) = &selection {
            while let Ok(event) = events.try_recv() {
                if let Some(running) = active.as_mut() {
                    running.on_selection(&event);
                }
            }
        }
        if let Some(running) = active.as_mut()
            && let Some(reason) = running.maintain()
            && let Some(running) = active.take()
        {
            finish(running, &reason);
        }
    }
}

/// `last_stop.json` shows how far a Stop got even when the process dies in the middle of it.
fn persist_stop(dir: &Path, state: &str, reason: &str, step: &str, report: Option<&StopReport>) {
    let at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let body = serde_json::json!({
        "state": state, "reason": reason, "step": step, "at_unix": at_unix, "report": report,
    });
    if let Err(error) = display::write_private(dir, "last_stop.json", body.to_string().as_bytes()) {
        tracing::warn!(%error, "could not write last_stop.json");
    }
}

fn finish(active: Active<'_>, reason: &str) -> StopReport {
    let shared = Arc::clone(&active.shared);
    let state_dir = active.config.state_dir.clone();
    *lock_ok(&shared.phase) = Phase::Stopping;
    tracing::info!(reason, "stopping the remote console");
    persist_stop(&state_dir, "stopping", reason, "begin", None);
    let report = active.teardown(reason);
    tracing::info!(?report, "remote console stopped");
    persist_stop(&state_dir, "stopped", reason, "done", Some(&report));
    if report.released_early.iter().any(|why| why == "chord") {
        shared.emergencies.fetch_add(1, Ordering::Relaxed);
    }
    *lock_ok(&shared.slot) = None;
    *lock_ok(&shared.tuning) = None;
    shared.clipboard_live.store(false, Ordering::Relaxed);
    shared.sessions_stopped.fetch_add(1, Ordering::Relaxed);
    *lock_ok(&shared.last_stop) = Some(report.clone());
    *lock_ok(&shared.phase) = Phase::Idle;
    report
}

fn begin<'c>(
    conn: &'c Connection,
    config: &Arc<ConsoleConfig>,
    shared: &Arc<Shared>,
) -> Result<Active<'c>, String> {
    if let Some(pending) = lock_ok(&shared.recovery_pending).clone() {
        return Err(format!("recovery pending: {pending}"));
    }
    // Counts every attempt: a failed start also runs `finish`, so started and stopped meet again when idle.
    shared.sessions_started.fetch_add(1, Ordering::Relaxed);
    lock_ok(&shared.notes).clear();
    shared.input_accepted.store(0, Ordering::Relaxed);
    shared.input_refused.store(0, Ordering::Relaxed);
    *lock_ok(&shared.last_stop) = None;
    let mut active = Active {
        conn,
        config: Arc::clone(config),
        shared: Arc::clone(shared),
        authority: Authority::new(SESSION_TTL),
        session: SessionInfo {
            session_id: String::new(),
            uid: 0,
            seat: String::new(),
            is_wayland: true,
            active: true,
        },
        backup: None,
        remote: None,
        sc: None,
        video: None,
        virtual_connector: None,
        watchdog: None,
        isolated: false,
        grab: None,
        held_keys: BTreeSet::new(),
        held_buttons: BTreeSet::new(),
        size: (0, 0),
        last_maintenance: Instant::now(),
        last_lease: Instant::now(),
        last_watchdog: Instant::now(),
        released_early: Vec::new(),
        sleep_inhibitor: None,
        clipboard_text: None,
        clipboard_owned: false,
    };
    match active.setup() {
        Ok(()) => {
            *lock_ok(&shared.beat) = Instant::now();
            *lock_ok(&shared.size) = active.size;
            *lock_ok(&shared.phase) = Phase::Running;
            Ok(active)
        }
        Err(error) => {
            let message = format!("{error:#}");
            let report = finish(active, &format!("start failed: {message}"));
            if !report.errors.is_empty() {
                return Err(format!("{message}; cleanup: {}", report.errors.join("; ")));
            }
            Err(message)
        }
    }
}

fn describe(error: impl std::fmt::Display) -> String {
    error.to_string()
}

impl<'c> Active<'c> {
    fn note(&self, text: String) {
        tracing::warn!("{text}");
        lock_ok(&self.shared.notes).push(text);
    }

    fn setup(&mut self) -> anyhow::Result<()> {
        let config = Arc::clone(&self.config);
        let locked = if config.headless {
            display::require_headless_shell(self.conn)?;
            self.session.session_id = HEADLESS_SESSION.into();
            false
        } else {
            self.preflight_live(&config)?
        };

        if !config.headless {
            match display::inhibit_sleep(self.conn) {
                Ok(fd) => self.sleep_inhibitor = Some(fd),
                Err(error) => self.note(format!("could not hold off suspend: {error:#}")),
            }
        }

        let backup = display_config::snapshot(self.conn, &self.session.session_id)
            .map_err(|e| anyhow::anyhow!("display snapshot: {e}"))?;
        let (width, height) = single_monitor(&backup, config.headless)?;
        self.size = (width, height);
        let shell_pid = display::gnome_shell_pid(self.conn)?;
        let backup_path = display::write_backup(&config.state_dir, &backup, shell_pid)?;
        tracing::info!(path = %backup_path.display(), "display backup written");
        if !config.headless {
            display::write_recovery_marker(&config.state_dir, &backup_path)?;
        }
        self.backup = Some(BackupCtx {
            canonical: backup,
            shell_pid,
        });

        let mut seen: Vec<DeviceSeen> = Vec::new();
        let remote = open_remote_with(
            self.conn,
            &self.authority,
            &mut seen,
            self.shared.clipboard_enabled.load(Ordering::Relaxed),
        )
        .map_err(|(step, detail)| {
            let hint = if locked { LOCKED_HINT } else { "" };
            anyhow::anyhow!("remote session {step}: {detail}{hint}")
        })?;
        if self.shared.clipboard_enabled.load(Ordering::Relaxed) {
            if remote.clipboard_enabled() {
                self.shared.clipboard_live.store(true, Ordering::Relaxed);
            } else {
                self.note("the laptop refused the clipboard: clipboard buttons are off".into());
            }
        }
        self.remote = Some(remote);
        if locked {
            self.note("started on the lock screen: type the account password to unlock".into());
        }

        let before = display::connectors(self.conn)?;
        let sc = ScreenCastSession::create(self.conn)?;
        let stream = sc.record_virtual(i32::try_from(width)?, i32::try_from(height)?, 60.0)?;
        let node = stream.start_and_wait_for_pipewire_node(&sc)?;
        self.sc = Some(sc);
        let slot = JpegSlot::new();
        let stop = Arc::new(AtomicBool::new(false));
        let quality = *lock_ok(&self.shared.quality);
        let options = VideoOptions {
            preferred_width: i32::try_from(width)?,
            preferred_height: i32::try_from(height)?,
            quality: quality.jpeg_quality(),
            max_fps: quality.max_fps(),
        };
        let tuning = VideoTuning::new(&options);
        *lock_ok(&self.shared.tuning) = Some(Arc::clone(&tuning));
        let handle = {
            let (slot, stop) = (Arc::clone(&slot), Arc::clone(&stop));
            thread::Builder::new()
                .name("console-video".into())
                .spawn(move || stream_jpeg_tuned(node, options, &tuning, &stop, &slot))?
        };
        self.video = Some(VideoHandle { stop, handle });
        *lock_ok(&self.shared.slot) = Some(slot);
        // The connector only appears once a consumer streams.
        let connector = poll_new_connector(self.conn, &before)?;
        self.virtual_connector = Some(connector.clone());

        if !config.headless {
            let mut watchdog = Watchdog::new(config.restore_bin.clone(), backup_path)?;
            watchdog.refresh(WATCHDOG_SECONDS)?;
            self.last_watchdog = Instant::now();
            self.watchdog = Some(watchdog);
        }

        self.isolated = true;
        display_config::disable_physical_outputs(self.conn, &connector)
            .map_err(|e| anyhow::anyhow!("isolate the panel: {e}"))?;

        if let Some(socket) = &config.grab_socket
            && !config.headless
        {
            let mut client = Client::connect(socket, DAEMON_ISOLATE_REPLY)?;
            match client.isolate(GRAB_LEASE_MS)? {
                Outcome::Isolated { nodes } => tracing::info!(nodes, "physical input grabbed"),
                other => anyhow::bail!("the daemon did not grab: {other:?}"),
            }
            client.set_reply_timeout(DAEMON_REPLY)?;
            self.grab = Some(client);
        }

        // Mutter recreates the absolute pointer device when the monitor layout changes.
        let deadline = Instant::now() + Duration::from_secs(3);
        if let Some(remote) = self.remote.as_mut() {
            while remote.devices.pointer_absolute.is_none() && Instant::now() < deadline {
                remote.pump(Duration::from_millis(100));
            }
            if remote.devices.pointer_absolute.is_none() {
                self.note(
                    "no absolute pointer device: touch and mouse positioning is unavailable".into(),
                );
            }
        }
        Ok(())
    }

    /// Returns whether the screen is locked; the password is then typed remotely, never bypassed.
    fn preflight_live(&mut self, config: &ConsoleConfig) -> anyhow::Result<bool> {
        let socket = config
            .grab_socket
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("a grab socket is required unless headless"))?;
        let mut client = Client::connect(socket, Duration::from_secs(10))?;
        let status = client.status()?;
        anyhow::ensure!(
            status.latched != Some(true),
            "an emergency stop is latched: stop console.sh and start it again (the daemon restarts), then Start"
        );
        anyhow::ensure!(
            status.phase.as_deref() == Some("idle") && status.grabs_enabled == Some(true),
            "the emergency daemon is not idle with grabs enabled: {status:?}"
        );
        drop(client);
        self.session = discover_session().map_err(|e| anyhow::anyhow!("session discovery: {e}"))?;
        let observed = lock::observe(&self.session)
            .map_err(|e| anyhow::anyhow!("lock state unreadable: {e}"))?;
        anyhow::ensure!(
            !display::watchdog_pending(),
            "a console restore timer is still pending (wait for it to fire or stop it)"
        );
        Ok(observed.screen_saver_active || observed.logind_locked_hint)
    }

    fn verify_identity(&self) -> Result<(), String> {
        let backup = self.backup.as_ref().ok_or("no display backup")?;
        let session_id = if self.config.headless {
            HEADLESS_SESSION.to_string()
        } else {
            discover_session().map_err(describe)?.session_id
        };
        let shell_pid = display::gnome_shell_pid(self.conn).map_err(describe)?;
        if session_id == backup.canonical.session_id && shell_pid == backup.shell_pid {
            Ok(())
        } else {
            Err("refusing to restore: different GNOME session or Shell process".into())
        }
    }

    fn clipboard_session_path(&self) -> Option<String> {
        self.remote
            .as_ref()
            .map(|remote| remote.session().object_path().to_string())
    }

    fn clipboard_set(&mut self, text: String) -> Result<(), ClipboardError> {
        let remote = self.remote.as_ref().ok_or(ClipboardError::NotRunning)?;
        remote
            .session()
            .set_selection(&clipboard::TEXT_MIMES)
            .map_err(|error| ClipboardError::Failed(error.to_string()))?;
        self.clipboard_text = Some(text);
        self.clipboard_owned = true;
        tracing::info!("clipboard: set from the browser");
        Ok(())
    }

    fn clipboard_get(&mut self) -> Result<String, ClipboardError> {
        if self.clipboard_owned
            && let Some(text) = &self.clipboard_text
        {
            return Ok(text.clone());
        }
        let remote = self.remote.as_ref().ok_or(ClipboardError::NotRunning)?;
        let mut last = ClipboardError::NoText;
        for mime in clipboard::TEXT_MIMES {
            match remote.session().selection_read(mime) {
                Ok(fd) => match clipboard::read_with_timeout(fd, clipboard::TRANSFER_TIMEOUT) {
                    Ok(text) => {
                        tracing::info!(bytes = text.len(), "clipboard: read for the browser");
                        return Ok(text);
                    }
                    Err(error) => last = error,
                },
                Err(error) => {
                    tracing::debug!(%error, mime, "clipboard: no data in this format");
                }
            }
        }
        Err(last)
    }

    fn on_selection(&mut self, event: &SelectionEvent) {
        let Some(own) = self.clipboard_session_path() else {
            return;
        };
        match event {
            SelectionEvent::OwnerChanged {
                path,
                session_is_owner,
                ..
            } if *path == own => {
                self.clipboard_owned = *session_is_owner;
                if !*session_is_owner {
                    self.clipboard_text = None;
                }
            }
            SelectionEvent::Transfer {
                path,
                mime_type,
                serial,
            } if *path == own => {
                let Some(remote) = self.remote.as_ref() else {
                    return;
                };
                let session = remote.session();
                let served = match (
                    &self.clipboard_text,
                    clipboard::TEXT_MIMES.contains(&mime_type.as_str()),
                ) {
                    (Some(text), true) => session.selection_write(*serial).is_ok_and(|fd| {
                        clipboard::write_with_timeout(
                            fd,
                            text.clone().into_bytes(),
                            clipboard::TRANSFER_TIMEOUT,
                        )
                    }),
                    _ => false,
                };
                let _ = session.selection_write_done(*serial, served);
                tracing::info!(served, "clipboard: paste on the laptop answered");
            }
            _ => {}
        }
    }

    fn apply_input(&mut self, events: Vec<InputEvent>) {
        let mut pending_move = None;
        for event in events {
            if let InputEvent::Move { x, y } = event {
                pending_move = Some((x, y));
                continue;
            }
            self.flush_move(&mut pending_move);
            self.apply_one(event);
        }
        self.flush_move(&mut pending_move);
    }

    fn flush_move(&mut self, pending: &mut Option<(f32, f32)>) {
        if let Some((x, y)) = pending.take() {
            self.apply_one(InputEvent::Move { x, y });
        }
    }

    fn apply_one(&mut self, event: InputEvent) {
        let (width, height) = (self.size.0 as f32, self.size.1 as f32);
        let Some(remote) = self.remote.as_mut() else {
            return;
        };
        let result = match event {
            InputEvent::Key { code, down } => {
                let sent = remote.key(&self.authority, code, down);
                if sent.is_ok() {
                    if down {
                        self.held_keys.insert(code);
                    } else {
                        self.held_keys.remove(&code);
                    }
                }
                sent
            }
            InputEvent::Button { code, down } => {
                let sent = remote.button(&self.authority, code, down);
                if sent.is_ok() {
                    if down {
                        self.held_buttons.insert(code);
                    } else {
                        self.held_buttons.remove(&code);
                    }
                }
                sent
            }
            InputEvent::Move { x, y } => remote.pointer_absolute(
                &self.authority,
                x.clamp(0.0, 1.0) * (width - 1.0).max(0.0),
                y.clamp(0.0, 1.0) * (height - 1.0).max(0.0),
            ),
            InputEvent::Scroll { dx, dy } => remote.scroll(&self.authority, dx, dy),
        };
        if let Err(error) = result {
            self.shared.input_refused.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(%error, ?event, "input event refused");
        } else {
            self.shared.input_accepted.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Housekeeping between commands; returns the reason Stop must run.
    fn maintain(&mut self) -> Option<String> {
        if self.last_maintenance.elapsed() < MAINTENANCE_EVERY {
            return None;
        }
        self.last_maintenance = Instant::now();
        if let Some(remote) = self.remote.as_mut() {
            if let Some(error) = remote.pump(Duration::from_millis(1)) {
                return Some(format!("input connection lost: {error}"));
            }
            if !remote.eis.is_ready() {
                return Some("input connection lost".into());
            }
        }
        if self.video.as_ref().is_some_and(|v| v.handle.is_finished()) {
            return Some("video stream ended".into());
        }
        if lock_ok(&self.shared.beat).elapsed() > self.config.heartbeat_timeout {
            return Some("browser heartbeat lost".into());
        }
        if self.last_lease.elapsed() >= LEASE_EVERY
            && let Some(client) = self.grab.as_mut()
        {
            self.last_lease = Instant::now();
            let renewed = client.renew();
            let released = client.take_released();
            if !released.is_empty() {
                self.released_early.clone_from(&released);
                return Some(format!(
                    "input grab released by the daemon: {}",
                    released.join(",")
                ));
            }
            if !matches!(renewed, Ok(true)) {
                return Some("input grab lease could not be renewed".into());
            }
        }
        if self.last_watchdog.elapsed() >= WATCHDOG_EVERY
            && let Some(watchdog) = self.watchdog.as_mut()
        {
            self.last_watchdog = Instant::now();
            if let Err(error) = watchdog.refresh(WATCHDOG_SECONDS) {
                return Some(format!("restore watchdog refresh failed: {error}"));
            }
        }
        None
    }

    /// Undoes whatever `setup` and the session built, in the order the integrated probe proved.
    fn teardown(mut self, reason: &str) -> StopReport {
        let mut report = StopReport {
            reason: reason.into(),
            ..StopReport::default()
        };

        // The grab is held until after the lock so local input never reaches an unlocked desktop.
        let state_dir = self.config.state_dir.clone();
        let stage = |step: &str| persist_stop(&state_dir, "stopping", reason, step, None);
        self.renew_grab();

        if let Some(remote) = self.remote.as_mut() {
            for code in std::mem::take(&mut self.held_keys) {
                let _ = remote.key(&self.authority, code, false);
            }
            for code in std::mem::take(&mut self.held_buttons) {
                let _ = remote.button(&self.authority, code, false);
            }
        }
        self.remote = None;
        stage("input closed");

        if self.isolated
            && let (Some(backup), Some(connector)) = (&self.backup, &self.virtual_connector)
        {
            let restored = self.verify_identity().and_then(|()| {
                display_config::restore_physical_outputs_keeping_virtual(
                    self.conn,
                    &backup.canonical,
                    connector,
                )
                .map_err(describe)
            });
            if let Err(error) = restored {
                report
                    .errors
                    .push(format!("restore keeping the virtual monitor: {error}"));
            }
        }

        if let Some(session) = lock_ok(&self.shared.webrtc).take() {
            session.close();
        }

        if let Some(video) = self.video.take() {
            video.stop.store(true, Ordering::Relaxed);
            match video.handle.join() {
                Ok(Err(error)) => report.errors.push(format!("video: {error}")),
                Err(_) => report.errors.push("video thread panicked".into()),
                Ok(Ok(())) => {}
            }
        }

        if let Some(mut sc) = self.sc.take()
            && let Err(error) = sc.stop()
        {
            report.errors.push(format!("ScreenCast stop: {error}"));
        }

        if let Some(connector) = &self.virtual_connector {
            report.virtual_gone = Some(wait_connector_gone(self.conn, connector));
        }
        stage("screencast stopped");

        if self.isolated
            && let Some(backup) = &self.backup
        {
            let verified = |conn: &Connection| {
                display_config::verify_restored(conn, &backup.canonical).unwrap_or(false)
            };
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut restored = verified(self.conn);
            while !restored && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(150));
                restored = verified(self.conn);
            }
            if !restored && report.virtual_gone != Some(false) {
                let repaired = self.verify_identity().and_then(|()| {
                    display_config::restore_physical_outputs(self.conn, &backup.canonical)
                        .map_err(describe)
                });
                match repaired {
                    Ok(()) => {
                        thread::sleep(Duration::from_millis(500));
                        restored = verified(self.conn);
                    }
                    Err(error) => report.errors.push(format!("repair restore: {error}")),
                }
            }
            report.topology_restored = Some(restored);
        }
        stage("topology verified");

        if report.topology_restored != Some(false)
            && let Some(mut watchdog) = self.watchdog.take()
        {
            watchdog.disarm();
        }
        stage("watchdog handled");

        self.renew_grab();
        if self.isolated && !self.config.headless {
            report.locked = Some(display::lock_session(&self.session.session_id));
        }
        stage("lock requested");

        if let Some(mut client) = self.grab.take() {
            self.released_early.extend(client.take_released());
            report.released_early.clone_from(&self.released_early);
            report.grab_released = Some(client.restore().is_ok());
        }
        if !self.config.headless && (!self.isolated || report.topology_restored == Some(true)) {
            display::clear_recovery_marker(&self.config.state_dir);
        }
        self.sleep_inhibitor = None;
        report
    }

    fn renew_grab(&mut self) {
        if let Some(client) = self.grab.as_mut() {
            let _ = client.renew();
        }
    }
}

fn single_monitor(backup: &DisplayBackup, headless: bool) -> anyhow::Result<(u32, u32)> {
    let [logical] = backup.topology.as_slice() else {
        anyhow::bail!("exactly one active logical monitor is supported");
    };
    let [(connector, serial)] = logical.monitors.as_slice() else {
        anyhow::bail!("the active monitor must not be mirrored");
    };
    anyhow::ensure!(
        (logical.scale - 1.0).abs() < f64::EPSILON && logical.transform == 0,
        "only scale 1.0 without a transform is supported"
    );
    anyhow::ensure!(
        headless || connector == "eDP-1",
        "only the built-in eDP-1 panel is supported as the sole output"
    );
    let output = backup
        .outputs
        .iter()
        .find(|o| &o.connector == connector && &o.serial == serial)
        .ok_or_else(|| anyhow::anyhow!("the active monitor has no output entry"))?;
    Ok((u32::try_from(output.width)?, u32::try_from(output.height)?))
}

fn poll_new_connector(conn: &Connection, before: &[String]) -> anyhow::Result<String> {
    let deadline = Instant::now() + SETTLE;
    loop {
        if let Some(connector) = display::connectors(conn)?
            .into_iter()
            .find(|c| !before.contains(c))
        {
            return Ok(connector);
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "no virtual connector appeared in time"
        );
        thread::sleep(Duration::from_millis(150));
    }
}

fn wait_connector_gone(conn: &Connection, connector: &str) -> bool {
    let deadline = Instant::now() + SETTLE;
    loop {
        match display::connectors(conn) {
            Ok(names) if !names.iter().any(|c| c == connector) => return true,
            _ if Instant::now() >= deadline => return false,
            _ => thread::sleep(Duration::from_millis(150)),
        }
    }
}

#[cfg(test)]
mod tests {
    use blackroom_gnome::mutter::display_config::{LogicalMonitorBackup, OutputBackup};

    use super::*;

    fn backup(connector: &str, scale: f64, transform: u32, mirrored: bool) -> DisplayBackup {
        let mut monitors = vec![(connector.to_string(), "S".to_string())];
        if mirrored {
            monitors.push(("HDMI-1".into(), "T".into()));
        }
        DisplayBackup {
            timestamp_unix: 0,
            session_id: "1".into(),
            outputs: vec![OutputBackup {
                connector: connector.into(),
                vendor: String::new(),
                product: String::new(),
                serial: "S".into(),
                mode_id: "m".into(),
                width: 1920,
                height: 1080,
                refresh_rate: 60.0,
                enabled: true,
            }],
            topology: vec![LogicalMonitorBackup {
                x: 0,
                y: 0,
                scale,
                transform,
                primary: true,
                monitors,
            }],
            primary_output: None,
            configuration_hash: 0,
        }
    }

    #[test]
    fn only_the_built_in_panel_at_scale_one_is_supported_live() {
        assert_eq!(
            single_monitor(&backup("eDP-1", 1.0, 0, false), false).unwrap(),
            (1920, 1080)
        );
        assert!(single_monitor(&backup("HDMI-1", 1.0, 0, false), false).is_err());
        assert!(single_monitor(&backup("eDP-1", 1.25, 0, false), false).is_err());
        assert!(single_monitor(&backup("eDP-1", 1.0, 1, false), false).is_err());
        assert!(single_monitor(&backup("eDP-1", 1.0, 0, true), false).is_err());
        assert!(single_monitor(&backup("Meta-0", 1.0, 0, false), true).is_ok());
    }

    #[test]
    fn input_events_are_range_checked() {
        let ok = [
            InputEvent::Key {
                code: 30,
                down: true,
            },
            InputEvent::Button {
                code: 0x110,
                down: false,
            },
            InputEvent::Move { x: 0.5, y: 2.0 },
            InputEvent::Scroll { dx: 0.0, dy: -15.0 },
        ];
        for event in ok {
            assert!(event.validate().is_ok(), "{event:?}");
        }
        let bad = [
            InputEvent::Key {
                code: 0,
                down: true,
            },
            InputEvent::Key {
                code: 0x300,
                down: true,
            },
            InputEvent::Key {
                code: 116,
                down: true,
            },
            InputEvent::Button {
                code: 1,
                down: true,
            },
            InputEvent::Move {
                x: f32::NAN,
                y: 0.0,
            },
            InputEvent::Scroll {
                dx: 0.0,
                dy: f32::INFINITY,
            },
            InputEvent::Scroll {
                dx: 5000.0,
                dy: 0.0,
            },
        ];
        for event in bad {
            assert!(event.validate().is_err(), "{event:?}");
        }
    }

    #[test]
    fn input_json_uses_a_tag_and_rejects_unknown_kinds() {
        let events: Vec<InputEvent> = serde_json::from_str(
            r#"[{"t":"key","code":30,"down":true},{"t":"move","x":0.1,"y":0.2},{"t":"scroll","dx":0,"dy":3}]"#,
        )
        .unwrap();
        assert_eq!(events.len(), 3);
        assert!(serde_json::from_str::<Vec<InputEvent>>(r#"[{"t":"exec","cmd":"x"}]"#).is_err());
        assert!(
            serde_json::from_str::<Vec<InputEvent>>(r#"[{"t":"key","code":-1,"down":true}]"#)
                .is_err()
        );
    }
}
