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

/// Outcome of applying the Doc 05 §12–14 selection rule.
#[derive(Debug, PartialEq, Eq)]
enum Selection<'a> {
    Unique(&'a SessionCandidate),
    /// No candidate matches `seat0 ∧ uid ∧ active` yet — may be transient
    /// (the graphical session has not finished starting, Doc 06 §29).
    NoneYet,
    /// At least one candidate matches `seat0 ∧ uid ∧ active` but fails a
    /// definitive criterion (`Type`/`Class`) — will not resolve by
    /// retrying (e.g. an X11 session for this user will never become
    /// Wayland by waiting longer).
    DefinitiveMismatch,
    /// More than one full match — a stable ambiguity, will not resolve by
    /// retrying.
    Ambiguous,
}

/// Pure selection rule (Doc 05 §12–14): never picks "the first" candidate.
/// Splits matching into two passes so a *transient* absence (no session for
/// this user yet) can be told apart from a *definitive* mismatch (a session
/// exists but is not Wayland/user-class) or a stable ambiguity — only the
/// former is worth retrying (Doc 06 §29).
fn select(candidates: &[SessionCandidate], uid: u32) -> Selection<'_> {
    let identity_matches: Vec<&SessionCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.seat == "seat0" && candidate.uid == uid && candidate.active)
        .collect();
    if identity_matches.is_empty() {
        return Selection::NoneYet;
    }

    let mut full_matches = identity_matches
        .into_iter()
        .filter(|candidate| candidate.session_type == "wayland" && candidate.class == "user");
    let Some(first) = full_matches.next() else {
        return Selection::DefinitiveMismatch;
    };
    if full_matches.next().is_some() {
        Selection::Ambiguous
    } else {
        Selection::Unique(first)
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
    // Retryable::Conditional (Doc 16 §53): a transient "not up yet"
    // condition (Doc 06 §29) that startup::attempt inspects via
    // `error.retryable` to decide whether to retry.
    BlackroomError::new(
        ErrorCode::GnomeSessionUnavailable,
        format!("login1 session discovery failed: {detail}"),
    )
}

/// A definitive, non-retryable selection failure (Doc 05 §11: an
/// unsupported host must not attempt an unsafe fallback). Uses
/// `ErrorCode::HostUnsupported`, whose `retryable()` is `No` — the caller
/// must fail closed to `AgentState::Failed`, never retry.
fn selection_unsupported(detail: impl std::fmt::Display) -> BlackroomError {
    BlackroomError::new(ErrorCode::HostUnsupported, detail.to_string())
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
        // A per-candidate inspection failure must not be silently skipped:
        // dropping a session from the pool could manufacture a false
        // "unique" match among the ones that remain. Fail the whole
        // attempt instead (transient/retryable — the race that caused the
        // read failure may well have cleared by the next attempt).
        let candidate = inspect(conn, session_id.clone(), path).map_err(|error| {
            session_unavailable(format!(
                "failed to query logind session {session_id}'s properties: {error} \
                 (cannot confirm uniqueness without it)"
            ))
        })?;
        candidates.push(candidate);
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
    match select(&candidates, uid) {
        Selection::Unique(selected) => Ok(SessionInfo {
            session_id: selected.session_id.clone(),
            uid: selected.uid,
            seat: selected.seat.clone(),
            // `Selection::Unique` already enforced `session_type == "wayland"`.
            is_wayland: true,
            active: selected.active,
        }),
        Selection::NoneYet => Err(session_unavailable(
            "no session yet matches this user on seat0 (may be transient at startup)",
        )),
        Selection::DefinitiveMismatch => Err(selection_unsupported(
            "a session exists for this user on seat0 but is not a Wayland user session",
        )),
        Selection::Ambiguous => Err(selection_unsupported(
            "more than one Wayland/GNOME user session matches on seat0; cannot select unambiguously",
        )),
    }
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
        assert_eq!(select(&candidates, 1000), Selection::Unique(&candidates[0]));
    }

    #[test]
    fn none_yet_when_no_candidates_at_all() {
        let candidates: [SessionCandidate; 0] = [];
        assert_eq!(select(&candidates, 1000), Selection::NoneYet);
    }

    #[test]
    fn none_yet_when_wrong_seat() {
        let candidates = [candidate("1", 1000, "seat1", "wayland", "user", true)];
        assert_eq!(select(&candidates, 1000), Selection::NoneYet);
    }

    #[test]
    fn none_yet_when_inactive() {
        let candidates = [candidate("1", 1000, "seat0", "wayland", "user", false)];
        assert_eq!(select(&candidates, 1000), Selection::NoneYet);
    }

    #[test]
    fn none_yet_when_other_users_session() {
        let candidates = [candidate("1", 2000, "seat0", "wayland", "user", true)];
        assert_eq!(select(&candidates, 1000), Selection::NoneYet);
    }

    #[test]
    fn definitive_mismatch_when_not_wayland() {
        // Identity (seat0+uid+active) matches, so this is not "not up
        // yet" — an X11 session for this user will never become Wayland
        // by retrying, so this must not be treated as transient.
        let candidates = [candidate("1", 1000, "seat0", "x11", "user", true)];
        assert_eq!(select(&candidates, 1000), Selection::DefinitiveMismatch);
    }

    #[test]
    fn definitive_mismatch_when_not_user_class() {
        let candidates = [candidate("1", 1000, "seat0", "wayland", "greeter", true)];
        assert_eq!(select(&candidates, 1000), Selection::DefinitiveMismatch);
    }

    #[test]
    fn ambiguous_matches_never_pick_the_first() {
        // Reproduces this host's real two-sessions-on-seat0 condition
        // (Phase 0-1 finding): two otherwise-matching sessions must not
        // silently resolve to "the first" one found, and retrying will not
        // resolve a stable ambiguity.
        let candidates = [
            candidate("1", 1000, "seat0", "wayland", "user", true),
            candidate("2", 1000, "seat0", "wayland", "user", true),
        ];
        assert_eq!(select(&candidates, 1000), Selection::Ambiguous);
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
