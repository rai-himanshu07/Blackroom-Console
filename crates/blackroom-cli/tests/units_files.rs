//! The packaged user units must agree with each other: a chord's stop marker only reaches the login authority
//! when both name the same state directory.

use std::path::PathBuf;

fn unit(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/units")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The value after `flag` on the unit's `ExecStart=` line.
fn flag_value(unit: &str, flag: &str) -> Option<String> {
    let line = unit.lines().find(|l| l.starts_with("ExecStart="))?;
    let mut words = line.split_whitespace();
    words.find(|w| *w == flag)?;
    words.next().map(str::to_owned)
}

#[test]
fn the_grab_daemon_shares_the_login_authoritys_state_directory() {
    let hostd = flag_value(&unit("remote-hostd.service"), "--state-dir").expect("hostd state dir");
    let daemon = flag_value(&unit("blackroom-console-emergencyd.service"), "--state-dir").expect(
        "the packaged grab daemon must carry --state-dir (R8: the chord's durable stop marker)",
    );
    assert_eq!(daemon, hostd);
}

#[test]
fn the_grab_daemon_does_not_lock_on_emergency_until_a_live_run_has_shown_it_safe() {
    let daemon = unit("blackroom-console-emergencyd.service");
    let exec = daemon
        .lines()
        .find(|l| l.starts_with("ExecStart="))
        .unwrap();
    assert!(!exec.contains("--lock-on-emergency"), "{exec}");
}
