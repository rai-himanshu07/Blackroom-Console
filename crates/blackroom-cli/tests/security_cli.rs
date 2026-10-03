//! Security verbs of the operator CLI, driven against a real state directory and, for the live
//! verbs, a real in-process auth service on temporary sockets (a fake password check; no PAM).

use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use blackroom_store::SecretStore;
use remote_hostd::authd::{ADMIN_SOCKET, AUTH_SOCKET, AuthDaemon, OwnerOnly, bind_private, serve};
use remote_hostd::login::MultiFactorVerifier;
use remote_hostd::password::{PasswordCheck, PasswordOutcome};
use remote_hostd::ratelimit::FailureLimiter;
use remote_hostd::totp::{self, Limits, base32_decode, totp as code_at};
use remote_hostd::{access_key, recovery_codes, trusted_devices};

fn private() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

fn blackroom(state: &Path, runtime: &Path, verb: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_blackroom"))
        .arg("--state-dir")
        .arg(state)
        .arg("--runtime-dir")
        .arg(runtime)
        .args(verb)
        .output()
        .unwrap()
}

fn out(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn err(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn initialised() -> (tempfile::TempDir, tempfile::TempDir) {
    let (state, runtime) = (private(), private());
    let fd = File::open(state.path()).unwrap();
    drop(remote_hostd::store::PersistentHostAuthority::open(&fd).unwrap());
    (state, runtime)
}

#[test]
fn disable_and_enable_work_without_hostd_and_status_shows_the_switch() {
    let (state, runtime) = initialised();
    let status: serde_json::Value =
        serde_json::from_slice(&blackroom(state.path(), runtime.path(), &["status"]).stdout)
            .unwrap();
    assert_eq!(status["remote_access"], "enabled");

    let disabled = blackroom(
        state.path(),
        runtime.path(),
        &["disable", "--reason", "travelling"],
    );
    assert!(disabled.status.success(), "{}", err(&disabled));
    assert!(out(&disabled).contains("REMOTE ACCESS DISABLED"));
    assert!(out(&disabled).contains("hostd is not running"));
    let status: serde_json::Value =
        serde_json::from_slice(&blackroom(state.path(), runtime.path(), &["status"]).stdout)
            .unwrap();
    assert_eq!(status["remote_access"], "disabled");

    let enabled = blackroom(state.path(), runtime.path(), &["enable"]);
    assert!(enabled.status.success(), "{}", err(&enabled));
    assert!(out(&enabled).contains("enabled"));
    let status: serde_json::Value =
        serde_json::from_slice(&blackroom(state.path(), runtime.path(), &["status"]).stdout)
            .unwrap();
    assert_eq!(status["remote_access"], "enabled");
}

#[test]
fn live_verbs_say_plainly_that_hostd_is_not_running() {
    let (state, runtime) = initialised();
    for verb in [
        vec!["sessions"],
        vec!["revoke-all"],
        vec!["revoke-session", "0123456789abcdef"],
    ] {
        let output = blackroom(state.path(), runtime.path(), &verb);
        assert_eq!(output.status.code(), Some(1), "{verb:?}");
        assert!(
            err(&output).contains("hostd is not running"),
            "{}",
            err(&output)
        );
    }
    let bad = blackroom(
        state.path(),
        runtime.path(),
        &["revoke-session", "not-an-id"],
    );
    assert_eq!(bad.status.code(), Some(1));
}

#[test]
fn credential_verbs_print_a_secret_once_and_never_store_or_list_it() {
    let (state, runtime) = initialised();
    let rotated = blackroom(
        state.path(),
        runtime.path(),
        &["rotate-key", "--account", "owner"],
    );
    assert!(rotated.status.success(), "{}", err(&rotated));
    let key = out(&rotated)
        .lines()
        .find_map(|line| line.strip_prefix("remote access key: "))
        .unwrap()
        .to_string();
    assert_eq!(key.len(), 43);
    assert!(!err(&rotated).contains(&key));
    let file = std::fs::read_to_string(state.path().join("access-keys")).unwrap();
    assert!(!file.contains(&key));

    let codes = blackroom(
        state.path(),
        runtime.path(),
        &["recovery-codes", "--account", "owner"],
    );
    assert!(codes.status.success(), "{}", err(&codes));
    let shown: Vec<String> = out(&codes)
        .lines()
        .filter(|line| line.len() == 11 && line.contains('-'))
        .map(str::to_string)
        .collect();
    assert_eq!(shown.len(), 10);

    let store = SecretStore::open(&File::open(state.path()).unwrap()).unwrap();
    let (id, secret) = trusted_devices::register(&store, "owner", "Laptop A").unwrap();
    let listed = blackroom(
        state.path(),
        runtime.path(),
        &["devices", "--account", "owner"],
    );
    assert!(out(&listed).contains(&id) && out(&listed).contains("trusted"));
    assert!(!out(&listed).contains(secret.as_str()));
    let revoked = blackroom(state.path(), runtime.path(), &["revoke-device", &id]);
    assert!(revoked.status.success(), "{}", err(&revoked));
    assert!(
        out(&blackroom(
            state.path(),
            runtime.path(),
            &["devices", "--account", "owner"]
        ))
        .contains("REVOKED")
    );
    let again = blackroom(state.path(), runtime.path(), &["revoke-device", &id]);
    assert_eq!(again.status.code(), Some(1));

    let rotated_all = blackroom(
        state.path(),
        runtime.path(),
        &["rotate-key", "--account", "owner", "--revoke-devices"],
    );
    assert!(rotated_all.status.success(), "{}", err(&rotated_all));
    assert!(out(&rotated_all).contains("revoked 0 trusted device(s)"));
    assert!(
        blackroom(
            state.path(),
            runtime.path(),
            &["rotate-key", "--account", "../x"]
        )
        .status
        .code()
            == Some(1)
    );
    assert_eq!(
        blackroom(state.path(), runtime.path(), &["rotate-key"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn diagnostics_and_compatibility_are_json_without_secrets() {
    let (state, runtime) = initialised();
    totp::enroll(&File::open(state.path()).unwrap(), "owner").unwrap();
    let key = out(&blackroom(
        state.path(),
        runtime.path(),
        &["rotate-key", "--account", "owner"],
    ));
    let codes = out(&blackroom(
        state.path(),
        runtime.path(),
        &["recovery-codes", "--account", "owner"],
    ));
    let output = blackroom(state.path(), runtime.path(), &["diagnostics"]);
    assert!(output.status.success(), "{}", err(&output));
    let text = out(&output);
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["remote_access"], "enabled");
    assert_eq!(report["credentials"][0]["account"], "owner");
    assert_eq!(report["credentials"][0]["access_key"], true);
    assert_eq!(report["credentials"][0]["recovery_codes_left"], 10);
    let secret_key = key
        .lines()
        .find_map(|l| l.strip_prefix("remote access key: "))
        .unwrap();
    assert!(!text.contains(secret_key));
    for code in codes.lines().filter(|line| line.len() == 11) {
        assert!(!text.contains(code) && !text.contains(&code.replace('-', "")));
    }
    let stored = std::fs::read_to_string(state.path().join("totp-credentials")).unwrap();
    let totp_secret = stored
        .split('"')
        .find(|part| {
            part.len() == 32
                && part
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b))
        })
        .unwrap();
    assert!(!text.contains(totp_secret));

    let compat: serde_json::Value =
        serde_json::from_slice(&blackroom(state.path(), runtime.path(), &["compatibility"]).stdout)
            .unwrap();
    assert!(compat.get("pam_service_installed").is_some());
}

struct Fake;

impl PasswordCheck for Fake {
    fn check(&mut self, _: &str, password: &str) -> PasswordOutcome {
        if password == "pw" {
            PasswordOutcome::Accepted
        } else {
            PasswordOutcome::Rejected
        }
    }
}

#[test]
fn live_verbs_list_and_revoke_sessions_through_the_admin_socket() {
    let (state, runtime) = initialised();
    let fd = File::open(state.path()).unwrap();
    let secret = base32_decode(&totp::enroll(&fd, "owner").unwrap()).unwrap();
    let store = SecretStore::open(&fd).unwrap();
    let key = access_key::rotate(&store, "owner").unwrap();
    recovery_codes::generate(&store, "owner").unwrap();
    let verifier = MultiFactorVerifier::new(
        SecretStore::open(&fd).unwrap(),
        totp::load_verifier(&fd, Limits::default()).unwrap(),
        None,
        FailureLimiter::new(Limits::default()),
    );
    let daemon = Arc::new(AuthDaemon::new(&fd, verifier, Box::new(OwnerOnly)).unwrap());
    let (auth, admin) = (
        bind_private(runtime.path(), AUTH_SOCKET).unwrap(),
        bind_private(runtime.path(), ADMIN_SOCKET).unwrap(),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let server = {
        let (daemon, stop) = (Arc::clone(&daemon), Arc::clone(&stop));
        std::thread::spawn(move || {
            let pam: remote_hostd::authd::PamFactory =
                std::sync::Arc::new(|| Box::new(Fake) as Box<dyn PasswordCheck>);
            serve(&daemon, auth, admin, &pam, &stop)
        })
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let login: remote_hostd::authd::AuthReply = remote_hostd::authd::call(
        &runtime.path().join(AUTH_SOCKET),
        &serde_json::json!({
            "command": "login", "account": "owner", "client_id": "cli-test", "password": "pw",
            "totp": code_at(&secret, now), "access_key": key.as_str(),
        }),
    )
    .unwrap();
    assert!(login.ok);
    let token = login.token.unwrap();

    let listed = blackroom(state.path(), runtime.path(), &["sessions"]);
    assert!(listed.status.success(), "{}", err(&listed));
    assert!(out(&listed).contains("1 session(s)"));
    assert!(out(&listed).contains("owner\tcli-test"));
    assert!(
        !out(&listed).contains(token.as_str()),
        "the listing never shows the bearer token"
    );
    let id = out(&listed)
        .lines()
        .nth(1)
        .unwrap()
        .split('\t')
        .next()
        .unwrap()
        .to_string();

    let revoked = blackroom(state.path(), runtime.path(), &["revoke-session", &id]);
    assert!(
        out(&revoked).contains("revoked 1 session(s)"),
        "{}{}",
        out(&revoked),
        err(&revoked)
    );
    assert!(out(&blackroom(state.path(), runtime.path(), &["sessions"])).contains("0 session(s)"));

    let all = blackroom(state.path(), runtime.path(), &["revoke-all"]);
    assert!(all.status.success(), "{}", err(&all));
    assert!(out(&all).contains("security epoch is now"));
    let disabled = blackroom(state.path(), runtime.path(), &["disable"]);
    assert!(
        out(&disabled).contains("REMOTE ACCESS DISABLED (0 session(s) ended)"),
        "{}",
        out(&disabled)
    );
    assert!(
        blackroom(state.path(), runtime.path(), &["enable"])
            .status
            .success()
    );
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap().unwrap();
}
