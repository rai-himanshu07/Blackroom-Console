use std::path::PathBuf;
use std::time::Duration;

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

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
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
