//! Emergency chord detection: a fixed set of keys held together for a minimum
//! time. Only chord keys are tracked; other key codes are never stored.

use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    codes: BTreeSet<u16>,
    hold_ms: u64,
}

impl Chord {
    /// `None` for an empty chord, which would fire on any poll.
    pub fn new(codes: &[u16], hold_ms: u64) -> Option<Self> {
        if codes.is_empty() {
            return None;
        }
        Some(Self {
            codes: codes.iter().copied().collect(),
            hold_ms,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ChordDetector {
    chord: Chord,
    down: BTreeSet<u16>,
    complete_since: Option<u64>,
    fired: bool,
}

impl ChordDetector {
    pub fn new(chord: Chord) -> Self {
        Self {
            chord,
            down: BTreeSet::new(),
            complete_since: None,
            fired: false,
        }
    }

    /// Feeds one key transition. Non-chord keys are ignored.
    pub fn key(&mut self, code: u16, pressed: bool, now_ms: u64) {
        if !self.chord.codes.contains(&code) {
            return;
        }
        if pressed {
            self.down.insert(code);
        } else {
            self.down.remove(&code);
        }
        if self.down.len() == self.chord.codes.len() {
            self.complete_since.get_or_insert(now_ms);
        } else {
            self.complete_since = None;
            self.fired = false;
        }
    }

    /// True once per hold, when every chord key has been down long enough.
    pub fn poll(&mut self, now_ms: u64) -> bool {
        match self.complete_since {
            Some(since) if !self.fired && now_ms.saturating_sub(since) >= self.chord.hold_ms => {
                self.fired = true;
                true
            }
            _ => false,
        }
    }

    /// Forget held keys, for example when devices are released or removed.
    pub fn reset(&mut self) {
        self.down.clear();
        self.complete_since = None;
        self.fired = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detector() -> ChordDetector {
        ChordDetector::new(Chord::new(&[29, 56, 42, 1], 3000).expect("chord"))
    }

    fn press_all(detector: &mut ChordDetector, at: u64) {
        for code in [29, 56, 42, 1] {
            detector.key(code, true, at);
        }
    }

    #[test]
    fn fires_once_after_the_full_chord_is_held_long_enough() {
        let mut detector = detector();
        press_all(&mut detector, 100);
        assert!(!detector.poll(3099));
        assert!(detector.poll(3100));
        assert!(!detector.poll(4000), "only once per hold");
    }

    #[test]
    fn releasing_any_chord_key_cancels_and_a_new_hold_can_fire_again() {
        let mut detector = detector();
        press_all(&mut detector, 0);
        detector.key(1, false, 2000);
        assert!(!detector.poll(5000));
        detector.key(1, true, 5000);
        assert!(!detector.poll(7999));
        assert!(detector.poll(8000));
    }

    #[test]
    fn partial_chords_and_other_keys_never_fire() {
        let mut detector = detector();
        for code in [29, 56, 42] {
            detector.key(code, true, 0);
        }
        for other in [30, 31, 32] {
            detector.key(other, true, 0);
        }
        assert!(!detector.poll(10_000));
        assert!(detector.down.len() <= 3, "non-chord keys are not stored");
    }

    #[test]
    fn reset_forgets_held_keys_and_an_empty_chord_is_refused() {
        let mut detector = detector();
        press_all(&mut detector, 0);
        detector.reset();
        assert!(!detector.poll(10_000));
        assert!(Chord::new(&[], 1000).is_none());
    }
}
