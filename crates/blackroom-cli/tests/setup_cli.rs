//! `blackroom setup`, `reset` and `repair` against temporary state and runtime directories, with the
//! system steps (systemctl, PAM, installed files) switched off by `--no-system`.

use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Output};

use blackroom_store::SecretStore;
use remote_hostd::{access_key, recovery_codes, totp};

fn run(state: &Path, runtime: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_blackroom"))
        .args(args)
        .args(["--no-system", "--account", "tester", "--state-dir"])
        .arg(state)
        .arg("--runtime-dir")
        .arg(runtime)
        .output()
        .unwrap()
}

fn out(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn err(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn value_after<'a>(text: &'a str, label: &str) -> &'a str {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(label))
        .unwrap_or_else(|| panic!("no `{label}` in:\n{text}"))
        .trim()
}

fn scratch() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let runtime = root.path().join("run");
    std::fs::create_dir(&runtime).unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    (root, state, runtime)
}

fn store(state: &Path) -> (File, SecretStore) {
    let directory = File::open(state).unwrap();
    let store = SecretStore::open(&directory).unwrap();
    (directory, store)
}

#[test]
fn setup_creates_every_credential_once_and_is_safe_to_rerun() {
    let (_root, state, runtime) = scratch();
    let first = run(&state, &runtime, &["setup"]);
    assert!(first.status.success(), "{}", err(&first));
    let text = out(&first);
    let secret = value_after(&text, "authenticator secret:").to_string();
    let key = value_after(&text, "remote access key:").to_string();
    assert!(text.contains("otpauth://totp/"), "{text}");
    assert!(
        text.contains('█') || text.contains('▀') || text.contains('▄'),
        "a QR code is drawn"
    );
    assert!(text.contains("recovery codes"), "{text}");
    assert_eq!(
        std::fs::metadata(&state).unwrap().permissions().mode() & 0o777,
        0o700
    );

    let (directory, store) = store(&state);
    assert!(access_key::verify(&store, "tester", &key).unwrap());
    assert!(recovery_codes::remaining(&store, "tester").unwrap() > 0);
    assert_eq!(totp::enrolled_accounts(&directory).unwrap(), ["tester"]);
    drop((directory, store));

    // A rerun keeps everything and prints no secret again.
    let second = run(&state, &runtime, &["setup"]);
    assert!(second.status.success(), "{}", err(&second));
    let again = out(&second);
    assert!(again.contains("already enrolled"), "{again}");
    assert!(again.contains("already set"), "{again}");
    assert!(!again.contains(&secret) && !again.contains(&key));
    assert!(
        !again.lines().any(|line| {
            let line = line.trim();
            line.starts_with("authenticator secret:") || line.starts_with("remote access key:")
        }),
        "{again}"
    );
    let (_directory, store) = self::store(&state);
    assert!(access_key::verify(&store, "tester", &key).unwrap());
}

#[test]
fn reset_security_replaces_every_secret() {
    let (_root, state, runtime) = scratch();
    let first = out(&run(&state, &runtime, &["setup"]));
    let old_secret = value_after(&first, "authenticator secret:").to_string();
    let old_key = value_after(&first, "remote access key:").to_string();

    let reset = run(&state, &runtime, &["reset", "security"]);
    assert!(reset.status.success(), "{}", err(&reset));
    let text = out(&reset);
    let new_secret = value_after(&text, "authenticator secret:").to_string();
    let new_key = value_after(&text, "remote access key:").to_string();
    assert_ne!(new_secret, old_secret);
    assert_ne!(new_key, old_key);
    assert!(text.contains("trusted device(s) revoked"), "{text}");

    let (directory, store) = store(&state);
    assert!(!access_key::verify(&store, "tester", &old_key).unwrap());
    assert!(access_key::verify(&store, "tester", &new_key).unwrap());
    assert_eq!(totp::enrolled_accounts(&directory).unwrap(), ["tester"]);
}

#[test]
fn reset_soft_keeps_credentials() {
    let (_root, state, runtime) = scratch();
    let first = out(&run(&state, &runtime, &["setup"]));
    let key = value_after(&first, "remote access key:").to_string();
    let soft = run(&state, &runtime, &["reset", "soft"]);
    assert!(soft.status.success(), "{}", err(&soft));
    let (_directory, store) = store(&state);
    assert!(access_key::verify(&store, "tester", &key).unwrap());
}

#[test]
fn reset_full_needs_confirmation_refuses_a_latch_and_leaves_foreign_files() {
    let (_root, state, runtime) = scratch();
    run(&state, &runtime, &["setup"]);

    // No terminal and no --yes: nothing is deleted.
    let unconfirmed = run(&state, &runtime, &["reset", "full"]);
    assert_eq!(unconfirmed.status.code(), Some(2), "{}", err(&unconfirmed));
    assert!(state.join("totp-credentials").exists());

    // A safety latch blocks the reset even with --yes.
    std::fs::write(state.join("emergency-stop"), [0_u8; 8]).unwrap();
    let latched = run(&state, &runtime, &["reset", "full", "--yes"]);
    assert_eq!(latched.status.code(), Some(1));
    assert!(err(&latched).contains("emergency-stop"));
    assert!(state.join("totp-credentials").exists());
    std::fs::remove_file(state.join("emergency-stop")).unwrap();

    // A file that is not Blackroom's survives, and so does the directory holding it.
    std::fs::write(state.join("mine.txt"), "keep").unwrap();
    let done = run(&state, &runtime, &["reset", "full", "--yes"]);
    assert!(done.status.success(), "{}", err(&done));
    assert!(!state.join("totp-credentials").exists());
    assert!(!state.join("access-keys").exists());
    assert!(!state.join("host-identity.key").exists());
    assert_eq!(
        std::fs::read_to_string(state.join("mine.txt")).unwrap(),
        "keep"
    );
    assert!(out(&done).contains("left alone"));

    // Without foreign files the empty directory goes too, and setup can start over.
    std::fs::remove_file(state.join("mine.txt")).unwrap();
    let again = run(&state, &runtime, &["reset", "full", "--yes"]);
    assert!(again.status.success(), "{}", err(&again));
    assert!(!state.exists());
    let fresh = run(&state, &runtime, &["setup"]);
    assert!(fresh.status.success(), "{}", err(&fresh));
    assert!(out(&fresh).contains("authenticator secret:"));
}

#[test]
fn repair_reports_then_fixes_modes_and_stale_sockets() {
    let (_root, state, runtime) = scratch();
    run(&state, &runtime, &["setup"]);
    let healthy = run(&state, &runtime, &["repair"]);
    assert!(
        healthy.status.success(),
        "{}{}",
        out(&healthy),
        err(&healthy)
    );
    assert!(out(&healthy).contains("Nothing needs repair"));

    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(
        state.join("access-keys"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    drop(UnixListener::bind(runtime.join("auth.sock")).unwrap());
    assert!(runtime.join("auth.sock").exists());

    let report = run(&state, &runtime, &["repair"]);
    assert_eq!(report.status.code(), Some(1), "{}", out(&report));
    let text = out(&report);
    assert!(
        text.contains("TODO") && text.contains("access-keys") && text.contains("stale socket"),
        "{text}"
    );
    assert_eq!(
        std::fs::metadata(&state).unwrap().permissions().mode() & 0o777,
        0o755,
        "read-only without --fix"
    );

    let fixed = run(&state, &runtime, &["repair", "--fix"]);
    assert!(fixed.status.success(), "{}{}", out(&fixed), err(&fixed));
    assert_eq!(
        std::fs::metadata(&state).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(state.join("access-keys"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(!runtime.join("auth.sock").exists());
}

#[test]
fn repair_never_clears_a_latch() {
    let (_root, state, runtime) = scratch();
    run(&state, &runtime, &["setup"]);
    std::fs::write(state.join("recovery-pending"), [0_u8; 8]).unwrap();
    let repaired = run(&state, &runtime, &["repair", "--fix"]);
    assert_eq!(repaired.status.code(), Some(1));
    assert!(out(&repaired).contains("recovery-pending marker is set"));
    assert!(state.join("recovery-pending").exists());
}

#[test]
fn bad_arguments_print_usage() {
    let blackroom = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_blackroom"))
            .args(args)
            .output()
            .unwrap()
    };
    for args in [
        &["reset"][..],
        &["reset", "bogus"],
        &["setup", "--nope"],
        &["setup", "--state-dir"],
    ] {
        let output = blackroom(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(err(&output).contains("usage: blackroom setup"));
    }
}
