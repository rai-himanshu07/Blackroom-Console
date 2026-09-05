//! Message envelope (Doc 16 §32) and request IDs (Doc 16 §33). The
//! concrete message catalogue (Plane A–D) is Phase 12+ scope (assessment
//! C18/C19); this phase only needs the envelope shape and its
//! idempotency/staleness-relevant fields.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A `req_<ULID>`-style request identifier (architecture.md §2 correlation
/// ID convention; Doc 16 §33: "Responses must reference the request... for
/// correlation, timeout handling, duplicate detection, race debugging").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestId(String);

impl RequestId {
    pub fn generate() -> Self {
        Self(format!("req_{}", ulid::Ulid::generate()))
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The common message envelope (Doc 16 §32). "Not every field is required
/// for every message" (§32) — `session_id`/`transition_id`/`security_epoch`
/// are `Option` accordingly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub protocol_version: u32,
    pub message_type: String,
    pub request_id: RequestId,
    /// Unix seconds. A plain integer, not a `time`-crate type, so this
    /// wire shape does not depend on a timestamp-serialization decision
    /// that belongs to Phase 12.
    pub timestamp: u64,
    pub sender: String,
    pub session_id: Option<String>,
    pub transition_id: Option<String>,
    pub security_epoch: Option<u64>,
    pub payload: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_id_has_the_req_prefix() {
        let id = RequestId::generate();
        assert!(id.to_string().starts_with("req_"));
    }

    #[test]
    fn envelope_round_trips_through_json() {
        let envelope = Envelope {
            protocol_version: 1,
            message_type: "GET_SESSION_STATUS".to_string(),
            request_id: RequestId::generate(),
            timestamp: 1_757_030_400,
            sender: "remote-gateway".to_string(),
            session_id: Some("rs_01TESTSESSION".to_string()),
            transition_id: None,
            security_epoch: Some(0),
            payload: serde_json::json!({}),
        };
        let json = serde_json::to_string(&envelope).expect("serializes");
        let round_tripped: Envelope = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(round_tripped.request_id, envelope.request_id);
        assert_eq!(round_tripped.message_type, envelope.message_type);
        assert_eq!(round_tripped.session_id, envelope.session_id);
    }
}
