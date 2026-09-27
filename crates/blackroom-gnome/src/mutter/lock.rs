//! Read-only lock observation for a selected GNOME session. This does not
//! call Lock, SetActive or any input/display mutation method.

use blackroom_core::error::{BlackroomError, ErrorCode};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

use crate::backend::SessionInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockObservation {
    pub screen_saver_active: bool,
    pub logind_locked_hint: bool,
}

impl LockObservation {
    pub fn classification(self) -> &'static str {
        match (self.screen_saver_active, self.logind_locked_hint) {
            (true, true) => "LOCKED_OBSERVED",
            (false, false) => "UNLOCKED_OBSERVED",
            _ => "INDETERMINATE",
        }
    }
}

fn unavailable() -> BlackroomError {
    BlackroomError::new(
        ErrorCode::GnomeSessionUnavailable,
        "read-only lock state could not be observed",
    )
}

fn matches_selected_session(
    selected: &SessionInfo,
    uid: u32,
    seat: &str,
    session_type: &str,
    class: &str,
    active: bool,
) -> bool {
    selected.is_wayland
        && selected.active
        && selected.uid == uid
        && selected.seat == seat
        && seat == "seat0"
        && session_type == "wayland"
        && class == "user"
        && active
}

pub fn observe(selected: &SessionInfo) -> Result<LockObservation, BlackroomError> {
    let system = Connection::system().map_err(|_| unavailable())?;
    let manager = Proxy::new(
        &system,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .map_err(|_| unavailable())?;
    let path: OwnedObjectPath = manager
        .call("GetSession", &(selected.session_id.as_str(),))
        .map_err(|_| unavailable())?;
    let session = Proxy::new(
        &system,
        "org.freedesktop.login1",
        path,
        "org.freedesktop.login1.Session",
    )
    .map_err(|_| unavailable())?;
    let user: (u32, OwnedObjectPath) = session.get_property("User").map_err(|_| unavailable())?;
    let seat: (String, OwnedObjectPath) =
        session.get_property("Seat").map_err(|_| unavailable())?;
    let session_type: String = session.get_property("Type").map_err(|_| unavailable())?;
    let class: String = session.get_property("Class").map_err(|_| unavailable())?;
    let active: bool = session.get_property("Active").map_err(|_| unavailable())?;
    if !matches_selected_session(selected, user.0, &seat.0, &session_type, &class, active) {
        return Err(unavailable());
    }
    let logind_locked_hint = session
        .get_property("LockedHint")
        .map_err(|_| unavailable())?;

    let bus = Connection::session().map_err(|_| unavailable())?;
    let screen_saver = Proxy::new(
        &bus,
        "org.gnome.ScreenSaver",
        "/org/gnome/ScreenSaver",
        "org.gnome.ScreenSaver",
    )
    .map_err(|_| unavailable())?;
    let screen_saver_active = screen_saver
        .call("GetActive", &())
        .map_err(|_| unavailable())?;
    Ok(LockObservation {
        screen_saver_active,
        logind_locked_hint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_observation_classifies_conflicting_evidence() {
        for (screen_saver_active, logind_locked_hint, expected) in [
            (true, true, "LOCKED_OBSERVED"),
            (false, false, "UNLOCKED_OBSERVED"),
            (true, false, "INDETERMINATE"),
            (false, true, "INDETERMINATE"),
        ] {
            assert_eq!(
                LockObservation {
                    screen_saver_active,
                    logind_locked_hint
                }
                .classification(),
                expected
            );
        }
    }

    #[test]
    fn lock_observation_refuses_changed_or_inactive_session() {
        let selected = SessionInfo {
            session_id: "test-session".into(),
            uid: 1000,
            seat: "seat0".into(),
            is_wayland: true,
            active: true,
        };
        assert!(matches_selected_session(
            &selected, 1000, "seat0", "wayland", "user", true
        ));
        assert!(!matches_selected_session(
            &selected, 1001, "seat0", "wayland", "user", true
        ));
        assert!(!matches_selected_session(
            &selected, 1000, "seat0", "wayland", "user", false
        ));
        assert!(!matches_selected_session(
            &selected, 1000, "seat0", "x11", "user", true
        ));
    }
}
