pub(crate) mod backup;
pub(crate) mod gallery;
mod persistence;
pub(crate) mod profile;
#[path = "storage/read/history.rs"]
pub(crate) mod read_history;
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

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
        let mut bytes = encode_postcard_limited(&"small", 6, "test data").unwrap();
        assert_eq!(decode_postcard_exact::<&str>(&bytes).unwrap(), "small");
        bytes.push(0);
        assert!(decode_postcard_exact::<&str>(&bytes).is_err());
    }
}
