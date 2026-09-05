//! Shared GNOME session discovery (Document 10 §8): select the logind
//! session for the current desktop user without guessing from `$DISPLAY` or
//! process names (Document 05 §12-14). Used by Experiment 1 (which reports
//! the full rationale) and Experiment 2 (which needs the selected session's
//! object path for its Mutter/Session introspection).

use std::process::Command;

use serde::Serialize;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

/// Properties read from a `org.freedesktop.login1.Session` object.
#[derive(Debug, Serialize)]
pub struct SessionProperties {
    pub r#type: String,
    pub class: String,
    pub seat: String,
    pub active: bool,
    pub state: String,
    pub user_uid: u32,
    pub display: String,
    pub desktop: String,
    pub locked_hint: bool,
    pub scope: String,
    pub name: String,
}

/// One session as reported by `ListSessions`, with its queried properties
/// (or the error if the query failed) and whether it was selected.
#[derive(Debug, Serialize)]
pub struct SessionCandidate {
    pub session_id: String,
    pub uid_from_list: u32,
    pub seat_from_list: String,
    pub object_path: String,
    pub properties: Option<SessionProperties>,
    pub query_error: Option<String>,
    pub selected: bool,
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

fn command_output_any_status(cmd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(cmd).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// The current process's uid, via `id -u` (no extra dependency for a single
/// syscall wrapper).
pub fn current_uid() -> anyhow::Result<u32> {
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

/// Query every logind session and select the unique one matching
/// `Type=wayland, Class=user, Seat=seat0, User=<uid>, Active=true`. Zero or
/// ambiguous (>1) matches both fail closed (`selected_id` is `None`).
pub fn discover(uid: u32) -> anyhow::Result<(Vec<SessionCandidate>, Option<String>)> {
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

/// Render a one-line-per-session rationale (redact the display name first).
pub fn render_rationale(candidates: &[SessionCandidate]) -> String {
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
                    properties.name,
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
