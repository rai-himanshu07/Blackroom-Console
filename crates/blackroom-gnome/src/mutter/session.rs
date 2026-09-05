//! Real logind session discovery (Doc 05 §12–15): select the unique
//! session matching `Type=wayland ∧ Class=user ∧ Seat=seat0 ∧ User=uid ∧
//! Active`, failing closed on zero or ambiguous matches — never "the first
//! session" (this host has two sessions on `seat0`, Phase 0-1 finding).
//! Ports the algorithm proven in `blackroom-experiments::session::discover()`
//! (Experiment 1) as real production code; `blackroom-experiments` itself is
//! discardable scaffolding (`docs/security/architecture.md` §1) and must not
//! be a production dependency.

use blackroom_core::error::{BlackroomError, ErrorCode};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

use crate::backend::SessionInfo;

/// One `login1` session as reported by `Manager.ListSessions` plus its
/// queried `Session` properties.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionCandidate {
    session_id: String,
    uid: u32,
    seat: String,
    session_type: String,
    class: String,
    active: bool,
}

/// Pure selection rule (Doc 05 §12–14): fails closed (`None`) on zero or
/// ambiguous (more than one) matches — never picks "the first" candidate.
fn select(candidates: &[SessionCandidate], uid: u32) -> Option<&SessionCandidate> {
    let mut matches = candidates.iter().filter(|candidate| {
        candidate.session_type == "wayland"
            && candidate.class == "user"
            && candidate.seat == "seat0"
            && candidate.uid == uid
            && candidate.active
    });
    let first = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(first)
    }
}

/// Doc 05 §14: explicitly detect `XDG_SESSION_TYPE=wayland` and corroborate
/// with runtime state, in addition to (never instead of, Doc 05 §12) the
/// `login1`-based selection above. A `false` result is logged, not fatal —
/// the D-Bus selection remains the sole source of truth.
fn corroborate_wayland_environment(
    xdg_session_type: Option<&str>,
    wayland_display_set: bool,
) -> bool {
    let corroborated = xdg_session_type == Some("wayland") && wayland_display_set;
    if !corroborated {
        tracing::warn!(
            ?xdg_session_type,
            wayland_display_set,
            "agent process environment does not corroborate a Wayland session \
             (informational only; the login1-based selection is authoritative)"
        );
    }
    corroborated
}

fn session_unavailable(detail: impl std::fmt::Display) -> BlackroomError {
    BlackroomError::new(
        ErrorCode::GnomeSessionUnavailable,
        format!("login1 session discovery failed: {detail}"),
    )
}

fn inspect(
    conn: &Connection,
    session_id: String,
    path: OwnedObjectPath,
) -> zbus::Result<SessionCandidate> {
    let proxy = Proxy::new(
        conn,
        "org.freedesktop.login1",
        path,
        "org.freedesktop.login1.Session",
    )?;
    let seat: (String, OwnedObjectPath) = proxy.get_property("Seat")?;
    let user: (u32, OwnedObjectPath) = proxy.get_property("User")?;
    Ok(SessionCandidate {
        session_id,
        uid: user.0,
        seat: seat.0,
        session_type: proxy.get_property("Type")?,
        class: proxy.get_property("Class")?,
        active: proxy.get_property("Active")?,
    })
}

fn fetch_candidates(conn: &Connection) -> Result<Vec<SessionCandidate>, BlackroomError> {
    let manager = Proxy::new(
        conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .map_err(session_unavailable)?;
    let sessions: Vec<(String, u32, String, String, OwnedObjectPath)> = manager
        .call("ListSessions", &())
        .map_err(session_unavailable)?;

    let mut candidates = Vec::with_capacity(sessions.len());
    for (session_id, _list_uid, _user_name, _seat_from_list, path) in sessions {
        match inspect(conn, session_id.clone(), path) {
            Ok(candidate) => candidates.push(candidate),
            Err(error) => {
                tracing::warn!(session_id, %error, "failed to query a logind session's properties; skipping it");
            }
        }
    }
    Ok(candidates)
}

/// Real candidate for `GnomeBackend::discover_session` (Doc 05 §8).
pub fn discover_session() -> Result<SessionInfo, BlackroomError> {
    let uid = rustix::process::getuid().as_raw();
    corroborate_wayland_environment(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty()),
    );

    let connection = Connection::system().map_err(session_unavailable)?;
    let candidates = fetch_candidates(&connection)?;
    let selected = select(&candidates, uid).ok_or_else(|| {
        session_unavailable(
            "no unique Wayland/GNOME user session found for the current user on seat0",
        )
    })?;

    Ok(SessionInfo {
        session_id: selected.session_id.clone(),
        uid: selected.uid,
        seat: selected.seat.clone(),
        is_wayland: selected.session_type == "wayland",
        active: selected.active,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        session_id: &str,
        uid: u32,
        seat: &str,
        session_type: &str,
        class: &str,
        active: bool,
    ) -> SessionCandidate {
        SessionCandidate {
            session_id: session_id.to_string(),
            uid,
            seat: seat.to_string(),
            session_type: session_type.to_string(),
            class: class.to_string(),
            active,
        }
    }

    #[test]
    fn selects_the_unique_matching_session() {
        let candidates = [candidate("1", 1000, "seat0", "wayland", "user", true)];
        let selected = select(&candidates, 1000).expect("unique match");
        assert_eq!(selected.session_id, "1");
    }

    #[test]
    fn fails_closed_on_zero_matches() {
        let candidates = [candidate("1", 1000, "seat0", "x11", "user", true)];
        assert!(select(&candidates, 1000).is_none());
    }

    #[test]
    fn fails_closed_on_ambiguous_matches_never_picks_the_first() {
        // Reproduces this host's real two-sessions-on-seat0 condition
        // (Phase 0-1 finding): two otherwise-matching sessions must not
        // silently resolve to "the first" one found.
        let candidates = [
            candidate("1", 1000, "seat0", "wayland", "user", true),
            candidate("2", 1000, "seat0", "wayland", "user", true),
        ];
        assert!(select(&candidates, 1000).is_none());
    }

    #[test]
    fn rejects_non_wayland_session_type() {
        let candidates = [candidate("1", 1000, "seat0", "x11", "user", true)];
        assert!(select(&candidates, 1000).is_none());
    }

    #[test]
    fn rejects_non_user_class() {
        let candidates = [candidate("1", 1000, "seat0", "wayland", "greeter", true)];
        assert!(select(&candidates, 1000).is_none());
    }

    #[test]
    fn rejects_wrong_seat() {
        let candidates = [candidate("1", 1000, "seat1", "wayland", "user", true)];
        assert!(select(&candidates, 1000).is_none());
    }

    #[test]
    fn rejects_inactive_session() {
        let candidates = [candidate("1", 1000, "seat0", "wayland", "user", false)];
        assert!(select(&candidates, 1000).is_none());
    }

    #[test]
    fn rejects_other_users_session() {
        let candidates = [candidate("1", 2000, "seat0", "wayland", "user", true)];
        assert!(select(&candidates, 1000).is_none());
    }

    #[test]
    fn wayland_environment_corroborated_when_both_present() {
        assert!(corroborate_wayland_environment(Some("wayland"), true));
    }

    #[test]
    fn wayland_environment_not_corroborated_when_missing() {
        assert!(!corroborate_wayland_environment(None, false));
        assert!(!corroborate_wayland_environment(Some("wayland"), false));
        assert!(!corroborate_wayland_environment(Some("x11"), true));
    }
}
