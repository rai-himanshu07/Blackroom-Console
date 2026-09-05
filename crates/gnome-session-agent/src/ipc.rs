//! `agent.sock`: Unix domain socket server with `SO_PEERCRED`-based peer
//! verification (architecture.md §3, reused, not redesigned). Never parses
//! a password, TOTP value, Access Key, or recovery code (Doc 05 §18) — no
//! message handling exists yet, since `remote-hostd` (the only intended
//! peer) does not exist yet either (skeleton, this phase).
//!
//! `std::os::unix::net::UnixStream::peer_cred()` is still gated behind the
//! unstable `peer_credentials_unix_socket` feature at the pinned toolchain
//! (verified directly against `rustc 1.96.0`, not assumed from
//! documentation alone) — `rustix::net::sockopt::socket_peercred` is used
//! instead, keeping this crate's `#![forbid(unsafe_code)]`.

use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

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
}
