use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use bevy::prelude::*;
use keine_core::Program;
use keine_loader::{DiagnosticLevel, load_scenes_with};

use super::resources::{
    ContentProjectResource, GameConfigResource, GameState, LocalAssetManifest, LocalSceneAssets,
    ScriptLanguages,
};

pub(crate) struct AuthoringPreviewConfig {
    pub(crate) document_revision: u64,
}

#[derive(Resource, Default)]
pub(crate) struct AuthoringPreviewSession {
    pub(crate) settle_until: Option<Instant>,
}

const SOURCE_SETTLE_TIME: Duration = Duration::from_millis(250);

fn keep_loading_frames_alive(world: &mut World) {
    if let Some(mut preview) = world.get_resource_mut::<AuthoringPreviewSession>() {
        preview.settle_until = Some(Instant::now() + SOURCE_SETTLE_TIME);
    }
}

#[derive(Resource)]
struct PreviewDocumentRevision(u64);

pub(crate) struct AuthoringPreviewPlugin {
    config: std::sync::Mutex<Option<AuthoringPreviewConfig>>,
}

impl AuthoringPreviewPlugin {
    pub(crate) fn new(config: AuthoringPreviewConfig) -> Self {
        Self {
            config: std::sync::Mutex::new(Some(config)),
        }
    }
}

impl Plugin for AuthoringPreviewPlugin {
    fn build(&self, app: &mut App) {
        let config = self
            .config
            .lock()
            .expect("authoring preview configuration lock poisoned")
            .take()
            .expect("authoring preview plugin may only be built once");
        app.insert_resource(PreviewDocumentRevision(config.document_revision))
            .init_resource::<AuthoringPreviewSession>();
    }
}

pub(crate) fn set_document_revision(world: &mut World, revision: u64) {
    if let Some(mut current) = world.get_resource_mut::<PreviewDocumentRevision>() {
        current.0 = revision;
    }
}

/// Swap only the authored Program. The native window, GPU surfaces, and
/// ordinary runtime input remain alive across source edits.
pub(crate) fn reload_source(world: &mut World, revision: u64) -> Result<()> {
    let content = &world.resource::<ContentProjectResource>().0;
    let languages = &world.resource::<ScriptLanguages>().0;
    let config = &world.resource::<GameConfigResource>().0;
    let mut scenes =
        load_scenes_with(content, languages).context("failed to reload preview source")?;
    let native = config.adapter.script.eq_ignore_ascii_case("keine");
    if native {
        keine_loader::validate_native_entry_flow(&mut scenes, &config.script.entry);
        anyhow::ensure!(
            scenes.iter().any(|scene| scene.name == config.script.entry),
            "preview entry scene is missing"
        );
        anyhow::ensure!(
            !scenes.iter().any(|scene| scene
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.level == DiagnosticLevel::Error)),
            "preview source has script errors"
        );
    }
    let mut manifest = LocalAssetManifest::default();
    let mut program_scenes = Vec::with_capacity(scenes.len());
    for scene in scenes {
        for diagnostic in &scene.diagnostics {
            let message = format!(
                "{}:{}:{}: {}",
                scene.path.display(),
                diagnostic.span.line,
                diagnostic.span.column,
                diagnostic.message
            );
            match diagnostic.level {
                DiagnosticLevel::Warning => log::warn!("{message}"),
                DiagnosticLevel::Error => log::error!("{message}"),
            }
        }
        manifest.insert(
            scene.name.clone(),
            LocalSceneAssets {
                source_path: scene.path,
                resources: scene.resources,
                sub_scenes: scene.sub_scenes,
                action_spans: scene.action_spans,
            },
        );
        program_scenes.push((scene.name, scene.actions));
    }
    let mut image_roles = crate::scene::images::ImageRoleRegistry::default();
    image_roles.rebuild(world.resource::<GameConfigResource>(), &manifest);
    *world.resource_mut::<LocalAssetManifest>() = manifest;
    *world.resource_mut::<crate::scene::images::ImageRoleRegistry>() = image_roles;
    super::tick::restart_after_program_reload(
        &mut world.resource_mut::<GameState>(),
        Program::from_scenes(program_scenes),
    );
    set_document_revision(world, revision);
    keep_loading_frames_alive(world);
    Ok(())
}

pub(crate) fn seek_source(world: &mut World, path: &Path, line: usize) -> bool {
    let Some(content) = world
        .get_resource::<ContentProjectResource>()
        .map(|content| content.0.clone())
    else {
        return false;
    };
    let Some(manifest) = world
        .get_resource::<LocalAssetManifest>()
        .map(|manifest| manifest.0.clone())
    else {
        return false;
    };
    let Some(mut state) = world.get_resource_mut::<GameState>() else {
        return false;
    };
    let accepted = super::tick::sync_editor_source_position(
        &content,
        &mut state,
        &LocalAssetManifest(manifest),
        path,
        line,
    );
    if accepted {
        keep_loading_frames_alive(world);
    }
    accepted
}

pub(crate) fn source_location(world: &World) -> Option<(PathBuf, usize, usize)> {
    let state = world.get_resource::<GameState>()?;
    let scene = world
        .get_resource::<LocalAssetManifest>()?
        .get(&state.current_scene)?;
    let index = state
        .cursor
        .saturating_sub(1)
        .min(scene.action_spans.len().checked_sub(1)?);
    let span = scene.action_spans.get(index)?;
    let path = if scene.source_path.starts_with("scripts") {
        scene.source_path.clone()
    } else {
        Path::new("scripts").join(&scene.source_path)
    };
    Some((path, span.line, span.column))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use keine_core::State;
    use keine_loader::SourceSpan;

    use super::*;

    #[test]
    fn source_location_follows_the_executed_action_in_its_script() {
        let mut world = World::new();
        let mut state = State::new();
        state.current_scene = "opening".into();
        state.cursor = 2;
        world.insert_resource(GameState(state));
        world.insert_resource(LocalAssetManifest(HashMap::from([(
            "opening".into(),
            LocalSceneAssets {
                source_path: PathBuf::from("main.shou"),
                action_spans: vec![
                    SourceSpan { line: 2, column: 3 },
                    SourceSpan { line: 4, column: 5 },
                ],
                ..default()
            },
        )])));
        assert_eq!(
            source_location(&world),
            Some((PathBuf::from("scripts/main.shou"), 4, 5))
        );
    }
}
