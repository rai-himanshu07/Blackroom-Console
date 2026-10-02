//! `blackroom-console`: serves the remote console to a browser on the LAN.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use blackroom_console::server::{random_token, router};
use blackroom_console::{ConsoleConfig, RemoteConsole};
use blackroom_gnome::mutter::video::VideoOptions;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(about = "Remote console: view and drive this laptop's desktop from a browser")]
struct Args {
    /// Address and port to listen on.
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: SocketAddr,
    /// Control socket of `remote-emergencyd --enable-grabs` (absolute path).
    #[arg(long, required_unless_present = "headless")]
    grab_socket: Option<PathBuf>,
    /// Directory for backup.json and the printed URL.
    #[arg(long)]
    state_dir: Option<PathBuf>,
    /// The restore binary the watchdog arms (default: exp07_restore next to this binary).
    #[arg(long)]
    restore_bin: Option<PathBuf>,
    /// Seconds without a browser heartbeat before Stop runs.
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u64).range(5..=120))]
    heartbeat_secs: u64,
    /// JPEG quality, 1 to 100.
    #[arg(long, default_value_t = 70, value_parser = clap::value_parser!(u8).range(1..=100))]
    quality: u8,
    /// Frame rate cap; 0 means no cap.
    #[arg(long, default_value_t = 30)]
    max_fps: u32,
    /// Development only: serve a throwaway `--headless` Shell on a private bus (no grab, watchdog or lock).
    #[arg(long)]
    headless: bool,
}

fn lan_addresses() -> Vec<String> {
    std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .ok()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .filter(|address| !address.contains(':'))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "blackroom_console=info".into()),
        )
        .init();
    let args = Args::parse();
    if let Some(socket) = &args.grab_socket {
        anyhow::ensure!(
            socket.is_absolute(),
            "--grab-socket must be an absolute path"
        );
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let state_dir = args
        .state_dir
        .unwrap_or_else(|| runtime.join("blackroom-console"));
    let restore_bin = match args.restore_bin {
        Some(path) => path,
        None => std::env::current_exe()?.with_file_name("exp07_restore"),
    };
    let console = RemoteConsole::spawn(ConsoleConfig {
        grab_socket: args.grab_socket,
        state_dir: state_dir.clone(),
        headless: args.headless,
        video: VideoOptions {
            quality: args.quality,
            max_fps: args.max_fps,
            ..VideoOptions::default()
        },
        heartbeat_timeout: Duration::from_secs(args.heartbeat_secs),
        restore_bin,
    });

    let token = random_token()?;
    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    let port = listener.local_addr()?.port();
    let mut urls: Vec<String> = if args.listen.ip().is_unspecified() {
        lan_addresses()
            .into_iter()
            .map(|ip| format!("http://{ip}:{port}/?t={token}"))
            .collect()
    } else {
        vec![format!("http://{}/?t={token}", listener.local_addr()?)]
    };
    urls.push(format!("http://127.0.0.1:{port}/?t={token}"));
    println!("Open one of these in a browser (plain http, trusted LAN only):");
    for url in &urls {
        println!("  {url}");
    }
    blackroom_console::display::write_private(&state_dir, "url", urls.join("\n").as_bytes())?;
    println!(
        "Panel stuck black? pkill -KILL -x remote-emergenc; exp07_restore --keep-live-virtual --backup {}/backup.json; loginctl unlock-session <id>",
        state_dir.display()
    );

    let shutdown_console = console.clone();
    axum::serve(listener, router(console, &token))
        .with_graceful_shutdown(async move {
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("install the SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            let report = tokio::task::spawn_blocking(move || shutdown_console.stop()).await;
            tracing::info!(?report, "shut down");
        })
        .await?;
    Ok(())
}
