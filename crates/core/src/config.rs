// Game configuration, loaded from config.yaml.
// Inspired by Raven's MainConfig pattern.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::{collections::HashMap, error::Error, fmt, fs};

use crate::DESIGN_HEIGHT;

const MAX_FONT_SIZE: f32 = DESIGN_HEIGHT;
const MAX_SPRITE_HEIGHT: f32 = DESIGN_HEIGHT * 4.0;

#[derive(Debug)]
pub enum GameConfigError {
    Parse(noyalib::Error),
    Validation(ConfigValidationError),
}

impl fmt::Display for GameConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::Validation(error) => error.fmt(formatter),
        }
    }
}

impl Error for GameConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Validation(error) => Some(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigValidationError {
    field: &'static str,
    requirement: &'static str,
}

impl ConfigValidationError {
    fn new(field: &'static str, requirement: &'static str) -> Self {
        Self { field, requirement }
    }
}

impl fmt::Display for ConfigValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.field, self.requirement)
    }
}

impl Error for ConfigValidationError {}

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
    /// Stable, filesystem-safe identity used to namespace shipping saves.
    /// Development projects may omit it, but packaging requires one.
    #[serde(default)]
    pub id: String,
    /// Optional reverse-DNS native application identity. When omitted,
    /// shipping tools derive `moe.maincore.keine.<id>`.
    #[serde(default)]
    pub bundle_identifier: String,
    /// Short description of the currently loaded visual novel.
    #[serde(default)]
    pub description: String,
}

impl ProjectMetadata {
    pub fn valid_id(&self) -> Option<&str> {
        valid_project_id(&self.id).then_some(self.id.as_str())
    }

    pub fn application_identifier(&self) -> Option<String> {
        let id = self.valid_id()?;
        if self.bundle_identifier.is_empty() {
            return Some(format!("moe.maincore.keine.{id}"));
        }
        valid_bundle_identifier(&self.bundle_identifier).then(|| self.bundle_identifier.clone())
    }
}

fn valid_project_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn valid_bundle_identifier(identifier: &str) -> bool {
    !identifier.is_empty()
        && identifier.len() <= 255
        && identifier.split('.').count() >= 3
        && identifier.split('.').all(|component| {
            let bytes = component.as_bytes();
            !bytes.is_empty()
                && bytes[0].is_ascii_alphanumeric()
                && bytes[bytes.len() - 1].is_ascii_alphanumeric()
                && bytes
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        })
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
    "keine".into()
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
    /// Per-glyph reveal animation used by editor-authored dialogue boxes.
    #[serde(default)]
    pub text_reveal: TextRevealConfig,
}

/// Adapter-neutral form of LetsGal Studio 1.9.1's dialogue reveal settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextRevealConfig {
    /// Time for a newly revealed glyph to settle, in seconds.
    #[serde(default)]
    pub duration: f32,
    #[serde(default)]
    pub effect: TextRevealEffect,
    #[serde(default = "default_reveal_distance")]
    pub distance: f32,
    #[serde(default = "default_reveal_scale")]
    pub scale: f32,
    #[serde(default = "default_reveal_rotation")]
    pub rotation: f32,
    #[serde(default = "default_reveal_blur")]
    pub blur: f32,
}

impl Default for TextRevealConfig {
    fn default() -> Self {
        Self {
            duration: 0.0,
            effect: TextRevealEffect::Instant,
            distance: default_reveal_distance(),
            scale: default_reveal_scale(),
            rotation: default_reveal_rotation(),
            blur: default_reveal_blur(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TextRevealEffect {
    SmoothRise,
    Classic,
    #[default]
    Instant,
    SmoothDrop,
    SlideLeft,
    SlideRight,
    Pop,
    Flip,
    Swing,
    Blur,
}

// ── Defaults ──

fn default_title() -> String {
    "Kēne".into()
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
fn default_reveal_distance() -> f32 {
    8.0
}
fn default_reveal_scale() -> f32 {
    0.82
}
fn default_reveal_rotation() -> f32 {
    70.0
}
fn default_reveal_blur() -> f32 {
    4.0
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
            text_reveal: TextRevealConfig::default(),
        }
    }
}

impl GameConfig {
    /// Parse project configuration from an arbitrary content source.
    pub fn from_yaml(yaml: &str) -> Result<Self, GameConfigError> {
        let config: Self = noyalib::from_str(yaml).map_err(GameConfigError::Parse)?;
        config.validate().map_err(GameConfigError::Validation)?;
        Ok(config)
    }

    /// Validates every numeric field once at the adapter/runtime boundary so
    /// render, decode and timing consumers cannot disagree about malformed
    /// values.
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        finite_f32("layout.anchor_offset", self.layout.anchor_offset)?;
        positive_bounded_f32(
            "layout.sprite_height",
            self.layout.sprite_height,
            MAX_SPRITE_HEIGHT,
        )?;
        finite_f32("layout.sprite_y_offset", self.layout.sprite_y_offset)?;
        percent("layout.textbox_left", self.layout.textbox_left, false)?;
        percent(
            "layout.textbox_dodge_left",
            self.layout.textbox_dodge_left,
            false,
        )?;
        percent("layout.textbox_bottom", self.layout.textbox_bottom, false)?;
        percent("layout.textbox_height", self.layout.textbox_height, true)?;
        percent("layout.namebar_bottom", self.layout.namebar_bottom, false)?;

        positive_bounded_f32("fonts.speaker_size", self.fonts.speaker_size, MAX_FONT_SIZE)?;
        positive_bounded_f32(
            "fonts.dialogue_size",
            self.fonts.dialogue_size,
            MAX_FONT_SIZE,
        )?;
        positive_bounded_f32("fonts.icon_size", self.fonts.icon_size, MAX_FONT_SIZE)?;
        positive_bounded_f32("fonts.label_size", self.fonts.label_size, MAX_FONT_SIZE)?;

        bounded_f32("styles.textbox_alpha", self.styles.textbox_alpha, 0.0, 1.0)?;
        positive_f64("styles.typewriter_speed", self.styles.typewriter_speed)?;
        non_negative_f64("styles.auto_delay", self.styles.auto_delay)?;
        non_negative_f32(
            "styles.text_reveal.duration",
            self.styles.text_reveal.duration,
        )?;
        finite_f32(
            "styles.text_reveal.distance",
            self.styles.text_reveal.distance,
        )?;
        finite_f32("styles.text_reveal.scale", self.styles.text_reveal.scale)?;
        finite_f32(
            "styles.text_reveal.rotation",
            self.styles.text_reveal.rotation,
        )?;
        non_negative_f32("styles.text_reveal.blur", self.styles.text_reveal.blur)?;
        Ok(())
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
        resolve_asset_path(&self.assets.backgrounds, name, |name| {
            format!("background/{name}")
        })
    }

    /// Resolve a figure asset name to its file path.
    pub fn figure_path(&self, name: &str) -> String {
        resolve_asset_path(&self.assets.figures, name, |name| format!("figure/{name}"))
    }

    /// Resolve a voice asset name to its path below the local asset root.
    pub fn voice_path(&self, name: &str) -> String {
        resolve_asset_path(&self.assets.voices, name, |name| format!("vocal/{name}"))
    }

    /// Resolve a sound effect below the local asset root.
    pub fn effect_path(&self, name: &str) -> String {
        resolve_asset_path(&self.assets.effects, name, |name| format!("vocal/{name}"))
    }

    /// Whether a sound effect still depends on the historical `vocal/`
    /// single-name fallback. Callers can use this to guide projects towards
    /// the canonical `se/` directory without silently breaking old content.
    pub fn uses_legacy_effect_fallback(&self, name: &str) -> bool {
        !self.assets.effects.contains_key(name) && !is_direct_asset_path(name)
    }

    /// Resolve background music below the local asset root.
    pub fn bgm_path(&self, name: &str) -> String {
        resolve_asset_path(&self.assets.bgm, name, |name| format!("bgm/{name}"))
    }

    /// Resolve video below the local asset root.
    pub fn video_path(&self, name: &str) -> String {
        resolve_asset_path(&self.assets.videos, name, |name| format!("video/{name}"))
    }

    pub fn lut_path(&self, name: &str) -> String {
        resolve_asset_path(&self.assets.luts, name, |name| format!("luts/{name}.webp"))
    }
}

fn finite_f32(field: &'static str, value: f32) -> Result<(), ConfigValidationError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ConfigValidationError::new(field, "must be finite"))
    }
}

fn non_negative_f32(field: &'static str, value: f32) -> Result<(), ConfigValidationError> {
    finite_f32(field, value)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(ConfigValidationError::new(
            field,
            "must be greater than or equal to 0",
        ))
    }
}

fn bounded_f32(
    field: &'static str,
    value: f32,
    minimum: f32,
    maximum: f32,
) -> Result<(), ConfigValidationError> {
    finite_f32(field, value)?;
    if value >= minimum && value <= maximum {
        Ok(())
    } else {
        Err(ConfigValidationError::new(
            field,
            "is outside its valid range",
        ))
    }
}

fn positive_bounded_f32(
    field: &'static str,
    value: f32,
    maximum: f32,
) -> Result<(), ConfigValidationError> {
    finite_f32(field, value)?;
    if value > 0.0 && value <= maximum {
        Ok(())
    } else {
        Err(ConfigValidationError::new(
            field,
            "must be greater than 0 and within its hard limit",
        ))
    }
}

fn percent(
    field: &'static str,
    value: f32,
    strictly_positive: bool,
) -> Result<(), ConfigValidationError> {
    bounded_f32(field, value, 0.0, 100.0)?;
    if strictly_positive && value == 0.0 {
        return Err(ConfigValidationError::new(field, "must be greater than 0"));
    }
    Ok(())
}

fn positive_f64(field: &'static str, value: f64) -> Result<(), ConfigValidationError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(ConfigValidationError::new(
            field,
            "must be finite and greater than 0",
        ))
    }
}

fn non_negative_f64(field: &'static str, value: f64) -> Result<(), ConfigValidationError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(ConfigValidationError::new(
            field,
            "must be finite and greater than or equal to 0",
        ))
    }
}

fn resolve_asset_path(
    aliases: &HashMap<String, String>,
    name: &str,
    fallback: impl FnOnce(&str) -> String,
) -> String {
    aliases
        .get(name)
        .cloned()
        .or_else(|| is_direct_asset_path(name).then(|| name.to_owned()))
        .unwrap_or_else(|| fallback(name))
}

/// Accept portable, root-relative paths with at least one directory segment.
/// The content mount remains the final containment boundary; this check only
/// distinguishes explicit paths from logical asset names consistently across
/// host operating systems.
fn is_direct_asset_path(name: &str) -> bool {
    let mut segments = 0usize;
    for segment in name.split('/') {
        if segment.is_empty() || matches!(segment, "." | "..") || segment.contains(['\\', ':']) {
            return false;
        }
        segments += 1;
    }
    segments > 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = GameConfig::default();
        assert_eq!(cfg.title, "Kēne");
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
        let cfg = GameConfig::from_yaml(yaml).unwrap();
        assert_eq!(cfg.title, "Test Game");
        assert_eq!(cfg.styles.typewriter_speed, 60.0);
        assert!(!cfg.features.extra);
        assert_eq!(cfg.adapter, AdapterConfig::default());
        assert_eq!(cfg.layout.sprite_y_offset, 0.0);
    }

    #[test]
    fn rejects_non_finite_and_semantically_invalid_numeric_fields() {
        let mut config = GameConfig::default();
        config.layout.sprite_height = f32::NAN;
        assert_eq!(
            config.validate().unwrap_err().to_string(),
            "layout.sprite_height must be finite"
        );

        config = GameConfig::default();
        config.styles.textbox_alpha = 1.1;
        assert_eq!(
            config.validate().unwrap_err().to_string(),
            "styles.textbox_alpha is outside its valid range"
        );

        config = GameConfig::default();
        config.styles.typewriter_speed = 0.0;
        assert_eq!(
            config.validate().unwrap_err().to_string(),
            "styles.typewriter_speed must be finite and greater than 0"
        );
    }

    #[test]
    fn yaml_parsing_applies_the_same_semantic_validator() {
        let error = GameConfig::from_yaml(
            r#"
layout:
  sprite_height: -1
"#,
        )
        .unwrap_err();

        assert!(error.to_string().starts_with("layout.sprite_height "));
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
  id: example-game
  bundle_identifier: moe.example.game
  description: "A short visual novel."
"#;
        let cfg = GameConfig::from_yaml(yaml).unwrap();
        assert_eq!(cfg.project.valid_id(), Some("example-game"));
        assert_eq!(
            cfg.project.application_identifier().as_deref(),
            Some("moe.example.game")
        );
        assert_eq!(cfg.project.description, "A short visual novel.");
    }

    #[test]
    fn shipping_identity_is_strict_and_has_a_stable_default_bundle_id() {
        let metadata = ProjectMetadata {
            id: "example-game".into(),
            ..ProjectMetadata::default()
        };
        assert_eq!(
            metadata.application_identifier().as_deref(),
            Some("moe.maincore.keine.example-game")
        );

        for invalid in ["", "Example", "-example", "example_1", "example/one"] {
            let metadata = ProjectMetadata {
                id: invalid.into(),
                ..ProjectMetadata::default()
            };
            assert_eq!(metadata.valid_id(), None, "{invalid:?}");
            assert_eq!(metadata.application_identifier(), None, "{invalid:?}");
        }

        let invalid_bundle = ProjectMetadata {
            id: "example".into(),
            bundle_identifier: "not a bundle/id".into(),
            ..ProjectMetadata::default()
        };
        assert_eq!(invalid_bundle.application_identifier(), None);
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
    - path: "content/route"
      format: auto
  script: webgal
  store: keine
"#;
        let cfg: GameConfig = noyalib::from_str(yaml).unwrap();
        assert_eq!(cfg.adapter.asset.len(), 3);
        assert_eq!(cfg.adapter.asset[1].format, "fs");
        assert_eq!(cfg.adapter.asset[2].format, "auto");
        assert_eq!(cfg.adapter.script, "webgal");
        assert_eq!(cfg.adapter.store, "keine");
    }

    #[test]
    fn ignores_the_removed_compiled_program_policy() {
        let cfg = GameConfig::from_yaml("compiled_program: require\n").unwrap();
        assert_eq!(cfg.title, default_title());
    }

    #[test]
    fn aliases_take_priority_over_direct_paths() {
        let mut cfg = GameConfig::default();
        cfg.assets
            .backgrounds
            .insert("background/day.webp".into(), "packs/day.webp".into());

        assert_eq!(cfg.bg_path("background/day.webp"), "packs/day.webp");
    }

    #[test]
    fn explicit_relative_asset_paths_are_preserved() {
        let cfg = GameConfig::default();

        assert_eq!(cfg.bg_path("background/day.webp"), "background/day.webp");
        assert_eq!(cfg.figure_path("figure/a.webp"), "figure/a.webp");
        assert_eq!(cfg.voice_path("vocal/a.opus"), "vocal/a.opus");
        assert_eq!(cfg.effect_path("se/a.opus"), "se/a.opus");
        assert_eq!(cfg.bgm_path("bgm/a.opus"), "bgm/a.opus");
        assert_eq!(cfg.video_path("video/a.mp4"), "video/a.mp4");
        assert_eq!(cfg.lut_path("luts/night.png"), "luts/night.png");
    }

    #[test]
    fn logical_asset_names_keep_compatible_fallbacks() {
        let cfg = GameConfig::default();

        assert_eq!(cfg.bg_path("day.webp"), "background/day.webp");
        assert_eq!(cfg.figure_path("a.webp"), "figure/a.webp");
        assert_eq!(cfg.voice_path("a.opus"), "vocal/a.opus");
        assert_eq!(cfg.effect_path("a.opus"), "vocal/a.opus");
        assert_eq!(cfg.bgm_path("a.opus"), "bgm/a.opus");
        assert_eq!(cfg.video_path("a.mp4"), "video/a.mp4");
        assert_eq!(cfg.lut_path("night"), "luts/night.webp");
        assert!(cfg.uses_legacy_effect_fallback("a.opus"));
        assert!(!cfg.uses_legacy_effect_fallback("se/a.opus"));
    }

    #[test]
    fn parent_and_platform_specific_paths_are_not_treated_as_direct() {
        let cfg = GameConfig::default();

        assert_eq!(cfg.bg_path("../day.webp"), "background/../day.webp");
        assert_eq!(
            cfg.bg_path("C:\\assets\\day.webp"),
            "background/C:\\assets\\day.webp"
        );
    }
}
