//! The laptop's own control channel: a D-Bus service on the user's session bus that the top-bar indicator
//! (`docs/ops/gnome-extension/blackroom-indicator@blackroom.local`) reads and uses.
//!
//! It can only show state and end a session; it cannot start one, change a setting or read a credential. Any process of
//! the same user may call it, which is no more than that user can already do with `pkill` or `systemctl --user stop`.

use serde_json::{Value, json};
use zbus::blocking::{Connection, connection::Builder};

use crate::console::{Phase, RemoteConsole, Status};

pub const BUS_NAME: &str = "org.blackroom.Console";
pub const OBJECT_PATH: &str = "/org/blackroom/Console";

/// The few facts the indicator shows; everything else stays inside the console.
pub fn summary(status: &Status, local_url: Option<&str>) -> Value {
    json!({
        "phase": status.phase,
        "mode": status.mode,
        "session_secs": status.session_secs,
        "blank_panel": status.session.blank_panel,
        "block_local_input": status.session.block_local_input,
        "audio": status.audio,
        "host": status.host,
        "version": status.version,
        "input_accepted": status.input_accepted,
        "last_stop": status.last_stop.as_ref().map(|report| json!({
            "reason": report.reason,
            "locked": report.locked,
            "restored": report.topology_restored,
        })),
        "local_url": local_url,
    })
}

struct Control {
    console: RemoteConsole,
    local_url: Option<String>,
}

#[zbus::interface(name = "org.blackroom.Console1")]
impl Control {
    /// JSON text: see `summary`.
    fn status(&self) -> String {
        summary(&self.console.status(), self.local_url.as_deref()).to_string()
    }

    /// Ends the running session (restore the display, lock, release the input grab). Returns at once:
    /// "stopping", or "idle" when nothing runs.
    fn disconnect(&self) -> String {
        if self.console.status().phase == Phase::Idle {
            return "idle".into();
        }
        let console = self.console.clone();
        std::thread::spawn(move || {
            let report = console.stop();
            tracing::info!(?report, "session ended from the laptop's indicator");
        });
        "stopping".into()
    }
}

/// Keep the returned connection alive for as long as the service should be reachable.
pub fn serve(console: RemoteConsole, local_url: Option<String>) -> zbus::Result<Connection> {
    Builder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, Control { console, local_url })?
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::{ConsoleConfig, Quality};
    use std::time::Duration;

    fn console() -> RemoteConsole {
        RemoteConsole::spawn(ConsoleConfig {
            grab_socket: None,
            state_dir: std::env::temp_dir().join("br-control-test"),
            headless: true,
            quality: Quality::Medium,
            heartbeat_timeout: Duration::from_secs(15),
            restore_bin: std::path::PathBuf::new(),
        })
    }

    #[test]
    fn the_summary_names_only_what_the_indicator_shows() {
        let value = summary(&console().status(), Some("https://localhost:8443/"));
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "audio",
                "blank_panel",
                "block_local_input",
                "host",
                "input_accepted",
                "last_stop",
                "local_url",
                "mode",
                "phase",
                "session_secs",
                "version"
            ]
        );
        assert_eq!(value["phase"], "idle");
        assert_eq!(value["local_url"], "https://localhost:8443/");
    }

    #[test]
    fn disconnect_with_nothing_running_does_nothing() {
        let control = Control {
            console: console(),
            local_url: None,
        };
        assert_eq!(control.disconnect(), "idle");
    }
}
