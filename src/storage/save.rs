use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bevy::app::AppExit;
use bevy::prelude::*;
use keine_core::State;
use keine_loader::{SavedState, StoreAdapter, StoreStatus};

use crate::runtime::resources::{
    EditorSyncSession, GameState, PersistenceDisabled, ProjectRoot, StoreCodec,
};

pub const QUICK_SAVE_SLOT: u32 = 0;
pub use keine_loader::StoreMetadata as SaveMetadata;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotStatus {
    Empty,
    Ready(SaveMetadata),
    Corrupt,
    Unsupported(u32),
}

pub fn save_game(
    store: &dyn StoreAdapter,
    state: &State,
    slot: u32,
    project_root: &Path,
) -> Result<()> {
    let path = slot_path(store, project_root, slot);
    let parent = path.parent().context("save slot path has no parent")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create save directory {}", parent.display()))?;

    let bytes = store.encode(state)?;
    let maximum = store.maximum_encoded_size();
    if bytes.len() > maximum {
        bail!(
            "{} store encoded {} bytes, exceeding its {maximum}-byte limit",
            store.name(),
            bytes.len()
        );
    }

    super::write_atomically(&path, &bytes)?;
    log::info!("saved slot {slot}");
    Ok(())
}

pub fn load_game(store: &dyn StoreAdapter, slot: u32, project_root: &Path) -> Result<SavedState> {
    let path = slot_path(store, project_root, slot);
    let bytes = super::read_limited(&path, store.maximum_encoded_size())
        .with_context(|| format!("failed to open save {}", path.display()))?;
    let state = store
        .decode(&bytes)
        .with_context(|| format!("failed to parse save {}", path.display()))?;
    log::info!("loaded slot {slot}");
    Ok(state)
}

/// Flushes the current game state before Bevy completes a graceful shutdown.
/// Window close, the in-game EXIT action and the first terminal Ctrl+C all
/// produce `AppExit`; title-screen exits intentionally preserve the previous
/// quick save instead of replacing it with an empty title state.
pub(crate) fn quick_save_on_exit(
    mut exits: MessageReader<AppExit>,
    state: Res<GameState>,
    project_root: Res<ProjectRoot>,
    store: Res<StoreCodec>,
    editor_sync: Option<Res<EditorSyncSession>>,
    persistence_disabled: Option<Res<PersistenceDisabled>>,
) {
    if editor_sync.is_some() || persistence_disabled.is_some() {
        return;
    }
    if exits.read().next().is_none() || state.ended {
        return;
    }
    if let Err(error) = save_game(store.0.as_ref(), &state, QUICK_SAVE_SLOT, &project_root) {
        log::error!("failed to quick-save during shutdown: {error:#}");
    } else {
        log::info!("quick-saved current game before shutdown");
    }
}

/// Reads only the small metadata prefix; the full state is untouched until load.
pub fn inspect_slot(store: &dyn StoreAdapter, slot: u32, project_root: &Path) -> SlotStatus {
    let path = slot_path(store, project_root, slot);
    match inspect_file(store, &path) {
        Ok(status) => status,
        Err(error) => {
            log::warn!("failed to inspect save {}: {error:#}", path.display());
            SlotStatus::Corrupt
        }
    }
}

pub fn preview_path(project_root: &Path, slot: u32) -> PathBuf {
    project_root.join("saves").join(format!("slot_{slot}.webp"))
}

/// Reads a save-card sidecar under the same compressed-input ceiling as every
/// other WebP path. Keeping this beside [`preview_path`] prevents UI callers
/// from accidentally allocating an untrusted sidecar before the decoder can
/// enforce its own limit.
pub(crate) fn read_preview(project_root: &Path, slot: u32) -> Result<Vec<u8>> {
    let path = preview_path(project_root, slot);
    super::read_limited(&path, keine_media::MAX_WEBP_FILE_BYTES)
        .with_context(|| format!("failed to read save preview {}", path.display()))
}

pub fn delete_game(store: &dyn StoreAdapter, slot: u32, project_root: &Path) -> Result<()> {
    for path in [
        slot_path(store, project_root, slot),
        preview_path(project_root, slot),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("failed to delete {}", path.display()));
            }
        }
    }
    log::info!("deleted slot {slot}");
    Ok(())
}

/// Deletes every manual and quick-save slot while preserving settings,
/// read-history and gallery data stored beside them.
pub fn clear_games(store: &dyn StoreAdapter, project_root: &Path) -> Result<()> {
    let directory = project_root.join("saves");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("failed to read save directory"),
    };
    let store_suffix = format!(".{}", store.extension());
    for entry in entries {
        let entry = entry.context("failed to inspect save directory entry")?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_slot =
            name.starts_with("slot_") && (name.ends_with(&store_suffix) || name.ends_with(".webp"));
        if is_slot {
            fs::remove_file(entry.path())
                .with_context(|| format!("failed to delete {}", entry.path().display()))?;
        }
    }
    log::info!("cleared all save slots");
    Ok(())
}

/// Deletes the complete project persistence directory, including save slots,
/// previews, settings, profile, read history, gallery and interrupted writes.
pub(crate) fn clear_all_data(project_root: &Path) -> Result<()> {
    let directory = project_root.join("saves");
    match fs::remove_dir_all(&directory) {
        Ok(()) => {
            log::info!("cleared all persistent project data");
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to delete save directory {}", directory.display())),
    }
}

fn inspect_file(store: &dyn StoreAdapter, path: &Path) -> Result<SlotStatus> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(SlotStatus::Empty),
        Err(error) => return Err(error.into()),
    };
    Ok(match store.inspect(&mut file)? {
        StoreStatus::Ready(metadata) => SlotStatus::Ready(metadata),
        StoreStatus::Corrupt => SlotStatus::Corrupt,
        StoreStatus::Unsupported(version) => SlotStatus::Unsupported(version),
    })
}

fn slot_path(store: &dyn StoreAdapter, project_root: &Path, slot: u32) -> PathBuf {
    project_root
        .join("saves")
        .join(format!("slot_{slot}.{}", store.extension()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use keine_loader::KeineStore;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("keine-save-{label}-{nonce}"))
    }

    fn sample_state() -> State {
        let mut state = State::new();
        state.current_scene = "demo".into();
        state.cursor = 42;
        state
    }

    #[test]
    fn round_trips_state_and_inspects_metadata() {
        let root = temp_root("round-trip");
        let state = sample_state();
        save_game(&KeineStore, &state, 3, &root).unwrap();

        assert_eq!(load_game(&KeineStore, 3, &root).unwrap().snapshot(), &state);
        assert!(
            matches!(inspect_slot(&KeineStore, 3, &root), SlotStatus::Ready(meta) if meta.scene == "demo")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_legacy_files_and_defers_state_integrity_to_load() {
        let root = temp_root("invalid");
        fs::create_dir_all(root.join("saves")).unwrap();
        fs::write(
            slot_path(&KeineStore, &root, 1),
            postcard::to_stdvec(&sample_state()).unwrap(),
        )
        .unwrap();
        save_game(&KeineStore, &sample_state(), 2, &root).unwrap();
        let mut bytes = fs::read(slot_path(&KeineStore, &root, 2)).unwrap();
        *bytes.last_mut().unwrap() ^= 0xff;
        fs::write(slot_path(&KeineStore, &root, 2), bytes).unwrap();

        assert_eq!(inspect_slot(&KeineStore, 1, &root), SlotStatus::Corrupt);
        assert!(matches!(
            inspect_slot(&KeineStore, 2, &root),
            SlotStatus::Ready(_)
        ));
        assert!(load_game(&KeineStore, 1, &root).is_err());
        assert!(load_game(&KeineStore, 2, &root).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_oversized_slots_before_reading_the_payload() {
        let root = temp_root("oversized");
        let path = slot_path(&KeineStore, &root, 1);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = File::create(&path).unwrap();
        file.set_len(KeineStore.maximum_encoded_size() as u64 + 1)
            .unwrap();

        let error = load_game(&KeineStore, 1, &root).unwrap_err();
        assert!(error.to_string().contains("failed to open save"));
        assert!(format!("{error:#}").contains("exceeding the"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_oversized_preview_sidecars_before_reading_the_payload() {
        let root = temp_root("oversized-preview");
        let path = preview_path(&root, 1);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = File::create(&path).unwrap();
        file.set_len(keine_media::MAX_WEBP_FILE_BYTES as u64 + 1)
            .unwrap();

        let error = read_preview(&root, 1).unwrap_err();
        assert!(format!("{error:#}").contains("exceeding the"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deletes_state_and_preview_together() {
        let root = temp_root("delete");
        save_game(&KeineStore, &sample_state(), 4, &root).unwrap();
        fs::write(preview_path(&root, 4), b"preview").unwrap();

        delete_game(&KeineStore, 4, &root).unwrap();

        assert_eq!(inspect_slot(&KeineStore, 4, &root), SlotStatus::Empty);
        assert!(!preview_path(&root, 4).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clears_slots_without_removing_settings_data() {
        let root = temp_root("clear");
        save_game(&KeineStore, &sample_state(), QUICK_SAVE_SLOT, &root).unwrap();
        save_game(&KeineStore, &sample_state(), 4, &root).unwrap();
        fs::write(preview_path(&root, 4), b"preview").unwrap();
        fs::write(root.join("saves/settings.bin"), b"settings").unwrap();

        clear_games(&KeineStore, &root).unwrap();

        assert_eq!(
            inspect_slot(&KeineStore, QUICK_SAVE_SLOT, &root),
            SlotStatus::Empty
        );
        assert_eq!(inspect_slot(&KeineStore, 4, &root), SlotStatus::Empty);
        assert!(root.join("saves/settings.bin").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn graceful_exit_quick_saves_only_during_gameplay() {
        let root = temp_root("exit");
        let mut state = sample_state();
        state.ended = false;
        let mut app = App::new();
        app.add_message::<AppExit>()
            .insert_resource(GameState(state.clone()))
            .insert_resource(ProjectRoot(root.clone()))
            .insert_resource(StoreCodec(Arc::new(KeineStore)))
            .add_systems(Last, quick_save_on_exit);

        app.world_mut().write_message(AppExit::Success);
        app.update();

        assert_eq!(
            load_game(&KeineStore, QUICK_SAVE_SLOT, &root)
                .unwrap()
                .snapshot(),
            &state
        );

        app.world_mut().resource_mut::<GameState>().ended = true;
        fs::remove_file(slot_path(&KeineStore, &root, QUICK_SAVE_SLOT)).unwrap();
        app.world_mut().write_message(AppExit::Success);
        app.update();
        assert_eq!(
            inspect_slot(&KeineStore, QUICK_SAVE_SLOT, &root),
            SlotStatus::Empty
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn editor_sync_exit_never_writes_into_the_source_project() {
        let root = temp_root("editor-sync-exit");
        let mut state = sample_state();
        state.ended = false;
        let mut app = App::new();
        app.add_message::<AppExit>()
            .insert_resource(GameState(state))
            .insert_resource(ProjectRoot(root.clone()))
            .insert_resource(StoreCodec(Arc::new(KeineStore)))
            .init_resource::<EditorSyncSession>()
            .add_systems(Last, quick_save_on_exit);

        app.world_mut().write_message(AppExit::Success);
        app.update();

        assert_eq!(
            inspect_slot(&KeineStore, QUICK_SAVE_SLOT, &root),
            SlotStatus::Empty
        );
        assert!(!root.join("saves").exists());
        let _ = fs::remove_dir_all(root);
    }
}
