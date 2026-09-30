use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::time::SystemTime;

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::lease::ControlLease;
use blackroom_core::limits::CONTROL_LEASE_TTL;
use blackroom_core::protocol::AuthorityUpdate;
use blackroom_core::state::State;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use rustix::fs::{AtFlags, FlockOperation, Mode, OFlags};

use crate::OfflineHostAuthority;
use crate::auth::AuthSession;

const KEY_FILE: &str = "host-identity.key";
const EPOCH_FILE: &str = "security-epoch";
const EMERGENCY_FILE: &str = "emergency-stop";
const RECOVERY_FILE: &str = "recovery-pending";

fn validate_directory(directory: &File) -> io::Result<()> {
    let metadata = directory.metadata()?;
    if !metadata.is_dir()
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "host authority directory must be owner-controlled",
        ));
    }
    Ok(())
}

fn read_private(directory: &File, name: &str, length: u64) -> io::Result<Vec<u8>> {
    let fd = rustix::fs::openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let mut file = File::from(fd);
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
        || metadata.len() != length
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "host authority state has invalid ownership, mode or length",
        ));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.read_to_end(&mut bytes)?;
    if bytes.len() != length as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "host authority state changed while reading",
        ));
    }
    Ok(bytes)
}

fn create_private(directory: &File, name: &str, bytes: &[u8]) -> io::Result<()> {
    let fd = rustix::fs::openat(
        directory,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )?;
    let mut file = File::from(fd);
    file.write_all(bytes)?;
    file.sync_all()?;
    rustix::fs::fsync(directory)?;
    Ok(())
}

fn persist_epoch(directory: &File, epoch: SecurityEpoch) -> io::Result<()> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    let temporary = format!(".epoch-{:032x}.tmp", u128::from_be_bytes(random));
    let fd = rustix::fs::openat(
        directory,
        temporary.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )?;
    let mut file = File::from(fd);
    let result = (|| {
        file.write_all(&epoch.value().to_be_bytes())?;
        file.sync_all()?;
        rustix::fs::renameat(directory, temporary.as_str(), directory, EPOCH_FILE)?;
        rustix::fs::fsync(directory)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = rustix::fs::unlinkat(directory, temporary.as_str(), AtFlags::empty());
    }
    result
}

fn next_epoch(current: SecurityEpoch) -> io::Result<SecurityEpoch> {
    current
        .value()
        .checked_add(1)
        .map(SecurityEpoch::from_value)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "security epoch exhausted"))
}

/// Offline file-backed authority, scoped to an already-opened trusted state
/// directory. No system paths or installed agent socket are accessed here.
pub struct PersistentHostAuthority {
    inner: OfflineHostAuthority,
    directory: File,
    blocked: bool,
}

impl PersistentHostAuthority {
    pub fn recovery_epoch(directory: &File) -> io::Result<Option<SecurityEpoch>> {
        validate_directory(directory)?;
        match read_private(directory, RECOVERY_FILE, 8) {
            Ok(bytes) => Ok(Some(SecurityEpoch::from_value(u64::from_be_bytes(
                bytes
                    .try_into()
                    .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?,
            )))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn verify_recovery(directory: &File, expected: SecurityEpoch) -> io::Result<()> {
        if Self::recovery_epoch(directory)? != Some(expected) || Self::emergency_pending(directory)?
        {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        rustix::fs::unlinkat(directory, RECOVERY_FILE, AtFlags::empty())?;
        rustix::fs::fsync(directory)?;
        Ok(())
    }

    pub fn emergency_pending(directory: &File) -> io::Result<bool> {
        Ok(Self::emergency_epoch(directory)?.is_some())
    }

    pub fn emergency_epoch(directory: &File) -> io::Result<Option<SecurityEpoch>> {
        validate_directory(directory)?;
        match read_private(directory, EMERGENCY_FILE, 8) {
            Ok(bytes) => {
                let marker = u64::from_be_bytes(
                    bytes
                        .try_into()
                        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?,
                );
                if marker == 0 {
                    return Err(io::Error::from(io::ErrorKind::InvalidData));
                }
                let current = u64::from_be_bytes(
                    read_private(directory, EPOCH_FILE, 8)?
                        .try_into()
                        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?,
                );
                Ok(Some(SecurityEpoch::from_value(marker.max(current))))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Offline-only independent stop: the durable marker blocks grants even
    /// if the epoch write is interrupted. The next start never clears it.
    pub fn emergency_stop(directory: &File) -> io::Result<SecurityEpoch> {
        validate_directory(directory)?;
        read_private(directory, KEY_FILE, 32)?;
        let current = u64::from_be_bytes(
            read_private(directory, EPOCH_FILE, 8)?
                .try_into()
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?,
        );
        let existing = || -> io::Result<u64> {
            let bytes = read_private(directory, EMERGENCY_FILE, 8)?;
            let value = u64::from_be_bytes(
                bytes
                    .try_into()
                    .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?,
            );
            if value == 0 {
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
            Ok(value)
        };
        let target = match existing() {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let next = current.saturating_add(1);
                let mut random = [0_u8; 16];
                getrandom::fill(&mut random)
                    .map_err(|error| io::Error::other(error.to_string()))?;
                let temporary = format!(".emergency-{:032x}.tmp", u128::from_be_bytes(random));
                create_private(directory, temporary.as_str(), &next.to_be_bytes())?;
                let published = rustix::fs::linkat(
                    directory,
                    temporary.as_str(),
                    directory,
                    EMERGENCY_FILE,
                    AtFlags::empty(),
                );
                rustix::fs::unlinkat(directory, temporary.as_str(), AtFlags::empty())?;
                match published {
                    Ok(()) => {
                        rustix::fs::fsync(directory)?;
                        next
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => existing()?,
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) => return Err(error),
        };
        if current < target {
            persist_epoch(directory, SecurityEpoch::from_value(target))?;
        }
        if current == u64::MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "security epoch exhausted",
            ));
        }
        Ok(SecurityEpoch::from_value(target.max(current)))
    }

    pub fn open(directory: &File) -> io::Result<Self> {
        validate_directory(directory)?;
        let fd = rustix::fs::openat(
            directory,
            ".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        let directory = File::from(fd);
        rustix::fs::flock(&directory, FlockOperation::NonBlockingLockExclusive)?;

        let key = read_private(&directory, KEY_FILE, 32);
        let epoch = read_private(&directory, EPOCH_FILE, 8);
        let (secret, epoch) = match (key, epoch) {
            (Ok(secret), Ok(bytes)) => {
                let current = SecurityEpoch::from_value(u64::from_be_bytes(
                    bytes
                        .as_slice()
                        .try_into()
                        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid epoch"))?,
                ));
                let next = next_epoch(current)?;
                persist_epoch(&directory, next)?;
                (secret, next)
            }
            (Err(key_error), Err(epoch_error))
                if key_error.kind() == io::ErrorKind::NotFound
                    && epoch_error.kind() == io::ErrorKind::NotFound =>
            {
                let mut secret = vec![0_u8; 32];
                getrandom::fill(&mut secret)
                    .map_err(|error| io::Error::other(error.to_string()))?;
                create_private(&directory, KEY_FILE, &secret)?;
                create_private(
                    &directory,
                    EPOCH_FILE,
                    &SecurityEpoch::INITIAL.value().to_be_bytes(),
                )?;
                (secret, SecurityEpoch::INITIAL)
            }
            (Err(error), _) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            (_, Err(error)) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "host authority state is incomplete",
                ));
            }
        };
        let signing_key = SigningKey::from_bytes(&secret.as_slice().try_into().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid host identity length")
        })?);
        let blocked =
            Self::emergency_pending(&directory)? || Self::recovery_epoch(&directory)?.is_some();
        Ok(Self {
            inner: OfflineHostAuthority {
                signing_key,
                epoch,
                state: State::LocalLocked,
            },
            directory,
            blocked,
        })
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.inner.verifying_key()
    }

    pub fn epoch(&self) -> SecurityEpoch {
        self.inner.epoch()
    }

    pub fn state(&self) -> State {
        if self.blocked {
            State::FailedSafe
        } else {
            self.inner.state()
        }
    }

    pub fn emergency_required(&mut self) -> bool {
        if Self::emergency_pending(&self.directory).unwrap_or(true) {
            self.blocked = true;
            self.inner.state = State::LocalLocked;
        }
        self.blocked
    }

    pub fn start(&mut self) -> Result<(ControlLease, Signature), BlackroomError> {
        self.begin(|inner| inner.start())
    }

    /// Session-bound grant. The session is checked once, before any recovery
    /// intent is persisted, and the same instant is used to sign the lease.
    pub fn start_for(
        &mut self,
        session: &AuthSession,
    ) -> Result<(ControlLease, Signature), BlackroomError> {
        let now = SystemTime::now();
        self.inner.check_session(session, now)?;
        self.begin(|inner| {
            inner.issue(
                session.principal(),
                Some(session.expires_at()),
                CONTROL_LEASE_TTL,
                now,
            )
        })
    }

    fn begin(
        &mut self,
        issue: impl FnOnce(
            &mut OfflineHostAuthority,
        ) -> Result<(ControlLease, Signature), BlackroomError>,
    ) -> Result<(ControlLease, Signature), BlackroomError> {
        if self.emergency_required() {
            return Err(BlackroomError::new(
                ErrorCode::RecoveryFailed,
                "host authority state cannot be trusted",
            ));
        }
        if self.inner.state() == State::LocalLocked
            && create_private(
                &self.directory,
                RECOVERY_FILE,
                &self.inner.epoch().value().to_be_bytes(),
            )
            .is_err()
        {
            self.blocked = true;
            return Err(BlackroomError::new(
                ErrorCode::RecoveryFailed,
                "host recovery intent could not be persisted",
            ));
        }
        issue(&mut self.inner)
    }

    pub fn grant_update(&mut self) -> Result<AuthorityUpdate, BlackroomError> {
        let (lease, signature) = self.start()?;
        Ok(AuthorityUpdate::Grant {
            lease,
            signature: signature.to_bytes().to_vec(),
        })
    }

    pub fn grant_update_for(
        &mut self,
        session: &AuthSession,
    ) -> Result<AuthorityUpdate, BlackroomError> {
        let (lease, signature) = self.start_for(session)?;
        Ok(AuthorityUpdate::Grant {
            lease,
            signature: signature.to_bytes().to_vec(),
        })
    }

    /// Re-signs the active grant; the recovery marker and epoch are untouched.
    pub fn renew_update_for(
        &mut self,
        session: &AuthSession,
    ) -> Result<AuthorityUpdate, BlackroomError> {
        if self.emergency_required() {
            return Err(BlackroomError::new(
                ErrorCode::RecoveryFailed,
                "host authority state cannot be trusted",
            ));
        }
        let (lease, signature) = self.inner.renew_for(session)?;
        Ok(AuthorityUpdate::Grant {
            lease,
            signature: signature.to_bytes().to_vec(),
        })
    }

    pub fn revoke_update(&mut self) -> io::Result<AuthorityUpdate> {
        self.emergency_required();
        if self.inner.state() != State::RemoteActive || self.blocked {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "host authority is not active",
            ));
        }
        let next = match next_epoch(self.inner.epoch())
            .and_then(|next| persist_epoch(&self.directory, next).map(|_| next))
        {
            Ok(next) => next,
            Err(error) => {
                self.blocked = true;
                self.inner.state = State::LocalLocked;
                return Err(error);
            }
        };
        debug_assert_eq!(next, self.inner.epoch().next());
        Ok(self.inner.revoke_update())
    }

    pub fn complete_recovery(&self, granted_epoch: SecurityEpoch) -> io::Result<()> {
        if self.blocked || self.inner.state() != State::LocalLocked {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        match Self::recovery_epoch(&self.directory)? {
            Some(_) => Self::verify_recovery(&self.directory, granted_epoch),
            None if !Self::emergency_pending(&self.directory)? => Ok(()),
            None => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn private_dir() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        directory
    }

    #[test]
    fn incomplete_or_symlinked_identity_never_regenerates() {
        let directory = private_dir();
        let dirfd = File::open(directory.path()).unwrap();
        std::fs::write(directory.path().join(EPOCH_FILE), 0_u64.to_be_bytes()).unwrap();
        assert!(PersistentHostAuthority::open(&dirfd).is_err());

        let directory = private_dir();
        let dirfd = File::open(directory.path()).unwrap();
        let host = PersistentHostAuthority::open(&dirfd).unwrap();
        drop(host);
        std::fs::rename(
            directory.path().join(KEY_FILE),
            directory.path().join("saved-key"),
        )
        .unwrap();
        symlink(
            directory.path().join("saved-key"),
            directory.path().join(KEY_FILE),
        )
        .unwrap();
        assert!(PersistentHostAuthority::open(&dirfd).is_err());
    }

    #[test]
    fn insecure_directory_or_corrupt_epoch_fails_closed() {
        let directory = private_dir();
        let dirfd = File::open(directory.path()).unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o750)).unwrap();
        assert_eq!(
            PersistentHostAuthority::open(&dirfd).err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let host = PersistentHostAuthority::open(&dirfd).unwrap();
        drop(host);
        std::fs::write(directory.path().join(EPOCH_FILE), [0_u8; 9]).unwrap();
        assert!(PersistentHostAuthority::open(&dirfd).is_err());
    }

    #[test]
    fn persisted_revoke_precedes_new_host_restart() {
        let directory = private_dir();
        let dirfd = File::open(directory.path()).unwrap();
        let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
        let (old_lease, _) = host.start().unwrap();
        let AuthorityUpdate::Revoke { epoch } = host.revoke_update().unwrap() else {
            panic!("expected a revocation");
        };
        assert_eq!(epoch, SecurityEpoch::INITIAL.next());
        drop(host);
        let restarted = PersistentHostAuthority::open(&dirfd).unwrap();
        assert_eq!(restarted.epoch(), epoch.next());
        assert_ne!(old_lease.security_epoch, restarted.epoch());
    }

    #[test]
    fn interrupted_grant_blocks_new_start_until_recovery_is_verified() {
        let directory = private_dir();
        let dirfd = File::open(directory.path()).unwrap();
        let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
        let (lease, _) = host.start().unwrap();
        drop(host);

        let mut restarted = PersistentHostAuthority::open(&dirfd).unwrap();
        assert!(restarted.epoch() > lease.security_epoch);
        assert_eq!(
            restarted.start().unwrap_err().code,
            ErrorCode::RecoveryFailed
        );
    }

    #[test]
    fn refused_session_leaves_no_recovery_intent() {
        use crate::auth::HostSessions;
        use crate::offline_control::{DEMO_CODE, DemoCredential, DemoCredentialVerifier};

        let directory = private_dir();
        let dirfd = File::open(directory.path()).unwrap();
        let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
        let mut sessions = HostSessions::default();
        let mut verifier = DemoCredentialVerifier::new("proof".into());
        let credential = || DemoCredential {
            proof: "proof".into(),
            demo_code: DEMO_CODE.into(),
        };
        let now = std::time::SystemTime::now();
        let foreign_epoch = host.epoch().next();
        let token = sessions
            .authenticate(&mut verifier, credential(), foreign_epoch, now)
            .unwrap();
        let session = sessions.resolve(&token, foreign_epoch, now).unwrap();
        assert_eq!(
            host.grant_update_for(session).unwrap_err().code,
            ErrorCode::SessionEpochMismatch
        );
        assert!(!directory.path().join(RECOVERY_FILE).exists());
        assert_eq!(host.state(), State::LocalLocked);

        let token = sessions
            .authenticate(&mut verifier, credential(), host.epoch(), now)
            .unwrap();
        let session = sessions.resolve(&token, host.epoch(), now).unwrap();
        assert!(matches!(
            host.grant_update_for(session).unwrap(),
            AuthorityUpdate::Grant { .. }
        ));
        assert!(directory.path().join(RECOVERY_FILE).exists());
    }

    #[test]
    fn persisted_host_produces_signed_updates() {
        let directory = private_dir();
        let dirfd = File::open(directory.path()).unwrap();
        let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
        let AuthorityUpdate::Grant { lease, signature } = host.grant_update().unwrap() else {
            panic!("expected a grant");
        };
        let signature = Signature::from_slice(&signature).unwrap();
        lease.verify(&host.verifying_key(), &signature).unwrap();
        assert_eq!(host.state(), State::RemoteActive);
        let AuthorityUpdate::Revoke { epoch } = host.revoke_update().unwrap() else {
            panic!("expected a revoke");
        };
        assert_eq!(host.state(), State::LocalLocked);
        assert_eq!(epoch, host.epoch());
    }

    #[test]
    fn exhausted_epoch_blocks_new_host_grants() {
        let directory = private_dir();
        let dirfd = File::open(directory.path()).unwrap();
        drop(PersistentHostAuthority::open(&dirfd).unwrap());
        std::fs::write(
            directory.path().join(EPOCH_FILE),
            (u64::MAX - 1).to_be_bytes(),
        )
        .unwrap();
        let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
        assert_eq!(host.epoch().value(), u64::MAX);
        host.start().unwrap();
        assert!(host.revoke_update().is_err());
        assert_eq!(host.start().unwrap_err().code, ErrorCode::RecoveryFailed);
    }

    #[test]
    fn independent_emergency_marker_persists_epoch_and_blocks_restart() {
        let directory = private_dir();
        let dirfd = File::open(directory.path()).unwrap();
        let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
        let (old_lease, _) = host.start().unwrap();
        let target = PersistentHostAuthority::emergency_stop(&dirfd).unwrap();
        assert_eq!(target, old_lease.security_epoch.next());
        assert_eq!(
            PersistentHostAuthority::emergency_stop(&dirfd).unwrap(),
            target
        );
        assert_eq!(host.start().unwrap_err().code, ErrorCode::RecoveryFailed);
        assert!(host.revoke_update().is_err());
        drop(host);
        let mut restarted = PersistentHostAuthority::open(&dirfd).unwrap();
        assert!(restarted.epoch() > old_lease.security_epoch);
        assert_eq!(
            restarted.start().unwrap_err().code,
            ErrorCode::RecoveryFailed
        );
    }

    #[test]
    fn concurrent_emergency_requests_converge_on_one_persisted_stop() {
        let directory = private_dir();
        let dirfd = File::open(directory.path()).unwrap();
        let host = PersistentHostAuthority::open(&dirfd).unwrap();
        let previous = host.epoch();
        std::thread::scope(|scope| {
            let first = scope.spawn(|| PersistentHostAuthority::emergency_stop(&dirfd));
            let second = scope.spawn(|| PersistentHostAuthority::emergency_stop(&dirfd));
            assert!(first.join().unwrap().is_ok());
            assert!(second.join().unwrap().is_ok());
        });
        assert!(PersistentHostAuthority::emergency_pending(&dirfd).unwrap());
        let stored = u64::from_be_bytes(
            std::fs::read(directory.path().join(EPOCH_FILE))
                .unwrap()
                .try_into()
                .unwrap(),
        );
        assert!(stored > previous.value());
    }
}
