//! What the laptop's owner allows, kept on the laptop in `host.json` next to `profile.json`. A client can only choose inside
//! these limits: `apply` is the one place that enforces them on a session's options, and the other fields gate the clipboard,
//! sound and text typing. Nothing here is trusted until `validated`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::options::{
    MAX_BITRATE_KBPS, MAX_FPS, MAX_IDLE_MINUTES, MAX_SESSION_HOURS, MIN_BITRATE_KBPS, MIN_FPS,
};

pub const HOST_FILE: &str = "host.json";
const MAX_BYTES: u64 = 16 * 1024;
const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    /// A client connects at once (remote use while nobody is at the laptop).
    #[default]
    Never,
    /// Accept or Deny on the laptop; no answer in 30 seconds is a Deny.
    Ask,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Indicator {
    /// A notification when a session starts and ends.
    pub notify: bool,
    /// The running time next to the icon.
    pub show_timer: bool,
}

impl Default for Indicator {
    fn default() -> Self {
        Self {
            notify: true,
            show_timer: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HostConfig {
    pub version: u32,
    pub allow_private: bool,
    pub allow_shared: bool,
    /// `Some(true)` always locks when a session ends, `Some(false)` never does, `None` lets the client choose.
    pub force_lock_on_stop: Option<bool>,
    /// Longest session in hours; 0 = the client may choose any (or none).
    pub max_session_hours: u32,
    /// Longest idle time in minutes before a session ends; 0 = the client may choose.
    pub max_idle_minutes: u32,
    /// Frame-rate ceiling; 0 = none beyond the quality level.
    pub max_fps: u32,
    /// Bitrate ceiling in kbit/s; 0 = none beyond the quality level.
    pub max_bitrate_kbps: u32,
    pub allow_audio: bool,
    /// `None` keeps the command-line setting (`--clipboard`).
    pub allow_clipboard: Option<bool>,
    pub allow_text: bool,
    /// PipeWire output node whose sound is sent; `None` keeps `--audio-sink` or the default output.
    pub audio_sink: Option<String>,
    pub approval: Approval,
    pub indicator: Indicator,
    /// Plain-http address; `None` keeps `--listen`. Needs a restart.
    pub http_listen: Option<String>,
    /// https address; `Some("")` turns https off, `None` keeps `--tls-listen`. Needs a restart.
    pub tls_listen: Option<String>,
    /// A certificate from a real authority and its key (both or neither); `None` keeps the flags.
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    /// Internet mode (`--public`): refuses to start without hostd login and a real certificate.
    pub public: Option<bool>,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            version: VERSION,
            allow_private: true,
            allow_shared: true,
            force_lock_on_stop: None,
            max_session_hours: 0,
            max_idle_minutes: 0,
            max_fps: 0,
            max_bitrate_kbps: 0,
            allow_audio: true,
            allow_clipboard: None,
            allow_text: true,
            audio_sink: None,
            approval: Approval::Never,
            indicator: Indicator::default(),
            http_listen: None,
            tls_listen: None,
            tls_cert: None,
            tls_key: None,
            public: None,
        }
    }
}

/// The part of the owner's settings a client may see, so its page can show what is not available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Policy {
    pub allow_private: bool,
    pub allow_shared: bool,
    pub force_lock_on_stop: Option<bool>,
    pub max_session_hours: u32,
    pub max_idle_minutes: u32,
    pub max_fps: u32,
    pub max_bitrate_kbps: u32,
    pub allow_audio: bool,
    pub allow_text: bool,
    pub approval: Approval,
}

fn valid_listen(text: &str) -> bool {
    text.parse::<SocketAddr>().is_ok()
}

impl HostConfig {
    pub fn validated(self) -> Result<Self, String> {
        if self.version != VERSION {
            return Err(format!("version must be {VERSION}"));
        }
        if !self.allow_private && !self.allow_shared {
            return Err("at least one mode (private or shared) must be allowed".into());
        }
        if self.max_session_hours > MAX_SESSION_HOURS {
            return Err(format!(
                "max_session_hours must be at most {MAX_SESSION_HOURS}"
            ));
        }
        if self.max_idle_minutes > MAX_IDLE_MINUTES {
            return Err(format!(
                "max_idle_minutes must be at most {MAX_IDLE_MINUTES}"
            ));
        }
        if self.max_fps != 0 && !(MIN_FPS..=MAX_FPS).contains(&self.max_fps) {
            return Err(format!("max_fps must be 0 or {MIN_FPS} to {MAX_FPS}"));
        }
        if self.max_bitrate_kbps != 0
            && !(MIN_BITRATE_KBPS..=MAX_BITRATE_KBPS).contains(&self.max_bitrate_kbps)
        {
            return Err(format!(
                "max_bitrate_kbps must be 0 or {MIN_BITRATE_KBPS} to {MAX_BITRATE_KBPS}"
            ));
        }
        if let Some(sink) = &self.audio_sink
            && (sink.len() > 200 || sink.chars().any(char::is_control))
        {
            return Err("audio_sink is not a sound output name".into());
        }
        for (name, value) in [
            ("http_listen", &self.http_listen),
            ("tls_listen", &self.tls_listen),
        ] {
            if let Some(text) = value
                && !(text.is_empty() && name == "tls_listen")
                && !valid_listen(text)
            {
                return Err(format!("{name} must look like 127.0.0.1:8080"));
            }
        }
        if self.tls_cert.is_some() != self.tls_key.is_some() {
            return Err("tls_cert and tls_key go together".into());
        }
        for path in [&self.tls_cert, &self.tls_key].into_iter().flatten() {
            if !path.is_absolute() {
                return Err("certificate and key paths must be absolute".into());
            }
        }
        Ok(self)
    }

    /// Enforces the limits on what a client asked for. A mode that is not allowed is refused (the page does not offer it);
    /// numbers are clamped and sound is dropped when not allowed.
    pub fn apply(
        &self,
        mut options: crate::options::SessionOptions,
    ) -> Result<crate::options::SessionOptions, String> {
        let shared_like = !options.blank_panel && !options.block_local_input;
        if self.allow_private != self.allow_shared {
            if self.allow_private && !(options.blank_panel && options.block_local_input) {
                return Err("the laptop owner allows only Private sessions (blank screen, laptop input blocked)".into());
            }
            if self.allow_shared && !shared_like {
                return Err(
                    "the laptop owner allows only Shared sessions (screen and input left alone)"
                        .into(),
                );
            }
        }
        if let Some(lock) = self.force_lock_on_stop {
            options.lock_on_stop = lock;
        }
        let clamp = |value: u32, limit: u32| {
            if limit != 0 && (value == 0 || value > limit) {
                limit
            } else {
                value
            }
        };
        options.max_hours = clamp(options.max_hours, self.max_session_hours);
        options.idle_minutes = clamp(options.idle_minutes, self.max_idle_minutes);
        if self.max_fps != 0 && options.fps_cap > self.max_fps {
            options.fps_cap = self.max_fps;
        }
        if self.max_bitrate_kbps != 0 && options.bitrate_kbps > self.max_bitrate_kbps {
            options.bitrate_kbps = self.max_bitrate_kbps;
        }
        if !self.allow_audio {
            options.audio = false;
        }
        options.validated()
    }

    pub fn policy(&self) -> Policy {
        Policy {
            allow_private: self.allow_private,
            allow_shared: self.allow_shared,
            force_lock_on_stop: self.force_lock_on_stop,
            max_session_hours: self.max_session_hours,
            max_idle_minutes: self.max_idle_minutes,
            max_fps: self.max_fps,
            max_bitrate_kbps: self.max_bitrate_kbps,
            allow_audio: self.allow_audio,
            allow_text: self.allow_text,
            approval: self.approval,
        }
    }

    pub fn load(dir: &Path) -> (Self, Option<String>) {
        let path = dir.join(HOST_FILE);
        let text = match std::fs::metadata(&path) {
            Err(_) => return (Self::default(), None),
            Ok(meta) if meta.len() > MAX_BYTES => {
                return (
                    Self::default(),
                    Some("the host settings file is too large: defaults used".into()),
                );
            }
            Ok(_) => std::fs::read_to_string(&path),
        };
        match text
            .map_err(|error| error.to_string())
            .and_then(|text| serde_json::from_str::<Self>(&text).map_err(|error| error.to_string()))
            .and_then(Self::validated)
        {
            Ok(config) => (config, None),
            Err(error) => (
                Self::default(),
                Some(format!(
                    "the host settings file was not used ({error}): defaults in effect"
                )),
            ),
        }
    }

    /// Owner-only and atomic: a crash leaves the old file or the new one.
    pub fn save(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let temp = dir.join(format!(".{HOST_FILE}.tmp"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
        file.sync_all()?;
        let path = dir.join(HOST_FILE);
        std::fs::rename(&temp, &path)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::SessionOptions;

    #[test]
    fn the_default_limits_nothing() {
        let config = HostConfig::default();
        let private = SessionOptions::default();
        assert_eq!(config.apply(private.clone()).unwrap(), private);
        let shared = SessionOptions::shared();
        assert_eq!(config.apply(shared.clone()).unwrap(), shared);
    }

    #[test]
    fn a_mode_that_is_not_allowed_is_refused() {
        let private_only = HostConfig {
            allow_shared: false,
            ..HostConfig::default()
        };
        assert!(private_only.apply(SessionOptions::default()).is_ok());
        assert!(private_only.apply(SessionOptions::shared()).is_err());
        let half = SessionOptions {
            block_local_input: false,
            ..SessionOptions::default()
        };
        assert!(
            private_only.apply(half.clone()).is_err(),
            "a mixed session is not Private"
        );
        let shared_only = HostConfig {
            allow_private: false,
            ..HostConfig::default()
        };
        assert!(shared_only.apply(SessionOptions::shared()).is_ok());
        assert!(shared_only.apply(SessionOptions::default()).is_err());
        assert!(shared_only.apply(half).is_err());
    }

    #[test]
    fn limits_are_clamped_and_the_lock_can_be_forced() {
        let config = HostConfig {
            force_lock_on_stop: Some(true),
            max_session_hours: 2,
            max_idle_minutes: 15,
            max_fps: 30,
            max_bitrate_kbps: 5000,
            allow_audio: false,
            ..HostConfig::default()
        };
        let asked = SessionOptions {
            lock_on_stop: false,
            max_hours: 0,
            idle_minutes: 60,
            fps_cap: 60,
            bitrate_kbps: 20_000,
            audio: true,
            ..SessionOptions::shared()
        };
        let got = config.apply(asked).unwrap();
        assert!(got.lock_on_stop && !got.audio);
        assert_eq!((got.max_hours, got.idle_minutes), (2, 15));
        assert_eq!((got.fps_cap, got.bitrate_kbps), (30, 5000));
        let tighter = SessionOptions {
            max_hours: 1,
            idle_minutes: 5,
            fps_cap: 15,
            bitrate_kbps: 1000,
            ..SessionOptions::default()
        };
        let got = config.apply(tighter).unwrap();
        assert_eq!(
            (
                got.max_hours,
                got.idle_minutes,
                got.fps_cap,
                got.bitrate_kbps
            ),
            (1, 5, 15, 1000)
        );
        let never = HostConfig {
            force_lock_on_stop: Some(false),
            ..HostConfig::default()
        };
        assert!(!never.apply(SessionOptions::default()).unwrap().lock_on_stop);
    }

    #[test]
    fn validation_refuses_nonsense() {
        let bad = |config: HostConfig| config.validated().is_err();
        assert!(bad(HostConfig {
            allow_private: false,
            allow_shared: false,
            ..HostConfig::default()
        }));
        assert!(bad(HostConfig {
            max_fps: 1,
            ..HostConfig::default()
        }));
        assert!(bad(HostConfig {
            max_bitrate_kbps: 5,
            ..HostConfig::default()
        }));
        assert!(bad(HostConfig {
            max_session_hours: 1000,
            ..HostConfig::default()
        }));
        assert!(bad(HostConfig {
            http_listen: Some("localhost".into()),
            ..HostConfig::default()
        }));
        assert!(bad(HostConfig {
            tls_listen: Some("8443".into()),
            ..HostConfig::default()
        }));
        assert!(bad(HostConfig {
            tls_cert: Some("/a".into()),
            ..HostConfig::default()
        }));
        assert!(bad(HostConfig {
            tls_cert: Some("a".into()),
            tls_key: Some("b".into()),
            ..HostConfig::default()
        }));
        assert!(bad(HostConfig {
            audio_sink: Some("a\nb".into()),
            ..HostConfig::default()
        }));
        assert!(
            HostConfig {
                tls_listen: Some(String::new()),
                http_listen: Some("127.0.0.1:8080".into()),
                ..HostConfig::default()
            }
            .validated()
            .is_ok()
        );
        assert!(serde_json::from_str::<HostConfig>(r#"{"unknown":1}"#).is_err());
        assert!(serde_json::from_str::<HostConfig>(r#"{"max_fps":-1}"#).is_err());
    }

    #[test]
    fn saved_settings_round_trip_privately_and_bad_files_fall_back() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (fresh, note) = HostConfig::load(dir.path());
        assert_eq!((fresh, note), (HostConfig::default(), None));
        let config = HostConfig {
            approval: Approval::Ask,
            max_fps: 30,
            ..HostConfig::default()
        };
        let path = config.save(dir.path()).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(HostConfig::load(dir.path()), (config, None));
        std::fs::write(dir.path().join(HOST_FILE), "{ nope").unwrap();
        let (fallback, note) = HostConfig::load(dir.path());
        assert_eq!(fallback, HostConfig::default());
        assert!(note.unwrap().contains("defaults in effect"));
    }
}
