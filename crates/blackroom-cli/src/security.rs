//! Operator verbs for the security authority (Phase 11): sessions, revocation, the remote-access
//! switch, credential rotation, diagnostics. Everything here prints metadata only; a secret is
//! printed exactly once, by the verb that creates it, and never goes to a log.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use blackroom_store::{SecretStore, StoreError};
use remote_hostd::authd::{ADMIN_SOCKET, AUTH_SOCKET, AdminReply, AuthReply, call};
use remote_hostd::{access_key, recovery_codes, remote_switch, totp, trusted_devices};
use serde_json::{Value, json};
use zeroize::Zeroizing;

pub fn store_error(error: StoreError) -> io::Error {
    io::Error::other(error.to_string())
}

/// `$XDG_RUNTIME_DIR/blackroom-hostd`, where the user unit creates its sockets.
pub fn default_runtime() -> io::Result<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|dir| PathBuf::from(dir).join("blackroom-hostd"))
        .filter(|path| path.is_absolute())
        .ok_or_else(|| io::Error::other("no absolute --runtime-dir and no $XDG_RUNTIME_DIR"))
}

fn admin(runtime: &Path, request: Value) -> io::Result<AdminReply> {
    call(&runtime.join(ADMIN_SOCKET), &request).map_err(|error| {
        if matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
        ) {
            io::Error::other("hostd is not running (no admin socket)")
        } else {
            error
        }
    })
}

fn require_ok(reply: AdminReply) -> io::Result<AdminReply> {
    if reply.ok {
        Ok(reply)
    } else {
        Err(io::Error::other(format!(
            "hostd refused: {}",
            reply.code.as_deref().unwrap_or("UNKNOWN")
        )))
    }
}

/// What to say about live sessions when hostd could not end them. Only a missing admin socket proves none exist;
/// a timeout or a refusal does not.
pub fn admin_failure_note(error: &io::Error) -> String {
    if error.to_string().contains("not running") {
        "hostd is not running: no live sessions exist".into()
    } else {
        format!(
            "hostd did not answer ({error}): live sessions may stay open until they expire; run revoke-all when hostd is back"
        )
    }
}

pub fn sessions(runtime: &Path) -> io::Result<String> {
    let reply = require_ok(admin(runtime, json!({"command": "sessions"}))?)?;
    let mut lines = vec![format!(
        "epoch {} | {} session(s)",
        reply.epoch.unwrap_or(0),
        reply.sessions.as_ref().map_or(0, Vec::len)
    )];
    for session in reply.sessions.unwrap_or_default() {
        lines.push(format!(
            "{}\t{}\t{}\texpires_unix_ms={}",
            session.id, session.user_id, session.client_id, session.expires_unix_ms
        ));
    }
    Ok(lines.join("\n"))
}

pub fn revoke_session(runtime: &Path, id: &str) -> io::Result<String> {
    if id.len() != 16 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::other("a session id is 16 hex characters"));
    }
    let reply = require_ok(admin(
        runtime,
        json!({"command": "revoke_session", "id": id}),
    )?)?;
    Ok(format!("revoked {} session(s)", reply.revoked.unwrap_or(0)))
}

pub fn revoke_all(runtime: &Path) -> io::Result<String> {
    let reply = require_ok(admin(runtime, json!({"command": "revoke_all"}))?)?;
    Ok(format!(
        "revoked {} session(s); security epoch is now {}",
        reply.revoked.unwrap_or(0),
        reply.epoch.unwrap_or(0)
    ))
}

/// The flag is written first, directly, so it works when hostd is down; hostd is then told to end
/// its sessions.
pub fn disable(directory: &File, runtime: &Path, reason: Option<&str>) -> io::Result<String> {
    let store = SecretStore::open(directory).map_err(store_error)?;
    remote_switch::set_disabled(&store, reason.unwrap_or("operator")).map_err(store_error)?;
    let ended = match admin(
        runtime,
        json!({"command": "disable", "reason": reason.unwrap_or("operator")}),
    ) {
        Ok(reply) if reply.ok => format!("{} session(s) ended", reply.revoked.unwrap_or(0)),
        Ok(reply) => format!(
            "hostd refused to end sessions: {}",
            reply.code.unwrap_or_default()
        ),
        Err(error) => admin_failure_note(&error),
    };
    Ok(format!("REMOTE ACCESS DISABLED ({ended})"))
}

/// Through hostd when it runs (so a configured polkit gate applies), otherwise directly.
pub fn enable(directory: &File, runtime: &Path) -> io::Result<String> {
    match admin(runtime, json!({"command": "enable"})) {
        Ok(reply) => {
            require_ok(reply)?;
        }
        Err(error) if error.to_string().contains("not running") => {
            let store = SecretStore::open(directory).map_err(store_error)?;
            remote_switch::set_enabled(&store).map_err(store_error)?;
        }
        Err(error) => return Err(error),
    }
    Ok("remote access enabled".into())
}

fn valid_account(account: &str) -> io::Result<()> {
    if totp::valid_account(account) {
        Ok(())
    } else {
        Err(io::Error::other("invalid account name"))
    }
}

pub fn rotate_key(
    directory: &File,
    runtime: &Path,
    account: &str,
    revoke_devices: bool,
) -> io::Result<String> {
    valid_account(account)?;
    let store = SecretStore::open(directory).map_err(store_error)?;
    let key = access_key::rotate(&store, account).map_err(store_error)?;
    let mut text = format!("account: {account}\nremote access key: {}\n", key.as_str());
    if revoke_devices {
        let count = trusted_devices::revoke_all(&store, account).map_err(store_error)?;
        let ended = admin(runtime, json!({"command": "revoke_all"}))
            .ok()
            .filter(|reply| reply.ok)
            .map_or(0, |reply| reply.revoked.unwrap_or(0));
        text.push_str(&format!(
            "revoked {count} trusted device(s) and ended {ended} live session(s)\n"
        ));
    }
    Ok(text)
}

pub fn new_recovery_codes(directory: &File, account: &str) -> io::Result<String> {
    valid_account(account)?;
    let store = SecretStore::open(directory).map_err(store_error)?;
    let codes = recovery_codes::generate(&store, account).map_err(store_error)?;
    let mut text = format!("account: {account}\n");
    for code in codes {
        text.push_str(code.as_str());
        text.push('\n');
    }
    Ok(text)
}

pub fn devices(directory: &File, account: &str) -> io::Result<String> {
    valid_account(account)?;
    let store = SecretStore::open(directory).map_err(store_error)?;
    let listed = trusted_devices::list(&store, account).map_err(store_error)?;
    Ok(listed
        .iter()
        .map(|device| {
            format!(
                "{}\t{}\tcreated={}\tlast_used={}\t{}",
                device.device_id,
                device.label,
                device.created_unix,
                device.last_used_unix,
                if device.revoked { "REVOKED" } else { "trusted" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

pub fn revoke_device(directory: &File, runtime: &Path, device_id: &str) -> io::Result<String> {
    let store = SecretStore::open(directory).map_err(store_error)?;
    if !trusted_devices::revoke(&store, device_id).map_err(store_error)? {
        return Err(io::Error::other(
            "no such trusted device, or already revoked",
        ));
    }
    let ended = match admin(
        runtime,
        json!({"command": "revoke_client", "client_id": device_id}),
    ) {
        Ok(reply) if reply.ok => format!("{} live session(s) ended", reply.revoked.unwrap_or(0)),
        Ok(reply) => format!(
            "hostd refused to end sessions: {}",
            reply.code.unwrap_or_default()
        ),
        Err(error) => admin_failure_note(&error),
    };
    Ok(format!("device revoked; {ended}"))
}

/// Per-account credential state: presence and counts only.
pub fn credentials(directory: &File) -> Value {
    let Ok(store) = SecretStore::open(directory) else {
        return json!("unreadable");
    };
    let accounts = totp::enrolled_accounts(directory).unwrap_or_default();
    json!(
        accounts
            .iter()
            .map(|account| {
                let key = access_key::configured(&store, account).map_err(|_| ());
                let codes = recovery_codes::remaining(&store, account).map_err(|_| ());
                let devices = trusted_devices::list(&store, account)
                    .map(|list| list.iter().filter(|device| !device.revoked).count())
                    .map_err(|_| ());
                json!({
                    "account": account,
                    "totp": true,
                    "access_key": key.map_or(json!("unreadable"), |present| json!(present)),
                    "recovery_codes_left": codes.map_or(json!("unreadable"), |count| json!(count)),
                    "trusted_devices": devices.map_or(json!("unreadable"), |count| json!(count)),
                })
            })
            .collect::<Vec<_>>()
    )
}

pub fn remote_access_label(directory: &File) -> &'static str {
    let Ok(store) = SecretStore::open(directory) else {
        return "unreadable";
    };
    match remote_switch::state(&store) {
        Ok(None) => "enabled",
        Ok(Some(_)) => "disabled",
        Err(_) => "unreadable",
    }
}

fn first_line(command: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait().ok()? {
            Some(status) if status.success() => break,
            Some(_) => return None,
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut line = String::new();
    BufReader::new(child.stdout.take()?)
        .read_line(&mut line)
        .ok()?;
    Some(line.trim().chars().take(120).collect())
}

fn os_name() -> Option<String> {
    std::fs::read_to_string("/etc/os-release")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|name| name.trim_matches('"').to_string())
}

pub fn compatibility() -> Value {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    json!({
        "os": os_name(),
        "kernel": std::fs::read_to_string("/proc/sys/kernel/osrelease").ok().map(|text| text.trim().to_string()),
        "session_type": std::env::var("XDG_SESSION_TYPE").ok(),
        "desktop": std::env::var("XDG_CURRENT_DESKTOP").ok(),
        "gnome_shell": first_line("gnome-shell", &["--version"]),
        "pipewire_socket": runtime.as_ref().is_some_and(|dir| dir.join("pipewire-0").exists()),
        "pam_service_installed": Path::new("/etc/pam.d/blackroom-console").is_file(),
        "polkit_policy_installed": Path::new("/usr/share/polkit-1/actions/org.blackroom.console.policy").is_file(),
    })
}

/// Everything an operator may paste into a bug report: no secrets by construction.
pub fn diagnostics(
    directory: &File,
    path: &Path,
    status: Value,
    doctor: Vec<String>,
    audit_tail: Vec<Value>,
) -> Value {
    json!({
        "note": "contains no passwords, keys, codes, tokens or credential files",
        "state_dir": path.display().to_string(),
        "status": status,
        "remote_access": remote_access_label(directory),
        "credentials": credentials(directory),
        "doctor": doctor,
        "compatibility": compatibility(),
        "audit_tail": audit_tail,
    })
}

pub(crate) fn prompt(prompt: &str, echo: bool) -> io::Result<Zeroizing<String>> {
    use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};
    let tty = File::options().read(true).write(true).open("/dev/tty")?;
    (&tty).write_all(prompt.as_bytes())?;
    let original = tcgetattr(&tty)?;
    if !echo {
        let mut quiet = original.clone();
        quiet.local_modes.remove(LocalModes::ECHO);
        tcsetattr(&tty, OptionalActions::Flush, &quiet)?;
    }
    let mut line = Zeroizing::new(String::new());
    let read = BufReader::new(&tty).read_line(&mut line);
    if !echo {
        let _ = tcsetattr(&tty, OptionalActions::Flush, &original);
        (&tty).write_all(b"\n")?;
    }
    read?;
    Ok(Zeroizing::new(
        line.trim_end_matches(['\n', '\r']).to_string(),
    ))
}

/// A real login against the running auth service, typed at the terminal; it prints only the
/// verdict and ends the session it just opened.
pub fn login_check(runtime: &Path, account: &str) -> io::Result<String> {
    valid_account(account)?;
    let socket = runtime.join(AUTH_SOCKET);
    let password = prompt("Linux password: ", false)?;
    let second = prompt("Authenticator code (6 digits) or recovery code: ", false)?;
    let client = prompt("Remote Access Key, or <device-id>:<device-secret>: ", false)?;
    let mut request = json!({
        "command": "login", "account": account, "client_id": "login-check",
        "password": password.as_str(),
    });
    let fields = request.as_object_mut().expect("object");
    if second.len() == 6 && second.bytes().all(|byte| byte.is_ascii_digit()) {
        fields.insert("totp".into(), json!(second.as_str()));
    } else {
        fields.insert("recovery_code".into(), json!(second.as_str()));
    }
    match client.split_once(':') {
        Some((id, secret)) => {
            fields.insert("device_id".into(), json!(id));
            fields.insert("device_secret".into(), json!(secret));
        }
        None => {
            fields.insert("access_key".into(), json!(client.as_str()));
        }
    }
    let reply: AuthReply = call(&socket, &request).map_err(|error| {
        if matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
        ) {
            io::Error::other("hostd is not running (no auth socket)")
        } else {
            error
        }
    })?;
    if reply.ok {
        if let Some(token) = &reply.token {
            let _: AuthReply = call(
                &socket,
                &json!({"command": "logout", "token": token.as_str()}),
            )?;
        }
        Ok("LOGIN OK: password, authenticator and key/device all accepted; the test session was ended".into())
    } else {
        Err(io::Error::other(format!(
            "LOGIN REFUSED: {}",
            reply.code.unwrap_or_else(|| "UNKNOWN".into())
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_missing_admin_socket_proves_that_no_sessions_exist() {
        let absent = io::Error::other("hostd is not running (no admin socket)");
        assert!(admin_failure_note(&absent).contains("no live sessions exist"));
        let silent = io::Error::new(io::ErrorKind::TimedOut, "read timed out");
        let note = admin_failure_note(&silent);
        assert!(!note.contains("no live sessions exist"), "{note}");
        assert!(note.contains("revoke-all"), "{note}");
    }
}
