//! First-run wizard, reset levels and repair for one user's install. These are the only verbs that
//! start or stop services; everything they do to credentials goes through the same functions as the
//! individual verbs. A secret is printed exactly once, by the step that creates it.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use blackroom_store::SecretStore;
use remote_hostd::authd::{ADMIN_SOCKET, AUTH_SOCKET};
use remote_hostd::store::{PersistentHostAuthority, open_state_directory};
use remote_hostd::{access_key, recovery_codes, remote_switch, totp, trusted_devices};

use crate::security;

pub const USAGE: &str = "usage: blackroom setup [OPTIONS]\n       blackroom reset <soft|security|full> [OPTIONS]\n       blackroom repair [--fix] [OPTIONS]\n  options: --account <name> (default $USER)  --state-dir <abs path> (default ~/.local/share/blackroom-console/hostd)\n           --runtime-dir <abs path> (default $XDG_RUNTIME_DIR/blackroom-hostd)  --no-system (skip systemctl, PAM and file checks of the install)\n           --no-login-check (setup)  --yes (reset full: skip the typed confirmation)";

const LIB_DIRS: [&str; 2] = ["/usr/lib/blackroom", ".local/lib/blackroom"];
const HOSTD_UNIT: &str = "remote-hostd.service";
const CONSOLE_UNITS: [&str; 2] = [
    "blackroom-console.service",
    "blackroom-console-emergencyd.service",
];
/// Everything `reset full` removes: the files hostd and the verbs create, and nothing else.
const OWNED_FILES: [&str; 10] = [
    "totp-credentials",
    "totp-credentials.lock",
    "access-keys",
    "recovery-codes",
    "trusted-devices",
    "remote-access",
    "audit.log",
    "security-epoch",
    "host-identity.key",
    ".store.lock",
];
/// Safety latches: a full reset refuses while one is set, because deleting the state would hide it.
const LATCHES: [&str; 2] = ["emergency-stop", "recovery-pending"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Soft,
    Security,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Setup,
    Reset(Level),
    Repair,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub task: Task,
    pub state: PathBuf,
    pub runtime: PathBuf,
    pub account: String,
    pub system: bool,
    pub yes: bool,
    pub fix: bool,
    pub login_check: bool,
}

type Failure = (u8, String);

fn refuse(error: impl std::fmt::Display) -> Failure {
    (1, format!("refused: {error}"))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// Parses the arguments after `setup`, `reset <level>` or `repair`.
pub fn parse(verb: &str, args: &[OsString]) -> Result<Options, String> {
    let (task, rest) = match verb {
        "setup" => (Task::Setup, args),
        "repair" => (Task::Repair, args),
        "reset" => {
            let (level, rest) = args.split_first().ok_or(USAGE)?;
            let level = match level.to_str() {
                Some("soft") => Level::Soft,
                Some("security") => Level::Security,
                Some("full") => Level::Full,
                _ => return Err(USAGE.into()),
            };
            (Task::Reset(level), rest)
        }
        _ => return Err(USAGE.into()),
    };
    let mut state = None;
    let mut runtime = None;
    let mut account = None;
    let (mut system, mut yes, mut fix, mut login_check) = (true, false, false, true);
    let mut iter = rest.iter();
    let value = |iter: &mut std::slice::Iter<'_, OsString>| {
        iter.next().cloned().ok_or_else(|| USAGE.to_string())
    };
    while let Some(arg) = iter.next() {
        match arg.to_str() {
            Some("--state-dir") => state = Some(PathBuf::from(value(&mut iter)?)),
            Some("--runtime-dir") => runtime = Some(PathBuf::from(value(&mut iter)?)),
            Some("--account") => {
                account = Some(
                    value(&mut iter)?
                        .into_string()
                        .map_err(|_| USAGE.to_string())?,
                );
            }
            Some("--no-system") => system = false,
            Some("--no-login-check") => login_check = false,
            Some("--yes") => yes = true,
            Some("--fix") if task == Task::Repair => fix = true,
            _ => return Err(USAGE.into()),
        }
    }
    let state = match state {
        Some(path) => path,
        None => home()
            .ok_or("no absolute $HOME: pass --state-dir")?
            .join(".local/share/blackroom-console/hostd"),
    };
    let runtime = match runtime {
        Some(path) => path,
        None => security::default_runtime().map_err(|error| error.to_string())?,
    };
    if !state.is_absolute() || !runtime.is_absolute() {
        return Err("--state-dir and --runtime-dir must be absolute paths".into());
    }
    let account = account
        .or_else(|| std::env::var("USER").ok())
        .filter(|name| totp::valid_account(name))
        .ok_or("no usable account name: pass --account <name> (a-z, 0-9, _ . -)")?;
    Ok(Options {
        task,
        state,
        runtime,
        account,
        system,
        yes,
        fix,
        login_check,
    })
}

pub fn run(options: &Options) -> Result<(), Failure> {
    match options.task {
        Task::Setup => setup(options),
        Task::Reset(Level::Soft) => reset_soft(options),
        Task::Reset(Level::Security) => reset_security(options),
        Task::Reset(Level::Full) => reset_full(options),
        Task::Repair => repair(options),
    }
}

// ---- helpers ----

fn command(program: &str, args: &[&str]) -> Option<(bool, String)> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

fn systemctl(args: &[&str]) -> bool {
    let mut full = vec!["--user", "--no-pager"];
    full.extend_from_slice(args);
    command("systemctl", &full).is_some_and(|(ok, _)| ok)
}

/// Restarts the console unit (ends a running remote session).
pub fn restart_console() -> bool {
    systemctl(&["restart", CONSOLE_UNITS[0]])
}

fn unit_active(unit: &str) -> bool {
    command(
        "systemctl",
        &["--user", "--no-pager", "is-active", "--quiet", unit],
    )
    .is_some_and(|(ok, _)| ok)
}

fn console_running() -> bool {
    // 15 characters: the kernel truncates the process name.
    command("pgrep", &["-x", "blackroom-conso"]).is_some_and(|(ok, _)| ok)
}

fn helper_path() -> Option<PathBuf> {
    LIB_DIRS.iter().find_map(|dir| {
        let path = if dir.starts_with('/') {
            PathBuf::from(dir)
        } else {
            home()?.join(dir)
        }
        .join("pam-auth-helper");
        path.is_file().then_some(path)
    })
}

fn ensure_state_dir(path: &Path) -> Result<(), Failure> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.uid() != rustix::process::getuid().as_raw() {
                return Err(refuse(
                    "the state directory is not a directory of yours (remove it or pass another --state-dir)",
                ));
            }
            if metadata.mode() & 0o077 != 0 {
                return Err(refuse(
                    "the state directory is readable by others: run `blackroom repair --fix`",
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(refuse),
        Err(error) => Err(refuse(error)),
    }
}

fn open_store(path: &Path) -> Result<(File, SecretStore), Failure> {
    let directory = open_state_directory(path).map_err(refuse)?;
    let store =
        SecretStore::open(&directory).map_err(|error| refuse(security::store_error(error)))?;
    Ok((directory, store))
}

fn qr_text(uri: &str) -> Option<String> {
    use qrcode::render::unicode::Dense1x2;
    let code = qrcode::QrCode::new(uri.as_bytes()).ok()?;
    Some(
        code.render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .quiet_zone(true)
            .build(),
    )
}

fn print_authenticator(account: &str, secret: &str) {
    let uri = totp::otpauth_uri(account, secret);
    println!("  authenticator secret: {secret}");
    println!("  otpauth uri: {uri}");
    match qr_text(&uri) {
        Some(qr) => {
            println!(
                "  Scan this with your authenticator app (on a light terminal the colours may need inverting; or type the secret):"
            );
            println!("{qr}");
        }
        None => {
            println!("  (no QR code could be drawn: type the secret into your authenticator app)")
        }
    }
}

fn print_key(account: &str, key: &str) {
    println!("  remote access key: {key}");
    println!("  (account {account}; shown once, store it in a password manager)");
}

fn print_codes(codes: &[zeroize::Zeroizing<String>]) {
    println!("  recovery codes (each works once, instead of the authenticator code; shown once):");
    for code in codes {
        println!("    {}", code.as_str());
    }
}

fn wait_for_socket(runtime: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if runtime.join(AUTH_SOCKET).exists() && runtime.join(ADMIN_SOCKET).exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

/// A wrong password must be rejected with exit 1; 2 means PAM could not decide.
fn pam_self_test(helper: &Path) -> Result<(), String> {
    let user = std::env::var("USER").map_err(|_| "no $USER".to_string())?;
    let mut child = Command::new(helper)
        .args(["--service", "blackroom-console"])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{user}\nthis-is-not-the-password");
    }
    match child.wait().map_err(|error| error.to_string())?.code() {
        Some(1) => Ok(()),
        other => Err(format!(
            "the PAM self-test returned {other:?} (expected 1: wrong password rejected)"
        )),
    }
}

// ---- setup ----

fn setup(o: &Options) -> Result<(), Failure> {
    println!("Blackroom Console setup for account {}", o.account);
    if o.system {
        if rustix::process::getuid().is_root() {
            return Err((2, "run setup as your normal user, not root".into()));
        }
        if !Path::new("/etc/pam.d/blackroom-console").is_file() {
            return Err(refuse(
                "/etc/pam.d/blackroom-console is missing: install the blackroom-console package (or docs/ops/install-security.sh)",
            ));
        }
        if helper_path().is_none() {
            return Err(refuse(
                "pam-auth-helper is not installed in /usr/lib/blackroom",
            ));
        }
        if std::env::var("XDG_SESSION_TYPE").is_ok_and(|kind| kind != "wayland") {
            println!("WARN this is not a Wayland session: the console needs GNOME on Wayland");
        }
    }
    ensure_state_dir(&o.state)?;
    let directory = open_state_directory(&o.state).map_err(refuse)?;
    if PersistentHostAuthority::inspect(&directory).is_err() {
        drop(PersistentHostAuthority::open(&directory).map_err(refuse)?);
    }
    let store = SecretStore::open(&directory).map_err(|e| refuse(security::store_error(e)))?;
    println!("1. state directory {} (owner only)", o.state.display());

    let enrolled = totp::enrolled_accounts(&directory).map_err(refuse)?;
    if enrolled.contains(&o.account) {
        println!("2. authenticator: already enrolled (kept)");
    } else {
        let secret = totp::enroll(&directory, &o.account).map_err(refuse)?;
        println!("2. authenticator: new secret");
        print_authenticator(&o.account, secret.as_str());
    }

    if access_key::configured(&store, &o.account).map_err(|e| refuse(security::store_error(e)))? {
        println!("3. remote access key: already set (kept)");
    } else {
        let key =
            access_key::rotate(&store, &o.account).map_err(|e| refuse(security::store_error(e)))?;
        println!("3. remote access key: new");
        print_key(&o.account, key.as_str());
    }

    if recovery_codes::remaining(&store, &o.account)
        .map_err(|e| refuse(security::store_error(e)))?
        > 0
    {
        println!("4. recovery codes: some are left (kept)");
    } else {
        let codes = recovery_codes::generate(&store, &o.account)
            .map_err(|e| refuse(security::store_error(e)))?;
        println!("4. recovery codes: new");
        print_codes(&codes);
    }
    println!(
        "5. remote access is {}",
        security::remote_access_label(&directory)
    );

    let mut problems = Vec::new();
    if o.system {
        if let Some(helper) = helper_path() {
            match pam_self_test(&helper) {
                Ok(()) => println!("6. password check: PAM answers (a wrong password is rejected)"),
                Err(error) => problems.push(error),
            }
        }
        systemctl(&["daemon-reload"]);
        if systemctl(&["start", HOSTD_UNIT]) && wait_for_socket(&o.runtime) {
            println!("7. login authority running ({HOSTD_UNIT})");
        } else {
            problems.push(format!(
                "{HOSTD_UNIT} did not start: journalctl --user -u {HOSTD_UNIT} -n 30"
            ));
        }
    } else {
        println!("6. (system steps skipped: --no-system)");
    }

    if problems.is_empty() && o.system && o.login_check && rustix::termios::isatty(io::stdin()) {
        println!(
            "8. test login (type your password, a current authenticator code and the key; nothing is stored):"
        );
        match security::login_check(&o.runtime, &o.account) {
            Ok(text) => println!("   {text}"),
            Err(error) => problems.push(format!("test login: {error}")),
        }
    }
    for problem in &problems {
        println!("PROBLEM {problem}");
    }

    if problems.is_empty() && o.system && rustix::termios::isatty(io::stdin()) {
        println!(
            "9. Reach the laptop from outside your home network? The default is no: home network only. You can do this later with `blackroom internet`."
        );
        let stdin = io::stdin();
        let mut input = stdin.lock();
        let mut out = io::stdout();
        let mut asker = crate::internet::Io::new(&mut input, &mut out);
        if asker.yes_no("   Set it up now?", false).unwrap_or(false) {
            let outcome = match (
                crate::internet::Binary::find(),
                crate::internet::default_tls_dir(),
            ) {
                (Some(path), Some(tls_dir)) => crate::internet::wizard(
                    &crate::internet::Binary {
                        path,
                        runtime: o.runtime.clone(),
                    },
                    &mut asker,
                    &tls_dir,
                    &crate::internet::restart_unit,
                    &crate::internet::tailscale_name,
                ),
                _ => Err((
                    1,
                    "blackroom-console is not installed where it is expected".to_string(),
                )),
            };
            if let Err((_, message)) = outcome {
                println!("   {message}");
                println!("   Nothing was left half done: run `blackroom internet` to try again.");
            }
        }
    }

    println!("\nNext:");
    println!(
        "  sudo blackroom-grant-input grant          # once per boot: lets the console grab the built-in keyboard and touchpad"
    );
    println!("  systemctl --user start blackroom-console.service");
    println!(
        "  cat $XDG_RUNTIME_DIR/blackroom-console/url   # the https address for the tablet; log in with password, code and key"
    );
    println!(
        "Outside your home network: blackroom internet   (guided: a VPN such as Tailscale, or direct access with a name or a static IP; see /usr/share/doc/blackroom-console/internet-access.md)"
    );
    println!(
        "Trouble: blackroom repair   |   lost phone or key: blackroom reset security   |   forget everything: blackroom reset full"
    );
    if problems.is_empty() {
        Ok(())
    } else {
        Err((1, "setup finished with problems (see PROBLEM lines)".into()))
    }
}

// ---- reset ----

fn end_sessions(runtime: &Path) -> String {
    match security::revoke_all(runtime) {
        Ok(text) => text,
        Err(error) => security::admin_failure_note(&error),
    }
}

fn stop_services(o: &Options) {
    if !o.system {
        return;
    }
    let mut args = vec!["stop"];
    args.extend(CONSOLE_UNITS);
    args.push(HOSTD_UNIT);
    systemctl(&args);
    clear_leftovers(true);
}

fn reset_soft(o: &Options) -> Result<(), Failure> {
    println!("Soft reset: end every login session and stop the services. Credentials are kept.");
    println!("- {}", end_sessions(&o.runtime));
    stop_services(o);
    println!(
        "- services stopped (start them again with `blackroom setup` or `systemctl --user start remote-hostd.service`)"
    );
    Ok(())
}

fn reset_security(o: &Options) -> Result<(), Failure> {
    println!(
        "Security reset for {}: new authenticator, new key, new recovery codes; every trusted browser and session is dropped.",
        o.account
    );
    let (directory, store) = open_store(&o.state)?;
    // Remote login is closed and every session ended before anything changes, and it re-opens only when every step worked:
    // a half-finished reset must not leave a login that accepts part of the old secrets and part of the new ones.
    let was_disabled = remote_switch::is_disabled(&store);
    println!(
        "0. {}",
        security::disable(&directory, &o.runtime, Some("security reset")).map_err(refuse)?
    );
    println!("Old secrets stop working now.");
    if let Err(failure) = replace_secrets(o, &directory, &store) {
        println!(
            "STOPPED half-way: remote access stays DISABLED. Run `blackroom reset security` again, or `blackroom disable`/`enable` once you have checked."
        );
        return Err(failure);
    }
    if o.system && unit_active(HOSTD_UNIT) {
        // hostd reads the authenticator file at start: it must pick up the new secret.
        systemctl(&["restart", HOSTD_UNIT]);
        println!("5. login authority restarted");
    }
    if was_disabled {
        println!("Remote access was already disabled before this reset and stays disabled.");
    } else {
        println!(
            "6. {}",
            security::enable(&directory, &o.runtime).map_err(refuse)?
        );
    }
    Ok(())
}

fn replace_secrets(o: &Options, directory: &File, store: &SecretStore) -> Result<(), Failure> {
    totp::remove(directory, &o.account).map_err(refuse)?;
    let secret = totp::enroll(directory, &o.account).map_err(refuse)?;
    println!("1. authenticator:");
    print_authenticator(&o.account, secret.as_str());
    let key =
        access_key::rotate(store, &o.account).map_err(|e| refuse(security::store_error(e)))?;
    println!("2. remote access key:");
    print_key(&o.account, key.as_str());
    let codes = recovery_codes::generate(store, &o.account)
        .map_err(|e| refuse(security::store_error(e)))?;
    println!("3.");
    print_codes(&codes);
    let devices = trusted_devices::revoke_all(store, &o.account)
        .map_err(|e| refuse(security::store_error(e)))?;
    println!(
        "4. {devices} trusted device(s) revoked; {}",
        end_sessions(&o.runtime)
    );
    Ok(())
}

fn confirmed(o: &Options) -> Result<(), Failure> {
    if o.yes {
        return Ok(());
    }
    if !rustix::termios::isatty(io::stdin()) {
        return Err((
            2,
            "this deletes every credential: run it in a terminal, or pass --yes".into(),
        ));
    }
    let answer = security::prompt("This deletes your authenticator, keys, trusted devices and audit log. Type RESET to continue: ", true)
        .map_err(refuse)?;
    if answer.as_str() == "RESET" {
        Ok(())
    } else {
        Err((2, "not confirmed: nothing was changed".into()))
    }
}

fn reset_full(o: &Options) -> Result<(), Failure> {
    println!("Full reset: forget every credential and the host identity of this install.");
    for latch in LATCHES {
        if std::fs::symlink_metadata(o.state.join(latch)).is_ok() {
            return Err((
                1,
                format!(
                    "refused: the {latch} marker is set; recover locally first (docs/ops/emergency-recovery.md), a reset must not hide it"
                ),
            ));
        }
    }
    confirmed(o)?;
    if let Ok(directory) = open_state_directory(&o.state) {
        let _ = security::disable(&directory, &o.runtime, Some("full reset"));
    }
    stop_services(o);
    let mut removed = 0;
    for name in OWNED_FILES {
        match std::fs::remove_file(o.state.join(name)) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(refuse(format!("{name}: {error}"))),
        }
    }
    let emptied = std::fs::remove_dir(&o.state).is_ok();
    println!(
        "- removed {removed} file(s){}",
        if emptied {
            "; the empty state directory is gone too".to_string()
        } else {
            format!(
                "; {} still holds files that are not Blackroom's and was left alone",
                o.state.display()
            )
        }
    );
    println!(
        "Start again with `blackroom setup`. The package itself stays installed (apt remove blackroom-console)."
    );
    Ok(())
}

// ---- repair ----

/// A restore timer (`blackroom-console-wd-*`) is the last thing that puts a dead console's display back and locks the
/// laptop: it stays while the console's recovery marker says the display was never verified as restored.
fn keeps_restore_timer(unit: &str, recovery_pending: bool) -> bool {
    recovery_pending && unit.starts_with("blackroom-console-wd-")
}

fn recovery_pending() -> bool {
    std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|dir| {
        Path::new(&dir)
            .join("blackroom-console/recovery.json")
            .exists()
    })
}

/// Timers and units a crashed run can leave behind; only with no console running. Each entry says whether `--fix`
/// clears it; a pending restore timer is reported and kept.
fn clear_leftovers(fix: bool) -> Vec<(String, bool)> {
    let mut found = Vec::new();
    if console_running() {
        return found;
    }
    let pending = recovery_pending();
    let listed = command(
        "systemctl",
        &[
            "--user",
            "--no-pager",
            "--no-legend",
            "list-units",
            "--all",
            "--plain",
            "blackroom-live-kill*",
            "blackroom-console-wd-*",
        ],
    )
    .map(|(_, text)| text)
    .unwrap_or_default();
    for line in listed.lines() {
        if let Some(unit) = line
            .split_whitespace()
            .next()
            .filter(|name| name.starts_with("blackroom-"))
        {
            if keeps_restore_timer(unit, pending) {
                found.push((
                    format!(
                        "restore timer {unit} kept: the display was not verified as restored (start the console, which restores it, or wait for the timer)"
                    ),
                    false,
                ));
                continue;
            }
            found.push((format!("left-over unit {unit}"), true));
            if fix {
                systemctl(&["stop", unit]);
                systemctl(&["reset-failed", unit]);
            }
        }
    }
    if command(
        "systemctl",
        &[
            "--user",
            "--no-pager",
            "is-enabled",
            "gnome-remote-desktop.service",
        ],
    )
    .is_some_and(|(_, text)| text.trim() == "masked")
    {
        found.push(("gnome-remote-desktop.service is masked".into(), true));
        if fix {
            systemctl(&["unmask", "gnome-remote-desktop.service"]);
        }
    }
    found
}

const INSTALLED_CLI: &str = "/usr/bin/blackroom";

/// The first `blackroom` on `path` when it is not the installed one, so an older copy earlier in `PATH` is noticed.
fn shadowing_cli(path: &std::ffi::OsStr, installed: &Path) -> Option<PathBuf> {
    let installed = installed.canonicalize().ok()?;
    let first = std::env::split_paths(path)
        .map(|dir| dir.join("blackroom"))
        .find(|candidate| candidate.is_file())?;
    (first.canonicalize().ok()? != installed).then_some(first)
}

/// The plain-http address of the unit's last `ExecStart=` when https is on and that address is not on this laptop only.
fn plain_http_on_the_network(unit_text: &str) -> Option<String> {
    let exec = unit_text
        .lines()
        .rev()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("ExecStart="))?;
    let words: Vec<&str> = exec.split_whitespace().collect();
    let value = |flag: &str| {
        words.iter().enumerate().find_map(|(i, word)| {
            word.strip_prefix(&format!("{flag}="))
                .or_else(|| (*word == flag).then(|| words.get(i + 1).copied()).flatten())
        })
    };
    value("--tls-listen")?;
    let listen = value("--listen")?;
    let host = listen.rsplit_once(':').map_or(listen, |(host, _)| host);
    let loopback = host == "localhost"
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    (!loopback).then(|| listen.to_string())
}

fn repair(o: &Options) -> Result<(), Failure> {
    println!(
        "Repair{}:",
        if o.fix {
            " (fixing what is safe)"
        } else {
            " (read-only; add --fix to apply the safe fixes)"
        }
    );
    let uid = rustix::process::getuid().as_raw();
    let open_issues = std::cell::Cell::new(0_usize);
    let note = |fixed: bool, text: String| {
        if fixed {
            println!("FIXED {text}");
        } else {
            open_issues.set(open_issues.get() + 1);
            println!("TODO  {text}");
        }
    };
    match std::fs::symlink_metadata(&o.state) {
        Ok(metadata) if metadata.is_dir() && metadata.uid() == uid => {
            if metadata.mode() & 0o077 != 0 {
                let fixed = o.fix
                    && std::fs::set_permissions(&o.state, std::fs::Permissions::from_mode(0o700))
                        .is_ok();
                note(fixed, "state directory was readable by others".into());
            }
            if let Ok(entries) = std::fs::read_dir(&o.state) {
                for entry in entries.flatten() {
                    let Ok(meta) = entry.metadata() else { continue };
                    if meta.is_file() && meta.mode() & 0o077 != 0 {
                        let fixed = o.fix
                            && std::fs::set_permissions(
                                entry.path(),
                                std::fs::Permissions::from_mode(0o600),
                            )
                            .is_ok();
                        note(
                            fixed,
                            format!(
                                "{} was readable by others",
                                entry.file_name().to_string_lossy()
                            ),
                        );
                    }
                }
            }
            for latch in LATCHES {
                if std::fs::symlink_metadata(o.state.join(latch)).is_ok() {
                    println!(
                        "WARN  the {latch} marker is set: this is a safety latch, never cleared automatically; recover locally (docs/ops/emergency-recovery.md)"
                    );
                    open_issues.set(open_issues.get() + 1);
                }
            }
        }
        Ok(_) => note(
            false,
            "the state directory is not a directory of yours".into(),
        ),
        Err(_) => note(
            false,
            format!(
                "no state directory at {}: run `blackroom setup`",
                o.state.display()
            ),
        ),
    }

    // A socket left by a hostd that died: nothing answers on it.
    for name in [AUTH_SOCKET, ADMIN_SOCKET] {
        let path = o.runtime.join(name);
        if path.exists() && std::os::unix::net::UnixStream::connect(&path).is_err() {
            let fixed = o.fix && std::fs::remove_file(&path).is_ok();
            note(
                fixed,
                format!("stale socket {name} (hostd is not listening)"),
            );
        }
    }

    if o.system {
        for unit in std::iter::once(HOSTD_UNIT).chain(CONSOLE_UNITS) {
            let failed = command(
                "systemctl",
                &["--user", "--no-pager", "is-failed", "--quiet", unit],
            )
            .is_some_and(|(ok, _)| ok);
            if failed {
                let fixed = o.fix && systemctl(&["reset-failed", unit]);
                note(fixed, format!("{unit} is in the failed state"));
            }
        }
        for (leftover, clearable) in clear_leftovers(o.fix) {
            note(o.fix && clearable, leftover);
        }
        if !unit_active(HOSTD_UNIT) {
            println!(
                "INFO  {HOSTD_UNIT} is not running (systemctl --user start {HOSTD_UNIT}, or `blackroom setup`)"
            );
        }
        let recovery = std::env::var_os("XDG_RUNTIME_DIR")
            .map(|dir| Path::new(&dir).join("blackroom-console/recovery.json"));
        if recovery.is_some_and(|path| path.exists()) {
            println!(
                "INFO  a display recovery file exists: the next console start restores the display from it"
            );
        }
        if let Some(first) =
            std::env::var_os("PATH").and_then(|path| shadowing_cli(&path, Path::new(INSTALLED_CLI)))
        {
            note(
                false,
                format!(
                    "{} comes first in PATH and hides the installed {INSTALLED_CLI} (an older copy may lack newer commands): remove it or put /usr/bin first",
                    first.display()
                ),
            );
        }
        if let Some(text) = command(
            "systemctl",
            &["--user", "--no-pager", "cat", CONSOLE_UNITS[0]],
        )
        .map(|(_, text)| text)
            && let Some(listen) = plain_http_on_the_network(&text)
        {
            note(
                false,
                format!(
                    "{} starts the plain-http listener on {listen} while https is on: anyone on the network can reach plain http. The packaged unit uses 127.0.0.1:8080",
                    CONSOLE_UNITS[0]
                ),
            );
        }
        println!(
            "INFO  input access is not checked here (it needs root): `sudo blackroom-grant-input status`"
        );
    }
    let open_issues = open_issues.get();
    if open_issues == 0 {
        println!("Nothing needs repair.");
        Ok(())
    } else {
        Err((1, format!("{open_issues} item(s) need attention")))
    }
}

/// `blackroom setup|reset|repair ...`; `None` when the first argument is not one of them.
pub fn dispatch(args: &[OsString]) -> Option<Result<(), Failure>> {
    let (verb, rest) = args.split_first()?;
    let verb = verb
        .to_str()
        .filter(|v| matches!(*v, "setup" | "reset" | "repair"))?;
    Some(match parse(verb, rest) {
        Ok(options) => run(&options),
        Err(message) => Err((2, message)),
    })
}

#[cfg(test)]
mod tests {
    use super::{keeps_restore_timer, plain_http_on_the_network, shadowing_cli};

    #[test]
    fn an_earlier_blackroom_in_the_path_is_named_and_the_installed_one_first_is_not() {
        let old = tempfile::tempdir().unwrap();
        let installed_dir = tempfile::tempdir().unwrap();
        std::fs::write(old.path().join("blackroom"), "x").unwrap();
        std::fs::write(installed_dir.path().join("blackroom"), "y").unwrap();
        let installed = installed_dir.path().join("blackroom");
        let path = std::env::join_paths([old.path(), installed_dir.path()]).unwrap();
        assert_eq!(
            shadowing_cli(&path, &installed),
            Some(old.path().join("blackroom"))
        );
        let path = std::env::join_paths([installed_dir.path(), old.path()]).unwrap();
        assert_eq!(shadowing_cli(&path, &installed), None);
    }

    #[test]
    fn plain_http_on_all_addresses_with_https_on_is_reported() {
        let unit = |exec: &str| {
            format!("[Service]\nExecStart=/usr/lib/blackroom/blackroom-console {exec}\n")
        };
        assert_eq!(
            plain_http_on_the_network(&unit("--listen 0.0.0.0:8080 --tls-listen 0.0.0.0:8443"))
                .as_deref(),
            Some("0.0.0.0:8080")
        );
        assert_eq!(
            plain_http_on_the_network(&unit("--listen=0.0.0.0:8080 --tls-listen=0.0.0.0:8443"))
                .as_deref(),
            Some("0.0.0.0:8080")
        );
        assert_eq!(
            plain_http_on_the_network(&unit("--listen 127.0.0.1:8080 --tls-listen 0.0.0.0:8443")),
            None
        );
        assert_eq!(
            plain_http_on_the_network(&unit("--listen [::1]:8080 --tls-listen 0.0.0.0:8443")),
            None
        );
        assert_eq!(
            plain_http_on_the_network(&unit("--listen 0.0.0.0:8080")),
            None,
            "https off: nothing to compare"
        );
    }

    #[test]
    fn a_restore_timer_is_kept_only_while_recovery_is_pending() {
        assert!(keeps_restore_timer("blackroom-console-wd-1790.timer", true));
        assert!(!keeps_restore_timer(
            "blackroom-console-wd-1790.timer",
            false
        ));
        assert!(!keeps_restore_timer("blackroom-live-kill.timer", true));
    }
}
