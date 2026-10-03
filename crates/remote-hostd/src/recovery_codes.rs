//! Single-use recovery codes (Doc 03 §14, §22): a stand-in for the TOTP code when the
//! authenticator is lost. A recovery code never replaces the Remote Access Key or a trusted
//! device, and a spent code is marked in the file before the login is accepted.

use blackroom_store::{SecretStore, StoreError};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::secret_hash::{Verifier, burn, random_bytes};
use crate::totp::{base32_encode, valid_account};

pub const FILE: &str = "recovery-codes";
const SCHEMA: u32 = 1;
pub const CODES: usize = 10;
const CODE_CHARS: usize = 10;

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Codes {
    codes: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    account: String,
    verifier: Verifier,
    used: bool,
}

/// Uppercases and drops separators, so `abcde-fghij` and `ABCDEFGHIJ` are the same code.
fn normalise(text: &str) -> Option<String> {
    let code: String = text
        .chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (code.len() == CODE_CHARS
        && code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || (b'2'..=b'7').contains(&byte)))
    .then_some(code)
}

/// Replaces every code of the account with a fresh set and returns them exactly once.
pub fn generate(store: &SecretStore, account: &str) -> Result<Vec<Zeroizing<String>>, StoreError> {
    if !valid_account(account) {
        return Err(StoreError::Corrupt("invalid account name"));
    }
    let mut shown = Vec::with_capacity(CODES);
    let mut entries = Vec::with_capacity(CODES);
    for _ in 0..CODES {
        let code = base32_encode(&random_bytes::<7>()?)[..CODE_CHARS].to_string();
        entries.push(Entry {
            account: account.to_string(),
            verifier: Verifier::new(&code)?,
            used: false,
        });
        shown.push(Zeroizing::new(format!("{}-{}", &code[..5], &code[5..])));
    }
    store.update::<Codes, _>(FILE, SCHEMA, None, |codes| {
        codes.codes.retain(|entry| entry.account != account);
        codes.codes.extend(entries);
    })?;
    Ok(shown)
}

/// True once, for an unused code of this account; the spend is persisted before returning.
pub fn redeem(store: &SecretStore, account: &str, presented: &str) -> Result<bool, StoreError> {
    let Some(code) = normalise(presented) else {
        burn("");
        return Ok(false);
    };
    let accepted = store.update::<Codes, _>(FILE, SCHEMA, None, |codes| {
        let mut accepted = false;
        for entry in codes
            .codes
            .iter_mut()
            .filter(|entry| entry.account == account)
        {
            if !entry.used && entry.verifier.matches(&code) && !accepted {
                entry.used = true;
                accepted = true;
            }
        }
        accepted
    })?;
    if !accepted {
        burn(&code);
    }
    Ok(accepted)
}

pub fn remaining(store: &SecretStore, account: &str) -> Result<usize, StoreError> {
    Ok(store.read::<Codes>(FILE, SCHEMA, None)?.map_or(0, |codes| {
        codes
            .codes
            .iter()
            .filter(|entry| entry.account == account && !entry.used)
            .count()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access_key::tests::store;

    #[test]
    fn ten_codes_each_work_exactly_once() {
        let (_directory, store) = store();
        let codes = generate(&store, "owner").unwrap();
        assert_eq!(codes.len(), CODES);
        assert_eq!(remaining(&store, "owner").unwrap(), CODES);
        assert!(redeem(&store, "owner", &codes[0]).unwrap());
        assert!(!redeem(&store, "owner", &codes[0]).unwrap(), "single use");
        assert_eq!(remaining(&store, "owner").unwrap(), CODES - 1);
        let loose = codes[1].replace('-', "").to_lowercase();
        assert!(redeem(&store, "owner", &loose).unwrap());
    }

    #[test]
    fn a_code_belongs_to_one_account_and_garbage_is_refused() {
        let (_directory, store) = store();
        let codes = generate(&store, "owner").unwrap();
        assert!(!redeem(&store, "other", &codes[0]).unwrap());
        for bad in ["", "short", "AAAAA-AAAAA", "ZZZZZ-ZZZZZ-ZZ", "../etc/x"] {
            assert!(!redeem(&store, "owner", bad).unwrap(), "{bad}");
        }
        assert_eq!(remaining(&store, "owner").unwrap(), CODES);
    }

    #[test]
    fn regenerating_invalidates_the_previous_set() {
        let (_directory, store) = store();
        let old = generate(&store, "owner").unwrap();
        let new = generate(&store, "owner").unwrap();
        assert!(!redeem(&store, "owner", &old[0]).unwrap());
        assert!(redeem(&store, "owner", &new[0]).unwrap());
    }

    #[test]
    fn the_file_holds_no_code() {
        let (directory, store) = store();
        let codes = generate(&store, "owner").unwrap();
        let file = std::fs::read_to_string(directory.path().join(FILE)).unwrap();
        for code in codes {
            assert!(!file.contains(code.replace('-', "").as_str()));
        }
    }
}
