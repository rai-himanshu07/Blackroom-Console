use serde::{Deserialize, Serialize};

use crate::epoch::SecurityEpoch;
use crate::lease::ControlLease;

/// Hostd-to-agent updates. Only the signed lease grants input; the transport
/// must authenticate the host peer before accepting either variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthorityUpdate {
    Grant {
        lease: ControlLease,
        signature: Vec<u8>,
    },
    Revoke {
        epoch: SecurityEpoch,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lease::Capability;
    use ed25519_dalek::{Signature, SigningKey};
    use std::time::{Duration, SystemTime};

    #[test]
    fn authority_update_preserves_signed_lease() {
        let signing_key = SigningKey::from_bytes(&[11; 32]);
        let now = SystemTime::now();
        let lease = ControlLease {
            session_id: "session-a".into(),
            host_id: "host-a".into(),
            user_id: "user-a".into(),
            client_id: "client-a".into(),
            security_epoch: SecurityEpoch::INITIAL,
            issued_at: now,
            expires_at: now + Duration::from_secs(30),
            capabilities: vec![Capability::Control],
        };
        let update = AuthorityUpdate::Grant {
            signature: lease.sign(&signing_key).to_bytes().to_vec(),
            lease,
        };
        let bytes = serde_json::to_vec(&update).unwrap();
        let AuthorityUpdate::Grant { lease, signature } =
            serde_json::from_slice::<AuthorityUpdate>(&bytes).unwrap()
        else {
            panic!("expected a grant");
        };
        let signature = Signature::from_slice(&signature).unwrap();
        lease
            .verify(&signing_key.verifying_key(), &signature)
            .unwrap();
    }
}
