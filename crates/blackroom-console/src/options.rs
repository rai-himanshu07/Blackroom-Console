//! What one remote session does to this laptop, chosen at Start. Every field has a safe default and a hard range:
//! the values come from a browser, so nothing here is trusted until `validated`.

use serde::{Deserialize, Serialize};

pub const MIN_FPS: u32 = 5;
pub const MAX_FPS: u32 = 60;
pub const MIN_BITRATE_KBPS: u32 = 300;
pub const MAX_BITRATE_KBPS: u32 = 30_000;
pub const MIN_HEARTBEAT_SECS: u32 = 5;
pub const MAX_HEARTBEAT_SECS: u32 = 120;
pub const MAX_IDLE_MINUTES: u32 = 24 * 60;
pub const MAX_SESSION_HOURS: u32 = 72;
const MIN_SIDE: u32 = 640;
const MAX_WIDTH: u32 = 3840;
const MAX_HEIGHT: u32 = 2160;
const MIN_HEIGHT: u32 = 360;

/// A requested virtual-monitor size (private mode with the panel blanked).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionOptions {
    /// Replace the physical panel with a virtual monitor and blank it (needs a restore watchdog).
    pub blank_panel: bool,
    /// Grab the built-in keyboard and touchpad so only the remote user types.
    pub block_local_input: bool,
    /// Lock the screen when the session ends normally.
    pub lock_on_stop: bool,
    /// Virtual monitor size; `None` keeps the panel's own size. Ignored unless `blank_panel`.
    pub resolution: Option<Size>,
    /// Draw the laptop's pointer into the picture (the page then hides its own pointer marker).
    pub cursor_in_video: bool,
    /// Frame-rate ceiling; 0 follows the quality level.
    pub fps_cap: u32,
    /// Video bitrate in kbit/s; 0 follows the quality level.
    pub bitrate_kbps: u32,
    /// Seconds without a browser heartbeat before the session ends; `None` uses the console's own setting.
    pub heartbeat_secs: Option<u32>,
    /// Minutes without remote input before the session ends; 0 = never.
    pub idle_minutes: u32,
    /// Hours a session may last; 0 = no limit.
    pub max_hours: u32,
}

impl Default for SessionOptions {
    /// Private: the panel goes blank, local input is blocked and the screen locks at the end.
    fn default() -> Self {
        Self {
            blank_panel: true,
            block_local_input: true,
            lock_on_stop: true,
            resolution: None,
            cursor_in_video: false,
            fps_cap: 0,
            bitrate_kbps: 0,
            heartbeat_secs: None,
            idle_minutes: 0,
            max_hours: 0,
        }
    }
}

impl SessionOptions {
    /// Leaves this laptop's screen and input alone.
    pub fn shared() -> Self {
        Self {
            blank_panel: false,
            block_local_input: false,
            lock_on_stop: false,
            ..Self::default()
        }
    }

    pub fn label(&self) -> &'static str {
        match (self.blank_panel, self.block_local_input) {
            (true, true) => "private",
            (false, false) => "shared",
            _ => "custom",
        }
    }

    /// Range-checks every value; sizes are made even because the encoders need it.
    pub fn validated(mut self) -> Result<Self, String> {
        if let Some(secs) = self.heartbeat_secs
            && !(MIN_HEARTBEAT_SECS..=MAX_HEARTBEAT_SECS).contains(&secs)
        {
            return Err(format!(
                "heartbeat_secs must be {MIN_HEARTBEAT_SECS} to {MAX_HEARTBEAT_SECS}"
            ));
        }
        if self.fps_cap != 0 && !(MIN_FPS..=MAX_FPS).contains(&self.fps_cap) {
            return Err(format!("fps_cap must be 0 or {MIN_FPS} to {MAX_FPS}"));
        }
        if self.bitrate_kbps != 0
            && !(MIN_BITRATE_KBPS..=MAX_BITRATE_KBPS).contains(&self.bitrate_kbps)
        {
            return Err(format!(
                "bitrate_kbps must be 0 or {MIN_BITRATE_KBPS} to {MAX_BITRATE_KBPS}"
            ));
        }
        if self.idle_minutes > MAX_IDLE_MINUTES {
            return Err(format!("idle_minutes must be at most {MAX_IDLE_MINUTES}"));
        }
        if self.max_hours > MAX_SESSION_HOURS {
            return Err(format!("max_hours must be at most {MAX_SESSION_HOURS}"));
        }
        if let Some(size) = &mut self.resolution {
            if !(MIN_SIDE..=MAX_WIDTH).contains(&size.width)
                || !(MIN_HEIGHT..=MAX_HEIGHT).contains(&size.height)
            {
                return Err(format!(
                    "resolution must be {MIN_SIDE}x{MIN_HEIGHT} to {MAX_WIDTH}x{MAX_HEIGHT}"
                ));
            }
            size.width &= !1;
            size.height &= !1;
        }
        Ok(self)
    }
}

impl SessionOptions {
    /// Why a session should end for lack of input or for age, if it should.
    pub fn expired(&self, idle: std::time::Duration, age: std::time::Duration) -> Option<String> {
        if self.idle_minutes > 0
            && idle > std::time::Duration::from_secs(u64::from(self.idle_minutes) * 60)
        {
            return Some(format!("no input for {} minutes", self.idle_minutes));
        }
        if self.max_hours > 0
            && age > std::time::Duration::from_secs(u64::from(self.max_hours) * 3600)
        {
            return Some(format!(
                "the {} hour session limit was reached",
                self.max_hours
            ));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_private_and_shared_leaves_the_laptop_alone() {
        let private = SessionOptions::default();
        assert_eq!(private.label(), "private");
        assert!(private.lock_on_stop);
        let shared = SessionOptions::shared();
        assert_eq!(shared.label(), "shared");
        assert!(!shared.blank_panel && !shared.block_local_input && !shared.lock_on_stop);
        let mixed = SessionOptions {
            block_local_input: false,
            ..SessionOptions::default()
        };
        assert_eq!(mixed.label(), "custom");
    }

    #[test]
    fn json_fills_gaps_with_defaults_and_refuses_unknown_keys() {
        let parsed: SessionOptions = serde_json::from_str(r#"{"blank_panel":false}"#).unwrap();
        assert!(!parsed.blank_panel && parsed.block_local_input && parsed.lock_on_stop);
        assert!(serde_json::from_str::<SessionOptions>(r#"{"blank":false}"#).is_err());
        assert!(serde_json::from_str::<SessionOptions>(r#"{"idle_minutes":-1}"#).is_err());
        assert!(serde_json::from_str::<SessionOptions>(r#"{"resolution":{"width":1}}"#).is_err());
        assert_eq!(
            serde_json::from_str::<SessionOptions>("{}").unwrap(),
            SessionOptions::default()
        );
    }

    #[test]
    fn ranges_are_enforced_and_sizes_made_even() {
        let ok = SessionOptions {
            resolution: Some(Size {
                width: 1281,
                height: 721,
            }),
            heartbeat_secs: Some(30),
            idle_minutes: 60,
            max_hours: 8,
            ..SessionOptions::default()
        }
        .validated()
        .unwrap();
        assert_eq!(
            ok.resolution,
            Some(Size {
                width: 1280,
                height: 720
            })
        );
        for bad in [
            SessionOptions {
                fps_cap: 4,
                ..Default::default()
            },
            SessionOptions {
                fps_cap: 61,
                ..Default::default()
            },
            SessionOptions {
                bitrate_kbps: 299,
                ..Default::default()
            },
            SessionOptions {
                bitrate_kbps: 30_001,
                ..Default::default()
            },
            SessionOptions {
                heartbeat_secs: Some(4),
                ..Default::default()
            },
            SessionOptions {
                heartbeat_secs: Some(121),
                ..Default::default()
            },
            SessionOptions {
                idle_minutes: MAX_IDLE_MINUTES + 1,
                ..Default::default()
            },
            SessionOptions {
                max_hours: MAX_SESSION_HOURS + 1,
                ..Default::default()
            },
            SessionOptions {
                resolution: Some(Size {
                    width: 100,
                    height: 100,
                }),
                ..Default::default()
            },
            SessionOptions {
                resolution: Some(Size {
                    width: 8000,
                    height: 4000,
                }),
                ..Default::default()
            },
        ] {
            assert!(bad.validated().is_err());
        }
    }

    #[test]
    fn idle_and_age_limits_end_a_session_only_when_set() {
        use std::time::Duration;
        let minutes = |m: u64| Duration::from_secs(m * 60);
        let none = SessionOptions::default();
        assert_eq!(none.expired(minutes(10_000), minutes(10_000)), None);
        let limited = SessionOptions {
            idle_minutes: 30,
            max_hours: 2,
            ..SessionOptions::default()
        };
        assert_eq!(limited.expired(minutes(29), minutes(60)), None);
        assert!(
            limited
                .expired(minutes(31), minutes(60))
                .unwrap()
                .contains("30 minutes")
        );
        assert!(
            limited
                .expired(minutes(1), minutes(121))
                .unwrap()
                .contains("2 hour")
        );
    }
}
