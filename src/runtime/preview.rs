use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use bevy::ecs::system::SystemState;
use bevy::prelude::*;

use super::resources::{
    ContentProjectResource, GameConfigResource, GameState, LocalAssetManifest, ScriptLanguages,
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
    if world.contains_resource::<crate::storage::save::ContinuationCheckpoint>() {
        world.resource_scope(
            |world, mut checkpoint: Mut<crate::storage::save::ContinuationCheckpoint>| {
                checkpoint.reset(world.resource::<GameState>());
            },
        );
    }
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

fn set_document_revision(world: &mut World, revision: u64) {
    if let Some(mut current) = world.get_resource_mut::<PreviewDocumentRevision>() {
        current.0 = revision;
    }
}

pub(crate) fn document_revision(world: &World) -> Option<u64> {
    world
        .get_resource::<PreviewDocumentRevision>()
        .map(|revision| revision.0)
}

/// Swap only the authored Program. The native window, GPU surfaces, and
/// ordinary runtime input remain alive across source edits.
pub(crate) fn reload_source(world: &mut World, revision: u64) -> Result<bool> {
    let content = &world.resource::<ContentProjectResource>().0;
    let languages = &world.resource::<ScriptLanguages>().0;
    let build = super::tick::reload::build_hot_reload(
        content,
        languages,
        &world.resource::<GameConfigResource>().0,
    )?;
    // Borrow in place: resource_scope removes/reinserts resources and marks
    // them changed even when only source spans were updated in Bevy 0.19.
    type ReloadResources<'w> = (
        ResMut<'w, GameState>,
        ResMut<'w, LocalAssetManifest>,
        ResMut<'w, GameConfigResource>,
        ResMut<'w, crate::scene::images::ImageRoleRegistry>,
    );
    let mut resources: SystemState<ReloadResources<'_>> = SystemState::new(world);
    let (mut state, mut manifest, mut config, mut image_roles) = resources
        .get_mut(world)
        .map_err(|error| anyhow::anyhow!("preview resources unavailable: {error}"))?;
    let restarted = super::tick::reload::apply_hot_reload(
        build,
        state.bypass_change_detection(),
        manifest.reborrow(),
        config.reborrow(),
        image_roles.reborrow(),
        true,
    );
    if restarted {
        world.resource_mut::<GameState>().set_changed();
    }
    set_document_revision(world, revision);
    if restarted {
        keep_loading_frames_alive(world);
    }
    Ok(restarted)
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

mod inspection;
pub(crate) use inspection::runtime_snapshot;

pub(crate) fn source_location(world: &World) -> Option<(PathBuf, usize, usize)> {
    let state = world.get_resource::<GameState>()?;
    if state.ended || state.cursor == 0 {
        return None;
    }
    state_location(world, state)
}

fn state_location(world: &World, state: &keine_core::State) -> Option<(PathBuf, usize, usize)> {
    state_location_at(world, &state.current_scene, state.cursor.saturating_sub(1))
}

fn state_location_at(
    world: &World,
    scene_name: &str,
    index: usize,
) -> Option<(PathBuf, usize, usize)> {
    let scene = world
        .get_resource::<LocalAssetManifest>()?
        .get(scene_name)?;
    let index = index.min(scene.action_spans.len().checked_sub(1)?);
    let span = scene.action_spans.get(index)?;
    let path = if scene.source_path.starts_with("scripts") {
        scene.source_path.clone()
    } else {
        Path::new("scripts").join(&scene.source_path)
    };
    Some((path, span.line, span.column))
}

/// Preview-only scalar edits preserve the existing variable type. No source writes.
pub(crate) fn set_variable(
    world: &mut World,
    name: &str,
    global: bool,
    expected: &str,
    value: &str,
) -> Result<(), String> {
    use keine_core::Value;
    if name.len() > 128 || value.len() > 512 {
        return Err("Variable value is too long".into());
    }
    let mut state = world
        .get_resource_mut::<GameState>()
        .ok_or("Preview is not running")?;
    let variables = if global {
        &mut state.global_vars
    } else {
        &mut state.vars
    };
    let current = variables.get_mut(name).ok_or("Variable no longer exists")?;
    let displayed = match &*current {
        Value::Str(s) => s.clone(),
        value => value.display(),
    };
    if displayed != expected {
        return Err("Variable changed; reopen its editor".into());
    }
    let invalid = || "Enter a value of the same type".to_string();
    *current = match current {
        Value::Bool(_) => Value::Bool(value.parse().map_err(|_| invalid())?),
        Value::Int(_) => Value::Int(value.parse().map_err(|_| invalid())?),
        Value::Float(_) => {
            let number: f64 = value.parse().map_err(|_| invalid())?;
            if !number.is_finite() {
                return Err(invalid());
            }
            Value::Float(number)
        }
        Value::Str(_) => Value::Str(value.into()),
        Value::Array(_) => return Err("Array editing is not supported".into()),
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::resources::LocalSceneAssets;
    use keine_core::State;
    use keine_loader::SourceSpan;

    use super::*;

    #[test]
    fn preview_variable_edits_preserve_types_and_reject_stale_values() {
        use keine_core::Value;
        let mut world = World::new();
        let mut state = State::new();
        state.vars.insert("score".into(), Value::Int(1));
        state
            .global_vars
            .insert("ending".into(), Value::Bool(false));
        world.insert_resource(GameState(state));
        set_variable(&mut world, "score", false, "1", "10").unwrap();
        assert_eq!(world.resource::<GameState>().vars["score"], Value::Int(10));
        assert!(set_variable(&mut world, "score", false, "1", "20").is_err());
        assert!(set_variable(&mut world, "score", false, "10", "1.5").is_err());
        set_variable(&mut world, "ending", true, "false", "true").unwrap();
        assert_eq!(
            world.resource::<GameState>().global_vars["ending"],
            Value::Bool(true)
        );
        assert!(set_variable(&mut world, "missing", false, "", "1").is_err());
    }

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
