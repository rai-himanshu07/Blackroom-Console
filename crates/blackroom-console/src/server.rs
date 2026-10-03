//! The browser side: one page, an MJPEG stream, input, Start and Stop, behind one random token.
//! A silent browser is handled below this layer (`ConsoleConfig::heartbeat_timeout`).

use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, LOCATION, ORIGIN, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;

use crate::console::{InputEvent, Quality, RemoteConsole};
use crate::login::{Login, LoginError};

const PAGE: &str = include_str!("page.html");
const LOGIN_PAGE: &str = include_str!("login.html");
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
}

#[derive(Clone)]
struct AppState {
    console: RemoteConsole,
    auth: Auth,
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
    build(console, Auth::Token(Arc::from(token)))
}

/// The same routes behind a TOTP login instead of the URL token.
pub fn router_totp(console: RemoteConsole, login: Arc<Login>) -> Router {
    build(console, Auth::Totp(login))
}

fn build(console: RemoteConsole, auth: Auth) -> Router {
    let state = AppState { console, auth };
    Router::new()
        .route("/login", post(login).layer(DefaultBodyLimit::max(1024)))
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
        .with_state(state)
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
        let cookie = format!("{COOKIE_NAME}={given}; HttpOnly; SameSite=Strict; Path=/");
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
        };
    }
    ([(CACHE_CONTROL, "no-store")], Html(PAGE)).into_response()
}

#[derive(Deserialize)]
struct LoginBody {
    code: String,
}

async fn login(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
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
                format!("{SESSION_COOKIE}={id}; HttpOnly; SameSite=Strict; Path=/"),
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
