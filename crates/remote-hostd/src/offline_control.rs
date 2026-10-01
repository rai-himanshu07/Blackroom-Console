use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::limits::MAX_MESSAGE_SIZE_BYTES;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::auth::{CredentialVerifier, Principal};

pub const DEMO_CODE: &str = "SIMULATE";

pub struct DemoCredential {
    pub proof: String,
    pub demo_code: String,
}

/// Fake adapter: the public demo code and rotating simulation proof are
/// loopback simulation gates, not authentication. They yield the synthetic
/// principal only so the session and grant path can be exercised offline.
pub struct DemoCredentialVerifier {
    proof: String,
}

impl DemoCredentialVerifier {
    pub fn new(proof: String) -> Self {
        Self { proof }
    }

    pub fn proof(&self) -> &str {
        &self.proof
    }

    pub fn rotate(&mut self, proof: String) {
        self.proof = proof;
    }
}

impl CredentialVerifier for DemoCredentialVerifier {
    type Presented = DemoCredential;

    fn verify(&mut self, presented: DemoCredential) -> Result<Principal, BlackroomError> {
        if presented.proof == self.proof && presented.demo_code == DEMO_CODE {
            Ok(Principal::synthetic())
        } else {
            Err(BlackroomError::new(
                ErrorCode::AuthInvalid,
                "offline simulation credential refused",
            ))
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum OfflineCommand {
    Start {
        proof: String,
        demo_code: String,
    },
    Revoke {},
    Input {
        epoch: u64,
        sequence: u64,
        grant_id: String,
    },
    Renew {
        epoch: u64,
        grant_id: String,
    },
    Status {},
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineReply {
    pub accepted: bool,
    pub state: String,
    pub epoch: u64,
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_proof: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_grant: Option<String>,
    /// Absolute deadlines on accepted Start/Renew replies, for display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_expires_unix_ms: Option<u64>,
}

pub fn write_frame<T: Serialize>(stream: &mut UnixStream, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.is_empty() || bytes.len() > MAX_MESSAGE_SIZE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "offline control frame exceeds limit",
        ));
    }
    stream.set_write_timeout(Some(Duration::from_millis(300)))?;
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(&bytes)
}

pub fn read_frame<T: DeserializeOwned>(stream: &mut UnixStream) -> io::Result<T> {
    read_frame_within(stream, Duration::from_millis(300))
}

/// Like [`read_frame`] with a caller-chosen deadline, e.g. a Start that waits for physical input.
pub fn read_frame_within<T: DeserializeOwned>(
    stream: &mut UnixStream,
    within: Duration,
) -> io::Result<T> {
    let deadline = Instant::now() + within;
    let mut header = [0_u8; 4];
    read_until(stream, &mut header, deadline)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_MESSAGE_SIZE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid offline control length",
        ));
    }
    let mut bytes = vec![0_u8; length];
    read_until(stream, &mut bytes, deadline)?;
    serde_json::from_slice(&bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid offline control message",
        )
    })
}

fn read_until(stream: &mut UnixStream, bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    let mut read = 0;
    while read < bytes.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(&mut bytes[read..]) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(length) => read += length,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framed_offline_command_round_trip_and_rejects_extra_fields() {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_frame(
            &mut sender,
            &OfflineCommand::Input {
                epoch: 1,
                sequence: 1,
                grant_id: "g".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            read_frame::<OfflineCommand>(&mut receiver).unwrap(),
            OfflineCommand::Input { epoch: 1, sequence: 1, grant_id } if grant_id == "g"
        ));
        let invalid = serde_json::json!({"command": "start", "authenticated": true});
        write_frame(&mut sender, &invalid).unwrap();
        assert_eq!(
            read_frame::<OfflineCommand>(&mut receiver)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        write_frame(
            &mut sender,
            &serde_json::json!({"command": "start", "proof": "test"}),
        )
        .unwrap();
        assert_eq!(
            read_frame::<OfflineCommand>(&mut receiver)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
