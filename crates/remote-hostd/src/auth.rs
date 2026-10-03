//! Hostd-owned authentication sessions (Doc 03 §24-27; assessment C5).
//!
//! A lease principal can only come from a session that this registry issued
//! after a [`CredentialVerifier`] accepted the caller. Verifiers here are
//! fakes: no account, PAM, TOTP or device key is read, so nothing in this
//! module is real authentication.

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, SystemTime};

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::limits::{AUTH_SESSION_TTL, MAX_CONCURRENT_AUTH_SESSIONS};
use zeroize::Zeroizing;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal {
    pub user_id: String,
    pub client_id: String,
}

impl Principal {
    /// Identity of the offline fake adapter; not a real account or device.
    pub fn synthetic() -> Self {
        Self {
            user_id: "synthetic-user".into(),
            client_id: "synthetic-client".into(),
        }
    }
}

/// Adapter seam for the future PAM/TOTP/device-key verifier.
pub trait CredentialVerifier {
    type Presented;

    /// Must fail with `AuthInvalid` for any credential it does not accept.
    fn verify(&mut self, presented: Self::Presented) -> Result<Principal, BlackroomError>;
}

/// Unpredictable bearer credential; never logged or serialized here.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SessionToken([u8; 32]);

impl SessionToken {
    /// For handing the bearer credential to the client that just authenticated, once.
    pub fn to_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(hex::encode(self.0))
    }

    pub fn from_hex(text: &str) -> Option<Self> {
        <[u8; 32]>::try_from(hex::decode(text).ok()?).ok().map(Self)
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionToken(<redacted>)")
    }
}

#[derive(Debug)]
pub struct AuthSession {
    id: String,
    principal: Principal,
    epoch: SecurityEpoch,
    expires_at: SystemTime,
    /// `touch` never extends a session past this; equal to the first expiry for a fixed session.
    hard_limit: SystemTime,
}

/// What an operator may see of a session: an identifier that is not the bearer credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub user_id: String,
    pub client_id: String,
    pub expires_at: SystemTime,
}

impl AuthSession {
    /// Public label for listing and revoking; it cannot be used to authenticate.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    pub fn epoch(&self) -> SecurityEpoch {
        self.epoch
    }

    pub fn expires_at(&self) -> SystemTime {
        self.expires_at
    }

    fn check(&self, epoch: SecurityEpoch, now: SystemTime) -> Result<(), BlackroomError> {
        if self.epoch != epoch {
            return Err(BlackroomError::new(
                ErrorCode::SessionEpochMismatch,
                "authentication session belongs to another security epoch",
            ));
        }
        if now >= self.expires_at {
            return Err(BlackroomError::new(
                ErrorCode::AuthInvalid,
                "authentication session expired",
            ));
        }
        Ok(())
    }
}

/// In-memory registry: sessions never survive a hostd restart, and every
/// epoch change invalidates them.
#[derive(Debug, Default)]
pub struct HostSessions {
    sessions: HashMap<SessionToken, AuthSession>,
    grant: Option<(SessionToken, String)>,
}

/// Random 128-bit input binding issued with a grant. It is an opaque label
/// the peer must echo with each input, not a credential.
pub fn mint_input_grant() -> Result<String, BlackroomError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| {
        BlackroomError::new(ErrorCode::RecoveryFailed, "host input grant unavailable")
    })?;
    Ok(hex::encode(bytes))
}

/// Length-then-XOR comparison that does not short-circuit on the first difference.
pub fn same_bytes(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

impl HostSessions {
    pub fn authenticate<V: CredentialVerifier>(
        &mut self,
        verifier: &mut V,
        presented: V::Presented,
        epoch: SecurityEpoch,
        now: SystemTime,
    ) -> Result<SessionToken, BlackroomError> {
        self.authenticate_with(
            verifier,
            presented,
            epoch,
            now,
            AUTH_SESSION_TTL,
            AUTH_SESSION_TTL,
        )
    }

    /// A session that lives `idle` after each [`Self::touch`] but never beyond `absolute` from `now`.
    pub fn authenticate_with<V: CredentialVerifier>(
        &mut self,
        verifier: &mut V,
        presented: V::Presented,
        epoch: SecurityEpoch,
        now: SystemTime,
        idle: Duration,
        absolute: Duration,
    ) -> Result<SessionToken, BlackroomError> {
        self.sessions
            .retain(|_, session| session.check(epoch, now).is_ok());
        if self.sessions.len() >= MAX_CONCURRENT_AUTH_SESSIONS as usize {
            return Err(BlackroomError::new(
                ErrorCode::AuthRateLimited,
                "too many concurrent authentication sessions",
            ));
        }
        let principal = verifier.verify(presented)?;
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| {
            BlackroomError::new(
                ErrorCode::RecoveryFailed,
                "host session credential unavailable",
            )
        })?;
        let mut id = [0_u8; 8];
        getrandom::fill(&mut id).map_err(|_| {
            BlackroomError::new(
                ErrorCode::RecoveryFailed,
                "host session credential unavailable",
            )
        })?;
        let token = SessionToken(bytes);
        self.sessions.insert(
            token.clone(),
            AuthSession {
                id: hex::encode(id),
                principal,
                epoch,
                expires_at: now + idle.min(absolute),
                hard_limit: now + absolute,
            },
        );
        Ok(token)
    }

    /// Use of a live session pushes its expiry to `idle` from now, bounded by its hard limit.
    pub fn touch(
        &mut self,
        token: &SessionToken,
        epoch: SecurityEpoch,
        now: SystemTime,
        idle: Duration,
    ) -> Result<&AuthSession, BlackroomError> {
        self.resolve(token, epoch, now)?;
        let session = self.sessions.get_mut(token).expect("resolved above");
        session.expires_at = (now + idle).min(session.hard_limit).max(session.expires_at);
        Ok(session)
    }

    pub fn resolve(
        &self,
        token: &SessionToken,
        epoch: SecurityEpoch,
        now: SystemTime,
    ) -> Result<&AuthSession, BlackroomError> {
        let session = self.sessions.get(token).ok_or_else(|| {
            BlackroomError::new(ErrorCode::SessionNotFound, "unknown authentication session")
        })?;
        session.check(epoch, now)?;
        Ok(session)
    }

    /// Ties the single active grant and its input binding to `token`; a later
    /// loss of that session is reported by [`Self::grant_session_lost`].
    pub fn bind_grant(&mut self, token: &SessionToken, input_grant: String) {
        self.grant = Some((token.clone(), input_grant));
    }

    /// The live session behind the active grant, but only for its own input grant.
    pub fn session_for_grant(
        &self,
        presented: &str,
        epoch: SecurityEpoch,
        now: SystemTime,
    ) -> Result<&AuthSession, BlackroomError> {
        let (token, input_grant) = self.grant.as_ref().ok_or_else(|| {
            BlackroomError::new(ErrorCode::SessionNotFound, "no active grant session")
        })?;
        if !same_bytes(input_grant.as_bytes(), presented.as_bytes()) {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "input grant does not match the active grant",
            ));
        }
        self.resolve(token, epoch, now)
    }

    /// True only for the active grant's binding while its session is live.
    pub fn input_grant_valid(
        &self,
        presented: &str,
        epoch: SecurityEpoch,
        now: SystemTime,
    ) -> bool {
        self.session_for_grant(presented, epoch, now).is_ok()
    }

    pub fn revoke(&mut self, token: &SessionToken) -> bool {
        self.sessions.remove(token).is_some()
    }

    /// Live sessions of the current epoch, oldest first by expiry.
    pub fn list(&self, epoch: SecurityEpoch, now: SystemTime) -> Vec<SessionInfo> {
        let mut live: Vec<SessionInfo> = self
            .sessions
            .values()
            .filter(|session| session.check(epoch, now).is_ok())
            .map(|session| SessionInfo {
                id: session.id.clone(),
                user_id: session.principal.user_id.clone(),
                client_id: session.principal.client_id.clone(),
                expires_at: session.expires_at,
            })
            .collect();
        live.sort_by_key(|info| info.expires_at);
        live
    }

    /// Operator revocation by the public session id.
    pub fn revoke_by_id(&mut self, id: &str) -> bool {
        let before = self.sessions.len();
        self.sessions.retain(|_, session| session.id != id);
        before != self.sessions.len()
    }

    /// Device revocation (Doc 03 §23): ends every session of `client_id`.
    pub fn revoke_client(&mut self, client_id: &str) -> usize {
        let before = self.sessions.len();
        self.sessions
            .retain(|_, session| session.principal.client_id != client_id);
        before - self.sessions.len()
    }

    /// Ends every session and the grant binding; called with each revocation.
    pub fn clear(&mut self) {
        self.sessions.clear();
        self.grant = None;
    }

    #[cfg(test)]
    pub(crate) fn live(&self) -> usize {
        self.sessions.len()
    }

    /// True when the session behind the active grant was revoked, expired or
    /// belongs to an older epoch, so the grant must be revoked too.
    pub fn grant_session_lost(&self, epoch: SecurityEpoch, now: SystemTime) -> bool {
        self.grant
            .as_ref()
            .is_some_and(|(token, _)| self.resolve(token, epoch, now).is_err())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct Accepts(&'static str);

    impl CredentialVerifier for Accepts {
        type Presented = &'static str;

        fn verify(&mut self, presented: &'static str) -> Result<Principal, BlackroomError> {
            if presented == self.0 {
                Ok(Principal {
                    user_id: "user".into(),
                    client_id: format!("client-{presented}"),
                })
            } else {
                Err(BlackroomError::new(ErrorCode::AuthInvalid, "refused"))
            }
        }
    }

    fn login(sessions: &mut HostSessions, secret: &'static str, now: SystemTime) -> SessionToken {
        sessions
            .authenticate(&mut Accepts(secret), secret, SecurityEpoch::INITIAL, now)
            .unwrap()
    }

    #[test]
    fn refused_credential_creates_no_session() {
        let mut sessions = HostSessions::default();
        let error = sessions
            .authenticate(
                &mut Accepts("right"),
                "wrong",
                SecurityEpoch::INITIAL,
                SystemTime::now(),
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AuthInvalid);
        assert!(sessions.sessions.is_empty());
    }

    #[test]
    fn session_is_bound_to_principal_epoch_and_ttl() {
        let now = SystemTime::now();
        let mut sessions = HostSessions::default();
        let token = login(&mut sessions, "a", now);
        let session = sessions
            .resolve(&token, SecurityEpoch::INITIAL, now)
            .unwrap();
        assert_eq!(session.principal().client_id, "client-a");
        assert_eq!(session.expires_at(), now + AUTH_SESSION_TTL);
        assert_eq!(
            sessions
                .resolve(&token, SecurityEpoch::INITIAL.next(), now)
                .unwrap_err()
                .code,
            ErrorCode::SessionEpochMismatch
        );
        let last = now + AUTH_SESSION_TTL - Duration::from_secs(1);
        assert!(
            sessions
                .resolve(&token, SecurityEpoch::INITIAL, last)
                .is_ok()
        );
        assert_eq!(
            sessions
                .resolve(&token, SecurityEpoch::INITIAL, now + AUTH_SESSION_TTL)
                .unwrap_err()
                .code,
            ErrorCode::AuthInvalid
        );
    }

    #[test]
    fn tokens_are_unique_redacted_and_forgeries_are_unknown() {
        let now = SystemTime::now();
        let mut sessions = HostSessions::default();
        let first = login(&mut sessions, "a", now);
        let second = login(&mut sessions, "a", now);
        assert_ne!(first, second);
        assert_eq!(format!("{first:?}"), "SessionToken(<redacted>)");
        assert!(format!("{sessions:?}").contains("<redacted>"));
        assert_eq!(
            sessions
                .resolve(&SessionToken([0; 32]), SecurityEpoch::INITIAL, now)
                .unwrap_err()
                .code,
            ErrorCode::SessionNotFound
        );
    }

    #[test]
    fn concurrent_cap_counts_only_live_sessions() {
        let now = SystemTime::now();
        let mut sessions = HostSessions::default();
        for _ in 0..MAX_CONCURRENT_AUTH_SESSIONS {
            login(&mut sessions, "a", now);
        }
        assert_eq!(
            sessions
                .authenticate(&mut Accepts("a"), "a", SecurityEpoch::INITIAL, now)
                .unwrap_err()
                .code,
            ErrorCode::AuthRateLimited
        );
        login(&mut sessions, "a", now + AUTH_SESSION_TTL);
        assert_eq!(sessions.sessions.len(), 1);
    }

    #[test]
    fn losing_the_grant_session_is_detected_for_every_revocation_path() {
        let now = SystemTime::now();
        let epoch = SecurityEpoch::INITIAL;
        let mut sessions = HostSessions::default();
        assert!(!sessions.grant_session_lost(epoch, now));

        let token = login(&mut sessions, "a", now);
        sessions.bind_grant(&token, "g".into());
        assert!(!sessions.grant_session_lost(epoch, now));
        assert!(sessions.grant_session_lost(epoch.next(), now));
        assert!(sessions.grant_session_lost(epoch, now + AUTH_SESSION_TTL));
        assert!(sessions.revoke(&token));
        assert!(sessions.grant_session_lost(epoch, now));

        let token = login(&mut sessions, "a", now);
        let other = login(&mut sessions, "b", now);
        sessions.bind_grant(&token, "g".into());
        assert_eq!(sessions.revoke_client("client-b"), 1);
        assert!(!sessions.grant_session_lost(epoch, now));
        assert!(sessions.resolve(&other, epoch, now).is_err());
        assert_eq!(sessions.revoke_client("client-a"), 1);
        assert!(sessions.grant_session_lost(epoch, now));

        let token = login(&mut sessions, "a", now);
        sessions.bind_grant(&token, "g".into());
        sessions.clear();
        assert!(!sessions.grant_session_lost(epoch, now));
        assert!(sessions.resolve(&token, epoch, now).is_err());
    }

    #[test]
    fn input_grant_is_random_and_valid_only_for_the_live_bound_session() {
        let first = mint_input_grant().unwrap();
        assert_eq!(first.len(), 32);
        assert!(first.bytes().all(|digit| digit.is_ascii_hexdigit()));
        assert_ne!(first, mint_input_grant().unwrap());

        let now = SystemTime::now();
        let epoch = SecurityEpoch::INITIAL;
        let mut sessions = HostSessions::default();
        assert!(!sessions.input_grant_valid(&first, epoch, now));

        let token = login(&mut sessions, "a", now);
        sessions.bind_grant(&token, first.clone());
        assert!(sessions.input_grant_valid(&first, epoch, now));
        assert!(!sessions.input_grant_valid("", epoch, now));
        assert!(!sessions.input_grant_valid(&first[..31], epoch, now));
        assert!(!sessions.input_grant_valid(&mint_input_grant().unwrap(), epoch, now));
        assert!(!sessions.input_grant_valid(&first, epoch.next(), now));
        assert!(!sessions.input_grant_valid(&first, epoch, now + AUTH_SESSION_TTL));

        let other = login(&mut sessions, "b", now);
        sessions.bind_grant(&other, "replacement".into());
        assert!(!sessions.input_grant_valid(&first, epoch, now));
        assert!(sessions.revoke(&other));
        assert!(!sessions.input_grant_valid("replacement", epoch, now));

        let token = login(&mut sessions, "a", now);
        sessions.bind_grant(&token, first.clone());
        sessions.clear();
        assert!(!sessions.input_grant_valid(&first, epoch, now));
    }
}
