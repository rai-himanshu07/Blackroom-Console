//! Phase 11 console login: a TOTP code instead of the URL token. Opt-in with `--auth-dir`; the verifier,
//! its limiter and its replay protection are `remote_hostd::totp`'s. Sessions live in memory only.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use remote_hostd::auth::CredentialVerifier;
use remote_hostd::totp::{TotpCredential, TotpVerifier};

/// A browser session that was not used for this long is gone.
pub const IDLE: Duration = Duration::from_secs(30 * 60);
/// No session outlives this, however active.
pub const ABSOLUTE: Duration = Duration::from_secs(12 * 3600);
const MAX_SESSIONS: usize = 8;
/// The limiter keys on (account, client); every browser is one client here, the account-wide limit still applies.
const CLIENT_ID: &str = "console";

struct Session {
    /// The console's emergency count when the code was accepted; a chord changes it and ends the session.
    epoch: u64,
    created: Instant,
    seen: Instant,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LoginError {
    Refused,
    Locked,
    Random,
}

pub struct Login {
    account: String,
    verifier: Mutex<TotpVerifier>,
    sessions: Mutex<HashMap<String, Session>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Login {
    pub fn new(account: &str, verifier: TotpVerifier) -> Self {
        Self {
            account: account.to_string(),
            verifier: Mutex::new(verifier),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn account(&self) -> &str {
        &self.account
    }

    /// Checks one code and, if it is accepted, opens a session and returns its cookie value.
    pub fn login(
        &self,
        code: &str,
        epoch: u64,
        now: Instant,
        new_id: impl FnOnce() -> Option<String>,
    ) -> Result<String, LoginError> {
        let mut verifier = lock(&self.verifier);
        if verifier.is_locked(&self.account, CLIENT_ID) {
            return Err(LoginError::Locked);
        }
        verifier
            .verify(TotpCredential {
                account: self.account.clone(),
                client_id: CLIENT_ID.to_string(),
                code: code.to_string(),
            })
            .map_err(|_| LoginError::Refused)?;
        drop(verifier);
        let id = new_id().ok_or(LoginError::Random)?;
        let mut sessions = lock(&self.sessions);
        sessions.retain(|_, session| Self::alive(session, epoch, now));
        while sessions.len() >= MAX_SESSIONS {
            let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, session)| session.seen)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            sessions.remove(&oldest);
        }
        sessions.insert(
            id.clone(),
            Session {
                epoch,
                created: now,
                seen: now,
            },
        );
        Ok(id)
    }

    fn alive(session: &Session, epoch: u64, now: Instant) -> bool {
        session.epoch == epoch
            && now.saturating_duration_since(session.seen) < IDLE
            && now.saturating_duration_since(session.created) < ABSOLUTE
    }

    /// True for a live session; use refreshes its idle timer.
    pub fn check(&self, id: &str, epoch: u64, now: Instant) -> bool {
        let mut sessions = lock(&self.sessions);
        match sessions.get_mut(id) {
            Some(session) if Self::alive(session, epoch, now) => {
                session.seen = now;
                true
            }
            Some(_) => {
                sessions.remove(id);
                false
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use remote_hostd::totp::{Limits, totp};

    use super::*;

    const SECRET: &[u8] = b"12345678901234567890";
    const NOW: u64 = 1_790_000_000;

    fn login() -> Login {
        let mut verifier = TotpVerifier::new(Limits::default()).with_clock(|| NOW);
        assert!(verifier.add_account("owner", SECRET.to_vec(), 0));
        Login::new("owner", verifier)
    }

    fn id(text: &'static str) -> impl FnOnce() -> Option<String> {
        move || Some(text.to_string())
    }

    #[test]
    fn a_right_code_opens_a_session_and_the_same_code_cannot_be_replayed() {
        let login = login();
        let t0 = Instant::now();
        let code = totp(SECRET, NOW);
        assert_eq!(login.login(&code, 0, t0, id("a")), Ok("a".into()));
        assert!(login.check("a", 0, t0));
        assert_eq!(login.login(&code, 0, t0, id("b")), Err(LoginError::Refused));
        assert!(!login.check("b", 0, t0));
    }

    #[test]
    fn wrong_codes_are_refused_and_repeated_failures_lock_the_account() {
        let login = login();
        let t0 = Instant::now();
        let wrong = if totp(SECRET, NOW) == "000000" {
            "111111"
        } else {
            "000000"
        };
        let mut seen_locked = false;
        for _ in 0..20 {
            match login.login(wrong, 0, t0, id("x")) {
                Err(LoginError::Locked) => seen_locked = true,
                Err(LoginError::Refused) => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(seen_locked, "no lockout after 20 failures");
        // Locked means even the right code is refused.
        assert_eq!(
            login.login(&totp(SECRET, NOW), 0, t0, id("y")),
            Err(LoginError::Locked)
        );
        assert!(!login.check("x", 0, t0));
    }

    #[test]
    fn sessions_expire_when_idle_or_old_and_die_with_an_emergency() {
        let login = login();
        let t0 = Instant::now();
        login.login(&totp(SECRET, NOW), 0, t0, id("a")).unwrap();
        assert!(login.check("a", 0, t0 + IDLE - Duration::from_secs(1)));
        assert!(!login.check("a", 0, t0 + IDLE * 2));
        let login = self::login();
        login.login(&totp(SECRET, NOW), 0, t0, id("b")).unwrap();
        assert!(
            !login.check("b", 1, t0),
            "a changed emergency count ends the session"
        );
        let login = self::login();
        login.login(&totp(SECRET, NOW), 0, t0, id("c")).unwrap();
        let mut now = t0;
        // Used every 10 minutes it stays alive until the absolute limit ends it.
        while now - t0 < ABSOLUTE {
            now += Duration::from_secs(600);
            if now - t0 >= ABSOLUTE {
                assert!(!login.check("c", 0, now));
            } else {
                assert!(login.check("c", 0, now));
            }
        }
    }

    #[test]
    fn at_most_eight_sessions_exist_and_the_least_recently_used_goes_first() {
        let login = login();
        let t0 = Instant::now();
        let ids: Vec<String> = (0..9).map(|n| format!("s{n}")).collect();
        for (n, session) in ids.iter().enumerate() {
            // A fresh code each time: a later step than the last accepted one.
            let at = NOW + 30 * (n as u64 + 1);
            let mut verifier = lock(&login.verifier);
            *verifier = {
                let mut fresh = TotpVerifier::new(Limits::default()).with_clock(move || at);
                fresh.add_account("owner", SECRET.to_vec(), 0);
                fresh
            };
            drop(verifier);
            let session = session.clone();
            login
                .login(
                    &totp(SECRET, at),
                    0,
                    t0 + Duration::from_secs(n as u64),
                    move || Some(session),
                )
                .unwrap();
        }
        let now = t0 + Duration::from_secs(10);
        assert!(!login.check("s0", 0, now), "the oldest session was evicted");
        assert!(login.check("s8", 0, now));
        assert_eq!(lock(&login.sessions).len(), 8);
    }
}
