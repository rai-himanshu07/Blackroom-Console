//! The owner's settings, kept on the laptop so every browser starts the same way. `session` is enforced here (what a
//! session does to the laptop); `client` is stored and range-checked here and applied by the page.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::console::Quality;
use crate::options::SessionOptions;

pub const PROFILE_FILE: &str = "profile.json";
const MAX_PROFILE_BYTES: u64 = 16 * 1024;
const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    /// The whole desktop, letterboxed.
    #[default]
    Fit,
    /// Fill the window, ignoring the aspect ratio.
    Stretch,
    /// One remote pixel per page pixel, scrolling to follow the cursor.
    Actual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextMode {
    /// Soft-keyboard text becomes key presses of a US layout.
    #[default]
    Keys,
    /// Soft-keyboard text is sent as characters, whatever the laptop's layout.
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TouchMode {
    #[default]
    Trackpad,
    Touch,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClientSettings {
    pub quality: Quality,
    pub scale: Scale,
    /// Show this device's own pointer over the picture (desktop browsers).
    pub show_local_cursor: bool,
    /// 0 to 100, applied to the laptop's sound in the page.
    pub volume: u8,
    pub text_mode: TextMode,
    /// Command (Meta) acts as Control: for Mac and iPad keyboards.
    pub mac_keys: bool,
    pub touch_mode: TouchMode,
    /// Size the blanked screen to this device's window at connect (the page computes it).
    pub fit_resolution: bool,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            quality: Quality::Medium,
            scale: Scale::Fit,
            show_local_cursor: false,
            volume: 80,
            text_mode: TextMode::Keys,
            mac_keys: false,
            touch_mode: TouchMode::Trackpad,
            fit_resolution: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub version: u32,
    pub session: SessionOptions,
    pub client: ClientSettings,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            version: VERSION,
            session: SessionOptions::default(),
            client: ClientSettings::default(),
        }
    }
}

impl Profile {
    pub fn validated(mut self) -> Result<Self, String> {
        if self.version != VERSION {
            return Err(format!("unsupported settings version {}", self.version));
        }
        if self.client.volume > 100 {
            return Err("volume must be 0 to 100".into());
        }
        self.session = self.session.validated()?;
        Ok(self)
    }

    /// The saved profile, or the defaults when there is none or it is unreadable (never an error:
    /// a damaged settings file must not stop the console).
    pub fn load(dir: &Path) -> (Self, Option<String>) {
        let path = dir.join(PROFILE_FILE);
        let text = match std::fs::metadata(&path) {
            Err(_) => return (Self::default(), None),
            Ok(meta) if meta.len() > MAX_PROFILE_BYTES => {
                return (
                    Self::default(),
                    Some("the settings file is too large: defaults used".into()),
                );
            }
            Ok(_) => std::fs::read_to_string(&path),
        };
        match text
            .map_err(|error| error.to_string())
            .and_then(|text| serde_json::from_str::<Self>(&text).map_err(|error| error.to_string()))
            .and_then(Self::validated)
        {
            Ok(profile) => (profile, None),
            Err(error) => (
                Self::default(),
                Some(format!(
                    "the settings file was not used ({error}): defaults in effect"
                )),
            ),
        }
    }

    /// Writes the profile owner-only and atomically, so a crash leaves the old file or the new one.
    pub fn save(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let temp = dir.join(format!(".{PROFILE_FILE}.tmp"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.sync_all()?;
        let path = dir.join(PROFILE_FILE);
        std::fs::rename(&temp, &path)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn defaults_round_trip_and_unknown_keys_are_refused() {
        let json = serde_json::to_string(&Profile::default()).unwrap();
        assert_eq!(
            serde_json::from_str::<Profile>(&json).unwrap(),
            Profile::default()
        );
        assert_eq!(
            serde_json::from_str::<Profile>("{}").unwrap(),
            Profile::default()
        );
        assert!(serde_json::from_str::<Profile>(r#"{"client":{"colour":"red"}}"#).is_err());
        assert!(serde_json::from_str::<Profile>(r#"{"client":{"scale":"huge"}}"#).is_err());
    }

    #[test]
    fn ranges_and_version_are_checked() {
        let loud = Profile {
            client: ClientSettings {
                volume: 101,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(loud.validated().is_err());
        let old = Profile {
            version: 0,
            ..Default::default()
        };
        assert!(old.validated().is_err());
        let odd = Profile {
            session: SessionOptions {
                idle_minutes: u32::MAX,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(odd.validated().is_err());
    }

    #[test]
    fn saving_is_private_and_loading_survives_damage() {
        let dir = tempfile::tempdir().unwrap().keep().join("profile-dir");
        let (fresh, note) = Profile::load(&dir);
        assert_eq!((fresh, note), (Profile::default(), None));

        let mut profile = Profile {
            session: SessionOptions::shared(),
            ..Profile::default()
        };
        profile.client.scale = Scale::Actual;
        let path = profile.save(&dir).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(!dir.join(format!(".{PROFILE_FILE}.tmp")).exists());
        assert_eq!(Profile::load(&dir), (profile, None));

        std::fs::write(&path, "{ not json").unwrap();
        let (fallback, note) = Profile::load(&dir);
        assert_eq!(fallback, Profile::default());
        assert!(note.unwrap().contains("defaults in effect"));
        std::fs::write(&path, vec![b' '; 20_000]).unwrap();
        assert!(Profile::load(&dir).1.unwrap().contains("too large"));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
