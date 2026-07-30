// Game configuration, loaded from config.yaml.
// Inspired by Raven's MainConfig pattern.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::{collections::HashMap, fs};

/// Top-level game configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    /// Game title displayed in window title bar.
    #[serde(default = "default_title")]
    pub title: String,

    /// Background asset alias or path used by the dedicated title screen.
    #[serde(default = "default_title_background")]
    pub title_background: String,

    /// Optional metadata shown by the engine's About page.
    #[serde(default)]
    pub project: ProjectMetadata,

    /// Optional native-shell features. Features stay off unless the project
    /// explicitly opts in.
    #[serde(default)]
    pub features: FeatureConfig,

    /// Independently selected parser/codec categories.
    #[serde(default)]
    pub adapter: AdapterConfig,

    /// Asset path mappings (key → relative path under assets/).
    #[serde(default)]
    pub assets: AssetMap,

    /// Font configuration.
    #[serde(default)]
    pub fonts: FontConfig,

    /// UI style overrides.
    #[serde(default)]
    pub styles: StyleConfig,

    /// Layout settings (anchor offsets, dodge, etc).
    #[serde(default)]
    pub layout: LayoutConfig,
}

/// Human-facing project information. It deliberately stays independent from
/// resource adapters so packaged and development projects expose the same
/// metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMetadata {
    /// Short description of the currently loaded visual novel.
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureConfig {
    /// Enables the title-screen EXTRA entry and its CG/BGM gallery.
    #[serde(default)]
    pub extra: bool,
}

/// One resource source consumed through an asset adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetSourceConfig {
    /// Adapter-specific location. Built-in local adapters resolve it relative
    /// to the directory containing `config.yaml`.
    #[serde(default = "default_source_path")]
    pub path: String,
    /// Asset adapter option selected from `adapter/asset/*`.
    #[serde(default = "default_asset_adapter")]
    pub format: String,
}

fn default_source_path() -> String {
    ".".into()
}

fn default_asset_adapter() -> String {
    "fs".into()
}

fn default_asset_sources() -> Vec<AssetSourceConfig> {
    vec![AssetSourceConfig::default()]
}

impl Default for AssetSourceConfig {
    fn default() -> Self {
        Self {
            path: default_source_path(),
            format: default_asset_adapter(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterConfig {
    /// Ordered asset layers. Later sources override earlier logical paths.
    #[serde(default = "default_asset_sources")]
    pub asset: Vec<AssetSourceConfig>,
    /// Script syntax selected from the loader adapter's script category.
    #[serde(default = "default_script_adapter")]
    pub script: String,
    /// Save-state codec selected from `adapter/store/*`.
    #[serde(default = "default_store_adapter")]
    pub store: String,
}

fn default_script_adapter() -> String {
    "webgal".into()
}

fn default_store_adapter() -> String {
    "crabgal".into()
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            asset: default_asset_sources(),
            script: default_script_adapter(),
            store: default_store_adapter(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    /// Default offset from left/right screen edge for sprites (design px).
    #[serde(default = "default_anchor_offset")]
    pub anchor_offset: f32,
    /// Height of standing sprites in design pixels.
    #[serde(default = "default_sprite_height")]
    pub sprite_height: f32,
    /// Project-wide vertical sprite baseline offset in design pixels.
    /// Positive values move figures up; negative values move them down.
    #[serde(default = "default_sprite_y_offset")]
    pub sprite_y_offset: f32,

    // ── Textbox positioning (percent of the 1920×1080 design area) ──
    /// Textbox left edge when no mini avatar is displayed (%).
    #[serde(default = "default_textbox_left")]
    pub textbox_left: f32,
    /// Textbox left edge while a mini avatar occupies the leading edge (%).
    #[serde(default = "default_textbox_dodge_left")]
    pub textbox_dodge_left: f32,
    /// Textbox distance from bottom (%).
    #[serde(default = "default_textbox_bottom")]
    pub textbox_bottom: f32,
    /// Textbox height (%).
    #[serde(default = "default_textbox_height")]
    pub textbox_height: f32,
    /// Name bar distance from bottom (%).
    #[serde(default = "default_namebar_bottom")]
    pub namebar_bottom: f32,
}

fn default_anchor_offset() -> f32 {
    30.0
}
fn default_sprite_height() -> f32 {
    825.0
}
fn default_sprite_y_offset() -> f32 {
    0.0
}
fn default_textbox_left() -> f32 {
    0.0
}
fn default_textbox_dodge_left() -> f32 {
    10.0
}
fn default_textbox_bottom() -> f32 {
    1.0
}
fn default_textbox_height() -> f32 {
    22.0
}
fn default_namebar_bottom() -> f32 {
    24.0
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            anchor_offset: default_anchor_offset(),
            sprite_height: default_sprite_height(),
            sprite_y_offset: default_sprite_y_offset(),
            textbox_left: default_textbox_left(),
            textbox_dodge_left: default_textbox_dodge_left(),
            textbox_bottom: default_textbox_bottom(),
            textbox_height: default_textbox_height(),
            namebar_bottom: default_namebar_bottom(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssetMap {
    /// Background name → file path.
    #[serde(default)]
    pub backgrounds: HashMap<String, String>,
    /// Figure/character name → file path.
    #[serde(default)]
    pub figures: HashMap<String, String>,
    /// BGM name → file path.
    #[serde(default)]
    pub bgm: HashMap<String, String>,
    /// Voice name → file path.
    #[serde(default)]
    pub voices: HashMap<String, String>,
    /// Sound-effect name → file path.
    #[serde(default)]
    pub effects: HashMap<String, String>,
    /// Video name → file path.
    #[serde(default)]
    pub videos: HashMap<String, String>,
    /// Camera color-grade preset → LUT image path.
    #[serde(default)]
    pub luts: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontConfig {
    /// Speaker name size (design px).
    #[serde(default = "default_speaker_size")]
    pub speaker_size: f32,
    /// Dialogue text size (design px).
    #[serde(default = "default_dialogue_size")]
    pub dialogue_size: f32,
    /// Control bar icon size (design px).
    #[serde(default = "default_icon_size")]
    pub icon_size: f32,
    /// Control bar label size (design px).
    #[serde(default = "default_label_size")]
    pub label_size: f32,
}

fn default_speaker_size() -> f32 {
    39.0
}
fn default_dialogue_size() -> f32 {
    45.0
}
fn default_icon_size() -> f32 {
    19.5
}
fn default_label_size() -> f32 {
    18.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleConfig {
    /// Textbox background opacity (0.0–1.0).
    #[serde(default = "default_textbox_alpha")]
    pub textbox_alpha: f32,
    /// Typewriter speed in chars per second.
    #[serde(default = "default_typewriter_speed")]
    pub typewriter_speed: f64,
    /// Auto-play delay in seconds.
    #[serde(default = "default_auto_delay")]
    pub auto_delay: f64,
}

// ── Defaults ──

fn default_title() -> String {
    "crabgal".into()
}
fn default_title_background() -> String {
    "bg".into()
}
fn default_textbox_alpha() -> f32 {
    0.72
}
fn default_typewriter_speed() -> f64 {
    45.0
}
fn default_auto_delay() -> f64 {
    2.0
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            title: default_title(),
            title_background: default_title_background(),
            project: ProjectMetadata::default(),
            features: FeatureConfig::default(),
            adapter: AdapterConfig::default(),
            assets: AssetMap::default(),
            fonts: FontConfig::default(),
            styles: StyleConfig::default(),
            layout: LayoutConfig::default(),
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            speaker_size: default_speaker_size(),
            dialogue_size: default_dialogue_size(),
            icon_size: default_icon_size(),
            label_size: default_label_size(),
        }
    }
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            textbox_alpha: default_textbox_alpha(),
            typewriter_speed: default_typewriter_speed(),
            auto_delay: default_auto_delay(),
        }
    }
}

impl GameConfig {
    /// Parse project configuration from an arbitrary content source.
    pub fn from_yaml(yaml: &str) -> Result<Self, noyalib::Error> {
        noyalib::from_str(yaml)
    }

    /// Load from a YAML file, falling back to defaults.
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(yaml) => Self::from_yaml(&yaml).unwrap_or_else(|error| {
                log::error!("invalid config {}: {error}; using defaults", path.display());
                Self::default()
            }),
            Err(error) => {
                log::warn!(
                    "failed to read config {}: {error}; using defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Resolve a background asset name to its file path.
    pub fn bg_path(&self, name: &str) -> String {
        self.assets
            .backgrounds
            .get(name)
            .cloned()
            .unwrap_or_else(|| format!("background/{}", name))
    }

    /// Resolve a figure asset name to its file path.
    pub fn figure_path(&self, name: &str) -> String {
        self.assets
            .figures
            .get(name)
            .cloned()
            .unwrap_or_else(|| format!("figure/{}", name))
    }

    /// Resolve a voice asset name to its path below the local asset root.
    pub fn voice_path(&self, name: &str) -> String {
        self.assets
            .voices
            .get(name)
            .cloned()
            .unwrap_or_else(|| format!("vocal/{name}"))
    }

    /// Resolve a sound effect below the local asset root.
    pub fn effect_path(&self, name: &str) -> String {
        self.assets
            .effects
            .get(name)
            .cloned()
            .unwrap_or_else(|| format!("vocal/{name}"))
    }

    /// Resolve background music below the local asset root.
    pub fn bgm_path(&self, name: &str) -> String {
        self.assets
            .bgm
            .get(name)
            .cloned()
            .unwrap_or_else(|| format!("bgm/{name}"))
    }

    /// Resolve video below the local asset root.
    pub fn video_path(&self, name: &str) -> String {
        self.assets
            .videos
            .get(name)
            .cloned()
            .unwrap_or_else(|| format!("video/{name}"))
    }

    pub fn lut_path(&self, name: &str) -> String {
        self.assets
            .luts
            .get(name)
            .cloned()
            .unwrap_or_else(|| format!("luts/{name}.png"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = GameConfig::default();
        assert_eq!(cfg.title, "crabgal");
        assert_eq!(cfg.styles.typewriter_speed, 45.0);
        assert!(!cfg.features.extra);
        assert_eq!(cfg.adapter, AdapterConfig::default());
        assert_eq!(cfg.layout.textbox_left, 0.0);
        assert_eq!(cfg.layout.textbox_dodge_left, 10.0);
    }

    #[test]
    fn test_parse_minimal() {
        let yaml = r#"
title: "Test Game"
styles:
  typewriter_speed: 60.0
"#;
        let cfg: GameConfig = noyalib::from_str(yaml).unwrap();
        assert_eq!(cfg.title, "Test Game");
        assert_eq!(cfg.styles.typewriter_speed, 60.0);
        assert!(!cfg.features.extra);
        assert_eq!(cfg.adapter, AdapterConfig::default());
        assert_eq!(cfg.layout.sprite_y_offset, 0.0);
    }

    #[test]
    fn parses_project_wide_sprite_y_offset() {
        let cfg = GameConfig::from_yaml(
            r#"
layout:
  sprite_y_offset: -90
"#,
        )
        .unwrap();

        assert_eq!(cfg.layout.sprite_y_offset, -90.0);
    }

    #[test]
    fn parses_project_metadata() {
        let yaml = r#"
title: "Example"
project:
  description: "A short visual novel."
"#;
        let cfg = GameConfig::from_yaml(yaml).unwrap();
        assert_eq!(cfg.project.description, "A short visual novel.");
    }

    #[test]
    fn extra_requires_an_explicit_feature_opt_in() {
        let cfg = GameConfig::from_yaml(
            r#"
features:
  extra: true
"#,
        )
        .unwrap();

        assert!(cfg.features.extra);
    }

    #[test]
    fn parses_adapter_options_and_ordered_asset_sources() {
        let yaml = r#"
adapter:
  asset:
    - path: "."
      format: fs
    - path: "packs/voices"
      format: fs
    - path: "packs/route.hxz"
      format: hexz
  script: webgal
  store: crabgal
"#;
        let cfg: GameConfig = noyalib::from_str(yaml).unwrap();
        assert_eq!(cfg.adapter.asset.len(), 3);
        assert_eq!(cfg.adapter.asset[1].format, "fs");
        assert_eq!(cfg.adapter.asset[2].format, "hexz");
        assert_eq!(cfg.adapter.script, "webgal");
        assert_eq!(cfg.adapter.store, "crabgal");
    }
}
