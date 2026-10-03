#![forbid(unsafe_code)]
//! One-shot Linux password check for `remote-hostd` (Doc 03 §8).
//!
//! Protocol: `pam-auth-helper --service <name>` reads `account\npassword\n` from standard input
//! and answers only through its exit status: 0 accepted, 1 rejected, 2 could not decide (service,
//! configuration or input problem). It prints nothing, never takes the password from an argument or
//! environment variable, runs no PAM session and keeps nothing after it exits.
//!
//! It runs as the same user as hostd; `pam_unix` then verifies that user's own password through
//! `unix_chkpwd`, so no privilege is needed and none is requested.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::process::ExitCode;

use nonstick::{
    AuthnFlags, ConversationAdapter, ErrorCode, Result as PamResult, Transaction,
    TransactionBuilder,
};
use zeroize::Zeroizing;

const ACCEPTED: u8 = 0;
const REJECTED: u8 = 1;
const UNDECIDED: u8 = 2;
const MAX_INPUT: u64 = 512;

fn service_ok(service: &str) -> bool {
    !service.is_empty()
        && service.len() <= 40
        && service
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn account_ok(account: &str) -> bool {
    (1..=32).contains(&account.len())
        && !account.starts_with(['.', '-'])
        && account.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
        })
}

struct Answers {
    account: String,
    password: Zeroizing<String>,
}

impl ConversationAdapter for Answers {
    fn prompt(&self, _request: impl AsRef<OsStr>) -> PamResult<OsString> {
        Ok(OsString::from(&self.account))
    }

    fn masked_prompt(&self, _request: impl AsRef<OsStr>) -> PamResult<OsString> {
        Ok(OsString::from(self.password.as_str()))
    }

    // Module messages could carry account details; they are dropped, never printed.
    fn error_msg(&self, _message: impl AsRef<OsStr>) {}

    fn info_msg(&self, _message: impl AsRef<OsStr>) {}
}

fn read_request() -> Option<(String, Zeroizing<String>)> {
    let mut bytes = Zeroizing::new(Vec::new());
    std::io::stdin()
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_INPUT {
        return None;
    }
    let text = Zeroizing::new(String::from_utf8(bytes.to_vec()).ok()?);
    let mut lines = text.split('\n');
    let account = lines.next()?.to_string();
    let password = Zeroizing::new(lines.next()?.to_string());
    // Exactly "account\npassword\n": nothing may follow.
    (lines.next() == Some("") && lines.next().is_none() && !password.is_empty())
        .then_some((account, password))
}

fn decide(service: &str) -> u8 {
    let Some((account, password)) = read_request() else {
        return UNDECIDED;
    };
    if !account_ok(&account) || password.contains('\0') {
        return REJECTED;
    }
    let conversation = Answers {
        account: account.clone(),
        password,
    };
    let mut transaction = match TransactionBuilder::new_with_service(service)
        .username(&account)
        .build(conversation.into_conversation())
    {
        Ok(transaction) => transaction,
        Err(_) => return UNDECIDED,
    };
    // No account_management: pam_unix's account stage runs a helper that does setuid() and fails
    // for a non-root caller ("setuid failed: Operation not permitted"), refusing every correct
    // password. A locked account already fails authenticate; password-expiry checks need root.
    match transaction.authenticate(AuthnFlags::DISALLOW_NULL_AUTHTOK) {
        Ok(()) => ACCEPTED,
        Err(
            ErrorCode::AuthenticationError
            | ErrorCode::UserUnknown
            | ErrorCode::MaxTries
            | ErrorCode::PermissionDenied
            | ErrorCode::AccountExpired
            | ErrorCode::NewAuthTokRequired
            | ErrorCode::CredentialsExpired,
        ) => REJECTED,
        Err(_) => UNDECIDED,
    }
}

fn main() -> ExitCode {
    // No core file may ever hold the password.
    let _ = rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [flag, service] = args.as_slice() else {
        return ExitCode::from(UNDECIDED);
    };
    if flag != "--service" || !service_ok(service) {
        return ExitCode::from(UNDECIDED);
    }
    ExitCode::from(decide(service))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_names_and_accounts_are_validated() {
        assert!(service_ok("blackroom-console"));
        for bad in ["", "Upper", "a b", "../x", "/etc/pam.d/su", &"a".repeat(41)] {
            assert!(!service_ok(bad), "{bad}");
        }
        assert!(account_ok("owner"));
        for bad in ["", "..", "-x", "a b", "A", "a/b", &"a".repeat(33)] {
            assert!(!account_ok(bad), "{bad}");
        }
    }
}
