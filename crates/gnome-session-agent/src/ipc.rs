//! `agent.sock`: Unix domain socket server with `SO_PEERCRED`-based peer
//! verification (architecture.md §3, reused, not redesigned). Never parses
//! a password, TOTP value, Access Key, or recovery code (Doc 05 §18).
//! The explicit offline-agent mode consumes authority updates; the normal
//! `agent.sock` startup path still closes authorized peers without parsing.
//!
//! `std::os::unix::net::UnixStream::peer_cred()` is still gated behind the
//! unstable `peer_credentials_unix_socket` feature at the pinned toolchain
//! (verified directly against `rustc 1.96.0`, not assumed from
//! documentation alone) — `rustix::net::sockopt::socket_peercred` is used
//! instead, keeping this crate's `#![forbid(unsafe_code)]`.

use std::io::{self, Read};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::{Duration, Instant};

use blackroom_core::limits::MAX_MESSAGE_SIZE_BYTES;
use blackroom_core::protocol::AuthorityUpdate;

use crate::authority::InputAuthority;

const UPDATE_READ_TIMEOUT: Duration = Duration::from_millis(300);

/// Pure authorization decision: only the exact expected uid may connect.
/// No `remote-hostd` exists yet to extend this to the group-based ACL
/// architecture.md §6.2 describes (`agent.sock` owned `remote-hostd:
/// blackroom-session`).
fn peer_authorized(peer_uid: u32, expected_uid: u32) -> bool {
    peer_uid == expected_uid
}

/// Binds `path`, removing a stale socket file left by a previous run first.
pub fn bind(path: &Path) -> io::Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    UnixListener::bind(path)
}

/// Verifies `stream`'s peer credentials via `SO_PEERCRED`. Returns the
/// verified peer uid on success; never returns a stream whose peer failed
/// verification.
pub fn verify_peer(stream: &UnixStream, expected_uid: u32) -> io::Result<u32> {
    let credentials = rustix::net::sockopt::socket_peercred(stream).map_err(io::Error::from)?;
    let peer_uid = credentials.uid.as_raw();
    if peer_authorized(peer_uid, expected_uid) {
        Ok(peer_uid)
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("agent.sock: rejected peer uid {peer_uid} (expected {expected_uid})"),
        ))
    }
}

fn read_until(stream: &mut UnixStream, bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    let mut read = 0;
    while read < bytes.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "host update timed out",
            ));
        }
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(&mut bytes[read..]) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(count) => read += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "host update timed out",
                ));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Offline-only host update receiver; `expected_uid` must come from a trusted
/// host configuration, not from the update. No live input listener is installed.
pub fn receive_host_update(
    stream: &mut UnixStream,
    expected_uid: u32,
    authority: &mut InputAuthority,
) -> io::Result<()> {
    let result = (|| {
        verify_peer(stream, expected_uid)?;
        let deadline = Instant::now() + UPDATE_READ_TIMEOUT;
        let mut header = [0_u8; 4];
        read_until(stream, &mut header, deadline)?;
        let length = u32::from_be_bytes(header) as usize;
        if length == 0 || length > MAX_MESSAGE_SIZE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid host authority update length",
            ));
        }
        let mut payload = vec![0; length];
        read_until(stream, &mut payload, deadline)?;
        let update: AuthorityUpdate = serde_json::from_slice(&payload).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid host authority update")
        })?;
        authority
            .apply_host_update(update)
            .map_err(|error| io::Error::new(io::ErrorKind::PermissionDenied, error.to_string()))
    })();
    if result.is_err() {
        authority.fail_closed();
    }
    result
}

/// Apply queued host updates before an input event, including EOF. A bounded
/// batch prevents a busy peer from indefinitely delaying the input gate.
pub fn drain_host_updates(
    stream: &mut UnixStream,
    expected_uid: u32,
    authority: &mut InputAuthority,
) -> io::Result<()> {
    let result = (|| {
        for _ in 0..16 {
            let mut fds = [rustix::event::PollFd::new(
                &*stream,
                rustix::event::PollFlags::IN,
            )];
            let timeout = rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            let ready = rustix::event::poll(&mut fds, Some(&timeout)).map_err(io::Error::from)?;
            if ready == 0 {
                return Ok(());
            }
            receive_host_update(stream, expected_uid, authority)?;
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "too many pending host authority updates",
        ))
    })();
    if result.is_err() {
        authority.fail_closed();
    }
    result
}

/// Accepts connections from `listener` until an authorized one arrives;
/// unauthorized peers are logged and dropped, never silently accepted.
pub fn accept_authorized(listener: &UnixListener, expected_uid: u32) -> io::Result<UnixStream> {
    loop {
        let (stream, _addr) = listener.accept()?;
        match verify_peer(&stream, expected_uid) {
            Ok(peer_uid) => {
                tracing::info!(peer_uid, "agent.sock: accepted authorized peer");
                return Ok(stream);
            }
            Err(error) => {
                tracing::warn!(%error, "agent.sock: rejected unauthorized peer");
                drop(stream);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackroom_core::epoch::SecurityEpoch;
    use remote_hostd::{OfflineHostAuthority, SIMULATED_SESSION_ID, write_update};
    use std::thread;

    #[test]
    fn peer_authorized_only_for_exact_expected_uid() {
        assert!(peer_authorized(1000, 1000));
        assert!(!peer_authorized(1000, 0));
        assert!(!peer_authorized(0, 1000));
    }

    #[test]
    fn verify_peer_accepts_a_same_process_connection_and_extracts_the_real_uid() {
        let socket_path = std::env::temp_dir().join(format!(
            "blackroom-agent-sock-test-{}.sock",
            std::process::id()
        ));
        let listener = bind(&socket_path).expect("bind should succeed");
        let my_uid = rustix::process::getuid().as_raw();

        let accept_path = socket_path.clone();
        let acceptor = thread::spawn(move || {
            let (stream, _addr) = listener.accept().expect("accept should succeed");
            verify_peer(&stream, my_uid)
        });
        let _client = UnixStream::connect(&accept_path).expect("connect should succeed");
        let verified_uid = acceptor
            .join()
            .expect("acceptor thread should not panic")
            .expect("same-process peer should be authorized");
        assert_eq!(verified_uid, my_uid);

        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn verify_peer_rejects_a_peer_that_is_not_the_expected_uid() {
        let socket_path = std::env::temp_dir().join(format!(
            "blackroom-agent-sock-reject-test-{}.sock",
            std::process::id()
        ));
        let listener = bind(&socket_path).expect("bind should succeed");
        let my_uid = rustix::process::getuid().as_raw();
        let wrong_uid = my_uid.wrapping_add(1);

        let accept_path = socket_path.clone();
        let acceptor = thread::spawn(move || {
            let (stream, _addr) = listener.accept().expect("accept should succeed");
            verify_peer(&stream, wrong_uid)
        });
        let _client = UnixStream::connect(&accept_path).expect("connect should succeed");
        let result = acceptor.join().expect("acceptor thread should not panic");
        assert!(
            result.is_err(),
            "a peer that is not the expected uid must be rejected"
        );

        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn host_update_socket_gates_input_and_eof_fails_closed() {
        let (mut host_wire, mut agent_wire) = UnixStream::pair().unwrap();
        let expected_uid = rustix::process::getuid().as_raw();
        let mut host = OfflineHostAuthority::new();
        let mut authority = InputAuthority::new(
            host.verifying_key(),
            host.epoch(),
            SIMULATED_SESSION_ID.into(),
        );
        write_update(&mut host_wire, &host.grant_update().unwrap()).unwrap();
        receive_host_update(&mut agent_wire, expected_uid, &mut authority).unwrap();
        assert!(authority.dispatch((), |_, _| Ok(())).is_ok());

        write_update(&mut host_wire, &host.revoke_update()).unwrap();
        receive_host_update(&mut agent_wire, expected_uid, &mut authority).unwrap();
        assert!(authority.dispatch((), |_, _| Ok(())).is_err());
        write_update(&mut host_wire, &host.grant_update().unwrap()).unwrap();
        receive_host_update(&mut agent_wire, expected_uid, &mut authority).unwrap();
        assert!(authority.dispatch((), |_, _| Ok(())).is_ok());

        drop(host_wire);
        assert_eq!(
            receive_host_update(&mut agent_wire, expected_uid, &mut authority)
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert!(authority.dispatch((), |_, _| Ok(())).is_err());
    }

    #[test]
    fn host_update_refuses_wrong_peer_and_oversized_frame() {
        use std::io::Write;

        let (mut host_wire, mut agent_wire) = UnixStream::pair().unwrap();
        let mut host = OfflineHostAuthority::new();
        let mut authority = InputAuthority::new(
            host.verifying_key(),
            SecurityEpoch::INITIAL,
            SIMULATED_SESSION_ID.into(),
        );
        write_update(&mut host_wire, &host.grant_update().unwrap()).unwrap();
        let uid = rustix::process::getuid().as_raw();
        assert_eq!(
            receive_host_update(&mut agent_wire, uid.wrapping_add(1), &mut authority)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(authority.dispatch((), |_, _| Ok(())).is_err());

        let (mut host_wire, mut agent_wire) = UnixStream::pair().unwrap();
        let mut authority = InputAuthority::new(
            host.verifying_key(),
            SecurityEpoch::INITIAL,
            SIMULATED_SESSION_ID.into(),
        );
        host_wire
            .write_all(&((MAX_MESSAGE_SIZE_BYTES + 1) as u32).to_be_bytes())
            .unwrap();
        assert_eq!(
            receive_host_update(&mut agent_wire, uid, &mut authority)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(authority.dispatch((), |_, _| Ok(())).is_err());
    }

    #[test]
    fn malformed_forged_and_truncated_updates_fail_closed() {
        use std::io::Write;

        let uid = rustix::process::getuid().as_raw();
        let mut host = OfflineHostAuthority::new();
        let verifying_key = host.verifying_key();
        let epoch = host.epoch();
        let authority = || InputAuthority::new(verifying_key, epoch, SIMULATED_SESSION_ID.into());

        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        sender.write_all(&3_u32.to_be_bytes()).unwrap();
        sender.write_all(b"{x}").unwrap();
        let mut agent = authority();
        assert_eq!(
            receive_host_update(&mut receiver, uid, &mut agent)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(agent.dispatch((), |_, _| Ok(())).is_err());

        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        let mut forged = host.grant_update().unwrap();
        let AuthorityUpdate::Grant { signature, .. } = &mut forged else {
            unreachable!();
        };
        signature[0] ^= 1;
        write_update(&mut sender, &forged).unwrap();
        let mut agent = authority();
        assert_eq!(
            receive_host_update(&mut receiver, uid, &mut agent)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(agent.dispatch((), |_, _| Ok(())).is_err());

        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        sender.write_all(&100_u32.to_be_bytes()).unwrap();
        sender.write_all(b"{").unwrap();
        drop(sender);
        let mut agent = authority();
        assert_eq!(
            receive_host_update(&mut receiver, uid, &mut agent)
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert!(agent.dispatch((), |_, _| Ok(())).is_err());
    }

    #[test]
    fn silent_host_update_times_out_and_closes_authority() {
        let (_sender, mut receiver) = UnixStream::pair().unwrap();
        let host = OfflineHostAuthority::new();
        let mut authority = InputAuthority::new(
            host.verifying_key(),
            host.epoch(),
            SIMULATED_SESSION_ID.into(),
        );
        let uid = rustix::process::getuid().as_raw();
        assert_eq!(
            receive_host_update(&mut receiver, uid, &mut authority)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(authority.dispatch((), |_, _| Ok(())).is_err());
    }

    #[test]
    fn invalid_update_closes_an_already_active_authority() {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        let mut host = OfflineHostAuthority::new();
        let mut authority = InputAuthority::new(
            host.verifying_key(),
            host.epoch(),
            SIMULATED_SESSION_ID.into(),
        );
        let uid = rustix::process::getuid().as_raw();
        let mut update = host.grant_update().unwrap();
        write_update(&mut sender, &update).unwrap();
        receive_host_update(&mut receiver, uid, &mut authority).unwrap();
        assert!(authority.dispatch((), |_, _| Ok(())).is_ok());

        let AuthorityUpdate::Grant { signature, .. } = &mut update else {
            unreachable!();
        };
        signature.clear();
        write_update(&mut sender, &update).unwrap();
        assert_eq!(
            receive_host_update(&mut receiver, uid, &mut authority)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(authority.dispatch((), |_, _| Ok(())).is_err());
    }
}
