use std::path::Path;

use anyhow::Result;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::runtime::resources::{GameConfigResource, PersistenceRoot};
use crate::ui::control_bar::{SkipMode, ToggleStates};

const SETTINGS_VERSION: u32 = 4;
const MAX_SETTINGS_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiLocale {
    #[default]
    ZhCn,
    ZhTw,
    En,
    Ja,
}

impl UiLocale {
    pub const ALL: [Self; 4] = [Self::ZhCn, Self::ZhTw, Self::En, Self::Ja];

    pub const fn native_name(self) -> &'static str {
        match self {
            Self::ZhCn => "简体中文",
            Self::ZhTw => "繁體中文",
            Self::En => "English",
            Self::Ja => "日本語",
        }
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSettings {
    pub locale: UiLocale,
    pub master_volume: f32,
    pub vocal_volume: f32,
    pub bgm_volume: f32,
    pub se_volume: f32,
    pub ui_se_volume: f32,
    pub typewriter_speed: f64,
    pub auto_delay: f64,
    pub text_size: u8,
    pub textbox_opacity: f32,
    pub fullscreen: bool,
    pub skip_all: bool,
}

#[derive(Serialize, Deserialize)]
struct SettingsFile {
    version: u32,
    settings: RuntimeSettings,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            locale: UiLocale::ZhCn,
            master_volume: 1.0,
            vocal_volume: 1.0,
            bgm_volume: 1.0,
            se_volume: 1.0,
            ui_se_volume: 1.0,
            typewriter_speed: 45.0,
            auto_delay: 2.0,
            text_size: 1,
            textbox_opacity: 0.75,
            fullscreen: false,
            skip_all: false,
        }
    }
}

pub fn load_settings(
    project_root: Res<PersistenceRoot>,
    config: Res<GameConfigResource>,
    mut settings: ResMut<RuntimeSettings>,
    mut toggles: ResMut<ToggleStates>,
) {
    *settings = load(&project_root).unwrap_or_else(|| RuntimeSettings {
        typewriter_speed: config.styles.typewriter_speed,
        auto_delay: config.styles.auto_delay,
        ..default()
    });
    sanitize(&mut settings);
    toggles.skip_mode = if settings.skip_all {
        SkipMode::All
    } else {
        SkipMode::Read
    };
}

pub fn persist(settings: &RuntimeSettings, project_root: &Path) -> Result<()> {
    let path = path(project_root);
    let file = SettingsFile {
        version: SETTINGS_VERSION,
        settings: settings.clone(),
    };
    let bytes = super::encode_postcard_limited(&file, MAX_SETTINGS_BYTES, "settings")?;
    super::recovery::write_preserving(&path, &bytes, MAX_SETTINGS_BYTES, decode)
}

pub(crate) fn load(project_root: &Path) -> Option<RuntimeSettings> {
    super::recovery::read_optional(&path(project_root), MAX_SETTINGS_BYTES, decode)
        .ok()
        .flatten()
}

fn decode(bytes: &[u8]) -> Result<RuntimeSettings> {
    let file: SettingsFile = super::decode_postcard_exact(bytes)?;
    anyhow::ensure!(
        file.version == SETTINGS_VERSION,
        "unsupported settings version"
    );
    Ok(file.settings)
}

pub(super) fn check(root: &Path) -> Result<()> {
    super::recovery::read_optional(&path(root), MAX_SETTINGS_BYTES, decode).map(|_| ())
}

pub(crate) fn sanitize(settings: &mut RuntimeSettings) {
    let defaults = RuntimeSettings::default();
    macro_rules! clamp {
        ($field:ident, $min:expr, $max:expr) => {
            settings.$field = if settings.$field.is_finite() {
                settings.$field.clamp($min, $max)
            } else {
                defaults.$field
            };
        };
    }
    clamp!(master_volume, 0.0, 1.0);
    clamp!(vocal_volume, 0.0, 1.0);
    clamp!(bgm_volume, 0.0, 1.0);
    clamp!(se_volume, 0.0, 1.0);
    clamp!(ui_se_volume, 0.0, 1.0);
    clamp!(typewriter_speed, 10.0, 120.0);
    clamp!(auto_delay, 0.5, 5.0);
    settings.text_size = settings.text_size.min(2);
    clamp!(textbox_opacity, 0.0, 1.0);
}

fn path(project_root: &Path) -> std::path::PathBuf {
    project_root.join("saves").join("settings.bin")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn non_finite_settings_use_defaults_and_finite_values_are_clamped() {
        let mut settings = RuntimeSettings {
            master_volume: f32::NAN,
            vocal_volume: f32::INFINITY,
            bgm_volume: f32::NEG_INFINITY,
            se_volume: -1.0,
            ui_se_volume: 2.0,
            typewriter_speed: f64::NAN,
            auto_delay: f64::INFINITY,
            textbox_opacity: f32::NAN,
            text_size: 255,
            ..Default::default()
        };
        sanitize(&mut settings);
        assert_eq!(
            settings,
            RuntimeSettings {
                se_volume: 0.0,
                text_size: 2,
                ..Default::default()
            }
        );
    }

    #[test]
    fn settings_round_trip_and_legacy_recovery_preserve_values() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-settings-{nonce}"));
        let expected = RuntimeSettings {
            locale: UiLocale::Ja,
            master_volume: 0.4,
            vocal_volume: 0.6,
            bgm_volume: 0.7,
            se_volume: 0.8,
            ui_se_volume: 0.3,
            typewriter_speed: 72.0,
            auto_delay: 1.5,
            text_size: 2,
            textbox_opacity: 0.55,
            fullscreen: true,
            skip_all: true,
        };

        persist(&expected, &root).unwrap();
        assert_eq!(load(&root).unwrap(), expected);

        // Replace the saved file with the old unversioned format, then recover it.
        fs::write(
            path(&root),
            postcard::to_stdvec(&RuntimeSettings::default()).unwrap(),
        )
        .unwrap();

        assert!(load(&root).is_none());
        let original = fs::read(path(&root)).unwrap();
        persist(&RuntimeSettings::default(), &root).unwrap();
        assert_eq!(
            fs::read(root.join("saves/settings.bin.recovery-1")).unwrap(),
            original
        );
        assert_eq!(load(&root).unwrap(), RuntimeSettings::default());
        let _ = fs::remove_dir_all(root);
    }
}
