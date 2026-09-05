//! `ControlLease` (Doc 07 §13; assessment C5) signed with `ed25519-dalek`
//! (architecture.md §4/§6 — reused, not re-litigated). Real host-identity
//! key management is Phase 15 (Host Security Authority); here a keypair is
//! just a value callers generate (e.g. a fresh test key), matching this
//! crate's no-I/O, logic-only scope.

use std::time::SystemTime;

use ed25519_dalek::{Signature, SignatureError, Signer, SigningKey, Verifier, VerifyingKey};

use crate::epoch::SecurityEpoch;
use crate::error::{BlackroomError, ErrorCode};
use crate::state::State;

/// Capability granted by a lease (assessment C6: lease capabilities are
/// `VIEW, CONTROL` for v1; touch/tablet/clipboard are deferred).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
