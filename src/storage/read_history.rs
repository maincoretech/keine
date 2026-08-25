use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use bevy::prelude::*;
use keine_core::state::DialogueKey;
use serde::{Deserialize, Serialize};

use crate::runtime::resources::{
    EditorSyncSession, GameState, PersistenceDisabled, PersistenceRoot,
};

const VERSION: u32 = 1;
const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;
const MAX_HISTORY_ENTRIES: usize = 1_000_000;

#[derive(Deserialize)]
struct HistoryFileWire {
    version: u32,
    entries: Vec<DialogueKey>,
}

#[derive(Serialize)]
struct HistoryFileRef<'a> {
    version: u32,
    entries: &'a HashSet<DialogueKey>,
}

#[derive(Resource, Default)]
pub(crate) struct ReadHistoryWriter {
    pub(super) saved_len: usize,
    pub(super) dirty_seconds: f32,
}

impl ReadHistoryWriter {
    pub(crate) fn loaded(count: usize) -> Self {
        Self {
            saved_len: count,
            dirty_seconds: 0.0,
        }
    }
}

pub(crate) fn load(project_root: &Path) -> HashSet<DialogueKey> {
    let path = history_path(project_root);
    super::read_limited(&path, MAX_HISTORY_BYTES)
        .and_then(|bytes| decode(&bytes))
        .map(|file| {
            if file.version == VERSION {
                file.entries.into_iter().collect()
            } else {
                HashSet::new()
            }
        })
        .map_err(|error| log::debug!("read history unavailable at {}: {error:#}", path.display()))
        .unwrap_or_default()
}

pub(crate) fn persist_read_history(
    time: Res<Time>,
    state: Res<GameState>,
    project_root: Res<PersistenceRoot>,
    mut writer: ResMut<ReadHistoryWriter>,
    editor_sync: Option<Res<EditorSyncSession>>,
    persistence_disabled: Option<Res<PersistenceDisabled>>,
) {
    if editor_sync.is_some() || persistence_disabled.is_some() {
        return;
    }
    if writer.dirty_seconds == 0.0 && !state.is_changed() {
        return;
    }
    if writer.saved_len == state.read_dialogues.len() {
        if writer.dirty_seconds != 0.0 {
            writer.dirty_seconds = 0.0;
        }
        return;
    }
    writer.dirty_seconds += time.delta_secs();
    if writer.dirty_seconds < 1.0 {
        return;
    }
    match save(&state.read_dialogues, &project_root) {
        Ok(()) => {
            writer.saved_len = state.read_dialogues.len();
            writer.dirty_seconds = 0.0;
        }
        Err(error) => log::warn!("failed to persist read history: {error:#}"),
    }
}

pub(super) fn reset_memory(state: &mut keine_core::State, writer: &mut ReadHistoryWriter) {
    state.read_dialogues.clear();
    writer.saved_len = 0;
    writer.dirty_seconds = 0.0;
}

fn save(history: &HashSet<DialogueKey>, project_root: &Path) -> Result<()> {
    let path = history_path(project_root);
    let bytes = super::encode_postcard_limited(
        &HistoryFileRef {
            version: VERSION,
            entries: history,
        },
        MAX_HISTORY_BYTES,
        "read history",
    )?;
    super::write_atomically(&path, &bytes)
}

fn decode(bytes: &[u8]) -> Result<HistoryFileWire> {
    let file: HistoryFileWire = postcard::from_bytes(bytes)?;
    if file.entries.len() > MAX_HISTORY_ENTRIES {
        anyhow::bail!("read history contains too many entries");
    }
    Ok(file)
}

fn history_path(project_root: &Path) -> PathBuf {
    project_root.join("saves").join("read_history.bin")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn persists_read_positions_across_runs() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-read-history-{nonce}"));
        let expected = HashSet::from([DialogueKey {
            scene: "main".into(),
            action_index: 7,
        }]);

        save(&expected, &root).unwrap();
        assert_eq!(load(&root), expected);

        let _ = fs::remove_dir_all(root);
    }
}
