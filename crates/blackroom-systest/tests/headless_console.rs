//! System tests of the remote console against a throwaway `--headless` GNOME Shell on a private D-Bus
//! (`docs/ops/headless-repro.sh`); the real screen, session bus and input are never touched.
//! Gated `#[ignore]`: `cargo build -p blackroom-console && BLACKROOM_SYSTEST=1 \
//!  cargo test -p blackroom-systest --test headless_console -- --ignored --test-threads=1`.

use std::path::PathBuf;
use std::process::Command;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs one of the `docs/ops/headless-*.sh` scripts through the headless harness and returns its output.
fn headless(script: &str, extra_env: &[(&str, &str)]) -> String {
    assert!(
        std::env::var_os("BLACKROOM_SYSTEST").is_some(),
        "set BLACKROOM_SYSTEST=1 to run system tests"
    );
    let root = repo();
    assert!(
        root.join("target/debug/blackroom-console").is_file(),
        "build first: cargo build -p blackroom-console"
    );
    let mut command = Command::new(root.join("docs/ops/headless-repro.sh"));
    command
        .current_dir(&root)
        .env("BR_BIN", root.join("docs/ops").join(script))
        .env("BR_TIMEOUT", "400")
        .env("BR_SHELL_TIMEOUT", "420");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let output = command.output().expect("run the headless harness");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_all_ok(output: &str) {
    assert!(
        output.contains("ALL OK"),
        "script did not report ALL OK:\n{output}"
    );
    assert!(
        output.contains("headless_shell=alive"),
        "the headless Shell did not survive:\n{output}"
    );
}

#[test]
#[ignore = "needs a headless Shell: BLACKROOM_SYSTEST=1 cargo test -p blackroom-systest --test headless_console -- --ignored"]
fn start_stop_input_and_heartbeat_loss() {
    assert_all_ok(&headless("headless-console-test.sh", &[]));
}

#[test]
#[ignore = "needs a headless Shell: BLACKROOM_SYSTEST=1 cargo test -p blackroom-systest --test headless_console -- --ignored"]
fn races_and_twenty_cycles_leak_nothing() {
    assert_all_ok(&headless("headless-cycles-test.sh", &[("BR_CYCLES", "20")]));
}

#[test]
#[ignore = "needs a headless Shell: BLACKROOM_SYSTEST=1 cargo test -p blackroom-systest --test headless_console -- --ignored"]
fn sigkilled_consoles_leave_the_shell_and_the_display_usable() {
    assert_all_ok(&headless("headless-faults-test.sh", &[]));
}
