//! The browser side: one page, an MJPEG stream, input, Start and Stop, behind one random token.
//! A silent browser is handled below this layer (`ConsoleConfig::heartbeat_timeout`).

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, LOCATION, ORIGIN, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;

use crate::console::{InputEvent, LOCKED_MESSAGE, Quality, RemoteConsole};

const PAGE: &str = include_str!("page.html");
const COOKIE_NAME: &str = "br_token";
const BOUNDARY: &str = "frame";
/// A frame is resent at least this often so a still desktop does not look like a dead stream.
const KEEPALIVE: Duration = Duration::from_secs(1);
const MAX_INPUT_BODY: usize = 64 * 1024;

#[derive(Clone)]
struct AppState {
    console: RemoteConsole,
    /// True on the https listener: unlocking is only offered where the cookie cannot be sniffed.
    secure: bool,
    token: Arc<str>,
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

fn authorized(state: &AppState, headers: &HeaderMap) -> bool {
    cookie_token(headers).is_some_and(|token| tokens_equal(token, &state.token))
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

pub fn router(console: RemoteConsole, token: &str, secure: bool) -> Router {
    let state = AppState {
        console,
        secure,
        token: Arc::from(token),
    };
    Router::new()
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
    if let Some(given) = query.t {
        if !tokens_equal(&given, &state.token) {
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
        return (StatusCode::UNAUTHORIZED, "open the URL printed at start\n").into_response();
    }
    ([(CACHE_CONTROL, "no-store")], Html(PAGE)).into_response()
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

#[derive(Deserialize)]
struct StartBody {
    #[serde(default)]
    unlock: bool,
}

/// Why a locked screen cannot be unlocked from this connection; empty when it can.
fn unlock_hint(state: &AppState) -> &'static str {
    if !state.console.remote_unlock_enabled() {
        "Unlock the laptop locally, or restart the console with --remote-unlock."
    } else if !state.secure {
        "Open the https URL to unlock remotely."
    } else {
        ""
    }
}

async fn start(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(refusal) = guard(&state, &headers) {
        return refusal;
    }
    let unlock = if body.is_empty() {
        false
    } else {
        match parse_body::<StartBody>(&body) {
            Some(parsed) => parsed.unlock,
            None => return malformed(),
        }
    };
    if unlock && !unlock_hint(&state).is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": unlock_hint(&state) })),
        )
            .into_response();
    }
    let console = state.console.clone();
    match tokio::task::spawn_blocking(move || console.start(unlock)).await {
        Ok(Ok(status)) => Json(status).into_response(),
        Ok(Err(message)) => {
            let hint = unlock_hint(&state);
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": message,
                    "locked": message == LOCKED_MESSAGE,
                    "unlock_available": hint.is_empty(),
                    "hint": hint,
                })),
            )
                .into_response()
        }
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

    fn app() -> Router {
        let console = RemoteConsole::spawn(ConsoleConfig {
            grab_socket: None,
            state_dir: std::env::temp_dir().join("br-console-server-test"),
            headless: true,
            quality: crate::console::Quality::Medium,
            heartbeat_timeout: Duration::from_secs(15),
            restore_bin: std::path::PathBuf::new(),
            remote_unlock: false,
        });
        router(console, TOKEN, false)
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
    async fn unlocking_needs_the_opt_in_and_an_https_listener() {
        let app = app();
        let refused = call(&app, with_cookie("POST", "/start", r#"{"unlock":true}"#)).await;
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
        let malformed = call(&app, with_cookie("POST", "/start", r#"{"unlock":"yes"}"#)).await;
        assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
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
