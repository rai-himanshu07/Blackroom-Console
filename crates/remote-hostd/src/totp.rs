//! Time-based one-time password credential for the `CredentialVerifier` seam (RFC 6238 over
//! RFC 4226, HMAC-SHA1, 6 digits, 30 s steps, one step of clock skew), with replay protection,
//! per-(account, client) and per-account failure limits, and an owner-only secret file.
//!
//! This is a verifier adapter only: nothing in the offline simulation service uses it yet, and no
//! secret is created unless [`enroll`] is called explicitly. SHA-1 and the HMAC are implemented here
//! (about 60 lines) to keep the host free of extra crypto dependencies; they are checked against the
//! RFC test vectors. TOTP does not need collision resistance.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::time::{SystemTime, UNIX_EPOCH};

use blackroom_core::error::{BlackroomError, ErrorCode};
use rustix::fs::{AtFlags, Mode, OFlags};
use serde::{Deserialize, Serialize};

use crate::auth::{CredentialVerifier, Principal};

const STEP_SECS: u64 = 30;
const DIGITS: u32 = 6;
const SECRET_BYTES: usize = 20;
const STORE_FILE: &str = "totp-credentials";
const MAX_STORE_BYTES: usize = 64 * 1024;
const MAX_ACCOUNTS: usize = 64;

fn sha1(message: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut data = message.to_vec();
    let bit_length = (message.len() as u64).wrapping_mul(8);
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_length.to_be_bytes());
    for chunk in data.chunks_exact(64) {
        let mut w = [0_u32; 80];
        for (word, bytes) in w.iter_mut().zip(chunk.chunks_exact(4)) {
            *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e]) {
            *state = state.wrapping_add(value);
        }
    }
    let mut out = [0_u8; 20];
    for (bytes, word) in out.chunks_exact_mut(4).zip(h) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let mut block = [0_u8; 64];
    if key.len() > 64 {
        block[..20].copy_from_slice(&sha1(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner: Vec<u8> = block.iter().map(|byte| byte ^ 0x36).collect();
    inner.extend_from_slice(message);
    let mut outer: Vec<u8> = block.iter().map(|byte| byte ^ 0x5c).collect();
    outer.extend_from_slice(&sha1(&inner));
    sha1(&outer)
}

/// RFC 4226 HOTP with 6 digits.
pub fn hotp(secret: &[u8], counter: u64) -> String {
    let digest = hmac_sha1(secret, &counter.to_be_bytes());
    let offset = usize::from(digest[19] & 0x0f);
    let value = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    format!(
        "{:0width$}",
        value % 10_u32.pow(DIGITS),
        width = DIGITS as usize
    )
}

/// RFC 6238 code for the 30-second step containing `unix_secs`.
pub fn totp(secret: &[u8], unix_secs: u64) -> String {
    hotp(secret, unix_secs / STEP_SECS)
}

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(BASE32[((buffer >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(BASE32[((buffer << (5 - bits)) & 31) as usize]));
    }
    out
}

/// Strict RFC 4648 base32 without padding; `None` for any other character or a dangling bit.
pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for character in text.bytes() {
        let value = BASE32
            .iter()
            .position(|symbol| *symbol == character.to_ascii_uppercase())?;
        buffer = (buffer << 5) | u32::try_from(value).ok()?;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buffer >> bits) & 0xff).ok()?);
        }
    }
    (bits < 5 || buffer & ((1 << bits) - 1) == 0)
        .then_some(out)
        .filter(|bytes| !bytes.is_empty())
}

/// Accounts are short lowercase names: no path, URI or log-injection surprises.
pub fn valid_account(account: &str) -> bool {
    (1..=32).contains(&account.len())
        && account.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
        })
}

pub fn otpauth_uri(account: &str, secret_base32: &str) -> String {
    format!(
        "otpauth://totp/Blackroom%20Console:{account}?secret={secret_base32}&issuer=Blackroom%20Console&algorithm=SHA1&digits=6&period=30"
    )
}

/// Constant-time equality for equal-length ASCII digit strings.
fn same_code(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0_u8, |difference, (x, y)| difference | (x ^ y))
            == 0
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Failures per (account, client) before any lockout.
    pub free_failures: u32,
    pub base_lock_secs: u64,
    pub max_lock_secs: u64,
    /// Failures across all clients within `window_secs` that lock the whole account.
    pub account_failures: u32,
    pub window_secs: u64,
    pub account_lock_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            free_failures: 3,
            base_lock_secs: 30,
            max_lock_secs: 900,
            account_failures: 10,
            window_secs: 600,
            account_lock_secs: 900,
        }
    }
}

#[derive(Default)]
struct Attempts {
    failures: u32,
    locked_until: u64,
}

#[derive(Default)]
struct AccountAttempts {
    recent: Vec<u64>,
    locked_until: u64,
}

struct Account {
    secret: Vec<u8>,
    last_step: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAccount {
    account: String,
    secret: String,
    last_step: u64,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct StoredFile {
    accounts: Vec<StoredAccount>,
}

pub struct TotpCredential {
    pub account: String,
    pub client_id: String,
    pub code: String,
}

type Clock = Box<dyn Fn() -> u64 + Send>;
type Persist = Box<dyn FnMut(&str, u64) -> io::Result<()> + Send>;

pub struct TotpVerifier {
    accounts: HashMap<String, Account>,
    per_client: HashMap<(String, String), Attempts>,
    per_account: HashMap<String, AccountAttempts>,
    limits: Limits,
    now_secs: Clock,
    /// Called with `(account, step)` after each accepted code so replay protection survives a
    /// restart; a failure refuses the login.
    persist: Option<Persist>,
}

fn refuse(code: ErrorCode, message: &'static str) -> BlackroomError {
    BlackroomError::new(code, message)
}

fn system_clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

impl TotpVerifier {
    pub fn new(limits: Limits) -> Self {
        Self {
            accounts: HashMap::new(),
            per_client: HashMap::new(),
            per_account: HashMap::new(),
            limits,
            now_secs: Box::new(system_clock),
            persist: None,
        }
    }

    pub fn with_clock(mut self, clock: impl Fn() -> u64 + Send + 'static) -> Self {
        self.now_secs = Box::new(clock);
        self
    }

    pub fn add_account(&mut self, account: &str, secret: Vec<u8>, last_step: u64) -> bool {
        if !valid_account(account) || secret.len() < 10 || self.accounts.len() >= MAX_ACCOUNTS {
            return false;
        }
        self.accounts
            .insert(account.to_string(), Account { secret, last_step });
        true
    }

    pub fn is_locked(&self, account: &str, client_id: &str) -> bool {
        let now = (self.now_secs)();
        self.per_client
            .get(&(account.to_string(), client_id.to_string()))
            .is_some_and(|attempts| attempts.locked_until > now)
            || self
                .per_account
                .get(account)
                .is_some_and(|attempts| attempts.locked_until > now)
    }

    fn record_failure(&mut self, account: &str, client_id: &str, now: u64) {
        let limits = self.limits;
        let attempts = self
            .per_client
            .entry((account.to_string(), client_id.to_string()))
            .or_default();
        attempts.failures = attempts.failures.saturating_add(1);
        if attempts.failures > limits.free_failures {
            let doublings = (attempts.failures - limits.free_failures - 1).min(16);
            let lock = limits
                .base_lock_secs
                .saturating_mul(1_u64 << doublings)
                .min(limits.max_lock_secs);
            attempts.locked_until = now.saturating_add(lock);
        }
        let account_attempts = self.per_account.entry(account.to_string()).or_default();
        account_attempts
            .recent
            .retain(|at| now.saturating_sub(*at) < limits.window_secs);
        account_attempts.recent.push(now);
        if u32::try_from(account_attempts.recent.len()).unwrap_or(u32::MAX)
            >= limits.account_failures
        {
            account_attempts.locked_until = now.saturating_add(limits.account_lock_secs);
        }
        // Bounded memory: drop expired, never-locked entries once the tables grow.
        if self.per_client.len() > 1024 {
            self.per_client
                .retain(|_, attempts| attempts.locked_until > now);
        }
        if self.per_account.len() > 1024 {
            self.per_account.retain(|_, attempts| {
                attempts.locked_until > now
                    || attempts
                        .recent
                        .iter()
                        .any(|at| now.saturating_sub(*at) < limits.window_secs)
            });
        }
    }
}

impl CredentialVerifier for TotpVerifier {
    type Presented = TotpCredential;

    fn verify(&mut self, presented: TotpCredential) -> Result<Principal, BlackroomError> {
        let now = (self.now_secs)();
        let well_formed = valid_account(&presented.account)
            && (1..=64).contains(&presented.client_id.len())
            && presented
                .client_id
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
            && presented.code.len() == DIGITS as usize
            && presented.code.bytes().all(|byte| byte.is_ascii_digit());
        if !well_formed {
            // Malformed input is refused without touching any account's counters, so garbage
            // cannot be used to lock a real account.
            return Err(refuse(ErrorCode::AuthInvalid, "credential refused"));
        }
        if self.is_locked(&presented.account, &presented.client_id) {
            return Err(refuse(
                ErrorCode::AuthRateLimited,
                "too many failed attempts",
            ));
        }
        let current = now / STEP_SECS;
        let mut matched: Option<u64> = None;
        let mut last_step = 0;
        // Unknown accounts take the same path and time as known ones.
        let secret = match self.accounts.get(&presented.account) {
            Some(account) => {
                last_step = account.last_step;
                account.secret.clone()
            }
            None => vec![0_u8; SECRET_BYTES],
        };
        let known = self.accounts.contains_key(&presented.account);
        for step in [
            current.saturating_sub(1),
            current,
            current.saturating_add(1),
        ] {
            let expected = hotp(&secret, step);
            if same_code(&expected, &presented.code) && known && step > last_step {
                matched = Some(step);
            }
        }
        let Some(step) = matched else {
            self.record_failure(&presented.account, &presented.client_id, now);
            return Err(refuse(ErrorCode::AuthInvalid, "credential refused"));
        };
        if let Some(persist) = self.persist.as_mut()
            && persist(&presented.account, step).is_err()
        {
            return Err(refuse(
                ErrorCode::AuthInvalid,
                "credential store unavailable",
            ));
        }
        if let Some(account) = self.accounts.get_mut(&presented.account) {
            account.last_step = step;
        }
        self.per_client
            .remove(&(presented.account.clone(), presented.client_id.clone()));
        Ok(Principal {
            user_id: presented.account,
            client_id: presented.client_id,
        })
    }
}

fn store_dir_ok(directory: &File) -> io::Result<()> {
    let meta = directory.metadata()?;
    if !meta.is_dir()
        || meta.uid() != rustix::process::getuid().as_raw()
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "credential directory must be an owner-only directory we own",
        ));
    }
    Ok(())
}

fn read_store(directory: &File) -> io::Result<StoredFile> {
    store_dir_ok(directory)?;
    let fd = match rustix::fs::openat(
        directory,
        STORE_FILE,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(StoredFile::default()),
        Err(error) => return Err(error.into()),
    };
    let mut file = File::from(fd);
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != rustix::process::getuid().as_raw()
        || meta.mode() & 0o177 != 0
        || meta.len() > MAX_STORE_BYTES as u64
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "credential file must be a private regular file we own",
        ));
    }
    let mut text = Vec::new();
    file.read_to_end(&mut text)?;
    serde_json::from_slice(&text)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "credential file is malformed"))
}

fn write_store(directory: &File, stored: &StoredFile) -> io::Result<()> {
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    let temporary = format!(".totp-{}.tmp", hex::encode(random));
    let fd = rustix::fs::openat(
        directory,
        temporary.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?;
    let mut file = File::from(fd);
    let bytes = serde_json::to_vec(stored).map_err(io::Error::other)?;
    let result = file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            rustix::fs::renameat(directory, temporary.as_str(), directory, STORE_FILE)
                .map_err(io::Error::from)
        });
    if result.is_err() {
        let _ = rustix::fs::unlinkat(directory, temporary.as_str(), AtFlags::empty());
    }
    result?;
    rustix::fs::fsync(directory)?;
    Ok(())
}

/// Creates a new account with a fresh random secret and returns it (base32) exactly once. The
/// secret is never logged here; the caller shows it to the operator.
pub fn enroll(directory: &File, account: &str) -> io::Result<String> {
    if !valid_account(account) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "account must be 1 to 32 characters of a-z, 0-9, '_', '.', '-'",
        ));
    }
    let mut stored = read_store(directory)?;
    if stored.accounts.iter().any(|entry| entry.account == account) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "account is already enrolled",
        ));
    }
    if stored.accounts.len() >= MAX_ACCOUNTS {
        return Err(io::Error::other("too many accounts"));
    }
    let mut secret = [0_u8; SECRET_BYTES];
    getrandom::fill(&mut secret).map_err(|error| io::Error::other(error.to_string()))?;
    let encoded = base32_encode(&secret);
    stored.accounts.push(StoredAccount {
        account: account.to_string(),
        secret: encoded.clone(),
        last_step: 0,
    });
    write_store(directory, &stored)?;
    Ok(encoded)
}

/// Account names in the store, for listing; never the secrets.
pub fn enrolled_accounts(directory: &File) -> io::Result<Vec<String>> {
    Ok(read_store(directory)?
        .accounts
        .into_iter()
        .map(|entry| entry.account)
        .collect())
}

/// Loads every enrolled account into a verifier whose accepted steps are written back, so a used
/// code stays used across a restart.
pub fn load_verifier(directory: &File, limits: Limits) -> io::Result<TotpVerifier> {
    let stored = read_store(directory)?;
    let mut verifier = TotpVerifier::new(limits);
    for entry in &stored.accounts {
        let secret = base32_decode(&entry.secret)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad stored secret"))?;
        if !verifier.add_account(&entry.account, secret, entry.last_step) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bad stored account",
            ));
        }
    }
    let directory = directory.try_clone()?;
    verifier.persist = Some(Box::new(move |account, step| {
        let mut stored = read_store(&directory)?;
        if let Some(entry) = stored
            .accounts
            .iter_mut()
            .find(|entry| entry.account == account)
        {
            entry.last_step = entry.last_step.max(step);
        }
        write_store(&directory, &stored)
    }));
    Ok(verifier)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    const RFC_SECRET: &[u8] = b"12345678901234567890";

    fn hex_of(bytes: [u8; 20]) -> String {
        hex::encode(bytes)
    }

    #[test]
    fn sha1_and_hmac_match_the_published_vectors() {
        assert_eq!(
            hex_of(sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex_of(sha1(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            hex_of(sha1(&[b'a'; 1000])),
            "291e9a6c66994949b57ba5e650361e98fc36b1ba"
        );
        assert_eq!(
            hex_of(hmac_sha1(&[0x0b; 20], b"Hi There")),
            "b617318655057264e28bc0b6fb378c8ef146be00"
        );
        assert_eq!(
            hex_of(hmac_sha1(b"Jefe", b"what do ya want for nothing?")),
            "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79"
        );
        assert_eq!(
            hex_of(hmac_sha1(
                &[0xaa; 80],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "aa4ae5e15272d00e95705637ce8a3b55ed402112"
        );
    }

    #[test]
    fn hotp_and_totp_match_rfc_4226_and_rfc_6238() {
        let expected = [
            "755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583",
            "399871", "520489",
        ];
        for (counter, code) in expected.iter().enumerate() {
            assert_eq!(hotp(RFC_SECRET, counter as u64), *code);
        }
        for (time, code) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (1_111_111_111, "050471"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
            (20_000_000_000, "353130"),
        ] {
            assert_eq!(totp(RFC_SECRET, time), code);
        }
    }

    #[test]
    fn base32_round_trips_and_rejects_garbage() {
        assert_eq!(base32_encode(b"Hello!\xde\xad\xbe\xef"), "JBSWY3DPEHPK3PXP");
        assert_eq!(
            base32_decode("jbswy3dpehpk3pxp"),
            Some(b"Hello!\xde\xad\xbe\xef".to_vec())
        );
        assert_eq!(base32_decode("JBSWY3DP1"), None);
        assert_eq!(base32_decode(""), None);
        assert_eq!(base32_decode("JBSWY3DPEHPK3PX="), None);
        let secret: Vec<u8> = (0..20).collect();
        assert_eq!(base32_decode(&base32_encode(&secret)), Some(secret));
    }

    #[test]
    fn account_names_are_short_and_plain() {
        assert!(valid_account("alice"));
        assert!(valid_account("a.b-c_9"));
        assert!(!valid_account(""));
        assert!(!valid_account("Alice"));
        assert!(!valid_account("a/b"));
        assert!(!valid_account("a b"));
        assert!(!valid_account(&"a".repeat(33)));
    }

    fn verifier(now: Arc<AtomicU64>) -> TotpVerifier {
        let mut verifier =
            TotpVerifier::new(Limits::default()).with_clock(move || now.load(Ordering::SeqCst));
        assert!(verifier.add_account("alice", RFC_SECRET.to_vec(), 0));
        verifier
    }

    fn credential(code: &str) -> TotpCredential {
        TotpCredential {
            account: "alice".to_string(),
            client_id: "laptop".to_string(),
            code: code.to_string(),
        }
    }

    #[test]
    fn a_current_code_is_accepted_once_and_only_once() {
        let now = Arc::new(AtomicU64::new(1_000_000));
        let mut verifier = verifier(Arc::clone(&now));
        let code = totp(RFC_SECRET, 1_000_000);
        let principal = verifier.verify(credential(&code)).expect("accepted");
        assert_eq!(principal.user_id, "alice");
        assert_eq!(principal.client_id, "laptop");
        assert_eq!(
            verifier.verify(credential(&code)).unwrap_err().code,
            ErrorCode::AuthInvalid,
            "a used code is a replay"
        );
        // The next step's code works; an older step's code does not (replay protection).
        now.store(1_000_030, Ordering::SeqCst);
        verifier
            .verify(credential(&totp(RFC_SECRET, 1_000_030)))
            .expect("next step");
        assert!(
            verifier
                .verify(credential(&totp(RFC_SECRET, 1_000_000)))
                .is_err()
        );
    }

    #[test]
    fn one_step_of_clock_skew_is_tolerated_but_no_more() {
        let now = Arc::new(AtomicU64::new(1_000_000));
        let mut verifier = verifier(Arc::clone(&now));
        verifier
            .verify(credential(&totp(RFC_SECRET, 1_000_000 + STEP_SECS)))
            .expect("one step ahead");
        let mut far = self::verifier(Arc::clone(&now));
        assert!(
            far.verify(credential(&totp(RFC_SECRET, 1_000_000 + 2 * STEP_SECS)))
                .is_err()
        );
        assert!(
            far.verify(credential(&totp(RFC_SECRET, 1_000_000 - 2 * STEP_SECS)))
                .is_err()
        );
    }

    #[test]
    fn repeated_failures_lock_the_client_with_a_growing_delay_and_unlock_later() {
        let now = Arc::new(AtomicU64::new(2_000_000));
        let mut verifier = verifier(Arc::clone(&now));
        for _ in 0..3 {
            assert_eq!(
                verifier.verify(credential("000000")).unwrap_err().code,
                ErrorCode::AuthInvalid
            );
        }
        assert!(!verifier.is_locked("alice", "laptop"));
        assert_eq!(
            verifier.verify(credential("000000")).unwrap_err().code,
            ErrorCode::AuthInvalid
        );
        assert!(verifier.is_locked("alice", "laptop"));
        let good = totp(RFC_SECRET, 2_000_000);
        assert_eq!(
            verifier.verify(credential(&good)).unwrap_err().code,
            ErrorCode::AuthRateLimited,
            "even the right code waits while locked"
        );
        now.store(2_000_031, Ordering::SeqCst);
        verifier
            .verify(credential(&totp(RFC_SECRET, 2_000_031)))
            .expect("unlocked");
        assert!(
            !verifier.is_locked("alice", "laptop"),
            "success clears the client's failures"
        );
    }

    #[test]
    fn failures_from_many_clients_lock_the_whole_account() {
        let now = Arc::new(AtomicU64::new(3_000_000));
        let mut verifier = verifier(Arc::clone(&now));
        for client in 0..10 {
            let _ = verifier.verify(TotpCredential {
                account: "alice".to_string(),
                client_id: format!("client-{client}"),
                code: "111111".to_string(),
            });
        }
        assert_eq!(
            verifier
                .verify(TotpCredential {
                    account: "alice".to_string(),
                    client_id: "fresh".to_string(),
                    code: totp(RFC_SECRET, 3_000_000),
                })
                .unwrap_err()
                .code,
            ErrorCode::AuthRateLimited
        );
    }

    #[test]
    fn unknown_accounts_behave_like_wrong_codes_and_malformed_input_cannot_lock_real_accounts() {
        let now = Arc::new(AtomicU64::new(4_000_000));
        let mut verifier = verifier(Arc::clone(&now));
        let unknown = || TotpCredential {
            account: "mallory".to_string(),
            client_id: "x".to_string(),
            code: "123456".to_string(),
        };
        // Same sequence a real account shows: invalid, then rate limited.
        for _ in 0..4 {
            assert_eq!(
                verifier.verify(unknown()).unwrap_err().code,
                ErrorCode::AuthInvalid
            );
        }
        assert_eq!(
            verifier.verify(unknown()).unwrap_err().code,
            ErrorCode::AuthRateLimited
        );
        for bad in [
            "",
            "12345",
            "1234567",
            "12345a",
            "\u{661}\u{662}\u{663}\u{664}\u{665}\u{666}",
        ] {
            assert_eq!(
                verifier.verify(credential(bad)).unwrap_err().code,
                ErrorCode::AuthInvalid
            );
        }
        assert!(!verifier.is_locked("alice", "laptop"));
        verifier
            .verify(credential(&totp(RFC_SECRET, 4_000_000)))
            .expect("the real account is untouched");
    }

    #[test]
    fn enrolled_secrets_live_in_an_owner_only_file_and_used_codes_stay_used_after_a_restart() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let handle = crate::store::open_state_directory(dir.path()).expect("open");
        let secret_text = enroll(&handle, "alice").expect("enroll");
        assert!(base32_decode(&secret_text).is_some_and(|secret| secret.len() == 20));
        assert_eq!(
            enroll(&handle, "alice").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(enroll(&handle, "Bad Name").is_err());
        assert_eq!(
            enrolled_accounts(&handle).unwrap(),
            vec!["alice".to_string()]
        );
        let mode = std::fs::metadata(dir.path().join(STORE_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);

        let secret = base32_decode(&secret_text).unwrap();
        let now = Arc::new(AtomicU64::new(5_000_000));
        let clock = Arc::clone(&now);
        let mut first = load_verifier(&handle, Limits::default())
            .unwrap()
            .with_clock(move || clock.load(Ordering::SeqCst));
        let code = totp(&secret, 5_000_000);
        first.verify(credential(&code)).expect("accepted");
        drop(first);

        let clock = Arc::clone(&now);
        let mut restarted = load_verifier(&handle, Limits::default())
            .unwrap()
            .with_clock(move || clock.load(Ordering::SeqCst));
        assert!(
            restarted.verify(credential(&code)).is_err(),
            "the used code is still used"
        );
    }

    #[test]
    fn a_loose_or_foreign_credential_file_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let handle = crate::store::open_state_directory(dir.path()).expect("open");
        enroll(&handle, "alice").expect("enroll");
        let file = dir.path().join(STORE_FILE);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_verifier(&handle, Limits::default()).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&file, b"{\"accounts\":[],\"extra\":1}").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            load_verifier(&handle, Limits::default()).is_err(),
            "unknown fields are refused"
        );
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o750)).unwrap();
        assert!(
            enroll(&handle, "bob").is_err(),
            "a group-accessible directory is refused"
        );
    }
}
