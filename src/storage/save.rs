use std::collections::HashMap;
use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::{Context, Result, bail};
use bevy::app::AppExit;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use keine_core::{PersistenceSafety, State};
use keine_loader::{SavedState, StoreAdapter, StoreStatus};

use crate::runtime::resources::{
    EditorSyncSession, GameState, PersistenceDisabled, PersistenceRoot, StoreCodec,
};

pub const QUICK_SAVE_SLOT: u32 = 0;
pub use keine_loader::StoreMetadata as SaveMetadata;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SavePreviewGeneration {
    epoch: u64,
    slot: u64,
}

#[derive(Default)]
struct SavePreviewGenerations {
    epoch: u64,
    slots: HashMap<u32, u64>,
}

/// Serializes preview invalidation with the worker's final atomic commit.
/// Encoding remains outside the lock; only the short generation check and
/// rename are coordinated, so delete/clear/import cannot be followed by a
/// stale preview resurrection.
#[derive(Resource, Clone, Default)]
pub(crate) struct SavePreviewCoordinator(Arc<Mutex<SavePreviewGenerations>>);

impl SavePreviewCoordinator {
    fn lock(&self) -> MutexGuard<'_, SavePreviewGenerations> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn invalidate_slot(&self, slot: u32) -> SavePreviewGeneration {
        let mut generations = self.lock();
        let epoch = generations.epoch;
        let slot_generation = generations.slots.entry(slot).or_default();
        *slot_generation = slot_generation.wrapping_add(1);
        SavePreviewGeneration {
            epoch,
            slot: *slot_generation,
        }
    }

    pub(crate) fn invalidate_all(&self) {
        let mut generations = self.lock();
        generations.epoch = generations.epoch.wrapping_add(1);
        generations.slots.clear();
    }

    pub(crate) fn is_current(&self, slot: u32, generation: SavePreviewGeneration) -> bool {
        let generations = self.lock();
        current_preview_generation(&generations, slot) == generation
    }

    pub(crate) fn commit_if_current(
        &self,
        slot: u32,
        generation: SavePreviewGeneration,
        commit: impl FnOnce() -> Result<()>,
    ) -> Result<bool> {
        let generations = self.lock();
        if current_preview_generation(&generations, slot) != generation {
            return Ok(false);
        }
        commit()?;
        Ok(true)
    }
}

fn current_preview_generation(
    generations: &SavePreviewGenerations,
    slot: u32,
) -> SavePreviewGeneration {
    SavePreviewGeneration {
        epoch: generations.epoch,
        slot: generations.slots.get(&slot).copied().unwrap_or_default(),
    }
}

/// Latest exact continuation point kept only in RAM.
///
/// Capturing a checkpoint never touches the filesystem. Disk I/O still occurs
/// only for an explicit save, return-to-title continuation, or graceful exit.
#[derive(Resource, Default)]
pub(crate) struct ContinuationCheckpoint(Option<State>);

impl ContinuationCheckpoint {
    pub(crate) fn capture(&mut self, state: &State) {
        if !state.ended && state.persistence_safety().is_exact() {
            self.0 = Some(state.clone());
        }
    }

    pub(crate) fn reset(&mut self, state: &State) {
        self.0 = None;
        self.capture(state);
    }

    pub(crate) fn ensure_current_program(&mut self, state: &State) {
        let matches = self
            .0
            .as_ref()
            .is_some_and(|saved| saved.program_fingerprint == state.program_fingerprint);
        if !matches {
            self.reset(state);
        }
    }

    fn continuation<'a>(&'a self, live: &'a State) -> Option<ContinuationState<'a>> {
        if live.persistence_safety().is_exact() {
            return Some(ContinuationState::Live(live));
        }
        self.0
            .as_ref()
            .filter(|saved| saved.program_fingerprint == live.program_fingerprint)
            .map(ContinuationState::Checkpoint)
    }

    pub(crate) fn state_for_continuation<'a>(&'a self, live: &'a State) -> Option<&'a State> {
        match self.continuation(live)? {
            ContinuationState::Live(state) | ContinuationState::Checkpoint(state) => Some(state),
        }
    }
}

enum ContinuationState<'a> {
    Live(&'a State),
    Checkpoint(&'a State),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContinuationSave {
    Live,
    Checkpoint,
    Skipped,
}

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
    if let PersistenceSafety::ActiveTransient(hazard) = state.persistence_safety() {
        bail!("save is unavailable while {hazard:?} presentation is active");
    }
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

pub(crate) fn save_continuation(
    store: &dyn StoreAdapter,
    live: &State,
    checkpoint: &ContinuationCheckpoint,
    project_root: &Path,
) -> Result<ContinuationSave> {
    let Some(state) = checkpoint.continuation(live) else {
        return Ok(ContinuationSave::Skipped);
    };
    let (state, result) = match state {
        ContinuationState::Live(state) => (state, ContinuationSave::Live),
        ContinuationState::Checkpoint(state) => (state, ContinuationSave::Checkpoint),
    };
    save_game(store, state, QUICK_SAVE_SLOT, project_root)?;
    Ok(result)
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
#[derive(SystemParam)]
pub(crate) struct QuickSaveExitContext<'w> {
    state: Res<'w, GameState>,
    project_root: Res<'w, PersistenceRoot>,
    store: Res<'w, StoreCodec>,
    checkpoint: Res<'w, ContinuationCheckpoint>,
    previews: Res<'w, SavePreviewCoordinator>,
    editor_sync: Option<Res<'w, EditorSyncSession>>,
    persistence_disabled: Option<Res<'w, PersistenceDisabled>>,
}

pub(crate) fn quick_save_on_exit(mut exits: MessageReader<AppExit>, context: QuickSaveExitContext) {
    if context.editor_sync.is_some() || context.persistence_disabled.is_some() {
        return;
    }
    if exits.read().next().is_none() || context.state.ended {
        return;
    }
    let result = save_continuation(
        context.store.0.as_ref(),
        &context.state,
        &context.checkpoint,
        &context.project_root,
    );
    match &result {
        Ok(ContinuationSave::Live) => log::info!("quick-saved current game before shutdown"),
        Ok(ContinuationSave::Checkpoint) => {
            log::info!("quick-saved the last exact checkpoint before shutdown")
        }
        Ok(ContinuationSave::Skipped) => {
            log::warn!("kept the previous quick save because this session has no exact checkpoint")
        }
        Err(error) => log::error!("failed to quick-save during shutdown: {error:#}"),
    }
    if matches!(
        result,
        Ok(ContinuationSave::Live | ContinuationSave::Checkpoint)
    ) {
        context.previews.invalidate_slot(QUICK_SAVE_SLOT);
        if let Err(error) = remove_preview(&context.project_root, QUICK_SAVE_SLOT) {
            log::warn!("failed to invalidate stale quick-save preview: {error:#}");
        }
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

pub(crate) fn remove_preview(project_root: &Path, slot: u32) -> Result<()> {
    let path = preview_path(project_root, slot);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to delete preview {}", path.display()))
        }
    }
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
    let path = slot_path(store, project_root, slot);
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("failed to delete {}", path.display()));
        }
    }
    remove_preview(project_root, slot)?;
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

    fn state_with_active_video(cursor: usize) -> State {
        let mut state = sample_state();
        state.cursor = cursor;
        state.videos.insert(
            "opening".into(),
            keine_core::VideoState {
                spec: keine_core::VideoSpec {
                    id: "opening".into(),
                    file: "video/opening.mp4".into(),
                    looped: false,
                    muted: false,
                    alpha: 1.0,
                    skippable: true,
                    wait_for_finished: false,
                    mode: keine_core::VideoMode::Fullscreen,
                },
                revision: 1,
                elapsed: 0.5,
                opacity: 1.0,
                stopping: false,
                fade_out: 0.0,
            },
        );
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
    fn manual_save_rejects_active_transient_presentation() {
        let root = temp_root("unsafe-manual");
        let error = save_game(&KeineStore, &state_with_active_video(50), 1, &root).unwrap_err();

        assert!(format!("{error:#}").contains("Video"));
        assert_eq!(inspect_slot(&KeineStore, 1, &root), SlotStatus::Empty);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn continuation_checkpoint_is_memory_only_until_a_real_save() {
        let root = temp_root("checkpoint");
        let stable = sample_state();
        let mut checkpoint = ContinuationCheckpoint::default();
        checkpoint.capture(&stable);

        assert!(
            !root.exists(),
            "RAM checkpoint capture must not touch storage"
        );
        assert_eq!(
            save_continuation(
                &KeineStore,
                &state_with_active_video(50),
                &checkpoint,
                &root,
            )
            .unwrap(),
            ContinuationSave::Checkpoint
        );
        assert_eq!(
            load_game(&KeineStore, QUICK_SAVE_SLOT, &root)
                .unwrap()
                .snapshot()
                .cursor,
            stable.cursor
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_checkpoint_never_overwrites_an_existing_continuation() {
        let root = temp_root("checkpoint-missing");
        let stable = sample_state();
        save_game(&KeineStore, &stable, QUICK_SAVE_SLOT, &root).unwrap();

        assert_eq!(
            save_continuation(
                &KeineStore,
                &state_with_active_video(50),
                &ContinuationCheckpoint::default(),
                &root,
            )
            .unwrap(),
            ContinuationSave::Skipped
        );
        assert_eq!(
            load_game(&KeineStore, QUICK_SAVE_SLOT, &root)
                .unwrap()
                .snapshot()
                .cursor,
            stable.cursor
        );
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
    fn invalidated_preview_job_cannot_resurrect_a_deleted_slot() {
        let root = temp_root("preview-delete-race");
        let coordinator = SavePreviewCoordinator::default();
        let stale = coordinator.invalidate_slot(4);
        coordinator.invalidate_slot(4);
        delete_game(&KeineStore, 4, &root).unwrap();

        let committed = coordinator
            .commit_if_current(4, stale, || {
                fs::create_dir_all(root.join("saves"))?;
                fs::write(preview_path(&root, 4), b"stale")?;
                Ok(())
            })
            .unwrap();

        assert!(!committed);
        assert!(!preview_path(&root, 4).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn queue_loss_leaves_no_preview_from_the_previous_save() {
        let root = temp_root("preview-queue-loss");
        let coordinator = SavePreviewCoordinator::default();
        let old = coordinator.invalidate_slot(4);
        assert!(
            coordinator
                .commit_if_current(4, old, || {
                    fs::create_dir_all(root.join("saves"))?;
                    fs::write(preview_path(&root, 4), b"old")?;
                    Ok(())
                })
                .unwrap()
        );

        coordinator.invalidate_slot(4);
        remove_preview(&root, 4).unwrap();

        assert!(!preview_path(&root, 4).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_generation_invalidates_every_pending_slot() {
        let root = temp_root("preview-import-race");
        let coordinator = SavePreviewCoordinator::default();
        let first = coordinator.invalidate_slot(1);
        let second = coordinator.invalidate_slot(2);
        coordinator.invalidate_all();

        for (slot, generation) in [(1, first), (2, second)] {
            assert!(
                !coordinator
                    .commit_if_current(slot, generation, || {
                        fs::create_dir_all(root.join("saves"))?;
                        fs::write(preview_path(&root, slot), b"stale")?;
                        Ok(())
                    })
                    .unwrap()
            );
        }
        assert!(!root.join("saves").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn graceful_exit_quick_saves_only_during_gameplay() {
        let root = temp_root("exit");
        let mut state = sample_state();
        state.ended = false;
        fs::create_dir_all(root.join("saves")).unwrap();
        fs::write(preview_path(&root, QUICK_SAVE_SLOT), b"old-preview").unwrap();
        let mut app = App::new();
        app.add_message::<AppExit>()
            .insert_resource(GameState(state.clone()))
            .insert_resource(ContinuationCheckpoint::default())
            .insert_resource(PersistenceRoot(root.clone()))
            .insert_resource(StoreCodec(Arc::new(KeineStore)))
            .init_resource::<SavePreviewCoordinator>()
            .add_systems(Last, quick_save_on_exit);

        app.world_mut().write_message(AppExit::Success);
        app.update();

        assert_eq!(
            load_game(&KeineStore, QUICK_SAVE_SLOT, &root)
                .unwrap()
                .snapshot(),
            &state
        );
        assert!(!preview_path(&root, QUICK_SAVE_SLOT).exists());

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
            .insert_resource(ContinuationCheckpoint::default())
            .insert_resource(PersistenceRoot(root.clone()))
            .insert_resource(StoreCodec(Arc::new(KeineStore)))
            .init_resource::<SavePreviewCoordinator>()
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
