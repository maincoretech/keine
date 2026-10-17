pub(crate) mod backup;
pub(crate) mod gallery;
mod persistence;
pub(crate) mod profile;
#[path = "storage/read/history.rs"]
pub(crate) mod read_history;
mod recovery;
pub(crate) mod save;
pub(crate) mod settings;

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::runtime::GameSystemSet;

pub(crate) struct StoragePlugin;

/// Pending disk work can wake a resting window without advancing the game clock.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct PersistenceWake(pub(crate) Option<std::time::Duration>);

fn update_write_deadline(
    profile: Res<profile::ProfileWriter>,
    history: Res<read_history::ReadHistoryWriter>,
    gallery: Res<gallery::GallerySnapshot>,
    mut wake: ResMut<PersistenceWake>,
) {
    let next = [
        profile
            .dirty_seconds
            .map(|elapsed| profile::WRITE_DELAY_SECONDS - elapsed),
        history
            .dirty_seconds
            .map(|elapsed| read_history::WRITE_DELAY_SECONDS - elapsed),
        gallery
            .retry_seconds
            .map(|elapsed| gallery::RETRY_DELAY_SECONDS - elapsed),
    ]
    .into_iter()
    .flatten()
    .map(|remaining| std::time::Duration::from_secs_f32(remaining.max(0.0)))
    .min();
    wake.set_if_neq(PersistenceWake(next));
}

pub(crate) use persistence::{prepare as prepare_persistence, root as persistence_root};

pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("persistent data path has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(match path.extension().and_then(|value| value.to_str()) {
        Some(extension) => format!("{extension}.tmp"),
        None => "tmp".to_owned(),
    });
    let mut file = File::create(&temporary)
        .with_context(|| format!("failed to create {}", temporary.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("failed to synchronize {}", temporary.display()))?;
    drop(file);
    fs::rename(&temporary, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    sync_directory(parent)?;
    Ok(())
}

pub(crate) fn read_limited(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let declared = file.metadata()?.len();
    if declared > maximum as u64 {
        bail!(
            "{} is {declared} bytes, exceeding the {maximum}-byte limit",
            path.display()
        );
    }
    let mut bytes = Vec::with_capacity(declared as usize);
    file.take((maximum as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        bail!("{} grew beyond the {maximum}-byte limit", path.display());
    }
    Ok(bytes)
}

/// Serialize a persistent postcard value while enforcing the same envelope
/// limit used by its reader. This prevents the runtime from writing a file it
/// will reject on the next launch.
pub(crate) fn encode_postcard_limited<T: Serialize>(
    value: &T,
    maximum: usize,
    label: &str,
) -> Result<Vec<u8>> {
    let bytes = postcard::to_stdvec(value)?;
    if bytes.len() > maximum {
        bail!(
            "encoded {label} is {} bytes, exceeding the {maximum}-byte limit",
            bytes.len()
        );
    }
    Ok(bytes)
}

pub(crate) fn decode_postcard_exact<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T> {
    let (value, remaining) = postcard::take_from_bytes(bytes)?;
    if !remaining.is_empty() {
        bail!("persistent data contains trailing bytes");
    }
    Ok(value)
}

pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

impl Plugin for StoragePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<gallery::GallerySnapshot>();
        app.init_resource::<PersistenceWake>();
        app.init_resource::<save::ContinuationCheckpoint>();
        app.init_resource::<save::SavePreviewCoordinator>();
        app.add_systems(
            Startup,
            (settings::load_settings, recovery::report_load_failures),
        );
        app.add_systems(
            Update,
            (
                (
                    read_history::persist_read_history,
                    gallery::persist,
                    profile::persist,
                ),
                update_write_deadline,
            )
                .chain()
                .in_set(GameSystemSet::Sync),
        );
        app.add_systems(
            Last,
            (
                save::quick_save_on_exit,
                profile::flush_on_exit,
                read_history::flush_on_exit,
                gallery::flush_on_exit,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn persistence_app(root: &Path) -> App {
        use crate::runtime::resources::{GameState, PersistenceRoot};
        let mut state = keine_core::State::new();
        state
            .global_vars
            .insert("ending".into(), keine_core::Value::Int(1));
        state.read_dialogues.insert(keine_core::state::DialogueKey {
            scene: "main".into(),
            action_index: 7,
            source_id: None,
        });
        state
            .unlocked_cg
            .insert("memory.webp".into(), "Memory".into());
        let mut app = App::new();
        app.insert_resource(Time::<Real>::default())
            .init_resource::<Time<Virtual>>()
            .init_resource::<PersistenceWake>()
            .insert_resource(GameState(state))
            .insert_resource(PersistenceRoot(root.to_owned()))
            .init_resource::<profile::ProfileWriter>()
            .init_resource::<read_history::ReadHistoryWriter>()
            .init_resource::<gallery::GallerySnapshot>()
            .add_message::<bevy::app::AppExit>()
            .add_systems(
                Update,
                (
                    (
                        profile::persist,
                        read_history::persist_read_history,
                        gallery::persist,
                    ),
                    update_write_deadline,
                )
                    .chain(),
            )
            .add_systems(
                Last,
                (
                    profile::flush_on_exit,
                    read_history::flush_on_exit,
                    gallery::flush_on_exit,
                ),
            );
        app
    }

    #[test]
    fn failed_background_writes_wait_and_retry_without_further_state_changes() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-persistence-retry-{nonce}"));
        fs::write(&root, b"block the data directory").unwrap();
        let mut app = persistence_app(&root);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(1));
        app.update();
        fs::remove_file(&root).unwrap();
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_millis(100));
        app.update();
        assert!(
            !root.exists(),
            "failed writes must not retry on the next frame"
        );
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(1));
        app.update();
        for file in ["profile.bin", "read_history.bin", "gallery.bin"] {
            assert!(
                root.join("saves").join(file).is_file(),
                "{file} retry was lost"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exit_flushes_profile_history_and_pending_gallery_before_the_write_delay() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-persistence-exit-{nonce}"));
        fs::write(&root, b"block the initial gallery write").unwrap();
        let mut app = persistence_app(&root);
        app.update();
        assert!(!root.join("saves/read_history.bin").exists());
        assert!(!root.join("saves/profile.bin").exists());
        fs::remove_file(&root).unwrap();
        app.world_mut().write_message(bevy::app::AppExit::Success);
        app.update();
        let state = app
            .world()
            .resource::<crate::runtime::resources::GameState>();
        assert_eq!(profile::load(&root), state.global_vars);
        assert_eq!(read_history::load(&root), state.read_dialogues);
        let mut loaded = keine_core::State::new();
        gallery::load(&mut loaded, &root);
        assert_eq!(loaded.unlocked_cg, state.unlocked_cg);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delayed_writes_finish_while_the_game_clock_is_paused() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-persistence-idle-{nonce}"));
        let mut app = persistence_app(&root);
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        app.update();
        assert_eq!(
            app.world().resource::<PersistenceWake>().0,
            Some(std::time::Duration::from_millis(500))
        );
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(1));
        app.update();
        assert_eq!(app.world().resource::<PersistenceWake>().0, None);
        assert!(app.world().resource::<Time<Virtual>>().elapsed().is_zero());
        let state = app
            .world()
            .resource::<crate::runtime::resources::GameState>();
        assert_eq!(profile::load(&root), state.global_vars);
        assert_eq!(read_history::load(&root), state.read_dialogues);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn atomic_write_replaces_existing_data_and_limited_read_rejects_oversize_files() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-atomic-storage-{nonce}"));
        let target = root.join("settings.bin");

        write_atomically(&target, b"old").unwrap();
        write_atomically(&target, b"replacement").unwrap();
        assert_eq!(read_limited(&target, 11).unwrap(), b"replacement");
        assert!(read_limited(&target, 10).is_err());
        assert!(!target.with_extension("bin.tmp").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn postcard_writer_enforces_the_reader_envelope_limit() {
        assert_eq!(
            encode_postcard_limited(&"small", 6, "test data").unwrap(),
            postcard::to_stdvec(&"small").unwrap()
        );
        let error = encode_postcard_limited(&"too large", 4, "test data").unwrap_err();
        assert!(error.to_string().contains("encoded test data"));
        let mut bytes = encode_postcard_limited(&"small", 6, "test data").unwrap();
        assert_eq!(decode_postcard_exact::<&str>(&bytes).unwrap(), "small");
        bytes.push(0);
        assert!(decode_postcard_exact::<&str>(&bytes).is_err());
    }
}
