//! Browser login through `remote-hostd` (Phase 11): the console no longer decides who may log in.
//! A login needs the Linux password, an authenticator (or recovery) code and either the Remote
//! Access Key or a trusted-device credential; hostd checks all of it and owns the session. This
//! module only talks to hostd's private `auth.sock` and `admin.sock`.
//!
//! Two things stay local: a session is also tied to the console's emergency counter (a chord ends
//! every browser session at once), and every socket call has a short deadline so a hung hostd
//! cannot stall the web server for long.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use remote_hostd::authd::{ADMIN_SOCKET, AUTH_SOCKET, AdminReply, AuthReply, call_within};
use serde::Deserialize;
use serde_json::json;
use zeroize::Zeroizing;

const CHECK_WAIT: Duration = Duration::from_secs(2);
const LOGIN_WAIT: Duration = Duration::from_secs(30);
const MAX_TRACKED: usize = 32;

/// What the login page posts. Unknown fields are refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginForm {
    pub account: String,
    pub password: Zeroizing<String>,
    /// Six digits for the authenticator, anything else is tried as a recovery code.
    pub code: Zeroizing<String>,
    #[serde(default)]
    pub access_key: Option<Zeroizing<String>>,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub device_secret: Option<Zeroizing<String>>,
    /// With the access key: also remember this browser as a trusted device.
    #[serde(default)]
    pub trust_label: Option<String>,
}

pub enum LoginOutcome {
    Accepted {
        token: Zeroizing<String>,
        /// Shown once, only after `trust_label`.
        device: Option<(String, Zeroizing<String>)>,
    },
    /// The code hostd gave (`AUTH_INVALID`, `AUTH_TOTP_REQUIRED`, ...); never says which factor was wrong.
    Refused(String),
    Locked,
    Unavailable,
}

pub struct HostdAuth {
    runtime: PathBuf,
    /// Token to the emergency count current when it logged in.
    logins: Mutex<HashMap<String, u64>>,
    last_emergency: AtomicU64,
}

pub fn sockets_present(runtime: &Path) -> bool {
    runtime.join(AUTH_SOCKET).exists() && runtime.join(ADMIN_SOCKET).exists()
}

impl HostdAuth {
    pub fn new(runtime: PathBuf, emergency: u64) -> Self {
        Self {
            runtime,
            logins: Mutex::new(HashMap::new()),
            last_emergency: AtomicU64::new(emergency),
        }
    }

    fn logins(&self) -> std::sync::MutexGuard<'_, HashMap<String, u64>> {
        self.logins
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Blocking; call from `spawn_blocking`. `client_id` is the browser's address, so hostd limits
    /// failures per source.
    pub fn login(&self, form: &LoginForm, client_id: &str, emergency: u64) -> LoginOutcome {
        let mut request = json!({
            "command": "login", "account": form.account, "client_id": client_id,
            "password": form.password.as_str(),
        });
        let fields = request.as_object_mut().expect("object");
        let code = form.code.trim();
        if code.len() == 6 && code.bytes().all(|byte| byte.is_ascii_digit()) {
            fields.insert("totp".into(), json!(code));
        } else if !code.is_empty() {
            fields.insert("recovery_code".into(), json!(code));
        }
        if let Some(key) = &form.access_key {
            fields.insert("access_key".into(), json!(key.as_str()));
        }
        if let (Some(id), Some(secret)) = (&form.device_id, &form.device_secret) {
            fields.insert("device_id".into(), json!(id));
            fields.insert("device_secret".into(), json!(secret.as_str()));
        }
        if let Some(label) = &form.trust_label {
            fields.insert("trust_label".into(), json!(label));
        }
        let reply: AuthReply =
            match call_within(&self.runtime.join(AUTH_SOCKET), &request, LOGIN_WAIT) {
                Ok(reply) => reply,
                Err(_) => return LoginOutcome::Unavailable,
            };
        if !reply.ok {
            return match reply.code.as_deref() {
                Some("AUTH_RATE_LIMITED") => LoginOutcome::Locked,
                Some("HOST_UNAVAILABLE") => LoginOutcome::Unavailable,
                Some(code) => LoginOutcome::Refused(code.to_string()),
                None => LoginOutcome::Refused("AUTH_INVALID".into()),
            };
        }
        let Some(token) = reply.token else {
            return LoginOutcome::Unavailable;
        };
        let mut logins = self.logins();
        if logins.len() >= MAX_TRACKED {
            logins.clear();
        }
        logins.insert(token.to_string(), emergency);
        drop(logins);
        LoginOutcome::Accepted {
            token,
            device: reply.device_id.zip(reply.device_secret),
        }
    }

    /// True for a session hostd still accepts that was opened under the current emergency count.
    /// Blocking, with a short deadline: a local socket answers in well under a millisecond.
    pub fn check(&self, token: &str, emergency: u64) -> bool {
        if self.last_emergency.swap(emergency, Ordering::SeqCst) != emergency {
            self.revoke_all();
        }
        let seen = self.logins().get(token).copied();
        match seen {
            Some(seen) if seen == emergency => {}
            Some(_) => {
                self.logins().remove(token);
                return false;
            }
            None => return false,
        }
        call_within::<_, AuthReply>(
            &self.runtime.join(AUTH_SOCKET),
            &json!({"command": "check", "token": token}),
            CHECK_WAIT,
        )
        .is_ok_and(|reply| reply.ok)
    }

    pub fn logout(&self, token: &str) {
        self.logins().remove(token);
        let _ = call_within::<_, AuthReply>(
            &self.runtime.join(AUTH_SOCKET),
            &json!({"command": "logout", "token": token}),
            CHECK_WAIT,
        );
    }

    /// An emergency stop ends every login at the authority too, not only in this process.
    fn revoke_all(&self) {
        self.logins().clear();
        let _ = call_within::<_, AdminReply>(
            &self.runtime.join(ADMIN_SOCKET),
            &json!({"command": "revoke_all"}),
            CHECK_WAIT,
        );
    }
}
