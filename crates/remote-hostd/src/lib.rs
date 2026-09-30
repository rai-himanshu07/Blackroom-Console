#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, SystemTime};

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::lease::{Capability, ControlLease};
use blackroom_core::limits::{CONTROL_LEASE_TTL, MAX_MESSAGE_SIZE_BYTES};
use blackroom_core::protocol::AuthorityUpdate;
use blackroom_core::state::State;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};

use auth::{AuthSession, Principal};

pub mod audit;
pub mod auth;
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

    /// Issues a lease for the fixed synthetic principal, without any session.
    /// It keeps a fixed 120 s simulation lifetime and cannot be renewed.
    pub fn start(&mut self) -> Result<(ControlLease, Signature), BlackroomError> {
        self.issue(
            &Principal::synthetic(),
            None,
            Duration::from_secs(120),
            SystemTime::now(),
        )
    }

    /// Issues a lease for an authenticated hostd session; it never outlives
    /// that session, and the session must belong to the current epoch. It
    /// lasts `CONTROL_LEASE_TTL` unless renewed with [`Self::renew_for`].
    pub fn start_for(
        &mut self,
        session: &AuthSession,
    ) -> Result<(ControlLease, Signature), BlackroomError> {
        let now = SystemTime::now();
        self.check_session(session, now)?;
        self.issue(
            session.principal(),
            Some(session.expires_at()),
            CONTROL_LEASE_TTL,
            now,
        )
    }

    /// Re-signs the active grant for the same epoch and session, without any
    /// state change; the caller must have verified the session binding.
    pub fn renew_for(
        &mut self,
        session: &AuthSession,
    ) -> Result<(ControlLease, Signature), BlackroomError> {
        if self.state != State::RemoteActive {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "no active grant to renew",
            ));
        }
        let now = SystemTime::now();
        self.check_session(session, now)?;
        Ok(self.sign_lease(
            session.principal(),
            Some(session.expires_at()),
            CONTROL_LEASE_TTL,
            now,
        ))
    }

    fn check_session(&self, session: &AuthSession, now: SystemTime) -> Result<(), BlackroomError> {
        if session.epoch() != self.epoch {
            return Err(BlackroomError::new(
                ErrorCode::SessionEpochMismatch,
                "authentication session belongs to another security epoch",
            ));
        }
        if now >= session.expires_at() {
            return Err(BlackroomError::new(
                ErrorCode::AuthInvalid,
                "authentication session expired",
            ));
        }
        Ok(())
    }

    fn issue(
        &mut self,
        principal: &Principal,
        not_after: Option<SystemTime>,
        ttl: Duration,
        now: SystemTime,
    ) -> Result<(ControlLease, Signature), BlackroomError> {
        if self.state != State::LocalLocked {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "simulation is already active",
            ));
        }
        let signed = self.sign_lease(principal, not_after, ttl, now);
        self.state = State::RemoteActive;
        Ok(signed)
    }

    fn sign_lease(
        &self,
        principal: &Principal,
        not_after: Option<SystemTime>,
        ttl: Duration,
        now: SystemTime,
    ) -> (ControlLease, Signature) {
        let mut expires_at = now + ttl;
        if let Some(limit) = not_after {
            expires_at = expires_at.min(limit);
        }
        let lease = ControlLease {
            session_id: SIMULATED_SESSION_ID.into(),
            host_id: "synthetic-host".into(),
            user_id: principal.user_id.clone(),
            client_id: principal.client_id.clone(),
            security_epoch: self.epoch,
            issued_at: now,
            expires_at,
            capabilities: vec![Capability::Control],
        };
        let signature = lease.sign(&self.signing_key);
        (lease, signature)
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
    use auth::HostSessions;
    use blackroom_core::limits::AUTH_SESSION_TTL;
    use offline_control::{DEMO_CODE, DemoCredential, DemoCredentialVerifier};
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    use store::PersistentHostAuthority;

    fn demo_credential() -> DemoCredential {
        DemoCredential {
            proof: "proof".into(),
            demo_code: DEMO_CODE.into(),
        }
    }

    struct Named;

    impl auth::CredentialVerifier for Named {
        type Presented = ();

        fn verify(&mut self, _: ()) -> Result<Principal, BlackroomError> {
            Ok(Principal {
                user_id: "user-x".into(),
                client_id: "client-x".into(),
            })
        }
    }

    #[test]
    fn session_bound_lease_takes_principal_and_never_outlives_session() {
        let mut host = OfflineHostAuthority::new();
        let mut sessions = HostSessions::default();
        let issued = SystemTime::now() - Duration::from_secs(285);
        let token = sessions
            .authenticate(&mut Named, (), host.epoch(), issued)
            .unwrap();
        let session = sessions
            .resolve(&token, host.epoch(), SystemTime::now())
            .unwrap();
        let (lease, signature) = host.start_for(session).unwrap();
        lease.verify(&host.verifying_key(), &signature).unwrap();
        assert_eq!(lease.user_id, "user-x");
        assert_eq!(lease.client_id, "client-x");
        assert_eq!(lease.expires_at, session.expires_at());
        assert!(lease.expires_at < lease.issued_at + CONTROL_LEASE_TTL);
    }

    #[test]
    fn renewal_resigns_only_an_active_grant_for_a_live_same_epoch_session() {
        let mut host = OfflineHostAuthority::new();
        let mut sessions = HostSessions::default();
        let now = SystemTime::now();
        let token = sessions
            .authenticate(&mut Named, (), host.epoch(), now)
            .unwrap();
        let session = sessions.resolve(&token, host.epoch(), now).unwrap();
        assert_eq!(
            host.renew_for(session).unwrap_err().code,
            ErrorCode::LeaseInvalid
        );
        let (first, _) = host.start_for(session).unwrap();
        assert_eq!(
            first.expires_at.duration_since(first.issued_at).unwrap(),
            CONTROL_LEASE_TTL
        );

        std::thread::sleep(Duration::from_millis(5));
        let (renewed, signature) = host.renew_for(session).unwrap();
        renewed.verify(&host.verifying_key(), &signature).unwrap();
        assert!(renewed.expires_at > first.expires_at);
        assert_eq!(renewed.security_epoch, first.security_epoch);
        assert_eq!(renewed.user_id, "user-x");
        assert_eq!(renewed.client_id, "client-x");
        assert_eq!(host.state(), State::RemoteActive);

        let foreign = sessions
            .authenticate(&mut Named, (), host.epoch().next(), now)
            .unwrap();
        let foreign = sessions
            .resolve(&foreign, host.epoch().next(), now)
            .unwrap();
        assert_eq!(
            host.renew_for(foreign).unwrap_err().code,
            ErrorCode::SessionEpochMismatch
        );
        let issued = now - AUTH_SESSION_TTL - Duration::from_secs(1);
        let old = sessions
            .authenticate(&mut Named, (), host.epoch(), issued)
            .unwrap();
        let old = sessions.resolve(&old, host.epoch(), issued).unwrap();
        assert_eq!(
            host.renew_for(old).unwrap_err().code,
            ErrorCode::AuthInvalid
        );
        assert_eq!(host.state(), State::RemoteActive);
    }

    #[test]
    fn stale_or_expired_sessions_never_get_a_lease() {
        let mut host = OfflineHostAuthority::new();
        let mut sessions = HostSessions::default();
        let mut verifier = DemoCredentialVerifier::new("proof".into());
        let foreign = sessions
            .authenticate(
                &mut verifier,
                demo_credential(),
                host.epoch().next(),
                SystemTime::now(),
            )
            .unwrap();
        let session = sessions
            .resolve(&foreign, host.epoch().next(), SystemTime::now())
            .unwrap();
        assert_eq!(
            host.start_for(session).unwrap_err().code,
            ErrorCode::SessionEpochMismatch
        );

        let issued = SystemTime::now() - AUTH_SESSION_TTL - Duration::from_secs(1);
        let old = sessions
            .authenticate(&mut verifier, demo_credential(), host.epoch(), issued)
            .unwrap();
        let session = sessions.resolve(&old, host.epoch(), issued).unwrap();
        assert_eq!(
            host.start_for(session).unwrap_err().code,
            ErrorCode::AuthInvalid
        );
        assert_eq!(host.state(), State::LocalLocked);
    }

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
