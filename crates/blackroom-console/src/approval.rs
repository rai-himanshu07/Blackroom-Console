//! A connection the laptop owner has to accept: the client's Start waits here (at most `WAIT`) for an Accept or Deny that only
//! the laptop's own interfaces can give (the indicator over D-Bus). No answer is a Deny.

use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;

pub const WAIT: Duration = Duration::from_secs(30);

struct Pending {
    id: u64,
    mode: String,
    device: String,
    since: Instant,
    wait: Duration,
    decision: Option<bool>,
}

#[derive(Default)]
struct State {
    next_id: u64,
    pending: Option<Pending>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingView {
    pub id: u64,
    pub mode: String,
    pub device: String,
    pub secs_left: u64,
}

#[derive(Default)]
pub struct Approvals {
    state: Mutex<State>,
    changed: Condvar,
}

fn lock(mutex: &Mutex<State>) -> MutexGuard<'_, State> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Text that goes into a notification: printable ASCII only, short.
pub fn clean(text: &str, max: usize) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '?'
            }
        })
        .take(max)
        .collect()
}

impl Approvals {
    /// Blocks until the owner decides or `wait` passes. Only one request can wait at a time.
    pub fn ask(&self, mode: &str, device: &str, wait: Duration) -> Result<(), String> {
        let id = {
            let mut state = lock(&self.state);
            if state.pending.is_some() {
                return Err("another connection is waiting for the laptop owner".into());
            }
            state.next_id += 1;
            let id = state.next_id;
            state.pending = Some(Pending {
                id,
                mode: clean(mode, 16),
                device: clean(device, 100),
                since: Instant::now(),
                wait,
                decision: None,
            });
            id
        };
        let guard = lock(&self.state);
        let (mut state, _) = self
            .changed
            .wait_timeout_while(guard, wait, |state| {
                state
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.id == id && pending.decision.is_none())
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let decision = state.pending.take().and_then(|pending| pending.decision);
        drop(state);
        match decision {
            Some(true) => Ok(()),
            Some(false) => Err("the laptop owner denied the connection".into()),
            None => Err("nobody on the laptop accepted the connection in time".into()),
        }
    }

    /// False when `id` is not the request that is waiting (late, repeated or invented).
    pub fn decide(&self, id: u64, accept: bool) -> bool {
        let mut state = lock(&self.state);
        match state.pending.as_mut() {
            Some(pending) if pending.id == id && pending.decision.is_none() => {
                pending.decision = Some(accept);
                drop(state);
                self.changed.notify_all();
                true
            }
            _ => false,
        }
    }

    pub fn pending(&self) -> Option<PendingView> {
        let state = lock(&self.state);
        state
            .pending
            .as_ref()
            .filter(|pending| pending.decision.is_none())
            .map(|pending| PendingView {
                id: pending.id,
                mode: pending.mode.clone(),
                device: pending.device.clone(),
                secs_left: pending
                    .wait
                    .saturating_sub(pending.since.elapsed())
                    .as_secs(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn asked(
        approvals: &Arc<Approvals>,
        wait: Duration,
    ) -> std::thread::JoinHandle<Result<(), String>> {
        let approvals = Arc::clone(approvals);
        std::thread::spawn(move || approvals.ask("private", "192.168.1.52 (Chrome)", wait))
    }

    fn wait_for_pending(approvals: &Approvals) -> PendingView {
        for _ in 0..200 {
            if let Some(view) = approvals.pending() {
                return view;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no request became pending");
    }

    #[test]
    fn accept_lets_the_start_through() {
        let approvals = Arc::new(Approvals::default());
        let waiting = asked(&approvals, Duration::from_secs(5));
        let view = wait_for_pending(&approvals);
        assert_eq!(
            (view.mode.as_str(), view.device.as_str()),
            ("private", "192.168.1.52 (Chrome)")
        );
        assert!(approvals.decide(view.id, true));
        assert_eq!(waiting.join().unwrap(), Ok(()));
        assert!(approvals.pending().is_none());
    }

    #[test]
    fn deny_and_silence_both_refuse() {
        let approvals = Arc::new(Approvals::default());
        let waiting = asked(&approvals, Duration::from_secs(5));
        let view = wait_for_pending(&approvals);
        assert!(approvals.decide(view.id, false));
        assert!(waiting.join().unwrap().unwrap_err().contains("denied"));
        let started = Instant::now();
        let err = approvals
            .ask("shared", "x", Duration::from_millis(100))
            .unwrap_err();
        assert!(err.contains("in time") && started.elapsed() < Duration::from_secs(3));
        assert!(approvals.pending().is_none(), "a timed-out request is gone");
    }

    #[test]
    fn a_stale_or_invented_answer_does_nothing_and_only_one_request_waits() {
        let approvals = Arc::new(Approvals::default());
        assert!(!approvals.decide(1, true), "nothing is pending");
        let waiting = asked(&approvals, Duration::from_millis(400));
        let view = wait_for_pending(&approvals);
        assert!(!approvals.decide(view.id + 7, true));
        assert!(
            approvals
                .ask("private", "other", Duration::from_secs(1))
                .unwrap_err()
                .contains("another")
        );
        assert!(waiting.join().unwrap().is_err());
        assert!(!approvals.decide(view.id, true), "late answers are refused");
    }

    #[test]
    fn notification_text_is_plain_ascii_and_short() {
        assert_eq!(clean("a\nb\u{202e}c<>", 100), "a?b?c<>");
        assert_eq!(clean(&"x".repeat(500), 10).len(), 10);
    }
}
