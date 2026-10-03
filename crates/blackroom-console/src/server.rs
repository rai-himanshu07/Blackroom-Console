//! The browser side: one page, an MJPEG stream, input, Start and Stop, behind one random token.
//! A silent browser is handled below this layer (`ConsoleConfig::heartbeat_timeout`).

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, LOCATION, ORIGIN, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;

use crate::clipboard::{self, ClipboardError};
use crate::console::{InputEvent, Quality, RemoteConsole};
use crate::hostd_auth::{HostdAuth, LoginForm, LoginOutcome};
use crate::login::{Login, LoginError};
use crate::options::SessionOptions;
use crate::profile::Profile;

const PAGE: &str = include_str!("web/index.html");
const APP_JS: &str = include_str!("web/app.js");
const APP_CSS: &str = include_str!("web/app.css");
const UI_JS: &str = include_str!("web/ui.js");
// Public on purpose (a browser fetches the manifest and worker without credentials): static files, no secrets.
const MANIFEST: &str = include_str!("web/manifest.webmanifest");
const SERVICE_WORKER: &str = include_str!("web/sw.js");
const ICON_SVG: &str = include_str!("web/icon.svg");
const ICON_192: &[u8] = include_bytes!("web/icons/icon-192.png");
const ICON_512: &[u8] = include_bytes!("web/icons/icon-512.png");
const ICON_MASKABLE: &[u8] = include_bytes!("web/icons/icon-maskable-512.png");
const LOGIN_PAGE: &str = include_str!("login.html");
const LOGIN_FULL_PAGE: &str = include_str!("login_full.html");
// No inline handler: the page's CSP allows only its own script files; ui.js wires this button by id.
const LOGOUT_BUTTON: &str = r#"<button type="button" id="logout" class="ghost">Log out</button>"#;
const SESSION_COOKIE: &str = "br_session";
/// Start options and (later) settings: small JSON documents.
const MAX_SETTINGS_BODY: usize = 16 * 1024;
const COOKIE_NAME: &str = "br_token";
const BOUNDARY: &str = "frame";
/// A frame is resent at least this often so a still desktop does not look like a dead stream.
const KEEPALIVE: Duration = Duration::from_secs(1);
const MAX_INPUT_BODY: usize = 64 * 1024;

#[derive(Clone)]
enum Auth {
    /// The default: one random token in the URL, kept as a cookie.
    Token(Arc<str>),
    /// A TOTP code per browser session (`--auth-dir`).
    Totp(Arc<Login>),
    /// Linux password, authenticator code and key or trusted device, checked by hostd (`--hostd-dir`).
    Hostd(Arc<HostdAuth>),
}

#[derive(Clone)]
struct AppState {
    console: RemoteConsole,
    auth: Auth,
    hardening: Hardening,
}

/// What differs between the plain-http listener and an https or public one.
#[derive(Clone, Copy, Default)]
pub struct Hardening {
    /// Cookies get the `Secure` flag (only ever sent over https).
    pub secure: bool,
    /// Adds `Strict-Transport-Security` (only for a real certificate on a public name).
    pub hsts: bool,
}

impl Hardening {
    fn cookie_flags(self) -> &'static str {
        if self.secure {
            "; HttpOnly; SameSite=Strict; Path=/; Secure"
        } else {
            "; HttpOnly; SameSite=Strict; Path=/"
        }
    }
}

/// 48 hex characters from the OS random source.
pub fn random_token() -> anyhow::Result<String> {
    use std::fmt::Write;
    let mut bytes = [0_u8; 24];
    getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!("random token: {error}"))?;
    Ok(bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    }))
}

/// For repeated test runs only: the token kept in `path` (created there with mode 0600 if missing or
/// malformed), so the URL does not change on every start. Default starts use a fresh random token.
pub fn token_from_file(path: &Path) -> anyhow::Result<String> {
    if let Ok(text) = std::fs::read_to_string(path) {
        let token = text.trim();
        if token.len() == 48 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(token.to_string());
        }
    }
    let token = random_token()?;
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("the token file needs a directory"))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("the token file needs a name"))?;
    crate::display::write_private(dir, name, token.as_bytes())?;
    Ok(token)
}

/// Compares in time independent of where the first difference is; the token length is public.
pub(crate) fn tokens_equal(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0_u8, |difference, (x, y)| difference | (x ^ y))
            == 0
}

fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| pair.trim().strip_prefix("br_token="))
}

pub(crate) fn cookie_value<'h>(headers: &'h HeaderMap, name: &str) -> Option<&'h str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(name)?.strip_prefix('='))
}

fn authorized(state: &AppState, headers: &HeaderMap) -> bool {
    match &state.auth {
        Auth::Token(token) => cookie_token(headers).is_some_and(|given| tokens_equal(given, token)),
        Auth::Totp(login) => cookie_value(headers, SESSION_COOKIE)
            .is_some_and(|id| login.check(id, state.console.emergency_count(), Instant::now())),
        Auth::Hostd(hostd) => cookie_value(headers, SESSION_COOKIE)
            .is_some_and(|token| hostd.check(token, state.console.emergency_count())),
    }
}

/// A browser sends `Origin` on cross-site posts; it must name this host. Plain clients send none.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN) else {
        return true;
    };
    let host = headers.get(HOST).and_then(|value| value.to_str().ok());
    origin
        .to_str()
        .ok()
        .and_then(|origin| origin.split_once("://"))
        .is_some_and(|(_, origin_host)| Some(origin_host) == host)
}

fn guard(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    if !authorized(state, headers) {
        return Some((StatusCode::UNAUTHORIZED, "open the URL printed at start\n").into_response());
    }
    if !same_origin(headers) {
        return Some((StatusCode::FORBIDDEN, "cross-origin request refused\n").into_response());
    }
    None
}

pub fn router(console: RemoteConsole, token: &str) -> Router {
    build(console, Auth::Token(Arc::from(token)), Hardening::default())
}

pub fn router_with(console: RemoteConsole, token: &str, hardening: Hardening) -> Router {
    build(console, Auth::Token(Arc::from(token)), hardening)
}

/// The same routes behind a TOTP login instead of the URL token.
pub fn router_totp(console: RemoteConsole, login: Arc<Login>) -> Router {
    build(console, Auth::Totp(login), Hardening::default())
}

pub fn router_totp_with(console: RemoteConsole, login: Arc<Login>, hardening: Hardening) -> Router {
    build(console, Auth::Totp(login), hardening)
}

/// The same routes behind hostd's three-factor login.
pub fn router_hostd(console: RemoteConsole, hostd: Arc<HostdAuth>) -> Router {
    build(console, Auth::Hostd(hostd), Hardening::default())
}

pub fn router_hostd_with(
    console: RemoteConsole,
    hostd: Arc<HostdAuth>,
    hardening: Hardening,
) -> Router {
    build(console, Auth::Hostd(hostd), hardening)
}

fn build(console: RemoteConsole, auth: Auth, hardening: Hardening) -> Router {
    let state = AppState {
        console,
        auth,
        hardening,
    };
    Router::new()
        .route("/login", post(login).layer(DefaultBodyLimit::max(4096)))
        .route("/logout", post(logout))
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/app.css", get(app_css))
        .route("/ui.js", get(ui_js))
        .route("/manifest.webmanifest", get(manifest))
        .route("/sw.js", get(service_worker))
        .route(
            "/icon.svg",
            get(|| async {
                (
                    [
                        (CONTENT_TYPE, "image/svg+xml"),
                        (CACHE_CONTROL, "public, max-age=86400"),
                    ],
                    ICON_SVG,
                )
            }),
        )
        .route(
            "/icon-192.png",
            get(|| async {
                (
                    [
                        (CONTENT_TYPE, "image/png"),
                        (CACHE_CONTROL, "public, max-age=86400"),
                    ],
                    ICON_192,
                )
            }),
        )
        .route(
            "/icon-512.png",
            get(|| async {
                (
                    [
                        (CONTENT_TYPE, "image/png"),
                        (CACHE_CONTROL, "public, max-age=86400"),
                    ],
                    ICON_512,
                )
            }),
        )
        .route(
            "/icon-maskable-512.png",
            get(|| async {
                (
                    [
                        (CONTENT_TYPE, "image/png"),
                        (CACHE_CONTROL, "public, max-age=86400"),
                    ],
                    ICON_MASKABLE,
                )
            }),
        )
        .route("/video", get(video))
        .route("/status", get(status))
        .route(
            "/input",
            post(input).layer(DefaultBodyLimit::max(MAX_INPUT_BODY)),
        )
        .route(
            "/settings",
            get(settings_get)
                .post(settings_set)
                .layer(DefaultBodyLimit::max(MAX_SETTINGS_BODY)),
        )
        .route("/settings/reset", post(settings_reset))
        .route(
            "/start",
            post(start).layer(DefaultBodyLimit::max(MAX_SETTINGS_BODY)),
        )
        .route("/stop", post(stop))
        .route(
            "/webrtc",
            post(webrtc).layer(DefaultBodyLimit::max(MAX_INPUT_BODY)),
        )
        .route(
            "/quality",
            post(quality).layer(DefaultBodyLimit::max(MAX_INPUT_BODY)),
        )
        .route(
            "/audio",
            post(audio).layer(DefaultBodyLimit::max(MAX_INPUT_BODY)),
        )
        .route(
            "/tuning",
            post(tuning).layer(DefaultBodyLimit::max(MAX_INPUT_BODY)),
        )
        .route("/ice", get(ice))
        .route(
            "/clipboard",
            get(clipboard_get)
                .post(clipboard_set)
                .layer(DefaultBodyLimit::max(clipboard::MAX_BYTES + 1024)),
        )
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            security_headers,
        ))
        .layer(axum::middleware::map_request(host_from_authority))
}

/// HTTP/2 carries the host as `:authority` and no `Host` header; the same-origin check reads the header.
async fn host_from_authority(mut request: axum::extract::Request) -> axum::extract::Request {
    if !request.headers().contains_key(HOST)
        && let Some(authority) = request.uri().authority().cloned()
        && let Ok(value) = authority.as_str().parse()
    {
        request.headers_mut().insert(HOST, value);
    }
    request
}

#[derive(Deserialize)]
struct IndexQuery {
    t: Option<String>,
}

async fn index(
    State(state): State<AppState>,
    Query(query): Query<IndexQuery>,
    headers: HeaderMap,
) -> Response {
    if let Auth::Token(token) = &state.auth
        && let Some(given) = query.t
    {
        if !tokens_equal(&given, token) {
            return (StatusCode::UNAUTHORIZED, "wrong token\n").into_response();
        }
        let cookie = format!("{COOKIE_NAME}={given}{}", state.hardening.cookie_flags());
        return (
            StatusCode::SEE_OTHER,
            [(SET_COOKIE, cookie), (LOCATION, "/".to_string())],
        )
            .into_response();
    }
    if !authorized(&state, &headers) {
        return match state.auth {
            Auth::Token(_) => {
                (StatusCode::UNAUTHORIZED, "open the URL printed at start\n").into_response()
            }
            Auth::Totp(_) => ([(CACHE_CONTROL, "no-store")], Html(LOGIN_PAGE)).into_response(),
            Auth::Hostd(_) => {
                ([(CACHE_CONTROL, "no-store")], Html(LOGIN_FULL_PAGE)).into_response()
            }
        };
    }
    let page = match state.auth {
        Auth::Hostd(_) => PAGE.replace("<!--LOGOUT-->", LOGOUT_BUTTON),
        _ => PAGE.replace("<!--LOGOUT-->", ""),
    };
    ([(CACHE_CONTROL, "no-store")], Html(page)).into_response()
}

#[derive(Deserialize)]
struct LoginBody {
    code: String,
}

async fn login(
    State(state): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Auth::Hostd(hostd) = &state.auth {
        return login_hostd(&state, hostd, peer, &headers, &body).await;
    }
    let Auth::Totp(login) = &state.auth else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !same_origin(&headers) {
        return (StatusCode::FORBIDDEN, "cross-origin request refused\n").into_response();
    }
    let Some(LoginBody { code }) = parse_body(&body) else {
        return malformed();
    };
    let (login, epoch) = (Arc::clone(login), state.console.emergency_count());
    let result = tokio::task::spawn_blocking(move || {
        login.login(&code, epoch, Instant::now(), || random_token().ok())
    })
    .await;
    match result {
        Ok(Ok(id)) => (
            StatusCode::NO_CONTENT,
            [(
                SET_COOKIE,
                format!("{SESSION_COOKIE}={id}{}", state.hardening.cookie_flags()),
            )],
        )
            .into_response(),
        Ok(Err(LoginError::Locked)) => {
            (StatusCode::TOO_MANY_REQUESTS, "too many attempts, wait\n").into_response()
        }
        Ok(Err(_)) => (StatusCode::UNAUTHORIZED, "refused\n").into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// One failed-attempt counter per browser address (hostd keys its limits on this).
fn client_id(peer: Peer) -> String {
    peer.0.map_or_else(
        || "ip-unknown".to_string(),
        |address| format!("ip-{}", address.ip()),
    )
}

/// The browser's address when the server was started with connect info; none in unit tests.
struct Peer(Option<SocketAddr>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for Peer {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Infallible> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(address)| *address),
        ))
    }
}

fn session_cookie(token: &str, hardening: Hardening) -> String {
    format!("{SESSION_COOKIE}={token}{}", hardening.cookie_flags())
}

async fn login_hostd(
    state: &AppState,
    hostd: &Arc<HostdAuth>,
    peer: Peer,
    headers: &HeaderMap,
    body: &Bytes,
) -> Response {
    if !same_origin(headers) {
        return (StatusCode::FORBIDDEN, "cross-origin request refused\n").into_response();
    }
    let Some(form) = parse_body::<LoginForm>(body) else {
        return malformed();
    };
    let (hostd, client, emergency) = (
        Arc::clone(hostd),
        client_id(peer),
        state.console.emergency_count(),
    );
    let outcome = tokio::task::spawn_blocking(move || hostd.login(&form, &client, emergency)).await;
    match outcome {
        Ok(LoginOutcome::Accepted { token, device }) => {
            let reply = match device {
                Some((id, secret)) => {
                    serde_json::json!({"ok": true, "device_id": id, "device_secret": secret.as_str()})
                }
                None => serde_json::json!({"ok": true}),
            };
            (
                StatusCode::OK,
                [
                    (SET_COOKIE, session_cookie(&token, state.hardening)),
                    (CACHE_CONTROL, "no-store".to_string()),
                ],
                Json(reply),
            )
                .into_response()
        }
        Ok(LoginOutcome::Refused(code)) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "code": code })),
        )
            .into_response(),
        Ok(LoginOutcome::Locked) => {
            (StatusCode::TOO_MANY_REQUESTS, "too many attempts, wait\n").into_response()
        }
        Ok(LoginOutcome::Unavailable) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "login service unavailable\n",
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Auth::Hostd(hostd) = &state.auth else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !same_origin(&headers) {
        return (StatusCode::FORBIDDEN, "cross-origin request refused\n").into_response();
    }
    if let Some(token) = cookie_value(&headers, SESSION_COOKIE) {
        let (hostd, token) = (Arc::clone(hostd), token.to_string());
        let _ = tokio::task::spawn_blocking(move || hostd.logout(&token)).await;
    }
    (
        StatusCode::NO_CONTENT,
        [(
            SET_COOKIE,
            format!(
                "{SESSION_COOKIE}={}; Max-Age=0",
                state.hardening.cookie_flags()
            ),
        )],
    )
        .into_response()
}

/// The page loads nothing from elsewhere and cannot be framed; the policy keeps inline script because the
/// page is one self-contained file.
/// Standard padded base64 (CSP hash sources), without a dependency for twelve lines.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |n, (i, byte)| n | (u32::from(*byte) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The `'sha256-...'` source for every inline script of the pages: they are the only scripts that run, so
/// `script-src` needs no `'unsafe-inline'` and an injected script would not execute.
fn script_hash_sources(pages: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut sources = Vec::new();
    for page in pages {
        let mut rest = *page;
        while let Some(start) = rest.find("<script>") {
            let body = &rest[start + "<script>".len()..];
            let Some(end) = body.find("</script>") else {
                break;
            };
            sources.push(format!(
                "'sha256-{}'",
                base64(&Sha256::digest(&body.as_bytes()[..end]))
            ));
            rest = &body[end..];
        }
    }
    sources.join(" ")
}

fn content_security_policy() -> &'static str {
    static POLICY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    POLICY.get_or_init(|| {
        format!(
            "default-src 'self'; script-src 'self' {}; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
media-src 'self' blob:; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
            script_hash_sources(&[LOGIN_PAGE, LOGIN_FULL_PAGE])
        )
    })
}

async fn security_headers(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    // Nothing here is worth keeping in a browser cache or a proxy (the page, status, clipboard, login replies).
    if !headers.contains_key(CACHE_CONTROL) {
        headers.insert(
            CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
    }
    if let Ok(value) = axum::http::HeaderValue::from_str(content_security_policy()) {
        headers.insert(
            axum::http::HeaderName::from_static("content-security-policy"),
            value,
        );
    }
    let mut set = |name: &'static str, value: &'static str| {
        headers.insert(
            axum::http::HeaderName::from_static(name),
            axum::http::HeaderValue::from_static(value),
        );
    };
    set("x-content-type-options", "nosniff");
    set("x-frame-options", "DENY");
    set("referrer-policy", "no-referrer");
    set("cross-origin-resource-policy", "same-origin");
    set("cross-origin-opener-policy", "same-origin");
    set(
        "permissions-policy",
        "camera=(), microphone=(), geolocation=(), payment=()",
    );
    if state.hardening.hsts {
        set("strict-transport-security", "max-age=31536000");
    }
    response
}

/// `RTCConfiguration` for the logged-in browser: STUN and short-lived TURN credentials.
async fn ice(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    (
        [(CACHE_CONTROL, "no-store")],
        Json(crate::ice::config().browser_servers(std::time::SystemTime::now())),
    )
        .into_response()
}

async fn status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    Json(state.console.status()).into_response()
}

/// Parsed after the auth check so an unauthenticated caller learns nothing from validation errors.
fn parse_body<T: serde::de::DeserializeOwned>(body: &[u8]) -> Option<T> {
    serde_json::from_slice(body).ok()
}

fn malformed() -> Response {
    (StatusCode::BAD_REQUEST, "malformed body\n").into_response()
}

async fn input(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let Some(events): Option<Vec<InputEvent>> = parse_body(&body) else {
        return malformed();
    };
    match state.console.input(events) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(message) if message == "not running" => (StatusCode::CONFLICT, message).into_response(),
        Err(message) => (StatusCode::BAD_REQUEST, message).into_response(),
    }
}

/// A JSON object only: serde would also read a bare array as a struct's fields in order.
fn json_object<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, String> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|error| error.to_string())?;
    if !value.is_object() {
        return Err("expected a JSON object".into());
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

async fn settings_get(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    Json(state.console.profile()).into_response()
}

async fn settings_set(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let result =
        json_object::<Profile>(&body).and_then(|profile| state.console.set_profile(profile));
    match result {
        Ok(profile) => Json(profile).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error })),
        )
            .into_response(),
    }
}

async fn settings_reset(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    match state.console.set_profile(Profile::default()) {
        Ok(profile) => Json(profile).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error })),
        )
            .into_response(),
    }
}

fn clipboard_refusal(error: &ClipboardError) -> Response {
    let status = match error {
        ClipboardError::Disabled => StatusCode::FORBIDDEN,
        ClipboardError::NotRunning => StatusCode::CONFLICT,
        ClipboardError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        ClipboardError::Invalid(_) => StatusCode::BAD_REQUEST,
        ClipboardError::TooFast => StatusCode::TOO_MANY_REQUESTS,
        ClipboardError::NoText => StatusCode::NOT_FOUND,
        ClipboardError::Failed(_) => StatusCode::BAD_GATEWAY,
    };
    (status, format!("{error}\n")).into_response()
}

/// The browser's text goes onto the laptop clipboard (body: the text, UTF-8).
async fn clipboard_set(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let Ok(text) = String::from_utf8(body.to_vec()) else {
        return malformed();
    };
    let console = state.console.clone();
    match tokio::task::spawn_blocking(move || console.clipboard_set(text)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => clipboard_refusal(&error),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The laptop clipboard as plain text, never cached.
async fn clipboard_get(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let console = state.console.clone();
    match tokio::task::spawn_blocking(move || console.clipboard_get()).await {
        Ok(Ok(text)) => (
            [
                (CONTENT_TYPE, "text/plain; charset=utf-8"),
                (CACHE_CONTROL, "no-store"),
            ],
            text,
        )
            .into_response(),
        Ok(Err(error)) => clipboard_refusal(&error),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn start(
    State(state): State<AppState>,
    peer: Peer,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let options = if body.iter().all(u8::is_ascii_whitespace) {
        state.console.default_options()
    } else {
        match json_object::<SessionOptions>(&body).and_then(SessionOptions::validated) {
            Ok(options) => options,
            Err(error) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("session options: {error}") })),
                )
                    .into_response();
            }
        }
    };
    let options = match state.console.apply_policy(options) {
        Ok(options) => options,
        Err(message) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": message })),
            )
                .into_response();
        }
    };
    let console = state.console.clone();
    let device = format!(
        "{} ({})",
        peer.0.map_or_else(
            || "unknown address".to_string(),
            |address| address.ip().to_string()
        ),
        crate::approval::clean(
            headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("unknown browser"),
            60
        )
    );
    let mode = options.label();
    let started = tokio::task::spawn_blocking(move || {
        if let Err(message) = console.ask_approval(mode, &device) {
            return Err((StatusCode::FORBIDDEN, message));
        }
        console
            .start(options)
            .map_err(|message| (StatusCode::CONFLICT, message))
    })
    .await;
    match started {
        Ok(Err((code, message))) => {
            (code, Json(serde_json::json!({ "error": message }))).into_response()
        }
        Ok(Ok(status)) => Json(status).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn stop(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let console = state.console.clone();
    match tokio::task::spawn_blocking(move || console.stop()).await {
        Ok(report) => Json(report).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
struct OfferBody {
    sdp: String,
}

async fn webrtc(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let Some(offer): Option<OfferBody> = parse_body(&body) else {
        return malformed();
    };
    let console = state.console.clone();
    match tokio::task::spawn_blocking(move || console.webrtc_answer(&offer.sdp)).await {
        Ok(Ok(sdp)) => Json(serde_json::json!({ "sdp": sdp })).into_response(),
        Ok(Err(message)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": message })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
struct QualityBody {
    level: Quality,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioBody {
    enabled: bool,
}

async fn audio(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let Some(body): Option<AudioBody> = parse_body(&body) else {
        return malformed();
    };
    state.console.set_audio(body.enabled);
    Json(state.console.status()).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TuningBody {
    fps_cap: u32,
    bitrate_kbps: u32,
}

/// Frame-rate ceiling and bitrate of the running session; 0 follows the quality level.
async fn tuning(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let Some(body): Option<TuningBody> = parse_body(&body) else {
        return malformed();
    };
    match state.console.set_rates(body.fps_cap, body.bitrate_kbps) {
        Ok(()) => Json(state.console.status()).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error })),
        )
            .into_response(),
    }
}

async fn quality(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let Some(body): Option<QualityBody> = parse_body(&body) else {
        return malformed();
    };
    state.console.set_quality(body.level);
    Json(state.console.status()).into_response()
}

fn mjpeg_part(jpeg: &[u8]) -> Bytes {
    let mut part = format!(
        "--{BOUNDARY}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
        jpeg.len()
    )
    .into_bytes();
    part.extend_from_slice(jpeg);
    part.extend_from_slice(b"\r\n");
    Bytes::from(part)
}

async fn app_js(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    ([(CONTENT_TYPE, "text/javascript; charset=utf-8")], APP_JS).into_response()
}

async fn manifest() -> Response {
    (
        [
            (CONTENT_TYPE, "application/manifest+json"),
            (CACHE_CONTROL, "no-cache"),
        ],
        MANIFEST,
    )
        .into_response()
}

async fn service_worker() -> Response {
    (
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
            (
                axum::http::HeaderName::from_static("service-worker-allowed"),
                "/",
            ),
        ],
        SERVICE_WORKER,
    )
        .into_response()
}

async fn ui_js(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    ([(CONTENT_TYPE, "text/javascript; charset=utf-8")], UI_JS).into_response()
}

async fn app_css(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    ([(CONTENT_TYPE, "text/css; charset=utf-8")], APP_CSS).into_response()
}

async fn video(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let Some(slot) = state.console.video() else {
        return (StatusCode::CONFLICT, "not running\n").into_response();
    };
    // A full channel blocks the reader thread, so a slow client skips frames instead of lagging.
    let (parts, mut receiver) = tokio::sync::mpsc::channel::<Bytes>(2);
    tokio::task::spawn_blocking(move || {
        let (mut seen, mut newest) = (0, None);
        loop {
            match slot.next_after(seen, KEEPALIVE) {
                Some((seq, frame)) => {
                    seen = seq;
                    newest = Some(frame);
                }
                None if slot.is_closed() => break,
                None => {}
            }
            if let Some(frame) = &newest
                && parts.blocking_send(mjpeg_part(frame)).is_err()
            {
                break;
            }
        }
    });
    let stream = futures_util::stream::poll_fn(move |cx| receiver.poll_recv(cx))
        .map(Ok::<Bytes, Infallible>);
    Response::builder()
        .header(
            CONTENT_TYPE,
            format!("multipart/x-mixed-replace; boundary={BOUNDARY}"),
        )
        .header(CACHE_CONTROL, "no-store")
        .body(Body::from_stream(stream))
        .expect("static response parts are valid")
}

#[cfg(test)]
mod tests {
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;
    use crate::console::ConsoleConfig;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn a_token_file_keeps_one_private_token_across_calls_and_replaces_garbage() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("br-token-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("token");
        let first = token_from_file(&path).unwrap();
        assert_eq!(first.len(), 48);
        assert_eq!(token_from_file(&path).unwrap(), first);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::write(&path, "short").unwrap();
        let replaced = token_from_file(&path).unwrap();
        assert_eq!(replaced.len(), 48);
        assert_ne!(replaced, first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn app() -> Router {
        let console = RemoteConsole::spawn(ConsoleConfig {
            grab_socket: None,
            state_dir: std::env::temp_dir().join("br-console-server-test"),
            headless: true,
            quality: crate::console::Quality::Medium,
            heartbeat_timeout: Duration::from_secs(15),
            restore_bin: std::path::PathBuf::new(),
        });
        router(console, TOKEN)
    }

    async fn call(app: &Router, request: Request<Body>) -> Response {
        app.clone().oneshot(request).await.expect("a response")
    }

    fn with_cookie(method: &str, uri: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(COOKIE, format!("{COOKIE_NAME}={TOKEN}"))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    const SECRET: &[u8] = b"12345678901234567890";

    fn totp_app(clock: Arc<std::sync::atomic::AtomicU64>) -> (Router, RemoteConsole) {
        let console = RemoteConsole::spawn(ConsoleConfig {
            grab_socket: None,
            state_dir: std::env::temp_dir().join("br-console-server-test-totp"),
            headless: true,
            quality: crate::console::Quality::Medium,
            heartbeat_timeout: Duration::from_secs(15),
            restore_bin: std::path::PathBuf::new(),
        });
        let mut verifier = remote_hostd::totp::TotpVerifier::new(Default::default())
            .with_clock(move || clock.load(std::sync::atomic::Ordering::SeqCst));
        assert!(verifier.add_account("owner", SECRET.to_vec(), 0));
        let login = Arc::new(Login::new("owner", verifier));
        (router_totp(console.clone(), login), console)
    }

    fn login_request(code: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(format!("{{\"code\":\"{code}\"}}")))
            .unwrap()
    }

    fn session_request(session: &str) -> Request<Body> {
        Request::builder()
            .uri("/status")
            .header(COOKIE, format!("{SESSION_COOKIE}={session}"))
            .body(Body::empty())
            .unwrap()
    }

    fn session_of(response: &Response) -> String {
        let cookie = response.headers()[SET_COOKIE].to_str().unwrap();
        let (_, rest) = cookie.split_once('=').expect("name=value");
        rest.split(';').next().unwrap().to_string()
    }

    #[tokio::test]
    async fn totp_mode_needs_a_code_and_an_emergency_ends_the_session() {
        use remote_hostd::totp::totp;
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(1_000_000));
        let (app, console) = totp_app(Arc::clone(&clock));

        let page = call(
            &app,
            Request::builder().uri("/").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(page.status(), StatusCode::OK);
        let unauthenticated = call(
            &app,
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            call(&app, login_request("000000")).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(
                &app,
                Request::builder()
                    .method("POST")
                    .uri("/login")
                    .body(Body::from("not json"))
                    .unwrap()
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );

        let accepted = call(&app, login_request(&totp(SECRET, 1_000_000))).await;
        assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
        let session = session_of(&accepted);
        assert_eq!(
            call(&app, session_request(&session)).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            call(&app, session_request("forged")).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&app, login_request(&totp(SECRET, 1_000_000)))
                .await
                .status(),
            StatusCode::UNAUTHORIZED,
            "a used code is refused"
        );

        console.note_emergency();
        assert_eq!(
            call(&app, session_request(&session)).await.status(),
            StatusCode::UNAUTHORIZED
        );

        clock.store(1_000_090, std::sync::atomic::Ordering::SeqCst);
        let again = call(&app, login_request(&totp(SECRET, 1_000_090))).await;
        assert_eq!(again.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            call(&app, session_request(&session_of(&again)))
                .await
                .status(),
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn token_mode_has_no_login() {
        let response = call(&app(), login_request("123456")).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // ---- login through hostd (real authority on temporary sockets, fake password check) ----

    struct Rig {
        _state: tempfile::TempDir,
        runtime: tempfile::TempDir,
        clock: Arc<std::sync::atomic::AtomicU64>,
        secret: Vec<u8>,
        key: String,
        stop: Arc<std::sync::atomic::AtomicBool>,
        server: Option<std::thread::JoinHandle<()>>,
    }

    struct Pw;

    impl remote_hostd::password::PasswordCheck for Pw {
        fn check(&mut self, _: &str, password: &str) -> remote_hostd::password::PasswordOutcome {
            if password == "pw" {
                remote_hostd::password::PasswordOutcome::Accepted
            } else {
                remote_hostd::password::PasswordOutcome::Rejected
            }
        }
    }

    impl Rig {
        fn new() -> Self {
            use std::os::unix::fs::PermissionsExt;
            use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
            let private = || {
                let dir = tempfile::tempdir().unwrap();
                std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
                    .unwrap();
                dir
            };
            let (state, runtime) = (private(), private());
            let fd = std::fs::File::open(state.path()).unwrap();
            let secret = remote_hostd::totp::base32_decode(
                &remote_hostd::totp::enroll(&fd, "owner").unwrap(),
            )
            .unwrap();
            let store = blackroom_store::SecretStore::open(&fd).unwrap();
            let key = remote_hostd::access_key::rotate(&store, "owner")
                .unwrap()
                .to_string();
            let clock = Arc::new(AtomicU64::new(1_800_000_000));
            let (c1, c2) = (Arc::clone(&clock), Arc::clone(&clock));
            let totp = remote_hostd::totp::load_verifier(&fd, Default::default())
                .unwrap()
                .with_clock(move || c1.load(Ordering::SeqCst));
            let limiter = remote_hostd::ratelimit::FailureLimiter::new(Default::default())
                .with_clock(move || c2.load(Ordering::SeqCst));
            let verifier = remote_hostd::login::MultiFactorVerifier::new(
                blackroom_store::SecretStore::open(&fd).unwrap(),
                totp,
                None,
                limiter,
            );
            let daemon = Arc::new(
                remote_hostd::authd::AuthDaemon::new(
                    &fd,
                    verifier,
                    Box::new(remote_hostd::authd::OwnerOnly),
                )
                .unwrap(),
            );
            let auth =
                remote_hostd::authd::bind_private(runtime.path(), remote_hostd::authd::AUTH_SOCKET)
                    .unwrap();
            let admin = remote_hostd::authd::bind_private(
                runtime.path(),
                remote_hostd::authd::ADMIN_SOCKET,
            )
            .unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let server = {
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    let pam: remote_hostd::authd::PamFactory =
                        Arc::new(|| Box::new(Pw) as Box<dyn remote_hostd::password::PasswordCheck>);
                    remote_hostd::authd::serve(&daemon, auth, admin, &pam, &stop).unwrap();
                })
            };
            Self {
                _state: state,
                runtime,
                clock,
                secret,
                key,
                stop,
                server: Some(server),
            }
        }

        fn code(&self) -> String {
            let now = self
                .clock
                .fetch_add(30, std::sync::atomic::Ordering::SeqCst)
                + 30;
            remote_hostd::totp::totp(&self.secret, now)
        }

        fn app(&self) -> (Router, RemoteConsole) {
            let console = RemoteConsole::spawn(ConsoleConfig {
                grab_socket: None,
                state_dir: std::env::temp_dir().join("br-console-server-test-hostd"),
                headless: true,
                quality: crate::console::Quality::Medium,
                heartbeat_timeout: Duration::from_secs(15),
                restore_bin: std::path::PathBuf::new(),
            });
            let hostd = Arc::new(HostdAuth::new(
                self.runtime.path().to_path_buf(),
                console.emergency_count(),
            ));
            (router_hostd(console.clone(), hostd), console)
        }

        fn admin(&self, command: &str) {
            let _: remote_hostd::authd::AdminReply = remote_hostd::authd::call(
                &self.runtime.path().join(remote_hostd::authd::ADMIN_SOCKET),
                &serde_json::json!({ "command": command }),
            )
            .unwrap();
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(server) = self.server.take() {
                let _ = server.join();
            }
        }
    }

    fn json_login(body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    async fn text_of(response: Response) -> String {
        String::from_utf8(
            axum::body::to_bytes(response.into_body(), 1 << 20)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    }

    fn page_request(session: &str) -> Request<Body> {
        Request::builder()
            .uri("/")
            .header(COOKIE, format!("{SESSION_COOKIE}={session}"))
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hostd_login_needs_all_factors_trusts_a_browser_and_dies_with_an_emergency() {
        let rig = Rig::new();
        let (app, console) = rig.app();

        let page = call(
            &app,
            Request::builder().uri("/").body(Body::empty()).unwrap(),
        )
        .await;
        assert!(text_of(page).await.contains("Linux password"));
        let status = call(
            &app,
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status.status(), StatusCode::UNAUTHORIZED);

        let wrong = call(
            &app,
            json_login(serde_json::json!({"account": "owner", "password": "bad", "code": rig.code(), "access_key": rig.key})),
        )
        .await;
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(body_json(wrong).await["code"], "AUTH_INVALID");
        let no_key = call(
            &app,
            json_login(
                serde_json::json!({"account": "owner", "password": "pw", "code": rig.code()}),
            ),
        )
        .await;
        assert_eq!(body_json(no_key).await["code"], "AUTH_ACCESS_KEY_REQUIRED");
        let extra = call(&app, json_login(serde_json::json!({"account": "owner", "password": "pw", "code": "1", "admin": true}))).await;
        assert_eq!(extra.status(), StatusCode::BAD_REQUEST);

        let first = call(
            &app,
            json_login(serde_json::json!({"account": "owner", "password": "pw", "code": rig.code(), "access_key": rig.key, "trust_label": "Tablet"})),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        let first_cookie = session_of(&first);
        let reply = body_json(first).await;
        let (device_id, device_secret) = (
            reply["device_id"].as_str().unwrap().to_string(),
            reply["device_secret"].as_str().unwrap().to_string(),
        );
        assert_eq!(
            call(&app, session_request(&first_cookie)).await.status(),
            StatusCode::OK
        );
        assert!(
            text_of(call(&app, page_request(&first_cookie)).await)
                .await
                .contains("id=\"logout\"")
        );

        let by_device = call(
            &app,
            json_login(serde_json::json!({"account": "owner", "password": "pw", "code": rig.code(), "device_id": device_id, "device_secret": device_secret})),
        )
        .await;
        assert_eq!(by_device.status(), StatusCode::OK);
        let second_cookie = session_of(&by_device);
        assert!(body_json(by_device).await.get("device_id").is_none());

        console.note_emergency();
        assert_eq!(
            call(&app, session_request(&first_cookie)).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&app, session_request(&second_cookie)).await.status(),
            StatusCode::UNAUTHORIZED,
            "the authority ended every session too"
        );

        let again = call(
            &app,
            json_login(serde_json::json!({"account": "owner", "password": "pw", "code": rig.code(), "access_key": rig.key})),
        )
        .await;
        assert_eq!(again.status(), StatusCode::OK);
        let third = session_of(&again);
        assert_eq!(
            call(&app, session_request(&third)).await.status(),
            StatusCode::OK
        );

        let cross = Request::builder()
            .method("POST")
            .uri("/logout")
            .header(COOKIE, format!("{SESSION_COOKIE}={third}"))
            .header(ORIGIN, "http://evil.example")
            .header(HOST, "console.local")
            .body(Body::empty())
            .unwrap();
        assert_eq!(call(&app, cross).await.status(), StatusCode::FORBIDDEN);
        let out = Request::builder()
            .method("POST")
            .uri("/logout")
            .header(COOKIE, format!("{SESSION_COOKIE}={third}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(call(&app, out).await.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            call(&app, session_request(&third)).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hostd_disable_ends_sessions_and_a_stopped_hostd_refuses_logins() {
        let rig = Rig::new();
        let (app, _console) = rig.app();
        let login = |rig: &Rig| {
            json_login(
                serde_json::json!({"account": "owner", "password": "pw", "code": rig.code(), "access_key": rig.key}),
            )
        };
        let ok = call(&app, login(&rig)).await;
        let cookie = session_of(&ok);
        rig.admin("disable");
        assert_eq!(
            call(&app, session_request(&cookie)).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&app, login(&rig)).await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        rig.admin("enable");
        assert_eq!(call(&app, login(&rig)).await.status(), StatusCode::OK);

        drop(rig);
        let down = call(
            &app,
            json_login(serde_json::json!({"account": "owner", "password": "pw", "code": "000000", "access_key": "x"})),
        )
        .await;
        assert_eq!(down.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn token_and_totp_modes_have_no_logout_or_logout_button() {
        let response = call(
            &app(),
            Request::builder()
                .method("POST")
                .uri("/logout")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    fn hardened(hardening: Hardening) -> Router {
        let console = RemoteConsole::spawn(ConsoleConfig {
            grab_socket: None,
            state_dir: std::env::temp_dir().join("br-console-server-test-hardening"),
            headless: true,
            quality: crate::console::Quality::Medium,
            heartbeat_timeout: Duration::from_secs(15),
            restore_bin: std::path::PathBuf::new(),
        });
        router_with(console, TOKEN, hardening)
    }

    #[tokio::test]
    async fn every_response_carries_the_security_headers_and_hsts_only_when_asked() {
        for (hardening, hsts) in [
            (
                Hardening {
                    secure: false,
                    hsts: false,
                },
                false,
            ),
            (
                Hardening {
                    secure: true,
                    hsts: true,
                },
                true,
            ),
        ] {
            let app = hardened(hardening);
            for uri in ["/", "/status", "/nowhere"] {
                let response = call(
                    &app,
                    Request::builder().uri(uri).body(Body::empty()).unwrap(),
                )
                .await;
                let headers = response.headers();
                let csp = headers["content-security-policy"].to_str().unwrap();
                assert!(
                    csp.contains("default-src 'self'") && csp.contains("frame-ancestors 'none'"),
                    "{uri}"
                );
                assert_eq!(headers["x-content-type-options"], "nosniff");
                assert_eq!(headers["x-frame-options"], "DENY");
                assert_eq!(headers["referrer-policy"], "no-referrer");
                assert_eq!(
                    headers.contains_key("strict-transport-security"),
                    hsts,
                    "{uri}"
                );
            }
        }
    }

    #[tokio::test]
    async fn the_secure_flag_is_set_only_on_the_secure_listener_and_the_ice_list_needs_a_login() {
        for secure in [false, true] {
            let app = hardened(Hardening {
                secure,
                hsts: false,
            });
            let response = call(
                &app,
                Request::builder()
                    .uri(format!("/?t={TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            let cookie = response.headers()[SET_COOKIE].to_str().unwrap().to_string();
            assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
            assert_eq!(cookie.contains("; Secure"), secure, "{cookie}");

            let anonymous = call(
                &app,
                Request::builder().uri("/ice").body(Body::empty()).unwrap(),
            )
            .await;
            assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
            let authorised = call(&app, with_cookie("GET", "/ice", "")).await;
            assert_eq!(authorised.status(), StatusCode::OK);
            assert_eq!(authorised.headers()["cache-control"], "no-store");
            let body = body_json(authorised).await;
            assert!(body["iceServers"].is_array());
        }
    }

    #[test]
    fn tokens_compare_exactly() {
        assert!(tokens_equal(TOKEN, TOKEN));
        assert!(!tokens_equal(TOKEN, &TOKEN[..47]));
        assert!(!tokens_equal(TOKEN, &format!("{}0", &TOKEN[..47])));
        let token = random_token().unwrap();
        assert_eq!(token.len(), 48);
        assert_ne!(token, random_token().unwrap());
    }

    #[tokio::test]
    async fn the_page_needs_the_token_and_sets_an_http_only_strict_cookie() {
        let app = app();
        let bare = call(&app, Request::get("/").body(Body::empty()).unwrap()).await;
        assert_eq!(bare.status(), StatusCode::UNAUTHORIZED);
        let wrong = call(&app, Request::get("/?t=nope").body(Body::empty()).unwrap()).await;
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

        let right = call(
            &app,
            Request::get(format!("/?t={TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(right.status(), StatusCode::SEE_OTHER);
        let cookie = right.headers()[SET_COOKIE].to_str().unwrap();
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
        assert_eq!(right.headers()[LOCATION], "/");

        let page = call(&app, with_cookie("GET", "/", "")).await;
        assert_eq!(page.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn every_action_needs_the_cookie() {
        let app = app();
        for (method, uri) in [
            ("GET", "/video"),
            ("GET", "/status"),
            ("POST", "/input"),
            ("POST", "/start"),
            ("POST", "/stop"),
            ("POST", "/webrtc"),
            ("POST", "/quality"),
        ] {
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from("[]"))
                .unwrap();
            assert_eq!(
                call(&app, request).await.status(),
                StatusCode::UNAUTHORIZED,
                "{uri}"
            );
        }
    }

    #[tokio::test]
    async fn cross_origin_posts_are_refused() {
        let app = app();
        let mut request = with_cookie("POST", "/stop", "");
        request
            .headers_mut()
            .insert(HOST, "laptop:8080".parse().unwrap());
        request
            .headers_mut()
            .insert(ORIGIN, "http://evil.example".parse().unwrap());
        assert_eq!(call(&app, request).await.status(), StatusCode::FORBIDDEN);

        let mut same = with_cookie("POST", "/stop", "");
        same.headers_mut()
            .insert(HOST, "laptop:8080".parse().unwrap());
        same.headers_mut()
            .insert(ORIGIN, "http://laptop:8080".parse().unwrap());
        assert_eq!(call(&app, same).await.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn http2_style_requests_without_a_host_header_pass_the_origin_check() {
        let app = app();
        let request = Request::builder()
            .method("POST")
            .uri("https://laptop:8443/stop")
            .header(COOKIE, format!("{COOKIE_NAME}={TOKEN}"))
            .header(ORIGIN, "https://laptop:8443")
            .body(Body::empty())
            .unwrap();
        assert_eq!(call(&app, request).await.status(), StatusCode::OK);
        let foreign = Request::builder()
            .method("POST")
            .uri("https://laptop:8443/stop")
            .header(COOKIE, format!("{COOKIE_NAME}={TOKEN}"))
            .header(ORIGIN, "https://evil.example")
            .body(Body::empty())
            .unwrap();
        assert_eq!(call(&app, foreign).await.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn quality_levels_are_validated_and_webrtc_needs_a_running_session() {
        let app = app();
        let bad = call(
            &app,
            with_cookie("POST", "/quality", r#"{"level":"ultra"}"#),
        )
        .await;
        assert!(bad.status().is_client_error());
        let good = call(&app, with_cookie("POST", "/quality", r#"{"level":"high"}"#)).await;
        assert_eq!(good.status(), StatusCode::OK);
        let status = call(&app, with_cookie("GET", "/status", "")).await;
        let body = axum::body::to_bytes(status.into_body(), 1 << 16)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains(r#""quality":"high""#));
        let idle = call(&app, with_cookie("POST", "/webrtc", r#"{"sdp":"v=0"}"#)).await;
        assert_eq!(idle.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn clipboard_is_guarded_off_by_default_and_size_capped() {
        let app = app();
        let anonymous = Request::builder()
            .method("GET")
            .uri("/clipboard")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            call(&app, anonymous).await.status(),
            StatusCode::UNAUTHORIZED
        );
        // Off unless the console was started with --clipboard, in both directions.
        let off = call(&app, with_cookie("GET", "/clipboard", "")).await;
        assert_eq!(off.status(), StatusCode::FORBIDDEN);
        let off = call(&app, with_cookie("POST", "/clipboard", "hello")).await;
        assert_eq!(off.status(), StatusCode::FORBIDDEN);
        let big = "x".repeat(clipboard::MAX_BYTES + 1);
        let big = call(&app, with_cookie("POST", "/clipboard", &big)).await;
        assert_eq!(big.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let huge = "x".repeat(clipboard::MAX_BYTES + 4096);
        let huge = call(&app, with_cookie("POST", "/clipboard", &huge)).await;
        assert_eq!(huge.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn an_enabled_clipboard_still_needs_a_running_session() {
        let console = RemoteConsole::spawn(ConsoleConfig {
            grab_socket: None,
            state_dir: std::env::temp_dir().join("br-console-server-test-clip"),
            headless: true,
            quality: crate::console::Quality::Medium,
            heartbeat_timeout: Duration::from_secs(15),
            restore_bin: std::path::PathBuf::new(),
        });
        console.set_clipboard_enabled(true);
        let app = router(console, TOKEN);
        let idle = call(&app, with_cookie("POST", "/clipboard", "hello")).await;
        assert_eq!(idle.status(), StatusCode::CONFLICT);
        let idle = call(&app, with_cookie("GET", "/clipboard", "")).await;
        assert_eq!(idle.status(), StatusCode::CONFLICT);
        let status = call(&app, with_cookie("GET", "/status", "")).await;
        let body = axum::body::to_bytes(status.into_body(), 1 << 16)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("\"clipboard\":true"));
    }

    #[tokio::test]
    async fn settings_round_trip_validate_and_reset() {
        let app = app();
        let anonymous = Request::builder()
            .uri("/settings")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            call(&app, anonymous).await.status(),
            StatusCode::UNAUTHORIZED
        );
        let read = async |app: &Router| {
            let response = call(app, with_cookie("GET", "/settings", "")).await;
            assert_eq!(response.status(), StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), 1 << 16)
                .await
                .unwrap();
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()
        };
        let first = read(&app).await;
        assert_eq!(first["session"]["blank_panel"], true);
        assert_eq!(first["client"]["scale"], "fit");

        let shared = r#"{"session":{"blank_panel":false,"block_local_input":false,"lock_on_stop":false},"client":{"scale":"actual","volume":40}}"#;
        let saved = call(&app, with_cookie("POST", "/settings", shared)).await;
        assert_eq!(saved.status(), StatusCode::OK);
        let now = read(&app).await;
        assert_eq!(now["session"]["blank_panel"], false);
        assert_eq!(now["client"]["scale"], "actual");
        assert_eq!(now["client"]["volume"], 40);

        for bad in [
            r#"{"client":{"volume":101}}"#,
            r#"{"client":{"scale":"huge"}}"#,
            r#"{"session":{"heartbeat_secs":1}}"#,
            r#"{"version":9}"#,
            r#"{"unknown":1}"#,
            "[1]",
        ] {
            let refused = call(&app, with_cookie("POST", "/settings", bad)).await;
            assert_eq!(refused.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        assert_eq!(
            read(&app).await["client"]["volume"],
            40,
            "a refused save changes nothing"
        );

        let reset = call(&app, with_cookie("POST", "/settings/reset", "")).await;
        assert_eq!(reset.status(), StatusCode::OK);
        assert_eq!(read(&app).await["session"]["blank_panel"], true);
    }

    #[tokio::test]
    async fn saved_settings_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("br-profile-server-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let make = |dir: &std::path::Path| {
            let console = RemoteConsole::spawn(ConsoleConfig {
                grab_socket: None,
                state_dir: std::env::temp_dir().join("br-console-server-test-profile"),
                headless: true,
                quality: crate::console::Quality::Medium,
                heartbeat_timeout: Duration::from_secs(15),
                restore_bin: std::path::PathBuf::new(),
            });
            assert_eq!(console.load_profile(dir.to_path_buf()), None);
            router(console, TOKEN)
        };
        let app = make(&dir);
        let saved = call(
            &app,
            with_cookie(
                "POST",
                "/settings",
                r#"{"client":{"quality":"high","mac_keys":true}}"#,
            ),
        )
        .await;
        assert_eq!(saved.status(), StatusCode::OK);
        let again = make(&dir);
        let response = call(&again, with_cookie("GET", "/settings", "")).await;
        let body = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["client"]["quality"], "high");
        assert_eq!(json["client"]["mac_keys"], true);
        let status = call(&again, with_cookie("GET", "/status", "")).await;
        let body = axum::body::to_bytes(status.into_body(), 1 << 16)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("\"quality\":\"high\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_page_uses_an_inline_event_handler_the_policy_would_block() {
        let handler = |text: &str| {
            text.split(|c: char| !c.is_ascii_alphanumeric())
                .any(|word| {
                    word.len() > 2 && word.starts_with("on") && text.contains(&format!(" {word}="))
                })
        };
        for (name, text) in [
            ("index", PAGE),
            ("login", LOGIN_PAGE),
            ("login_full", LOGIN_FULL_PAGE),
            ("logout button", LOGOUT_BUTTON),
        ] {
            assert!(!handler(text), "{name} has an inline event handler");
            assert!(
                !text.contains("javascript:"),
                "{name} has a javascript: URL"
            );
        }
    }

    #[tokio::test]
    async fn status_reports_the_process_resources() {
        let app = app();
        let status = call(&app, with_cookie("GET", "/status", "")).await;
        let body = axum::body::to_bytes(status.into_body(), 1 << 16)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let resources = &json["resources"];
        assert!(resources["rss_kb"].as_u64().unwrap() > 0);
        assert!(resources["open_fds"].as_u64().unwrap() >= 3);
        assert!(resources["threads"].as_u64().unwrap() >= 1);
        assert_eq!(resources["sessions_started"], 0);
        assert_eq!(resources["sessions_stopped"], 0);
        // Only counts and sizes: nothing that names a path, user or secret.
        assert_eq!(resources.as_object().unwrap().len(), 6);
    }

    #[tokio::test]
    async fn input_and_video_need_a_running_session_and_valid_events() {
        let app = app();
        let idle = call(&app, with_cookie("POST", "/input", "[]")).await;
        assert_eq!(idle.status(), StatusCode::CONFLICT);
        let video = call(&app, with_cookie("GET", "/video", "")).await;
        assert_eq!(video.status(), StatusCode::CONFLICT);
        let malformed = call(&app, with_cookie("POST", "/input", r#"[{"t":"exec"}]"#)).await;
        assert!(malformed.status().is_client_error());
        let stop = call(&app, with_cookie("POST", "/stop", "")).await;
        assert_eq!(stop.status(), StatusCode::OK);
    }
}
