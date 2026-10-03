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
use blackroom_core::limits::{
    AUTH_RATE_LIMIT_FAILURES, AUTH_RATE_LIMIT_LOCKOUT, AUTH_RATE_LIMIT_WINDOW,
};
use rustix::fs::{AtFlags, FlockOperation, Mode, OFlags};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::auth::{CredentialVerifier, Principal};

const STEP_SECS: u64 = 30;
const DIGITS: u32 = 6;
const SECRET_BYTES: usize = 20;
const STORE_FILE: &str = "totp-credentials";
const MAX_STORE_BYTES: usize = 64 * 1024;
const MAX_ACCOUNTS: usize = 64;
/// Hard cap on limiter entries; unknown account names beyond it are refused, never tracked.
const MAX_TRACKED: usize = 4096;
const LOCK_FILE: &str = "totp-credentials.lock";

fn sha1(message: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut data = Zeroizing::new(message.to_vec());
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
        w.zeroize();
    }
    let mut out = [0_u8; 20];
    for (bytes, word) in out.chunks_exact_mut(4).zip(h) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let mut block = Zeroizing::new([0_u8; 64]);
    if key.len() > 64 {
        let mut digest = Zeroizing::new(sha1(key));
        block[..20].copy_from_slice(&*digest);
        digest.zeroize();
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Zeroizing::new(block.iter().map(|byte| byte ^ 0x36).collect::<Vec<u8>>());
    inner.extend_from_slice(message);
    let mut outer = Zeroizing::new(block.iter().map(|byte| byte ^ 0x5c).collect::<Vec<u8>>());
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
    (bits < 5 && buffer & ((1 << bits) - 1) == 0)
        .then_some(out)
        .filter(|bytes| !bytes.is_empty())
}

/// Accounts are short lowercase names: no path, URI or log-injection surprises.
pub fn valid_account(account: &str) -> bool {
    (1..=32).contains(&account.len())
        && !account.starts_with(['.', '-'])
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
    /// First account-wide lock; each further trigger without a success doubles it up to
    /// `account_max_lock_secs`, so slow guessing spread over clients cannot stay profitable.
    pub account_lock_secs: u64,
    pub account_max_lock_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        // The documented policy (docs/security/architecture.md): 5 failures per 15 minutes lock a
        // (client, account) for 15 minutes; repeated locks double, bounded at 4 h per client and
        // 24 h per account.
        Self {
            free_failures: AUTH_RATE_LIMIT_FAILURES - 1,
            base_lock_secs: AUTH_RATE_LIMIT_LOCKOUT.as_secs(),
            max_lock_secs: 4 * 3600,
            account_failures: 2 * AUTH_RATE_LIMIT_FAILURES,
            window_secs: AUTH_RATE_LIMIT_WINDOW.as_secs(),
            account_lock_secs: AUTH_RATE_LIMIT_LOCKOUT.as_secs(),
            account_max_lock_secs: 24 * 3600,
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
    /// Account locks since the last success, for the doubling delay.
    locks: u32,
}

struct Account {
    secret: Zeroizing<Vec<u8>>,
    last_step: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAccount {
    account: String,
    secret: Zeroizing<String>,
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
        self.accounts.insert(
            account.to_string(),
            Account {
                secret: Zeroizing::new(secret),
                last_step,
            },
        );
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

    fn prune(&mut self, now: u64) {
        let window = self.limits.window_secs;
        let accounts = &self.accounts;
        self.per_client
            .retain(|_, attempts| attempts.locked_until > now);
        self.per_account.retain(|name, attempts| {
            attempts.locked_until > now
                || (attempts.locks > 0 && accounts.contains_key(name))
                || attempts
                    .recent
                    .iter()
                    .any(|at| now.saturating_sub(*at) < window)
        });
    }

    /// Returns `false` when an unknown account's failure could not be tracked because the tables
    /// are full; the caller then refuses as rate limited. Known accounts (at most
    /// `MAX_ACCOUNTS`) are always tracked, and their tables are bounded by the account lockout.
    fn record_failure(&mut self, account: &str, client_id: &str, now: u64, known: bool) -> bool {
        let limits = self.limits;
        let key = (account.to_string(), client_id.to_string());
        let untracked =
            !self.per_client.contains_key(&key) || !self.per_account.contains_key(account);
        if !known
            && untracked
            && (self.per_client.len() >= MAX_TRACKED || self.per_account.len() >= MAX_TRACKED)
        {
            self.prune(now);
            if self.per_client.len() >= MAX_TRACKED || self.per_account.len() >= MAX_TRACKED {
                return false;
            }
        }
        let attempts = self.per_client.entry(key).or_default();
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
            let lock = limits
                .account_lock_secs
                .saturating_mul(1_u64 << account_attempts.locks.min(16))
                .min(limits.account_max_lock_secs);
            account_attempts.locked_until = now.saturating_add(lock);
            account_attempts.locks = account_attempts.locks.saturating_add(1);
            account_attempts.recent.clear();
        }
        if self.per_client.len() > MAX_TRACKED || self.per_account.len() > MAX_TRACKED {
            self.prune(now);
        }
        true
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
        // Unknown accounts take the same path and time as known ones.
        let placeholder = [0_u8; SECRET_BYTES];
        let (secret, last_step, known): (&[u8], u64, bool) =
            match self.accounts.get(&presented.account) {
                Some(account) => (account.secret.as_slice(), account.last_step, true),
                None => (&placeholder, 0, false),
            };
        for step in [
            current.saturating_sub(1),
            current,
            current.saturating_add(1),
        ] {
            let expected = hotp(secret, step);
            if same_code(&expected, &presented.code) && known && step > last_step {
                matched = Some(step);
            }
        }
        let Some(step) = matched else {
            if !self.record_failure(&presented.account, &presented.client_id, now, known) {
                return Err(refuse(
                    ErrorCode::AuthRateLimited,
                    "too many failed attempts",
                ));
            }
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
        if let Some(attempts) = self.per_account.get_mut(&presented.account) {
            attempts.locks = 0;
        }
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

/// Serialises every read-modify-write of the credential file across threads and processes (the
/// CLI and the host), so an enrolment can never overwrite a newer replay step or vice versa.
fn lock_store(directory: &File) -> io::Result<File> {
    store_dir_ok(directory)?;
    let fd = rustix::fs::openat(
        directory,
        LOCK_FILE,
        OFlags::WRONLY | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?;
    let file = File::from(fd);
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != rustix::process::getuid().as_raw()
        || meta.mode() & 0o177 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "credential lock must be a private regular file we own",
        ));
    }
    rustix::fs::flock(&file, FlockOperation::LockExclusive)?;
    Ok(file)
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
    let mut text = Zeroizing::new(Vec::new());
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
    let bytes = Zeroizing::new(serde_json::to_vec(stored).map_err(io::Error::other)?);
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
pub fn enroll(directory: &File, account: &str) -> io::Result<Zeroizing<String>> {
    if !valid_account(account) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "account must be 1 to 32 characters of a-z, 0-9, '_', '.', '-'",
        ));
    }
    let _lock = lock_store(directory)?;
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
    let mut secret = Zeroizing::new([0_u8; SECRET_BYTES]);
    getrandom::fill(&mut *secret).map_err(|error| io::Error::other(error.to_string()))?;
    let encoded = Zeroizing::new(base32_encode(&*secret));
    stored.accounts.push(StoredAccount {
        account: account.to_string(),
        secret: Zeroizing::new((*encoded).clone()),
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
        if verifier.accounts.contains_key(&entry.account) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "duplicate stored account",
            ));
        }
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
        let _lock = lock_store(&directory)?;
        let mut stored = read_store(&directory)?;
        let entry = stored
            .accounts
            .iter_mut()
            .find(|entry| entry.account == account)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "account left the store"))?;
        entry.last_step = entry.last_step.max(step);
        write_store(&directory, &stored)
    }));
    Ok(verifier)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
        assert_eq!(base32_decode("MF"), None, "a non-zero dangling bit");
        assert_eq!(base32_decode("AAA"), None, "an impossible length");
        assert_eq!(base32_decode("AAAAAA"), None, "an impossible length");
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

    /// Short, round numbers so the lock arithmetic is easy to read in the tests.
    fn fast_limits() -> Limits {
        Limits {
            free_failures: 3,
            base_lock_secs: 30,
            max_lock_secs: 900,
            account_failures: 10,
            window_secs: 600,
            account_lock_secs: 900,
            account_max_lock_secs: 86_400,
        }
    }

    #[test]
    fn default_limits_follow_the_documented_policy() {
        let limits = Limits::default();
        assert_eq!(limits.free_failures + 1, 5, "the 5th failure locks");
        assert_eq!(limits.base_lock_secs, 15 * 60);
        assert_eq!(limits.window_secs, 15 * 60);
        assert_eq!(limits.account_lock_secs, 15 * 60);
        assert!(limits.max_lock_secs >= limits.base_lock_secs);
        assert!(limits.account_max_lock_secs >= limits.account_lock_secs);
    }

    fn verifier(now: Arc<AtomicU64>) -> TotpVerifier {
        let mut verifier =
            TotpVerifier::new(fast_limits()).with_clock(move || now.load(Ordering::SeqCst));
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
        assert!(
            verifier
                .verify(credential(&totp(RFC_SECRET, 1_000_000)))
                .is_err(),
            "an earlier step cannot be used after a later one was accepted"
        );
        let mut behind = self::verifier(Arc::clone(&now));
        behind
            .verify(credential(&totp(RFC_SECRET, 1_000_000 - STEP_SECS)))
            .expect("one step behind");
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
        let wrong =
            |verifier: &mut TotpVerifier| verifier.verify(credential("000000")).unwrap_err().code;
        for _ in 0..3 {
            assert_eq!(wrong(&mut verifier), ErrorCode::AuthInvalid);
        }
        assert!(!verifier.is_locked("alice", "laptop"));
        assert_eq!(wrong(&mut verifier), ErrorCode::AuthInvalid);
        assert!(verifier.is_locked("alice", "laptop"), "30 s lock");
        assert_eq!(
            verifier
                .verify(credential(&totp(RFC_SECRET, 2_000_000)))
                .unwrap_err()
                .code,
            ErrorCode::AuthRateLimited,
            "even the right code waits while locked"
        );
        // The next failure after the first lock doubles it: 60 s, not 30 s.
        now.store(2_000_031, Ordering::SeqCst);
        assert!(!verifier.is_locked("alice", "laptop"));
        assert_eq!(wrong(&mut verifier), ErrorCode::AuthInvalid);
        now.store(2_000_062, Ordering::SeqCst);
        assert!(
            verifier.is_locked("alice", "laptop"),
            "still locked 31 s into a 60 s lock"
        );
        now.store(2_000_092, Ordering::SeqCst);
        assert!(!verifier.is_locked("alice", "laptop"));
        verifier
            .verify(credential(&totp(RFC_SECRET, 2_000_092)))
            .expect("unlocked");
        // Success cleared the client's failures: three free failures again, then a 30 s lock.
        for _ in 0..3 {
            assert_eq!(wrong(&mut verifier), ErrorCode::AuthInvalid);
        }
        assert!(!verifier.is_locked("alice", "laptop"));
        assert_eq!(wrong(&mut verifier), ErrorCode::AuthInvalid);
        assert!(verifier.is_locked("alice", "laptop"));
        now.store(2_000_092 + 31, Ordering::SeqCst);
        assert!(
            !verifier.is_locked("alice", "laptop"),
            "30 s again, not 120 s"
        );
    }

    #[test]
    fn account_locks_escalate_until_a_success_and_never_unlock_early() {
        let now = Arc::new(AtomicU64::new(3_500_000));
        let mut verifier = verifier(Arc::clone(&now));
        let burst = |verifier: &mut TotpVerifier, round: u64| {
            for client in 0..10 {
                let _ = verifier.verify(TotpCredential {
                    account: "alice".to_string(),
                    client_id: format!("c{round}-{client}"),
                    code: "111111".to_string(),
                });
            }
        };
        let locked_until = |verifier: &TotpVerifier| verifier.per_account["alice"].locked_until;
        burst(&mut verifier, 0);
        assert_eq!(locked_until(&verifier) - 3_500_000, 900);
        now.store(3_500_901, Ordering::SeqCst);
        burst(&mut verifier, 1);
        assert_eq!(
            locked_until(&verifier) - 3_500_901,
            1800,
            "the second lock doubles"
        );
        now.store(3_500_901 + 1801, Ordering::SeqCst);
        verifier
            .verify(credential(&totp(RFC_SECRET, 3_500_901 + 1801)))
            .expect("a legitimate success after the lock");
        burst(&mut verifier, 2);
        assert_eq!(
            locked_until(&verifier) - (3_500_901 + 1801),
            900,
            "a success resets the escalation"
        );
    }

    #[test]
    fn lock_delays_double_up_to_the_cap() {
        let now = Arc::new(AtomicU64::new(2_500_000));
        let limits = Limits {
            account_failures: 1000,
            ..fast_limits()
        };
        let clock = Arc::clone(&now);
        let mut verifier =
            TotpVerifier::new(limits).with_clock(move || clock.load(Ordering::SeqCst));
        assert!(verifier.add_account("alice", RFC_SECRET.to_vec(), 0));
        for _ in 0..3 {
            let _ = verifier.verify(credential("000000"));
        }
        let mut expected = 30;
        for _ in 0..8 {
            let _ = verifier.verify(credential("000000"));
            let locked_until =
                verifier.per_client[&("alice".to_string(), "laptop".to_string())].locked_until;
            assert_eq!(locked_until - now.load(Ordering::SeqCst), expected);
            now.fetch_add(expected, Ordering::SeqCst);
            expected = (expected * 2).min(900);
        }
        assert_eq!(expected, 900, "the delay reached its cap");
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
        let before = std::fs::read(dir.path().join(STORE_FILE)).unwrap();
        assert_eq!(
            enroll(&handle, "alice").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            std::fs::read(dir.path().join(STORE_FILE)).unwrap(),
            before,
            "a refused enrolment leaves the stored secret untouched"
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

    #[test]
    fn a_failed_persist_refuses_the_login_without_consuming_the_code() {
        let now = Arc::new(AtomicU64::new(6_000_000));
        let mut verifier = verifier(Arc::clone(&now));
        let failing = Arc::new(AtomicBool::new(true));
        let switch = Arc::clone(&failing);
        verifier.persist = Some(Box::new(move |_, _| {
            if switch.load(Ordering::SeqCst) {
                Err(io::Error::other("disk full"))
            } else {
                Ok(())
            }
        }));
        let code = totp(RFC_SECRET, 6_000_000);
        assert_eq!(
            verifier.verify(credential(&code)).unwrap_err().code,
            ErrorCode::AuthInvalid
        );
        failing.store(false, Ordering::SeqCst);
        verifier
            .verify(credential(&code))
            .expect("the code was not consumed");
    }

    #[test]
    fn a_symlinked_or_duplicated_credential_file_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let handle = crate::store::open_state_directory(dir.path()).expect("open");
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, b"{\"accounts\":[]}").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let file = dir.path().join(STORE_FILE);
        std::os::unix::fs::symlink(&target, &file).unwrap();
        assert!(load_verifier(&handle, Limits::default()).is_err());
        assert!(enroll(&handle, "alice").is_err());
        std::fs::remove_file(&file).unwrap();

        let secret = base32_encode(&[7_u8; SECRET_BYTES]);
        let twice = format!(
            "{{\"accounts\":[{{\"account\":\"alice\",\"secret\":\"{secret}\",\"last_step\":0}},\
             {{\"account\":\"alice\",\"secret\":\"{secret}\",\"last_step\":0}}]}}"
        );
        std::fs::write(&file, twice).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            load_verifier(&handle, Limits::default()).is_err(),
            "duplicate accounts are refused"
        );
    }

    #[test]
    fn concurrent_enrolment_and_logins_never_lose_an_update() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let handle = crate::store::open_state_directory(dir.path()).expect("open");
        let secret = base32_decode(&enroll(&handle, "alice").unwrap()).unwrap();
        let now = Arc::new(AtomicU64::new(9_000_000));
        let clock = Arc::clone(&now);
        let mut verifier = load_verifier(&handle, Limits::default())
            .unwrap()
            .with_clock(move || clock.load(Ordering::SeqCst));
        let mut last = String::new();
        std::thread::scope(|scope| {
            for n in 0..8 {
                let handle = &handle;
                scope.spawn(move || enroll(handle, &format!("user{n}")).expect("enrol"));
            }
            for n in 0..40 {
                let at = 9_000_000 + n * STEP_SECS;
                now.store(at, Ordering::SeqCst);
                last = totp(&secret, at);
                verifier.verify(credential(&last)).expect("login");
            }
        });
        assert_eq!(
            enrolled_accounts(&handle).unwrap().len(),
            9,
            "no enrolment was lost"
        );
        let clock = Arc::clone(&now);
        let mut restarted = load_verifier(&handle, Limits::default())
            .unwrap()
            .with_clock(move || clock.load(Ordering::SeqCst));
        assert!(
            restarted.verify(credential(&last)).is_err(),
            "the newest replay step survived the concurrent enrolments"
        );
    }

    #[test]
    fn unknown_account_floods_are_bounded_and_leave_real_accounts_working() {
        let now = Arc::new(AtomicU64::new(8_000_000));
        let mut verifier = verifier(Arc::clone(&now));
        let ghost = |n: usize| TotpCredential {
            account: format!("ghost{n}"),
            client_id: "c".to_string(),
            code: "123456".to_string(),
        };
        for n in 0..MAX_TRACKED * 2 {
            let _ = verifier.verify(ghost(n));
        }
        assert!(verifier.per_account.len() <= MAX_TRACKED + 1);
        assert!(verifier.per_client.len() <= MAX_TRACKED + 1);
        assert_eq!(
            verifier.verify(ghost(usize::MAX)).unwrap_err().code,
            ErrorCode::AuthRateLimited,
            "a new unknown name is refused while the tables are full"
        );
        verifier
            .verify(credential(&totp(RFC_SECRET, 8_000_000)))
            .expect("the real account is unaffected");
    }
}
