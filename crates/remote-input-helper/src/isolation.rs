//! Exclusive-grab state machine: all-or-nothing isolate, fail-closed hotplug,
//! a dead-man lease, and a release that keeps retrying what failed.

use std::collections::BTreeMap;

use crate::classify::{Caps, Classification, Role, classify};

pub type DeviceId = u32;

/// Failure text from the grabber; must never carry key or pointer data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrabError(pub String);

/// `EVIOCGRAB` on a node, implemented by the privileged caller.
pub trait DeviceGrab {
    fn grab(&mut self, id: DeviceId) -> Result<(), GrabError>;
    fn release(&mut self, id: DeviceId) -> Result<(), GrabError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Isolated,
    /// A release failed; some local input may still be grabbed.
    ReleaseFailed,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RestoreOutcome {
    pub released: Vec<DeviceId>,
    pub failed: Vec<(DeviceId, GrabError)>,
}

impl RestoreOutcome {
    pub fn is_clean(&self) -> bool {
        self.failed.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsolateError {
    NotIdle(State),
    /// No allow-listed physical device: nothing would be isolated.
    NothingToGrab,
    GrabFailed {
        id: DeviceId,
        error: GrabError,
        release: RestoreOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotplugOutcome {
    NotIsolated,
    Ignored,
    Grabbed(DeviceId),
    /// Still covered after a grabbed node went away.
    Removed,
    /// The last grabbed node went away; isolation no longer covers anything.
    CoverageLost,
    /// A new allow-listed node could not be grabbed: everything was released.
    FailClosed {
        id: DeviceId,
        error: GrabError,
        release: RestoreOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickEvent {
    LeaseExpired(RestoreOutcome),
}

pub struct Isolation<G: DeviceGrab> {
    grabber: G,
    state: State,
    held: BTreeMap<DeviceId, Vec<Role>>,
    lease_ms: u64,
    deadline_ms: u64,
}

impl<G: DeviceGrab> Isolation<G> {
    pub fn new(grabber: G) -> Self {
        Self {
            grabber,
            state: State::Idle,
            held: BTreeMap::new(),
            lease_ms: 0,
            deadline_ms: 0,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn held(&self) -> Vec<DeviceId> {
        self.held.keys().copied().collect()
    }

    pub fn grabber(&self) -> &G {
        &self.grabber
    }

    pub fn grabber_mut(&mut self) -> &mut G {
        &mut self.grabber
    }

    fn release_held(&mut self) -> RestoreOutcome {
        let mut outcome = RestoreOutcome::default();
        for id in self.held.keys().copied().collect::<Vec<_>>() {
            match self.grabber.release(id) {
                Ok(()) => {
                    self.held.remove(&id);
                    outcome.released.push(id);
                }
                Err(error) => outcome.failed.push((id, error)),
            }
        }
        self.state = if self.held.is_empty() {
            State::Idle
        } else {
            State::ReleaseFailed
        };
        outcome
    }

    /// Grabs every allow-listed node or none. `lease_ms` is the dead-man window:
    /// isolation lapses unless [`Self::renew`] is called within it.
    pub fn isolate(
        &mut self,
        devices: &[(DeviceId, Caps)],
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<Vec<DeviceId>, IsolateError> {
        if self.state != State::Idle {
            return Err(IsolateError::NotIdle(self.state));
        }
        let mut targets: Vec<(DeviceId, Vec<Role>)> = devices
            .iter()
            .filter_map(|(id, caps)| match classify(caps) {
                Classification::Grab(roles) => Some((*id, roles)),
                Classification::Skip(_) => None,
            })
            .collect();
        targets.sort_by_key(|(id, _)| *id);
        if targets.is_empty() {
            return Err(IsolateError::NothingToGrab);
        }
        for (id, roles) in targets {
            if let Err(error) = self.grabber.grab(id) {
                let release = self.release_held();
                return Err(IsolateError::GrabFailed { id, error, release });
            }
            self.held.insert(id, roles);
        }
        self.state = State::Isolated;
        self.lease_ms = lease_ms;
        self.deadline_ms = now_ms.saturating_add(lease_ms);
        Ok(self.held())
    }

    /// Releases everything; safe to repeat and retries any earlier failure.
    pub fn restore(&mut self) -> RestoreOutcome {
        self.release_held()
    }

    pub fn renew(&mut self, now_ms: u64) {
        if self.state == State::Isolated {
            self.deadline_ms = now_ms.saturating_add(self.lease_ms);
        }
    }

    /// Call from a loop independent of event reading so a stalled reader cannot
    /// keep the lease alive.
    pub fn tick(&mut self, now_ms: u64) -> Option<TickEvent> {
        if self.state == State::Isolated && now_ms >= self.deadline_ms {
            return Some(TickEvent::LeaseExpired(self.release_held()));
        }
        None
    }

    pub fn hotplug_add(&mut self, id: DeviceId, caps: &Caps) -> HotplugOutcome {
        if self.state != State::Isolated {
            return HotplugOutcome::NotIsolated;
        }
        let Classification::Grab(roles) = classify(caps) else {
            return HotplugOutcome::Ignored;
        };
        if self.held.contains_key(&id) {
            return HotplugOutcome::Ignored;
        }
        match self.grabber.grab(id) {
            Ok(()) => {
                self.held.insert(id, roles);
                HotplugOutcome::Grabbed(id)
            }
            Err(error) => {
                let release = self.release_held();
                HotplugOutcome::FailClosed { id, error, release }
            }
        }
    }

    /// The kernel already dropped a removed node's grab; forget it.
    pub fn hotplug_remove(&mut self, id: DeviceId) -> HotplugOutcome {
        if self.state != State::Isolated {
            return HotplugOutcome::NotIsolated;
        }
        if self.held.remove(&id).is_none() {
            return HotplugOutcome::Ignored;
        }
        if self.held.is_empty() {
            self.state = State::Idle;
            return HotplugOutcome::CoverageLost;
        }
        HotplugOutcome::Removed
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[derive(Default)]
    struct Fake {
        grabbed: BTreeSet<DeviceId>,
        fail_grab: BTreeSet<DeviceId>,
        fail_release: BTreeSet<DeviceId>,
    }

    impl DeviceGrab for Fake {
        fn grab(&mut self, id: DeviceId) -> Result<(), GrabError> {
            if self.fail_grab.contains(&id) {
                return Err(GrabError("busy".to_string()));
            }
            self.grabbed.insert(id);
            Ok(())
        }

        fn release(&mut self, id: DeviceId) -> Result<(), GrabError> {
            if self.fail_release.contains(&id) {
                return Err(GrabError("ioctl failed".to_string()));
            }
            self.grabbed.remove(&id);
            Ok(())
        }
    }

    fn keyboard() -> Caps {
        Caps {
            keys: [
                16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 30, 31, 32, 33, 34, 35, 36, 37, 38, 44, 45,
                46, 47, 48, 49, 50,
            ]
            .into_iter()
            .collect(),
            seat0: true,
            ..Caps::default()
        }
    }

    fn mouse() -> Caps {
        Caps {
            keys: [0x110].into_iter().collect(),
            rel_axes: [0, 1].into_iter().collect(),
            seat0: true,
            ..Caps::default()
        }
    }

    fn power() -> Caps {
        Caps {
            keys: [116].into_iter().collect(),
            seat0: true,
            ..Caps::default()
        }
    }

    fn devices() -> Vec<(DeviceId, Caps)> {
        vec![(3, mouse()), (1, keyboard()), (2, power())]
    }

    fn isolated() -> Isolation<Fake> {
        let mut isolation = Isolation::new(Fake::default());
        isolation.isolate(&devices(), 0, 1000).expect("isolate");
        isolation
    }

    #[test]
    fn isolate_grabs_only_allow_listed_nodes() {
        let isolation = isolated();
        assert_eq!(isolation.state(), State::Isolated);
        assert_eq!(isolation.held(), vec![1, 3]);
        assert_eq!(isolation.grabber().grabbed, BTreeSet::from([1, 3]));
    }

    #[test]
    fn one_failed_grab_releases_everything_taken_and_stays_idle() {
        let mut isolation = Isolation::new(Fake {
            fail_grab: BTreeSet::from([3]),
            ..Fake::default()
        });
        let error = isolation.isolate(&devices(), 0, 1000).expect_err("fails");
        let IsolateError::GrabFailed { id, release, .. } = error else {
            panic!("unexpected error");
        };
        assert_eq!(id, 3);
        assert_eq!(release.released, vec![1]);
        assert_eq!(isolation.state(), State::Idle);
        assert!(isolation.grabber().grabbed.is_empty());
    }

    #[test]
    fn a_failed_rollback_release_is_reported_and_retried() {
        let mut isolation = Isolation::new(Fake {
            fail_grab: BTreeSet::from([3]),
            fail_release: BTreeSet::from([1]),
            ..Fake::default()
        });
        let Err(IsolateError::GrabFailed { release, .. }) = isolation.isolate(&devices(), 0, 1000)
        else {
            panic!("expected a grab failure");
        };
        assert!(!release.is_clean());
        assert_eq!(isolation.state(), State::ReleaseFailed);
        assert_eq!(
            isolation.isolate(&devices(), 0, 1000),
            Err(IsolateError::NotIdle(State::ReleaseFailed))
        );
        isolation.grabber.fail_release.clear();
        assert!(isolation.restore().is_clean());
        assert_eq!(isolation.state(), State::Idle);
    }

    #[test]
    fn restore_continues_past_a_failure_and_is_idempotent() {
        let mut isolation = isolated();
        isolation.grabber.fail_release.insert(1);
        let outcome = isolation.restore();
        assert_eq!(outcome.released, vec![3]);
        assert_eq!(outcome.failed.len(), 1);
        assert_eq!(isolation.state(), State::ReleaseFailed);
        isolation.grabber.fail_release.clear();
        assert!(isolation.restore().is_clean());
        assert!(isolation.restore().released.is_empty());
        assert_eq!(isolation.state(), State::Idle);
    }

    #[test]
    fn isolating_twice_or_with_nothing_to_grab_is_refused() {
        let mut isolation = isolated();
        assert_eq!(
            isolation.isolate(&devices(), 0, 1000),
            Err(IsolateError::NotIdle(State::Isolated))
        );
        let mut empty = Isolation::new(Fake::default());
        assert_eq!(
            empty.isolate(&[(2, power())], 0, 1000),
            Err(IsolateError::NothingToGrab)
        );
        assert_eq!(empty.state(), State::Idle);
    }

    #[test]
    fn hotplug_grabs_new_nodes_ignores_others_and_fails_closed_on_error() {
        let mut isolation = Isolation::new(Fake::default());
        assert_eq!(
            isolation.hotplug_add(9, &keyboard()),
            HotplugOutcome::NotIsolated
        );
        isolation.isolate(&devices(), 0, 1000).expect("isolate");
        assert_eq!(
            isolation.hotplug_add(9, &keyboard()),
            HotplugOutcome::Grabbed(9)
        );
        assert_eq!(isolation.hotplug_add(8, &power()), HotplugOutcome::Ignored);
        assert_eq!(
            isolation.hotplug_add(9, &keyboard()),
            HotplugOutcome::Ignored
        );
        isolation.grabber.fail_grab.insert(10);
        let HotplugOutcome::FailClosed { id, release, .. } = isolation.hotplug_add(10, &keyboard())
        else {
            panic!("expected fail closed");
        };
        assert_eq!(id, 10);
        assert!(release.is_clean());
        assert_eq!(isolation.state(), State::Idle);
        assert!(isolation.grabber().grabbed.is_empty());
    }

    #[test]
    fn removing_the_last_grabbed_node_reports_lost_coverage() {
        let mut isolation = isolated();
        assert_eq!(isolation.hotplug_remove(2), HotplugOutcome::Ignored);
        assert_eq!(isolation.hotplug_remove(1), HotplugOutcome::Removed);
        assert_eq!(isolation.hotplug_remove(3), HotplugOutcome::CoverageLost);
        assert_eq!(isolation.state(), State::Idle);
    }

    #[test]
    fn the_lease_lapses_unless_renewed_and_only_while_isolated() {
        let mut isolation = isolated();
        assert_eq!(isolation.tick(999), None);
        isolation.renew(900);
        assert_eq!(isolation.tick(1899), None);
        let Some(TickEvent::LeaseExpired(outcome)) = isolation.tick(1900) else {
            panic!("expected expiry");
        };
        assert!(outcome.is_clean());
        assert_eq!(isolation.state(), State::Idle);
        assert!(isolation.grabber().grabbed.is_empty());
        assert_eq!(isolation.tick(5000), None);
        isolation.renew(5000);
        assert_eq!(isolation.state(), State::Idle);
    }
}
