use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::protocol::AuthorityUpdate;
use gnome_session_agent::{authority::InputAuthority, ipc};
use remote_hostd::{SIMULATED_SESSION_ID, store::PersistentHostAuthority, write_update};

#[test]
#[ignore = "only the parent process-boundary test launches this helper"]
fn child_hostd_sender() {
    let directory = std::env::var_os("BLACKROOM_TEST_STATE_DIR").unwrap();
    let directory = Path::new(&directory);
    let dirfd = File::open(directory).unwrap();
    let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
    assert_eq!(host.epoch(), SecurityEpoch::INITIAL.next());
    let mut stream = UnixStream::connect(directory.join("agent.sock")).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_update(
        &mut stream,
        &AuthorityUpdate::Revoke {
            epoch: host.epoch(),
        },
    )
    .unwrap();
    let (lease, signature) = host.start().unwrap();
    write_update(
        &mut stream,
        &AuthorityUpdate::Grant {
            lease,
            signature: signature.to_bytes().to_vec(),
        },
    )
    .unwrap();
    let mut acknowledged = [0_u8; 1];
    stream.read_exact(&mut acknowledged).unwrap();
    assert_eq!(acknowledged, [1]);
}

#[test]
#[ignore = "only the parent lock test launches this helper"]
fn child_checks_exclusive_state_lock() {
    let directory = std::env::var_os("BLACKROOM_TEST_STATE_DIR").unwrap();
    let dirfd = File::open(Path::new(&directory)).unwrap();
    assert!(PersistentHostAuthority::open(&dirfd).is_err());
}

#[test]
fn persisted_host_state_refuses_a_second_process() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let dirfd = File::open(directory.path()).unwrap();
    let first = PersistentHostAuthority::open(&dirfd).unwrap();
    drop(dirfd);
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child_checks_exclusive_state_lock",
            "--ignored",
            "--nocapture",
        ])
        .env("BLACKROOM_TEST_STATE_DIR", directory.path())
        .status()
        .unwrap();
    assert!(status.success());
    drop(first);
    let dirfd = File::open(directory.path()).unwrap();
    assert!(PersistentHostAuthority::open(&dirfd).is_ok());
}

#[test]
fn separate_offline_host_process_grants_then_eof_revokes() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let dirfd = File::open(directory.path()).unwrap();
    let first_host = PersistentHostAuthority::open(&dirfd).unwrap();
    let verifier = first_host.verifying_key();
    drop(first_host);

    let listener = UnixListener::bind(directory.path().join("agent.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_hostd_sender", "--ignored", "--nocapture"])
        .env("BLACKROOM_TEST_STATE_DIR", directory.path())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::yield_now();
            }
            Err(error) => {
                let _ = child.kill();
                panic!("offline host process did not connect: {error}");
            }
        }
    };
    let credentials = rustix::net::sockopt::socket_peercred(&stream).unwrap();
    assert_eq!(credentials.uid.as_raw(), rustix::process::getuid().as_raw());
    assert_ne!(
        credentials.pid.as_raw_pid(),
        rustix::process::getpid().as_raw_pid()
    );

    let mut authority = InputAuthority::new(
        verifier,
        SecurityEpoch::INITIAL,
        SIMULATED_SESSION_ID.into(),
    );
    let expected_uid = rustix::process::getuid().as_raw();
    ipc::receive_host_update(&mut stream, expected_uid, &mut authority).unwrap();
    ipc::receive_host_update(&mut stream, expected_uid, &mut authority).unwrap();
    let mut sent = false;
    authority
        .dispatch((), |_, _| {
            sent = true;
            Ok(())
        })
        .unwrap();
    assert!(sent);
    stream.write_all(&[1]).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(ipc::drain_host_updates(&mut stream, expected_uid, &mut authority).is_err());
    assert!(authority.dispatch((), |_, _| Ok(())).is_err());
}
