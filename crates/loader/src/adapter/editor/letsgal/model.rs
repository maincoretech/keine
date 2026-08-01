// The Studio format is intentionally open. Several retained fields are not
// consumed by the initial compiler yet, but keeping them typed prevents a
// future write-capable adapter from dropping extension-owned data.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProjectDocument {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub keine: KeineProjectConfig,
    #[serde(default)]
    pub chapter_order: Vec<String>,
    #[serde(default)]
    pub chapter_folders: Vec<ChapterFolder>,
    #[serde(default)]
    pub chapter_tree_order: Option<Vec<ChapterTreeEntry>>,
    #[serde(default)]
    pub resolution: Resolution,
    #[serde(default)]
    pub extensions: BTreeMap<String, ExtensionSelection>,
    #[serde(default)]
    pub extension_settings: BTreeMap<String, Value>,
    #[serde(default)]
    pub system_bindings: BTreeMap<String, String>,
    #[serde(default)]
    pub action_bindings: BTreeMap<String, Vec<String>>,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChapterFolder {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub chapter_ids: Vec<String>,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ChapterTreeEntry {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct KeineProjectConfig {
    #[serde(default)]
    pub features: keine_core::config::FeatureConfig,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Resolution {
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
}

impl Default for Resolution {
    fn default() -> Self {
        Self {
            width: default_width(),
            height: default_height(),
        }
    }
}

const fn default_width() -> u32 {
    1920
}

const fn default_height() -> u32 {
    1080
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ExtensionSelection {
    #[serde(default)]
    pub enabled: bool,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ChapterDocument {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub fragments: Vec<StoryFragment>,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct StoryFragment {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub blocks: Vec<StoryBlock>,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

/// Studio deliberately treats blocks as an open structure. Known fields are
/// typed and every unknown field is retained so version additions never get
/// destroyed by a keine read/write round trip.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct StoryBlock {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub content: Value,
    #[serde(default)]
    pub props: Map<String, Value>,
    #[serde(default)]
    pub children: Vec<StoryBlock>,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct ScenesDocument {
    #[serde(default)]
    pub scenes: Vec<SceneDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SceneDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub layers: Vec<SceneLayer>,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SceneLayer {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub asset_path: String,
    #[serde(default = "default_distance")]
    pub distance: f32,
    #[serde(default)]
    pub offset: String,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

const fn default_distance() -> f32 {
    1.0
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CharactersDocument {
    #[serde(default)]
    pub global_settings: CharacterGlobalSettings,
    #[serde(default)]
    pub attribute_template: Vec<CharacterAttribute>,
    #[serde(default)]
    pub characters: Vec<CharacterDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CharacterAttribute {
    pub name: String,
    #[serde(default)]
    pub default_value: Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CharacterGlobalSettings {
    #[serde(default)]
    pub positions: Vec<CharacterPosition>,
    #[serde(default)]
    pub default_position_id: String,
    #[serde(default)]
    pub graphics: CharacterGraphics,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CharacterGraphics {
    #[serde(default)]
    pub height_ratio: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct CharacterPosition {
    pub id: String,
    #[serde(default)]
    pub left: f32,
    #[serde(default)]
    pub top: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CharacterDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub expressions: Vec<CharacterExpression>,
    #[serde(default)]
    pub default_position: String,
    #[serde(default)]
    pub attribute_values: HashMap<String, Value>,
    #[serde(default)]
    pub portrait_skin_config: Option<PortraitSkinConfig>,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PortraitSkinConfig {
    #[serde(default)]
    pub skins: Vec<String>,
    #[serde(default)]
    pub default_skin: String,
    #[serde(default)]
    pub attribute_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CharacterExpression {
    pub name: String,
    #[serde(default)]
    pub asset_path: String,
    #[serde(default)]
    pub skin_assets: HashMap<String, String>,
    #[serde(default)]
    pub graphics_override: CharacterGraphics,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct AssetManifest {
    #[serde(default)]
    pub entries: BTreeMap<String, AssetEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct AssetEntry {
    pub path: String,
    #[serde(default)]
    pub voice: Option<VoiceMetadata>,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VoiceMetadata {
    #[serde(default)]
    pub character_id: String,
    #[serde(default)]
    pub asr_text: String,
    #[serde(flatten)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StudioState {
    #[serde(default)]
    pub active_chapter_id: String,
    #[serde(default)]
    pub active_fragment_id: String,
    #[serde(default)]
    pub cursor_block_index: usize,
    #[serde(default)]
    pub cursor_block_index_by_fragment: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct VariablesDocument {
    #[serde(default)]
    pub variables: Vec<VariableDeclaration>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DialogueBoxDocument {
    #[serde(default)]
    pub dialogue_behavior: DialogueBehavior,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct DialogueBehavior {
    #[serde(default = "default_studio_text_speed")]
    pub text_speed: f64,
    #[serde(default = "default_studio_fade_duration")]
    pub char_fade_in_duration: f32,
    #[serde(default = "default_studio_reveal_effect")]
    pub text_reveal_effect: keine_core::config::TextRevealEffect,
    #[serde(default)]
    pub text_reveal_parameters: DialogueRevealParameters,
}

impl Default for DialogueBehavior {
    fn default() -> Self {
        Self {
            text_speed: default_studio_text_speed(),
            char_fade_in_duration: default_studio_fade_duration(),
            text_reveal_effect: default_studio_reveal_effect(),
            text_reveal_parameters: DialogueRevealParameters::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct DialogueRevealParameters {
    #[serde(default = "default_studio_reveal_distance")]
    pub distance_px: f32,
    #[serde(default = "default_studio_reveal_scale")]
    pub scale_percent: f32,
    #[serde(default = "default_studio_reveal_rotation")]
    pub rotation_degrees: f32,
    #[serde(default = "default_studio_reveal_blur")]
    pub blur_px: f32,
}

impl Default for DialogueRevealParameters {
    fn default() -> Self {
        Self {
            distance_px: default_studio_reveal_distance(),
            scale_percent: default_studio_reveal_scale(),
            rotation_degrees: default_studio_reveal_rotation(),
            blur_px: default_studio_reveal_blur(),
        }
    }
}

const fn default_studio_text_speed() -> f64 {
    80.0
}

const fn default_studio_fade_duration() -> f32 {
    100.0
}

const fn default_studio_reveal_effect() -> keine_core::config::TextRevealEffect {
    keine_core::config::TextRevealEffect::SmoothRise
}

const fn default_studio_reveal_distance() -> f32 {
    8.0
}

const fn default_studio_reveal_scale() -> f32 {
    82.0
}

const fn default_studio_reveal_rotation() -> f32 {
    70.0
}

const fn default_studio_reveal_blur() -> f32 {
    4.0
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VariableDeclaration {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(rename = "type", default)]
    pub value_type: String,
    #[serde(default)]
    pub default_value: Value,
    #[serde(default)]
    pub persistence: String,
}
