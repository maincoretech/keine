use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use keine_core::config::{AssetMap, AssetSourceConfig, GameConfig, ScriptConfig};
use keine_core::{
    Action, Anchor, ChoiceTarget, Easing, Position, SpriteLayout, SpriteTransform, Transition,
};
use keine_loader::{ContentProject, DiagnosticLevel, LoadedScene, LoaderRegistry, ResourceKind};
use serde::Serialize;

#[path = "project/objects.rs"]
mod objects;
#[path = "project/v11.rs"]
mod v11;

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
    object_ids: BTreeMap<String, String>,
    prefix_ids: BTreeMap<String, String>,
    objects: objects::ObjectManifest,
    variable_ids: BTreeMap<String, String>,
    initial_variables: BTreeMap<String, keine_core::Value>,
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
    let staging = tempfile::Builder::new()
        .prefix(".keine-migrate-")
        .tempdir_in(parent)
        .context("failed to create migration staging directory")?;
    let model = build_model(&opened.config, &opened.content, &scenes, staging.path())?;
    write_project(staging.path(), opened.config, &scenes, &model, loader)?;

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
    let mut speaker_ids = speakers
        .iter()
        .enumerate()
        .map(|(index, name)| (name.clone(), format!("speaker_{:04}", index + 1)))
        .collect::<HashMap<_, _>>();
    let mut characters = CharacterManifest {
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
    let initial = content.initial_state()?;
    if !initial.session_variables.is_empty() || !initial.shared_variables.is_empty() {
        bail!("persistent compatibility variables require manual migration");
    }
    let initial_variables = initial.variables.into_iter().collect::<BTreeMap<_, _>>();
    let mut variable_names = initial_variables.keys().cloned().collect::<Vec<_>>();
    for scene in scenes {
        for action in &scene.actions {
            if let Action::Set {
                name,
                global: false,
                ..
            } = action
            {
                variable_names.push(name.clone());
            }
        }
    }
    variable_names.sort();
    variable_names.dedup();
    let variable_ids = variable_names
        .into_iter()
        .enumerate()
        .map(|(index, name)| (name, format!("variable_{:04}", index + 1)))
        .collect();
    let (object_ids, prefix_ids, objects) = objects::build(scenes);
    if scenes
        .iter()
        .flat_map(|scene| &scene.actions)
        .any(has_portrait_rule)
    {
        let mut portraits = BTreeMap::new();
        for scene in scenes {
            for pair in scene.actions.windows(2) {
                if let [
                    Action::FocusPortrait {
                        speaker_id: Some(id),
                    },
                    Action::Say { speaker, .. },
                ] = pair
                    && !speaker.is_empty()
                {
                    let id = object_ids
                        .get(id)
                        .context("missing dialogue portrait mapping")?;
                    if let Some(previous) = portraits.insert(speaker.clone(), id.clone())
                        && previous != *id
                    {
                        bail!(
                            "speaker {speaker:?} uses multiple portrait identities; give them distinct character names before migration"
                        );
                    }
                }
            }
        }
        speaker_ids.extend(portraits);
        if speaker_ids
            .values()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != speaker_ids.len()
        {
            bail!(
                "multiple speaker names use one portrait identity; unify the character name before migration"
            );
        }
        characters.characters = speaker_ids
            .iter()
            .map(|(name, id)| (id.clone(), CharacterEntry { name: name.clone() }))
            .collect();
    }
    Ok(MigrationModel {
        scene_ids,
        object_ids,
        prefix_ids,
        objects,
        variable_ids,
        initial_variables,
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
    model: &MigrationModel,
    loader: &LoaderRegistry,
) -> Result<()> {
    fs::create_dir_all(stage.join("scripts"))?;
    let mut source = render_scenes(scenes, model)?;
    if !model.initial_variables.is_empty() {
        let source_entry = model
            .scene_ids
            .get(&config.script.entry)
            .unwrap_or(&model.scene_ids[&scenes[0].name]);
        source.push_str("\nscene migration_start {\n");
        for (name, value) in &model.initial_variables {
            source.push_str(&format!(
                "  // Original variable: {}\n  let {} = {},\n",
                name.replace(['\n', '\r'], " "),
                model.variable_ids[name],
                v11::value(value)?
            ));
        }
        source.push_str(&format!("  goto({source_entry})\n}}\n"));
    }
    let source = keine_loader::format_native_source(&source)
        .context("generated migration source could not be formatted")?;
    fs::write(stage.join("scripts/main.shou"), source)?;
    fs::write(
        stage.join("objects.yaml"),
        noyalib::to_string(&model.objects)?,
    )?;
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
    let entry = model
        .scene_ids
        .get(&source_entry)
        .cloned()
        .unwrap_or_else(|| model.scene_ids[&scenes[0].name].clone());
    config.script = ScriptConfig {
        version: ScriptConfig::default().version,
        entry: if model.initial_variables.is_empty() {
            entry.clone()
        } else {
            "migration_start".into()
        },
        assets: "assets.yaml".into(),
        characters: "characters.yaml".into(),
        objects: "objects.yaml".into(),
    };
    let title = AssetKey {
        kind: ResourceKind::Background,
        source_name: config.title_background.clone(),
    };
    if let Some(id) = model.asset_ids.get(&title) {
        config.title_background = id.clone();
    }
    fs::write(stage.join("config.yaml"), noyalib::to_string(&config)?)?;

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

fn render_scenes(scenes: &[LoadedScene], model: &MigrationModel) -> Result<String> {
    // Without a rule, FocusPortrait is a runtime no-op. Scan the whole project
    // because a configured rule persists across scene changes.
    let uses_focus = scenes
        .iter()
        .flat_map(|scene| &scene.actions)
        .any(has_portrait_rule);
    let mut output = String::new();
    for (scene_index, scene) in scenes.iter().enumerate() {
        if scene_index > 0 {
            output.push('\n');
        }
        output.push_str("// Original scene: ");
        output.push_str(&scene.name.replace(['\n', '\r'], " "));
        output.push('\n');
        output.push_str("scene ");
        output.push_str(&model.scene_ids[&scene.name]);
        output.push_str(" {\n");
        let mut statements = Vec::new();
        let mut action_index = 0;
        while action_index < scene.actions.len() {
            if let Some(reset) = v11::render_camera_reset(&scene.actions[action_index..], model)? {
                statements.push(reset);
                action_index += 4;
                continue;
            }
            let action = &scene.actions[action_index];
            action_index += 1;
            if matches!(action, Action::Comment)
                || (matches!(action, Action::FocusPortrait { .. })
                    && (!uses_focus
                        || matches!(scene.actions.get(action_index), Some(Action::Say { .. }))))
            {
                continue;
            }
            statements.push(render_action(action, model).with_context(|| {
                format!(
                    "unsupported action in scene {:?} at index {}",
                    scene.name,
                    action_index - 1
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

fn has_portrait_rule(action: &Action) -> bool {
    match action {
        Action::ConfigurePortraits { .. } => true,
        Action::Flow { action, .. } => has_portrait_rule(action),
        _ => false,
    }
}

fn render_action(action: &Action, model: &MigrationModel) -> Result<String> {
    if let Some(source) = v11::render(action, model)? {
        v11::verify(action, &source, model)?;
        return Ok(source);
    }
    match action {
        Action::HideBg { transition } => {
            Ok(format!("background(none{})", transition_arg(*transition)))
        }
        Action::HideSprite { id, transition } => Ok(format!(
            "hide({}{})",
            object_id(model, id)?,
            transition_arg(*transition)
        )),
        Action::Say {
            speaker,
            text,
            options,
        } => {
            let text = string_literal(text);
            let mut voice = options
                .vocal
                .as_ref()
                .map(|voice| {
                    asset_id(model, ResourceKind::Voice, voice).map(|id| format!(", {id}"))
                })
                .transpose()?
                .unwrap_or_default();
            if options.volume != 1.0 {
                voice.push_str(&format!(", volume: {}", number(options.volume)));
            }
            if options.concat {
                voice.push_str(", concat: true");
            }
            if options.auto_advance {
                voice.push_str(", auto: true");
            }
            if options.inherit_speaker {
                voice.push_str(", inherit_speaker: true");
            }
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
            if file == "none" {
                "none".into()
            } else {
                asset_id(model, ResourceKind::Bgm, file)?
            },
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
        if choice.enable_when.is_some() {
            bail!("disabled legacy choices require an enabled condition");
        }
        let target = match &choice.target {
            ChoiceTarget::ChangeScene(scene) => format!("goto({})", scene_id(model, scene)?),
            ChoiceTarget::CallScene(scene) => format!("call({})", scene_id(model, scene)?),
            ChoiceTarget::Label(_) => bail!("label choices require manual migration"),
        };
        output.push_str("    ");
        output.push_str(&string_literal(&choice.text));
        if let Some(condition) = &choice.show_when {
            output.push_str(&format!(
                " when ({})",
                v11::expression_source(condition, model)?
            ));
        }
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

fn object_id<'a>(model: &'a MigrationModel, name: &str) -> Result<&'a str> {
    model
        .object_ids
        .get(name)
        .map(String::as_str)
        .with_context(|| format!("missing migrated object mapping for {name:?}"))
}

fn native_identifier(value: &str) -> Result<&str> {
    if !value.is_empty()
        && !value.as_bytes()[0].is_ascii_digit()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        Ok(value)
    } else {
        bail!("preset {value:?} is not a native identifier")
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
    if milliseconds.fract() == 0.0 && (milliseconds as f64 / 1000.0) as f32 == seconds {
        format!("{milliseconds}ms")
    } else {
        format!("{seconds}s")
    }
}

fn number(value: f32) -> String {
    value.to_string()
}

fn resolve_resource(config: &GameConfig, kind: ResourceKind, name: &str) -> Result<String> {
    Ok(match kind {
        ResourceKind::Background => config.bg_path(name),
        ResourceKind::Figure => config.figure_path(name),
        ResourceKind::Voice => config.voice_path(name),
        ResourceKind::Bgm => config.bgm_path(name),
        ResourceKind::Effect => config.effect_path(name),
        ResourceKind::Video => config.video_path(name),
        ResourceKind::Particle => name.to_owned(),
        ResourceKind::MiniAvatar | ResourceKind::Lut => {
            bail!("{kind:?} resources require manual migration")
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
        ResourceKind::Particle => "particles",
        ResourceKind::MiniAvatar | ResourceKind::Lut => {
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
        ResourceKind::MiniAvatar | ResourceKind::Lut => {
            bail!("{kind:?} resources require manual migration")
        }
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
    fn dialogue_and_choice_visibility_migrate_without_losing_options() {
        let model = MigrationModel {
            scene_ids: HashMap::from([("next".into(), "next".into())]),
            speaker_ids: HashMap::from([("Hero".into(), "hero".into())]),
            asset_ids: HashMap::new(),
            object_ids: BTreeMap::new(),
            prefix_ids: BTreeMap::new(),
            objects: objects::ObjectManifest::default(),
            variable_ids: BTreeMap::new(),
            initial_variables: BTreeMap::new(),
            assets: AssetManifest::default(),
            characters: CharacterManifest {
                characters: BTreeMap::new(),
            },
        };
        let options = keine_core::action::SayOptions {
            volume: 0.3,
            concat: true,
            auto_advance: true,
            inherit_speaker: true,
            ..Default::default()
        };
        let dialogue = render_action(
            &Action::Say {
                speaker: "Hero".into(),
                text: "前[wait=100]后".into(),
                options: options.clone(),
            },
            &model,
        )
        .unwrap();
        let parsed =
            keine_loader::adapter::parse_native_scenes(&format!("scene test {{ {dialogue} }}"));
        assert!(parsed[0].report.diagnostics.is_empty());
        let Action::EiyashouSay(say) = &parsed[0].report.actions[0] else {
            panic!("not dialogue");
        };
        assert_eq!(say.options, options);
        let choices = [keine_core::action::Choice {
            text: "Next".into(),
            target: ChoiceTarget::ChangeScene("next".into()),
            show_when: Some("false".into()),
            enable_when: None,
        }];
        let menu = render_menu("", &choices, &model).unwrap();
        let parsed =
            keine_loader::adapter::parse_native_scenes(&format!("scene test {{ {menu} }}"));
        assert!(
            !parsed[0]
                .report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.level == DiagnosticLevel::Error),
            "{:?}",
            parsed[0].report.diagnostics
        );
        let Action::EiyashouMenu { choices, .. } = &parsed[0].report.actions[0] else {
            panic!("not menu");
        };
        assert!(choices[0].show_when.is_some());
        let scene = LoadedScene {
            name: "next".into(),
            path: "next.json".into(),
            actions: vec![
                Action::FocusPortrait { speaker_id: None },
                Action::Say {
                    speaker: "Hero".into(),
                    text: "Hello".into(),
                    options: Default::default(),
                },
            ],
            action_spans: Vec::new(),
            diagnostics: Vec::new(),
            resources: Vec::new(),
            sub_scenes: Vec::new(),
        };
        let source = render_scenes(&[scene], &model).unwrap();
        assert!(!source.contains("sprite.focus("));
        assert!(source.contains("hero: \"Hello\""));
    }

    #[test]
    fn durations_prefer_exact_milliseconds() {
        assert_eq!(duration(0.3), "300ms");
        assert_eq!(duration(1.25), "1250ms");
    }

    #[test]
    fn configured_portraits_migrate_to_automatic_dialogue_focus() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let stage = root.path().join("native");
        fs::create_dir_all(source.join("assets")).unwrap();
        fs::create_dir_all(&stage).unwrap();
        let config = GameConfig::default();
        let content = keine_loader::load_project(&source, &config.adapter.asset).unwrap();
        let scenes = vec![LoadedScene {
            name: "start".into(),
            path: "start.json".into(),
            actions: vec![
                Action::ConfigurePortraits {
                    enabled: true,
                    character_ids: vec!["original-character".into()],
                    speaking: Default::default(),
                    others: Default::default(),
                    narration: Default::default(),
                    duration: 0.3,
                    easing: Easing::Linear,
                },
                Action::FocusPortrait {
                    speaker_id: Some("original-character".into()),
                },
                Action::Say {
                    speaker: "Hero".into(),
                    text: "Hi".into(),
                    options: Default::default(),
                },
                Action::FocusPortrait { speaker_id: None },
                Action::Say {
                    speaker: String::new(),
                    text: "Narration".into(),
                    options: Default::default(),
                },
            ],
            action_spans: Vec::new(),
            diagnostics: Vec::new(),
            resources: Vec::new(),
            sub_scenes: Vec::new(),
        }];
        let model = build_model(&config, &content, &scenes, &stage).unwrap();
        assert_eq!(
            model.speaker_ids["Hero"],
            model.object_ids["original-character"]
        );
        write_project(&stage, config, &scenes, &model, &LoaderRegistry::default()).unwrap();
        let generated = fs::read_to_string(stage.join("scripts/main.shou")).unwrap();
        assert!(!generated.contains("sprite.focus("));
        assert_eq!(
            keine_loader::format_native_source(&generated).unwrap(),
            generated
        );
        let opened = open_project(&stage, &LoaderRegistry::default()).unwrap();
        let loaded = keine_loader::load_scenes(&opened.content).unwrap();
        assert!(
            matches!(&loaded[0].actions[1], Action::FocusPortrait { speaker_id } if speaker_id.as_deref() == Some("original-character"))
        );
        assert!(matches!(
            &loaded[0].actions[3],
            Action::FocusPortrait { speaker_id: None }
        ));
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
        assert_eq!(GameConfig::from_yaml(&config).unwrap().script.version, 2);
        let native = fs::read_to_string(target.join("scripts/main.shou")).unwrap();
        assert!(native.contains("speaker_0001: \"Hello\""));
        assert!(!native.contains("Alice:Hello;"));
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
