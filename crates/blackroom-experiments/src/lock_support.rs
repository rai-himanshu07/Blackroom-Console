//! Shared lock-observation helpers for the supervised lock experiments (exp11, exp12).

use std::sync::{Arc, Mutex, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant};

use blackroom_gnome::backend::SessionInfo;
use blackroom_gnome::mutter::lock::{self, LockObservation};
use reis::event::EiEvent;
use serde::Serialize;
use serde_json::Value;
use zbus::blocking::{Connection, Proxy};

use crate::eis_support::command_line;

pub const POLL: Duration = Duration::from_millis(400);

/// Grab holders and earlier experiment binaries (kernel `comm` is cut at 15 characters).
const GRAB_HOLDERS: [&str; 6] = [
    "remote-emergenc",
    "remote-hostd",
    "remote-gateway",
    "exp09_grab_prob",
    "exp09_freeze",
    "exp08_remote_in",
];

/// Refuses to start while a physical-input grab holder runs: it would leave only SSH to unlock.
pub fn ensure_no_grab_holders() -> anyhow::Result<()> {
    for holder in GRAB_HOLDERS {
        anyhow::ensure!(
            command_line("pgrep", &["-x", holder]).is_none(),
            "{holder} is running: a physical-input grab would leave only SSH to unlock"
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct LockSnap {
    pub screen_saver_active: bool,
    pub logind_locked_hint: bool,
    /// False on this host: the ScreenSaver owner is not in the login1 session.
    pub owner_session_verified: bool,
}

impl From<LockObservation> for LockSnap {
    fn from(value: LockObservation) -> Self {
        Self {
            screen_saver_active: value.screen_saver_active,
            logind_locked_hint: value.logind_locked_hint,
            owner_session_verified: value.screen_saver_session_verified,
        }
    }
}

impl LockSnap {
    pub fn locked(self) -> bool {
        self.screen_saver_active && self.logind_locked_hint
    }

    pub fn unlocked(self) -> bool {
        !self.screen_saver_active && !self.logind_locked_hint
    }
}

pub fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub fn eis_label(event: &EiEvent) -> &'static str {
    match event {
        EiEvent::DeviceAdded(_) => "DeviceAdded",
        EiEvent::DeviceResumed(_) => "DeviceResumed",
        EiEvent::DevicePaused(_) => "DevicePaused",
        EiEvent::DeviceRemoved(_) => "DeviceRemoved",
        EiEvent::SeatRemoved(_) => "SeatRemoved",
        EiEvent::Disconnected(_) => "Disconnected",
        _ => "Other",
    }
}

pub fn lock_state(info: &SessionInfo) -> Option<LockSnap> {
    lock::observe(info).ok().map(LockSnap::from)
}

/// Polls until `matches` holds; returns the milliseconds since `since`.
pub fn wait_lock_state(
    info: &SessionInfo,
    matches: fn(LockSnap) -> bool,
    timeout: Duration,
    since: Instant,
) -> Option<u64> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if lock_state(info).is_some_and(matches) {
            return Some(elapsed_ms(since));
        }
        sleep(POLL);
    }
    None
}

/// Records every `ActiveChanged` the ScreenSaver emits, with milliseconds since `t0`.
pub fn watch_active_changed(t0: Instant) -> Arc<Mutex<Vec<(u64, bool)>>> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    std::thread::spawn(move || {
        let Ok(connection) = Connection::session() else {
            return;
        };
        let Ok(proxy) = Proxy::new(
            &connection,
            "org.gnome.ScreenSaver",
            "/org/gnome/ScreenSaver",
            "org.gnome.ScreenSaver",
        ) else {
            return;
        };
        let Ok(signals) = proxy.receive_signal("ActiveChanged") else {
            return;
        };
        for message in signals {
            if let Ok((active,)) = message.body().deserialize::<(bool,)>() {
                sink.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((elapsed_ms(t0), active));
            }
        }
    });
    log
}

/// Counts only: a leak while locked must not persist whatever the operator typed.
#[derive(Debug, Serialize)]
pub struct LockedCounts {
    pub keys: u64,
    pub buttons: u64,
    pub pointer_moves: u64,
    pub wheel_events: u64,
}

pub fn locked_counts(tally: &Value) -> LockedCounts {
    let length = |value: &Value| {
        value
            .as_array()
            .map_or(u64::MAX, |items| items.len() as u64)
    };
    LockedCounts {
        keys: length(&tally["keys"]),
        buttons: length(&tally["buttons"]),
        pointer_moves: tally["pointer"]["moves"].as_u64().unwrap_or(u64::MAX),
        wheel_events: tally["wheel"]["events"].as_u64().unwrap_or(u64::MAX),
    }
}

pub fn key_events(tally: &Value, kind: &str, code: &str) -> usize {
    tally["keys"].as_array().map_or(0, |keys| {
        keys.iter()
            .filter(|key| key["type"] == kind && key["code"] == code)
            .count()
    })
}

pub fn untrusted(tally: &Value) -> u64 {
    tally["untrusted"].as_u64().unwrap_or(u64::MAX)
}

/// Before the lock only the injected Shift tap may have arrived.
pub fn judge_pre(tally: &Value) -> Vec<String> {
    let mut notes = Vec::new();
    let total = tally["keys"].as_array().map_or(0, Vec::len);
    if key_events(tally, "down", "ShiftLeft") != 1 || key_events(tally, "up", "ShiftLeft") != 1 {
        notes.push("pre-lock Shift tap did not arrive exactly once".to_string());
    }
    if total != 2 {
        notes.push(format!("{total} key events before the lock (expected 2)"));
    }
    if untrusted(tally) != 0 {
        notes.push("page saw untrusted events before the lock".to_string());
    }
    notes
}

/// While locked nothing injected (or physical) may reach a window behind the lock screen. The
/// pointer and wheel counts are the real witness: the page has no keyboard focus while locked.
pub fn judge_locked(counts: &LockedCounts) -> Vec<String> {
    let total = [
        counts.keys,
        counts.buttons,
        counts.pointer_moves,
        counts.wheel_events,
    ]
    .into_iter()
    .fold(0_u64, u64::saturating_add);
    if total == 0 {
        return Vec::new();
    }
    vec![format!(
        "input reached the page while locked: {} key, {} button, {} pointer, {} wheel events",
        counts.keys, counts.buttons, counts.pointer_moves, counts.wheel_events
    )]
}

/// After the unlock the same EIS connection must deliver Shift, `a` and Left, plain.
pub fn judge_unlocked(tally: &Value) -> Vec<String> {
    let mut notes = Vec::new();
    let keys = tally["keys"].as_array().cloned().unwrap_or_default();
    for code in ["ShiftLeft", "KeyA", "ArrowLeft"] {
        if key_events(tally, "down", code) != 1 || key_events(tally, "up", code) != 1 {
            notes.push(format!("{code} not exactly one down/up after the unlock"));
        }
    }
    let other = keys
        .iter()
        .filter(|key| {
            !matches!(
                key["code"].as_str(),
                Some("ShiftLeft" | "KeyA" | "ArrowLeft")
            )
        })
        .count();
    if other != 0 {
        notes.push(format!("{other} unexpected key events after the unlock"));
    }
    if keys.iter().any(|key| key["repeat"] == true) {
        notes.push("a repeated key event after the unlock".to_string());
    }
    if untrusted(tally) != 0 {
        notes.push("page saw untrusted events after the unlock".to_string());
    }
    notes
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn key(kind: &str, code: &str) -> Value {
        json!({"type": kind, "code": code, "repeat": false, "shift": false})
    }

    fn tally(keys: Vec<Value>) -> Value {
        json!({
            "keys": keys,
            "buttons": [],
            "pointer": {"moves": 0, "positions": []},
            "wheel": {"events": 0, "deltaY": 0, "scrollY": 0},
            "untrusted": 0,
        })
    }

    fn tap(code: &str) -> Vec<Value> {
        vec![key("down", code), key("up", code)]
    }

    #[test]
    fn lock_snapshots_require_both_signals_to_agree() {
        let snap = |active, hint| LockSnap {
            screen_saver_active: active,
            logind_locked_hint: hint,
            owner_session_verified: false,
        };
        assert!(snap(true, true).locked() && !snap(true, true).unlocked());
        assert!(snap(false, false).unlocked() && !snap(false, false).locked());
        for mixed in [snap(true, false), snap(false, true)] {
            assert!(!mixed.locked() && !mixed.unlocked());
        }
    }

    #[test]
    fn pre_lock_tally_is_exactly_one_shift_tap() {
        assert!(judge_pre(&tally(tap("ShiftLeft"))).is_empty());
        assert!(!judge_pre(&tally(vec![])).is_empty());
        let mut extra = tap("ShiftLeft");
        extra.extend(tap("KeyX"));
        assert!(!judge_pre(&tally(extra)).is_empty());
    }

    #[test]
    fn nothing_may_reach_the_page_while_locked() {
        assert!(judge_locked(&locked_counts(&tally(vec![]))).is_empty());
        assert!(!judge_locked(&locked_counts(&tally(tap("KeyX")))).is_empty());
        let mut moved = tally(vec![]);
        moved["pointer"]["moves"] = json!(1);
        assert!(!judge_locked(&locked_counts(&moved)).is_empty());
        let mut scrolled = tally(vec![]);
        scrolled["wheel"]["events"] = json!(1);
        assert!(!judge_locked(&locked_counts(&scrolled)).is_empty());
        let mut clicked = tally(vec![]);
        clicked["buttons"] = json!([{"type": "click", "button": 0}]);
        assert!(!judge_locked(&locked_counts(&clicked)).is_empty());
        // A malformed tally never passes as silence.
        assert!(!judge_locked(&locked_counts(&json!({}))).is_empty());
    }

    #[test]
    fn the_locked_record_keeps_counts_and_no_key_codes() {
        let leaked = locked_counts(&tally(tap("KeyP")));
        let text = serde_json::to_string(&leaked).expect("serialises");
        assert!(!text.contains("KeyP"), "{text}");
        assert_eq!(leaked.keys, 2);
    }

    #[test]
    fn the_unlocked_tally_is_exactly_shift_a_left_plain() {
        let mut good = tap("ShiftLeft");
        good.extend(tap("KeyA"));
        good.extend(tap("ArrowLeft"));
        assert!(judge_unlocked(&tally(good.clone())).is_empty());

        let mut missing = tap("ShiftLeft");
        missing.extend(tap("KeyA"));
        assert!(!judge_unlocked(&tally(missing)).is_empty());

        let mut extra = good.clone();
        extra.extend(tap("KeyX"));
        assert!(!judge_unlocked(&tally(extra)).is_empty());

        let mut repeated = good.clone();
        repeated[2]["repeat"] = json!(true);
        assert!(!judge_unlocked(&tally(repeated)).is_empty());

        let mut synthetic = tally(good);
        synthetic["untrusted"] = json!(1);
        assert!(!judge_unlocked(&synthetic).is_empty());
    }
}
