//! Adversarial HTTP tests that need no hardware: what a hostile browser, page or network peer can and cannot do
//! against the console's routes. They never start a session (no Shell, no display, no input device).

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, LOCATION, ORIGIN, SET_COOKIE};
use axum::http::{Method, Request, Response, StatusCode};
use blackroom_console::server::{Hardening, router_with};
use blackroom_console::{ConsoleConfig, InputEvent, Quality, RemoteConsole};
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";
const COOKIE_NAME: &str = "br_token";
const POSTS: [&str; 11] = [
    "/input",
    "/start",
    "/stop",
    "/webrtc",
    "/quality",
    "/clipboard",
    "/logout",
    "/settings",
    "/settings/reset",
    "/tuning",
    "/audio",
];
const GETS: [&str; 8] = [
    "/",
    "/video",
    "/status",
    "/ice",
    "/clipboard",
    "/settings",
    "/app.js",
    "/app.css",
];

fn app(hardening: Hardening) -> Router {
    let console = RemoteConsole::spawn(ConsoleConfig {
        grab_socket: None,
        state_dir: std::env::temp_dir().join("br-adversarial-state"),
        headless: true,
        quality: Quality::Medium,
        heartbeat_timeout: Duration::from_secs(15),
        restore_bin: std::path::PathBuf::new(),
    });
    console.set_clipboard_enabled(true);
    router_with(console, TOKEN, hardening)
}

async fn send(app: &Router, request: Request<Body>) -> Response<Body> {
    app.clone().oneshot(request).await.expect("a response")
}

fn request(
    method: Method,
    uri: &str,
    cookie: bool,
    extra: &[(&str, &str)],
    body: Vec<u8>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(HOST, "127.0.0.1:8080");
    if cookie {
        builder = builder.header(COOKIE, format!("{COOKIE_NAME}={TOKEN}"));
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::from(body)).unwrap()
}

/// Cheap deterministic generator: the same inputs on every run, so a failure can be reproduced.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[tokio::test]
async fn every_route_refuses_an_anonymous_caller() {
    let app = app(Hardening::default());
    for uri in GETS {
        let response = send(&app, request(Method::GET, uri, false, &[], vec![])).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "GET {uri}");
    }
    for uri in POSTS.iter().filter(|uri| **uri != "/logout") {
        let response = send(&app, request(Method::POST, uri, false, &[], b"[]".to_vec())).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "POST {uri}");
    }
}

#[tokio::test]
async fn near_miss_credentials_are_refused() {
    let app = app(Hardening::default());
    let wrong_last = format!("{}0", &TOKEN[..TOKEN.len() - 1]);
    let attempts = [
        String::new(),
        COOKIE_NAME.to_string(),
        format!("{COOKIE_NAME}="),
        format!("{COOKIE_NAME}={}", &TOKEN[..TOKEN.len() - 1]),
        format!("{COOKIE_NAME}={TOKEN}0"),
        format!("{COOKIE_NAME}={}", TOKEN.to_uppercase()),
        format!("{COOKIE_NAME}={wrong_last}"),
        format!("x{COOKIE_NAME}={TOKEN}"),
        format!("{COOKIE_NAME}x={TOKEN}"),
        format!("{COOKIE_NAME}=\"{TOKEN}\""),
        format!("other={TOKEN}"),
    ];
    for cookie in attempts {
        let response = send(
            &app,
            Request::builder()
                .uri("/status")
                .header(COOKIE, cookie.as_str())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "cookie {cookie:?}"
        );
    }
    // The token in the URL is exchanged for a cookie only when right, and never echoed back.
    let wrong = send(&app, request(Method::GET, "/?t=nope", false, &[], vec![])).await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert!(wrong.headers().get(SET_COOKIE).is_none());
    let right = send(
        &app,
        request(Method::GET, &format!("/?t={TOKEN}"), false, &[], vec![]),
    )
    .await;
    assert_eq!(right.status(), StatusCode::SEE_OTHER);
    assert_eq!(right.headers()[LOCATION], "/");
    let cookie = right.headers()[SET_COOKIE].to_str().unwrap().to_string();
    for flag in ["HttpOnly", "SameSite=Strict", "Path=/"] {
        assert!(cookie.contains(flag), "{cookie}");
    }
    assert!(!cookie.contains("Domain"), "{cookie}");
    assert!(
        !cookie.contains("Secure"),
        "plain http must not set Secure: {cookie}"
    );
}

#[tokio::test]
async fn an_https_listener_sets_secure_cookies_and_hsts_only_when_asked() {
    let secure = app(Hardening {
        secure: true,
        hsts: true,
    });
    let response = send(
        &secure,
        request(Method::GET, &format!("/?t={TOKEN}"), false, &[], vec![]),
    )
    .await;
    assert!(
        response.headers()[SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Secure")
    );
    assert!(response.headers().contains_key("strict-transport-security"));
    let plain = app(Hardening::default());
    let response = send(
        &plain,
        request(Method::GET, &format!("/?t={TOKEN}"), false, &[], vec![]),
    )
    .await;
    assert!(!response.headers().contains_key("strict-transport-security"));
}

#[tokio::test]
async fn a_foreign_origin_cannot_drive_any_state_changing_route() {
    let app = app(Hardening::default());
    let hostile = [
        "http://evil.example",
        "https://evil.example",
        "null",
        "http://127.0.0.1:8080.evil.example",
        "http://evil.example/127.0.0.1:8080",
        "http://127.0.0.1:8081",
        "http://localhost:8080",
        "file://",
        "",
    ];
    for uri in POSTS.iter().filter(|uri| **uri != "/logout") {
        for origin in hostile {
            let response = send(
                &app,
                request(
                    Method::POST,
                    uri,
                    true,
                    &[("origin", origin)],
                    b"[]".to_vec(),
                ),
            )
            .await;
            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "POST {uri} from {origin:?}"
            );
        }
        let own = send(
            &app,
            request(
                Method::POST,
                uri,
                true,
                &[(ORIGIN.as_str(), "http://127.0.0.1:8080")],
                b"[]".to_vec(),
            ),
        )
        .await;
        assert_ne!(
            own.status(),
            StatusCode::FORBIDDEN,
            "POST {uri} from its own origin"
        );
    }
    // Reads are guarded the same way: a foreign page cannot read the clipboard or the status.
    for uri in GETS.iter().filter(|uri| **uri != "/") {
        let response = send(
            &app,
            request(
                Method::GET,
                uri,
                true,
                &[("origin", "http://evil.example")],
                vec![],
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "GET {uri}");
    }
}

#[tokio::test]
async fn no_cors_permission_is_ever_granted() {
    let app = app(Hardening::default());
    for (method, uri) in [
        (Method::OPTIONS, "/status"),
        (Method::OPTIONS, "/input"),
        (Method::GET, "/status"),
        (Method::POST, "/input"),
    ] {
        let response = send(
            &app,
            request(
                method.clone(),
                uri,
                true,
                &[
                    ("origin", "http://evil.example"),
                    ("access-control-request-method", "POST"),
                ],
                b"[]".to_vec(),
            ),
        )
        .await;
        assert!(
            !response
                .headers()
                .keys()
                .any(|name| name.as_str().starts_with("access-control-")),
            "{method} {uri}: {:?}",
            response.headers()
        );
    }
    for method in [
        Method::TRACE,
        Method::CONNECT,
        Method::PUT,
        Method::DELETE,
        Method::PATCH,
    ] {
        let response = send(&app, request(method.clone(), "/status", true, &[], vec![])).await;
        assert!(
            response.status().is_client_error(),
            "{method} gave {}",
            response.status()
        );
    }
}

#[tokio::test]
async fn every_response_carries_the_security_headers_and_a_script_hash_policy() {
    let app = app(Hardening::default());
    let cases = [
        request(Method::GET, "/", true, &[], vec![]),
        request(Method::GET, "/", false, &[], vec![]),
        request(Method::GET, "/status", true, &[], vec![]),
        request(Method::GET, "/status", false, &[], vec![]),
        request(Method::GET, "/missing", false, &[], vec![]),
        request(Method::POST, "/", true, &[], vec![]),
    ];
    for case in cases {
        let label = format!("{} {}", case.method(), case.uri());
        let response = send(&app, case).await;
        let headers = response.headers();
        let csp = headers["content-security-policy"]
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            csp.contains("default-src 'self'") && csp.contains("frame-ancestors 'none'"),
            "{label}: {csp}"
        );
        assert!(
            csp.contains("object-src 'none'") && csp.contains("base-uri 'none'"),
            "{label}: {csp}"
        );
        let scripts = csp
            .split(';')
            .find(|part| part.trim().starts_with("script-src"))
            .unwrap();
        assert!(
            !scripts.contains("unsafe-inline")
                && !scripts.contains("unsafe-eval")
                && !scripts.contains('*'),
            "{label}: {scripts}"
        );
        assert!(scripts.contains("'sha256-"), "{label}: {scripts}");
        assert_eq!(headers["x-content-type-options"], "nosniff", "{label}");
        assert_eq!(headers["x-frame-options"], "DENY", "{label}");
        assert_eq!(headers["referrer-policy"], "no-referrer", "{label}");
        assert_eq!(
            headers["cross-origin-resource-policy"], "same-origin",
            "{label}"
        );
        assert_eq!(
            headers["cross-origin-opener-policy"], "same-origin",
            "{label}"
        );
        assert!(headers.contains_key("permissions-policy"), "{label}");
        assert_eq!(headers[CACHE_CONTROL], "no-store", "{label}");
    }
}

#[tokio::test]
async fn oversized_bodies_are_refused_before_they_are_read() {
    let app = app(Hardening::default());
    for (uri, size) in [
        ("/input", 70 * 1024),
        ("/quality", 70 * 1024),
        ("/webrtc", 70 * 1024),
        ("/clipboard", 262_144 + 4096),
    ] {
        let response = send(
            &app,
            request(Method::POST, uri, true, &[], vec![b'a'; size]),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE, "{uri}");
    }
}

#[tokio::test]
async fn hostile_bodies_never_cause_a_server_error() {
    let app = app(Hardening::default());
    let mut random = Lcg(0x5eed);
    let valid = br#"[{"t":"move","x":0.5,"y":0.5},{"t":"key","code":30,"down":true},{"t":"scroll","dx":1.0,"dy":-2.0}]"#;
    let mut bodies: Vec<Vec<u8>> = vec![
        vec![],
        b"null".to_vec(),
        b"{}".to_vec(),
        b"[null]".to_vec(),
        b"[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[".to_vec(),
        "[".repeat(100_000).into_bytes(),
        br#"[{"t":"move","x":1e999,"y":0}]"#.to_vec(),
        br#"[{"t":"move","x":"NaN","y":0}]"#.to_vec(),
        br#"[{"t":"key","code":-1,"down":true}]"#.to_vec(),
        br#"[{"t":"key","code":4294967296,"down":true}]"#.to_vec(),
        br#"[{"t":"button","code":272,"down":"yes"}]"#.to_vec(),
        br#"[{"t":"exec","cmd":"rm -rf /"}]"#.to_vec(),
        br#"{"level":"ludicrous"}"#.to_vec(),
        "\u{0}\u{1}\u{2}".as_bytes().to_vec(),
        valid.to_vec(),
    ];
    for _ in 0..300 {
        let mut body = valid.to_vec();
        for _ in 0..1 + random.below(6) {
            if body.is_empty() {
                break;
            }
            let at = random.below(body.len() as u64) as usize;
            match random.below(4) {
                0 => body[at] = random.below(256) as u8,
                1 => body.truncate(at),
                2 => body.insert(at, random.below(256) as u8),
                _ => {
                    body.remove(at);
                }
            }
        }
        bodies.push(body);
    }
    for _ in 0..100 {
        bodies.push(
            (0..random.below(512))
                .map(|_| random.below(256) as u8)
                .collect(),
        );
    }
    for body in bodies {
        for uri in [
            "/input",
            "/quality",
            "/webrtc",
            "/clipboard",
            "/settings",
            "/tuning",
        ] {
            let response = send(
                &app,
                request(
                    Method::POST,
                    uri,
                    true,
                    &[(CONTENT_TYPE.as_str(), "application/json")],
                    body.clone(),
                ),
            )
            .await;
            assert!(
                !response.status().is_server_error(),
                "POST {uri} with {} bytes gave {}",
                body.len(),
                response.status()
            );
            // An empty object is a valid (all defaults) settings document.
            if uri != "/settings" {
                assert_ne!(
                    response.status(),
                    StatusCode::OK,
                    "POST {uri} accepted hostile input"
                );
            }
        }
    }
}

#[tokio::test]
async fn odd_paths_never_reach_a_handler_without_credentials() {
    let app = app(Hardening::default());
    for uri in [
        "//status",
        "/status/",
        "/%73tatus",
        "/../status",
        "/status/..",
        "/./status",
        "/status%00",
        "/status?x=%ff",
        "/STATUS",
        "/Status",
    ] {
        let response = send(&app, request(Method::GET, uri, false, &[], vec![])).await;
        assert!(
            matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST
            ),
            "{uri} gave {}",
            response.status()
        );
    }
}

#[test]
fn input_validation_holds_for_every_generated_event() {
    let mut random = Lcg(0xfeed);
    let mut accepted = 0_u32;
    for _ in 0..20_000 {
        let text = match random.below(4) {
            0 => format!(
                r#"{{"t":"key","code":{},"down":{}}}"#,
                random.below(0x400),
                random.below(2) == 0
            ),
            1 => format!(
                r#"{{"t":"button","code":{},"down":true}}"#,
                0x100 + random.below(0x30)
            ),
            2 => format!(
                r#"{{"t":"move","x":{}.{},"y":-{}.{}}}"#,
                random.below(5),
                random.below(1000),
                random.below(5),
                random.below(1000)
            ),
            _ => format!(
                r#"{{"t":"scroll","dx":{}.5,"dy":-{}.25}}"#,
                random.below(5000),
                random.below(5000)
            ),
        };
        let Ok(event) = serde_json::from_str::<InputEvent>(&text) else {
            continue;
        };
        let Ok(valid) = event.validate() else {
            continue;
        };
        accepted += 1;
        match valid {
            InputEvent::Key { code, .. } => assert!(
                (1..=0x2ff).contains(&code) && ![116, 142, 143, 205].contains(&code),
                "{text}"
            ),
            InputEvent::Button { code, .. } => assert!((0x110..=0x117).contains(&code), "{text}"),
            InputEvent::Move { x, y } => assert!(x.is_finite() && y.is_finite(), "{text}"),
            InputEvent::Scroll { dx, dy } => assert!(
                dx.is_finite() && dy.is_finite() && dx.abs() <= 1000.0 && dy.abs() <= 1000.0,
                "{text}"
            ),
            InputEvent::Text { s } => assert!(s.chars().count() <= 256, "{text}"),
        }
    }
    assert!(
        accepted > 1000,
        "the generator should produce plenty of valid events ({accepted})"
    );
    for event in [
        InputEvent::Move {
            x: f32::NAN,
            y: 0.0,
        },
        InputEvent::Move {
            x: 0.0,
            y: f32::INFINITY,
        },
        InputEvent::Scroll {
            dx: f32::NEG_INFINITY,
            dy: 0.0,
        },
        InputEvent::Scroll {
            dx: 1000.5,
            dy: 0.0,
        },
    ] {
        assert!(event.clone().validate().is_err(), "{event:?}");
    }
}
