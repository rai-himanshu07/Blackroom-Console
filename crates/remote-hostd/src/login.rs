//! Full login (Doc 03 §3, §20-21): Linux password through PAM, a TOTP code, and either the
//! Remote Access Key (new client) or a trusted-device credential (known client). No factor is
//! optional and none can stand in for another; a recovery code replaces only the TOTP code.
//!
//! Every refusal before the password is verified looks the same, so the answer reveals nothing about
//! which accounts exist. Specific "TOTP required" / "access key required" answers are given only
//! after the password passed.

use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_store::SecretStore;
use zeroize::Zeroizing;

use crate::auth::{CredentialVerifier, Principal};
use crate::password::{PasswordCheck, PasswordOutcome, password_ok};
use crate::ratelimit::FailureLimiter;
use crate::totp::{TotpCredential, TotpVerifier, valid_account};
use crate::trusted_devices::DeviceCheck;
use crate::{access_key, recovery_codes, remote_switch, trusted_devices};

pub const DISABLED_MESSAGE: &str = "REMOTE ACCESS DISABLED";

pub enum SecondFactor {
    Totp(String),
    Recovery(Zeroizing<String>),
}

pub enum ClientFactor {
    AccessKey(Zeroizing<String>),
    Device {
        device_id: String,
        secret: Zeroizing<String>,
    },
}

/// Each field is optional so that a missing factor is a refusal the verifier can reason about,
/// not a parse error somewhere else.
pub struct LoginAttempt {
    pub account: String,
    /// The connection the attempt came from (for rate limiting); a trusted device's session uses
    /// its device id instead.
    pub client_id: String,
    pub password: Option<Zeroizing<String>>,
    pub second: Option<SecondFactor>,
    pub client: Option<ClientFactor>,
}

pub struct MultiFactorVerifier {
    store: SecretStore,
    totp: TotpVerifier,
    /// `None` when the caller runs the password check itself between [`Self::begin`] and
    /// [`Self::complete`], outside any lock that guards this verifier.
    password: Option<Box<dyn PasswordCheck>>,
    limiter: FailureLimiter,
}

fn error(code: ErrorCode, message: &'static str) -> BlackroomError {
    BlackroomError::new(code, message)
}

fn unavailable() -> BlackroomError {
    error(ErrorCode::HostUnavailable, DISABLED_MESSAGE)
}

impl MultiFactorVerifier {
    pub fn new(
        store: SecretStore,
        totp: TotpVerifier,
        password: Option<Box<dyn PasswordCheck>>,
        limiter: FailureLimiter,
    ) -> Self {
        Self {
            store,
            totp,
            password,
            limiter,
        }
    }

    pub fn store(&self) -> &SecretStore {
        &self.store
    }

    pub fn limiter(&self) -> &FailureLimiter {
        &self.limiter
    }

    fn refuse(&mut self, account: &str, client: &str) -> BlackroomError {
        if self.limiter.record_failure(account, client) {
            error(ErrorCode::AuthInvalid, "credential refused")
        } else {
            error(ErrorCode::AuthRateLimited, "too many failed attempts")
        }
    }
}

impl MultiFactorVerifier {
    /// First phase: everything that must hold before the (slow) password check is attempted.
    pub fn begin(&mut self, attempt: &LoginAttempt) -> Result<(), BlackroomError> {
        if remote_switch::is_disabled(&self.store) {
            return Err(unavailable());
        }
        // Malformed input never touches a counter, so garbage cannot lock a real account.
        if !valid_account(&attempt.account)
            || !(1..=64).contains(&attempt.client_id.len())
            || !attempt
                .client_id
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
        {
            return Err(error(ErrorCode::AuthInvalid, "credential refused"));
        }
        let limited = || error(ErrorCode::AuthRateLimited, "too many failed attempts");
        if self
            .limiter
            .client_locked(&attempt.account, &attempt.client_id)
            || !self.limiter.has_room(&attempt.account, &attempt.client_id)
        {
            return Err(limited());
        }
        // Strangers' guesses can lock the account for everyone; a valid trusted device is not
        // locked out by that (it still needs the password and the code, and its own source limit).
        if self.limiter.account_locked(&attempt.account) && !self.presents_trusted_device(attempt) {
            return Err(limited());
        }
        Ok(())
    }

    fn presents_trusted_device(&self, attempt: &LoginAttempt) -> bool {
        let Some(ClientFactor::Device { device_id, secret }) = &attempt.client else {
            return false;
        };
        trusted_devices::check(&self.store, &attempt.account, device_id, secret)
            .is_ok_and(|check| check == DeviceCheck::Trusted)
    }

    /// Second phase, given the password verdict (`None` when no password was presented).
    pub fn complete(
        &mut self,
        attempt: LoginAttempt,
        password: Option<PasswordOutcome>,
    ) -> Result<Principal, BlackroomError> {
        // The switch may have been thrown while the password was being checked.
        if remote_switch::is_disabled(&self.store) {
            return Err(unavailable());
        }
        let (account, client) = (attempt.account.as_str(), attempt.client_id.as_str());
        match password {
            Some(PasswordOutcome::Accepted) => {}
            Some(PasswordOutcome::Unavailable) => {
                return Err(error(
                    ErrorCode::HostUnavailable,
                    "password check unavailable",
                ));
            }
            Some(PasswordOutcome::Rejected) | None => return Err(self.refuse(account, client)),
        }

        let Some(second) = attempt.second else {
            self.limiter.record_failure(account, client);
            return Err(error(
                ErrorCode::AuthTotpRequired,
                "authenticator code required",
            ));
        };
        let Some(client_factor) = attempt.client else {
            self.limiter.record_failure(account, client);
            return Err(error(
                ErrorCode::AuthAccessKeyRequired,
                "remote access key or trusted device required",
            ));
        };

        let mut session_client = attempt.client_id.clone();
        let client_ok = match &client_factor {
            ClientFactor::AccessKey(key) => access_key::verify(&self.store, account, key),
            ClientFactor::Device { device_id, secret } => {
                trusted_devices::check(&self.store, account, device_id, secret).map(|check| {
                    let trusted = check == DeviceCheck::Trusted;
                    if trusted {
                        session_client = device_id.clone();
                    }
                    trusted
                })
            }
        }
        .map_err(|_| unavailable())?;
        if !client_ok {
            return Err(self.refuse(account, client));
        }

        // The code is spent only after every other factor passed.
        let second_ok = match second {
            SecondFactor::Totp(code) => self
                .totp
                .verify(TotpCredential {
                    account: attempt.account.clone(),
                    client_id: attempt.client_id.clone(),
                    code,
                })
                .is_ok(),
            SecondFactor::Recovery(code) => {
                recovery_codes::redeem(&self.store, account, &code).map_err(|_| unavailable())?
            }
        };
        if !second_ok {
            return Err(self.refuse(account, client));
        }
        self.limiter.record_success(account, client);
        Ok(Principal {
            user_id: attempt.account,
            client_id: session_client,
        })
    }
}

impl CredentialVerifier for MultiFactorVerifier {
    type Presented = LoginAttempt;

    fn verify(&mut self, attempt: LoginAttempt) -> Result<Principal, BlackroomError> {
        self.begin(&attempt)?;
        let verdict = match (&attempt.password, self.password.as_mut()) {
            (Some(password), Some(check)) if password_ok(password) => {
                Some(check.check(&attempt.account, password))
            }
            (Some(_), None) => return Err(unavailable()),
            _ => None,
        };
        self.complete(attempt, verdict)
    }
}
