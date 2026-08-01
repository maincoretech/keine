use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bevy::prelude::*;
use keine_core::state::DialogueKey;
use serde::{Deserialize, Serialize};

use crate::runtime::resources::{EditorSyncSession, GameState, PersistenceDisabled, ProjectRoot};

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct HistoryFile {
    version: u32,
    entries: HashSet<DialogueKey>,
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
    fs::read(&path)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| postcard::from_bytes::<HistoryFile>(&bytes).map_err(anyhow::Error::from))
        .map(|file| {
            if file.version == VERSION {
                file.entries
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
    project_root: Res<ProjectRoot>,
    mut writer: ResMut<ReadHistoryWriter>,
    editor_sync: Option<Res<EditorSyncSession>>,
    persistence_disabled: Option<Res<PersistenceDisabled>>,
) {
    if editor_sync.is_some() || persistence_disabled.is_some() {
        return;
    }
    if writer.saved_len == state.read_dialogues.len() {
        writer.dirty_seconds = 0.0;
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
    let temporary = path.with_extension("bin.tmp");
    let parent = path.parent().context("read history path has no parent")?;
    fs::create_dir_all(parent)?;
    fs::write(
        &temporary,
        postcard::to_stdvec(&HistoryFileRef {
            version: VERSION,
            entries: history,
        })?,
    )?;
    fs::rename(&temporary, &path)?;
    Ok(())
}

fn history_path(project_root: &Path) -> PathBuf {
    project_root.join("saves").join("read_history.bin")
}

#[cfg(test)]
mod tests {
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
