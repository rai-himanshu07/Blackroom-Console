//! `RemoteConsole`: start, stop and drive one isolated remote session. The flow is lifted from
//! `exp06 --integrated-probe`: RemoteDesktop/EIS, a virtual monitor with a video consumer, the
//! panel isolated, the physical input grab; Stop reverses it keeping the virtual monitor until the
//! ScreenCast session is stopped (Mutter 50.1 crashes otherwise, exp13), locks the session and only
//! then releases the input grab.
//!
//! All GNOME objects live on one actor thread, so the handle is a cheap, thread-safe `Clone`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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

use crate::approval::{Approvals, PendingView};
use crate::clipboard::{self, ClipboardError};
use crate::display::{self, Watchdog};
use crate::eis_support::{Authority, DeviceSeen, Remote, open_remote_with};
use crate::host::{Approval, HostConfig};
use crate::options::SessionOptions;
use crate::profile::Profile;
use crate::webrtc::{AudioPlan, audio_available, pick_opus};
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
/// Input batches waiting for the actor; more are dropped so Stop and housekeeping never queue behind a flood.
const MAX_QUEUED_INPUT: usize = 64;
const HEADLESS_SESSION: &str = "headless";
/// GNOME refuses remote sessions on a locked screen unless the Blackroom extension lifts that.
const LOCKED_HINT: &str = " (the screen is locked: switch on \"Remote use on the lock screen\" in the tray menu or Host settings, or unlock locally)";

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

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|name| name.trim().chars().take(64).collect())
        .unwrap_or_default()
}

/// `host_max` is the owner's ceiling (0 = none).
fn effective_fps(quality: Quality, cap: u32, host_max: u32) -> u32 {
    let wanted = if cap == 0 { quality.max_fps() } else { cap };
    if host_max == 0 {
        wanted
    } else {
        wanted.min(host_max)
    }
}

fn effective_bitrate(quality: Quality, override_kbps: u32, host_max: u32) -> u32 {
    let wanted = if override_kbps == 0 {
        quality.bitrate_kbps()
    } else {
        override_kbps
    };
    if host_max == 0 {
        wanted
    } else {
        wanted.min(host_max)
    }
}

fn proc_status_value(text: &str, key: &str) -> u64 {
    text.lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse().ok())
        .unwrap_or(0)
}

/// What the running session has actually done to this laptop, apart from what was asked for (`session`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct IsolationView {
    /// The panel was switched off and the virtual monitor took over.
    pub screen_blanked: bool,
    /// The grab daemon holds the built-in keyboard and touchpad.
    pub input_blocked: bool,
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
    /// The options of the running session (the defaults while idle).
    pub session: SessionOptions,
    /// What the session confirmed doing: the page compares it with `session` and shows any gap as a warning.
    pub isolation: IsolationView,
    /// What the laptop owner allows; the page greys out the rest.
    pub policy: crate::host::Policy,
    /// A connection waiting for the laptop owner's Accept or Deny.
    pub pending: Option<PendingView>,
    /// "private", "shared" or "custom".
    pub mode: &'static str,
    /// "off", "waiting" (asked for, no WebRTC yet), "on" or "unavailable" (with `audio_note`).
    pub audio: &'static str,
    pub audio_note: Option<String>,
    pub session_secs: u64,
    pub host: String,
    pub version: &'static str,
}

/// One input event from the browser. Pointer positions are fractions of the screen.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum InputEvent {
    Key {
        code: u32,
        down: bool,
    },
    Button {
        code: u32,
        down: bool,
    },
    Move {
        x: f32,
        y: f32,
    },
    Scroll {
        dx: f32,
        dy: f32,
    },
    /// Soft-keyboard text, typed as characters whatever the laptop's layout.
    Text {
        s: String,
    },
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
            Self::Text { s } => crate::keysym::keysyms(&s).map(|_| Self::Text { s }),
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
    isolation: Mutex<IsolationView>,
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
    options: Mutex<SessionOptions>,
    profile: Mutex<Profile>,
    profile_dir: Mutex<Option<PathBuf>>,
    host: Mutex<HostConfig>,
    approvals: Approvals,
    audio_sink: Mutex<Option<String>>,
    audio_note: Mutex<Option<String>>,
    /// The certificate renewal reminder, for the laptop's own indicator only (clients never see it).
    cert_note: Mutex<Option<String>>,
    session_started: Mutex<Option<Instant>>,
    last_input: Mutex<Instant>,
    sessions_started: AtomicU64,
    sessions_stopped: AtomicU64,
    last_clipboard: Mutex<Option<Instant>>,
    input_queued: AtomicUsize,
}

fn lock_ok<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Takes one place in the input queue; false when it is full (the caller must not enqueue).
fn reserve_input_slot(queued: &AtomicUsize) -> bool {
    queued
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < MAX_QUEUED_INPUT).then_some(n + 1)
        })
        .is_ok()
}

/// The restore timer may be cancelled only once the display is back and the lock was not asked for or took.
fn may_disarm_watchdog(
    topology_restored: Option<bool>,
    lock_wanted: bool,
    locked: Option<bool>,
) -> bool {
    topology_restored != Some(false) && (!lock_wanted || locked == Some(true))
}

enum Command {
    Start(SessionOptions, Sender<Result<(), String>>),
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
            isolation: Mutex::new(IsolationView::default()),
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
            options: Mutex::new(SessionOptions::default()),
            profile: Mutex::new(Profile::default()),
            profile_dir: Mutex::new(None),
            host: Mutex::new(HostConfig::default()),
            approvals: Approvals::default(),
            audio_sink: Mutex::new(None),
            audio_note: Mutex::new(None),
            cert_note: Mutex::new(None),
            session_started: Mutex::new(None),
            last_input: Mutex::new(Instant::now()),
            sessions_started: AtomicU64::new(0),
            sessions_stopped: AtomicU64::new(0),
            last_clipboard: Mutex::new(None),
            input_queued: AtomicUsize::new(0),
        });
        let actor_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("console-actor".into())
            .spawn(move || run_actor(&receiver, &actor_shared, &Arc::new(config)))
            .expect("spawn the console actor thread");
        Self { commands, shared }
    }

    /// What a Start without a body uses: private until a saved profile says otherwise.
    pub fn default_options(&self) -> SessionOptions {
        lock_ok(&self.shared.profile).session.clone()
    }

    /// Reads the saved settings from `dir` (and keeps writing there). Returns a note when the file was unusable.
    pub fn load_profile(&self, dir: PathBuf) -> Option<String> {
        let (profile, note) = Profile::load(&dir);
        let (host, host_note) = HostConfig::load(&dir);
        *lock_ok(&self.shared.quality) = profile.client.quality;
        *lock_ok(&self.shared.profile) = profile;
        *lock_ok(&self.shared.host) = host;
        *lock_ok(&self.shared.profile_dir) = Some(dir);
        match (note, host_note) {
            (Some(a), Some(b)) => Some(format!("{a}; {b}")),
            (a, b) => a.or(b),
        }
    }

    /// The owner's limits, read once at start: a change is saved and takes effect when the console restarts.
    pub fn host_config(&self) -> HostConfig {
        lock_ok(&self.shared.host).clone()
    }

    /// Where the settings files live (None until `load_profile`).
    pub fn settings_dir(&self) -> Option<PathBuf> {
        lock_ok(&self.shared.profile_dir).clone()
    }

    /// What a client may ask for, after the owner's limits.
    pub fn apply_policy(&self, options: SessionOptions) -> Result<SessionOptions, String> {
        lock_ok(&self.shared.host).apply(options.validated()?)
    }

    /// Waits for the laptop owner when the host setting is Ask; a start without it is refused by the caller.
    pub fn ask_approval(&self, mode: &str, device: &str) -> Result<(), String> {
        if lock_ok(&self.shared.host).approval != Approval::Ask {
            return Ok(());
        }
        self.shared
            .approvals
            .ask(mode, device, crate::approval::WAIT)
    }

    /// An answer from the laptop's own interfaces; false when `id` is not the request that waits.
    pub fn decide_approval(&self, id: u64, accept: bool) -> bool {
        self.shared.approvals.decide(id, accept)
    }

    pub fn pending_approval(&self) -> Option<PendingView> {
        self.shared.approvals.pending()
    }

    fn host_caps(&self) -> (u32, u32) {
        let host = lock_ok(&self.shared.host);
        (host.max_fps, host.max_bitrate_kbps)
    }

    /// The PipeWire output whose sound is sent (default: the system's default output).
    pub fn set_audio_sink(&self, sink: Option<String>) {
        *lock_ok(&self.shared.audio_sink) = sink;
    }

    pub fn set_cert_note(&self, note: Option<String>) {
        *lock_ok(&self.shared.cert_note) = note;
    }

    pub fn cert_note(&self) -> Option<String> {
        lock_ok(&self.shared.cert_note).clone()
    }

    /// Turns the laptop's sound on or off for the next WebRTC connection (the page reconnects its video).
    pub fn set_audio(&self, enabled: bool) {
        let enabled = enabled && lock_ok(&self.shared.host).allow_audio;
        lock_ok(&self.shared.options).audio = enabled;
        *lock_ok(&self.shared.audio_note) = None;
    }

    pub fn profile(&self) -> Profile {
        lock_ok(&self.shared.profile).clone()
    }

    /// Validates, saves (when a settings directory is set) and applies the new settings to the next session.
    pub fn set_profile(&self, profile: Profile) -> Result<Profile, String> {
        let profile = profile.validated()?;
        if let Some(dir) = lock_ok(&self.shared.profile_dir).as_ref() {
            profile
                .save(dir)
                .map_err(|error| format!("the settings could not be saved: {error}"))?;
        }
        self.set_quality(profile.client.quality);
        *lock_ok(&self.shared.profile) = profile.clone();
        Ok(profile)
    }

    /// Blocks until the session runs or setup failed (everything already undone).
    pub fn start(&self, options: SessionOptions) -> Result<Status, String> {
        let options = self.apply_policy(options)?;
        {
            let mut phase = lock_ok(&self.shared.phase);
            if *phase != Phase::Idle {
                return Err(format!("not idle ({:?})", *phase));
            }
            *phase = Phase::Starting;
        }
        let (reply, answer) = mpsc::channel();
        self.commands
            .send(Command::Start(options, reply))
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
        if !lock_ok(&self.shared.host).allow_text
            && events
                .iter()
                .any(|event| matches!(event, InputEvent::Text { .. }))
        {
            return Err("typing text is not allowed by the laptop owner".into());
        }
        self.beat();
        if events.is_empty() {
            return Ok(());
        }
        *lock_ok(&self.shared.last_input) = Instant::now();
        if !reserve_input_slot(&self.shared.input_queued) {
            return Err("too much input is waiting".into());
        }
        self.commands.send(Command::Input(events)).map_err(|_| {
            self.shared.input_queued.fetch_sub(1, Ordering::AcqRel);
            "console actor is gone".to_string()
        })
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
        self.apply_rates();
    }

    /// Changes the frame-rate ceiling and bitrate of the running session and of the next one (0 = follow the quality level).
    pub fn set_rates(&self, fps_cap: u32, bitrate_kbps: u32) -> Result<(), String> {
        let mut probe = lock_ok(&self.shared.options).clone();
        (probe.fps_cap, probe.bitrate_kbps) = (fps_cap, bitrate_kbps);
        let mut probe = probe.validated()?;
        let (max_fps, max_bitrate) = self.host_caps();
        if max_fps != 0 && probe.fps_cap > max_fps {
            probe.fps_cap = max_fps;
        }
        if max_bitrate != 0 && probe.bitrate_kbps > max_bitrate {
            probe.bitrate_kbps = max_bitrate;
        }
        {
            let mut options = lock_ok(&self.shared.options);
            (options.fps_cap, options.bitrate_kbps) = (probe.fps_cap, probe.bitrate_kbps);
        }
        self.apply_rates();
        Ok(())
    }

    fn apply_rates(&self) {
        let quality = *lock_ok(&self.shared.quality);
        let (fps_cap, bitrate) = {
            let options = lock_ok(&self.shared.options);
            (options.fps_cap, options.bitrate_kbps)
        };
        let (max_fps, max_bitrate) = self.host_caps();
        if let Some(tuning) = lock_ok(&self.shared.tuning).as_ref() {
            tuning.set(
                quality.jpeg_quality(),
                effective_fps(quality, fps_cap, max_fps),
            );
        }
        if let Some(session) = lock_ok(&self.shared.webrtc).as_ref() {
            session.set_bitrate(effective_bitrate(quality, bitrate, max_bitrate));
        }
    }

    /// Answers a browser's WebRTC offer with the live desktop as H.264; replaces any earlier peer.
    /// Blocks while ICE gathers, so call it from a blocking context.
    pub fn webrtc_answer(
        &self,
        offer_sdp: &str,
        allowed: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<String, String> {
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
        let input: InputSink = Arc::new(move |text| {
            // The channel outlives the HTTP request: input stops (and the heartbeat with it) when the login ends.
            if allowed() {
                console.input_json(text);
            }
        });
        let bitrate = effective_bitrate(
            quality,
            lock_ok(&self.shared.options).bitrate_kbps,
            self.host_caps().1,
        );
        let (want_audio, sink) = (
            lock_ok(&self.shared.options).audio,
            lock_ok(&self.shared.audio_sink).clone(),
        );
        let (plan, note) = if !want_audio {
            (None, None)
        } else if let Some(payload) = pick_opus(offer_sdp) {
            if audio_available(sink.as_deref()) {
                (Some(AudioPlan { payload, sink }), None)
            } else {
                (
                    None,
                    Some("no laptop sound could be captured (no sound output?)".to_string()),
                )
            }
        } else {
            (None, Some("this browser offered no audio".to_string()))
        };
        *lock_ok(&self.shared.audio_note) = note;
        let session = WebRtcSession::start(&tuning, bitrate, &h264, plan.as_ref(), input)?;
        let answer = session.answer(offer_sdp);
        let mut slot = lock_ok(&self.shared.webrtc);
        // Stop (and even a new Start) may have run while negotiating: only the session whose tuning we took may keep it.
        let same_session = lock_ok(&self.shared.tuning)
            .as_ref()
            .is_some_and(|now| Arc::ptr_eq(now, &tuning));
        if answer.is_err() || !same_session || *lock_ok(&self.shared.phase) != Phase::Running {
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
        let session = lock_ok(&self.shared.options).clone();
        let audio_note = lock_ok(&self.shared.audio_note).clone();
        let audio_state = if !session.audio {
            "off"
        } else if lock_ok(&self.shared.webrtc)
            .as_ref()
            .is_some_and(WebRtcSession::has_audio)
        {
            "on"
        } else if audio_note.is_some() {
            "unavailable"
        } else {
            "waiting"
        };
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
            policy: lock_ok(&self.shared.host).policy(),
            pending: self.shared.approvals.pending(),
            mode: session.label(),
            audio: audio_state,
            audio_note,
            session,
            isolation: *lock_ok(&self.shared.isolation),
            host: hostname(),
            version: env!("CARGO_PKG_VERSION"),
            session_secs: lock_ok(&self.shared.session_started)
                .map_or(0, |at| at.elapsed().as_secs()),
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
    options: SessionOptions,
    /// Top-left of the captured monitor in the shared layout: absolute pointer positions are layout coordinates.
    origin: (f32, f32),
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
            Ok(Command::Start(options, reply)) => {
                if selection.is_none()
                    && shared.clipboard_enabled.load(Ordering::Relaxed)
                    && let Ok(conn) = &conn
                {
                    selection = listen_selection_events(conn).ok();
                }
                let result = match (&conn, active.is_some()) {
                    (_, true) => Err("already running".to_string()),
                    (Err(error), _) => Err(format!("session bus: {error}")),
                    (Ok(conn), false) => match begin(conn, config, shared, options) {
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
                shared.input_queued.fetch_sub(1, Ordering::AcqRel);
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
    *lock_ok(&shared.session_started) = None;
    *lock_ok(&shared.isolation) = IsolationView::default();
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
    options: SessionOptions,
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
        options,
        origin: (0.0, 0.0),
        clipboard_text: None,
        clipboard_owned: false,
    };
    match active.setup() {
        Ok(()) => {
            *lock_ok(&shared.beat) = Instant::now();
            *lock_ok(&shared.size) = active.size;
            *lock_ok(&shared.options) = active.options.clone();
            *lock_ok(&shared.session_started) = Some(Instant::now());
            *lock_ok(&shared.isolation) = IsolationView {
                screen_blanked: active.isolated,
                input_blocked: active.grab.is_some(),
            };
            *lock_ok(&shared.last_input) = Instant::now();
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
        let blank = self.options.blank_panel;
        let target = if blank {
            let (width, height) = single_monitor(&backup, config.headless)?;
            let (width, height) = self
                .options
                .resolution
                .map_or((width, height), |size| (size.width, size.height));
            self.size = (width, height);
            None
        } else {
            let target = shared_target(&backup)?;
            self.size = target.logical;
            self.origin = (target.x as f32, target.y as f32);
            Some(target)
        };
        let shell_pid = display::gnome_shell_pid(self.conn)?;
        let backup_path = if blank {
            let path = display::write_backup(&config.state_dir, &backup, shell_pid)?;
            tracing::info!(path = %path.display(), "display backup written");
            if !config.headless {
                display::write_recovery_marker(&config.state_dir, &path)?;
            }
            Some(path)
        } else {
            None
        };
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
        let (stream, shape) = match &target {
            None => {
                let (width, height) = self.size;
                (
                    sc.record_virtual_with_cursor(
                        i32::try_from(width)?,
                        i32::try_from(height)?,
                        60.0,
                        self.options.cursor_in_video,
                    )?,
                    (width, height),
                )
            }
            Some(target) => (
                sc.record_monitor_with_cursor(&target.connector, self.options.cursor_in_video)?,
                target.native,
            ),
        };
        let node = stream.start_and_wait_for_pipewire_node(&sc)?;
        self.sc = Some(sc);
        let slot = JpegSlot::new();
        let stop = Arc::new(AtomicBool::new(false));
        let quality = *lock_ok(&self.shared.quality);
        let options = VideoOptions {
            preferred_width: i32::try_from(shape.0)?,
            preferred_height: i32::try_from(shape.1)?,
            quality: quality.jpeg_quality(),
            max_fps: effective_fps(
                quality,
                self.options.fps_cap,
                lock_ok(&self.shared.host).max_fps,
            ),
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

        if let Some(backup_path) = backup_path {
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
        }

        if self.options.block_local_input
            && !config.headless
            && let Some(socket) = &config.grab_socket
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
        if self.options.block_local_input {
            let socket = config.grab_socket.as_ref().ok_or_else(|| {
                anyhow::anyhow!("blocking local input needs the grab daemon (no --grab-socket)")
            })?;
            let mut client = Client::connect(socket, Duration::from_secs(10))?;
            let status = client.status()?;
            anyhow::ensure!(
                status.latched != Some(true),
                "an emergency stop is latched: stop the console and start it again (the daemon restarts), then Start"
            );
            anyhow::ensure!(
                status.phase.as_deref() == Some("idle") && status.grabs_enabled == Some(true),
                "the emergency daemon is not idle with grabs enabled: {status:?}"
            );
            drop(client);
        }
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
                self.origin.0 + x.clamp(0.0, 1.0) * (width - 1.0).max(0.0),
                self.origin.1 + y.clamp(0.0, 1.0) * (height - 1.0).max(0.0),
            ),
            InputEvent::Scroll { dx, dy } => remote.scroll(&self.authority, dx, dy),
            InputEvent::Text { ref s } => type_text(remote, &self.authority, s),
        };
        if let Err(error) = result {
            self.shared.input_refused.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(%error, "input event refused");
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
        let heartbeat = self
            .options
            .heartbeat_secs
            .map_or(self.config.heartbeat_timeout, |secs| {
                Duration::from_secs(u64::from(secs))
            });
        if lock_ok(&self.shared.beat).elapsed() > heartbeat {
            return Some("browser heartbeat lost".into());
        }
        let age = lock_ok(&self.shared.session_started).map_or(Duration::ZERO, |at| at.elapsed());
        if let Some(reason) = self
            .options
            .expired(lock_ok(&self.shared.last_input).elapsed(), age)
        {
            return Some(reason);
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

        self.renew_grab();
        let lock_wanted = self.options.lock_on_stop && !self.config.headless;
        if lock_wanted {
            report.locked = Some(display::lock_session(&self.session.session_id));
        }
        stage("lock requested");

        // A failed lock leaves the restore timer pending: it restores and locks again within 60 s.
        if let Some(mut watchdog) = self.watchdog.take() {
            if may_disarm_watchdog(report.topology_restored, lock_wanted, report.locked) {
                watchdog.disarm();
            } else if report.topology_restored != Some(false) {
                watchdog.disarm_guard();
            }
        }
        stage("watchdog handled");

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

/// Types `text` through Mutter's keysym path; stops at the first key it refuses.
fn type_text(remote: &Remote<'_>, authority: &Authority, text: &str) -> Result<(), BlackroomError> {
    authority.authorization(false).validate()?;
    let symbols = crate::keysym::keysyms(text).map_err(|reason| {
        BlackroomError::new(blackroom_core::error::ErrorCode::LeaseInvalid, reason)
    })?;
    for symbol in symbols {
        remote.session().notify_keyboard_keysym(symbol, true)?;
        remote.session().notify_keyboard_keysym(symbol, false)?;
    }
    Ok(())
}

/// The monitor a shared session captures: the primary logical monitor, with its place in the layout.
struct SharedTarget {
    connector: String,
    x: i32,
    y: i32,
    /// Logical pixels: the area the absolute pointer moves over.
    logical: (u32, u32),
    /// The monitor's own mode: what the capture delivers.
    native: (u32, u32),
}

fn shared_target(backup: &DisplayBackup) -> anyhow::Result<SharedTarget> {
    let logical = backup
        .topology
        .iter()
        .find(|logical| logical.primary)
        .or(backup.topology.first())
        .ok_or_else(|| anyhow::anyhow!("no active monitor to share"))?;
    let (connector, serial) = logical
        .monitors
        .first()
        .ok_or_else(|| anyhow::anyhow!("the active monitor has no output"))?;
    let output = backup
        .outputs
        .iter()
        .find(|o| &o.connector == connector && &o.serial == serial)
        .ok_or_else(|| anyhow::anyhow!("the active monitor has no output entry"))?;
    let (native_w, native_h) = (u32::try_from(output.width)?, u32::try_from(output.height)?);
    let scale = if logical.scale > 0.0 {
        logical.scale
    } else {
        1.0
    };
    let scaled = |value: u32| ((f64::from(value) / scale).round() as u32).max(1);
    // Rotated by 90 or 270 degrees (with or without a flip): the sides swap.
    let (w, h) = if logical.transform % 2 == 1 {
        (scaled(native_h), scaled(native_w))
    } else {
        (scaled(native_w), scaled(native_h))
    };
    Ok(SharedTarget {
        connector: connector.clone(),
        x: logical.x,
        y: logical.y,
        logical: (w, h),
        native: (native_w, native_h),
    })
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
            assert!(event.clone().validate().is_ok(), "{event:?}");
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
            assert!(event.clone().validate().is_err(), "{event:?}");
        }
    }

    #[test]
    fn input_queue_is_capped_and_freed_one_by_one() {
        let queued = AtomicUsize::new(0);
        for _ in 0..MAX_QUEUED_INPUT {
            assert!(reserve_input_slot(&queued));
        }
        assert!(!reserve_input_slot(&queued));
        queued.fetch_sub(1, Ordering::AcqRel);
        assert!(reserve_input_slot(&queued));
        assert!(!reserve_input_slot(&queued));
    }

    #[test]
    fn the_restore_timer_stays_when_the_lock_failed_or_the_display_is_not_back() {
        assert!(may_disarm_watchdog(Some(true), true, Some(true)));
        assert!(may_disarm_watchdog(None, false, None));
        assert!(may_disarm_watchdog(Some(true), false, None));
        assert!(!may_disarm_watchdog(Some(true), true, Some(false)));
        assert!(!may_disarm_watchdog(Some(false), true, Some(true)));
        assert!(!may_disarm_watchdog(Some(false), false, None));
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
