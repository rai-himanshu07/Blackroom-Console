//! Red-team suite for the authentication authority (Doc 18 §7-10 and §15): the `RT-AUTH`,
//! `RT-SESSION`, `RT-EPOCH` and `RT-FILE` cases that can be driven without a browser, a real PAM
//! stack or a second Unix user. Each test name starts with its case id; `docs/security/rt-matrix.md`
//! lists what is covered here, elsewhere or deferred.

use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use blackroom_core::limits::MAX_CONCURRENT_AUTH_SESSIONS;
use blackroom_store::SecretStore;
use remote_hostd::authd::{
    AdminReply, AdminRequest, AuthDaemon, AuthReply, AuthRequest, EnableAuthorizer, OwnerOnly,
    Peer, SESSION_ABSOLUTE, SESSION_IDLE,
};
use remote_hostd::login::MultiFactorVerifier;
use remote_hostd::password::{PasswordCheck, PasswordOutcome};
use remote_hostd::ratelimit::FailureLimiter;
use remote_hostd::store::PersistentHostAuthority;
use remote_hostd::totp::{self, Limits, base32_decode, totp as code_at};
use remote_hostd::{access_key, recovery_codes, remote_switch, trusted_devices};
use zeroize::Zeroizing;

const OWNER: &str = "owner";
const PASSWORD: &str = "correct horse battery";
const START: u64 = 1_800_000_000;

struct FakePam(Arc<AtomicUsize>);

impl PasswordCheck for FakePam {
    fn check(&mut self, account: &str, password: &str) -> PasswordOutcome {
        self.0.fetch_add(1, Ordering::SeqCst);
        if account == OWNER && password == PASSWORD {
            PasswordOutcome::Accepted
        } else {
            PasswordOutcome::Rejected
        }
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    fd: File,
    daemon: Option<Arc<AuthDaemon>>,
    clock: Arc<AtomicU64>,
    calls: Arc<AtomicUsize>,
    totp_secret: Vec<u8>,
    key: Zeroizing<String>,
}

fn open_daemon(
    fd: &File,
    clock: &Arc<AtomicU64>,
    gate: Box<dyn EnableAuthorizer>,
) -> Arc<AuthDaemon> {
    let (verifier_clock, limiter_clock) = (Arc::clone(clock), Arc::clone(clock));
    let totp = totp::load_verifier(fd, Limits::default())
        .unwrap()
        .with_clock(move || verifier_clock.load(Ordering::SeqCst));
    let limiter = FailureLimiter::new(Limits::default())
        .with_clock(move || limiter_clock.load(Ordering::SeqCst));
    let verifier = MultiFactorVerifier::new(SecretStore::open(fd).unwrap(), totp, None, limiter);
    Arc::new(AuthDaemon::new(fd, verifier, gate).unwrap())
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let fd = File::open(dir.path()).unwrap();
        let secret = totp::enroll(&fd, OWNER).unwrap();
        let store = SecretStore::open(&fd).unwrap();
        let key = access_key::rotate(&store, OWNER).unwrap();
        let clock = Arc::new(AtomicU64::new(START));
        let daemon = open_daemon(&fd, &clock, Box::new(OwnerOnly));
        Self {
            dir,
            fd,
            daemon: Some(daemon),
            clock,
            calls: Arc::new(AtomicUsize::new(0)),
            totp_secret: base32_decode(&secret).unwrap(),
            key,
        }
    }

    fn daemon(&self) -> &Arc<AuthDaemon> {
        self.daemon.as_ref().unwrap()
    }

    /// Simulates a hostd restart on the same state directory (the old one must release its lock).
    fn restart(&mut self) {
        self.restart_with(Box::new(OwnerOnly));
    }

    fn restart_with(&mut self, gate: Box<dyn EnableAuthorizer>) {
        self.daemon = None;
        self.daemon = Some(open_daemon(&self.fd, &self.clock, gate));
    }

    fn store(&self) -> SecretStore {
        SecretStore::open(&self.fd).unwrap()
    }

    /// A TOTP code from a time step nobody has used yet.
    fn fresh_code(&self) -> String {
        let now = self.clock.fetch_add(30, Ordering::SeqCst) + 30;
        code_at(&self.totp_secret, now)
    }

    fn pam(&self) -> FakePam {
        FakePam(Arc::clone(&self.calls))
    }

    fn send(&self, request: AuthRequest) -> AuthReply {
        self.daemon()
            .handle_auth(request, &mut self.pam(), SystemTime::now())
    }

    fn admin(&self, request: AdminRequest) -> AdminReply {
        self.daemon().handle_admin(
            Peer {
                uid: rustix::process::getuid().as_raw(),
                pid: std::process::id() as i32,
            },
            request,
            SystemTime::now(),
        )
    }

    fn sessions(&self) -> usize {
        self.admin(AdminRequest::Sessions {})
            .sessions
            .unwrap()
            .len()
    }

    /// The complete, correct login for a new client.
    fn good(&self) -> Attempt {
        Attempt {
            account: OWNER.into(),
            client: "browser-1".into(),
            password: Some(PASSWORD.into()),
            totp: Some(self.fresh_code()),
            recovery: None,
            key: Some(self.key.to_string()),
            device: None,
            trust: None,
        }
    }
}

struct Attempt {
    account: String,
    client: String,
    password: Option<String>,
    totp: Option<String>,
    recovery: Option<String>,
    key: Option<String>,
    device: Option<(String, String)>,
    trust: Option<String>,
}

impl Attempt {
    fn request(self) -> AuthRequest {
        let secret = |text: String| Zeroizing::new(text);
        AuthRequest::Login {
            account: self.account,
            client_id: self.client,
            password: self.password.map(secret),
            totp: self.totp,
            recovery_code: self.recovery.map(secret),
            access_key: self.key.map(secret),
            device_id: self.device.as_ref().map(|(id, _)| id.clone()),
            device_secret: self.device.map(|(_, device)| secret(device)),
            trust_label: self.trust,
        }
    }
}

fn refused(reply: &AuthReply, code: &str) {
    assert!(!reply.ok, "login must be refused");
    assert_eq!(reply.code.as_deref(), Some(code));
    assert!(reply.token.is_none() && reply.device_secret.is_none());
}

fn wire(reply: &AuthReply) -> String {
    serde_json::to_string(reply).unwrap()
}

// ---- RT-AUTH ----

#[test]
fn rt_auth_001_missing_password_is_refused_without_a_session() {
    let fx = Fixture::new();
    let mut attempt = fx.good();
    attempt.password = None;
    refused(&fx.send(attempt.request()), "AUTH_INVALID");
    assert_eq!(fx.sessions(), 0);
    assert_eq!(
        fx.calls.load(Ordering::SeqCst),
        0,
        "no password check was even attempted"
    );
}

#[test]
fn rt_auth_002_a_wrong_password_is_refused_and_repeats_are_rate_limited() {
    let fx = Fixture::new();
    for _ in 0..5 {
        let mut attempt = fx.good();
        attempt.password = Some("wrong".into());
        refused(&fx.send(attempt.request()), "AUTH_INVALID");
    }
    let mut attempt = fx.good();
    attempt.password = Some(PASSWORD.into());
    refused(&fx.send(attempt.request()), "AUTH_RATE_LIMITED");
    assert_eq!(
        fx.calls.load(Ordering::SeqCst),
        5,
        "a locked client costs no password check"
    );
    assert_eq!(fx.sessions(), 0);
}

#[test]
fn rt_auth_003_a_password_never_substitutes_for_totp() {
    let fx = Fixture::new();
    let mut attempt = fx.good();
    attempt.totp = None;
    refused(&fx.send(attempt.request()), "AUTH_TOTP_REQUIRED");
    assert_eq!(fx.sessions(), 0);
}

#[test]
fn rt_auth_004_an_invalid_totp_is_refused() {
    let fx = Fixture::new();
    let mut attempt = fx.good();
    attempt.totp = Some("000000".into());
    refused(&fx.send(attempt.request()), "AUTH_INVALID");
    assert_eq!(fx.sessions(), 0);
}

#[test]
fn rt_auth_005_a_totp_code_is_accepted_once() {
    let fx = Fixture::new();
    let attempt = fx.good();
    let code = attempt.totp.clone().unwrap();
    assert!(fx.send(attempt.request()).ok);
    let mut replay = fx.good();
    replay.totp = Some(code);
    refused(&fx.send(replay.request()), "AUTH_INVALID");
    assert_eq!(fx.sessions(), 1);
}

#[test]
fn rt_auth_006_a_new_device_needs_the_remote_access_key() {
    let fx = Fixture::new();
    let mut attempt = fx.good();
    attempt.key = None;
    refused(&fx.send(attempt.request()), "AUTH_ACCESS_KEY_REQUIRED");
    assert_eq!(fx.sessions(), 0);
}

#[test]
fn rt_auth_007_a_wrong_access_key_is_refused_and_spends_no_totp_code() {
    let fx = Fixture::new();
    let mut attempt = fx.good();
    let code = attempt.totp.clone().unwrap();
    attempt.key = Some("not-the-key".into());
    refused(&fx.send(attempt.request()), "AUTH_INVALID");
    assert_eq!(fx.sessions(), 0);
    let mut retry = fx.good();
    retry.totp = Some(code);
    assert!(
        fx.send(retry.request()).ok,
        "the unspent code still works with the right key"
    );
}

fn trusted(fx: &Fixture) -> (String, String) {
    let mut attempt = fx.good();
    attempt.trust = Some("Laptop A".into());
    let reply = fx.send(attempt.request());
    assert!(reply.ok);
    let (id, secret) = (reply.device_id.unwrap(), reply.device_secret.unwrap());
    (id, secret.to_string())
}

#[test]
fn rt_auth_008_a_trusted_device_still_needs_totp() {
    let fx = Fixture::new();
    let device = trusted(&fx);
    let mut attempt = fx.good();
    attempt.key = None;
    attempt.device = Some(device);
    attempt.totp = None;
    refused(&fx.send(attempt.request()), "AUTH_TOTP_REQUIRED");
}

#[test]
fn rt_auth_009_a_copied_device_credential_alone_opens_nothing_and_can_be_revoked() {
    let fx = Fixture::new();
    let (id, secret) = trusted(&fx);
    // Credential only: no password, no TOTP.
    let mut stolen = fx.good();
    stolen.password = None;
    stolen.totp = None;
    stolen.key = None;
    stolen.device = Some((id.clone(), secret.clone()));
    refused(&fx.send(stolen.request()), "AUTH_INVALID");
    // With the password and TOTP too (the documented trust model), it works, then revocation ends it.
    let mut full = fx.good();
    full.key = None;
    full.device = Some((id.clone(), secret));
    let reply = fx.send(full.request());
    assert!(reply.ok);
    assert_eq!(
        reply.client_id.as_deref(),
        Some(id.as_str()),
        "the session is bound to the device"
    );
    let live = fx.sessions();
    let ended = fx.admin(AdminRequest::RevokeClient { client_id: id });
    assert_eq!(ended.revoked, Some(1));
    assert_eq!(fx.sessions(), live - 1);
}

#[test]
fn rt_auth_010_a_revoked_device_is_refused() {
    let fx = Fixture::new();
    let (id, secret) = trusted(&fx);
    assert!(trusted_devices::revoke(&fx.store(), &id).unwrap());
    let before = fx.sessions();
    let mut attempt = fx.good();
    attempt.key = None;
    attempt.device = Some((id, secret));
    refused(&fx.send(attempt.request()), "AUTH_INVALID");
    assert_eq!(fx.sessions(), before);
}

#[test]
fn rt_auth_011_a_rotated_access_key_is_refused() {
    let fx = Fixture::new();
    let old = fx.key.to_string();
    let new = access_key::rotate(&fx.store(), OWNER).unwrap();
    let mut attempt = fx.good();
    attempt.key = Some(old);
    refused(&fx.send(attempt.request()), "AUTH_INVALID");
    let mut attempt = fx.good();
    attempt.key = Some(new.to_string());
    assert!(fx.send(attempt.request()).ok);
}

#[test]
fn rt_auth_012_a_recovery_code_never_replaces_the_access_key() {
    let fx = Fixture::new();
    let codes = recovery_codes::generate(&fx.store(), OWNER).unwrap();
    let mut without_key = fx.good();
    without_key.totp = None;
    without_key.recovery = Some(codes[0].to_string());
    without_key.key = None;
    refused(&fx.send(without_key.request()), "AUTH_ACCESS_KEY_REQUIRED");
    assert_eq!(
        recovery_codes::remaining(&fx.store(), OWNER).unwrap(),
        10,
        "the code was not spent"
    );

    let mut with_key = fx.good();
    with_key.totp = None;
    with_key.recovery = Some(codes[0].to_string());
    assert!(fx.send(with_key.request()).ok);
    let mut again = fx.good();
    again.totp = None;
    again.recovery = Some(codes[0].to_string());
    refused(&fx.send(again.request()), "AUTH_INVALID");
}

#[test]
fn rt_auth_013_unknown_and_known_accounts_are_indistinguishable() {
    let fx = Fixture::new();
    let mut known = fx.good();
    known.password = Some("wrong".into());
    let known = fx.send(known.request());
    let mut unknown = fx.good();
    unknown.account = "ghost".into();
    unknown.password = Some("wrong".into());
    let unknown = fx.send(unknown.request());
    assert_eq!(wire(&known), wire(&unknown));
    assert_eq!(
        fx.calls.load(Ordering::SeqCst),
        2,
        "both cost the same password check"
    );
    // Missing-factor answers are only given after the password passed.
    let mut probe = fx.good();
    probe.account = "ghost".into();
    probe.totp = None;
    refused(&fx.send(probe.request()), "AUTH_INVALID");
}

#[test]
fn rt_auth_014_a_flood_is_bounded_and_stops_costing_password_checks() {
    let fx = Fixture::new();
    for index in 0..4200 {
        let mut attempt = fx.good();
        attempt.account = format!("u{index}");
        attempt.client = format!("c{index}");
        assert!(!fx.send(attempt.request()).ok);
        // Keep the fake clock still so no failure ages out during the flood.
        fx.clock.store(START, Ordering::SeqCst);
    }
    let checks = fx.calls.load(Ordering::SeqCst);
    assert!(checks <= 4200);
    let before = fx.calls.load(Ordering::SeqCst);
    let mut attempt = fx.good();
    attempt.account = "one-more".into();
    attempt.client = "one-more".into();
    refused(&fx.send(attempt.request()), "AUTH_RATE_LIMITED");
    assert_eq!(
        fx.calls.load(Ordering::SeqCst),
        before,
        "saturated: refused before any password check"
    );
    assert_eq!(fx.sessions(), 0);
}

#[test]
fn rt_auth_015_concurrent_logins_with_one_code_open_exactly_one_session() {
    let fx = Fixture::new();
    let code = fx.fresh_code();
    let mut threads = Vec::new();
    for index in 0..8 {
        let (daemon, calls) = (Arc::clone(fx.daemon()), Arc::clone(&fx.calls));
        let (key, code) = (fx.key.to_string(), code.clone());
        threads.push(std::thread::spawn(move || {
            let request = Attempt {
                account: OWNER.into(),
                client: format!("client-{index}"),
                password: Some(PASSWORD.into()),
                totp: Some(code),
                recovery: None,
                key: Some(key),
                device: None,
                trust: None,
            }
            .request();
            daemon
                .handle_auth(request, &mut FakePam(calls), SystemTime::now())
                .ok
        }));
    }
    let accepted = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(accepted, 1);
    assert_eq!(fx.sessions(), 1);
}

// ---- RT-SESSION ----

fn token_of(reply: &AuthReply) -> Zeroizing<String> {
    reply.token.clone().expect("a session token")
}

fn check(fx: &Fixture, token: &Zeroizing<String>, at: SystemTime) -> AuthReply {
    fx.daemon().handle_auth(
        AuthRequest::Check {
            token: token.clone(),
        },
        &mut fx.pam(),
        at,
    )
}

#[test]
fn rt_session_001_an_expired_session_credential_is_refused() {
    let fx = Fixture::new();
    let token = token_of(&fx.send(fx.good().request()));
    assert!(check(&fx, &token, SystemTime::now()).ok);
    let later = SystemTime::now() + SESSION_IDLE + Duration::from_secs(1);
    assert!(!check(&fx, &token, later).ok);
}

#[test]
fn rt_session_001b_use_keeps_a_session_alive_but_never_past_the_hard_limit() {
    let fx = Fixture::new();
    let token = token_of(&fx.send(fx.good().request()));
    let start = SystemTime::now();
    for step in 1..=35_u32 {
        let at = start + Duration::from_secs(20 * 60) * step;
        assert!(
            check(&fx, &token, at).ok,
            "used every 20 minutes: step {step}"
        );
    }
    // Idle longer than the idle limit ends it even before the hard limit.
    let idle_end =
        start + Duration::from_secs(20 * 60) * 35 + SESSION_IDLE + Duration::from_secs(1);
    assert!(!check(&fx, &token, idle_end).ok);

    let other = token_of(&fx.send(fx.good().request()));
    let begun = SystemTime::now();
    let mut at = begun;
    while at < begun + SESSION_ABSOLUTE - Duration::from_secs(25 * 60) {
        at += Duration::from_secs(25 * 60);
        assert!(check(&fx, &other, at).ok);
    }
    let past = begun + SESSION_ABSOLUTE + Duration::from_secs(60);
    assert!(
        !check(&fx, &other, past).ok,
        "never beyond 12 hours in total"
    );
}

#[test]
fn rt_session_002_a_credential_from_another_host_is_unknown() {
    let (here, elsewhere) = (Fixture::new(), Fixture::new());
    let foreign = token_of(&elsewhere.send(elsewhere.good().request()));
    refused(
        &check(&here, &foreign, SystemTime::now()),
        "SESSION_NOT_FOUND",
    );
    for bad in ["", "zz", &"0".repeat(64), &"a".repeat(65)] {
        let reply = check(&here, &Zeroizing::new(bad.to_string()), SystemTime::now());
        assert!(!reply.ok, "{bad}");
    }
}

#[test]
fn rt_session_003_a_session_is_bound_to_its_principal() {
    let fx = Fixture::new();
    let reply = fx.send(fx.good().request());
    let seen = check(&fx, &token_of(&reply), SystemTime::now());
    assert_eq!(seen.user_id.as_deref(), Some(OWNER));
    assert_eq!(seen.client_id.as_deref(), Some("browser-1"));
    assert_eq!(seen.epoch, reply.epoch);
}

#[test]
fn rt_session_004_a_captured_credential_fails_after_logout_expiry_revocation_and_epoch_change() {
    let fx = Fixture::new();
    let after_logout = token_of(&fx.send(fx.good().request()));
    assert!(
        fx.send(AuthRequest::Logout {
            token: after_logout.clone()
        })
        .ok
    );
    assert!(!check(&fx, &after_logout, SystemTime::now()).ok);

    let by_id = token_of(&fx.send(fx.good().request()));
    let id = fx.admin(AdminRequest::Sessions {}).sessions.unwrap()[0]
        .id
        .clone();
    assert!(
        !id.is_empty() && !by_id.contains(&id),
        "the public id is not the bearer token"
    );
    assert_eq!(
        fx.admin(AdminRequest::RevokeSession { id }).revoked,
        Some(1)
    );
    assert!(!check(&fx, &by_id, SystemTime::now()).ok);

    let after_epoch = token_of(&fx.send(fx.good().request()));
    assert!(fx.admin(AdminRequest::RevokeAll {}).ok);
    assert!(!check(&fx, &after_epoch, SystemTime::now()).ok);
}

#[test]
fn rt_session_005_concurrent_sessions_are_capped() {
    let fx = Fixture::new();
    for _ in 0..MAX_CONCURRENT_AUTH_SESSIONS {
        assert!(fx.send(fx.good().request()).ok);
    }
    refused(&fx.send(fx.good().request()), "AUTH_RATE_LIMITED");
    assert_eq!(fx.sessions(), MAX_CONCURRENT_AUTH_SESSIONS as usize);
}

// ---- RT-EPOCH ----

#[test]
fn rt_epoch_001_every_session_of_an_older_epoch_stops_resolving() {
    let fx = Fixture::new();
    let tokens: Vec<_> = (0..3)
        .map(|_| token_of(&fx.send(fx.good().request())))
        .collect();
    let before = fx.admin(AdminRequest::Status {}).epoch.unwrap();
    assert_eq!(fx.admin(AdminRequest::RevokeAll {}).revoked, Some(3));
    assert_eq!(fx.admin(AdminRequest::Status {}).epoch, Some(before + 1));
    for token in &tokens {
        assert!(!check(&fx, token, SystemTime::now()).ok);
    }
    assert_eq!(fx.sessions(), 0);
    assert!(
        fx.send(fx.good().request()).ok,
        "a fresh login works in the new epoch"
    );
}

#[test]
fn rt_epoch_002_after_an_emergency_stop_old_credentials_and_new_logins_are_refused() {
    let mut fx = Fixture::new();
    let token = token_of(&fx.send(fx.good().request()));
    let epoch = fx.admin(AdminRequest::Status {}).epoch.unwrap();
    fx.daemon = None;
    let stopped = PersistentHostAuthority::emergency_stop(&fx.fd).unwrap();
    assert!(stopped.value() > epoch);
    fx.restart();
    assert!(!check(&fx, &token, SystemTime::now()).ok);
    refused(&fx.send(fx.good().request()), "HOST_UNAVAILABLE");
    assert_eq!(fx.sessions(), 0);
}

#[test]
fn rt_epoch_003_nothing_resolves_once_revoke_all_has_returned_even_under_load() {
    let fx = Fixture::new();
    let tokens: Vec<_> = (0..3)
        .map(|_| token_of(&fx.send(fx.good().request())))
        .collect();
    let revoked = Arc::new(AtomicBool::new(false));
    let checker = {
        let (daemon, revoked, calls) = (
            Arc::clone(fx.daemon()),
            Arc::clone(&revoked),
            Arc::clone(&fx.calls),
        );
        std::thread::spawn(move || {
            let mut violations = 0;
            for round in 0..2000 {
                let done = revoked.load(Ordering::SeqCst);
                let token = tokens[round % tokens.len()].clone();
                let reply = daemon.handle_auth(
                    AuthRequest::Check { token },
                    &mut FakePam(Arc::clone(&calls)),
                    SystemTime::now(),
                );
                if done && reply.ok {
                    violations += 1;
                }
            }
            violations
        })
    };
    std::thread::sleep(Duration::from_millis(5));
    assert!(fx.admin(AdminRequest::RevokeAll {}).ok);
    revoked.store(true, Ordering::SeqCst);
    assert_eq!(checker.join().unwrap(), 0);
}

#[test]
fn rt_epoch_004_the_epoch_survives_a_restart_and_never_goes_back() {
    let mut fx = Fixture::new();
    let first = fx.admin(AdminRequest::Status {}).epoch.unwrap();
    fx.admin(AdminRequest::RevokeAll {});
    let raised = fx.admin(AdminRequest::Status {}).epoch.unwrap();
    assert!(raised > first);
    let token = token_of(&fx.send(fx.good().request()));
    fx.restart();
    let after = fx.admin(AdminRequest::Status {}).epoch.unwrap();
    assert!(after > raised);
    assert!(!check(&fx, &token, SystemTime::now()).ok);
}

#[test]
fn a_strangers_guesses_lock_the_account_but_not_a_trusted_device() {
    let fx = Fixture::new();
    let (id, secret) = trusted(&fx);
    // Ten sources each fail five times: the account itself is now locked.
    for source in 0..10 {
        let mut attempt = fx.good();
        attempt.client = format!("stranger-{source}");
        attempt.password = Some("guess".into());
        assert!(!fx.send(attempt.request()).ok);
    }
    let mut by_key = fx.good();
    by_key.client = "owner-new-browser".into();
    refused(&fx.send(by_key.request()), "AUTH_RATE_LIMITED");
    let mut by_device = fx.good();
    by_device.client = "owner-trusted-browser".into();
    by_device.key = None;
    by_device.device = Some((id, secret));
    assert!(
        fx.send(by_device.request()).ok,
        "a trusted device is not locked out by strangers"
    );
}

// ---- operator control ----

struct Deny;

impl EnableAuthorizer for Deny {
    fn allow(&self, _: Peer) -> bool {
        false
    }
}

#[test]
fn disable_closes_remote_access_at_once_and_enable_needs_the_gate() {
    let mut fx = Fixture::new();
    let token = token_of(&fx.send(fx.good().request()));
    let reason = "travelling";
    assert!(
        fx.admin(AdminRequest::Disable {
            reason: Some(reason.into())
        })
        .ok
    );
    assert_eq!(
        fx.admin(AdminRequest::Status {}).remote_access.as_deref(),
        Some("disabled")
    );
    assert!(!check(&fx, &token, SystemTime::now()).ok);
    refused(&fx.send(fx.good().request()), "HOST_UNAVAILABLE");

    fx.restart_with(Box::new(Deny));
    assert_eq!(
        fx.admin(AdminRequest::Status {}).remote_access.as_deref(),
        Some("disabled"),
        "disabled survives a restart"
    );
    let denied = fx.admin(AdminRequest::Enable {});
    assert!(!denied.ok);
    assert_eq!(denied.code.as_deref(), Some("IPC_UNAUTHORIZED"));
    assert!(remote_switch::is_disabled(&fx.store()));

    fx.restart();
    assert!(fx.admin(AdminRequest::Enable {}).ok);
    assert!(fx.send(fx.good().request()).ok);
}

#[test]
fn only_the_owner_uid_may_use_the_admin_socket() {
    let fx = Fixture::new();
    let other = Peer {
        uid: rustix::process::getuid().as_raw() + 1,
        pid: 1,
    };
    let reply = fx
        .daemon()
        .handle_admin(other, AdminRequest::RevokeAll {}, SystemTime::now());
    assert!(!reply.ok);
    assert_eq!(reply.code.as_deref(), Some("IPC_UNAUTHORIZED"));
}

#[test]
fn a_hung_password_check_does_not_delay_operator_commands() {
    struct Slow(Arc<AtomicBool>);
    impl PasswordCheck for Slow {
        fn check(&mut self, _: &str, _: &str) -> PasswordOutcome {
            self.0.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(1500));
            PasswordOutcome::Rejected
        }
    }
    let fx = Fixture::new();
    let started = Arc::new(AtomicBool::new(false));
    let login = {
        let (daemon, started) = (Arc::clone(fx.daemon()), Arc::clone(&started));
        let request = fx.good().request();
        std::thread::spawn(move || {
            daemon.handle_auth(request, &mut Slow(started), SystemTime::now())
        })
    };
    while !started.load(Ordering::SeqCst) {
        std::thread::yield_now();
    }
    let began = std::time::Instant::now();
    assert!(fx.admin(AdminRequest::RevokeAll {}).ok);
    assert!(
        began.elapsed() < Duration::from_millis(500),
        "revoke-all waited for the password check"
    );
    assert!(!login.join().unwrap().ok);
}

// ---- RT-FILE ----

fn mode(path: &std::path::Path) -> u32 {
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn rt_file_001_every_secret_file_is_owner_only() {
    let fx = Fixture::new();
    let store = fx.store();
    recovery_codes::generate(&store, OWNER).unwrap();
    trusted_devices::register(&store, OWNER, "A").unwrap();
    remote_switch::set_disabled(&store, "x").unwrap();
    assert_eq!(mode(fx.dir.path()), 0o700);
    for entry in std::fs::read_dir(fx.dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            assert_eq!(mode(&path), 0o600, "{}", path.display());
        }
    }
}

#[test]
fn rt_file_002_a_symlinked_credential_file_closes_remote_access_and_is_never_written_through() {
    let fx = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("victim");
    std::fs::write(&target, b"untouched").unwrap();
    let path = fx.dir.path().join(access_key::FILE);
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    refused(&fx.send(fx.good().request()), "HOST_UNAVAILABLE");
    assert!(access_key::rotate(&fx.store(), OWNER).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
}

#[test]
fn rt_file_003_traversal_in_names_and_identifiers_is_refused() {
    let fx = Fixture::new();
    let store = fx.store();
    for bad in ["../x", "/etc/passwd", "a/b", "%2e%2e%2fx", ".."] {
        assert!(access_key::rotate(&store, bad).is_err(), "{bad}");
        assert!(recovery_codes::generate(&store, bad).is_err(), "{bad}");
        assert!(
            trusted_devices::register(&store, bad, "A").is_err(),
            "{bad}"
        );
        // A label is display text, never a path; ".." is a legal (if odd) label.
        assert!(
            bad == ".." || trusted_devices::register(&store, OWNER, bad).is_err(),
            "{bad}"
        );
        assert_eq!(
            trusted_devices::check(&store, OWNER, bad, "secret").unwrap(),
            trusted_devices::DeviceCheck::Refused
        );
        let mut attempt = fx.good();
        attempt.account = bad.into();
        assert!(!fx.send(attempt.request()).ok);
    }
    assert_eq!(fx.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn rt_file_004_injected_or_malformed_credential_files_fail_safe() {
    for (file, content) in [
        (
            access_key::FILE,
            r#"{"schema":1,"data":{"keys":[],"admin":true}}"#,
        ),
        (access_key::FILE, "not json at all"),
        (access_key::FILE, r#"{"schema":9,"data":{"keys":[]}}"#),
        (
            access_key::FILE,
            r#"{"schema":1,"data":{"keys":[{"account":"owner"}]}}"#,
        ),
        (
            remote_switch::FILE,
            r#"{"schema":1,"data":{"disabled":false,"since_unix":0,"reason":"","exec":"rm -rf /"}}"#,
        ),
    ] {
        let fx = Fixture::new();
        let path = fx.dir.path().join(file);
        std::fs::write(&path, content).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let reply = fx.send(fx.good().request());
        refused(&reply, "HOST_UNAVAILABLE");
    }
}

#[test]
fn rt_file_005_no_secret_reaches_the_audit_log_or_a_status_reply() {
    let fx = Fixture::new();
    let store = fx.store();
    let codes = recovery_codes::generate(&store, OWNER).unwrap();
    let mut first = fx.good();
    first.trust = Some("Laptop".into());
    let (code, key) = (first.totp.clone().unwrap(), first.key.clone().unwrap());
    let reply = fx.send(first.request());
    let (token, device_secret) = (
        token_of(&reply).to_string(),
        reply.device_secret.clone().unwrap().to_string(),
    );
    let mut wrong = fx.good();
    wrong.password = Some("hunter2-wrong".into());
    fx.send(wrong.request());
    let mut by_recovery = fx.good();
    by_recovery.totp = None;
    by_recovery.recovery = Some(codes[3].to_string());
    fx.send(by_recovery.request());
    fx.admin(AdminRequest::Disable { reason: None });

    let mut logs = String::new();
    for name in ["audit.log", "audit.log.1"] {
        if let Ok(text) = std::fs::read_to_string(fx.dir.path().join(name)) {
            logs.push_str(&text);
        }
    }
    assert!(logs.contains("login_accepted"));
    let status = serde_json::to_string(&fx.admin(AdminRequest::Status {})).unwrap();
    let listing = serde_json::to_string(&fx.admin(AdminRequest::Sessions {})).unwrap();
    for secret in [
        PASSWORD,
        "hunter2-wrong",
        key.as_str(),
        token.as_str(),
        device_secret.as_str(),
        &codes[3].replace('-', ""),
        codes[3].as_str(),
        &format!("\"{code}\""),
    ] {
        for (name, text) in [("log", &logs), ("status", &status), ("sessions", &listing)] {
            assert!(!text.contains(secret), "a secret leaked into the {name}");
        }
    }
}

#[test]
fn crash_leftovers_do_not_change_what_a_restart_trusts() {
    let mut fx = Fixture::new();
    for leftover in [
        ".tmp-aaaa",
        ".epoch-deadbeef.tmp",
        ".emergency-1.tmp",
        ".totp-ff.tmp",
    ] {
        std::fs::write(fx.dir.path().join(leftover), b"half a write").unwrap();
    }
    let key = access_key::rotate(&fx.store(), OWNER).unwrap();
    let epoch = fx.admin(AdminRequest::Status {}).epoch.unwrap();
    fx.admin(AdminRequest::RevokeAll {});
    fx.restart();
    assert!(fx.admin(AdminRequest::Status {}).epoch.unwrap() > epoch);
    assert!(access_key::verify(&fx.store(), OWNER, &key).unwrap());
    let mut attempt = fx.good();
    attempt.key = Some(key.to_string());
    assert!(fx.send(attempt.request()).ok);
}

// ---- sockets ----

#[test]
fn the_two_private_sockets_serve_a_login_and_the_operator_verbs() {
    use remote_hostd::authd::{ADMIN_SOCKET, AUTH_SOCKET, bind_private, call, serve};
    let fx = Fixture::new();
    let runtime = tempfile::tempdir().unwrap();
    std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let auth = bind_private(runtime.path(), AUTH_SOCKET).unwrap();
    let admin = bind_private(runtime.path(), ADMIN_SOCKET).unwrap();
    assert_eq!(mode(&runtime.path().join(AUTH_SOCKET)), 0o600);
    assert_eq!(mode(&runtime.path().join(ADMIN_SOCKET)), 0o600);
    let stop = Arc::new(AtomicBool::new(false));
    let server = {
        let (daemon, stop, calls) = (
            Arc::clone(fx.daemon()),
            Arc::clone(&stop),
            Arc::clone(&fx.calls),
        );
        std::thread::spawn(move || {
            let pam: remote_hostd::authd::PamFactory =
                Arc::new(move || Box::new(FakePam(Arc::clone(&calls))) as Box<dyn PasswordCheck>);
            serve(&daemon, auth, admin, &pam, &stop)
        })
    };

    let login = serde_json::json!({
        "command": "login", "account": OWNER, "client_id": "socket-client",
        "password": PASSWORD, "totp": fx.fresh_code(), "access_key": fx.key.as_str(),
    });
    let reply: AuthReply = call(&runtime.path().join(AUTH_SOCKET), &login).unwrap();
    assert!(reply.ok);
    let token = token_of(&reply);
    let seen: AuthReply = call(
        &runtime.path().join(AUTH_SOCKET),
        &serde_json::json!({"command": "check", "token": token.as_str()}),
    )
    .unwrap();
    assert_eq!(seen.user_id.as_deref(), Some(OWNER));

    let status: AdminReply = call(
        &runtime.path().join(ADMIN_SOCKET),
        &serde_json::json!({"command": "status"}),
    )
    .unwrap();
    assert_eq!(status.live_sessions, Some(1));
    let ended: AdminReply = call(
        &runtime.path().join(ADMIN_SOCKET),
        &serde_json::json!({"command": "revoke_all"}),
    )
    .unwrap();
    assert!(ended.ok);
    let gone: AuthReply = call(
        &runtime.path().join(AUTH_SOCKET),
        &serde_json::json!({"command": "check", "token": token.as_str()}),
    )
    .unwrap();
    assert!(!gone.ok);

    // Unknown fields and unknown commands are refused without a reply.
    let bad: Result<AuthReply, _> = call(
        &runtime.path().join(AUTH_SOCKET),
        &serde_json::json!({"command": "login", "account": OWNER, "client_id": "x", "admin": true}),
    );
    assert!(bad.is_err());
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap().unwrap();
}

#[test]
fn a_socket_path_held_by_a_regular_file_is_never_replaced() {
    use remote_hostd::authd::bind_private;
    let runtime = tempfile::tempdir().unwrap();
    std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(runtime.path().join("auth.sock"), b"precious").unwrap();
    assert!(bind_private(runtime.path(), "auth.sock").is_err());
    assert_eq!(
        std::fs::read(runtime.path().join("auth.sock")).unwrap(),
        b"precious"
    );
    std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(bind_private(runtime.path(), "other.sock").is_err());
}

#[test]
fn a_slow_password_check_on_the_socket_does_not_delay_check_or_logout() {
    use remote_hostd::authd::{ADMIN_SOCKET, AUTH_SOCKET, bind_private, call, serve};
    struct Sleepy(Arc<AtomicUsize>);
    impl PasswordCheck for Sleepy {
        fn check(&mut self, _: &str, password: &str) -> PasswordOutcome {
            self.0.fetch_add(1, Ordering::SeqCst);
            if password == "slow" {
                std::thread::sleep(Duration::from_millis(1500));
                return PasswordOutcome::Rejected;
            }
            PasswordOutcome::Accepted
        }
    }
    let fx = Fixture::new();
    let runtime = tempfile::tempdir().unwrap();
    std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (auth, admin) = (
        bind_private(runtime.path(), AUTH_SOCKET).unwrap(),
        bind_private(runtime.path(), ADMIN_SOCKET).unwrap(),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let server = {
        let (daemon, stop, calls) = (
            Arc::clone(fx.daemon()),
            Arc::clone(&stop),
            Arc::clone(&fx.calls),
        );
        std::thread::spawn(move || {
            let pam: remote_hostd::authd::PamFactory =
                Arc::new(move || Box::new(Sleepy(Arc::clone(&calls))) as Box<dyn PasswordCheck>);
            serve(&daemon, auth, admin, &pam, &stop)
        })
    };
    let socket = runtime.path().join(AUTH_SOCKET);
    let login = |password: &str, code: String| serde_json::json!({"command": "login", "account": OWNER, "client_id": "c", "password": password, "totp": code, "access_key": fx.key.as_str()});
    let first: AuthReply = call(&socket, &login("pw", fx.fresh_code())).unwrap();
    let token = token_of(&first);

    let slow = {
        let (socket, request) = (socket.clone(), login("slow", fx.fresh_code()));
        std::thread::spawn(move || call::<_, AuthReply>(&socket, &request).unwrap())
    };
    std::thread::sleep(Duration::from_millis(200));
    let began = std::time::Instant::now();
    let seen: AuthReply = call(
        &socket,
        &serde_json::json!({"command": "check", "token": token.as_str()}),
    )
    .unwrap();
    assert!(seen.ok);
    assert!(
        began.elapsed() < Duration::from_millis(500),
        "check waited for the password check"
    );
    assert!(!slow.join().unwrap().ok);
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap().unwrap();
}

// ---- Doc 12 section 7: the authentication flow matrix, one row per scenario ----

#[test]
fn doc12_authentication_flow_matrix() {
    let fx = Fixture::new();
    let (id, secret) = trusted(&fx);
    let codes = recovery_codes::generate(&fx.store(), OWNER).unwrap();
    let old_key = fx.key.to_string();

    // (scenario, build the attempt, expected: Ok or the refusal code)
    type Build<'a> = Box<dyn Fn(&Fixture) -> Attempt + 'a>;
    let rows: Vec<(&str, Build, Option<&str>)> = vec![
        ("new device", Box::new(|fx| fx.good()), None),
        (
            "trusted device",
            Box::new(|fx| {
                let mut a = fx.good();
                a.key = None;
                a.device = Some((id.clone(), secret.clone()));
                a
            }),
            None,
        ),
        (
            "wrong password",
            Box::new(|fx| {
                let mut a = fx.good();
                a.password = Some("wrong".into());
                a
            }),
            Some("AUTH_INVALID"),
        ),
        (
            "wrong TOTP",
            Box::new(|fx| {
                let mut a = fx.good();
                a.totp = Some("000000".into());
                a
            }),
            Some("AUTH_INVALID"),
        ),
        (
            "new device without key",
            Box::new(|fx| {
                let mut a = fx.good();
                a.key = None;
                a
            }),
            Some("AUTH_ACCESS_KEY_REQUIRED"),
        ),
        (
            "trusted device without TOTP",
            Box::new(|fx| {
                let mut a = fx.good();
                a.key = None;
                a.totp = None;
                a.device = Some((id.clone(), secret.clone()));
                a
            }),
            Some("AUTH_TOTP_REQUIRED"),
        ),
        (
            "lost authenticator, recovery code",
            Box::new(|fx| {
                let mut a = fx.good();
                a.totp = None;
                a.recovery = Some(codes[0].to_string());
                a
            }),
            None,
        ),
    ];
    // Distinct clients, so five wrong attempts never lock a later row.
    for (index, (scenario, build, expected)) in rows.iter().enumerate() {
        let mut attempt = build(&fx);
        attempt.client = format!("matrix-{index}");
        let reply = fx.send(attempt.request());
        match expected {
            None => assert!(reply.ok, "{scenario} must be allowed"),
            Some(code) => refused(&reply, code),
        }
    }

    // Revoked trusted device and revoked access key.
    assert!(trusted_devices::revoke(&fx.store(), &id).unwrap());
    let mut revoked_device = fx.good();
    revoked_device.key = None;
    revoked_device.device = Some((id.clone(), secret.clone()));
    refused(&fx.send(revoked_device.request()), "AUTH_INVALID");
    access_key::rotate(&fx.store(), OWNER).unwrap();
    let mut revoked_key = fx.good();
    revoked_key.key = Some(old_key);
    refused(&fx.send(revoked_key.request()), "AUTH_INVALID");

    // Revoked all sessions: every existing session ends.
    assert!(fx.sessions() > 0);
    fx.admin(AdminRequest::RevokeAll {});
    assert_eq!(fx.sessions(), 0);
}

// ---- crash consistency: a real SIGKILL during rotation and epoch increments ----

#[test]
fn crash_a_real_sigkill_during_key_rotation_and_epoch_increments_never_corrupts_state() {
    const CHILD: &str = "BLACKROOM_CRASH_CHILD_DIR";
    if let Some(dir) = std::env::var_os(CHILD) {
        // Child: rotate and bump forever until the parent kills it.
        let fd = File::open(&dir).unwrap();
        let store = SecretStore::open(&fd).unwrap();
        let mut host = PersistentHostAuthority::open(&fd).unwrap();
        loop {
            access_key::rotate(&store, OWNER).unwrap();
            host.advance_epoch().unwrap();
            recovery_codes::generate(&store, OWNER).unwrap();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let fd = File::open(dir.path()).unwrap();
    drop(PersistentHostAuthority::open(&fd).unwrap());
    let store = SecretStore::open(&fd).unwrap();
    let mut last_epoch = PersistentHostAuthority::inspect(&fd).unwrap().epoch;
    for round in 0..20_u64 {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "crash_a_real_sigkill_during_key_rotation_and_epoch_increments_never_corrupts_state",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(60 + (round * 37) % 140));
        child.kill().unwrap();
        child.wait().unwrap();
        let status = PersistentHostAuthority::inspect(&fd)
            .unwrap_or_else(|error| panic!("round {round}: {error}"));
        assert!(
            status.epoch >= last_epoch,
            "the epoch went backwards in round {round}"
        );
        last_epoch = status.epoch;
        access_key::configured(&store, OWNER)
            .unwrap_or_else(|error| panic!("round {round}: key file {error}"));
        recovery_codes::remaining(&store, OWNER)
            .unwrap_or_else(|error| panic!("round {round}: codes file {error}"));
    }
    assert!(last_epoch > 0, "the child must have made progress");
}
