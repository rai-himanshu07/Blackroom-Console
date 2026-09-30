//! Experiment 8 — Remote Input (Document 10 Experiment 8; Phase 6 Step 3; Gate FEAS-D).
//!
//! MUTATING and supervised: injects a short, harmless input sequence into the
//! *selected existing* GNOME session through `RemoteDesktop.Session.ConnectToEIS`,
//! then checks input stops after an authorization revoke and after session
//! stop. It never touches displays, grabs physical input, or opens a ScreenCast.
//!
//! Only modifier keys are sent (Shift tap; Shift+Right Ctrl chord) and pointer
//! motion nets to zero; no key with a GNOME or Firefox binding is used. The run serves a local observer page (loopback, one-time
//! token); the page reports focus/fullscreen every 250 ms and tallies the
//! events that actually reach it. Input is sent only while the page is focused
//! and fullscreen, and the recorded result comes from the page's own tally.
//!
//! Abort by defocusing the page (injection stops at the next stage). A signal
//! kill skips `Drop`; owner-death teardown of a RemoteDesktop session is unobserved.

use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::lease::{Capability, ControlLease, InputAuthorization};
use blackroom_core::state::State;
use blackroom_experiments::observer::{FRESH, Observer};
use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, current_uid, discover, evidence_dir,
    write_evidence,
};
use blackroom_gnome::mutter::eis::EiConnection;
use blackroom_gnome::mutter::remote_desktop::RemoteDesktopSession;
use clap::Parser;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use reis::event::{Device, DeviceCapability, DeviceResumed, EiEvent};
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;

const EXP_ID: &str = "exp08";
// F13 is evdev 183 = XF86Tools, which GNOME binds to Settings: not inert (2026-09-30 run).
const KEY_LEFTSHIFT: u32 = 42;
const KEY_RIGHTCTRL: u32 = 97;
const BTN_LEFT: u32 = 272;
const OBSERVER_PAGE: &str = include_str!("../../assets/exp08_observer.html");

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
    observer_at_abort: Option<String>,
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

/// Judges the page's own tally (cleared at arming) against the exact sequence
/// sent: Shift twice (tap, chord), Right Ctrl once while Shift is held, one left
/// click, net-zero pointer motion, a positive scroll, and nothing from the
/// revoked or post-stop attempts.
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
        key_count("down", "ShiftLeft") == 2 && key_count("up", "ShiftLeft") == 2,
        format!(
            "ShiftLeft down/up {}/{} (expected 2/2)",
            key_count("down", "ShiftLeft"),
            key_count("up", "ShiftLeft")
        ),
    );
    check(
        key_count("down", "ControlRight") == 1 && key_count("up", "ControlRight") == 1,
        "ControlRight not 1/1".to_string(),
    );
    check(
        keys.iter().any(|key| {
            key["type"] == "down" && key["code"] == "ControlRight" && key["shift"] == true
        }),
        "Right Ctrl did not arrive while Shift was held".to_string(),
    );
    let other = keys
        .iter()
        .filter(|key| key["code"] != "ControlRight" && key["code"] != "ShiftLeft")
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
    // Positions, not movementX: the browser reports 0 for the first move after
    // an idle period, which hid the +40 step in run 2.
    let pointer = &tally["pointer"];
    let number = |value: &Value| value.as_f64().unwrap_or(f64::NAN);
    let positions = pointer["positions"].as_array().cloned().unwrap_or_default();
    let (width, height) = (
        number(&tally["viewport"]["w"]),
        number(&tally["viewport"]["h"]),
    );
    let pointer_ok = match (positions.first(), positions.last()) {
        (Some(first), Some(last)) if positions.len() >= 2 => {
            let (dx, dy) = (
                number(&last["x"]) - number(&first["x"]),
                number(&last["y"]) - number(&first["y"]),
            );
            // The +40 step must land inside the viewport (a clamped edge move
            // also looks like a -40 return) and the -40 step must undo it.
            // Extra same-position moves are tolerated: browsers re-dispatch
            // mousemove after a scroll or click.
            let inside = (2.0..=width - 3.0).contains(&number(&first["x"]))
                && (2.0..=height - 3.0).contains(&number(&first["y"]));
            (dx + 40.0).abs() <= 4.0 && dy.abs() <= 2.0 && inside
        }
        _ => false,
    };
    check(
        pointer_ok,
        format!(
            "pointer motion did not show +40 then -40 away from an edge (positions {positions:?})"
        ),
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

    if !observer.arm(Duration::from_secs(10)) {
        run.aborted =
            Some("observer did not acknowledge the tally reset while focused".to_string());
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
                    run.observer_at_abort = Some(observer.describe());
                }
            }
        }};
    }
    stage!("key_tap_shift", keyboard, Expect::Accept, |d| {
        eis.send_key_tap(&auth, &d, KEY_LEFTSHIFT)
    });
    stage!(
        "key_chord_shift_right_ctrl",
        keyboard,
        Expect::Accept,
        |d| { eis.send_key_chord(&auth, &d, KEY_LEFTSHIFT, KEY_RIGHTCTRL) }
    );
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
        |d| eis.send_key_tap(&revoked, &d, KEY_LEFTSHIFT)
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
            Some(device) => eis.send_key_tap(&valid, &device, KEY_LEFTSHIFT),
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
    let observer = Observer::start(OBSERVER_PAGE)?;
    println!(
        "Rehearsal (no input is sent). Open in a browser, press F11:\n  {}",
        observer.url()
    );
    let ready = observer.wait_ready(
        Duration::from_secs(args.ready_timeout_secs),
        Duration::from_secs(args.settle_secs),
    );
    println!("focused and fullscreen for {} s: {ready}", args.settle_secs);
    if ready {
        println!(
            "tally reset acknowledged by the page: {}",
            observer.arm(Duration::from_secs(10))
        );
    }
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
        let observer = Observer::start(OBSERVER_PAGE)?;
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
             CreateSession; Start; ConnectToEIS; bind seat; Shift tap, Shift+Right Ctrl chord, pointer +40/-40, left \
             click, scroll 15; revoked-authorization tap (must be refused); Stop; valid-authorization tap \
             after Stop (delivery judged by the page); stale-path Stop call; Shell PID unchanged.",
            args.settle_secs
        ),
        expected: "Every injected stage accepted; the revoked stage refused as LeaseRevoked; observer tally: \
                   ShiftLeft 2/2, ControlRight 1/1 (with Shift held), one left click, net-zero pointer motion, positive scroll, no \
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
                {"type": "down", "code": "ShiftLeft", "shift": true},
                {"type": "up", "code": "ShiftLeft", "shift": false},
                {"type": "down", "code": "ShiftLeft", "shift": true},
                {"type": "down", "code": "ControlRight", "shift": true},
                {"type": "up", "code": "ControlRight", "shift": true},
                {"type": "up", "code": "ShiftLeft", "shift": false},
            ],
            "buttons": [
                {"type": "down", "button": 0}, {"type": "up", "button": 0}, {"type": "click", "button": 0},
            ],
            "pointer": {"moves": 2, "positions": [{"x": 540, "y": 400}, {"x": 500, "y": 400}]},
            "viewport": {"w": 1920, "h": 1080},
            "wheel": {"events": 1, "deltaY": 15, "scrollY": 15},
            "untrusted": 0,
        })
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
            .push(json!({"type": "down", "code": "ShiftLeft", "shift": true}));
        assert!(!evaluate_tally(&leaked).0);

        let mut drifted = good_tally();
        drifted["pointer"]["positions"][1]["x"] = json!(540);
        assert!(!evaluate_tally(&drifted).0);

        let mut repeated = good_tally();
        repeated["pointer"]["positions"]
            .as_array_mut()
            .expect("positions")
            .push(json!({"x": 500, "y": 400}));
        assert!(evaluate_tally(&repeated).0);

        // +40 clamped at the right edge still looks like a -40 return.
        drifted["pointer"]["positions"] = json!([{"x": 1919, "y": 400}, {"x": 1879, "y": 400}]);
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
}
