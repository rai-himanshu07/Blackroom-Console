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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginMethod {
    /// Linux password + authenticator code + Remote Access Key (or a trusted browser), through remote-hostd.
    Hostd,
    /// The one-time address with its token.
    Token,
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
    /// How clients sign in; `None` keeps the command-line choice. Needs a restart.
    pub login: Option<LoginMethod>,
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
    /// STUN servers; `None` keeps `--stun`, an empty list clears them.
    pub stun: Option<Vec<String>>,
    /// TURN relays; `None` keeps `--turn`, an empty list clears them. Needs `turn_secret_file`.
    pub turn: Option<Vec<String>>,
    /// coturn `static-auth-secret` (owner-only file); the secret itself is never stored here.
    pub turn_secret_file: Option<PathBuf>,
    pub turn_ttl_secs: Option<u64>,
    /// UDP range for media, `MIN-MAX`, so it can be forwarded on a router.
    pub ice_ports: Option<String>,
    /// The DNS name or IP address clients type, once the owner has set up access from outside.
    pub public_name: Option<String>,
    /// Which certificate backs `public`: `ca` (files from a real authority) or `self_signed` (the console's own, for a bare IP).
    pub public_cert: Option<PublicCert>,
    /// What the host settings page remembers for the access modes that are not in use, so switching back is one click.
    /// The running settings are the fields above; this is only a memory.
    pub saved_access: Option<SavedAccess>,
}

/// One access mode's remembered settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AccessProfile {
    pub public_name: Option<String>,
    pub public_cert: Option<PublicCert>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub ice_ports: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SavedAccess {
    /// A private network or VPN (Tailscale): nothing exposed, a certificate for the VPN name.
    pub vpn: Option<AccessProfile>,
    /// Direct access through the router.
    pub direct: Option<AccessProfile>,
}

impl AccessProfile {
    fn validated(&self) -> Result<(), String> {
        if let Some(name) = &self.public_name
            && !valid_public_name(name)
        {
            return Err("a remembered name must be a DNS name or an IP address".into());
        }
        if self.tls_cert.is_some() != self.tls_key.is_some() {
            return Err("a remembered certificate and key go together".into());
        }
        for path in [&self.tls_cert, &self.tls_key].into_iter().flatten() {
            if !path.is_absolute() {
                return Err("remembered certificate and key paths must be absolute".into());
            }
        }
        if let Some(text) = &self.ice_ports
            && crate::ice::parse_port_range(text).is_none()
        {
            return Err("a remembered media port range must be MIN-MAX".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicCert {
    Ca,
    SelfSigned,
}

/// What `HostConfig::load_checked` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub config: HostConfig,
    pub note: Option<String>,
    /// A file exists but could not be used.
    pub damaged: bool,
}

/// A DNS name or an IP address, nothing else (no scheme, port, path or userinfo).
pub fn valid_public_name(text: &str) -> bool {
    if text.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    !text.is_empty()
        && text.len() <= 253
        && text.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
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
            login: None,
            indicator: Indicator::default(),
            http_listen: None,
            tls_listen: None,
            tls_cert: None,
            tls_key: None,
            public: None,
            stun: None,
            turn: None,
            turn_secret_file: None,
            turn_ttl_secs: None,
            ice_ports: None,
            public_name: None,
            public_cert: None,
            saved_access: None,
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
    /// Do the two differ in anything a running console acts on? The remembered access modes are only a memory.
    pub fn needs_restart_from(&self, running: &Self) -> bool {
        let strip = |config: &Self| Self {
            saved_access: None,
            ..config.clone()
        };
        strip(self) != strip(running)
    }

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
        for (name, list, valid) in [
            (
                "stun",
                &self.stun,
                crate::ice::valid_stun as fn(&str) -> bool,
            ),
            (
                "turn",
                &self.turn,
                crate::ice::valid_turn as fn(&str) -> bool,
            ),
        ] {
            if let Some(list) = list {
                if list.len() > 8 {
                    return Err(format!("{name} may list at most 8 servers"));
                }
                if let Some(bad) = list.iter().find(|url| !valid(url)) {
                    return Err(format!(
                        "{name} entry {bad:?} is not a valid server address"
                    ));
                }
            }
        }
        if let Some(path) = &self.turn_secret_file
            && (!path.is_absolute() || path.to_string_lossy().chars().any(char::is_control))
        {
            return Err("turn_secret_file must be an absolute path".into());
        }
        if self.turn.as_ref().is_some_and(|turn| !turn.is_empty())
            && self.turn_secret_file.is_none()
        {
            return Err("turn needs turn_secret_file".into());
        }
        if let Some(ttl) = self.turn_ttl_secs
            && !(60..=86_400).contains(&ttl)
        {
            return Err("turn_ttl_secs must be 60 to 86400".into());
        }
        if let Some(text) = &self.ice_ports
            && crate::ice::parse_port_range(text).is_none()
        {
            return Err("ice_ports must be MIN-MAX, both between 1025 and 65535".into());
        }
        if let Some(name) = &self.public_name
            && !valid_public_name(name)
        {
            return Err("public_name must be a DNS name or an IP address".into());
        }
        if self.public_cert == Some(PublicCert::SelfSigned) && self.public_name.is_none() {
            return Err("public_cert self_signed needs public_name (the address the certificate is made for)".into());
        }
        if let Some(saved) = &self.saved_access {
            for profile in [&saved.vpn, &saved.direct].into_iter().flatten() {
                profile.validated()?;
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

    /// A missing file means the defaults. A file that exists but cannot be used also gives the defaults, except that
    /// every connection waits for the owner (Ask): a damaged policy must not silently open the laptop.
    pub fn load(dir: &Path) -> (Self, Option<String>) {
        let loaded = Self::load_checked(dir);
        (loaded.config, loaded.note)
    }

    /// Like `load`, and says whether a file was there but unusable: the console then keeps its network
    /// listeners on this laptop only, because the saved public-mode settings are unknown.
    pub fn load_checked(dir: &Path) -> Loaded {
        let damaged = |note: String| Loaded {
            config: Self {
                approval: Approval::Ask,
                ..Self::default()
            },
            note: Some(note),
            damaged: true,
        };
        let path = dir.join(HOST_FILE);
        let text = match std::fs::metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Loaded {
                    config: Self::default(),
                    note: None,
                    damaged: false,
                };
            }
            Err(error) => {
                return damaged(format!(
                    "the host settings file cannot be read ({error}): defaults in effect, every connection asks the owner; network listeners stay on this laptop until it is repaired"
                ));
            }
            Ok(meta) if meta.len() > MAX_BYTES => {
                return damaged("the host settings file is too large: defaults used, every connection asks the owner; network listeners stay on this laptop until it is repaired".into());
            }
            Ok(_) => std::fs::read_to_string(&path),
        };
        match text
            .map_err(|error| error.to_string())
            .and_then(|text| serde_json::from_str::<Self>(&text).map_err(|error| error.to_string()))
            .and_then(Self::validated)
        {
            Ok(config) => Loaded {
                config,
                note: None,
                damaged: false,
            },
            Err(error) => damaged(format!(
                "the host settings file was not used ({error}): defaults in effect, every connection asks the owner; network listeners stay on this laptop until it is repaired"
            )),
        }
    }

    /// Owner-only and atomic: a crash leaves the old file or the new one.
    pub fn save(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        use std::sync::atomic::{AtomicU64, Ordering};
        static SAVES: AtomicU64 = AtomicU64::new(0);
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        // One temp file per save: two requests at once never write into the same file.
        let temp = dir.join(format!(
            ".{HOST_FILE}.{}.{}.tmp",
            std::process::id(),
            SAVES.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
        file.sync_all()?;
        let path = dir.join(HOST_FILE);
        if let Err(error) = std::fs::rename(&temp, &path) {
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::SessionOptions;

    #[test]
    fn remembered_access_modes_are_validated_and_do_not_need_a_restart() {
        let remembered = |profile: AccessProfile| HostConfig {
            saved_access: Some(SavedAccess {
                vpn: Some(profile),
                direct: None,
            }),
            ..HostConfig::default()
        };
        assert!(
            remembered(AccessProfile {
                public_name: Some("laptop.tailnet.ts.net".into()),
                tls_cert: Some("/c.pem".into()),
                tls_key: Some("/k.pem".into()),
                ..AccessProfile::default()
            })
            .validated()
            .is_ok()
        );
        for bad in [
            AccessProfile {
                public_name: Some("not a name!".into()),
                ..AccessProfile::default()
            },
            AccessProfile {
                tls_cert: Some("/c.pem".into()),
                ..AccessProfile::default()
            },
            AccessProfile {
                tls_cert: Some("c.pem".into()),
                tls_key: Some("/k.pem".into()),
                ..AccessProfile::default()
            },
            AccessProfile {
                ice_ports: Some("99-100".into()),
                ..AccessProfile::default()
            },
        ] {
            assert!(remembered(bad.clone()).validated().is_err(), "{bad:?}");
        }
        let running = HostConfig::default();
        let memory_only = remembered(AccessProfile::default());
        assert!(!memory_only.needs_restart_from(&running));
        let live_change = HostConfig {
            public: Some(true),
            ..HostConfig::default()
        };
        assert!(live_change.needs_restart_from(&running));
    }

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
        assert!(serde_json::from_str::<HostConfig>(r#"{"login":"password-only"}"#).is_err());
        assert_eq!(
            serde_json::from_str::<HostConfig>(r#"{"login":"hostd"}"#)
                .unwrap()
                .login,
            Some(LoginMethod::Hostd)
        );
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
        assert_eq!(
            fallback,
            HostConfig {
                approval: Approval::Ask,
                ..HostConfig::default()
            },
            "a damaged policy must not open the laptop"
        );
        assert!(note.unwrap().contains("defaults in effect"));
        std::fs::write(dir.path().join(HOST_FILE), vec![b' '; 20_000]).unwrap();
        let (oversized, note) = HostConfig::load(dir.path());
        assert_eq!(oversized.approval, Approval::Ask);
        assert!(note.unwrap().contains("too large"));
    }

    #[test]
    fn concurrent_saves_each_use_their_own_temp_file_and_leave_a_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            for fps in [10, 20, 30, 40] {
                let path = dir.path();
                scope.spawn(move || {
                    for _ in 0..25 {
                        HostConfig {
                            max_fps: fps,
                            ..HostConfig::default()
                        }
                        .save(path)
                        .unwrap();
                    }
                });
            }
        });
        let (loaded, note) = HostConfig::load(dir.path());
        assert_eq!(note, None);
        assert!([10, 20, 30, 40].contains(&loaded.max_fps));
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn internet_settings_are_validated_and_a_damaged_file_is_reported() {
        let ok = |json: &str| {
            serde_json::from_str::<HostConfig>(json)
                .unwrap()
                .validated()
        };
        assert!(ok(r#"{"stun":["stun:stun.example.org:3478"],"ice_ports":"50000-50100","public_name":"home.example.org"}"#).is_ok());
        assert!(ok(r#"{"public_name":"203.0.113.7","public_cert":"self_signed"}"#).is_ok());
        for bad in [
            r#"{"stun":["http://evil"]}"#,
            r#"{"turn":["turn:t.example.org:3478"]}"#,
            r#"{"turn_secret_file":"relative/secret"}"#,
            r#"{"turn_ttl_secs":5}"#,
            r#"{"ice_ports":"80-90"}"#,
            r#"{"public_name":"https://home.example.org/"}"#,
            r#"{"public_name":"user@home.example.org"}"#,
            r#"{"public_name":"home.example.org:8443"}"#,
            r#"{"public_name":"bad name"}"#,
            r#"{"public_cert":"self_signed"}"#,
        ] {
            assert!(ok(bad).is_err(), "{bad}");
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(
            !HostConfig::load_checked(dir.path()).damaged,
            "no file is not damage"
        );
        std::fs::write(dir.path().join(HOST_FILE), "{ nope").unwrap();
        let loaded = HostConfig::load_checked(dir.path());
        assert!(loaded.damaged);
        assert!(
            loaded
                .note
                .unwrap()
                .contains("network listeners stay on this laptop")
        );
    }
}
