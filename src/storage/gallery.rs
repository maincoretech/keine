use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::runtime::resources::{EditorSyncSession, GameState, PersistenceDisabled, ProjectRoot};

const VERSION: u32 = 2;
const MAX_GALLERY_BYTES: usize = 16 * 1024 * 1024;
const MAX_GALLERY_ENTRIES: usize = 65_536;

#[derive(Serialize, Deserialize, Default)]
struct GalleryFile {
    version: u32,
    cg: HashMap<String, String>,
    bgm: HashMap<String, String>,
}

#[derive(Deserialize)]
struct GalleryFileWire {
    version: u32,
    cg: Vec<(String, String)>,
    bgm: Vec<(String, String)>,
}

#[derive(Resource, Default)]
pub(crate) struct GallerySnapshot {
    pub(super) cg: HashMap<String, String>,
    pub(super) bgm: HashMap<String, String>,
}

pub(crate) fn load(state: &mut keine_core::State, project_root: &Path) {
    let Ok(bytes) = super::read_limited(&path(project_root), MAX_GALLERY_BYTES) else {
        return;
    };
    let Ok(file) = decode(&bytes) else {
        return;
    };
    if file.version == VERSION {
        state.unlocked_cg = file.cg.into_iter().collect();
        state.unlocked_bgm = file.bgm.into_iter().collect();
    }
}

pub(crate) fn persist(
    state: Res<GameState>,
    project_root: Res<ProjectRoot>,
    mut previous: ResMut<GallerySnapshot>,
    editor_sync: Option<Res<EditorSyncSession>>,
    persistence_disabled: Option<Res<PersistenceDisabled>>,
) {
    if editor_sync.is_some() || persistence_disabled.is_some() {
        return;
    }
    if !state.is_changed()
        || (previous.cg == state.unlocked_cg && previous.bgm == state.unlocked_bgm)
    {
        return;
    }
    let file = GalleryFile {
        version: VERSION,
        cg: state.unlocked_cg.clone(),
        bgm: state.unlocked_bgm.clone(),
    };
    let target = path(&project_root);
    let result = postcard::to_stdvec(&file)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| super::write_atomically(&target, &bytes));
    match result {
        Ok(()) => {
            previous.cg.clone_from(&state.unlocked_cg);
            previous.bgm.clone_from(&state.unlocked_bgm);
        }
        Err(error) => log::error!("failed to persist gallery: {error:#}"),
    }
}

fn decode(bytes: &[u8]) -> anyhow::Result<GalleryFileWire> {
    let file: GalleryFileWire = postcard::from_bytes(bytes)?;
    if file.cg.len().saturating_add(file.bgm.len()) > MAX_GALLERY_ENTRIES {
        anyhow::bail!("gallery contains too many entries");
    }
    Ok(file)
}

pub(super) fn reset_memory(state: &mut keine_core::State, snapshot: &mut GallerySnapshot) {
    state.unlocked_cg.clear();
    state.unlocked_bgm.clear();
    snapshot.cg.clear();
    snapshot.bgm.clear();
}

fn path(project_root: &Path) -> std::path::PathBuf {
    project_root.join("saves/gallery.bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_wire_decoder_accepts_the_existing_map_encoding() {
        let encoded = postcard::to_stdvec(&GalleryFile {
            version: VERSION,
            cg: HashMap::from([("memory.webp".into(), "Memory".into())]),
            bgm: HashMap::from([("theme.opus".into(), "Theme".into())]),
        })
        .unwrap();

        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.cg, [("memory.webp".into(), "Memory".into())]);
        assert_eq!(decoded.bgm, [("theme.opus".into(), "Theme".into())]);
    }
}
