use std::time::Duration;

use gnome_session_agent::{AgentState, startup};

/// Doc 06 §29: bounded wait/retry if GNOME is not yet ready at startup.
const MAX_STARTUP_ATTEMPTS: u32 = 10;
const STARTUP_RETRY_DELAY: Duration = Duration::from_secs(2);

fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("gnome-session-agent starting");
    let state = startup::start(MAX_STARTUP_ATTEMPTS, STARTUP_RETRY_DELAY);
    tracing::info!(%state, "startup finished");
    if state == AgentState::Failed {
        std::process::exit(1);
    }
}
