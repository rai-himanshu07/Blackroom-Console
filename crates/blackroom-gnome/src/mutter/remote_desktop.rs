//! `Mutter.RemoteDesktop` session wrapper (Doc 05 §22; session sub-object
//! method *names* confirmed live via Experiment 3 introspection,
//! `docs/experiments/evidence/exp03/`: `Start`, `Stop`, `ConnectToEIS`, plus
//! input-injection and clipboard methods unused until Phase 6). Experiment 3
//! did **not** call `Start()` successfully — it only confirmed that
//! `Stop()` on an unstarted session errors with `Session not started`
//! (exercised below via the `started` flag); `RemoteDesktop.Start()`'s own
//! behaviour remains unverified until a phase that actually needs the
//! session started (Phase 6, `ConnectToEIS`), which is why
//! `remote_desktop_capable` stays `EXPERIMENTAL`, not `SUPPORTED`
//! (`capability.rs`). Experiment 3 found that pairing with a `ScreenCast`
//! session is not required for basic monitor capture (Doc 02 §9 step 2's
//! "if required" resolved to "not required" for that case) — this session
//! stands alone for now.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Type as MessageType;
use zbus::zvariant::{OwnedFd, OwnedObjectPath, OwnedValue, Value};

use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::lease::InputAuthorization;

use super::eis::EiConnection;

fn mutter_unavailable(detail: impl std::fmt::Display) -> BlackroomError {
    BlackroomError::new(ErrorCode::MutterUnavailable, detail.to_string())
}

/// A live `org.gnome.Mutter.RemoteDesktop.Session`. `Stop()` requires a
/// prior `Start()` (Experiment 3: calling `Stop()` on an unstarted session
/// returns `org.freedesktop.DBus.Error.Failed: Session not started`), so
/// `Drop` only best-effort-stops a session that was actually started.
pub struct RemoteDesktopSession<'a> {
    conn: &'a Connection,
    path: OwnedObjectPath,
    started: bool,
    stopped: bool,
}

impl<'a> RemoteDesktopSession<'a> {
    /// `Mutter.RemoteDesktop.CreateSession()` (no arguments; confirmed live,
    /// `docs/gnome/api-inventory.md`).
    pub fn create(conn: &'a Connection) -> Result<Self, BlackroomError> {
        let proxy = Proxy::new(
            conn,
            "org.gnome.Mutter.RemoteDesktop",
            "/org/gnome/Mutter/RemoteDesktop",
            "org.gnome.Mutter.RemoteDesktop",
        )
        .map_err(mutter_unavailable)?;
        let path: OwnedObjectPath = proxy
            .call("CreateSession", &())
            .map_err(mutter_unavailable)?;
        Ok(Self {
            conn,
            path,
            started: false,
            stopped: false,
        })
    }

    fn session_proxy(&self) -> Result<Proxy<'a>, BlackroomError> {
        Proxy::new(
            self.conn,
            "org.gnome.Mutter.RemoteDesktop",
            self.path.clone(),
            "org.gnome.Mutter.RemoteDesktop.Session",
        )
        .map_err(mutter_unavailable)
    }

    pub fn object_path(&self) -> &OwnedObjectPath {
        &self.path
    }

    /// `EnableClipboard` (signature confirmed live: `a{sv}`); only valid before
    /// `Start`, so the session never exposes the clipboard unless asked.
    pub fn enable_clipboard(&mut self) -> Result<(), BlackroomError> {
        if self.started {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "the clipboard must be enabled before the session starts",
            ));
        }
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        self.session_proxy()?
            .call::<_, _, ()>("EnableClipboard", &(options,))
            .map_err(mutter_unavailable)
    }

    /// `NotifyKeyboardKeysym(u, b)`: presses or releases the key that types `keysym` in the laptop's current layout.
    pub fn notify_keyboard_keysym(&self, keysym: u32, pressed: bool) -> Result<(), BlackroomError> {
        if !self.started {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "remote desktop session must be started before keyboard input",
            ));
        }
        self.session_proxy()?
            .call::<_, _, ()>("NotifyKeyboardKeysym", &(keysym, pressed))
            .map_err(mutter_unavailable)
    }

    /// Makes this session the clipboard owner offering `mime_types`; Mutter then
    /// raises `SelectionTransfer` for every paste.
    pub fn set_selection(&self, mime_types: &[&str]) -> Result<(), BlackroomError> {
        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("mime-types", Value::new(mime_types.to_vec()));
        self.session_proxy()?
            .call::<_, _, ()>("SetSelection", &(options,))
            .map_err(mutter_unavailable)
    }

    /// Answers one `SelectionTransfer`: write the data to the returned pipe, then
    /// call [`Self::selection_write_done`].
    pub fn selection_write(&self, serial: u32) -> Result<std::os::fd::OwnedFd, BlackroomError> {
        let fd: OwnedFd = self
            .session_proxy()?
            .call("SelectionWrite", &(serial,))
            .map_err(mutter_unavailable)?;
        blocking(fd.into())
    }

    pub fn selection_write_done(&self, serial: u32, success: bool) -> Result<(), BlackroomError> {
        self.session_proxy()?
            .call::<_, _, ()>("SelectionWriteDone", &(serial, success))
            .map_err(mutter_unavailable)
    }

    /// The pipe to read the current clipboard owner's data as `mime_type`.
    pub fn selection_read(&self, mime_type: &str) -> Result<std::os::fd::OwnedFd, BlackroomError> {
        let fd: OwnedFd = self
            .session_proxy()?
            .call("SelectionRead", &(mime_type,))
            .map_err(mutter_unavailable)?;
        blocking(fd.into())
    }

    pub fn start(&mut self) -> Result<(), BlackroomError> {
        self.session_proxy()?
            .call::<_, _, ()>("Start", &())
            .map_err(mutter_unavailable)?;
        self.started = true;
        Ok(())
    }

    pub fn connect_to_eis(
        &self,
        authority: &InputAuthorization<'_>,
    ) -> Result<EiConnection, BlackroomError> {
        authority.validate()?;
        if !self.started {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "remote desktop session must be started before EIS input",
            ));
        }
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        let fd: OwnedFd = self
            .session_proxy()?
            .call("ConnectToEIS", &(options,))
            .map_err(mutter_unavailable)?;
        EiConnection::from_fd(fd)
    }

    /// Idempotent (Doc 07 §27): a session that was never started is left
    /// alone rather than producing the real "Session not started" error.
    pub fn stop(&mut self) -> Result<(), BlackroomError> {
        if self.stopped || !self.started {
            return Ok(());
        }
        self.session_proxy()?
            .call::<_, _, ()>("Stop", &())
            .map_err(mutter_unavailable)?;
        self.stopped = true;
        Ok(())
    }
}

impl Drop for RemoteDesktopSession<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            tracing::warn!(%error, path = %self.path, "failed to stop RemoteDesktop session on drop");
        }
    }
}

/// Mutter hands the clipboard pipes out non-blocking (a read before the owner writes fails with
/// `EAGAIN`); callers read and write them from helper threads that expect blocking I/O.
fn blocking(fd: std::os::fd::OwnedFd) -> Result<std::os::fd::OwnedFd, BlackroomError> {
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
    let flags = fcntl_getfl(&fd).map_err(mutter_unavailable)?;
    fcntl_setfl(&fd, flags - OFlags::NONBLOCK).map_err(mutter_unavailable)?;
    Ok(fd)
}

/// A clipboard signal from any RemoteDesktop session on the bus. Mutter only
/// accepts replies from the session's own connection, so the listener shares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionEvent {
    /// Something wants to paste: answer with `SelectionWrite(serial)`.
    Transfer {
        path: String,
        mime_type: String,
        serial: u32,
    },
    /// The clipboard owner changed; `session_is_owner` is true for our own set.
    OwnerChanged {
        path: String,
        session_is_owner: bool,
        mime_types: Vec<String>,
    },
}

/// Starts one process-long thread that forwards `SelectionTransfer` and
/// `SelectionOwnerChanged` from every RemoteDesktop session. The blocking
/// iterator cannot be woken, so the thread simply lives as long as the process.
pub fn listen_selection_events(
    conn: &Connection,
) -> Result<Receiver<SelectionEvent>, BlackroomError> {
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("org.gnome.Mutter.RemoteDesktop.Session")
        .map_err(mutter_unavailable)?
        .build();
    let iterator =
        MessageIterator::for_match_rule(rule, conn, Some(64)).map_err(mutter_unavailable)?;
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("selection-events".into())
        .spawn(move || {
            for message in iterator.flatten() {
                let header = message.header();
                let (Some(path), Some(member)) = (header.path(), header.member()) else {
                    continue;
                };
                let path = path.to_string();
                let event = match member.as_str() {
                    "SelectionTransfer" => message.body().deserialize::<(String, u32)>().ok().map(
                        |(mime_type, serial)| SelectionEvent::Transfer {
                            path,
                            mime_type,
                            serial,
                        },
                    ),
                    "SelectionOwnerChanged" => message
                        .body()
                        .deserialize::<HashMap<String, OwnedValue>>()
                        .ok()
                        .map(|options| owner_changed(path, &options)),
                    _ => None,
                };
                if let Some(event) = event
                    && sender.send(event).is_err()
                {
                    return;
                }
            }
        })
        .map_err(mutter_unavailable)?;
    Ok(receiver)
}

fn owner_changed(path: String, options: &HashMap<String, OwnedValue>) -> SelectionEvent {
    let session_is_owner = options
        .get("session-is-owner")
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or(false);
    let mime_types = options
        .get("mime-types")
        .and_then(|value| Vec::<String>::try_from(value.try_clone().ok()?).ok())
        .unwrap_or_default();
    SelectionEvent::OwnerChanged {
        path,
        session_is_owner,
        mime_types,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_changed_reads_the_documented_keys() {
        let mut options = HashMap::new();
        options.insert(
            "session-is-owner".to_string(),
            OwnedValue::try_from(Value::from(true)).unwrap(),
        );
        options.insert(
            "mime-types".to_string(),
            OwnedValue::try_from(Value::new(vec!["text/plain"])).unwrap(),
        );
        assert_eq!(
            owner_changed("/p".into(), &options),
            SelectionEvent::OwnerChanged {
                path: "/p".into(),
                session_is_owner: true,
                mime_types: vec!["text/plain".into()],
            }
        );
        assert_eq!(
            owner_changed("/p".into(), &HashMap::new()),
            SelectionEvent::OwnerChanged {
                path: "/p".into(),
                session_is_owner: false,
                mime_types: vec![],
            }
        );
    }
}
