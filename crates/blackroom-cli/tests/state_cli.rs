use std::fs::File;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output};

use remote_hostd::audit::{AuditEvent, AuditLog, RevokeCause};
use remote_hostd::store::PersistentHostAuthority;

fn state_dir() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

fn initialised() -> tempfile::TempDir {
    let directory = state_dir();
    let fd = File::open(directory.path()).unwrap();
    drop(PersistentHostAuthority::open(&fd).unwrap());
    directory
}

fn blackroom(state: &Path, verb: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_blackroom"))
        .arg("--state-dir")
        .arg(state)
        .args(verb)
        .output()
        .unwrap()
}

fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[test]
fn status_reports_epoch_and_markers_without_touching_state() {
    let directory = initialised();
    let before = names(directory.path());
    let output = blackroom(directory.path(), &["status"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["mode"], "OFFLINE_SIMULATION");
    assert_eq!(status["epoch"], 0);
    assert_eq!(status["emergency_pending"], false);
    assert_eq!(status["recovery_pending"], false);
    assert_eq!(names(directory.path()), before);

    let fd = File::open(directory.path()).unwrap();
    let mut host = PersistentHostAuthority::open(&fd).unwrap();
    host.start().unwrap();
    let held = blackroom(directory.path(), &["status"]);
    assert!(held.status.success(), "status must not need the host lock");
    drop(host);
    PersistentHostAuthority::emergency_stop(&fd).unwrap();
    let stopped: serde_json::Value =
        serde_json::from_slice(&blackroom(directory.path(), &["status"]).stdout).unwrap();
    assert_eq!(stopped["emergency_pending"], true);
    assert_eq!(stopped["recovery_pending"], true);
    assert!(stopped["epoch"].as_u64().unwrap() > 0);
}

#[test]
fn unsafe_or_uninitialised_directories_are_refused_read_only() {
    let empty = state_dir();
    let output = blackroom(empty.path(), &["status"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(names(empty.path()).is_empty(), "no file may be created");

    let real = initialised();
    let holder = state_dir();
    let link = holder.path().join("link");
    symlink(real.path(), &link).unwrap();
    assert_eq!(blackroom(&link, &["status"]).status.code(), Some(1));

    let relative = Command::new(env!("CARGO_BIN_EXE_blackroom"))
        .args(["--state-dir", "relative", "status"])
        .output()
        .unwrap();
    assert_eq!(relative.status.code(), Some(1));

    let open = initialised();
    std::fs::set_permissions(open.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(blackroom(open.path(), &["status"]).status.code(), Some(1));
    for arguments in [
        vec!["--state-dir"],
        vec!["--state-dir", "/tmp", "logs", "--tail", "0"],
        vec!["--state-dir", "/tmp", "logs", "--tail", "1001"],
        vec!["--state-dir", "/tmp", "unknown"],
        vec!["status"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_blackroom"))
            .args(&arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
    }
}

#[test]
fn logs_print_the_requested_tail_and_reject_a_corrupt_log() {
    let directory = initialised();
    let fd = File::open(directory.path()).unwrap();
    let mut log = AuditLog::open(&fd).unwrap();
    for epoch in 1..=5 {
        log.record(&AuditEvent::GrantRevoked {
            epoch,
            cause: RevokeCause::Revoked,
        })
        .unwrap();
    }
    let output = blackroom(directory.path(), &["logs", "--tail", "2"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let lines: Vec<serde_json::Value> = text(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["epoch"], 4);
    assert_eq!(lines[1]["epoch"], 5);
    assert_eq!(
        text(&blackroom(directory.path(), &["logs"]).stdout)
            .lines()
            .count(),
        5
    );

    std::fs::OpenOptions::new()
        .append(true)
        .open(directory.path().join("audit.log"))
        .unwrap()
        .write_all(b"not json\n{\"no\":\"event\"}\n")
        .unwrap();
    let corrupt = blackroom(directory.path(), &["logs"]);
    assert_eq!(corrupt.status.code(), Some(1));
    assert!(text(&corrupt.stderr).contains("2 invalid"));
    assert_eq!(
        text(&corrupt.stdout).lines().count(),
        5,
        "valid lines still print"
    );

    std::fs::set_permissions(
        directory.path().join("audit.log"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(
        blackroom(directory.path(), &["logs"]).status.code(),
        Some(1)
    );
}

#[test]
fn doctor_reports_sound_state_and_fails_on_loose_permissions() {
    let directory = initialised();
    let fd = File::open(directory.path()).unwrap();
    AuditLog::open(&fd)
        .unwrap()
        .record(&AuditEvent::HostStarted { epoch: 0 })
        .unwrap();
    let before = names(directory.path());
    let sound = blackroom(directory.path(), &["doctor"]);
    assert!(sound.status.success(), "{}", text(&sound.stdout));
    assert!(
        text(&sound.stdout)
            .lines()
            .all(|line| line.starts_with("OK  "))
    );
    assert_eq!(names(directory.path()), before);

    let key = directory.path().join("host-identity.key");
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o640)).unwrap();
    let loose = blackroom(directory.path(), &["doctor"]);
    assert_eq!(loose.status.code(), Some(1));
    assert!(text(&loose.stdout).contains("FAIL host-identity.key"));
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();

    PersistentHostAuthority::emergency_stop(&fd).unwrap();
    let stopped = blackroom(directory.path(), &["doctor"]);
    assert!(stopped.status.success());
    assert!(text(&stopped.stdout).contains("WARN emergency stop persisted"));

    let fresh = state_dir();
    let uninitialised = blackroom(fresh.path(), &["doctor"]);
    assert!(uninitialised.status.success());
    assert!(text(&uninitialised.stdout).contains("WARN host-identity.key missing"));
}
