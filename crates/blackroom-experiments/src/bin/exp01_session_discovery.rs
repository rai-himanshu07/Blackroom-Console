//! Experiment 1 — GNOME Session Discovery (Document 10 §8).
//!
//! Read-only: enumerates logind sessions via `org.freedesktop.login1` on the
//! system bus and selects the one Wayland/GNOME/user/active session without
//! guessing from `$DISPLAY` or process names (Document 05 §12-14). Never
//! modifies the system (`ListSessions` and property `Get` calls only).

use std::process::Command;

use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, SessionCandidate, current_uid, discover,
    evidence_dir, redact, render_rationale, write_evidence,
};
use clap::Parser;
use serde::Serialize;
use time::OffsetDateTime;

const EXP_ID: &str = "exp01";
/// Set on the child process spawned by `--self-test` so it computes the same
/// result but never writes evidence (avoids clobbering the parent's report).
const SUPPRESS_EVIDENCE_ENV: &str = "BLACKROOM_EXP01_SUPPRESS_EVIDENCE";

#[derive(Parser, Debug)]
#[command(about = "Experiment 1: read-only GNOME session discovery")]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Also spawn a child process with `WAYLAND_DISPLAY` removed and confirm
    /// it fails closed (Document 10 §8 negative test).
    #[arg(long, default_value_t = false)]
    self_test: bool,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Ok = 0,
    SessionNotFound = 2,
    WaylandUnavailable = 3,
}

#[derive(Debug, Serialize)]
struct EnvAssertions {
    xdg_session_type: Option<String>,
    wayland_display_set: bool,
    dbus_session_bus_address_set: bool,
    all_asserted: bool,
}

#[derive(Debug, Serialize)]
struct NegativeTestResult {
    expected_one_of: Vec<String>,
    child_exit_code: Option<i32>,
    passed: bool,
}

#[derive(Debug, Serialize)]
struct SessionDiscoveryReport {
    current_uid: u32,
    candidates: Vec<SessionCandidate>,
    selected_session_id: Option<String>,
    env_assertions: EnvAssertions,
    gnome_session_units: Option<String>,
    outcome: String,
    negative_test: Option<NegativeTestResult>,
}

fn command_output_any_status(cmd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(cmd).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    let redact_on = args.common.redact_enabled();
    let suppress_evidence = std::env::var_os(SUPPRESS_EVIDENCE_ENV).is_some();
    let now = OffsetDateTime::now_utc();

    let uid = current_uid()?;
    let (mut candidates, selected_id) = discover(uid)?;
    // Redact at the source so both `session.json` and the rendered report
    // stay clean; report-time redaction alone would leave the raw name in
    // the JSON sidecar.
    for candidate in &mut candidates {
        if let Some(properties) = &mut candidate.properties {
            properties.name = redact(&properties.name, redact_on);
            properties.display = redact(&properties.display, redact_on);
        }
    }

    let xdg_session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let wayland_display_set = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    let dbus_set = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|v| !v.is_empty());
    let env_ok = xdg_session_type.as_deref() == Some("wayland") && wayland_display_set && dbus_set;

    let outcome = if selected_id.is_none() {
        Outcome::SessionNotFound
    } else if !env_ok {
        Outcome::WaylandUnavailable
    } else {
        Outcome::Ok
    };

    let gnome_session_units = command_output_any_status(
        "systemctl",
        &[
            "--user",
            "list-units",
            "gnome-session*",
            "graphical-session*",
        ],
    );

    let negative_test = if args.self_test {
        let exe = std::env::current_exe()?;
        let child = Command::new(&exe)
            .env_remove("WAYLAND_DISPLAY")
            .env(SUPPRESS_EVIDENCE_ENV, "1")
            .output()?;
        let child_exit_code = child.status.code();
        let passed = child_exit_code == Some(Outcome::WaylandUnavailable as i32)
            || child_exit_code == Some(Outcome::SessionNotFound as i32);
        Some(NegativeTestResult {
            expected_one_of: vec![
                "WAYLAND_UNAVAILABLE(3)".to_string(),
                "SESSION_NOT_FOUND(2)".to_string(),
            ],
            child_exit_code,
            passed,
        })
    } else {
        None
    };

    let rationale = render_rationale(&candidates);
    let observed = redact(
        &format!(
            "current uid: {uid}\n{rationale}\nselected session: {}\n\
             XDG_SESSION_TYPE={:?} WAYLAND_DISPLAY_set={wayland_display_set} \
             DBUS_SESSION_BUS_ADDRESS_set={dbus_set}\n\
             gnome-session/graphical-session user units:\n{}\nOutcome: {outcome:?}{}",
            selected_id.clone().unwrap_or_else(|| "NONE".to_string()),
            xdg_session_type,
            gnome_session_units
                .clone()
                .unwrap_or_else(|| "(none reported)".to_string()),
            negative_test
                .as_ref()
                .map(|nt| format!(
                    "\nNegative test (env -u WAYLAND_DISPLAY): child exit={:?}, expected one \
                     of {:?}, passed={}",
                    nt.child_exit_code, nt.expected_one_of, nt.passed
                ))
                .unwrap_or_default(),
        ),
        redact_on,
    );

    let result = match (outcome, negative_test.as_ref()) {
        (Outcome::Ok, Some(nt)) if nt.passed => ExperimentResult::Pass,
        (Outcome::Ok, None) => ExperimentResult::Pass,
        (Outcome::Ok, Some(_)) => ExperimentResult::Partial,
        _ => ExperimentResult::Fail,
    };

    let data = SessionDiscoveryReport {
        current_uid: uid,
        candidates,
        selected_session_id: selected_id,
        env_assertions: EnvAssertions {
            xdg_session_type,
            wayland_display_set,
            dbus_session_bus_address_set: dbus_set,
            all_asserted: env_ok,
        },
        gnome_session_units,
        outcome: format!("{outcome:?}"),
        negative_test,
    };

    let report = ExperimentReport {
        experiment: "Experiment 1 — GNOME Session Discovery".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1"
            .to_string(),
        objective: "Determine exactly how the active GNOME Wayland session is identified, \
                    without guessing from $DISPLAY or process names (Document 10 §8, \
                    Document 05 §12-14)."
            .to_string(),
        hypothesis: "Exactly one logind session satisfies Type=wayland, Class=user, \
                     Seat=seat0, User=<current uid>, Active=true; the binary fails closed \
                     with a classified exit code if WAYLAND_DISPLAY is unavailable."
            .to_string(),
        procedure: "Call login1 Manager.ListSessions, then Get each Session's Type/Class/ \
                    Seat/Active/State/User/Display/Desktop/LockedHint/Scope/Name property; \
                    select the unique match; assert XDG_SESSION_TYPE/WAYLAND_DISPLAY/ \
                    DBUS_SESSION_BUS_ADDRESS; report the gnome-session/graphical-session \
                    user units. With --self-test, also spawn a child with WAYLAND_DISPLAY \
                    removed and confirm it fails closed."
            .to_string(),
        expected: "Exactly one session selected; env assertions pass; the negative test (if \
                   run) exits non-zero with SESSION_NOT_FOUND or WAYLAND_UNAVAILABLE."
            .to_string(),
        observed,
        evidence: vec!["session.json (this directory)".to_string()],
        result,
        failure: if result == ExperimentResult::Fail {
            Some(format!("outcome={outcome:?}"))
        } else {
            None
        },
        root_cause: None,
        security_impact: Some(
            "None: read-only login1 introspection. Session/desktop names redacted by default."
                .to_string(),
        ),
        recommended_action: None,
        follow_up: Some("Experiment 2 — Mutter Capability Inventory.".to_string()),
    };

    if !suppress_evidence {
        let dir = evidence_dir(EXP_ID, now)?;
        write_evidence(&dir, &report.render(now), "session.json", &data)?;
        println!("Wrote evidence to {}", dir.display());
    }

    let final_exit_code = match &data.negative_test {
        Some(nt) if !nt.passed => 1,
        _ => outcome as i32,
    };
    std::process::exit(final_exit_code);
}
