//! Remote Access Key (Doc 03 §15-18): the secret a new or untrusted client must present. It is
//! generated here (256 random bits, base64url), shown once, stored only as a salted hash, and a
//! rotation invalidates the previous key at once because every check re-reads the file.

use blackroom_store::{SecretStore, StoreError};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::secret_hash::{Verifier, base64url, burn, random_bytes, unix_now, well_formed};
use crate::totp::valid_account;

pub const FILE: &str = "access-keys";
const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Keys {
    keys: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    account: String,
    verifier: Verifier,
    created_unix: u64,
}

/// Replaces the account's key and returns the new one exactly once.
pub fn rotate(store: &SecretStore, account: &str) -> Result<Zeroizing<String>, StoreError> {
    if !valid_account(account) {
        return Err(StoreError::Corrupt("invalid account name"));
    }
    let key = Zeroizing::new(base64url(&random_bytes::<32>()?));
    let verifier = Verifier::new(&key)?;
    store.update::<Keys, _>(FILE, SCHEMA, None, |keys| {
        keys.keys.retain(|entry| entry.account != account);
        keys.keys.push(Entry {
            account: account.to_string(),
            verifier,
            created_unix: unix_now(),
        });
    })?;
    Ok(key)
}

pub fn verify(store: &SecretStore, account: &str, presented: &str) -> Result<bool, StoreError> {
    let keys = store.read::<Keys>(FILE, SCHEMA, None)?.unwrap_or_default();
    let entry = keys.keys.iter().find(|entry| entry.account == account);
    if !well_formed(presented) {
        burn("");
        return Ok(false);
    }
    Ok(match entry {
        Some(entry) => entry.verifier.matches(presented),
        None => {
            burn(presented);
            false
        }
    })
}

pub fn configured(store: &SecretStore, account: &str) -> Result<bool, StoreError> {
    Ok(store
        .read::<Keys>(FILE, SCHEMA, None)?
        .is_some_and(|keys| keys.keys.iter().any(|entry| entry.account == account)))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs::File;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    pub(crate) fn store() -> (tempfile::TempDir, SecretStore) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let store = SecretStore::open(&File::open(directory.path()).unwrap()).unwrap();
        (directory, store)
    }

    #[test]
    fn a_new_key_is_256_bits_verifies_and_is_stored_hashed() {
        let (directory, store) = store();
        assert!(!configured(&store, "owner").unwrap());
        let key = rotate(&store, "owner").unwrap();
        assert_eq!(key.len(), 43);
        assert!(configured(&store, "owner").unwrap());
        assert!(verify(&store, "owner", &key).unwrap());
        assert!(!verify(&store, "owner", "wrong").unwrap());
        assert!(!verify(&store, "nobody", &key).unwrap());
        let file = std::fs::read_to_string(directory.path().join(FILE)).unwrap();
        assert!(!file.contains(key.as_str()));
    }

    #[test]
    fn rotation_invalidates_the_old_key_immediately() {
        let (_directory, store) = store();
        let old = rotate(&store, "owner").unwrap();
        let new = rotate(&store, "owner").unwrap();
        assert_ne!(*old, *new);
        assert!(!verify(&store, "owner", &old).unwrap());
        assert!(verify(&store, "owner", &new).unwrap());
    }

    #[test]
    fn an_invalid_account_name_is_refused() {
        let (_directory, store) = store();
        assert!(rotate(&store, "../x").is_err());
    }
}
