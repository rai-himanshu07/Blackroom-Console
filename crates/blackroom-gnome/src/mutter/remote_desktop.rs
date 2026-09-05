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

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

use blackroom_core::error::{BlackroomError, ErrorCode};

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

    pub fn start(&mut self) -> Result<(), BlackroomError> {
        self.session_proxy()?
            .call::<_, _, ()>("Start", &())
            .map_err(mutter_unavailable)?;
        self.started = true;
        Ok(())
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
