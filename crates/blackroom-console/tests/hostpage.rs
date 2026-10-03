//! The laptop owner's settings page against a hostile browser or peer: no hardware, no real password check.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, HOST, ORIGIN, SET_COOKIE};
use axum::http::{Method, Request, Response, StatusCode};
use blackroom_console::hostpage::{PasswordFactory, Settings, router};
use blackroom_console::{ConsoleConfig, Quality, RemoteConsole};
use remote_hostd::password::{PasswordCheck, PasswordOutcome};
use serde_json::{Value, json};
use tower::ServiceExt;

const PORT: u16 = 18090;
const HOSTNAME: &str = "127.0.0.1:18090";

struct Fake;
impl PasswordCheck for Fake {
    fn check(&mut self, account: &str, password: &str) -> PasswordOutcome {
        match (account, password) {
            ("owner", "correct horse") => PasswordOutcome::Accepted,
            ("owner", "unavailable") => PasswordOutcome::Unavailable,
            _ => PasswordOutcome::Rejected,
        }
    }
}

struct Fixture {
    app: Router,
    dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let tool = dir.path().join("gnome-extensions");
    std::fs::write(&tool, format!("#!/bin/bash\necho \"$@\" >> '{d}/ext-calls'\nU=blackroom-locked-remote@blackroom.local\ncase \"$1 $2\" in\n  \"list \") echo $U ;;\n  \"list --enabled\"|\"list --active\") [ -f '{d}/ext-on' ] && echo $U ;;\n  enable*) touch '{d}/ext-on' ;;\n  disable*) rm -f '{d}/ext-on' ;;\nesac\nexit 0\n", d = dir.path().display())).unwrap();
    std::fs::set_permissions(&tool, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let systemctl = dir.path().join("systemctl");
    std::fs::write(
        &systemctl,
        format!(
            "#!/bin/bash\necho \"$@\" >> '{}/systemctl-calls'\nexit 0\n",
            dir.path().display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        &systemctl,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let cli = dir.path().join("blackroom");
    std::fs::write(&cli, format!("#!/bin/bash\necho \"$@\" >> '{}/cli-calls'\necho 'secret: S3CRET-ONCE'\necho 'note on stderr' >&2\n[ \"$3\" = status ] && echo not-a-secret\nexit 0\n", dir.path().display())).unwrap();
    std::fs::set_permissions(&cli, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let console = RemoteConsole::spawn(ConsoleConfig {
        grab_socket: None,
        state_dir: dir.path().to_path_buf(),
        headless: true,
        quality: Quality::Medium,
        heartbeat_timeout: Duration::from_secs(15),
        restore_bin: std::path::PathBuf::new(),
    });
    assert_eq!(console.load_profile(dir.path().to_path_buf()), None);
    let settings = Settings {
        port: PORT,
        account: "owner".into(),
        effective: json!({ "http_listen": "127.0.0.1:8080", "tls_listen": null, "public": false, "clipboard": false }),
        unit: "blackroom-console.service".into(),
        managed: false,
        unit_dirs: vec![],
        home: dir.path().to_path_buf(),
        cli,
        state_dir: dir.path().join("hostd"),
        hostd_runtime: dir.path().join("runtime"),
        gnome_extensions: tool,
        systemctl,
    };
    let check: PasswordFactory = Arc::new(|| Box::new(Fake) as Box<dyn PasswordCheck>);
    Fixture {
        app: router(console, settings, check),
        dir,
    }
}

fn request(
    method: Method,
    uri: &str,
    host: &str,
    origin: Option<&str>,
    cookie: Option<&str>,
    body: Value,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(HOST, host);
    if let Some(origin) = origin {
        builder = builder.header(ORIGIN, origin);
    }
    if let Some(cookie) = cookie {
        builder = builder.header(COOKIE, cookie);
    }
    builder = builder.header(CONTENT_TYPE, "application/json");
    builder.body(Body::from(body.to_string())).unwrap()
}

async fn send(app: &Router, request: Request<Body>) -> Response<Body> {
    app.clone().oneshot(request).await.unwrap()
}

async fn json_of(response: Response<Body>) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn own_origin() -> String {
    format!("http://{HOSTNAME}")
}

async fn login(app: &Router, password: &str) -> Response<Body> {
    send(
        app,
        request(
            Method::POST,
            "/host/login",
            HOSTNAME,
            Some(&own_origin()),
            None,
            json!({ "password": password }),
        ),
    )
    .await
}

async fn signed_in(app: &Router) -> String {
    let response = login(app, "correct horse").await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()[SET_COOKIE].to_str().unwrap().to_string();
    cookie.split(';').next().unwrap().to_string()
}

#[tokio::test]
async fn only_a_loopback_name_reaches_the_page() {
    let fixture = fixture();
    for host in [
        "evil.example:18090",
        "192.168.1.50:18090",
        "127.0.0.1:18091",
        "127.0.0.1",
        "localhost.evil.com:18090",
    ] {
        for uri in ["/", "/host.js", "/app.css", "/host/state"] {
            let response = send(
                &fixture.app,
                request(Method::GET, uri, host, None, None, Value::Null),
            )
            .await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{host} {uri}");
        }
        let response = send(
            &fixture.app,
            request(
                Method::POST,
                "/host/login",
                host,
                Some(&format!("http://{host}")),
                None,
                json!({ "password": "correct horse" }),
            ),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "login from {host}"
        );
    }
    let page = send(
        &fixture.app,
        request(Method::GET, "/", "localhost:18090", None, None, Value::Null),
    )
    .await;
    assert_eq!(page.status(), StatusCode::OK);
}

#[tokio::test]
async fn nothing_but_the_page_is_open_without_a_login() {
    let fixture = fixture();
    for (method, uri) in [
        (Method::GET, "/host/state"),
        (Method::POST, "/host/config"),
        (Method::POST, "/host/restart"),
        (Method::POST, "/host/approve"),
        (Method::POST, "/host/autostart"),
        (Method::POST, "/host/totp/start"),
        (Method::POST, "/host/totp/verify"),
        (Method::POST, "/host/totp/cancel"),
        (Method::POST, "/host/lockscreen"),
        (Method::POST, "/host/logout"),
    ] {
        let response = send(
            &fixture.app,
            request(
                method.clone(),
                uri,
                HOSTNAME,
                Some(&own_origin()),
                None,
                json!({}),
            ),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri}"
        );
    }
    let forged = send(
        &fixture.app,
        request(
            Method::GET,
            "/host/state",
            HOSTNAME,
            None,
            Some("br_host=0123456789abcdef"),
            Value::Null,
        ),
    )
    .await;
    assert_eq!(forged.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_password_decides_and_wrong_ones_lock_the_page() {
    let fixture = fixture();
    let wrong = login(&fixture.app, "wrong").await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert!(wrong.headers().get(SET_COOKIE).is_none());
    assert_eq!(
        login(&fixture.app, "unavailable").await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let forged = send(
        &fixture.app,
        request(
            Method::POST,
            "/host/login",
            HOSTNAME,
            Some("http://evil.example"),
            None,
            json!({ "password": "correct horse" }),
        ),
    )
    .await;
    assert_eq!(
        forged.status(),
        StatusCode::FORBIDDEN,
        "a foreign origin cannot even try"
    );
    let no_origin = send(
        &fixture.app,
        request(
            Method::POST,
            "/host/login",
            HOSTNAME,
            None,
            None,
            json!({ "password": "correct horse" }),
        ),
    )
    .await;
    assert_eq!(no_origin.status(), StatusCode::FORBIDDEN);
    for _ in 0..4 {
        login(&fixture.app, "wrong").await;
    }
    assert_eq!(
        login(&fixture.app, "correct horse").await.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "locked even for the right one"
    );
    let fixture = self::fixture();
    let ok = login(&fixture.app, "correct horse").await;
    let cookie = ok.headers()[SET_COOKIE].to_str().unwrap();
    assert!(
        cookie.contains("HttpOnly")
            && cookie.contains("SameSite=Strict")
            && cookie.contains("Path=/")
    );
    for bad in ["", "a\nb", &"x".repeat(300)] {
        assert_eq!(
            login(&fixture.app, bad).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn settings_are_saved_validated_and_only_from_the_pages_own_origin() {
    let fixture = fixture();
    let cookie = signed_in(&fixture.app).await;
    let state = send(
        &fixture.app,
        request(
            Method::GET,
            "/host/state",
            HOSTNAME,
            None,
            Some(&cookie),
            Value::Null,
        ),
    )
    .await;
    assert_eq!(state.status(), StatusCode::OK);
    let state = json_of(state).await;
    assert_eq!(state["config"]["allow_private"], true);
    assert_eq!(state["restart_needed"], false);
    assert_eq!(state["status"]["phase"], "idle");

    let mut config = state["config"].clone();
    config["max_fps"] = json!(30);
    config["approval"] = json!("ask");
    config["allow_shared"] = json!(false);
    let foreign = send(
        &fixture.app,
        request(
            Method::POST,
            "/host/config",
            HOSTNAME,
            Some("http://evil.example"),
            Some(&cookie),
            config.clone(),
        ),
    )
    .await;
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    let none = send(
        &fixture.app,
        request(
            Method::POST,
            "/host/config",
            HOSTNAME,
            None,
            Some(&cookie),
            config.clone(),
        ),
    )
    .await;
    assert_eq!(
        none.status(),
        StatusCode::FORBIDDEN,
        "a change always names its origin"
    );
    assert!(!fixture.dir.path().join("host.json").exists());

    let saved = send(
        &fixture.app,
        request(
            Method::POST,
            "/host/config",
            HOSTNAME,
            Some(&own_origin()),
            Some(&cookie),
            config,
        ),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(json_of(saved).await["restart_needed"], true);
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.dir.path().join("host.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (on_disk["max_fps"].clone(), on_disk["approval"].clone()),
        (json!(30), json!("ask"))
    );
    let state = json_of(
        send(
            &fixture.app,
            request(
                Method::GET,
                "/host/state",
                HOSTNAME,
                None,
                Some(&cookie),
                Value::Null,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(
        state["restart_needed"], true,
        "saved but not in effect until the console restarts"
    );

    for (bad, why) in [
        (json!({ "max_fps": 1 }), "fps out of range"),
        (
            json!({ "allow_private": false, "allow_shared": false }),
            "no mode left",
        ),
        (json!({ "nonsense": true }), "unknown field"),
        (json!({ "http_listen": "not an address" }), "bad address"),
        (json!({ "tls_cert": "/a" }), "cert without key"),
        (json!([1, 2]), "not an object"),
        (json!("text"), "not an object"),
    ] {
        let response = send(
            &fixture.app,
            request(
                Method::POST,
                "/host/config",
                HOSTNAME,
                Some(&own_origin()),
                Some(&cookie),
                bad,
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{why}");
    }
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.dir.path().join("host.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        on_disk["max_fps"], 30,
        "refused changes leave the saved file alone"
    );
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(fixture.dir.path().join("host.json"))
            .unwrap()
            .permissions(),
    );
    assert_eq!(mode & 0o777, 0o600);
}

#[tokio::test]
async fn logging_out_ends_the_session_and_the_page_is_hardened() {
    let fixture = fixture();
    let cookie = signed_in(&fixture.app).await;
    let out = send(
        &fixture.app,
        request(
            Method::POST,
            "/host/logout",
            HOSTNAME,
            Some(&own_origin()),
            Some(&cookie),
            json!({}),
        ),
    )
    .await;
    assert_eq!(out.status(), StatusCode::OK);
    let after = send(
        &fixture.app,
        request(
            Method::GET,
            "/host/state",
            HOSTNAME,
            None,
            Some(&cookie),
            Value::Null,
        ),
    )
    .await;
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);

    let page = send(
        &fixture.app,
        request(Method::GET, "/", HOSTNAME, None, None, Value::Null),
    )
    .await;
    let policy = page.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        policy.contains("script-src 'self';") && !policy.contains("unsafe-inline"),
        "{policy}"
    );
    assert_eq!(page.headers()["x-frame-options"], "DENY");
    assert_eq!(page.headers()["cache-control"], "no-store");
    assert!(page.headers().get("access-control-allow-origin").is_none());
    let bytes = axum::body::to_bytes(page.into_body(), 1 << 20)
        .await
        .unwrap();
    let html = String::from_utf8_lossy(&bytes);
    let scripts = html.matches("<script").count();
    assert_eq!(
        scripts,
        html.matches("<script src=").count(),
        "no inline script"
    );
    assert!(
        !html.contains(" onclick=") && !html.contains(" onchange=") && !html.contains(" style=")
    );
}

#[tokio::test]
async fn approving_with_nothing_waiting_does_nothing() {
    let fixture = fixture();
    let cookie = signed_in(&fixture.app).await;
    let response = send(
        &fixture.app,
        request(
            Method::POST,
            "/host/approve",
            HOSTNAME,
            Some(&own_origin()),
            Some(&cookie),
            json!({ "id": 1, "accept": true }),
        ),
    )
    .await;
    assert_eq!(json_of(response).await["answered"], false);
    let bad = send(
        &fixture.app,
        request(
            Method::POST,
            "/host/approve",
            HOSTNAME,
            Some(&own_origin()),
            Some(&cookie),
            json!({ "id": -1, "accept": true }),
        ),
    )
    .await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let restart = send(
        &fixture.app,
        request(
            Method::POST,
            "/host/restart",
            HOSTNAME,
            Some(&own_origin()),
            Some(&cookie),
            json!({ "confirm": true }),
        ),
    )
    .await;
    assert_eq!(restart.status(), StatusCode::OK);
    assert_eq!(
        json_of(restart).await["restarting"],
        false,
        "not started by systemd: nothing is restarted"
    );
}

#[tokio::test]
async fn credential_changes_need_the_password_again_and_use_fixed_arguments() {
    let fixture = fixture();
    let cookie = signed_in(&fixture.app).await;
    let post = |body: Value, cookie: String| {
        request(
            Method::POST,
            "/host/credentials",
            HOSTNAME,
            Some(&own_origin()),
            Some(&cookie),
            body,
        )
    };
    // Reading the status needs no second password.
    let status = send(
        &fixture.app,
        post(json!({ "action": "status" }), cookie.clone()),
    )
    .await;
    assert_eq!(status.status(), StatusCode::OK);
    let status = json_of(status).await;
    assert!(status["output"].as_str().unwrap().contains("not-a-secret"));
    // A change without the password, or with a wrong one, runs nothing.
    for body in [
        json!({ "action": "rotate_key" }),
        json!({ "action": "rotate_key", "password": "wrong" }),
    ] {
        let refused = send(&fixture.app, post(body, cookie.clone())).await;
        assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    }
    let calls =
        || std::fs::read_to_string(fixture.dir.path().join("cli-calls")).unwrap_or_default();
    assert!(!calls().contains("rotate-key"), "{}", calls());
    let ok = send(
        &fixture.app,
        post(
            json!({ "action": "rotate_key", "password": "correct horse", "revoke_devices": true }),
            cookie.clone(),
        ),
    )
    .await;
    assert_eq!(ok.status(), StatusCode::OK);
    let ok = json_of(ok).await;
    assert_eq!(ok["ok"], true);
    assert!(
        ok["output"].as_str().unwrap().contains("S3CRET-ONCE"),
        "the new secret is shown to the owner once"
    );
    assert_eq!(ok["notice"], "note on stderr");
    assert!(
        calls().contains("rotate-key --account owner --revoke-devices"),
        "{}",
        calls()
    );
    // Nothing the browser sends reaches the command line except a plain device id.
    for device in ["", "a b", "x;rm -rf /", "../etc", "a\nb", &"d".repeat(200)] {
        let bad = send(
            &fixture.app,
            post(
                json!({ "action": "revoke_device", "password": "correct horse", "device": device }),
                cookie.clone(),
            ),
        )
        .await;
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST, "{device:?}");
    }
    let good = send(&fixture.app, post(json!({ "action": "revoke_device", "password": "correct horse", "device": "dev_1.a-b" }), cookie.clone())).await;
    assert_eq!(good.status(), StatusCode::OK);
    assert!(calls().contains("revoke-device dev_1.a-b"));
    let unknown = send(
        &fixture.app,
        post(
            json!({ "action": "format-disk", "password": "correct horse" }),
            cookie.clone(),
        ),
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
    let extra = send(
        &fixture.app,
        post(
            json!({ "action": "status", "args": ["--x"] }),
            cookie.clone(),
        ),
    )
    .await;
    assert_eq!(
        extra.status(),
        StatusCode::BAD_REQUEST,
        "unknown fields are refused"
    );
    for action in [
        "reset_security",
        "revoke_all",
        "disable",
        "enable",
        "recovery_codes",
    ] {
        let unlocked = send(
            &fixture.app,
            post(json!({ "action": action }), cookie.clone()),
        )
        .await;
        assert_eq!(
            unlocked.status(),
            StatusCode::UNAUTHORIZED,
            "{action} needs the password"
        );
    }
    let reset = send(
        &fixture.app,
        post(
            json!({ "action": "reset_security", "password": "correct horse" }),
            cookie.clone(),
        ),
    )
    .await;
    assert_eq!(reset.status(), StatusCode::OK);
    assert!(calls().contains("reset security --yes --state-dir"));
}

#[tokio::test]
async fn lock_screen_access_is_the_extension_and_turning_it_on_needs_the_password() {
    let fixture = fixture();
    let cookie = signed_in(&fixture.app).await;
    let post = |body: Value| {
        request(
            Method::POST,
            "/host/lockscreen",
            HOSTNAME,
            Some(&own_origin()),
            Some(&cookie),
            body,
        )
    };
    let state = |app: Router| {
        let request = request(
            Method::GET,
            "/host/state",
            HOSTNAME,
            None,
            Some(&cookie),
            Value::Null,
        );
        async move { json_of(send(&app, request).await).await }
    };
    let before = state(fixture.app.clone()).await;
    assert_eq!(
        before["lockscreen"],
        json!({ "installed": true, "enabled": false, "active": false })
    );
    for body in [
        json!({ "enabled": true }),
        json!({ "enabled": true, "password": "wrong" }),
    ] {
        assert_eq!(
            send(&fixture.app, post(body)).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert!(
        !fixture.dir.path().join("ext-on").exists(),
        "nothing was enabled without the password"
    );
    let on = send(
        &fixture.app,
        post(json!({ "enabled": true, "password": "correct horse" })),
    )
    .await;
    assert_eq!(on.status(), StatusCode::OK);
    assert_eq!(
        json_of(on).await,
        json!({ "installed": true, "enabled": true, "active": true })
    );
    assert_eq!(
        state(fixture.app.clone()).await["lockscreen"]["enabled"],
        true
    );
    let calls = std::fs::read_to_string(fixture.dir.path().join("ext-calls")).unwrap();
    assert!(
        calls.contains("enable blackroom-locked-remote@blackroom.local"),
        "{calls}"
    );
    // Turning it off is always allowed without the password.
    let off = send(&fixture.app, post(json!({ "enabled": false }))).await;
    assert_eq!(json_of(off).await["enabled"], false);
    assert_eq!(
        send(&fixture.app, post(json!({ "enabled": true, "extra": 1 })))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn the_sign_in_method_needs_a_running_login_authority_and_setup_needs_the_password() {
    let fixture = fixture();
    let cookie = signed_in(&fixture.app).await;
    let state = json_of(
        send(
            &fixture.app,
            request(
                Method::GET,
                "/host/state",
                HOSTNAME,
                None,
                Some(&cookie),
                Value::Null,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(state["login"]["ready"], false);
    let mut config = state["config"].clone();
    config["login"] = json!("hostd");
    let save = |config: Value| {
        request(
            Method::POST,
            "/host/config",
            HOSTNAME,
            Some(&own_origin()),
            Some(&cookie),
            config,
        )
    };
    let refused = send(&fixture.app, save(config.clone())).await;
    assert_eq!(
        refused.status(),
        StatusCode::CONFLICT,
        "no login authority yet"
    );
    assert!(!fixture.dir.path().join("host.json").exists());
    let setup = |password: &str| {
        request(
            Method::POST,
            "/host/credentials",
            HOSTNAME,
            Some(&own_origin()),
            Some(&cookie),
            json!({ "action": "setup", "password": password }),
        )
    };
    assert_eq!(
        send(&fixture.app, setup("")).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let done = send(&fixture.app, setup("correct horse")).await;
    assert_eq!(done.status(), StatusCode::OK);
    let calls = std::fs::read_to_string(fixture.dir.path().join("cli-calls")).unwrap();
    assert!(
        calls.starts_with("setup --state-dir"),
        "setup is the first argument: {calls}"
    );
    // The authority is now running (its sockets exist): the choice can be saved.
    let runtime = fixture.dir.path().join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::write(runtime.join("auth.sock"), "").unwrap();
    std::fs::write(runtime.join("admin.sock"), "").unwrap();
    assert_eq!(
        send(&fixture.app, save(config.clone())).await.status(),
        StatusCode::OK
    );
    config["login"] = json!("token");
    assert_eq!(
        send(&fixture.app, save(config.clone())).await.status(),
        StatusCode::OK
    );
    config["login"] = json!("password-only");
    assert_eq!(
        send(&fixture.app, save(config)).await.status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn a_new_authenticator_replaces_the_old_one_only_after_a_right_code() {
    use remote_hostd::totp::{base32_decode, totp, unix_now};
    let fixture = fixture();
    let state_dir = fixture.dir.path().join("hostd");
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::set_permissions(
        &state_dir,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let old = remote_hostd::totp::generate_secret().unwrap();
    remote_hostd::totp::set_secret(
        &remote_hostd::store::open_state_directory(&state_dir).unwrap(),
        "owner",
        &old,
        0,
    )
    .unwrap();
    let store = || std::fs::read_to_string(state_dir.join("totp-credentials")).unwrap();
    let cookie = signed_in(&fixture.app).await;
    let post = |path: &str, body: Value| {
        request(
            Method::POST,
            path,
            HOSTNAME,
            Some(&own_origin()),
            Some(&cookie),
            body,
        )
    };
    let state = json_of(
        send(
            &fixture.app,
            request(
                Method::GET,
                "/host/state",
                HOSTNAME,
                None,
                Some(&cookie),
                Value::Null,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(state["totp"]["enrolled"], true);

    // Starting asks for the password and stores nothing.
    for body in [json!({}), json!({ "password": "wrong" })] {
        let refused = send(&fixture.app, post("/host/totp/start", body)).await;
        assert!(
            refused.status() == StatusCode::BAD_REQUEST
                || refused.status() == StatusCode::UNAUTHORIZED
        );
    }
    let before = store();
    let started = send(
        &fixture.app,
        post("/host/totp/start", json!({ "password": "correct horse" })),
    )
    .await;
    assert_eq!(started.status(), StatusCode::OK);
    let started = json_of(started).await;
    let secret = started["secret"].as_str().unwrap().replace(' ', "");
    assert!(
        base32_decode(&secret).is_some_and(|bytes| bytes.len() == 20),
        "the manual key is the secret"
    );
    assert!(
        started["secret"]
            .as_str()
            .unwrap()
            .split(' ')
            .all(|group| group.len() <= 4)
    );
    assert!(
        started["uri"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/Blackroom%20Console:owner?secret=")
    );
    assert!(started["uri"].as_str().unwrap().contains(&secret));
    assert!(
        started["svg"].as_str().unwrap().contains("<svg"),
        "a QR code is drawn"
    );
    assert_eq!(store(), before, "nothing is stored before a code proves it");

    // A wrong or malformed code stores nothing; a right one does.
    for bad in ["000000", "12345", "abcdef", ""] {
        let wrong = send(
            &fixture.app,
            post("/host/totp/verify", json!({ "code": bad })),
        )
        .await;
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED, "{bad:?}");
    }
    assert_eq!(store(), before);
    let code = totp(&base32_decode(&secret).unwrap(), unix_now());
    let done = send(
        &fixture.app,
        post("/host/totp/verify", json!({ "code": code })),
    )
    .await;
    assert_eq!(done.status(), StatusCode::OK);
    assert_eq!(json_of(done).await["authority_restarted"], true);
    assert!(
        store().contains(&secret) && !store().contains(old.as_str()),
        "the new secret replaced the old one"
    );
    let calls = std::fs::read_to_string(fixture.dir.path().join("systemctl-calls")).unwrap();
    assert!(
        calls.contains("--user try-restart remote-hostd.service"),
        "{calls}"
    );
    let again = send(
        &fixture.app,
        post(
            "/host/totp/verify",
            json!({ "code": totp(&base32_decode(&secret).unwrap(), unix_now()) }),
        ),
    )
    .await;
    assert_eq!(
        again.status(),
        StatusCode::CONFLICT,
        "nothing is waiting any more"
    );

    // Five wrong codes end a setup.
    send(
        &fixture.app,
        post("/host/totp/start", json!({ "password": "correct horse" })),
    )
    .await;
    for _ in 0..4 {
        assert_eq!(
            send(
                &fixture.app,
                post("/host/totp/verify", json!({ "code": "000000" }))
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        send(
            &fixture.app,
            post("/host/totp/verify", json!({ "code": "000000" }))
        )
        .await
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        send(
            &fixture.app,
            post("/host/totp/verify", json!({ "code": "000000" }))
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    // Cancelling forgets the secret too.
    send(
        &fixture.app,
        post("/host/totp/start", json!({ "password": "correct horse" })),
    )
    .await;
    assert_eq!(
        send(&fixture.app, post("/host/totp/cancel", json!({})))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(
            &fixture.app,
            post("/host/totp/verify", json!({ "code": "000000" }))
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
}
