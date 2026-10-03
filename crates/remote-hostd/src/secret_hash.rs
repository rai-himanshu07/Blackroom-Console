//! Salted one-way verifiers for random, high-entropy secrets (access keys, recovery codes, device
//! credentials). These secrets are generated here, never chosen by a person, so a salted SHA-256 is
//! enough; the Linux password never reaches this module (it only goes to PAM).

use std::io;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::same_bytes;

const DOMAIN: &[u8] = b"blackroom-secret-v1\0";

pub fn random_bytes<const N: usize>() -> io::Result<[u8; N]> {
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(bytes)
}

fn hash(salt: &[u8], secret: &[u8]) -> [u8; 32] {
    Sha256::new()
        .chain_update(DOMAIN)
        .chain_update(salt)
        .chain_update(secret)
        .finalize()
        .into()
}

/// What is persisted instead of the secret.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Verifier {
    salt: String,
    hash: String,
}

impl Verifier {
    pub fn new(secret: &str) -> io::Result<Self> {
        let salt = random_bytes::<16>()?;
        Ok(Self {
            hash: hex::encode(hash(&salt, secret.as_bytes())),
            salt: hex::encode(salt),
        })
    }

    pub fn matches(&self, secret: &str) -> bool {
        let Ok(salt) = hex::decode(&self.salt) else {
            return false;
        };
        let Ok(expected) = hex::decode(&self.hash) else {
            return false;
        };
        same_bytes(&expected, &hash(&salt, secret.as_bytes()))
    }
}

/// Spends the same work as a real check, for an account or device that does not exist.
pub fn burn(secret: &str) {
    let _ = hash(&[0_u8; 16], secret.as_bytes());
}

/// Presented secrets are short printable ASCII; anything else is refused before hashing.
pub fn well_formed(secret: &str) -> bool {
    (1..=128).contains(&secret.len()) && secret.bytes().all(|byte| byte.is_ascii_graphic())
}

const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Unpadded base64url, the encoding Doc 03 names for the Remote Access Key.
pub fn base64url(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |value, (index, byte)| {
                value | (u32::from(*byte) << (16 - 8 * index))
            });
        for index in 0..=chunk.len() {
            text.push(char::from(
                URL_SAFE[((value >> (18 - 6 * index)) & 63) as usize],
            ));
        }
    }
    text
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verifier_matches_only_its_secret_and_never_contains_it() {
        let verifier = Verifier::new("correct-horse").unwrap();
        assert!(verifier.matches("correct-horse"));
        assert!(!verifier.matches("correct-horsf"));
        assert!(!verifier.matches(""));
        assert!(
            !serde_json::to_string(&verifier)
                .unwrap()
                .contains("correct")
        );
        assert_ne!(
            Verifier::new("same").unwrap().hash,
            Verifier::new("same").unwrap().hash,
            "salts differ"
        );
    }

    #[test]
    fn base64url_matches_the_rfc_vectors_without_padding() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        assert_eq!(base64url(&[0_u8; 32]).len(), 43);
    }

    #[test]
    fn malformed_secrets_are_refused() {
        assert!(well_formed("abc-DEF_123"));
        assert!(!well_formed(""));
        assert!(!well_formed("has space"));
        assert!(!well_formed("line\nbreak"));
        assert!(!well_formed(&"a".repeat(129)));
    }
}
