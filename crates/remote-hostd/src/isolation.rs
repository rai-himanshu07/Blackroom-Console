//! Hostd's side of physical-input isolation: it owns the only client of the emergency input daemon.
//!
//! The daemon's grab is a dead-man's switch. It lapses by itself unless hostd keeps renewing, so
//! the renewal rule below decides when local input comes back if the remote side disappears.
//! Nothing here runs unless hostd is started with an emergency socket, and the daemon itself
//! refuses to grab unless it was started with `--enable-grabs`.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use remote_emergency_client::client::{Client, Outcome};

/// How long the daemon keeps the grab without a renewal from hostd.
pub const ISOLATION_LEASE: Duration = Duration::from_secs(10);
/// A browser renew that the agent acknowledged is the heartbeat. The daemon lease is renewed only
/// while the newest one is at most this old, so a dead link or agent returns local input within
/// this window plus [`ISOLATION_LEASE`], which stays under the 30 s control lease.
pub const HEARTBEAT_WINDOW: Duration = Duration::from_secs(15);
/// How often a healthy grab is renewed and checked.
const TICK: Duration = Duration::from_secs(2);
/// The daemon waits up to 20 s for every key to be up before it grabs.
const ENGAGE_TIMEOUT: Duration = Duration::from_secs(25);
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// The operations hostd needs from the daemon; reasons are short fixed codes, never key data.
pub trait InputIsolation {
    fn engage(&mut self, lease: Duration) -> Result<(), String>;
    fn renew(&mut self) -> Result<(), String>;
    /// `Some(reason)` once the daemon no longer holds the grab or cannot be reached.
    fn lost(&mut self) -> Option<String>;
    /// Idempotent and best effort; the daemon also releases when the connection closes.
    fn release(&mut self);
}

/// Talks to a running `remote-emergencyd`. The daemon serves one client, so the connection is
/// opened on `engage` and closed on `release`, which keeps `blackroom emergency-status` usable
/// while nothing is isolated.
pub struct DaemonIsolation {
    socket: PathBuf,
    client: Option<Client>,
}

impl DaemonIsolation {
    pub fn new(socket: PathBuf) -> io::Result<Self> {
        if !socket.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "emergency socket path must be absolute",
            ));
        }
        Ok(Self {
            socket,
            client: None,
        })
    }
}

/// The daemon serves one client and drops a newcomer until it has noticed the previous one is gone,
/// which takes a loop pass after a release; connection-level failures are retried briefly.
const ENGAGE_ATTEMPTS: u32 = 10;
const ENGAGE_RETRY_PAUSE: Duration = Duration::from_millis(100);

enum Attempt {
    Retry(&'static str),
    Refused(String),
}

/// Only a connection that was dropped is retried; a timeout or a bad reply is final, so one hung
/// daemon cannot hold hostd's single loop for more than one `ENGAGE_TIMEOUT`.
fn dropped(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
    )
}

/// The daemon must run as hostd's own user, like the stop marker it writes; anything else that
/// answers on this path could claim a grab that does not exist.
fn peer_is_hostd(client: &Client) -> bool {
    rustix::net::sockopt::socket_peercred(client.socket())
        .is_ok_and(|peer| peer.uid.as_raw() == rustix::process::getuid().as_raw())
}

impl DaemonIsolation {
    fn attempt(&self, lease_ms: u64) -> Result<Client, Attempt> {
        let mut client = Client::connect(&self.socket, ENGAGE_TIMEOUT)
            .map_err(|_| Attempt::Retry("daemon_unreachable"))?;
        if !peer_is_hostd(&client) {
            return Err(Attempt::Refused("daemon_untrusted".to_string()));
        }
        match client.isolate(lease_ms) {
            Ok(Outcome::Isolated { .. }) => Ok(client),
            Ok(Outcome::Refused(reason) | Outcome::Error(reason)) => Err(Attempt::Refused(reason)),
            Err(error) if dropped(&error) => Err(Attempt::Retry("daemon_io")),
            Err(_) => Err(Attempt::Refused("daemon_io".to_string())),
        }
    }
}

impl InputIsolation for DaemonIsolation {
    fn engage(&mut self, lease: Duration) -> Result<(), String> {
        self.release();
        let lease_ms = u64::try_from(lease.as_millis()).unwrap_or(u64::MAX);
        let mut attempts = 0;
        let mut client = loop {
            match self.attempt(lease_ms) {
                Ok(client) => break client,
                Err(Attempt::Refused(reason)) => return Err(reason),
                Err(Attempt::Retry(reason)) => {
                    attempts += 1;
                    if attempts >= ENGAGE_ATTEMPTS {
                        return Err(reason.to_string());
                    }
                    std::thread::sleep(ENGAGE_RETRY_PAUSE);
                }
            }
        };
        client
            .set_reply_timeout(REPLY_TIMEOUT)
            .map_err(|_| "daemon_io".to_string())?;
        self.client = Some(client);
        Ok(())
    }

    fn renew(&mut self) -> Result<(), String> {
        let client = self.client.as_mut().ok_or("not_engaged")?;
        match client.renew() {
            Ok(true) => Ok(()),
            Ok(false) => Err("renew_refused".into()),
            Err(_) => Err("daemon_io".into()),
        }
    }

    fn lost(&mut self) -> Option<String> {
        let client = self.client.as_mut()?;
        let status = match client.status() {
            Ok(status) => status,
            Err(_) => return Some("daemon_io".into()),
        };
        // Events pushed while waiting for the reply are queued by the client.
        if let Some(reason) = client.take_released().into_iter().next() {
            return Some(reason);
        }
        (status.phase.as_deref() != Some("isolated")).then(|| "not_isolated".to_string())
    }

    fn release(&mut self) {
        if let Some(mut client) = self.client.take() {
            let _ = client.restore();
        }
    }
}

/// Renewal policy around an [`InputIsolation`]; the clock is passed in so it can be tested.
pub struct IsolationGate {
    isolation: Box<dyn InputIsolation>,
    engaged: bool,
    last_heartbeat: Instant,
    last_tick: Instant,
}

impl IsolationGate {
    pub fn new(isolation: Box<dyn InputIsolation>, now: Instant) -> Self {
        Self {
            isolation,
            engaged: false,
            last_heartbeat: now,
            last_tick: now,
        }
    }

    pub fn is_engaged(&self) -> bool {
        self.engaged
    }

    pub fn engage(&mut self, now: Instant) -> Result<(), String> {
        let started = Instant::now();
        self.isolation.engage(ISOLATION_LEASE)?;
        // The grab can take many seconds (the daemon waits for keys to be released); the lease
        // and the heartbeat count from when it was granted.
        let granted = now + started.elapsed();
        self.engaged = true;
        self.last_heartbeat = granted;
        self.last_tick = granted;
        Ok(())
    }

    /// Records a browser renew that the agent acknowledged.
    pub fn heartbeat(&mut self, now: Instant) {
        self.last_heartbeat = now;
    }

    pub fn release(&mut self) {
        if self.engaged {
            self.engaged = false;
            self.isolation.release();
        }
    }

    /// Renews while the heartbeat is fresh and reports why the grab ended, if it did. The gate is
    /// released when it reports a loss.
    pub fn tick(&mut self, now: Instant) -> Option<String> {
        if !self.engaged || now.saturating_duration_since(self.last_tick) < TICK {
            return None;
        }
        self.last_tick = now;
        let fresh = now.saturating_duration_since(self.last_heartbeat) <= HEARTBEAT_WINDOW;
        let reason = if fresh {
            self.isolation
                .renew()
                .err()
                .or_else(|| self.isolation.lost())
        } else {
            self.isolation.lost()
        };
        if reason.is_some() {
            self.release();
        }
        reason
    }
}

impl Drop for IsolationGate {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[derive(Default)]
    struct Log {
        calls: Vec<&'static str>,
        fail_engage: Option<String>,
        fail_renew: Option<String>,
        lost: Option<String>,
    }

    struct Fake(Rc<RefCell<Log>>);

    impl InputIsolation for Fake {
        fn engage(&mut self, lease: Duration) -> Result<(), String> {
            assert_eq!(lease, ISOLATION_LEASE);
            let mut log = self.0.borrow_mut();
            log.calls.push("engage");
            log.fail_engage.clone().map_or(Ok(()), Err)
        }
        fn renew(&mut self) -> Result<(), String> {
            let mut log = self.0.borrow_mut();
            log.calls.push("renew");
            log.fail_renew.clone().map_or(Ok(()), Err)
        }
        fn lost(&mut self) -> Option<String> {
            let mut log = self.0.borrow_mut();
            log.calls.push("lost");
            log.lost.clone()
        }
        fn release(&mut self) {
            self.0.borrow_mut().calls.push("release");
        }
    }

    fn fixture() -> (IsolationGate, Rc<RefCell<Log>>, Instant) {
        let log = Rc::new(RefCell::new(Log::default()));
        let start = Instant::now();
        (
            IsolationGate::new(Box::new(Fake(Rc::clone(&log))), start),
            log,
            start,
        )
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    // `engage` counts from when the grab was granted, a few microseconds after `start`.
    const SLACK: Duration = Duration::from_millis(100);

    #[test]
    fn a_refused_engage_leaves_the_gate_disengaged_and_nothing_to_release() {
        let (mut gate, log, start) = fixture();
        log.borrow_mut().fail_engage = Some("keys_held".into());
        assert_eq!(gate.engage(start), Err("keys_held".to_string()));
        assert!(!gate.is_engaged());
        gate.release();
        assert_eq!(log.borrow().calls, ["engage"]);
    }

    #[test]
    fn a_healthy_grab_is_renewed_and_checked_every_tick_but_not_more_often() {
        let (mut gate, log, start) = fixture();
        gate.engage(start).unwrap();
        assert_eq!(gate.tick(start + secs(1)), None, "too early");
        assert_eq!(gate.tick(start + secs(2) + SLACK), None);
        gate.heartbeat(start + secs(10));
        assert_eq!(gate.tick(start + secs(12)), None);
        assert_eq!(
            log.borrow().calls,
            ["engage", "renew", "lost", "renew", "lost"]
        );
    }

    #[test]
    fn a_stale_heartbeat_stops_the_renewals_so_local_input_can_return() {
        let (mut gate, log, start) = fixture();
        gate.engage(start).unwrap();
        // 15 s without a heartbeat is still tolerated; one tick later it is not.
        assert_eq!(gate.tick(start + HEARTBEAT_WINDOW), None);
        let renews = |log: &Rc<RefCell<Log>>| {
            log.borrow()
                .calls
                .iter()
                .filter(|call| **call == "renew")
                .count()
        };
        assert_eq!(renews(&log), 1);
        assert_eq!(gate.tick(start + HEARTBEAT_WINDOW + secs(2)), None);
        assert_eq!(gate.tick(start + HEARTBEAT_WINDOW + secs(4)), None);
        assert_eq!(renews(&log), 1, "no renewal once the heartbeat is stale");
        // The daemon lapsed on its own; the next check reports it and the gate lets go.
        log.borrow_mut().lost = Some("lease_expired".into());
        assert_eq!(
            gate.tick(start + HEARTBEAT_WINDOW + secs(6)),
            Some("lease_expired".to_string())
        );
        assert!(!gate.is_engaged());
    }

    #[test]
    fn a_refused_renewal_or_a_lost_daemon_is_reported_and_releases() {
        let (mut gate, log, start) = fixture();
        gate.engage(start).unwrap();
        log.borrow_mut().fail_renew = Some("daemon_io".into());
        assert_eq!(
            gate.tick(start + secs(2) + SLACK),
            Some("daemon_io".to_string())
        );
        assert!(!gate.is_engaged());
        assert_eq!(gate.tick(start + secs(4)), None, "nothing left to check");

        let (mut gate, log, start) = fixture();
        gate.engage(start).unwrap();
        log.borrow_mut().lost = Some("chord".into());
        assert_eq!(
            gate.tick(start + secs(2) + SLACK),
            Some("chord".to_string())
        );
        assert_eq!(log.borrow().calls.last(), Some(&"release"));
    }

    #[test]
    fn release_is_idempotent_and_dropping_an_engaged_gate_releases() {
        let (mut gate, log, start) = fixture();
        gate.engage(start).unwrap();
        gate.release();
        gate.release();
        assert_eq!(log.borrow().calls, ["engage", "release"]);
        gate.engage(start).unwrap();
        drop(gate);
        assert_eq!(
            log.borrow().calls,
            ["engage", "release", "engage", "release"]
        );
    }

    #[test]
    fn the_daemon_socket_must_be_an_absolute_path() {
        assert!(DaemonIsolation::new(PathBuf::from("relative.sock")).is_err());
        assert!(DaemonIsolation::new(PathBuf::from("/run/blackroom/emergency.sock")).is_ok());
    }
}
