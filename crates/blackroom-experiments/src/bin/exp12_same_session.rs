//! Experiment 12 — Same session through lock, unlock and reconnect (Document 10 Experiment 12;
//! Phase 8; Gate FEAS-A, replacement path after exp11).
//!
//! exp11 showed that gnome-shell 50.1 inhibits remote access while the screen is locked, so every
//! RemoteDesktop session ends at the lock and none can be used to drive the unlock dialog. This
//! run tests the replacement path the product would use, on the operator's live session:
//!
//!  1. an EIS sender works before the lock (page sees the injected Shift tap);
//!  2. `loginctl lock-session` locks; the old EIS connection ends (recorded, not judged); a new
//!     `CreateSession` while locked is only observed (accepted or refused, and how far it gets);
//!     nothing may reach the observer page while locked;
//!  3. the program unlocks with `loginctl unlock-session` (what hostd would do after its own
//!     authentication; no password is typed or read; if logind refuses, the operator unlocks with
//!     their own password and the result is PARTIAL);
//!  4. a fresh RemoteDesktop/EIS session after the unlock delivers Shift, `a` and Left once each;
//!  5. the session is disconnected, locked and unlocked once more, and the same Shell and the same
//!     observer page instance (its page-load-relative first transition) must still be there.
//!
//! No physical-input grab, no display change, no virtual monitor, no capture.

use std::process::{Command, Stdio};
use std::sync::PoisonError;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::Context;
use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_experiments::eis_support::{
    Authority, DeviceSeen, Devices, bind_devices, command_line, pump,
};
use blackroom_experiments::lock_support::{
    LockSnap, LockedCounts, eis_label, elapsed_ms, ensure_no_grab_holders, judge_locked, judge_pre,
    judge_unlocked, lock_state, locked_counts, wait_lock_state, watch_active_changed,
};
use blackroom_experiments::observer::{FRESH, Observer};
use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, evidence_dir, write_evidence,
};
use blackroom_gnome::backend::SessionInfo;
use blackroom_gnome::mutter::eis::EiConnection;
use blackroom_gnome::mutter::lock;
use blackroom_gnome::mutter::remote_desktop::RemoteDesktopSession;
use blackroom_gnome::mutter::session::discover_session;
use clap::Parser;
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;
use zbus::blocking::Connection;

const EXP_ID: &str = "exp12";
const OBSERVER_PAGE: &str = include_str!("../../assets/exp08_observer.html");
// Checked 2026-10-01 against gsettings and xkb: no bare binding for these evdev keys.
const KEY_A: u32 = 30;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_LEFT: u32 = 105;
const LOCK_WAIT: Duration = Duration::from_secs(15);
const UNLOCK_WAIT: Duration = Duration::from_secs(20);
/// How long the first lock stays up, so the operator can look at it and the page's silence counts.
const LOCK_HOLD: Duration = Duration::from_secs(20);
const SECOND_LOCK_HOLD: Duration = Duration::from_secs(8);
/// The page must keep beating through the lock for its silence to count as evidence.
const MIN_BEATS_DURING_LOCK: u64 = 8;
const MAX_BEAT_AGE: Duration = Duration::from_secs(3);

#[derive(Parser, Debug)]
#[command(
    about = "Experiment 12: same session through lock, logind unlock and reconnect (MUTATING)"
)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Required: the operator is present, work is saved, a second-device SSH session is open and
    /// the operator can unlock with their own password (docs/ops/experiment-safety.md §7).
    #[arg(long, default_value_t = false)]
    operator_present: bool,
    /// Seconds the observer page must stay focused and fullscreen before the run starts.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(3..=30))]
    settle_secs: u64,
    /// Seconds to wait for the operator to focus the observer page.
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(30..=600))]
    ready_timeout_secs: u64,
    /// Seconds to wait for a manual unlock if logind did not unlock the session.
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(30..=600))]
    manual_unlock_secs: u64,
}

#[derive(Debug, Serialize)]
struct Step {
    name: String,
    outcome: String,
    at_ms: u64,
}

#[derive(Debug, Default, Serialize)]
struct Unlock {
    /// `loginctl` when logind unlocked the session, `manual` when the operator had to.
    method: Option<&'static str>,
    logind_exit: Option<i32>,
    logind_stderr: Option<String>,
    observed_after_ms: Option<u64>,
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
    blocked: Option<String>,
    failure: Option<String>,
    aborted: Option<String>,
    inconclusive: Vec<String>,
    violations: Vec<String>,
    initial_lock: Option<LockSnap>,
    steps: Vec<Step>,
    devices_seen: Vec<DeviceSeen>,
    lock1_engaged_after_ms: Option<u64>,
    eis1_ended_after_ms: Option<u64>,
    eis1_events: Vec<String>,
    /// What a new CreateSession did while the session was locked (observation only).
    locked_create_session: Option<String>,
    locked_at_snapshot: Option<LockSnap>,
    beats_during_lock: u64,
    tally_pre: Option<Value>,
    tally_locked: Option<LockedCounts>,
    tally_unlocked: Option<Value>,
    notes_pre: Vec<String>,
    notes_locked: Vec<String>,
    notes_unlocked: Vec<String>,
    unlock1: Option<Unlock>,
    fresh_eis_after_unlock: Option<bool>,
    lock2_engaged_after_ms: Option<u64>,
    unlock2: Option<Unlock>,
    page_loads_before: Option<u64>,
    page_loads_after: Option<u64>,
    session_id_after: Option<String>,
    active_changed: Vec<(u64, bool)>,
    beats_total: u64,
}

impl Run {
    fn step(&mut self, name: &str, outcome: impl Into<String>, t0: Instant) {
        self.steps.push(Step {
            name: name.to_string(),
            outcome: outcome.into(),
            at_ms: elapsed_ms(t0),
        });
    }

    /// An injected tap: a refusal is a violation.
    fn tap_result(&mut self, name: &str, result: Result<(), BlackroomError>, t0: Instant) {
        match result {
            Ok(()) => self.step(name, "accepted", t0),
            Err(error) => {
                let outcome = format!("refused:{:?}", error.code);
                self.violations.push(format!("{name}: {outcome}"));
                self.step(name, outcome, t0);
            }
        }
    }

    fn complete(&self) -> bool {
        self.tally_pre.is_some()
            && self.tally_locked.is_some()
            && self.tally_unlocked.is_some()
            && self.lock1_engaged_after_ms.is_some()
            && self
                .unlock1
                .as_ref()
                .is_some_and(|u| u.observed_after_ms.is_some())
            && self.fresh_eis_after_unlock == Some(true)
            && self.lock2_engaged_after_ms.is_some()
            && self
                .unlock2
                .as_ref()
                .is_some_and(|u| u.observed_after_ms.is_some())
            && self.page_same() == Some(true)
            && self.same_session() == Some(true)
    }

    /// The observer page was served once and never reloaded.
    fn page_same(&self) -> Option<bool> {
        let (before, after) = (self.page_loads_before?, self.page_loads_after?);
        Some(before >= 1 && before == after)
    }

    fn same_session(&self) -> Option<bool> {
        Some(self.session_id_after.as_ref()? == self.selected_session_id.as_ref()?)
    }
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
         docs/ops/experiment-safety.md §7 (work saved, second-device SSH open)"
    );
    Ok(())
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
    ensure_no_grab_holders()?;
    run.git_head = command_line("git", &["rev-parse", "--short", "HEAD"]);
    run.git_dirty = Some(command_line("git", &["status", "--porcelain", "--", "crates"]).is_some());
    Ok(info)
}

/// A RemoteDesktop session with its EIS sender. Fields drop in order: devices, connection, session.
struct Remote<'a> {
    devices: Devices,
    eis: EiConnection,
    _session: RemoteDesktopSession<'a>,
}

fn open_remote<'a>(
    conn: &'a Connection,
    authority: &Authority,
    seen: &mut Vec<DeviceSeen>,
) -> Result<Remote<'a>, (String, String)> {
    let fail = |step: &str, error: BlackroomError| (step.to_string(), error.to_string());
    let mut session = RemoteDesktopSession::create(conn).map_err(|e| fail("CreateSession", e))?;
    session.start().map_err(|e| fail("Start", e))?;
    let mut eis = session
        .connect_to_eis(&authority.authorization(false))
        .map_err(|e| fail("ConnectToEIS", e))?;
    eis.handshake_sender(Duration::from_secs(5))
        .map_err(|e| fail("EIS handshake", e))?;
    let mut devices = Devices::default();
    bind_devices(&mut eis, &mut devices, seen)
        .map_err(|(step, detail)| (step.to_string(), detail))?;
    Ok(Remote {
        devices,
        eis,
        _session: session,
    })
}

impl Remote<'_> {
    fn tap(&mut self, authority: &Authority, key: u32) -> Result<(), BlackroomError> {
        let _ = pump(
            &mut self.eis,
            &mut self.devices,
            &mut Vec::new(),
            Duration::from_millis(30),
            |_| {},
        );
        match self.devices.keyboard.clone() {
            Some(device) => self
                .eis
                .send_key_tap(&authority.authorization(false), &device, key),
            None => Err(BlackroomError::new(
                ErrorCode::MutterUnavailable,
                "no active keyboard device",
            )),
        }
    }
}

fn page_loads(observer: &Observer) -> u64 {
    observer.snapshot(|state| state.page_loads)
}

const LOGINCTL_TIMEOUT: Duration = Duration::from_secs(10);

/// Runs `loginctl` with a null stdin and a hard timeout; returns the exit code and a short stderr.
fn loginctl(args: &[&str]) -> Option<(Option<i32>, Option<String>)> {
    let mut child = Command::new("loginctl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + LOGINCTL_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        sleep(Duration::from_millis(50));
    };
    let mut text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
    }
    let text: String = text.trim().chars().take(160).collect();
    Some((status.code(), (!text.is_empty()).then_some(text)))
}

enum LockOutcome {
    /// Both signals agreed after this many milliseconds.
    Engaged(Instant, u64),
    /// The call succeeded but the signals never agreed: the screen may be locked.
    Uncertain,
    /// The call itself failed or timed out.
    Failed,
}

fn lock_session(info: &SessionInfo) -> LockOutcome {
    let started = Instant::now();
    match loginctl(&["lock-session", &info.session_id]) {
        Some((Some(0), _)) => {}
        _ => return LockOutcome::Failed,
    }
    match wait_lock_state(info, LockSnap::locked, LOCK_WAIT, started) {
        Some(ms) => LockOutcome::Engaged(started, ms),
        None => LockOutcome::Uncertain,
    }
}

/// Unlocks the way hostd would (logind `Unlock`); falls back to waiting for the operator. The
/// method is `loginctl` only if the session was still locked right before the call, the call
/// exited 0 and the session then unlocked.
fn unlock_session(info: &SessionInfo, manual_wait: Duration) -> Unlock {
    let mut record = Unlock::default();
    let still_locked = lock_state(info).is_some_and(LockSnap::locked);
    let started = Instant::now();
    if still_locked {
        match loginctl(&["unlock-session", &info.session_id]) {
            Some((code, stderr)) => {
                record.logind_exit = code;
                record.logind_stderr = stderr;
            }
            None => record.logind_stderr = Some("loginctl timed out or did not start".into()),
        }
        if record.logind_exit == Some(0)
            && let Some(ms) = wait_lock_state(info, LockSnap::unlocked, UNLOCK_WAIT, started)
        {
            record.method = Some("loginctl");
            record.observed_after_ms = Some(ms);
            return record;
        }
    } else {
        record.logind_stderr = Some("the session was no longer locked before the call".into());
    }
    println!(
        "the session was not unlocked by logind: unlock it with your own password ({} s)",
        manual_wait.as_secs()
    );
    if let Some(ms) = wait_lock_state(info, LockSnap::unlocked, manual_wait, started) {
        record.method = Some("manual");
        record.observed_after_ms = Some(ms);
    }
    record
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
    run.page_loads_before = Some(page_loads(observer));
    let conn = Connection::session().context("session bus")?;
    let authority = Authority::new(Duration::from_secs(900));
    let manual_wait = Duration::from_secs(args.manual_unlock_secs);

    // 1. The injection path works before the lock.
    let mut remote = open_remote(&conn, &authority, &mut run.devices_seen)
        .map_err(|(step, detail)| block(run, &step, detail))?;
    if !observer.arm(Duration::from_secs(10)) {
        run.aborted = Some("observer did not acknowledge the tally reset while focused".into());
        return Ok(());
    }
    let result = remote.tap(&authority, KEY_LEFTSHIFT);
    run.tap_result("pre_lock_key_tap_shift", result, t0);
    sleep(Duration::from_millis(400));
    observer.wait_beats(2, Duration::from_secs(3));
    run.tally_pre = observer.snapshot(|state| state.tally.clone());
    let pre_notes = judge_pre(run.tally_pre.as_ref().unwrap_or(&Value::Null));
    if !pre_notes.is_empty() || !run.violations.is_empty() {
        run.aborted = Some(format!(
            "pre-lock checks failed, not locking: {pre_notes:?}"
        ));
        return Ok(());
    }

    // 2. Lock; the old connection ends; a new session while locked is only observed.
    observer.set_prompt(
        "LOCKING in 6 s. HANDS OFF for about 45 s: the program locks, looks around, then unlocks the \
         session itself (no password needed). If the lock screen is still up 60 s from now, unlock \
         with your password.",
    );
    sleep(Duration::from_secs(6));
    if !observer.arm(Duration::from_secs(10)) {
        run.aborted = Some("observer lost focus before the lock".into());
        return Ok(());
    }
    let (lock_started, engaged) = match lock_session(info) {
        LockOutcome::Engaged(started, ms) => (started, ms),
        LockOutcome::Failed => {
            run.aborted = Some("loginctl lock-session failed".into());
            return Ok(());
        }
        LockOutcome::Uncertain => {
            run.aborted = Some("the lock signals never agreed; unlocking best-effort".into());
            run.unlock1 = Some(unlock_session(info, manual_wait));
            return Ok(());
        }
    };
    run.lock1_engaged_after_ms = Some(engaged);
    let beats_at_lock = observer.snapshot(|state| state.beats);
    println!("LOCKED after {engaged} ms. Hands off; the program unlocks it in about 20 s.");

    let mut labels: Vec<String> = Vec::new();
    let end_deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < end_deadline {
        if let Some(error) = pump(
            &mut remote.eis,
            &mut remote.devices,
            &mut Vec::new(),
            Duration::from_millis(200),
            |event| labels.push(eis_label(event).to_string()),
        ) {
            labels.push(format!("transport_closed: {error}"));
            run.eis1_ended_after_ms = Some(elapsed_ms(lock_started));
            break;
        }
        if labels.iter().any(|label| label == "Disconnected") {
            run.eis1_ended_after_ms = Some(elapsed_ms(lock_started));
            break;
        }
    }
    run.eis1_events = labels;
    run.step(
        "old_eis_connection_after_lock",
        format!("ended_after_ms={:?}", run.eis1_ended_after_ms),
        t0,
    );

    let mut seen_locked = Vec::new();
    let attempt = match open_remote(&conn, &authority, &mut seen_locked) {
        Err((step, detail)) => format!("refused at {step}: {detail}"),
        Ok(mut locked_remote) => {
            // Observation only: whatever reaches the lock screen must not reach the page.
            let ok = lock_state(info).is_some_and(LockSnap::locked);
            if ok {
                for dx in [5.0_f32, -5.0] {
                    if let Some(device) = locked_remote.devices.pointer.clone() {
                        let _ = locked_remote.eis.send_pointer_motion(
                            &authority.authorization(false),
                            &device,
                            dx,
                            0.0,
                        );
                    }
                }
                let _ = locked_remote.tap(&authority, KEY_LEFTSHIFT);
            }
            format!(
                "accepted: EIS bound, devices {}, tap sent={ok}",
                seen_locked.len()
            )
        }
    };
    run.step("create_session_while_locked", attempt.clone(), t0);
    run.locked_create_session = Some(attempt);

    while lock_started.elapsed() < LOCK_HOLD {
        sleep(Duration::from_millis(200));
    }
    observer.wait_beats(2, Duration::from_secs(5));
    let snapshot_lock = lock_state(info);
    run.locked_at_snapshot = snapshot_lock;
    run.beats_during_lock = observer
        .snapshot(|state| state.beats)
        .saturating_sub(beats_at_lock);
    run.tally_locked = observer
        .snapshot(|state| state.tally.clone())
        .map(|tally| locked_counts(&tally));
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
    if !observer.snapshot(|state| state.beat_at.is_some_and(|at| at.elapsed() <= MAX_BEAT_AGE)) {
        run.inconclusive.push(
            "the page's last heartbeat was stale when the locked tally was taken".to_string(),
        );
    }

    // 3. Unlock the way hostd would.
    let unlock = unlock_session(info, manual_wait);
    let unlocked = unlock.observed_after_ms.is_some();
    if unlock.method != Some("loginctl") {
        run.inconclusive.push(format!(
            "logind did not unlock the session (exit {:?}, {:?}, method {:?})",
            unlock.logind_exit, unlock.logind_stderr, unlock.method
        ));
    }
    run.unlock1 = Some(unlock);
    if !unlocked {
        run.aborted = Some(
            "no unlock observed (unlock manually; `sudo loginctl unlock-session` over SSH)".into(),
        );
        return Ok(());
    }
    observer.set_prompt("UNLOCKED. Hands off until the next lock message.");

    // 4. A fresh session after the unlock delivers input.
    drop(remote);
    if !observer.wait_ready(Duration::from_secs(90), Duration::from_secs(3)) {
        run.aborted =
            Some("observer page not focused and fullscreen again after the unlock".into());
        return Ok(());
    }
    if !observer.arm(Duration::from_secs(10)) {
        run.aborted = Some("observer did not acknowledge the tally reset after the unlock".into());
        return Ok(());
    }
    let mut fresh = match open_remote(&conn, &authority, &mut run.devices_seen) {
        Ok(fresh) => {
            run.fresh_eis_after_unlock = Some(true);
            run.step("fresh_remote_session_after_unlock", "accepted", t0);
            fresh
        }
        Err((step, detail)) => {
            run.fresh_eis_after_unlock = Some(false);
            run.violations.push(format!(
                "fresh session after the unlock failed at {step}: {detail}"
            ));
            return Ok(());
        }
    };
    if !observer.wait_ready(Duration::from_secs(30), Duration::from_secs(1)) {
        run.aborted = Some("observer page lost focus before the post-unlock taps".into());
        return Ok(());
    }
    for (name, key) in [
        ("unlocked_key_tap_shift", KEY_LEFTSHIFT),
        ("unlocked_key_tap_a", KEY_A),
        ("unlocked_key_tap_left", KEY_LEFT),
    ] {
        if !observer.ready_now(FRESH) {
            run.aborted = Some(format!("observer page lost focus before {name}"));
            return Ok(());
        }
        let result = fresh.tap(&authority, key);
        run.tap_result(name, result, t0);
        sleep(Duration::from_millis(300));
    }
    observer.wait_beats(2, Duration::from_secs(3));
    run.tally_unlocked = observer.snapshot(|state| state.tally.clone());

    // 5. Disconnect, lock again, unlock again: the same session and page must still be there.
    drop(fresh);
    if run.unlock1.as_ref().and_then(|unlock| unlock.method) != Some("loginctl") {
        run.inconclusive
            .push("second lock cycle skipped: logind did not unlock the first time".to_string());
        return Ok(());
    }
    observer.set_prompt(
        "LOCKING AGAIN in 6 s. HANDS OFF for about 30 s: the program locks and unlocks the session \
         once more.",
    );
    sleep(Duration::from_secs(6));
    let (lock2_started, engaged2) = match lock_session(info) {
        LockOutcome::Engaged(started, ms) => (started, ms),
        LockOutcome::Failed => {
            run.aborted = Some("the second loginctl lock-session failed".into());
            return Ok(());
        }
        LockOutcome::Uncertain => {
            run.aborted =
                Some("the second lock signals never agreed; unlocking best-effort".into());
            run.unlock2 = Some(unlock_session(info, manual_wait));
            return Ok(());
        }
    };
    run.lock2_engaged_after_ms = Some(engaged2);
    while lock2_started.elapsed() < SECOND_LOCK_HOLD {
        sleep(Duration::from_millis(200));
    }
    let unlock2 = unlock_session(info, manual_wait);
    let unlocked2 = unlock2.observed_after_ms.is_some();
    if unlock2.method != Some("loginctl") {
        run.inconclusive.push(format!(
            "second unlock was not by logind (method {:?})",
            unlock2.method
        ));
    }
    run.unlock2 = Some(unlock2);
    if !unlocked2 {
        run.aborted = Some("no second unlock observed".into());
        return Ok(());
    }
    if !observer.wait_ready(Duration::from_secs(90), Duration::from_secs(3)) {
        run.aborted =
            Some("observer page not focused and fullscreen after the second unlock".into());
        return Ok(());
    }
    observer.wait_beats(2, Duration::from_secs(3));
    run.page_loads_after = Some(page_loads(observer));
    run.session_id_after = discover_session().ok().map(|info| info.session_id);
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
            if run.page_loads_after.is_none() {
                run.page_loads_after = Some(page_loads(&observer));
            }
            if run.session_id_after.is_none() {
                run.session_id_after = discover_session().ok().map(|info| info.session_id);
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
    if run.page_same() == Some(false) {
        run.inconclusive
            .push("the observer page was reloaded during the run".to_string());
    }
    if run.same_session() == Some(false) {
        run.violations
            .push("the selected login session changed".to_string());
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

    let result = classify(&run);
    let observed = format!(
        "result={result}; blocked={:?}; failure={:?}; aborted={:?}; inconclusive={:?}; \
         lock1_ms={:?}; old_eis_ended_after_ms={:?}; locked_create_session={:?}; beats_during_lock={}; \
         unlock1={:?}; fresh_eis_after_unlock={:?}; lock2_ms={:?}; unlock2={:?}; page_same={:?}; \
         violations={:?}; shell {:?}->{:?}",
        run.blocked,
        run.failure,
        run.aborted,
        run.inconclusive,
        run.lock1_engaged_after_ms,
        run.eis1_ended_after_ms,
        run.locked_create_session,
        run.beats_during_lock,
        run.unlock1,
        run.fresh_eis_after_unlock,
        run.lock2_engaged_after_ms,
        run.unlock2,
        run.page_same(),
        run.violations,
        run.shell_pid_before,
        run.shell_pid_after
    );
    println!("{observed}");
    for step in &run.steps {
        println!("  {:>6} ms {:<36} {}", step.at_ms, step.name, step.outcome);
    }

    let report = ExperimentReport {
        experiment: "Experiment 12 — Same session through lock, unlock and reconnect (FEAS-A)".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1, single built-in \
                      display"
            .to_string(),
        objective: "Show that, with remote access inhibited while the screen is locked (exp11), the \
                    session can still be locked, unlocked by logind from a user process, and driven by a \
                    fresh RemoteDesktop/EIS session after the unlock, with the same login session, \
                    Shell and observer page instance throughout."
            .to_string(),
        hypothesis: "Nothing injected before the lock reaches the page while locked; `loginctl \
                     unlock-session` from the session's own user unlocks it without a password; a new \
                     RemoteDesktop session after the unlock delivers Shift, `a` and Left once each; the \
                     Shell PID and the observer page instance survive two lock cycles."
            .to_string(),
        procedure: format!(
            "Preflight (unlocked session, no grab holder, remote desktop service inactive); observer page \
             focused and fullscreen ({} s settle); RemoteDesktop session and EIS; pre-lock Shift tap; \
             `loginctl lock-session`; observe the old EIS connection, try a new CreateSession while \
             locked, hold {} s; `loginctl unlock-session` (manual unlock only as the fallback); fresh \
             session: Shift, `a`, Left; disconnect; lock, hold {} s, `loginctl unlock-session` again.",
            args.settle_secs,
            LOCK_HOLD.as_secs(),
            SECOND_LOCK_HOLD.as_secs()
        ),
        expected: "Lock observed by GetActive and LockedHint agreeing (the ScreenSaver owner is not mapped to \
                   the login1 session on this host: not provenance-verified); the page tally while locked \
                   has no key, button, pointer or wheel event and the page keeps beating (a leak can only \
                   show if a CreateSession while locked is accepted and its pointer and Shift events are \
                   sent; otherwise the locked tally only shows that the operator kept their hands off, \
                   and keyboard silence is weak anyway because the page has no keyboard focus); logind unlocks both times; the \
                   post-unlock tally is exactly Shift, A and Left, plain; login session id, Shell PID and \
                   page instance (page-load count) unchanged. How the old EIS connection ends and what a CreateSession while locked does \
                   are observations, not criteria."
            .to_string(),
        observed,
        evidence: vec![
            "findings.json (this directory), including the page's tally at three points and the lock/EIS timings"
                .to_string(),
        ],
        result,
        failure: run.failure.clone().or_else(|| run.blocked.clone()),
        root_cause: None,
        security_impact: Some(format!(
            "The live session is locked up to twice. {} No password is typed or read by the program. \
             Only Shift, `a`, Left and a net-zero 5 px pointer move (only if a session could be created \
             while locked) are injected. PASS here does not promote FEAS-A by itself.",
            match run.unlock1.as_ref().and_then(|unlock| unlock.method) {
                Some("loginctl") => "The program unlocked the session through logind; on this host, per \
                                     this run, a process of the session's own user can do that, so the \
                                     lock screen is not a boundary against it.",
                Some("manual") => "logind did not unlock the session in this run; the operator unlocked \
                                   it with their own password.",
                _ => "No unlock was observed in this run.",
            }
        )),
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

    fn complete_run() -> Run {
        let mut run = Run {
            tally_pre: Some(Value::Null),
            tally_locked: Some(locked_counts(&serde_json::json!({
                "keys": [], "buttons": [], "pointer": {"moves": 0}, "wheel": {"events": 0},
            }))),
            tally_unlocked: Some(Value::Null),
            lock1_engaged_after_ms: Some(800),
            unlock1: Some(Unlock {
                method: Some("loginctl"),
                observed_after_ms: Some(300),
                ..Unlock::default()
            }),
            fresh_eis_after_unlock: Some(true),
            lock2_engaged_after_ms: Some(700),
            unlock2: Some(Unlock {
                method: Some("loginctl"),
                observed_after_ms: Some(300),
                ..Unlock::default()
            }),
            page_loads_before: Some(1),
            page_loads_after: Some(1),
            selected_session_id: Some("2".to_string()),
            session_id_after: Some("2".to_string()),
            ..Run::default()
        };
        run.step("x", "accepted", Instant::now());
        run
    }

    #[test]
    fn lock_requires_the_operator_flag_and_bounded_waits() {
        assert!(require_operator(&Args::parse_from(["exp12"])).is_err());
        assert!(require_operator(&Args::parse_from(["exp12", "--operator-present"])).is_ok());
        assert!(Args::try_parse_from(["exp12", "--manual-unlock-secs", "5"]).is_err());
        assert!(Args::try_parse_from(["exp12", "--settle-secs", "1"]).is_err());
    }

    #[test]
    fn only_a_complete_clean_run_passes() {
        let mut run = complete_run();
        assert_eq!(classify(&run), ExperimentResult::Pass);

        run.page_loads_after = Some(2);
        assert!(!run.complete());
        assert_eq!(classify(&run), ExperimentResult::Partial);
        run.page_loads_after = Some(1);

        run.unlock1 = Some(Unlock {
            method: Some("manual"),
            observed_after_ms: Some(30_000),
            ..Unlock::default()
        });
        run.inconclusive.push("logind refused".to_string());
        assert_eq!(classify(&run), ExperimentResult::Partial);
        run.inconclusive.clear();

        run.fresh_eis_after_unlock = Some(false);
        assert_eq!(classify(&run), ExperimentResult::Partial);
        run.violations.push("fresh session failed".to_string());
        assert_eq!(classify(&run), ExperimentResult::Fail);
        run.blocked = Some("preflight".to_string());
        assert_eq!(classify(&run), ExperimentResult::Blocked);
    }

    #[test]
    fn a_changed_session_id_is_not_a_pass() {
        let mut run = complete_run();
        run.session_id_after = Some("3".to_string());
        assert_eq!(run.same_session(), Some(false));
        assert!(!run.complete());
    }

    #[test]
    fn a_reload_is_detected_by_the_page_load_counter() {
        let mut run = complete_run();
        assert_eq!(run.page_same(), Some(true));
        run.page_loads_after = Some(2);
        assert_eq!(run.page_same(), Some(false));
        run.page_loads_before = Some(0);
        run.page_loads_after = Some(0);
        assert_eq!(run.page_same(), Some(false));
    }

    #[test]
    fn a_refused_tap_is_a_violation_and_a_missing_page_instance_is_not_a_pass() {
        let t0 = Instant::now();
        let mut run = Run::default();
        run.tap_result(
            "a",
            Err(BlackroomError::new(ErrorCode::MutterUnavailable, "t")),
            t0,
        );
        assert_eq!(run.violations.len(), 1);
        let mut incomplete = complete_run();
        incomplete.page_loads_before = None;
        assert_eq!(incomplete.page_same(), None);
        assert!(!incomplete.complete());
    }
}
