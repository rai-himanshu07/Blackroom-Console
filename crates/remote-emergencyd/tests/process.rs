//! Process-level checks of the real binary against an empty input directory: nothing here opens
//! a real input device, and nothing is grabbed.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use remote_emergencyd::client::{Client, Outcome};

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(dir: &Path, extra: &[&str]) -> (Daemon, std::path::PathBuf) {
    let input = dir.join("input");
    std::fs::create_dir_all(&input).expect("input dir");
    let socket = dir.join("emergency.sock");
    let child = Command::new(env!("CARGO_BIN_EXE_remote-emergencyd"))
        .args(["--socket", socket.to_str().expect("utf8")])
        .args([
            "--client-uid",
            &rustix::process::getuid().as_raw().to_string(),
        ])
        .args(["--input-dir", input.to_str().expect("utf8")])
        .args(extra)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !socket.exists() {
        assert!(
            Instant::now() < deadline,
            "the daemon never created its socket"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    (Daemon(child), socket)
}

#[test]
fn by_default_the_daemon_serves_status_but_never_grabs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_daemon, socket) = start(dir.path(), &[]);
    let mode = std::fs::metadata(&socket)
        .expect("socket")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "the control socket is private");
    let mut client = Client::connect(&socket, Duration::from_secs(5)).expect("connect");
    let status = client.status().expect("status");
    assert_eq!(status.phase.as_deref(), Some("idle"));
    assert_eq!(status.grabs_enabled, Some(false));
    assert_eq!(
        client.isolate(5_000).expect("isolate"),
        Outcome::Error("grabs_disabled".to_string())
    );
}

#[test]
fn with_grabs_enabled_an_empty_input_directory_leaves_nothing_to_grab() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_daemon, socket) = start(dir.path(), &["--enable-grabs"]);
    let mut client = Client::connect(&socket, Duration::from_secs(10)).expect("connect");
    assert_eq!(client.status().expect("status").grabs_enabled, Some(true));
    assert_eq!(
        client.isolate(5_000).expect("isolate"),
        Outcome::Refused("nothing_to_grab".to_string())
    );
    assert_eq!(client.status().expect("status").held, Some(0));
}

#[test]
fn a_relative_socket_path_or_a_foreign_file_is_refused() {
    let relative = Command::new(env!("CARGO_BIN_EXE_remote-emergencyd"))
        .args(["--socket", "relative.sock", "--client-uid", "0"])
        .output()
        .expect("run");
    assert!(!relative.status.success());
    assert!(String::from_utf8_lossy(&relative.stderr).contains("absolute"));

    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("not-a-socket");
    std::fs::write(&file, b"keep me").expect("file");
    let refused = Command::new(env!("CARGO_BIN_EXE_remote-emergencyd"))
        .args([
            "--socket",
            file.to_str().expect("utf8"),
            "--client-uid",
            "0",
        ])
        .output()
        .expect("run");
    assert!(!refused.status.success());
    assert_eq!(std::fs::read(&file).expect("still there"), b"keep me");
}
