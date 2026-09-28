pub mod projection;
pub mod syntax;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use keine_core::config::{
    EiyashouAssetEntry, EiyashouAssetManifest, EiyashouCharacterManifest, GameConfig,
};
use keine_core::{Action, EiyashouTextPart};
use keine_loader::{
    DiagnosticLevel, NativeTokenKind, ResourceKind, parse_native_document, parse_native_scenes,
};

use crate::projection::{BlockKind, EiyashouProjection};
use crate::workspace::WorkspaceFile;

const MAX_INDEXED_SOURCE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssetKind {
    Background,
    Figure,
    Voice,
    Bgm,
    Effect,
    Video,
}

impl AssetKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Background => "Background",
            Self::Figure => "Figure",
            Self::Voice => "Voice",
            Self::Bgm => "BGM",
            Self::Effect => "Effect",
            Self::Video => "Video",
        }
    }

    fn from_resource(kind: ResourceKind) -> Option<Self> {
        match kind {
            ResourceKind::Background => Some(Self::Background),
            ResourceKind::Figure | ResourceKind::MiniAvatar => Some(Self::Figure),
            ResourceKind::Voice => Some(Self::Voice),
            ResourceKind::Bgm => Some(Self::Bgm),
            ResourceKind::Effect => Some(Self::Effect),
            ResourceKind::Video => Some(Self::Video),
            ResourceKind::Particle | ResourceKind::Lut => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetEntry {
    pub kind: AssetKind,
    pub id: String,
    pub path: PathBuf,
    pub tags: Vec<String>,
    pub exists: bool,
    pub reference_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetReference {
    pub key: AssetKey,
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub range: Option<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnmappedAsset {
    pub kind: AssetKind,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssetKey {
    pub kind: AssetKind,
    pub id: String,
}

impl AssetEntry {
    pub fn key(&self) -> AssetKey {
        AssetKey {
            kind: self.kind,
            id: self.id.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AssetSort {
    #[default]
    Name,
    Path,
    References,
}

#[derive(Clone, Debug, Default)]
pub struct AssetQuery {
    pub search: String,
    pub kind: Option<AssetKind>,
    pub folder: Option<PathBuf>,
    pub sort: AssetSort,
}

impl AssetQuery {
    pub fn results<'a>(&self, assets: &'a [AssetEntry]) -> Vec<&'a AssetEntry> {
        let search = self.search.trim().to_lowercase();
        let mut results = assets
            .iter()
            .filter(|asset| self.kind.is_none_or(|kind| asset.kind == kind))
            .filter(|asset| {
                self.folder
                    .as_ref()
                    .is_none_or(|folder| asset.path.starts_with(folder))
            })
            .filter(|asset| {
                search.is_empty()
                    || asset.id.to_lowercase().contains(&search)
                    || asset
                        .path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&search)
                    || asset
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&search))
            })
            .collect::<Vec<_>>();
        results.sort_by(|left, right| {
            let primary = match self.sort {
                AssetSort::Name => left.id.cmp(&right.id),
                AssetSort::Path => left.path.cmp(&right.path),
                AssetSort::References => right.reference_count.cmp(&left.reference_count),
            };
            primary
                .then_with(|| left.kind.cmp(&right.kind))
                .then_with(|| left.id.cmp(&right.id))
                .then_with(|| left.path.cmp(&right.path))
        });
        results
    }

    pub fn unmapped_results<'a>(&self, assets: &'a [UnmappedAsset]) -> Vec<&'a UnmappedAsset> {
        let search = self.search.trim().to_lowercase();
        let mut results = assets
            .iter()
            .filter(|asset| self.kind.is_none_or(|kind| asset.kind == kind))
            .filter(|asset| {
                self.folder
                    .as_ref()
                    .is_none_or(|folder| asset.path.starts_with(folder))
            })
            .filter(|asset| {
                search.is_empty()
                    || asset
                        .path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&search)
            })
            .collect::<Vec<_>>();
        results.sort_by(|left, right| match self.sort {
            AssetSort::Name => left
                .path
                .file_name()
                .cmp(&right.path.file_name())
                .then_with(|| left.path.cmp(&right.path)),
            AssetSort::Path | AssetSort::References => left.path.cmp(&right.path),
        });
        results
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacterEntry {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneEntry {
    pub path: PathBuf,
    pub name: String,
    pub line: usize,
    pub name_range: Range<usize>,
    pub source_range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogueEntry {
    pub path: PathBuf,
    pub scene: String,
    pub speaker: String,
    pub text: String,
    pub editable: bool,
    pub line: usize,
    pub column: usize,
    pub source_range: Range<usize>,
    pub text_range: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemSeverity {
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoringProblem {
    pub severity: ProblemSeverity,
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct AuthoringIndex {
    pub native: bool,
    pub assets_manifest: Option<PathBuf>,
    pub characters_manifest: Option<PathBuf>,
    pub assets: Vec<AssetEntry>,
    pub asset_references: Vec<AssetReference>,
    pub unindexed_sources: Vec<PathBuf>,
    pub unmapped: Vec<UnmappedAsset>,
    pub characters: Vec<CharacterEntry>,
    pub scenes: Vec<SceneEntry>,
    pub dialogues: Vec<DialogueEntry>,
    pub problems: Vec<AuthoringProblem>,
}

impl AuthoringIndex {
    pub fn load(
        root: &Path,
        files: &[WorkspaceFile],
        overrides: &BTreeMap<PathBuf, String>,
    ) -> Self {
        let mut index = Self::default();
        let config_path = PathBuf::from("config.yaml");
        let Some(config_source) = read_text(root, &config_path, overrides) else {
            index.problems.push(problem(
                ProblemSeverity::Error,
                config_path,
                1,
                1,
                "config.yaml is missing or is not UTF-8",
            ));
            return index;
        };
        let config = match GameConfig::from_yaml(&config_source) {
            Ok(config) => config,
            Err(error) => {
                index.problems.push(problem(
                    ProblemSeverity::Error,
                    config_path,
                    1,
                    1,
                    format!("Invalid project configuration: {error}"),
                ));
                return index;
            }
        };
        if config.adapter.script != "keine" {
            return index;
        }
        index.native = true;

        let assets_path = confined_relative(&config.script.assets);
        let characters_path = confined_relative(&config.script.characters);
        index.assets_manifest = assets_path.clone();
        index.characters_manifest = characters_path.clone();

        let mut asset_lookup = HashSet::new();
        if let Some(path) = assets_path {
            match read_text(root, &path, overrides)
                .ok_or_else(|| "manifest is missing or is not UTF-8".to_owned())
                .and_then(|source| {
                    EiyashouAssetManifest::from_yaml(&source).map_err(|error| error.to_string())
                }) {
                Ok(manifest) => {
                    let unknown_namespaces = manifest
                        .unknown_namespaces()
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Background,
                        manifest.backgrounds,
                    );
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Figure,
                        manifest.figures,
                    );
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Voice,
                        manifest.voices,
                    );
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Bgm,
                        manifest.bgm,
                    );
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Effect,
                        manifest.effects,
                    );
                    push_assets(
                        root,
                        &mut index,
                        &mut asset_lookup,
                        AssetKind::Video,
                        manifest.videos,
                    );
                    for namespace in unknown_namespaces {
                        index.problems.push(problem(
                            ProblemSeverity::Warning,
                            path.clone(),
                            1,
                            1,
                            format!("Unknown asset namespace `{namespace}` is preserved"),
                        ));
                    }
                }
                Err(error) => index.problems.push(problem(
                    ProblemSeverity::Error,
                    path,
                    1,
                    1,
                    format!("Invalid asset manifest: {error}"),
                )),
            }
        } else {
            index.problems.push(problem(
                ProblemSeverity::Error,
                config_path.clone(),
                1,
                1,
                "script.assets must be a confined relative path",
            ));
        }

        let mapped = index
            .assets
            .iter()
            .map(|asset| asset.path.as_path())
            .collect::<HashSet<_>>();
        index.unmapped = files
            .iter()
            .filter(|file| {
                !file.is_dir() && file.size > 0 && !mapped.contains(file.relative_path.as_path())
            })
            .filter_map(|file| {
                crate::file_ops::unmapped_candidate_kind(&file.relative_path).map(|kind| {
                    UnmappedAsset {
                        kind,
                        path: file.relative_path.clone(),
                    }
                })
            })
            .collect();
        index
            .unmapped
            .sort_by(|left, right| left.path.cmp(&right.path));

        if let Some(path) = characters_path {
            match read_text(root, &path, overrides)
                .ok_or_else(|| "manifest is missing or is not UTF-8".to_owned())
                .and_then(|source| {
                    EiyashouCharacterManifest::from_yaml(&source).map_err(|error| error.to_string())
                }) {
                Ok(manifest) => {
                    let unknown_fields = manifest
                        .unknown_fields()
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    index.characters = manifest
                        .characters
                        .into_iter()
                        .map(|(id, character)| CharacterEntry {
                            id,
                            name: character.name,
                            color: character.color,
                        })
                        .collect();
                    index
                        .characters
                        .sort_by(|left, right| left.id.cmp(&right.id));
                    for field in unknown_fields {
                        index.problems.push(problem(
                            ProblemSeverity::Warning,
                            path.clone(),
                            1,
                            1,
                            format!("Unknown character manifest field `{field}` is preserved"),
                        ));
                    }
                }
                Err(error) => index.problems.push(problem(
                    ProblemSeverity::Error,
                    path,
                    1,
                    1,
                    format!("Invalid character manifest: {error}"),
                )),
            }
        } else {
            index.problems.push(problem(
                ProblemSeverity::Error,
                config_path,
                1,
                1,
                "script.characters must be a confined relative path",
            ));
        }

        let mut referenced = HashMap::<(AssetKind, String), usize>::new();
        for file in files.iter().filter(|file| {
            file.relative_path
                .extension()
                .and_then(|value| value.to_str())
                == Some("shou")
        }) {
            if file.size > MAX_INDEXED_SOURCE_BYTES {
                index.unindexed_sources.push(file.relative_path.clone());
                continue;
            }
            let Some(source) = read_text(root, &file.relative_path, overrides) else {
                index.unindexed_sources.push(file.relative_path.clone());
                index.problems.push(problem(
                    ProblemSeverity::Error,
                    file.relative_path.clone(),
                    1,
                    1,
                    "Source is missing or is not UTF-8",
                ));
                continue;
            };
            if source.len() as u64 > MAX_INDEXED_SOURCE_BYTES {
                index.unindexed_sources.push(file.relative_path.clone());
                continue;
            }
            index_source(
                &file.relative_path,
                &source,
                &mut index,
                &mut referenced,
                &asset_lookup,
            );
            if index.problems.iter().any(|problem| {
                problem.path == file.relative_path && problem.severity == ProblemSeverity::Error
            }) {
                index.unindexed_sources.push(file.relative_path.clone());
            }
        }

        for asset in &mut index.assets {
            asset.reference_count = referenced
                .get(&(asset.kind, asset.id.clone()))
                .copied()
                .unwrap_or_default();
        }
        index.assets.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.id.cmp(&right.id))
        });
        index.scenes.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.source_range.start.cmp(&right.source_range.start))
        });
        index.dialogues.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.source_range.start.cmp(&right.source_range.start))
        });
        index.problems.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.column.cmp(&right.column))
                .then_with(|| left.message.cmp(&right.message))
        });
        index.problems.dedup();
        index
    }

    pub fn selection(&self, path: &Path, line: usize) -> AuthoringSelection<'_> {
        if let Some(dialogue) = self
            .dialogues
            .iter()
            .find(|dialogue| dialogue.path == path && dialogue.line.saturating_sub(1) == line)
        {
            return AuthoringSelection::Dialogue(dialogue);
        }
        self.scenes
            .iter()
            .filter(|scene| scene.path == path && scene.line.saturating_sub(1) <= line)
            .max_by_key(|scene| scene.line)
            .map_or(AuthoringSelection::Source, AuthoringSelection::Scene)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum AuthoringSelection<'a> {
    Source,
    Scene(&'a SceneEntry),
    Dialogue(&'a DialogueEntry),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertKind {
    Narration,
    Dialogue,
    Background,
    Figure,
    Choice,
    Conditional,
    Loop,
    Variable,
    Goto,
    Call,
    Wait,
    Hide,
    Move,
    CameraMove,
    CameraShake,
    SpriteFocusRule,
    SpriteFocus,
    Bgm,
    Effect,
    Video,
    Return,
    Native(&'static str),
}

impl InsertKind {
    /// Canonical source names, including commands whose required asset does not
    /// exist yet. Text completion and the insertion palette share this catalogue.
    pub fn source_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Narration | Self::Dialogue => return None,
            Self::Background => "background",
            Self::Figure => "sprite",
            Self::Choice => "choice",
            Self::Conditional => "if",
            Self::Loop => "loop",
            Self::Variable => "let",
            Self::Goto => "goto",
            Self::Call => "call",
            Self::Wait => "wait",
            Self::Hide => "hide",
            Self::Move => "move",
            Self::CameraMove => "camera.move",
            Self::CameraShake => "camera.shake",
            Self::SpriteFocusRule => "sprite.focus.configure",
            Self::SpriteFocus => "sprite.focus",
            Self::Bgm => "bgm",
            Self::Effect => "se",
            Self::Video => "video",
            Self::Return => "return",
            Self::Native(name) => name,
        })
    }

    pub const ALL: [Self; 75] = [
        Self::Narration,
        Self::Dialogue,
        Self::Background,
        Self::Figure,
        Self::Hide,
        Self::Move,
        Self::CameraMove,
        Self::CameraShake,
        Self::SpriteFocusRule,
        Self::SpriteFocus,
        Self::Bgm,
        Self::Effect,
        Self::Video,
        Self::Choice,
        Self::Conditional,
        Self::Loop,
        Self::Goto,
        Self::Call,
        Self::Return,
        Self::Wait,
        Self::Variable,
        Self::Native("story.end"),
        Self::Native("avatar.show"),
        Self::Native("avatar.hide"),
        Self::Native("vocal.play"),
        Self::Native("vocal.stop"),
        Self::Native("screen.film"),
        Self::Native("text.box"),
        Self::Native("wait.advance"),
        Self::Native("playback.auto"),
        Self::Native("ui.show"),
        Self::Native("ui.hide"),
        Self::Native("particle.layers.clear"),
        Self::Native("text.presentation"),
        Self::Native("text.retract"),
        Self::Native("text.float.hide"),
        Self::Native("text.float.configure"),
        Self::Native("text.style"),
        Self::Native("scene.parallax.stop"),
        Self::Native("particle.hide"),
        Self::Native("video.stop"),
        Self::Native("gallery.unlock"),
        Self::Native("input.simple"),
        Self::Native("camera.bind"),
        Self::Native("camera.unbind"),
        Self::Native("sprite.offset"),
        Self::Native("sprite.transform"),
        Self::Native("background.transform"),
        Self::Native("sprite.filter"),
        Self::Native("sprite.animate"),
        Self::Native("sprite.transition"),
        Self::Native("se.loop"),
        Self::Native("se.stop"),
        Self::Native("video.play"),
        Self::Native("screen.curtain.show"),
        Self::Native("screen.curtain.hide"),
        Self::Native("text.float"),
        Self::Native("scene.parallax"),
        Self::Native("particle.show"),
        Self::Native("ui.message"),
        Self::Native("text.intro"),
        Self::Native("sprite.sequence"),
        Self::Native("sprite.sequence.timed"),
        Self::Native("sprite.select"),
        Self::Native("sprite.keyframes"),
        Self::Native("assets.loading"),
        Self::Native("input.request"),
        Self::Native("text.paragraph.style"),
        Self::Native("sprite.update"),
        Self::Native("camera.effect"),
        Self::Native("camera.effect.v2"),
        Self::Native("stage.mask.show"),
        Self::Native("stage.mask.hide"),
        Self::Native("sprite.select.when"),
        Self::Native("stage.animate"),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Narration => "Narration",
            Self::Dialogue => "Dialogue",
            Self::Background => "Background",
            Self::Figure => "Figure",
            Self::Choice => "Choice",
            Self::Conditional => "If",
            Self::Loop => "Loop",
            Self::Variable => "Variable",
            Self::Goto => "Goto",
            Self::Call => "Call",
            Self::Wait => "Wait",
            Self::Hide => "Hide",
            Self::Move => "Move",
            Self::CameraMove => "Camera move",
            Self::CameraShake => "Camera shake",
            Self::SpriteFocusRule => "Focus rule",
            Self::SpriteFocus => "Focus speaker",
            Self::Bgm => "BGM",
            Self::Effect => "Sound",
            Self::Video => "Video",
            Self::Return => "Return",
            Self::Native(name) => match name {
                "story.end" => "End story",
                "avatar.show" => "Avatar",
                "avatar.hide" => "Hide avatar",
                "vocal.play" => "Vocal",
                "vocal.stop" => "Stop vocal",
                "screen.film" => "Film bars",
                "text.box" => "Textbox",
                "wait.advance" => "Wait for input",
                "playback.auto" => "Autoplay",
                "ui.show" => "Open UI",
                "ui.hide" => "Close UI",
                "particle.layers.clear" => "Clear particle layers",
                "text.presentation" => "Text presentation",
                "text.retract" => "Retract text",
                "text.float.hide" => "Hide floating text",
                "text.float.configure" => "Floating text lifetime",
                "text.style" => "Dialogue style",
                "scene.parallax.stop" => "Stop parallax",
                "particle.hide" => "Hide particles",
                "video.stop" => "Stop video",
                "gallery.unlock" => "Unlock gallery",
                "input.simple" => "Input",
                "camera.bind" => "Bind camera",
                "camera.unbind" => "Unbind camera",
                "sprite.offset" => "Sprite offset",
                "sprite.transform" => "Sprite transform",
                "background.transform" => "Background transform",
                "sprite.filter" => "Sprite filter",
                "sprite.animate" => "Animate sprite",
                "sprite.transition" => "Sprite transition",
                "se.loop" => "Loop sound",
                "se.stop" => "Stop sound",
                "video.play" => "Play video",
                "screen.curtain.show" => "Show curtain",
                "screen.curtain.hide" => "Hide curtain",
                "text.float" => "Floating text",
                "scene.parallax" => "Scene parallax",
                "particle.show" => "Show particles",
                "ui.message" => "System message",
                "text.intro" => "Intro pages",
                "sprite.sequence" => "Sprite sequence",
                "sprite.sequence.timed" => "Timed sprite sequence",
                "sprite.select" => "Select sprite image",
                "sprite.keyframes" => "Sprite keyframes",
                "assets.loading" => "Prepare assets",
                "input.request" => "Input request",
                "text.paragraph.style" => "Paragraph style",
                "sprite.update" => "Update sprite",
                "camera.effect" => "Camera effect",
                "camera.effect.v2" => "Camera effect V2",
                "stage.mask.show" => "Show stage mask",
                "stage.mask.hide" => "Hide stage mask",
                "sprite.select.when" => "Select sprite by condition",
                "stage.animate" => "Stage animation",
                _ => "Command",
            },
        }
    }

    pub fn for_command(name: &str) -> Option<Self> {
        Some(match name {
            "background" => Self::Background,
            "sprite" => Self::Figure,
            "hide" => Self::Hide,
            "move" => Self::Move,
            "bgm" => Self::Bgm,
            "se" => Self::Effect,
            "video" => Self::Video,
            "goto" => Self::Goto,
            "call" => Self::Call,
            "wait" => Self::Wait,
            "camera.move" => Self::CameraMove,
            "camera.shake" => Self::CameraShake,
            "sprite.focus.configure" => Self::SpriteFocusRule,
            "sprite.focus" => Self::SpriteFocus,
            _ => {
                return Self::ALL
                    .iter()
                    .copied()
                    .find(|kind| matches!(kind, Self::Native(command) if *command == name));
            }
        })
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::Narration | Self::Dialogue => "Text",
            Self::Background
            | Self::Figure
            | Self::Hide
            | Self::Move
            | Self::CameraMove
            | Self::CameraShake
            | Self::SpriteFocusRule
            | Self::SpriteFocus => "Scene",
            Self::Bgm | Self::Effect | Self::Video => "Media",
            Self::Choice
            | Self::Conditional
            | Self::Loop
            | Self::Goto
            | Self::Call
            | Self::Return
            | Self::Wait => "Flow",
            Self::Variable => "Data",
            Self::Native(name) => match name {
                "avatar.show"
                | "avatar.hide"
                | "screen.film"
                | "particle.layers.clear"
                | "scene.parallax.stop"
                | "particle.hide"
                | "camera.bind"
                | "camera.unbind"
                | "sprite.offset"
                | "sprite.transform"
                | "background.transform"
                | "sprite.filter"
                | "sprite.animate"
                | "sprite.transition"
                | "screen.curtain.show"
                | "screen.curtain.hide"
                | "scene.parallax"
                | "particle.show" => "Scene",
                "sprite.sequence"
                | "sprite.sequence.timed"
                | "sprite.select"
                | "sprite.select.when"
                | "stage.animate"
                | "sprite.keyframes"
                | "assets.loading"
                | "sprite.update" => "Scene",
                "camera.effect" | "camera.effect.v2" | "stage.mask.show" | "stage.mask.hide" => {
                    "Scene"
                }
                "vocal.play" | "vocal.stop" | "video.stop" | "se.loop" | "se.stop"
                | "video.play" => "Media",
                "text.box"
                | "text.presentation"
                | "text.retract"
                | "text.float.hide"
                | "text.float.configure"
                | "text.style"
                | "text.float"
                | "text.intro"
                | "text.paragraph.style" => "Text",
                _ => "Flow",
            },
        }
    }

    pub fn search_terms(self) -> &'static str {
        match self {
            Self::Native("text.retract") => "retract text backspace erase tail prefix 退格 回删",
            Self::Narration => "narration text narrator",
            Self::Dialogue => "dialogue speaker character text",
            Self::Background => "background scene image",
            Self::Figure => "figure sprite character show",
            Self::Choice => "choice branch option",
            Self::Conditional => "if conditional branch",
            Self::Loop => "loop repeat",
            Self::Variable => "variable let data",
            Self::Goto => "goto scene jump",
            Self::Call => "call scene",
            Self::Wait => "wait delay time",
            Self::Hide => "hide sprite figure",
            Self::Move => "move sprite figure position",
            Self::CameraMove => "camera move scene characters transform",
            Self::CameraShake => "camera shake scene characters",
            Self::SpriteFocusRule => "sprite focus configure portrait characters",
            Self::SpriteFocus => "sprite focus speaker portrait",
            Self::Bgm => "bgm music audio",
            Self::Effect => "sound effect se audio",
            Self::Video => "video movie",
            Self::Return => "return flow",
            Self::Native(name) => name,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthoringEditError {
    InvalidIdentifier,
    DuplicateIdentifier,
    EmptyName,
    InvalidColor,
    MissingInsertionPoint,
    InvalidManifest,
    UnsupportedDynamicText,
    StaleRange,
}

impl fmt::Display for AuthoringEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier => formatter.write_str("identifier is invalid"),
            Self::DuplicateIdentifier => formatter.write_str("identifier is already used"),
            Self::EmptyName => formatter.write_str("display name cannot be empty"),
            Self::InvalidColor => formatter.write_str("color must use #RRGGBB"),
            Self::MissingInsertionPoint => formatter.write_str("no stable insertion point exists"),
            Self::InvalidManifest => formatter.write_str("manifest is not valid Eiyashou YAML"),
            Self::UnsupportedDynamicText => {
                formatter.write_str("dynamic dialogue must be edited in Text view")
            }
            Self::StaleRange => formatter.write_str("source changed; refresh this view"),
        }
    }
}

impl std::error::Error for AuthoringEditError {}

pub fn replace_dialogue_text(
    source: &str,
    dialogue: &DialogueEntry,
    text: &str,
) -> Result<String, AuthoringEditError> {
    if !dialogue.editable {
        return Err(AuthoringEditError::UnsupportedDynamicText);
    }
    if source.get(dialogue.source_range.clone()).is_none()
        || source.get(dialogue.text_range.clone()).is_none()
    {
        return Err(AuthoringEditError::StaleRange);
    }
    let mut edited = source.to_owned();
    edited.replace_range(dialogue.text_range.clone(), &escape_eiyashou_string(text));
    Ok(edited)
}

pub fn dialogues_for_source(path: &Path, source: &str) -> Vec<DialogueEntry> {
    let mut index = AuthoringIndex::default();
    index_source(
        path,
        source,
        &mut index,
        &mut HashMap::new(),
        &HashSet::new(),
    );
    // The runtime parser omits empty speech, but the editor must keep those
    // source-backed blocks editable so Enter can continue a blank sequence.
    if source.contains("\"\"")
        && path
            .extension()
            .is_some_and(|extension| extension == "shou")
    {
        for scene in EiyashouProjection::parse(source).scenes {
            for block in scene.blocks {
                let Some(range) = block.text_range.clone() else {
                    continue;
                };
                if range.start != range.end || block.read_only {
                    continue;
                }
                let speaker = match block.kind {
                    BlockKind::Narration => String::new(),
                    BlockKind::Dialogue { speaker } => speaker,
                    _ => continue,
                };
                if index
                    .dialogues
                    .iter()
                    .any(|dialogue| dialogue.text_range == range)
                {
                    continue;
                }
                index.dialogues.push(DialogueEntry {
                    path: path.to_owned(),
                    scene: scene.name.clone(),
                    speaker,
                    text: String::new(),
                    editable: true,
                    line: block.line + 1,
                    column: block.column + 1,
                    source_range: block.source_range,
                    text_range: range,
                });
            }
        }
        index
            .dialogues
            .sort_by_key(|dialogue| dialogue.source_range.start);
    }
    index.dialogues
}

pub fn insert_statement(
    source: &str,
    line: usize,
    kind: InsertKind,
    index: &AuthoringIndex,
) -> Result<String, AuthoringEditError> {
    let line_start =
        line_start_offset(source, line).ok_or(AuthoringEditError::MissingInsertionPoint)?;
    let line_end = source[line_start..]
        .find('\n')
        .map(|offset| line_start + offset + 1)
        .unwrap_or(source.len());
    let content_end = line_end.saturating_sub(usize::from(
        line_end > line_start && source.as_bytes().get(line_end - 1) == Some(&b'\n'),
    ));
    let current = source[line_start..content_end].trim();
    if current.starts_with('}') {
        return Err(AuthoringEditError::MissingInsertionPoint);
    }
    let next = source[line_end..].trim_start();
    let statement_has_follower = !next.is_empty() && !next.starts_with('}');
    let current_needs_separator = !current.is_empty()
        && !current.ends_with('{')
        && !current.ends_with(',')
        && !current.ends_with('}');
    let indent = source[line_start..]
        .chars()
        .take_while(|character| character.is_whitespace() && *character != '\n')
        .collect::<String>();
    let statement_indent = if indent.is_empty() {
        "  ".to_owned()
    } else {
        indent
    };
    let statement = insertion_statement(source, kind, index, &statement_indent)?;
    let mut edited = source.to_owned();
    let mut insertion = line_end;
    if current_needs_separator {
        edited.insert(content_end, ',');
        insertion += 1;
    }
    let trailing = if statement_has_follower { "," } else { "" };
    edited.insert_str(
        insertion,
        &format!("{statement_indent}{statement}{trailing}\n"),
    );
    Ok(edited)
}

pub fn insertion_statement(
    source: &str,
    kind: InsertKind,
    index: &AuthoringIndex,
    statement_indent: &str,
) -> Result<String, AuthoringEditError> {
    let first_character = index.characters.first().map(|entry| entry.id.as_str());
    let first_background = index
        .assets
        .iter()
        .find(|entry| entry.kind == AssetKind::Background)
        .map(|entry| entry.id.as_str());
    let first_figure = index
        .assets
        .iter()
        .find(|entry| entry.kind == AssetKind::Figure)
        .map(|entry| entry.id.as_str());
    let first_scene = index.scenes.first().map(|entry| entry.name.as_str());
    let statement = match kind {
        InsertKind::Narration => "\"New narration\"".to_owned(),
        InsertKind::Dialogue => format!(
            "{}: \"New dialogue\"",
            first_character.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Background => format!(
            "background({})",
            first_background.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Figure => {
            let figure = first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!("sprite({figure}_slot, {figure}, position: center)")
        }
        InsertKind::Choice => {
            let scene = first_scene.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!(
                "choice {{\n{statement_indent}  \"Continue\": goto({scene})\n{statement_indent}}}"
            )
        }
        InsertKind::Conditional => {
            format!("if (true) {{\n{statement_indent}  \"New narration\"\n{statement_indent}}}")
        }
        InsertKind::Loop => "loop { break }".to_owned(),
        InsertKind::Variable => format!("let {} = 0", unique_variable_name(source)),
        InsertKind::Goto => format!(
            "goto({})",
            first_scene.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Call => format!(
            "call({})",
            first_scene.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Wait => "wait(500ms)".to_owned(),
        InsertKind::Hide => {
            let figure = first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!("hide({figure}_slot)")
        }
        InsertKind::Move => {
            let figure = first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!("move({figure}_slot, center)")
        }
        InsertKind::CameraMove => "camera.move(scene, x: 0, y: 0, duration: 300ms)".to_owned(),
        InsertKind::CameraShake => {
            "camera.shake(scene, amplitude: 8, frequency: 12, duration: 300ms)".to_owned()
        }
        InsertKind::SpriteFocusRule => {
            let figure = first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!(
                "sprite.focus.configure(characters: [{figure}_slot], speaking: style(), others: style(brightness: 0.7), narration: style(), duration: 300ms)"
            )
        }
        InsertKind::SpriteFocus => "sprite.focus(none)".to_owned(),
        InsertKind::Bgm => format!(
            "bgm({})",
            index
                .assets
                .iter()
                .find(|entry| entry.kind == AssetKind::Bgm)
                .map(|entry| entry.id.as_str())
                .ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Effect => format!(
            "se({})",
            index
                .assets
                .iter()
                .find(|entry| entry.kind == AssetKind::Effect)
                .map(|entry| entry.id.as_str())
                .ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Video => format!(
            "video({})",
            index
                .assets
                .iter()
                .find(|entry| entry.kind == AssetKind::Video)
                .map(|entry| entry.id.as_str())
                .ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Return => "return".to_owned(),
        InsertKind::Native(name) => match name {
            "story.end" => "story.end()".to_owned(),
            "avatar.show" => format!(
                "avatar.show({})",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "avatar.hide" => "avatar.hide()".to_owned(),
            "vocal.play" => format!(
                "vocal.play({})",
                index
                    .assets
                    .iter()
                    .find(|entry| entry.kind == AssetKind::Voice)
                    .map(|entry| entry.id.as_str())
                    .ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "vocal.stop" => "vocal.stop()".to_owned(),
            "screen.film" => "screen.film(true)".to_owned(),
            "text.box" => "text.box(visible: true, auto: false)".to_owned(),
            "wait.advance" => "wait.advance()".to_owned(),
            "playback.auto" => "playback.auto(true)".to_owned(),
            "ui.show" => "ui.show(save)".to_owned(),
            "ui.hide" => "ui.hide(save)".to_owned(),
            "particle.layers.clear" => "particle.layers.clear()".to_owned(),
            "text.presentation" => "text.presentation(paragraph)".to_owned(),
            "text.retract" => "text.retract(source: \"\", keep: \"\")".to_owned(),
            "text.float.hide" => "text.float.hide()".to_owned(),
            "text.float.configure" => "text.float.configure(infinite: false)".to_owned(),
            "text.style" => "text.style(default)".to_owned(),
            "scene.parallax.stop" => "scene.parallax.stop()".to_owned(),
            "particle.hide" => "particle.hide(*)".to_owned(),
            "video.stop" => "video.stop(*)".to_owned(),
            "gallery.unlock" => format!(
                "gallery.unlock(cg, {}, name: \"Artwork\")",
                first_background.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "input.simple" => format!(
                "input.simple({}, title: \"Name\", button: \"OK\")",
                unique_variable_name(source)
            ),
            "camera.bind" | "camera.unbind" => format!(
                "{name}({}_slot, distance: 1.5)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.offset" => format!(
                "sprite.offset({}_slot, x: 0, y: 0)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.transform" => format!(
                "sprite.transform({}_slot, alpha: 1)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "background.transform" => "background.transform(alpha: 1)".to_owned(),
            "sprite.filter" => format!(
                "sprite.filter({}_slot, brightness: 1)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.animate" => format!(
                "sprite.animate({}_slot, shake, duration: 300ms)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.transition" => format!(
                "sprite.transition({}_slot, enter: enter, duration: 300ms)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "se.loop" => format!(
                "se.loop(ambient, {})",
                index
                    .assets
                    .iter()
                    .find(|entry| entry.kind == AssetKind::Effect)
                    .map(|entry| entry.id.as_str())
                    .ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "se.stop" => "se.stop(*)".to_owned(),
            "video.play" => format!(
                "video.play(cutscene, {})",
                index
                    .assets
                    .iter()
                    .find(|entry| entry.kind == AssetKind::Video)
                    .map(|entry| entry.id.as_str())
                    .ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "screen.curtain.show" => "screen.curtain.show(color: rgba(0, 0, 0, 1), duration: 300ms)".to_owned(),
            "screen.curtain.hide" => "screen.curtain.hide(color: rgba(0, 0, 0, 1), duration: 300ms)".to_owned(),
            "text.float" => "text.float(\"New text\", x: 960, y: 540)".to_owned(),
            "scene.parallax" => "scene.parallax(amplitude_percent: 4, scale: 1.08)".to_owned(),
            "particle.show" => "particle.show(sparkles, sparkles)".to_owned(),
            "ui.message" => "ui.message(alert, title: \"Notice\", message: \"Message\", confirm_text: \"OK\", cancel_text: \"Cancel\")".to_owned(),
            "text.intro" => "text.intro(hold: true) { page(\"New page\") }".to_owned(),
            "sprite.sequence" => format!(
                "sprite.sequence({}_slot, fps: 12, loop: true) {{ frame({}) }}",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.sequence.timed" => format!(
                "sprite.sequence.timed({}_slot, loop: true) {{ frame({}, duration: 120ms) }}",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.select" => format!(
                "sprite.select({}_slot, mood, default: {}) {{ case(\"happy\", {}) }}",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.keyframes" => format!(
                "sprite.keyframes({}_slot, repeat: 0, blocking: true) {{ frame(x: 0, duration: 300ms) }}",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "assets.loading" => format!(
                "assets.loading(mode: auto, lookahead: 20, blocking: false) {{ resource({}, kind: background) }}",
                first_background.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "input.request" => format!(
                "input.request({}, type: string, title: \"Name\")",
                unique_variable_name(source)
            ),
            "text.paragraph.style" => "text.paragraph.style(literary, typewriter_speed: 0.03)".to_owned(),
            "sprite.update" => format!(
                "sprite.update({}_slot, {}, position: center, scale: 1)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "camera.effect" => "camera.effect(scene, bloom_intensity: 0.4, duration: 300ms)".to_owned(),
            "camera.effect.v2" => concat!(
                "camera.effect.v2(scene, mirror_shatter_intensity: 0, mirror_shatter_center_x: 0.5, ",
                "mirror_shatter_center_y: 0.5, mirror_shatter_spread: 1, mirror_shatter_seed: 0, ",
                "speed_lines_intensity: 0, speed_lines_radial: true, speed_lines_density: 0.55, ",
                "speed_lines_angle: 0, speed_lines_speed: 0, speed_lines_center_x: 0.5, ",
                "speed_lines_center_y: 0.5, speed_lines_region_ellipse: false, ",
                "speed_lines_region_x: 0.5, speed_lines_region_y: 0.5, ",
                "speed_lines_region_width: 1, speed_lines_region_height: 1, ",
                "speed_lines_region_feather: 0.05)"
            ).to_owned(),
            "stage.mask.show" => "stage.mask.show(overlay, shape: rectangle, opacity: 0.5)".to_owned(),
            "stage.mask.hide" => "stage.mask.hide(overlay)".to_owned(),
            "sprite.select.when" => format!(
                "sprite.select.when({}_slot, default: {}) {{ case(true, {}) }}",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "stage.animate" => "stage.animate(opening, duration: 1s) { track(camera, x) { key(time: 0ms, value: 0), key(time: 1s, value: 20) } }".to_owned(),
            _ => return Err(AuthoringEditError::MissingInsertionPoint),
        },
    };
    Ok(statement)
}

pub fn append_scene(source: &str, id: &str) -> Result<String, AuthoringEditError> {
    if !valid_identifier(id) {
        return Err(AuthoringEditError::InvalidIdentifier);
    }
    if EiyashouProjection::parse(source)
        .scenes
        .iter()
        .any(|scene| scene.name == id)
    {
        return Err(AuthoringEditError::DuplicateIdentifier);
    }
    let separator = if source.is_empty() || source.ends_with("\n\n") {
        ""
    } else if source.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    Ok(format!("{source}{separator}scene {id} {{\n}}\n"))
}

pub fn rename_scene(source: &str, start: usize, id: &str) -> Result<String, AuthoringEditError> {
    if !valid_identifier(id) {
        return Err(AuthoringEditError::InvalidIdentifier);
    }
    let document = parse_native_document(source);
    let scene = document
        .scenes
        .iter()
        .find(|scene| scene.range.start == start)
        .ok_or(AuthoringEditError::StaleRange)?;
    if document
        .scenes
        .iter()
        .any(|other| other.range.start != start && other.name == id)
    {
        return Err(AuthoringEditError::DuplicateIdentifier);
    }
    let mut ranges = scene_references(source, &scene.name);
    ranges.push(scene.name_range.clone());
    ranges.sort_by_key(|range| range.start);
    let mut edited = source.to_owned();
    for range in ranges.into_iter().rev() {
        edited.replace_range(range, id);
    }
    Ok(edited)
}

pub fn rename_scene_references(source: &str, old: &str, new: &str) -> String {
    let mut edited = source.to_owned();
    for range in scene_references(source, old).into_iter().rev() {
        edited.replace_range(range, new);
    }
    edited
}

pub fn delete_scene(source: &str, start: usize) -> Result<String, AuthoringEditError> {
    let scene = parse_native_document(source)
        .scenes
        .into_iter()
        .find(|scene| scene.range.start == start)
        .ok_or(AuthoringEditError::StaleRange)?;
    let mut edited = source.to_owned();
    edited.replace_range(scene.range, "");
    Ok(edited)
}

pub fn move_scene(
    source: &str,
    start: usize,
    direction: crate::projection::MoveDirection,
) -> Result<String, AuthoringEditError> {
    let document = parse_native_document(source);
    let index = document
        .scenes
        .iter()
        .position(|scene| scene.range.start == start)
        .ok_or(AuthoringEditError::StaleRange)?;
    let neighbor = match direction {
        crate::projection::MoveDirection::Up => index.checked_sub(1),
        crate::projection::MoveDirection::Down => {
            (index + 1 < document.scenes.len()).then_some(index + 1)
        }
    }
    .ok_or(AuthoringEditError::MissingInsertionPoint)?;
    let (first, second) = if index < neighbor {
        (&document.scenes[index], &document.scenes[neighbor])
    } else {
        (&document.scenes[neighbor], &document.scenes[index])
    };
    let before = source
        .get(first.range.clone())
        .ok_or(AuthoringEditError::StaleRange)?;
    let between = source
        .get(first.range.end..second.range.start)
        .ok_or(AuthoringEditError::StaleRange)?;
    let after = source
        .get(second.range.clone())
        .ok_or(AuthoringEditError::StaleRange)?;
    let mut edited = source.to_owned();
    edited.replace_range(
        first.range.start..second.range.end,
        &format!("{after}{between}{before}"),
    );
    Ok(edited)
}

pub fn scene_references(source: &str, name: &str) -> Vec<Range<usize>> {
    let document = parse_native_document(source);
    let tokens = document
        .tokens
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                NativeTokenKind::Whitespace | NativeTokenKind::Comment
            )
        })
        .collect::<Vec<_>>();
    tokens
        .windows(4)
        .filter_map(|window| {
            let [command, open, target, close] = window else {
                return None;
            };
            let text = |token: &keine_loader::NativeToken| source.get(token.range.clone());
            ((text(command) == Some("goto") || text(command) == Some("call"))
                && text(open) == Some("(")
                && target.kind == NativeTokenKind::Identifier
                && text(target) == Some(name)
                && text(close) == Some(")"))
            .then(|| target.range.clone())
        })
        .collect()
}

pub fn append_character(
    source: &str,
    id: &str,
    name: &str,
    color: Option<&str>,
) -> Result<String, AuthoringEditError> {
    if !valid_identifier(id) {
        return Err(AuthoringEditError::InvalidIdentifier);
    }
    if name.trim().is_empty() {
        return Err(AuthoringEditError::EmptyName);
    }
    if let Some(color) = color.filter(|value| !value.trim().is_empty())
        && !valid_color(color)
    {
        return Err(AuthoringEditError::InvalidColor);
    }
    let manifest = EiyashouCharacterManifest::from_yaml(source)
        .map_err(|_| AuthoringEditError::InvalidManifest)?;
    if manifest.characters.contains_key(id) {
        return Err(AuthoringEditError::DuplicateIdentifier);
    }
    let mut entry = format!("  {id}:\n    name: \"{}\"\n", escape_yaml_string(name));
    if let Some(color) = color.filter(|value| !value.trim().is_empty()) {
        entry.push_str(&format!("    color: \"{}\"\n", escape_yaml_string(color)));
    }
    let mut edited = source.to_owned();
    if let Some(empty_mapping) = source.find("characters: {}") {
        let range = empty_mapping..empty_mapping + "characters: {}".len();
        edited.replace_range(range, &format!("characters:\n{entry}"));
    } else {
        let insertion =
            character_insertion_offset(source).ok_or(AuthoringEditError::MissingInsertionPoint)?;
        edited.insert_str(insertion, &entry);
    }
    EiyashouCharacterManifest::from_yaml(&edited)
        .map_err(|_| AuthoringEditError::InvalidManifest)?;
    Ok(edited)
}

pub fn escape_eiyashou_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '$' if characters.peek() == Some(&'{') => escaped.push_str("/$"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn unique_variable_name(source: &str) -> String {
    let document = parse_native_document(source);
    let identifiers = document
        .tokens
        .iter()
        .filter(|token| token.kind == NativeTokenKind::Identifier)
        .filter_map(|token| source.get(token.range.clone()))
        .collect::<HashSet<_>>();
    if !identifiers.contains("value") {
        return "value".to_owned();
    }
    (2..)
        .map(|suffix| format!("value_{suffix}"))
        .find(|candidate| !identifiers.contains(candidate.as_str()))
        .expect("an unbounded numeric suffix always yields a unique identifier")
}

fn index_source(
    path: &Path,
    source: &str,
    index: &mut AuthoringIndex,
    referenced: &mut HashMap<(AssetKind, String), usize>,
    asset_lookup: &HashSet<(AssetKind, String)>,
) {
    let document = parse_native_document(source);
    for scene in &document.scenes {
        let (line, _) = line_column(source, scene.name_range.start);
        index.scenes.push(SceneEntry {
            path: path.to_owned(),
            name: scene.name.clone(),
            line,
            name_range: scene.name_range.clone(),
            source_range: scene.range.clone(),
        });
    }
    for diagnostic in &document.diagnostics {
        index.problems.push(problem(
            match diagnostic.level {
                DiagnosticLevel::Warning => ProblemSeverity::Warning,
                DiagnosticLevel::Error => ProblemSeverity::Error,
            },
            path.to_owned(),
            diagnostic.span.line,
            diagnostic.span.column,
            diagnostic.message.clone(),
        ));
    }

    let strings = document
        .tokens
        .iter()
        .filter(|token| token.kind == NativeTokenKind::String)
        .map(|token| {
            let (line, column) = line_column(source, token.range.start);
            (token.range.clone(), line, column)
        })
        .collect::<Vec<_>>();
    let mut used_strings = HashSet::new();
    for (scene_index, parsed) in parse_native_scenes(source).into_iter().enumerate() {
        let scene_name = parsed.name.unwrap_or_else(|| "scene".to_owned());
        let scene_range = document
            .scenes
            .get(scene_index)
            .map(|scene| scene.range.clone())
            .unwrap_or(0..source.len());
        for (action_index, action) in parsed.report.actions.iter().enumerate() {
            let Some(span) = parsed.report.spans.get(action_index) else {
                continue;
            };
            if let Action::EiyashouSay(dialogue) = action
                && let Some((string_index, (range, line, column))) = strings
                    .iter()
                    .enumerate()
                    .find(|(token_index, (_, token_line, _))| {
                        *token_line == span.line && !used_strings.contains(token_index)
                    })
                    .or_else(|| {
                        strings
                            .iter()
                            .enumerate()
                            .find(|(token_index, (range, token_line, _))| {
                                scene_range.contains(&range.start)
                                    && *token_line >= span.line
                                    && !used_strings.contains(token_index)
                            })
                    })
            {
                used_strings.insert(string_index);
                let raw = source.get(range.clone()).unwrap_or("");
                let inner = range.start.saturating_add(1)..range.end.saturating_sub(1);
                let editable = !raw.contains("${");
                let text = if editable {
                    dialogue
                        .text
                        .parts
                        .iter()
                        .filter_map(|part| match part {
                            EiyashouTextPart::Literal(value) => Some(value.as_str()),
                            EiyashouTextPart::Expression(_) => None,
                        })
                        .collect::<String>()
                } else {
                    source.get(inner.clone()).unwrap_or("").to_owned()
                };
                index.dialogues.push(DialogueEntry {
                    path: path.to_owned(),
                    scene: scene_name.clone(),
                    speaker: dialogue.speaker.clone(),
                    text,
                    editable,
                    line: *line,
                    column: *column,
                    source_range: range.clone(),
                    text_range: inner,
                });
            }
        }
        for diagnostic in parsed.report.diagnostics {
            index.problems.push(problem(
                match diagnostic.level {
                    DiagnosticLevel::Warning => ProblemSeverity::Warning,
                    DiagnosticLevel::Error => ProblemSeverity::Error,
                },
                path.to_owned(),
                diagnostic.span.line,
                diagnostic.span.column,
                diagnostic.message,
            ));
        }
        for resource in parsed.report.resources {
            let Some(kind) = AssetKind::from_resource(resource.kind) else {
                continue;
            };
            if resource.is_dynamic() {
                continue;
            }
            let matches = document
                .tokens
                .iter()
                .filter(|token| {
                    matches!(
                        token.kind,
                        NativeTokenKind::Identifier | NativeTokenKind::String
                    )
                })
                .filter(|token| line_column(source, token.range.start).0 == resource.span.line)
                .filter_map(|token| {
                    let raw = source.get(token.range.clone())?;
                    (raw == resource.path || raw == format!("\"{}\"", resource.path))
                        .then_some(token.range.clone())
                })
                .collect::<Vec<_>>();
            let range = (matches.len() == 1).then(|| matches[0].clone());
            let (line, column) = range
                .as_ref()
                .map(|range| line_column(source, range.start))
                .unwrap_or((resource.span.line, resource.span.column));
            index.asset_references.push(AssetReference {
                key: AssetKey {
                    kind,
                    id: resource.path.clone(),
                },
                path: path.to_owned(),
                line,
                column,
                range,
            });
            *referenced.entry((kind, resource.path.clone())).or_default() += 1;
            if !asset_lookup.contains(&(kind, resource.path.clone())) {
                index.problems.push(problem(
                    ProblemSeverity::Error,
                    path.to_owned(),
                    resource.span.line,
                    resource.span.column,
                    format!("Unknown {} asset `{}`", kind.label(), resource.path),
                ));
            }
        }
    }
}

fn push_assets(
    root: &Path,
    index: &mut AuthoringIndex,
    lookup: &mut HashSet<(AssetKind, String)>,
    kind: AssetKind,
    values: HashMap<String, EiyashouAssetEntry>,
) {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_by(|left, right| left.0.cmp(&right.0));
    let mut physical_files = HashMap::<PathBuf, String>::new();
    for (id, entry) in values {
        let value = entry.path();
        let tags = entry.tags().to_vec();
        lookup.insert((kind, id.clone()));
        let relative = confined_relative(value);
        let existing = relative
            .as_deref()
            .and_then(|path| confined_existing_file(root, path));
        let exists = existing.is_some();
        let path = relative.unwrap_or_else(|| PathBuf::from(value));
        if id.is_empty() {
            index.problems.push(problem(
                ProblemSeverity::Error,
                index
                    .assets_manifest
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("assets.yaml")),
                1,
                1,
                format!("{} asset identifiers must not be empty", kind.label()),
            ));
        }
        let identity = existing.unwrap_or_else(|| path.clone());
        if let Some(previous) = physical_files.insert(identity, id.clone()) {
            index.problems.push(problem(
                ProblemSeverity::Error,
                index
                    .assets_manifest
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("assets.yaml")),
                1,
                1,
                format!(
                    "{} assets `{previous}` and `{id}` map to the same file: {}",
                    kind.label(),
                    path.display()
                ),
            ));
        }
        if !exists {
            index.problems.push(problem(
                ProblemSeverity::Error,
                index
                    .assets_manifest
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("assets.yaml")),
                1,
                1,
                format!(
                    "{} asset `{id}` is missing or escapes the project: {}",
                    kind.label(),
                    path.display()
                ),
            ));
        }
        index.assets.push(AssetEntry {
            kind,
            id,
            path,
            tags,
            exists,
            reference_count: 0,
        });
    }
}

fn read_text(
    root: &Path,
    relative: &Path,
    overrides: &BTreeMap<PathBuf, String>,
) -> Option<String> {
    overrides
        .get(relative)
        .cloned()
        .or_else(|| fs::read_to_string(root.join(relative)).ok())
}

fn confined_relative(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    (!path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_))))
    .then(|| path.to_owned())
}

pub(crate) fn confined_existing_file(root: &Path, relative: &Path) -> Option<PathBuf> {
    let Ok(canonical_root) = root.canonicalize() else {
        return None;
    };
    let Ok(canonical) = root.join(relative).canonicalize() else {
        return None;
    };
    (canonical.starts_with(canonical_root) && canonical.is_file()).then_some(canonical)
}

fn problem(
    severity: ProblemSeverity,
    path: PathBuf,
    line: usize,
    column: usize,
    message: impl Into<String>,
) -> AuthoringProblem {
    AuthoringProblem {
        severity,
        path,
        line,
        column,
        message: message.into(),
    }
}

fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = source.get(..offset).unwrap_or(source);
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix.chars().count(), |(_, tail)| tail.chars().count())
        + 1;
    (line, column)
}

fn line_start_offset(source: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return Some(0);
    }
    source
        .match_indices('\n')
        .nth(line.saturating_sub(1))
        .map(|(offset, _)| offset + 1)
}

fn character_insertion_offset(source: &str) -> Option<usize> {
    let mut offset = 0;
    let mut in_characters = false;
    for segment in source.split_inclusive('\n') {
        let trimmed = segment.trim_end_matches(['\r', '\n']);
        if !in_characters {
            if trimmed.trim() == "characters:" && !trimmed.starts_with(char::is_whitespace) {
                in_characters = true;
            }
        } else if !trimmed.is_empty() && !trimmed.starts_with(char::is_whitespace) {
            return Some(offset);
        }
        offset += segment.len();
    }
    in_characters.then_some(source.len())
}

fn escape_yaml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) fn valid_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

fn valid_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn checked_in_native_fixture_populates_editor_views() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/native-smoke");
        let workspace = crate::workspace::WorkspaceSession::open(&root).unwrap();
        let files = workspace.files();
        assert!(
            files
                .iter()
                .any(|file| file.relative_path == Path::new("scripts/main.shou"))
        );
        let index = AuthoringIndex::load(&root, files, &BTreeMap::new());
        assert!(index.native);
        assert_eq!(index.scenes.len(), 1);
        assert!(index.assets.is_empty());
        assert!(index.problems.is_empty(), "{:?}", index.problems);
    }

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "keine-authoring-index-{}-{nonce}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("config.yaml"),
            "adapter:\n  script: keine\nscript:\n  assets: assets.yaml\n  characters: characters.yaml\n",
        )
        .unwrap();
        fs::write(
            root.join("assets.yaml"),
            "backgrounds:\n  room:\n    path: assets/room.webp\n    tags: [interior, chapter-1]\nfigures:\n  rin: assets/missing.webp\n",
        )
        .unwrap();
        fs::write(root.join("assets/room.webp"), b"image").unwrap();
        fs::write(
            root.join("characters.yaml"),
            "characters:\n  rin:\n    name: \"Rin\"\n",
        )
        .unwrap();
        fs::write(
            root.join("scripts/main.shou"),
            "scene start {\n  background(room),\n  rin: \"Hello\",\n}\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn index_is_deterministic_and_reports_confined_missing_assets() {
        let root = fixture();
        let files = vec![WorkspaceFile {
            relative_path: PathBuf::from("scripts/main.shou"),
            size: fs::metadata(root.join("scripts/main.shou")).unwrap().len(),
            kind: crate::workspace::WorkspaceEntryKind::File,
        }];
        let index = AuthoringIndex::load(&root, &files, &BTreeMap::new());
        assert!(index.native);
        assert_eq!(index.scenes[0].name, "start");
        assert_eq!(index.dialogues[0].speaker, "rin");
        assert_eq!(index.assets[0].id, "room");
        assert_eq!(index.assets[0].tags, ["interior", "chapter-1"]);
        assert_eq!(index.assets[0].reference_count, 1);
        let reference = index
            .asset_references
            .iter()
            .find(|reference| reference.key.id == "room")
            .unwrap();
        let source = fs::read_to_string(root.join(&reference.path)).unwrap();
        assert_eq!(source.get(reference.range.clone().unwrap()), Some("room"));
        assert!(
            index
                .problems
                .iter()
                .any(|problem| problem.message.contains("missing.webp"))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn asset_query_keeps_stable_identity_across_sort_and_filter() {
        let assets = [
            AssetEntry {
                kind: AssetKind::Background,
                id: "room".into(),
                path: "assets/background/room.webp".into(),
                tags: vec!["interior".into()],
                exists: true,
                reference_count: 2,
            },
            AssetEntry {
                kind: AssetKind::Figure,
                id: "rin".into(),
                path: "assets/figure/rin.webp".into(),
                tags: vec!["hero".into()],
                exists: true,
                reference_count: 1,
            },
        ];
        let mut query = AssetQuery {
            search: "hero".into(),
            ..Default::default()
        };
        assert_eq!(query.results(&assets)[0].key(), assets[1].key());
        query.search.clear();
        query.sort = AssetSort::References;
        assert_eq!(
            query
                .results(&assets)
                .iter()
                .map(|asset| asset.key())
                .collect::<Vec<_>>(),
            vec![assets[0].key(), assets[1].key()]
        );
        query.kind = Some(AssetKind::Figure);
        assert_eq!(query.results(&assets).len(), 1);
    }

    #[test]
    fn index_reports_same_type_duplicate_files_but_allows_cross_type_sharing() {
        let root = fixture();
        fs::write(
            root.join("assets.yaml"),
            concat!(
                "backgrounds:\n",
                "  first: assets/room.webp\n",
                "  second: assets/room.webp\n",
                "figures:\n",
                "  shared: assets/room.webp\n",
            ),
        )
        .unwrap();
        let index = AuthoringIndex::load(&root, &[], &BTreeMap::new());
        let duplicate = index
            .problems
            .iter()
            .filter(|problem| problem.message.contains("map to the same file"))
            .collect::<Vec<_>>();

        assert_eq!(duplicate.len(), 1);
        assert!(
            duplicate[0]
                .message
                .contains("Background assets `first` and `second`")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dialogue_replacement_is_bounded_and_escapes_literal_interpolation() {
        let source = "scene start {\n  rin: \"Hello\",\n}\n";
        let root = fixture();
        let path = PathBuf::from("scripts/main.shou");
        fs::write(root.join(&path), source).unwrap();
        let files = vec![WorkspaceFile {
            relative_path: path,
            size: source.len() as u64,
            kind: crate::workspace::WorkspaceEntryKind::File,
        }];
        let index = AuthoringIndex::load(&root, &files, &BTreeMap::new());
        let edited = replace_dialogue_text(source, &index.dialogues[0], "你说 \"${x}\"").unwrap();
        assert_eq!(edited, "scene start {\n  rin: \"你说 \\\"/${x}\\\"\",\n}\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn continuous_dialogue_block_is_indexed_in_source_order() {
        let source = concat!(
            "scene start {\n",
            "  rin: {\n",
            "    \"First\",\n",
            "    \"Second\"\n",
            "  }\n",
            "}\n",
        );
        let dialogues = dialogues_for_source(Path::new("scripts/main.shou"), source);
        assert_eq!(
            dialogues
                .iter()
                .map(|dialogue| dialogue.text.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second"]
        );
        assert_eq!(dialogues[0].line, 3);
        assert_eq!(dialogues[1].line, 4);
    }

    #[test]
    fn empty_text_blocks_remain_editable_after_insertion() {
        let source = "scene start {\n  \"\",\n  \"\"\n}\n";
        let dialogues = dialogues_for_source(Path::new("scripts/main.shou"), source);
        assert_eq!(dialogues.len(), 2);
        assert!(dialogues.iter().all(|dialogue| dialogue.editable));
        assert_eq!(dialogues[0].line, 2);
        assert_eq!(dialogues[1].line, 3);
        assert_eq!(
            replace_dialogue_text(source, &dialogues[1], "Next").unwrap(),
            "scene start {\n  \"\",\n  \"Next\"\n}\n"
        );
    }

    #[test]
    fn block_line_break_round_trips_through_source() {
        let source = "scene start {\n  \"Line one\\nLine two\"\n}\n";
        let dialogues = dialogues_for_source(Path::new("scripts/main.shou"), source);
        assert_eq!(dialogues.len(), 1);
        assert_eq!(dialogues[0].text, "Line one\nLine two");
    }

    #[test]
    fn character_and_scene_creation_preserve_surrounding_source() {
        let characters = "characters:\n  rin:\n    name: \"Rin\"\nmetadata: keep\n";
        let edited = append_character(characters, "yui", "Yui", Some("#BAEBFF")).unwrap();
        assert!(
            edited.contains("  yui:\n    name: \"Yui\"\n    color: \"#BAEBFF\"\nmetadata: keep")
        );
        let script = "// keep\nscene start { \"Hi\" }\n";
        let edited = append_scene(script, "next").unwrap();
        assert!(edited.starts_with(script));
        assert!(edited.ends_with("scene next {\n}\n"));
        let projected = EiyashouProjection::parse(&edited);
        assert_eq!(projected.scenes.len(), 2);
        assert!(projected.read_only.is_empty());
    }

    #[test]
    fn scene_edits_preserve_other_source_and_identify_real_targets() {
        let source =
            "// keep\nscene first { goto(second) }\n\n// between\nscene second { \"Hello\" }\n";
        let parsed = parse_native_document(source);
        let first = parsed.scenes[0].range.start;
        let second = parsed.scenes[1].range.start;
        let renamed = rename_scene(source, second, "ending").unwrap();
        assert!(renamed.contains("scene ending { \"Hello\" }"));
        assert!(renamed.contains("goto(ending)"));
        assert_eq!(scene_references(source, "second").len(), 1);
        assert!(scene_references("scene x { \"goto(second)\" }", "second").is_empty());
        let moved = move_scene(source, second, crate::projection::MoveDirection::Up).unwrap();
        assert!(moved.find("scene second").unwrap() < moved.find("scene first").unwrap());
        assert!(moved.contains("// between"));
        let deleted = delete_scene(source, first).unwrap();
        assert!(!deleted.contains("scene first"));
        assert!(deleted.contains("scene second"));
        let self_ref = "scene second { call(second) }";
        let renamed = rename_scene(self_ref, 0, "longer_ending").unwrap();
        assert_eq!(renamed, "scene longer_ending { call(longer_ending) }");
    }

    #[test]
    fn palette_inserts_after_a_stable_line_boundary() {
        let source = "scene start {\n  \"First\",\n}\n";
        let mut index = AuthoringIndex::default();
        index.characters.push(CharacterEntry {
            id: "rin".into(),
            name: "Rin".into(),
            color: None,
        });
        let edited = insert_statement(source, 1, InsertKind::Dialogue, &index).unwrap();
        assert_eq!(
            edited,
            "scene start {\n  \"First\",\n  rin: \"New dialogue\"\n}\n"
        );

        index.assets.push(AssetEntry {
            kind: AssetKind::Background,
            id: "room".into(),
            path: "assets/room.webp".into(),
            tags: Vec::new(),
            exists: true,
            reference_count: 0,
        });
        index.assets.push(AssetEntry {
            kind: AssetKind::Figure,
            id: "rin_smile".into(),
            path: "assets/rin.webp".into(),
            tags: Vec::new(),
            exists: true,
            reference_count: 0,
        });
        index.scenes.push(SceneEntry {
            path: "scripts/main.shou".into(),
            name: "start".into(),
            line: 1,
            name_range: 6..11,
            source_range: 0..source.len(),
        });
        for kind in [
            InsertKind::Narration,
            InsertKind::Background,
            InsertKind::Figure,
            InsertKind::Choice,
        ] {
            let edited = insert_statement(source, 1, kind, &index).unwrap();
            let diagnostics = parse_native_document(&edited).diagnostics;
            assert!(
                diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.level != DiagnosticLevel::Error),
                "{kind:?}: {diagnostics:?}\n{edited}"
            );
        }
    }

    #[test]
    fn v11_palette_templates_parse_and_project_as_blocks() {
        let source = "scene start {\n  \"Ready\"\n}\n";
        let mut index = AuthoringIndex::default();
        for (kind, id) in [
            (AssetKind::Background, "room"),
            (AssetKind::Figure, "hero"),
            (AssetKind::Voice, "voice"),
            (AssetKind::Effect, "sound"),
            (AssetKind::Video, "movie"),
        ] {
            index.assets.push(AssetEntry {
                kind,
                id: id.into(),
                path: format!("assets/{id}").into(),
                tags: Vec::new(),
                exists: true,
                reference_count: 0,
            });
        }
        for kind in InsertKind::ALL {
            if !matches!(kind, InsertKind::Native(_)) {
                continue;
            }
            let edited = insert_statement(source, 1, kind, &index).unwrap();
            let document = parse_native_document(&edited);
            assert!(
                document
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.level != DiagnosticLevel::Error),
                "{kind:?}: {:?}\n{edited}",
                document.diagnostics
            );
            let projection = EiyashouProjection::parse(&edited);
            assert!(
                projection.scenes[0]
                    .blocks
                    .iter()
                    .any(|block| block.kind == BlockKind::Command && !block.read_only)
            );
        }
    }

    #[test]
    fn rejects_escape_paths_and_duplicate_identifiers() {
        assert!(confined_relative("../outside").is_none());
        assert_eq!(
            append_scene("scene start {}", "start"),
            Err(AuthoringEditError::DuplicateIdentifier)
        );
        assert_eq!(
            append_character("characters: {}\n", "not valid", "Name", None),
            Err(AuthoringEditError::InvalidIdentifier)
        );
        assert_eq!(
            append_character("characters: {}\n", "rin", "", None),
            Err(AuthoringEditError::EmptyName)
        );
        assert_eq!(
            append_character("characters: {}\n", "rin", "Rin", Some("blue")),
            Err(AuthoringEditError::InvalidColor)
        );
        assert!(
            append_character("characters: {}\n", "rin", "Rin", Some("#BAEBFF"))
                .unwrap()
                .contains("characters:\n  rin:")
        );
    }
}
