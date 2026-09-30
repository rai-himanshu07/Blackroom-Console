#![forbid(unsafe_code)]

mod separated;

use std::fs::File;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::extract::{DefaultBodyLimit, State as ExtractState};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::routing::{get, post};
use axum::{Json, Router, extract::Request, response::Response};
use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::limits::{MAX_INPUT_EVENT_SIZE_BYTES, MAX_MESSAGE_SIZE_BYTES};
use blackroom_core::protocol::AuthorityUpdate;
use blackroom_core::state::State;
use ed25519_dalek::VerifyingKey;
use gnome_session_agent::{authority::InputAuthority, ipc};
use remote_hostd::{
    OfflineHostAuthority, SIMULATED_SESSION_ID, offline_control::DEMO_CODE,
    store::PersistentHostAuthority, write_update,
};
use serde::{Deserialize, Serialize};

use separated::SeparatedHost;

type SharedHost = Arc<Mutex<OfflineConsole>>;
type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;
const MAX_INVALID_DEMO_CODES: u8 = 5;

enum SimulationBackend {
    InProcess(Box<SimulatedHost>),
    Separated(Box<SeparatedHost>),
}

impl SimulationBackend {
    fn snapshot(&mut self) -> Snapshot {
        match self {
            Self::InProcess(host) => host.snapshot(),
            Self::Separated(host) => host.snapshot(),
        }
    }

    /// The grant is hostd-issued in SEPARATE mode and `None` for in-process hosts.
    fn start(&mut self, demo_code: &str) -> Result<Option<String>, BlackroomError> {
        match self {
            Self::InProcess(host) => host.start().map(|()| None),
            Self::Separated(host) => host.start(demo_code).map(Some),
        }
    }

    fn revoke(&mut self) -> Result<(), BlackroomError> {
        match self {
            Self::InProcess(host) => host.revoke(),
            Self::Separated(host) => host.revoke(),
        }
    }

    fn input(
        &mut self,
        event: InputEvent,
        sequence: u64,
        grant_id: &str,
    ) -> Result<(), BlackroomError> {
        match self {
            Self::InProcess(host) => host.input(event),
            Self::Separated(host) => host.input(event, sequence, grant_id),
        }
    }
}

struct OfflineConsole {
    backend: SimulationBackend,
    invalid_demo_codes: u8,
    input_grant: Option<String>,
    last_sequence: u64,
}

impl OfflineConsole {
    fn snapshot(&mut self) -> Snapshot {
        let mut snapshot = self.backend.snapshot();
        if snapshot.state != State::RemoteActive.as_str() {
            self.input_grant = None;
            self.last_sequence = 0;
        }
        snapshot.auth_blocked = self.invalid_demo_codes >= MAX_INVALID_DEMO_CODES;
        snapshot.input_grant = self.input_grant.clone();
        snapshot.next_sequence = self
            .input_grant
            .as_ref()
            .and_then(|_| self.last_sequence.checked_add(1));
        snapshot
    }

    fn start(&mut self, demo_code: &str) -> Result<(), BlackroomError> {
        if self.invalid_demo_codes >= MAX_INVALID_DEMO_CODES {
            return Err(BlackroomError::new(
                ErrorCode::AuthRateLimited,
                "offline demo access is blocked",
            ));
        }
        if demo_code != DEMO_CODE {
            self.invalid_demo_codes += 1;
            if self.invalid_demo_codes == MAX_INVALID_DEMO_CODES {
                self.backend.revoke()?;
                return Err(BlackroomError::new(
                    ErrorCode::AuthRateLimited,
                    "offline demo access is blocked",
                ));
            }
            return Err(BlackroomError::new(
                ErrorCode::AuthInvalid,
                "invalid offline demo code",
            ));
        }
        let mut grant = [0_u8; 16];
        getrandom::fill(&mut grant).map_err(|_| {
            BlackroomError::new(ErrorCode::RecoveryFailed, "offline input grant unavailable")
        })?;
        let hostd_grant = self.backend.start(demo_code)?;
        self.input_grant =
            Some(hostd_grant.unwrap_or_else(|| format!("{:032x}", u128::from_be_bytes(grant))));
        self.last_sequence = 0;
        self.invalid_demo_codes = 0;
        Ok(())
    }

    fn input(&mut self, command: InputCommand) -> Result<(), BlackroomError> {
        let current = self.snapshot();
        if current.state != State::RemoteActive.as_str()
            || current.input_grant.as_deref() != Some(command.grant_id.as_str())
            || current.next_sequence != Some(command.sequence)
        {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "stale offline input grant or sequence",
            ));
        }
        self.backend
            .input(command.event, command.sequence, &command.grant_id)?;
        self.last_sequence = command.sequence;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputEvent {
    Key { code: u32 },
    Move { dx: f32, dy: f32 },
    Click { button: u32 },
    Scroll { dy: f32 },
}

impl InputEvent {
    fn valid(&self) -> bool {
        match self {
            Self::Key { code } => (1..=255).contains(code),
            Self::Move { dx, dy } => {
                dx.is_finite() && dy.is_finite() && dx.abs() <= 500.0 && dy.abs() <= 500.0
            }
            Self::Click { button } => matches!(button, 272 | 273),
            Self::Scroll { dy } => dy.is_finite() && dy.abs() <= 120.0,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputCommand {
    grant_id: String,
    sequence: u64,
    event: InputEvent,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub mode: &'static str,
    pub live_control: bool,
    pub authority_store: &'static str,
    pub state: &'static str,
    pub epoch: u64,
    pub auth_blocked: bool,
    pub input_grant: Option<String>,
    pub next_sequence: Option<u64>,
    pub events: Vec<InputEvent>,
    pub pointer: PointerPosition,
}

#[derive(Clone, Copy, Serialize)]
pub struct PointerPosition {
    pub x: f32,
    pub y: f32,
}

#[derive(Serialize)]
pub struct ApiError {
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyCommand {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartCommand {
    demo_code: String,
}

enum HostMode {
    Ephemeral(OfflineHostAuthority),
    Persisted(PersistentHostAuthority),
}

impl HostMode {
    fn verifying_key(&self) -> VerifyingKey {
        match self {
            Self::Ephemeral(host) => host.verifying_key(),
            Self::Persisted(host) => host.verifying_key(),
        }
    }

    fn epoch(&self) -> SecurityEpoch {
        match self {
            Self::Ephemeral(host) => host.epoch(),
            Self::Persisted(host) => host.epoch(),
        }
    }

    fn state(&self) -> State {
        match self {
            Self::Ephemeral(host) => host.state(),
            Self::Persisted(host) => host.state(),
        }
    }

    fn grant_update(&mut self) -> Result<AuthorityUpdate, BlackroomError> {
        match self {
            Self::Ephemeral(host) => host.grant_update(),
            Self::Persisted(host) => host.grant_update(),
        }
    }

    fn revoke_update(&mut self) -> Result<AuthorityUpdate, BlackroomError> {
        match self {
            Self::Ephemeral(host) => Ok(host.revoke_update()),
            Self::Persisted(host) => host.revoke_update().map_err(|_| {
                BlackroomError::new(
                    ErrorCode::RecoveryFailed,
                    "host epoch could not be persisted",
                )
            }),
        }
    }

    fn complete_recovery(&self, granted_epoch: SecurityEpoch) -> Result<(), BlackroomError> {
        match self {
            Self::Ephemeral(_) => Ok(()),
            Self::Persisted(host) => host.complete_recovery(granted_epoch).map_err(|_| {
                BlackroomError::new(ErrorCode::RecoveryFailed, "offline recovery is unverified")
            }),
        }
    }
}

pub struct SimulatedHost {
    host: HostMode,
    authority: InputAuthority,
    host_wire: UnixStream,
    agent_wire: UnixStream,
    expected_host_uid: u32,
    events: Vec<InputEvent>,
    pointer: PointerPosition,
}

impl Default for SimulatedHost {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatedHost {
    pub fn new() -> Self {
        Self::with_host(HostMode::Ephemeral(OfflineHostAuthority::new()))
    }

    pub fn from_persisted_directory(directory: &File) -> io::Result<Self> {
        Ok(Self::with_host(HostMode::Persisted(
            PersistentHostAuthority::open(directory)?,
        )))
    }

    fn with_host(host: HostMode) -> Self {
        let (host_wire, agent_wire) =
            UnixStream::pair().expect("offline simulation needs a Unix socket pair");
        Self {
            authority: InputAuthority::new(
                host.verifying_key(),
                host.epoch(),
                SIMULATED_SESSION_ID.into(),
            ),
            host,
            host_wire,
            agent_wire,
            expected_host_uid: rustix::process::getuid().as_raw(),
            events: Vec::new(),
            pointer: PointerPosition { x: 50.0, y: 55.0 },
        }
    }

    fn reconcile(&mut self) -> Result<(), BlackroomError> {
        if self.host.state() == State::RemoteActive
            && self.authority.dispatch((), |_, _| Ok(())).is_err()
        {
            self.revoke()?;
        }
        Ok(())
    }

    pub fn snapshot(&mut self) -> Snapshot {
        let _ = self.reconcile();
        Snapshot {
            mode: "OFFLINE_SIMULATION",
            live_control: false,
            authority_store: match self.host {
                HostMode::Ephemeral(_) => "EPHEMERAL",
                HostMode::Persisted(_) => "PERSISTED",
            },
            state: self.host.state().as_str(),
            epoch: self.host.epoch().value(),
            auth_blocked: false,
            input_grant: None,
            next_sequence: None,
            events: self.events.clone(),
            pointer: self.pointer,
        }
    }

    pub fn start(&mut self) -> Result<(), BlackroomError> {
        self.reconcile()?;
        if ipc::drain_host_updates(
            &mut self.agent_wire,
            self.expected_host_uid,
            &mut self.authority,
        )
        .is_err()
        {
            return Err(BlackroomError::new(
                ErrorCode::IpcUnauthorized,
                "offline host authority connection was lost",
            ));
        }
        let update = self.host.grant_update()?;
        if write_update(&mut self.host_wire, &update).is_err()
            || ipc::receive_host_update(
                &mut self.agent_wire,
                self.expected_host_uid,
                &mut self.authority,
            )
            .is_err()
        {
            let failure = BlackroomError::new(
                ErrorCode::IpcUnauthorized,
                "offline host grant was not accepted by the agent",
            );
            return Err(self.revoke().err().unwrap_or(failure));
        }
        Ok(())
    }

    pub fn revoke(&mut self) -> Result<(), BlackroomError> {
        if self.host.state() == State::LocalLocked {
            return Ok(());
        }
        let granted_epoch = self.host.epoch();
        let update = self.host.revoke_update();
        self.authority.fail_closed();
        let update = update?;
        if write_update(&mut self.host_wire, &update).is_err()
            || ipc::receive_host_update(
                &mut self.agent_wire,
                self.expected_host_uid,
                &mut self.authority,
            )
            .is_err()
        {
            return Err(BlackroomError::new(
                ErrorCode::IpcUnauthorized,
                "offline revoke was not accepted by the agent",
            ));
        }
        self.host.complete_recovery(granted_epoch)?;
        Ok(())
    }

    pub fn input(&mut self, event: InputEvent) -> Result<(), BlackroomError> {
        if !event.valid() {
            return Err(BlackroomError::new(
                ErrorCode::IpcInvalidMessage,
                "invalid simulated input",
            ));
        }
        if ipc::drain_host_updates(
            &mut self.agent_wire,
            self.expected_host_uid,
            &mut self.authority,
        )
        .is_err()
        {
            let _ = self.revoke();
            self.authority.fail_closed();
            return Err(BlackroomError::new(
                ErrorCode::IpcUnauthorized,
                "offline host authority connection was lost",
            ));
        }
        let events = &mut self.events;
        let pointer = &mut self.pointer;
        self.authority.dispatch(event, |event, _authorization| {
            if let InputEvent::Move { dx, dy } = event {
                pointer.x = (pointer.x + dx / 10.0).clamp(5.0, 95.0);
                pointer.y = (pointer.y + dy / 10.0).clamp(8.0, 88.0);
            }
            if events.len() == 12 {
                events.remove(0);
            }
            events.push(event);
            Ok(())
        })
    }
}

fn error_response(error: BlackroomError) -> (StatusCode, Json<ApiError>) {
    let status = if error.code == ErrorCode::IpcInvalidMessage {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::CONFLICT
    };
    (
        status,
        Json(ApiError {
            code: error.code.as_str(),
            message: "Simulated command refused",
        }),
    )
}

fn start_error_response(error: BlackroomError) -> (StatusCode, Json<ApiError>) {
    let status = match error.code {
        ErrorCode::AuthInvalid => StatusCode::UNAUTHORIZED,
        ErrorCode::AuthRateLimited => StatusCode::TOO_MANY_REQUESTS,
        _ => return error_response(error),
    };
    let (_, body) = error_response(error);
    (status, body)
}

async fn local_host(request: Request, next: Next) -> Result<Response, StatusCode> {
    if request.headers().get(header::HOST) != Some(&HeaderValue::from_static("127.0.0.1:8787")) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(next.run(request).await)
}

fn check_origin(headers: &HeaderMap) -> Result<(), (StatusCode, Json<ApiError>)> {
    let allowed = headers.get(header::ORIGIN).is_none_or(|value| {
        value == "http://127.0.0.1:5173"
            || value == "http://localhost:5173"
            || value == "http://127.0.0.1:8787"
    });
    if allowed {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(ApiError {
                code: "LOCAL_ONLY",
                message: "Local simulation only",
            }),
        ))
    }
}

async fn status(ExtractState(host): ExtractState<SharedHost>) -> Json<Snapshot> {
    Json(host.lock().expect("simulation lock poisoned").snapshot())
}

async fn start(
    ExtractState(host): ExtractState<SharedHost>,
    headers: HeaderMap,
    Json(command): Json<StartCommand>,
) -> ApiResult<Snapshot> {
    check_origin(&headers)?;
    let mut host = host.lock().expect("simulation lock poisoned");
    host.start(&command.demo_code)
        .map_err(start_error_response)?;
    Ok(Json(host.snapshot()))
}

async fn revoke(
    ExtractState(host): ExtractState<SharedHost>,
    headers: HeaderMap,
    Json(_command): Json<EmptyCommand>,
) -> ApiResult<Snapshot> {
    check_origin(&headers)?;
    let mut host = host.lock().expect("simulation lock poisoned");
    host.backend.revoke().map_err(error_response)?;
    Ok(Json(host.snapshot()))
}

async fn input(
    ExtractState(host): ExtractState<SharedHost>,
    headers: HeaderMap,
    Json(command): Json<InputCommand>,
) -> ApiResult<Snapshot> {
    check_origin(&headers)?;
    let mut host = host.lock().expect("simulation lock poisoned");
    host.input(command).map_err(error_response)?;
    Ok(Json(host.snapshot()))
}

pub fn router() -> Router {
    with_backend(SimulationBackend::InProcess(Box::default()))
}

pub fn persisted_router(directory: &File) -> io::Result<Router> {
    Ok(with_backend(SimulationBackend::InProcess(Box::new(
        SimulatedHost::from_persisted_directory(directory)?,
    ))))
}

pub fn separated_router(
    state_directory: &Path,
    hostd_binary: &Path,
    agent_binary: &Path,
) -> io::Result<Router> {
    Ok(with_backend(SimulationBackend::Separated(Box::new(
        SeparatedHost::launch(state_directory, hostd_binary, agent_binary)?,
    ))))
}

fn with_backend(host: SimulationBackend) -> Router {
    let host = Arc::new(Mutex::new(OfflineConsole {
        backend: host,
        invalid_demo_codes: 0,
        input_grant: None,
        last_sequence: 0,
    }));
    Router::new()
        .route("/api/simulation", get(status))
        .route("/api/simulation/start", post(start))
        .route("/api/simulation/revoke", post(revoke))
        .route(
            "/api/simulation/input",
            post(input).layer(DefaultBodyLimit::max(MAX_INPUT_EVENT_SIZE_BYTES)),
        )
        .with_state(host)
        .layer(DefaultBodyLimit::max(MAX_MESSAGE_SIZE_BYTES))
        .layer(middleware::from_fn(local_host))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use std::os::unix::fs::PermissionsExt;
    use tower::ServiceExt;

    async fn call(
        router: &Router,
        path: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder()
            .uri(path)
            .header("host", "127.0.0.1:8787");
        if body.is_some() {
            builder = builder
                .method("POST")
                .header("content-type", "application/json");
        }
        let response = router
            .clone()
            .oneshot(
                builder
                    .body(Body::from(body.unwrap_or("").to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    async fn call_start(router: &Router) -> (StatusCode, serde_json::Value) {
        call(
            router,
            "/api/simulation/start",
            Some(r#"{"demo_code":"SIMULATE"}"#),
        )
        .await
    }

    async fn call_input(router: &Router, event: &str) -> (StatusCode, serde_json::Value) {
        let (_, snapshot) = call(router, "/api/simulation", None).await;
        let command = serde_json::json!({
            "grant_id": snapshot["input_grant"].as_str().unwrap_or(""),
            "sequence": snapshot["next_sequence"].as_u64().unwrap_or(1),
            "event": serde_json::from_str::<serde_json::Value>(event).unwrap(),
        });
        call(router, "/api/simulation/input", Some(&command.to_string())).await
    }

    #[tokio::test]
    async fn demo_code_refusal_is_bounded_and_revokes_active_fake_control() {
        let router = router();
        assert_eq!(
            call(&router, "/api/simulation/start", Some("{}")).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let valid = r#"{"demo_code":"SIMULATE"}"#;
        let invalid = r#"{"demo_code":"incorrect"}"#;
        assert_eq!(
            call(&router, "/api/simulation/start", Some(valid)).await.0,
            StatusCode::OK
        );
        for attempt in 1..=5 {
            let (status, error) = call(&router, "/api/simulation/start", Some(invalid)).await;
            assert_eq!(
                status,
                if attempt == 5 {
                    StatusCode::TOO_MANY_REQUESTS
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
            assert_eq!(
                error["code"],
                if attempt == 5 {
                    "AUTH_RATE_LIMITED"
                } else {
                    "AUTH_INVALID"
                }
            );
        }
        let (_, snapshot) = call(&router, "/api/simulation", None).await;
        assert_eq!(snapshot["state"], "LOCAL_LOCKED");
        assert_eq!(snapshot["auth_blocked"], true);
        assert_eq!(snapshot["epoch"], 1);
        assert_eq!(
            call(&router, "/api/simulation/start", Some(valid)).await.0,
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            call_input(&router, r#"{"kind":"key","code":30}"#).await.0,
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn simulated_commands_are_agent_gated_and_revoke_advances_epoch() {
        let router = router();
        let (_, status) = call(&router, "/api/simulation", None).await;
        assert_eq!(status["mode"], "OFFLINE_SIMULATION");
        assert_eq!(status["live_control"], false);
        assert_eq!(status["authority_store"], "EPHEMERAL");
        let (_, still_locked) = call(&router, "/api/simulation/revoke", Some("{}")).await;
        assert_eq!(still_locked["epoch"], 0);
        assert_eq!(
            call_input(&router, r#"{"kind":"key","code":30}"#).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(call_start(&router).await.0, StatusCode::OK);
        assert_eq!(
            call_input(&router, r#"{"kind":"key","code":30}"#).await.0,
            StatusCode::OK
        );
        assert_eq!(
            call_input(&router, r#"{"kind":"move","dx":999,"dy":0}"#)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        let (_, after_invalid) = call(&router, "/api/simulation", None).await;
        assert_eq!(after_invalid["next_sequence"], 2);
        assert_eq!(after_invalid["events"].as_array().unwrap().len(), 1);
        let (_, revoked) = call(&router, "/api/simulation/revoke", Some("{}")).await;
        assert_eq!(revoked["state"], "LOCAL_LOCKED");
        assert_eq!(revoked["epoch"], 1);
        assert_eq!(revoked["events"].as_array().unwrap().len(), 1);
        assert_eq!(
            call_input(&router, r#"{"kind":"key","code":30}"#).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(call_start(&router).await.0, StatusCode::OK);
        assert_eq!(
            call_input(&router, r#"{"kind":"click","button":272}"#)
                .await
                .0,
            StatusCode::OK
        );
        let (_, current) = call(&router, "/api/simulation", None).await;
        assert_eq!(current["events"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn old_unbound_input_cannot_cross_revoke_and_new_grant() {
        let router = router();
        assert_eq!(call_start(&router).await.0, StatusCode::OK);
        assert_eq!(
            call(&router, "/api/simulation/revoke", Some("{}")).await.0,
            StatusCode::OK
        );
        assert_eq!(call_start(&router).await.0, StatusCode::OK);
        let (status, _) = call(
            &router,
            "/api/simulation/input",
            Some(r#"{"kind":"key","code":30}"#),
        )
        .await;
        assert_ne!(status, StatusCode::OK);
        let (_, snapshot) = call(&router, "/api/simulation", None).await;
        assert!(snapshot["events"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn sequenced_input_rejects_replay_skips_and_prior_grants() {
        let router = router();
        let (_, first) = call_start(&router).await;
        let first_grant = first["input_grant"].as_str().unwrap().to_owned();
        assert_eq!(first_grant.len(), 32);
        assert_eq!(first["next_sequence"], 1);
        let first_event = serde_json::json!({
            "grant_id": first_grant, "sequence": 1,
            "event": {"kind": "key", "code": 30},
        });
        let first_event = first_event.to_string();
        assert_eq!(
            call(&router, "/api/simulation/input", Some(&first_event))
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            call(&router, "/api/simulation/input", Some(&first_event))
                .await
                .0,
            StatusCode::CONFLICT
        );
        let skipped = serde_json::json!({
            "grant_id": first_grant, "sequence": 3,
            "event": {"kind": "click", "button": 272},
        });
        assert_eq!(
            call(&router, "/api/simulation/input", Some(&skipped.to_string()))
                .await
                .0,
            StatusCode::CONFLICT
        );
        let (_, current) = call(&router, "/api/simulation", None).await;
        assert_eq!(current["next_sequence"], 2);
        assert_eq!(current["events"].as_array().unwrap().len(), 1);
        assert_eq!(
            call_input(&router, r#"{"kind":"click","button":272}"#)
                .await
                .0,
            StatusCode::OK
        );

        assert_eq!(
            call(&router, "/api/simulation/revoke", Some("{}")).await.0,
            StatusCode::OK
        );
        let (_, second) = call_start(&router).await;
        assert_ne!(second["input_grant"], first_grant);
        assert_eq!(second["next_sequence"], 1);
        assert_eq!(
            call(&router, "/api/simulation/input", Some(&first_event))
                .await
                .0,
            StatusCode::CONFLICT
        );
        drop(router);

        let restarted = super::router();
        let (_, new_session) = call_start(&restarted).await;
        assert_ne!(new_session["input_grant"], first_grant);
        assert_eq!(
            call(&restarted, "/api/simulation/input", Some(&first_event))
                .await
                .0,
            StatusCode::CONFLICT
        );
        let (_, snapshot) = call(&restarted, "/api/simulation", None).await;
        assert!(snapshot["events"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn oversized_fake_commands_do_not_mutate_authority_or_consume_input_sequence() {
        let router = router();
        let start = serde_json::json!({"demo_code": "X".repeat(MAX_MESSAGE_SIZE_BYTES)});
        assert_eq!(
            call(&router, "/api/simulation/start", Some(&start.to_string()))
                .await
                .0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let (_, locked) = call(&router, "/api/simulation", None).await;
        assert_eq!(locked["state"], "LOCAL_LOCKED");
        assert_eq!(locked["auth_blocked"], false);

        let (_, active) = call_start(&router).await;
        let large = serde_json::json!({
            "grant_id": active["input_grant"], "sequence": 1,
            "event": {"kind": "key", "code": 30},
            "padding": "X".repeat(MAX_INPUT_EVENT_SIZE_BYTES),
        });
        assert_eq!(
            call(&router, "/api/simulation/input", Some(&large.to_string()))
                .await
                .0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let (_, unchanged) = call(&router, "/api/simulation", None).await;
        assert_eq!(unchanged["next_sequence"], 1);
        assert!(unchanged["events"].as_array().unwrap().is_empty());
        assert_eq!(
            call_input(&router, r#"{"kind":"key","code":30}"#).await.0,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn persisted_unverified_restart_reports_failed_safe() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let dirfd = File::open(directory.path()).unwrap();
        let active = persisted_router(&dirfd).unwrap();
        assert_eq!(call_start(&active).await.0, StatusCode::OK);
        drop(active);

        let restarted = persisted_router(&dirfd).unwrap();
        let (_, snapshot) = call(&restarted, "/api/simulation", None).await;
        assert_eq!(snapshot["state"], "FAILED_SAFE");
        assert_eq!(snapshot["live_control"], false);
        assert_eq!(call_start(&restarted).await.0, StatusCode::CONFLICT);
    }

    #[test]
    fn invalid_active_authority_reconciles_before_snapshot_or_new_start() {
        let mut simulation = SimulatedHost::new();
        simulation.start().unwrap();
        simulation.authority.fail_closed();
        let snapshot = simulation.snapshot();
        assert_eq!(snapshot.state, State::LocalLocked.as_str());
        assert_eq!(snapshot.epoch, 1);
        simulation.start().unwrap();
        assert_eq!(simulation.snapshot().state, State::RemoteActive.as_str());
        assert_eq!(simulation.snapshot().epoch, 1);
    }

    #[tokio::test]
    async fn requests_cannot_supply_authority_fields_or_cross_origin()
    -> Result<(), Box<dyn std::error::Error>> {
        let router = router();
        assert_eq!(
            call(
                &router,
                "/api/simulation/start",
                Some(r#"{"authenticated":true}"#)
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            call_input(
                &router,
                r#"{"kind":"key","code":30,"current_state":"REMOTE_ACTIVE"}"#
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("host", "127.0.0.1:8787")
                    .header("origin", "https://untrusted.example")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"demo_code":"SIMULATE"}"#))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("host", "127.0.0.1:8787")
                    .header("origin", "http://localhost:5173")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"demo_code":"SIMULATE"}"#))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/simulation")
                    .header("host", "untrusted.example")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        Ok(())
    }

    #[tokio::test]
    async fn pointer_position_survives_event_log_rollover() {
        let router = router();
        call_start(&router).await;
        for _ in 0..13 {
            assert_eq!(
                call_input(&router, r#"{"kind":"move","dx":24,"dy":0}"#)
                    .await
                    .0,
                StatusCode::OK
            );
        }
        let (_, snapshot) = call(&router, "/api/simulation", None).await;
        assert_eq!(snapshot["events"].as_array().unwrap().len(), 12);
        assert!(snapshot["pointer"]["x"].as_f64().unwrap() > 80.0);
        call(&router, "/api/simulation/revoke", Some("{}")).await;
        let (_, after) = call(&router, "/api/simulation", None).await;
        assert_eq!(after["pointer"]["x"], snapshot["pointer"]["x"]);
        assert_eq!(
            call_input(&router, r#"{"kind":"move","dx":24,"dy":0}"#)
                .await
                .0,
            StatusCode::CONFLICT
        );
        let (_, refused) = call(&router, "/api/simulation", None).await;
        assert_eq!(refused["pointer"]["x"], snapshot["pointer"]["x"]);
    }

    #[tokio::test]
    async fn persisted_simulation_restarts_locked_with_a_new_epoch() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let directory_fd = File::open(directory.path()).unwrap();
        let router = persisted_router(&directory_fd).unwrap();
        assert_eq!(call_start(&router).await.0, StatusCode::OK);
        assert_eq!(
            call_input(&router, r#"{"kind":"key","code":30}"#).await.0,
            StatusCode::OK
        );
        let (_, revoked) = call(&router, "/api/simulation/revoke", Some("{}")).await;
        assert_eq!(revoked["epoch"], 1);
        drop(router);

        let restarted = persisted_router(&directory_fd).unwrap();
        let (_, state) = call(&restarted, "/api/simulation", None).await;
        assert_eq!(state["mode"], "OFFLINE_SIMULATION");
        assert_eq!(state["live_control"], false);
        assert_eq!(state["authority_store"], "PERSISTED");
        assert_eq!(state["state"], "LOCAL_LOCKED");
        assert_eq!(state["epoch"], 2);
        assert!(state["events"].as_array().unwrap().is_empty());
        assert_eq!(
            call_input(&restarted, r#"{"kind":"key","code":30}"#)
                .await
                .0,
            StatusCode::CONFLICT
        );
        assert_eq!(call_start(&restarted).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn persisted_epoch_failure_closes_input_and_refuses_restart() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let directory_fd = File::open(directory.path()).unwrap();
        drop(persisted_router(&directory_fd).unwrap());
        std::fs::write(
            directory.path().join("security-epoch"),
            (u64::MAX - 1).to_be_bytes(),
        )
        .unwrap();
        let router = persisted_router(&directory_fd).unwrap();
        assert_eq!(call_start(&router).await.0, StatusCode::OK);
        let (status, error) = call(&router, "/api/simulation/revoke", Some("{}")).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(error["code"], "RECOVERY_FAILED");
        let (_, snapshot) = call(&router, "/api/simulation", None).await;
        assert_eq!(snapshot["state"], "FAILED_SAFE");
        assert_eq!(
            call_input(&router, r#"{"kind":"key","code":30}"#).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(call_start(&router).await.0, StatusCode::CONFLICT);
        drop(router);
        assert!(persisted_router(&directory_fd).is_err());
    }

    #[tokio::test]
    #[ignore = "run with built offline agent/hostd binaries in BLACKROOM_TEST_AGENT_BIN and BLACKROOM_TEST_HOSTD_BIN"]
    async fn separated_router_controls_real_offline_processes() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let agent = std::env::var_os("BLACKROOM_TEST_AGENT_BIN").unwrap();
        let hostd = std::env::var_os("BLACKROOM_TEST_HOSTD_BIN").unwrap();
        let router =
            separated_router(directory.path(), Path::new(&hostd), Path::new(&agent)).unwrap();
        let (_, initial) = call(&router, "/api/simulation", None).await;
        assert_eq!(initial["authority_store"], "SEPARATE");
        assert_eq!(initial["live_control"], false);
        assert_eq!(initial["state"], "LOCAL_LOCKED");
        std::thread::sleep(std::time::Duration::from_millis(3200));
        assert_eq!(call_start(&router).await.0, StatusCode::OK);
        assert_eq!(call_start(&router).await.0, StatusCode::CONFLICT);
        let (_, after_duplicate) = call(&router, "/api/simulation", None).await;
        assert_eq!(after_duplicate["state"], "REMOTE_ACTIVE");
        let old_input = serde_json::json!({
            "grant_id": after_duplicate["input_grant"], "sequence": 1,
            "event": {"kind": "key", "code": 30},
        })
        .to_string();
        let (_, sent) = call_input(&router, r#"{"kind":"key","code":30}"#).await;
        assert_eq!(sent["events"].as_array().unwrap().len(), 1);
        assert_eq!(
            call(&router, "/api/simulation/input", Some(&old_input))
                .await
                .0,
            StatusCode::CONFLICT
        );
        let (_, revoked) = call(&router, "/api/simulation/revoke", Some("{}")).await;
        assert_eq!(revoked["state"], "LOCAL_LOCKED");
        assert_eq!(revoked["epoch"], 1);
        assert_eq!(
            call_input(&router, r#"{"kind":"key","code":30}"#).await.0,
            StatusCode::CONFLICT
        );
        drop(router);

        let restarted =
            separated_router(directory.path(), Path::new(&hostd), Path::new(&agent)).unwrap();
        let (_, state) = call(&restarted, "/api/simulation", None).await;
        assert_eq!(state["state"], "LOCAL_LOCKED");
        assert_eq!(state["epoch"], 2);
        assert!(state["events"].as_array().unwrap().is_empty());
        let (status, fresh) = call_start(&restarted).await;
        assert_eq!(status, StatusCode::OK);
        assert_ne!(fresh["input_grant"], after_duplicate["input_grant"]);
        assert_eq!(
            call(&restarted, "/api/simulation/input", Some(&old_input))
                .await
                .0,
            StatusCode::CONFLICT
        );
        let (_, current) = call(&restarted, "/api/simulation", None).await;
        assert!(current["events"].as_array().unwrap().is_empty());
        drop(restarted);
        let after_active_shutdown =
            separated_router(directory.path(), Path::new(&hostd), Path::new(&agent)).unwrap();
        let (_, state) = call(&after_active_shutdown, "/api/simulation", None).await;
        assert_eq!(state["state"], "LOCAL_LOCKED");
        assert_eq!(state["epoch"], 4);
    }

    #[test]
    fn rejected_agent_grant_rolls_back_host_activation() {
        let mut simulated = SimulatedHost::new();
        simulated.authority.revoke();
        assert_eq!(
            simulated.start().unwrap_err().code,
            ErrorCode::IpcUnauthorized
        );
        assert_eq!(simulated.host.state(), State::LocalLocked);
        assert_eq!(
            simulated.host.epoch(),
            blackroom_core::epoch::SecurityEpoch::INITIAL.next()
        );
        simulated.start().unwrap();
        assert_eq!(simulated.host.state(), State::RemoteActive);
    }

    #[test]
    fn missing_offline_host_peer_never_opens_input() {
        use std::net::Shutdown;

        let mut simulated = SimulatedHost::new();
        simulated.host_wire.shutdown(Shutdown::Write).unwrap();
        assert!(simulated.start().is_err());
        assert_eq!(simulated.host.state(), State::LocalLocked);
        assert!(simulated.input(InputEvent::Key { code: 30 }).is_err());
        assert!(simulated.snapshot().events.is_empty());
    }

    #[test]
    fn active_simulation_refuses_input_after_host_peer_disappears() {
        use std::net::Shutdown;

        let mut simulated = SimulatedHost::new();
        simulated.start().unwrap();
        simulated.host_wire.shutdown(Shutdown::Write).unwrap();
        assert!(simulated.input(InputEvent::Key { code: 30 }).is_err());
        assert_eq!(simulated.snapshot().state, State::LocalLocked.as_str());
        assert!(simulated.snapshot().events.is_empty());
        let epoch_after_loss = simulated.snapshot().epoch;
        assert!(simulated.start().is_err());
        assert_eq!(simulated.snapshot().epoch, epoch_after_loss);
    }

    #[test]
    fn pending_host_revoke_blocks_the_next_input_event() {
        let mut simulated = SimulatedHost::new();
        simulated.start().unwrap();
        let update = simulated.host.revoke_update().unwrap();
        write_update(&mut simulated.host_wire, &update).unwrap();
        assert!(simulated.input(InputEvent::Key { code: 30 }).is_err());
        assert_eq!(simulated.snapshot().state, State::LocalLocked.as_str());
        assert_eq!(simulated.snapshot().epoch, 1);
        assert!(simulated.snapshot().events.is_empty());
    }
}
