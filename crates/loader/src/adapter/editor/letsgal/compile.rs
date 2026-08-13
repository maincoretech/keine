use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use keine_core::action::Choice;
use keine_core::config::{
    AdapterConfig, AssetSourceConfig, GameConfig, ProjectMetadata, TextRevealConfig,
};
use keine_core::{
    Action, Anchor, BlendMode, CameraShakeAxis, CameraShakeFalloff, CameraShakeSpec, CameraTargets,
    ChoiceTarget, ColorToneMode, Easing, InputValueType, PortraitStyle, Position,
    PostProcessEffect, PostProcessPatch, SayOptions, SceneFit, SceneLayerLayout, SpriteLayout,
    SpriteTransform, StageAnimation, StageAudioCue, StageAudioKind, StageEvent, StageEventKind,
    StageKeyframe, StageProperty, StageSceneCue, StageSceneLayer, StageTarget, StageTrack,
    SystemUiSlot, TransformKeyframe, TransformPatch, Transition, UserInputSpec, VideoMode,
    VideoSpec,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::model::{
    AssetManifest, ChapterDocument, CharacterDefinition, CharactersDocument, DialogueBehavior,
    ProjectDocument, SceneDefinition, ScenesDocument, StoryBlock, StoryFragment,
    VariableDeclaration, VariablesDocument,
};
use super::read_json;
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

/// Runtime-facing block registry observed in LetsGal Studio 1.9.1's bundled
/// editor schema. `cmdDraft` is editor-only and therefore intentionally not in
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
    "hideExtensionUI",
    "if",
    "narration",
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
    "stopSound",
    "stopVideo",
    "storyParagraph",
    "switchDialogueStyle",
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
            description: project.description.clone().unwrap_or_default(),
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
            // The first layer uses keine's background renderer; later layers
            // use the generic sprite path and need the same native asset map.
            insert_aliases(&mut config.assets.figures, hash, &path);
        } else if is_figure(&path) {
            insert_aliases(&mut config.assets.figures, hash, &path);
        } else if is_bgm(&path) {
            insert_aliases(&mut config.assets.bgm, hash, &path);
        } else if is_voice(&path) {
            insert_aliases(&mut config.assets.voices, hash, &path);
        } else if is_effect(&path) {
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
        let chapter: ChapterDocument = read_json(&path)?;
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
    _project: &ProjectDocument,
    chapters: &[(PathBuf, ChapterDocument)],
    characters: &CharactersDocument,
    scenes: &ScenesDocument,
    manifest: &AssetManifest,
) -> Result<Vec<LoadedScene>> {
    let enabled = chapters
        .iter()
        .filter(|(_, chapter)| !chapter.disabled)
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
    let entry = enabled
        .get(entry_index)
        .and_then(|(_, chapter)| chapter.fragments.first())
        .map(|fragment| fragment.id.clone())
        .context("LetsGal project has no enabled fragment")?;

    let chapter_next = enabled
        .iter()
        .enumerate()
        .map(|(index, (_, chapter))| {
            let next = enabled
                .get(index + 1)
                .and_then(|(_, next)| next.fragments.first())
                .map(|fragment| fragment.id.clone());
            (chapter.id.clone(), next)
        })
        .collect::<HashMap<_, _>>();
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
    let positions = characters
        .global_settings
        .positions
        .iter()
        .map(|position| (position.id.as_str(), (position.left, position.top)))
        .collect::<HashMap<_, _>>();
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
    for (path, chapter) in enabled {
        for fragment in &chapter.fragments {
            loaded.push(compile_fragment(path, chapter, fragment, &context));
        }
    }
    // Runtime entry points stay language-neutral while every native fragment
    // keeps its Studio UUID for call/branch/debug stability.
    loaded.push(LoadedScene {
        name: "start".into(),
        path: project_root.join("project.json"),
        actions: vec![Action::ChangeScene(entry.clone())],
        action_spans: vec![SourceSpan { line: 1, column: 1 }],
        diagnostics: Vec::new(),
        resources: Vec::new(),
        sub_scenes: vec![crate::SceneRef {
            scene: context.entry.to_owned(),
            action_index: 0,
            span: SourceSpan { line: 1, column: 1 },
        }],
    });
    Ok(loaded)
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
    chapter_next: &'a HashMap<String, Option<String>>,
    characters: &'a HashMap<&'a str, &'a CharacterDefinition>,
    scenes: &'a HashMap<&'a str, &'a SceneDefinition>,
    voices: &'a HashMap<&'a str, &'a str>,
    positions: &'a HashMap<&'a str, (f32, f32)>,
    portrait_height_ratio: Option<f32>,
}

fn compile_fragment(
    path: &Path,
    chapter: &ChapterDocument,
    fragment: &StoryFragment,
    context: &CompileContext<'_>,
) -> LoadedScene {
    let mut report = ParseReport::default();
    for (index, block) in fragment.blocks.iter().enumerate() {
        let span = SourceSpan {
            line: index + 1,
            column: 1,
        };
        compile_block(block, chapter, context, span, &mut report);
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
        "removeCharacter" => report.push(
            Action::HideSprite {
                id: character_id(block),
                transition: fade(block, "animated", 0.2),
            },
            span,
        ),
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
        "particle" => compile_particle(block, span, report),
        "endChapter" => match context.chapter_next.get(&chapter.id).cloned().flatten() {
            Some(next) => report.push(Action::ChangeScene(next), span),
            None => report.push(Action::End, span),
        },
        "returnToEntry" => report.push(Action::ChangeScene(context.entry.to_owned()), span),
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
        "portraitStyleRule" => compile_portrait_rule(block, context, span, report),
        "floatingText" => compile_floating_text(block, span, report),
        "enterAutoPlay" => report.push(Action::SetAutoplay { enabled: true }, span),
        "exitAutoPlay" => report.push(Action::SetAutoplay { enabled: false }, span),
        _ => unreachable!("the 1.9.1 block registry is exhaustively matched"),
    }
}

fn compile_dialogue(
    block: &StoryBlock,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    if character(block, context).is_some()
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
    if prop_bool(&block.props, "isLast", true) && !prop_bool(&block.props, "keepCharacter", true) {
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
    let locked_skin = prop_string(&block.props, "skin");
    let skin_config = character.portrait_skin_config.as_ref();
    let default_skin = skin_config
        .map(|config| config.default_skin.as_str())
        .unwrap_or_default();
    let image = if !locked_skin.is_empty() {
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
    let position_id = prop_string_or(
        &block.props,
        "position",
        if character.default_position.is_empty() {
            "center"
        } else {
            &character.default_position
        },
    );
    report.push(
        Action::ShowSprite {
            id: character.id.clone(),
            image: image.clone(),
            position: studio_position(&position_id, context),
            layout: expression
                .graphics_override
                .height_ratio
                .or(context.portrait_height_ratio)
                .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
                .map(SpriteLayout::ViewportHeight)
                .unwrap_or(SpriteLayout::Natural),
            transition: fade(block, "animated", 0.2),
            transform: SpriteTransform::default(),
            z_index: 100,
            blend: BlendMode::Alpha,
        },
        span,
    );
    if locked_skin.is_empty()
        && let Some(config) = skin_config
        && !config.attribute_name.is_empty()
        && !expression.skin_assets.is_empty()
    {
        report.push(
            Action::SelectSpriteImage {
                id: character.id.clone(),
                variable: format!("{}.{}", character.id, config.attribute_name),
                default_image: image,
                variants: expression
                    .skin_assets
                    .iter()
                    .map(|(skin, image)| (skin.clone(), image.clone()))
                    .collect(),
            },
            span,
        );
    }
    if prop_bool(&block.props, "cameraBound", false) {
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

fn studio_position(id: &str, context: &CompileContext<'_>) -> Position {
    if let Some((left, top)) = context.positions.get(id) {
        return Position {
            x: Anchor::Left(keine_core::DESIGN_WIDTH * *left / 100.0),
            y: keine_core::DESIGN_HEIGHT * *top / 100.0,
        };
    }
    match id {
        "left" | "center-left" => Position::left(0.0),
        "right" | "center-right" => Position::right(0.0),
        _ => Position::center(0.0),
    }
}

fn compile_scene(
    block: &StoryBlock,
    context: &CompileContext<'_>,
    span: SourceSpan,
    report: &mut ParseReport,
) {
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
        .filter(|layer| !layer.asset_path.is_empty())
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
}

fn compile_branch(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let choices = json_string::<Vec<BranchChoice>>(&block.props, "choices").unwrap_or_default();
    report.push(
        Action::Menu {
            prompt: prop_string(&block.props, "title"),
            choices: choices
                .into_iter()
                .filter(|choice| !choice.fragment_id.is_empty())
                .map(|choice| Choice {
                    text: choice.text,
                    target: if choice.mode == "call" {
                        ChoiceTarget::CallScene(choice.fragment_id)
                    } else {
                        ChoiceTarget::ChangeScene(choice.fragment_id)
                    },
                    show_when: choice.visible_if,
                    enable_when: None,
                })
                .collect(),
        },
        span,
    );
}

fn compile_if(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let expression = prop_string_or(&block.props, "expression", "false");
    let then_scene = prop_string(&block.props, "thenFragmentId");
    let else_scene = prop_string(&block.props, "elseFragmentId");
    if !then_scene.is_empty() {
        report.push(
            Action::Flow {
                action: Box::new(Action::ChangeScene(then_scene)),
                when: Some(expression.clone()),
                next: false,
            },
            span,
        );
    }
    if !else_scene.is_empty() {
        report.push(
            Action::Flow {
                action: Box::new(Action::ChangeScene(else_scene)),
                when: Some(format!("!({expression})")),
                next: false,
            },
            span,
        );
    }
}

fn compile_set_variable(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let name = prop_string(&block.props, "key");
    if name.is_empty() {
        return;
    }
    let operand = studio_operand(block, "a");
    let operand = if prop_string(&block.props, "bKind") == "none" {
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
        "=" => operand,
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
    if kind == "BGM" {
        report.push(
            Action::Bgm {
                file,
                volume,
                fade_seconds: prop_f32(&block.props, "fadeDuration", 0.0) / 1000.0,
            },
            span,
        );
    } else if kind == "VOCAL" || kind == "VOICE" {
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
        report.push(
            Action::Effect {
                file: (!file.is_empty()).then_some(file),
                volume,
                id: (looped || !id.is_empty()).then(|| {
                    if id.is_empty() {
                        "letsgal-loop".into()
                    } else {
                        id
                    }
                }),
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
            Action::Effect {
                file: None,
                volume: 0.0,
                id: (!id.is_empty()).then_some(id),
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
    let shake = optional_f32(&block.props, "shakeAmplitude")
        .zip(optional_f32(&block.props, "shakeFrequency"))
        .zip(optional_f32(&block.props, "shakeDuration"));
    if let Some(((amplitude, frequency), duration_ms)) = shake {
        report.push(
            Action::ShakeCamera {
                targets,
                shake: CameraShakeSpec {
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
                },
                blocking: prop_bool(&block.props, "shakeWaitForComplete", false),
            },
            span,
        );
    }
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
    let mut transform = TransformPatch::default();
    transform.set_offset_x(0.0);
    transform.set_offset_y(0.0);
    transform.set_scale_x(1.0);
    transform.set_scale_y(1.0);
    let targets = CameraTargets::ALL;
    let mut timed = Vec::with_capacity(3);
    timed.push(Action::ShakeCamera {
        targets,
        shake: CameraShakeSpec {
            amplitude: 0.0,
            frequency: 0.0,
            duration: 0.0,
            axis: CameraShakeAxis::Both,
            falloff: CameraShakeFalloff::Linear,
        },
        blocking: false,
    });
    timed.push(Action::SetCameraTransform {
        targets,
        transform,
        duration,
        easing,
        blocking: wait,
    });
    let defaults = PostProcessEffect::default();
    timed.push(Action::SetPostProcess {
        targets,
        effect: Box::new(PostProcessPatch {
            focal_distance: Some(None),
            blur_strength: Some(defaults.blur_strength),
            distortion_strength: Some(defaults.distortion_strength),
            vignette_intensity: Some(defaults.vignette_intensity),
            vignette_size: Some(defaults.vignette_size),
            blur_amount: Some(defaults.blur_amount),
            color_tone: Some(defaults.color_tone),
            color_tone_intensity: Some(defaults.color_tone_intensity),
            color_exposure: Some(defaults.color_exposure),
            color_brightness: Some(defaults.color_brightness),
            color_contrast: Some(defaults.color_contrast),
            color_saturation: Some(defaults.color_saturation),
            color_temperature: Some(defaults.color_temperature),
            old_film_intensity: Some(defaults.old_film_intensity),
            shock_intensity: Some(defaults.shock_intensity),
            godray_intensity: Some(defaults.godray_intensity),
            godray_angle: Some(defaults.godray_angle),
            godray_gain: Some(defaults.godray_gain),
            godray_lacunarity: Some(defaults.godray_lacunarity),
            godray_speed: Some(defaults.godray_speed),
            godray_parallel: Some(defaults.godray_parallel),
            godray_center_x: Some(defaults.godray_center_x),
            godray_center_y: Some(defaults.godray_center_y),
            lut_preset: Some(None),
            lut_intensity: Some(defaults.lut_intensity),
            bloom_intensity: Some(defaults.bloom_intensity),
            chromatic_aberration: Some(defaults.chromatic_aberration),
            pixelate_size: Some(defaults.pixelate_size),
            glitch_intensity: Some(defaults.glitch_intensity),
            crt_intensity: Some(defaults.crt_intensity),
            sharpen_strength: Some(defaults.sharpen_strength),
            radial_blur_strength: Some(defaults.radial_blur_strength),
            radial_blur_center_x: Some(defaults.radial_blur_center_x),
            radial_blur_center_y: Some(defaults.radial_blur_center_y),
            motion_blur_strength: Some(defaults.motion_blur_strength),
            motion_blur_angle: Some(defaults.motion_blur_angle),
            zoom_blur_strength: Some(defaults.zoom_blur_strength),
            zoom_blur_center_x: Some(defaults.zoom_blur_center_x),
            zoom_blur_center_y: Some(defaults.zoom_blur_center_y),
            light_leak_intensity: Some(defaults.light_leak_intensity),
            light_leak_angle: Some(defaults.light_leak_angle),
            lens_flare_intensity: Some(defaults.lens_flare_intensity),
            lens_flare_center_x: Some(defaults.lens_flare_center_x),
            lens_flare_center_y: Some(defaults.lens_flare_center_y),
            film_grain_intensity: Some(defaults.film_grain_intensity),
            film_grain_size: Some(defaults.film_grain_size),
            heat_haze_intensity: Some(defaults.heat_haze_intensity),
            heat_haze_speed: Some(defaults.heat_haze_speed),
            heat_haze_scale: Some(defaults.heat_haze_scale),
            water_ripple_intensity: Some(defaults.water_ripple_intensity),
            water_ripple_frequency: Some(defaults.water_ripple_frequency),
            water_ripple_speed: Some(defaults.water_ripple_speed),
            water_ripple_center_x: Some(defaults.water_ripple_center_x),
            water_ripple_center_y: Some(defaults.water_ripple_center_y),
            fog_intensity: Some(defaults.fog_intensity),
            fog_speed: Some(defaults.fog_speed),
            fog_scale: Some(defaults.fog_scale),
            vhs_intensity: Some(defaults.vhs_intensity),
            vhs_jitter: Some(defaults.vhs_jitter),
            vhs_noise: Some(defaults.vhs_noise),
            halftone_intensity: Some(defaults.halftone_intensity),
            halftone_scale: Some(defaults.halftone_scale),
            halftone_angle: Some(defaults.halftone_angle),
            dither_intensity: Some(defaults.dither_intensity),
            dither_levels: Some(defaults.dither_levels),
            outline_intensity: Some(defaults.outline_intensity),
            outline_thickness: Some(defaults.outline_thickness),
            eyelid_openness: Some(defaults.eyelid_openness),
            eyelid_width: Some(defaults.eyelid_width),
            eyelid_curvature: Some(defaults.eyelid_curvature),
            eyelid_softness: Some(defaults.eyelid_softness),
            eyelid_center_x: Some(defaults.eyelid_center_x),
            eyelid_center_y: Some(defaults.eyelid_center_y),
        }),
        duration,
        easing,
        blocking: wait,
    });
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
}

fn compile_curtain(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    let closed = prop_string(&block.props, "op") == "close";
    if prop_string(&block.props, "mode") == "letterbox" {
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

fn compile_floating_text(block: &StoryBlock, span: SourceSpan, report: &mut ParseReport) {
    report.push(
        Action::FloatingText {
            text: plain_text(&block.content),
            position: parse_position(&prop_string(&block.props, "position")),
            font_size: prop_f32(&block.props, "fontSize", 50.0),
            color: parse_color(&prop_string_or(&block.props, "color", "#ffffff")),
            fade_in: prop_f32(&block.props, "inDuration", 0.0) / 1000.0,
            hold: prop_f32(&block.props, "duration", 0.0) / 1000.0,
            fade_out: prop_f32(&block.props, "outDuration", 0.0) / 1000.0,
            blocking: prop_bool(&block.props, "blocking", false),
        },
        span,
    );
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
    let options =
        json_string::<StudioParticleOverrides>(&block.props, "optionsJson").unwrap_or_default();
    let count = options.count.unwrap_or(0).min(u16::MAX as u32) as u16;
    report.push(
        Action::ShowParticles {
            id,
            effect: keine_core::ParticleEffect {
                texture: (!texture.is_empty()).then_some(texture),
                preset: prop_string_or(&block.props, "preset", "LIGHT_SNOW"),
                count,
                wind: options.wind,
                gravity: options.gravity,
                fade_in: prop_f32(&block.props, "fadeInDuration", 500.0).max(0.0) / 1000.0,
            },
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

    let mut tracks = clip
        .tracks
        .into_iter()
        .filter_map(|track| compile_stage_track(track, context))
        .collect::<Vec<_>>();
    for track in &mut tracks {
        track
            .keyframes
            .sort_by(|left, right| left.time.total_cmp(&right.time));
    }
    let mut events = clip
        .events
        .iter()
        .filter_map(|event| compile_stage_event(event, context))
        .collect::<Vec<_>>();
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
        "cameraShake" => StageEventKind::CameraShake(CameraShakeSpec {
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
        }),
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
            let options: StudioParticleOverrides = payload
                .get("options")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .or_else(|| {
                    payload
                        .get("optionsJson")
                        .and_then(Value::as_str)
                        .and_then(|value| serde_json::from_str(value).ok())
                })
                .unwrap_or_default();
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
        "out" | "easeout" | "ease-out" | "outquad" | "outcubic" | "outquart" | "outquint"
        | "outsine" | "outcirc" | "outexpo" => Easing::EaseOut,
        "inout" | "easeinout" | "ease-in-out" | "inoutquad" | "inoutcubic" | "inoutquart"
        | "inoutquint" | "inoutsine" | "inoutcirc" | "inoutexpo" => Easing::EaseInOut,
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
    let source = plain_text(value);
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
        "size" => style.size = Some(value.to_owned()),
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
    if value.parse::<f64>().is_ok() || matches!(value, "true" | "false") {
        value.to_owned()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studio_180_registry_is_exhaustively_matched() {
        assert_eq!(BUILTIN_BLOCK_TYPES.len(), 34);
        for required in [
            "playerInput",
            "enterAutoPlay",
            "callExtensionFunction",
            "stageAnimation",
            "video",
        ] {
            assert!(BUILTIN_BLOCK_TYPES.contains(&required));
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
            if *kind != "camera" {
                assert!(!report.actions.is_empty(), "{kind} did not emit runtime IR");
            }
            assert!(
                report
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.level != DiagnosticLevel::Error),
                "{kind} emitted an error: {:?}",
                report.diagnostics
            );
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
                            {"type":"particleCue","time":300,"data":{"id":"snow","preset":"LIGHT_SNOW","duration":600,"fadeOutDuration":100,"options":{"count":24,"wind":2,"gravity":3}}},
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
                && variants.len() == 2
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
            report.actions.first(),
            Some(Action::Flow { action, next: true, .. })
                if matches!(action.as_ref(), Action::ShakeCamera { shake, .. } if shake.duration == 0.0)
        ));
        assert!(matches!(
            report.actions.get(1),
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
        assert!(show_index >= 4, "scene appeared before the camera reset");
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
            &report.actions[0],
            Action::Flow { action, next: true, .. }
                if matches!(action.as_ref(), Action::HideSprites { prefix, .. } if prefix == "scene-layer:")
        ));
        assert!(matches!(
            &report.actions[1],
            Action::Flow { action, next: true, .. }
                if matches!(action.as_ref(), Action::HideBg { .. })
        ));
        assert!(matches!(
            &report.actions[2],
            Action::Flow { action, next: true, .. }
                if matches!(action.as_ref(), Action::ShowSprite {
                    id,
                    image,
                    layout: SpriteLayout::Scene(_),
                    ..
                } if id == "scene-layer:new-bg" && image == "new-bg.png")
        ));
        assert!(matches!(
            &report.actions[3],
            Action::SetCameraBinding { target, distance, .. }
                if target == "scene-layer:new-bg" && *distance == 1.0
        ));
        assert!(matches!(
            &report.actions[4],
            Action::Flow { action, next: true, .. }
                if matches!(action.as_ref(), Action::ShowSprite { id, .. } if id == "scene-layer:new-overlay")
        ));
        assert!(matches!(
            &report.actions[5],
            Action::SetCameraBinding { target, distance, .. }
                if target == "scene-layer:new-overlay" && *distance == 1.0
        ));
        assert!(matches!(
            report.actions[6],
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
        assert_eq!(config.bg_path("hash"), "backgrounds/sea.png");
        assert_eq!(config.bg_path("backgrounds/sea.png"), "backgrounds/sea.png");
        assert_eq!(config.layout.anchor_offset, 0.0);
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
}
