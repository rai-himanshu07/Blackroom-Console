use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::lease::{Capability, ControlLease};
use blackroom_core::protocol::AuthorityUpdate;
use ed25519_dalek::SigningKey;
use remote_hostd::{OfflineHostAuthority, store::PersistentHostAuthority, write_update};

fn finish_child(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("offline agent did not exit after the host socket closed");
        }
        std::thread::yield_now();
    }
}

#[test]
fn offline_agent_process_applies_host_grant_and_revoke_without_gnome() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.path().join("agent.sock");
    let mut host = OfflineHostAuthority::new();
    let verifier = host.verifying_key().to_bytes();
    let verifier_hex = verifier
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gnome-session-agent"))
        .arg("--offline-sim-agent")
        .arg("--socket")
        .arg(&socket)
        .arg("--verifier-hex")
        .arg(&verifier_hex)
        .arg("--epoch")
        .arg("0")
        .arg("--host-uid")
        .arg(rustix::process::getuid().as_raw().to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stream = loop {
        match UnixStream::connect(&socket) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("offline agent did not listen: {error}");
            }
        }
    };
    write_update(&mut stream, &host.grant_update().unwrap()).unwrap();
    write_update(&mut stream, &host.revoke_update()).unwrap();
    drop(stream);
    let output = finish_child(child);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("OFFLINE SIMULATION"));
    assert!(stdout.contains("REMOTE_ACTIVE"));
    assert!(stdout.contains("LOCAL_LOCKED"));
}

#[test]
fn offline_agent_process_rejects_correct_signature_from_wrong_uid() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.path().join("agent.sock");
    let mut host = OfflineHostAuthority::new();
    let verifier_hex = host
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gnome-session-agent"))
        .arg("--offline-sim-agent")
        .arg("--socket")
        .arg(&socket)
        .arg("--verifier-hex")
        .arg(verifier_hex)
        .arg("--epoch")
        .arg("0")
        .arg("--host-uid")
        .arg(
            rustix::process::getuid()
                .as_raw()
                .wrapping_add(1)
                .to_string(),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stream = loop {
        match UnixStream::connect(&socket) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("offline agent did not listen: {error}");
            }
        }
    };
    let sent = write_update(&mut stream, &host.grant_update().unwrap());
    assert!(sent.is_ok() || sent.unwrap_err().kind() == std::io::ErrorKind::BrokenPipe);
    drop(stream);
    let output = finish_child(child);
    assert!(!output.status.success());
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("REMOTE_ACTIVE")
    );
}

#[test]
fn offline_agent_restart_rejects_stale_grant_then_accepts_current_epoch() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut host = OfflineHostAuthority::new();
    let verifier_hex = host
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let old_grant = host.grant_update().unwrap();
    host.revoke();
    let spawn = |name: &str| {
        let socket = directory.path().join(name);
        let child = Command::new(env!("CARGO_BIN_EXE_gnome-session-agent"))
            .arg("--offline-sim-agent")
            .arg("--socket")
            .arg(&socket)
            .arg("--verifier-hex")
            .arg(&verifier_hex)
            .arg("--epoch")
            .arg("1")
            .arg("--host-uid")
            .arg(rustix::process::getuid().as_raw().to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        (child, socket)
    };
    let connect = |socket: &std::path::Path, child: &mut Child| {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match UnixStream::connect(socket) {
                Ok(stream) => return stream,
                Err(_) if Instant::now() < deadline => std::thread::yield_now(),
                Err(error) => {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("offline agent did not listen: {error}");
                }
            }
        }
    };

    let (mut first, stale_socket) = spawn("stale.sock");
    let mut stale_stream = connect(&stale_socket, &mut first);
    let sent = write_update(&mut stale_stream, &old_grant);
    assert!(sent.is_ok() || sent.unwrap_err().kind() == std::io::ErrorKind::BrokenPipe);
    drop(stale_stream);
    let rejected = finish_child(first);
    assert!(!rejected.status.success());
    assert!(
        !String::from_utf8(rejected.stdout)
            .unwrap()
            .contains("REMOTE_ACTIVE")
    );

    let (mut second, current_socket) = spawn("current.sock");
    let mut current_stream = connect(&current_socket, &mut second);
    write_update(&mut current_stream, &host.grant_update().unwrap()).unwrap();
    drop(current_stream);
    let accepted = finish_child(second);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let stdout = String::from_utf8(accepted.stdout).unwrap();
    assert!(stdout.contains("REMOTE_ACTIVE"));
    assert!(stdout.contains("LOCAL_LOCKED"));
}

#[test]
fn offline_agent_never_replaces_an_existing_socket_path() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.path().join("agent.sock");
    std::fs::write(&socket, b"leave this file untouched").unwrap();
    let host = OfflineHostAuthority::new();
    let verifier_hex = host
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let result = Command::new(env!("CARGO_BIN_EXE_gnome-session-agent"))
        .arg("--offline-sim-agent")
        .arg("--socket")
        .arg(&socket)
        .arg("--verifier-hex")
        .arg(verifier_hex)
        .arg("--epoch")
        .arg("0")
        .arg("--host-uid")
        .arg(rustix::process::getuid().as_raw().to_string())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        std::fs::read(&socket).unwrap(),
        b"leave this file untouched"
    );
}

#[test]
fn offline_agent_service_acknowledges_grant_and_revoke() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.path().join("agent.sock");
    let mut host = OfflineHostAuthority::new();
    let verifier_hex = host
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gnome-session-agent"))
        .arg("--offline-sim-agent-service")
        .arg("--socket")
        .arg(&socket)
        .arg("--verifier-hex")
        .arg(verifier_hex)
        .arg("--epoch")
        .arg("0")
        .arg("--host-uid")
        .arg(rustix::process::getuid().as_raw().to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stream = loop {
        match UnixStream::connect(&socket) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("offline agent did not listen: {error}");
            }
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_update(&mut stream, &host.grant_update().unwrap()).unwrap();
    let mut answer = [0_u8; 1];
    stream.read_exact(&mut answer).unwrap();
    assert_eq!(answer, [1]);
    write_update(&mut stream, &host.revoke_update()).unwrap();
    stream.read_exact(&mut answer).unwrap();
    assert_eq!(answer, [0]);
    write_update(&mut stream, &host.grant_update().unwrap()).unwrap();
    stream.read_exact(&mut answer).unwrap();
    assert_eq!(answer, [1]);
    write_update(&mut stream, &host.revoke_update()).unwrap();
    stream.read_exact(&mut answer).unwrap();
    assert_eq!(answer, [0]);
    drop(stream);
    assert!(finish_child(child).status.success());
}

#[test]
fn offline_agent_restores_after_lease_expiry_with_host_socket_open() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.path().join("agent.sock");
    let key = SigningKey::from_bytes(&[27; 32]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_gnome-session-agent"))
        .arg("--offline-sim-agent-service")
        .arg("--socket")
        .arg(&socket)
        .arg("--verifier-hex")
        .arg(hex::encode(key.verifying_key().to_bytes()))
        .arg("--epoch")
        .arg("0")
        .arg("--host-uid")
        .arg(rustix::process::getuid().as_raw().to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (sender, receiver) = mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stream = loop {
        match UnixStream::connect(&socket) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("offline agent did not listen: {error}");
            }
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert!(
        receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .contains("OFFLINE SIMULATION")
    );
    let now = SystemTime::now();
    let lease = ControlLease {
        session_id: "simulated-session-only".into(),
        host_id: "synthetic-host".into(),
        user_id: "synthetic-user".into(),
        client_id: "synthetic-client".into(),
        security_epoch: SecurityEpoch::INITIAL,
        issued_at: now,
        expires_at: now + Duration::from_secs(1),
        capabilities: vec![Capability::Control],
    };
    let signature = lease.sign(&key).to_bytes().to_vec();
    write_update(&mut stream, &AuthorityUpdate::Grant { lease, signature }).unwrap();
    let mut answer = [0_u8; 1];
    stream.read_exact(&mut answer).unwrap();
    assert_eq!(answer, [1]);
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
        "REMOTE_ACTIVE"
    );
    let recovered = receiver.recv_timeout(Duration::from_secs(3));
    if !matches!(recovered.as_deref(), Ok("LOCAL_LOCKED")) {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("idle agent did not recover after lease expiry: {recovered:?}");
    }
    drop(stream);
    assert!(finish_child(child).status.success());
    reader.join().unwrap();
}

#[test]
fn offline_agent_observes_emergency_with_host_socket_still_open() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let dirfd = std::fs::File::open(directory.path()).unwrap();
    let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
    let verifier_hex = hex::encode(host.verifying_key().to_bytes());
    let socket = directory.path().join("agent.sock");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gnome-session-agent"))
        .arg("--offline-sim-agent-service")
        .arg("--socket")
        .arg(&socket)
        .arg("--verifier-hex")
        .arg(verifier_hex)
        .arg("--epoch")
        .arg(host.epoch().value().to_string())
        .arg("--host-uid")
        .arg(rustix::process::getuid().as_raw().to_string())
        .arg("--state-dir")
        .arg(directory.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stream = loop {
        match UnixStream::connect(&socket) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::yield_now(),
            Err(error) => {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("offline agent did not listen: {error}");
            }
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_update(&mut stream, &host.grant_update().unwrap()).unwrap();
    let mut answer = [0_u8; 1];
    stream.read_exact(&mut answer).unwrap();
    assert_eq!(answer, [1]);
    PersistentHostAuthority::emergency_stop(&dirfd).unwrap();
    let output = finish_child(child);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("REMOTE_ACTIVE"));
    assert!(stdout.contains("LOCAL_LOCKED"));
    assert!(host.revoke_update().is_err());
    drop(stream);
}
