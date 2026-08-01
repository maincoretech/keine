use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::runtime::resources::{GameConfigResource, ProjectRoot};
use crate::ui::control_bar::{SkipMode, ToggleStates};

const SETTINGS_VERSION: u32 = 4;

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
    project_root: Res<ProjectRoot>,
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
    let temporary = path.with_extension("bin.tmp");
    let parent = path.parent().context("settings path has no parent")?;
    fs::create_dir_all(parent)?;
    let file = SettingsFile {
        version: SETTINGS_VERSION,
        settings: settings.clone(),
    };
    fs::write(&temporary, postcard::to_stdvec(&file)?)?;
    fs::rename(&temporary, &path)?;
    Ok(())
}

pub(crate) fn load(project_root: &Path) -> Option<RuntimeSettings> {
    let bytes = fs::read(path(project_root)).ok()?;
    let file: SettingsFile = postcard::from_bytes(&bytes).ok()?;
    (file.version == SETTINGS_VERSION).then_some(file.settings)
}

pub(crate) fn sanitize(settings: &mut RuntimeSettings) {
    settings.master_volume = settings.master_volume.clamp(0.0, 1.0);
    settings.vocal_volume = settings.vocal_volume.clamp(0.0, 1.0);
    settings.bgm_volume = settings.bgm_volume.clamp(0.0, 1.0);
    settings.se_volume = settings.se_volume.clamp(0.0, 1.0);
    settings.ui_se_volume = settings.ui_se_volume.clamp(0.0, 1.0);
    settings.typewriter_speed = settings.typewriter_speed.clamp(10.0, 120.0);
    settings.auto_delay = settings.auto_delay.clamp(0.5, 5.0);
    settings.text_size = settings.text_size.min(2);
    settings.textbox_opacity = settings.textbox_opacity.clamp(0.0, 1.0);
}

pub(super) fn reset_memory(settings: &mut RuntimeSettings) {
    *settings = RuntimeSettings::default();
}

fn path(project_root: &Path) -> std::path::PathBuf {
    project_root.join("saves").join("settings.bin")
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn persists_runtime_settings() {
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
        let actual = load(&root).unwrap();
        assert_eq!(actual.master_volume, expected.master_volume);
        assert_eq!(actual.locale, expected.locale);
        assert_eq!(actual.vocal_volume, expected.vocal_volume);
        assert_eq!(actual.bgm_volume, expected.bgm_volume);
        assert_eq!(actual.se_volume, expected.se_volume);
        assert_eq!(actual.ui_se_volume, expected.ui_se_volume);
        assert_eq!(actual.typewriter_speed, expected.typewriter_speed);
        assert_eq!(actual.auto_delay, expected.auto_delay);
        assert_eq!(actual.text_size, expected.text_size);
        assert_eq!(actual.textbox_opacity, expected.textbox_opacity);
        assert_eq!(actual.fullscreen, expected.fullscreen);
        assert_eq!(actual.skip_all, expected.skip_all);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unversioned_settings() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-legacy-settings-{nonce}"));
        fs::create_dir_all(root.join("saves")).unwrap();
        fs::write(
            path(&root),
            postcard::to_stdvec(&RuntimeSettings::default()).unwrap(),
        )
        .unwrap();

        assert!(load(&root).is_none());
        let _ = fs::remove_dir_all(root);
    }
}
