//! The authentication authority (roadmap Phase 11): hostd's real login service.
//!
//! Two private Unix sockets in the owner-only runtime directory:
//! * `auth.sock`: `login`, `check`, `logout` for the console or gateway. A login needs the Linux
//!   password (PAM helper), a TOTP code (or recovery code) and the Remote Access Key or a trusted
//!   device; it returns a bearer session token bound to the current security epoch.
//! * `admin.sock`: the operator verbs (`status`, `sessions`, `revoke_session`, `revoke_client`,
//!   `revoke_all`, `disable`, `enable`), for the owner's uid only. `enable` can additionally be
//!   gated by polkit so that only a local, active session can re-open remote access.
//!
//! The slow password check runs outside the lock that guards sessions and the epoch, so a flood of
//! logins can never delay an operator's `revoke_all` or `disable`.

use std::fs::File;
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_store::SecretStore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::audit::{AuditEvent, AuditLog};
use crate::auth::{CredentialVerifier, HostSessions, Principal, SessionToken};
use crate::login::{ClientFactor, LoginAttempt, MultiFactorVerifier, SecondFactor};
use crate::offline_control::{read_frame_within, write_frame};
use crate::password::{PasswordCheck, password_ok};
use crate::store::PersistentHostAuthority;
use crate::{remote_switch, trusted_devices};

pub const AUTH_SOCKET: &str = "auth.sock";
pub const ADMIN_SOCKET: &str = "admin.sock";
pub const POLKIT_ACTION: &str = "org.blackroom.console.enable-remote-access";
const REQUEST_WAIT: Duration = Duration::from_secs(5);
/// A login session ends after this long without use, and never lives past [`SESSION_ABSOLUTE`].
pub const SESSION_IDLE: Duration = Duration::from_secs(30 * 60);
pub const SESSION_ABSOLUTE: Duration = blackroom_core::limits::SESSION_CREDENTIAL_MAX_LIFETIME;

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthRequest {
    Login {
        account: String,
        client_id: String,
        #[serde(default)]
        password: Option<Zeroizing<String>>,
        #[serde(default)]
        totp: Option<String>,
        #[serde(default)]
        recovery_code: Option<Zeroizing<String>>,
        #[serde(default)]
        access_key: Option<Zeroizing<String>>,
        #[serde(default)]
        device_id: Option<String>,
        #[serde(default)]
        device_secret: Option<Zeroizing<String>>,
        /// After a login with the access key: also register this client as a trusted device.
        #[serde(default)]
        trust_label: Option<String>,
    },
    Check {
        token: Zeroizing<String>,
    },
    Logout {
        token: Zeroizing<String>,
    },
}

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AuthReply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<Zeroizing<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<u64>,
    /// Shown once, after `trust_label`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_secret: Option<Zeroizing<String>>,
}

impl AuthReply {
    fn refused(code: ErrorCode) -> Self {
        Self {
            code: Some(code.as_str().to_string()),
            ..Self::default()
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdminRequest {
    Status {},
    Sessions {},
    RevokeSession {
        id: String,
    },
    RevokeClient {
        client_id: String,
    },
    RevokeAll {},
    Disable {
        #[serde(default)]
        reason: Option<String>,
    },
    Enable {},
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(deny_unknown_fields)]
pub struct SessionView {
    pub id: String,
    pub user_id: String,
    pub client_id: String,
    pub expires_unix_ms: u64,
}

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
pub struct AdminReply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<u64>,
    /// `enabled`, `disabled` or `unreadable` (an unreadable store counts as disabled).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_access: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_sessions: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<Vec<SessionView>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked: Option<u64>,
}

impl AdminReply {
    fn refused(code: &str) -> Self {
        Self {
            code: Some(code.to_string()),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    pub uid: u32,
    pub pid: i32,
}

/// Decides whether the caller may re-open remote access.
pub trait EnableAuthorizer: Send + Sync {
    fn allow(&self, peer: Peer) -> bool;
}

/// The owner's uid is enough (the default).
pub struct OwnerOnly;

impl EnableAuthorizer for OwnerOnly {
    fn allow(&self, _peer: Peer) -> bool {
        true
    }
}

/// Asks polkit (`pkcheck`) about the calling process; by the shipped policy only a local, active
/// session of the owner is allowed, so a remote shell can close remote access but not re-open it.
pub struct Polkit {
    pub pkcheck: PathBuf,
}

fn process_start_time(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(19)?.parse().ok()
}

impl EnableAuthorizer for Polkit {
    fn allow(&self, peer: Peer) -> bool {
        let Some(start) = process_start_time(peer.pid) else {
            return false;
        };
        let Ok(mut child) = Command::new(&self.pkcheck)
            .args(["--action-id", POLKIT_ACTION, "--process"])
            .arg(format!("{},{start},{}", peer.pid, peer.uid))
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.success(),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
            }
        }
    }
}

struct Authority {
    host: PersistentHostAuthority,
    sessions: HostSessions,
    verifier: MultiFactorVerifier,
    store: SecretStore,
    audit: AuditLog,
}

pub struct AuthDaemon {
    inner: Mutex<Authority>,
    owner_uid: u32,
    enable_gate: Box<dyn EnableAuthorizer>,
}

fn lock(inner: &Mutex<Authority>) -> MutexGuard<'_, Authority> {
    inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Hands an already-verified principal to `HostSessions::authenticate`.
struct Approved(Option<Principal>);

impl CredentialVerifier for Approved {
    type Presented = ();

    fn verify(&mut self, (): ()) -> Result<Principal, BlackroomError> {
        self.0
            .take()
            .ok_or_else(|| BlackroomError::new(ErrorCode::AuthInvalid, "credential refused"))
    }
}

fn unix_ms(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

impl AuthDaemon {
    /// `verifier` must have been built without a password check of its own.
    pub fn new(
        directory: &File,
        verifier: MultiFactorVerifier,
        enable_gate: Box<dyn EnableAuthorizer>,
    ) -> io::Result<Self> {
        let host = PersistentHostAuthority::open(directory)?;
        let mut audit = AuditLog::open(directory)?;
        audit.record(&AuditEvent::HostStarted {
            epoch: host.epoch().value(),
        })?;
        let store = SecretStore::open(directory).map_err(io::Error::other)?;
        Ok(Self {
            inner: Mutex::new(Authority {
                host,
                sessions: HostSessions::default(),
                verifier,
                store,
                audit,
            }),
            owner_uid: rustix::process::getuid().as_raw(),
            enable_gate,
        })
    }

    pub fn handle_auth(
        &self,
        request: AuthRequest,
        pam: &mut dyn PasswordCheck,
        now: SystemTime,
    ) -> AuthReply {
        match request {
            AuthRequest::Login {
                account,
                client_id,
                password,
                totp,
                recovery_code,
                access_key,
                device_id,
                device_secret,
                trust_label,
            } => {
                let second = match (totp, recovery_code) {
                    (Some(code), None) => Some(SecondFactor::Totp(code)),
                    (None, Some(code)) => Some(SecondFactor::Recovery(code)),
                    (None, None) => None,
                    // Two second factors at once is ambiguous: refuse rather than choose.
                    (Some(_), Some(_)) => {
                        return AuthReply::refused(ErrorCode::AuthInvalid);
                    }
                };
                let client = match (access_key, device_id, device_secret) {
                    (Some(key), None, None) => Some(ClientFactor::AccessKey(key)),
                    (None, Some(device_id), Some(secret)) => {
                        Some(ClientFactor::Device { device_id, secret })
                    }
                    (None, None, None) => None,
                    _ => return AuthReply::refused(ErrorCode::AuthInvalid),
                };
                let used_access_key = matches!(client, Some(ClientFactor::AccessKey(_)));
                let attempt = LoginAttempt {
                    account,
                    client_id,
                    password,
                    second,
                    client,
                };
                self.login(attempt, pam, trust_label.filter(|_| used_access_key), now)
            }
            AuthRequest::Check { token } => self.check(&token, now),
            AuthRequest::Logout { token } => {
                let mut authority = lock(&self.inner);
                let ended = SessionToken::from_hex(&token)
                    .is_some_and(|token| authority.sessions.revoke(&token));
                AuthReply {
                    ok: ended,
                    ..AuthReply::default()
                }
            }
        }
    }

    fn login(
        &self,
        attempt: LoginAttempt,
        pam: &mut dyn PasswordCheck,
        trust_label: Option<String>,
        now: SystemTime,
    ) -> AuthReply {
        {
            let mut authority = lock(&self.inner);
            // After an emergency stop remote access stays closed until local recovery clears it.
            if authority.host.emergency_required() {
                return AuthReply::refused(ErrorCode::HostUnavailable);
            }
            if let Err(error) = authority.verifier.begin(&attempt) {
                let _ = authority.audit.record(&AuditEvent::AuthRefused {
                    code: error.code.as_str(),
                });
                return AuthReply::refused(error.code);
            }
        }
        // The slow PAM call: no lock is held, so operator commands are never delayed by it.
        let verdict = attempt
            .password
            .as_ref()
            .filter(|password| password_ok(password))
            .map(|password| pam.check(&attempt.account, password));

        let mut authority = lock(&self.inner);
        let (account, client_id) = (attempt.account.clone(), attempt.client_id.clone());
        let principal = match authority.verifier.complete(attempt, verdict) {
            Ok(principal) => principal,
            Err(error) => {
                let _ = authority.audit.record(&AuditEvent::AuthRefused {
                    code: error.code.as_str(),
                });
                return AuthReply::refused(error.code);
            }
        };
        let epoch = authority.host.epoch();
        let token = match authority.sessions.authenticate_with(
            &mut Approved(Some(principal.clone())),
            (),
            epoch,
            now,
            SESSION_IDLE,
            SESSION_ABSOLUTE,
        ) {
            Ok(token) => token,
            Err(error) => return AuthReply::refused(error.code),
        };
        let _ = authority.audit.record(&AuditEvent::LoginAccepted {
            user_id: &account,
            client_id: &client_id,
        });
        let mut reply = AuthReply {
            ok: true,
            token: Some(token.to_hex()),
            expires_unix_ms: authority
                .sessions
                .resolve(&token, epoch, now)
                .ok()
                .map(|session| unix_ms(session.expires_at())),
            user_id: Some(principal.user_id.clone()),
            client_id: Some(principal.client_id),
            epoch: Some(epoch.value()),
            ..AuthReply::default()
        };
        if let Some(label) = trust_label {
            match trusted_devices::register(&authority.store, &account, &label) {
                Ok((device_id, secret)) => {
                    let _ = authority.audit.record(&AuditEvent::CredentialChanged {
                        kind: "trusted_device",
                        action: "registered",
                    });
                    reply.device_id = Some(device_id);
                    reply.device_secret = Some(secret);
                }
                Err(_) => reply.code = Some("TRUST_NOT_REGISTERED".into()),
            }
        }
        reply
    }

    fn check(&self, token: &str, now: SystemTime) -> AuthReply {
        let mut authority = lock(&self.inner);
        if authority.host.emergency_required() || remote_switch::is_disabled(&authority.store) {
            return AuthReply::refused(ErrorCode::HostUnavailable);
        }
        let Some(token) = SessionToken::from_hex(token) else {
            return AuthReply::refused(ErrorCode::SessionNotFound);
        };
        let epoch = authority.host.epoch();
        match authority.sessions.touch(&token, epoch, now, SESSION_IDLE) {
            Ok(session) => AuthReply {
                ok: true,
                user_id: Some(session.principal().user_id.clone()),
                client_id: Some(session.principal().client_id.clone()),
                expires_unix_ms: Some(unix_ms(session.expires_at())),
                epoch: Some(session.epoch().value()),
                ..AuthReply::default()
            },
            Err(error) => AuthReply::refused(error.code),
        }
    }

    pub fn handle_admin(&self, peer: Peer, request: AdminRequest, now: SystemTime) -> AdminReply {
        if peer.uid != self.owner_uid {
            return AdminReply::refused(ErrorCode::IpcUnauthorized.as_str());
        }
        // Polkit is asked outside the lock; it may take a while.
        if matches!(request, AdminRequest::Enable {}) && !self.enable_gate.allow(peer) {
            return AdminReply::refused(ErrorCode::IpcUnauthorized.as_str());
        }
        let mut authority = lock(&self.inner);
        let epoch = authority.host.epoch();
        match request {
            AdminRequest::Status {} => AdminReply {
                ok: true,
                epoch: Some(epoch.value()),
                remote_access: Some(access_label(&authority.store)),
                live_sessions: Some(authority.sessions.list(epoch, now).len() as u64),
                ..AdminReply::default()
            },
            AdminRequest::Sessions {} => AdminReply {
                ok: true,
                epoch: Some(epoch.value()),
                sessions: Some(
                    authority
                        .sessions
                        .list(epoch, now)
                        .into_iter()
                        .map(|info| SessionView {
                            id: info.id,
                            user_id: info.user_id,
                            client_id: info.client_id,
                            expires_unix_ms: unix_ms(info.expires_at),
                        })
                        .collect(),
                ),
                ..AdminReply::default()
            },
            AdminRequest::RevokeSession { id } => {
                let count = u64::from(authority.sessions.revoke_by_id(&id));
                admin_done(&mut authority, "revoke_session", count)
            }
            AdminRequest::RevokeClient { client_id } => {
                let count = authority.sessions.revoke_client(&client_id) as u64;
                admin_done(&mut authority, "revoke_client", count)
            }
            AdminRequest::RevokeAll {} => revoke_all(&mut authority, "revoke_all"),
            AdminRequest::Disable { reason } => {
                // Authority is removed first; only then is the durable flag written, so a store
                // failure can never leave sessions alive.
                let ended = revoke_all(&mut authority, "disable");
                if !ended.ok {
                    return ended;
                }
                match remote_switch::set_disabled(
                    &authority.store,
                    reason.as_deref().unwrap_or("operator"),
                ) {
                    Ok(()) => ended,
                    Err(_) => AdminReply::refused(ErrorCode::HostUnavailable.as_str()),
                }
            }
            AdminRequest::Enable {} => match remote_switch::set_enabled(&authority.store) {
                Ok(()) => admin_done(&mut authority, "enable", 0),
                Err(_) => AdminReply::refused(ErrorCode::HostUnavailable.as_str()),
            },
        }
    }
}

fn access_label(store: &SecretStore) -> String {
    match remote_switch::state(store) {
        Ok(None) => "enabled",
        Ok(Some(_)) => "disabled",
        Err(_) => "unreadable",
    }
    .into()
}

fn admin_done(authority: &mut Authority, action: &'static str, count: u64) -> AdminReply {
    let _ = authority
        .audit
        .record(&AuditEvent::AdminAction { action, count });
    AdminReply {
        ok: true,
        epoch: Some(authority.host.epoch().value()),
        revoked: Some(count),
        ..AdminReply::default()
    }
}

/// Ends every session and raises the epoch, so that nothing issued earlier can resolve again.
fn revoke_all(authority: &mut Authority, action: &'static str) -> AdminReply {
    let count = authority
        .sessions
        .list(authority.host.epoch(), SystemTime::now())
        .len() as u64;
    authority.sessions.clear();
    if authority.host.advance_epoch().is_err() {
        // Sessions are already gone; the epoch could not be persisted, so report the failure.
        return AdminReply::refused(ErrorCode::RecoveryFailed.as_str());
    }
    admin_done(authority, action, count)
}

/// Binds a socket that only this user can reach, replacing a stale socket of ours.
pub fn bind_private(runtime: &Path, name: &str) -> io::Result<UnixListener> {
    let directory = runtime.metadata()?;
    if !directory.is_dir()
        || directory.uid() != rustix::process::getuid().as_raw()
        || directory.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "runtime directory must be owner-only",
        ));
    }
    let path = runtime.join(name);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata)
            if metadata.file_type().is_socket()
                && metadata.uid() == rustix::process::getuid().as_raw() =>
        {
            std::fs::remove_file(&path)?;
        }
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "a non-socket file is in the way",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn peer_of(stream: &UnixStream) -> io::Result<Peer> {
    let credentials = rustix::net::sockopt::socket_peercred(stream)?;
    Ok(Peer {
        uid: credentials.uid.as_raw(),
        pid: credentials.pid.as_raw_nonzero().get(),
    })
}

fn next_connection(listener: &UnixListener) -> io::Result<Option<UnixStream>> {
    let mut fds = [rustix::event::PollFd::new(
        listener,
        rustix::event::PollFlags::IN,
    )];
    let timeout = rustix::event::Timespec {
        tv_sec: 0,
        tv_nsec: 250_000_000,
    };
    if rustix::event::poll(&mut fds, Some(&timeout))? == 0 {
        return Ok(None);
    }
    match listener.accept() {
        Ok((stream, _)) => Ok(Some(stream)),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(error),
    }
}

/// Builds one password check per login connection, so a slow PAM call never blocks another request.
pub type PamFactory = Arc<dyn Fn() -> Box<dyn PasswordCheck> + Send + Sync>;

/// At most this many `auth.sock` requests are handled at once; more are dropped at accept.
const MAX_IN_FLIGHT: usize = 16;

/// Serves both sockets until `stop` is set. The admin socket has its own thread; each auth
/// connection gets a thread too, so `check` and `logout` never wait behind a slow password check.
pub fn serve(
    daemon: &Arc<AuthDaemon>,
    auth: UnixListener,
    admin: UnixListener,
    pam: &PamFactory,
    stop: &Arc<AtomicBool>,
) -> io::Result<()> {
    auth.set_nonblocking(true)?;
    admin.set_nonblocking(true)?;
    let admin_thread = {
        let (daemon, stop) = (Arc::clone(daemon), Arc::clone(stop));
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let Ok(connection) = next_connection(&admin) else {
                    return;
                };
                let Some(mut stream) = connection else {
                    continue;
                };
                let Ok(peer) = peer_of(&stream) else { continue };
                if let Ok(request) = read_frame_within::<AdminRequest>(&mut stream, REQUEST_WAIT) {
                    let reply = daemon.handle_admin(peer, request, SystemTime::now());
                    let _ = write_frame(&mut stream, &reply);
                }
            }
        })
    };
    let in_flight = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::SeqCst) {
        let Some(mut stream) = next_connection(&auth)? else {
            continue;
        };
        let Ok(peer) = peer_of(&stream) else { continue };
        if peer.uid != daemon.owner_uid || in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT
        {
            if peer.uid == daemon.owner_uid {
                in_flight.fetch_sub(1, Ordering::SeqCst);
            }
            continue;
        }
        let (daemon, pam, in_flight) =
            (Arc::clone(daemon), Arc::clone(pam), Arc::clone(&in_flight));
        std::thread::spawn(move || {
            if let Ok(request) = read_frame_within::<AuthRequest>(&mut stream, REQUEST_WAIT) {
                let reply = daemon.handle_auth(request, pam().as_mut(), SystemTime::now());
                let _ = write_frame(&mut stream, &reply);
            }
            in_flight.fetch_sub(1, Ordering::SeqCst);
        });
    }
    let _ = admin_thread.join();
    // Let requests already accepted finish, bounded.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while in_flight.load(Ordering::SeqCst) > 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

/// For the CLI and tests: one request, one reply.
pub fn call<Req: Serialize, Rep: serde::de::DeserializeOwned>(
    socket: &Path,
    request: &Req,
) -> io::Result<Rep> {
    call_within(socket, request, Duration::from_secs(30))
}

/// Like [`call`] with a caller-chosen deadline for the reply.
pub fn call_within<Req: Serialize, Rep: serde::de::DeserializeOwned>(
    socket: &Path,
    request: &Req,
    within: Duration,
) -> io::Result<Rep> {
    let mut stream = UnixStream::connect(socket)?;
    let credentials = rustix::net::sockopt::socket_peercred(&stream)?;
    if credentials.uid.as_raw() != rustix::process::getuid().as_raw() {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    write_frame(&mut stream, request)?;
    read_frame_within(&mut stream, within)
}
