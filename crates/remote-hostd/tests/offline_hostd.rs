use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::protocol::AuthorityUpdate;
use ed25519_dalek::Signature;
use remote_hostd::{
    offline_control::{DEMO_CODE, OfflineCommand, OfflineReply, read_frame, write_frame},
    service::OfflineBootstrap,
    store::PersistentHostAuthority,
};

// A forked-but-not-yet-exec'd child from another test thread can briefly keep a
// just-released state-directory flock alive, so process-spawning tests run serially.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn independent_emergency_process_invalidates_an_active_host() {
    let _serial = serial();
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let dirfd = File::open(directory.path()).unwrap();
    let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
    let old_epoch = host.grant_update().unwrap();
    let AuthorityUpdate::Grant { lease, .. } = old_epoch else {
        panic!("expected grant");
    };
    let output = Command::new(env!("CARGO_BIN_EXE_offline-emergency"))
        .args(["--offline-sim-emergency", "--state-dir"])
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(PersistentHostAuthority::emergency_pending(&dirfd).unwrap());
    let epoch = u64::from_be_bytes(
        std::fs::read(directory.path().join("security-epoch"))
            .unwrap()
            .try_into()
            .unwrap(),
    );
    assert!(epoch > lease.security_epoch.value());
    assert!(host.revoke_update().is_err());
    assert_eq!(host.start().unwrap_err().code.as_str(), "RECOVERY_FAILED");
}

#[test]
fn failed_safe_agent_ack_persists_stop_before_host_restart() {
    let _serial = serial();
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let runtime = tempfile::tempdir().unwrap();
    std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let agent_socket = runtime.path().join("agent.sock");
    let control_socket = runtime.path().join("control.sock");
    let listener = UnixListener::bind(&agent_socket).unwrap();
    let peer = std::thread::spawn(move || {
        let (mut agent, _) = listener.accept().unwrap();
        agent
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut header = [0_u8; 4];
        agent.read_exact(&mut header).unwrap();
        let mut bytes = vec![0; u32::from_be_bytes(header) as usize];
        agent.read_exact(&mut bytes).unwrap();
        assert!(matches!(
            serde_json::from_slice::<AuthorityUpdate>(&bytes).unwrap(),
            AuthorityUpdate::Grant { .. }
        ));
        agent.write_all(&[2]).unwrap();
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_remote-hostd"))
        .args(["--offline-sim-service", "--state-dir"])
        .arg(directory.path())
        .arg("--runtime-dir")
        .arg(runtime.path())
        .arg("--agent-socket")
        .arg(&agent_socket)
        .arg("--control-socket")
        .arg(&control_socket)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut bootstrap = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut bootstrap)
        .unwrap();
    let bootstrap: OfflineBootstrap = serde_json::from_str(&bootstrap).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut gateway = loop {
        match UnixStream::connect(&control_socket) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => panic!("offline host did not listen: {error}"),
        }
    };
    gateway
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(
        &mut gateway,
        &OfflineCommand::Start {
            proof: bootstrap.simulation_proof,
            demo_code: DEMO_CODE.into(),
        },
    )
    .unwrap();
    assert!(read_frame::<OfflineReply>(&mut gateway).is_err());
    assert!(!child.wait().unwrap().success());
    peer.join().unwrap();
    let dirfd = File::open(directory.path()).unwrap();
    assert!(PersistentHostAuthority::emergency_pending(&dirfd).unwrap());
    assert!(
        PersistentHostAuthority::open(&dirfd)
            .unwrap()
            .start()
            .is_err()
    );
}

#[test]
fn unverified_abuse_revocation_blocks_host_restart() {
    let _serial = serial();
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let runtime = tempfile::tempdir().unwrap();
    std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let agent_socket = runtime.path().join("agent.sock");
    let control_socket = runtime.path().join("control.sock");
    let listener = UnixListener::bind(&agent_socket).unwrap();
    let peer = std::thread::spawn(move || {
        let (mut agent, _) = listener.accept().unwrap();
        agent
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        for (grant, ack) in [(true, 1_u8), (false, 2)] {
            let mut header = [0_u8; 4];
            agent.read_exact(&mut header).unwrap();
            let mut bytes = vec![0; u32::from_be_bytes(header) as usize];
            agent.read_exact(&mut bytes).unwrap();
            let update: AuthorityUpdate = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(matches!(update, AuthorityUpdate::Grant { .. }), grant);
            agent.write_all(&[ack]).unwrap();
        }
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_remote-hostd"))
        .args(["--offline-sim-service", "--state-dir"])
        .arg(directory.path())
        .arg("--runtime-dir")
        .arg(runtime.path())
        .arg("--agent-socket")
        .arg(&agent_socket)
        .arg("--control-socket")
        .arg(&control_socket)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut bootstrap = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut bootstrap)
        .unwrap();
    let bootstrap: OfflineBootstrap = serde_json::from_str(&bootstrap).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut gateway = loop {
        match UnixStream::connect(&control_socket) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => panic!("offline host did not listen: {error}"),
        }
    };
    write_frame(
        &mut gateway,
        &OfflineCommand::Start {
            proof: bootstrap.simulation_proof.clone(),
            demo_code: DEMO_CODE.into(),
        },
    )
    .unwrap();
    assert!(read_frame::<OfflineReply>(&mut gateway).unwrap().accepted);
    for attempt in 1..=5 {
        write_frame(
            &mut gateway,
            &OfflineCommand::Start {
                proof: bootstrap.simulation_proof.clone(),
                demo_code: DEMO_CODE.into(),
            },
        )
        .unwrap();
        if attempt < 5 {
            let reply: OfflineReply = read_frame(&mut gateway).unwrap();
            assert_eq!(reply.code.as_deref(), Some("AUTH_INVALID"));
        } else {
            assert!(read_frame::<OfflineReply>(&mut gateway).is_err());
        }
    }
    assert!(!child.wait().unwrap().success());
    peer.join().unwrap();
    let dirfd = File::open(directory.path()).unwrap();
    assert!(PersistentHostAuthority::emergency_pending(&dirfd).unwrap());
    assert!(
        PersistentHostAuthority::open(&dirfd)
            .unwrap()
            .start()
            .is_err()
    );
}

#[test]
fn offline_hostd_process_sends_a_signed_grant_and_persisted_revoke() {
    let _serial = serial();
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let dirfd = File::open(directory.path()).unwrap();
    let first = PersistentHostAuthority::open(&dirfd).unwrap();
    let verifier = first.verifying_key();
    drop(first);
    let socket = directory.path().join("agent.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_remote-hostd"))
        .args(["--offline-sim-host", "--state-dir"])
        .arg(directory.path())
        .arg("--agent-socket")
        .arg(&socket)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut connection = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::yield_now();
            }
            Err(error) => {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("offline host did not connect: {error}");
            }
        }
    };
    connection
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut receive = || {
        let mut header = [0_u8; 4];
        connection.read_exact(&mut header).unwrap();
        let length = u32::from_be_bytes(header) as usize;
        assert!((1..=blackroom_core::limits::MAX_MESSAGE_SIZE_BYTES).contains(&length));
        let mut payload = vec![0_u8; length];
        connection.read_exact(&mut payload).unwrap();
        serde_json::from_slice::<AuthorityUpdate>(&payload).unwrap()
    };
    let AuthorityUpdate::Grant { lease, signature } = receive() else {
        panic!("expected a signed grant");
    };
    lease
        .verify(&verifier, &Signature::from_slice(&signature).unwrap())
        .unwrap();
    assert_eq!(lease.security_epoch, SecurityEpoch::INITIAL.next());
    let AuthorityUpdate::Revoke { epoch } = receive() else {
        panic!("expected a revoke");
    };
    assert_eq!(epoch, lease.security_epoch.next());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("OFFLINE SIMULATION")
    );
}

#[test]
fn offline_hostd_refuses_external_socket_before_creating_identity() {
    let _serial = serial();
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_remote-hostd"))
        .args(["--offline-sim-host", "--state-dir"])
        .arg(directory.path())
        .arg("--agent-socket")
        .arg("/tmp/agent.sock")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!directory.path().join("host-identity.key").exists());
    assert!(!directory.path().join("security-epoch").exists());
}

#[test]
fn offline_hostd_service_routes_commands_through_signed_agent_updates() {
    let _serial = serial();
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let runtime = tempfile::tempdir().unwrap();
    std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let dirfd = File::open(directory.path()).unwrap();
    let first = PersistentHostAuthority::open(&dirfd).unwrap();
    let verifier = first.verifying_key();
    drop(first);
    let agent_socket = runtime.path().join("agent.sock");
    let control_socket = runtime.path().join("control.sock");
    let listener = UnixListener::bind(&agent_socket).unwrap();
    let peer = std::thread::spawn(move || {
        let (mut agent, _) = listener.accept().unwrap();
        agent
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        agent
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        for (expected, expected_epoch) in [(1_u8, 1), (1, 1), (1, 1), (0, 2), (1, 2), (0, 3)] {
            let mut header = [0_u8; 4];
            agent.read_exact(&mut header).unwrap();
            let length = u32::from_be_bytes(header) as usize;
            assert!((1..=blackroom_core::limits::MAX_MESSAGE_SIZE_BYTES).contains(&length));
            let mut payload = vec![0_u8; length];
            agent.read_exact(&mut payload).unwrap();
            match serde_json::from_slice::<AuthorityUpdate>(&payload).unwrap() {
                AuthorityUpdate::Grant { lease, signature } => {
                    assert_eq!(expected, 1);
                    assert_eq!(lease.security_epoch.value(), expected_epoch);
                    lease
                        .verify(&verifier, &Signature::from_slice(&signature).unwrap())
                        .unwrap();
                }
                AuthorityUpdate::Revoke { epoch } => {
                    assert_eq!(expected, 0);
                    assert_eq!(epoch.value(), expected_epoch);
                }
            }
            agent.write_all(&[expected]).unwrap();
        }
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_remote-hostd"))
        .args(["--offline-sim-service", "--state-dir"])
        .arg(directory.path())
        .arg("--runtime-dir")
        .arg(runtime.path())
        .arg("--agent-socket")
        .arg(&agent_socket)
        .arg("--control-socket")
        .arg(&control_socket)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut bootstrap = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut bootstrap)
        .unwrap();
    let bootstrap: OfflineBootstrap = serde_json::from_str(&bootstrap).unwrap();
    assert_eq!(bootstrap.epoch, 1);
    assert_eq!(bootstrap.verifier_hex, hex::encode(verifier.to_bytes()));
    assert_eq!(bootstrap.simulation_proof.len(), 64);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut gateway = loop {
        match UnixStream::connect(&control_socket) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("offline host service did not listen: {error}");
            }
        }
    };
    gateway
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(
        &mut gateway,
        &OfflineCommand::Start {
            proof: "wrong".into(),
            demo_code: DEMO_CODE.into(),
        },
    )
    .unwrap();
    let refused: OfflineReply = read_frame(&mut gateway).unwrap();
    assert!(!refused.accepted);
    assert_eq!(refused.code.as_deref(), Some("AUTH_INVALID"));
    assert_eq!(refused.state, "LOCAL_LOCKED");
    assert!(refused.next_proof.is_none());
    write_frame(
        &mut gateway,
        &OfflineCommand::Start {
            proof: bootstrap.simulation_proof.clone(),
            demo_code: "wrong".into(),
        },
    )
    .unwrap();
    let refused: OfflineReply = read_frame(&mut gateway).unwrap();
    assert!(!refused.accepted);
    assert_eq!(refused.code.as_deref(), Some("AUTH_INVALID"));
    assert_eq!(refused.state, "LOCAL_LOCKED");
    assert!(refused.next_proof.is_none());
    assert!(refused.input_grant.is_none());
    let mut current_proof = bootstrap.simulation_proof.clone();
    let mut current_grant = String::new();
    let mut first_grant = String::new();
    for (index, (command, state)) in [
        (
            OfflineCommand::Start {
                proof: bootstrap.simulation_proof.clone(),
                demo_code: DEMO_CODE.into(),
            },
            "REMOTE_ACTIVE",
        ),
        (
            OfflineCommand::Input {
                epoch: 1,
                sequence: 1,
                grant_id: String::new(),
            },
            "REMOTE_ACTIVE",
        ),
        (OfflineCommand::Revoke {}, "LOCAL_LOCKED"),
        (
            OfflineCommand::Start {
                proof: bootstrap.simulation_proof.clone(),
                demo_code: DEMO_CODE.into(),
            },
            "REMOTE_ACTIVE",
        ),
        (OfflineCommand::Revoke {}, "LOCAL_LOCKED"),
    ]
    .into_iter()
    .enumerate()
    {
        let command = match command {
            OfflineCommand::Start { .. } => OfflineCommand::Start {
                proof: current_proof.clone(),
                demo_code: DEMO_CODE.into(),
            },
            OfflineCommand::Input {
                epoch, sequence, ..
            } => OfflineCommand::Input {
                epoch,
                sequence,
                grant_id: current_grant.clone(),
            },
            other => other,
        };
        write_frame(&mut gateway, &command).unwrap();
        let response: OfflineReply = read_frame(&mut gateway).unwrap();
        assert!(response.accepted);
        assert_eq!(response.state, state);
        if matches!(command, OfflineCommand::Start { .. }) {
            let replacement = response.next_proof.expect("accepted Start rotates proof");
            assert_ne!(replacement, current_proof);
            assert_eq!(replacement.len(), 64);
            current_proof = replacement;
            let grant = response.input_grant.expect("accepted Start issues a grant");
            assert_eq!(grant.len(), 32);
            assert_ne!(grant, current_grant);
            if first_grant.is_empty() {
                first_grant = grant.clone();
            }
            current_grant = grant;
        } else {
            assert!(response.next_proof.is_none());
            assert!(response.input_grant.is_none());
        }
        if index == 1 {
            for (epoch, sequence) in [(1, 1), (1, 0), (1, 3), (2, 2)] {
                write_frame(
                    &mut gateway,
                    &OfflineCommand::Input {
                        epoch,
                        sequence,
                        grant_id: current_grant.clone(),
                    },
                )
                .unwrap();
                let refused: OfflineReply = read_frame(&mut gateway).unwrap();
                assert!(!refused.accepted);
                assert_eq!(refused.code.as_deref(), Some("LEASE_INVALID"));
            }
            for grant_id in [String::new(), "0".repeat(32), current_grant[1..].to_owned()] {
                write_frame(
                    &mut gateway,
                    &OfflineCommand::Input {
                        epoch: 1,
                        sequence: 2,
                        grant_id,
                    },
                )
                .unwrap();
                let refused: OfflineReply = read_frame(&mut gateway).unwrap();
                assert!(!refused.accepted);
                assert_eq!(refused.code.as_deref(), Some("LEASE_INVALID"));
            }
            write_frame(
                &mut gateway,
                &OfflineCommand::Input {
                    epoch: 1,
                    sequence: 2,
                    grant_id: current_grant.clone(),
                },
            )
            .unwrap();
            assert!(read_frame::<OfflineReply>(&mut gateway).unwrap().accepted);
        }
        if index == 2 {
            write_frame(
                &mut gateway,
                &OfflineCommand::Start {
                    proof: bootstrap.simulation_proof.clone(),
                    demo_code: DEMO_CODE.into(),
                },
            )
            .unwrap();
            let replay: OfflineReply = read_frame(&mut gateway).unwrap();
            assert!(!replay.accepted);
            assert_eq!(replay.code.as_deref(), Some("AUTH_INVALID"));
            assert!(replay.next_proof.is_none());
        }
        if index == 3 {
            write_frame(
                &mut gateway,
                &OfflineCommand::Input {
                    epoch: 2,
                    sequence: 1,
                    grant_id: first_grant.clone(),
                },
            )
            .unwrap();
            let stale: OfflineReply = read_frame(&mut gateway).unwrap();
            assert!(!stale.accepted);
            assert_eq!(stale.code.as_deref(), Some("LEASE_INVALID"));
            for attempt in 1..=5 {
                write_frame(
                    &mut gateway,
                    &OfflineCommand::Start {
                        proof: if attempt % 2 == 0 {
                            current_proof.clone()
                        } else {
                            bootstrap.simulation_proof.clone()
                        },
                        demo_code: if attempt % 2 == 0 {
                            "wrong".into()
                        } else {
                            DEMO_CODE.into()
                        },
                    },
                )
                .unwrap();
                let refused: OfflineReply = read_frame(&mut gateway).unwrap();
                assert!(!refused.accepted);
                assert_eq!(
                    refused.code.as_deref(),
                    Some(if attempt == 5 {
                        "AUTH_RATE_LIMITED"
                    } else {
                        "AUTH_INVALID"
                    })
                );
                assert_eq!(
                    refused.state,
                    if attempt == 5 {
                        "LOCAL_LOCKED"
                    } else {
                        "REMOTE_ACTIVE"
                    }
                );
            }
            write_frame(
                &mut gateway,
                &OfflineCommand::Start {
                    proof: current_proof.clone(),
                    demo_code: DEMO_CODE.into(),
                },
            )
            .unwrap();
            let locked: OfflineReply = read_frame(&mut gateway).unwrap();
            assert!(!locked.accepted);
            assert_eq!(locked.code.as_deref(), Some("AUTH_RATE_LIMITED"));
            write_frame(
                &mut gateway,
                &OfflineCommand::Input {
                    epoch: 2,
                    sequence: 1,
                    grant_id: current_grant.clone(),
                },
            )
            .unwrap();
            let blocked: OfflineReply = read_frame(&mut gateway).unwrap();
            assert!(!blocked.accepted);
            assert_eq!(blocked.state, "LOCAL_LOCKED");
        }
    }
    drop(gateway);
    assert!(child.wait().unwrap().success());
    peer.join().unwrap();
    let audit = std::fs::read_to_string(directory.path().join("audit.log")).unwrap();
    let events: Vec<serde_json::Value> = audit
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let names: Vec<&str> = events
        .iter()
        .map(|event| event["event"].as_str().unwrap())
        .collect();
    assert_eq!(names.first(), Some(&"host_started"));
    assert!(names.contains(&"grant_issued") && names.contains(&"auth_refused"));
    let causes: Vec<&str> = events
        .iter()
        .filter(|event| event["event"] == "grant_revoked")
        .map(|event| event["cause"].as_str().unwrap())
        .collect();
    assert!(causes.contains(&"revoked") && causes.contains(&"abuse_limit"));
    for secret in [
        bootstrap.simulation_proof.as_str(),
        current_proof.as_str(),
        current_grant.as_str(),
        first_grant.as_str(),
        DEMO_CODE,
    ] {
        assert!(!audit.contains(secret), "audit log leaked a secret");
    }
    let restarted = PersistentHostAuthority::open(&dirfd).unwrap();
    assert_eq!(restarted.epoch().value(), 4);
    assert_eq!(restarted.state(), blackroom_core::state::State::LocalLocked);
}

#[test]
fn offline_hostd_renews_only_for_the_holder_of_the_active_grant() {
    let _serial = serial();
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let runtime = tempfile::tempdir().unwrap();
    std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let agent_socket = runtime.path().join("agent.sock");
    let control_socket = runtime.path().join("control.sock");
    let listener = UnixListener::bind(&agent_socket).unwrap();
    let peer = std::thread::spawn(move || {
        let (mut agent, _) = listener.accept().unwrap();
        agent
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut updates = Vec::new();
        for _ in 0..4 {
            let mut header = [0_u8; 4];
            agent.read_exact(&mut header).unwrap();
            let mut bytes = vec![0; u32::from_be_bytes(header) as usize];
            agent.read_exact(&mut bytes).unwrap();
            let update: AuthorityUpdate = serde_json::from_slice(&bytes).unwrap();
            agent
                .write_all(&[u8::from(matches!(update, AuthorityUpdate::Grant { .. }))])
                .unwrap();
            updates.push(update);
        }
        updates
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_remote-hostd"))
        .args(["--offline-sim-service", "--state-dir"])
        .arg(directory.path())
        .arg("--runtime-dir")
        .arg(runtime.path())
        .arg("--agent-socket")
        .arg(&agent_socket)
        .arg("--control-socket")
        .arg(&control_socket)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut bootstrap = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut bootstrap)
        .unwrap();
    let bootstrap: OfflineBootstrap = serde_json::from_str(&bootstrap).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut gateway = loop {
        match UnixStream::connect(&control_socket) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => panic!("offline host did not listen: {error}"),
        }
    };
    gateway
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut send = |command: OfflineCommand| {
        write_frame(&mut gateway, &command).unwrap();
        read_frame::<OfflineReply>(&mut gateway).unwrap()
    };

    let renew = |epoch: u64, grant_id: &str| OfflineCommand::Renew {
        epoch,
        grant_id: grant_id.into(),
    };
    let none = send(renew(0, &"0".repeat(32)));
    assert!(!none.accepted);
    assert_eq!(none.code.as_deref(), Some("AUTH_INVALID"));

    let started = send(OfflineCommand::Start {
        proof: bootstrap.simulation_proof,
        demo_code: DEMO_CODE.into(),
    });
    assert!(started.accepted);
    let grant = started.input_grant.unwrap();
    for (epoch, grant_id) in [
        (0, "0".repeat(32)),
        (0, String::new()),
        (0, grant[1..].to_owned()),
        (9, grant.clone()),
    ] {
        let refused = send(renew(epoch, &grant_id));
        assert!(!refused.accepted, "{epoch} {grant_id}");
        assert_eq!(refused.code.as_deref(), Some("LEASE_INVALID"));
    }
    std::thread::sleep(Duration::from_millis(1100));
    let renewed = send(renew(0, &grant));
    assert!(renewed.accepted);
    assert_eq!(renewed.state, "REMOTE_ACTIVE");
    assert!(renewed.next_proof.is_none() && renewed.input_grant.is_none());
    let input = send(OfflineCommand::Input {
        epoch: 0,
        sequence: 1,
        grant_id: grant.clone(),
    });
    assert!(input.accepted);
    assert!(send(OfflineCommand::Revoke {}).accepted);
    let after = send(renew(0, &grant));
    assert!(!after.accepted);
    assert_eq!(after.code.as_deref(), Some("AUTH_INVALID"));
    drop(gateway);
    assert!(child.wait().unwrap().success());

    let updates = peer.join().unwrap();
    let [
        AuthorityUpdate::Grant { lease: first, .. },
        AuthorityUpdate::Grant {
            lease: second,
            signature,
        },
        AuthorityUpdate::Grant { lease: third, .. },
        AuthorityUpdate::Revoke { .. },
    ] = &updates[..]
    else {
        panic!("unexpected update sequence: {updates:?}");
    };
    let dirfd = File::open(directory.path()).unwrap();
    assert_eq!(second.security_epoch, first.security_epoch);
    assert!(second.expires_at > first.expires_at);
    assert_eq!(third, second);
    assert_eq!(signature.len(), 64);
    let audit = std::fs::read_to_string(directory.path().join("audit.log")).unwrap();
    let events: Vec<serde_json::Value> = audit
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let names: Vec<&str> = events
        .iter()
        .map(|event| event["event"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "host_started",
            "grant_issued",
            "renew_refused",
            "renew_refused",
            "renew_refused",
            "renew_refused",
            "grant_revoked"
        ]
    );
    assert_eq!(events[1]["client_id"], "synthetic-client");
    assert_eq!(events[6]["cause"], "revoked");
    assert!(!audit.contains(&grant) && !audit.contains(DEMO_CODE));
    let restarted = PersistentHostAuthority::open(&dirfd).unwrap();
    assert_eq!(restarted.state(), blackroom_core::state::State::LocalLocked);
}

#[test]
fn malformed_control_frames_stop_hostd_without_any_grant_or_state_change() {
    let _serial = serial();
    let oversized = (blackroom_core::limits::MAX_MESSAGE_SIZE_BYTES as u32 + 1)
        .to_be_bytes()
        .to_vec();
    let frame = |json: &str| {
        let mut bytes = (json.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(json.as_bytes());
        bytes
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("zero length", vec![0, 0, 0, 0]),
        ("oversized length", oversized),
        (
            "truncated body",
            [vec![0, 0, 0, 40], b"{\"command\":".to_vec()].concat(),
        ),
        ("not json", frame("not json at all")),
        ("unknown command", frame(r#"{"command":"grant"}"#)),
        (
            "unknown field",
            frame(r#"{"command":"status","authenticated":true}"#),
        ),
        (
            "wrong types",
            frame(r#"{"command":"input","epoch":-1,"sequence":"x","grant_id":7}"#),
        ),
        (
            "nested overflow",
            frame(&format!("{}1{}", "[".repeat(200), "]".repeat(200))),
        ),
        (
            "invalid utf8",
            [vec![0, 0, 0, 3], vec![0xff, 0xfe, 0xfd]].concat(),
        ),
    ];
    for (name, bytes) in cases {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = tempfile::tempdir().unwrap();
        std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let agent_socket = runtime.path().join("agent.sock");
        let control_socket = runtime.path().join("control.sock");
        let listener = UnixListener::bind(&agent_socket).unwrap();
        let peer = std::thread::spawn(move || {
            let (mut agent, _) = listener.accept().unwrap();
            agent
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut byte = [0_u8; 1];
            agent.read(&mut byte).unwrap_or(0)
        });
        let mut child = Command::new(env!("CARGO_BIN_EXE_remote-hostd"))
            .args(["--offline-sim-service", "--state-dir"])
            .arg(directory.path())
            .arg("--runtime-dir")
            .arg(runtime.path())
            .arg("--agent-socket")
            .arg(&agent_socket)
            .arg("--control-socket")
            .arg(&control_socket)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut bootstrap = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut bootstrap)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut gateway = loop {
            match UnixStream::connect(&control_socket) {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => std::thread::yield_now(),
                Err(error) => panic!("{name}: offline host did not listen: {error}"),
            }
        };
        gateway.write_all(&bytes).unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success(), "{name}: malformed frame must stop hostd");
        drop(gateway);
        assert_eq!(
            peer.join().unwrap(),
            0,
            "{name}: agent must receive nothing"
        );

        assert!(
            !directory.path().join("recovery-pending").exists(),
            "{name}"
        );
        assert!(!directory.path().join("emergency-stop").exists(), "{name}");
        let audit = std::fs::read_to_string(directory.path().join("audit.log")).unwrap();
        assert_eq!(
            audit.lines().count(),
            1,
            "{name}: only host_started expected"
        );
        let dirfd = File::open(directory.path()).unwrap();
        let restarted = PersistentHostAuthority::open(&dirfd).unwrap();
        assert_eq!(restarted.state(), blackroom_core::state::State::LocalLocked);
    }
}

struct RunningHostd {
    child: std::process::Child,
    gateway: UnixStream,
    proof: String,
}

fn spawn_hostd(directory: &std::path::Path, runtime: &std::path::Path) -> RunningHostd {
    let control_socket = runtime.join("control.sock");
    let mut child = Command::new(env!("CARGO_BIN_EXE_remote-hostd"))
        .args(["--offline-sim-service", "--state-dir"])
        .arg(directory)
        .arg("--runtime-dir")
        .arg(runtime)
        .arg("--agent-socket")
        .arg(runtime.join("agent.sock"))
        .arg("--control-socket")
        .arg(&control_socket)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
            Err(error) => panic!("offline host did not listen: {error}"),
        }
    };
    gateway
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    RunningHostd {
        child,
        gateway,
        proof: bootstrap.simulation_proof,
    }
}

fn private_directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

#[test]
fn audit_write_failure_refuses_the_grant_and_leaves_recoverable_state() {
    let _serial = serial();
    let directory = private_directory();
    let runtime = private_directory();
    let dirfd = File::open(directory.path()).unwrap();
    drop(PersistentHostAuthority::open(&dirfd).unwrap());
    // One byte short of the rotation limit, and a rotation target that cannot be replaced.
    let mut log = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.path().join("audit.log"))
        .unwrap();
    log.write_all(&vec![b' '; 1024 * 1024 - 10]).unwrap();
    std::fs::create_dir(directory.path().join("audit.log.1")).unwrap();
    std::fs::write(directory.path().join("audit.log.1").join("keep"), b"x").unwrap();

    let listener = UnixListener::bind(runtime.path().join("agent.sock")).unwrap();
    let peer = std::thread::spawn(move || {
        let (mut agent, _) = listener.accept().unwrap();
        agent
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        agent.read(&mut [0_u8; 1]).unwrap_or(0)
    });
    let mut host = spawn_hostd(directory.path(), runtime.path());
    write_frame(
        &mut host.gateway,
        &OfflineCommand::Start {
            proof: host.proof.clone(),
            demo_code: DEMO_CODE.into(),
        },
    )
    .unwrap();
    assert!(read_frame::<OfflineReply>(&mut host.gateway).is_err());
    assert!(!host.child.wait().unwrap().success());
    assert_eq!(
        peer.join().unwrap(),
        0,
        "the agent must never see the grant"
    );

    let status = PersistentHostAuthority::inspect(&dirfd).unwrap();
    assert!(!status.recovery_pending && !status.emergency_pending);
    let restarted = PersistentHostAuthority::open(&dirfd).unwrap();
    assert_eq!(restarted.state(), blackroom_core::state::State::LocalLocked);
}

#[test]
fn gateway_disconnect_revokes_with_agent_confirmation_and_verifies_recovery() {
    let _serial = serial();
    let directory = private_directory();
    let runtime = private_directory();
    let listener = UnixListener::bind(runtime.path().join("agent.sock")).unwrap();
    let peer = std::thread::spawn(move || {
        let (mut agent, _) = listener.accept().unwrap();
        agent
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut kinds = Vec::new();
        for _ in 0..2 {
            let mut header = [0_u8; 4];
            agent.read_exact(&mut header).unwrap();
            let mut bytes = vec![0; u32::from_be_bytes(header) as usize];
            agent.read_exact(&mut bytes).unwrap();
            let grant = matches!(
                serde_json::from_slice::<AuthorityUpdate>(&bytes).unwrap(),
                AuthorityUpdate::Grant { .. }
            );
            agent.write_all(&[u8::from(grant)]).unwrap();
            kinds.push(grant);
        }
        kinds
    });
    let mut host = spawn_hostd(directory.path(), runtime.path());
    write_frame(
        &mut host.gateway,
        &OfflineCommand::Start {
            proof: host.proof.clone(),
            demo_code: DEMO_CODE.into(),
        },
    )
    .unwrap();
    assert!(
        read_frame::<OfflineReply>(&mut host.gateway)
            .unwrap()
            .accepted
    );
    drop(host.gateway);
    assert!(host.child.wait().unwrap().success());
    assert_eq!(peer.join().unwrap(), [true, false]);

    let dirfd = File::open(directory.path()).unwrap();
    let status = PersistentHostAuthority::inspect(&dirfd).unwrap();
    assert!(!status.recovery_pending);
    let audit = std::fs::read_to_string(directory.path().join("audit.log")).unwrap();
    let last: serde_json::Value = serde_json::from_str(audit.lines().last().unwrap()).unwrap();
    assert_eq!(last["event"], "grant_revoked");
    assert_eq!(last["cause"], "peer_closed");
    let restarted = PersistentHostAuthority::open(&dirfd).unwrap();
    assert_eq!(restarted.state(), blackroom_core::state::State::LocalLocked);
}
