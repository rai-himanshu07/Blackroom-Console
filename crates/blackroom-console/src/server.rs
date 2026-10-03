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

const PAGE: &str = include_str!("page.html");
const LOGIN_PAGE: &str = include_str!("login.html");
const LOGIN_FULL_PAGE: &str = include_str!("login_full.html");
const LOGOUT_BUTTON: &str = r#"<button id="logout" onclick="fetch('/logout',{method:'POST',credentials:'same-origin'}).then(()=>location.reload())">Log out</button>"#;
const SESSION_COOKIE: &str = "br_session";
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
fn tokens_equal(a: &str, b: &str) -> bool {
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

fn cookie_value<'h>(headers: &'h HeaderMap, name: &str) -> Option<&'h str> {
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
        .route("/video", get(video))
        .route("/status", get(status))
        .route(
            "/input",
            post(input).layer(DefaultBodyLimit::max(MAX_INPUT_BODY)),
        )
        .route("/start", post(start))
        .route("/stop", post(stop))
        .route(
            "/webrtc",
            post(webrtc).layer(DefaultBodyLimit::max(MAX_INPUT_BODY)),
        )
        .route(
            "/quality",
            post(quality).layer(DefaultBodyLimit::max(MAX_INPUT_BODY)),
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
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self'; \
object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

async fn security_headers(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    let mut set = |name: &'static str, value: &'static str| {
        headers.insert(
            axum::http::HeaderName::from_static(name),
            axum::http::HeaderValue::from_static(value),
        );
    };
    set("content-security-policy", CONTENT_SECURITY_POLICY);
    set("x-content-type-options", "nosniff");
    set("x-frame-options", "DENY");
    set("referrer-policy", "no-referrer");
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

fn clipboard_refusal(error: &ClipboardError) -> Response {
    let status = match error {
        ClipboardError::Disabled => StatusCode::FORBIDDEN,
        ClipboardError::NotRunning => StatusCode::CONFLICT,
        ClipboardError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
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

async fn start(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let console = state.console.clone();
    match tokio::task::spawn_blocking(move || console.start()).await {
        Ok(Ok(status)) => Json(status).into_response(),
        Ok(Err(message)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": message })),
        )
            .into_response(),
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
