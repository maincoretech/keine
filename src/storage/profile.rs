use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use bevy::app::AppExit;
use bevy::prelude::*;
use keine_core::Value;
use serde::{Deserialize, Serialize};

use crate::runtime::preview::AuthoringPreviewSession;
use crate::runtime::resources::{
    EditorSyncSession, GameState, PersistenceDisabled, PersistenceRoot, writable_runtime_session,
};

const VERSION: u32 = 1;
pub(super) const WRITE_DELAY_SECONDS: f32 = 0.5;
const MAX_PROFILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_PROFILE_ENTRIES: usize = 65_536;

#[derive(Serialize)]
struct ProfileFile<'a> {
    version: u32,
    global_vars: &'a HashMap<String, Value>,
}

#[derive(Deserialize)]
struct ProfileFileWire {
    version: u32,
    global_vars: Vec<(String, Value)>,
}

#[derive(Resource, Default)]
pub(crate) struct ProfileWriter {
    pub(super) saved: HashMap<String, Value>,
    // None is clean; Some(0.0) keeps a failed write pending for the next delay.
    pub(super) dirty_seconds: Option<f32>,
}

impl ProfileWriter {
    pub(crate) fn loaded(global_vars: &HashMap<String, Value>) -> Self {
        Self {
            saved: global_vars.clone(),
            dirty_seconds: None,
        }
    }
}

pub(crate) fn load(project_root: &Path) -> HashMap<String, Value> {
    let target = path(project_root);
    super::read_limited(&target, MAX_PROFILE_BYTES)
        .and_then(|bytes| decode(&bytes))
        .map(|file| {
            if file.version == VERSION {
                file.global_vars.into_iter().collect()
            } else {
                HashMap::new()
            }
        })
        .map_err(|error| log::debug!("profile unavailable at {}: {error:#}", target.display()))
        .unwrap_or_default()
}

#[allow(
    clippy::too_many_arguments,
    reason = "ECS persistence inputs and failure reporting stay explicit"
)]
pub(crate) fn persist(
    mut commands: Commands,
    time: Res<Time<Real>>,
    state: Res<GameState>,
    project_root: Res<PersistenceRoot>,
    mut writer: ResMut<ProfileWriter>,
    editor_sync: Option<Res<EditorSyncSession>>,
    authoring_preview: Option<Res<AuthoringPreviewSession>>,
    persistence_disabled: Option<Res<PersistenceDisabled>>,
) {
    if !writable_runtime_session(
        editor_sync.is_some(),
        authoring_preview.is_some(),
        persistence_disabled.is_some(),
    ) {
        return;
    }
    if writer.dirty_seconds.is_none() && !state.is_changed() {
        return;
    }
    if writer.saved == state.global_vars {
        writer.dirty_seconds = None;
        return;
    }
    let elapsed = writer.dirty_seconds.get_or_insert(0.0);
    *elapsed += time.delta_secs();
    if *elapsed < WRITE_DELAY_SECONDS {
        return;
    }
    persist_now(
        &state.global_vars,
        &project_root,
        &mut writer,
        &mut commands,
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "ECS persistence inputs and failure reporting stay explicit"
)]
pub(crate) fn flush_on_exit(
    mut commands: Commands,
    mut exits: MessageReader<AppExit>,
    state: Res<GameState>,
    project_root: Res<PersistenceRoot>,
    mut writer: ResMut<ProfileWriter>,
    editor_sync: Option<Res<EditorSyncSession>>,
    authoring_preview: Option<Res<AuthoringPreviewSession>>,
    persistence_disabled: Option<Res<PersistenceDisabled>>,
) {
    if !writable_runtime_session(
        editor_sync.is_some(),
        authoring_preview.is_some(),
        persistence_disabled.is_some(),
    ) {
        return;
    }
    if exits.read().next().is_some() && writer.saved != state.global_vars {
        persist_now(
            &state.global_vars,
            &project_root,
            &mut writer,
            &mut commands,
        );
    }
}

fn persist_now(
    values: &HashMap<String, Value>,
    project_root: &Path,
    writer: &mut ProfileWriter,
    commands: &mut Commands,
) {
    match save(values, project_root) {
        Ok(()) => {
            writer.saved.clone_from(values);
            writer.dirty_seconds = None;
        }
        Err(error) => {
            crate::ui::dialog::report_background_save_failure(commands);
            writer.dirty_seconds = Some(0.0);
            log::warn!("failed to persist profile: {error:#}");
        }
    }
}

fn save(values: &HashMap<String, Value>, project_root: &Path) -> Result<()> {
    let target = path(project_root);
    let bytes = super::encode_postcard_limited(
        &ProfileFile {
            version: VERSION,
            global_vars: values,
        },
        MAX_PROFILE_BYTES,
        "profile",
    )?;
    super::write_atomically(&target, &bytes)
}

fn decode(bytes: &[u8]) -> Result<ProfileFileWire> {
    let file: ProfileFileWire = super::decode_postcard_exact(bytes)?;
    if file.global_vars.len() > MAX_PROFILE_ENTRIES {
        anyhow::bail!("profile contains too many variables");
    }
    Ok(file)
}

fn path(project_root: &Path) -> PathBuf {
    project_root.join("saves").join("profile.bin")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn profile_round_trip() {
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

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_a_collection_length_that_exceeds_the_available_payload() {
        let mut bytes = vec![VERSION as u8];
        bytes.extend([0xff; 9]);
        bytes.push(0x01);

        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn borrowed_encoder_keeps_the_existing_wire_schema() {
        let values = HashMap::from([("ending".into(), Value::Int(2))]);
        let bytes = postcard::to_stdvec(&ProfileFile {
            version: VERSION,
            global_vars: &values,
        })
        .unwrap();

        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.global_vars, [("ending".into(), Value::Int(2))]);
    }
}
