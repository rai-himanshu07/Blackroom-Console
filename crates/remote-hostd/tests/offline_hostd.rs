use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::protocol::AuthorityUpdate;
use ed25519_dalek::Signature;
use remote_hostd::{
    offline_control::{OfflineCommand, OfflineReply, read_frame, write_frame},
    service::OfflineBootstrap,
    store::PersistentHostAuthority,
};

#[test]
fn independent_emergency_process_invalidates_an_active_host() {
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
fn offline_hostd_process_sends_a_signed_grant_and_persisted_revoke() {
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
        for (expected, expected_epoch) in [(1_u8, 1), (1, 1), (0, 2), (1, 2), (0, 3)] {
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
        },
    )
    .unwrap();
    let refused: OfflineReply = read_frame(&mut gateway).unwrap();
    assert!(!refused.accepted);
    assert_eq!(refused.code.as_deref(), Some("AUTH_INVALID"));
    assert_eq!(refused.state, "LOCAL_LOCKED");
    assert!(refused.next_proof.is_none());
    let mut current_proof = bootstrap.simulation_proof.clone();
    for (index, (command, state)) in [
        (
            OfflineCommand::Start {
                proof: bootstrap.simulation_proof.clone(),
            },
            "REMOTE_ACTIVE",
        ),
        (OfflineCommand::Input {}, "REMOTE_ACTIVE"),
        (OfflineCommand::Revoke {}, "LOCAL_LOCKED"),
        (
            OfflineCommand::Start {
                proof: bootstrap.simulation_proof.clone(),
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
        } else {
            assert!(response.next_proof.is_none());
        }
        if index == 2 {
            write_frame(
                &mut gateway,
                &OfflineCommand::Start {
                    proof: bootstrap.simulation_proof.clone(),
                },
            )
            .unwrap();
            let replay: OfflineReply = read_frame(&mut gateway).unwrap();
            assert!(!replay.accepted);
            assert_eq!(replay.code.as_deref(), Some("AUTH_INVALID"));
            assert!(replay.next_proof.is_none());
        }
    }
    drop(gateway);
    assert!(child.wait().unwrap().success());
    peer.join().unwrap();
}
