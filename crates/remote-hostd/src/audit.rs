//! Append-only security audit log for the offline hostd (Doc 13 event taxonomy,
//! reduced). Events carry no secret by construction: the only strings are
//! static codes and the principal's user/client identifiers, which JSON
//! escaping keeps on one line even when a verifier supplies hostile text.

use std::fs::File;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::fs::{Mode, OFlags};
use serde::Serialize;

pub const AUDIT_FILE: &str = "audit.log";
pub const ROTATED_AUDIT_FILE: &str = "audit.log.1";
const MAX_AUDIT_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevokeCause {
    Revoked,
    LeaseExpired,
    SessionEnded,
    AbuseLimit,
    AgentRefused,
    PeerClosed,
    IsolationFailed,
    IsolationLost,
}

#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AuditEvent<'a> {
    HostStarted {
        epoch: u64,
    },
    AuthRefused {
        code: &'static str,
    },
    LoginAccepted {
        user_id: &'a str,
        client_id: &'a str,
    },
    /// An operator action on the running host (`revoke-all`, `disable`, ...); `count` is how many
    /// sessions it ended.
    AdminAction {
        action: &'static str,
        count: u64,
    },
    CredentialChanged {
        kind: &'static str,
        action: &'static str,
    },
    GrantIssued {
        epoch: u64,
        user_id: &'a str,
        client_id: &'a str,
    },
    RenewRefused {
        epoch: u64,
    },
    GrantRevoked {
        epoch: u64,
        cause: RevokeCause,
    },
}

#[derive(Serialize)]
struct Line<'a> {
    ts_unix_ms: u128,
    #[serde(flatten)]
    event: &'a AuditEvent<'a>,
}

pub struct AuditLog {
    directory: File,
    file: File,
}

fn open_log(directory: &File) -> io::Result<File> {
    let fd = rustix::fs::openat(
        directory,
        AUDIT_FILE,
        OFlags::WRONLY | OFlags::APPEND | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )?;
    let file = File::from(fd);
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "audit log must be an owner-only regular file",
        ));
    }
    Ok(file)
}

impl AuditLog {
    /// `directory` must already be the validated owner-only state directory.
    pub fn open(directory: &File) -> io::Result<Self> {
        Ok(Self {
            directory: directory.try_clone()?,
            file: open_log(directory)?,
        })
    }

    /// One durable line per event; a full log rotates to a single older file.
    pub fn record(&mut self, event: &AuditEvent<'_>) -> io::Result<()> {
        if self.file.metadata()?.len() >= MAX_AUDIT_BYTES {
            rustix::fs::renameat(
                &self.directory,
                AUDIT_FILE,
                &self.directory,
                ROTATED_AUDIT_FILE,
            )?;
            self.file = open_log(&self.directory)?;
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?;
        let mut line = serde_json::to_vec(&Line {
            ts_unix_ms: timestamp.as_millis(),
            event,
        })
        .map_err(io::Error::other)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.sync_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn state_dir() -> (tempfile::TempDir, File) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let fd = File::open(directory.path()).unwrap();
        (directory, fd)
    }

    fn lines(directory: &tempfile::TempDir, name: &str) -> Vec<serde_json::Value> {
        std::fs::read_to_string(directory.path().join(name))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn events_are_owner_only_single_line_json_without_extra_fields() {
        let (directory, fd) = state_dir();
        let mut log = AuditLog::open(&fd).unwrap();
        log.record(&AuditEvent::HostStarted { epoch: 3 }).unwrap();
        log.record(&AuditEvent::AuthRefused {
            code: "AUTH_INVALID",
        })
        .unwrap();
        log.record(&AuditEvent::GrantIssued {
            epoch: 3,
            user_id: "user\nforged",
            client_id: "client\"}{",
        })
        .unwrap();
        log.record(&AuditEvent::GrantRevoked {
            epoch: 3,
            cause: RevokeCause::LeaseExpired,
        })
        .unwrap();
        let mode = std::fs::metadata(directory.path().join(AUDIT_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);

        let entries = lines(&directory, AUDIT_FILE);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0]["event"], "host_started");
        assert_eq!(entries[0]["epoch"], 3);
        assert_eq!(entries[1]["code"], "AUTH_INVALID");
        assert_eq!(entries[2]["user_id"], "user\nforged");
        assert_eq!(entries[2]["client_id"], "client\"}{");
        assert_eq!(entries[3]["cause"], "lease_expired");
        assert!(
            entries
                .iter()
                .all(|entry| entry["ts_unix_ms"].as_u64().unwrap() > 0)
        );
    }

    #[test]
    fn reopening_appends_and_unsafe_files_are_refused() {
        let (directory, fd) = state_dir();
        AuditLog::open(&fd)
            .unwrap()
            .record(&AuditEvent::HostStarted { epoch: 1 })
            .unwrap();
        AuditLog::open(&fd)
            .unwrap()
            .record(&AuditEvent::HostStarted { epoch: 2 })
            .unwrap();
        assert_eq!(lines(&directory, AUDIT_FILE).len(), 2);

        let path = directory.path().join(AUDIT_FILE);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(AuditLog::open(&fd).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::write(directory.path().join("elsewhere"), b"").unwrap();
        symlink(directory.path().join("elsewhere"), &path).unwrap();
        assert!(AuditLog::open(&fd).is_err());
    }

    #[test]
    fn a_full_log_rotates_once_without_losing_the_new_event() {
        let (directory, fd) = state_dir();
        let path = directory.path().join(AUDIT_FILE);
        let mut log = AuditLog::open(&fd).unwrap();
        log.record(&AuditEvent::HostStarted { epoch: 1 }).unwrap();
        let filler = vec![b' '; MAX_AUDIT_BYTES as usize];
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&filler)
            .unwrap();
        log.record(&AuditEvent::HostStarted { epoch: 2 }).unwrap();
        assert_eq!(lines(&directory, AUDIT_FILE).len(), 1);
        assert_eq!(lines(&directory, AUDIT_FILE)[0]["epoch"], 2);
        assert!(
            std::fs::metadata(directory.path().join(ROTATED_AUDIT_FILE))
                .unwrap()
                .len()
                >= MAX_AUDIT_BYTES
        );
    }
}
