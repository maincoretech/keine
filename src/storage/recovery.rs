//! Preserve unreadable settings/gallery before replacing them. No per-frame IO.
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

pub(super) fn read_optional<T>(
    path: &Path,
    maximum: usize,
    decode: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<Option<T>> {
    match super::read_limited(path, maximum) {
        Ok(bytes) => decode(&bytes).map(Some),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn write_preserving<T>(
    path: &Path,
    bytes: &[u8],
    maximum: usize,
    decode: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<()> {
    if read_optional(path, maximum, decode).is_err() {
        // Linking keeps the original bytes without reading an oversized file into memory.
        // Never overwrite an older recovery copy. If preservation fails, do not write.
        let metadata = fs::symlink_metadata(path)?;
        anyhow::ensure!(
            metadata.file_type().is_file(),
            "persistent data is not a regular file"
        );
        let mut preserved = false;
        for index in 1..=100 {
            let mut name = path.as_os_str().to_os_string();
            name.push(format!(".recovery-{index}"));
            match fs::hard_link(path, Path::new(&name)) {
                Ok(()) => {
                    super::sync_directory(path.parent().context("missing data directory")?)?;
                    preserved = true;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error).context("could not preserve unreadable data"),
            }
        }
        anyhow::ensure!(preserved, "no free recovery filename; original data kept");
    }
    super::write_atomically(path, bytes)
}

pub(super) fn report_load_failures(
    root: bevy::prelude::Res<crate::runtime::resources::PersistenceRoot>,
    mut commands: bevy::prelude::Commands,
    editor: Option<bevy::prelude::Res<crate::runtime::resources::EditorSyncSession>>,
    preview: Option<bevy::prelude::Res<crate::runtime::preview::AuthoringPreviewSession>>,
    disabled: Option<bevy::prelude::Res<crate::runtime::resources::PersistenceDisabled>>,
) {
    if !crate::runtime::resources::writable_runtime_session(
        editor.is_some(),
        preview.is_some(),
        disabled.is_some(),
    ) {
        return;
    }
    for result in [super::settings::check(&root), super::gallery::check(&root)] {
        if let Err(error) = result {
            log::warn!("persistent data could not be loaded: {error:#}");
            crate::ui::dialog::report_data_read_failure(&mut commands);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_invalid_data_and_refuses_unpreservable_targets() {
        let root = std::env::temp_dir().join(format!(
            "keine-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.bin");
        let decode = |bytes: &[u8]| -> Result<()> {
            anyhow::ensure!(bytes == b"valid", "invalid");
            Ok(())
        };
        assert!(read_optional(&path, 10, decode).unwrap().is_none());
        for original in [b"broken".as_slice(), b"oversized original".as_slice()] {
            fs::write(&path, original).unwrap();
            write_preserving(&path, b"valid", 10, decode).unwrap();
        }
        assert_eq!(
            fs::read(root.join("settings.bin.recovery-1")).unwrap(),
            b"broken"
        );
        assert_eq!(
            fs::read(root.join("settings.bin.recovery-2")).unwrap(),
            b"oversized original"
        );
        assert_eq!(fs::read(&path).unwrap(), b"valid");
        fs::write(&path, b"keep me").unwrap();
        for index in 3..=100 {
            fs::write(
                root.join(format!("settings.bin.recovery-{index}")),
                b"older recovery",
            )
            .unwrap();
        }
        assert!(write_preserving(&path, b"valid", 10, decode).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"keep me");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(write_preserving(&path, b"valid", 10, decode).is_err());
        assert!(path.is_dir());
        fs::remove_dir_all(root).unwrap();
    }
}
