#![forbid(unsafe_code)]
//! Owner-only secret files with atomic replacement and schema versions (Doc 17 secrets storage).
//!
//! Every file is a JSON envelope `{"schema": N, "data": ...}` inside one directory that the
//! current user owns and nobody else can enter. A file is never trusted when anything about it is
//! unexpected (type, owner, mode, size, syntax, unknown field): the caller gets
//! [`StoreError::Corrupt`] and must keep remote access disabled until the owner repairs it.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;

use rustix::fs::{AtFlags, FlockOperation, Mode, OFlags};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// What an operator sees whenever a store problem forces remote access off.
pub const DISABLED_BANNER: &str = "REMOTE ACCESS DISABLED";
pub const MAX_FILE_BYTES: u64 = 256 * 1024;
const LOCK_FILE: &str = ".store.lock";

#[derive(Debug)]
pub enum StoreError {
    /// The directory or file cannot be trusted; remote access must stay disabled.
    Corrupt(&'static str),
    /// The file was written by a newer version than this build understands.
    Unsupported {
        found: u32,
        supported: u32,
    },
    Io(io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(reason) => write!(
                formatter,
                "{DISABLED_BANNER}: credential store is not trustworthy ({reason})"
            ),
            Self::Unsupported { found, supported } => write!(
                formatter,
                "{DISABLED_BANNER}: credential store schema {found} is newer than {supported}"
            ),
            Self::Io(error) => write!(formatter, "credential store I/O error: {}", error.kind()),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rustix::io::Errno> for StoreError {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Io(error.into())
    }
}

impl StoreError {
    /// True when the right response is the `REMOTE ACCESS DISABLED` state.
    pub fn disables_remote_access(&self) -> bool {
        matches!(self, Self::Corrupt(_) | Self::Unsupported { .. })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    schema: u32,
    data: T,
}

/// Upgrades one schema step: called with version `v` and its data, returns version `v + 1` data.
pub type Migrate = fn(u32, serde_json::Value) -> Option<serde_json::Value>;

/// Names are a fixed alphabet with no separators, so a name can never leave the directory.
pub fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && !name.starts_with('.')
        && !name.contains("..")
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        })
}

pub struct SecretStore {
    directory: File,
}

/// Held while a read-modify-write is in progress; dropping it releases the lock.
pub struct StoreLock {
    _file: File,
}

impl SecretStore {
    /// `directory` must be a directory owned by this user that no one else can enter.
    pub fn open(directory: &File) -> Result<Self, StoreError> {
        let metadata = directory.metadata()?;
        if !metadata.is_dir()
            || metadata.uid() != rustix::process::getuid().as_raw()
            || metadata.mode() & 0o077 != 0
        {
            return Err(StoreError::Corrupt("directory is not private to this user"));
        }
        Ok(Self {
            directory: directory.try_clone()?,
        })
    }

    pub fn lock(&self) -> Result<StoreLock, StoreError> {
        let fd = rustix::fs::openat(
            &self.directory,
            LOCK_FILE,
            OFlags::WRONLY | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?;
        let file = File::from(fd);
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::getuid().as_raw()
            || metadata.mode() & 0o177 != 0
        {
            return Err(StoreError::Corrupt(
                "lock file is not a private regular file",
            ));
        }
        rustix::fs::flock(&file, FlockOperation::LockExclusive)?;
        Ok(StoreLock { _file: file })
    }

    /// `Ok(None)` only when the file does not exist; anything else odd is an error.
    pub fn read<T: DeserializeOwned>(
        &self,
        name: &str,
        schema: u32,
        migrate: Option<Migrate>,
    ) -> Result<Option<T>, StoreError> {
        if !valid_name(name) {
            return Err(StoreError::Corrupt("invalid file name"));
        }
        let fd = match rustix::fs::openat(
            &self.directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(StoreError::Corrupt("file cannot be opened without links")),
        };
        let mut file = File::from(fd);
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::getuid().as_raw()
            || metadata.mode() & 0o177 != 0
            || metadata.len() > MAX_FILE_BYTES
        {
            return Err(StoreError::Corrupt(
                "file is not a private bounded regular file",
            ));
        }
        let mut bytes = Zeroizing::new(Vec::new());
        file.read_to_end(&mut bytes)?;
        let envelope: Envelope<serde_json::Value> =
            serde_json::from_slice(&bytes).map_err(|_| StoreError::Corrupt("malformed file"))?;
        if envelope.schema == 0 {
            return Err(StoreError::Corrupt("schema 0 does not exist"));
        }
        if envelope.schema > schema {
            return Err(StoreError::Unsupported {
                found: envelope.schema,
                supported: schema,
            });
        }
        let mut data = envelope.data;
        for version in envelope.schema..schema {
            data = migrate
                .and_then(|migrate| migrate(version, data))
                .ok_or(StoreError::Corrupt("schema cannot be migrated"))?;
        }
        serde_json::from_value(data)
            .map(Some)
            .map_err(|_| StoreError::Corrupt("file does not match its schema"))
    }

    /// Replaces the file atomically: a reader sees the old or the new content, never a mix.
    pub fn write<T: Serialize>(
        &self,
        _lock: &StoreLock,
        name: &str,
        schema: u32,
        data: &T,
    ) -> Result<(), StoreError> {
        if !valid_name(name) || schema == 0 {
            return Err(StoreError::Corrupt("invalid file name or schema"));
        }
        let bytes = Zeroizing::new(
            serde_json::to_vec(&Envelope { schema, data })
                .map_err(|_| StoreError::Corrupt("value cannot be serialised"))?,
        );
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(StoreError::Corrupt("file would exceed the size limit"));
        }
        let mut random = [0_u8; 8];
        getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
        let temporary = format!(".tmp-{}", hex::encode(random));
        let fd = rustix::fs::openat(
            &self.directory,
            temporary.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?;
        let mut file = File::from(fd);
        let result = file
            .write_all(&bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| {
                rustix::fs::renameat(&self.directory, temporary.as_str(), &self.directory, name)
                    .map_err(io::Error::from)
            });
        if result.is_err() {
            let _ = rustix::fs::unlinkat(&self.directory, temporary.as_str(), AtFlags::empty());
        }
        result?;
        rustix::fs::fsync(&self.directory)?;
        Ok(())
    }

    /// Locked read-modify-write; a missing file starts from `T::default()`.
    pub fn update<T, R>(
        &self,
        name: &str,
        schema: u32,
        migrate: Option<Migrate>,
        change: impl FnOnce(&mut T) -> R,
    ) -> Result<R, StoreError>
    where
        T: Serialize + DeserializeOwned + Default,
    {
        let lock = self.lock()?;
        let mut value = self.read::<T>(name, schema, migrate)?.unwrap_or_default();
        let result = change(&mut value);
        self.write(&lock, name, schema, &value)?;
        Ok(result)
    }

    pub fn remove(&self, _lock: &StoreLock, name: &str) -> Result<(), StoreError> {
        if !valid_name(name) {
            return Err(StoreError::Corrupt("invalid file name"));
        }
        match rustix::fs::unlinkat(&self.directory, name, AtFlags::empty()) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        rustix::fs::fsync(&self.directory)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    #[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Sample {
        count: u32,
    }

    fn private() -> (tempfile::TempDir, SecretStore) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let store = SecretStore::open(&File::open(directory.path()).unwrap()).unwrap();
        (directory, store)
    }

    fn put(store: &SecretStore, name: &str, schema: u32, count: u32) {
        let lock = store.lock().unwrap();
        store.write(&lock, name, schema, &Sample { count }).unwrap();
    }

    #[test]
    fn a_value_round_trips_through_a_private_file() {
        let (directory, store) = private();
        assert_eq!(store.read::<Sample>("a", 1, None).unwrap(), None);
        put(&store, "a", 1, 7);
        assert_eq!(
            store.read::<Sample>("a", 1, None).unwrap(),
            Some(Sample { count: 7 })
        );
        let mode = std::fs::metadata(directory.path().join("a"))
            .unwrap()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(
            std::fs::read_dir(directory.path())
                .unwrap()
                .filter(|entry| {
                    let name = entry.as_ref().unwrap().file_name();
                    name.to_string_lossy().starts_with(".tmp-")
                })
                .count(),
            0
        );
    }

    #[test]
    fn a_directory_others_can_enter_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o750)).unwrap();
        let error = SecretStore::open(&File::open(directory.path()).unwrap())
            .err()
            .unwrap();
        assert!(error.disables_remote_access());
        assert!(error.to_string().starts_with(DISABLED_BANNER));
    }

    #[test]
    fn names_cannot_leave_the_directory() {
        let (_directory, store) = private();
        for name in [
            "../x",
            "/etc/passwd",
            "a/b",
            ".hidden",
            "a..b",
            "",
            "A",
            "%2e%2e%2fx",
        ] {
            assert!(!valid_name(name), "{name}");
            assert!(store.read::<Sample>(name, 1, None).is_err(), "{name}");
        }
        assert!(valid_name("totp-credentials"));
    }

    #[test]
    fn a_symlinked_file_is_never_followed_for_reading_or_writing() {
        let (directory, store) = private();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("target");
        std::fs::write(&target, b"untouched").unwrap();
        symlink(&target, directory.path().join("a")).unwrap();
        assert!(matches!(
            store.read::<Sample>("a", 1, None),
            Err(StoreError::Corrupt(_))
        ));
        put(&store, "a", 1, 1);
        assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
        assert!(
            !std::fs::symlink_metadata(directory.path().join("a"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_file_with_the_wrong_mode_size_syntax_or_fields_is_corrupt() {
        let (directory, store) = private();
        let path = directory.path().join("a");
        std::fs::write(&path, br#"{"schema":1,"data":{"count":1}}"#).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            store.read::<Sample>("a", 1, None),
            Err(StoreError::Corrupt(_))
        ));
        for bad in [
            &br#"not json"#[..],
            br#"{"schema":1,"data":{"count":1},"extra":1}"#,
            br#"{"schema":1,"data":{"count":1,"x":2}}"#,
            br#"{"schema":0,"data":{"count":1}}"#,
            br#"{"schema":1,"data":{"count":-1}}"#,
        ] {
            std::fs::write(&path, bad).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert!(
                matches!(
                    store.read::<Sample>("a", 1, None),
                    Err(StoreError::Corrupt(_))
                ),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
        std::fs::write(&path, vec![b' '; MAX_FILE_BYTES as usize + 1]).unwrap();
        assert!(matches!(
            store.read::<Sample>("a", 1, None),
            Err(StoreError::Corrupt(_))
        ));
    }

    #[test]
    fn a_newer_schema_is_refused_and_an_older_one_is_migrated_step_by_step() {
        let (directory, store) = private();
        put(&store, "a", 3, 1);
        let error = store.read::<Sample>("a", 2, None).unwrap_err();
        assert!(matches!(
            error,
            StoreError::Unsupported {
                found: 3,
                supported: 2
            }
        ));
        assert!(error.disables_remote_access());

        std::fs::write(
            directory.path().join("b"),
            br#"{"schema":1,"data":{"n":5}}"#,
        )
        .unwrap();
        std::fs::set_permissions(
            directory.path().join("b"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let migrate: Migrate = |version, data| match version {
            1 => Some(serde_json::json!({ "count": data["n"] })),
            _ => None,
        };
        assert_eq!(
            store.read::<Sample>("b", 2, Some(migrate)).unwrap(),
            Some(Sample { count: 5 })
        );
        assert!(store.read::<Sample>("b", 2, None).is_err());
        assert!(store.read::<Sample>("b", 3, Some(migrate)).is_err());
    }

    #[test]
    fn update_is_locked_and_a_leftover_temporary_file_changes_nothing() {
        let (directory, store) = private();
        std::fs::write(directory.path().join(".tmp-dead"), b"half").unwrap();
        for _ in 0..3 {
            store
                .update::<Sample, _>("a", 1, None, |value| value.count += 1)
                .unwrap();
        }
        assert_eq!(
            store.read::<Sample>("a", 1, None).unwrap(),
            Some(Sample { count: 3 })
        );
    }

    #[test]
    fn a_concurrent_reader_sees_only_whole_old_or_new_values() {
        let (_directory, store) = private();
        put(&store, "a", 1, 0);
        let store = std::sync::Arc::new(store);
        let reader = {
            let store = std::sync::Arc::clone(&store);
            std::thread::spawn(move || {
                for _ in 0..300 {
                    let value = store.read::<Sample>("a", 1, None).unwrap().unwrap();
                    assert!(value.count <= 300);
                }
            })
        };
        for count in 1..=300 {
            put(&store, "a", 1, count);
        }
        reader.join().unwrap();
    }

    #[test]
    fn remove_deletes_and_tolerates_a_missing_file() {
        let (_directory, store) = private();
        put(&store, "a", 1, 1);
        let lock = store.lock().unwrap();
        store.remove(&lock, "a").unwrap();
        store.remove(&lock, "a").unwrap();
        drop(lock);
        assert_eq!(store.read::<Sample>("a", 1, None).unwrap(), None);
    }
}
