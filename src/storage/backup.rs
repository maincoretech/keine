use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const VERSION: u32 = 2;
// The postcard V2 envelope is encoded in one pass, so export temporarily holds
// source bytes and the serialized buffer together. Keep the accepted envelope
// small enough for that bounded peak; import borrows each payload in place.
const MAX_BACKUP_BYTES: usize = 128 * 1024 * 1024;
const MAX_BACKUP_FILES: usize = 4_096;
const MAX_BACKUP_FILE_BYTES: usize = 72 * 1024 * 1024;

#[derive(Serialize)]
struct BackupBundle {
    version: u32,
    files: Vec<BackupFile>,
}

#[derive(Serialize)]
struct BackupFile {
    name: String,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
struct BorrowedBackupBundle<'a> {
    version: u32,
    #[serde(borrow)]
    files: Vec<BorrowedBackupFile<'a>>,
}

#[derive(Deserialize)]
struct BorrowedBackupFile<'a> {
    #[serde(borrow)]
    name: &'a str,
    #[serde(borrow)]
    bytes: &'a [u8],
}

pub(crate) fn export(project_root: &Path, target: &Path) -> Result<()> {
    let directory = project_root.join("saves");
    let mut files = Vec::new();
    let mut total_bytes = 0usize;
    match fs::read_dir(&directory) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.context("failed to inspect save data")?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                if files.len() >= MAX_BACKUP_FILES {
                    bail!("save data contains too many files");
                }
                let file_size = usize::try_from(entry.metadata()?.len())
                    .context("save data file size exceeds this platform")?;
                total_bytes = total_bytes
                    .checked_add(file_size)
                    .context("backup size overflow")?;
                if total_bytes > MAX_BACKUP_BYTES {
                    bail!("save data exceeds the {MAX_BACKUP_BYTES}-byte backup limit");
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                files.push(BackupFile {
                    name,
                    bytes: super::read_limited(&entry.path(), MAX_BACKUP_FILE_BYTES)?,
                });
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to open save data directory"),
    }
    files.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    let bytes = postcard::to_stdvec(&BackupBundle {
        version: VERSION,
        files,
    })?;
    if bytes.len() > MAX_BACKUP_BYTES {
        bail!("backup exceeds the {MAX_BACKUP_BYTES}-byte limit");
    }
    super::write_atomically(target, &bytes)
}

pub(crate) fn import(project_root: &Path, source: &Path) -> Result<()> {
    let bytes = super::read_limited(source, MAX_BACKUP_BYTES)?;
    let bundle: BorrowedBackupBundle<'_> =
        postcard::from_bytes(&bytes).context("invalid backup file")?;
    if bundle.version != VERSION {
        bail!("unsupported backup version {}", bundle.version);
    }
    if bundle.files.len() > MAX_BACKUP_FILES {
        bail!("backup contains too many files");
    }
    if bundle.files.iter().any(|file| !safe_name(file.name)) {
        bail!("backup contains an unsafe file name");
    }
    if bundle
        .files
        .iter()
        .any(|file| file.bytes.len() > MAX_BACKUP_FILE_BYTES)
    {
        bail!("backup contains an oversized file");
    }

    let target = project_root.join("saves");
    let incoming = sibling(&target, "saves.importing");
    let previous = sibling(&target, "saves.previous");
    remove_if_present(&incoming)?;
    remove_if_present(&previous)?;
    fs::create_dir_all(&incoming)?;
    for file in bundle.files {
        super::write_atomically(&incoming.join(file.name), file.bytes)?;
    }
    let parent = target.parent().context("save directory has no parent")?;
    if target.exists() {
        fs::rename(&target, &previous)?;
        super::sync_directory(parent)?;
    }
    if let Err(error) = fs::rename(&incoming, &target) {
        if previous.exists() {
            let _ = fs::rename(&previous, &target);
            let _ = super::sync_directory(parent);
        }
        return Err(error).context("failed to install imported save data");
    }
    super::sync_directory(parent)?;
    cleanup_previous_after_commit(&previous, parent);
    Ok(())
}

fn cleanup_previous_after_commit(previous: &Path, parent: &Path) {
    if let Err(error) = remove_if_present(previous).and_then(|()| super::sync_directory(parent)) {
        log::warn!(
            "save import committed, but the previous save directory could not be cleaned up: {error:#}"
        );
    }
}

fn safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

fn sibling(path: &Path, name: &str) -> PathBuf {
    path.parent().unwrap_or_else(|| Path::new(".")).join(name)
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn round_trips_flat_save_data() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-backup-{nonce}"));
        let export = root.join("backup.keine-backup");
        fs::create_dir_all(root.join("saves")).unwrap();
        fs::write(root.join("saves/settings.bin"), b"settings").unwrap();
        fs::write(root.join("saves/slot_1.keine"), b"save").unwrap();

        super::export(&root, &export).unwrap();
        fs::remove_dir_all(root.join("saves")).unwrap();
        super::import(&root, &export).unwrap();

        assert_eq!(
            fs::read(root.join("saves/settings.bin")).unwrap(),
            b"settings"
        );
        assert_eq!(fs::read(root.join("saves/slot_1.keine")).unwrap(), b"save");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_failure_does_not_turn_a_committed_import_into_an_error() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-backup-cleanup-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        let previous = root.join("saves.previous");
        fs::write(&previous, b"not a directory").unwrap();

        cleanup_previous_after_commit(&previous, &root);

        assert!(previous.is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn import_decoder_borrows_file_payloads_from_the_bounded_input() {
        let bytes = postcard::to_stdvec(&BackupBundle {
            version: VERSION,
            files: vec![BackupFile {
                name: "slot_1.keine".into(),
                bytes: b"save payload".to_vec(),
            }],
        })
        .unwrap();
        let bundle: BorrowedBackupBundle<'_> = postcard::from_bytes(&bytes).unwrap();
        let payload = bundle.files[0].bytes;
        let input = bytes.as_ptr_range();

        assert!(payload.as_ptr() >= input.start);
        assert!(payload.as_ptr() < input.end);
        assert_eq!(payload, b"save payload");
    }
}
