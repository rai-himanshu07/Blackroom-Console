#![forbid(unsafe_code)]
//! Operator CLI for the offline simulation state directory (roadmap Phase 11 first verbs).
//! `status`, `logs`, `doctor`, `accounts` and `emergency-status` never create, lock or change
//! anything and never print key material, proofs, input grants or credential secrets.
//! `enroll` is the one verb that writes: it adds a TOTP account to the credential file and prints
//! the new secret exactly once. The security verbs (`sessions`, `revoke-session`, `revoke-all`,
//! `disable`, `enable`, `rotate-key`, `recovery-codes`, `devices`, `revoke-device`, `login-check`,
//! `compatibility`, `diagnostics`) live in `security.rs`; the live ones talk to hostd's private
//! admin socket.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod security;
mod setup;

use remote_emergencyd::client::Client;
use remote_hostd::audit::AUDIT_FILE;
use remote_hostd::store::{PersistentHostAuthority, open_state_directory};
use remote_hostd::totp;
use rustix::fs::{Mode, OFlags};

const DEFAULT_TAIL: usize = 20;
const MAX_TAIL: usize = 1000;
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const USAGE: &str = "usage: blackroom --state-dir <absolute path> [--runtime-dir <absolute path>] <verb>\n  verbs: status | logs [--tail <1-1000>] | doctor | accounts | enroll --account <name>\n         sessions | revoke-session <id> | revoke-all | disable [--reason <text>] | enable\n         rotate-key --account <name> [--revoke-devices] | recovery-codes --account <name>\n         devices --account <name> | revoke-device <id> | login-check --account <name>\n         compatibility | diagnostics\n       blackroom emergency-status --socket <absolute path>\n       blackroom setup | reset <soft|security|full> | repair [--fix]   (first run, credential resets, repair; blackroom setup --help style options in docs)";

enum Verb {
    Status,
    Logs(usize),
    Doctor,
    Accounts,
    Enroll(String),
    Sessions,
    RevokeSession(String),
    RevokeAll,
    Disable(Option<String>),
    Enable,
    RotateKey {
        account: String,
        revoke_devices: bool,
    },
    RecoveryCodes(String),
    Devices(String),
    RevokeDevice(String),
    LoginCheck(String),
    Compatibility,
    Diagnostics,
}

enum Command {
    State {
        path: PathBuf,
        runtime: Option<PathBuf>,
        verb: Verb,
    },
    EmergencyStatus(PathBuf),
}

fn account_flag(rest: &[OsString]) -> Result<String, String> {
    match rest {
        [flag, account] if flag == OsStr::new("--account") => {
            Ok(account.to_str().ok_or(USAGE)?.to_string())
        }
        _ => Err(USAGE.into()),
    }
}

fn parse(args: &[OsString]) -> Result<Command, String> {
    if let [verb, flag, socket] = args
        && verb == OsStr::new("emergency-status")
        && flag == OsStr::new("--socket")
    {
        let socket = PathBuf::from(socket);
        return if socket.is_absolute() {
            Ok(Command::EmergencyStatus(socket))
        } else {
            Err(USAGE.into())
        };
    }
    let [flag, path, tail @ ..] = args else {
        return Err(USAGE.into());
    };
    if flag != OsStr::new("--state-dir") {
        return Err(USAGE.into());
    }
    let (runtime, tail) = match tail {
        [flag, runtime, rest @ ..] if flag == OsStr::new("--runtime-dir") => {
            let runtime = PathBuf::from(runtime);
            if !runtime.is_absolute() {
                return Err(USAGE.into());
            }
            (Some(runtime), rest)
        }
        _ => (None, tail),
    };
    let [verb, rest @ ..] = tail else {
        return Err(USAGE.into());
    };
    let verb = match (verb.to_str(), rest) {
        (Some("sessions"), []) => Verb::Sessions,
        (Some("revoke-session"), [id]) => Verb::RevokeSession(id.to_str().ok_or(USAGE)?.into()),
        (Some("revoke-all"), []) => Verb::RevokeAll,
        (Some("disable"), []) => Verb::Disable(None),
        (Some("disable"), [flag, reason]) if flag == OsStr::new("--reason") => {
            Verb::Disable(Some(reason.to_str().ok_or(USAGE)?.into()))
        }
        (Some("enable"), []) => Verb::Enable,
        (Some("rotate-key"), rest) => {
            let (rest, revoke_devices) = match rest {
                [account @ .., flag] if flag == OsStr::new("--revoke-devices") => (account, true),
                rest => (rest, false),
            };
            Verb::RotateKey {
                account: account_flag(rest)?,
                revoke_devices,
            }
        }
        (Some("recovery-codes"), rest) => Verb::RecoveryCodes(account_flag(rest)?),
        (Some("devices"), rest) => Verb::Devices(account_flag(rest)?),
        (Some("revoke-device"), [id]) => Verb::RevokeDevice(id.to_str().ok_or(USAGE)?.into()),
        (Some("login-check"), rest) => Verb::LoginCheck(account_flag(rest)?),
        (Some("compatibility"), []) => Verb::Compatibility,
        (Some("diagnostics"), []) => Verb::Diagnostics,
        (Some("status"), []) => Verb::Status,
        (Some("doctor"), []) => Verb::Doctor,
        (Some("accounts"), []) => Verb::Accounts,
        (Some("enroll"), [account_flag, account]) if account_flag == OsStr::new("--account") => {
            Verb::Enroll(account.to_str().ok_or(USAGE)?.to_string())
        }
        (Some("logs"), []) => Verb::Logs(DEFAULT_TAIL),
        (Some("logs"), [tail, count]) if tail == OsStr::new("--tail") => {
            let count = count
                .to_str()
                .and_then(|count| count.parse::<usize>().ok())
                .filter(|count| (1..=MAX_TAIL).contains(count))
                .ok_or(USAGE)?;
            Verb::Logs(count)
        }
        _ => return Err(USAGE.into()),
    };
    Ok(Command::State {
        path: PathBuf::from(path),
        runtime,
        verb,
    })
}

fn status_value(directory: &File, status: &remote_hostd::store::HostStatus) -> serde_json::Value {
    serde_json::json!({
        "mode": "OFFLINE_SIMULATION",
        "epoch": status.epoch,
        "emergency_pending": status.emergency_pending,
        "recovery_pending": status.recovery_pending,
        "remote_access": security::remote_access_label(directory),
        "credentials": security::credentials(directory),
    })
}

fn status(directory: &File) -> io::Result<String> {
    let status = PersistentHostAuthority::inspect(directory)?;
    Ok(status_value(directory, &status).to_string())
}

fn logs(directory: &File, tail: usize) -> io::Result<(String, usize)> {
    let fd = rustix::fs::openat(
        directory,
        AUDIT_FILE,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let mut file = File::from(fd);
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.mode() & 0o077 != 0
        || metadata.len() > MAX_LOG_BYTES
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "audit log is not a private, bounded regular file",
        ));
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    let mut valid = Vec::new();
    let mut invalid = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) if value["event"].is_string() => valid.push(line),
            _ => invalid += 1,
        }
    }
    let start = valid.len().saturating_sub(tail);
    Ok((valid[start..].join("\n"), invalid))
}

fn doctor(directory: &Path) -> (Vec<String>, bool) {
    let mut report = Vec::new();
    let mut failed = false;
    let uid = rustix::process::getuid().as_raw();
    let mut check = |ok: bool, what: String| {
        failed |= !ok;
        report.push(format!("{} {what}", if ok { "OK  " } else { "FAIL" }));
    };
    match std::fs::symlink_metadata(directory) {
        Ok(metadata) => check(
            metadata.is_dir() && metadata.uid() == uid && metadata.mode() & 0o077 == 0,
            "state directory is an owner-only directory".into(),
        ),
        Err(error) => check(
            false,
            format!("state directory unreadable: {}", error.kind()),
        ),
    }
    let files: [(&str, Option<u64>, bool); 10] = [
        ("host-identity.key", Some(32), true),
        ("security-epoch", Some(8), true),
        ("audit.log", None, false),
        ("emergency-stop", Some(8), false),
        ("recovery-pending", Some(8), false),
        ("totp-credentials", None, false),
        ("access-keys", None, false),
        ("recovery-codes", None, false),
        ("trusted-devices", None, false),
        ("remote-access", None, false),
    ];
    let mut warnings = Vec::new();
    for (name, length, required) in files {
        match std::fs::symlink_metadata(directory.join(name)) {
            Ok(metadata) => {
                check(
                    metadata.is_file()
                        && metadata.uid() == uid
                        && metadata.mode() & 0o777 == 0o600
                        && length.is_none_or(|length| metadata.len() == length),
                    format!("{name} is an owner-only regular file of the expected size"),
                );
                match name {
                    "emergency-stop" => {
                        warnings
                            .push("emergency stop persisted; local recovery required".to_owned());
                    }
                    "recovery-pending" => {
                        warnings.push(
                            "unverified grant recovery pending; hostd will refuse to start"
                                .to_owned(),
                        );
                    }
                    _ => {}
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if required {
                    warnings.push(format!("{name} missing; host not initialised"));
                }
            }
            Err(error) => check(false, format!("{name} unreadable: {}", error.kind())),
        }
    }
    report.extend(
        warnings
            .into_iter()
            .map(|warning| format!("WARN {warning}")),
    );
    (report, failed)
}

fn emergency_status(socket: &Path) -> io::Result<String> {
    let mut client = Client::connect(socket, std::time::Duration::from_secs(3))?;
    let status = client.status()?;
    Ok(serde_json::json!({
        "phase": status.phase,
        "held": status.held,
        "grabs_enabled": status.grabs_enabled,
        "reads": status.reads,
        "active_nodes": status.active_nodes,
    })
    .to_string())
}

fn run(args: &[OsString]) -> Result<ExitCode, (u8, String)> {
    let refuse = |error: io::Error| (1, format!("refused: {error}"));
    let (path, runtime, verb) = match parse(args).map_err(|message| (2, message))? {
        Command::EmergencyStatus(socket) => {
            println!("{}", emergency_status(&socket).map_err(refuse)?);
            return Ok(ExitCode::SUCCESS);
        }
        Command::State {
            path,
            runtime,
            verb,
        } => (path, runtime, verb),
    };
    let directory = open_state_directory(&path).map_err(refuse)?;
    let runtime = || match &runtime {
        Some(runtime) => Ok(runtime.clone()),
        None => security::default_runtime(),
    };
    match verb {
        Verb::Sessions => println!(
            "{}",
            security::sessions(&runtime().map_err(refuse)?).map_err(refuse)?
        ),
        Verb::RevokeSession(id) => println!(
            "{}",
            security::revoke_session(&runtime().map_err(refuse)?, &id).map_err(refuse)?
        ),
        Verb::RevokeAll => println!(
            "{}",
            security::revoke_all(&runtime().map_err(refuse)?).map_err(refuse)?
        ),
        Verb::Disable(reason) => println!(
            "{}",
            security::disable(&directory, &runtime().map_err(refuse)?, reason.as_deref())
                .map_err(refuse)?
        ),
        Verb::Enable => println!(
            "{}",
            security::enable(&directory, &runtime().map_err(refuse)?).map_err(refuse)?
        ),
        Verb::RotateKey {
            account,
            revoke_devices,
        } => {
            print!(
                "{}",
                security::rotate_key(
                    &directory,
                    &runtime().map_err(refuse)?,
                    &account,
                    revoke_devices
                )
                .map_err(refuse)?
            );
            eprintln!(
                "The key is shown once and is not recoverable; store it in a password manager now."
            );
        }
        Verb::RecoveryCodes(account) => {
            print!(
                "{}",
                security::new_recovery_codes(&directory, &account).map_err(refuse)?
            );
            eprintln!(
                "Each code works once and replaces the authenticator code only; these are shown once."
            );
        }
        Verb::Devices(account) => println!(
            "{}",
            security::devices(&directory, &account).map_err(refuse)?
        ),
        Verb::RevokeDevice(id) => println!(
            "{}",
            security::revoke_device(&directory, &runtime().map_err(refuse)?, &id)
                .map_err(refuse)?
        ),
        Verb::LoginCheck(account) => println!(
            "{}",
            security::login_check(&runtime().map_err(refuse)?, &account).map_err(refuse)?
        ),
        Verb::Compatibility => println!("{}", security::compatibility()),
        Verb::Diagnostics => {
            let status = PersistentHostAuthority::inspect(&directory)
                .map(|status| status_value(&directory, &status))
                .unwrap_or_else(
                    |error| serde_json::json!({"unavailable": error.kind().to_string()}),
                );
            let (report, _) = doctor(&path);
            let tail = logs(&directory, 50)
                .map(|(text, _)| {
                    text.lines()
                        .filter_map(|line| serde_json::from_str(line).ok())
                        .collect::<Vec<serde_json::Value>>()
                })
                .unwrap_or_default();
            println!(
                "{}",
                serde_json::to_string_pretty(&security::diagnostics(
                    &directory, &path, status, report, tail
                ))
                .map_err(|error| (1, error.to_string()))?
            );
        }
        Verb::Status => println!("{}", status(&directory).map_err(refuse)?),
        Verb::Logs(tail) => {
            let (text, invalid) = logs(&directory, tail).map_err(refuse)?;
            if !text.is_empty() {
                println!("{text}");
            }
            if invalid > 0 {
                return Err((1, format!("audit log has {invalid} invalid line(s)")));
            }
        }
        Verb::Accounts => {
            for account in totp::enrolled_accounts(&directory).map_err(refuse)? {
                println!("{account}");
            }
        }
        Verb::Enroll(account) => {
            let secret = totp::enroll(&directory, &account).map_err(refuse)?;
            println!("account: {account}");
            println!("secret: {}", secret.as_str());
            println!("uri: {}", totp::otpauth_uri(&account, &secret));
            eprintln!(
                "The secret is shown once and is not recoverable; add it to an authenticator app now."
            );
        }
        Verb::Doctor => {
            let (report, failed) = doctor(&path);
            for line in report {
                println!("{line}");
            }
            if failed {
                return Ok(ExitCode::from(1));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if let Some(result) = setup::dispatch(&args) {
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err((code, message)) => {
                eprintln!("{message}");
                ExitCode::from(code)
            }
        };
    }
    match run(&args) {
        Ok(code) => code,
        Err((code, message)) => {
            eprintln!("{message}");
            ExitCode::from(code)
        }
    }
}
