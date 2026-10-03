//! `Mutter.ScreenCast` session wrapper (Doc 05 §23; real session sub-object
//! methods confirmed live via Experiment 3,
//! `docs/experiments/evidence/exp03/`: `Start`, `Stop`, `RecordMonitor`
//! (used here), plus `RecordWindow`/`RecordArea`/`RecordVirtual` discovered
//! but not yet exposed — `RecordVirtual` is Experiment 4's job
//! (`virtual_monitor.rs`). `CreateSession` takes an empty options dict;
//! pairing with a `RemoteDesktop` session is not required for monitor
//! capture (Experiment 3 finding).

use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, Value};

use blackroom_core::error::{BlackroomError, ErrorCode};

/// How long to wait for `PipeWireStreamAdded` after `Start()` (Experiment 3
/// default).
const SIGNAL_WAIT: Duration = Duration::from_secs(10);

fn cursor_props(embedded: bool) -> HashMap<&'static str, Value<'static>> {
    let mut props = HashMap::new();
    if embedded {
        props.insert("cursor-mode", Value::from(1_u32));
    }
    props
}

fn mutter_unavailable(detail: impl std::fmt::Display) -> BlackroomError {
    BlackroomError::new(ErrorCode::MutterUnavailable, detail.to_string())
}

/// A live `org.gnome.Mutter.ScreenCast.Session`. `Drop` best-effort-calls
/// `Stop()` if the caller has not already stopped it (Doc 19 §16: never
/// leave a stale session behind).
pub struct ScreenCastSession<'a> {
    conn: &'a Connection,
    path: OwnedObjectPath,
    stopped: bool,
}

/// A stream created by a [`ScreenCastSession`]'s `Record*` method, not yet
/// started.
pub struct ScreenCastStream<'a> {
    conn: &'a Connection,
    path: OwnedObjectPath,
}

impl<'a> ScreenCastSession<'a> {
    /// `Mutter.ScreenCast.CreateSession({})` (Experiment 3: an empty options
    /// dict is sufficient for monitor capture).
    pub fn create(conn: &'a Connection) -> Result<Self, BlackroomError> {
        let proxy = Proxy::new(
            conn,
            "org.gnome.Mutter.ScreenCast",
            "/org/gnome/Mutter/ScreenCast",
            "org.gnome.Mutter.ScreenCast",
        )
        .map_err(mutter_unavailable)?;
        let empty_props: HashMap<&str, Value<'_>> = HashMap::new();
        let path: OwnedObjectPath = proxy
            .call("CreateSession", &(empty_props,))
            .map_err(mutter_unavailable)?;
        Ok(Self {
            conn,
            path,
            stopped: false,
        })
    }

    fn session_proxy(&self) -> Result<Proxy<'a>, BlackroomError> {
        Proxy::new(
            self.conn,
            "org.gnome.Mutter.ScreenCast",
            self.path.clone(),
            "org.gnome.Mutter.ScreenCast.Session",
        )
        .map_err(mutter_unavailable)
    }

    pub fn object_path(&self) -> &OwnedObjectPath {
        &self.path
    }

    /// Capture an existing physical output by connector name (e.g.
    /// `"eDP-1"`) — the proven mechanism for capturing the existing desktop
    /// (Experiment 3), not a virtual monitor.
    pub fn record_monitor(&self, connector: &str) -> Result<ScreenCastStream<'a>, BlackroomError> {
        self.record_monitor_with_cursor(connector, false)
    }

    /// `cursor_embedded` draws the pointer into the picture (`cursor-mode` 1); otherwise Mutter hides it.
    pub fn record_monitor_with_cursor(
        &self,
        connector: &str,
        cursor_embedded: bool,
    ) -> Result<ScreenCastStream<'a>, BlackroomError> {
        let props = cursor_props(cursor_embedded);
        let stream_path: OwnedObjectPath = self
            .session_proxy()?
            .call("RecordMonitor", &(connector, props))
            .map_err(mutter_unavailable)?;
        Ok(ScreenCastStream {
            conn: self.conn,
            path: stream_path,
        })
    }

    /// Creates a **virtual** monitor rather than capturing a physical one
    /// (Experiment 4, `virtual_monitor.rs`). `RecordVirtual`'s
    /// properties-dict schema is not documented anywhere in Mutter's public
    /// surface (`feasibility-research.md` topic 2) — Experiment 4 found the
    /// negotiated PipeWire video format (not this dict) actually drives the
    /// monitor's real resolution; the dict is still populated with the
    /// best-known key spellings as a hint.
    pub fn record_virtual(
        &self,
        width: i32,
        height: i32,
        refresh_rate: f64,
    ) -> Result<ScreenCastStream<'a>, BlackroomError> {
        self.record_virtual_with_cursor(width, height, refresh_rate, false)
    }

    pub fn record_virtual_with_cursor(
        &self,
        width: i32,
        height: i32,
        refresh_rate: f64,
        cursor_embedded: bool,
    ) -> Result<ScreenCastStream<'a>, BlackroomError> {
        let mut props: HashMap<&str, Value<'_>> = HashMap::from([
            ("width", Value::from(width)),
            ("height", Value::from(height)),
            ("framerate", Value::from(refresh_rate)),
        ]);
        props.extend(cursor_props(cursor_embedded));
        let stream_path: OwnedObjectPath = self
            .session_proxy()?
            .call("RecordVirtual", &(props,))
            .map_err(mutter_unavailable)?;
        Ok(ScreenCastStream {
            conn: self.conn,
            path: stream_path,
        })
    }

    pub fn start(&self) -> Result<(), BlackroomError> {
        self.session_proxy()?
            .call::<_, _, ()>("Start", &())
            .map_err(mutter_unavailable)
    }

    /// Idempotent (Doc 07 §27): safe to call more than once.
    pub fn stop(&mut self) -> Result<(), BlackroomError> {
        if self.stopped {
            return Ok(());
        }
        self.session_proxy()?
            .call::<_, _, ()>("Stop", &())
            .map_err(mutter_unavailable)?;
        self.stopped = true;
        Ok(())
    }
}

impl Drop for ScreenCastSession<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            tracing::warn!(%error, path = %self.path, "failed to stop ScreenCast session on drop");
        }
    }
}

impl ScreenCastStream<'_> {
    pub fn object_path(&self) -> &OwnedObjectPath {
        &self.path
    }

    /// Subscribes to `PipeWireStreamAdded` on this stream, then calls
    /// `session.Start()`, then waits up to [`SIGNAL_WAIT`] for the node id
    /// (Experiment 3's proven ordering — the signal listener must be armed
    /// *before* `Start()`, since a bare D-Bus match-rule registration alone
    /// does not retroactively catch an already-emitted signal). Bounded on a
    /// background thread since `zbus::blocking`'s signal iterator has no
    /// built-in timeout.
    pub fn start_and_wait_for_pipewire_node(
        &self,
        session: &ScreenCastSession<'_>,
    ) -> Result<u32, BlackroomError> {
        let stream_proxy = Proxy::new(
            self.conn,
            "org.gnome.Mutter.ScreenCast",
            self.path.as_ref(),
            "org.gnome.Mutter.ScreenCast.Stream",
        )
        .map_err(mutter_unavailable)?;
        let mut signal_iter = stream_proxy
            .receive_signal("PipeWireStreamAdded")
            .map_err(mutter_unavailable)?;

        session.start()?;

        let node_id = thread::scope(|scope| {
            let (tx, rx) = std::sync::mpsc::channel();
            scope.spawn(move || {
                if let Some(msg) = signal_iter.next() {
                    let _ = tx.send(msg.body().deserialize::<(u32,)>().ok());
                }
            });
            rx.recv_timeout(SIGNAL_WAIT).ok().flatten()
        });

        node_id
            .map(|(id,)| id)
            .ok_or_else(|| mutter_unavailable("PipeWireStreamAdded not received within timeout"))
    }
}
