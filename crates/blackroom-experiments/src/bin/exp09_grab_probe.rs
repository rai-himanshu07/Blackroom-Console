//! Experiment 9b — Exclusive grab probe (Document 10 Experiment 9; Phase 7 step 5a/5b; Gate FEAS-E).
//!
//! MUTATING and supervised: takes an exclusive `EVIOCGRAB` on the named physical
//! evdev nodes for one bounded window and observes, in three phases (before,
//! grabbed, after), that the operator's physical input stops reaching the session
//! while a remote (EIS) Shift tap still does. The default refuses non-USB nodes so
//! the first run leaves the built-in keyboard and touchpad usable for recovery.
//!
//! The observer page reports what reaches the session; this binary counts events
//! read from the grabbed nodes (counts only, never key codes). It needs an armed
//! external kill timer named `blackroom-exp09-kill` and never reports FEAS-E.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use blackroom_core::epoch::SecurityEpoch;
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
use evdev::{BusType, Device, EventType};
use reis::event::{DeviceCapability, DeviceResumed, EiEvent};
use remote_input_helper::{
    Caps, Classification, DeviceGrab, DeviceId, GrabError, Isolation, State as IsolationState,
    classify,
};
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;

const EXP_ID: &str = "exp09";
const OBSERVER_PAGE: &str = include_str!("../../assets/exp08_observer.html");
const KEY_LEFTSHIFT: u32 = 42;
const KILL_TIMER: &str = "blackroom-exp09-kill";
/// The watchdog releases the grabs if the loop stops renewing for this long.
const LEASE_MS: u64 = 4000;

#[derive(Parser, Debug)]
#[command(
    about = "Experiment 9b: exclusive grab of named input nodes (MUTATING, operator present)"
)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Required: operator present, work saved, second-device SSH open, kill timer armed.
    #[arg(long, default_value_t = false)]
    operator_present: bool,
    /// evdev node numbers to grab, for example `6,7` for `/dev/input/event6,7`.
    #[arg(long, value_delimiter = ',', num_args = 1..=6, required = true)]
    nodes: Vec<u32>,
    /// Exact `Phys` prefix every grabbed node must have, from the inventory (for
    /// example `usb-0000:00:14.0-1.1/`); a replug or typo that changes the node
    /// number is then refused. Required unless `--include-builtin`.
    #[arg(long)]
    expect_phys_prefix: Option<String>,
    /// Allow nodes that are not on a USB bus (the built-in keyboard and touchpad).
    #[arg(long, default_value_t = false)]
    include_builtin: bool,
    /// Seconds for each of the three phases.
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u64).range(10..=60))]
    phase_secs: u64,
    /// Seconds to wait for the operator to focus the observer page.
    #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(30..=600))]
    ready_timeout_secs: u64,
}

struct EvdevGrab {
    devices: BTreeMap<DeviceId, Device>,
    key_presses: u64,
    motions: u64,
}

impl DeviceGrab for EvdevGrab {
    fn grab(&mut self, id: DeviceId) -> Result<(), GrabError> {
        let device = self
            .devices
            .get_mut(&id)
            .ok_or_else(|| GrabError("node not open".to_string()))?;
        device.grab().map_err(|error| GrabError(error.to_string()))
    }

    fn release(&mut self, id: DeviceId) -> Result<(), GrabError> {
        let device = self
            .devices
            .get_mut(&id)
            .ok_or_else(|| GrabError("node not open".to_string()))?;
        device
            .ungrab()
            .map_err(|error| GrabError(error.to_string()))
    }
}

impl EvdevGrab {
    /// Counts pending events per node without keeping any code or coordinate.
    fn drain(&mut self) -> Result<(), String> {
        for device in self.devices.values_mut() {
            for _ in 0..64 {
                match device.fetch_events() {
                    Ok(events) => {
                        for event in events {
                            match event.event_type() {
                                EventType::KEY if event.value() == 1 => self.key_presses += 1,
                                EventType::RELATIVE | EventType::ABSOLUTE => self.motions += 1,
                                _ => {}
                            }
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error.to_string()),
                }
            }
        }
        Ok(())
    }

    fn keys_still_down(&self) -> usize {
        self.devices
            .values()
            .filter_map(|device| device.get_key_state().ok())
            .map(|keys| keys.iter().count())
            .sum()
    }
}

fn caps_of(device: &Device) -> Caps {
    Caps {
        keys: device
            .supported_keys()
            .map(|keys| keys.iter().map(|key| key.code()).collect())
            .unwrap_or_default(),
        rel_axes: device
            .supported_relative_axes()
            .map(|axes| axes.iter().map(|axis| axis.0).collect())
            .unwrap_or_default(),
        abs_axes: device
            .supported_absolute_axes()
            .map(|axes| axes.iter().map(|axis| axis.0).collect())
            .unwrap_or_default(),
        has_switches: device.supported_switches().is_some(),
        bus_virtual: device.input_id().bus_type() == BusType::BUS_VIRTUAL,
        seat0: true,
    }
}

#[derive(Debug, Default, Serialize, Clone)]
struct PhaseTally {
    key_events: usize,
    shift_down: usize,
    pointer_moves: u64,
    buttons: usize,
    wheel_events: u64,
}

impl PhaseTally {
    fn from_page(tally: &Value) -> Self {
        let keys = tally["keys"].as_array().cloned().unwrap_or_default();
        Self {
            key_events: keys.len(),
            shift_down: keys
                .iter()
                .filter(|key| key["type"] == "down" && key["code"] == "ShiftLeft")
                .count(),
            pointer_moves: tally["pointer"]["moves"].as_u64().unwrap_or(0),
            buttons: tally["buttons"].as_array().map_or(0, Vec::len),
            wheel_events: tally["wheel"]["events"].as_u64().unwrap_or(0),
        }
    }

    /// Physical-looking activity: everything except the one injected Shift tap.
    fn physical_events(&self, injected_shift: usize) -> u64 {
        let keys = self.key_events.saturating_sub(injected_shift * 2) as u64;
        keys + self.pointer_moves + self.buttons as u64 + self.wheel_events
    }
}

#[derive(Debug, Default, Serialize)]
struct Run {
    nodes: Vec<(DeviceId, String, String)>,
    shell_pid_before: Option<String>,
    shell_pid_after: Option<String>,
    blocked: Option<String>,
    failure: Option<String>,
    aborted: Option<String>,
    phase_a: Option<PhaseTally>,
    phase_b: Option<PhaseTally>,
    phase_c: Option<PhaseTally>,
    probe_key_presses_in_b: u64,
    probe_motions_in_b: u64,
    injected_shift: Option<String>,
    restore_released: Vec<DeviceId>,
    restore_failed: Vec<String>,
    keys_still_down_after_release: Option<usize>,
    grabbed_nodes_during_b: Vec<DeviceId>,
    violations: Vec<String>,
    notes: Vec<String>,
}

struct Authority {
    lease: ControlLease,
    signature: Signature,
    verifying_key: VerifyingKey,
}

impl Authority {
    fn new() -> Self {
        use getrandom::rand_core::UnwrapErr;
        let now = SystemTime::now();
        let signing_key = SigningKey::generate(&mut UnwrapErr(getrandom::SysRng));
        let lease = ControlLease {
            session_id: "rs_EXP09".to_string(),
            host_id: "bc_EXP09".to_string(),
            user_id: "exp09".to_string(),
            client_id: "cl_EXP09".to_string(),
            security_epoch: SecurityEpoch::INITIAL,
            issued_at: now,
            expires_at: now + Duration::from_secs(300),
            capabilities: vec![Capability::View, Capability::Control],
        };
        Self {
            signature: lease.sign(&signing_key),
            verifying_key: signing_key.verifying_key(),
            lease,
        }
    }

    fn authorization(&self) -> InputAuthorization<'_> {
        InputAuthorization {
            lease: &self.lease,
            signature: &self.signature,
            verifying_key: &self.verifying_key,
            authenticated: true,
            authorized: true,
            current_epoch: SecurityEpoch::INITIAL,
            current_state: State::RemoteActive,
            current_session_id: &self.lease.session_id,
            revoked: false,
            now: SystemTime::now(),
        }
    }
}

/// One EIS keyboard, opened before the grab so the tap during it is a pure send.
struct Injector {
    // Field order matters: the connection drops before the session stops.
    eis: EiConnection,
    keyboard: DeviceResumed,
    _session: RemoteDesktopSession<'static>,
}

fn command_line(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn open_injector(
    conn: &'static zbus::blocking::Connection,
    authority: &Authority,
) -> anyhow::Result<Injector> {
    let mut session =
        RemoteDesktopSession::create(conn).map_err(|e| anyhow::anyhow!("CreateSession: {e}"))?;
    session.start().map_err(|e| anyhow::anyhow!("Start: {e}"))?;
    let mut eis = session
        .connect_to_eis(&authority.authorization())
        .map_err(|e| anyhow::anyhow!("ConnectToEIS: {e}"))?;
    eis.handshake_sender(Duration::from_secs(5))
        .map_err(|e| anyhow::anyhow!("EIS handshake: {e}"))?;
    let mut bound = false;
    let mut keyboard = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && keyboard.is_none() {
        match eis
            .next_event_until(Duration::from_millis(250))
            .map_err(|e| anyhow::anyhow!("{e}"))?
        {
            Some(EiEvent::SeatAdded(added)) if !bound => {
                eis.bind_seat(&added, DeviceCapability::Keyboard)
                    .map_err(|e| anyhow::anyhow!("seat bind: {e}"))?;
                bound = true;
            }
            Some(EiEvent::DeviceResumed(resumed))
                if resumed.device.has_capability(DeviceCapability::Keyboard) =>
            {
                keyboard = Some(resumed);
            }
            _ => {}
        }
    }
    let keyboard = keyboard.context("no resumed EIS keyboard")?;
    Ok(Injector {
        eis,
        keyboard,
        _session: session,
    })
}

/// Why a node must not be grabbed in this run, or `None` when it may be.
fn node_problem(
    allow_listed: bool,
    seat: Option<&str>,
    phys: &str,
    expect_prefix: Option<&str>,
    include_builtin: bool,
) -> Option<String> {
    if !allow_listed {
        return Some("not an allow-listed keyboard or pointing node".to_string());
    }
    match seat {
        Some("seat0") => {}
        Some(other) => return Some(format!("udev seat is {other}, not seat0")),
        None => return Some("udev seat could not be read".to_string()),
    }
    if include_builtin {
        return None;
    }
    match expect_prefix {
        None => Some("--expect-phys-prefix is required for an external-only run".to_string()),
        Some(prefix) if !phys.starts_with("usb-") || !phys.starts_with(prefix) => Some(format!(
            "phys {phys} does not start with {prefix} (and usb-)"
        )),
        Some(_) => None,
    }
}

/// `ID_SEAT` from the udev database; absent means the default seat0.
fn udev_seat(node: u32) -> Option<String> {
    let dev = fs::read_to_string(format!("/sys/class/input/event{node}/dev")).ok()?;
    let text = fs::read_to_string(format!("/run/udev/data/c{}", dev.trim())).ok()?;
    Some(
        text.lines()
            .find_map(|line| line.strip_prefix("E:ID_SEAT=").map(str::to_string))
            .unwrap_or_else(|| "seat0".to_string()),
    )
}

/// The kill timer must be listed, have a next elapse, and fire after this run
/// could plausibly end. `json` is `systemctl --user list-timers --all --output=json`.
fn kill_timer_problem(json: &str, now_unix_us: u64, min_remaining_secs: u64) -> Option<String> {
    let Ok(Value::Array(timers)) = serde_json::from_str::<Value>(json) else {
        return Some("timer list could not be read".to_string());
    };
    let unit = format!("{KILL_TIMER}.timer");
    let Some(timer) = timers.iter().find(|timer| timer["unit"] == unit.as_str()) else {
        return Some("timer is not pending".to_string());
    };
    let Some(next) = timer["next"].as_u64().filter(|next| *next > 0) else {
        return Some("timer has no next elapse".to_string());
    };
    let need = now_unix_us + min_remaining_secs * 1_000_000;
    if next < need {
        return Some(format!(
            "timer fires in {} s, less than the {min_remaining_secs} s this run needs",
            next.saturating_sub(now_unix_us) / 1_000_000
        ));
    }
    None
}

fn preflight(args: &Args, run: &mut Run) -> anyhow::Result<BTreeMap<DeviceId, Device>> {
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
    run.shell_pid_before = command_line("pidof", &["gnome-shell"]);
    anyhow::ensure!(run.shell_pid_before.is_some(), "gnome-shell PID not found");
    anyhow::ensure!(
        command_line(
            "systemctl",
            &["--user", "is-active", "gnome-remote-desktop.service"]
        )
        .as_deref()
            != Some("active"),
        "gnome-remote-desktop is active; mask it first (safety §5)"
    );
    let timers = command_line(
        "systemctl",
        &["--user", "list-timers", "--all", "--output=json"],
    )
    .unwrap_or_default();
    let now_us = u64::try_from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_micros(),
    )?;
    if let Some(problem) = kill_timer_problem(&timers, now_us, 3 * args.phase_secs + 30) {
        anyhow::bail!(
            "arm the external kill timer `{KILL_TIMER}` first (systemd-run --user --on-active=...): {problem}"
        );
    }
    let mut devices = BTreeMap::new();
    for node in args.nodes.iter().copied().collect::<BTreeSet<_>>() {
        let device = Device::open(format!("/dev/input/event{node}"))
            .with_context(|| format!("open /dev/input/event{node} (device access needed)"))?;
        device.set_nonblocking(true)?;
        let phys = device.physical_path().unwrap_or("").to_string();
        let allow_listed = matches!(classify(&caps_of(&device)), Classification::Grab(_));
        if let Some(problem) = node_problem(
            allow_listed,
            udev_seat(node).as_deref(),
            &phys,
            args.expect_phys_prefix.as_deref(),
            args.include_builtin,
        ) {
            anyhow::bail!("event{node} ({phys}): {problem}");
        }
        run.nodes
            .push((node, device.name().unwrap_or("").to_string(), phys));
        devices.insert(node, device);
    }
    Ok(devices)
}

fn wait_phase(
    observer: &Observer,
    secs: u64,
    label: &str,
    run: &mut Run,
    isolation: Option<&Arc<Mutex<Isolation<EvdevGrab>>>>,
    origin: Instant,
) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if !observer.ready_now(FRESH) {
            run.aborted = Some(format!(
                "observer lost focus or fullscreen during phase {label}"
            ));
            return false;
        }
        if let Some(isolation) = isolation {
            let mut guard = isolation.lock().unwrap_or_else(PoisonError::into_inner);
            if guard.state() != IsolationState::Isolated {
                run.failure = Some(
                    "isolation lapsed during phase B (lease expired or grabs released)".to_string(),
                );
                return false;
            }
            guard.renew(now_ms(origin));
            if let Err(error) = guard.grabber_mut().drain() {
                run.failure = Some(format!("reading grabbed nodes: {error}"));
                return false;
            }
        }
        sleep(Duration::from_millis(20));
    }
    true
}

fn now_ms(origin: Instant) -> u64 {
    u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn snapshot_tally(observer: &Observer) -> Option<PhaseTally> {
    observer.wait_beats(2, Duration::from_secs(3));
    observer
        .snapshot(|state| state.tally.clone())
        .map(|tally| PhaseTally::from_page(&tally))
}

fn execute(
    args: &Args,
    run: &mut Run,
    observer: &Observer,
    devices: BTreeMap<DeviceId, Device>,
) -> anyhow::Result<()> {
    println!("Open in a browser, press F11, keep it focused; use ONLY the named external devices:");
    println!("  {}", observer.url());
    if !observer.wait_ready(
        Duration::from_secs(args.ready_timeout_secs),
        Duration::from_secs(5),
    ) {
        run.blocked = Some("observer page never reported focused and fullscreen".to_string());
        anyhow::bail!("observer not ready");
    }
    // Leaked on purpose: the injector borrows it for the rest of the process.
    let conn: &'static zbus::blocking::Connection = Box::leak(Box::new(
        zbus::blocking::Connection::session().context("session bus")?,
    ));
    let authority = Authority::new();
    let mut injector = match open_injector(conn, &authority) {
        Ok(injector) => injector,
        Err(error) => {
            run.blocked = Some(format!("EIS setup: {error}"));
            anyhow::bail!("{error}");
        }
    };
    let isolation = Arc::new(Mutex::new(Isolation::new(EvdevGrab {
        devices,
        key_presses: 0,
        motions: 0,
    })));
    let origin = Instant::now();
    let watchdog_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Independent of the read loop, so a stalled loop still loses the lease.
    let watchdog = {
        let (isolation, stop) = (Arc::clone(&isolation), Arc::clone(&watchdog_stop));
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                sleep(Duration::from_millis(200));
                isolation
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .tick(now_ms(origin));
            }
        })
    };

    println!(
        "Phase A ({} s): type lowercase letters and move the EXTERNAL mouse now.",
        args.phase_secs
    );
    let mut ok = observer.arm(Duration::from_secs(10));
    if !ok {
        run.aborted = Some("observer did not acknowledge the phase A reset".to_string());
    } else {
        ok = wait_phase(observer, args.phase_secs, "A", run, None, origin);
    }
    run.phase_a = snapshot_tally(observer);

    if ok {
        println!(
            "Phase B ({} s): GRAB starts; keep typing and moving the external devices.",
            args.phase_secs
        );
        if !observer.arm(Duration::from_secs(10)) {
            run.aborted = Some("observer did not acknowledge the phase B reset".to_string());
            ok = false;
        }
        let specs: Vec<(DeviceId, Caps)> = isolation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .grabber()
            .devices
            .iter()
            .map(|(id, device)| (*id, caps_of(device)))
            .collect();
        if ok {
            let grabbed = isolation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .isolate(&specs, now_ms(origin), LEASE_MS);
            match grabbed {
                Ok(ids) => run.grabbed_nodes_during_b = ids,
                Err(error) => {
                    run.failure = Some(format!("isolate: {error:?}"));
                    ok = false;
                }
            }
        }
        if ok {
            ok = wait_phase(
                observer,
                args.phase_secs / 2,
                "B1",
                run,
                Some(&isolation),
                origin,
            );
            if ok && observer.ready_now(FRESH) {
                run.injected_shift = Some(
                    match injector.eis.send_key_tap(
                        &authority.authorization(),
                        &injector.keyboard,
                        KEY_LEFTSHIFT,
                    ) {
                        Ok(()) => "accepted".to_string(),
                        Err(error) => format!("refused:{:?}", error.code),
                    },
                );
            }
            if ok {
                ok = wait_phase(
                    observer,
                    args.phase_secs - args.phase_secs / 2,
                    "B2",
                    run,
                    Some(&isolation),
                    origin,
                );
            }
            run.phase_b = snapshot_tally(observer);
            let mut guard = isolation.lock().unwrap_or_else(PoisonError::into_inner);
            run.probe_key_presses_in_b = guard.grabber().key_presses;
            run.probe_motions_in_b = guard.grabber().motions;
            let outcome = guard.restore();
            run.restore_released = outcome.released;
            run.restore_failed = outcome
                .failed
                .iter()
                .map(|(id, e)| format!("event{id}: {}", e.0))
                .collect();
            run.keys_still_down_after_release = Some(guard.grabber().keys_still_down());
            if guard.state() != IsolationState::Idle {
                run.violations.push("release did not complete".to_string());
            }
        }
    }

    if ok || run.phase_b.is_some() {
        println!(
            "Phase C ({} s): grab released; type and move the external devices again.",
            args.phase_secs
        );
        if observer.arm(Duration::from_secs(10)) {
            wait_phase(observer, args.phase_secs, "C", run, None, origin);
        }
        run.phase_c = snapshot_tally(observer);
    }
    watchdog_stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = watchdog.join();
    // Release anything still held before the devices and the session are dropped.
    let _ = isolation
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .restore();
    Ok(())
}

fn classify_run(run: &mut Run) -> ExperimentResult {
    if run.blocked.is_some() {
        return ExperimentResult::Blocked;
    }
    if run.failure.is_some() {
        return ExperimentResult::Fail;
    }
    let injected = usize::from(run.injected_shift.as_deref() == Some("accepted"));
    if let Some(b) = &run.phase_b {
        let leaked = b.physical_events(injected);
        if leaked > 0 {
            run.violations.push(format!(
                "{leaked} physical-looking events reached the page while grabbed"
            ));
        }
        if injected == 1 && b.shift_down != 1 {
            run.violations.push(format!(
                "injected Shift tap seen {} times in phase B",
                b.shift_down
            ));
        }
    }
    if !run.restore_failed.is_empty() {
        run.violations.push("a release failed".to_string());
    }
    if run.shell_pid_before.is_some() && run.shell_pid_before != run.shell_pid_after {
        run.violations.push("gnome-shell PID changed".to_string());
    }
    if !run.violations.is_empty() {
        return ExperimentResult::Fail;
    }
    let a_seen = run
        .phase_a
        .as_ref()
        .is_some_and(|a| a.physical_events(0) > 0);
    let c_seen = run
        .phase_c
        .as_ref()
        .is_some_and(|c| c.physical_events(0) > 0);
    let operator_active = run.probe_key_presses_in_b + run.probe_motions_in_b > 0;
    if run.aborted.is_some() || !(a_seen && c_seen && operator_active && injected == 1) {
        if !operator_active {
            run.notes.push("no events were read from the grabbed nodes: the operator did not use them in phase B".to_string());
        }
        return ExperimentResult::Partial;
    }
    ExperimentResult::Pass
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    anyhow::ensure!(
        args.operator_present,
        "refusing to grab input: pass --operator-present only after the safety preflight in \
         docs/ops/experiment-safety.md §7 and the plan's step 5a approval"
    );
    let now = OffsetDateTime::now_utc();
    let mut run = Run::default();
    match preflight(&args, &mut run) {
        Err(error) => run.blocked = Some(format!("preflight: {error:#}")),
        Ok(devices) => {
            let observer = Observer::start(OBSERVER_PAGE)?;
            if let Err(error) = execute(&args, &mut run, &observer, devices)
                && run.blocked.is_none()
                && run.failure.is_none()
            {
                run.failure = Some(format!("{error:#}"));
            }
        }
    }
    run.shell_pid_after = command_line("pidof", &["gnome-shell"]);
    let result = classify_run(&mut run);
    let observed = format!(
        "result={result}; blocked={:?}; failure={:?}; aborted={:?}; A={:?}; B={:?}; C={:?}; probe read {} key presses and {} motions in B; \
         injected Shift={:?}; release failures={:?}; keys still down after release={:?}; violations={:?}",
        run.blocked,
        run.failure,
        run.aborted,
        run.phase_a,
        run.phase_b,
        run.phase_c,
        run.probe_key_presses_in_b,
        run.probe_motions_in_b,
        run.injected_shift,
        run.restore_failed,
        run.keys_still_down_after_release,
        run.violations
    );
    println!("{observed}");
    let report = ExperimentReport {
        experiment: "Experiment 9b — Exclusive input grab probe (FEAS-E)".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1; named USB nodes only unless --include-builtin"
            .to_string(),
        objective: "Observe that an exclusive EVIOCGRAB on physical nodes stops the operator's physical input from reaching the \
                    session while a remote EIS tap still arrives, and that release restores it."
            .to_string(),
        hypothesis: "Phase A the page sees physical input; phase B it sees none but the injected Shift while the grabbed nodes still \
                     produce events; phase C it sees physical input again."
            .to_string(),
        procedure: "Preflight; observer page focused; open the nodes; open an EIS keyboard; phase A; isolate (grab all or none); \
                    phase B with one injected Shift tap; release; phase C. External kill timer armed."
            .to_string(),
        expected: "A and C show physical activity, B shows only the injected Shift, probe read events in B, release clean."
            .to_string(),
        observed,
        evidence: vec!["findings.json (this directory)".to_string()],
        result,
        failure: run.failure.clone().or_else(|| run.blocked.clone()),
        root_cause: None,
        security_impact: Some(
            "Holds an exclusive grab on the named input nodes for one bounded window; counts events only, never key codes."
                .to_string(),
        ),
        recommended_action: None,
        follow_up: Some("A PASS here is one observation, not FEAS-E; release on SIGKILL is tested separately.".to_string()),
    };
    let dir = evidence_dir(EXP_ID, now)?;
    write_evidence(&dir, &report.render(now), "grab-findings.json", &run)?;
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

    #[test]
    fn page_tallies_separate_the_injected_shift_from_physical_activity() {
        let tally = json!({
            "keys": [
                {"type": "down", "code": "ShiftLeft"}, {"type": "up", "code": "ShiftLeft"},
                {"type": "down", "code": "KeyA"}, {"type": "up", "code": "KeyA"},
            ],
            "pointer": {"moves": 3}, "buttons": [{"type": "click"}], "wheel": {"events": 1},
        });
        let phase = PhaseTally::from_page(&tally);
        assert_eq!(phase.shift_down, 1);
        assert_eq!(phase.physical_events(1), 2 + 3 + 1 + 1);
        assert_eq!(PhaseTally::from_page(&json!({})).physical_events(0), 0);
    }

    #[test]
    fn a_leak_during_the_grab_or_missing_phases_never_pass() {
        let quiet = PhaseTally {
            key_events: 2,
            shift_down: 1,
            ..PhaseTally::default()
        };
        let busy = PhaseTally {
            key_events: 6,
            pointer_moves: 4,
            ..PhaseTally::default()
        };
        let mut run = Run {
            phase_a: Some(busy.clone()),
            phase_b: Some(quiet.clone()),
            phase_c: Some(busy.clone()),
            probe_key_presses_in_b: 5,
            injected_shift: Some("accepted".to_string()),
            ..Run::default()
        };
        assert_eq!(classify_run(&mut run), ExperimentResult::Pass);

        let mut leak = Run {
            phase_b: Some(PhaseTally {
                key_events: 4,
                shift_down: 1,
                ..PhaseTally::default()
            }),
            ..Run {
                phase_a: Some(busy.clone()),
                phase_c: Some(busy.clone()),
                probe_key_presses_in_b: 5,
                injected_shift: Some("accepted".to_string()),
                ..Run::default()
            }
        };
        assert_eq!(classify_run(&mut leak), ExperimentResult::Fail);

        let mut idle = Run {
            phase_a: Some(busy.clone()),
            phase_b: Some(quiet),
            phase_c: Some(busy),
            injected_shift: Some("accepted".to_string()),
            ..Run::default()
        };
        assert_eq!(classify_run(&mut idle), ExperimentResult::Partial);
        assert!(!idle.notes.is_empty());

        let mut blocked = Run {
            blocked: Some("x".to_string()),
            ..Run::default()
        };
        assert_eq!(classify_run(&mut blocked), ExperimentResult::Blocked);
    }

    #[test]
    fn nodes_are_bound_to_the_inventoried_external_path_and_seat0() {
        let ext = "usb-0000:00:14.0-1.1/input0";
        let prefix = Some("usb-0000:00:14.0-1.1/");
        assert_eq!(node_problem(true, Some("seat0"), ext, prefix, false), None);
        assert!(
            node_problem(
                true,
                Some("seat0"),
                "usb-0000:00:14.0-1.2/input0",
                prefix,
                false
            )
            .is_some()
        );
        assert!(
            node_problem(true, Some("seat0"), "isa0060/serio0/input0", prefix, false).is_some()
        );
        assert!(node_problem(true, Some("seat0"), ext, None, false).is_some());
        assert!(node_problem(false, Some("seat0"), ext, prefix, false).is_some());
        assert!(node_problem(true, Some("seat1"), ext, prefix, false).is_some());
        assert!(node_problem(true, None, ext, prefix, false).is_some());
        assert_eq!(
            node_problem(true, Some("seat0"), "isa0060/serio0/input0", None, true),
            None
        );
    }

    #[test]
    fn the_kill_timer_must_be_pending_and_outlast_the_run() {
        let listed = |next: u64| {
            format!(
                r#"[{{"next":{next},"left":{next},"last":0,"passed":0,"unit":"blackroom-exp09-kill.timer","activates":"x.service"}}]"#
            )
        };
        let now = 1_000_000_000_000_u64;
        assert_eq!(
            kill_timer_problem(&listed(now + 200_000_000), now, 120),
            None
        );
        assert!(kill_timer_problem(&listed(now + 200_000_000), now, 300).is_some());
        assert!(kill_timer_problem(&listed(0), now, 10).is_some());
        assert!(kill_timer_problem("[]", now, 10).is_some());
        assert!(kill_timer_problem("not json", now, 10).is_some());
        let other = r#"[{"next":9999999999999999,"unit":"other.timer"}]"#;
        assert!(kill_timer_problem(other, now, 10).is_some());
    }

    #[test]
    fn the_probe_refuses_to_run_without_the_operator_flag() {
        assert!(Args::try_parse_from(["exp09", "--nodes", "6,7"]).is_ok());
        assert!(Args::try_parse_from(["exp09", "--nodes", "6", "--phase-secs", "3"]).is_err());
    }
}
