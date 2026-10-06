//! Failure limiter shared by every credential check that has no limiter of its own: per
//! (account, client) lockouts that double up to a bound, plus a per-account lock that stops a
//! distributed guesser. Same policy numbers as the TOTP verifier (`totp::Limits`).

use std::collections::HashMap;

use crate::secret_hash::unix_now;
use crate::totp::Limits;

const MAX_TRACKED: usize = 4096;

#[derive(Default)]
struct Attempts {
    failures: u32,
    locked_until: u64,
}

#[derive(Default)]
struct AccountAttempts {
    recent: Vec<u64>,
    locked_until: u64,
    locks: u32,
}

type Clock = Box<dyn Fn() -> u64 + Send>;

pub struct FailureLimiter {
    limits: Limits,
    per_client: HashMap<(String, String), Attempts>,
    per_account: HashMap<String, AccountAttempts>,
    /// Checks that have started and not finished: each counts as a failure that has not been recorded yet.
    in_flight: HashMap<(String, String), u32>,
    in_flight_account: HashMap<String, u32>,
    now: Clock,
}

impl FailureLimiter {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            per_client: HashMap::new(),
            per_account: HashMap::new(),
            in_flight: HashMap::new(),
            in_flight_account: HashMap::new(),
            now: Box::new(unix_now),
        }
    }

    pub fn with_clock(mut self, clock: impl Fn() -> u64 + Send + 'static) -> Self {
        self.now = Box::new(clock);
        self
    }

    pub fn is_locked(&self, account: &str, client: &str) -> bool {
        self.client_locked(account, client) || self.account_locked(account)
    }

    /// This source has failed too often for this account.
    pub fn client_locked(&self, account: &str, client: &str) -> bool {
        let now = (self.now)();
        self.per_client
            .get(&(account.to_string(), client.to_string()))
            .is_some_and(|attempts| attempts.locked_until > now)
    }

    /// Failures across all sources locked the account (a stranger's guesses can cause this).
    pub fn account_locked(&self, account: &str) -> bool {
        let now = (self.now)();
        self.per_account
            .get(account)
            .is_some_and(|attempts| attempts.locked_until > now)
    }

    fn prune(&mut self, now: u64) {
        let window = self.limits.window_secs;
        self.per_client
            .retain(|_, attempts| attempts.locked_until > now);
        self.per_account.retain(|_, attempts| {
            attempts.locked_until > now
                || attempts.locks > 0
                || attempts
                    .recent
                    .iter()
                    .any(|at| now.saturating_sub(*at) < window)
        });
    }

    /// False when the tables are full of live entries and this pair is new: the caller must then
    /// refuse as rate limited before doing any expensive check, so a flood of unknown names can
    /// neither grow memory nor cost a password check each.
    pub fn has_room(&mut self, account: &str, client: &str) -> bool {
        let key = (account.to_string(), client.to_string());
        if (self.per_client.contains_key(&key) && self.per_account.contains_key(account))
            || (self.per_client.len() < MAX_TRACKED && self.per_account.len() < MAX_TRACKED)
        {
            return true;
        }
        let now = (self.now)();
        self.prune(now);
        self.per_client.len() < MAX_TRACKED && self.per_account.len() < MAX_TRACKED
    }

    /// Starts a check. `false` when the checks already running could, if they all fail, use up this source's or this
    /// account's allowance: parallel guesses must not get past a limit that none of them has tripped yet.
    pub fn reserve(&mut self, account: &str, client: &str) -> bool {
        let key = (account.to_string(), client.to_string());
        let failures = self.per_client.get(&key).map_or(0, |a| a.failures);
        let flying = self.in_flight.get(&key).copied().unwrap_or(0);
        if failures.saturating_add(flying) > self.limits.free_failures {
            return false;
        }
        let recent = self
            .per_account
            .get(account)
            .map_or(0, |a| u32::try_from(a.recent.len()).unwrap_or(u32::MAX));
        let account_flying = self.in_flight_account.get(account).copied().unwrap_or(0);
        if recent.saturating_add(account_flying) >= self.limits.account_failures {
            return false;
        }
        *self.in_flight.entry(key).or_insert(0) += 1;
        *self
            .in_flight_account
            .entry(account.to_string())
            .or_insert(0) += 1;
        true
    }

    /// Ends a check started with [`Self::reserve`] (whatever its outcome).
    pub fn release(&mut self, account: &str, client: &str) {
        let key = (account.to_string(), client.to_string());
        if let Some(count) = self.in_flight.get_mut(&key) {
            *count -= 1;
            if *count == 0 {
                self.in_flight.remove(&key);
            }
        }
        if let Some(count) = self.in_flight_account.get_mut(account) {
            *count -= 1;
            if *count == 0 {
                self.in_flight_account.remove(account);
            }
        }
    }

    /// Returns `false` when there is no room to track the failure (see [`Self::has_room`]).
    pub fn record_failure(&mut self, account: &str, client: &str) -> bool {
        if !self.has_room(account, client) {
            return false;
        }
        let now = (self.now)();
        let limits = self.limits;
        let key = (account.to_string(), client.to_string());
        let attempts = self.per_client.entry(key).or_default();
        attempts.failures = attempts.failures.saturating_add(1);
        if attempts.failures > limits.free_failures {
            let doublings = (attempts.failures - limits.free_failures - 1).min(16);
            attempts.locked_until = now.saturating_add(
                limits
                    .base_lock_secs
                    .saturating_mul(1_u64 << doublings)
                    .min(limits.max_lock_secs),
            );
        }
        let account_attempts = self.per_account.entry(account.to_string()).or_default();
        account_attempts
            .recent
            .retain(|at| now.saturating_sub(*at) < limits.window_secs);
        account_attempts.recent.push(now);
        if u32::try_from(account_attempts.recent.len()).unwrap_or(u32::MAX)
            >= limits.account_failures
        {
            account_attempts.locked_until = now.saturating_add(
                limits
                    .account_lock_secs
                    .saturating_mul(1_u64 << account_attempts.locks.min(16))
                    .min(limits.account_max_lock_secs),
            );
            account_attempts.locks = account_attempts.locks.saturating_add(1);
            account_attempts.recent.clear();
        }
        true
    }

    pub fn record_success(&mut self, account: &str, client: &str) {
        self.per_client
            .remove(&(account.to_string(), client.to_string()));
        if let Some(attempts) = self.per_account.get_mut(account) {
            attempts.locks = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn limiter() -> (FailureLimiter, Arc<AtomicU64>) {
        let clock = Arc::new(AtomicU64::new(1_000));
        let shared = Arc::clone(&clock);
        (
            FailureLimiter::new(Limits::default())
                .with_clock(move || shared.load(Ordering::SeqCst)),
            clock,
        )
    }

    #[test]
    fn five_failures_lock_a_client_and_the_lock_expires() {
        let (mut limiter, clock) = limiter();
        for _ in 0..4 {
            assert!(limiter.record_failure("owner", "c"));
            assert!(!limiter.is_locked("owner", "c"));
        }
        limiter.record_failure("owner", "c");
        assert!(limiter.is_locked("owner", "c"));
        assert!(!limiter.is_locked("owner", "other"));
        clock.store(1_000 + 15 * 60, Ordering::SeqCst);
        assert!(!limiter.is_locked("owner", "c"));
        limiter.record_failure("owner", "c");
        assert!(
            limiter.is_locked("owner", "c"),
            "the next failure doubles the lock"
        );
    }

    #[test]
    fn many_clients_cannot_dodge_the_account_lock() {
        let (mut limiter, _) = limiter();
        for index in 0..10 {
            limiter.record_failure("owner", &format!("client-{index}"));
        }
        assert!(limiter.is_locked("owner", "brand-new-client"));
    }

    #[test]
    fn success_clears_the_client_counter() {
        let (mut limiter, _) = limiter();
        for _ in 0..4 {
            limiter.record_failure("owner", "c");
        }
        limiter.record_success("owner", "c");
        limiter.record_failure("owner", "c");
        assert!(!limiter.is_locked("owner", "c"));
    }

    #[test]
    fn a_flood_of_unknown_names_is_bounded() {
        let (mut limiter, _) = limiter();
        let mut refused = false;
        for index in 0..(MAX_TRACKED + 10) {
            refused |= !limiter.record_failure(&format!("a{index}"), "c");
        }
        assert!(refused);
        assert!(limiter.per_client.len() <= MAX_TRACKED);
    }

    #[test]
    fn parallel_checks_cannot_get_past_a_limit_none_of_them_has_tripped() {
        let (mut limiter, _) = limiter();
        // The source may fail five times before the lock: five checks may run at once, not sixteen.
        assert!((0..5).all(|_| limiter.reserve("owner", "tablet")));
        assert!(!limiter.reserve("owner", "tablet"));
        limiter.release("owner", "tablet");
        assert!(limiter.reserve("owner", "tablet"));
        // A failure already recorded takes a place from the allowance.
        let (mut second, _) = self::tests::limiter();
        second.record_failure("owner", "tablet");
        second.record_failure("owner", "tablet");
        assert!((0..3).all(|_| second.reserve("owner", "tablet")));
        assert!(!second.reserve("owner", "tablet"));
    }
}
