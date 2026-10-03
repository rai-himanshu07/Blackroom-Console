//! The remote-access switch (Doc 03 §46): one durable flag the owner sets with `blackroom disable`.
//! Every unreadable, corrupt or unexpected state counts as disabled, so a damaged store can only
//! ever close remote access, never open it.

use blackroom_store::{SecretStore, StoreError};
use serde::{Deserialize, Serialize};

use crate::secret_hash::unix_now;

pub const FILE: &str = "remote-access";
const SCHEMA: u32 = 1;
const MAX_REASON: usize = 40;

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Switch {
    disabled: bool,
    since_unix: u64,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disabled {
    pub since_unix: u64,
    pub reason: String,
}

fn clean_reason(reason: &str) -> String {
    reason
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'))
        .take(MAX_REASON)
        .collect()
}

/// `Ok(None)` means enabled; an error means the state cannot be trusted (treat as disabled).
pub fn state(store: &SecretStore) -> Result<Option<Disabled>, StoreError> {
    Ok(store
        .read::<Switch>(FILE, SCHEMA, None)?
        .filter(|switch| switch.disabled)
        .map(|switch| Disabled {
            since_unix: switch.since_unix,
            reason: switch.reason,
        }))
}

pub fn is_disabled(store: &SecretStore) -> bool {
    !matches!(state(store), Ok(None))
}

pub fn set_disabled(store: &SecretStore, reason: &str) -> Result<(), StoreError> {
    let reason = clean_reason(reason);
    store.update::<Switch, _>(FILE, SCHEMA, None, |switch| {
        *switch = Switch {
            disabled: true,
            since_unix: unix_now(),
            reason,
        };
    })
}

pub fn set_enabled(store: &SecretStore) -> Result<(), StoreError> {
    store.update::<Switch, _>(FILE, SCHEMA, None, |switch| *switch = Switch::default())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::access_key::tests::store;

    #[test]
    fn enabled_by_default_and_toggled_durably() {
        let (_directory, store) = store();
        assert!(!is_disabled(&store));
        set_disabled(&store, "travelling\nwith injected line").unwrap();
        assert!(is_disabled(&store));
        let disabled = state(&store).unwrap().unwrap();
        assert!(!disabled.reason.contains('\n'));
        set_enabled(&store).unwrap();
        assert!(!is_disabled(&store));
    }

    #[test]
    fn a_damaged_file_counts_as_disabled() {
        let (directory, store) = store();
        let path = directory.path().join(FILE);
        std::fs::write(&path, b"garbage").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(is_disabled(&store));
        assert!(state(&store).is_err());
    }
}
