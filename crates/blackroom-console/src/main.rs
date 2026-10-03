//! `blackroom-console`: serves the remote console to a browser on the LAN.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use blackroom_console::hostd_auth::{HostdAuth, sockets_present};
use blackroom_console::login::Login;
use blackroom_console::server::{
    Hardening, random_token, router_hostd_with, router_totp_with, router_with, token_from_file,
};
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
    /// Phase 11: log in through remote-hostd (Linux password, authenticator code and Remote Access Key
    /// or trusted device). The directory holding its `auth.sock` and `admin.sock`, normally
    /// `$XDG_RUNTIME_DIR/blackroom-hostd`; start `remote-hostd.service` first.
    #[arg(long, conflicts_with = "auth_dir")]
    hostd_dir: Option<PathBuf>,
    /// Phase 11: log in with a TOTP code instead of the URL token. A directory (mode 0700, absolute) holding
    /// the enrolled credentials; enrol with `blackroom --state-dir <dir> enroll --account <name>`.
    #[arg(long)]
    auth_dir: Option<PathBuf>,
    /// The enrolled account that may log in (default: the current user name).
    #[arg(long)]
    account: Option<String>,
    /// Tests only: keep the token in this file (0600) so the URL survives restarts. Default: a fresh
    /// random token on every start.
    #[arg(long)]
    token_file: Option<PathBuf>,
    /// PEM certificate chain for `--tls-listen` from a real authority (Let's Encrypt, `tailscale cert`), with
    /// `--tls-key`. Re-read every 6 hours, so a renewed certificate needs no restart.
    #[arg(long, requires = "tls_key")]
    tls_cert: Option<PathBuf>,
    #[arg(long, requires = "tls_cert")]
    tls_key: Option<PathBuf>,
    /// Internet-facing mode: refuses to start unless login is through hostd, https uses a real certificate and the
    /// plain-http port is loopback only; cookies are `Secure` and HSTS is sent.
    #[arg(long)]
    public: bool,
    /// STUN server for reaching the console from outside the LAN, e.g. `stun:stun.l.google.com:19302` (repeatable).
    #[arg(long = "stun")]
    stun: Vec<String>,
    /// TURN relay (coturn), e.g. `turn:turn.example.org:3478?transport=udp` (repeatable); needs `--turn-secret-file`.
    #[arg(long = "turn")]
    turn: Vec<String>,
    /// coturn `static-auth-secret` (file, mode 0600): short-lived TURN credentials are derived from it.
    #[arg(long)]
    turn_secret_file: Option<PathBuf>,
    /// How long a TURN credential stays valid.
    #[arg(long, default_value_t = 3600, value_parser = clap::value_parser!(u64).range(60..=86400))]
    turn_ttl_secs: u64,
    /// UDP port range for media, `MIN-MAX`, so it can be forwarded on a router (default: any port).
    #[arg(long)]
    ice_port_range: Option<String>,
    /// Control socket of `remote-emergencyd --enable-grabs` (absolute path).
    #[arg(long, required_unless_present_any = ["headless", "check_compat"])]
    grab_socket: Option<PathBuf>,
    /// Directory for backup.json and the printed URLs.
    #[arg(long)]
    state_dir: Option<PathBuf>,
    /// The restore binary the watchdog arms (default: exp07_restore next to this binary).
    #[arg(long)]
    restore_bin: Option<PathBuf>,
    /// Seconds without a browser heartbeat before Stop runs.
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(5..=120))]
    heartbeat_secs: u64,
    /// Starting quality; the page can change it while a session runs.
    #[arg(long, default_value = "medium", value_parser = ["low", "medium", "high"])]
    quality: String,
    /// PipeWire node name of the sound output to send to the browser (default: the default output).
    #[arg(long)]
    audio_sink: Option<String>,
    /// Where the saved settings (profile.json) live; default ~/.local/share/blackroom-console, or the state
    /// directory with --headless so tests never touch the real profile.
    #[arg(long)]
    profile_dir: Option<PathBuf>,
    /// Start even when this GNOME/PipeWire combination has not been tested (docs/ops/compatibility-matrix.md). Setups
    /// that cannot work (no GNOME Shell, an X11 session) are refused regardless.
    #[arg(long)]
    allow_untested: bool,
    /// Print the compatibility verdict for this host as JSON and exit.
    #[arg(long)]
    check_compat: bool,
    /// Offer the text clipboard: two buttons in the page send text to the laptop and fetch the laptop's
    /// text (explicit, 256 KiB, rate limited, never logged). Off unless given.
    #[arg(long)]
    clipboard: bool,
    /// The laptop owner's settings page, on this loopback address only (default 127.0.0.1:8090; with `--headless`
    /// only when given). It asks for the laptop account's password.
    #[arg(long)]
    host_listen: Option<SocketAddr>,
    /// The PAM helper that checks that password (default: `pam-auth-helper` next to this binary).
    #[arg(long)]
    pam_helper: Option<PathBuf>,
    /// Development only: serve a throwaway `--headless` Shell on a private bus (no grab, watchdog or lock).
    #[arg(long)]
    headless: bool,
}

fn ice_config_from(args: &Args) -> anyhow::Result<blackroom_console::ice::IceConfig> {
    use blackroom_console::ice;
    for url in &args.stun {
        anyhow::ensure!(
            ice::valid_stun(url),
            "--stun {url:?} must look like stun:host[:port]"
        );
    }
    for url in &args.turn {
        anyhow::ensure!(
            ice::valid_turn(url),
            "--turn {url:?} must look like turn:host[:port][?transport=udp|tcp] (or turns:)"
        );
    }
    let turn_secret = match &args.turn_secret_file {
        Some(path) => {
            let metadata = std::fs::metadata(path)?;
            anyhow::ensure!(
                std::os::unix::fs::MetadataExt::mode(&metadata) & 0o077 == 0,
                "--turn-secret-file must be readable by you only (chmod 600)"
            );
            let secret =
                zeroize::Zeroizing::new(std::fs::read_to_string(path)?.trim().as_bytes().to_vec());
            anyhow::ensure!(
                secret.len() >= 16,
                "--turn-secret-file holds fewer than 16 characters"
            );
            Some(secret)
        }
        None => None,
    };
    anyhow::ensure!(
        args.turn.is_empty() || turn_secret.is_some(),
        "--turn needs --turn-secret-file"
    );
    let port_range = match &args.ice_port_range {
        Some(text) => Some(ice::parse_port_range(text).ok_or_else(|| {
            anyhow::anyhow!("--ice-port-range must be MIN-MAX, both between 1025 and 65535")
        })?),
        None => None,
    };
    Ok(ice::IceConfig {
        stun: args.stun.clone(),
        turn: args.turn.clone(),
        turn_secret,
        turn_ttl: Duration::from_secs(args.turn_ttl_secs),
        port_range,
    })
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
        .map(|host| {
            if token.is_empty() {
                format!("{scheme}://{host}:{port}/")
            } else {
                format!("{scheme}://{host}:{port}/?t={token}")
            }
        })
        .collect()
}

/// What `start_host_page` needs from the command line, copied before `args` is taken apart.
struct HostPageArgs {
    listen: Option<SocketAddr>,
    helper: Option<PathBuf>,
    effective: serde_json::Value,
    hostd_dir: Option<PathBuf>,
}

/// True when systemd says this process is the unit's main process (a console started by hand is not).
fn managed_by_unit(unit: &str) -> bool {
    std::process::Command::new("systemctl")
        .args([
            "--user",
            "show",
            "--no-pager",
            "-p",
            "MainPID",
            "--value",
            unit,
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| text.trim().parse::<u32>().ok())
        == Some(std::process::id())
}

/// Starts the loopback-only settings page; a problem is a warning, the console runs without it.
async fn start_host_page(args: &HostPageArgs, console: &RemoteConsole) -> Option<String> {
    let addr = args.listen?;
    if !addr.ip().is_loopback() {
        tracing::warn!(%addr, "--host-listen must be a loopback address: the settings page is off");
        return None;
    }
    let Ok(account) = std::env::var("USER") else {
        tracing::warn!("no $USER: the settings page is off");
        return None;
    };
    let helper = match &args.helper {
        Some(path) => path.clone(),
        None => std::env::current_exe()
            .ok()?
            .with_file_name("pam-auth-helper"),
    };
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let settings = blackroom_console::hostpage::Settings {
        port: addr.port(),
        account,
        effective: args.effective.clone(),
        managed: managed_by_unit("blackroom-console.service"),
        unit: "blackroom-console.service".into(),
        unit_dirs: vec![
            home.join(".config/systemd/user"),
            PathBuf::from("/etc/systemd/user"),
            PathBuf::from("/usr/lib/systemd/user"),
        ],
        cli: std::env::current_exe().ok()?.with_file_name("blackroom"),
        hostd_dir: args.hostd_dir.clone().unwrap_or_default(),
        home,
    };
    let check: blackroom_console::hostpage::PasswordFactory = std::sync::Arc::new(move || {
        Box::new(remote_hostd::password::PamHelper::new(helper.clone()))
            as Box<dyn remote_hostd::password::PasswordCheck>
    });
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::warn!(%error, %addr, "the settings page could not start (is another console running?)");
            return None;
        }
    };
    let app = blackroom_console::hostpage::router(console.clone(), settings, check);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    println!(
        "Host settings (this laptop only): http://localhost:{}/",
        addr.port()
    );
    Some(format!("http://localhost:{}/", addr.port()))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "blackroom_console=info,blackroom_gnome=info".into()),
        )
        .init();
    // Registered before anything starts: a closed terminal sends SIGHUP and the default action would
    // kill the process with the panel still black.
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let mut args = Args::parse();
    // The console acts for one user's desktop: root would add nothing but blast radius.
    anyhow::ensure!(
        !rustix::process::geteuid().is_root(),
        "refusing to run as root: start the console as the user whose desktop it controls"
    );
    let facts = blackroom_console::compat::detect();
    let verdict = blackroom_console::compat::judge(&facts, args.headless);
    if args.check_compat {
        println!(
            "{}",
            serde_json::json!({ "facts": facts, "verdict": verdict })
        );
        return Ok(());
    }
    match &verdict {
        blackroom_console::compat::Verdict::Supported => {}
        blackroom_console::compat::Verdict::Untested(reasons) if args.allow_untested => {
            tracing::warn!(
                "untested combination, continuing because of --allow-untested: {}",
                reasons.join("; ")
            );
        }
        blackroom_console::compat::Verdict::Untested(reasons) => anyhow::bail!(
            "this desktop combination has not been tested: {}. Start with --allow-untested to try anyway (docs/ops/compatibility-matrix.md)",
            reasons.join("; ")
        ),
        blackroom_console::compat::Verdict::Unsupported(reasons) => {
            anyhow::bail!("this host cannot run the console: {}", reasons.join("; "))
        }
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let state_dir = args
        .state_dir
        .clone()
        .unwrap_or_else(|| runtime.join("blackroom-console"));
    let profile_dir = args.profile_dir.clone().unwrap_or_else(|| {
        if args.headless {
            state_dir.clone()
        } else {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join(".local/share/blackroom-console")
        }
    });
    // The owner's saved host settings win over the command line (the page that edits them restarts the console).
    let (host, host_note) = blackroom_console::host::HostConfig::load(&profile_dir);
    if let Some(note) = host_note {
        tracing::warn!("{note}");
    }
    let parse = |text: &str| text.parse::<SocketAddr>();
    if let Some(text) = &host.http_listen {
        args.listen = parse(text)?;
    }
    match host.tls_listen.as_deref() {
        Some("") => args.tls_listen = None,
        Some(text) => args.tls_listen = Some(parse(text)?),
        None => {}
    }
    if let (Some(cert), Some(key)) = (&host.tls_cert, &host.tls_key) {
        args.tls_cert = Some(cert.clone());
        args.tls_key = Some(key.clone());
    }
    if let Some(public) = host.public {
        args.public = public;
    }
    if let Some(clipboard) = host.allow_clipboard {
        args.clipboard = clipboard;
    }
    if host.audio_sink.is_some() {
        args.audio_sink.clone_from(&host.audio_sink);
    }
    let args_for_host_page = HostPageArgs {
        listen: args
            .host_listen
            .or_else(|| (!args.headless).then(|| SocketAddr::from(([127, 0, 0, 1], 8090)))),
        helper: args.pam_helper.clone(),
        effective: serde_json::json!({
            "http_listen": args.listen.to_string(),
            "tls_listen": args.tls_listen.map(|addr| addr.to_string()),
            "tls_cert": args.tls_cert,
            "tls_key": args.tls_key,
            "public": args.public,
            "clipboard": args.clipboard,
            "audio_sink": args.audio_sink,
            "hostd_login": args.hostd_dir.is_some(),
        }),
        hostd_dir: args.hostd_dir.clone(),
    };
    let ice_config = ice_config_from(&args)?;
    if args.public {
        let problems =
            blackroom_console::exposure::public_problems(blackroom_console::exposure::Facts {
                hostd_login: args.hostd_dir.is_some(),
                tls_listener: args.tls_listen.is_some(),
                certificate_files: args.tls_cert.is_some(),
                http_loopback_only: args.listen.ip().is_loopback(),
            });
        anyhow::ensure!(
            problems.is_empty(),
            "refusing to start in --public mode:\n  {}",
            problems.join("\n  ")
        );
    }
    blackroom_console::ice::install(ice_config);
    if let Some(socket) = &args.grab_socket {
        anyhow::ensure!(
            socket.is_absolute(),
            "--grab-socket must be an absolute path"
        );
    }
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
    console.set_clipboard_enabled(args.clipboard);
    console.set_audio_sink(args.audio_sink.clone());
    if let Some(note) = console.load_profile(profile_dir.clone()) {
        tracing::warn!("{note}");
    }

    enum Mode {
        Hostd(std::sync::Arc<HostdAuth>),
        Totp(std::sync::Arc<Login>),
        Token(String),
    }
    let hostd_login = args.hostd_dir.is_some();
    let (token, mode, login_account) = match (&args.hostd_dir, &args.auth_dir) {
        (Some(dir), _) => {
            anyhow::ensure!(dir.is_absolute(), "--hostd-dir must be an absolute path");
            anyhow::ensure!(
                sockets_present(dir),
                "hostd is not running: no auth.sock in {} (systemctl --user start remote-hostd.service)",
                dir.display()
            );
            let hostd = std::sync::Arc::new(HostdAuth::new(dir.clone(), console.emergency_count()));
            (String::new(), Mode::Hostd(hostd), None)
        }
        (None, Some(dir)) => {
            let directory = remote_hostd::store::open_state_directory(dir)?;
            let account = args
                .account
                .clone()
                .or_else(|| std::env::var("USER").ok())
                .ok_or_else(|| anyhow::anyhow!("--account is needed (no $USER)"))?;
            let enrolled = remote_hostd::totp::enrolled_accounts(&directory)?;
            anyhow::ensure!(
                enrolled.contains(&account),
                "account {account:?} is not enrolled in {} (blackroom --state-dir <dir> enroll --account {account})",
                dir.display()
            );
            let verifier = remote_hostd::totp::load_verifier(
                &directory,
                remote_hostd::totp::Limits::default(),
            )?;
            let login = std::sync::Arc::new(Login::new(&account, verifier));
            (String::new(), Mode::Totp(login), Some(account))
        }
        (None, None) => {
            let token = match &args.token_file {
                Some(path) => token_from_file(path)?,
                None => random_token()?,
            };
            (token.clone(), Mode::Token(token), None)
        }
    };
    let real_certificate = args.tls_cert.is_some();
    let make_app = |hardening: Hardening| match &mode {
        Mode::Hostd(hostd) => {
            router_hostd_with(console.clone(), std::sync::Arc::clone(hostd), hardening)
        }
        Mode::Totp(login) => {
            router_totp_with(console.clone(), std::sync::Arc::clone(login), hardening)
        }
        Mode::Token(token) => router_with(console.clone(), token, hardening),
    };
    let http_hardening = Hardening {
        secure: args.public,
        hsts: false,
    };
    let tls_hardening = Hardening {
        secure: true,
        hsts: args.public && real_certificate,
    };
    let app = make_app(http_hardening);
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
        let config = match (&args.tls_cert, &args.tls_key) {
            (Some(cert), Some(key)) => {
                let config =
                    axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
                // A renewed certificate (Let's Encrypt every ~60 days) is picked up without a restart.
                let (reloading, cert, key) = (config.clone(), cert.clone(), key.clone());
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
                        match reloading.reload_from_pem_file(&cert, &key).await {
                            Ok(()) => tracing::info!("tls certificate reloaded"),
                            Err(error) => {
                                tracing::warn!(%error, "tls certificate reload failed; keeping the old one")
                            }
                        }
                    }
                });
                config
            }
            _ => {
                let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
                names.extend(lan_addresses());
                names.extend(command_words("hostname", &[]));
                let (cert, key) = tls::load_or_create(&cert_dir, &names)?;
                axum_server::tls_rustls::RustlsConfig::from_pem(cert, key).await?
            }
        };
        let server = axum_server::bind_rustls(addr, config)
            .handle(handle.clone())
            .serve(make_app(tls_hardening).into_make_service_with_connect_info::<SocketAddr>());
        tls_task = Some(tokio::spawn(server));
        all_urls.extend(urls("https", addr, &token));
    }

    println!("Open one of these in a browser (https needs a one-time certificate exception):");
    if hostd_login {
        println!(
            "Log in with your Linux password, an authenticator code and the Remote Access Key (or a trusted browser)."
        );
    }
    if let Some(account) = &login_account {
        println!(
            "Log in with the 6-digit code of the authenticator enrolled for account {account:?}."
        );
    }
    for url in &all_urls {
        println!("  {url}");
    }
    blackroom_console::display::write_private(&state_dir, "url", all_urls.join("\n").as_bytes())?;
    println!(
        "Panel stuck black? pkill -KILL -x remote-emergenc; exp07_restore --keep-live-virtual --lock-after --backup {}/backup.json; loginctl unlock-session <id>",
        state_dir.display()
    );

    let local_url = match args.tls_listen {
        Some(addr) => format!("https://localhost:{}/", addr.port()),
        None => format!("http://localhost:{}/", http.local_addr()?.port()),
    };
    let host_url = start_host_page(&args_for_host_page, &console).await;
    // Held until exit; the indicator shows "off" without it, so a failure here is only a warning.
    let _control = match blackroom_console::control::serve(
        console.clone(),
        blackroom_console::control::Urls {
            page: Some(local_url),
            host: host_url,
        },
    ) {
        Ok(connection) => Some(connection),
        Err(error) => {
            tracing::warn!(%error, "the laptop indicator service is not available");
            None
        }
    };

    let shutdown_console = console.clone();
    let shutdown_handle = handle.clone();
    axum::serve(
        http,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let signal = tokio::select! {
            _ = tokio::signal::ctrl_c() => "SIGINT",
            _ = terminate.recv() => "SIGTERM",
            _ = hangup.recv() => "SIGHUP",
        };
        tracing::info!(signal, "shutdown signal received");
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
