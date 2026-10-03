//! Device-free core of the emergency daemon: a release gate before the grab, the grab itself
//! (all-or-nothing), a dead-man lease, hotplug, and the emergency chord. Everything the real
//! hardware can do to it arrives through [`Nodes`], so the whole state machine runs against fakes.

use std::collections::{BTreeMap, BTreeSet};

use remote_input_helper::{
    Caps, Chord, ChordDetector, DeviceGrab, DeviceId, HotplugOutcome, IsolateError, Isolation,
    State as IsolationState, TickEvent,
};

/// What a read of the nodes saw. Key codes stay in memory and only the chord detector keeps any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    Key {
        node: DeviceId,
        code: u16,
        pressed: bool,
    },
    Activity {
        node: DeviceId,
    },
    /// The node was unplugged; the kernel already dropped any grab on it.
    Removed {
        node: DeviceId,
    },
}

/// The input nodes the daemon watches and may grab.
pub trait Nodes: DeviceGrab {
    /// Allow-list candidates at this moment (rescans for hotplug).
    fn candidates(&mut self) -> Vec<(DeviceId, Caps)>;
    /// Keys and buttons physically down across the candidate nodes.
    fn keys_down(&self) -> usize;
    /// Reads pending events from every open node without blocking.
    fn poll(&mut self) -> Result<Vec<Observed>, String>;
}

#[derive(Debug, Clone)]
pub struct Config {
    pub lease_min_ms: u64,
    pub lease_max_ms: u64,
    /// Give up waiting for every key and button to be up after this long.
    pub gate_timeout_ms: u64,
    /// Nothing may be down, and nothing pressed, for this long before the grab.
    pub gate_stable_ms: u64,
    /// Grab attempts when a key is pressed in the gap before the grab lands.
    pub max_attempts: u32,
    pub rescan_ms: u64,
    pub chord: Chord,
    /// Only keys from these nodes count towards the chord; `None` accepts any node.
    pub chord_nodes: Option<BTreeSet<DeviceId>>,
}

impl Config {
    /// Left Ctrl, Left Shift, Left Alt and Esc held 2 s: the chord observed live on this laptop.
    pub fn experiment_chord() -> Chord {
        Chord::new(&[29, 42, 56, 1], 2000).unwrap_or_else(|| unreachable!("chord is not empty"))
    }

    pub fn standard() -> Self {
        Self {
            lease_min_ms: 1_000,
            lease_max_ms: 60_000,
            gate_timeout_ms: 20_000,
            gate_stable_ms: 500,
            max_attempts: 5,
            rescan_ms: 250,
            chord: Self::experiment_chord(),
            chord_nodes: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    /// Waiting for every key and button to be up before grabbing.
    Gating,
    Isolated,
    /// A release failed; grabs may remain and every step retries them.
    ReleaseFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Restore,
    LeaseExpired,
    Chord,
    HotplugFailClosed,
    CoverageLost,
    ReadError,
    /// A release failed; the daemon keeps retrying and reports `ReleaseRecovered` when it works.
    ReleaseFailed,
    ReleaseRecovered,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Restore => "restore",
            Self::LeaseExpired => "lease_expired",
            Self::Chord => "chord",
            Self::HotplugFailClosed => "hotplug_fail_closed",
            Self::CoverageLost => "coverage_lost",
            Self::ReadError => "read_error",
            Self::ReleaseFailed => "release_failed",
            Self::ReleaseRecovered => "release_recovered",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Busy,
    BadLease,
    /// A key or button stayed down (or kept being pressed) until the gate gave up.
    KeysHeld,
    NothingToGrab,
    GrabFailed(String),
    /// Reading the nodes failed while waiting for the gate.
    ReadError,
    /// The chord was used: no remote isolation until the daemon is restarted on purpose.
    EmergencyLatched,
}

impl Refusal {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::BadLease => "bad_lease",
            Self::KeysHeld => "keys_held",
            Self::NothingToGrab => "nothing_to_grab",
            Self::GrabFailed(_) => "grab_failed",
            Self::ReadError => "read_error",
            Self::EmergencyLatched => "emergency_latched",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Isolated {
        nodes: usize,
    },
    Refused(Refusal),
    Released(Reason),
    /// The emergency chord fired. Always reported, even when the release that follows failed, so
    /// the caller runs its own emergency actions regardless.
    ChordFired,
}

struct Gate {
    started_ms: u64,
    clear_since: Option<u64>,
    attempts: u32,
    lease_ms: u64,
}

pub struct Daemon<N: Nodes> {
    isolation: Isolation<N>,
    config: Config,
    chord: ChordDetector,
    phase: Phase,
    latched: bool,
    gate: Option<Gate>,
    known: BTreeMap<DeviceId, Caps>,
    last_rescan_ms: Option<u64>,
    /// Observations per node since the grab landed: counts only, never codes or positions.
    reads: BTreeMap<DeviceId, u64>,
}

impl<N: Nodes> Daemon<N> {
    pub fn new(nodes: N, config: Config) -> Self {
        let chord = ChordDetector::new(config.chord.clone());
        Self {
            isolation: Isolation::new(nodes),
            config,
            chord,
            phase: Phase::Idle,
            latched: false,
            gate: None,
            known: BTreeMap::new(),
            last_rescan_ms: None,
            reads: BTreeMap::new(),
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn held(&self) -> usize {
        self.isolation.held().len()
    }

    /// Key presses, releases and pointer passes the grabbed nodes saw since the last grab landed:
    /// the total and how many nodes saw any. Kept after a release until the next grab.
    pub fn reads(&self) -> (u64, usize) {
        (self.reads.values().sum(), self.reads.len())
    }

    /// True after an emergency chord: every further isolate is refused until the daemon restarts.
    pub fn is_latched(&self) -> bool {
        self.latched
    }

    pub fn nodes_mut(&mut self) -> &mut N {
        self.isolation.grabber_mut()
    }

    /// Starts the gate; the outcome arrives as an [`Event`] from [`Self::step`].
    pub fn isolate(&mut self, lease_ms: u64, now_ms: u64) -> Result<(), Refusal> {
        if self.latched {
            return Err(Refusal::EmergencyLatched);
        }
        if lease_ms < self.config.lease_min_ms || lease_ms > self.config.lease_max_ms {
            return Err(Refusal::BadLease);
        }
        if self.phase != Phase::Idle {
            return Err(Refusal::Busy);
        }
        self.chord.reset();
        self.phase = Phase::Gating;
        self.gate = Some(Gate {
            started_ms: now_ms,
            clear_since: None,
            attempts: 0,
            lease_ms,
        });
        Ok(())
    }

    /// Keeps the lease alive; only the controlling client may call this.
    pub fn renew(&mut self, now_ms: u64) {
        self.isolation.renew(now_ms);
    }

    /// Releases everything; safe to repeat. Returns the event to report, if any.
    pub fn restore(&mut self) -> Option<Event> {
        let was_active = self.phase != Phase::Idle;
        self.gate = None;
        self.chord.reset();
        self.isolation.restore();
        self.phase = Self::phase_after_release(self.isolation.state());
        (was_active && self.phase == Phase::Idle).then_some(Event::Released(Reason::Restore))
    }

    fn phase_after_release(state: IsolationState) -> Phase {
        if state == IsolationState::Idle {
            Phase::Idle
        } else {
            Phase::ReleaseFailed
        }
    }

    fn fail_closed(&mut self, reason: Reason, events: &mut Vec<Event>) {
        self.gate = None;
        self.chord.reset();
        self.isolation.restore();
        self.phase = Self::phase_after_release(self.isolation.state());
        events.push(Event::Released(if self.phase == Phase::Idle {
            reason
        } else {
            Reason::ReleaseFailed
        }));
    }

    fn feed_chord(&mut self, observed: &[Observed], now_ms: u64) -> u32 {
        let mut presses = 0;
        for item in observed {
            if let Observed::Key {
                node,
                code,
                pressed,
            } = *item
            {
                if pressed {
                    presses += 1;
                }
                let counts = self
                    .config
                    .chord_nodes
                    .as_ref()
                    .is_none_or(|nodes| nodes.contains(&node));
                if counts {
                    self.chord.key(code, pressed, now_ms);
                }
            }
        }
        presses
    }

    /// One pass of the daemon loop: read, then act on the current phase. Call every few ms.
    pub fn step(&mut self, now_ms: u64) -> Vec<Event> {
        let mut events = Vec::new();
        let observed = match self.isolation.grabber_mut().poll() {
            Ok(observed) => observed,
            Err(_) => {
                match self.phase {
                    Phase::Gating => self.refuse_after_release(Refusal::ReadError, &mut events),
                    Phase::Isolated | Phase::ReleaseFailed => {
                        self.fail_closed(Reason::ReadError, &mut events);
                    }
                    Phase::Idle => self.chord.reset(),
                }
                return events;
            }
        };
        if self.drop_removed(&observed, &mut events) {
            return events;
        }
        if self.phase == Phase::Isolated {
            self.count_reads(&observed);
        }
        let presses = self.feed_chord(&observed, now_ms);
        self.rescan(now_ms, &mut events);
        match self.phase {
            Phase::Idle => {}
            Phase::Gating => self.gate_step(now_ms, presses, &mut events),
            Phase::Isolated => self.isolated_step(now_ms, &mut events),
            Phase::ReleaseFailed => {}
        }
        // A release that failed earlier is retried on every pass until it works.
        if self.phase != Phase::Isolated && self.isolation.state() != IsolationState::Idle {
            self.isolation.restore();
            let recovered = self.isolation.state() == IsolationState::Idle;
            if recovered && self.phase == Phase::ReleaseFailed {
                events.push(Event::Released(Reason::ReleaseRecovered));
            }
            self.phase = Self::phase_after_release(self.isolation.state());
        }
        events
    }

    fn count_reads(&mut self, observed: &[Observed]) {
        for item in observed {
            if let Observed::Key { node, .. } | Observed::Activity { node } = *item {
                *self.reads.entry(node).or_default() += 1;
            }
        }
    }

    /// Releases and refuses the pending isolate.
    fn refuse_after_release(&mut self, refusal: Refusal, events: &mut Vec<Event>) {
        self.gate = None;
        self.chord.reset();
        self.isolation.restore();
        self.phase = Self::phase_after_release(self.isolation.state());
        events.push(Event::Refused(refusal));
    }

    /// A node that vanished is forgotten at once, so a new node reusing its id is seen as added.
    /// Returns true when that ended isolation.
    fn drop_removed(&mut self, observed: &[Observed], events: &mut Vec<Event>) -> bool {
        for item in observed {
            if let Observed::Removed { node } = *item {
                self.known.remove(&node);
                if self.phase == Phase::Isolated
                    && self.isolation.hotplug_remove(node) == HotplugOutcome::CoverageLost
                {
                    self.phase = Phase::Idle;
                    events.push(Event::Released(Reason::CoverageLost));
                    return true;
                }
            }
        }
        false
    }

    fn rescan(&mut self, now_ms: u64, events: &mut Vec<Event>) {
        if self
            .last_rescan_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < self.config.rescan_ms)
        {
            return;
        }
        self.last_rescan_ms = Some(now_ms);
        let current: BTreeMap<DeviceId, Caps> = self
            .isolation
            .grabber_mut()
            .candidates()
            .into_iter()
            .collect();
        let gone: Vec<DeviceId> = self
            .known
            .keys()
            .filter(|id| !current.contains_key(id))
            .copied()
            .collect();
        let added: Vec<DeviceId> = current
            .keys()
            .filter(|id| !self.known.contains_key(id))
            .copied()
            .collect();
        self.known = current;
        if self.phase != Phase::Isolated {
            return;
        }
        for id in gone {
            if self.isolation.hotplug_remove(id) == HotplugOutcome::CoverageLost {
                self.gate = None;
                self.phase = Phase::Idle;
                events.push(Event::Released(Reason::CoverageLost));
                return;
            }
        }
        for id in added {
            let caps = self.known.get(&id).cloned().unwrap_or_default();
            if let HotplugOutcome::FailClosed { .. } = self.isolation.hotplug_add(id, &caps) {
                self.phase = Self::phase_after_release(self.isolation.state());
                events.push(Event::Released(if self.phase == Phase::Idle {
                    Reason::HotplugFailClosed
                } else {
                    Reason::ReleaseFailed
                }));
                return;
            }
        }
    }

    fn isolated_step(&mut self, now_ms: u64, events: &mut Vec<Event>) {
        if let Some(TickEvent::LeaseExpired(_)) = self.isolation.tick(now_ms) {
            self.phase = Self::phase_after_release(self.isolation.state());
            events.push(Event::Released(if self.phase == Phase::Idle {
                Reason::LeaseExpired
            } else {
                Reason::ReleaseFailed
            }));
            return;
        }
        if self.chord.poll(now_ms) {
            self.latched = true;
            events.push(Event::ChordFired);
            self.fail_closed(Reason::Chord, events);
        }
    }

    fn gate_step(&mut self, now_ms: u64, presses: u32, events: &mut Vec<Event>) {
        let keys_down = self.isolation.grabber().keys_down();
        let Some(gate) = self.gate.as_mut() else {
            self.phase = Phase::Idle;
            return;
        };
        if keys_down > 0 || presses > 0 {
            gate.clear_since = None;
        } else {
            gate.clear_since.get_or_insert(now_ms);
        }
        if now_ms.saturating_sub(gate.started_ms) > self.config.gate_timeout_ms {
            self.refuse_after_release(Refusal::KeysHeld, events);
            return;
        }
        let stable = gate
            .clear_since
            .is_some_and(|since| now_ms.saturating_sub(since) >= self.config.gate_stable_ms);
        if !stable {
            return;
        }
        let lease_ms = gate.lease_ms;
        let candidates = self.isolation.grabber_mut().candidates();
        match self.isolation.isolate(&candidates, now_ms, lease_ms) {
            Ok(held) => self.after_grab(now_ms, held.len(), events),
            Err(IsolateError::NothingToGrab) => self.refuse(Refusal::NothingToGrab, events),
            Err(IsolateError::GrabFailed { error, .. }) => {
                self.refuse(Refusal::GrabFailed(error.0), events);
            }
            Err(IsolateError::NotIdle(_)) => {
                self.isolation.restore();
                self.refuse(Refusal::Busy, events);
            }
        }
    }

    fn refuse(&mut self, refusal: Refusal, events: &mut Vec<Event>) {
        self.gate = None;
        self.phase = Self::phase_after_release(self.isolation.state());
        events.push(Event::Refused(refusal));
    }

    /// A key pressed in the gap before the grab landed would be seen as down by the session and
    /// never released (it would auto-repeat), so any press or key still down undoes the grab.
    fn after_grab(&mut self, now_ms: u64, nodes: usize, events: &mut Vec<Event>) {
        let observed = self.isolation.grabber_mut().poll().unwrap_or_default();
        let presses = self.feed_chord(&observed, now_ms);
        let tainted = presses > 0 || self.isolation.grabber().keys_down() > 0;
        if !tainted {
            self.gate = None;
            self.reads.clear();
            self.phase = Phase::Isolated;
            events.push(Event::Isolated { nodes });
            return;
        }
        self.isolation.restore();
        if self.isolation.state() != IsolationState::Idle {
            self.gate = None;
            self.phase = Phase::ReleaseFailed;
            events.push(Event::Released(Reason::ReleaseFailed));
            return;
        }
        let attempts = self.gate.as_mut().map_or(self.config.max_attempts, |gate| {
            gate.attempts += 1;
            gate.clear_since = None;
            gate.attempts
        });
        if attempts >= self.config.max_attempts {
            self.refuse(Refusal::KeysHeld, events);
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    use remote_input_helper::{Caps, DeviceGrab, DeviceId, GrabError};

    use super::{Nodes, Observed};

    #[derive(Default)]
    pub(crate) struct Fake {
        pub(crate) caps: BTreeMap<DeviceId, Caps>,
        pub(crate) down: usize,
        pub(crate) queue: VecDeque<Observed>,
        pub(crate) grabbed: BTreeSet<DeviceId>,
        pub(crate) fail_grab: bool,
        pub(crate) fail_release: bool,
        pub(crate) fail_poll: bool,
        pub(crate) press_on_grab: u32,
        /// Presses the four chord keys a few reads after the grab has landed.
        pub(crate) chord_when_grabbed: bool,
        grabbed_polls: u32,
    }

    impl DeviceGrab for Fake {
        fn grab(&mut self, id: DeviceId) -> Result<(), GrabError> {
            if self.fail_grab {
                return Err(GrabError("busy".to_string()));
            }
            if self.press_on_grab > 0 {
                self.press_on_grab -= 1;
                self.queue.push_back(Observed::Key {
                    node: id,
                    code: 30,
                    pressed: true,
                });
            }
            self.grabbed.insert(id);
            Ok(())
        }

        fn release(&mut self, id: DeviceId) -> Result<(), GrabError> {
            if self.fail_release {
                return Err(GrabError("ioctl".to_string()));
            }
            self.grabbed.remove(&id);
            Ok(())
        }
    }

    impl Nodes for Fake {
        fn candidates(&mut self) -> Vec<(DeviceId, Caps)> {
            self.caps.iter().map(|(id, c)| (*id, c.clone())).collect()
        }

        fn keys_down(&self) -> usize {
            self.down
        }

        fn poll(&mut self) -> Result<Vec<Observed>, String> {
            if self.fail_poll {
                return Err("read".to_string());
            }
            if self.grabbed.is_empty() {
                self.grabbed_polls = 0;
            } else {
                self.grabbed_polls += 1;
                if self.chord_when_grabbed && self.grabbed_polls >= 3 {
                    self.chord_when_grabbed = false;
                    self.queue.extend([29, 42, 56, 1].map(|code| Observed::Key {
                        node: 2,
                        code,
                        pressed: true,
                    }));
                }
            }
            Ok(self.queue.drain(..).collect())
        }
    }

    pub(crate) fn keyboard() -> Caps {
        Caps {
            keys: (16..=50).collect(),
            seat0: true,
            ..Caps::default()
        }
    }

    pub(crate) fn fake() -> Fake {
        Fake {
            caps: BTreeMap::from([(2, keyboard()), (6, keyboard())]),
            ..Fake::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{Fake, fake, keyboard};
    use super::*;

    fn cfg() -> Config {
        Config {
            gate_stable_ms: 100,
            gate_timeout_ms: 1_000,
            rescan_ms: 50,
            ..Config::standard()
        }
    }

    /// Steps every 10 ms from `from` to `to` and returns all events.
    fn run(daemon: &mut Daemon<Fake>, from: u64, to: u64) -> Vec<Event> {
        (from..=to)
            .step_by(10)
            .flat_map(|now| daemon.step(now))
            .collect()
    }

    fn isolated(daemon: &mut Daemon<Fake>) {
        daemon.isolate(10_000, 0).expect("accepted");
        let events = run(daemon, 0, 300);
        assert_eq!(events, vec![Event::Isolated { nodes: 2 }]);
        assert_eq!(daemon.phase(), Phase::Isolated);
    }

    fn chord_keys(pressed: bool) -> Vec<Observed> {
        [29, 42, 56, 1]
            .into_iter()
            .map(|code| Observed::Key {
                node: 2,
                code,
                pressed,
            })
            .collect()
    }

    #[test]
    fn the_grab_waits_until_everything_is_up_and_stays_up() {
        let mut daemon = Daemon::new(fake(), cfg());
        daemon.nodes_mut().down = 1;
        daemon.isolate(10_000, 0).expect("accepted");
        assert!(run(&mut daemon, 0, 500).is_empty());
        assert_eq!(daemon.phase(), Phase::Gating);
        assert_eq!(daemon.held(), 0);

        daemon.nodes_mut().down = 0;
        assert!(run(&mut daemon, 510, 590).is_empty(), "not stable yet");
        // A press inside the window restarts it.
        daemon.nodes_mut().queue.push_back(Observed::Key {
            node: 2,
            code: 30,
            pressed: true,
        });
        assert!(run(&mut daemon, 600, 690).is_empty());
        assert_eq!(
            run(&mut daemon, 700, 800),
            vec![Event::Isolated { nodes: 2 }]
        );
    }

    #[test]
    fn a_press_in_the_gap_before_the_grab_undoes_it_and_retries() {
        let mut fake = fake();
        fake.press_on_grab = 1;
        let mut daemon = Daemon::new(fake, cfg());
        daemon.isolate(10_000, 0).expect("accepted");
        let events = run(&mut daemon, 0, 600);
        assert_eq!(events, vec![Event::Isolated { nodes: 2 }]);
        assert_eq!(
            daemon.nodes_mut().press_on_grab,
            0,
            "first attempt was undone"
        );
    }

    #[test]
    fn a_key_that_keeps_getting_pressed_ends_in_a_refusal_with_nothing_held() {
        let mut fake = fake();
        fake.press_on_grab = 100;
        let mut daemon = Daemon::new(fake, cfg());
        daemon.isolate(10_000, 0).expect("accepted");
        let events = run(&mut daemon, 0, 2_000);
        assert_eq!(events, vec![Event::Refused(Refusal::KeysHeld)]);
        assert_eq!(daemon.phase(), Phase::Idle);
        assert!(daemon.nodes_mut().grabbed.is_empty());
    }

    #[test]
    fn a_key_held_past_the_gate_timeout_is_refused() {
        let mut daemon = Daemon::new(fake(), cfg());
        daemon.nodes_mut().down = 2;
        daemon.isolate(10_000, 0).expect("accepted");
        let events = run(&mut daemon, 0, 1_200);
        assert_eq!(events, vec![Event::Refused(Refusal::KeysHeld)]);
        assert_eq!(daemon.phase(), Phase::Idle);
    }

    #[test]
    fn bad_leases_and_concurrent_isolation_are_refused_up_front() {
        let mut daemon = Daemon::new(fake(), cfg());
        assert_eq!(daemon.isolate(10, 0), Err(Refusal::BadLease));
        assert_eq!(daemon.isolate(10_000_000, 0), Err(Refusal::BadLease));
        assert_eq!(daemon.isolate(5_000, 0), Ok(()));
        assert_eq!(daemon.isolate(5_000, 0), Err(Refusal::Busy));
    }

    #[test]
    fn nothing_to_grab_and_a_failed_grab_leave_nothing_held() {
        let mut empty = Daemon::new(Fake::default(), cfg());
        empty.isolate(5_000, 0).expect("accepted");
        assert_eq!(
            run(&mut empty, 0, 300),
            vec![Event::Refused(Refusal::NothingToGrab)]
        );

        let mut fake = fake();
        fake.fail_grab = true;
        let mut busy = Daemon::new(fake, cfg());
        busy.isolate(5_000, 0).expect("accepted");
        let events = run(&mut busy, 0, 300);
        assert_eq!(
            events,
            vec![Event::Refused(Refusal::GrabFailed("busy".to_string()))]
        );
        assert_eq!(busy.phase(), Phase::Idle);
    }

    #[test]
    fn the_lease_lapses_unless_renewed() {
        let mut daemon = Daemon::new(fake(), cfg());
        isolated(&mut daemon);
        daemon.renew(9_000);
        assert!(run(&mut daemon, 310, 18_000).is_empty());
        let events = run(&mut daemon, 18_010, 20_000);
        assert_eq!(events, vec![Event::Released(Reason::LeaseExpired)]);
        assert_eq!(daemon.phase(), Phase::Idle);
        assert!(daemon.nodes_mut().grabbed.is_empty());
    }

    #[test]
    fn the_chord_releases_after_its_hold_time_and_only_while_isolated() {
        let mut daemon = Daemon::new(fake(), cfg());
        // Not isolated: the chord does nothing.
        daemon.nodes_mut().queue.extend(chord_keys(true));
        assert!(run(&mut daemon, 0, 2_500).is_empty());
        daemon.nodes_mut().queue.extend(chord_keys(false));
        run(&mut daemon, 2_510, 2_600);

        isolated_from(&mut daemon, 3_000);
        daemon.nodes_mut().queue.extend(chord_keys(true));
        assert!(
            run(&mut daemon, 3_310, 4_500).is_empty(),
            "held too briefly"
        );
        let events = run(&mut daemon, 4_510, 5_600);
        assert_eq!(
            events,
            vec![Event::ChordFired, Event::Released(Reason::Chord)]
        );
        assert_eq!(daemon.phase(), Phase::Idle);
        assert!(daemon.nodes_mut().grabbed.is_empty());
    }

    #[test]
    fn a_chord_from_the_wrong_node_is_ignored_when_nodes_are_restricted() {
        let config = Config {
            chord_nodes: Some(BTreeSet::from([2])),
            ..cfg()
        };
        let mut daemon = Daemon::new(fake(), config);
        isolated(&mut daemon);
        daemon
            .nodes_mut()
            .queue
            .extend(chord_keys(true).into_iter().map(|item| match item {
                Observed::Key { code, pressed, .. } => Observed::Key {
                    node: 6,
                    code,
                    pressed,
                },
                other => other,
            }));
        assert!(run(&mut daemon, 310, 3_000).is_empty());
        assert_eq!(daemon.phase(), Phase::Isolated);
    }

    #[test]
    fn reads_count_what_the_grabbed_nodes_saw_only_while_isolated() {
        let mut daemon = Daemon::new(fake(), cfg());
        daemon
            .nodes_mut()
            .queue
            .push_back(Observed::Activity { node: 2 });
        isolated(&mut daemon);
        assert_eq!(
            daemon.reads(),
            (0, 0),
            "events before the grab landed are not counted"
        );
        let nodes = daemon.nodes_mut();
        nodes.queue.extend([
            Observed::Key {
                node: 2,
                code: 30,
                pressed: true,
            },
            Observed::Key {
                node: 2,
                code: 30,
                pressed: false,
            },
            Observed::Activity { node: 6 },
        ]);
        daemon.step(400);
        assert_eq!(daemon.reads(), (3, 2));
        daemon.restore();
        assert_eq!(
            daemon.reads(),
            (3, 2),
            "kept for the client until the next grab"
        );
        daemon
            .nodes_mut()
            .queue
            .push_back(Observed::Activity { node: 6 });
        daemon.step(500);
        assert_eq!(daemon.reads(), (3, 2), "nothing is counted once released");
        isolated_from(&mut daemon, 600);
        assert_eq!(daemon.reads(), (0, 0), "the next grab starts a fresh count");
    }

    #[test]
    fn a_read_error_while_isolated_fails_closed() {
        let mut daemon = Daemon::new(fake(), cfg());
        isolated(&mut daemon);
        daemon.nodes_mut().fail_poll = true;
        assert_eq!(daemon.step(400), vec![Event::Released(Reason::ReadError)]);
        assert!(daemon.nodes_mut().grabbed.is_empty());
        assert_eq!(daemon.phase(), Phase::Idle);
    }

    #[test]
    fn hotplug_grabs_new_nodes_and_fails_closed_when_one_cannot_be_grabbed() {
        let mut daemon = Daemon::new(fake(), cfg());
        isolated(&mut daemon);
        daemon.renew(300);
        daemon.nodes_mut().caps.insert(9, keyboard());
        run(&mut daemon, 310, 400);
        assert!(daemon.nodes_mut().grabbed.contains(&9));
        assert_eq!(daemon.held(), 3);

        daemon.nodes_mut().fail_grab = true;
        daemon.nodes_mut().caps.insert(10, keyboard());
        let events = run(&mut daemon, 410, 500);
        assert_eq!(events, vec![Event::Released(Reason::HotplugFailClosed)]);
        assert!(daemon.nodes_mut().grabbed.is_empty());
        assert_eq!(daemon.phase(), Phase::Idle);
    }

    #[test]
    fn losing_every_grabbed_node_reports_lost_coverage() {
        let mut daemon = Daemon::new(fake(), cfg());
        isolated(&mut daemon);
        daemon.nodes_mut().caps.clear();
        let events = run(&mut daemon, 310, 400);
        assert_eq!(events, vec![Event::Released(Reason::CoverageLost)]);
        assert_eq!(daemon.phase(), Phase::Idle);
    }

    #[test]
    fn restore_cancels_a_gate_and_a_grab_and_a_failed_release_is_retried() {
        let mut daemon = Daemon::new(fake(), cfg());
        daemon.nodes_mut().down = 1;
        daemon.isolate(5_000, 0).expect("accepted");
        run(&mut daemon, 0, 50);
        assert_eq!(daemon.restore(), Some(Event::Released(Reason::Restore)));
        assert_eq!(daemon.phase(), Phase::Idle);
        assert_eq!(daemon.restore(), None, "idempotent");

        daemon.nodes_mut().down = 0;
        isolated_from(&mut daemon, 1_000);
        daemon.nodes_mut().fail_release = true;
        assert_eq!(
            daemon.restore(),
            None,
            "a failed release is not reported as released"
        );
        assert_eq!(daemon.phase(), Phase::ReleaseFailed);
        daemon.nodes_mut().fail_release = false;
        daemon.step(2_000);
        assert_eq!(daemon.phase(), Phase::Idle);
        assert!(daemon.nodes_mut().grabbed.is_empty());
    }

    #[test]
    fn after_the_chord_isolation_is_refused_until_the_daemon_is_restarted() {
        let mut daemon = Daemon::new(fake(), cfg());
        isolated_from(&mut daemon, 0);
        daemon.nodes_mut().queue.extend(chord_keys(true));
        let events = run(&mut daemon, 310, 2_600);
        assert!(events.contains(&Event::ChordFired));
        assert_eq!(daemon.isolate(5_000, 3_000), Err(Refusal::EmergencyLatched));
    }

    #[test]
    fn the_chord_is_reported_even_when_the_release_fails_and_the_release_is_retried() {
        let mut daemon = Daemon::new(fake(), cfg());
        isolated_from(&mut daemon, 0);
        daemon.nodes_mut().fail_release = true;
        daemon.nodes_mut().queue.extend(chord_keys(true));
        let events = run(&mut daemon, 310, 2_600);
        assert_eq!(
            events,
            vec![Event::ChordFired, Event::Released(Reason::ReleaseFailed)]
        );
        assert_eq!(daemon.phase(), Phase::ReleaseFailed);
        daemon.nodes_mut().fail_release = false;
        assert_eq!(
            daemon.step(2_700),
            vec![Event::Released(Reason::ReleaseRecovered)]
        );
        assert_eq!(daemon.phase(), Phase::Idle);
        assert!(daemon.nodes_mut().grabbed.is_empty());
    }

    #[test]
    fn a_failed_release_after_a_tainted_grab_or_a_gate_timeout_is_still_retried() {
        let mut fake = fake();
        fake.press_on_grab = 1;
        fake.fail_release = true;
        let mut daemon = Daemon::new(fake, cfg());
        daemon.isolate(5_000, 0).expect("accepted");
        let events = run(&mut daemon, 0, 300);
        assert_eq!(events, vec![Event::Released(Reason::ReleaseFailed)]);
        assert_eq!(daemon.phase(), Phase::ReleaseFailed);
        daemon.nodes_mut().fail_release = false;
        assert_eq!(
            run(&mut daemon, 310, 400),
            vec![Event::Released(Reason::ReleaseRecovered)]
        );
        assert!(daemon.nodes_mut().grabbed.is_empty());
    }

    #[test]
    fn a_read_error_while_waiting_for_the_gate_refuses_the_isolate() {
        let mut daemon = Daemon::new(fake(), cfg());
        daemon.nodes_mut().down = 1;
        daemon.isolate(5_000, 0).expect("accepted");
        daemon.nodes_mut().fail_poll = true;
        assert_eq!(daemon.step(10), vec![Event::Refused(Refusal::ReadError)]);
        assert_eq!(daemon.phase(), Phase::Idle);
    }

    #[test]
    fn a_node_replaced_under_the_same_id_is_grabbed_again() {
        let mut daemon = Daemon::new(fake(), cfg());
        isolated_from(&mut daemon, 0);
        // Node 6 goes away and a new node reuses its id before the next rescan.
        daemon.nodes_mut().grabbed.remove(&6);
        daemon
            .nodes_mut()
            .queue
            .push_back(Observed::Removed { node: 6 });
        run(&mut daemon, 310, 400);
        assert!(
            daemon.nodes_mut().grabbed.contains(&6),
            "the replacement was grabbed"
        );
        assert_eq!(daemon.held(), 2);
    }

    fn isolated_from(daemon: &mut Daemon<Fake>, start: u64) {
        daemon.isolate(10_000, start).expect("accepted");
        let events = run(daemon, start, start + 300);
        assert_eq!(events, vec![Event::Isolated { nodes: 2 }]);
    }
}
