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
use std::io::{BufRead, BufReader, ErrorKind, Read};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
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
    Caps, Chord, ChordDetector, Classification, DeviceGrab, DeviceId, GrabError, Isolation,
    State as IsolationState, classify,
};
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;

const EXP_ID: &str = "exp09";
const OBSERVER_PAGE: &str = include_str!("../../assets/exp08_observer.html");
const KEY_LEFTSHIFT: u32 = 42;
const KILL_TIMER: &str = "blackroom-exp09-kill";
/// The kernel truncates process names to 15 bytes, so `pkill -x` must use this, not the binary name.
const KILL_COMM: &str = "exp09_grab_prob";
/// The watchdog releases the grabs if the loop stops renewing for this long.
const LEASE_MS: u64 = 4000;
const HOLD_SECS: u64 = 10;
const FOCUS_RECOVERY: Duration = Duration::from_secs(30);
const AFTER_SECS: u64 = 10;
const CHORD_WAIT_SECS: u64 = 40;
/// The stalled helper runs under this name (a symlink), so only its own timer can kill it.
const STALL_COMM: &str = "exp09_grab_hold";
const STALL_TIMER: &str = "blackroom-exp09-stall-kill";
const STALL_KILL_SECS: u64 = 24;
/// Experiment chord, not a product decision: Left Ctrl, Left Shift, Left Alt and Esc held 2 s (keys every laptop has).
const EMERGENCY_CHORD_KEYS: [u16; 4] = [29, 42, 56, 1];
const EMERGENCY_CHORD_HOLD_MS: u64 = 2000;

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
    /// Also run the SIGKILL, stalled-helper and (with --builtin-nodes) emergency-chord stages.
    #[arg(long, default_value_t = false)]
    full: bool,
    /// Built-in keyboard and touchpad nodes for the last stage, for example `2,3,4,5`.
    #[arg(long, value_delimiter = ',', num_args = 1..=6)]
    builtin_nodes: Vec<u32>,
    /// Run only the built-in stage (needs --builtin-nodes); for repeating it after the others passed.
    #[arg(long, default_value_t = false)]
    chord_only: bool,
    /// Internal: run as the separate process that holds the grab.
    #[arg(long, hide = true, default_value_t = false)]
    hold_grab: bool,
    /// Internal: the helper exits by itself after this many seconds.
    #[arg(long, hide = true, default_value_t = 60)]
    max_secs: u64,
    /// Internal: the helper releases when the emergency chord is held.
    #[arg(long, hide = true, default_value_t = false)]
    chord: bool,
    /// Internal: only keys from these nodes count towards the chord (the built-in keyboard).
    #[arg(long, hide = true, value_delimiter = ',')]
    chord_nodes: Vec<u32>,
}

struct EvdevGrab {
    devices: BTreeMap<DeviceId, Device>,
    key_presses: u64,
    motions: u64,
    per_node: BTreeMap<DeviceId, u64>,
    chord: Option<ChordDetector>,
    chord_nodes: BTreeSet<DeviceId>,
    chord_fired: bool,
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
    fn new(devices: BTreeMap<DeviceId, Device>, chord: Option<ChordDetector>) -> Self {
        Self {
            devices,
            key_presses: 0,
            motions: 0,
            per_node: BTreeMap::new(),
            chord,
            chord_nodes: BTreeSet::new(),
            chord_fired: false,
        }
    }

    /// Counts pending events per node without keeping any code or coordinate;
    /// only the emergency chord keys are tracked, and only when a chord is set.
    fn drain(&mut self, now_ms: u64) -> Result<(), String> {
        for (id, device) in &mut self.devices {
            for _ in 0..64 {
                match device.fetch_events() {
                    Ok(events) => {
                        for event in events {
                            match event.event_type() {
                                EventType::KEY => {
                                    if event.value() == 1 {
                                        self.key_presses += 1;
                                        *self.per_node.entry(*id).or_default() += 1;
                                    }
                                    if let Some(chord) = &mut self.chord
                                        && event.value() != 2
                                        && self.chord_nodes.contains(id)
                                    {
                                        chord.key(event.code(), event.value() == 1, now_ms);
                                    }
                                }
                                EventType::RELATIVE | EventType::ABSOLUTE => {
                                    self.motions += 1;
                                    *self.per_node.entry(*id).or_default() += 1;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error.to_string()),
                }
            }
        }
        if let Some(chord) = &mut self.chord
            && chord.poll(now_ms)
        {
            self.chord_fired = true;
        }
        Ok(())
    }

    /// Forgets counts so far; the kernel queue is drained first so earlier events are not attributed to a later phase.
    fn reset_counts(&mut self) {
        self.key_presses = 0;
        self.motions = 0;
        self.per_node.clear();
    }

    /// Per-node event counts, plus how many chord keys are down when a chord is set.
    fn counts(&self) -> String {
        let mut parts: Vec<String> = self
            .per_node
            .iter()
            .map(|(id, n)| format!("e{id}={n}"))
            .collect();
        if let Some(chord) = &self.chord {
            parts.push(format!("chord={}", chord.down_count()));
        }
        parts.join(" ")
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
    key_downs: usize,
    /// Down (d), repeat (r) and up (u) letters in order, without key codes.
    key_shape: String,
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
            key_downs: keys.iter().filter(|key| key["type"] == "down").count(),
            key_shape: keys
                .iter()
                .take(40)
                .map(
                    |key| match (key["type"].as_str(), key["repeat"].as_bool()) {
                        (Some("up"), _) => 'u',
                        (_, Some(true)) => 'r',
                        _ => 'd',
                    },
                )
                .collect(),
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
    phase_c0: Option<PhaseTally>,
    phase_c: Option<PhaseTally>,
    c0_device_events: u64,
    chord_only: bool,
    /// Page focus and fullscreen changes (times and flags only), to diagnose a lost page.
    focus_history: Vec<String>,
    stages_requested: usize,
    stages: Vec<StageResult>,
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

#[derive(Debug, Default, Serialize)]
struct StageResult {
    name: String,
    held: Option<PhaseTally>,
    after: Option<PhaseTally>,
    /// Events the un-grabbed devices produced in the after window (told apart from an idle operator).
    after_device_events: u64,
    helper_reads: String,
    helper_reads_end: String,
    chord_keys_max: usize,
    helper_end: String,
    seconds_to_end: Option<u64>,
    key_polls: u32,
    failures: Vec<String>,
    gaps: Vec<String>,
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

/// The timer must run `pkill -KILL -x <comm>`; `show` is
/// `systemctl --user --no-pager show <unit>.service -p ExecStart`.
fn kill_command_problem(show: &str, comm: &str) -> Option<String> {
    let argv = show
        .split("argv[]=")
        .nth(1)
        .and_then(|rest| rest.split(" ; ").next())
        .unwrap_or("");
    let tokens: Vec<&str> = argv.split_whitespace().collect();
    match tokens.as_slice() {
        [program, "-KILL", "-x", name] if program.ends_with("pkill") && *name == comm => None,
        _ => Some(format!(
            "the timer must run `pkill -KILL -x {comm}` (15-character process name), not `{argv}`"
        )),
    }
}

/// A built-in node must pass the ordinary checks and must not be a USB node.
fn builtin_problem(allow_listed: bool, seat: Option<&str>, phys: &str) -> Option<String> {
    node_problem(allow_listed, seat, phys, None, true).or_else(|| {
        phys.starts_with("usb-")
            .then(|| format!("phys {phys} is a USB node, not a built-in one"))
    })
}

/// `show` is `systemctl --user --no-pager show <unit>.timer -p AccuracyUSec`. systemd defaults to
/// 1 min, which lets a timer fire that much late.
fn accuracy_problem(show: &str) -> Option<String> {
    let value = show
        .lines()
        .find_map(|line| line.strip_prefix("AccuracyUSec="))
        .unwrap_or("")
        .trim();
    let split = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let ok = match (number.parse::<u64>(), unit) {
        (Ok(_), "us" | "ms") => true,
        (Ok(seconds), "s") => seconds <= 5,
        _ => false,
    };
    (!ok).then(|| {
        format!(
            "timer accuracy is `{value}`, so it can fire that late (arm it with --timer-property=AccuracySec=1s)"
        )
    })
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
    anyhow::ensure!(
        !args.chord_only || !args.builtin_nodes.is_empty(),
        "--chord-only needs --builtin-nodes"
    );
    let comm = comm_of(std::process::id());
    anyhow::ensure!(
        comm.as_deref() == Some(KILL_COMM),
        "this process is named {comm:?}; the external kill timer only matches `{KILL_COMM}`"
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
    let need_secs = if args.chord_only {
        120
    } else if args.full {
        240
    } else {
        3 * args.phase_secs + 30
    };
    if let Some(problem) = kill_timer_problem(&timers, now_us, need_secs) {
        anyhow::bail!(
            "arm the external kill timer `{KILL_TIMER}` first (systemd-run --user --on-active=...): {problem}"
        );
    }
    let exec = command_line(
        "systemctl",
        &[
            "--user",
            "--no-pager",
            "show",
            &format!("{KILL_TIMER}.service"),
            "-p",
            "ExecStart",
        ],
    )
    .unwrap_or_default();
    if let Some(problem) = kill_command_problem(&exec, KILL_COMM) {
        anyhow::bail!("external kill timer `{KILL_TIMER}` is not a working kill switch: {problem}");
    }
    let accuracy = command_line(
        "systemctl",
        &[
            "--user",
            "--no-pager",
            "show",
            &format!("{KILL_TIMER}.timer"),
            "-p",
            "AccuracyUSec",
        ],
    )
    .unwrap_or_default();
    if let Some(problem) = accuracy_problem(&accuracy) {
        anyhow::bail!("external kill timer `{KILL_TIMER}` is not precise enough: {problem}");
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
    let mut builtin_keys: BTreeSet<u16> = BTreeSet::new();
    for node in args.builtin_nodes.iter().copied().collect::<BTreeSet<_>>() {
        anyhow::ensure!(
            !devices.contains_key(&node),
            "event{node} is in both --nodes and --builtin-nodes"
        );
        let device = Device::open(format!("/dev/input/event{node}"))
            .with_context(|| format!("open /dev/input/event{node} (device access needed)"))?;
        let phys = device.physical_path().unwrap_or("").to_string();
        builtin_keys.extend(caps_of(&device).keys.iter().copied());
        let allow_listed = matches!(classify(&caps_of(&device)), Classification::Grab(_));
        if let Some(problem) = builtin_problem(allow_listed, udev_seat(node).as_deref(), &phys) {
            anyhow::bail!("event{node} ({phys}): {problem}");
        }
        run.nodes
            .push((node, device.name().unwrap_or("").to_string(), phys));
    }
    anyhow::ensure!(
        args.builtin_nodes.is_empty()
            || EMERGENCY_CHORD_KEYS
                .iter()
                .all(|key| builtin_keys.contains(key)),
        "the built-in nodes lack a key of the emergency chord, so it could not release them"
    );
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
    let mut end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if !observer.ready_now(FRESH) {
            // Only un-grabbed phases can be recovered: with a grab held the operator cannot refocus.
            if isolation.is_none() && recover_focus(observer, run, label, FOCUS_RECOVERY) {
                end = Instant::now() + Duration::from_secs(secs);
                continue;
            }
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
            if let Err(error) = guard.grabber_mut().drain(now_ms(origin)) {
                run.failure = Some(format!("reading grabbed nodes: {error}"));
                return false;
            }
        }
        sleep(Duration::from_millis(20));
    }
    true
}

/// Shows an instruction in the terminal and on the observer page, which is all the operator can see.
/// Waits for the page to be focused and fullscreen again after the operator lost it in a window
/// with no grab held, then clears the tally so the caller can restart the window. The earlier
/// prompt is put back either way.
fn recover_focus(observer: &Observer, run: &mut Run, label: &str, timeout: Duration) -> bool {
    let previous = observer.prompt();
    announce(
        observer,
        "The page lost focus or fullscreen. Click once in the middle of the page and press F11 if it is not fullscreen. Keep the mouse away from the screen edges. This step restarts.",
    );
    let ok = observer.wait_ready(timeout, Duration::from_secs(2))
        && observer.arm(Duration::from_secs(10));
    observer.set_prompt(&previous);
    if ok {
        run.notes
            .push(format!("the page lost focus during {label}; it restarted"));
    }
    ok
}

fn announce(observer: &Observer, text: &str) {
    println!("{text}");
    observer.set_prompt(text);
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
    println!("Emergency from SSH: pkill -KILL -f exp09_grab_");
    println!("Open in a browser, press F11, keep it focused; use ONLY the named external devices:");
    println!("  {}", observer.url());
    if !observer.wait_ready(
        Duration::from_secs(args.ready_timeout_secs),
        Duration::from_secs(5),
    ) {
        run.blocked = Some("observer page never reported focused and fullscreen".to_string());
        anyhow::bail!("observer not ready");
    }
    if args.chord_only {
        let isolation = Arc::new(Mutex::new(Isolation::new(EvdevGrab::new(devices, None))));
        run_stages(args, run, observer, &isolation)?;
        announce(
            observer,
            "Run finished. You can leave fullscreen (F11) and close this page.",
        );
        sleep(Duration::from_millis(1200));
        return Ok(());
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
    let isolation = Arc::new(Mutex::new(Isolation::new(EvdevGrab::new(devices, None))));
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

    announce(
        observer,
        &format!(
            "Phase A ({} s): with the EXTERNAL keyboard type lowercase letters, AND move the EXTERNAL mouse around the MIDDLE of the screen. Do not click, keep away from the screen edges, and do not press Esc or F11.",
            args.phase_secs
        ),
    );
    let mut ok = observer.arm(Duration::from_secs(10));
    if !ok {
        run.aborted = Some("observer did not acknowledge the phase A reset".to_string());
    } else {
        ok = wait_phase(observer, args.phase_secs, "A", run, None, origin);
    }
    run.phase_a = snapshot_tally(observer);

    if ok {
        announce(
            observer,
            "Get ready: LIFT ALL fingers off the keyboard and mouse buttons now. The grab starts in a moment.",
        );
        if !wait_until_released(
            || {
                isolation
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .grabber()
                    .keys_still_down()
            },
            Duration::from_secs(15),
            Duration::from_millis(500),
        ) {
            run.aborted =
                Some("a key or mouse button stayed down, so the grab was not started".to_string());
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
            {
                let mut guard = isolation.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = guard.grabber_mut().drain(now_ms(origin));
                guard.grabber_mut().reset_counts();
            }
            let grabbed = isolation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .isolate(&specs, now_ms(origin), LEASE_MS);
            match grabbed {
                Ok(ids) => {
                    run.grabbed_nodes_during_b = ids;
                    announce(
                        observer,
                        &format!(
                            "Phase B ({} s): the grab is ON. Keep typing letters on the EXTERNAL keyboard and moving the EXTERNAL mouse. Nothing should reach this page.",
                            args.phase_secs
                        ),
                    );
                    // Armed after the grab so keys pressed just before it cannot count as leaks.
                    if !observer.arm(Duration::from_secs(10)) {
                        run.aborted =
                            Some("observer did not acknowledge the phase B reset".to_string());
                        let _ = isolation
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .restore();
                        ok = false;
                    }
                }
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
        announce(
            observer,
            "Phase C0 (3 s): grab released. HANDS OFF: do not touch the keyboard, mouse wheel or touchpad.",
        );
        sleep(Duration::from_millis(1500));
        let mut builtin_counter = open_counter(&args.builtin_nodes);
        if observer.arm(Duration::from_secs(10)) {
            run.c0_device_events =
                quiet_window(observer, 3, run, &isolation, builtin_counter.as_mut());
            run.phase_c0 = snapshot_tally(observer);
        }
        announce(
            observer,
            &format!(
                "Phase C ({} s): grab released. Type letters on the EXTERNAL keyboard AND move the EXTERNAL mouse around the MIDDLE of the screen again. Do not click.",
                args.phase_secs
            ),
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
    if args.full {
        run_stages(args, run, observer, &isolation)?;
    }
    announce(
        observer,
        "Run finished. You can leave fullscreen (F11) and close this page.",
    );
    sleep(Duration::from_millis(1200));
    Ok(())
}

fn run_stages(
    args: &Args,
    run: &mut Run,
    observer: &Observer,
    isolation: &Arc<Mutex<Isolation<EvdevGrab>>>,
) -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("current_exe")?;
    let mut go = args.chord_only || can_continue(run);
    for (index, min_secs) in [(2, 200_u64), (3, 130), (4, 90)] {
        if args.chord_only && index < 4 {
            continue;
        }
        if !go || (index == 4 && args.builtin_nodes.is_empty()) {
            break;
        }
        if let Some(problem) = timer_left_problem(min_secs) {
            run.notes
                .push(format!("stage {index} and later skipped: {problem}"));
            break;
        }
        let stage = match index {
            2 => stage_sigkill(args, run, observer, &exe, isolation),
            3 => stage_stall(args, run, observer, &exe, isolation),
            _ => stage_chord(args, run, observer, &exe),
        };
        go = stage.failures.is_empty() && stage.gaps.is_empty() && run.aborted.is_none();
        run.stages.push(stage);
    }
    if run.stages.len() < run.stages_requested {
        run.notes
            .push("later stages were skipped because an earlier stage did not pass".to_string());
    }
    Ok(())
}

/// Stage 1 must fully pass before the riskier stages start.
fn can_continue(run: &Run) -> bool {
    run.failure.is_none()
        && run.aborted.is_none()
        && run.restore_failed.is_empty()
        && run.injected_shift.as_deref() == Some("accepted")
        && run.probe_key_presses_in_b > 0
        && run.probe_motions_in_b > 0
        && run.phase_a.as_ref().is_some_and(baseline_ok)
        && run
            .phase_b
            .as_ref()
            .is_some_and(|b| b.physical_events(1) == 0 && b.key_downs == 1)
        && run
            .phase_c0
            .as_ref()
            .is_some_and(|c0| c0.physical_events(0) == 0 || run.c0_device_events > 0)
        && run
            .phase_c
            .as_ref()
            .is_some_and(|c| c.physical_events(0) > 0)
}

/// The main kill timer is re-checked before each stage: it must still have this long to run.
fn timer_left_problem(min_secs: u64) -> Option<String> {
    let timers = command_line(
        "systemctl",
        &["--user", "list-timers", "--all", "--output=json"],
    )
    .unwrap_or_default();
    let Ok(now) = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) else {
        return Some("clock is before the epoch".to_string());
    };
    let now_us = u64::try_from(now.as_micros()).unwrap_or(u64::MAX);
    kill_timer_problem(&timers, now_us, min_secs)
        .map(|problem| format!("external kill timer: {problem}"))
}

fn emergency_chord() -> Option<ChordDetector> {
    Chord::new(&EMERGENCY_CHORD_KEYS, EMERGENCY_CHORD_HOLD_MS).map(ChordDetector::new)
}

fn describe_exit(status: ExitStatus) -> String {
    status.signal().map_or_else(
        || format!("exit {}", status.code().unwrap_or(-1)),
        |signal| format!("signal {signal}"),
    )
}

fn comm_of(pid: u32) -> Option<String> {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|text| text.trim().to_string())
}

/// State letter from `/proc/<pid>/stat`; the process name may contain spaces and parentheses.
fn parse_proc_state(stat: &str) -> Option<char> {
    stat.rsplit_once(')')?.1.trim_start().chars().next()
}

fn wait_state(pid: u32, wanted: char, timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        let state = fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| parse_proc_state(&stat));
        if state == Some(wanted) {
            return true;
        }
        sleep(Duration::from_millis(50));
    }
    false
}

/// Chord keys down, from a helper `READ ... chord=3` line.
fn parse_chord(line: &str) -> Option<usize> {
    line.split_whitespace()
        .find_map(|item| item.strip_prefix("chord=")?.parse().ok())
}

/// Per-node event counts from a helper `READ e6=12 e7=300` line.
fn parse_reads(line: &str) -> BTreeMap<DeviceId, u64> {
    line.split_whitespace()
        .skip(1)
        .filter_map(|item| {
            let (node, count) = item.strip_prefix('e')?.split_once('=')?;
            Some((node.parse().ok()?, count.parse().ok()?))
        })
        .collect()
}

/// Events the helper read between two `READ` lines, optionally only on some nodes.
fn reads_delta(
    end: &BTreeMap<DeviceId, u64>,
    base: &BTreeMap<DeviceId, u64>,
    only: Option<&[u32]>,
) -> u64 {
    end.iter()
        .filter(|(id, _)| only.is_none_or(|nodes| nodes.contains(id)))
        .map(|(id, count)| count.saturating_sub(base.get(id).copied().unwrap_or(0)))
        .sum()
}

fn helper_args(args: &Args, nodes: &[u32], builtin: bool, max_secs: u64) -> Vec<String> {
    let join = |list: &[u32]| {
        list.iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut out = vec![
        "--operator-present".to_string(),
        "--hold-grab".to_string(),
        "--nodes".to_string(),
        join(nodes),
        "--max-secs".to_string(),
        max_secs.to_string(),
    ];
    if builtin {
        out.push("--include-builtin".to_string());
        out.push("--chord".to_string());
        out.push("--chord-nodes".to_string());
        out.push(join(&args.builtin_nodes));
    } else if let Some(prefix) = &args.expect_phys_prefix {
        out.push("--expect-phys-prefix".to_string());
        out.push(prefix.clone());
    }
    out
}

/// The separate process that holds the grab (the shape of the real emergency binary). It exits,
/// releasing the grab, on the chord, when its parent goes away, or after `--max-secs`.
fn hold_main(args: &Args) -> anyhow::Result<()> {
    let mut devices = BTreeMap::new();
    for node in args.nodes.iter().copied().collect::<BTreeSet<_>>() {
        let device = Device::open(format!("/dev/input/event{node}"))
            .with_context(|| format!("open /dev/input/event{node}"))?;
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
        devices.insert(node, device);
    }
    let chord = if args.chord { emergency_chord() } else { None };
    let mut grab = EvdevGrab::new(devices, chord);
    grab.chord_nodes = args.chord_nodes.iter().copied().collect();
    if !wait_until_released(
        || grab.keys_still_down(),
        Duration::from_secs(20),
        Duration::from_millis(500),
    ) {
        println!("KEYS-HELD");
        anyhow::bail!("a key or button is held down; not grabbing");
    }
    let mut isolation = Isolation::new(grab);
    let specs: Vec<(DeviceId, Caps)> = isolation
        .grabber()
        .devices
        .iter()
        .map(|(id, device)| (*id, caps_of(device)))
        .collect();
    let origin = Instant::now();
    isolation
        .isolate(&specs, 0, LEASE_MS)
        .map_err(|error| anyhow::anyhow!("isolate: {error:?}"))?;
    println!("GRABBED");
    let parent_gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let flag = Arc::clone(&parent_gone);
        std::thread::spawn(move || {
            let _ = std::io::stdin().read_to_end(&mut Vec::new());
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
    }
    let mut last_report = Instant::now();
    let reason = loop {
        let now = now_ms(origin);
        isolation.renew(now);
        if isolation.grabber_mut().drain(now).is_err() {
            break "read-error";
        }
        if isolation.grabber().chord_fired {
            break "chord";
        }
        if parent_gone.load(std::sync::atomic::Ordering::Relaxed) {
            break "parent-gone";
        }
        if origin.elapsed() >= Duration::from_secs(args.max_secs) {
            break "deadline";
        }
        if last_report.elapsed() >= Duration::from_secs(1) {
            println!("READ {}", isolation.grabber().counts());
            last_report = Instant::now();
        }
        sleep(Duration::from_millis(20));
    };
    let outcome = isolation.restore();
    println!("RELEASED {reason} clean={}", outcome.is_clean());
    Ok(())
}

struct Helper {
    child: Child,
    lines: mpsc::Receiver<String>,
    last_read: String,
    ended: Option<String>,
    grabbed: bool,
    keys_held: bool,
    /// Most chord keys the helper saw down at once (from its `chord=N` report).
    chord_max: usize,
}

impl Helper {
    fn spawn(program: &Path, args: &[String]) -> anyhow::Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("spawn {}", program.display()))?;
        let stdout = child.stdout.take().context("helper stdout")?;
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            lines,
            last_read: String::new(),
            ended: None,
            grabbed: false,
            keys_held: false,
            chord_max: 0,
        })
    }

    fn note(&mut self, line: String) {
        if line == "KEYS-HELD" {
            self.keys_held = true;
        } else if line == "GRABBED" {
            self.grabbed = true;
        } else if line.starts_with("READ") {
            self.chord_max = self.chord_max.max(parse_chord(&line).unwrap_or(0));
            self.last_read = line;
        } else if line.starts_with("RELEASED") {
            self.ended = Some(line);
        }
    }

    fn pump(&mut self) {
        while let Ok(line) = self.lines.try_recv() {
            self.note(line);
        }
    }

    fn wait_grabbed(&mut self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        while Instant::now() < end && !self.grabbed {
            match self.lines.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => self.note(line),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        self.grabbed
    }

    /// Reads what the helper printed before its pipe closed.
    fn finish(&mut self) {
        while let Ok(line) = self.lines.recv_timeout(Duration::from_secs(2)) {
            self.note(line);
        }
    }

    fn exited(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A symlink to this binary named `STALL_COMM`, so the process name differs from the main timer's.
fn hold_link(exe: &Path) -> anyhow::Result<(TempDir, PathBuf)> {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = TempDir(base.join(format!("blackroom-exp09-{}", std::process::id())));
    fs::create_dir_all(&dir.0)?;
    let link = dir.0.join(STALL_COMM);
    std::os::unix::fs::symlink(exe, &link)?;
    Ok((dir, link))
}

/// Armed by the probe before it stops the helper; stopped again on drop.
struct StallTimer;

impl StallTimer {
    fn arm() -> Result<Self, String> {
        let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
        let output = Command::new("systemd-run")
            .args([
                "--user",
                "--collect",
                "--timer-property=AccuracySec=1s",
                &format!("--on-active={STALL_KILL_SECS}"),
                &format!("--unit={STALL_TIMER}"),
                &format!("--working-directory={}", cwd.display()),
                "pkill",
                "-KILL",
                "-x",
                STALL_COMM,
            ])
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "systemd-run failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let timer = Self;
        let exec = command_line(
            "systemctl",
            &[
                "--user",
                "--no-pager",
                "show",
                &format!("{STALL_TIMER}.service"),
                "-p",
                "ExecStart",
            ],
        )
        .unwrap_or_default();
        let accuracy = command_line(
            "systemctl",
            &[
                "--user",
                "--no-pager",
                "show",
                &format!("{STALL_TIMER}.timer"),
                "-p",
                "AccuracyUSec",
            ],
        )
        .unwrap_or_default();
        match kill_command_problem(&exec, STALL_COMM).or_else(|| accuracy_problem(&accuracy)) {
            Some(problem) => Err(problem),
            None => Ok(timer),
        }
    }
}

impl Drop for StallTimer {
    fn drop(&mut self) {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", &format!("{STALL_TIMER}.timer")])
            .status();
    }
}

/// Waits `secs` while the page stays focused. `polls` counts samples with a key or button held
/// down, but only after one sample with nothing held (so a key already down does not count).
fn watch(
    observer: &Observer,
    secs: u64,
    label: &str,
    run: &mut Run,
    mut helper: Option<&mut Helper>,
    isolation: Option<&Arc<Mutex<Isolation<EvdevGrab>>>>,
    mut polls: Option<&mut u32>,
) -> Result<(), String> {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut saw_up = false;
    while Instant::now() < end {
        if !observer.ready_now(FRESH) {
            let message = format!("observer lost focus or fullscreen during {label}");
            run.aborted = Some(message.clone());
            return Err(message);
        }
        if let Some(helper) = helper.as_mut() {
            helper.pump();
            if let Some(status) = helper.exited() {
                return Err(format!(
                    "helper ended early during {label}: {}",
                    describe_exit(status)
                ));
            }
        }
        if let (Some(isolation), Some(count)) = (isolation, polls.as_mut()) {
            let down = isolation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .grabber()
                .keys_still_down()
                > 0;
            if !down {
                saw_up = true;
            } else if saw_up {
                **count += 1;
            }
        }
        sleep(Duration::from_millis(50));
    }
    Ok(())
}

/// Like `watch`, but drains un-grabbed devices so their event counts show the operator used them.
fn watch_counting(
    observer: &Observer,
    secs: u64,
    label: &str,
    run: &mut Run,
    mut counter: Option<&mut EvdevGrab>,
) -> Result<(), String> {
    let mut end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if !observer.ready_now(FRESH) {
            if recover_focus(observer, run, label, FOCUS_RECOVERY) {
                if let Some(counter) = counter.as_mut() {
                    let _ = counter.drain(0);
                    counter.reset_counts();
                }
                end = Instant::now() + Duration::from_secs(secs);
                continue;
            }
            let message = format!("observer lost focus or fullscreen during {label}");
            run.aborted = Some(message.clone());
            return Err(message);
        }
        if let Some(counter) = counter.as_mut() {
            let _ = counter.drain(0);
        }
        sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn open_counter(nodes: &[u32]) -> Option<EvdevGrab> {
    let mut devices = BTreeMap::new();
    for node in nodes {
        let device = Device::open(format!("/dev/input/event{node}")).ok()?;
        device.set_nonblocking(true).ok()?;
        devices.insert(*node, device);
    }
    Some(EvdevGrab::new(devices, None))
}

/// Waits `secs` counting what the un-grabbed devices produce, so page events in the same window
/// can be told apart from ghost input. Returns the number of device events seen.
fn quiet_window(
    observer: &Observer,
    secs: u64,
    run: &mut Run,
    isolation: &Arc<Mutex<Isolation<EvdevGrab>>>,
    mut builtin: Option<&mut EvdevGrab>,
) -> u64 {
    {
        let mut guard = isolation.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = guard.grabber_mut().drain(0);
        guard.grabber_mut().reset_counts();
    }
    if let Some(builtin) = builtin.as_mut() {
        let _ = builtin.drain(0);
        builtin.reset_counts();
    }
    let mut end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if !observer.ready_now(FRESH) {
            if recover_focus(observer, run, "a quiet window", FOCUS_RECOVERY) {
                let mut guard = isolation.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = guard.grabber_mut().drain(0);
                guard.grabber_mut().reset_counts();
                if let Some(builtin) = builtin.as_mut() {
                    let _ = builtin.drain(0);
                    builtin.reset_counts();
                }
                end = Instant::now() + Duration::from_secs(secs);
                continue;
            }
            run.aborted =
                Some("observer lost focus or fullscreen in the hands-off window".to_string());
            break;
        }
        let _ = isolation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .grabber_mut()
            .drain(0);
        if let Some(builtin) = builtin.as_mut() {
            let _ = builtin.drain(0);
        }
        sleep(Duration::from_millis(20));
    }
    let external = {
        let guard = isolation.lock().unwrap_or_else(PoisonError::into_inner);
        guard.grabber().key_presses + guard.grabber().motions
    };
    external + builtin.map_or(0, |builtin| builtin.key_presses + builtin.motions)
}

/// True once `down()` has reported nothing held for `stable`, false if `timeout` passes first.
/// A key or button held when a grab starts never sends its release to the session, which then
/// auto-repeats it (seen live in run 5), so every grab waits for this first.
fn wait_until_released(
    mut down: impl FnMut() -> usize,
    timeout: Duration,
    stable: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    let mut clear_since: Option<Instant> = None;
    while Instant::now() < deadline {
        if down() == 0 {
            if clear_since.get_or_insert_with(Instant::now).elapsed() >= stable {
                return true;
            }
        } else {
            clear_since = None;
        }
        sleep(Duration::from_millis(25));
    }
    false
}

/// Asks the operator to lift every finger and waits for the dongle and any extra nodes to report it.
fn release_gate(
    observer: &Observer,
    stage: &mut StageResult,
    mut down: impl FnMut() -> usize,
) -> bool {
    announce(
        observer,
        "Get ready: LIFT ALL fingers off every keyboard key and mouse button now. The grab starts in a moment.",
    );
    if wait_until_released(
        &mut down,
        Duration::from_secs(15),
        Duration::from_millis(500),
    ) {
        return true;
    }
    stage
        .gaps
        .push("a key or mouse button stayed down, so the grab was not started".to_string());
    false
}

fn fail_on(stage: &mut StageResult, result: Result<(), String>) -> bool {
    match result {
        Ok(()) => true,
        Err(message) => {
            stage.failures.push(message);
            false
        }
    }
}

fn arm_page(observer: &Observer, run: &mut Run, stage: &mut StageResult, what: &str) -> bool {
    if observer.arm(Duration::from_secs(10)) {
        return true;
    }
    let message = format!("observer did not acknowledge the {what} reset");
    run.aborted = Some(message.clone());
    stage.gaps.push(message);
    false
}

fn start_helper(
    program: &Path,
    args: &Args,
    nodes: &[u32],
    builtin: bool,
    max_secs: u64,
    expect_comm: &str,
    stage: &mut StageResult,
) -> Option<Helper> {
    let mut helper = match Helper::spawn(program, &helper_args(args, nodes, builtin, max_secs)) {
        Ok(helper) => helper,
        Err(error) => {
            stage.failures.push(format!("spawn helper: {error:#}"));
            return None;
        }
    };
    let comm = comm_of(helper.child.id());
    if comm.as_deref() != Some(expect_comm) {
        stage.failures.push(format!(
            "helper process name is {comm:?}, expected {expect_comm}: its kill timer would not match it"
        ));
        return None;
    }
    if !helper.wait_grabbed(Duration::from_secs(10)) {
        if helper.keys_held {
            stage.gaps.push(
                "a key or button was held down when the helper checked, so it did not grab"
                    .to_string(),
            );
        } else {
            stage
                .failures
                .push("helper did not report GRABBED (see its message above)".to_string());
        }
        return None;
    }
    Some(helper)
}

/// Verdict for one helper stage from what the page saw while the helper held the grab and after it ended.
fn judge_stage(
    stage: &mut StageResult,
    operator_active: bool,
    after_active: bool,
    end_problem: Option<String>,
) {
    match &stage.held {
        None => stage
            .gaps
            .push("no page tally while the helper held the grab".to_string()),
        Some(held) => {
            let leaked = held.physical_events(0);
            if leaked > 0 {
                stage.failures.push(format!(
                    "{leaked} physical-looking events reached the page while grabbed (key shape `{}`; all `r` means a key was down when the grab started)",
                    held.key_shape
                ));
            }
        }
    }
    if !operator_active {
        stage
            .gaps
            .push("no operator activity was observed while grabbed".to_string());
    }
    if let Some(problem) = end_problem {
        stage.failures.push(problem);
    }
    match &stage.after {
        None => stage
            .gaps
            .push("no page tally after the helper ended".to_string()),
        Some(after) if after.physical_events(0) == 0 && after_active => stage.failures.push(
            "the devices produced events after the grab ended but none reached the page"
                .to_string(),
        ),
        Some(after) if after.physical_events(0) == 0 => stage.gaps.push(
            "the devices produced no events after the grab ended, so they may not have been used"
                .to_string(),
        ),
        Some(after) if after.key_downs == 0 || after.pointer_moves == 0 => stage
            .gaps
            .push("after the grab ended the page saw only one kind of input".to_string()),
        Some(_) => {}
    }
}

fn stage_sigkill(
    args: &Args,
    run: &mut Run,
    observer: &Observer,
    exe: &Path,
    isolation: &Arc<Mutex<Isolation<EvdevGrab>>>,
) -> StageResult {
    let mut stage = StageResult {
        name: "helper killed with SIGKILL".to_string(),
        ..StageResult::default()
    };
    if !release_gate(observer, &mut stage, || {
        isolation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .grabber()
            .keys_still_down()
    }) {
        return stage;
    }
    let Some(mut helper) = start_helper(exe, args, &args.nodes, false, 60, KILL_COMM, &mut stage)
    else {
        return stage;
    };
    if !arm_page(observer, run, &mut stage, "held") {
        return stage;
    }
    helper.pump();
    let base = parse_reads(&helper.last_read);
    announce(
        observer,
        &format!(
            "Stage 2 ({HOLD_SECS} s): a separate helper process holds the grab. Type and move the EXTERNAL devices; nothing should reach the page."
        ),
    );
    let watched = watch(
        observer,
        HOLD_SECS,
        "stage 2 held",
        run,
        Some(&mut helper),
        None,
        None,
    );
    if !fail_on(&mut stage, watched) {
        return stage;
    }
    stage.held = snapshot_tally(observer);
    helper.pump();
    stage.helper_reads.clone_from(&helper.last_read);
    let moved = reads_delta(&parse_reads(&stage.helper_reads), &base, None);
    let _ = helper.child.kill();
    stage.helper_end = helper
        .child
        .wait()
        .map_or_else(|error| error.to_string(), describe_exit);
    if arm_page(observer, run, &mut stage, "after") {
        announce(
            observer,
            &format!(
                "Stage 2 after ({AFTER_SECS} s): the helper was killed. Type and move the external devices again."
            ),
        );
        stage.after_device_events = quiet_window(observer, AFTER_SECS, run, isolation, None);
        stage.after = snapshot_tally(observer);
    }
    let end_problem = (stage.helper_end != "signal 9")
        .then(|| format!("the helper ended with {}, not SIGKILL", stage.helper_end));
    let after_active = stage.after_device_events > 0;
    judge_stage(&mut stage, moved > 0, after_active, end_problem);
    stage
}

fn stage_stall(
    args: &Args,
    run: &mut Run,
    observer: &Observer,
    exe: &Path,
    isolation: &Arc<Mutex<Isolation<EvdevGrab>>>,
) -> StageResult {
    let mut stage = StageResult {
        name: "stalled helper killed by its own timer".to_string(),
        ..StageResult::default()
    };
    let (_dir, link) = match hold_link(exe) {
        Ok(pair) => pair,
        Err(error) => {
            stage.failures.push(format!("helper symlink: {error:#}"));
            return stage;
        }
    };
    if !release_gate(observer, &mut stage, || {
        isolation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .grabber()
            .keys_still_down()
    }) {
        return stage;
    }
    let Some(mut helper) =
        start_helper(&link, args, &args.nodes, false, 90, STALL_COMM, &mut stage)
    else {
        return stage;
    };
    let timer = match StallTimer::arm() {
        Ok(timer) => timer,
        Err(error) => {
            stage.failures.push(format!("stall kill timer: {error}"));
            return stage;
        }
    };
    if !arm_page(observer, run, &mut stage, "stalled") {
        return stage;
    }
    let pid = helper.child.id();
    let stopped = Command::new("kill")
        .args(["-STOP", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success());
    if !stopped || !wait_state(pid, 'T', Duration::from_secs(2)) {
        stage
            .failures
            .push("the helper did not enter the stopped state".to_string());
        return stage;
    }
    let stopped_at = Instant::now();
    announce(
        observer,
        &format!(
            "Stage 3 ({HOLD_SECS} s): the helper is FROZEN but still holds the grab, so the external devices stay dead until its own timer kills it (about {STALL_KILL_SECS} s). Release every key first, then HOLD one letter key down and keep moving the mouse."
        ),
    );
    let mut polls = 0_u32;
    let watched = watch(
        observer,
        HOLD_SECS,
        "stage 3 stalled",
        run,
        Some(&mut helper),
        Some(isolation),
        Some(&mut polls),
    );
    stage.key_polls = polls;
    if !fail_on(&mut stage, watched) {
        return stage;
    }
    stage.held = snapshot_tally(observer);
    announce(
        observer,
        "Stage 3: the devices are still dead (expected). Keep the key held and keep moving the mouse until this text changes.",
    );
    let deadline = Instant::now() + Duration::from_secs(45);
    let status = loop {
        if let Some(status) = helper.exited() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        sleep(Duration::from_millis(100));
    };
    match status {
        Some(status) => {
            stage.helper_end = describe_exit(status);
            stage.seconds_to_end = Some(stopped_at.elapsed().as_secs());
        }
        None => {
            let _ = helper.child.kill();
            let _ = helper.child.wait();
            stage.helper_end = "still stopped after 45 s; killed by the probe".to_string();
        }
    }
    drop(timer);
    if arm_page(observer, run, &mut stage, "after") {
        announce(
            observer,
            &format!(
                "Stage 3 after ({AFTER_SECS} s): release the held key, then type and move the external devices."
            ),
        );
        stage.after_device_events = quiet_window(observer, AFTER_SECS, run, isolation, None);
        stage.after = snapshot_tally(observer);
    }
    if stage.seconds_to_end.is_some_and(|secs| secs < HOLD_SECS) {
        stage
            .gaps
            .push("the helper died before the stall window ended".to_string());
    }
    let end_problem = (stage.helper_end != "signal 9").then(|| {
        format!(
            "the helper ended with {}, so its own timer did not kill it",
            stage.helper_end
        )
    });
    let active = stage.key_polls >= 3;
    let after_active = stage.after_device_events > 0;
    judge_stage(&mut stage, active, after_active, end_problem);
    stage
}

fn stage_chord(args: &Args, run: &mut Run, observer: &Observer, exe: &Path) -> StageResult {
    let mut stage = StageResult {
        name: "built-in devices released by the emergency chord".to_string(),
        ..StageResult::default()
    };
    let nodes: Vec<u32> = args
        .nodes
        .iter()
        .chain(&args.builtin_nodes)
        .copied()
        .collect();
    let mut gate_nodes = open_counter(&nodes);
    if !release_gate(observer, &mut stage, || {
        gate_nodes.as_ref().map_or(0, EvdevGrab::keys_still_down)
    }) {
        return stage;
    }
    drop(gate_nodes.take());
    let Some(mut helper) = start_helper(exe, args, &nodes, true, 80, KILL_COMM, &mut stage) else {
        return stage;
    };
    if !arm_page(observer, run, &mut stage, "held") {
        return stage;
    }
    helper.pump();
    let base = parse_reads(&helper.last_read);
    announce(
        observer,
        &format!(
            "Stage 4 ({HOLD_SECS} s): EVERYTHING is grabbed, including this laptop's own keyboard and touchpad. Type and move on BOTH the built-in and the external devices; nothing should reach the page."
        ),
    );
    let watched = watch(
        observer,
        HOLD_SECS,
        "stage 4 held",
        run,
        Some(&mut helper),
        None,
        None,
    );
    if !fail_on(&mut stage, watched) {
        return stage;
    }
    stage.held = snapshot_tally(observer);
    helper.pump();
    stage.helper_reads.clone_from(&helper.last_read);
    let builtin_held = reads_delta(
        &parse_reads(&stage.helper_reads),
        &base,
        Some(&args.builtin_nodes),
    );
    announce(
        observer,
        &format!(
            "Stage 4: now HOLD Left Ctrl + Left Shift + Left Alt + Esc together on the BUILT-IN keyboard for about 2 s until the grab releases (up to {CHORD_WAIT_SECS} s)."
        ),
    );
    let started = Instant::now();
    let status = loop {
        helper.pump();
        if let Some(status) = helper.exited() {
            break Some(status);
        }
        if started.elapsed() >= Duration::from_secs(CHORD_WAIT_SECS) {
            break None;
        }
        sleep(Duration::from_millis(50));
    };
    let mut released_by_chord = false;
    match status {
        Some(status) => {
            helper.finish();
            stage.seconds_to_end = Some(started.elapsed().as_secs());
            let line = helper.ended.clone().unwrap_or_default();
            released_by_chord = status.code() == Some(0)
                && line.starts_with("RELEASED chord")
                && line.contains("clean=true");
            stage.helper_end = format!("{}; {line}", describe_exit(status));
        }
        None => {
            let _ = helper.child.kill();
            let _ = helper.child.wait();
            stage.helper_end =
                format!("no release within {CHORD_WAIT_SECS} s; killed by the probe");
        }
    }
    helper.pump();
    stage.chord_keys_max = helper.chord_max;
    stage.helper_reads_end.clone_from(&helper.last_read);
    let mut counter = open_counter(&args.builtin_nodes);
    if arm_page(observer, run, &mut stage, "after") {
        announce(
            observer,
            &format!(
                "Stage 4 after ({AFTER_SECS} s): type and move on the BUILT-IN keyboard and touchpad ONLY; leave the external devices alone."
            ),
        );
        let watched = watch_counting(observer, AFTER_SECS, "stage 4 after", run, counter.as_mut());
        fail_on(&mut stage, watched);
        stage.after = snapshot_tally(observer);
    }
    let builtin_after: u64 = counter
        .as_ref()
        .map_or(0, |counter| counter.per_node.values().sum());
    let end_problem = (!released_by_chord).then(|| {
        format!(
            "the grab was not released cleanly by the emergency chord ({}; most chord keys seen down at once: {} of {})",
            stage.helper_end,
            stage.chord_keys_max,
            EMERGENCY_CHORD_KEYS.len()
        )
    });
    judge_stage(&mut stage, builtin_held > 0, builtin_after > 0, end_problem);
    stage
}

/// A baseline needs both keyboard and mouse activity on the page.
fn baseline_ok(tally: &PhaseTally) -> bool {
    tally.key_events > 0 && tally.pointer_moves + tally.buttons as u64 + tally.wheel_events > 0
}

/// A chord-only run is judged on its one stage; the other phases did not run.
fn classify_chord_only(run: &mut Run) -> ExperimentResult {
    for stage in &run.stages {
        for failure in &stage.failures {
            run.violations.push(format!("{}: {failure}", stage.name));
        }
    }
    if run.shell_pid_before.is_some() && run.shell_pid_before != run.shell_pid_after {
        run.violations.push("gnome-shell PID changed".to_string());
    }
    if !run.violations.is_empty() {
        return ExperimentResult::Fail;
    }
    let mut partial = run.aborted.is_some() || run.stages.is_empty();
    for stage in &run.stages {
        for gap in &stage.gaps {
            run.notes.push(format!("{}: {gap}", stage.name));
            partial = true;
        }
    }
    if partial {
        ExperimentResult::Partial
    } else {
        ExperimentResult::Pass
    }
}

fn classify_run(run: &mut Run) -> ExperimentResult {
    if run.blocked.is_some() {
        return ExperimentResult::Blocked;
    }
    if run.failure.is_some() {
        return ExperimentResult::Fail;
    }
    if run.chord_only {
        return classify_chord_only(run);
    }
    let injected = usize::from(run.injected_shift.as_deref() == Some("accepted"));
    if let Some(b) = &run.phase_b {
        let leaked = b.physical_events(injected);
        if leaked > 0 {
            run.violations.push(format!(
                "{leaked} physical-looking events reached the page while grabbed (key shape `{}`)",
                b.key_shape
            ));
        }
        if injected == 1 && !(b.key_events == 2 && b.key_downs == 1 && b.shift_down == 1) {
            run.violations.push(format!(
                "phase B keys are not exactly the injected Shift tap (events {}, downs {}, Shift downs {}, shape {})",
                b.key_events, b.key_downs, b.shift_down, b.key_shape
            ));
        }
    }
    let mut c0_touched = false;
    if let Some(c0) = &run.phase_c0 {
        let page = c0.physical_events(0);
        if page > 0 && run.c0_device_events == 0 {
            run.violations.push(format!(
                "{page} events reached the page in the hands-off window although no device produced any (ghost input)"
            ));
        } else if page > 0 {
            run.notes.push(
                "a device was touched in the hands-off window, so ghost input was not checked"
                    .to_string(),
            );
            c0_touched = true;
        }
    }
    for stage in &run.stages {
        for failure in &stage.failures {
            run.violations.push(format!("{}: {failure}", stage.name));
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
    let a_seen = run.phase_a.as_ref().is_some_and(baseline_ok);
    if run.phase_c0.is_none() {
        run.notes
            .push("no hands-off tally after release, so ghost input was not checked".to_string());
    }
    if !a_seen {
        run.notes
            .push("phase A needs both keyboard and mouse activity as a baseline".to_string());
    }
    let mut stage_gaps = run.stages.len() < run.stages_requested;
    for stage in &run.stages {
        for gap in &stage.gaps {
            run.notes.push(format!("{}: {gap}", stage.name));
            stage_gaps = true;
        }
    }
    let c_seen = run.phase_c.as_ref().is_some_and(baseline_ok);
    let operator_active = run.probe_key_presses_in_b > 0 && run.probe_motions_in_b > 0;
    if run.aborted.is_some()
        || stage_gaps
        || run.phase_c0.is_none()
        || c0_touched
        || !(a_seen && c_seen && operator_active && injected == 1)
    {
        if !operator_active {
            run.notes.push("the operator did not use both a keyboard and a pointing device on the grabbed nodes in phase B".to_string());
        }
        return ExperimentResult::Partial;
    }
    ExperimentResult::Pass
}

fn focus_history(observer: &Observer) -> Vec<String> {
    observer.snapshot(|state| {
        state
            .tally
            .as_ref()
            .and_then(|tally| tally["transitions"].as_array())
            .map(|list| {
                list.iter()
                    .map(|entry| {
                        format!(
                            "t={}ms focus={} fullscreen={} visibility={}",
                            entry["t"], entry["focus"], entry["fullscreen"], entry["visibility"]
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn stage_summary(run: &Run) -> String {
    run.stages
        .iter()
        .map(|stage| {
            format!(
                "[{}: held={:?} after={:?} reads=`{}` end=`{}` failures={:?} gaps={:?}]",
                stage.name,
                stage.held,
                stage.after,
                stage.helper_reads,
                stage.helper_end,
                stage.failures,
                stage.gaps
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    anyhow::ensure!(
        args.operator_present,
        "refusing to grab input: pass --operator-present only after the safety preflight in \
         docs/ops/experiment-safety.md §7 and the plan's step 5a approval"
    );
    if args.hold_grab {
        return hold_main(&args);
    }
    let now = OffsetDateTime::now_utc();
    let mut run = Run {
        chord_only: args.chord_only,
        stages_requested: if args.chord_only {
            1
        } else if args.full {
            2 + usize::from(!args.builtin_nodes.is_empty())
        } else {
            0
        },
        ..Run::default()
    };
    match preflight(&args, &mut run) {
        Err(error) => run.blocked = Some(format!("preflight: {error:#}")),
        Ok(devices) => {
            let observer = Observer::start(OBSERVER_PAGE)?;
            let executed = execute(&args, &mut run, &observer, devices);
            run.focus_history = focus_history(&observer);
            if let Err(error) = executed
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
        "result={result}; blocked={:?}; failure={:?}; aborted={:?}; A={:?}; B={:?}; C0={:?}; C={:?}; probe read {} key presses and {} motions in B; \
         injected Shift={:?}; release failures={:?}; keys still down after release={:?}; violations={:?}; notes={:?}; stages={}",
        run.blocked,
        run.failure,
        run.aborted,
        run.phase_a,
        run.phase_b,
        run.phase_c0,
        run.phase_c,
        run.probe_key_presses_in_b,
        run.probe_motions_in_b,
        run.injected_shift,
        run.restore_failed,
        run.keys_still_down_after_release,
        run.violations,
        run.notes,
        stage_summary(&run)
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
            .to_string()
            + if args.full {
                " --full then runs: a separate helper process holds the grab and is SIGKILLed; a SIGSTOPped helper is killed by its own timer; \
                  with --builtin-nodes the built-in devices are grabbed too and released by the emergency chord."
            } else {
                ""
            },
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
        follow_up: Some("A PASS here is one observation, not FEAS-E; hotplug, LED/repeat state and repeated cycles are untested.".to_string()),
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
            key_downs: 1,
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
            phase_c0: Some(PhaseTally::default()),
            phase_c: Some(busy.clone()),
            probe_key_presses_in_b: 5,
            probe_motions_in_b: 50,
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
                probe_motions_in_b: 50,
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
    fn the_kill_command_must_match_the_truncated_process_name() {
        let show = |name: &str| {
            format!(
                "ExecStart={{ path=/usr/bin/pkill ; argv[]=/usr/bin/pkill -KILL -x {name} ; ignore_errors=no ; start_time=[n/a] }}"
            )
        };
        assert_eq!(
            kill_command_problem(&show("exp09_grab_prob"), KILL_COMM),
            None
        );
        assert!(kill_command_problem(&show("exp09_grab_probe"), KILL_COMM).is_some());
        assert!(kill_command_problem(&show("exp09_grab_prob extra"), KILL_COMM).is_some());
        assert!(
            kill_command_problem(
                "ExecStart={ path=/bin/true ; argv[]=/bin/true ; }",
                KILL_COMM
            )
            .is_some()
        );
        assert!(kill_command_problem("", KILL_COMM).is_some());
    }

    #[test]
    fn focus_recovery_gives_up_and_restores_the_prompt() {
        let observer = Observer::start("<html></html>").expect("observer");
        observer.set_prompt("Phase A: type");
        let mut run = Run::default();
        assert!(!recover_focus(
            &observer,
            &mut run,
            "phase A",
            Duration::from_millis(150)
        ));
        assert_eq!(observer.prompt(), "Phase A: type");
        assert!(run.notes.is_empty());
    }

    #[test]
    fn a_chord_only_run_is_judged_on_its_one_stage() {
        let clean = || Run {
            chord_only: true,
            stages: vec![StageResult {
                name: "chord".to_string(),
                ..StageResult::default()
            }],
            ..Run::default()
        };
        assert_eq!(classify_run(&mut clean()), ExperimentResult::Pass);
        let mut failed = clean();
        failed.stages[0].failures.push("no release".to_string());
        assert_eq!(classify_run(&mut failed), ExperimentResult::Fail);
        let mut gap = clean();
        gap.stages[0].gaps.push("idle".to_string());
        assert_eq!(classify_run(&mut gap), ExperimentResult::Partial);
        let mut none = Run {
            chord_only: true,
            ..Run::default()
        };
        assert_eq!(classify_run(&mut none), ExperimentResult::Partial);
        assert_eq!(parse_chord("READ e2=4 chord=3"), Some(3));
        assert_eq!(parse_chord("READ e2=4"), None);
    }

    #[test]
    fn a_grab_waits_for_every_key_to_be_up_and_stay_up() {
        let stable = Duration::from_millis(60);
        let mut polls = 0;
        assert!(wait_until_released(
            || {
                polls += 1;
                usize::from(polls < 4)
            },
            Duration::from_secs(2),
            stable
        ));
        assert!(
            !wait_until_released(|| 1, Duration::from_millis(150), stable),
            "a key that stays down never passes the gate"
        );
        let mut flicker = 0;
        assert!(
            !wait_until_released(
                || {
                    flicker += 1;
                    usize::from(flicker % 2 == 0)
                },
                Duration::from_millis(300),
                Duration::from_millis(200)
            ),
            "a key that keeps coming back resets the stability window"
        );
    }

    #[test]
    fn a_timer_left_at_the_default_accuracy_is_refused() {
        assert_eq!(accuracy_problem("AccuracyUSec=1s"), None);
        assert_eq!(accuracy_problem("AccuracyUSec=250ms"), None);
        assert!(accuracy_problem("AccuracyUSec=1min").is_some());
        assert!(accuracy_problem("AccuracyUSec=6s").is_some());
        assert!(accuracy_problem("").is_some());
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
    fn helper_reads_and_arguments_round_trip() {
        let reads = parse_reads("READ e2=5 e6=12 bogus e7=x");
        assert_eq!(reads.get(&2), Some(&5));
        assert_eq!(reads.get(&6), Some(&12));
        assert_eq!(reads.len(), 2);
        assert!(parse_reads("READ").is_empty());

        let args = Args::try_parse_from([
            "exp09",
            "--operator-present",
            "--nodes",
            "6,7",
            "--expect-phys-prefix",
            "usb-x/",
            "--full",
            "--builtin-nodes",
            "2,3",
        ])
        .expect("parse");
        assert!(args.full);
        assert_eq!(args.builtin_nodes, vec![2, 3]);
        let external = helper_args(&args, &args.nodes, false, 60);
        assert!(external.contains(&"--expect-phys-prefix".to_string()));
        assert!(!external.contains(&"--chord".to_string()));
        let builtin = helper_args(&args, &[6, 7, 2, 3], true, 80);
        assert!(builtin.contains(&"--chord".to_string()));
        assert!(builtin.contains(&"6,7,2,3".to_string()));
        assert!(
            builtin.contains(&"2,3".to_string()),
            "chord nodes are the built-in ones"
        );
        for helper in [&external, &builtin] {
            let parsed = Args::try_parse_from(
                std::iter::once("exp09".to_string()).chain(helper.iter().cloned()),
            );
            assert!(parsed.is_ok(), "helper arguments must parse: {parsed:?}");
        }
        assert!(!builtin.contains(&"--expect-phys-prefix".to_string()));
    }

    #[test]
    fn built_in_nodes_must_not_be_usb_and_the_chord_needs_all_four_keys() {
        assert_eq!(
            builtin_problem(true, Some("seat0"), "isa0060/serio0/input0"),
            None
        );
        assert!(builtin_problem(true, Some("seat0"), "usb-0000:00:14.0-1.1/input0").is_some());
        assert!(builtin_problem(true, Some("seat1"), "isa0060/serio0/input0").is_some());
        assert!(builtin_problem(false, Some("seat0"), "isa0060/serio0/input0").is_some());

        let mut chord = emergency_chord().expect("chord");
        for code in [29, 42, 56] {
            chord.key(code, true, 0);
        }
        assert!(!chord.poll(5000), "three of four keys never fire");
        chord.key(1, true, 100);
        assert!(!chord.poll(1000), "not held long enough");
        assert!(chord.poll(2200));
        assert!(!chord.poll(2300), "fires once per hold");
    }

    #[test]
    fn a_stage_passes_only_when_the_grab_holds_and_input_returns() {
        let busy = PhaseTally {
            key_events: 4,
            key_downs: 2,
            pointer_moves: 3,
            ..PhaseTally::default()
        };
        let quiet = PhaseTally::default();
        let stage = |held: Option<PhaseTally>, after: Option<PhaseTally>| StageResult {
            held,
            after,
            ..StageResult::default()
        };

        let mut good = stage(Some(quiet.clone()), Some(busy.clone()));
        judge_stage(&mut good, true, true, None);
        assert!(good.failures.is_empty() && good.gaps.is_empty());

        let mut leak = stage(Some(busy.clone()), Some(busy.clone()));
        judge_stage(&mut leak, true, true, None);
        assert_eq!(leak.failures.len(), 1);

        let mut stuck = stage(Some(quiet.clone()), Some(quiet.clone()));
        judge_stage(&mut stuck, true, true, None);
        assert_eq!(stuck.failures.len(), 1);

        let mut silent = stage(Some(quiet.clone()), Some(quiet.clone()));
        judge_stage(&mut silent, true, false, None);
        assert!(
            silent.failures.is_empty() && silent.gaps.len() == 1,
            "an idle operator after the grab is a gap, not a failure"
        );

        let mut idle = stage(Some(quiet.clone()), Some(busy.clone()));
        judge_stage(&mut idle, false, true, None);
        assert!(idle.failures.is_empty() && idle.gaps.len() == 1);

        let mut wrong_end = stage(Some(quiet), Some(busy));
        judge_stage(&mut wrong_end, true, true, Some("ended early".to_string()));
        assert_eq!(wrong_end.failures, vec!["ended early".to_string()]);

        let mut missing = stage(None, None);
        judge_stage(&mut missing, true, true, None);
        assert_eq!(missing.gaps.len(), 2);
    }

    #[test]
    fn a_failed_stage_fails_the_run_and_a_gap_only_makes_it_partial() {
        let busy = PhaseTally {
            key_events: 6,
            pointer_moves: 4,
            ..PhaseTally::default()
        };
        let quiet = PhaseTally {
            key_events: 2,
            key_downs: 1,
            shift_down: 1,
            ..PhaseTally::default()
        };
        let base = || Run {
            phase_a: Some(busy.clone()),
            phase_b: Some(quiet.clone()),
            phase_c0: Some(PhaseTally::default()),
            phase_c: Some(busy.clone()),
            probe_key_presses_in_b: 5,
            probe_motions_in_b: 50,
            injected_shift: Some("accepted".to_string()),
            ..Run::default()
        };
        let mut run = base();
        assert_eq!(classify_run(&mut run), ExperimentResult::Pass);

        let mut failed = base();
        failed.stages.push(StageResult {
            name: "s".to_string(),
            failures: vec!["x".to_string()],
            ..StageResult::default()
        });
        assert_eq!(classify_run(&mut failed), ExperimentResult::Fail);

        let mut gap = base();
        gap.stages.push(StageResult {
            name: "s".to_string(),
            gaps: vec!["idle".to_string()],
            ..StageResult::default()
        });
        assert_eq!(classify_run(&mut gap), ExperimentResult::Partial);

        let mut skipped = base();
        skipped.stages_requested = 2;
        assert_eq!(classify_run(&mut skipped), ExperimentResult::Partial);

        let mut ghost = base();
        ghost.phase_c0 = Some(busy.clone());
        assert_eq!(classify_run(&mut ghost), ExperimentResult::Fail);

        let mut touched = base();
        touched.phase_c0 = Some(busy.clone());
        touched.c0_device_events = 12;
        assert_eq!(classify_run(&mut touched), ExperimentResult::Partial);

        let mut no_keyboard_after = base();
        no_keyboard_after.phase_c = Some(PhaseTally {
            pointer_moves: 9,
            ..PhaseTally::default()
        });
        assert_eq!(
            classify_run(&mut no_keyboard_after),
            ExperimentResult::Partial
        );

        let mut keys_only_in_b = base();
        keys_only_in_b.probe_motions_in_b = 0;
        assert_eq!(classify_run(&mut keys_only_in_b), ExperimentResult::Partial);

        let mut no_keyboard_baseline = base();
        no_keyboard_baseline.phase_a = Some(PhaseTally {
            pointer_moves: 3,
            ..PhaseTally::default()
        });
        assert_eq!(
            classify_run(&mut no_keyboard_baseline),
            ExperimentResult::Partial
        );
    }

    #[test]
    fn helper_state_and_activity_helpers() {
        assert_eq!(parse_proc_state("123 (exp09_grab_hold) T 1 2 3"), Some('T'));
        assert_eq!(parse_proc_state("9 (a b) (c) S 1"), Some('S'));
        assert_eq!(parse_proc_state("garbage"), None);
        let end = parse_reads("READ e2=15 e6=30");
        let base = parse_reads("READ e2=5 e6=30");
        assert_eq!(reads_delta(&end, &base, None), 10);
        assert_eq!(reads_delta(&end, &base, Some(&[6])), 0);
        assert_eq!(reads_delta(&end, &base, Some(&[2])), 10);
    }

    #[test]
    fn riskier_stages_need_a_fully_clean_first_stage() {
        let busy = PhaseTally {
            key_events: 6,
            key_downs: 3,
            pointer_moves: 4,
            ..PhaseTally::default()
        };
        let quiet = PhaseTally {
            key_events: 2,
            key_downs: 1,
            shift_down: 1,
            ..PhaseTally::default()
        };
        let clean = || Run {
            phase_a: Some(busy.clone()),
            phase_b: Some(quiet.clone()),
            phase_c0: Some(PhaseTally::default()),
            phase_c: Some(busy.clone()),
            probe_key_presses_in_b: 5,
            probe_motions_in_b: 50,
            injected_shift: Some("accepted".to_string()),
            ..Run::default()
        };
        assert!(can_continue(&clean()));
        let mut run = clean();
        run.injected_shift = Some("refused".to_string());
        assert!(!can_continue(&run));
        let mut run = clean();
        run.probe_key_presses_in_b = 0;
        assert!(!can_continue(&run));
        let mut run = clean();
        run.phase_c0 = None;
        assert!(!can_continue(&run));
        let mut run = clean();
        run.phase_c0 = Some(busy.clone());
        assert!(!can_continue(&run));
        let mut run = clean();
        run.phase_c = Some(PhaseTally::default());
        assert!(!can_continue(&run));
        let mut run = clean();
        run.phase_a = Some(PhaseTally {
            pointer_moves: 3,
            ..PhaseTally::default()
        });
        assert!(!can_continue(&run));
        let mut run = clean();
        run.restore_failed.push("e".to_string());
        assert!(!can_continue(&run));
    }

    #[test]
    fn the_probe_refuses_to_run_without_the_operator_flag() {
        assert!(Args::try_parse_from(["exp09", "--nodes", "6,7"]).is_ok());
        assert!(Args::try_parse_from(["exp09", "--nodes", "6", "--phase-secs", "3"]).is_err());
    }
}
