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

use std::ffi::{CStr, CString};
use std::io::Read;
use std::process::ExitCode;

use pam_client::{Context, ConversationHandler, ErrorCode, Flag};
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
    account: CString,
    password: Zeroizing<Vec<u8>>,
}

impl ConversationHandler for Answers {
    fn prompt_echo_on(&mut self, _prompt: &CStr) -> Result<CString, ErrorCode> {
        Ok(self.account.clone())
    }

    fn prompt_echo_off(&mut self, _prompt: &CStr) -> Result<CString, ErrorCode> {
        CString::new(self.password.to_vec()).map_err(|_| ErrorCode::CONV_ERR)
    }

    // Module messages could carry account details; they are dropped, never printed.
    fn text_info(&mut self, _msg: &CStr) {}

    fn error_msg(&mut self, _msg: &CStr) {}
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
    let Ok(account_c) = CString::new(account.as_str()) else {
        return REJECTED;
    };
    let conversation = Answers {
        account: account_c,
        password: Zeroizing::new(password.as_bytes().to_vec()),
    };
    let mut context = match Context::new(service, Some(&account), conversation) {
        Ok(context) => context,
        Err(_) => return UNDECIDED,
    };
    let verdict = |result: Result<(), pam_client::Error>| match result {
        Ok(()) => None,
        Err(error) => Some(match error.code() {
            ErrorCode::AUTH_ERR
            | ErrorCode::USER_UNKNOWN
            | ErrorCode::MAXTRIES
            | ErrorCode::PERM_DENIED
            | ErrorCode::ACCT_EXPIRED
            | ErrorCode::NEW_AUTHTOK_REQD
            | ErrorCode::CRED_EXPIRED => REJECTED,
            _ => UNDECIDED,
        }),
    };
    if let Some(code) = verdict(context.authenticate(Flag::DISALLOW_NULL_AUTHTOK)) {
        return code;
    }
    // No acct_mgmt: pam_unix's account stage runs a helper that does setuid() and fails for a
    // non-root caller ("setuid failed: Operation not permitted"), refusing every correct password.
    // A locked account already fails authenticate; password-expiry checks need root.
    ACCEPTED
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
