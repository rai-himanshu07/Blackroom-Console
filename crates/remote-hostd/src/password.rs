//! Linux password check through the PAM helper process (Doc 03 §6-8).
//!
//! The password is written to the helper's standard input and nowhere else: never an argument, an
//! environment variable, a file or a log line. The helper is a separate executable with a fixed
//! command line, so a remote client can never choose the PAM service or any PAM option.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::totp::valid_account;

pub const MAX_PASSWORD_BYTES: usize = 256;
/// The PAM service name installed in `/etc/pam.d/` by `docs/ops/install-security.sh`.
pub const PAM_SERVICE: &str = "blackroom-console";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordOutcome {
    Accepted,
    Rejected,
    /// The helper could not run or give an answer; not a verdict on the password.
    Unavailable,
}

pub trait PasswordCheck: Send {
    fn check(&mut self, account: &str, password: &str) -> PasswordOutcome;
}

/// A password the helper protocol can carry: one line, printable or space, bounded.
pub fn password_ok(password: &str) -> bool {
    (1..=MAX_PASSWORD_BYTES).contains(&password.len())
        && !password
            .bytes()
            .any(|byte| matches!(byte, b'\n' | b'\r' | 0))
}

pub struct PamHelper {
    helper: PathBuf,
    service: String,
    timeout: Duration,
}

impl PamHelper {
    pub fn new(helper: PathBuf) -> Self {
        Self {
            helper,
            service: PAM_SERVICE.to_string(),
            timeout: Duration::from_secs(15),
        }
    }

    /// Tests only: a service file or another service name.
    pub fn with_service(mut self, service: &str) -> Self {
        self.service = service.to_string();
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl PasswordCheck for PamHelper {
    fn check(&mut self, account: &str, password: &str) -> PasswordOutcome {
        if !valid_account(account) || !password_ok(password) {
            return PasswordOutcome::Rejected;
        }
        let Ok(mut child) = Command::new(&self.helper)
            .arg("--service")
            .arg(&self.service)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return PasswordOutcome::Unavailable;
        };
        let request = Zeroizing::new(format!("{account}\n{password}\n"));
        let written = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(request.as_bytes()).is_ok());
        if !written {
            let _ = child.kill();
            let _ = child.wait();
            return PasswordOutcome::Unavailable;
        }
        let deadline = Instant::now() + self.timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return match status.code() {
                        Some(0) => PasswordOutcome::Accepted,
                        Some(1) => PasswordOutcome::Rejected,
                        _ => PasswordOutcome::Unavailable,
                    };
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return PasswordOutcome::Unavailable;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    // A fork in one test can hold another test's freshly written script open, making exec fail
    // with "text file busy"; scripts are therefore written and run one test at a time.
    static SCRIPTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn script(directory: &std::path::Path, body: &str) -> PathBuf {
        let path = directory.join("helper.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    fn exit_codes_map_to_verdicts_and_the_password_arrives_on_stdin_only() {
        let _serial = SCRIPTS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = tempfile::tempdir().unwrap();
        let helper = script(
            directory.path(),
            r#"read user; read pass
case "$*" in *"$pass"*) exit 9;; esac
[ "$user" = "owner" ] && [ "$pass" = "right" ] && exit 0
[ "$user" = "broken" ] && exit 7
exit 1"#,
        );
        let mut check = PamHelper::new(helper);
        assert_eq!(check.check("owner", "right"), PasswordOutcome::Accepted);
        assert_eq!(check.check("owner", "wrong"), PasswordOutcome::Rejected);
        assert_eq!(check.check("broken", "x"), PasswordOutcome::Unavailable);
    }

    #[test]
    fn malformed_input_is_rejected_without_running_anything() {
        let mut check = PamHelper::new(PathBuf::from("/nonexistent/helper"));
        assert_eq!(
            check.check("owner", "line\nbreak"),
            PasswordOutcome::Rejected
        );
        assert_eq!(check.check("../x", "pw"), PasswordOutcome::Rejected);
        assert_eq!(check.check("owner", ""), PasswordOutcome::Rejected);
        assert_eq!(
            check.check("owner", &"a".repeat(257)),
            PasswordOutcome::Rejected
        );
        assert_eq!(check.check("owner", "pw"), PasswordOutcome::Unavailable);
    }

    #[test]
    fn a_hung_helper_is_killed_at_the_timeout() {
        let _serial = SCRIPTS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = tempfile::tempdir().unwrap();
        let helper = script(directory.path(), "sleep 30");
        let mut check = PamHelper::new(helper).with_timeout(Duration::from_millis(200));
        let started = Instant::now();
        assert_eq!(check.check("owner", "pw"), PasswordOutcome::Unavailable);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
