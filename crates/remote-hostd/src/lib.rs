#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, SystemTime};

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::lease::{Capability, ControlLease};
use blackroom_core::limits::MAX_MESSAGE_SIZE_BYTES;
use blackroom_core::protocol::AuthorityUpdate;
use blackroom_core::state::State;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};

pub mod offline_control;
pub mod service;
pub mod store;

pub const SIMULATED_SESSION_ID: &str = "simulated-session-only";

/// Synthetic host-side grant issuer. This demo has no durable identity or IPC.
pub struct OfflineHostAuthority {
    signing_key: SigningKey,
    epoch: SecurityEpoch,
    state: State,
}

impl Default for OfflineHostAuthority {
    fn default() -> Self {
        Self::new()
    }
}

impl OfflineHostAuthority {
    pub fn new() -> Self {
        Self {
            signing_key: SigningKey::from_bytes(&[42; 32]),
            epoch: SecurityEpoch::INITIAL,
            state: State::LocalLocked,
        }
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn epoch(&self) -> SecurityEpoch {
        self.epoch
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn start(&mut self) -> Result<(ControlLease, Signature), BlackroomError> {
        if self.state != State::LocalLocked {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "simulation is already active",
            ));
        }
        let now = SystemTime::now();
        let lease = ControlLease {
            session_id: SIMULATED_SESSION_ID.into(),
            host_id: "synthetic-host".into(),
            user_id: "synthetic-user".into(),
            client_id: "synthetic-client".into(),
            security_epoch: self.epoch,
            issued_at: now,
            expires_at: now + Duration::from_secs(120),
            capabilities: vec![Capability::Control],
        };
        let signature = lease.sign(&self.signing_key);
        self.state = State::RemoteActive;
        Ok((lease, signature))
    }

    pub fn grant_update(&mut self) -> Result<AuthorityUpdate, BlackroomError> {
        let (lease, signature) = self.start()?;
        Ok(AuthorityUpdate::Grant {
            lease,
            signature: signature.to_bytes().to_vec(),
        })
    }

    pub fn revoke(&mut self) {
        if self.state == State::LocalLocked {
            return;
        }
        self.state = State::LocalLocked;
        self.epoch = self.epoch.next();
    }

    pub fn revoke_update(&mut self) -> AuthorityUpdate {
        self.revoke();
        AuthorityUpdate::Revoke { epoch: self.epoch }
    }
}

pub fn write_update(stream: &mut UnixStream, update: &AuthorityUpdate) -> io::Result<()> {
    let payload = serde_json::to_vec(update).map_err(io::Error::other)?;
    if payload.len() > MAX_MESSAGE_SIZE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "host authority update exceeds IPC limit",
        ));
    }
    let length = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid IPC length"))?;
    stream.write_all(&length.to_be_bytes())?;
    stream.write_all(&payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    use store::PersistentHostAuthority;

    #[test]
    fn host_owns_grant_and_invalidates_it_on_revoke() {
        let mut host = OfflineHostAuthority::new();
        assert_eq!(host.state(), State::LocalLocked);
        host.revoke();
        assert_eq!(host.epoch(), SecurityEpoch::INITIAL);
        let (old_lease, signature) = host.start().unwrap();
        old_lease.verify(&host.verifying_key(), &signature).unwrap();
        assert!(host.start().is_err());
        host.revoke();
        assert_eq!(host.epoch(), SecurityEpoch::INITIAL.next());
        host.revoke();
        assert_eq!(host.epoch(), SecurityEpoch::INITIAL.next());
        assert_eq!(
            old_lease
                .validate(
                    host.epoch(),
                    host.state(),
                    SIMULATED_SESSION_ID,
                    false,
                    SystemTime::now()
                )
                .unwrap_err()
                .code,
            ErrorCode::SessionEpochMismatch
        );
        let (new_lease, new_signature) = host.start().unwrap();
        new_lease
            .verify(&host.verifying_key(), &new_signature)
            .unwrap();
        assert_eq!(new_lease.security_epoch, host.epoch());
    }

    #[test]
    fn hostd_writes_bounded_authority_updates() {
        let (mut host_wire, mut agent_wire) = UnixStream::pair().unwrap();
        let mut host = OfflineHostAuthority::new();
        for update in [host.grant_update().unwrap(), host.revoke_update()] {
            write_update(&mut host_wire, &update).unwrap();
            let mut header = [0; 4];
            agent_wire.read_exact(&mut header).unwrap();
            let length = u32::from_be_bytes(header) as usize;
            assert!(length > 0 && length <= MAX_MESSAGE_SIZE_BYTES);
            let mut body = vec![0; length];
            agent_wire.read_exact(&mut body).unwrap();
            let decoded: AuthorityUpdate = serde_json::from_slice(&body).unwrap();
            assert_eq!(serde_json::to_vec(&decoded).unwrap(), body);
        }
    }

    #[test]
    fn persisted_host_restart_advances_epoch_without_changing_identity() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let directory_fd = std::fs::File::open(directory.path()).unwrap();
        let metadata = directory_fd.metadata().unwrap();
        assert!(metadata.is_dir());
        assert_eq!(metadata.uid(), rustix::process::getuid().as_raw());
        assert_eq!(metadata.mode() & 0o022, 0, "mode {:o}", metadata.mode());
        let mut first = PersistentHostAuthority::open(&directory_fd).unwrap();
        let verifier = first.verifying_key();
        let (old_lease, old_signature) = first.start().unwrap();
        drop(first);

        let mut restarted = PersistentHostAuthority::open(&directory_fd).unwrap();
        assert_eq!(restarted.verifying_key(), verifier);
        old_lease
            .verify(&restarted.verifying_key(), &old_signature)
            .unwrap();
        assert_eq!(restarted.epoch(), old_lease.security_epoch.next());
        assert_eq!(
            old_lease
                .validate(
                    restarted.epoch(),
                    State::RemoteActive,
                    SIMULATED_SESSION_ID,
                    false,
                    SystemTime::now()
                )
                .unwrap_err()
                .code,
            ErrorCode::SessionEpochMismatch
        );
        assert_eq!(
            restarted.start().unwrap_err().code,
            ErrorCode::RecoveryFailed
        );
        drop(restarted);
        PersistentHostAuthority::verify_recovery(&directory_fd, old_lease.security_epoch).unwrap();
        let mut recovered = PersistentHostAuthority::open(&directory_fd).unwrap();
        let (new_lease, signature) = recovered.start().unwrap();
        new_lease.verify(&verifier, &signature).unwrap();
    }
}
