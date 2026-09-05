//! Experiment 1 — GNOME Session Discovery (Document 10 §8).
//!
//! Read-only: enumerates logind sessions via `org.freedesktop.login1` on the
//! system bus and selects the one Wayland/GNOME/user/active session without
//! guessing from `$DISPLAY` or process names (Document 05 §12-14). Never
//! modifies the system (`ListSessions` and property `Get` calls only).

use std::process::Command;

use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, evidence_dir, redact, write_evidence,
};
use clap::Parser;
use serde::Serialize;
use time::OffsetDateTime;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

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
struct SessionProperties {
    r#type: String,
    class: String,
    seat: String,
    active: bool,
    state: String,
    user_uid: u32,
    display: String,
    desktop: String,
    locked_hint: bool,
    scope: String,
    name: String,
}

#[derive(Debug, Serialize)]
struct SessionCandidate {
    session_id: String,
    uid_from_list: u32,
    seat_from_list: String,
    object_path: String,
    properties: Option<SessionProperties>,
    query_error: Option<String>,
    selected: bool,
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

fn current_uid() -> anyhow::Result<u32> {
    command_output_any_status("id", &["-u"])
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("failed to determine current uid via `id -u`"))
}

fn inspect_session(conn: &Connection, path: OwnedObjectPath) -> zbus::Result<SessionProperties> {
    let proxy = Proxy::new(
        conn,
        "org.freedesktop.login1",
        path,
        "org.freedesktop.login1.Session",
    )?;
    let seat: (String, OwnedObjectPath) = proxy.get_property("Seat")?;
    let user: (u32, OwnedObjectPath) = proxy.get_property("User")?;
    Ok(SessionProperties {
        r#type: proxy.get_property("Type")?,
        class: proxy.get_property("Class")?,
        seat: seat.0,
        active: proxy.get_property("Active")?,
        state: proxy.get_property("State")?,
        user_uid: user.0,
        display: proxy.get_property("Display")?,
        desktop: proxy.get_property("Desktop")?,
        locked_hint: proxy.get_property("LockedHint")?,
        scope: proxy.get_property("Scope")?,
        name: proxy.get_property("Name")?,
    })
}

struct RawCandidate {
    session_id: String,
    uid_from_list: u32,
    seat_from_list: String,
    object_path: String,
    properties: Option<SessionProperties>,
    query_error: Option<String>,
    matches: bool,
}

/// Query every logind session and select the unique one matching
/// `Type=wayland, Class=user, Seat=seat0, User=<uid>, Active=true`.
fn discover(uid: u32) -> anyhow::Result<(Vec<SessionCandidate>, Option<String>)> {
    let conn = Connection::system()?;
    let manager = Proxy::new(
        &conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    let sessions: Vec<(String, u32, String, String, OwnedObjectPath)> =
        manager.call("ListSessions", &())?;

    let mut raw = Vec::new();
    for (session_id, list_uid, _user_name, seat_from_list, path) in sessions {
        let object_path = path.to_string();
        match inspect_session(&conn, path) {
            Ok(properties) => {
                let matches = properties.r#type == "wayland"
                    && properties.class == "user"
                    && properties.seat == "seat0"
                    && properties.user_uid == uid
                    && properties.active;
                raw.push(RawCandidate {
                    session_id,
                    uid_from_list: list_uid,
                    seat_from_list,
                    object_path,
                    properties: Some(properties),
                    query_error: None,
                    matches,
                });
            }
            Err(error) => raw.push(RawCandidate {
                session_id,
                uid_from_list: list_uid,
                seat_from_list,
                object_path,
                properties: None,
                query_error: Some(error.to_string()),
                matches: false,
            }),
        }
    }

    // A unique match is required; zero or ambiguous (>1) both fail closed.
    let match_ids: Vec<&str> = raw
        .iter()
        .filter(|candidate| candidate.matches)
        .map(|candidate| candidate.session_id.as_str())
        .collect();
    let selected_id = if match_ids.len() == 1 {
        Some(match_ids[0].to_string())
    } else {
        None
    };

    let candidates = raw
        .into_iter()
        .map(|candidate| SessionCandidate {
            selected: selected_id.as_deref() == Some(candidate.session_id.as_str()),
            session_id: candidate.session_id,
            uid_from_list: candidate.uid_from_list,
            seat_from_list: candidate.seat_from_list,
            object_path: candidate.object_path,
            properties: candidate.properties,
            query_error: candidate.query_error,
        })
        .collect();

    Ok((candidates, selected_id))
}

fn render_rationale(candidates: &[SessionCandidate], redact_on: bool) -> String {
    candidates
        .iter()
        .map(
            |candidate| match (&candidate.properties, &candidate.query_error) {
                (Some(properties), _) => format!(
                    "session {} (seat={}, list_uid={}): type={} class={} seat={} active={} \
                 state={} name={} => {}",
                    candidate.session_id,
                    candidate.seat_from_list,
                    candidate.uid_from_list,
                    properties.r#type,
                    properties.class,
                    properties.seat,
                    properties.active,
                    properties.state,
                    redact(&properties.name, redact_on),
                    if candidate.selected {
                        "SELECTED"
                    } else {
                        "rejected"
                    }
                ),
                (None, Some(error)) => {
                    format!(
                        "session {}: property query FAILED: {error}",
                        candidate.session_id
                    )
                }
                (None, None) => {
                    format!("session {}: no properties collected", candidate.session_id)
                }
            },
        )
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    let redact_on = args.common.redact_enabled();
    let suppress_evidence = std::env::var_os(SUPPRESS_EVIDENCE_ENV).is_some();
    let now = OffsetDateTime::now_utc();

    let uid = current_uid()?;
    let (candidates, selected_id) = discover(uid)?;

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

    let rationale = render_rationale(&candidates, redact_on);
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
