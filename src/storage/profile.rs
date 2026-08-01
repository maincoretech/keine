use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bevy::app::AppExit;
use bevy::prelude::*;
use keine_core::Value;
use serde::{Deserialize, Serialize};

use crate::runtime::resources::{EditorSyncSession, GameState, PersistenceDisabled, ProjectRoot};

const VERSION: u32 = 1;
const WRITE_DELAY_SECONDS: f32 = 0.5;

#[derive(Serialize, Deserialize)]
struct ProfileFile {
    version: u32,
    global_vars: HashMap<String, Value>,
}

#[derive(Resource, Default)]
pub(crate) struct ProfileWriter {
    pub(super) saved: HashMap<String, Value>,
    pub(super) dirty_seconds: f32,
}

impl ProfileWriter {
    pub(crate) fn loaded(global_vars: &HashMap<String, Value>) -> Self {
        Self {
            saved: global_vars.clone(),
            dirty_seconds: 0.0,
        }
    }
}

pub(crate) fn load(project_root: &Path) -> HashMap<String, Value> {
    let target = path(project_root);
    fs::read(&target)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| postcard::from_bytes::<ProfileFile>(&bytes).map_err(anyhow::Error::from))
        .map(|file| {
            if file.version == VERSION {
                file.global_vars
            } else {
                HashMap::new()
            }
        })
        .map_err(|error| log::debug!("profile unavailable at {}: {error:#}", target.display()))
        .unwrap_or_default()
}

pub(crate) fn persist(
    time: Res<Time>,
    state: Res<GameState>,
    project_root: Res<ProjectRoot>,
    mut writer: ResMut<ProfileWriter>,
    editor_sync: Option<Res<EditorSyncSession>>,
    persistence_disabled: Option<Res<PersistenceDisabled>>,
) {
    if editor_sync.is_some() || persistence_disabled.is_some() {
        return;
    }
    if writer.saved == state.global_vars {
        writer.dirty_seconds = 0.0;
        return;
    }
    writer.dirty_seconds += time.delta_secs();
    if writer.dirty_seconds < WRITE_DELAY_SECONDS {
        return;
    }
    persist_now(&state.global_vars, &project_root, &mut writer);
}

pub(crate) fn flush_on_exit(
    mut exits: MessageReader<AppExit>,
    state: Res<GameState>,
    project_root: Res<ProjectRoot>,
    mut writer: ResMut<ProfileWriter>,
    editor_sync: Option<Res<EditorSyncSession>>,
    persistence_disabled: Option<Res<PersistenceDisabled>>,
) {
    if editor_sync.is_some() || persistence_disabled.is_some() {
        return;
    }
    if exits.read().next().is_some() && writer.saved != state.global_vars {
        persist_now(&state.global_vars, &project_root, &mut writer);
    }
}

pub(super) fn reset_memory(state: &mut keine_core::State, writer: &mut ProfileWriter) {
    state.global_vars.clear();
    writer.saved.clear();
    writer.dirty_seconds = 0.0;
}

fn persist_now(values: &HashMap<String, Value>, project_root: &Path, writer: &mut ProfileWriter) {
    match save(values, project_root) {
        Ok(()) => {
            writer.saved.clone_from(values);
            writer.dirty_seconds = 0.0;
        }
        Err(error) => log::warn!("failed to persist profile: {error:#}"),
    }
}

fn save(values: &HashMap<String, Value>, project_root: &Path) -> Result<()> {
    let target = path(project_root);
    let temporary = target.with_extension("tmp");
    let parent = target.parent().context("profile path has no parent")?;
    fs::create_dir_all(parent)?;
    fs::write(
        &temporary,
        postcard::to_stdvec(&ProfileFile {
            version: VERSION,
            global_vars: values.clone(),
        })?,
    )?;
    fs::rename(&temporary, &target)?;
    Ok(())
}

fn path(project_root: &Path) -> PathBuf {
    project_root.join("saves").join("profile.bin")
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn profile_round_trip_and_reset() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-profile-{nonce}"));
        let values = HashMap::from([
            ("ending".into(), Value::Int(2)),
            ("name".into(), Value::Str("Echo".into())),
        ]);

        save(&values, &root).unwrap();
        assert_eq!(load(&root), values);

        let mut state = keine_core::State::new();
        state.global_vars = values.clone();
        let mut writer = ProfileWriter::loaded(&values);
        writer.dirty_seconds = WRITE_DELAY_SECONDS;
        reset_memory(&mut state, &mut writer);
        assert!(state.global_vars.is_empty());
        assert!(writer.saved.is_empty());
        assert_eq!(writer.dirty_seconds, 0.0);
        assert_eq!(load(&root), values);
        let _ = fs::remove_dir_all(root);
    }
}
