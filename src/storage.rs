pub(crate) mod backup;
pub(crate) mod gallery;
pub(crate) mod profile;
pub(crate) mod read_history;
pub(crate) mod save;
pub(crate) mod settings;

use std::path::Path;

use anyhow::Result;
use bevy::prelude::*;
use keine_core::State;

use crate::runtime::GameSystemSet;

pub(crate) struct StoragePlugin;

impl Plugin for StoragePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<gallery::GallerySnapshot>();
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
