//! `blackroom-console`: serves the remote console to a browser on the LAN.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use blackroom_console::server::{random_token, router};
use blackroom_console::{ConsoleConfig, Quality, RemoteConsole, tls};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(about = "Remote console: view and drive this laptop's desktop from a browser")]
struct Args {
    /// Plain-http address and port.
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: SocketAddr,
    /// Also serve https (self-signed certificate) here; needed for full keyboard capture in Chromium.
    #[arg(long)]
    tls_listen: Option<SocketAddr>,
    /// Where the certificate and key are kept (default: ~/.local/share/blackroom-console).
    #[arg(long)]
    cert_dir: Option<PathBuf>,
    /// Control socket of `remote-emergencyd --enable-grabs` (absolute path).
    #[arg(long, required_unless_present = "headless")]
    grab_socket: Option<PathBuf>,
    /// Directory for backup.json and the printed URLs.
    #[arg(long)]
    state_dir: Option<PathBuf>,
    /// The restore binary the watchdog arms (default: exp07_restore next to this binary).
    #[arg(long)]
    restore_bin: Option<PathBuf>,
    /// Seconds without a browser heartbeat before Stop runs.
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u64).range(5..=120))]
    heartbeat_secs: u64,
    /// Starting quality; the page can change it while a session runs.
    #[arg(long, default_value = "medium", value_parser = ["low", "medium", "high"])]
    quality: String,
    /// Development only: serve a throwaway `--headless` Shell on a private bus (no grab, watchdog or lock).
    #[arg(long)]
    headless: bool,
}

fn command_words(program: &str, args: &[&str]) -> Vec<String> {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Docker (172.16/12) and libvirt (192.168.122/24) bridges are not reachable from a tablet.
fn is_virtual_bridge(address: &str) -> bool {
    address.starts_with("192.168.122.")
        || address
            .strip_prefix("172.")
            .and_then(|rest| rest.split('.').next()?.parse::<u8>().ok())
            .is_some_and(|second| (16..=31).contains(&second))
}

fn lan_addresses() -> Vec<String> {
    command_words("hostname", &["-I"])
        .into_iter()
        .filter(|address| !address.contains(':') && !is_virtual_bridge(address))
        .collect()
}

fn urls(scheme: &str, listen: SocketAddr, token: &str) -> Vec<String> {
    let port = listen.port();
    let mut hosts = if listen.ip().is_unspecified() {
        lan_addresses()
    } else {
        vec![listen.ip().to_string()]
    };
    hosts.push("127.0.0.1".into());
    hosts
        .into_iter()
        .map(|host| format!("{scheme}://{host}:{port}/?t={token}"))
        .collect()
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
    let quality = match args.quality.as_str() {
        "low" => Quality::Low,
        "high" => Quality::High,
        _ => Quality::Medium,
    };
    let console = RemoteConsole::spawn(ConsoleConfig {
        grab_socket: args.grab_socket,
        state_dir: state_dir.clone(),
        headless: args.headless,
        quality,
        heartbeat_timeout: Duration::from_secs(args.heartbeat_secs),
        restore_bin,
    });

    let token = random_token()?;
    let app = router(console.clone(), &token);
    let handle = axum_server::Handle::new();

    let http = tokio::net::TcpListener::bind(args.listen).await?;
    let mut all_urls = urls("http", http.local_addr()?, &token);
    let mut tls_task = None;
    if let Some(addr) = args.tls_listen {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let cert_dir = args.cert_dir.unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join(".local/share/blackroom-console")
        });
        let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
        names.extend(lan_addresses());
        names.extend(command_words("hostname", &[]));
        let (cert, key) = tls::load_or_create(&cert_dir, &names)?;
        let config = axum_server::tls_rustls::RustlsConfig::from_pem(cert, key).await?;
        let server = axum_server::bind_rustls(addr, config)
            .handle(handle.clone())
            .serve(app.clone().into_make_service());
        tls_task = Some(tokio::spawn(server));
        all_urls.extend(urls("https", addr, &token));
    }

    println!("Open one of these in a browser (https needs a one-time certificate exception):");
    for url in &all_urls {
        println!("  {url}");
    }
    blackroom_console::display::write_private(&state_dir, "url", all_urls.join("\n").as_bytes())?;
    println!(
        "Panel stuck black? pkill -KILL -x remote-emergenc; exp07_restore --keep-live-virtual --backup {}/backup.json; loginctl unlock-session <id>",
        state_dir.display()
    );

    let shutdown_console = console.clone();
    let shutdown_handle = handle.clone();
    axum::serve(http, app)
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
            shutdown_handle.graceful_shutdown(Some(Duration::from_secs(3)));
        })
        .await?;
    if let Some(task) = tls_task {
        let _ = task.await;
    }
    Ok(())
}
