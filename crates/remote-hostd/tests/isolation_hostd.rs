//! Process-level checks that hostd, started with an emergency socket, holds the physical-input grab
//! exactly as long as a grant lives. The daemon and the agent are in-test fakes speaking the real
//! wire formats; no input device and no GNOME session is involved.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use blackroom_core::protocol::AuthorityUpdate;
use remote_emergency_client::proto::{Reply, Request, read_request, write_reply};
use remote_hostd::offline_control::{
    DEMO_CODE, OfflineCommand, OfflineReply, read_frame, write_frame,
};
use remote_hostd::service::OfflineBootstrap;

static SERIAL: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
enum Daemon {
    Healthy,
    Refuses,
    /// Pushes `released lease_expired` and reports itself idle from the second status on.
    LosesTheGrab,
}

type Seen = Arc<Mutex<Vec<String>>>;

fn private_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

fn start_fake_daemon(socket: &std::path::Path, mode: Daemon, seen: Seen) {
    let listener = UnixListener::bind(socket).unwrap();
    std::thread::spawn(move || {
        // One client at a time, like the real daemon; each hostd engage is a new connection.
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                return;
            };
            serve_one(stream, mode, &seen);
        }
    });
}

fn serve_one(stream: UnixStream, mode: Daemon, seen: &Seen) {
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    let mut statuses = 0;
    while let Ok(Some(request)) = read_request(&mut reader) {
        let name = match &request {
            Request::Isolate { lease_ms } => format!("isolate {lease_ms}"),
            Request::Renew {} => "renew".to_string(),
            Request::Restore {} => "restore".to_string(),
            Request::Status {} => "status".to_string(),
        };
        seen.lock().unwrap().push(name);
        let replies: Vec<Reply> = match request {
            Request::Isolate { .. } if matches!(mode, Daemon::Refuses) => {
                vec![Reply::Refused {
                    reason: "keys_held",
                }]
            }
            Request::Isolate { .. } => vec![Reply::Isolated { nodes: 2 }],
            Request::Renew {} | Request::Restore {} => vec![Reply::Accepted],
            Request::Status {} => {
                statuses += 1;
                if matches!(mode, Daemon::LosesTheGrab) && statuses >= 2 {
                    vec![
                        Reply::Released {
                            reason: "lease_expired",
                        },
                        Reply::Status {
                            phase: "idle",
                            held: 0,
                            grabs_enabled: true,
                            reads: 0,
                            active_nodes: 0,
                        },
                    ]
                } else {
                    vec![Reply::Status {
                        phase: "isolated",
                        held: 2,
                        grabs_enabled: true,
                        reads: 0,
                        active_nodes: 0,
                    }]
                }
            }
        };
        for reply in &replies {
            if write_reply(&mut writer, reply).is_err() {
                return;
            }
        }
    }
    seen.lock().unwrap().push("closed".to_string());
}

/// Acknowledges every update: a grant with 1, a revoke with 0, and records what it saw.
fn start_fake_agent(socket: &std::path::Path, seen: Seen) {
    let listener = UnixListener::bind(socket).unwrap();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        loop {
            let mut header = [0_u8; 4];
            if stream.read_exact(&mut header).is_err() {
                return;
            }
            let mut payload = vec![0_u8; u32::from_be_bytes(header) as usize];
            if stream.read_exact(&mut payload).is_err() {
                return;
            }
            let (name, ack) = match serde_json::from_slice::<AuthorityUpdate>(&payload).unwrap() {
                AuthorityUpdate::Grant { .. } => ("grant", 1_u8),
                AuthorityUpdate::Revoke { .. } => ("revoke", 0_u8),
            };
            seen.lock().unwrap().push(name.to_string());
            if stream.write_all(&[ack]).is_err() {
                return;
            }
        }
    });
}

struct Host {
    child: Child,
    gateway: UnixStream,
    proof: String,
    state: tempfile::TempDir,
    _runtime: tempfile::TempDir,
    _daemon_dir: tempfile::TempDir,
    daemon_seen: Seen,
    agent_seen: Seen,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn launch(mode: Daemon) -> Host {
    let state = private_dir();
    let runtime = private_dir();
    let daemon_dir = private_dir();
    let daemon_socket = daemon_dir.path().join("emergency.sock");
    let agent_socket = runtime.path().join("agent.sock");
    let control_socket = runtime.path().join("control.sock");
    let daemon_seen: Seen = Arc::default();
    let agent_seen: Seen = Arc::default();
    start_fake_daemon(&daemon_socket, mode, Arc::clone(&daemon_seen));
    start_fake_agent(&agent_socket, Arc::clone(&agent_seen));
    let mut child = Command::new(env!("CARGO_BIN_EXE_remote-hostd"))
        .args(["--offline-sim-service", "--state-dir"])
        .arg(state.path())
        .arg("--runtime-dir")
        .arg(runtime.path())
        .arg("--agent-socket")
        .arg(&agent_socket)
        .arg("--control-socket")
        .arg(&control_socket)
        .arg("--emergency-socket")
        .arg(&daemon_socket)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut bootstrap = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut bootstrap)
        .unwrap();
    let bootstrap: OfflineBootstrap = serde_json::from_str(&bootstrap).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let gateway = loop {
        match UnixStream::connect(&control_socket) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => panic!("hostd did not listen: {error}"),
        }
    };
    gateway
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    Host {
        child,
        gateway,
        proof: bootstrap.simulation_proof,
        state,
        _runtime: runtime,
        _daemon_dir: daemon_dir,
        daemon_seen,
        agent_seen,
    }
}

impl Host {
    fn send(&mut self, command: &OfflineCommand) -> OfflineReply {
        write_frame(&mut self.gateway, command).unwrap();
        read_frame(&mut self.gateway).unwrap()
    }

    fn start(&mut self) -> OfflineReply {
        let command = OfflineCommand::Start {
            proof: self.proof.clone(),
            demo_code: DEMO_CODE.into(),
        };
        let reply = self.send(&command);
        if let Some(next) = reply.next_proof.clone() {
            self.proof = next;
        }
        reply
    }

    fn daemon_saw(&self) -> Vec<String> {
        self.daemon_seen.lock().unwrap().clone()
    }

    fn agent_saw(&self) -> Vec<String> {
        self.agent_seen.lock().unwrap().clone()
    }

    fn audit(&self) -> String {
        std::fs::read_to_string(self.state.path().join("audit.log")).unwrap_or_default()
    }
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_grant_holds_the_grab_while_renewed_and_revoke_restores_it() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut host = launch(Daemon::Healthy);
    let started = host.start();
    assert!(started.accepted, "{:?}", started.code);
    assert_eq!(started.state, "REMOTE_ACTIVE");
    assert_eq!(host.daemon_saw(), ["isolate 10000"]);

    // hostd renews the daemon lease on its own tick; the Start and this Renew are both fresh
    // heartbeats (the 15 s staleness rule is covered with an injected clock in isolation.rs).
    let renew = OfflineCommand::Renew {
        epoch: started.epoch,
        grant_id: started.input_grant.clone().unwrap(),
    };
    assert!(host.send(&renew).accepted);
    wait_for("the daemon lease renewal", || {
        host.daemon_saw().iter().any(|call| call == "renew")
    });

    let revoked = host.send(&OfflineCommand::Revoke {});
    assert!(revoked.accepted);
    assert_eq!(revoked.state, "LOCAL_LOCKED");
    wait_for("the grab to be released", || {
        host.daemon_saw().iter().any(|call| call == "restore")
    });
    wait_for("the daemon connection to close", || {
        host.daemon_saw().last().map(String::as_str) == Some("closed")
    });
    // The browser renew re-signs the grant, so the agent sees it twice before the revoke.
    assert_eq!(host.agent_saw(), ["grant", "grant", "revoke"]);
}

#[test]
fn a_refused_grab_fails_the_start_closed_and_leaves_no_grant() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut host = launch(Daemon::Refuses);
    let started = host.start();
    assert!(!started.accepted);
    assert_eq!(started.code.as_deref(), Some("INPUT_ISOLATION_FAILED"));
    assert_eq!(started.state, "LOCAL_LOCKED");
    assert!(started.input_grant.is_none());
    assert_eq!(
        host.agent_saw(),
        ["grant", "revoke"],
        "the agent was told to stand down"
    );
    // The proof was not burned: the same client can try again instead of being locked out.
    let again = host.start();
    assert_eq!(again.code.as_deref(), Some("INPUT_ISOLATION_FAILED"));
    assert!(host.audit().contains("\"cause\":\"isolation_failed\""));
    assert!(!host.state.path().join("recovery-pending").exists());
    assert!(!host.state.path().join("emergency-stop").exists());
}

#[test]
fn losing_the_grab_ends_the_grant_without_any_browser_action() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut host = launch(Daemon::LosesTheGrab);
    let started = host.start();
    assert!(started.accepted, "{:?}", started.code);
    // The fake reports the lapse on its second status poll; hostd notices on its own tick.
    wait_for("hostd to revoke the grant", || {
        host.agent_saw() == ["grant", "revoke"]
    });
    wait_for("the audit entry and the recovery marker", || {
        host.audit().contains("\"cause\":\"isolation_lost\"")
            && !host.state.path().join("recovery-pending").exists()
    });
    let status = host.send(&OfflineCommand::Status {});
    assert_eq!(status.state, "LOCAL_LOCKED");
    let renew = OfflineCommand::Renew {
        epoch: started.epoch,
        grant_id: started.input_grant.unwrap(),
    };
    assert!(
        !host.send(&renew).accepted,
        "a lost grab cannot be renewed back"
    );
    // A new Start opens a new daemon connection and engages again.
    let again = host.start();
    assert!(again.accepted, "{:?}", again.code);
    assert_eq!(
        host.daemon_saw()
            .iter()
            .filter(|call| call.starts_with("isolate"))
            .count(),
        2
    );
}

#[test]
fn a_vanishing_gateway_revokes_the_grant_and_releases_the_grab() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut host = launch(Daemon::Healthy);
    assert!(host.start().accepted);
    host.gateway.shutdown(std::net::Shutdown::Both).unwrap();
    wait_for("the agent revoke", || {
        host.agent_saw() == ["grant", "revoke"]
    });
    wait_for("the grab to be released", || {
        let seen = host.daemon_saw();
        seen.iter().any(|call| call == "restore")
            && seen.last().map(String::as_str) == Some("closed")
    });
}

#[test]
fn a_relative_emergency_socket_is_refused_before_any_state_changes() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let state = private_dir();
    let runtime = private_dir();
    let output = Command::new(env!("CARGO_BIN_EXE_remote-hostd"))
        .args(["--offline-sim-service", "--state-dir"])
        .arg(state.path())
        .arg("--runtime-dir")
        .arg(runtime.path())
        .arg("--agent-socket")
        .arg(runtime.path().join("agent.sock"))
        .arg("--control-socket")
        .arg(runtime.path().join("control.sock"))
        .args(["--emergency-socket", "relative.sock"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("absolute"));
    assert!(!state.path().join("audit.log").exists());
}
