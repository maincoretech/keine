use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use keine_core::config::{
    AssetMap, AssetSourceConfig, GameConfig, ScriptConfig, ScriptInitialState,
};
use keine_core::{
    Action, Anchor, BlendMode, ChoiceTarget, Easing, Position, SpriteLayout, SpriteTransform,
    Transition, VideoMode,
};
use keine_loader::{
    ContentProject, DiagnosticLevel, LoadedScene, LoaderRegistry, ProjectInitialState, ResourceKind,
};
use serde::Serialize;

use crate::runtime::bootstrap::{open_project, validate_project};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct AssetKey {
    kind: ResourceKind,
    source_name: String,
}

#[derive(Default, Serialize)]
struct AssetManifest {
    backgrounds: BTreeMap<String, String>,
    figures: BTreeMap<String, String>,
    voices: BTreeMap<String, String>,
    bgm: BTreeMap<String, String>,
    #[serde(rename = "se")]
    effects: BTreeMap<String, String>,
    videos: BTreeMap<String, String>,
    particles: BTreeMap<String, String>,
    mini_avatars: BTreeMap<String, String>,
    luts: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct CharacterManifest {
    characters: BTreeMap<String, CharacterEntry>,
}

#[derive(Serialize)]
struct CharacterEntry {
    name: String,
}

struct MigrationModel {
    scene_ids: HashMap<String, String>,
    speaker_ids: HashMap<String, String>,
    asset_ids: HashMap<AssetKey, String>,
    assets: AssetManifest,
    characters: CharacterManifest,
}

pub(crate) fn run(source: &Path, target: &Path, loader: &LoaderRegistry) -> Result<()> {
    let source = source
        .canonicalize()
        .with_context(|| format!("failed to resolve source project {}", source.display()))?;
    let target = absolute_new_target(target)?;
    if target.exists() {
        bail!("migration target already exists: {}", target.display());
    }
    if target.starts_with(&source) {
        bail!("migration target must not be inside the source project");
    }

    let opened = open_project(&source, loader)?;
    if opened.packaged {
        bail!("packaged projects cannot be migration sources");
    }
    if opened.config.adapter.script.eq_ignore_ascii_case("keine")
        && opened.content.project_adapter().is_none()
    {
        bail!("source project already uses Eiyashou");
    }
    let languages = loader
        .languages(&opened.config.adapter.script)
        .context("failed to select source script adapter")?;
    let mut scenes = keine_loader::load_scenes_with(&opened.content, &languages)
        .context("failed to compile source project")?;
    scenes.sort_by(|left, right| left.name.cmp(&right.name));
    fail_on_source_errors(&scenes)?;
    if scenes.is_empty() {
        bail!("source project contains no scenes");
    }
    let initial_state = opened.content.initial_state()?;

    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        bail!(
            "migration target parent does not exist: {}",
            parent.display()
        );
    }
    let mut staging = tempfile::Builder::new()
        .prefix(".keine-migrate-")
        .tempdir_in(parent)
        .context("failed to create migration staging directory")?;
    let ordinary =
        build_model(&opened.config, &opened.content, &scenes, staging.path()).and_then(|model| {
            write_project(
                staging.path(),
                opened.config.clone(),
                &scenes,
                &initial_state,
                &model,
                loader,
            )
        });
    if ordinary.is_err() {
        // The compact v1 spelling covers common actions. Advanced typed actions
        // preserve the full engine IR when one of those spellings would lose data.
        drop(staging);
        staging = tempfile::Builder::new()
            .prefix(".keine-migrate-")
            .tempdir_in(parent)
            .context("failed to create migration staging directory")?;
        write_typed_project(
            staging.path(),
            &opened.config,
            &opened.content,
            &scenes,
            &initial_state,
            loader,
        )?;
    }

    let staged_path = staging.keep();
    if let Err(error) = fs::rename(&staged_path, &target) {
        let _ = fs::remove_dir_all(&staged_path);
        return Err(error).with_context(|| {
            format!("failed to install migrated project at {}", target.display())
        });
    }
    println!(
        "migration complete · {} -> {} · {} scene(s)",
        source.display(),
        target.display(),
        scenes.len()
    );
    Ok(())
}

fn absolute_new_target(target: &Path) -> Result<PathBuf> {
    if target.as_os_str().is_empty() {
        bail!("migration target must not be empty");
    }
    let absolute = if target.is_absolute() {
        target.to_owned()
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory")?
            .join(target)
    };
    let name = absolute
        .file_name()
        .context("migration target must name a project directory")?;
    let parent = absolute.parent().unwrap_or_else(|| Path::new("."));
    let parent = parent
        .canonicalize()
        .with_context(|| format!("failed to resolve target parent {}", parent.display()))?;
    Ok(parent.join(name))
}

fn fail_on_source_errors(scenes: &[LoadedScene]) -> Result<()> {
    let errors = scenes
        .iter()
        .flat_map(|scene| {
            scene
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.level == DiagnosticLevel::Error)
                .map(move |diagnostic| {
                    format!(
                        "{}:{}:{}: {}",
                        scene.path.display(),
                        diagnostic.span.line,
                        diagnostic.span.column,
                        diagnostic.message
                    )
                })
        })
        .collect::<Vec<_>>();
    if errors.is_empty() {
        return Ok(());
    }
    bail!(
        "source project has {} error(s):\n{}",
        errors.len(),
        errors.join("\n")
    )
}

fn build_model(
    config: &GameConfig,
    content: &ContentProject,
    scenes: &[LoadedScene],
    stage: &Path,
) -> Result<MigrationModel> {
    let scene_ids = scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| (scene.name.clone(), format!("scene_{:04}", index + 1)))
        .collect::<HashMap<_, _>>();

    let mut speakers = scenes
        .iter()
        .flat_map(|scene| scene.actions.iter())
        .filter_map(|action| match action {
            Action::Say { speaker, .. } if !speaker.is_empty() => Some(speaker.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    speakers.sort();
    speakers.dedup();
    let speaker_ids = speakers
        .iter()
        .enumerate()
        .map(|(index, name)| (name.clone(), format!("speaker_{:04}", index + 1)))
        .collect::<HashMap<_, _>>();
    let characters = CharacterManifest {
        characters: speakers
            .into_iter()
            .map(|name| {
                let id = speaker_ids[&name].clone();
                (id, CharacterEntry { name })
            })
            .collect(),
    };

    let mut keys = scenes
        .iter()
        .flat_map(|scene| scene.resources.iter())
        .map(|resource| {
            if resource.is_dynamic() {
                bail!(
                    "dynamic resource reference cannot be migrated losslessly: {}",
                    resource.path
                );
            }
            Ok(AssetKey {
                kind: resource.kind,
                source_name: resource.path.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let title_key = AssetKey {
        kind: ResourceKind::Background,
        source_name: config.title_background.clone(),
    };
    let title_path = config.bg_path(&config.title_background);
    if !config.title_background.is_empty() && content.contains_asset(Path::new(&title_path)) {
        keys.push(title_key);
    }
    keys.sort_by(|left, right| {
        resource_order(left.kind)
            .cmp(&resource_order(right.kind))
            .then_with(|| left.source_name.cmp(&right.source_name))
    });
    keys.dedup();

    let mut assets = AssetManifest::default();
    let mut asset_ids = HashMap::new();
    for (index, key) in keys.into_iter().enumerate() {
        let resolved = resolve_resource(config, key.kind, &key.source_name)?;
        let bytes = content
            .read_asset(Path::new(&resolved))
            .with_context(|| format!("failed to read source asset {resolved}"))?;
        let id = format!("asset_{:05}", index + 1);
        let extension = Path::new(&resolved)
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .map(|value| format!(".{value}"))
            .unwrap_or_default();
        let namespace = resource_namespace(key.kind)?;
        let relative = format!("assets/migrated/{namespace}/{id}{extension}");
        let destination = stage.join(&relative);
        fs::create_dir_all(destination.parent().expect("asset path has parent"))?;
        fs::write(&destination, bytes)
            .with_context(|| format!("failed to write {}", destination.display()))?;
        manifest_namespace_mut(&mut assets, key.kind)?.insert(id.clone(), relative);
        asset_ids.insert(key, id);
    }
    Ok(MigrationModel {
        scene_ids,
        speaker_ids,
        asset_ids,
        assets,
        characters,
    })
}

fn write_project(
    stage: &Path,
    mut config: GameConfig,
    scenes: &[LoadedScene],
    initial_state: &ProjectInitialState,
    model: &MigrationModel,
    loader: &LoaderRegistry,
) -> Result<()> {
    fs::create_dir_all(stage.join("scripts"))?;
    let source = render_scenes(scenes, model)?;
    fs::write(stage.join("scripts/main.shou"), source)?;
    fs::write(
        stage.join("assets.yaml"),
        noyalib::to_string(&model.assets)?,
    )?;
    fs::write(
        stage.join("characters.yaml"),
        noyalib::to_string(&model.characters)?,
    )?;

    config.adapter.asset = vec![AssetSourceConfig::default()];
    config.adapter.script = "keine".into();
    config.assets = AssetMap::default();
    let source_entry = config.script.entry.clone();
    config.script = ScriptConfig {
        version: 1,
        entry: model
            .scene_ids
            .get(&source_entry)
            .cloned()
            .unwrap_or_else(|| model.scene_ids[&scenes[0].name].clone()),
        assets: "assets.yaml".into(),
        characters: "characters.yaml".into(),
        initial_state: native_initial_state(initial_state),
    };
    let title = AssetKey {
        kind: ResourceKind::Background,
        source_name: config.title_background.clone(),
    };
    if let Some(id) = model.asset_ids.get(&title) {
        config.title_background = id.clone();
    }
    fs::write(stage.join("config.yaml"), noyalib::to_string(&config)?)?;

    validate_migrated_project(stage, loader)?;
    Ok(())
}

fn validate_migrated_project(stage: &Path, loader: &LoaderRegistry) -> Result<()> {
    let migrated = open_project(stage, loader).context("failed to reopen migrated project")?;
    let languages = loader.languages(&migrated.config.adapter.script)?;
    let report = validate_project(&migrated.config, &migrated.content, &languages)?;
    if report.errors > 0 {
        let details = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.level == keine_authoring::DiagnosticLevel::Error)
            .map(|diagnostic| {
                format!(
                    "{}:{}:{}: {}",
                    diagnostic.path.display(),
                    diagnostic.line,
                    diagnostic.column,
                    diagnostic.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        bail!("migrated project failed validation:\n{details}");
    }
    Ok(())
}

fn write_typed_project(
    stage: &Path,
    source_config: &GameConfig,
    content: &ContentProject,
    scenes: &[LoadedScene],
    initial_state: &ProjectInitialState,
    loader: &LoaderRegistry,
) -> Result<()> {
    let scene_ids = scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| (scene.name.clone(), format!("scene_{:04}", index + 1)))
        .collect::<HashMap<_, _>>();
    let mut source = String::new();
    let mut expected = HashMap::new();
    for scene in scenes {
        let name = &scene_ids[&scene.name];
        source.push_str(&format!("scene {name} {{\n"));
        let mut actions = scene.actions.clone();
        for action in &mut actions {
            remap_action_scenes(action, &scene_ids)?;
            source.push_str("  engine {\"ron\":");
            source.push_str(&serde_json::to_string(&ron::to_string(action)?)?);
            source.push_str("},\n");
        }
        if !actions.is_empty() {
            source.truncate(source.len() - 2);
            source.push('\n');
        }
        source.push_str("}\n\n");
        expected.insert(name.clone(), actions);
    }

    let assets = copy_typed_assets(stage, source_config, content, scenes)?;
    fs::create_dir_all(stage.join("scripts"))?;
    fs::write(stage.join("scripts/main.shou"), source)?;
    fs::write(stage.join("assets.yaml"), noyalib::to_string(&assets)?)?;
    fs::write(
        stage.join("characters.yaml"),
        noyalib::to_string(&CharacterManifest {
            characters: BTreeMap::new(),
        })?,
    )?;

    let mut config = source_config.clone();
    config.adapter.asset = vec![AssetSourceConfig::default()];
    config.adapter.script = "keine".into();
    config.script = ScriptConfig {
        version: 1,
        entry: scene_ids
            .get(&source_config.script.entry)
            .cloned()
            .unwrap_or_else(|| scene_ids[&scenes[0].name].clone()),
        assets: "assets.yaml".into(),
        characters: "characters.yaml".into(),
        initial_state: native_initial_state(initial_state),
    };
    fs::write(stage.join("config.yaml"), noyalib::to_string(&config)?)?;
    validate_migrated_project(stage, loader)?;

    let migrated = open_project(stage, loader)?;
    if migrated.content.initial_state()? != *initial_state {
        bail!("migrated project changed initial variables");
    }
    let languages = loader.languages(&migrated.config.adapter.script)?;
    let actual = keine_loader::load_scenes_with(&migrated.content, &languages)?;
    for scene in actual {
        let name = scene.name;
        let expected_actions = expected
            .remove(&name)
            .with_context(|| format!("unexpected migrated scene {name:?}"))?;
        if scene.actions != expected_actions {
            let first = scene
                .actions
                .iter()
                .zip(&expected_actions)
                .position(|(actual, expected)| actual != expected);
            bail!(
                "migrated scene {name:?} changed typed engine actions at {first:?} ({} actual, {} expected): {:?} -> {:?}",
                scene.actions.len(),
                expected_actions.len(),
                first.and_then(|index| expected_actions.get(index)),
                first.and_then(|index| scene.actions.get(index)),
            );
        }
    }
    if !expected.is_empty() {
        bail!("migrated project is missing typed engine scenes");
    }
    Ok(())
}

fn native_initial_state(initial: &ProjectInitialState) -> ScriptInitialState {
    ScriptInitialState {
        variables: initial.variables.clone(),
        session_variables: initial.session_variables.clone(),
        shared_variables: initial.shared_variables.clone(),
    }
}

fn remap_action_scenes(action: &mut Action, scene_ids: &HashMap<String, String>) -> Result<()> {
    match action {
        Action::ChangeScene(scene) | Action::CallScene(scene) => {
            *scene = scene_ids
                .get(scene)
                .cloned()
                .with_context(|| format!("unknown scene {scene:?}"))?;
        }
        Action::Menu { choices, .. } => {
            for choice in choices {
                remap_choice_scene(&mut choice.target, scene_ids)?;
            }
        }
        Action::EiyashouMenu { choices, .. } => {
            for choice in choices {
                remap_choice_scene(&mut choice.target, scene_ids)?;
            }
        }
        Action::Flow { action, .. } => remap_action_scenes(action, scene_ids)?,
        _ => {}
    }
    Ok(())
}

fn remap_choice_scene(
    target: &mut ChoiceTarget,
    scene_ids: &HashMap<String, String>,
) -> Result<()> {
    if let ChoiceTarget::ChangeScene(scene) | ChoiceTarget::CallScene(scene) = target {
        *scene = scene_ids
            .get(scene)
            .cloned()
            .with_context(|| format!("unknown scene {scene:?}"))?;
    }
    Ok(())
}

fn copy_typed_assets(
    stage: &Path,
    config: &GameConfig,
    content: &ContentProject,
    scenes: &[LoadedScene],
) -> Result<AssetManifest> {
    let mut paths = BTreeSet::new();
    let mut manifest = AssetManifest::default();
    for scene in scenes {
        for resource in &scene.resources {
            if resource.is_dynamic() {
                bail!(
                    "dynamic resource reference cannot be migrated losslessly: {}",
                    resource.path
                );
            }
            let logical = resource.resolved_path(config);
            manifest_namespace_mut(&mut manifest, resource.kind)?
                .insert(resource.path.clone(), format!("assets/{logical}"));
            paths.insert(logical);
        }
    }
    for aliases in [
        &config.assets.backgrounds,
        &config.assets.figures,
        &config.assets.voices,
        &config.assets.bgm,
        &config.assets.effects,
        &config.assets.videos,
        &config.assets.luts,
    ] {
        paths.extend(aliases.values().cloned());
    }
    if !config.title_background.is_empty() {
        let title = config.bg_path(&config.title_background);
        if content.contains_asset(Path::new(&title)) {
            paths.insert(title);
        }
    }
    for logical in paths {
        let path = Path::new(&logical);
        if path.as_os_str().is_empty()
            || !path
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
        {
            bail!("asset path is not project-relative: {logical}");
        }
        let bytes = content
            .read_asset(path)
            .with_context(|| format!("failed to read source asset {logical}"))?;
        let destination = stage.join("assets").join(path);
        fs::create_dir_all(destination.parent().expect("asset path has parent"))?;
        fs::write(&destination, bytes)?;
    }
    Ok(manifest)
}

fn render_scenes(scenes: &[LoadedScene], model: &MigrationModel) -> Result<String> {
    let mut output = String::new();
    for (scene_index, scene) in scenes.iter().enumerate() {
        if scene_index > 0 {
            output.push('\n');
        }
        output.push_str("scene ");
        output.push_str(&model.scene_ids[&scene.name]);
        output.push_str(" {\n");
        let mut statements = Vec::new();
        for (action_index, action) in scene.actions.iter().enumerate() {
            if matches!(action, Action::End) && action_index + 1 == scene.actions.len() {
                continue;
            }
            statements.push(render_action(action, model).with_context(|| {
                format!(
                    "unsupported action in scene {:?} at index {}",
                    scene.name, action_index
                )
            })?);
        }
        for (index, statement) in statements.iter().enumerate() {
            output.push_str("  ");
            output.push_str(statement);
            if index + 1 < statements.len() {
                output.push(',');
            }
            output.push('\n');
        }
        output.push_str("}\n");
    }
    Ok(output)
}

fn render_action(action: &Action, model: &MigrationModel) -> Result<String> {
    match action {
        Action::ShowBg {
            image,
            transition,
            transform,
        } if *transform == SpriteTransform::default() => Ok(format!(
            "background({}{})",
            asset_id(model, ResourceKind::Background, image)?,
            transition_arg(*transition)
        )),
        Action::HideBg { transition } => {
            Ok(format!("background(none{})", transition_arg(*transition)))
        }
        Action::ShowSprite {
            id,
            image,
            position,
            layout,
            transition,
            transform,
            z_index,
            blend,
        } if *layout == SpriteLayout::Natural
            && *transform == SpriteTransform::default()
            && *blend == BlendMode::Alpha =>
        {
            Ok(format!(
                "sprite({}, {}, position: {}, z: {}{})",
                identifier(id)?,
                asset_id(model, ResourceKind::Figure, image)?,
                position_name(*position)?,
                z_index,
                transition_arg(*transition)
            ))
        }
        Action::HideSprite { id, transition } => Ok(format!(
            "hide({}{})",
            identifier(id)?,
            transition_arg(*transition)
        )),
        Action::HideSprites { prefix, transition } if prefix.is_empty() => {
            Ok(format!("hide(\"*\"{})", transition_arg(*transition)))
        }
        Action::MoveSprite {
            id,
            position,
            duration: seconds,
            easing,
            blocking: true,
        } => Ok(format!(
            "move({}, {}, duration: {}, easing: {})",
            identifier(id)?,
            position_name(*position)?,
            duration(*seconds),
            easing_name(*easing)
        )),
        Action::Say {
            speaker,
            text,
            options,
        } if options.volume == 1.0
            && !options.concat
            && !options.auto_advance
            && !options.inherit_speaker =>
        {
            let text = string_literal(text);
            let voice = options
                .vocal
                .as_ref()
                .map(|voice| {
                    asset_id(model, ResourceKind::Voice, voice).map(|id| format!(" voice({id})"))
                })
                .transpose()?
                .unwrap_or_default();
            if speaker.is_empty() {
                Ok(format!("{text}{voice}"))
            } else {
                Ok(format!("{}: {text}{voice}", model.speaker_ids[speaker]))
            }
        }
        Action::Menu { prompt, choices } => render_menu(prompt, choices, model),
        Action::ChangeScene(scene) => Ok(format!("goto({})", scene_id(model, scene)?)),
        Action::CallScene(scene) => Ok(format!("call({})", scene_id(model, scene)?)),
        Action::ReturnScene => Ok("return".into()),
        Action::Bgm {
            file,
            volume,
            fade_seconds,
        } => Ok(format!(
            "bgm({}, volume: {}, fade: {})",
            asset_id(model, ResourceKind::Bgm, file)?,
            number(*volume),
            duration(*fade_seconds)
        )),
        Action::Effect { file, volume, id } if id.is_none() => Ok(format!(
            "se({}, volume: {})",
            file.as_ref()
                .map(|file| asset_id(model, ResourceKind::Effect, file))
                .transpose()?
                .unwrap_or_else(|| "none".into()),
            number(*volume)
        )),
        Action::Wait { seconds } if *seconds > 0.0 => Ok(format!("wait({})", duration(*seconds))),
        Action::PlayVideo { video }
            if !video.looped
                && !video.muted
                && video.alpha == 1.0
                && video.wait_for_finished
                && video.mode == VideoMode::Fullscreen =>
        {
            Ok(format!(
                "video({}, skippable: {})",
                asset_id(model, ResourceKind::Video, &video.file)?,
                video.skippable
            ))
        }
        Action::Flow {
            action,
            when: None,
            next: false,
        } => render_action(action, model),
        Action::End => bail!("non-final End cannot be represented losslessly"),
        other => bail!("{other:?}"),
    }
}

fn render_menu(
    prompt: &str,
    choices: &[keine_core::action::Choice],
    model: &MigrationModel,
) -> Result<String> {
    if choices.is_empty() {
        bail!("empty menu cannot be represented");
    }
    let mut output = if prompt.is_empty() {
        "choice {\n".to_owned()
    } else {
        format!("choice({}) {{\n", string_literal(prompt))
    };
    for (index, choice) in choices.iter().enumerate() {
        if choice.show_when.is_some() || choice.enable_when.is_some() {
            bail!("conditional legacy choices require manual migration");
        }
        let target = match &choice.target {
            ChoiceTarget::ChangeScene(scene) => format!("goto({})", scene_id(model, scene)?),
            ChoiceTarget::CallScene(scene) => format!("call({})", scene_id(model, scene)?),
            ChoiceTarget::Label(_) => bail!("label choices require manual migration"),
        };
        output.push_str("    ");
        output.push_str(&string_literal(&choice.text));
        output.push_str(": ");
        output.push_str(&target);
        if index + 1 < choices.len() {
            output.push(',');
        }
        output.push('\n');
    }
    output.push_str("  }");
    Ok(output)
}

fn asset_id(model: &MigrationModel, kind: ResourceKind, name: &str) -> Result<String> {
    model
        .asset_ids
        .get(&AssetKey {
            kind,
            source_name: name.to_owned(),
        })
        .cloned()
        .with_context(|| format!("missing migrated asset mapping for {kind:?} {name:?}"))
}

fn scene_id<'a>(model: &'a MigrationModel, name: &str) -> Result<&'a str> {
    model
        .scene_ids
        .get(name)
        .map(String::as_str)
        .with_context(|| format!("missing migrated scene mapping for {name:?}"))
}

fn identifier(value: &str) -> Result<&str> {
    let valid = !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !value.as_bytes()[0].is_ascii_digit();
    if valid {
        Ok(value)
    } else {
        bail!("identifier {value:?} requires manual migration")
    }
}

fn string_literal(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character => output.push(character),
        }
    }
    output = output.replace("${", "/${");
    output.push('"');
    output
}

fn transition_arg(transition: Transition) -> String {
    let (name, seconds) = match transition {
        Transition::Instant => return String::new(),
        Transition::Fade(value) => ("fade", value),
        Transition::SlideFromLeft(value) => ("slide_from_left", value),
        Transition::SlideFromRight(value) => ("slide_from_right", value),
        Transition::Crossfade(value) => ("crossfade", value),
        Transition::Wipe(value) => ("wipe", value),
        Transition::Dissolve(value) => ("dissolve", value),
    };
    format!(", transition: {name}({})", duration(seconds))
}

fn position_name(position: Position) -> Result<&'static str> {
    if position.y != 0.0 {
        bail!("non-zero sprite y position requires manual migration");
    }
    match position.x {
        Anchor::Left(0.0) => Ok("left"),
        Anchor::Center(0.0) => Ok("center"),
        Anchor::Right(0.0) => Ok("right"),
        _ => bail!("offset sprite position requires manual migration"),
    }
}

fn easing_name(easing: Easing) -> &'static str {
    match easing {
        Easing::Linear => "linear",
        Easing::EaseIn => "ease_in",
        Easing::EaseOut => "ease_out",
        Easing::EaseInOut => "ease_in_out",
        Easing::InOutQuad => "in_out_quad",
        Easing::OutCubic => "out_cubic",
        Easing::InOutCubic => "in_out_cubic",
        Easing::OutBack => "out_back",
        Easing::OutBounce => "out_bounce",
    }
}

fn duration(seconds: f32) -> String {
    let milliseconds = seconds * 1000.0;
    if (milliseconds - milliseconds.round()).abs() < 0.0001 {
        format!("{}ms", milliseconds.round() as i64)
    } else {
        format!("{}s", number(seconds))
    }
}

fn number(value: f32) -> String {
    let mut output = format!("{value:.6}");
    while output.contains('.') && output.ends_with('0') {
        output.pop();
    }
    if output.ends_with('.') {
        output.push('0');
    }
    output
}

fn resolve_resource(config: &GameConfig, kind: ResourceKind, name: &str) -> Result<String> {
    Ok(match kind {
        ResourceKind::Background => config.bg_path(name),
        ResourceKind::Figure => config.figure_path(name),
        ResourceKind::Voice => config.voice_path(name),
        ResourceKind::Bgm => config.bgm_path(name),
        ResourceKind::Effect => config.effect_path(name),
        ResourceKind::Video => config.video_path(name),
        ResourceKind::Particle | ResourceKind::MiniAvatar | ResourceKind::Lut => {
            bail!("{kind:?} resources require typed migration")
        }
    })
}

fn resource_order(kind: ResourceKind) -> u8 {
    match kind {
        ResourceKind::Background => 0,
        ResourceKind::Figure => 1,
        ResourceKind::Voice => 2,
        ResourceKind::Bgm => 3,
        ResourceKind::Effect => 4,
        ResourceKind::Video => 5,
        ResourceKind::Particle => 6,
        ResourceKind::MiniAvatar => 7,
        ResourceKind::Lut => 8,
    }
}

fn resource_namespace(kind: ResourceKind) -> Result<&'static str> {
    Ok(match kind {
        ResourceKind::Background => "backgrounds",
        ResourceKind::Figure => "figures",
        ResourceKind::Voice => "voices",
        ResourceKind::Bgm => "bgm",
        ResourceKind::Effect => "se",
        ResourceKind::Video => "videos",
        ResourceKind::Particle | ResourceKind::MiniAvatar | ResourceKind::Lut => {
            bail!("{kind:?} resources require manual migration")
        }
    })
}

fn manifest_namespace_mut(
    manifest: &mut AssetManifest,
    kind: ResourceKind,
) -> Result<&mut BTreeMap<String, String>> {
    Ok(match kind {
        ResourceKind::Background => &mut manifest.backgrounds,
        ResourceKind::Figure => &mut manifest.figures,
        ResourceKind::Voice => &mut manifest.voices,
        ResourceKind::Bgm => &mut manifest.bgm,
        ResourceKind::Effect => &mut manifest.effects,
        ResourceKind::Video => &mut manifest.videos,
        ResourceKind::Particle => &mut manifest.particles,
        ResourceKind::MiniAvatar => &mut manifest.mini_avatars,
        ResourceKind::Lut => &mut manifest.luts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_interpolation_is_disabled_without_changing_visible_text() {
        assert_eq!(
            string_literal("cost ${value} /${raw}"),
            "\"cost /${value} //${raw}\""
        );
    }

    #[test]
    fn durations_prefer_exact_milliseconds() {
        assert_eq!(duration(0.3), "300ms");
        assert_eq!(duration(1.25), "1250ms");
    }

    #[test]
    fn migrates_a_compatibility_project_without_copying_source_text() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("legacy");
        let target = root.path().join("native");
        fs::create_dir_all(source.join("scripts")).unwrap();
        fs::write(
            source.join("config.yaml"),
            "title: Legacy\nadapter:\n  script: webgal\n  asset:\n    - path: .\n      format: fs\n",
        )
        .unwrap();
        fs::write(source.join("scripts/start.txt"), "Alice:Hello;").unwrap();

        run(&source, &target, &LoaderRegistry::default()).unwrap();

        assert!(target.join("scripts/main.shou").is_file());
        assert!(!target.join("scripts/start.txt").exists());
        let config = fs::read_to_string(target.join("config.yaml")).unwrap();
        assert!(config.contains("script: keine"));
        let native = fs::read_to_string(target.join("scripts/main.shou")).unwrap();
        assert!(native.contains("speaker_0001: \"Hello\""));
        assert!(!native.contains("Alice:Hello;"));
    }

    #[test]
    fn migrates_complex_letsgal_fixture_with_typed_actions() {
        let root = tempfile::tempdir().unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/letsgal-timeline");
        let target = root.path().join("native");

        run(&source, &target, &LoaderRegistry::default()).unwrap();

        let native = fs::read_to_string(target.join("scripts/main.shou")).unwrap();
        assert!(native.contains("StageAnimation"));
        assert!(native.contains("engine {\"ron\":"));
        assert!(!target.join("project.json").exists());
    }

    #[test]
    fn existing_target_is_never_modified() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("legacy");
        let target = root.path().join("native");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.txt"), "untouched").unwrap();

        let error = run(&source, &target, &LoaderRegistry::default()).unwrap_err();

        assert!(error.to_string().contains("already exists"));
        assert_eq!(
            fs::read_to_string(target.join("keep.txt")).unwrap(),
            "untouched"
        );
    }
}
