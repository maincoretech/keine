use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use keine_core::action::Choice;
use keine_core::config::{
    AdapterConfig, AssetSourceConfig, GameConfig, ProjectMetadata, TextRevealConfig,
};
use keine_core::{
    Action, Anchor, AssetHint, AssetHintKind, BlendMode, CameraShakeAxis, CameraShakeFalloff,
    CameraShakeRandomness, CameraShakeSpec, CameraShakeTweenSpec, CameraTargets, CameraTweenField,
    CameraTweenSpec, ChoiceTarget, ColorToneMode, Easing, InputValueType, LoadingStrategy,
    LoadingStrategyMode, PortraitStyle, Position, PostProcessPatch, PostProcessV2, SayOptions,
    SceneFit, SceneLayerLayout, SceneMouseParallax, SpriteLayout, SpriteTransform, StageAnimation,
    StageAudioCue, StageAudioKind, StageEvent, StageEventKind, StageKeyframe, StageMask,
    StageMaskFillMode, StageMaskFit, StageMaskImageChannel, StageMaskMode, StageMaskPlane,
    StageMaskScope, StageMaskShape, StageMaskTextureBlend, StageMaskVisibility, StageProperty,
    StageSceneCue, StageSceneLayer, StageTarget, StageTrack, SystemMessageMode, SystemMessageSpec,
    SystemUiSlot, TransformKeyframe, TransformPatch, Transition, UserInputSpec, VideoMode,
    VideoSpec,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::ProjectSourceReader;
use super::model::{
    AssetManifest, ChapterDocument, CharacterDefinition, CharactersDocument, DialogueBehavior,
    ProjectDocument, SceneDefinition, ScenesDocument, ScheduleCondition, ScheduleGraph,
    ScheduleNode, StoryBlock, StoryFragment, VariableDeclaration, VariablesDocument,
};
use crate::{Diagnostic, DiagnosticLevel, LoadedScene, ParseReport, SourceSpan};

pub(super) fn initial_state(
    variables: &VariablesDocument,
    characters: &CharactersDocument,
) -> crate::ProjectInitialState {
    let mut state = crate::ProjectInitialState::default();
    for declaration in variables
        .variables
        .iter()
        .filter(|declaration| declaration.kind != "system")
    {
        let Some(value) = declared_value(declaration) else {
            continue;
        };
        if declaration.persistence == "shared" {
            state
                .shared_variables
                .insert(declaration.name.clone(), value);
        } else if declaration.persistence == "session" {
            state
                .session_variables
                .insert(declaration.name.clone(), value);
        } else {
            state.variables.insert(declaration.name.clone(), value);
        }
    }
    for character in &characters.characters {
        for attribute in &characters.attribute_template {
            let value = character
                .attribute_values
                .get(&attribute.name)
                .unwrap_or(&attribute.default_value);
            if let Some(value) = core_value(value) {
                state
                    .variables
                    .insert(format!("{}.{}", character.id, attribute.name), value);
            }
        }
    }
    state
}

fn declared_value(declaration: &VariableDeclaration) -> Option<keine_core::Value> {
    if !declaration.default_value.is_null() {
        return core_value(&declaration.default_value);
    }
    Some(match declaration.value_type.as_str() {
        "number" => keine_core::Value::Int(0),
        "bool" | "boolean" => keine_core::Value::Bool(false),
        "string" => keine_core::Value::Str(String::new()),
        _ => return None,
    })
}

fn core_value(value: &Value) -> Option<keine_core::Value> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .map(keine_core::Value::Int)
            .or_else(|| number.as_f64().map(keine_core::Value::Float)),
        Value::String(value) => Some(keine_core::Value::Str(value.clone())),
        Value::Bool(value) => Some(keine_core::Value::Bool(*value)),
        Value::Array(values) => values
            .iter()
            .map(core_value)
            .collect::<Option<Vec<_>>>()
            .map(keine_core::Value::Array),
        Value::Null | Value::Object(_) => None,
    }
}

/// Runtime-facing block registry documented by LetsGal Studio 2.0.
/// schema. `cmdDraft` is editor-only and therefore intentionally not in
/// this compatibility contract.
pub(super) const BUILTIN_BLOCK_TYPES: &[&str] = &[
    "animateSprite",
    "branch",
    "callExtensionFunction",
    "callFragment",
    "camera",
    "comment",
    "curtain",
    "destroyScene",
    "dialogue",
    "endChapter",
    "enterAutoPlay",
    "exitAutoPlay",
    "floatingText",
    "hideFloatingText",
    "hideExtensionUI",
    "if",
    "loadingStrategy",
    "narration",
    "openExternalUrl",
    "particle",
    "playerInput",
    "portraitStyleRule",
    "removeCharacter",
    "resetCamera",
    "returnToEntry",
    "scene",
    "setver",
    "showCharacter",
    "showExtensionUI",
    "sound",
    "stageAnimation",
    "stageMask",
    "stopSound",
    "stopVideo",
    "storyParagraph",
    "switchDialogueStyle",
    "switchParagraphStyle",
    "systemMessage",
    "steamAction",
    "unlockSteamAchievement",
    "updateCharacter",
    "video",
    "wait",
];

pub(super) fn game_config(
    project: &ProjectDocument,
    manifest: &AssetManifest,
    dialogue_behavior: Option<&DialogueBehavior>,
) -> GameConfig {
    let mut config = GameConfig {
        title: project.name.clone(),
        project: ProjectMetadata {
            id: shipping_project_id(project),
            description: project.description.clone().unwrap_or_default(),
            ..ProjectMetadata::default()
        },
        features: project.keine.features.clone(),
        adapter: AdapterConfig {
            asset: vec![AssetSourceConfig {
                path: "assets".into(),
                format: "fs".into(),
            }],
            script: "webgal".into(),
            store: "keine".into(),
        },
        ..GameConfig::default()
    };
    // Studio positions full-canvas sprites in its 1920x1080 design space.
    // A 1080px baseline preserves those authored proportions in keine. Its
    // scene-layer origin is the canvas edge, so the character-oriented inset
    // used by native keine projects must not shift imported layers.
    config.layout.sprite_height = keine_core::DESIGN_HEIGHT;
    config.layout.anchor_offset = 0.0;
    if let Some(behavior) = dialogue_behavior {
        config.styles.typewriter_speed = if behavior.text_speed <= f64::EPSILON {
            120.0
        } else {
            (1000.0 / behavior.text_speed).clamp(10.0, 120.0)
        };
        let parameters = &behavior.text_reveal_parameters;
        config.styles.text_reveal = TextRevealConfig {
            duration: (behavior.char_fade_in_duration.max(0.0) / 1000.0).min(1.0),
            effect: behavior.text_reveal_effect,
            distance: parameters.distance_px.clamp(0.0, 48.0),
            scale: (parameters.scale_percent.clamp(30.0, 100.0) / 100.0),
            rotation: parameters.rotation_degrees.clamp(0.0, 180.0),
            blur: parameters.blur_px.clamp(0.0, 12.0),
        };
    }

    let mut first_background = None;
    for (hash, entry) in &manifest.entries {
        let path = entry.path.replace('\\', "/");
        if is_background(&path) {
            first_background.get_or_insert_with(|| path.clone());
            insert_aliases(&mut config.assets.backgrounds, hash, &path);
            // LetsGal scenes can compose several background assets as layers.
            // Scene layers use the sprite renderer. Keep source background
            // metadata distinct from that drawing command for migration.
            insert_aliases(&mut config.assets.figures, hash, &path);
        } else if is_figure(&path) {
            insert_aliases(&mut config.assets.figures, hash, &path);
        } else if is_bgm(&path) {
            insert_aliases(&mut config.assets.bgm, hash, &path);
        } else if is_voice(&path) {
            insert_aliases(&mut config.assets.voices, hash, &path);
        } else if is_effect(&path) {
            insert_aliases(&mut config.assets.effects, hash, &path);
        } else if normalized_head(&path).eq_ignore_ascii_case("audio") {
            // LetsGal projects commonly keep every audio class in one generic
            // directory. The authored action still supplies the semantic
            // BGM/voice/effect class, so make the same canonical path
            // resolvable through each class-specific runtime map.
            insert_aliases(&mut config.assets.bgm, hash, &path);
            insert_aliases(&mut config.assets.voices, hash, &path);
            insert_aliases(&mut config.assets.effects, hash, &path);
        } else if is_video(&path) {
            insert_aliases(&mut config.assets.videos, hash, &path);
        } else if is_lut(&path) {
            insert_aliases(&mut config.assets.luts, hash, &path);
            if let Some(stem) = Path::new(&path)
                .file_stem()
                .and_then(|value| value.to_str())
            {
                config.assets.luts.insert(stem.to_owned(), path.clone());
            }
        }
    }
    if let Some(background) = first_background {
        config.title_background = background.clone();
        config
            .assets
            .backgrounds
            .insert(background.clone(), background);
    }
    config
}

pub(super) fn shipping_project_id(project: &ProjectDocument) -> String {
    if !project.keine.project_id.is_empty() {
        return project.keine.project_id.clone();
    }
    let source = ProjectMetadata {
        id: project.id.clone(),
        ..ProjectMetadata::default()
    };
    if source.valid_id().is_some() {
        return project.id.clone();
    }

    const PREFIX: &str = "letsgal-";
    const HASH_BYTES: usize = 8;
    const SEPARATOR_BYTES: usize = 1;
    const MAX_BASE_BYTES: usize = 64 - PREFIX.len() - SEPARATOR_BYTES - HASH_BYTES;

    let mut base = String::with_capacity(MAX_BASE_BYTES);
    for byte in project.id.bytes() {
        let byte = match byte {
            b'A'..=b'Z' => byte.to_ascii_lowercase(),
            b'a'..=b'z' | b'0'..=b'9' => byte,
            _ => b'-',
        };
        if byte == b'-' && (base.is_empty() || base.ends_with('-')) {
            continue;
        }
        if base.len() == MAX_BASE_BYTES {
            break;
        }
        base.push(char::from(byte));
    }
    while base.ends_with('-') {
        base.pop();
    }
    if base.is_empty() {
        base.push_str("project");
    }
    format!(
        "{PREFIX}{base}-{:08x}",
        crc32fast::hash(project.id.as_bytes())
    )
}

fn insert_aliases(map: &mut HashMap<String, String>, hash: &str, path: &str) {
    map.insert(path.to_owned(), path.to_owned());
    map.insert(hash.to_owned(), path.to_owned());
}

fn normalized_head(path: &str) -> &str {
    path.split('/').next().unwrap_or_default()
}

fn is_background(path: &str) -> bool {
    matches!(normalized_head(path), "background" | "backgrounds" | "cg")
}

fn is_figure(path: &str) -> bool {
    matches!(
        normalized_head(path),
        "character" | "characters" | "figure" | "figures"
    )
}

fn is_bgm(path: &str) -> bool {
    normalized_head(path).eq_ignore_ascii_case("bgm")
}

fn is_voice(path: &str) -> bool {
    matches!(normalized_head(path), "voice" | "voices" | "vocal")
}

fn is_effect(path: &str) -> bool {
    matches!(
        normalized_head(path),
        "se" | "sound" | "sounds" | "effect" | "effects"
    )
}

fn is_video(path: &str) -> bool {
    matches!(normalized_head(path), "video" | "videos")
        || matches!(
            Path::new(path).extension().and_then(|value| value.to_str()),
            Some("mp4" | "m4v" | "mov" | "webm" | "mkv")
        )
}

fn is_lut(path: &str) -> bool {
    matches!(normalized_head(path), "lut" | "luts")
}

pub(super) fn load_chapters(
    project_root: &Path,
    project: &ProjectDocument,
    sources: &ProjectSourceReader,
) -> Result<Vec<(PathBuf, ChapterDocument)>> {
    let directory = project_root.join("chapters");
    let mut by_name = BTreeMap::new();
    for entry in fs::read_dir(&directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let chapter: ChapterDocument = sources.read_json(&path)?;
        if by_name
            .insert(chapter.name.clone(), (path.clone(), chapter))
            .is_some()
        {
            bail!("duplicate LetsGal chapter name in {}", path.display());
        }
    }

    let name_by_id = by_name
        .iter()
        .map(|(name, (_, chapter))| (chapter.id.as_str(), name.as_str()))
        .collect::<HashMap<_, _>>();
    let folder_by_id = project
        .chapter_folders
        .iter()
        .map(|folder| (folder.id.as_str(), folder))
        .collect::<HashMap<_, _>>();
    let mut requested_order = Vec::new();
    if let Some(tree) = &project.chapter_tree_order {
        for entry in tree {
            match entry.kind.as_str() {
                "chapter" => {
                    if let Some(name) = name_by_id.get(entry.id.as_str()) {
                        requested_order.push((*name).to_owned());
                    }
                }
                "folder" => {
                    if let Some(folder) = folder_by_id.get(entry.id.as_str()) {
                        requested_order.extend(
                            folder
                                .chapter_ids
                                .iter()
                                .filter_map(|id| name_by_id.get(id.as_str()).copied())
                                .map(str::to_owned),
                        );
                    }
                }
                _ => {}
            }
        }
    }
    requested_order.extend(project.chapter_order.iter().cloned());

    let mut seen = BTreeSet::new();
    let mut ordered = Vec::with_capacity(by_name.len());
    for name in requested_order {
        if !seen.insert(name.clone()) {
            continue;
        }
        if let Some(chapter) = by_name.remove(&name) {
            ordered.push(chapter);
        }
    }
    ordered.extend(by_name.into_values());
    Ok(ordered)
}

pub(super) fn compile_project(
    project_root: &Path,
    project: &ProjectDocument,
    chapters: &[(PathBuf, ChapterDocument)],
    characters: &CharactersDocument,
    scenes: &ScenesDocument,
    manifest: &AssetManifest,
) -> Result<Vec<LoadedScene>> {
    let all_enabled = chapters
        .iter()
        .filter(|(_, chapter)| !chapter.disabled)
        .collect::<Vec<_>>();
    let chapter_preprocess = all_enabled
        .iter()
        .find(|(_, chapter)| chapter.kind == "schedule-preprocessing")
        .and_then(|(_, chapter)| chapter.fragments.first())
        .map(|fragment| fragment.id.as_str());
    let enabled = all_enabled
        .iter()
        .copied()
        .filter(|(_, chapter)| chapter.kind != "schedule-preprocessing")
        .collect::<Vec<_>>();
    // LetsGal's default shell stores its title screen as the first chapter and
    // opens `slot:internal.system.title` from there. keine already owns the
    // native title screen, so keep that chapter available for Studio block-selection
    // debugging while starting normal gameplay from the following chapter.
    let entry_index = usize::from(
        enabled.len() > 1
            && enabled
                .first()
                .is_some_and(|(_, chapter)| is_title_bootstrap(chapter)),
    );
    let scheduled = (project.schedule_mode == "advanced")
        .then_some(project.schedule.as_ref())
        .flatten()
        .map(|schedule| &schedule.graph)
        .filter(|graph| !graph.nodes.is_empty());
    let linear_entry = enabled
        .get(entry_index)
        .and_then(|(_, chapter)| chapter.fragments.first())
        .map(|fragment| fragment.id.clone())
        .context("LetsGal project has no enabled fragment")?;
    let entry = scheduled
        .and_then(schedule_entry)
        .map(schedule_scene_name)
        .unwrap_or_else(|| linear_entry.clone());

    const SCHEDULE_RETURN: &str = "__letsgal_schedule_return";
    let mut chapter_next = if scheduled.is_some() {
        enabled
            .iter()
            .map(|(_, chapter)| {
                (
                    ChapterRoute::Chapter(chapter.id.clone()),
                    Some(SCHEDULE_RETURN.to_owned()),
                )
            })
            .collect::<HashMap<_, _>>()
    } else {
        enabled
            .iter()
            .enumerate()
            .map(|(index, (_, chapter))| {
                let next = enabled
                    .get(index + 1)
                    .and_then(|(_, next)| next.fragments.first())
                    .map(|fragment| fragment.id.clone());
                (ChapterRoute::Chapter(chapter.id.clone()), next)
            })
            .collect::<HashMap<_, _>>()
    };
    chapter_next.insert(
        ChapterRoute::Preprocess,
        scheduled
            .is_none()
            .then(|| chapter_preprocess.map(str::to_owned))
            .flatten(),
    );
    let character_map = characters
        .characters
        .iter()
        .map(|character| (character.id.as_str(), character))
        .collect::<HashMap<_, _>>();
    let scene_map = scenes
        .scenes
        .iter()
        .map(|scene| (scene.id.as_str(), scene))
        .collect::<HashMap<_, _>>();
    let voice_map = manifest
        .entries
        .iter()
        .map(|(hash, entry)| (hash.as_str(), entry.path.as_str()))
        .collect::<HashMap<_, _>>();
    let positions = portrait_positions(characters);
    let context = CompileContext {
        entry: &entry,
        chapter_next: &chapter_next,
        characters: &character_map,
        scenes: &scene_map,
        voices: &voice_map,
        positions: &positions,
        portrait_height_ratio: characters.global_settings.graphics.height_ratio,
    };

    let mut loaded = Vec::new();
    for (path, chapter) in all_enabled {
        for (index, fragment) in chapter.fragments.iter().enumerate() {
            loaded.push(compile_fragment(
                path,
                chapter,
                fragment,
                &context,
                scheduled.is_none() && chapter.kind != "schedule-preprocessing" && index == 0,
            ));
        }
    }
    if let Some(graph) = scheduled {
        loaded.push(LoadedScene {
            name: SCHEDULE_RETURN.into(),
            path: project_root.join("project.json"),
            actions: Vec::new(),
            action_spans: Vec::new(),
            diagnostics: Vec::new(),
            resources: Vec::new(),
            sub_scenes: Vec::new(),
        });
        loaded.extend(compile_schedule(
            project_root,
            graph,
            &enabled,
            chapter_preprocess,
        ));
    }
    // Runtime entry points stay language-neutral while every native fragment
    // keeps its Studio UUID for call/branch/debug stability.
    let mut start_actions = Vec::new();
    if scheduled.is_none()
        && let Some(preprocess) = chapter_preprocess
    {
        start_actions.push(Action::CallScene(preprocess.to_owned()));
    }
    start_actions.push(Action::ChangeScene(entry.clone()));
    loaded.push(LoadedScene {
        name: "start".into(),
        path: project_root.join("project.json"),
        action_spans: vec![SourceSpan { line: 1, column: 1 }; start_actions.len()],
        actions: start_actions,
        diagnostics: Vec::new(),
        resources: Vec::new(),
        sub_scenes: scheduled
            .is_none()
            .then_some(chapter_preprocess)
            .flatten()
            .into_iter()
            .chain(std::iter::once(context.entry))
            .enumerate()
            .map(|(action_index, scene)| crate::SceneRef {
                scene: scene.to_owned(),
                action_index,
                span: SourceSpan { line: 1, column: 1 },
            })
            .collect(),
    });
    Ok(loaded)
}

fn schedule_scene_name(node_id: &str) -> String {
    format!("letsgal-schedule:{node_id}")
}

fn schedule_entry(graph: &ScheduleGraph) -> Option<&str> {
    graph
        .nodes
        .iter()
        .find(|node| node.kind == "start")
        .map(|node| node.id.as_str())
}

fn compile_schedule(
    project_root: &Path,
    graph: &ScheduleGraph,
    chapters: &[&(PathBuf, ChapterDocument)],
    preprocess: Option<&str>,
) -> Vec<LoadedScene> {
    let chapters = chapters
        .iter()
        .map(|(_, chapter)| (chapter.id.as_str(), chapter))
        .collect::<HashMap<_, _>>();
    graph
        .nodes
        .iter()
        .map(|node| {
            let span = SourceSpan { line: 1, column: 1 };
            let mut report = ParseReport::default();
            compile_schedule_node(node, graph, &chapters, preprocess, span, &mut report);
            LoadedScene {
                name: schedule_scene_name(&node.id),
                path: project_root.join("project.json"),
                actions: report.actions,
                action_spans: report.spans,
                diagnostics: report.diagnostics,
                resources: report.resources,
                sub_scenes: report.sub_scenes,
            }
        })
        .collect()
}

fn compile_schedule_node(
    node: &ScheduleNode,
    graph: &ScheduleGraph,
    chapters: &HashMap<&str, &ChapterDocument>,
    preprocess: Option<&str>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    let target = |port: &str| schedule_target(graph, &node.id, port);
    let change = |target: Option<&str>, report: &mut ParseReport| {
        if let Some(target) = target {
            report.push(Action::ChangeScene(schedule_scene_name(target)), span);
        }
    };
    match node.kind.as_str() {
        "start" => change(target("next"), report),
        "end" => report.push(Action::End, span),
        "chapter" => {
            let Some(chapter) = chapters.get(node.chapter_id.as_str()) else {
                report.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    span,
                    message: format!(
                        "LetsGal schedule chapter {:?} is unresolved",
                        node.chapter_id
                    ),
                });
                return;
            };
            if let Some(preprocess) = preprocess {
                report.push(Action::CallScene(preprocess.to_owned()), span);
            }
            if let Some(fragment) = chapter.fragments.first() {
                report.push(Action::CallScene(fragment.id.clone()), span);
            }
            change(target("next"), report);
        }
        "choice" => report.push(
            Action::Menu {
                prompt: String::new(),
                choices: node
                    .options
                    .iter()
                    .filter_map(|option| {
                        target(&option.id).map(|target| Choice {
                            text: option.text.clone(),
                            target: ChoiceTarget::ChangeScene(schedule_scene_name(target)),
                            show_when: None,
                            enable_when: None,
                        })
                    })
                    .collect(),
            },
            span,
        ),
        "condition" | "code" => {
            let expression = if node.kind == "code" {
                node.expression.trim().to_owned()
            } else {
                schedule_condition_expression(&node.conditions, &node.logic)
            };
            if expression.is_empty() {
                report.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    span,
                    message: format!("LetsGal schedule node {:?} has no condition", node.id),
                });
                return;
            }
            for (port, when) in [("true", expression.clone()), ("false", format!("!({expression})"))] {
                if let Some(target) = target(port) {
                    report.push(
                        Action::Flow {
                            action: Box::new(Action::ChangeScene(schedule_scene_name(target))),
                            when: Some(when),
                            next: false,
                        },
                        span,
                    );
                }
            }
        }
        "set" => {
            if !node.variable.is_empty() {
                report.push(
                    Action::Set {
                        name: node.variable.clone(),
                        expression: serde_json::to_string(&node.value)
                            .unwrap_or_else(|_| "null".into()),
                        global: false,
                    },
                    span,
                );
            }
            change(target("next"), report);
        }
        "extension" => report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: "unsupported LetsGal schedule extension strategy; Kēne keeps scheduling in typed native control flow".into(),
        }),
        kind => report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: format!("unsupported LetsGal schedule node kind {kind:?}"),
        }),
    }
}

fn schedule_target<'a>(graph: &'a ScheduleGraph, source: &str, port: &str) -> Option<&'a str> {
    graph
        .edges
        .iter()
        .find(|edge| edge.source == source && edge.port == port)
        .map(|edge| edge.target.as_str())
}

fn schedule_condition_expression(conditions: &[ScheduleCondition], logic: &str) -> String {
    let join = if logic == "or" { " || " } else { " && " };
    conditions
        .iter()
        .filter(|condition| !condition.variable.is_empty())
        .map(|condition| {
            let operator = match condition.operator.as_str() {
                "ne" => "!=",
                "gt" => ">",
                "gte" => ">=",
                "lt" => "<",
                "lte" => "<=",
                _ => "==",
            };
            let value = serde_json::to_string(&condition.value).unwrap_or_else(|_| "null".into());
            format!("({} {operator} {value})", condition.variable)
        })
        .collect::<Vec<_>>()
        .join(join)
}

fn is_title_bootstrap(chapter: &ChapterDocument) -> bool {
    chapter.fragments.iter().any(|fragment| {
        fragment.blocks.iter().any(|block| {
            block.kind == "showExtensionUI"
                && prop_string(&block.props, "target") == "slot:internal.system.title"
        })
    })
}

struct CompileContext<'a> {
    entry: &'a str,
    chapter_next: &'a HashMap<ChapterRoute, Option<String>>,
    characters: &'a HashMap<&'a str, &'a CharacterDefinition>,
    scenes: &'a HashMap<&'a str, &'a SceneDefinition>,
    voices: &'a HashMap<&'a str, &'a str>,
    positions: &'a HashMap<String, PortraitPlacement>,
    portrait_height_ratio: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ChapterRoute {
    Chapter(String),
    Preprocess,
}

fn compile_fragment(
    path: &Path,
    chapter: &ChapterDocument,
    fragment: &StoryFragment,
    context: &CompileContext<'_>,
    finish_chapter: bool,
) -> LoadedScene {
    let mut report = ParseReport::default();
    for (index, block) in fragment.blocks.iter().enumerate() {
        let span = SourceSpan {
            line: index + 1,
            column: 1,
        };
        compile_block(block, chapter, context, span, &mut report);
    }
    // LetsGal advances linear chapters at the main fragment's end. Auxiliary
    // fragments and blueprint chapters must still return to their caller.
    if finish_chapter
        && !matches!(
            report.actions.last(),
            Some(Action::ChangeScene(_) | Action::ReturnScene | Action::End)
        )
    {
        let span = SourceSpan {
            line: fragment.blocks.len() + 1,
            column: 1,
        };
        if let Some(next) = context
            .chapter_next
            .get(&ChapterRoute::Chapter(chapter.id.clone()))
            .and_then(Clone::clone)
        {
            push_chapter_change(next, context, span, &mut report);
        } else {
            report.push(Action::End, span);
        }
    }
    LoadedScene {
        name: fragment.id.clone(),
        path: path.to_owned(),
        actions: report.actions,
        action_spans: report.spans,
        diagnostics: report.diagnostics,
        resources: report.resources,
        sub_scenes: report.sub_scenes,
    }
}

fn compile_block(
    block: &StoryBlock,
    chapter: &ChapterDocument,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    if prop_bool(&block.props, "disabled", false) || block.kind == "cmdDraft" {
        return;
    }
    if !BUILTIN_BLOCK_TYPES.contains(&block.kind.as_str()) {
        report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: format!("unsupported LetsGal block type {:?}", block.kind),
        });
        return;
    }
    match block.kind.as_str() {
        "dialogue" => compile_dialogue(block, context, span, report),
        "narration" | "storyParagraph" => {
            if block.kind == "storyParagraph" {
                report.push(Action::SelectTextPresentation { paragraph: true }, span);
            }
            report.push(Action::FocusPortrait { speaker_id: None }, span);
            report.push(
                Action::Say {
                    speaker: String::new(),
                    text: studio_dialogue_markup(&block.content),
                    options: say_options(block, context),
                },
                span,
            );
            push_dialogue_lifetime(block, span, report);
        }
        "showCharacter" => show_character(block, context, span, report),
        "updateCharacter" => update_character(block, context, span, report),
        "removeCharacter" => compile_remove_characters(block, span, report),
        "scene" => compile_scene(block, context, span, report),
        "destroyScene" => compile_destroy_scene(block, context, span, report),
        "branch" => compile_branch(block, span, report),
        "callFragment" => report.push(
            Action::CallScene(prop_string(&block.props, "fragmentId")),
            span,
        ),
        "if" => compile_if(block, span, report),
        "setver" => compile_set_variable(block, span, report),
        "sound" => compile_sound(block, span, report),
        "stopSound" => compile_stop_sound(block, span, report),
        "wait" => report.push(
            if prop_bool(&block.props, "waitForInput", false) {
                Action::WaitForAdvance
            } else {
                Action::Wait {
                    seconds: prop_f32(&block.props, "duration", 0.0) / 1000.0,
                }
            },
            span,
        ),
        "playerInput" => {
            report.push(
                Action::RequestInput {
                    spec: UserInputSpec {
                        variable: prop_string(&block.props, "variable"),
                        value_type: match prop_string(&block.props, "valueType").as_str() {
                            "number" => InputValueType::Number,
                            "bool" => InputValueType::Bool,
                            _ => InputValueType::String,
                        },
                        title: prop_string_or(&block.props, "title", "请输入"),
                        description: prop_string(&block.props, "description"),
                        placeholder: prop_string_or(&block.props, "placeholder", "请输入…"),
                        confirm_text: prop_string_or(&block.props, "confirmText", "确认"),
                        required_text: prop_string_or(
                            &block.props,
                            "requiredText",
                            "请填写后再继续",
                        ),
                        required: prop_bool(&block.props, "required", true),
                        min_length: prop_f32(&block.props, "minLength", 0.0).max(0.0) as usize,
                        max_length: prop_f32(&block.props, "maxLength", 0.0).max(0.0) as usize,
                        min_value: optional_f32(&block.props, "minValue").map(f64::from),
                        max_value: optional_f32(&block.props, "maxValue").map(f64::from),
                        step: f64::from(prop_f32(&block.props, "step", 1.0).max(f32::EPSILON)),
                        true_text: prop_string_or(&block.props, "trueText", "是"),
                        false_text: prop_string_or(&block.props, "falseText", "否"),
                    },
                },
                span,
            );
        }
        "camera" => compile_camera(block, span, report),
        "resetCamera" => compile_reset_camera(block, span, report),
        "animateSprite" => compile_animate_sprite(block, context, span, report),
        "stageAnimation" => compile_stage_animation(block, context, span, report),
        "stageMask" => compile_stage_mask(block, span, report),
        "particle" => compile_particle(block, span, report),
        "loadingStrategy" => compile_loading_strategy(block, context, span, report),
        "openExternalUrl" => compile_external_url(block, span, report),
        "steamAction" | "unlockSteamAchievement" => report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: format!(
                "unsupported LetsGal 2.0 Steam block {:?}; Kēne has no Steam runtime bridge",
                block.kind
            ),
        }),
        "endChapter" => match context
            .chapter_next
            .get(&ChapterRoute::Chapter(chapter.id.clone()))
            .cloned()
            .flatten()
        {
            Some(next) => push_chapter_change(next, context, span, report),
            None => report.push(Action::End, span),
        },
        "returnToEntry" => push_chapter_change(context.entry.to_owned(), context, span, report),
        "comment" => report.push(Action::Comment, span),
        "curtain" => compile_curtain(block, span, report),
        "video" => compile_video(block, span, report),
        "stopVideo" => compile_stop_video(block, span, report),
        "showExtensionUI" => compile_system_ui(block, true, span, report),
        "hideExtensionUI" => compile_system_ui(block, false, span, report),
        "callExtensionFunction" => {
            if !compile_known_extension(block, span, report) {
                push_host(block, "extension", "method.call", span, report);
            }
        }
        "switchDialogueStyle" => report.push(
            Action::SetDialogueStyle {
                style: keine_core::DialogueStyle::from_id(prop_string_or(
                    &block.props,
                    "targetId",
                    "default",
                )),
            },
            span,
        ),
        "switchParagraphStyle" => report.push(
            Action::SetParagraphStyle {
                style: keine_core::DialogueStyle::from_id(prop_string_or(
                    &block.props,
                    "targetId",
                    "default",
                )),
                typewriter_speed: optional_f32(&block.props, "textSpeed")
                    .filter(|speed| *speed > f32::EPSILON)
                    .map(|milliseconds| (1000.0 / f64::from(milliseconds)).clamp(10.0, 120.0)),
                text_reveal: paragraph_text_reveal(&block.props),
            },
            span,
        ),
        "portraitStyleRule" => compile_portrait_rule(block, context, span, report),
        "floatingText" => compile_floating_text(block, span, report),
        "hideFloatingText" => report.push(
            Action::HideFloatingText {
                id: non_empty(prop_string(&block.props, "floatingTextId")),
            },
            span,
        ),
        "systemMessage" => compile_system_message(block, span, report),
        "enterAutoPlay" => report.push(Action::SetAutoplay { enabled: true }, span),
        "exitAutoPlay" => report.push(Action::SetAutoplay { enabled: false }, span),
        _ => unreachable!("the 2.0 block registry is exhaustively matched"),
    }
}

fn push_chapter_change(
    target: String,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    if let Some(preprocess) = context
        .chapter_next
        .get(&ChapterRoute::Preprocess)
        .and_then(|preprocess| preprocess.as_ref())
    {
        report.push(Action::CallScene(preprocess.clone()), span);
    }
    report.push(Action::ChangeScene(target), span);
}

fn compile_external_url(_block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    report.diagnostics.push(Diagnostic {
        level: DiagnosticLevel::Error,
        span,
        message: "unsupported LetsGal 2.0 external-browser block; Kēne keeps script execution platform-neutral".into(),
    });
}

fn compile_loading_strategy(
    block: &StoryBlock,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    let mode = match prop_string_or(&block.props, "mode", "auto").as_str() {
        "manual" => LoadingStrategyMode::Manual,
        _ => LoadingStrategyMode::Auto,
    };
    let mut resources = Vec::new();
    if mode == LoadingStrategyMode::Manual {
        let values = json_value(&block.props, "resourcesJson")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        for value in values {
            let Some(resource) = value.as_object() else {
                continue;
            };
            match value_str(resource.get("kind")) {
                "character" => {
                    let character_id = value_str(resource.get("characterId"));
                    let expression_name = value_str(resource.get("expression"));
                    let skin = value_str(resource.get("skin"));
                    let image = context.characters.get(character_id).and_then(|character| {
                        character
                            .expressions
                            .iter()
                            .find(|expression| expression.name == expression_name)
                            .or_else(|| character.expressions.first())
                            .map(|expression| {
                                expression
                                    .skin_assets
                                    .get(skin)
                                    .filter(|_| !skin.is_empty())
                                    .unwrap_or(&expression.asset_path)
                                    .clone()
                            })
                    });
                    if let Some(path) = image.filter(|path| !path.is_empty()) {
                        resources.push(AssetHint {
                            path,
                            kind: AssetHintKind::Figure,
                        });
                    } else {
                        report.diagnostics.push(Diagnostic {
                            level: DiagnosticLevel::Warning,
                            span,
                            message: format!(
                                "LetsGal loading strategy character {character_id:?} is unresolved"
                            ),
                        });
                    }
                }
                "scene" => {
                    let scene_id = value_str(resource.get("sceneId"));
                    if let Some(scene) = context.scenes.get(scene_id) {
                        resources.extend(
                            scene
                                .layers
                                .iter()
                                .filter(|layer| !layer.asset_path.is_empty())
                                .map(|layer| AssetHint {
                                    path: layer.asset_path.clone(),
                                    kind: AssetHintKind::Figure,
                                }),
                        );
                    } else {
                        report.diagnostics.push(Diagnostic {
                            level: DiagnosticLevel::Warning,
                            span,
                            message: format!(
                                "LetsGal loading strategy scene {scene_id:?} is unresolved"
                            ),
                        });
                    }
                }
                kind => report.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Warning,
                    span,
                    message: format!("unknown LetsGal loading resource kind {kind:?}"),
                }),
            }
        }
    }
    resources.sort_by(|left, right| left.path.cmp(&right.path));
    resources.dedup();
    report.push(
        Action::ConfigureLoading {
            strategy: LoadingStrategy {
                mode,
                lookahead: prop_f32(&block.props, "lookahead", 20.0).clamp(1.0, 500.0) as u16,
                blocking: prop_string_or(&block.props, "execution", "background") == "wait",
                resources,
            },
        },
        span,
    );
}

fn compile_dialogue(
    block: &StoryBlock,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    if !prop_string(&block.props, "nameVariantId").is_empty() {
        logic_error(
            report,
            span,
            "name variants need an explicit native speaker name",
        );
        return;
    }
    let avatar_only = prop_bool(&block.props, "dialoguePortraitOnly", false);
    if avatar_only {
        let Some(character) = character(block, context) else {
            logic_error(report, span, "avatar-only dialogue has no character");
            return;
        };
        let expression_name = prop_string(&block.props, "expression");
        let expression = character
            .expressions
            .iter()
            .find(|expression| expression.name == expression_name)
            .or_else(|| character.expressions.first());
        let Some(expression) = expression.filter(|expression| {
            !expression.asset_path.is_empty() && expression.extras.get("presentation").is_none()
        }) else {
            logic_error(
                report,
                span,
                "composite avatars require an explicit native avatar asset",
            );
            return;
        };
        let skin = prop_string(&block.props, "skin");
        let skin = if skin.is_empty() {
            character
                .portrait_skin_config
                .as_ref()
                .map(|config| config.default_skin.as_str())
                .unwrap_or_default()
        } else {
            skin.as_str()
        };
        let image = expression
            .skin_assets
            .get(skin)
            .unwrap_or(&expression.asset_path);
        report.push(
            Action::MiniAvatar {
                image: image.clone(),
            },
            span,
        );
    }
    if !avatar_only
        && character(block, context).is_some()
        && prop_bool(&block.props, "showCharacter", true)
        && prop_bool(&block.props, "isFirst", true)
    {
        show_character(block, context, span, report);
    }
    let character = character(block, context);
    report.push(
        Action::FocusPortrait {
            speaker_id: character.map(|character| character.id.clone()),
        },
        span,
    );
    report.push(
        Action::Say {
            speaker: character
                .map(|character| character.name.clone())
                .unwrap_or_else(|| prop_string(&block.props, "characterName")),
            text: studio_dialogue_markup(&block.content),
            options: say_options(block, context),
        },
        span,
    );
    push_dialogue_lifetime(block, span, report);
    if avatar_only {
        report.push(Action::HideMiniAvatar, span);
    }
    if prop_bool(&block.props, "isLast", true) && !prop_bool(&block.props, "keepCharacter", true) {
        report.push(
            Action::HideSprites {
                prefix: differential_layer_prefix(&character_id(block)),
                transition: Transition::Fade(0.2),
            },
            span,
        );
        report.push(
            Action::HideSprite {
                id: character_id(block),
                transition: Transition::Fade(0.2),
            },
            span,
        );
    }
}

/// Studio keeps the most recently rendered line on screen by default. A
/// block with `keepDialogue: false` hides it only after that line has been
/// acknowledged, then lets the next dialogue block restore the textbox.
fn push_dialogue_lifetime(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    if !prop_bool(&block.props, "keepDialogue", true) {
        report.push(
            Action::SetTextbox {
                visible: false,
                auto: true,
            },
            span,
        );
    }
}

fn say_options(block: &StoryBlock, context: &CompileContext<'_>) -> SayOptions {
    let voice = prop_string(&block.props, "voiceHash");
    SayOptions {
        vocal: (!voice.is_empty()).then(|| {
            context
                .voices
                .get(voice.as_str())
                .copied()
                .unwrap_or(voice.as_str())
                .to_owned()
        }),
        ..SayOptions::default()
    }
}

fn show_character(
    block: &StoryBlock,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    compile_character(block, context, span, report, false);
}

fn update_character(
    block: &StoryBlock,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    compile_character(block, context, span, report, true);
}

#[derive(Deserialize)]
struct RemoveCharacterTarget {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

fn compile_remove_characters(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let mut targets =
        json_string::<Vec<RemoveCharacterTarget>>(&block.props, "characterTargetsJson")
            .unwrap_or_default()
            .into_iter()
            .filter_map(|target| {
                non_empty(if target.id.is_empty() {
                    target.name
                } else {
                    target.id
                })
            })
            .collect::<Vec<_>>();
    if targets.is_empty() {
        targets.extend(non_empty(character_id(block)));
    }
    let mut seen = BTreeSet::new();
    targets.retain(|target| seen.insert(target.clone()));

    let transition = fade(block, "animated", 0.2);
    for id in targets {
        report.push(
            Action::HideSprites {
                prefix: differential_layer_prefix(&id),
                transition,
            },
            span,
        );
        report.push(Action::HideSprite { id, transition }, span);
    }
}

fn compile_character(
    block: &StoryBlock,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
    update: bool,
) {
    let Some(character) = character(block, context) else {
        report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            span,
            message: "LetsGal character reference is unresolved".into(),
        });
        return;
    };
    let expression = prop_string(&block.props, "expression");
    let expression = character
        .expressions
        .iter()
        .find(|candidate| candidate.name == expression)
        .or_else(|| character.expressions.first());
    let Some(expression) = expression else {
        return;
    };
    let presentation = expression
        .extras
        .get("presentation")
        .and_then(Value::as_object);
    let presentation_kind = presentation
        .and_then(|presentation| presentation.get("type"))
        .and_then(Value::as_str);
    let locked_skin = prop_string(&block.props, "skin");
    let skin_config = character.portrait_skin_config.as_ref();
    let default_skin = skin_config
        .map(|config| config.default_skin.as_str())
        .unwrap_or_default();
    let active_skin = if locked_skin.is_empty() {
        default_skin
    } else {
        locked_skin.as_str()
    };
    let differential = if presentation_kind == Some("differential") {
        presentation.and_then(|presentation| {
            differential_portrait(character, presentation, active_skin, span, report)
        })
    } else {
        None
    };
    let sequence = match (presentation_kind, presentation) {
        (Some("sequence"), Some(presentation)) => {
            let Some(sequence) = portrait_sequence(presentation, character) else {
                report.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    span,
                    message: "LetsGal sequence portrait has no resolvable static frames".into(),
                });
                return;
            };
            Some(sequence)
        }
        (Some("differential"), _) => None,
        (Some(kind @ ("spine" | "live2d")), _) => {
            report.diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Error,
                span,
                message: format!(
                    "unsupported LetsGal 1.20.0 dynamic portrait type {kind:?}; \
                     Kēne currently supports static and sequence portraits only"
                ),
            });
            return;
        }
        (Some(kind), _) => {
            report.diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Error,
                span,
                message: format!("unsupported LetsGal portrait presentation type {kind:?}"),
            });
            return;
        }
        (None, _) => None,
    };
    let image = if let Some(differential) = &differential {
        differential
            .layers
            .first()
            .map(|layer| layer.image.clone())
            .unwrap_or_default()
    } else if let Some(sequence) = &sequence {
        sequence.frames[0].clone()
    } else if !locked_skin.is_empty() {
        expression
            .skin_assets
            .get(&locked_skin)
            .cloned()
            .unwrap_or_else(|| expression.asset_path.clone())
    } else if !default_skin.is_empty() {
        expression
            .skin_assets
            .get(default_skin)
            .cloned()
            .unwrap_or_else(|| expression.asset_path.clone())
    } else {
        expression.asset_path.clone()
    };
    if image.is_empty() {
        return;
    }
    let layout = character.portrait_layout.as_ref();
    let position_id = prop_string_or(
        &block.props,
        "position",
        if !character.default_position.is_empty() {
            &character.default_position
        } else if let Some(position) = layout
            .map(|layout| layout.default_position_id.as_str())
            .filter(|position| !position.is_empty())
        {
            position
        } else {
            "center"
        },
    );
    let distance_id = prop_string_or(
        &block.props,
        "distance",
        layout
            .map(|layout| layout.default_distance_id.as_str())
            .filter(|distance| !distance.is_empty())
            .unwrap_or_default(),
    );
    let height_ratio = expression
        .graphics_override
        .height_ratio
        .or_else(|| layout.and_then(|layout| layout.graphics.height_ratio))
        .or(context.portrait_height_ratio)
        .filter(|ratio| ratio.is_finite() && *ratio > 0.0);
    let Some((position, distance_scale)) = studio_position(
        &character.id,
        &distance_id,
        &position_id,
        height_ratio.unwrap_or(1.0) * keine_core::DESIGN_HEIGHT,
        context,
        span,
        report,
    ) else {
        return;
    };
    let transform = SpriteTransform {
        scale_x: distance_scale,
        scale_y: distance_scale,
        ..SpriteTransform::default()
    };
    if let Some(differential) = differential {
        compile_differential_character(
            block,
            character,
            expression,
            position,
            distance_scale,
            context.portrait_height_ratio,
            differential,
            span,
            report,
            update,
        );
        return;
    }
    let action = if update {
        let duration = if prop_bool(&block.props, "placementTransitionEnabled", true) {
            prop_f32(&block.props, "placementTransitionDuration", 350.0).max(0.0) / 1000.0
        } else {
            0.0
        };
        Action::UpdateSprite {
            id: character.id.clone(),
            image: image.clone(),
            position,
            layout: height_ratio
                .map(SpriteLayout::ViewportHeight)
                .unwrap_or(SpriteLayout::Natural),
            scale: distance_scale,
            duration,
            easing: easing(&prop_string(&block.props, "placementTransitionEasing")),
            blocking: prop_bool(&block.props, "placementTransitionBlocking", false),
        }
    } else {
        Action::ShowSprite {
            id: character.id.clone(),
            image: image.clone(),
            position,
            layout: height_ratio
                .map(SpriteLayout::ViewportHeight)
                .unwrap_or(SpriteLayout::Natural),
            transition: fade(block, "animated", 0.2),
            transform,
            z_index: 100,
            blend: BlendMode::Alpha,
        }
    };
    report.push(action, span);
    if let Some(sequence) = sequence {
        let action = if sequence.frame_durations.is_empty() {
            Action::ConfigureSpriteSequence {
                id: character.id.clone(),
                frames: sequence.frames,
                fps: sequence.fps,
                looped: sequence.looped,
            }
        } else {
            Action::ConfigureTimedSpriteSequence {
                id: character.id.clone(),
                frames: sequence.frames,
                frame_durations: sequence.frame_durations,
                looped: sequence.looped,
            }
        };
        report.push(action, span);
    } else if locked_skin.is_empty()
        && let Some(config) = skin_config
        && !config.attribute_name.is_empty()
        && !expression.skin_assets.is_empty()
    {
        report.push(
            Action::SelectSpriteImage {
                id: character.id.clone(),
                variable: format!("{}.{}", character.id, config.attribute_name),
                default_image: image,
                variants: sorted_skin_assets(&expression.skin_assets),
            },
            span,
        );
    }
    if !update && prop_bool(&block.props, "cameraBound", false) {
        report.push(
            Action::SetCameraBinding {
                target: character.id.clone(),
                bound: true,
                distance: prop_f32(&block.props, "cameraDistance", 1.0).max(f32::EPSILON),
            },
            span,
        );
    }
}

struct CompiledDifferentialPortrait {
    canvas: [f32; 2],
    layers: Vec<CompiledDifferentialLayer>,
}

struct CompiledDifferentialLayer {
    layer_id: String,
    image: String,
    rect: Option<[f32; 4]>,
    opacity: f32,
    frames: Vec<String>,
    fps: f32,
    variants: Vec<(String, String)>,
}

fn differential_portrait(
    character: &CharacterDefinition,
    presentation: &Map<String, Value>,
    skin: &str,
    span: SourceSpan,
    report: &mut ParseReport,
) -> Option<CompiledDifferentialPortrait> {
    let group_id = value_str(presentation.get("groupId"));
    let Some(group) = character
        .differential_portrait_groups
        .iter()
        .find(|group| group.id == group_id)
    else {
        report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: format!(
                "LetsGal differential portrait group {group_id:?} is unresolved for character {:?}",
                character.id
            ),
        });
        return None;
    };
    let mut selections = group
        .layers
        .iter()
        .map(|layer| (layer.id.clone(), layer.default_option_id.clone()))
        .collect::<HashMap<_, _>>();
    if let Some(skin_selections) = group.skin_selections.get(skin) {
        selections.extend(skin_selections.clone());
    }
    apply_differential_selections(&mut selections, presentation.get("selections"));
    if let Some(skin_selections) = presentation
        .get("skinSelections")
        .and_then(Value::as_object)
        .and_then(|selections| selections.get(skin))
    {
        apply_differential_selections(&mut selections, Some(skin_selections));
    }

    let layers = group
        .layers
        .iter()
        .filter_map(|layer| {
            let selected = selections.get(&layer.id).and_then(Option::as_deref)?;
            let option = layer.options.iter().find(|option| option.id == selected)?;
            let image = option
                .frames
                .first()
                .filter(|frame| !frame.is_empty())
                .cloned()
                .unwrap_or_else(|| option.asset_path.clone());
            if image.is_empty() {
                return None;
            }
            let variants = layer
                .variable_rules
                .iter()
                .filter_map(|rule| {
                    let option_id = rule.option_id.as_deref()?;
                    let option = layer.options.iter().find(|option| option.id == option_id)?;
                    let image = option
                        .frames
                        .first()
                        .filter(|frame| !frame.is_empty())
                        .cloned()
                        .unwrap_or_else(|| option.asset_path.clone());
                    (!image.is_empty()).then(|| {
                        let variable = if rule.scope == "global" || rule.variable.contains('.') {
                            rule.variable.clone()
                        } else {
                            format!("{}.{}", character.id, rule.variable)
                        };
                        let operator = match rule.operator.as_str() {
                            "ne" => "!=",
                            "gt" => ">",
                            "gte" => ">=",
                            "lt" => "<",
                            "lte" => "<=",
                            _ => "==",
                        };
                        let value =
                            serde_json::to_string(&rule.value).unwrap_or_else(|_| "null".into());
                        (format!("{variable} {operator} {value}"), image)
                    })
                })
                .collect();
            Some(CompiledDifferentialLayer {
                layer_id: layer.id.clone(),
                image,
                rect: option
                    .rect
                    .map(|rect| [rect.x, rect.y, rect.width, rect.height]),
                opacity: (layer.opacity * option.opacity).clamp(0.0, 1.0),
                frames: option.frames.clone(),
                fps: option.fps.clamp(1.0, 60.0),
                variants,
            })
        })
        .collect::<Vec<_>>();
    if layers.is_empty() {
        report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: format!(
                "LetsGal differential portrait group {group_id:?} has no selected static layers"
            ),
        });
        return None;
    }
    Some(CompiledDifferentialPortrait {
        canvas: [group.width.max(1.0), group.height.max(1.0)],
        layers,
    })
}

fn apply_differential_selections(
    selections: &mut HashMap<String, Option<String>>,
    value: Option<&Value>,
) {
    let Some(value) = value.and_then(Value::as_object) else {
        return;
    };
    for (layer, option) in value {
        let selected = option.as_str().map(str::to_owned);
        if option.is_null() || selected.is_some() {
            selections.insert(layer.clone(), selected);
        }
    }
}

fn differential_layer_prefix(character_id: &str) -> String {
    format!("character-layer:{character_id}:")
}

#[allow(clippy::too_many_arguments)]
fn compile_differential_character(
    block: &StoryBlock,
    character: &CharacterDefinition,
    expression: &super::model::CharacterExpression,
    position: Position,
    distance_scale: f32,
    global_height_ratio: Option<f32>,
    portrait: CompiledDifferentialPortrait,
    span: SourceSpan,
    report: &mut ParseReport,
    update: bool,
) {
    let portrait_layout = character.portrait_layout.as_ref();
    let height_ratio = expression
        .graphics_override
        .height_ratio
        .or_else(|| portrait_layout.and_then(|layout| layout.graphics.height_ratio))
        .or(global_height_ratio)
        .filter(|ratio| ratio.is_finite() && *ratio > 0.0);
    let duration = if prop_bool(&block.props, "placementTransitionEnabled", true) {
        prop_f32(&block.props, "placementTransitionDuration", 350.0).max(0.0) / 1000.0
    } else {
        0.0
    };
    if !update {
        report.push(
            Action::HideSprites {
                prefix: differential_layer_prefix(&character.id),
                transition: Transition::Instant,
            },
            span,
        );
    }
    let layer_count = portrait.layers.len();
    for (index, layer) in portrait.layers.into_iter().enumerate() {
        let id = if index == 0 {
            character.id.clone()
        } else {
            format!(
                "{}{}",
                differential_layer_prefix(&character.id),
                layer.layer_id
            )
        };
        let layout = SpriteLayout::Composite {
            canvas: portrait.canvas,
            rect: layer.rect,
            height_ratio,
        };
        let action = if update {
            Action::UpdateSprite {
                id: id.clone(),
                image: layer.image.clone(),
                position,
                layout,
                scale: distance_scale,
                duration,
                easing: easing(&prop_string(&block.props, "placementTransitionEasing")),
                blocking: prop_bool(&block.props, "placementTransitionBlocking", false),
            }
        } else {
            Action::ShowSprite {
                id: id.clone(),
                image: layer.image.clone(),
                position,
                layout,
                transition: fade(block, "animated", 0.2),
                transform: SpriteTransform {
                    scale_x: distance_scale,
                    scale_y: distance_scale,
                    alpha: layer.opacity,
                    ..SpriteTransform::default()
                },
                z_index: 100 + index as i32,
                blend: BlendMode::Alpha,
            }
        };
        if index + 1 == layer_count {
            report.push(action, span);
        } else {
            report.push(
                Action::Flow {
                    action: Box::new(action),
                    when: None,
                    next: true,
                },
                span,
            );
        }
        if !layer.variants.is_empty() {
            report.push(
                Action::SelectSpriteImageByCondition {
                    id: id.clone(),
                    default_image: layer.image.clone(),
                    variants: layer.variants,
                },
                span,
            );
        }
        if layer.frames.len() > 1 {
            report.push(
                Action::ConfigureSpriteSequence {
                    id: id.clone(),
                    frames: layer.frames,
                    fps: layer.fps,
                    looped: true,
                },
                span,
            );
        }
        if !update && prop_bool(&block.props, "cameraBound", false) {
            report.push(
                Action::SetCameraBinding {
                    target: id,
                    bound: true,
                    distance: prop_f32(&block.props, "cameraDistance", 1.0).max(f32::EPSILON),
                },
                span,
            );
        }
    }
}

fn sorted_skin_assets(skin_assets: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut variants = skin_assets
        .iter()
        .map(|(skin, image)| (skin.clone(), image.clone()))
        .collect::<Vec<_>>();
    variants.sort_unstable();
    variants
}

struct PortraitSequence {
    frames: Vec<String>,
    frame_durations: Vec<f32>,
    fps: f32,
    looped: bool,
}

fn portrait_sequence(
    presentation: &Map<String, Value>,
    character: &CharacterDefinition,
) -> Option<PortraitSequence> {
    let frame_values = presentation
        .get("frames")
        .or_else(|| presentation.get("frameExpressionNames"))
        .and_then(Value::as_array)?;
    let resolved = frame_values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let frame = value.as_str()?;
            character
                .expressions
                .iter()
                .find(|expression| expression.name == frame && !expression.asset_path.is_empty())
                .map(|expression| expression.asset_path.clone())
                .or_else(|| (!frame.is_empty()).then(|| frame.to_owned()))
                .map(|path| (index, path))
        })
        .collect::<Vec<_>>();
    let frames = resolved
        .iter()
        .map(|(_, path)| path.clone())
        .collect::<Vec<_>>();
    let frame_durations = presentation
        .get("frameDurationsMs")
        .and_then(Value::as_array)
        .filter(|durations| durations.len() == frame_values.len())
        .map(|durations| {
            resolved
                .iter()
                .map(|(index, _)| {
                    durations[*index]
                        .as_f64()
                        .filter(|duration| duration.is_finite() && *duration > 0.0)
                        .map_or(1.0 / 12.0, |duration| duration as f32 / 1000.0)
                })
                .collect()
        })
        .unwrap_or_default();
    (!frames.is_empty()).then(|| PortraitSequence {
        frames,
        frame_durations,
        fps: presentation
            .get("fps")
            .and_then(Value::as_f64)
            .map_or(12.0, |fps| fps as f32)
            .clamp(1.0, 120.0),
        looped: presentation
            .get("loop")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
}

#[derive(Clone, Debug)]
struct PortraitPlacement {
    left: f32,
    top: f32,
    scale: f32,
    // None retains the pre-v2 legacy baseline coordinates.
    canvas_anchor: Option<String>,
}

fn studio_position(
    character_id: &str,
    distance_id: &str,
    position_id: &str,
    base_height: f32,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) -> Option<(Position, f32)> {
    let keys = [
        format!("{character_id}\0{distance_id}\0{position_id}"),
        format!("\0{distance_id}\0{position_id}"),
        position_id.to_owned(),
    ];
    if let Some(placement) = keys.iter().find_map(|key| context.positions.get(key)) {
        let left = keine_core::DESIGN_WIDTH * placement.left / 100.0;
        let top = keine_core::DESIGN_HEIGHT * placement.top / 100.0;
        let position = match placement.canvas_anchor.as_deref() {
            None => Position {
                x: Anchor::Left(left),
                y: top,
            },
            Some("center") => Position {
                x: Anchor::Center(left - keine_core::DESIGN_WIDTH * 0.5),
                y: keine_core::DESIGN_HEIGHT - top - base_height * 0.5,
            },
            Some(anchor) => {
                report.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    span,
                    message: format!(
                        "unsupported LetsGal portrait anchor {anchor:?}; requires center"
                    ),
                });
                return None;
            }
        };
        return Some((position, placement.scale.max(f32::EPSILON)));
    }
    Some((
        match position_id {
            "left" | "center-left" => Position::left(0.0),
            "right" | "center-right" => Position::right(0.0),
            _ => Position::center(0.0),
        },
        1.0,
    ))
}

fn portrait_positions(characters: &CharactersDocument) -> HashMap<String, PortraitPlacement> {
    let canvas_anchor = (characters.version >= 2
        || !characters.global_settings.default_anchor.is_empty())
    .then(|| {
        if characters.global_settings.default_anchor.is_empty() {
            "center".to_owned()
        } else {
            characters.global_settings.default_anchor.clone()
        }
    });
    let mut positions = characters
        .global_settings
        .positions
        .iter()
        .map(|position| {
            (
                position.id.clone(),
                PortraitPlacement {
                    left: position.left,
                    top: position.top,
                    scale: 1.0,
                    canvas_anchor: canvas_anchor.clone(),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    insert_portrait_layout(
        &mut positions,
        "",
        &characters.global_settings.distance_presets,
        canvas_anchor.clone(),
    );
    if let Some(default) = characters
        .global_settings
        .distance_presets
        .iter()
        .find(|preset| preset.id == characters.global_settings.default_distance_id)
    {
        for position in &default.positions {
            positions.insert(
                format!("\0\0{}", position.id),
                PortraitPlacement {
                    left: position.left,
                    top: position.top,
                    scale: default.scale,
                    canvas_anchor: canvas_anchor.clone(),
                },
            );
        }
    }
    for character in &characters.characters {
        if let Some(layout) = &character.portrait_layout {
            let canvas_anchor = if layout.default_anchor.is_empty() {
                canvas_anchor.clone()
            } else {
                Some(layout.default_anchor.clone())
            };
            insert_portrait_layout(
                &mut positions,
                &character.id,
                &layout.distance_presets,
                canvas_anchor.clone(),
            );
            if let Some(default) = layout
                .distance_presets
                .iter()
                .find(|preset| preset.id == layout.default_distance_id)
            {
                for position in &default.positions {
                    positions.insert(
                        format!("{}\0\0{}", character.id, position.id),
                        PortraitPlacement {
                            left: position.left,
                            top: position.top,
                            scale: default.scale,
                            canvas_anchor: canvas_anchor.clone(),
                        },
                    );
                }
            }
        }
    }
    positions
}

fn insert_portrait_layout(
    positions: &mut HashMap<String, PortraitPlacement>,
    character_id: &str,
    presets: &[super::model::PortraitDistancePreset],
    canvas_anchor: Option<String>,
) {
    for preset in presets {
        for position in &preset.positions {
            positions.insert(
                format!("{character_id}\0{}\0{}", preset.id, position.id),
                PortraitPlacement {
                    left: position.left,
                    top: position.top,
                    scale: preset.scale,
                    canvas_anchor: canvas_anchor.clone(),
                },
            );
        }
    }
}

fn compile_scene(
    block: &StoryBlock,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    let parallax = prop_bool(&block.props, "mouseParallaxEnabled", false).then(|| {
        let amplitude = prop_f32(&block.props, "mouseParallaxAmplitude", 4.0).clamp(0.0, 100.0);
        let scale = match prop_string_or(&block.props, "mouseParallaxScaleMode", "auto").as_str() {
            "custom" | "manual" => {
                prop_f32(&block.props, "mouseParallaxScale", 1.08).clamp(1.0, 5.0)
            }
            _ => (1.0 + 2.0 * amplitude / 100.0).clamp(1.0, 5.0),
        };
        SceneMouseParallax {
            amplitude_percent: amplitude,
            edge_ease_percent: prop_f32(&block.props, "mouseParallaxEdgeEase", 0.0)
                .clamp(0.0, 100.0),
            return_to_center_on_leave: prop_bool(&block.props, "mouseParallaxReturnOnLeave", true),
            scale,
        }
    });
    report.push(Action::ConfigureSceneMouseParallax { parallax }, span);
    if prop_bool(&block.props, "resetCamera", false) {
        push_camera_reset(0.0, Easing::Linear, false, span, report);
    }
    let scene_id = prop_string(&block.props, "sceneId");
    let transition = scene_transition(block);
    let duration = transition.duration().unwrap_or(0.0);
    let Some(scene) = context.scenes.get(scene_id.as_str()).copied() else {
        let uri = prop_string(&block.props, "uri");
        if !uri.is_empty() {
            push_scene_layer_exits(transition, span, report);
            report.push(
                Action::Flow {
                    action: Box::new(Action::ShowBg {
                        image: uri,
                        transition,
                        transform: SpriteTransform::default(),
                    }),
                    when: None,
                    next: true,
                },
                span,
            );
            report.push(
                Action::SetCameraBinding {
                    target: "bg-main".into(),
                    bound: true,
                    distance: 1.0,
                },
                span,
            );
            if prop_bool(&block.props, "waitForComplete", false) && duration > 0.0 {
                report.push(Action::Wait { seconds: duration }, span);
            }
        } else {
            report.diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Error,
                span,
                message: format!("LetsGal scene {scene_id:?} does not exist"),
            });
        }
        return;
    };

    // Studio replaces the complete composed scene, including every auxiliary
    // layer from the previous scene. Start all leave/enter transitions in one
    // runtime step; serial blocking here makes a six-layer scene take seven
    // times the authored duration and leaves stale layers over later scenes.
    push_scene_layer_exits(transition, span, report);
    report.push(Action::HideParticleLayers, span);

    // A Studio scene is one canvas made from peer layers. Treating its first
    // image as keine's full-screen background silently squeezes wide
    // `by_height` canvases (for example 5359x1080) into 1920x1080 while every
    // other layer keeps the authored aspect. That creates a hard vertical
    // seam and makes the lowest layer diverge from the composition. Clear the
    // standalone background and render every Studio layer through the same
    // scene-canvas layout instead.
    report.push(
        Action::Flow {
            action: Box::new(Action::HideBg { transition }),
            when: None,
            next: true,
        },
        span,
    );
    let layout = SpriteLayout::Scene(scene_layer_layout(block));
    for (index, layer) in scene
        .layers
        .iter()
        .filter(|layer| layer.kind != "particle" && !layer.asset_path.is_empty())
        .enumerate()
    {
        let layer_offset = parse_position(&layer.offset);
        let mut transform = SpriteTransform::default();
        if !layer.offset.trim().is_empty() {
            transform.offset_x = layer_offset[0];
            // Studio's canvas is downward-positive while the stage transform
            // is upward-positive.
            transform.offset_y = -layer_offset[1];
        }
        report.push(
            Action::Flow {
                action: Box::new(Action::ShowSprite {
                    id: format!("scene-layer:{}", layer.id),
                    image: layer.asset_path.clone(),
                    // LetsGal scene layers use a left-top base at (0, 0).
                    // Centering wide layers changes every authored x animation.
                    position: Position::left(0.0),
                    layout,
                    transition,
                    transform,
                    z_index: index as i32,
                    blend: BlendMode::Alpha,
                }),
                when: None,
                next: true,
            },
            span,
        );
        report.push(
            Action::SetCameraBinding {
                target: format!("scene-layer:{}", layer.id),
                bound: true,
                distance: layer.distance.max(f32::EPSILON),
            },
            span,
        );
    }
    for layer in scene.layers.iter().filter(|layer| layer.kind == "particle") {
        let particle = layer.particle.as_ref();
        let options = match particle.map(|particle| particle.options_json.trim()) {
            None | Some("") => StudioParticleOverrides::default(),
            Some(source) => match serde_json::from_str::<StudioParticleOverrides>(source) {
                Ok(options) => options,
                Err(_) => {
                    logic_error(report, span, "invalid scene particle options JSON");
                    continue;
                }
            },
        };
        if !options.unsupported.is_empty() {
            logic_error(
                report,
                span,
                "scene particle overrides require explicit native controls",
            );
            continue;
        }
        report.push(
            Action::ShowParticles {
                id: format!("scene-particle:{}", layer.id),
                effect: studio_particle_effect(
                    &layer.asset_path,
                    particle
                        .map(|particle| particle.preset.as_str())
                        .unwrap_or("LIGHT_SNOW"),
                    options,
                    0.0,
                ),
            },
            span,
        );
    }
    if prop_bool(&block.props, "waitForComplete", false) && duration > 0.0 {
        report.push(Action::Wait { seconds: duration }, span);
    }
}

fn push_scene_layer_exits(transition: Transition, span: SourceSpan, report: &mut ParseReport) {
    report.push(
        Action::Flow {
            action: Box::new(Action::HideSprites {
                prefix: "scene-layer:".into(),
                transition,
            }),
            when: None,
            next: true,
        },
        span,
    );
}

fn compile_destroy_scene(
    block: &StoryBlock,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    report.push(Action::ConfigureSceneMouseParallax { parallax: None }, span);
    report.push(Action::HideParticleLayers, span);
    let transition = fade(block, "animated", 0.2);
    report.push(
        Action::Flow {
            action: Box::new(Action::HideBg { transition }),
            when: None,
            next: true,
        },
        span,
    );
    let target = prop_string(&block.props, "sceneId");
    if target == "all" || target.is_empty() {
        report.push(
            Action::Flow {
                action: Box::new(Action::HideSprites {
                    prefix: "scene-layer:".into(),
                    transition,
                }),
                when: None,
                next: true,
            },
            span,
        );
    } else if let Some(scene) = context.scenes.get(target.as_str()) {
        for layer in scene.layers.iter().skip(1) {
            report.push(
                Action::Flow {
                    action: Box::new(Action::HideSprite {
                        id: format!("scene-layer:{}", layer.id),
                        transition,
                    }),
                    when: None,
                    next: true,
                },
                span,
            );
        }
    }
    if prop_bool(&block.props, "waitForComplete", false)
        && let Some(duration) = transition.duration()
    {
        report.push(Action::Wait { seconds: duration }, span);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BranchChoice {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    fragment_id: String,
    #[serde(default)]
    visible_if: Option<String>,
    #[serde(default)]
    var_ops: Vec<Map<String, Value>>,
}

fn logic_error(report: &mut ParseReport, span: SourceSpan, message: impl Into<String>) {
    report.diagnostics.push(Diagnostic {
        level: DiagnosticLevel::Error,
        span,
        message: message.into(),
    });
}

fn studio_conditions(value: &Value, logic: &str) -> Result<String, String> {
    let (conditions, logic) = if let Some(object) = value.as_object() {
        (
            object.get("conditions").and_then(Value::as_array),
            object
                .get("logicOp")
                .and_then(Value::as_str)
                .unwrap_or(logic),
        )
    } else {
        (value.as_array(), logic)
    };
    let conditions = conditions.ok_or("LetsGal conditions must be an array")?;
    let join = match logic {
        "and" => " && ",
        "or" => " || ",
        _ => return Err(format!("unsupported condition logic {logic:?}")),
    };
    let mut expressions = Vec::new();
    for condition in conditions {
        let condition = condition.as_object().ok_or("invalid LetsGal condition")?;
        let text = |key: &str| {
            condition
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
        };
        if !matches!(text("sourceKind"), "" | "variable") {
            return Err("extension conditions need a native implementation".into());
        }
        let left = text("left");
        let operator = text("op");
        if operator == "custom" {
            expressions.push(format!("({})", text("rightLiteral")));
            continue;
        }
        if left.is_empty() {
            return Err("condition has no variable".into());
        }
        let expression = match operator {
            "isEmpty" => format!("{left} == \"\""),
            "isNotEmpty" => format!("{left} != \"\""),
            "==" | "!=" | ">" | ">=" | "<" | "<=" => {
                let right = match text("rightKind") {
                    "" | "literal" => expression_literal(text("rightLiteral")),
                    "variable" if !text("rightRef").is_empty() => text("rightRef").into(),
                    _ => return Err("invalid condition operand".into()),
                };
                format!("{left} {operator} {right}")
            }
            _ => return Err(format!("unsupported condition operator {operator:?}")),
        };
        expressions.push(format!("({expression})"));
    }
    Ok(if expressions.is_empty() {
        "false".into()
    } else {
        expressions.join(join)
    })
}

fn studio_visibility(source: &str) -> Result<Option<String>, String> {
    if source.trim().is_empty() {
        return Ok(None);
    }
    if source.trim_start().starts_with(['[', '{']) {
        let value =
            serde_json::from_str(source).map_err(|_| "invalid structured choice condition")?;
        studio_conditions(&value, "and").map(Some)
    } else {
        Ok(Some(source.into()))
    }
}

fn compile_branch(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let Some(source_choices) = (if block.props.contains_key("choices") {
        json_string::<Vec<BranchChoice>>(&block.props, "choices")
    } else {
        Some(Vec::new())
    }) else {
        logic_error(report, span, "invalid LetsGal branch choices");
        return;
    };
    let mut choices = Vec::new();
    for choice in source_choices {
        let target = match choice.mode.as_str() {
            "" | "jump" if choice.fragment_id.is_empty() => ChoiceTarget::Continue,
            "" | "jump" => ChoiceTarget::CallScene(choice.fragment_id),
            "change" if !choice.fragment_id.is_empty() => {
                ChoiceTarget::ChangeScene(choice.fragment_id)
            }
            "call" if !choice.fragment_id.is_empty() => ChoiceTarget::CallScene(choice.fragment_id),
            "vars" => {
                let mut assignments = Vec::new();
                for props in choice.var_ops {
                    if prop_string(&props, "key").is_empty() {
                        logic_error(report, span, "choice assignment has no variable");
                        return;
                    }
                    let mut assignment_report = ParseReport::default();
                    let assignment = StoryBlock {
                        props,
                        ..block.clone()
                    };
                    compile_set_variable(&assignment, span, &mut assignment_report);
                    if !assignment_report.diagnostics.is_empty() {
                        report.diagnostics.extend(assignment_report.diagnostics);
                        return;
                    }
                    if let Some(Action::Set {
                        name, expression, ..
                    }) = assignment_report.actions.pop()
                    {
                        assignments.push((name, expression));
                    }
                }
                ChoiceTarget::Assign(assignments)
            }
            _ => {
                logic_error(
                    report,
                    span,
                    format!("unsupported branch mode {:?}", choice.mode),
                );
                return;
            }
        };
        let show_when = match choice
            .visible_if
            .as_deref()
            .map(studio_visibility)
            .transpose()
        {
            Ok(value) => value.flatten(),
            Err(error) => {
                logic_error(report, span, error);
                return;
            }
        };
        choices.push(Choice {
            text: choice.text,
            target,
            show_when,
            enable_when: None,
        });
    }
    report.push(
        Action::Menu {
            prompt: prop_string(&block.props, "title"),
            choices,
        },
        span,
    );
}

fn compile_if(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let expression = if block.props.contains_key("conditions") {
        let Some(value) = json_value(&block.props, "conditions") else {
            logic_error(report, span, "invalid LetsGal conditions JSON");
            return;
        };
        match studio_conditions(&value, &prop_string_or(&block.props, "logicOp", "and")) {
            Ok(value) => value,
            Err(error) => {
                logic_error(report, span, error);
                return;
            }
        }
    } else {
        prop_string_or(&block.props, "expression", "false")
    };
    let then_scene = prop_string(&block.props, "thenFragmentId");
    let else_scene = prop_string(&block.props, "elseFragmentId");
    if then_scene.is_empty() {
        logic_error(report, span, "if has no then fragment");
        return;
    }
    report.push(
        Action::ConditionalCall {
            condition: expression,
            then_scene,
            else_scene: non_empty(else_scene),
        },
        span,
    );
}

fn compile_set_variable(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let name = prop_string(&block.props, "key");
    if name.is_empty() {
        logic_error(report, span, "assignment has no variable");
        return;
    }
    let op = prop_string_or(&block.props, "op", "=");
    let binary = prop_string_or(&block.props, "binOp", "+");
    let a_kind = prop_string_or(&block.props, "aKind", "lit");
    let b_kind = prop_string_or(&block.props, "bKind", "none");
    if !matches!(op.as_str(), "=" | "+=" | "-=" | "*=" | "/=" | "%=")
        || !matches!(binary.as_str(), "+" | "-" | "*" | "/" | "%")
        || !matches!(a_kind.as_str(), "lit" | "var")
        || !matches!(b_kind.as_str(), "none" | "lit" | "var")
        || (op != "=" && b_kind != "none")
        || (a_kind == "var" && prop_string(&block.props, "aVar").is_empty())
        || (b_kind == "var" && prop_string(&block.props, "bVar").is_empty())
    {
        logic_error(report, span, "invalid assignment operands or operator");
        return;
    }
    let operand = studio_operand(block, "a");
    let operand = if prop_string_or(&block.props, "bKind", "none") == "none" {
        operand
    } else {
        let right = studio_operand(block, "b");
        let operator = match prop_string_or(&block.props, "binOp", "+").as_str() {
            "+" | "-" | "*" | "/" | "%" => prop_string_or(&block.props, "binOp", "+"),
            _ => "+".into(),
        };
        format!("({operand}) {operator} ({right})")
    };
    let operator = prop_string_or(&block.props, "op", "=");
    let expression = match operator.as_str() {
        "+=" | "-=" | "*=" | "/=" | "%=" => {
            format!("{name} {} ({operand})", &operator[..1])
        }
        _ => operand,
    };
    report.push(
        Action::Set {
            name,
            expression,
            global: false,
        },
        span,
    );
}

fn studio_operand(block: &StoryBlock, prefix: &str) -> String {
    if prop_string(&block.props, &format!("{prefix}Kind")) == "var" {
        prop_string(&block.props, &format!("{prefix}Var"))
    } else {
        expression_literal(&prop_string(&block.props, &format!("{prefix}Lit")))
    }
}

fn compile_sound(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let kind = prop_string_or(&block.props, "soundType", "SE").to_ascii_uppercase();
    let file = prop_string(&block.props, "uri");
    let volume = prop_f32(&block.props, "volume", 100.0) / 100.0;
    if kind == "BGM" && !prop_bool(&block.props, "loop", true) {
        report.push(
            Action::EiyashouBgm {
                file: (!file.is_empty()).then_some(file),
                volume,
                fade_seconds: prop_f32(&block.props, "fadeDuration", 0.0) / 1000.0,
                looped: false,
            },
            span,
        );
    } else if kind == "BGM" {
        report.push(
            Action::Bgm {
                file,
                volume,
                fade_seconds: prop_f32(&block.props, "fadeDuration", 0.0) / 1000.0,
            },
            span,
        );
    } else if kind == "VOCAL" || kind == "VOICE" {
        if prop_bool(&block.props, "loop", false)
            || prop_f32(&block.props, "fadeDuration", 0.0) != 0.0
        {
            logic_error(
                report,
                span,
                "standalone voice loop/fade is unsupported; use a dialogue voice or a named sound effect",
            );
            return;
        }
        report.push(
            Action::Vocal {
                file: (!file.is_empty()).then_some(file),
                volume,
            },
            span,
        );
    } else {
        let looped = prop_bool(&block.props, "loop", false);
        let id = prop_string(&block.props, "soundId");
        let id = (!id.is_empty())
            .then_some(id)
            .or_else(|| looped.then(|| "letsgal-loop".into()));
        let fade = prop_f32(&block.props, "fadeDuration", 0.0) / 1000.0;
        report.push(
            if fade == 0.0 && (looped || id.is_none()) {
                Action::Effect {
                    file: (!file.is_empty()).then_some(file),
                    volume,
                    id,
                }
            } else {
                Action::SoundEffect {
                    file: (!file.is_empty()).then_some(file),
                    volume,
                    id,
                    looped,
                    fade,
                    fade_out: 0.0,
                }
            },
            span,
        );
    }
}

fn compile_stop_sound(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let kind = prop_string(&block.props, "soundType");
    if kind.eq_ignore_ascii_case("BGM") {
        report.push(
            Action::Bgm {
                file: "none".into(),
                volume: 0.0,
                fade_seconds: prop_f32(&block.props, "fadeDuration", 0.0) / 1000.0,
            },
            span,
        );
    } else if kind.eq_ignore_ascii_case("VOCAL") || kind.eq_ignore_ascii_case("VOICE") {
        report.push(
            Action::Vocal {
                file: None,
                volume: 0.0,
            },
            span,
        );
    } else {
        let id = prop_string(&block.props, "soundId");
        report.push(
            Action::SoundEffect {
                file: None,
                volume: 1.0,
                id: (!id.is_empty()).then_some(id),
                looped: false,
                fade: prop_f32(&block.props, "fadeDuration", 0.0) / 1000.0,
                fade_out: 0.0,
            },
            span,
        );
    }
}

fn compile_video(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let file = prop_string(&block.props, "uri");
    if file.is_empty() {
        return;
    }
    let looped = prop_bool(&block.props, "loop", false);
    report.push(
        Action::PlayVideo {
            video: VideoSpec {
                id: prop_string_or(&block.props, "videoId", "video"),
                file,
                looped,
                muted: prop_bool(&block.props, "muted", false),
                alpha: (prop_f32(&block.props, "alpha", 100.0) / 100.0).clamp(0.0, 1.0),
                skippable: true,
                wait_for_finished: !looped && prop_bool(&block.props, "waitForFinished", false),
                mode: if prop_string(&block.props, "mode") == "mixed" {
                    VideoMode::Mixed
                } else {
                    VideoMode::Fullscreen
                },
            },
        },
        span,
    );
}

fn compile_stop_video(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let id = prop_string(&block.props, "videoId");
    report.push(
        Action::StopVideo {
            id: (!id.is_empty() && id != "all").then_some(id),
            fade_out: prop_f32(&block.props, "fadeOutDuration", 0.0).max(0.0) / 1000.0,
        },
        span,
    );
}

fn compile_camera(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let mut patch = TransformPatch::default();
    if let Some(value) = optional_f32(&block.props, "offsetX") {
        patch.set_offset_x(value);
    }
    if let Some(value) = optional_f32(&block.props, "offsetY") {
        patch.set_offset_y(value);
    }
    if let Some(zoom) = optional_f32(&block.props, "zoom") {
        patch.set_scale_x(zoom);
        patch.set_scale_y(zoom);
    }
    let duration = prop_f32(&block.props, "duration", 0.0) / 1000.0;
    let targets = camera_targets(block);
    let wait = prop_bool(&block.props, "waitForComplete", true);
    let mut timed = Vec::new();
    if !patch.is_empty() {
        timed.push(Action::SetCameraTransform {
            targets,
            transform: patch,
            duration,
            easing: easing(&prop_string(&block.props, "easing")),
            blocking: wait,
        });
    }
    let post_process = post_process_patch(block);
    if !post_process.is_empty() {
        timed.push(Action::SetPostProcess {
            targets,
            effect: Box::new(post_process),
            duration,
            easing: easing(&prop_string(&block.props, "easing")),
            blocking: wait,
        });
    }
    if let Some(effect) = post_process_v2(block) {
        timed.push(Action::SetPostProcessV2 {
            targets,
            effect: Box::new(effect),
            duration,
            easing: easing(&prop_string(&block.props, "easing")),
            blocking: wait,
        });
    }
    // Studio stores camelCase field names. Absence retains the legacy whole-command behavior.
    if block.props.contains_key("tweenFields") {
        let mut fields = Vec::new();
        for name in prop_string(&block.props, "tweenFields")
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            let mapped = match name {
                "offsetX" => vec![CameraTweenField::X],
                "offsetY" => vec![CameraTweenField::Y],
                "zoom" => vec![CameraTweenField::ScaleX, CameraTweenField::ScaleY],
                _ => CameraTweenField::ALL
                    .iter()
                    .copied()
                    .filter(|field| field.name().replace('_', "").eq_ignore_ascii_case(name))
                    .collect(),
            };
            if mapped.is_empty() {
                // Studio's mask may include effects with explicitly unset values.
                // They do not change state and require no runtime capability.
                if block.props.get(name).is_some_and(|value| {
                    value.is_null() || value.as_str().is_some_and(|value| value.trim().is_empty())
                }) {
                    continue;
                }
                report.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    span,
                    message: format!("unsupported LetsGal numeric tween field {name:?}"),
                });
                return;
            }
            for field in mapped {
                if !fields.contains(&field) {
                    fields.push(field);
                }
            }
        }
        let mut spec = CameraTweenSpec {
            targets,
            transform: None,
            effect: None,
            v2: None,
            shake: camera_shake_spec(block),
            fields,
            duration,
            easing: easing(&prop_string(&block.props, "easing")),
            blocking: wait,
        };
        for action in timed.drain(..) {
            match action {
                Action::SetCameraTransform { transform, .. } => spec.transform = Some(transform),
                Action::SetPostProcess { effect, .. } => spec.effect = Some(effect),
                Action::SetPostProcessV2 { effect, .. } => spec.v2 = Some(effect),
                _ => unreachable!("camera components are typed above"),
            }
        }
        if spec.transform.is_some()
            || spec.effect.is_some()
            || spec.v2.is_some()
            || spec.shake.is_some()
        {
            timed.push(Action::SetCameraTween {
                spec: Box::new(spec),
            });
        }
    }
    let timed_len = timed.len();
    for (index, action) in timed.into_iter().enumerate() {
        report.push(
            Action::Flow {
                action: Box::new(action),
                when: None,
                next: !wait || index + 1 < timed_len,
            },
            span,
        );
    }
    if !block.props.contains_key("tweenFields")
        && let Some(CameraShakeTweenSpec { shake, randomness }) = camera_shake_spec(block)
    {
        let blocking = prop_bool(&block.props, "shakeWaitForComplete", false);
        report.push(
            if randomness.is_zero() {
                Action::ShakeCamera {
                    targets,
                    shake,
                    blocking,
                }
            } else {
                Action::ShakeCameraRandomized {
                    targets,
                    shake,
                    randomness,
                    blocking,
                }
            },
            span,
        );
    }
}

fn camera_shake_spec(block: &StoryBlock) -> Option<CameraShakeTweenSpec> {
    let values = optional_f32(&block.props, "shakeAmplitude")
        .zip(optional_f32(&block.props, "shakeFrequency"))
        .zip(
            optional_f32(&block.props, "duration")
                .filter(|value| *value > 0.0)
                .or_else(|| optional_f32(&block.props, "shakeDuration")),
        );
    values.map(|((amplitude, frequency), duration_ms)| {
        let randomness = CameraShakeRandomness {
            amplitude: (prop_f32(&block.props, "shakeAmplitudeRandomness", 0.0) / 100.0)
                .clamp(0.0, 1.0),
            frequency: (prop_f32(&block.props, "shakeFrequencyRandomness", 0.0) / 100.0)
                .clamp(0.0, 1.0),
        };
        let shake = CameraShakeSpec {
            amplitude,
            frequency,
            duration: duration_ms.max(0.0) / 1000.0,
            axis: match prop_string(&block.props, "shakeAxis").as_str() {
                "x" => CameraShakeAxis::X,
                "y" => CameraShakeAxis::Y,
                _ => CameraShakeAxis::Both,
            },
            falloff: if prop_string(&block.props, "shakeFalloff") == "expo" {
                CameraShakeFalloff::Exponential
            } else {
                CameraShakeFalloff::Linear
            },
        };
        CameraShakeTweenSpec { shake, randomness }
    })
}

fn post_process_v2(block: &StoryBlock) -> Option<PostProcessV2> {
    let props = &block.props;
    let present = [
        "mirrorShatterIntensity",
        "mirrorShatterCenterX",
        "mirrorShatterCenterY",
        "speedLinesIntensity",
        "speedLinesRegionMode",
    ]
    .iter()
    .any(|key| props.contains_key(*key));
    present.then(|| PostProcessV2 {
        mirror_shatter_intensity: prop_f32(props, "mirrorShatterIntensity", 0.0).clamp(0.0, 1.0),
        mirror_shatter_center_x: prop_f32(props, "mirrorShatterCenterX", 0.5),
        mirror_shatter_center_y: prop_f32(props, "mirrorShatterCenterY", 0.5),
        mirror_shatter_spread: prop_f32(props, "mirrorShatterSpread", 1.0).clamp(0.0, 3.0),
        mirror_shatter_seed: prop_f32(props, "mirrorShatterSeed", 0.0),
        speed_lines_intensity: prop_f32(props, "speedLinesIntensity", 0.0).clamp(0.0, 1.0),
        speed_lines_radial: prop_string_or(props, "speedLinesMode", "radial") == "radial",
        speed_lines_density: prop_f32(props, "speedLinesDensity", 0.55).clamp(0.0, 1.0),
        speed_lines_angle: prop_f32(props, "speedLinesAngle", 0.0).clamp(-180.0, 180.0),
        speed_lines_speed: prop_f32(props, "speedLinesSpeed", 0.0),
        speed_lines_center_x: prop_f32(props, "speedLinesCenterX", 0.5),
        speed_lines_center_y: prop_f32(props, "speedLinesCenterY", 0.5),
        speed_lines_region_ellipse: prop_string_or(props, "speedLinesRegionShape", "rectangle")
            == "ellipse",
        speed_lines_region_x: prop_f32(props, "speedLinesRegionX", 0.5),
        speed_lines_region_y: prop_f32(props, "speedLinesRegionY", 0.5),
        speed_lines_region_width: prop_f32(props, "speedLinesRegionWidth", 1.0).max(0.0),
        speed_lines_region_height: prop_f32(props, "speedLinesRegionHeight", 1.0).max(0.0),
        speed_lines_region_feather: if prop_string_or(props, "speedLinesRegionMode", "full")
            == "custom"
        {
            prop_f32(props, "speedLinesRegionFeather", 0.05).max(0.0)
        } else {
            1.0
        },
    })
}

fn camera_targets(block: &StoryBlock) -> CameraTargets {
    let requested = prop_string_or(&block.props, "targets", "scene,characters");
    let scene = requested.split(',').any(|target| target.trim() == "scene");
    let characters = requested
        .split(',')
        .any(|target| target.trim() == "characters");
    CameraTargets::new(scene, characters)
}

fn post_process_patch(block: &StoryBlock) -> PostProcessPatch {
    post_process_patch_from_props(&block.props)
}

fn post_process_patch_from_props(props: &Map<String, Value>) -> PostProcessPatch {
    let color_tone = match prop_string(props, "colorToneMode").as_str() {
        "grayscale" => Some(ColorToneMode::Grayscale),
        "sepia" => Some(ColorToneMode::Sepia),
        "none" => Some(ColorToneMode::None),
        _ => None,
    };
    let lut = prop_string(props, "lutPreset");
    PostProcessPatch {
        focal_distance: optional_f32(props, "focalDistance").map(Some),
        blur_strength: optional_f32(props, "blurStrength"),
        distortion_strength: optional_f32(props, "distortionStrength"),
        vignette_intensity: optional_f32(props, "vignetteIntensity"),
        vignette_size: optional_f32(props, "vignetteSize"),
        blur_amount: optional_f32(props, "blurAmount"),
        color_tone,
        color_tone_intensity: optional_f32(props, "colorToneIntensity"),
        color_exposure: optional_f32(props, "colorExposure"),
        color_brightness: optional_f32(props, "colorBrightness"),
        color_contrast: optional_f32(props, "colorContrast"),
        color_saturation: optional_f32(props, "colorSaturation"),
        color_temperature: optional_f32(props, "colorTemperature"),
        old_film_intensity: optional_f32(props, "oldFilmIntensity"),
        shock_intensity: optional_f32(props, "shockIntensity"),
        godray_intensity: optional_f32(props, "godrayIntensity"),
        godray_angle: optional_f32(props, "godrayAngle"),
        godray_gain: optional_f32(props, "godrayGain"),
        godray_lacunarity: optional_f32(props, "godrayLacunarity"),
        godray_speed: optional_f32(props, "godraySpeed"),
        godray_parallel: optional_bool(props, "godrayParallel"),
        godray_center_x: optional_f32(props, "godrayCenterX"),
        godray_center_y: optional_f32(props, "godrayCenterY"),
        lut_preset: (!lut.is_empty()).then_some(Some(lut)),
        lut_intensity: optional_f32(props, "lutIntensity"),
        bloom_intensity: optional_f32(props, "bloomIntensity"),
        chromatic_aberration: optional_f32(props, "chromaticAberration"),
        pixelate_size: optional_f32(props, "pixelateSize"),
        glitch_intensity: optional_f32(props, "glitchIntensity"),
        crt_intensity: optional_f32(props, "crtIntensity"),
        sharpen_strength: optional_f32(props, "sharpenStrength"),
        radial_blur_strength: optional_f32(props, "radialBlurStrength"),
        radial_blur_center_x: optional_f32(props, "radialBlurCenterX"),
        radial_blur_center_y: optional_f32(props, "radialBlurCenterY"),
        motion_blur_strength: optional_f32(props, "motionBlurStrength"),
        motion_blur_angle: optional_f32(props, "motionBlurAngle"),
        zoom_blur_strength: optional_f32(props, "zoomBlurStrength"),
        zoom_blur_center_x: optional_f32(props, "zoomBlurCenterX"),
        zoom_blur_center_y: optional_f32(props, "zoomBlurCenterY"),
        light_leak_intensity: optional_f32(props, "lightLeakIntensity"),
        light_leak_angle: optional_f32(props, "lightLeakAngle"),
        lens_flare_intensity: optional_f32(props, "lensFlareIntensity"),
        lens_flare_center_x: optional_f32(props, "lensFlareCenterX"),
        lens_flare_center_y: optional_f32(props, "lensFlareCenterY"),
        film_grain_intensity: optional_f32(props, "filmGrainIntensity"),
        film_grain_size: optional_f32(props, "filmGrainSize"),
        heat_haze_intensity: optional_f32(props, "heatHazeIntensity"),
        heat_haze_speed: optional_f32(props, "heatHazeSpeed"),
        heat_haze_scale: optional_f32(props, "heatHazeScale"),
        water_ripple_intensity: optional_f32(props, "waterRippleIntensity"),
        water_ripple_frequency: optional_f32(props, "waterRippleFrequency"),
        water_ripple_speed: optional_f32(props, "waterRippleSpeed"),
        water_ripple_center_x: optional_f32(props, "waterRippleCenterX"),
        water_ripple_center_y: optional_f32(props, "waterRippleCenterY"),
        fog_intensity: optional_f32(props, "fogIntensity"),
        fog_speed: optional_f32(props, "fogSpeed"),
        fog_scale: optional_f32(props, "fogScale"),
        vhs_intensity: optional_f32(props, "vhsIntensity"),
        vhs_jitter: optional_f32(props, "vhsJitter"),
        vhs_noise: optional_f32(props, "vhsNoise"),
        halftone_intensity: optional_f32(props, "halftoneIntensity"),
        halftone_scale: optional_f32(props, "halftoneScale"),
        halftone_angle: optional_f32(props, "halftoneAngle"),
        dither_intensity: optional_f32(props, "ditherIntensity"),
        dither_levels: optional_f32(props, "ditherLevels"),
        outline_intensity: optional_f32(props, "outlineIntensity"),
        outline_thickness: optional_f32(props, "outlineThickness"),
        eyelid_openness: optional_f32(props, "eyelidOpenness"),
        eyelid_width: optional_f32(props, "eyelidWidth"),
        eyelid_curvature: optional_f32(props, "eyelidCurvature"),
        eyelid_softness: optional_f32(props, "eyelidSoftness"),
        eyelid_center_x: optional_f32(props, "eyelidCenterX"),
        eyelid_center_y: optional_f32(props, "eyelidCenterY"),
        ..Default::default()
    }
}

fn compile_reset_camera(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let duration = if prop_string(&block.props, "resetMode") == "animated" {
        prop_f32(&block.props, "duration", 500.0).max(0.0) / 1000.0
    } else {
        0.0
    };
    let wait = prop_bool(&block.props, "waitForComplete", true);
    let easing = easing(&prop_string(&block.props, "easing"));
    push_camera_reset(duration, easing, wait, span, report);
}

fn push_camera_reset(
    duration: f32,
    easing: Easing,
    wait: bool,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    for action in crate::adapter::camera::reset(CameraTargets::ALL, duration, easing, wait) {
        report.push(action, span);
    }
}

fn compile_curtain(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let closed = prop_string(&block.props, "op") == "close";
    let mode = prop_string(&block.props, "mode");
    if matches!(mode.as_str(), "pillarbox" | "windowbox")
        || (mode == "letterbox" && prop_f32(&block.props, "curtainSize", 100.0) != 100.0)
    {
        logic_error(
            report,
            span,
            "custom curtain bars require native stage masks",
        );
        return;
    }
    if mode == "letterbox" {
        report.push(Action::FilmMode { enabled: closed }, span);
        return;
    }
    report.push(
        Action::Curtain {
            visible: closed,
            color: parse_color(&prop_string_or(&block.props, "color", "#000000")),
            duration: prop_f32(&block.props, "duration", 0.0) / 1000.0,
        },
        span,
    );
}

fn compile_stage_mask(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let id = prop_string_or(&block.props, "maskId", "mask");
    if prop_string(&block.props, "action") == "remove" {
        report.push(
            Action::StageMask {
                id,
                mask: None,
                duration: prop_f32(&block.props, "exitDuration", 0.0).max(0.0) / 1000.0,
                blocking: true,
            },
            span,
        );
        return;
    }
    if prop_string_or(&block.props, "coordinateSpace", "screen") != "screen" {
        report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: "unsupported stageMask coordinate space; expected screen".into(),
        });
        return;
    }

    let shape = match prop_string_or(&block.props, "shape", "rect").as_str() {
        "rounded" | "rounded-rect" => StageMaskShape::RoundedRectangle,
        "ellipse" => StageMaskShape::Ellipse,
        "image" => StageMaskShape::Image,
        _ => StageMaskShape::Rectangle,
    };
    let image = non_empty(prop_string(&block.props, "image"));
    if shape == StageMaskShape::Image && image.is_none() {
        report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: "stageMask image shape has no mask asset".into(),
        });
        return;
    }
    let fill_mode = match prop_string_or(&block.props, "fillMode", "solid").as_str() {
        "gradient" => StageMaskFillMode::Gradient,
        "texture" => StageMaskFillMode::Texture,
        _ => StageMaskFillMode::Solid,
    };
    let texture = non_empty(prop_string(&block.props, "texture"));
    if fill_mode == StageMaskFillMode::Texture && texture.is_none() {
        report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: "stageMask texture fill has no texture asset".into(),
        });
        return;
    }

    let mask = StageMask {
        mode: if prop_string(&block.props, "mode") == "clip" {
            StageMaskMode::Clip
        } else {
            StageMaskMode::Overlay
        },
        plane: match prop_string_or(&block.props, "plane", "bottom").as_str() {
            "behind-scene" => StageMaskPlane::BehindScene,
            "top" => StageMaskPlane::Top,
            "topmost" => StageMaskPlane::Topmost,
            _ => StageMaskPlane::Bottom,
        },
        scope: match prop_string_or(&block.props, "scope", "stage").as_str() {
            "scene" => StageMaskScope::Scene,
            "stage" => StageMaskScope::All,
            "characters" | "character" => StageMaskScope::Characters,
            "selected" => StageMaskScope::Selected,
            _ => StageMaskScope::All,
        },
        targets: stage_mask_targets(&block.props),
        shape,
        image,
        image_channel: if prop_string(&block.props, "imageChannel") == "luminance" {
            StageMaskImageChannel::Luminance
        } else {
            StageMaskImageChannel::Alpha
        },
        image_fit: stage_mask_fit(&prop_string_or(&block.props, "imageFit", "stretch")),
        center: [
            prop_f32(&block.props, "centerX", 50.0).clamp(-500.0, 500.0),
            prop_f32(&block.props, "centerY", 50.0).clamp(-500.0, 500.0),
        ],
        size: [
            prop_f32(&block.props, "width", 50.0).clamp(0.01, 1000.0),
            prop_f32(&block.props, "height", 50.0).clamp(0.01, 1000.0),
        ],
        rotation: prop_f32(&block.props, "rotation", 0.0).to_radians(),
        radius: prop_f32(&block.props, "radius", 24.0).max(0.0),
        visibility: if prop_string(&block.props, "visibility") == "outside" {
            StageMaskVisibility::Outside
        } else {
            StageMaskVisibility::Inside
        },
        feather: prop_f32(&block.props, "feather", 0.0).clamp(0.0, 512.0),
        opacity: (prop_f32(&block.props, "opacity", 100.0) / 100.0).clamp(0.0, 1.0),
        fill_mode,
        color: parse_color(&prop_string_or(&block.props, "color", "#000000")),
        gradient_start: parse_color(&prop_string_or(&block.props, "gradientStart", "#000000")),
        gradient_end: parse_color(&prop_string_or(&block.props, "gradientEnd", "#243247")),
        gradient_direction: prop_f32(&block.props, "gradientDirection", 0.0).to_radians(),
        texture,
        texture_fit: stage_mask_fit(&prop_string_or(&block.props, "textureFit", "cover")),
        texture_blend: match prop_string_or(&block.props, "textureBlend", "normal").as_str() {
            "multiply" => StageMaskTextureBlend::Multiply,
            "screen" => StageMaskTextureBlend::Screen,
            "add" => StageMaskTextureBlend::Add,
            _ => StageMaskTextureBlend::Normal,
        },
        texture_scale: (prop_f32(&block.props, "textureScale", 100.0) / 100.0).clamp(0.01, 20.0),
        texture_opacity: (prop_f32(&block.props, "textureOpacity", 100.0) / 100.0).clamp(0.0, 1.0),
        blur: if prop_bool(&block.props, "blurEnabled", false) {
            prop_f32(&block.props, "blurAmount", 8.0).clamp(0.0, 64.0)
        } else {
            0.0
        },
        vignette_amount: if prop_bool(&block.props, "vignetteEnabled", false) {
            (prop_f32(&block.props, "vignetteAmount", 35.0) / 100.0).clamp(0.0, 1.0)
        } else {
            0.0
        },
        vignette_size: (prop_f32(&block.props, "vignetteSize", 55.0) / 100.0).clamp(0.0, 1.0),
        noise_amount: if prop_bool(&block.props, "noiseEnabled", false) {
            (prop_f32(&block.props, "noiseAmount", 12.0) / 100.0).clamp(0.0, 1.0)
        } else {
            0.0
        },
        noise_size: prop_f32(&block.props, "noiseSize", 45.0).clamp(1.0, 512.0),
        hue: if prop_bool(&block.props, "colorAdjustmentEnabled", false) {
            prop_f32(&block.props, "colorHue", 0.0).to_radians()
        } else {
            0.0
        },
        saturation: if prop_bool(&block.props, "colorAdjustmentEnabled", false) {
            (prop_f32(&block.props, "colorSaturation", 100.0) / 100.0).clamp(0.0, 4.0)
        } else {
            1.0
        },
        brightness: if prop_bool(&block.props, "colorAdjustmentEnabled", false) {
            (prop_f32(&block.props, "colorBrightness", 100.0) / 100.0).clamp(0.0, 4.0)
        } else {
            1.0
        },
    };
    report.push(
        Action::StageMask {
            id,
            mask: Some(Box::new(mask)),
            duration: prop_f32(&block.props, "enterDuration", 0.0).max(0.0) / 1000.0,
            blocking: true,
        },
        span,
    );
}

fn stage_mask_fit(value: &str) -> StageMaskFit {
    match value {
        "cover" => StageMaskFit::Cover,
        "contain" | "fit" => StageMaskFit::Contain,
        _ => StageMaskFit::Stretch,
    }
}

fn stage_mask_targets(props: &Map<String, Value>) -> Vec<String> {
    fn collect(value: &Value, targets: &mut Vec<String>) {
        match value {
            Value::String(value) if !value.trim().is_empty() => targets.push(value.clone()),
            Value::Array(values) => values.iter().for_each(|value| collect(value, targets)),
            Value::Object(value) => {
                for key in ["id", "targetId", "sceneId", "layerId", "characterId"] {
                    if let Some(Value::String(id)) = value.get(key)
                        && !id.trim().is_empty()
                    {
                        targets.push(id.clone());
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    let mut targets = Vec::new();
    if let Some(value) = json_value(props, "targetsJson") {
        collect(&value, &mut targets);
    }
    let mut seen = BTreeSet::new();
    targets.retain(|target| seen.insert(target.clone()));
    targets
}

fn compile_floating_text(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let infinite = prop_bool(&block.props, "infinite", false);
    report.push(
        Action::FloatingText {
            text: plain_text(&block.content),
            position: parse_position(&prop_string(&block.props, "position")),
            font_size: prop_f32(&block.props, "fontSize", 50.0),
            color: parse_color(&prop_string_or(&block.props, "color", "#ffffff")),
            fade_in: prop_f32(&block.props, "inDuration", 0.0) / 1000.0,
            hold: prop_f32(&block.props, "duration", 0.0) / 1000.0,
            fade_out: prop_f32(&block.props, "outDuration", 0.0) / 1000.0,
            blocking: !infinite && prop_bool(&block.props, "blocking", false),
        },
        span,
    );
    let id = non_empty(prop_string(&block.props, "floatingTextId"));
    if id.is_some() || infinite {
        report.push(Action::ConfigureFloatingText { id, infinite }, span);
    }
}

fn compile_system_message(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let mode = if prop_string(&block.props, "mode") == "confirm" {
        SystemMessageMode::Confirm
    } else {
        SystemMessageMode::Alert
    };
    report.push(
        Action::SystemMessage {
            spec: SystemMessageSpec {
                mode,
                title: prop_string(&block.props, "title"),
                message: prop_string(&block.props, "message"),
                confirm_text: prop_string_or(&block.props, "confirmText", "确认"),
                cancel_text: prop_string_or(&block.props, "cancelText", "取消"),
                result_variable: non_empty(prop_string(&block.props, "resultVariable")),
            },
        },
        span,
    );
}

fn paragraph_text_reveal(props: &Map<String, Value>) -> Option<TextRevealConfig> {
    let effect = prop_string(props, "revealEffect");
    let duration = optional_f32(props, "charFadeIn").filter(|value| *value >= 0.0);
    let distance = optional_f32(props, "revealDistance").filter(|value| *value >= 0.0);
    let scale = optional_f32(props, "revealScale").filter(|value| *value >= 0.0);
    let rotation = optional_f32(props, "revealRotation").filter(|value| *value >= 0.0);
    let blur = optional_f32(props, "revealBlur").filter(|value| *value >= 0.0);
    if effect.is_empty()
        && duration.is_none()
        && distance.is_none()
        && scale.is_none()
        && rotation.is_none()
        && blur.is_none()
    {
        return None;
    }
    let defaults = TextRevealConfig::default();
    Some(TextRevealConfig {
        duration: duration.map_or(defaults.duration, |value| value / 1000.0),
        effect: serde_json::from_value(Value::String(effect)).unwrap_or(defaults.effect),
        distance: distance.unwrap_or(defaults.distance).clamp(0.0, 48.0),
        scale: scale
            .map_or(defaults.scale, |value| value / 100.0)
            .clamp(0.3, 1.0),
        rotation: rotation.unwrap_or(defaults.rotation).clamp(0.0, 180.0),
        blur: blur.unwrap_or(defaults.blur).clamp(0.0, 12.0),
    })
}

fn compile_portrait_rule(
    block: &StoryBlock,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    let style = |state: &str| match state {
        "speaking" => portrait_style(block, "speaking"),
        "listening" => portrait_style(block, "listening"),
        "inactive" => portrait_style(block, "inactive"),
        _ => PortraitStyle::default(),
    };
    report.push(
        Action::ConfigurePortraits {
            enabled: prop_bool(&block.props, "enabled", true),
            character_ids: context
                .characters
                .values()
                .map(|character| character.id.clone())
                .collect(),
            speaking: portrait_style(block, "speaking"),
            others: style(&prop_string_or(&block.props, "othersState", "inactive")),
            narration: style(&prop_string_or(&block.props, "narrationState", "listening")),
            duration: prop_f32(&block.props, "transitionDuration", 180.0) / 1000.0,
            easing: easing(&prop_string(&block.props, "transitionEasing")),
        },
        span,
    );
}

fn portrait_style(block: &StoryBlock, prefix: &str) -> PortraitStyle {
    let value =
        |suffix: &str, fallback| prop_f32(&block.props, &format!("{prefix}{suffix}"), fallback);
    PortraitStyle {
        scale: value("Scale", 1.0),
        brightness: value("Brightness", 1.0),
        saturation: value("Saturation", 1.0),
        contrast: value("Contrast", 1.0),
        blur: value("Blur", 0.0),
        alpha: value("Alpha", 1.0),
    }
}

fn parse_position(value: &str) -> [f32; 2] {
    let values = value
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .map(|part| part.trim().trim_end_matches('%').parse::<f32>().ok())
        .collect::<Vec<_>>();
    match values.as_slice() {
        [Some(x), Some(y)] => [
            keine_core::DESIGN_WIDTH * *x / 100.0,
            keine_core::DESIGN_HEIGHT * *y / 100.0,
        ],
        _ => [
            keine_core::DESIGN_WIDTH * 0.5,
            keine_core::DESIGN_HEIGHT * 0.5,
        ],
    }
}

fn scene_layer_layout(block: &StoryBlock) -> SceneLayerLayout {
    scene_layer_layout_from_props(&block.props)
}

fn scene_layer_layout_from_props(props: &Map<String, Value>) -> SceneLayerLayout {
    let fit = match prop_string_or(props, "displayType", "cover").as_str() {
        "contain" => SceneFit::Contain,
        "by_width" => SceneFit::ByWidth,
        "by_height" => SceneFit::ByHeight,
        "stretch" => SceneFit::Stretch,
        "center" => SceneFit::Center,
        _ => SceneFit::Cover,
    };
    let position = parse_studio_pair(
        &prop_string_or(props, "position", "(center,center)"),
        [
            keine_core::DESIGN_WIDTH * 0.5,
            keine_core::DESIGN_HEIGHT * 0.5,
        ],
    );
    let anchor = match prop_string_or(props, "anchor", "center").as_str() {
        "top-left" => [0.0, 0.0],
        "top" | "top-center" => [0.5, 0.0],
        "top-right" => [1.0, 0.0],
        "left" | "center-left" => [0.0, 0.5],
        "right" | "center-right" => [1.0, 0.5],
        "bottom-left" => [0.0, 1.0],
        "bottom" | "bottom-center" => [0.5, 1.0],
        "bottom-right" => [1.0, 1.0],
        _ => [0.5, 0.5],
    };
    let raw_size = prop_string(props, "size");
    let size = (!raw_size.trim().is_empty())
        .then(|| parse_studio_pair(&raw_size, [0.0, 0.0]))
        .filter(|size| size[0] > 0.0 && size[1] > 0.0);
    SceneLayerLayout {
        fit,
        position,
        anchor,
        size,
    }
}

fn parse_studio_pair(value: &str, fallback: [f32; 2]) -> [f32; 2] {
    let parts = value
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .map(str::trim)
        .collect::<Vec<_>>();
    let [x, y] = parts.as_slice() else {
        return fallback;
    };
    [
        parse_studio_coordinate(x, keine_core::DESIGN_WIDTH).unwrap_or(fallback[0]),
        parse_studio_coordinate(y, keine_core::DESIGN_HEIGHT).unwrap_or(fallback[1]),
    ]
}

fn parse_color(value: &str) -> [f32; 4] {
    let hex = value.trim().trim_start_matches('#');
    let parse = |range| u8::from_str_radix(&hex[range], 16).ok();
    if hex.len() == 6
        && hex.is_ascii()
        && let (Some(r), Some(g), Some(b)) = (parse(0..2), parse(2..4), parse(4..6))
    {
        return [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0];
    }
    [0.0, 0.0, 0.0, 1.0]
}

#[derive(Deserialize)]
struct StudioKeyframe {
    #[serde(default)]
    duration: f32,
    #[serde(default)]
    easing: String,
    #[serde(default)]
    properties: Map<String, Value>,
}

#[derive(Default, Deserialize)]
struct StudioParticleOverrides {
    count: Option<u32>,
    wind: Option<f32>,
    gravity: Option<f32>,
    #[serde(flatten)]
    unsupported: BTreeMap<String, Value>,
}

fn studio_particle_effect(
    texture: &str,
    preset: &str,
    options: StudioParticleOverrides,
    fade_in: f32,
) -> keine_core::ParticleEffect {
    keine_core::ParticleEffect {
        texture: (!texture.is_empty()).then(|| texture.to_owned()),
        preset: if preset.is_empty() {
            "LIGHT_SNOW".into()
        } else {
            preset.to_owned()
        },
        count: options.count.unwrap_or(0).min(u16::MAX as u32) as u16,
        wind: options.wind,
        gravity: options.gravity,
        fade_in,
    }
}

fn compile_particle(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let id = prop_string_or(
        &block.props,
        "effectId",
        block.id.as_deref().unwrap_or("particle"),
    );
    if prop_string(&block.props, "mode") == "hide" {
        report.push(
            Action::HideParticles {
                id: Some(id),
                duration: prop_f32(&block.props, "fadeOutDuration", 500.0).max(0.0) / 1000.0,
            },
            span,
        );
        return;
    }

    let texture = prop_string(&block.props, "textureUri");
    let options = if !block.props.contains_key("optionsJson")
        || prop_string(&block.props, "optionsJson").trim().is_empty()
    {
        StudioParticleOverrides::default()
    } else {
        let Some(options) = json_string::<StudioParticleOverrides>(&block.props, "optionsJson")
        else {
            logic_error(report, span, "invalid particle overrides");
            return;
        };
        options
    };
    if !options.unsupported.is_empty() {
        logic_error(
            report,
            span,
            format!(
                "unsupported particle overrides: {}; use native particle.show controls",
                options
                    .unsupported
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
        return;
    }
    report.push(
        Action::ShowParticles {
            id,
            effect: studio_particle_effect(
                &texture,
                &prop_string_or(&block.props, "preset", "LIGHT_SNOW"),
                options,
                prop_f32(&block.props, "fadeInDuration", 500.0).max(0.0) / 1000.0,
            ),
        },
        span,
    );
}

fn compile_animate_sprite(
    block: &StoryBlock,
    _context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    let target = prop_string(&block.props, "targetId");
    let target = if prop_string(&block.props, "targetType") == "sceneLayer" {
        format!("scene-layer:{target}")
    } else {
        target
    };
    let frames = json_string::<Vec<StudioKeyframe>>(&block.props, "frames").unwrap_or_default();
    if !frames.is_empty() {
        report.push(
            Action::AnimateKeyframes {
                target,
                frames: frames
                    .into_iter()
                    .map(|frame| TransformKeyframe {
                        transform: sprite_transform_patch(&frame.properties),
                        duration: frame.duration.max(0.0) / 1000.0,
                        easing: easing(&frame.easing),
                    })
                    .collect(),
                repeat: prop_f32(&block.props, "loop", 0.0).max(0.0) as u32,
                blocking: prop_bool(&block.props, "waitForComplete", true),
            },
            span,
        );
        return;
    }
    // Keep even the compact, single-frame form on the same native timeline as
    // Studio's frame-array form. A plain SetTransform is always blocking in
    // keine and would silently discard Studio's loop/waitForComplete flags.
    report.push(
        Action::AnimateKeyframes {
            target,
            frames: vec![TransformKeyframe {
                transform: sprite_transform_patch(&block.props),
                duration: prop_f32(&block.props, "duration", 0.0).max(0.0) / 1000.0,
                easing: easing(&prop_string(&block.props, "easing")),
            }],
            repeat: prop_f32(&block.props, "loop", 0.0).max(0.0) as u32,
            blocking: prop_bool(&block.props, "waitForComplete", true),
        },
        span,
    );
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StudioStageClip {
    duration: f32,
    tracks: Vec<StudioStageTrack>,
    events: Vec<Value>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StudioStageTrack {
    target: StudioStageTarget,
    property: String,
    keyframes: Vec<StudioStageKeyframe>,
    muted: bool,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StudioStageTarget {
    kind: String,
    id: String,
    character_id: String,
    expression_name: String,
    asset_path: String,
    layer_id: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct StudioStageKeyframe {
    time: f32,
    value: f32,
    easing: String,
}

fn compile_stage_animation(
    block: &StoryBlock,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    let Some(raw_clip) = json_value(&block.props, "clipJson") else {
        report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: "LetsGal stageAnimation has no valid clipJson".into(),
        });
        return;
    };
    let Ok(clip) = serde_json::from_value::<StudioStageClip>(raw_clip) else {
        report.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            span,
            message: "LetsGal stageAnimation clipJson has an invalid 1.9 timeline schema".into(),
        });
        return;
    };

    let mut tracks = Vec::new();
    for track in clip.tracks {
        let property = track.property.clone();
        let muted = track.muted;
        match compile_stage_track(track, context) {
            Some(track) => tracks.push(track),
            None if muted => {}
            None => logic_error(
                report,
                span,
                format!("unsupported stage track target/property {property:?}"),
            ),
        }
    }
    for track in &mut tracks {
        track
            .keyframes
            .sort_by(|left, right| left.time.total_cmp(&right.time));
    }
    let mut events = Vec::new();
    for event in &clip.events {
        match compile_stage_event(event, context) {
            Some(event) => events.push(event),
            None if event.get("muted").and_then(Value::as_bool) == Some(true)
                || event
                    .get("data")
                    .and_then(|data| data.get("muted"))
                    .and_then(Value::as_bool)
                    == Some(true) => {}
            None => logic_error(
                report,
                span,
                format!(
                    "unsupported or invalid stage event {:?}",
                    event.get("type").or_else(|| event.get("kind"))
                ),
            ),
        }
    }
    events.sort_by(|left, right| left.time.total_cmp(&right.time));

    let loop_value = prop_string_or(&block.props, "loop", "0");
    let infinite = matches!(
        loop_value.trim().to_ascii_lowercase().as_str(),
        "infinity" | "infinite" | "forever"
    );
    report.push(
        Action::StageAnimation {
            animation: StageAnimation {
                id: prop_string_or(
                    &block.props,
                    "name",
                    block.id.as_deref().unwrap_or("stage-animation"),
                ),
                duration: clip.duration.max(0.0) / 1000.0,
                tracks,
                events,
                repeat: if infinite {
                    0
                } else {
                    loop_value.parse::<f32>().unwrap_or_default().max(0.0) as u32
                },
                infinite,
                playback_rate: prop_f32(&block.props, "playbackRate", 1.0).max(f32::EPSILON),
                blocking: prop_bool(&block.props, "waitForComplete", true),
            },
        },
        span,
    );
}

fn compile_stage_track(
    track: StudioStageTrack,
    context: &CompileContext<'_>,
) -> Option<StageTrack> {
    let target = compile_stage_target(&track.target, context)?;
    let property = stage_property(&track.property)?;
    let invert = matches!(property, StageProperty::Y | StageProperty::Rotation)
        && !matches!(target, StageTarget::Camera);
    Some(StageTrack {
        target,
        property,
        keyframes: track
            .keyframes
            .into_iter()
            .map(|frame| StageKeyframe {
                time: frame.time.max(0.0) / 1000.0,
                value: if invert { -frame.value } else { frame.value },
                easing: easing(&frame.easing),
            })
            .collect(),
        muted: track.muted,
    })
}

fn compile_stage_target(
    target: &StudioStageTarget,
    context: &CompileContext<'_>,
) -> Option<StageTarget> {
    match target.kind.as_str() {
        "camera" => Some(StageTarget::Camera),
        "character" => {
            let id = first_non_empty([&target.character_id, &target.id]);
            if id.is_empty() {
                return None;
            }
            let image = if target.asset_path.is_empty() {
                context.characters.get(id).and_then(|character| {
                    character
                        .expressions
                        .iter()
                        .find(|expression| expression.name == target.expression_name)
                        .or_else(|| character.expressions.first())
                        .map(|expression| expression.asset_path.clone())
                })
            } else {
                Some(target.asset_path.clone())
            };
            Some(StageTarget::Character {
                id: id.to_owned(),
                image,
            })
        }
        "sceneLayer" | "scene-layer" => {
            let id = first_non_empty([&target.layer_id, &target.id]);
            (!id.is_empty()).then(|| StageTarget::SceneLayer {
                id: format!("scene-layer:{id}"),
            })
        }
        _ => None,
    }
}

fn first_non_empty<const N: usize>(values: [&String; N]) -> &str {
    values
        .into_iter()
        .find(|value| !value.is_empty())
        .map_or("", String::as_str)
}

fn stage_property(value: &str) -> Option<StageProperty> {
    use StageProperty as P;
    Some(match value {
        "x" | "offsetX" => P::X,
        "y" | "offsetY" => P::Y,
        "zoom" => P::Zoom,
        "scaleX" => P::ScaleX,
        "scaleY" => P::ScaleY,
        "alpha" => P::Alpha,
        "rotation" => P::Rotation,
        "width" => P::Width,
        "height" => P::Height,
        "focalDistance" => P::FocalDistance,
        "blurStrength" => P::BlurStrength,
        "distortionStrength" => P::DistortionStrength,
        "vignetteIntensity" => P::VignetteIntensity,
        "vignetteSize" => P::VignetteSize,
        "blurAmount" => P::BlurAmount,
        "colorToneIntensity" => P::ColorToneIntensity,
        "colorExposure" => P::ColorExposure,
        "colorBrightness" => P::ColorBrightness,
        "colorContrast" => P::ColorContrast,
        "colorSaturation" => P::ColorSaturation,
        "colorTemperature" => P::ColorTemperature,
        "oldFilmIntensity" => P::OldFilmIntensity,
        "shockIntensity" => P::ShockIntensity,
        "godrayIntensity" => P::GodrayIntensity,
        "godrayAngle" => P::GodrayAngle,
        "godrayGain" => P::GodrayGain,
        "godrayLacunarity" => P::GodrayLacunarity,
        "godraySpeed" => P::GodraySpeed,
        "godrayCenterX" => P::GodrayCenterX,
        "godrayCenterY" => P::GodrayCenterY,
        "lutIntensity" => P::LutIntensity,
        "bloomIntensity" => P::BloomIntensity,
        "chromaticAberration" => P::ChromaticAberration,
        "pixelateSize" => P::PixelateSize,
        "glitchIntensity" => P::GlitchIntensity,
        "crtIntensity" => P::CrtIntensity,
        "sharpenStrength" => P::SharpenStrength,
        "radialBlurStrength" => P::RadialBlurStrength,
        "radialBlurCenterX" => P::RadialBlurCenterX,
        "radialBlurCenterY" => P::RadialBlurCenterY,
        "motionBlurStrength" => P::MotionBlurStrength,
        "motionBlurAngle" => P::MotionBlurAngle,
        "zoomBlurStrength" => P::ZoomBlurStrength,
        "zoomBlurCenterX" => P::ZoomBlurCenterX,
        "zoomBlurCenterY" => P::ZoomBlurCenterY,
        "lightLeakIntensity" => P::LightLeakIntensity,
        "lightLeakAngle" => P::LightLeakAngle,
        "lensFlareIntensity" => P::LensFlareIntensity,
        "lensFlareCenterX" => P::LensFlareCenterX,
        "lensFlareCenterY" => P::LensFlareCenterY,
        "filmGrainIntensity" => P::FilmGrainIntensity,
        "filmGrainSize" => P::FilmGrainSize,
        "heatHazeIntensity" => P::HeatHazeIntensity,
        "heatHazeSpeed" => P::HeatHazeSpeed,
        "heatHazeScale" => P::HeatHazeScale,
        "waterRippleIntensity" => P::WaterRippleIntensity,
        "waterRippleFrequency" => P::WaterRippleFrequency,
        "waterRippleSpeed" => P::WaterRippleSpeed,
        "waterRippleCenterX" => P::WaterRippleCenterX,
        "waterRippleCenterY" => P::WaterRippleCenterY,
        "fogIntensity" => P::FogIntensity,
        "fogSpeed" => P::FogSpeed,
        "fogScale" => P::FogScale,
        "vhsIntensity" => P::VhsIntensity,
        "vhsJitter" => P::VhsJitter,
        "vhsNoise" => P::VhsNoise,
        "halftoneIntensity" => P::HalftoneIntensity,
        "halftoneScale" => P::HalftoneScale,
        "halftoneAngle" => P::HalftoneAngle,
        "ditherIntensity" => P::DitherIntensity,
        "ditherLevels" => P::DitherLevels,
        "outlineIntensity" => P::OutlineIntensity,
        "outlineThickness" => P::OutlineThickness,
        "eyelidOpenness" => P::EyelidOpenness,
        "eyelidWidth" => P::EyelidWidth,
        "eyelidCurvature" => P::EyelidCurvature,
        "eyelidSoftness" => P::EyelidSoftness,
        "eyelidCenterX" => P::EyelidCenterX,
        "eyelidCenterY" => P::EyelidCenterY,
        _ => return None,
    })
}

fn compile_stage_event(event: &Value, context: &CompileContext<'_>) -> Option<StageEvent> {
    let object = event.as_object()?;
    let payload = object
        .get("data")
        .and_then(Value::as_object)
        .or_else(|| object.get("payload").and_then(Value::as_object))
        .unwrap_or(object);
    if object
        .get("muted")
        .or_else(|| payload.get("muted"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let event_type = object
        .get("type")
        .or_else(|| object.get("kind"))
        .and_then(Value::as_str)?;
    let time = value_f32(object.get("time").or_else(|| payload.get("time")), 0.0).max(0.0) / 1000.0;
    let kind = match event_type {
        "cameraShake" => {
            let shake = CameraShakeSpec {
                amplitude: value_f32(payload.get("amplitude"), 8.0),
                frequency: value_f32(payload.get("frequency"), 18.0),
                duration: value_f32(payload.get("duration"), 300.0).max(0.0) / 1000.0,
                axis: match value_str(payload.get("axis")) {
                    "x" => CameraShakeAxis::X,
                    "y" => CameraShakeAxis::Y,
                    _ => CameraShakeAxis::Both,
                },
                falloff: if value_str(payload.get("falloff")) == "expo" {
                    CameraShakeFalloff::Exponential
                } else {
                    CameraShakeFalloff::Linear
                },
            };
            let randomness = CameraShakeRandomness {
                amplitude: value_f32(payload.get("amplitudeRandomness"), 0.0).clamp(0.0, 1.0),
                frequency: value_f32(payload.get("frequencyRandomness"), 0.0).clamp(0.0, 1.0),
            };
            if randomness.is_zero() {
                StageEventKind::CameraShake(shake)
            } else {
                StageEventKind::CameraShakeRandomized { shake, randomness }
            }
        }
        "cameraPatch" => {
            let patch = payload
                .get("patch")
                .and_then(Value::as_object)
                .unwrap_or(payload);
            let mut effect = post_process_patch_from_props(patch);
            if patch.contains_key("lutPreset") && prop_string(patch, "lutPreset").is_empty() {
                effect.lut_preset = Some(None);
            }
            StageEventKind::CameraPatch {
                targets: payload.get("targets").and_then(stage_camera_targets),
                effect: Box::new(effect),
            }
        }
        "particleCue" => {
            let options: StudioParticleOverrides = if let Some(options) = payload.get("options") {
                serde_json::from_value(options.clone()).ok()?
            } else if let Some(source) = payload.get("optionsJson") {
                let source = source.as_str()?;
                if source.trim().is_empty() {
                    StudioParticleOverrides::default()
                } else {
                    serde_json::from_str(source).ok()?
                }
            } else {
                StudioParticleOverrides::default()
            };
            if !options.unsupported.is_empty() {
                return None;
            }
            StageEventKind::Particle {
                id: non_empty_value(payload, &["id", "effectId"])
                    .unwrap_or("particle")
                    .into(),
                effect: keine_core::ParticleEffect {
                    texture: non_empty_value(payload, &["texture", "textureUri"])
                        .map(str::to_owned),
                    preset: non_empty_value(payload, &["preset"])
                        .unwrap_or("LIGHT_SNOW")
                        .into(),
                    count: options.count.unwrap_or(0).min(u16::MAX as u32) as u16,
                    wind: options.wind,
                    gravity: options.gravity,
                    fade_in: value_f32(payload.get("fadeInDuration"), 0.0).max(0.0) / 1000.0,
                },
                duration: value_f32(payload.get("duration"), 0.0).max(0.0) / 1000.0,
                fade_out: value_f32(payload.get("fadeOutDuration"), 0.0).max(0.0) / 1000.0,
            }
        }
        "sceneCue" => StageEventKind::Scene(compile_stage_scene_cue(payload, context)?),
        "audioCue" => StageEventKind::Audio(StageAudioCue {
            id: non_empty_value(payload, &["id", "soundId"])
                .unwrap_or("audio")
                .to_owned(),
            kind: match value_str(payload.get("soundType")) {
                "BGM" => StageAudioKind::Bgm,
                "VOCAL" => StageAudioKind::Vocal,
                _ => StageAudioKind::Effect,
            },
            file: non_empty_value(payload, &["uri", "file"])
                .unwrap_or_default()
                .to_owned(),
            volume: value_f32(payload.get("volume"), 1.0).clamp(0.0, 1.0),
            looped: payload
                .get("loop")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            duration: value_f32(payload.get("duration"), 0.0).max(0.0) / 1000.0,
            fade_in: value_f32(payload.get("fadeInDuration"), 0.0).max(0.0) / 1000.0,
            fade_out: value_f32(payload.get("fadeOutDuration"), 0.0).max(0.0) / 1000.0,
        }),
        _ => return None,
    };
    Some(StageEvent { time, kind })
}

fn compile_stage_scene_cue(
    payload: &Map<String, Value>,
    context: &CompileContext<'_>,
) -> Option<StageSceneCue> {
    let scene_id = non_empty_value(payload, &["sceneId", "id"])
        .unwrap_or_default()
        .to_owned();
    let layers: Vec<StageSceneLayer> =
        if let Some(layers) = payload.get("layers").and_then(Value::as_array) {
            layers.iter().filter_map(stage_scene_layer).collect()
        } else {
            context
                .scenes
                .get(scene_id.as_str())
                .map(|scene| {
                    scene
                        .layers
                        .iter()
                        .filter(|layer| !layer.asset_path.is_empty())
                        .map(|layer| StageSceneLayer {
                            id: layer.id.clone(),
                            image: layer.asset_path.clone(),
                            distance: layer.distance,
                            offset: parse_position(&layer.offset),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
    if scene_id.is_empty() && layers.is_empty() {
        return None;
    }
    Some(StageSceneCue {
        scene_id,
        transition: scene_transition_from_props(payload),
        reset_camera: value_bool(payload.get("resetCamera"), false),
        layout: scene_layer_layout_from_props(payload),
        layers,
    })
}

fn stage_scene_layer(value: &Value) -> Option<StageSceneLayer> {
    let layer = value.as_object()?;
    let id = non_empty_value(layer, &["id", "layerId"])?;
    let image = non_empty_value(layer, &["assetPath", "uri", "image"])?;
    Some(StageSceneLayer {
        id: id.to_owned(),
        image: image.to_owned(),
        distance: value_f32(layer.get("distance"), 1.0),
        offset: layer
            .get("offset")
            .and_then(Value::as_str)
            .map(parse_position)
            .unwrap_or([0.0, 0.0]),
    })
}

fn stage_camera_targets(value: &Value) -> Option<CameraTargets> {
    let mut scene = false;
    let mut characters = false;
    let mut visit = |target: &str| match target.trim() {
        "scene" => scene = true,
        "characters" | "character" => characters = true,
        _ => {}
    };
    match value {
        Value::String(value) => value.split(',').for_each(&mut visit),
        Value::Array(values) => values.iter().filter_map(Value::as_str).for_each(&mut visit),
        _ => return None,
    }
    Some(CameraTargets::new(scene, characters))
}

fn non_empty_value<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| object.get(*key).and_then(Value::as_str))
        .find(|value| !value.is_empty())
}

fn value_str(value: Option<&Value>) -> &str {
    value.and_then(Value::as_str).unwrap_or_default()
}

fn value_bool(value: Option<&Value>, fallback: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(fallback)
}

fn value_f32(value: Option<&Value>, fallback: f32) -> f32 {
    value
        .and_then(|value| match value {
            Value::Number(value) => value.as_f64(),
            Value::String(value) => value.parse().ok(),
            _ => None,
        })
        .map_or(fallback, |value| value as f32)
}

fn sprite_transform_patch(props: &Map<String, Value>) -> TransformPatch {
    let mut patch = TransformPatch::default();
    if let Some(value) = studio_coordinate(props, "x", keine_core::DESIGN_WIDTH) {
        patch.set_offset_x(value);
    }
    if let Some(value) = studio_coordinate(props, "y", keine_core::DESIGN_HEIGHT) {
        // LetsGal/Pixi uses a downward-positive canvas; Bevy's stage transform
        // uses upward-positive world offsets.
        patch.set_offset_y(-value);
    }
    if let Some(value) = optional_f32(props, "alpha") {
        patch.set_alpha(value.clamp(0.0, 1.0));
    }
    if let Some(value) = optional_f32(props, "scaleX") {
        patch.set_scale_x(value);
    }
    if let Some(value) = optional_f32(props, "scaleY") {
        patch.set_scale_y(value);
    }
    if let Some(value) = optional_f32(props, "rotation") {
        // LetsGal stores radians and Pixi rotates clockwise on its y-down
        // canvas. Preserve the visual direction in Bevy's y-up space.
        patch.set_rotation(-value);
    }
    patch
}

fn compile_known_extension(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) -> bool {
    let target = prop_string(&block.props, "target");
    let params = json_value(&block.props, "paramsJson").unwrap_or(Value::Null);
    match target.as_str() {
        "avg.internal.default-shell/add-to-gallery" => {
            let title = literal_extension_string(&params, "title").unwrap_or_else(|| "CG".into());
            let file = literal_extension_string(&params, "_sceneLayers")
                .and_then(|layers| serde_json::from_str::<Vec<Value>>(&layers).ok())
                .and_then(|layers| {
                    layers
                        .first()
                        .and_then(|layer| layer.get("assetPath"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            if let Some(file) = file {
                report.push(
                    Action::Unlock {
                        kind: keine_core::UnlockKind::Cg,
                        file,
                        name: title,
                    },
                    span,
                );
            }
            true
        }
        "shiftz.backspace/backspace-to" | "maincore.backspace-to/backspace-to" => {
            let source = literal_extension_string(&params, "source").unwrap_or_default();
            // Backspace 1.4.5 omits `keep` when the whole source line is
            // removed. Treating that as malformed rejects valid 1.9.1 Studio
            // projects even though an explicit source snapshot is present.
            let keep = literal_extension_string(&params, "keep").unwrap_or_default();
            if (!source.is_empty() && source.starts_with(&keep))
                || (source.is_empty() && !keep.is_empty())
            {
                report.push(Action::RetractDialogue { source, keep }, span);
            } else {
                report.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    span,
                    message: "invalid sentence-tail deletion: `keep` must be a source prefix; an \
                              empty prefix removes the whole line when a source snapshot exists"
                        .into(),
                });
            }
            true
        }
        _ => false,
    }
}

fn literal_extension_string(params: &Value, key: &str) -> Option<String> {
    let value = params.get(key)?;
    value
        .get("value")
        .and_then(Value::as_str)
        .or_else(|| value.as_str())
        .map(str::to_owned)
}

fn compile_system_ui(
    block: &StoryBlock,
    visible: bool,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    let target = prop_string(&block.props, "target");
    let slot = target.strip_prefix("slot:").unwrap_or(&target);
    let slot = match slot {
        "internal.system.title" => Some(SystemUiSlot::Title),
        "internal.system.save" => Some(SystemUiSlot::Save),
        "internal.system.load" => Some(SystemUiSlot::Load),
        "internal.system.settings" => Some(SystemUiSlot::Settings),
        "internal.system.history" => Some(SystemUiSlot::History),
        "internal.system.gallery" => Some(SystemUiSlot::Gallery),
        "internal.system.input" => Some(SystemUiSlot::Input),
        _ => None,
    };
    if let Some(slot) = slot {
        report.push(Action::SetSystemUi { slot, visible }, span);
    } else {
        push_host(
            block,
            "extension",
            if visible { "ui.show" } else { "ui.hide" },
            span,
            report,
        );
    }
}

fn push_host(
    block: &StoryBlock,
    namespace: &str,
    command: &str,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    let payload = json!({
        "id": block.id,
        "type": block.kind,
        "content": block.content,
        "props": block.props,
        "children": block.children,
        "extras": block.extras,
    });
    report.push(
        Action::HostCommand {
            namespace: namespace.into(),
            command: command.into(),
            payload: payload.to_string(),
        },
        span,
    );
}

fn character<'a>(
    block: &StoryBlock,
    context: &'a CompileContext<'a>,
) -> Option<&'a CharacterDefinition> {
    let id = prop_string(&block.props, "characterId");
    if let Some(character) = context.characters.get(id.as_str()) {
        return Some(*character);
    }
    let name = prop_string(&block.props, "characterName");
    context
        .characters
        .values()
        .copied()
        .find(|character| character.name == name)
}

fn character_id(block: &StoryBlock) -> String {
    prop_string_or(
        &block.props,
        "characterId",
        &prop_string(&block.props, "characterName"),
    )
}

fn scene_transition(block: &StoryBlock) -> Transition {
    scene_transition_from_props(&block.props)
}

fn scene_transition_from_props(props: &Map<String, Value>) -> Transition {
    let seconds = prop_f32(props, "transitionDuration", 0.0).max(0.0) / 1000.0;
    if seconds <= f32::EPSILON {
        return Transition::Instant;
    }
    match prop_string(props, "transitionMode").as_str() {
        "cut" => Transition::Instant,
        "wipe" | "blinds" | "checkerboard" | "radial-wipe" | "barn-door" | "diagonal-wipe"
        | "iris" => Transition::Wipe(seconds),
        "slide" => match prop_string(props, "transitionDirection").as_str() {
            "right" | "right-to-left" => Transition::SlideFromRight(seconds),
            _ => Transition::SlideFromLeft(seconds),
        },
        "pixel-dissolve" | "random-dissolve" | "rule" | "mosaic" | "glitch" => {
            Transition::Dissolve(seconds)
        }
        _ => Transition::Crossfade(seconds),
    }
}

fn fade(block: &StoryBlock, enabled_key: &str, fallback: f32) -> Transition {
    if prop_bool(&block.props, enabled_key, true) {
        Transition::Fade(fallback)
    } else {
        Transition::Instant
    }
}

fn easing(value: &str) -> Easing {
    match value.to_ascii_lowercase().as_str() {
        "in" | "easein" | "ease-in" | "inquad" | "incubic" | "inquart" | "inquint" | "insine"
        | "incirc" | "inexpo" => Easing::EaseIn,
        "outcubic" => Easing::OutCubic,
        "outback" => Easing::OutBack,
        "outbounce" => Easing::OutBounce,
        "out" | "easeout" | "ease-out" | "outquad" | "outquart" | "outquint" | "outsine"
        | "outcirc" | "outexpo" => Easing::EaseOut,
        "inoutquad" => Easing::InOutQuad,
        "inoutcubic" => Easing::InOutCubic,
        "inout" | "easeinout" | "ease-in-out" | "inoutquart" | "inoutquint" | "inoutsine"
        | "inoutcirc" | "inoutexpo" => Easing::EaseInOut,
        _ => Easing::Linear,
    }
}

fn plain_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(values) => values.iter().map(plain_text).collect(),
        Value::Object(value) => value
            .get("text")
            .map(plain_text)
            .or_else(|| value.get("content").map(plain_text))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

#[derive(Clone, Default)]
struct StudioInlineStyle {
    color: Option<String>,
    background: Option<String>,
    size: Option<String>,
    ruby: Option<String>,
    bold: bool,
    italic: bool,
    strike: bool,
}

fn studio_dialogue_markup(value: &Value) -> String {
    let source = if let Some(runs) = value.as_array() {
        let mut source = String::new();
        for run in runs {
            let mut style = StudioInlineStyle::default();
            if let Some(styles) = run.get("styles").and_then(Value::as_object) {
                for (field, value) in styles {
                    let name = match field.as_str() {
                        "textColor" => "color",
                        "backgroundColor" => "bg",
                        "fontSize" => "size",
                        "strikethrough" => "del",
                        _ => normalize_studio_tag(field),
                    };
                    if matches!(value, Value::Bool(false)) {
                        continue;
                    }
                    let raw = value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string());
                    apply_studio_style(&mut style, name, &raw);
                }
            }
            let mut text = plain_text(run);
            flush_studio_run(&mut source, &mut text, &style);
        }
        source
    } else {
        plain_text(value)
    };
    let chars = source.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut plain = String::new();
    let mut style = StudioInlineStyle::default();
    let mut stack = Vec::<(String, StudioInlineStyle)>::new();
    let mut cursor = 0;

    while cursor < chars.len() {
        if chars[cursor] != '[' {
            plain.push(chars[cursor]);
            cursor += 1;
            continue;
        }
        let Some(end_offset) = chars[cursor + 1..].iter().position(|value| *value == ']') else {
            plain.push(chars[cursor]);
            cursor += 1;
            continue;
        };
        let end = cursor + 1 + end_offset;
        let label = chars[cursor + 1..end].iter().collect::<String>();
        let (closing, body) = label
            .strip_prefix('/')
            .map_or((false, label.as_str()), |body| (true, body));
        let (tag, value) = body.split_once('=').unwrap_or((body, ""));
        let tag = normalize_studio_tag(tag);

        if !closing && tag == "br" {
            flush_studio_run(&mut output, &mut plain, &style);
            output.push('\n');
        } else if !closing && tag == "wait" {
            flush_studio_run(&mut output, &mut plain, &style);
            if value.is_empty() {
                output.push_str("[wait]");
            } else if value.bytes().all(|value| value.is_ascii_digit()) {
                output.push_str("[wait=");
                output.push_str(value);
                output.push(']');
            }
        } else if !closing && tag == "voice" {
            flush_studio_run(&mut output, &mut plain, &style);
        } else if is_studio_style_tag(tag) {
            flush_studio_run(&mut output, &mut plain, &style);
            if closing {
                if let Some(index) = stack.iter().rposition(|(open, _)| open == tag) {
                    style = stack[index].1.clone();
                    stack.truncate(index);
                }
            } else {
                stack.push((tag.to_owned(), style.clone()));
                apply_studio_style(&mut style, tag, value);
            }
        } else {
            plain.extend(chars[cursor..=end].iter());
        }
        cursor = end + 1;
    }
    flush_studio_run(&mut output, &mut plain, &style);
    output
}

fn normalize_studio_tag(tag: &str) -> &str {
    match tag.to_ascii_lowercase().as_str() {
        "c" | "color" => "color",
        "bg" | "bgcolor" => "bg",
        "b" | "bold" => "bold",
        "i" | "italic" => "italic",
        "s" | "size" => "size",
        "del" => "del",
        "rt" => "rt",
        "br" => "br",
        "wait" => "wait",
        "voice" => "voice",
        _ => "",
    }
}

fn is_studio_style_tag(tag: &str) -> bool {
    matches!(
        tag,
        "color" | "bg" | "bold" | "italic" | "size" | "del" | "rt"
    )
}

fn apply_studio_style(style: &mut StudioInlineStyle, tag: &str, value: &str) {
    match tag {
        "color" => style.color = Some(value.to_owned()),
        "bg" => style.background = Some(value.to_owned()),
        "size" => style.size = Some(value.trim_end_matches("px").trim().to_owned()),
        "rt" => style.ruby = Some(value.to_owned()),
        "bold" => style.bold = true,
        "italic" => style.italic = true,
        "del" => style.strike = true,
        _ => {}
    }
}

fn flush_studio_run(output: &mut String, plain: &mut String, style: &StudioInlineStyle) {
    if plain.is_empty() {
        return;
    }
    let value = std::mem::take(plain);
    for (index, line) in value.split('\n').enumerate() {
        if index > 0 {
            output.push('\n');
        }
        if line.is_empty() {
            continue;
        }
        let mut attributes = Vec::new();
        if let Some(ruby) = &style.ruby {
            attributes.push(format!("ruby={ruby}"));
        }
        if let Some(color) = &style.color {
            attributes.push(format!("color={color}"));
        }
        if let Some(background) = &style.background {
            attributes.push(format!("background={background}"));
        }
        if let Some(size) = &style.size {
            attributes.push(format!("size={size}px"));
        }
        if style.bold {
            attributes.push("bold".into());
        }
        if style.italic {
            attributes.push("italic".into());
        }
        if style.strike {
            attributes.push("strike".into());
        }
        if attributes.is_empty() {
            output.push_str(line);
        } else {
            output.push('[');
            output.push_str(line);
            output.push_str("](");
            output.push_str(&attributes.join(";"));
            output.push(')');
        }
    }
}

fn prop_string(props: &Map<String, Value>, key: &str) -> String {
    props
        .get(key)
        .map_or_else(String::new, |value| match value {
            Value::String(value) => value.clone(),
            Value::Number(value) => value.to_string(),
            Value::Bool(value) => value.to_string(),
            _ => String::new(),
        })
}

fn prop_string_or(props: &Map<String, Value>, key: &str, fallback: &str) -> String {
    let value = prop_string(props, key);
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value
    }
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn prop_bool(props: &Map<String, Value>, key: &str, fallback: bool) -> bool {
    match props.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) if value.eq_ignore_ascii_case("true") => true,
        Some(Value::String(value)) if value.eq_ignore_ascii_case("false") => false,
        _ => fallback,
    }
}

fn optional_bool(props: &Map<String, Value>, key: &str) -> Option<bool> {
    match props.get(key) {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("true") => Some(true),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

fn prop_f32(props: &Map<String, Value>, key: &str, fallback: f32) -> f32 {
    optional_f32(props, key).unwrap_or(fallback)
}

fn optional_f32(props: &Map<String, Value>, key: &str) -> Option<f32> {
    match props.get(key) {
        Some(Value::Number(value)) => value.as_f64().map(|value| value as f32),
        Some(Value::String(value)) if !value.trim().is_empty() => value.parse().ok(),
        _ => None,
    }
}

fn studio_coordinate(props: &Map<String, Value>, key: &str, extent: f32) -> Option<f32> {
    let raw = prop_string(props, key);
    parse_studio_coordinate(&raw, extent)
}

fn parse_studio_coordinate(raw: &str, extent: f32) -> Option<f32> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(value) = parse_studio_unit(raw, extent) {
        return Some(value);
    }

    // LetsGal also accepts `center+20`, `50%-10px` and the same suffixes
    // after numeric coordinates. The leading sign belongs to the base value.
    let split = raw
        .char_indices()
        .skip(1)
        .find_map(|(index, character)| matches!(character, '+' | '-').then_some(index))?;
    let base = parse_studio_unit(&raw[..split], extent)?;
    let delta = parse_studio_unit(&raw[split + 1..], extent)?;
    Some(if raw.as_bytes()[split] == b'+' {
        base + delta
    } else {
        base - delta
    })
}

fn parse_studio_unit(raw: &str, extent: f32) -> Option<f32> {
    let raw = raw.trim();
    match raw.to_ascii_lowercase().as_str() {
        "left" | "top" => return Some(0.0),
        "center" => return Some(extent * 0.5),
        "right" | "bottom" => return Some(extent),
        _ => {}
    }
    if let Some(percent) = raw.strip_suffix('%') {
        return percent
            .parse::<f32>()
            .ok()
            .map(|percent| extent * percent / 100.0);
    }
    raw.strip_suffix("px").unwrap_or(raw).parse::<f32>().ok()
}

fn json_string<T: for<'de> Deserialize<'de>>(props: &Map<String, Value>, key: &str) -> Option<T> {
    let value = props.get(key)?;
    if let Value::String(value) = value {
        serde_json::from_str(value).ok()
    } else {
        serde_json::from_value(value.clone()).ok()
    }
}

fn json_value(props: &Map<String, Value>, key: &str) -> Option<Value> {
    match props.get(key)? {
        Value::String(value) => serde_json::from_str(value).ok(),
        value => Some(value.clone()),
    }
}

fn expression_literal(value: &str) -> String {
    if value.parse::<f64>().is_ok()
        || matches!(value, "true" | "false")
        || matches!(serde_json::from_str::<Value>(value), Ok(Value::String(_)))
    {
        value.to_owned()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn conditions_and_variable_choices_keep_native_logic_and_continue_options() {
        let props = json!({"conditions": "[{\"left\":\"score\",\"op\":\">=\",\"rightLiteral\":\"5\"}]", "thenFragmentId":"yes", "elseFragmentId":"no"});
        let block = StoryBlock {
            id: None,
            kind: "if".into(),
            content: Value::Null,
            props: props.as_object().unwrap().clone(),
            children: Vec::new(),
            extras: Map::new(),
        };
        let span = SourceSpan { line: 1, column: 1 };
        let mut report = ParseReport::default();
        compile_if(&block, span, &mut report);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(
            matches!(&report.actions[0], Action::ConditionalCall { condition, then_scene, .. } if condition == "(score >= 5)" && then_scene == "yes")
        );
        let choices = json!([
            {"mode":"jump","text":"Continue","fragmentId":""},
            {"mode":"vars","text":"Add","varOps":[{"key":"score","op":"+=","aKind":"lit","aLit":"5","bKind":"none"}],"visibleIf":"[{\"left\":\"score\",\"op\":\">=\",\"rightLiteral\":\"0\"}]"},
            {"mode":"jump","text":"Substory","fragmentId":"sub"}]);
        let mut block = block;
        block.props = Map::from_iter([("choices".into(), Value::String(choices.to_string()))]);
        let mut report = ParseReport::default();
        compile_branch(&block, span, &mut report);
        assert!(report.diagnostics.is_empty());
        let Action::Menu { choices, .. } = &report.actions[0] else {
            panic!("menu");
        };
        assert_eq!(choices[0].target, ChoiceTarget::Continue);
        assert_eq!(
            choices[1].target,
            ChoiceTarget::Assign(vec![("score".into(), "score + (5)".into())])
        );
        assert_eq!(choices[1].show_when.as_deref(), Some("(score >= 0)"));
        assert_eq!(choices[2].target, ChoiceTarget::CallScene("sub".into()));
        assert!(
            studio_conditions(
                &json!([{"left":"x","op":"contains","rightLiteral":"y"}]),
                "and"
            )
            .is_err()
        );
    }

    #[test]
    fn avatar_only_dialogue_uses_the_avatar_and_clears_it_afterwards() {
        let character: CharacterDefinition = serde_json::from_value(json!({
            "id":"alice", "name":"Alice",
            "expressions":[{"name":"smile","assetPath":"alice.webp"}]
        }))
        .unwrap();
        let characters = HashMap::from([("alice", &character)]);
        let context = CompileContext {
            entry: "entry",
            chapter_next: &HashMap::new(),
            characters: &characters,
            scenes: &HashMap::new(),
            voices: &HashMap::new(),
            positions: &HashMap::new(),
            portrait_height_ratio: None,
        };
        let block: StoryBlock = serde_json::from_value(json!({
            "type":"dialogue", "content":"Hello",
            "props":{"characterId":"alice","expression":"smile","dialoguePortraitOnly":true}
        }))
        .unwrap();
        let mut report = ParseReport::default();
        compile_dialogue(
            &block,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );
        assert!(report.diagnostics.is_empty());
        assert!(
            matches!(&report.actions[0], Action::MiniAvatar { image } if image == "alice.webp")
        );
        assert!(
            !report
                .actions
                .iter()
                .any(|action| matches!(action, Action::ShowSprite { .. }))
        );
        assert!(
            report
                .actions
                .iter()
                .any(|action| matches!(action, Action::HideMiniAvatar))
        );
    }

    #[test]
    fn structured_dialogue_styles_and_nonlooping_music_are_not_lost() {
        let text = studio_dialogue_markup(
            &json!([{ "type":"text", "text":"Hello", "styles":{"bold":true,"textColor":"#ff8800"} }]),
        );
        assert_eq!(text, "[Hello](color=#ff8800;bold)");
        assert_eq!(
            studio_dialogue_markup(&json!([{"text":"Hi","styles":{"fontSize":"20px"}}])),
            "[Hi](size=20px)"
        );
        let block = StoryBlock {
            id: None,
            kind: "sound".into(),
            content: Value::Null,
            props: json!({"soundType":"BGM","uri":"theme.opus","loop":false})
                .as_object()
                .unwrap()
                .clone(),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();
        compile_sound(&block, SourceSpan { line: 1, column: 1 }, &mut report);
        assert!(matches!(
            &report.actions[0],
            Action::EiyashouBgm { looped: false, .. }
        ));
    }

    use super::*;

    #[test]
    fn studio_200_registry_is_exhaustively_matched() {
        assert_eq!(BUILTIN_BLOCK_TYPES.len(), 43);
        for required in [
            "playerInput",
            "enterAutoPlay",
            "callExtensionFunction",
            "stageAnimation",
            "video",
            "hideFloatingText",
            "switchParagraphStyle",
            "systemMessage",
            "updateCharacter",
            "stageMask",
            "loadingStrategy",
            "openExternalUrl",
            "steamAction",
            "unlockSteamAchievement",
        ] {
            assert!(BUILTIN_BLOCK_TYPES.contains(&required));
        }
    }

    #[test]
    fn studio_1200_remove_character_compiles_every_selected_target() {
        let block = StoryBlock {
            id: None,
            kind: "removeCharacter".into(),
            content: Value::Null,
            props: Map::from_iter([
                (
                    "characterTargetsJson".into(),
                    json!(r#"[{"id":"alice","name":"Alice"},{"id":"bob","name":"Bob"}]"#),
                ),
                ("characterId".into(), json!("alice")),
                ("characterName".into(), json!("Alice")),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        compile_remove_characters(&block, SourceSpan { line: 1, column: 1 }, &mut report);

        assert!(matches!(
            &report.actions[..],
            [
                Action::HideSprites { prefix: alice_layers, .. },
                Action::HideSprite { id: alice, .. },
                Action::HideSprites { prefix: bob_layers, .. },
                Action::HideSprite { id: bob, .. }
            ] if alice_layers == "character-layer:alice:"
                && alice == "alice"
                && bob_layers == "character-layer:bob:"
                && bob == "bob"
        ));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn studio_1200_stage_mask_compiles_typed_geometry_and_fill() {
        let chapter = ChapterDocument {
            id: "chapter".into(),
            name: "chapter".into(),
            kind: String::new(),
            disabled: false,
            fragments: Vec::new(),
        };
        let chapter_next = HashMap::new();
        let characters = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let block = StoryBlock {
            id: None,
            kind: "stageMask".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("action".into(), json!("show")),
                ("mode".into(), json!("overlay")),
                ("maskId".into(), json!("focus")),
                ("shape".into(), json!("ellipse")),
                ("visibility".into(), json!("outside")),
                ("centerX".into(), json!(25)),
                ("width".into(), json!(40)),
                ("fillMode".into(), json!("gradient")),
                ("gradientStart".into(), json!("#112233")),
                ("enterDuration".into(), json!(500)),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        compile_block(
            &block,
            &chapter,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        assert!(matches!(
            &report.actions[..],
            [Action::StageMask { id, mask: Some(mask), duration, .. }]
                if id == "focus"
                    && mask.shape == StageMaskShape::Ellipse
                    && mask.visibility == StageMaskVisibility::Outside
                    && mask.fill_mode == StageMaskFillMode::Gradient
                    && (mask.center[0] - 25.0).abs() <= f32::EPSILON
                    && (mask.size[0] - 40.0).abs() <= f32::EPSILON
                    && (*duration - 0.5).abs() <= f32::EPSILON
        ));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn studio_198_blocks_lower_to_native_ir() {
        let chapter = ChapterDocument {
            id: "chapter".into(),
            name: "chapter".into(),
            kind: String::new(),
            disabled: false,
            fragments: Vec::new(),
        };
        let chapter_next = HashMap::new();
        let characters = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let blocks = [
            StoryBlock {
                id: None,
                kind: "floatingText".into(),
                content: json!("persistent"),
                props: Map::from_iter([
                    ("floatingTextId".into(), json!("notice")),
                    ("infinite".into(), json!(true)),
                ]),
                children: Vec::new(),
                extras: Map::new(),
            },
            StoryBlock {
                id: None,
                kind: "hideFloatingText".into(),
                content: Value::Null,
                props: Map::from_iter([("floatingTextId".into(), json!("notice"))]),
                children: Vec::new(),
                extras: Map::new(),
            },
            StoryBlock {
                id: None,
                kind: "switchParagraphStyle".into(),
                content: Value::Null,
                props: Map::from_iter([("targetId".into(), json!("literary"))]),
                children: Vec::new(),
                extras: Map::new(),
            },
            StoryBlock {
                id: None,
                kind: "systemMessage".into(),
                content: Value::Null,
                props: Map::from_iter([
                    ("mode".into(), json!("confirm")),
                    ("title".into(), json!("Continue?")),
                    ("message".into(), json!("Choose")),
                    ("resultVariable".into(), json!("accepted")),
                ]),
                children: Vec::new(),
                extras: Map::new(),
            },
        ];
        let mut report = ParseReport::default();
        for (index, block) in blocks.iter().enumerate() {
            compile_block(
                block,
                &chapter,
                &context,
                SourceSpan {
                    line: index + 1,
                    column: 1,
                },
                &mut report,
            );
        }

        assert!(matches!(&report.actions[0], Action::FloatingText { .. }));
        assert!(matches!(
            &report.actions[1],
            Action::ConfigureFloatingText {
                id: Some(id),
                infinite: true
            } if id == "notice"
        ));
        assert!(matches!(
            &report.actions[2],
            Action::HideFloatingText { id: Some(id) } if id == "notice"
        ));
        assert!(matches!(
            &report.actions[3],
            Action::SetParagraphStyle {
                style: keine_core::DialogueStyle::Literary,
                ..
            }
        ));
        assert!(matches!(
            &report.actions[4],
            Action::SystemMessage { spec }
                if spec.mode == SystemMessageMode::Confirm
                    && spec.result_variable.as_deref() == Some("accepted")
        ));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn studio_v2_portrait_show_and_update_keep_authored_canvas_center() {
        let document: CharactersDocument = serde_json::from_value(json!({
            "version": 2,
            "globalSettings": { "graphics": { "heightRatio": 0.9 },
                "distancePresets": [{ "id": "middle", "scale": 1.32,
                    "positions": [{ "id": "right", "left": 84.8, "top": 90 }] }] },
            "characters": [{ "id": "hero", "name": "Hero",
                "expressions": [{ "name": "neutral", "assetPath": "hero.webp" },
                    { "name": "short", "assetPath": "short.webp", "graphicsOverride": { "heightRatio": 0.7 } }],
                "portraitLayout": { "defaultDistanceId": "middle", "defaultAnchor": "center",
                    "graphics": { "heightRatio": 0.8 },
                    "distancePresets": [{ "id": "middle", "scale": 1.32,
                        "positions": [{ "id": "left", "left": 20.1, "top": 90.4 }] }] }
            }]
        })).unwrap();
        let positions = portrait_positions(&document);
        let characters = HashMap::from([("hero", &document.characters[0])]);
        let empty = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &empty,
            characters: &characters,
            scenes: &HashMap::new(),
            voices: &HashMap::new(),
            positions: &positions,
            portrait_height_ratio: document.global_settings.graphics.height_ratio,
        };
        for (expression, position_id, ratio, x, top) in [
            ("neutral", "left", 0.8, 385.92, 976.32),
            ("short", "left", 0.7, 385.92, 976.32),
            ("neutral", "right", 0.8, 1628.16, 972.0),
        ] {
            for update in [false, true] {
                let block: StoryBlock = serde_json::from_value(json!({ "type": "showCharacter",
                    "props": { "characterId": "hero", "expression": expression,
                        "position": position_id, "distance": "middle" } }))
                .unwrap();
                let mut report = ParseReport::default();
                compile_character(
                    &block,
                    &context,
                    SourceSpan { line: 1, column: 1 },
                    &mut report,
                    update,
                );
                assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
                let (position, layout, scale) = match &report.actions[0] {
                    Action::ShowSprite {
                        position,
                        layout,
                        transform,
                        ..
                    } => (*position, *layout, transform.scale_x),
                    Action::UpdateSprite {
                        position,
                        layout,
                        scale,
                        ..
                    } => (*position, *layout, *scale),
                    action => panic!("{action:?}"),
                };
                let Anchor::Center(offset) = position.x else {
                    panic!("{position:?}");
                };
                assert!((960.0 + offset - x).abs() < 0.001);
                assert!((position.y + 1080.0 * ratio * 0.5 - (1080.0 - top)).abs() < 0.001);
                assert_eq!(layout, SpriteLayout::ViewportHeight(ratio));
                assert_eq!(scale, 1.32);
            }
        }
    }

    #[test]
    fn studio_198_dynamic_portraits_are_rejected_explicitly() {
        for kind in ["spine", "live2d"] {
            let character: CharacterDefinition = serde_json::from_value(json!({
                "id": "hero",
                "name": "Hero",
                "expressions": [{
                    "name": "dynamic",
                    "assetPath": "characters/fallback.png",
                    "presentation": {"type": kind}
                }]
            }))
            .unwrap();
            let characters = HashMap::from([("hero", &character)]);
            let chapter_next = HashMap::new();
            let scenes = HashMap::new();
            let voices = HashMap::new();
            let positions = HashMap::new();
            let context = CompileContext {
                entry: "entry",
                chapter_next: &chapter_next,
                characters: &characters,
                scenes: &scenes,
                voices: &voices,
                positions: &positions,
                portrait_height_ratio: None,
            };
            let block = StoryBlock {
                id: None,
                kind: "showCharacter".into(),
                content: Value::Null,
                props: Map::from_iter([
                    ("characterId".into(), json!("hero")),
                    ("expression".into(), json!("dynamic")),
                ]),
                children: Vec::new(),
                extras: Map::new(),
            };
            let mut report = ParseReport::default();

            show_character(
                &block,
                &context,
                SourceSpan { line: 1, column: 1 },
                &mut report,
            );

            assert!(report.actions.is_empty());
            assert!(report.diagnostics.iter().any(|diagnostic| {
                diagnostic.level == DiagnosticLevel::Error && diagnostic.message.contains(kind)
            }));
        }
    }

    #[test]
    fn studio_199_portrait_update_compiles_image_layout_and_motion() {
        let character: CharacterDefinition = serde_json::from_value(json!({
            "id": "hero",
            "name": "Hero",
            "expressions": [
                {"name": "calm", "assetPath": "characters/calm.webp"},
                {"name": "smile", "assetPath": "characters/smile.webp"}
            ],
            "portraitLayout": {
                "defaultDistanceId": "middle",
                "defaultPositionId": "center",
                "distancePresets": [{
                    "id": "near",
                    "scale": 1.25,
                    "positions": [{"id": "right", "left": 70, "top": 5}]
                }]
            }
        }))
        .unwrap();
        let characters_document = CharactersDocument {
            characters: vec![character.clone()],
            ..CharactersDocument::default()
        };
        let positions = portrait_positions(&characters_document);
        let characters = HashMap::from([("hero", &character)]);
        let chapter_next = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let block = StoryBlock {
            id: None,
            kind: "updateCharacter".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("characterId".into(), json!("hero")),
                ("expression".into(), json!("smile")),
                ("distance".into(), json!("near")),
                ("position".into(), json!("right")),
                ("placementTransitionEnabled".into(), json!(true)),
                ("placementTransitionDuration".into(), json!(500)),
                ("placementTransitionEasing".into(), json!("outCubic")),
                ("placementTransitionBlocking".into(), json!(true)),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        update_character(
            &block,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        assert!(matches!(
            &report.actions[0],
            Action::UpdateSprite {
                image,
                position: Position { x: keine_core::Anchor::Left(x), y },
                scale,
                duration,
                easing: Easing::OutCubic,
                blocking: true,
                ..
            } if image == "characters/smile.webp"
                && (*x - 1344.0).abs() < f32::EPSILON
                && (*y - 54.0).abs() < f32::EPSILON
                && (*scale - 1.25).abs() < f32::EPSILON
                && (*duration - 0.5).abs() < f32::EPSILON
        ));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn studio_198_sequence_portraits_compile_to_native_frame_playback() {
        let character: CharacterDefinition = serde_json::from_value(json!({
            "id": "hero",
            "name": "Hero",
            "expressions": [
                {
                    "name": "animated",
                    "presentation": {
                        "type": "sequence",
                        "frameExpressionNames": ["frame-1", "frame-2"],
                        "fps": 8,
                        "loop": true
                    }
                },
                {"name": "frame-1", "assetPath": "characters/frame-1.webp"},
                {"name": "frame-2", "assetPath": "characters/frame-2.webp"}
            ]
        }))
        .unwrap();
        let characters = HashMap::from([("hero", &character)]);
        let chapter_next = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let block = StoryBlock {
            id: None,
            kind: "showCharacter".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("characterId".into(), json!("hero")),
                ("expression".into(), json!("animated")),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        show_character(
            &block,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        assert!(matches!(
            &report.actions[0],
            Action::ShowSprite { image, .. } if image == "characters/frame-1.webp"
        ));
        assert!(matches!(
            &report.actions[1],
            Action::ConfigureSpriteSequence { frames, fps, looped, .. }
                if frames == &["characters/frame-1.webp", "characters/frame-2.webp"]
                    && *fps == 8.0
                    && *looped
        ));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn studio_197_character_distance_layout_overrides_global_layout() {
        let characters: CharactersDocument = serde_json::from_value(json!({
            "globalSettings": {
                "defaultDistanceId": "near",
                "distancePresets": [{
                    "id": "near",
                    "scale": 1.2,
                    "positions": [{"id": "center", "left": 30, "top": 4}]
                }]
            },
            "characters": [{
                "id": "hero",
                "name": "Hero",
                "portraitLayout": {
                    "defaultDistanceId": "close",
                    "defaultPositionId": "center",
                    "distancePresets": [{
                        "id": "close",
                        "scale": 1.5,
                        "positions": [{"id": "center", "left": 42, "top": 7}]
                    }]
                }
            }]
        }))
        .unwrap();

        let positions = portrait_positions(&characters);
        for (key, expected) in [
            ("\0\0center", (30.0, 4.0, 1.2)),
            ("hero\0\0center", (42.0, 7.0, 1.5)),
        ] {
            let placement = &positions[key];
            assert_eq!((placement.left, placement.top, placement.scale), expected);
            assert!(placement.canvas_anchor.is_none());
        }
    }

    #[test]
    fn sentence_tail_deletion_extension_lowers_to_native_ir() {
        for target in [
            "shiftz.backspace/backspace-to",
            "maincore.backspace-to/backspace-to",
        ] {
            let block = StoryBlock {
                id: Some("backspace".into()),
                kind: "callExtensionFunction".into(),
                content: Value::Null,
                props: Map::from_iter([
                    ("target".into(), json!(target)),
                    (
                        "paramsJson".into(),
                        json!({
                            "source": {"kind":"lit","value":"我当然来了"},
                            "keep": {"kind":"lit","value":"我当然"}
                        }),
                    ),
                ]),
                children: Vec::new(),
                extras: Map::new(),
            };
            let mut report = ParseReport::default();

            assert!(compile_known_extension(
                &block,
                SourceSpan { line: 1, column: 1 },
                &mut report
            ));
            assert_eq!(
                report.actions,
                vec![Action::RetractDialogue {
                    source: "我当然来了".into(),
                    keep: "我当然".into(),
                }]
            );
            assert!(report.diagnostics.is_empty());
        }
    }

    #[test]
    fn invalid_sentence_tail_deletion_is_not_forwarded_to_the_host() {
        let block = StoryBlock {
            id: Some("backspace".into()),
            kind: "callExtensionFunction".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("target".into(), json!("shiftz.backspace/backspace-to")),
                (
                    "paramsJson".into(),
                    json!({
                        "source": {"kind":"lit","value":"原文"},
                        "keep": {"kind":"lit","value":"不匹配"}
                    }),
                ),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        assert!(compile_known_extension(
            &block,
            SourceSpan { line: 1, column: 1 },
            &mut report
        ));
        assert!(report.actions.is_empty());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.level == DiagnosticLevel::Error)
        );
    }

    #[test]
    fn sentence_tail_deletion_can_remove_the_entire_line() {
        let block = StoryBlock {
            id: Some("backspace-all".into()),
            kind: "callExtensionFunction".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("target".into(), json!("shiftz.backspace/backspace-to")),
                (
                    "paramsJson".into(),
                    json!({
                        "source": {"kind":"lit","value":"整句话"},
                        "keep": {"kind":"lit","value":""}
                    }),
                ),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        assert!(compile_known_extension(
            &block,
            SourceSpan { line: 1, column: 1 },
            &mut report
        ));
        assert_eq!(
            report.actions,
            vec![Action::RetractDialogue {
                source: "整句话".into(),
                keep: String::new(),
            }]
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn sentence_tail_deletion_can_omit_keep_to_remove_the_entire_line() {
        let block = StoryBlock {
            id: Some("backspace-all-legacy".into()),
            kind: "callExtensionFunction".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("target".into(), json!("shiftz.backspace/backspace-to")),
                (
                    "paramsJson".into(),
                    json!({
                        "source": {"kind":"lit","value":"整句话"},
                        "sourceBlockId": "dialogue-block",
                        "waitMs": {"kind":"lit","value":500}
                    }),
                ),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        assert!(compile_known_extension(
            &block,
            SourceSpan { line: 1, column: 1 },
            &mut report
        ));
        assert_eq!(
            report.actions,
            vec![Action::RetractDialogue {
                source: "整句话".into(),
                keep: String::new(),
            }]
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn legacy_sentence_tail_deletion_can_resolve_source_from_current_dialogue() {
        let block = StoryBlock {
            id: Some("backspace".into()),
            kind: "callExtensionFunction".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("target".into(), json!("shiftz.backspace/backspace-to")),
                (
                    "paramsJson".into(),
                    json!({"keep": {"kind":"lit","value":"我当然"}}),
                ),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        assert!(compile_known_extension(
            &block,
            SourceSpan { line: 1, column: 1 },
            &mut report
        ));
        assert_eq!(
            report.actions,
            vec![Action::RetractDialogue {
                source: String::new(),
                keep: "我当然".into(),
            }]
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn every_studio_180_block_compiles_to_runtime_ir() {
        let character: CharacterDefinition = serde_json::from_value(json!({
            "id": "character",
            "name": "Character",
            "expressions": [{"name":"default","assetPath":"characters/a.png"}]
        }))
        .unwrap();
        let character_map = HashMap::from([("character", &character)]);
        let chapter_next = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &character_map,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let chapter: ChapterDocument = serde_json::from_value(json!({
            "id":"chapter", "name":"Chapter", "fragments":[]
        }))
        .unwrap();

        for kind in BUILTIN_BLOCK_TYPES {
            let mut props = Map::new();
            props.insert("characterId".into(), json!("character"));
            props.insert("uri".into(), json!("backgrounds/a.png"));
            props.insert("target".into(), json!("slot:internal.system.title"));
            props.insert("key".into(), json!("value"));
            props.insert("aLit".into(), json!("1"));
            props.insert("thenFragmentId".into(), json!("entry"));
            props.insert("url".into(), json!("https://example.com"));
            if *kind == "branch" {
                props.insert(
                    "choices".into(),
                    json!([{"mode":"jump", "text":"Continue", "fragmentId":""}]),
                );
            }
            if *kind == "stageAnimation" {
                props.insert(
                    "clipJson".into(),
                    json!({"version":1,"duration":1000,"tracks":[],"events":[]}),
                );
            }
            let block = StoryBlock {
                id: None,
                kind: (*kind).into(),
                content: json!([{"type":"text","text":"line"}]),
                props,
                children: Vec::new(),
                extras: Map::new(),
            };
            let mut report = ParseReport::default();
            compile_block(
                &block,
                &chapter,
                &context,
                SourceSpan { line: 1, column: 1 },
                &mut report,
            );
            // An empty camera block is a valid no-op in Studio. Every other
            // built-in must still lower to at least one runtime action.
            if !matches!(
                *kind,
                "camera" | "openExternalUrl" | "steamAction" | "unlockSteamAchievement"
            ) {
                assert!(!report.actions.is_empty(), "{kind} did not emit runtime IR");
            }
            if matches!(
                *kind,
                "openExternalUrl" | "steamAction" | "unlockSteamAchievement"
            ) {
                assert!(report.diagnostics.iter().any(|diagnostic| {
                    diagnostic.level == DiagnosticLevel::Error
                        && (diagnostic.message.contains("no Steam runtime bridge")
                            || diagnostic.message.contains("platform-neutral"))
                }));
            } else {
                assert!(
                    report
                        .diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.level != DiagnosticLevel::Error),
                    "{kind} emitted an error: {:?}",
                    report.diagnostics
                );
            }
            if *kind != "callExtensionFunction" {
                assert!(
                    report
                        .actions
                        .iter()
                        .all(|action| !contains_host_command(action)),
                    "built-in {kind} leaked through the third-party extension bridge"
                );
            }
        }
    }

    #[test]
    fn studio_180_stage_animation_covers_every_declared_property() {
        let names = [
            "x",
            "y",
            "zoom",
            "scaleX",
            "scaleY",
            "alpha",
            "rotation",
            "width",
            "height",
            "focalDistance",
            "blurStrength",
            "distortionStrength",
            "vignetteIntensity",
            "vignetteSize",
            "blurAmount",
            "colorToneIntensity",
            "colorExposure",
            "colorBrightness",
            "colorContrast",
            "colorSaturation",
            "colorTemperature",
            "oldFilmIntensity",
            "shockIntensity",
            "godrayIntensity",
            "godrayAngle",
            "godrayGain",
            "godrayLacunarity",
            "godraySpeed",
            "godrayCenterX",
            "godrayCenterY",
            "lutIntensity",
            "bloomIntensity",
            "chromaticAberration",
            "pixelateSize",
            "glitchIntensity",
            "crtIntensity",
            "sharpenStrength",
            "radialBlurStrength",
            "radialBlurCenterX",
            "radialBlurCenterY",
            "motionBlurStrength",
            "motionBlurAngle",
            "zoomBlurStrength",
            "zoomBlurCenterX",
            "zoomBlurCenterY",
            "lightLeakIntensity",
            "lightLeakAngle",
            "lensFlareIntensity",
            "lensFlareCenterX",
            "lensFlareCenterY",
            "filmGrainIntensity",
            "filmGrainSize",
            "heatHazeIntensity",
            "heatHazeSpeed",
            "heatHazeScale",
            "waterRippleIntensity",
            "waterRippleFrequency",
            "waterRippleSpeed",
            "waterRippleCenterX",
            "waterRippleCenterY",
            "fogIntensity",
            "fogSpeed",
            "fogScale",
            "vhsIntensity",
            "vhsJitter",
            "vhsNoise",
            "halftoneIntensity",
            "halftoneScale",
            "halftoneAngle",
            "ditherIntensity",
            "ditherLevels",
            "outlineIntensity",
            "outlineThickness",
            "eyelidOpenness",
            "eyelidWidth",
            "eyelidCurvature",
            "eyelidSoftness",
            "eyelidCenterX",
            "eyelidCenterY",
        ];
        let properties = names
            .iter()
            .map(|name| stage_property(name).unwrap_or_else(|| panic!("missing {name}")))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(names.len(), 79);
        assert_eq!(properties.len(), names.len());
    }

    #[test]
    fn studio_190_stage_animation_compiles_targets_audio_and_playback_contract() {
        let character: CharacterDefinition = serde_json::from_value(json!({
            "id": "hero",
            "name": "Hero",
            "expressions": [{"name":"smile","assetPath":"characters/smile.png"}]
        }))
        .unwrap();
        let characters = HashMap::from([("hero", &character)]);
        let chapter_next = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let block = StoryBlock {
            id: Some("timeline-block".into()),
            kind: "stageAnimation".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("name".into(), json!("chapter-intro")),
                ("loop".into(), json!("2")),
                ("playbackRate".into(), json!("1.5")),
                ("waitForComplete".into(), json!(false)),
                (
                    "clipJson".into(),
                    json!({
                        "version": 1,
                        "duration": 2000,
                        "tracks": [
                            {"target":{"kind":"camera"},"property":"bloomIntensity","keyframes":[{"time":1000,"value":0.8,"easing":"easeIn"}]},
                            {"target":{"kind":"character","characterId":"hero","expressionName":"smile"},"property":"y","keyframes":[{"time":500,"value":120,"easing":"easeOut"}]},
                            {"target":{"kind":"sceneLayer","layerId":"fog"},"property":"alpha","muted":true,"keyframes":[{"time":0,"value":0.5,"easing":"linear"}]}
                        ],
                        "events": [
                            {"type":"cameraShake","time":100,"data":{"amplitude":7,"frequency":15,"duration":250,"axis":"x","falloff":"expo"}},
                            {"type":"cameraPatch","time":200,"data":{"targets":["scene"],"patch":{"fogIntensity":0.4}}},
                            {"type":"particleCue","time":300,"data":{"id":"snow","preset":"LIGHT_SNOW","texture":"particles/snow.png","duration":600,"fadeOutDuration":100,"options":{"count":24,"wind":2,"gravity":3}}},
                            {"type":"sceneCue","time":400,"data":{"sceneId":"winter","resetCamera":true,"layers":[{"id":"fog","assetPath":"background/fog.png","distance":2,"offset":"(12,24)"}]}},
                            {"type":"audioCue","time":500,"duration":750,"soundType":"SE","uri":"audio/bell.opus","volume":0.4,"loop":false,"fadeInDuration":25,"fadeOutDuration":80,"soundId":"bell"},
                            {"type":"audioCue","time":600,"muted":true,"soundType":"BGM","uri":"audio/muted.opus"}
                        ]
                    }),
                ),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        compile_stage_animation(
            &block,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        let [Action::StageAnimation { animation }] = report.actions.as_slice() else {
            panic!("expected native stage timeline: {:?}", report.actions);
        };
        assert_eq!(animation.id, "chapter-intro");
        assert_eq!(animation.duration, 2.0);
        assert_eq!(animation.repeat, 2);
        assert_eq!(animation.playback_rate, 1.5);
        assert!(!animation.infinite);
        assert!(!animation.blocking);
        assert_eq!(animation.tracks.len(), 3);
        assert!(matches!(animation.tracks[0].target, StageTarget::Camera));
        assert!(matches!(
            &animation.tracks[1].target,
            StageTarget::Character { id, image: Some(image) }
                if id == "hero" && image == "characters/smile.png"
        ));
        assert_eq!(animation.tracks[1].keyframes[0].value, -120.0);
        assert!(matches!(
            &animation.tracks[2].target,
            StageTarget::SceneLayer { id } if id == "scene-layer:fog"
        ));
        assert!(animation.tracks[2].muted);
        assert_eq!(animation.events.len(), 5);
        assert!(matches!(
            animation.events[0].kind,
            StageEventKind::CameraShake(_)
        ));
        assert!(matches!(
            animation.events[1].kind,
            StageEventKind::CameraPatch { .. }
        ));
        assert!(matches!(
            animation.events[2].kind,
            StageEventKind::Particle { .. }
        ));
        assert!(matches!(animation.events[3].kind, StageEventKind::Scene(_)));
        assert!(matches!(
            &animation.events[4].kind,
            StageEventKind::Audio(cue)
                if cue.id == "bell"
                    && cue.kind == StageAudioKind::Effect
                    && cue.file == "audio/bell.opus"
                    && (cue.volume - 0.4).abs() <= f32::EPSILON
                    && (cue.duration - 0.75).abs() <= f32::EPSILON
        ));
        assert_eq!(
            report
                .resources
                .iter()
                .map(|resource| (resource.path.as_str(), resource.kind))
                .collect::<Vec<_>>(),
            vec![
                ("characters/smile.png", crate::ResourceKind::Figure),
                ("particles/snow.png", crate::ResourceKind::Particle),
                ("background/fog.png", crate::ResourceKind::Figure),
                ("audio/bell.opus", crate::ResourceKind::Effect),
            ]
        );
    }

    #[test]
    fn studio_190_wait_and_portrait_skin_compile_to_native_actions() {
        let character: CharacterDefinition = serde_json::from_value(json!({
            "id": "aya",
            "name": "Aya",
            "expressions": [{
                "name": "smile",
                "assetPath": "characters/default.webp",
                "skinAssets": {
                    "summer": "characters/summer.webp",
                    "winter": "characters/winter.webp"
                },
                "graphicsOverride": {"heightRatio": 0.76}
            }],
            "portraitSkinConfig": {
                "skins": ["summer", "winter"],
                "defaultSkin": "summer",
                "attributeName": "skin"
            }
        }))
        .unwrap();
        let characters = HashMap::from([("aya", &character)]);
        let chapter_next = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: Some(0.82),
        };
        let chapter: ChapterDocument = serde_json::from_value(json!({
            "id":"chapter", "name":"Chapter", "fragments":[]
        }))
        .unwrap();
        let show = StoryBlock {
            id: None,
            kind: "showCharacter".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("characterId".into(), json!("aya")),
                ("expression".into(), json!("smile")),
                ("position".into(), json!("center")),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let wait = StoryBlock {
            id: None,
            kind: "wait".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("duration".into(), json!(5000)),
                ("waitForInput".into(), json!(true)),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        compile_block(
            &show,
            &chapter,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );
        compile_block(
            &wait,
            &chapter,
            &context,
            SourceSpan { line: 2, column: 1 },
            &mut report,
        );

        assert!(matches!(
            &report.actions[0],
            Action::ShowSprite {
                image,
                layout: SpriteLayout::ViewportHeight(ratio),
                ..
            } if image == "characters/summer.webp" && (*ratio - 0.76).abs() <= f32::EPSILON
        ));
        assert!(matches!(
            &report.actions[1],
            Action::SelectSpriteImage {
                variable,
                default_image,
                variants,
                ..
            } if variable == "aya.skin"
                && default_image == "characters/summer.webp"
                && variants.as_slice() == [
                    ("summer".into(), "characters/summer.webp".into()),
                    ("winter".into(), "characters/winter.webp".into()),
                ]
        ));
        assert!(matches!(report.actions[2], Action::WaitForAdvance));
    }

    fn contains_host_command(action: &Action) -> bool {
        match action {
            Action::HostCommand { .. } => true,
            Action::Flow { action, .. } => contains_host_command(action),
            _ => false,
        }
    }

    #[test]
    fn extracts_inline_story_text_without_editor_nodes() {
        let content = json!([
            {"type":"text","text":"潮"},
            {"type":"text","text":"声","styles":{"bold":true}}
        ]);
        assert_eq!(plain_text(&content), "潮声");
    }

    #[test]
    fn translates_studio_inline_markup_and_keeps_waits_zero_width() {
        let content = json!([{
            "type": "text",
            "text": "[color=#ffffff][bold]前[/bold][/color][rt=かん]漢[/rt][wait=1000][br][bg=#315735]後[/bg]"
        }]);

        assert_eq!(
            studio_dialogue_markup(&content),
            "[前](color=#ffffff;bold)[漢](ruby=かん)[wait=1000]\n[後](background=#315735)"
        );
    }

    #[test]
    fn color_parser_rejects_non_ascii_six_byte_input_without_panicking() {
        assert_eq!(parse_color("aééa"), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            parse_color("#315735"),
            [49.0 / 255.0, 87.0 / 255.0, 53.0 / 255.0, 1.0]
        );
    }

    #[test]
    fn studio_sprite_values_keep_pixis_units_and_canvas_direction() {
        let props = Map::from_iter([
            ("x".into(), json!("center+100px")),
            ("y".into(), json!("25%")),
            ("alpha".into(), json!("0.35")),
            ("rotation".into(), json!("1.25")),
        ]);
        let transform = sprite_transform_patch(&props).apply_to(SpriteTransform::default());

        assert_eq!(transform.offset_x, keine_core::DESIGN_WIDTH * 0.5 + 100.0);
        assert_eq!(transform.offset_y, -keine_core::DESIGN_HEIGHT * 0.25);
        assert_eq!(transform.alpha, 0.35);
        assert_eq!(transform.rotation, -1.25);
        assert_eq!(parse_studio_coordinate("50%-10px", 1920.0), Some(950.0));
    }

    #[test]
    fn studio_particle_retains_texture_preset_density_and_fades() {
        let show = StoryBlock {
            id: Some("block-particle".into()),
            kind: "particle".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("mode".into(), json!("show")),
                ("effectId".into(), json!("snow")),
                ("preset".into(), json!("MODERATE_SNOW")),
                ("textureUri".into(), json!("particles/snow.png")),
                (
                    "optionsJson".into(),
                    json!(r#"{"count":84,"wind":18.5,"gravity":42.0}"#),
                ),
                ("fadeInDuration".into(), json!("250")),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();
        compile_particle(&show, SourceSpan { line: 1, column: 1 }, &mut report);

        assert!(matches!(
            report.actions.as_slice(),
            [Action::ShowParticles { id, effect }]
                if id == "snow"
                    && effect.texture.as_deref() == Some("particles/snow.png")
                    && effect.preset == "MODERATE_SNOW"
                    && effect.count == 84
                    && effect.wind == Some(18.5)
                    && effect.gravity == Some(42.0)
                    && (effect.fade_in - 0.25).abs() < f32::EPSILON
        ));

        let hide = StoryBlock {
            id: None,
            kind: "particle".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("mode".into(), json!("hide")),
                ("effectId".into(), json!("snow")),
                ("fadeOutDuration".into(), json!("400")),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();
        compile_particle(&hide, SourceSpan { line: 1, column: 1 }, &mut report);
        assert!(matches!(
            report.actions.as_slice(),
            [Action::HideParticles { id: Some(id), duration }]
                if id == "snow" && (*duration - 0.4).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn studio_dialogue_lifetime_matches_keep_dialogue() {
        let span = SourceSpan { line: 1, column: 1 };
        let retained = StoryBlock {
            id: None,
            kind: "narration".into(),
            content: json!([{"type":"text","text":"retained"}]),
            props: Map::from_iter([("keepDialogue".into(), json!(true))]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let hidden = StoryBlock {
            props: Map::from_iter([("keepDialogue".into(), json!(false))]),
            ..retained.clone()
        };

        let mut retained_report = ParseReport::default();
        push_dialogue_lifetime(&retained, span, &mut retained_report);
        assert!(retained_report.actions.is_empty());

        let mut hidden_report = ParseReport::default();
        push_dialogue_lifetime(&hidden, span, &mut hidden_report);
        assert!(matches!(
            hidden_report.actions.as_slice(),
            [Action::SetTextbox {
                visible: false,
                auto: true
            }]
        ));
    }

    #[test]
    fn native_camera_reset_preserves_studio_parallel_timing() {
        for (mode, duration, wait) in [
            ("instant", 0, false),
            ("animated", 500, true),
            ("animated", 750, false),
        ] {
            let block = StoryBlock {
                id: None,
                kind: "resetCamera".into(),
                content: Value::Null,
                props: Map::from_iter([
                    ("resetMode".into(), json!(mode)),
                    ("duration".into(), json!(duration)),
                    ("waitForComplete".into(), json!(wait)),
                    ("easing".into(), json!("easeOut")),
                ]),
                children: Vec::new(),
                extras: Map::new(),
            };
            let mut studio = ParseReport::default();
            compile_reset_camera(&block, SourceSpan { line: 1, column: 1 }, &mut studio);
            let native = crate::parse_native_scenes(&format!(
                "scene a {{ camera.reset(all, duration: {duration}ms, easing: ease_out, blocking: {wait}) }}"
            ));
            assert!(
                native[0].report.diagnostics.is_empty(),
                "{:?}",
                native[0].report.diagnostics
            );
            assert_eq!(native[0].report.actions, studio.actions);
            assert!(
                matches!(&studio.actions[0], Action::Flow { action, next: true, .. }
                if matches!(action.as_ref(), Action::ShakeCamera { shake, .. } if shake.duration == 0.0))
            );
            assert!(
                studio.actions[..3]
                    .iter()
                    .all(|action| matches!(action, Action::Flow { next: true, .. }))
            );
            assert!(
                matches!(studio.actions.last(), Some(Action::Flow { next, .. }) if *next == !wait)
            );
        }
        for args in [
            "all, duration: -1ms",
            "all, typo: true",
            "all, easing: nope",
            "all, blocking: 1",
            "nope",
            "all, all",
        ] {
            let parsed = crate::parse_native_scenes(&format!("scene a {{ camera.reset({args}) }}"));
            assert!(!parsed[0].report.diagnostics.is_empty(), "{args}");
            assert!(parsed[0].report.actions.is_empty(), "{args}");
        }
    }

    #[test]
    fn studio_scene_reset_camera_runs_before_composite_scene() {
        let scene: SceneDefinition = serde_json::from_value(json!({
            "id": "winter",
            "name": "Winter",
            "layers": [{"id":"background", "assetPath":"winter.png"}]
        }))
        .unwrap();
        let scenes = HashMap::from([("winter", &scene)]);
        let chapter_next = HashMap::new();
        let characters = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let block = StoryBlock {
            id: None,
            kind: "scene".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("sceneId".into(), json!("winter")),
                ("resetCamera".into(), json!(true)),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        compile_scene(
            &block,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        assert!(matches!(
            report.actions.get(1),
            Some(Action::Flow { action, next: true, .. })
                if matches!(action.as_ref(), Action::ShakeCamera { shake, .. } if shake.duration == 0.0)
        ));
        assert!(matches!(
            report.actions.get(2),
            Some(Action::Flow { action, next: true, .. })
                if matches!(action.as_ref(), Action::SetCameraTransform { duration, .. } if *duration == 0.0)
        ));
        let show_index = report
            .actions
            .iter()
            .position(|action| matches!(
                action,
                Action::Flow { action, .. }
                    if matches!(action.as_ref(), Action::ShowSprite { image, .. } if image == "winter.png")
            ))
            .unwrap();
        assert!(show_index >= 5, "scene appeared before the camera reset");
    }

    #[test]
    fn studio_scene_lowest_layer_shares_the_authored_canvas_layout() {
        let scene: SceneDefinition = serde_json::from_value(json!({
            "id": "train",
            "name": "Train",
            "layers": [
                {"id":"sky", "assetPath":"sky.png", "distance":9.4},
                {"id":"train", "assetPath":"train.png", "distance":2.7}
            ]
        }))
        .unwrap();
        let scenes = HashMap::from([("train", &scene)]);
        let chapter_next = HashMap::new();
        let characters = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let block = StoryBlock {
            id: None,
            kind: "scene".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("sceneId".into(), json!("train")),
                ("displayType".into(), json!("by_height")),
                ("position".into(), json!("(0%,0%)")),
                ("anchor".into(), json!("top-left")),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        compile_scene(
            &block,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        let layers = report
            .actions
            .iter()
            .filter_map(|action| match action {
                Action::Flow { action, .. } => match action.as_ref() {
                    Action::ShowSprite { id, layout, .. } if id.starts_with("scene-layer:") => {
                        Some((id.as_str(), *layout))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].0, "scene-layer:sky");
        assert_eq!(layers[1].0, "scene-layer:train");
        assert!(layers.iter().all(|(_, layout)| matches!(
            layout,
            SpriteLayout::Scene(SceneLayerLayout {
                fit: SceneFit::ByHeight,
                position: [0.0, 0.0],
                anchor: [0.0, 0.0],
                ..
            })
        )));
        assert!(!report.actions.iter().any(|action| matches!(
            action,
            Action::Flow { action, .. } if matches!(action.as_ref(), Action::ShowBg { .. })
        )));
    }

    #[test]
    fn studio_rich_input_lowers_to_typed_neutral_contract() {
        let block = StoryBlock {
            id: None,
            kind: "playerInput".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("variable".into(), json!("age")),
                ("valueType".into(), json!("number")),
                ("title".into(), json!("年龄")),
                ("description".into(), json!("请输入 12 到 18")),
                ("placeholder".into(), json!("15")),
                ("confirmText".into(), json!("继续")),
                ("requiredText".into(), json!("年龄不能为空")),
                ("minValue".into(), json!(12)),
                ("maxValue".into(), json!(18)),
                ("step".into(), json!(1)),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let chapter: ChapterDocument = serde_json::from_value(json!({
            "id":"chapter", "name":"Chapter", "fragments":[]
        }))
        .unwrap();
        let chapter_next = HashMap::new();
        let characters = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let mut report = ParseReport::default();

        compile_block(
            &block,
            &chapter,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        let [Action::RequestInput { spec }] = report.actions.as_slice() else {
            panic!("expected typed input action: {:?}", report.actions);
        };
        assert_eq!(spec.variable, "age");
        assert_eq!(spec.value_type, InputValueType::Number);
        assert_eq!(spec.min_value, Some(12.0));
        assert_eq!(spec.max_value, Some(18.0));
        assert_eq!(spec.confirm_text, "继续");
    }

    #[test]
    fn dialogue_style_presets_compile_to_native_presentation_state() {
        let block = StoryBlock {
            id: None,
            kind: "switchDialogueStyle".into(),
            content: Value::Null,
            props: Map::from_iter([("targetId".into(), json!("cinematic-centered"))]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let chapter: ChapterDocument = serde_json::from_value(json!({
            "id":"chapter", "name":"Chapter", "fragments":[]
        }))
        .unwrap();
        let chapter_next = HashMap::new();
        let characters = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let mut report = ParseReport::default();

        compile_block(
            &block,
            &chapter,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        assert!(matches!(
            report.actions.as_slice(),
            [Action::SetDialogueStyle {
                style: keine_core::DialogueStyle::CinematicCentered
            }]
        ));
    }

    #[test]
    fn studio_timeline_frames_compile_without_a_host_fallback() {
        let block = StoryBlock {
            id: None,
            kind: "animateSprite".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("targetType".into(), json!("character")),
                ("targetId".into(), json!("hero")),
                (
                    "frames".into(),
                    json!(
                        r#"[{"duration":1000,"properties":{"x":"120"}},{"duration":250,"properties":{}}]"#
                    ),
                ),
                ("loop".into(), json!("2")),
                ("waitForComplete".into(), json!("true")),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let chapter: ChapterDocument = serde_json::from_value(json!({
            "id":"chapter", "name":"Chapter", "fragments":[]
        }))
        .unwrap();
        let chapter_next = HashMap::new();
        let characters = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let mut report = ParseReport::default();

        compile_block(
            &block,
            &chapter,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        let [
            Action::AnimateKeyframes {
                target,
                frames,
                repeat,
                blocking,
            },
        ] = report.actions.as_slice()
        else {
            panic!("expected one native timeline action: {:?}", report.actions);
        };
        assert_eq!(target, "hero");
        assert_eq!(frames.len(), 2);
        assert_eq!(*repeat, 2);
        assert!(*blocking);
        assert!(!frames[0].transform.is_empty());
        assert!(frames[1].transform.is_empty());
    }

    #[test]
    fn studio_scene_replaces_stale_layers_and_starts_composite_transition_together() {
        let old_scene: SceneDefinition = serde_json::from_value(json!({
            "id": "old",
            "name": "Old",
            "layers": [
                {"id":"old-bg", "assetPath":"old-bg.png"},
                {"id":"old-overlay", "assetPath":"old-overlay.png"}
            ]
        }))
        .unwrap();
        let new_scene: SceneDefinition = serde_json::from_value(json!({
            "id": "new",
            "name": "New",
            "layers": [
                {"id":"new-bg", "assetPath":"new-bg.png"},
                {"id":"new-overlay", "assetPath":"new-overlay.png"}
            ]
        }))
        .unwrap();
        let scenes: HashMap<String, SceneDefinition> =
            HashMap::from([("old".into(), old_scene), ("new".into(), new_scene)]);
        let scene_refs = scenes
            .iter()
            .map(|(id, scene)| (id.as_str(), scene))
            .collect();
        let chapter_next = HashMap::new();
        let characters = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scene_refs,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let block = StoryBlock {
            id: None,
            kind: "scene".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("sceneId".into(), json!("new")),
                ("transitionDuration".into(), json!(400)),
                ("waitForComplete".into(), json!(true)),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();

        compile_scene(
            &block,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        assert!(matches!(
            &report.actions[1],
            Action::Flow { action, next: true, .. }
                if matches!(action.as_ref(), Action::HideSprites { prefix, .. } if prefix == "scene-layer:")
        ));
        assert!(matches!(&report.actions[2], Action::HideParticleLayers));
        assert!(matches!(
            &report.actions[3],
            Action::Flow { action, next: true, .. }
                if matches!(action.as_ref(), Action::HideBg { .. })
        ));
        assert!(matches!(
            &report.actions[4],
            Action::Flow { action, next: true, .. }
                if matches!(action.as_ref(), Action::ShowSprite {
                    id,
                    image,
                    layout: SpriteLayout::Scene(_),
                    ..
                } if id == "scene-layer:new-bg" && image == "new-bg.png")
        ));
        assert!(matches!(
            &report.actions[5],
            Action::SetCameraBinding { target, distance, .. }
                if target == "scene-layer:new-bg" && *distance == 1.0
        ));
        assert!(matches!(
            &report.actions[6],
            Action::Flow { action, next: true, .. }
                if matches!(action.as_ref(), Action::ShowSprite { id, .. } if id == "scene-layer:new-overlay")
        ));
        assert!(matches!(
            &report.actions[7],
            Action::SetCameraBinding { target, distance, .. }
                if target == "scene-layer:new-overlay" && *distance == 1.0
        ));
        assert!(matches!(
            report.actions[8],
            Action::Wait { seconds } if seconds == 0.4
        ));
    }

    #[test]
    fn studio_single_frame_animation_preserves_loop_and_nonblocking_mode() {
        let block = StoryBlock {
            id: None,
            kind: "animateSprite".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("targetType".into(), json!("character")),
                ("targetId".into(), json!("hero")),
                ("x".into(), json!("center+20")),
                ("duration".into(), json!(750)),
                ("easing".into(), json!("easeOut")),
                ("loop".into(), json!("3")),
                ("waitForComplete".into(), json!("false")),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let chapter: ChapterDocument = serde_json::from_value(json!({
            "id":"chapter", "name":"Chapter", "fragments":[]
        }))
        .unwrap();
        let chapter_next = HashMap::new();
        let characters = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let mut report = ParseReport::default();

        compile_block(
            &block,
            &chapter,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        let [
            Action::AnimateKeyframes {
                target,
                frames,
                repeat,
                blocking,
            },
        ] = report.actions.as_slice()
        else {
            panic!("expected one native timeline action: {:?}", report.actions);
        };
        assert_eq!(target, "hero");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].duration, 0.75);
        assert_eq!(frames[0].easing, Easing::EaseOut);
        assert_eq!(*repeat, 3);
        assert!(!blocking);
    }

    #[test]
    fn named_sound_preserves_loop_flag_fade_and_stop() {
        let mut block: StoryBlock = serde_json::from_value(json!({
            "type": "sound", "props": { "soundType": "SE", "soundId": "door", "uri": "se/door.wav", "loop": "false", "volume": "40", "fadeDuration": "200" }
        })).unwrap();
        let span = SourceSpan { line: 1, column: 1 };
        let mut report = ParseReport::default();
        compile_sound(&block, span, &mut report);
        assert!(
            matches!(&report.actions[0], Action::SoundEffect { id: Some(id), looped: false, fade, volume, .. }
            if id == "door" && *fade == 0.2 && *volume == 0.4)
        );
        assert_eq!(report.resources[0].kind, crate::ResourceKind::Effect);
        block.props.insert("loop".into(), json!("true"));
        compile_sound(&block, span, &mut report);
        assert!(matches!(
            &report.actions[1],
            Action::SoundEffect { looped: true, .. }
        ));
        compile_stop_sound(&block, span, &mut report);
        assert!(
            matches!(&report.actions[2], Action::SoundEffect { file: None, id: Some(id), fade, .. }
            if id == "door" && *fade == 0.2)
        );
    }

    #[test]
    fn camera_ignores_unset_unsupported_tweens_but_rejects_active_ones() {
        let mut block = StoryBlock {
            id: None,
            kind: "camera".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("offsetX".into(), json!(120)),
                (
                    "tweenFields".into(),
                    json!("offsetX,inkSplashIntensity,smokeOverlayIntensity"),
                ),
                ("inkSplashIntensity".into(), json!("")),
                ("smokeOverlayIntensity".into(), Value::Null),
                ("duration".into(), json!(500)),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let span = SourceSpan { line: 1, column: 1 };
        let mut report = ParseReport::default();
        compile_camera(&block, span, &mut report);
        assert!(report.diagnostics.is_empty());
        assert!(
            matches!(report.actions.as_slice(), [Action::Flow { action, .. }]
            if matches!(action.as_ref(), Action::SetCameraTween { spec }
                if spec.fields == [CameraTweenField::X]))
        );

        block.props.insert("inkSplashIntensity".into(), json!(0.5));
        let mut report = ParseReport::default();
        compile_camera(&block, span, &mut report);
        assert!(report.actions.is_empty());
        assert!(
            matches!(report.diagnostics.as_slice(), [Diagnostic { level: DiagnosticLevel::Error, message, .. }]
            if message.contains("inkSplashIntensity"))
        );
    }

    #[test]
    fn camera_preserves_studio_tween_masks_and_randomness_units() {
        let block = StoryBlock {
            id: None,
            kind: "camera".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("offsetX".into(), json!(120)),
                ("zoom".into(), json!(1.5)),
                ("blurAmount".into(), json!(3)),
                (
                    "tweenFields".into(),
                    json!("offsetX,blurAmount,shakeAmplitude,shakeFrequency"),
                ),
                ("duration".into(), json!(500)),
                ("shakeAmplitude".into(), json!(8)),
                ("shakeFrequency".into(), json!(12)),
                ("shakeAmplitudeRandomness".into(), json!(30)),
                ("shakeFrequencyRandomness".into(), json!(20)),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut report = ParseReport::default();
        compile_camera(&block, SourceSpan { line: 1, column: 1 }, &mut report);
        assert!(report.diagnostics.is_empty());
        assert_eq!(report.actions.len(), 1);
        assert!(
            matches!(&report.actions[0], Action::Flow { action, .. } if matches!(action.as_ref(), Action::SetCameraTween { spec } if spec.fields == [CameraTweenField::X, CameraTweenField::BlurAmount, CameraTweenField::ShakeAmplitude, CameraTweenField::ShakeFrequency] && spec.transform.is_some() && spec.effect.is_some()))
        );
        assert!(matches!(&report.actions[0], Action::Flow { action, .. }
            if matches!(action.as_ref(), Action::SetCameraTween { spec }
                if spec.shake.is_some_and(|value| value.shake.duration == 0.5
                    && value.randomness.amplitude == 0.3 && value.randomness.frequency == 0.2))));
    }

    #[test]
    fn camera_base_effects_compile_natively_and_post_process_is_explicit() {
        let native = StoryBlock {
            id: None,
            kind: "camera".into(),
            content: Value::Null,
            props: Map::from_iter([
                ("offsetX".into(), json!(24)),
                ("zoom".into(), json!(1.1)),
                ("blurAmount".into(), json!(3)),
                ("shakeAmplitude".into(), json!(8)),
                ("shakeFrequency".into(), json!(12)),
                ("shakeDuration".into(), json!(240)),
                ("shakeWaitForComplete".into(), json!(true)),
                ("godrayIntensity".into(), json!(0.45)),
                ("godrayAngle".into(), json!(30)),
                ("godrayGain".into(), json!(0.5)),
                ("godrayLacunarity".into(), json!(2.5)),
                ("godraySpeed".into(), json!(1.0)),
                ("godrayParallel".into(), json!(false)),
                ("godrayCenterX".into(), json!(0.4)),
                ("godrayCenterY".into(), json!(0.2)),
                ("targets".into(), json!("scene,characters")),
                ("waitForComplete".into(), json!(true)),
            ]),
            children: Vec::new(),
            extras: Map::new(),
        };
        let mut native_report = ParseReport::default();
        compile_camera(
            &native,
            SourceSpan { line: 1, column: 1 },
            &mut native_report,
        );

        assert_eq!(native_report.actions.len(), 3);
        assert!(
            native_report
                .actions
                .iter()
                .all(|action| !matches!(action, Action::HostCommand { .. }))
        );
        assert!(matches!(
            native_report.actions.get(1),
            Some(Action::Flow { next: false, .. })
        ));
        assert!(matches!(
            native_report.actions.last(),
            Some(Action::ShakeCamera { blocking: true, .. })
        ));

        let mut post_process = native;
        post_process
            .props
            .insert("vignetteIntensity".into(), json!(0.5));
        let mut post_report = ParseReport::default();
        compile_camera(
            &post_process,
            SourceSpan { line: 1, column: 1 },
            &mut post_report,
        );
        assert_eq!(
            post_report
                .actions
                .iter()
                .filter(|action| matches!(
                    action,
                    Action::Flow { action, .. }
                        if matches!(action.as_ref(), Action::SetPostProcess { .. })
                ))
                .count(),
            1
        );
        let effect = post_report
            .actions
            .iter()
            .find_map(|action| match action {
                Action::Flow { action, .. } => match action.as_ref() {
                    Action::SetPostProcess { effect, .. } => Some(effect),
                    _ => None,
                },
                _ => None,
            })
            .expect("camera should retain LetsGal Godray properties");
        assert_eq!(effect.godray_intensity, Some(0.45));
        assert_eq!(effect.godray_angle, Some(30.0));
        assert_eq!(effect.godray_gain, Some(0.5));
        assert_eq!(effect.godray_lacunarity, Some(2.5));
        assert_eq!(effect.godray_speed, Some(1.0));
        assert_eq!(effect.godray_parallel, Some(false));
        assert_eq!(effect.godray_center_x, Some(0.4));
        assert_eq!(effect.godray_center_y, Some(0.2));
    }

    #[test]
    fn maps_manifest_hash_and_native_path_to_the_same_asset() {
        let project: ProjectDocument = serde_json::from_value(json!({
            "id":"p", "name":"n", "engineVersion":"1", "chapterOrder":[]
        }))
        .unwrap();
        let manifest: AssetManifest = serde_json::from_value(json!({
            "entries":{"hash":{"path":"backgrounds/sea.png"}}
        }))
        .unwrap();
        let config = game_config(&project, &manifest, None);
        assert_eq!(config.project.id, "p");
        assert_eq!(config.bg_path("hash"), "backgrounds/sea.png");
        assert_eq!(config.bg_path("backgrounds/sea.png"), "backgrounds/sea.png");
        assert_eq!(config.layout.anchor_offset, 0.0);
    }

    #[test]
    fn derives_a_stable_shipping_id_without_changing_valid_or_explicit_ids() {
        let manifest: AssetManifest = serde_json::from_value(json!({"entries": {}})).unwrap();
        let valid: ProjectDocument = serde_json::from_value(json!({
            "id":"already-valid", "name":"n"
        }))
        .unwrap();
        assert_eq!(
            game_config(&valid, &manifest, None).project.id,
            "already-valid"
        );

        let editor_native: ProjectDocument = serde_json::from_value(json!({
            "id":"Project_测试.01", "name":"n"
        }))
        .unwrap();
        let first = game_config(&editor_native, &manifest, None).project.id;
        let second = game_config(&editor_native, &manifest, None).project.id;
        assert_eq!(first, second);
        assert!(first.starts_with("letsgal-project-01-"), "{first}");
        assert!(
            ProjectMetadata {
                id: first,
                ..ProjectMetadata::default()
            }
            .valid_id()
            .is_some()
        );

        let explicit: ProjectDocument = serde_json::from_value(json!({
            "id":"Project_测试.01",
            "name":"n",
            "keine":{"projectId":"fixed-release-id"}
        }))
        .unwrap();
        assert_eq!(
            game_config(&explicit, &manifest, None).project.id,
            "fixed-release-id"
        );
    }

    #[test]
    fn maps_generic_audio_directories_for_each_authored_audio_class() {
        let project: ProjectDocument = serde_json::from_value(json!({
            "id":"p", "name":"n", "engineVersion":"1", "chapterOrder":[]
        }))
        .unwrap();
        let manifest: AssetManifest = serde_json::from_value(json!({
            "entries":{"click":{"path":"audio/timeline-click.opus"}}
        }))
        .unwrap();

        let config = game_config(&project, &manifest, None);

        assert_eq!(
            config.bgm_path("audio/timeline-click.opus"),
            "audio/timeline-click.opus"
        );
        assert_eq!(
            config.voice_path("audio/timeline-click.opus"),
            "audio/timeline-click.opus"
        );
        assert_eq!(
            config.effect_path("audio/timeline-click.opus"),
            "audio/timeline-click.opus"
        );
    }

    #[test]
    fn maps_explicit_keine_feature_opt_ins() {
        let project: ProjectDocument = serde_json::from_value(json!({
            "id": "p",
            "name": "n",
            "engineVersion": "1",
            "chapterOrder": [],
            "keine": {
                "features": {
                    "extra": true
                }
            }
        }))
        .unwrap();
        let manifest: AssetManifest = serde_json::from_value(json!({"entries": {}})).unwrap();

        assert!(game_config(&project, &manifest, None).features.extra);
    }

    #[test]
    fn maps_studio_191_dialogue_reveal_settings() {
        let project: ProjectDocument = serde_json::from_value(json!({
            "id": "p", "name": "n", "engineVersion": "1", "chapterOrder": []
        }))
        .unwrap();
        let manifest: AssetManifest = serde_json::from_value(json!({"entries": {}})).unwrap();
        let behavior: DialogueBehavior = serde_json::from_value(json!({
            "text_speed": 40,
            "char_fade_in_duration": 180,
            "text_reveal_effect": "flip",
            "text_reveal_parameters": {
                "distance_px": 12,
                "scale_percent": 76,
                "rotation_degrees": 55,
                "blur_px": 6
            }
        }))
        .unwrap();

        let config = game_config(&project, &manifest, Some(&behavior));
        assert_eq!(config.styles.typewriter_speed, 25.0);
        assert_eq!(config.styles.text_reveal.duration, 0.18);
        assert_eq!(
            config.styles.text_reveal.effect,
            keine_core::config::TextRevealEffect::Flip
        );
        assert_eq!(config.styles.text_reveal.distance, 12.0);
        assert_eq!(config.styles.text_reveal.scale, 0.76);
        assert_eq!(config.styles.text_reveal.rotation, 55.0);
        assert_eq!(config.styles.text_reveal.blur, 6.0);
    }

    #[test]
    fn standalone_vocal_keeps_voice_routing_and_resource_kind() {
        let block: StoryBlock = serde_json::from_value(json!({
            "id": "voice",
            "type": "sound",
            "props": {
                "soundType": "VOCAL",
                "uri": "voice/009.wav",
                "volume": "70"
            }
        }))
        .unwrap();
        let mut report = ParseReport::default();
        compile_sound(&block, SourceSpan { line: 1, column: 1 }, &mut report);

        assert_eq!(
            report.actions,
            vec![Action::Vocal {
                file: Some("voice/009.wav".into()),
                volume: 0.7,
            }]
        );
        assert!(matches!(
            report.resources.as_slice(),
            [crate::ResourceRef {
                path,
                kind: crate::ResourceKind::Voice,
                ..
            }] if path == "voice/009.wav"
        ));
    }

    #[test]
    fn studio_200_manual_loading_strategy_resolves_authored_assets() {
        let character: CharacterDefinition = serde_json::from_value(json!({
            "id": "alice",
            "name": "Alice",
            "expressions": [{"name":"smile","assetPath":"characters/alice.webp"}]
        }))
        .unwrap();
        let scene: SceneDefinition = serde_json::from_value(json!({
            "id": "room",
            "layers": [{"id":"base","assetPath":"background/room.webp"}]
        }))
        .unwrap();
        let characters = HashMap::from([("alice", &character)]);
        let scenes = HashMap::from([("room", &scene)]);
        let chapter_next = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: None,
        };
        let block: StoryBlock = serde_json::from_value(json!({
            "type": "loadingStrategy",
            "props": {
                "mode": "manual",
                "execution": "wait",
                "resourcesJson": "[{\"kind\":\"character\",\"characterId\":\"alice\",\"expression\":\"smile\"},{\"kind\":\"scene\",\"sceneId\":\"room\"}]"
            }
        }))
        .unwrap();
        let mut report = ParseReport::default();

        compile_loading_strategy(
            &block,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
        );

        assert!(matches!(
            report.actions.as_slice(),
            [Action::ConfigureLoading { strategy }]
                if strategy.mode == LoadingStrategyMode::Manual
                    && strategy.blocking
                    && strategy.resources.len() == 2
        ));
        assert_eq!(report.resources.len(), 2);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn linear_chapter_fallthrough_keeps_preprocessing_and_auxiliary_returns() {
        let project = serde_json::from_value(json!({"id":"p", "name":"Project"})).unwrap();
        let chapter = |name: &str, mut value: Value| {
            value["name"] = name.into();
            (
                PathBuf::from(format!("{name}.json")),
                serde_json::from_value(value).unwrap(),
            )
        };
        let chapters = vec![
            chapter(
                "pre",
                json!({
                    "id":"pre-chapter", "kind":"schedule-preprocessing",
                    "fragments":[{"id":"pre", "blocks":[{"type":"comment"}]}]
                }),
            ),
            chapter(
                "one",
                json!({
                    "id":"one-chapter", "fragments":[
                        {"id":"one", "blocks":[{"type":"comment"}]},
                        {"id":"aside", "blocks":[{"type":"comment"}]}
                    ]
                }),
            ),
            chapter(
                "disabled",
                json!({
                    "id":"disabled-chapter", "disabled":true,
                    "fragments":[{"id":"disabled", "blocks":[]}]
                }),
            ),
            chapter(
                "two",
                json!({
                    "id":"two-chapter", "fragments":[{"id":"two", "blocks":[]}]
                }),
            ),
        ];
        let loaded = compile_project(
            Path::new("."),
            &project,
            &chapters,
            &CharactersDocument::default(),
            &ScenesDocument::default(),
            &AssetManifest::default(),
        )
        .unwrap();
        let actions = |name: &str| {
            &loaded
                .iter()
                .find(|scene| scene.name == name)
                .unwrap()
                .actions
        };
        assert_eq!(
            actions("one"),
            &vec![
                Action::Comment,
                Action::CallScene("pre".into()),
                Action::ChangeScene("two".into())
            ]
        );
        assert_eq!(actions("pre"), &vec![Action::Comment]);
        assert_eq!(actions("aside"), &vec![Action::Comment]);
        assert_eq!(actions("two"), &vec![Action::End]);
        assert!(!loaded.iter().any(|scene| scene.name == "disabled"));
    }

    #[test]
    fn studio_200_chapter_preprocess_wraps_entry_and_transitions() {
        let project: ProjectDocument = serde_json::from_value(json!({
            "id": "project", "name": "Project"
        }))
        .unwrap();
        let chapters = vec![
            (
                PathBuf::from("pre.json"),
                serde_json::from_value(json!({
                    "id":"pre-chapter", "name":"Pre", "kind":"schedule-preprocessing",
                    "fragments":[{"id":"pre", "blocks":[{"type":"comment"}]}]
                }))
                .unwrap(),
            ),
            (
                PathBuf::from("one.json"),
                serde_json::from_value(json!({
                    "id":"one-chapter", "name":"One",
                    "fragments":[{"id":"one", "blocks":[{"type":"endChapter"}]}]
                }))
                .unwrap(),
            ),
            (
                PathBuf::from("two.json"),
                serde_json::from_value(json!({
                    "id":"two-chapter", "name":"Two",
                    "fragments":[{"id":"two", "blocks":[{"type":"comment"}]}]
                }))
                .unwrap(),
            ),
        ];

        let loaded = compile_project(
            Path::new("."),
            &project,
            &chapters,
            &CharactersDocument::default(),
            &ScenesDocument::default(),
            &AssetManifest::default(),
        )
        .unwrap();

        let start = loaded.iter().find(|scene| scene.name == "start").unwrap();
        assert_eq!(
            start.actions,
            vec![
                Action::CallScene("pre".into()),
                Action::ChangeScene("one".into())
            ]
        );
        let first = loaded.iter().find(|scene| scene.name == "one").unwrap();
        assert_eq!(
            first.actions,
            vec![
                Action::CallScene("pre".into()),
                Action::ChangeScene("two".into())
            ]
        );
    }

    #[test]
    fn studio_200_blueprint_schedule_lowers_to_native_flow() {
        let project: ProjectDocument = serde_json::from_value(json!({
            "id": "project", "name": "Project", "scheduleMode": "advanced",
            "schedule": {"graph": {
                "nodes": [
                    {"id":"start","kind":"start"},
                    {"id":"set","kind":"set","variable":"route","value":1},
                    {"id":"chapter","kind":"chapter","chapterId":"chapter"},
                    {"id":"end","kind":"end"}
                ],
                "edges": [
                    {"source":"start","port":"next","target":"set"},
                    {"source":"set","port":"next","target":"chapter"},
                    {"source":"chapter","port":"next","target":"end"}
                ]
            }}
        }))
        .unwrap();
        let chapters = vec![(
            PathBuf::from("chapter.json"),
            serde_json::from_value(json!({
                "id":"chapter", "name":"Chapter",
                "fragments":[{"id":"chapter-main", "blocks":[{"type":"endChapter"}]}]
            }))
            .unwrap(),
        )];

        let loaded = compile_project(
            Path::new("."),
            &project,
            &chapters,
            &CharactersDocument::default(),
            &ScenesDocument::default(),
            &AssetManifest::default(),
        )
        .unwrap();

        let start = loaded.iter().find(|scene| scene.name == "start").unwrap();
        assert_eq!(
            start.actions,
            vec![Action::ChangeScene("letsgal-schedule:start".into())]
        );
        let set = loaded
            .iter()
            .find(|scene| scene.name == "letsgal-schedule:set")
            .unwrap();
        assert!(matches!(
            set.actions.as_slice(),
            [Action::Set { name, expression, .. }, Action::ChangeScene(target)]
                if name == "route" && expression == "1" && target == "letsgal-schedule:chapter"
        ));
        let chapter = loaded
            .iter()
            .find(|scene| scene.name == "letsgal-schedule:chapter")
            .unwrap();
        assert_eq!(
            chapter.actions,
            vec![
                Action::CallScene("chapter-main".into()),
                Action::ChangeScene("letsgal-schedule:end".into())
            ]
        );
        let body = loaded
            .iter()
            .find(|scene| scene.name == "chapter-main")
            .unwrap();
        assert_eq!(
            body.actions,
            vec![Action::ChangeScene("__letsgal_schedule_return".into())]
        );
    }

    #[test]
    fn studio_200_differential_portrait_uses_native_layered_sprites() {
        let character: CharacterDefinition = serde_json::from_value(json!({
            "id":"alice", "name":"Alice",
            "expressions":[{
                "name":"smile", "presentation":{
                    "type":"differential", "groupId":"face",
                    "selections":{"eyes":"open"}
                }
            }],
            "differentialPortraitGroups":[{
                "id":"face", "width":1000, "height":1500,
                "layers":[
                    {"id":"base", "defaultOptionId":"body", "options":[
                        {"id":"body", "assetPath":"characters/body.webp"}
                    ]},
                    {"id":"eyes", "defaultOptionId":"closed", "variableRules":[
                        {"variable":"mood", "operator":"eq", "value":"happy", "optionId":"open"}
                    ], "options":[
                        {"id":"closed", "assetPath":"characters/closed.webp"},
                        {"id":"open", "assetPath":"characters/open.webp", "rect":{"x":100,"y":200,"width":800,"height":500}}
                    ]}
                ]
            }]
        }))
        .unwrap();
        let characters = HashMap::from([("alice", &character)]);
        let chapter_next = HashMap::new();
        let scenes = HashMap::new();
        let voices = HashMap::new();
        let positions = HashMap::new();
        let context = CompileContext {
            entry: "entry",
            chapter_next: &chapter_next,
            characters: &characters,
            scenes: &scenes,
            voices: &voices,
            positions: &positions,
            portrait_height_ratio: Some(0.8),
        };
        let block: StoryBlock = serde_json::from_value(json!({
            "type":"showCharacter",
            "props":{"characterId":"alice","expression":"smile","animated":false}
        }))
        .unwrap();
        let mut report = ParseReport::default();

        compile_character(
            &block,
            &context,
            SourceSpan { line: 1, column: 1 },
            &mut report,
            false,
        );

        assert!(report.actions.iter().any(|action| matches!(
            action,
            Action::Flow { action, .. }
                if matches!(action.as_ref(), Action::ShowSprite {
                    id, image, layout: SpriteLayout::Composite { canvas, .. }, ..
                } if id == "alice" && image == "characters/body.webp" && *canvas == [1000.0, 1500.0])
        )));
        assert!(report.actions.iter().any(|action| matches!(
            action,
            Action::ShowSprite {
                id, image, layout: SpriteLayout::Composite { rect: Some(rect), .. }, ..
            } if id == "character-layer:alice:eyes"
                && image == "characters/open.webp"
                && *rect == [100.0, 200.0, 800.0, 500.0]
        )));
        assert!(report.actions.iter().any(|action| matches!(
            action,
            Action::SelectSpriteImageByCondition { id, variants, .. }
                if id == "character-layer:alice:eyes"
                    && variants[0].0 == "alice.mood == \"happy\""
        )));
        assert!(report.diagnostics.is_empty());
    }
}
