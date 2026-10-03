//! Trusted devices (Doc 03 §19-23): a client the owner has already authorised with the Remote
//! Access Key. Its credential is a random 256-bit bearer secret kept by the client; the host keeps
//! only a salted hash. A trusted device still needs the password and a TOTP code, and revocation
//! blocks it at the next check.

use blackroom_store::{SecretStore, StoreError};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::secret_hash::{Verifier, base64url, burn, random_bytes, unix_now, well_formed};
use crate::totp::valid_account;

pub const FILE: &str = "trusted-devices";
const SCHEMA: u32 = 1;
pub const MAX_PER_ACCOUNT: usize = 16;
const MAX_LABEL: usize = 40;

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Devices {
    devices: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    device_id: String,
    account: String,
    label: String,
    verifier: Verifier,
    created_unix: u64,
    last_used_unix: u64,
    revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub device_id: String,
    pub label: String,
    pub created_unix: u64,
    pub last_used_unix: u64,
    pub revoked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceCheck {
    Trusted,
    Revoked,
    Refused,
}

pub fn valid_label(label: &str) -> bool {
    (1..=MAX_LABEL).contains(&label.len())
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b" _.-".contains(&byte))
}

fn valid_device_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Returns the device id and its credential; the credential is shown once.
pub fn register(
    store: &SecretStore,
    account: &str,
    label: &str,
) -> Result<(String, Zeroizing<String>), StoreError> {
    if !valid_account(account) || !valid_label(label) {
        return Err(StoreError::Corrupt("invalid account or device label"));
    }
    let device_id = hex::encode(random_bytes::<16>()?);
    let secret = Zeroizing::new(base64url(&random_bytes::<32>()?));
    let verifier = Verifier::new(&secret)?;
    let now = unix_now();
    let added = store.update::<Devices, _>(FILE, SCHEMA, None, |devices| {
        let live = devices
            .devices
            .iter()
            .filter(|entry| entry.account == account && !entry.revoked)
            .count();
        if live >= MAX_PER_ACCOUNT {
            return false;
        }
        devices.devices.push(Entry {
            device_id: device_id.clone(),
            account: account.to_string(),
            label: label.to_string(),
            verifier,
            created_unix: now,
            last_used_unix: 0,
            revoked: false,
        });
        true
    })?;
    if !added {
        return Err(StoreError::Corrupt("too many trusted devices"));
    }
    Ok((device_id, secret))
}

pub fn check(
    store: &SecretStore,
    account: &str,
    device_id: &str,
    secret: &str,
) -> Result<DeviceCheck, StoreError> {
    if !valid_device_id(device_id) || !well_formed(secret) {
        burn("");
        return Ok(DeviceCheck::Refused);
    }
    store.update::<Devices, _>(FILE, SCHEMA, None, |devices| {
        let Some(entry) = devices
            .devices
            .iter_mut()
            .find(|entry| entry.device_id == device_id && entry.account == account)
        else {
            burn(secret);
            return DeviceCheck::Refused;
        };
        let matches = entry.verifier.matches(secret);
        if !matches {
            DeviceCheck::Refused
        } else if entry.revoked {
            DeviceCheck::Revoked
        } else {
            entry.last_used_unix = unix_now();
            DeviceCheck::Trusted
        }
    })
}

pub fn revoke(store: &SecretStore, device_id: &str) -> Result<bool, StoreError> {
    store.update::<Devices, _>(FILE, SCHEMA, None, |devices| {
        devices
            .devices
            .iter_mut()
            .find(|entry| entry.device_id == device_id && !entry.revoked)
            .map(|entry| entry.revoked = true)
            .is_some()
    })
}

pub fn revoke_all(store: &SecretStore, account: &str) -> Result<usize, StoreError> {
    store.update::<Devices, _>(FILE, SCHEMA, None, |devices| {
        devices
            .devices
            .iter_mut()
            .filter(|entry| entry.account == account && !entry.revoked)
            .map(|entry| entry.revoked = true)
            .count()
    })
}

pub fn list(store: &SecretStore, account: &str) -> Result<Vec<DeviceInfo>, StoreError> {
    Ok(store
        .read::<Devices>(FILE, SCHEMA, None)?
        .unwrap_or_default()
        .devices
        .into_iter()
        .filter(|entry| entry.account == account)
        .map(|entry| DeviceInfo {
            device_id: entry.device_id,
            label: entry.label,
            created_unix: entry.created_unix,
            last_used_unix: entry.last_used_unix,
            revoked: entry.revoked,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access_key::tests::store;

    #[test]
    fn a_registered_device_checks_and_records_use() {
        let (_directory, store) = store();
        let (id, secret) = register(&store, "owner", "Laptop A").unwrap();
        assert_eq!(
            check(&store, "owner", &id, &secret).unwrap(),
            DeviceCheck::Trusted
        );
        assert_eq!(
            check(&store, "owner", &id, "wrong").unwrap(),
            DeviceCheck::Refused
        );
        assert_eq!(
            check(&store, "other", &id, &secret).unwrap(),
            DeviceCheck::Refused
        );
        let listed = list(&store, "owner").unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].last_used_unix > 0 && !listed[0].revoked);
    }

    #[test]
    fn a_revoked_device_is_blocked_and_others_keep_working() {
        let (_directory, store) = store();
        let (a, a_secret) = register(&store, "owner", "A").unwrap();
        let (b, b_secret) = register(&store, "owner", "B").unwrap();
        assert!(revoke(&store, &a).unwrap());
        assert!(!revoke(&store, &a).unwrap());
        assert_eq!(
            check(&store, "owner", &a, &a_secret).unwrap(),
            DeviceCheck::Revoked
        );
        assert_eq!(
            check(&store, "owner", &b, &b_secret).unwrap(),
            DeviceCheck::Trusted
        );
        assert_eq!(revoke_all(&store, "owner").unwrap(), 1);
        assert_eq!(
            check(&store, "owner", &b, &b_secret).unwrap(),
            DeviceCheck::Revoked
        );
    }

    #[test]
    fn labels_ids_and_the_device_cap_are_bounded() {
        let (_directory, store) = store();
        assert!(register(&store, "owner", "").is_err());
        assert!(register(&store, "owner", "bad\nlabel").is_err());
        assert!(register(&store, "owner", &"x".repeat(41)).is_err());
        assert_eq!(
            check(&store, "owner", "../x", "s").unwrap(),
            DeviceCheck::Refused
        );
        for index in 0..MAX_PER_ACCOUNT {
            register(&store, "owner", &format!("d{index}")).unwrap();
        }
        assert!(register(&store, "owner", "one too many").is_err());
    }

    #[test]
    fn the_file_holds_no_credential() {
        let (directory, store) = store();
        let (_, secret) = register(&store, "owner", "A").unwrap();
        let file = std::fs::read_to_string(directory.path().join(FILE)).unwrap();
        assert!(!file.contains(secret.as_str()));
    }
}
