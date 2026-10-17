use std::collections::HashMap;
use std::path::Path;

use bevy::app::AppExit;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::runtime::preview::AuthoringPreviewSession;
use crate::runtime::resources::{
    EditorSyncSession, GameState, PersistenceDisabled, PersistenceRoot, writable_runtime_session,
};

const VERSION: u32 = 2;
pub(super) const RETRY_DELAY_SECONDS: f32 = 1.0;
const MAX_GALLERY_BYTES: usize = 16 * 1024 * 1024;
const MAX_GALLERY_ENTRIES: usize = 65_536;

#[derive(Serialize)]
struct GalleryFile<'a> {
    version: u32,
    cg: &'a HashMap<String, String>,
    bgm: &'a HashMap<String, String>,
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
    pub(super) retry_seconds: Option<f32>,
}

impl GallerySnapshot {
    pub(crate) fn loaded(state: &keine_core::State) -> Self {
        Self {
            cg: state.unlocked_cg.clone(),
            bgm: state.unlocked_bgm.clone(),
            retry_seconds: None,
        }
    }
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

#[allow(
    clippy::too_many_arguments,
    reason = "ECS persistence inputs and failure reporting stay explicit"
)]
pub(crate) fn persist(
    mut commands: Commands,
    time: Res<Time<Real>>,
    state: Res<GameState>,
    project_root: Res<PersistenceRoot>,
    mut previous: ResMut<GallerySnapshot>,
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
    if let Some(elapsed) = &mut previous.retry_seconds {
        *elapsed += time.delta_secs();
        if *elapsed < RETRY_DELAY_SECONDS {
            return;
        }
    } else if !state.is_changed() {
        return;
    }
    if previous.cg == state.unlocked_cg && previous.bgm == state.unlocked_bgm {
        previous.retry_seconds = None;
        return;
    }
    persist_now(&state, &project_root, &mut previous, &mut commands);
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
    mut previous: ResMut<GallerySnapshot>,
    editor_sync: Option<Res<EditorSyncSession>>,
    authoring_preview: Option<Res<AuthoringPreviewSession>>,
    persistence_disabled: Option<Res<PersistenceDisabled>>,
) {
    if writable_runtime_session(
        editor_sync.is_some(),
        authoring_preview.is_some(),
        persistence_disabled.is_some(),
    ) && exits.read().next().is_some()
        && (previous.cg != state.unlocked_cg || previous.bgm != state.unlocked_bgm)
    {
        persist_now(&state, &project_root, &mut previous, &mut commands);
    }
}

fn persist_now(
    state: &GameState,
    project_root: &Path,
    previous: &mut GallerySnapshot,
    commands: &mut Commands,
) {
    let file = GalleryFile {
        version: VERSION,
        cg: &state.unlocked_cg,
        bgm: &state.unlocked_bgm,
    };
    let target = path(project_root);
    let result =
        super::encode_postcard_limited(&file, MAX_GALLERY_BYTES, "gallery").and_then(|bytes| {
            super::recovery::write_preserving(&target, &bytes, MAX_GALLERY_BYTES, decode)
        });
    match result {
        Ok(()) => {
            previous.cg.clone_from(&state.unlocked_cg);
            previous.bgm.clone_from(&state.unlocked_bgm);
            previous.retry_seconds = None;
        }
        Err(error) => {
            crate::ui::dialog::report_background_save_failure(commands);
            previous.retry_seconds = Some(0.0);
            log::error!("failed to persist gallery: {error:#}");
        }
    }
}

fn decode(bytes: &[u8]) -> anyhow::Result<GalleryFileWire> {
    let file: GalleryFileWire = super::decode_postcard_exact(bytes)?;
    anyhow::ensure!(file.version == VERSION, "unsupported gallery version");
    if file.cg.len().saturating_add(file.bgm.len()) > MAX_GALLERY_ENTRIES {
        anyhow::bail!("gallery contains too many entries");
    }
    Ok(file)
}

pub(super) fn check(root: &Path) -> anyhow::Result<()> {
    super::recovery::read_optional(&path(root), MAX_GALLERY_BYTES, decode).map(|_| ())
}

fn path(project_root: &Path) -> std::path::PathBuf {
    project_root.join("saves/gallery.bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_wire_decoder_accepts_the_existing_map_encoding() {
        let cg = HashMap::from([("memory.webp".into(), "Memory".into())]);
        let bgm = HashMap::from([("theme.opus".into(), "Theme".into())]);
        let encoded = postcard::to_stdvec(&GalleryFile {
            version: VERSION,
            cg: &cg,
            bgm: &bgm,
        })
        .unwrap();

        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.cg, [("memory.webp".into(), "Memory".into())]);
        assert_eq!(decoded.bgm, [("theme.opus".into(), "Theme".into())]);
    }

    #[test]
    fn loaded_snapshot_matches_persisted_state() {
        let mut state = keine_core::State::new();
        state
            .unlocked_cg
            .insert("memory.webp".into(), "Memory".into());
        state
            .unlocked_bgm
            .insert("theme.opus".into(), "Theme".into());

        let snapshot = GallerySnapshot::loaded(&state);
        assert_eq!(snapshot.cg, state.unlocked_cg);
        assert_eq!(snapshot.bgm, state.unlocked_bgm);
    }
}
