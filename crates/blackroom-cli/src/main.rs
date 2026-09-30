#![forbid(unsafe_code)]
//! Operator CLI for the offline simulation state directory (roadmap Phase 11 first verbs).
//! `status`, `logs`, `doctor`, `accounts` and `emergency-status` never create, lock or change
//! anything and never print key material, proofs, input grants or credential secrets.
//! `enroll` is the one verb that writes: it adds a TOTP account to the credential file and prints
//! the new secret exactly once.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use remote_emergencyd::client::Client;
use remote_hostd::audit::AUDIT_FILE;
use remote_hostd::store::{PersistentHostAuthority, open_state_directory};
use remote_hostd::totp;
use rustix::fs::{Mode, OFlags};

const DEFAULT_TAIL: usize = 20;
const MAX_TAIL: usize = 1000;
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const USAGE: &str = "usage: blackroom --state-dir <absolute path> status | logs [--tail <1-1000>] | doctor | accounts | enroll --account <name>\n       blackroom emergency-status --socket <absolute path>";

enum Verb {
    Status,
    Logs(usize),
    Doctor,
    Accounts,
    Enroll(String),
}

enum Command {
    State(PathBuf, Verb),
    EmergencyStatus(PathBuf),
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
    let [flag, path, verb, rest @ ..] = args else {
        return Err(USAGE.into());
    };
    if flag != OsStr::new("--state-dir") {
        return Err(USAGE.into());
    }
    let verb = match (verb.to_str(), rest) {
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
    Ok(Command::State(PathBuf::from(path), verb))
}

fn status(directory: &File) -> io::Result<String> {
    let status = PersistentHostAuthority::inspect(directory)?;
    Ok(serde_json::json!({
        "mode": "OFFLINE_SIMULATION",
        "epoch": status.epoch,
        "emergency_pending": status.emergency_pending,
        "recovery_pending": status.recovery_pending,
    })
    .to_string())
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
    let files: [(&str, Option<u64>, bool); 6] = [
        ("host-identity.key", Some(32), true),
        ("security-epoch", Some(8), true),
        ("audit.log", None, false),
        ("emergency-stop", Some(8), false),
        ("recovery-pending", Some(8), false),
        ("totp-credentials", None, false),
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
    })
    .to_string())
}

fn run(args: &[OsString]) -> Result<ExitCode, (u8, String)> {
    let refuse = |error: io::Error| (1, format!("refused: {error}"));
    let (path, verb) = match parse(args).map_err(|message| (2, message))? {
        Command::EmergencyStatus(socket) => {
            println!("{}", emergency_status(&socket).map_err(refuse)?);
            return Ok(ExitCode::SUCCESS);
        }
        Command::State(path, verb) => (path, verb),
    };
    let directory = open_state_directory(&path).map_err(refuse)?;
    match verb {
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
            println!("secret: {secret}");
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
    match run(&args) {
        Ok(code) => code,
        Err((code, message)) => {
            eprintln!("{message}");
            ExitCode::from(code)
        }
    }
}
