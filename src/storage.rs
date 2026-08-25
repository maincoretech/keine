pub(crate) mod backup;
pub(crate) mod gallery;
mod persistence;
pub(crate) mod profile;
pub(crate) mod read_history;
pub(crate) mod save;
pub(crate) mod settings;

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use bevy::prelude::*;
use keine_core::State;
use serde::Serialize;

use crate::runtime::GameSystemSet;

pub(crate) struct StoragePlugin;

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
        app.init_resource::<save::ContinuationCheckpoint>();
        app.init_resource::<save::SavePreviewCoordinator>();
        app.add_systems(Startup, settings::load_settings);
        app.add_systems(
            Update,
            (
                read_history::persist_read_history,
                gallery::persist,
                profile::persist,
            )
                .in_set(GameSystemSet::Sync),
        );
        app.add_systems(Last, (save::quick_save_on_exit, profile::flush_on_exit));
    }
}

/// Clear every project-owned persistent data domain and synchronize the
/// in-memory persistence caches so the next update cannot recreate stale data.
pub(crate) fn reset_all(
    project_root: &Path,
    state: &mut State,
    settings: &mut settings::RuntimeSettings,
    profile_writer: &mut profile::ProfileWriter,
    read_history_writer: &mut read_history::ReadHistoryWriter,
    gallery_snapshot: &mut gallery::GallerySnapshot,
) -> Result<()> {
    settings::reset_memory(settings);
    profile::reset_memory(state, profile_writer);
    read_history::reset_memory(state, read_history_writer);
    gallery::reset_memory(state, gallery_snapshot);
    save::clear_all_data(project_root)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use keine_core::Value;
    use keine_core::state::DialogueKey;

    use super::*;

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
    }

    #[test]
    fn reset_all_clears_disk_runtime_state_and_writer_caches() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-reset-all-{nonce}"));
        let saves = root.join("saves");
        fs::create_dir_all(&saves).unwrap();
        for name in [
            "slot_0.keine",
            "slot_0.webp",
            "slot_9.legacy-store",
            "settings.bin",
            "profile.bin",
            "read_history.bin",
            "gallery.bin",
            "interrupted-write.tmp",
        ] {
            fs::write(saves.join(name), name).unwrap();
        }

        let mut state = State::new();
        state.global_vars.insert("ending".into(), Value::Int(2));
        state.read_dialogues.insert(DialogueKey {
            scene: "main".into(),
            action_index: 7,
        });
        state
            .unlocked_cg
            .insert("memory.webp".into(), "Memory".into());
        state
            .unlocked_bgm
            .insert("theme.opus".into(), "Theme".into());

        let mut settings = settings::RuntimeSettings {
            master_volume: 0.25,
            fullscreen: true,
            skip_all: true,
            ..Default::default()
        };
        let mut profile_writer = profile::ProfileWriter {
            saved: HashMap::from([("ending".into(), Value::Int(2))]),
            dirty_seconds: 0.4,
        };
        let mut read_history_writer = read_history::ReadHistoryWriter {
            saved_len: 1,
            dirty_seconds: 0.8,
        };
        let mut gallery_snapshot = gallery::GallerySnapshot {
            cg: HashMap::from([("memory.webp".into(), "Memory".into())]),
            bgm: HashMap::from([("theme.opus".into(), "Theme".into())]),
        };

        reset_all(
            &root,
            &mut state,
            &mut settings,
            &mut profile_writer,
            &mut read_history_writer,
            &mut gallery_snapshot,
        )
        .unwrap();

        assert!(!saves.exists());
        assert!(state.global_vars.is_empty());
        assert!(state.read_dialogues.is_empty());
        assert!(state.unlocked_cg.is_empty());
        assert!(state.unlocked_bgm.is_empty());
        assert_eq!(settings, settings::RuntimeSettings::default());
        assert!(profile_writer.saved.is_empty());
        assert_eq!(profile_writer.dirty_seconds, 0.0);
        assert_eq!(read_history_writer.saved_len, 0);
        assert_eq!(read_history_writer.dirty_seconds, 0.0);
        assert!(gallery_snapshot.cg.is_empty());
        assert!(gallery_snapshot.bgm.is_empty());

        // Reset is idempotent, and ordinary atomic persistence recreates the
        // directory after CLEAR ALL without special recovery code.
        reset_all(
            &root,
            &mut state,
            &mut settings,
            &mut profile_writer,
            &mut read_history_writer,
            &mut gallery_snapshot,
        )
        .unwrap();
        settings::persist(&settings, &root).unwrap();
        assert!(saves.join("settings.bin").is_file());

        let _ = fs::remove_dir_all(root);
    }
}
