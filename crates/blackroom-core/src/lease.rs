//! `ControlLease` (Doc 07 §13; assessment C5) signed with `ed25519-dalek`
//! (architecture.md §4/§6 — reused, not re-litigated). Real host-identity
//! key management is Phase 15 (Host Security Authority); here a keypair is
//! just a value callers generate (e.g. a fresh test key), matching this
//! crate's no-I/O, logic-only scope.

use std::time::SystemTime;

use ed25519_dalek::{Signature, SignatureError, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::epoch::SecurityEpoch;
use crate::error::{BlackroomError, ErrorCode};
use crate::state::State;

/// Capability granted by a lease (assessment C6: lease capabilities are
/// `VIEW, CONTROL` for v1; touch/tablet/clipboard are deferred).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    View,
    Control,
}

impl Capability {
    const fn as_str(self) -> &'static str {
        match self {
            Capability::View => "VIEW",
            Capability::Control => "CONTROL",
        }
    }
}

/// The 8 assessment-C5 fields (Doc 07 itself says `user`; C5 already
/// normalizes this to `user_id` — not a new conflict).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlLease {
    pub session_id: String,
    pub host_id: String,
    pub user_id: String,
    pub client_id: String,
    pub security_epoch: SecurityEpoch,
    pub issued_at: SystemTime,
    pub expires_at: SystemTime,
    pub capabilities: Vec<Capability>,
}

/// Values supplied by the trusted agent for each remote input event.
/// The verifier must come from the host identity, never from the client.
pub struct InputAuthorization<'a> {
    pub lease: &'a ControlLease,
    pub signature: &'a Signature,
    pub verifying_key: &'a VerifyingKey,
    pub authenticated: bool,
    pub authorized: bool,
    pub current_epoch: SecurityEpoch,
    pub current_state: State,
    pub current_session_id: &'a str,
    pub revoked: bool,
    pub now: SystemTime,
}

impl InputAuthorization<'_> {
    pub fn validate(&self) -> Result<(), BlackroomError> {
        if !self.authenticated {
            return Err(BlackroomError::new(
                ErrorCode::AuthInvalid,
                "remote input requires authentication",
            ));
        }
        if !self.authorized {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "remote input requires authorization",
            ));
        }
        self.lease
            .verify(self.verifying_key, self.signature)
            .map_err(|_| {
                BlackroomError::new(ErrorCode::LeaseInvalid, "invalid control lease signature")
            })?;
        self.lease.validate(
            self.current_epoch,
            self.current_state,
            self.current_session_id,
            self.revoked,
            self.now.max(SystemTime::now()),
        )
    }

    pub fn dispatch<T>(
        &self,
        event: T,
        send: impl FnOnce(T) -> Result<(), BlackroomError>,
    ) -> Result<(), BlackroomError> {
        self.validate()?;
        send(event)
    }
}

impl ControlLease {
    /// Deterministic byte representation signed/verified. Plain formatted
    /// fields, not a wire format — the wire envelope is `protocol::envelope`.
    fn canonical_bytes(&self) -> Vec<u8> {
        let caps = self
            .capabilities
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.session_id,
            self.host_id,
            self.user_id,
            self.client_id,
            self.security_epoch.value(),
            unix_secs(self.issued_at),
            unix_secs(self.expires_at),
            caps,
        )
        .into_bytes()
    }

    /// Signs this lease with the host identity key (Doc 07 §13: "so
    /// `gnome-session-agent` verifies lease/epoch/expiry locally on every
    /// input event without a round trip").
    pub fn sign(&self, signing_key: &SigningKey) -> Signature {
        signing_key.sign(&self.canonical_bytes())
    }

    /// Verifies a signature produced by [`Self::sign`].
    pub fn verify(
        &self,
        verifying_key: &VerifyingKey,
        signature: &Signature,
    ) -> Result<(), SignatureError> {
        verifying_key.verify(&self.canonical_bytes(), signature)
    }

    /// Invariant 1: `remote_input_enabled == true` only if the lease is
    /// valid, unrevoked, epoch-current, bound to the current session, and
    /// the host is `REMOTE_ACTIVE` (Doc 07 §13: "reject remote input when:
    /// lease expired OR revoked OR security epoch changed OR session ID
    /// invalid OR host state != REMOTE_ACTIVE"). Callers track revocation
    /// externally (a lease is an immutable signed credential; revocation is
    /// a fact about the world, not a field on it).
    pub fn validate(
        &self,
        current_epoch: SecurityEpoch,
        current_state: State,
        current_session_id: &str,
        revoked: bool,
        now: SystemTime,
    ) -> Result<(), BlackroomError> {
        if revoked {
            return Err(BlackroomError::new(
                ErrorCode::LeaseRevoked,
                "control lease has been revoked",
            ));
        }
        if now >= self.expires_at {
            return Err(BlackroomError::new(
                ErrorCode::LeaseExpired,
                "control lease has expired",
            ));
        }
        if self.security_epoch != current_epoch {
            return Err(BlackroomError::new(
                ErrorCode::SessionEpochMismatch,
                "control lease security epoch does not match the current epoch",
            ));
        }
        if self.session_id != current_session_id {
            return Err(BlackroomError::new(
                ErrorCode::SessionNotFound,
                "control lease session ID does not match the current session",
            ));
        }
        if current_state != State::RemoteActive {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "remote input requires state == REMOTE_ACTIVE",
            ));
        }
        if !self.capabilities.contains(&Capability::Control) {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "remote input requires the CONTROL capability",
            ));
        }
        Ok(())
    }
}

fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sample_lease(epoch: SecurityEpoch, now: SystemTime) -> ControlLease {
        ControlLease {
            session_id: "rs_01TESTSESSION".to_string(),
            host_id: "bc_deadbeef".to_string(),
            user_id: "user".to_string(),
            client_id: "cl_01TESTCLIENT".to_string(),
            security_epoch: epoch,
            issued_at: now,
            expires_at: now + Duration::from_secs(30),
            capabilities: vec![Capability::View, Capability::Control],
        }
    }

    #[test]
    fn serialized_control_lease_keeps_signature() {
        let lease = sample_lease(SecurityEpoch::INITIAL, SystemTime::now());
        let (signing_key, verifying_key) = test_keypair();
        let signature = lease.sign(&signing_key);
        let wire = serde_json::to_vec(&lease).unwrap();
        let restored: ControlLease = serde_json::from_slice(&wire).unwrap();
        assert_eq!(restored, lease);
        restored.verify(&verifying_key, &signature).unwrap();
    }

    #[test]
    fn serialized_lease_rejects_caller_authorization_fields() {
        let lease = sample_lease(SecurityEpoch::INITIAL, SystemTime::now());
        let mut value = serde_json::to_value(lease).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("authenticated".into(), serde_json::json!(true));
        assert!(serde_json::from_value::<ControlLease>(value).is_err());
    }

    #[test]
    fn validate_rejects_view_only_lease_for_remote_input() {
        let now = SystemTime::now();
        let mut lease = sample_lease(SecurityEpoch::INITIAL, now);
        lease.capabilities = vec![Capability::View];
        let error = lease
            .validate(
                SecurityEpoch::INITIAL,
                State::RemoteActive,
                "rs_01TESTSESSION",
                false,
                now,
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::LeaseInvalid);
    }

    #[test]
    fn input_authorization_checks_auth_signature_and_current_lease() {
        let now = SystemTime::now();
        let lease = sample_lease(SecurityEpoch::INITIAL, now);
        let (signing_key, verifying_key) = test_keypair();
        let signature = lease.sign(&signing_key);
        let mut authorization = InputAuthorization {
            lease: &lease,
            signature: &signature,
            verifying_key: &verifying_key,
            authenticated: true,
            authorized: true,
            current_epoch: SecurityEpoch::INITIAL,
            current_state: State::RemoteActive,
            current_session_id: "rs_01TESTSESSION",
            revoked: false,
            now,
        };
        assert!(authorization.validate().is_ok());

        authorization.authenticated = false;
        assert_eq!(
            authorization.validate().unwrap_err().code,
            ErrorCode::AuthInvalid
        );
        authorization.authenticated = true;
        authorization.authorized = false;
        assert_eq!(
            authorization.validate().unwrap_err().code,
            ErrorCode::LeaseInvalid
        );
        authorization.authorized = true;

        let (other_signing_key, _) = test_keypair();
        let wrong_signature = lease.sign(&other_signing_key);
        authorization.signature = &wrong_signature;
        assert_eq!(
            authorization.validate().unwrap_err().code,
            ErrorCode::LeaseInvalid
        );
        authorization.signature = &signature;

        authorization.revoked = true;
        assert_eq!(
            authorization.validate().unwrap_err().code,
            ErrorCode::LeaseRevoked
        );
        authorization.revoked = false;
        authorization.current_session_id = "rs_OTHER";
        assert_eq!(
            authorization.validate().unwrap_err().code,
            ErrorCode::SessionNotFound
        );
        authorization.current_session_id = "rs_01TESTSESSION";
        authorization.current_state = State::LocalLocked;
        assert_eq!(
            authorization.validate().unwrap_err().code,
            ErrorCode::LeaseInvalid
        );
    }

    #[test]
    fn input_dispatch_never_calls_sink_after_revocation_or_state_change() {
        use std::cell::Cell;

        let now = SystemTime::now();
        let lease = sample_lease(SecurityEpoch::INITIAL, now);
        let (signing_key, verifying_key) = test_keypair();
        let signature = lease.sign(&signing_key);
        let mut authorization = InputAuthorization {
            lease: &lease,
            signature: &signature,
            verifying_key: &verifying_key,
            authenticated: true,
            authorized: true,
            current_epoch: SecurityEpoch::INITIAL,
            current_state: State::RemoteActive,
            current_session_id: "rs_01TESTSESSION",
            revoked: false,
            now,
        };
        let sent = Cell::new(0);
        let send = |event| {
            sent.set(sent.get() + event);
            Ok(())
        };

        authorization.dispatch(1, send).unwrap();
        assert_eq!(sent.get(), 1);
        authorization.revoked = true;
        assert!(authorization.dispatch(1, send).is_err());
        authorization.revoked = false;
        authorization.current_state = State::LocalLocked;
        assert!(authorization.dispatch(1, send).is_err());
        assert_eq!(sent.get(), 1);
    }

    #[test]
    fn input_dispatch_rechecks_expiry_when_snapshot_time_is_stale() {
        use std::cell::Cell;

        let stale_time = SystemTime::now() - Duration::from_secs(60);
        let lease = sample_lease(SecurityEpoch::INITIAL, stale_time);
        let (signing_key, verifying_key) = test_keypair();
        let signature = lease.sign(&signing_key);
        let authorization = InputAuthorization {
            lease: &lease,
            signature: &signature,
            verifying_key: &verifying_key,
            authenticated: true,
            authorized: true,
            current_epoch: SecurityEpoch::INITIAL,
            current_state: State::RemoteActive,
            current_session_id: "rs_01TESTSESSION",
            revoked: false,
            now: stale_time,
        };
        let called = Cell::new(false);
        assert_eq!(
            authorization.validate().unwrap_err().code,
            ErrorCode::LeaseExpired
        );
        assert_eq!(
            authorization
                .dispatch((), |_| {
                    called.set(true);
                    Ok(())
                })
                .unwrap_err()
                .code,
            ErrorCode::LeaseExpired
        );
        assert!(!called.get());
    }

    fn test_keypair() -> (SigningKey, VerifyingKey) {
        // ed25519-dalek 3.0's `rand_core` feature expects an
        // `rand_core::CryptoRng`; `getrandom::SysRng` (fallible `TryRng`)
        // is adapted to that with `UnwrapErr` (docs.rs/ed25519-dalek/3.0.0
        // `SigningKey::generate` example).
        use getrandom::rand_core::UnwrapErr;
        let signing_key = SigningKey::generate(&mut UnwrapErr(getrandom::SysRng));
        let verifying_key = signing_key.verifying_key();
        (signing_key, verifying_key)
    }

    #[test]
    fn sign_then_verify_round_trips() {
        let (signing_key, verifying_key) = test_keypair();
        let lease = sample_lease(SecurityEpoch::INITIAL, SystemTime::now());
        let signature = lease.sign(&signing_key);
        assert!(lease.verify(&verifying_key, &signature).is_ok());
    }

    #[test]
    fn verify_rejects_a_tampered_lease() {
        let (signing_key, verifying_key) = test_keypair();
        let lease = sample_lease(SecurityEpoch::INITIAL, SystemTime::now());
        let signature = lease.sign(&signing_key);
        let mut tampered = lease;
        tampered.user_id = "attacker".to_string();
        assert!(tampered.verify(&verifying_key, &signature).is_err());
    }

    #[test]
    fn validate_accepts_a_fresh_matching_lease() {
        let now = SystemTime::now();
        let lease = sample_lease(SecurityEpoch::INITIAL, now);
        assert!(
            lease
                .validate(
                    SecurityEpoch::INITIAL,
                    State::RemoteActive,
                    "rs_01TESTSESSION",
                    false,
                    now
                )
                .is_ok()
        );
    }

    #[test]
    fn validate_rejects_expired_lease() {
        let now = SystemTime::now();
        let lease = sample_lease(SecurityEpoch::INITIAL, now);
        let later = now + Duration::from_secs(31);
        let err = lease
            .validate(
                SecurityEpoch::INITIAL,
                State::RemoteActive,
                "rs_01TESTSESSION",
                false,
                later,
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::LeaseExpired);
    }

    #[test]
    fn validate_rejects_revoked_lease() {
        let now = SystemTime::now();
        let lease = sample_lease(SecurityEpoch::INITIAL, now);
        let err = lease
            .validate(
                SecurityEpoch::INITIAL,
                State::RemoteActive,
                "rs_01TESTSESSION",
                true,
                now,
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::LeaseRevoked);
    }

    #[test]
    fn validate_rejects_epoch_mismatch() {
        let now = SystemTime::now();
        let lease = sample_lease(SecurityEpoch::INITIAL, now);
        let err = lease
            .validate(
                SecurityEpoch::INITIAL.next(),
                State::RemoteActive,
                "rs_01TESTSESSION",
                false,
                now,
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SessionEpochMismatch);
    }

    #[test]
    fn validate_rejects_wrong_session_id() {
        let now = SystemTime::now();
        let lease = sample_lease(SecurityEpoch::INITIAL, now);
        let err = lease
            .validate(
                SecurityEpoch::INITIAL,
                State::RemoteActive,
                "rs_01DIFFERENTSESSION",
                false,
                now,
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SessionNotFound);
    }

    #[test]
    fn validate_rejects_wrong_state() {
        let now = SystemTime::now();
        let lease = sample_lease(SecurityEpoch::INITIAL, now);
        let err = lease
            .validate(
                SecurityEpoch::INITIAL,
                State::RemoteDegraded,
                "rs_01TESTSESSION",
                false,
                now,
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::LeaseInvalid);
    }
}
