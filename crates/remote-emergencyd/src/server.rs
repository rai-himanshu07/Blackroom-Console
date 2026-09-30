//! Control socket loop: one authorized client (hostd or the agent), the daemon step, and the
//! actions the daemon takes on its own. Per assessment C25 an emergency chord first restores local
//! input (inside the core), then persists the stop marker, then locks sessions, and only then tells
//! the client, so nothing depends on hostd being alive.

use std::fs::File;
use std::io::{self, ErrorKind, Read};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use remote_hostd::store::PersistentHostAuthority;

use crate::core::{Daemon, Event, Nodes, Phase, Reason};
use crate::proto::{MAX_LINE_BYTES, Reply, Request, parse_request, write_reply};

/// Persists the independent stop marker and epoch bump.
pub trait StopMarker {
    fn persist(&mut self) -> Result<(), String>;
}

/// Locks every session (logind) after an emergency.
pub trait Locker {
    fn lock_sessions(&mut self) -> Result<(), String>;
}

/// The real marker: `remote-hostd`'s own independent emergency stop in the opened state directory.
pub struct DirMarker(pub File);

impl StopMarker for DirMarker {
    fn persist(&mut self) -> Result<(), String> {
        PersistentHostAuthority::emergency_stop(&self.0)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// `loginctl lock-sessions`; a root daemon may lock every session without the session bus.
pub struct LoginctlLocker;

impl Locker for LoginctlLocker {
    fn lock_sessions(&mut self) -> Result<(), String> {
        let status = Command::new("loginctl")
            .arg("lock-sessions")
            .status()
            .map_err(|error| error.to_string())?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("loginctl exited with {status}"))
    }
}

pub struct Policy {
    /// Only this uid may connect (hostd's service user).
    pub allowed_uid: u32,
    /// Without this the daemon answers every `isolate` with an error and never grabs.
    pub grabs_enabled: bool,
    pub marker: Option<Box<dyn StopMarker + Send>>,
    pub locker: Option<Box<dyn Locker + Send>>,
}

pub fn peer_allowed(peer_uid: u32, allowed_uid: u32) -> bool {
    peer_uid == allowed_uid
}

fn peer_uid(stream: &UnixStream) -> Option<u32> {
    rustix::net::sockopt::socket_peercred(stream)
        .ok()
        .map(|cred| cred.uid.as_raw())
}

struct Client {
    stream: UnixStream,
    buffer: Vec<u8>,
}

impl Client {
    fn new(stream: UnixStream) -> io::Result<Self> {
        stream.set_read_timeout(Some(Duration::from_millis(1)))?;
        stream.set_write_timeout(Some(Duration::from_millis(200)))?;
        Ok(Self {
            stream,
            buffer: Vec::new(),
        })
    }

    /// Complete request lines received so far; `Err` means the client is gone or misbehaving.
    fn requests(&mut self) -> Result<Vec<Request>, ()> {
        let mut chunk = [0_u8; 256];
        match self.stream.read(&mut chunk) {
            Ok(0) => return Err(()),
            Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return Err(()),
        }
        let mut requests = Vec::new();
        while let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            requests.push(parse_request(&line).map_err(|_| ())?);
        }
        if self.buffer.len() > MAX_LINE_BYTES {
            return Err(());
        }
        Ok(requests)
    }

    fn send(&mut self, reply: &Reply) -> Result<(), ()> {
        write_reply(&mut self.stream, reply).map_err(|_| ())
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Idle => "idle",
        Phase::Gating => "gating",
        Phase::Isolated => "isolated",
        Phase::ReleaseFailed => "release_failed",
    }
}

fn handle_request<N: Nodes>(
    request: Request,
    daemon: &mut Daemon<N>,
    policy: &Policy,
    now_ms: u64,
) -> Vec<Reply> {
    match request {
        Request::Status {} => vec![Reply::Status {
            phase: phase_name(daemon.phase()),
            held: daemon.held(),
            grabs_enabled: policy.grabs_enabled,
        }],
        Request::Isolate { lease_ms } => {
            if !policy.grabs_enabled {
                return vec![Reply::Error {
                    reason: "grabs_disabled",
                }];
            }
            match daemon.isolate(lease_ms, now_ms) {
                Ok(()) => vec![Reply::Accepted],
                Err(refusal) => vec![Reply::Refused {
                    reason: refusal.as_str(),
                }],
            }
        }
        Request::Renew {} => {
            if daemon.phase() == Phase::Isolated {
                daemon.renew(now_ms);
                vec![Reply::Accepted]
            } else {
                vec![Reply::Error {
                    reason: "not_isolated",
                }]
            }
        }
        Request::Restore {} => {
            let mut replies = vec![Reply::Accepted];
            if let Some(Event::Released(reason)) = daemon.restore() {
                replies.push(Reply::Released {
                    reason: reason.as_str(),
                });
            }
            replies
        }
    }
}

/// Runs until `stop` is set, then restores every grab. `heartbeat` runs once per loop pass, for
/// the systemd watchdog, so it stops only when the loop itself stops making progress.
pub fn serve<N: Nodes>(
    listener: &UnixListener,
    daemon: &mut Daemon<N>,
    policy: &mut Policy,
    stop: &AtomicBool,
    mut heartbeat: impl FnMut(),
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let origin = Instant::now();
    let mut client: Option<Client> = None;
    while !stop.load(Ordering::Relaxed) {
        let now_ms = u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX);
        match listener.accept() {
            Ok((stream, _)) => {
                let allowed =
                    peer_uid(&stream).is_some_and(|uid| peer_allowed(uid, policy.allowed_uid));
                if allowed && client.is_none() {
                    client = Client::new(stream).ok();
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(_) => {}
        }
        let mut gone = false;
        if let Some(active) = client.as_mut() {
            match active.requests() {
                Ok(requests) => {
                    for request in requests {
                        for reply in handle_request(request, daemon, policy, now_ms) {
                            gone |= active.send(&reply).is_err();
                        }
                    }
                }
                Err(()) => gone = true,
            }
        }
        for event in daemon.step(now_ms) {
            let reply = match event {
                Event::Isolated { nodes } => Reply::Isolated { nodes },
                Event::Refused(refusal) => Reply::Refused {
                    reason: refusal.as_str(),
                },
                Event::Released(reason) => {
                    if reason == Reason::Chord {
                        emergency_actions(policy);
                    }
                    Reply::Released {
                        reason: reason.as_str(),
                    }
                }
            };
            if let Some(active) = client.as_mut() {
                gone |= active.send(&reply).is_err();
            }
        }
        if gone {
            client = None;
            daemon.restore();
        }
        heartbeat();
        sleep(Duration::from_millis(4));
    }
    daemon.restore();
    Ok(())
}

/// What the daemon can do alone once local input is already restored.
fn emergency_actions(policy: &mut Policy) {
    if let Some(marker) = policy.marker.as_mut() {
        let _ = marker.persist();
    }
    if let Some(locker) = policy.locker.as_mut() {
        let _ = locker.lock_sessions();
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use remote_input_helper::Chord;

    use super::*;
    use crate::core::testing::{Fake, fake};
    use crate::core::{Config, Observed};

    struct Counter(Arc<AtomicUsize>);

    impl StopMarker for Counter {
        fn persist(&mut self) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    impl Locker for Counter {
        fn lock_sessions(&mut self) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn quick_config() -> Config {
        Config {
            gate_stable_ms: 30,
            gate_timeout_ms: 2_000,
            rescan_ms: 20,
            chord: Chord::new(&[29, 42, 56, 1], 100).expect("chord"),
            ..Config::standard()
        }
    }

    fn own_uid() -> u32 {
        rustix::process::getuid().as_raw()
    }

    fn policy(enabled: bool) -> Policy {
        Policy {
            allowed_uid: own_uid(),
            grabs_enabled: enabled,
            marker: None,
            locker: None,
        }
    }

    fn line(reader: &mut BufReader<UnixStream>) -> String {
        let mut text = String::new();
        reader.read_line(&mut text).expect("reply");
        text.trim_end().to_string()
    }

    fn connect(path: &std::path::Path) -> (UnixStream, BufReader<UnixStream>) {
        let stream = UnixStream::connect(path).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let reader = BufReader::new(stream.try_clone().expect("clone"));
        (stream, reader)
    }

    /// Stops the server thread even if the client closure panics, so a failed test cannot hang.
    struct StopOnDrop<'a>(&'a AtomicBool);

    impl Drop for StopOnDrop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    fn with_server<R>(
        daemon: &mut Daemon<Fake>,
        policy: &mut Policy,
        client: impl FnOnce(&std::path::Path) -> R,
    ) -> R {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("emergency.sock");
        let listener = UnixListener::bind(&path).expect("bind");
        let stop = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let server = scope.spawn(|| serve(&listener, daemon, policy, &stop, || {}));
            let guard = StopOnDrop(&stop);
            let result = client(&path);
            drop(guard);
            server.join().expect("server thread").expect("serve");
            result
        })
    }

    /// The server serves one client at a time and drops a newcomer until it has noticed the
    /// previous one is gone, so connect until a status request is actually answered.
    fn connect_answered(path: &std::path::Path) -> (UnixStream, BufReader<UnixStream>, String) {
        for _ in 0..300 {
            let (mut stream, mut reader) = connect(path);
            if writeln!(stream, "{{\"op\":\"status\"}}").is_ok() {
                let mut text = String::new();
                if reader.read_line(&mut text).is_ok_and(|read| read > 0) {
                    return (stream, reader, text.trim_end().to_string());
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the server never answered");
    }

    #[test]
    fn the_client_isolates_renews_and_restores_over_the_socket() {
        let mut daemon = Daemon::new(fake(), quick_config());
        with_server(&mut daemon, &mut policy(true), |path| {
            let (mut stream, mut reader) = connect(path);
            writeln!(stream, "{{\"op\":\"status\"}}").unwrap();
            assert_eq!(
                line(&mut reader),
                r#"{"event":"status","phase":"idle","held":0,"grabs_enabled":true}"#
            );
            writeln!(stream, "{{\"op\":\"renew\"}}").unwrap();
            assert_eq!(
                line(&mut reader),
                r#"{"event":"error","reason":"not_isolated"}"#
            );
            writeln!(stream, "{{\"op\":\"isolate\",\"lease_ms\":5000}}").unwrap();
            assert_eq!(line(&mut reader), r#"{"event":"accepted"}"#);
            assert_eq!(line(&mut reader), r#"{"event":"isolated","nodes":2}"#);
            writeln!(stream, "{{\"op\":\"renew\"}}").unwrap();
            assert_eq!(line(&mut reader), r#"{"event":"accepted"}"#);
            writeln!(stream, "{{\"op\":\"status\"}}").unwrap();
            assert!(line(&mut reader).contains("\"phase\":\"isolated\",\"held\":2"));
            writeln!(stream, "{{\"op\":\"restore\"}}").unwrap();
            assert_eq!(line(&mut reader), r#"{"event":"accepted"}"#);
            assert_eq!(
                line(&mut reader),
                r#"{"event":"released","reason":"restore"}"#
            );
        });
        assert!(daemon.nodes_mut().grabbed.is_empty());
    }

    #[test]
    fn with_grabs_disabled_nothing_is_ever_grabbed() {
        let mut daemon = Daemon::new(fake(), quick_config());
        with_server(&mut daemon, &mut policy(false), |path| {
            let (mut stream, mut reader) = connect(path);
            writeln!(stream, "{{\"op\":\"isolate\",\"lease_ms\":5000}}").unwrap();
            assert_eq!(
                line(&mut reader),
                r#"{"event":"error","reason":"grabs_disabled"}"#
            );
        });
        assert_eq!(daemon.phase(), Phase::Idle);
        assert!(daemon.nodes_mut().grabbed.is_empty());
    }

    #[test]
    fn a_vanished_client_or_a_hostile_line_releases_everything() {
        let mut daemon = Daemon::new(fake(), quick_config());
        with_server(&mut daemon, &mut policy(true), |path| {
            {
                let (mut stream, mut reader) = connect(path);
                writeln!(stream, "{{\"op\":\"isolate\",\"lease_ms\":5000}}").unwrap();
                assert_eq!(line(&mut reader), r#"{"event":"accepted"}"#);
                assert_eq!(line(&mut reader), r#"{"event":"isolated","nodes":2}"#);
            }
            let (mut stream, mut reader, first) = connect_answered(path);
            assert!(
                first.contains("\"phase\":\"idle\""),
                "dropped client released: {first}"
            );
            writeln!(stream, "{{\"op\":\"isolate\",\"lease_ms\":5000}}").unwrap();
            assert_eq!(line(&mut reader), r#"{"event":"accepted"}"#);
            assert_eq!(line(&mut reader), r#"{"event":"isolated","nodes":2}"#);
            stream.write_all(&vec![b'x'; MAX_LINE_BYTES + 10]).unwrap();
            let mut rest = String::new();
            let _ = reader.read_line(&mut rest);
            let (_stream, _reader, after) = connect_answered(path);
            assert!(
                after.contains("\"phase\":\"idle\""),
                "hostile line released: {after}"
            );
        });
        assert_eq!(daemon.phase(), Phase::Idle);
        assert!(daemon.nodes_mut().grabbed.is_empty());
    }

    #[test]
    fn the_chord_restores_persists_the_marker_locks_and_then_tells_the_client() {
        let marker = Arc::new(AtomicUsize::new(0));
        let lock = Arc::new(AtomicUsize::new(0));
        let mut policy = Policy {
            marker: Some(Box::new(Counter(Arc::clone(&marker)))),
            locker: Some(Box::new(Counter(Arc::clone(&lock)))),
            ..policy(true)
        };
        // The chord keys are already down when the server starts, so the core has held them for
        // longer than the chord's hold time by the moment the grab lands.
        let mut nodes = fake();
        nodes
            .queue
            .extend([29, 42, 56, 1].map(|code| Observed::Key {
                node: 2,
                code,
                pressed: true,
            }));
        let mut daemon = Daemon::new(nodes, quick_config());
        with_server(&mut daemon, &mut policy, |path| {
            let (mut stream, mut reader) = connect(path);
            std::thread::sleep(Duration::from_millis(150));
            writeln!(stream, "{{\"op\":\"isolate\",\"lease_ms\":10000}}").unwrap();
            assert_eq!(line(&mut reader), r#"{"event":"accepted"}"#);
            assert_eq!(line(&mut reader), r#"{"event":"isolated","nodes":2}"#);
            assert_eq!(
                line(&mut reader),
                r#"{"event":"released","reason":"chord"}"#
            );
        });
        assert_eq!(marker.load(Ordering::SeqCst), 1);
        assert_eq!(lock.load(Ordering::SeqCst), 1);
        assert!(daemon.nodes_mut().grabbed.is_empty());
    }

    #[test]
    fn the_client_library_drives_the_daemon_and_collects_pushed_releases() {
        use crate::client::{Client, Outcome};

        let mut daemon = Daemon::new(fake(), quick_config());
        with_server(&mut daemon, &mut policy(true), |path| {
            let mut client = Client::connect(path, Duration::from_secs(5)).expect("connect");
            assert_eq!(client.status().unwrap().phase.as_deref(), Some("idle"));
            assert_eq!(
                client.isolate(10).unwrap(),
                Outcome::Refused("bad_lease".to_string())
            );
            assert_eq!(
                client.isolate(1_000).unwrap(),
                Outcome::Isolated { nodes: 2 }
            );
            assert!(client.renew().unwrap());
            // No more renewals: the daemon lets go by itself and pushes the reason.
            assert_eq!(client.wait_released().unwrap(), "lease_expired");
            assert!(
                !client.renew().unwrap(),
                "nothing to renew after the release"
            );
            assert_eq!(client.status().unwrap().held, Some(0));
        });
        assert!(daemon.nodes_mut().grabbed.is_empty());
    }

    #[test]
    fn only_the_configured_uid_may_connect() {
        assert!(peer_allowed(1000, 1000));
        assert!(!peer_allowed(0, 1000));
        assert!(!peer_allowed(1001, 1000));
    }
}
