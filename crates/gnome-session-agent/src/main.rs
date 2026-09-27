use std::path::PathBuf;
use std::time::Duration;

use blackroom_gnome::mutter::{capability, lock, session};
use gnome_session_agent::{AgentState, ipc, startup};

mod offline;

/// Doc 06 §29: bounded wait/retry if GNOME is not yet ready at startup.
const MAX_STARTUP_ATTEMPTS: u32 = 10;
const STARTUP_RETRY_DELAY: Duration = Duration::from_secs(2);

/// `/run/blackroom-console/agent.sock` (architecture.md §6.2) does not
/// exist yet — no packaging/setup phase has created that directory with the
/// right ownership. Until then, the agent's own `$XDG_RUNTIME_DIR` (always
/// writable by the logged-in user) is the interim socket location.
fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    runtime_dir.join("blackroom-console").join("agent.sock")
}

fn compatibility_snapshot(report: &capability::CapabilityReport) -> serde_json::Value {
    serde_json::json!({
        "mode": "READ_ONLY_COMPATIBILITY",
        "phase3_session_gate_passed": report.phase3_gate_passed(),
        "remote_mode_allowed": false,
        "capabilities": {
            "os_supported": report.os_supported.as_str(),
            "gnome_supported": report.gnome_supported.as_str(),
            "wayland_supported": report.wayland_supported.as_str(),
            "systemd_supported": report.systemd_supported.as_str(),
            "session_found": report.session_found.as_str(),
            "mutter_capable": report.mutter_capable.as_str(),
            "remote_desktop_capable": report.remote_desktop_capable.as_str(),
            "screencast_capable": report.screencast_capable.as_str(),
            "pipewire_capable": report.pipewire_capable.as_str(),
            "virtual_display_capable": report.virtual_display_capable.as_str(),
            "display_config_capable": report.display_config_capable.as_str(),
            "remote_input_capable": report.remote_input_capable.as_str(),
            "physical_input_isolation_capable": report.physical_input_isolation_capable.as_str(),
            "session_lock_capable": report.session_lock_capable.as_str(),
            "emergency_capable": report.emergency_capable.as_str(),
            "gpu_capable": report.gpu_capable.as_str(),
        },
    })
}

fn lock_snapshot(observation: lock::LockObservation) -> serde_json::Value {
    serde_json::json!({
        "mode": "READ_ONLY_LOCK_OBSERVATION",
        "classification": observation.classification(),
        "screen_saver_active": observation.screen_saver_active,
        "logind_locked_hint": observation.logind_locked_hint,
        "remote_mode_allowed": false,
    })
}

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.as_slice() == [std::ffi::OsStr::new("--compatibility-report")] {
        match session::discover_session() {
            Ok(info) => {
                let report = capability::detect(&info);
                println!("{}", compatibility_snapshot(&report));
            }
            Err(error) => {
                eprintln!("read-only compatibility report unavailable: {}", error.code);
                std::process::exit(1);
            }
        }
        return;
    }
    if arguments.as_slice() == [std::ffi::OsStr::new("--lock-observation")] {
        match session::discover_session().and_then(|info| lock::observe(&info)) {
            Ok(observation) => println!("{}", lock_snapshot(observation)),
            Err(error) => {
                eprintln!("read-only lock observation unavailable: {}", error.code);
                std::process::exit(1);
            }
        }
        return;
    }
    if !arguments.is_empty() {
        if let Err(error) = offline::run(&arguments) {
            eprintln!("offline authority refused: {error}");
            std::process::exit(1);
        }
        return;
    }
    tracing_subscriber::fmt::init();
    tracing::info!("gnome-session-agent starting");
    let state = startup::start(MAX_STARTUP_ATTEMPTS, STARTUP_RETRY_DELAY);
    tracing::info!(%state, "startup finished");
    if state != AgentState::SessionReady {
        std::process::exit(1);
    }

    let path = socket_path();
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        tracing::error!(%error, ?parent, "failed to create agent.sock's parent directory");
        std::process::exit(1);
    }
    let listener = match ipc::bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%error, ?path, "failed to bind agent.sock");
            std::process::exit(1);
        }
    };
    let expected_uid = rustix::process::getuid().as_raw();
    tracing::info!(?path, "agent.sock listening");
    loop {
        match ipc::accept_authorized(&listener, expected_uid) {
            Ok(_stream) => {
                // The installed agent does not consume host authority updates;
                // the explicit offline-only entry path above is separate.
                tracing::info!("agent.sock: authorized connection closed (no protocol yet)");
            }
            Err(error) => {
                tracing::error!(%error, "agent.sock: accept loop failed");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackroom_gnome::mutter::capability::{CapabilityReport, CapabilityTier};

    #[test]
    fn phase3_support_never_implies_remote_control() {
        let mut report = CapabilityReport {
            os_supported: CapabilityTier::Supported,
            gnome_supported: CapabilityTier::Supported,
            wayland_supported: CapabilityTier::Supported,
            systemd_supported: CapabilityTier::Supported,
            session_found: CapabilityTier::Supported,
            mutter_capable: CapabilityTier::Supported,
            remote_desktop_capable: CapabilityTier::Supported,
            screencast_capable: CapabilityTier::Supported,
            pipewire_capable: CapabilityTier::Supported,
            virtual_display_capable: CapabilityTier::Supported,
            display_config_capable: CapabilityTier::Supported,
            remote_input_capable: CapabilityTier::Supported,
            physical_input_isolation_capable: CapabilityTier::Supported,
            session_lock_capable: CapabilityTier::Supported,
            emergency_capable: CapabilityTier::Supported,
            gpu_capable: CapabilityTier::Supported,
        };
        let snapshot = compatibility_snapshot(&report);
        assert_eq!(snapshot["phase3_session_gate_passed"], true);
        assert_eq!(snapshot["remote_mode_allowed"], false);
        assert_eq!(
            snapshot["capabilities"]["remote_input_capable"],
            "SUPPORTED"
        );
        report.os_supported = CapabilityTier::Unknown;
        let unknown = compatibility_snapshot(&report);
        assert_eq!(unknown["phase3_session_gate_passed"], false);
        assert_eq!(unknown["remote_mode_allowed"], false);
    }

    #[test]
    fn lock_observation_never_enables_remote_control() {
        let observation = lock::LockObservation {
            screen_saver_active: true,
            logind_locked_hint: false,
        };
        let snapshot = lock_snapshot(observation);
        assert_eq!(snapshot["classification"], "INDETERMINATE");
        assert_eq!(snapshot["remote_mode_allowed"], false);
    }
}
