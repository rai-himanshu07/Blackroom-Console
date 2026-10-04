//! The laptop owner's own page: host settings, approvals and (later) credentials. It is a separate listener on the loopback
//! address only, behind the owner's Linux password, so a tablet that can reach the client page cannot reach it. Every request
//! must name a loopback `Host` (a rebinding page cannot reach it) and every change must come from this page's own origin.
//!
//! Host settings are saved to `host.json` and take effect when the console restarts: nothing here changes a running session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HOST, ORIGIN, SET_COOKIE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use remote_hostd::password::{PasswordCheck, PasswordOutcome, password_ok};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::console::{Phase, RemoteConsole};
use crate::host::HostConfig;

const PAGE: &str = include_str!("web/host.html");
const SCRIPT: &str = include_str!("web/host.js");
const STYLE: &str = include_str!("web/app.css");
const HOST_STYLE: &str = include_str!("web/host.css");

const COOKIE_NAME: &str = "br_host";
const IDLE: Duration = Duration::from_secs(15 * 60);
const MAX_AGE: Duration = Duration::from_secs(60 * 60);
const FAILURES_BEFORE_LOCK: u32 = 5;
const LOCK: Duration = Duration::from_secs(60);

pub type PasswordFactory = Arc<dyn Fn() -> Box<dyn PasswordCheck> + Send + Sync>;

pub struct Settings {
    pub port: u16,
    /// The Linux account whose password opens the page.
    pub account: String,
    /// What the console was started with where `host.json` says nothing, shown as the current value.
    pub effective: Value,
    /// Directories searched for the console's systemd unit, and the owner's home (for the start-at-login link).
    pub unit: String,
    /// This process is the unit's main process, so a restart through systemd restarts this console.
    pub managed: bool,
    pub unit_dirs: Vec<PathBuf>,
    pub home: PathBuf,
    /// The `blackroom` command and its login-authority state directory, used for credential management.
    pub cli: PathBuf,
    pub state_dir: PathBuf,
    /// Where the login authority's sockets appear when it runs.
    pub hostd_runtime: PathBuf,
    /// The `gnome-extensions` command, used for the lock-screen extension.
    pub gnome_extensions: PathBuf,
    /// `systemctl`, used to restart the login authority after the authenticator changes.
    pub systemctl: PathBuf,
    /// What the console runs with now, so a saved internet mode can be checked before it can stop the next start.
    pub internet: Option<crate::internet::Effective>,
}

#[derive(Default)]
struct Sessions {
    live: HashMap<String, (Instant, Instant)>,
}

#[derive(Default)]
struct Limiter {
    failures: u32,
    /// Checks that started and have not answered: they count against the allowance before PAM runs, so parallel
    /// attempts cannot all pass a lock that none of them has tripped yet.
    in_flight: u32,
    locked_until: Option<Instant>,
}

#[derive(Clone)]
struct App {
    console: RemoteConsole,
    settings: Arc<Settings>,
    check: PasswordFactory,
    sessions: Arc<Mutex<Sessions>>,
    limiter: Arc<Mutex<Limiter>>,
    totp: Arc<Mutex<Option<PendingTotp>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn router(console: RemoteConsole, settings: Settings, check: PasswordFactory) -> Router {
    let app = App {
        console,
        settings: Arc::new(settings),
        check,
        sessions: Arc::default(),
        limiter: Arc::default(),
        totp: Arc::default(),
    };
    Router::new()
        .route("/", get(index))
        .route("/host.js", get(script))
        .route("/app.css", get(style))
        .route("/host.css", get(host_style))
        .route("/host/login", post(login))
        .route("/host/logout", post(logout))
        .route("/host/state", get(state))
        .route("/host/config", post(config))
        .route("/host/restart", post(restart))
        .route("/host/approve", post(approve))
        .route("/host/autostart", post(autostart))
        .route("/host/credentials", post(credentials))
        .route("/host/lockscreen", post(lockscreen))
        .route("/host/totp/start", post(totp_start))
        .route("/host/totp/verify", post(totp_verify))
        .route("/host/totp/cancel", post(totp_cancel))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(app)
}

async fn security_headers(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    let mut set = |name: &'static str, value: &'static str| {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    };
    set(
        "content-security-policy",
        "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    );
    set("x-content-type-options", "nosniff");
    set("x-frame-options", "DENY");
    set("referrer-policy", "no-referrer");
    set("cross-origin-resource-policy", "same-origin");
    set("cross-origin-opener-policy", "same-origin");
    set(
        "permissions-policy",
        "camera=(), microphone=(), geolocation=(), payment=()",
    );
    response
}

fn reply(code: StatusCode, value: &Value) -> Response {
    (code, Json(value.clone())).into_response()
}

fn error(code: StatusCode, message: &str) -> Response {
    reply(code, &json!({ "error": message }))
}

/// Only a loopback name and this listener's port: a page that rebinds its own name to 127.0.0.1 still sends its own `Host`.
fn host_ok(headers: &HeaderMap, port: u16) -> bool {
    headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| {
            [
                format!("127.0.0.1:{port}"),
                format!("localhost:{port}"),
                format!("[::1]:{port}"),
            ]
            .iter()
            .any(|ok| ok == host)
        })
}

fn origin_ok(headers: &HeaderMap) -> bool {
    let host = headers.get(HOST).and_then(|value| value.to_str().ok());
    let origin = headers.get(ORIGIN).and_then(|value| value.to_str().ok());
    matches!((host, origin), (Some(host), Some(origin)) if origin == format!("http://{host}"))
}

fn session_ok(app: &App, headers: &HeaderMap) -> bool {
    let Some(token) = crate::server::cookie_value(headers, COOKIE_NAME) else {
        return false;
    };
    let now = Instant::now();
    let mut sessions = lock(&app.sessions);
    sessions.live.retain(|_, (created, seen)| {
        now.duration_since(*created) < MAX_AGE && now.duration_since(*seen) < IDLE
    });
    sessions
        .live
        .iter_mut()
        .find(|(known, _)| crate::server::tokens_equal(known, token))
        .map(|(_, times)| times.1 = now)
        .is_some()
}

/// `None` lets the request through. A change must also come from this page's own origin.
fn guard(app: &App, headers: &HeaderMap, changes: bool) -> Option<Response> {
    if !host_ok(headers, app.settings.port) {
        return Some(error(
            StatusCode::FORBIDDEN,
            "this page answers only on the laptop's own address",
        ));
    }
    if changes && !origin_ok(headers) {
        return Some(error(StatusCode::FORBIDDEN, "cross-origin request refused"));
    }
    if !session_ok(app, headers) {
        return Some(error(StatusCode::UNAUTHORIZED, "log in first"));
    }
    None
}

async fn index(State(app): State<App>, headers: HeaderMap) -> Response {
    if !host_ok(&headers, app.settings.port) {
        return error(
            StatusCode::FORBIDDEN,
            "this page answers only on the laptop's own address",
        );
    }
    Html(PAGE).into_response()
}

async fn script(State(app): State<App>, headers: HeaderMap) -> Response {
    asset(&app, &headers, SCRIPT, "text/javascript; charset=utf-8")
}

async fn style(State(app): State<App>, headers: HeaderMap) -> Response {
    asset(&app, &headers, STYLE, "text/css; charset=utf-8")
}

async fn host_style(State(app): State<App>, headers: HeaderMap) -> Response {
    asset(&app, &headers, HOST_STYLE, "text/css; charset=utf-8")
}

fn asset(
    app: &App,
    headers: &HeaderMap,
    body: &'static str,
    content_type: &'static str,
) -> Response {
    if !host_ok(headers, app.settings.port) {
        return error(
            StatusCode::FORBIDDEN,
            "this page answers only on the laptop's own address",
        );
    }
    ([(CONTENT_TYPE, content_type)], body).into_response()
}

fn object<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, String> {
    match serde_json::from_slice::<Value>(body) {
        Ok(value @ Value::Object(_)) => {
            serde_json::from_value(value).map_err(|error| error.to_string())
        }
        Ok(_) => Err("a JSON object is expected".into()),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginBody {
    password: String,
}

/// Checks the laptop account's password (rate limited); `Err` is the reply to send.
async fn check_password(app: &App, password: String) -> Result<(), Response> {
    if !password_ok(&password) {
        return Err(error(StatusCode::UNAUTHORIZED, "wrong password"));
    }
    {
        let mut limiter = lock(&app.limiter);
        let locked = limiter
            .locked_until
            .is_some_and(|until| Instant::now() < until);
        if locked || limiter.failures + limiter.in_flight >= FAILURES_BEFORE_LOCK {
            return Err(error(
                StatusCode::TOO_MANY_REQUESTS,
                "too many wrong passwords: wait a minute",
            ));
        }
        limiter.in_flight += 1;
    }
    let account = app.settings.account.clone();
    let factory = Arc::clone(&app.check);
    let outcome = tokio::task::spawn_blocking(move || {
        let password = zeroize::Zeroizing::new(password);
        factory().check(&account, &password)
    })
    .await
    .unwrap_or(PasswordOutcome::Unavailable);
    lock(&app.limiter).in_flight -= 1;
    match outcome {
        PasswordOutcome::Accepted => {
            lock(&app.limiter).failures = 0;
            Ok(())
        }
        PasswordOutcome::Rejected => {
            let mut limiter = lock(&app.limiter);
            limiter.failures += 1;
            if limiter.failures >= FAILURES_BEFORE_LOCK {
                limiter.failures = 0;
                limiter.locked_until = Some(Instant::now() + LOCK);
            }
            Err(error(StatusCode::UNAUTHORIZED, "wrong password"))
        }
        PasswordOutcome::Unavailable => Err(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "the password check is not available (is the blackroom PAM helper installed?)",
        )),
    }
}

async fn login(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    if !host_ok(&headers, app.settings.port) {
        return error(
            StatusCode::FORBIDDEN,
            "this page answers only on the laptop's own address",
        );
    }
    if !origin_ok(&headers) {
        return error(StatusCode::FORBIDDEN, "cross-origin request refused");
    }
    let Ok(LoginBody { password }) = object::<LoginBody>(&body) else {
        return error(StatusCode::BAD_REQUEST, "send {\"password\": \"...\"}");
    };
    if let Err(refusal) = check_password(&app, password).await {
        return refusal;
    }
    let Ok(token) = crate::server::random_token() else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "no random source");
    };
    let now = Instant::now();
    lock(&app.sessions).live.insert(token.clone(), (now, now));
    let mut response = reply(StatusCode::OK, &json!({ "ok": true }));
    if let Ok(value) = HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/"
    )) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
}

async fn logout(State(app): State<App>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&app, &headers, true) {
        return refusal;
    }
    if let Some(token) = crate::server::cookie_value(&headers, COOKIE_NAME) {
        lock(&app.sessions)
            .live
            .retain(|known, _| !crate::server::tokens_equal(known, token));
    }
    let mut response = reply(StatusCode::OK, &json!({ "ok": true }));
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_static("br_host=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    );
    response
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sink {
    pub name: String,
    pub description: String,
}

/// The sound outputs in `pw-cli ls Node` output.
pub fn parse_sinks(text: &str) -> Vec<Sink> {
    let mut sinks = Vec::new();
    let (mut name, mut description, mut class) = (None::<String>, None::<String>, None::<String>);
    let mut flush = |name: &mut Option<String>,
                     description: &mut Option<String>,
                     class: &mut Option<String>| {
        if let (Some(node), Some("Audio/Sink")) = (name.take(), class.as_deref()) {
            sinks.push(Sink {
                description: description.take().unwrap_or_else(|| node.clone()),
                name: node,
            });
        }
        *description = None;
        *class = None;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("id ") {
            flush(&mut name, &mut description, &mut class);
            continue;
        }
        let Some((key, value)) = line.split_once(" = ") else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key.trim() {
            "node.name" => name = Some(value),
            "node.description" => description = Some(value),
            "media.class" => class = Some(value),
            _ => {}
        }
    }
    flush(&mut name, &mut description, &mut class);
    sinks
}

fn sound_outputs() -> Vec<Value> {
    let output = std::process::Command::new("pw-cli")
        .args(["ls", "Node"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    match output {
        Ok(output) if output.status.success() => {
            parse_sinks(&String::from_utf8_lossy(&output.stdout))
                .into_iter()
                .map(|sink| json!({ "name": sink.name, "description": sink.description }))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Where the start-at-login link lives: the same place `systemctl --user enable` would put it.
pub fn wants_link(home: &Path, unit: &str) -> PathBuf {
    home.join(".config/systemd/user/graphical-session.target.wants")
        .join(unit)
}

pub fn unit_file(dirs: &[PathBuf], unit: &str) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(unit))
        .find(|path| path.is_file())
}

pub fn autostart_enabled(home: &Path, unit: &str) -> bool {
    std::fs::symlink_metadata(wants_link(home, unit))
        .is_ok_and(|meta| meta.file_type().is_symlink())
}

pub fn set_autostart(
    home: &Path,
    dirs: &[PathBuf],
    unit: &str,
    enabled: bool,
) -> Result<(), String> {
    let link = wants_link(home, unit);
    if enabled {
        let target = unit_file(dirs, unit)
            .ok_or_else(|| format!("{unit} is not installed (no unit file found)"))?;
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        if autostart_enabled(home, unit) {
            return Ok(());
        }
        std::os::unix::fs::symlink(&target, &link)
            .map_err(|error| format!("cannot link {}: {error}", link.display()))
    } else {
        match std::fs::symlink_metadata(&link) {
            Ok(meta) if meta.file_type().is_symlink() => std::fs::remove_file(&link)
                .map_err(|error| format!("cannot remove {}: {error}", link.display())),
            Ok(_) => Err(format!(
                "{} is not a link made by this page: left alone",
                link.display()
            )),
            Err(_) => Ok(()),
        }
    }
}

fn reload_systemd() {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

async fn state(State(app): State<App>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&app, &headers, false) {
        return refusal;
    }
    let console = app.console.clone();
    let settings = Arc::clone(&app.settings);
    let built = tokio::task::spawn_blocking(move || {
        let dir = console.settings_dir();
        let (saved, note) = dir
            .as_deref()
            .map_or_else(|| (console.host_config(), None), HostConfig::load);
        let running = console.host_config();
        let status = console.status();
        // Read again each time: a renewed certificate or a passing month changes the answer while the console runs.
        let mut effective = settings.effective.clone();
        if let (Some(internet), Some(object)) = (&settings.internet, effective.as_object_mut()) {
            let report = crate::internet::preflight(internet, crate::internet::now_unix());
            object.insert(
                "internet".into(),
                serde_json::to_value(report).unwrap_or(Value::Null),
            );
        }
        json!({
            "account": settings.account,
            "host": status.host,
            "version": status.version,
            "config": saved,
            "restart_needed": saved.needs_restart_from(&running),
            "note": note,
            "effective": effective,
            "status": {
                "phase": status.phase,
                "mode": status.mode,
                "session_secs": status.session_secs,
                "pending": status.pending,
            },
            "unit": {
                "name": settings.unit,
                "installed": unit_file(&settings.unit_dirs, &settings.unit).is_some(),
                "by_systemd": settings.managed,
                "autostart": autostart_enabled(&settings.home, &settings.unit),
            },
            "sinks": sound_outputs(),
            "totp": { "enrolled": totp_enrolled(&settings) },
            "login": { "ready": crate::hostd_auth::sockets_present(&settings.hostd_runtime) },
            "lockscreen": with_pending_off(
                lock_state(&settings.gnome_extensions),
                matches!(status.phase, crate::console::Phase::Running | crate::console::Phase::Starting),
            ),
        })
    })
    .await;
    match built {
        Ok(value) => reply(StatusCode::OK, &value),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not read the state",
        ),
    }
}

async fn config(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&app, &headers, true) {
        return refusal;
    }
    let config = match object::<HostConfig>(&body).and_then(HostConfig::validated) {
        Ok(config) => config,
        Err(message) => {
            return error(
                StatusCode::BAD_REQUEST,
                &format!("host settings: {message}"),
            );
        }
    };
    let Some(dir) = app.console.settings_dir() else {
        return error(
            StatusCode::CONFLICT,
            "this console has no settings directory",
        );
    };
    // Internet mode that the next start would refuse must not be saved: the console would not come back.
    if config.public == Some(true)
        && let Some(running) = &app.settings.internet
    {
        let report =
            crate::internet::preflight(&running.overlaid(&config), crate::internet::now_unix());
        if !report.problems.is_empty() {
            return error(
                StatusCode::CONFLICT,
                &format!(
                    "internet mode cannot be saved yet: {}. Run `blackroom internet` on this laptop to set it up",
                    report.problems.join("; ")
                ),
            );
        }
    }
    // A certificate that cannot be read or does not match its key stops the console from starting.
    if let (Some(cert), Some(key)) = (&config.tls_cert, &config.tls_key)
        && let Err(problem) = crate::certcheck::inspect_files(cert, key)
    {
        return error(
            StatusCode::CONFLICT,
            &format!("the certificate cannot be used, so it was not saved: {problem}"),
        );
    }
    if config.login == Some(crate::host::LoginMethod::Hostd)
        && !crate::hostd_auth::sockets_present(&app.settings.hostd_runtime)
    {
        return error(
            StatusCode::CONFLICT,
            "the login authority is not running: use \"Set up the login authority\" first",
        );
    }
    match config.save(&dir) {
        Ok(_) => reply(
            StatusCode::OK,
            &json!({ "saved": true, "restart_needed": config.needs_restart_from(&app.console.host_config()) }),
        ),
        Err(save_error) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("could not save: {save_error}"),
        ),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestartBody {
    #[serde(default)]
    confirm: bool,
}

async fn restart(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&app, &headers, true) {
        return refusal;
    }
    let Ok(RestartBody { confirm }) = object::<RestartBody>(&body) else {
        return error(StatusCode::BAD_REQUEST, "send {\"confirm\": true}");
    };
    if app.console.status().phase != Phase::Idle && !confirm {
        return error(
            StatusCode::CONFLICT,
            "a remote session is running: restarting ends it; confirm to go on",
        );
    }
    if !app.settings.managed {
        return reply(
            StatusCode::OK,
            &json!({ "restarting": false, "note": "this console was not started by systemd: stop it and start it again yourself" }),
        );
    }
    let unit = app.settings.unit.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "restart", "--no-block", &unit])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    });
    reply(StatusCode::OK, &json!({ "restarting": true }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApproveBody {
    id: u64,
    accept: bool,
}

async fn approve(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&app, &headers, true) {
        return refusal;
    }
    let Ok(ApproveBody { id, accept }) = object::<ApproveBody>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "send {\"id\": 1, \"accept\": true}",
        );
    };
    reply(
        StatusCode::OK,
        &json!({ "answered": app.console.decide_approval(id, accept) }),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AutostartBody {
    enabled: bool,
}

async fn autostart(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&app, &headers, true) {
        return refusal;
    }
    let Ok(AutostartBody { enabled }) = object::<AutostartBody>(&body) else {
        return error(StatusCode::BAD_REQUEST, "send {\"enabled\": true}");
    };
    let settings = Arc::clone(&app.settings);
    let result = tokio::task::spawn_blocking(move || {
        let result = set_autostart(&settings.home, &settings.unit_dirs, &settings.unit, enabled);
        if result.is_ok() {
            reload_systemd();
        }
        result
    })
    .await;
    match result {
        Ok(Ok(())) => reply(StatusCode::OK, &json!({ "autostart": enabled })),
        Ok(Err(message)) => error(StatusCode::CONFLICT, &message),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "autostart change failed"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialBody {
    action: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    device: String,
    #[serde(default)]
    revoke_devices: bool,
}

/// The `blackroom` arguments for an action, or why there are none. Nothing from the browser reaches the command line but a
/// device id made of letters, digits and `_.-`.
pub fn cli_args(
    settings: &Settings,
    body_action: &str,
    device: &str,
    revoke_devices: bool,
) -> Result<(Vec<String>, bool), String> {
    let base = |verb: &[&str]| {
        let mut args = vec![
            "--state-dir".to_string(),
            settings.state_dir.display().to_string(),
        ];
        args.extend(verb.iter().map(|part| (*part).to_string()));
        args
    };
    let account = settings.account.as_str();
    // The second value says whether the action needs the password again.
    Ok(match body_action {
        "status" => (base(&["status"]), false),
        // `setup` must be the first argument of the command.
        "setup" => (
            vec![
                "setup".into(),
                "--state-dir".into(),
                settings.state_dir.display().to_string(),
                "--account".into(),
                account.into(),
                "--no-login-check".into(),
            ],
            true,
        ),
        "devices" => (base(&["devices", "--account", account]), false),
        "rotate_key" => {
            let mut args = base(&["rotate-key", "--account", account]);
            if revoke_devices {
                args.push("--revoke-devices".into());
            }
            (args, true)
        }
        "recovery_codes" => (base(&["recovery-codes", "--account", account]), true),
        "reset_security" => (
            vec![
                "reset".into(),
                "security".into(),
                "--yes".into(),
                "--state-dir".into(),
                settings.state_dir.display().to_string(),
                "--account".into(),
                account.into(),
            ],
            true,
        ),
        "revoke_all" => (base(&["revoke-all"]), true),
        "disable" => (
            base(&[
                "disable",
                "--reason",
                "switched off from the host settings page",
            ]),
            true,
        ),
        "enable" => (base(&["enable"]), true),
        "revoke_device" => {
            let valid = !device.is_empty()
                && device.len() <= 80
                && device
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
            if !valid {
                return Err("the device id may hold letters, digits and _ . - only".into());
            }
            (base(&["revoke-device", device]), true)
        }
        _ => return Err("unknown action".into()),
    })
}

fn run_cli(cli: &Path, args: &[String], limit: Duration) -> Result<(bool, String, String), String> {
    use std::io::Read;
    let mut child = std::process::Command::new(cli)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run {}: {error}", cli.display()))?;
    let (mut stdout, mut stderr) = (child.stdout.take(), child.stderr.take());
    let out = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.take(256 * 1024).read_to_string(&mut text);
        }
        text
    });
    let err = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.take(64 * 1024).read_to_string(&mut text);
        }
        text
    });
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the command did not finish in time".into());
            }
        }
    };
    Ok((
        status.success(),
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    ))
}

async fn credentials(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&app, &headers, true) {
        return refusal;
    }
    let Ok(CredentialBody {
        action,
        password,
        device,
        revoke_devices,
    }) = object::<CredentialBody>(&body)
    else {
        return error(StatusCode::BAD_REQUEST, "send {\"action\": \"status\"}");
    };
    let (args, needs_password) = match cli_args(&app.settings, &action, &device, revoke_devices) {
        Ok(found) => found,
        Err(message) => return error(StatusCode::BAD_REQUEST, &message),
    };
    // Changing a credential asks for the password again, so a page left open on an unlocked laptop cannot do it.
    if needs_password && let Err(refusal) = check_password(&app, password).await {
        return refusal;
    }
    if !app.settings.cli.is_file() {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "the blackroom command is not installed next to the console",
        );
    }
    let cli = app.settings.cli.clone();
    match tokio::task::spawn_blocking(move || run_cli(&cli, &args, Duration::from_secs(60))).await {
        Ok(Ok((success, output, notice))) => reply(
            StatusCode::OK,
            &json!({ "ok": success, "output": output, "notice": notice.trim() }),
        ),
        Ok(Err(message)) => error(StatusCode::BAD_GATEWAY, &message),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the command failed to run",
        ),
    }
}

pub const LOCK_EXTENSION: &str = "blackroom-locked-remote@blackroom.local";

fn extension_list(tool: &Path, which: &[&str]) -> Vec<String> {
    let output = std::process::Command::new(tool)
        .arg("list")
        .args(which)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.trim().to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether the lock-screen extension is installed, switched on, and loaded by the Shell.
pub fn lock_state(tool: &Path) -> Value {
    let has = |list: Vec<String>| list.iter().any(|name| name == LOCK_EXTENSION);
    let installed = has(extension_list(tool, &[]));
    let enabled = has(extension_list(tool, &["--enabled"]));
    let active = has(extension_list(tool, &["--active"]));
    json!({ "installed": installed, "enabled": enabled, "active": active })
}

/// "Off" is only fully true once no remote session is open: the extension keeps a session that started on a locked
/// screen alive after it is switched off, so the page says "pending off" while one runs.
fn with_pending_off(mut state: Value, session_live: bool) -> Value {
    let off = state["installed"] == json!(true) && state["enabled"] == json!(false);
    state["pending_off"] = json!(off && session_live);
    state
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockBody {
    enabled: bool,
    #[serde(default)]
    password: String,
}

/// Turning remote use on the lock screen ON asks for the password: while it is on, locking no longer ends a remote session.
async fn lockscreen(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&app, &headers, true) {
        return refusal;
    }
    let Ok(LockBody { enabled, password }) = object::<LockBody>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "send {\"enabled\": true, \"password\": \"...\"}",
        );
    };
    if enabled && let Err(refusal) = check_password(&app, password).await {
        return refusal;
    }
    let tool = app.settings.gnome_extensions.clone();
    let verb = if enabled { "enable" } else { "disable" };
    let changed = tokio::task::spawn_blocking(move || {
        run_cli(
            &tool,
            &[verb.to_string(), LOCK_EXTENSION.to_string()],
            Duration::from_secs(20),
        )
        .map(|(ok, _, notice)| (ok, notice, lock_state(&tool)))
    })
    .await;
    match changed {
        Ok(Ok((true, _, state))) => reply(StatusCode::OK, &state),
        Ok(Ok((false, notice, _))) => error(
            StatusCode::BAD_GATEWAY,
            &format!(
                "gnome-extensions could not change it ({}). A newly installed extension is found after you log out and in.",
                notice.trim()
            ),
        ),
        Ok(Err(message)) => error(StatusCode::BAD_GATEWAY, &message),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the change failed to run",
        ),
    }
}

/// A new authenticator secret that has been shown but not yet proven with a code: the old one keeps working until then.
struct PendingTotp {
    secret: zeroize::Zeroizing<String>,
    since: Instant,
    wrong: u32,
}

const TOTP_LIFETIME: Duration = Duration::from_secs(10 * 60);
const TOTP_WRONG_CODES: u32 = 5;

fn totp_enrolled(settings: &Settings) -> bool {
    remote_hostd::store::open_state_directory(&settings.state_dir)
        .and_then(|directory| remote_hostd::totp::enrolled_accounts(&directory))
        .is_ok_and(|accounts| accounts.contains(&settings.account))
}

fn qr_svg(text: &str) -> Option<String> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    Some(
        code.render::<qrcode::render::svg::Color>()
            .min_dimensions(256, 256)
            .quiet_zone(true)
            .dark_color(qrcode::render::svg::Color("#000000"))
            .light_color(qrcode::render::svg::Color("#ffffff"))
            .build(),
    )
}

/// The setup key as the apps show it: groups of four.
fn grouped(secret: &str) -> String {
    secret
        .as_bytes()
        .chunks(4)
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TotpStartBody {
    password: String,
}

/// Step 1: a new secret and its QR code. Nothing is stored and the current authenticator keeps working.
async fn totp_start(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&app, &headers, true) {
        return refusal;
    }
    let Ok(TotpStartBody { password }) = object::<TotpStartBody>(&body) else {
        return error(StatusCode::BAD_REQUEST, "send {\"password\": \"...\"}");
    };
    if let Err(refusal) = check_password(&app, password).await {
        return refusal;
    }
    let Ok(secret) = remote_hostd::totp::generate_secret() else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "no random source");
    };
    let uri = remote_hostd::totp::otpauth_uri(&app.settings.account, &secret);
    let Some(svg) = qr_svg(&uri) else {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not draw the QR code",
        );
    };
    let shown = grouped(&secret);
    *lock(&app.totp) = Some(PendingTotp {
        secret,
        since: Instant::now(),
        wrong: 0,
    });
    reply(
        StatusCode::OK,
        &json!({
            "account": app.settings.account,
            "secret": shown,
            "uri": uri,
            "svg": svg,
            "expires_secs": TOTP_LIFETIME.as_secs(),
        }),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TotpVerifyBody {
    code: String,
}

/// Step 2: a code from the app proves the secret works; only then is it stored as the authenticator.
async fn totp_verify(State(app): State<App>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&app, &headers, true) {
        return refusal;
    }
    let Ok(TotpVerifyBody { code }) = object::<TotpVerifyBody>(&body) else {
        return error(StatusCode::BAD_REQUEST, "send {\"code\": \"123456\"}");
    };
    let settings = Arc::clone(&app.settings);
    let pending = Arc::clone(&app.totp);
    let result = tokio::task::spawn_blocking(move || {
        let mut slot = lock(&pending);
        let Some(current) = slot.as_mut() else {
            return Err((
                StatusCode::CONFLICT,
                "start again: there is no authenticator waiting to be confirmed".to_string(),
            ));
        };
        if current.since.elapsed() > TOTP_LIFETIME {
            *slot = None;
            return Err((
                StatusCode::CONFLICT,
                "that setup expired: start again".to_string(),
            ));
        }
        let step = remote_hostd::totp::matching_step(
            &current.secret,
            code.trim(),
            remote_hostd::totp::unix_now(),
        );
        let Some(step) = step else {
            current.wrong += 1;
            if current.wrong >= TOTP_WRONG_CODES {
                *slot = None;
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    "too many wrong codes: start again".to_string(),
                ));
            }
            return Err((
                StatusCode::UNAUTHORIZED,
                "that code is not right: check the app and try the next one".to_string(),
            ));
        };
        let saved = (|| -> std::io::Result<()> {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&settings.state_dir)?;
            let directory = remote_hostd::store::open_state_directory(&settings.state_dir)?;
            remote_hostd::totp::set_secret(&directory, &settings.account, &current.secret, step)
        })();
        if let Err(save_error) = saved {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("the code is right but the authenticator could not be saved: {save_error}"),
            ));
        }
        *slot = None;
        // The login authority reads the authenticator when it starts.
        let restarted = std::process::Command::new(&settings.systemctl)
            .args(["--user", "try-restart", "remote-hostd.service"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        Ok(restarted)
    })
    .await;
    match result {
        Ok(Ok(restarted)) => reply(
            StatusCode::OK,
            &json!({ "ok": true, "authority_restarted": restarted }),
        ),
        Ok(Err((code, message))) => error(code, &message),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "the check failed to run"),
    }
}

async fn totp_cancel(State(app): State<App>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&app, &headers, true) {
        return refusal;
    }
    *lock(&app.totp) = None;
    reply(StatusCode::OK, &json!({ "ok": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PW_CLI: &str = "\
\tid 45, type PipeWire:Interface:Node/3
\t\tnode.description = \"Built-in Audio Analog Stereo\"
\t\tnode.name = \"alsa_output.pci-0000_00_1f.3.analog-stereo\"
\t\tmedia.class = \"Audio/Sink\"
\tid 46, type PipeWire:Interface:Node/3
\t\tnode.name = \"alsa_input.pci-0000_00_1f.3.analog-stereo\"
\t\tmedia.class = \"Audio/Source\"
\tid 47, type PipeWire:Interface:Node/3
\t\tmedia.class = \"Audio/Sink\"
\t\tnode.name = \"blackroom_test_sink\"
";

    #[test]
    fn off_is_pending_while_a_session_runs() {
        let state = |installed: bool, enabled: bool| json!({ "installed": installed, "enabled": enabled, "active": false });
        assert_eq!(
            with_pending_off(state(true, false), true)["pending_off"],
            json!(true)
        );
        assert_eq!(
            with_pending_off(state(true, false), false)["pending_off"],
            json!(false)
        );
        assert_eq!(
            with_pending_off(state(true, true), true)["pending_off"],
            json!(false)
        );
        assert_eq!(
            with_pending_off(state(false, false), true)["pending_off"],
            json!(false)
        );
    }

    #[test]
    fn sinks_are_read_from_the_node_list() {
        let sinks = parse_sinks(PW_CLI);
        assert_eq!(sinks.len(), 2);
        assert_eq!(sinks[0].name, "alsa_output.pci-0000_00_1f.3.analog-stereo");
        assert_eq!(sinks[0].description, "Built-in Audio Analog Stereo");
        assert_eq!(
            sinks[1].description, "blackroom_test_sink",
            "no description: the name"
        );
        assert!(parse_sinks("").is_empty());
    }

    #[test]
    fn only_a_loopback_host_and_the_pages_own_origin_pass() {
        let headers = |host: &str, origin: Option<&str>| {
            let mut map = HeaderMap::new();
            map.insert(HOST, HeaderValue::from_str(host).unwrap());
            if let Some(origin) = origin {
                map.insert(ORIGIN, HeaderValue::from_str(origin).unwrap());
            }
            map
        };
        assert!(host_ok(&headers("127.0.0.1:8090", None), 8090));
        assert!(host_ok(&headers("localhost:8090", None), 8090));
        for bad in [
            "evil.example:8090",
            "127.0.0.1:8091",
            "127.0.0.1",
            "192.168.1.50:8090",
            "localhost.evil.com:8090",
        ] {
            assert!(!host_ok(&headers(bad, None), 8090), "{bad}");
        }
        assert!(origin_ok(&headers(
            "127.0.0.1:8090",
            Some("http://127.0.0.1:8090")
        )));
        for bad in [
            None,
            Some("http://evil.example"),
            Some("null"),
            Some("https://127.0.0.1:8090"),
            Some("http://localhost:8090"),
        ] {
            assert!(!origin_ok(&headers("127.0.0.1:8090", bad)), "{bad:?}");
        }
    }

    #[test]
    fn autostart_is_a_link_in_the_users_wants_directory() {
        let home = tempfile::tempdir().unwrap();
        let units = tempfile::tempdir().unwrap();
        let dirs = vec![units.path().to_path_buf()];
        assert!(
            set_autostart(home.path(), &dirs, "blackroom-console.service", true).is_err(),
            "no unit installed"
        );
        std::fs::write(
            units.path().join("blackroom-console.service"),
            "[Service]\nExecStart=/bin/true\n",
        )
        .unwrap();
        assert!(!autostart_enabled(home.path(), "blackroom-console.service"));
        set_autostart(home.path(), &dirs, "blackroom-console.service", true).unwrap();
        set_autostart(home.path(), &dirs, "blackroom-console.service", true).unwrap();
        assert!(autostart_enabled(home.path(), "blackroom-console.service"));
        let link = wants_link(home.path(), "blackroom-console.service");
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            units.path().join("blackroom-console.service")
        );
        set_autostart(home.path(), &dirs, "blackroom-console.service", false).unwrap();
        assert!(!autostart_enabled(home.path(), "blackroom-console.service"));
        set_autostart(home.path(), &dirs, "blackroom-console.service", false).unwrap();
        std::fs::write(&link, "not a link").unwrap();
        assert!(
            set_autostart(home.path(), &dirs, "blackroom-console.service", false).is_err(),
            "a real file is left alone"
        );
        assert!(link.exists());
    }
}
