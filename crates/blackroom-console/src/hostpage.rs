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
    pub cli: PathBuf,
    pub hostd_dir: PathBuf,
}

#[derive(Default)]
struct Sessions {
    live: HashMap<String, (Instant, Instant)>,
}

#[derive(Default)]
struct Limiter {
    failures: u32,
    locked_until: Option<Instant>,
}

#[derive(Clone)]
struct App {
    console: RemoteConsole,
    settings: Arc<Settings>,
    check: PasswordFactory,
    sessions: Arc<Mutex<Sessions>>,
    limiter: Arc<Mutex<Limiter>>,
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
    {
        let limiter = lock(&app.limiter);
        if limiter
            .locked_until
            .is_some_and(|until| Instant::now() < until)
        {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "too many wrong passwords: wait a minute",
            );
        }
    }
    let Ok(LoginBody { password }) = object::<LoginBody>(&body) else {
        return error(StatusCode::BAD_REQUEST, "send {\"password\": \"...\"}");
    };
    if !password_ok(&password) {
        return error(StatusCode::UNAUTHORIZED, "wrong password");
    }
    let account = app.settings.account.clone();
    let factory = Arc::clone(&app.check);
    let outcome = tokio::task::spawn_blocking(move || {
        let password = zeroize::Zeroizing::new(password);
        factory().check(&account, &password)
    })
    .await
    .unwrap_or(PasswordOutcome::Unavailable);
    match outcome {
        PasswordOutcome::Accepted => {
            lock(&app.limiter).failures = 0;
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
        PasswordOutcome::Rejected => {
            let mut limiter = lock(&app.limiter);
            limiter.failures += 1;
            if limiter.failures >= FAILURES_BEFORE_LOCK {
                limiter.failures = 0;
                limiter.locked_until = Some(Instant::now() + LOCK);
            }
            error(StatusCode::UNAUTHORIZED, "wrong password")
        }
        PasswordOutcome::Unavailable => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "the password check is not available (is the blackroom PAM helper installed?)",
        ),
    }
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
        json!({
            "account": settings.account,
            "host": status.host,
            "version": status.version,
            "config": saved,
            "restart_needed": saved != running,
            "note": note,
            "effective": settings.effective,
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
    match config.save(&dir) {
        Ok(_) => reply(
            StatusCode::OK,
            &json!({ "saved": true, "restart_needed": config != app.console.host_config() }),
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
