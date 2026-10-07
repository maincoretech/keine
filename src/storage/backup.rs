use std::collections::HashSet;
use std::fs;
#[cfg(any(target_os = "android", test))]
use std::io::Read;
use std::io::{ErrorKind, Write};
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
    super::write_atomically(target, &export_bytes(project_root)?)
}

#[cfg(any(target_os = "android", test))]
pub(crate) fn export_to_stream(project_root: &Path, mut target: impl Write) -> Result<()> {
    target.write_all(&export_bytes(project_root)?)?;
    target.flush()?;
    Ok(())
}

fn export_bytes(project_root: &Path) -> Result<Vec<u8>> {
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
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("save data contains a non-UTF-8 file name"))?;
                if !safe_name(&name) {
                    bail!("save data contains an unsafe file name");
                }
                let bytes = read_backup_file(&entry.path(), &mut total_bytes, MAX_BACKUP_BYTES)?;
                files.push(BackupFile { name, bytes });
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to open save data directory"),
    }
    files.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    validate_names(files.iter().map(|file| file.name.as_str()))?;
    let bytes = postcard::to_stdvec(&BackupBundle {
        version: VERSION,
        files,
    })?;
    if bytes.len() > MAX_BACKUP_BYTES {
        bail!("backup exceeds the {MAX_BACKUP_BYTES}-byte limit");
    }
    Ok(bytes)
}

pub(crate) fn import(project_root: &Path, source: &Path) -> Result<()> {
    // Never begin a second transaction by deleting its sibling directories.
    // A previous process may have died after moving the only complete old
    // save set to `saves.previous`.
    recover(project_root)?;

    let bytes = super::read_limited(source, MAX_BACKUP_BYTES)?;
    import_bytes(project_root, &bytes)
}

#[cfg(any(target_os = "android", test))]
pub(crate) fn import_from_stream(project_root: &Path, source: impl Read) -> Result<()> {
    recover(project_root)?;
    // Document providers can return pipes with no length or seek support.
    let mut bytes = Vec::new();
    source
        .take(MAX_BACKUP_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BACKUP_BYTES {
        bail!("backup exceeds the {MAX_BACKUP_BYTES}-byte limit");
    }
    import_bytes(project_root, &bytes)
}

fn import_bytes(project_root: &Path, bytes: &[u8]) -> Result<()> {
    let bundle: BorrowedBackupBundle<'_> =
        super::decode_postcard_exact(bytes).context("invalid backup file")?;
    if bundle.version != VERSION {
        bail!("unsupported backup version {}", bundle.version);
    }
    if bundle.files.len() > MAX_BACKUP_FILES {
        bail!("backup contains too many files");
    }
    validate_names(bundle.files.iter().map(|file| file.name))?;
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
    fs::create_dir_all(&incoming)?;
    for file in bundle.files {
        let path = incoming.join(file.name);
        // The entire incoming directory is uncommitted. Exclusive creation
        // detects filesystem aliases and avoids temporary-name collisions
        // between legitimate entries such as `slot` and `slot.tmp`.
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context("failed to create a unique imported save file")?;
        output.write_all(file.bytes)?;
        output.sync_all()?;
    }
    super::sync_directory(&incoming)?;
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

/// Recover an interrupted save-directory replacement before any persistence
/// domain is loaded or another import begins.
///
/// `saves` is the only committed name. If it is missing, an existing
/// `saves.previous` is restored before the uncommitted incoming directory is
/// touched. Every rename/removal is followed by a parent-directory sync on
/// platforms that support it: syncing file contents alone does not make the
/// directory entry changed by `rename` crash-durable.
pub(crate) fn recover(project_root: &Path) -> Result<()> {
    let target = project_root.join("saves");
    let incoming = sibling(&target, "saves.importing");
    let previous = sibling(&target, "saves.previous");
    let parent = target.parent().context("save directory has no parent")?;

    let target_exists = directory_exists(&target)?;
    let previous_exists = directory_exists(&previous)?;
    let incoming_exists = directory_exists(&incoming)?;

    if target_exists {
        // The committed name wins. A leftover previous directory means the
        // commit completed but cleanup did not; incoming is uncommitted.
        if incoming_exists {
            remove_and_sync(&incoming, parent)?;
        }
        if previous_exists {
            remove_and_sync(&previous, parent)?;
        }
        return Ok(());
    }

    if previous_exists {
        // Restore the only known committed copy first. In particular, do not
        // delete it just because a newer incoming directory also exists.
        fs::rename(&previous, &target)
            .context("failed to restore save data after an interrupted import")?;
        super::sync_directory(parent)?;
        if incoming_exists {
            remove_and_sync(&incoming, parent)?;
        }
        return Ok(());
    }

    if incoming_exists {
        // Without a durable ready/commit marker, an orphaned incoming tree
        // may be only partially written and must never be promoted.
        remove_and_sync(&incoming, parent)?;
    }
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
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|ch| ch.is_control() || "<>:\"/\\|?*".contains(ch))
    {
        return false;
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
}

fn validate_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let mut unique = HashSet::new();
    for name in names {
        if !safe_name(name) {
            bail!("backup contains an unsafe file name");
        }
        if !unique.insert(name.to_lowercase()) {
            bail!("backup contains conflicting file names");
        }
    }
    Ok(())
}

fn read_backup_file(path: &Path, total: &mut usize, maximum: usize) -> Result<Vec<u8>> {
    let remaining = maximum
        .checked_sub(*total)
        .context("backup size overflow")?;
    let bytes = super::read_limited(path, remaining.min(MAX_BACKUP_FILE_BYTES))?;
    *total += bytes.len();
    Ok(bytes)
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

fn remove_and_sync(path: &Path, parent: &Path) -> Result<()> {
    remove_if_present(path)?;
    super::sync_directory(parent)
}

fn directory_exists(path: &Path) -> Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => bail!(
            "save transaction path is not a directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("failed to inspect save transaction path {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-backup-{label}-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_save_set(path: &Path, marker: &[u8]) {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("marker"), marker).unwrap();
    }

    #[test]
    fn document_streams_and_desktop_files_share_the_backup_format() {
        let root = test_root("document-stream");
        write_save_set(&root.join("saves"), b"original");
        let mut bytes = Vec::new();
        export_to_stream(&root, &mut bytes).unwrap();
        let path = root.join("backup.keine-backup");
        fs::write(&path, &bytes).unwrap();
        write_save_set(&root.join("saves"), b"changed");
        import(&root, &path).unwrap();
        assert_eq!(fs::read(root.join("saves/marker")).unwrap(), b"original");

        export(&root, &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        write_save_set(&root.join("saves"), b"changed again");
        // A byte slice implements Read, but has no Seek or file metadata.
        import_from_stream(&root, bytes.as_slice()).unwrap();
        assert_eq!(fs::read(root.join("saves/marker")).unwrap(), b"original");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_document_reads_and_invalid_archives_preserve_saves() {
        struct BrokenProvider;
        impl Read for BrokenProvider {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("document provider disconnected"))
            }
        }
        let root = test_root("document-read-error");
        write_save_set(&root.join("saves.previous"), b"original");
        write_save_set(&root.join("saves.importing"), b"partial");
        assert!(import_from_stream(&root, BrokenProvider).is_err());
        assert_eq!(fs::read(root.join("saves/marker")).unwrap(), b"original");
        assert!(!root.join("saves.previous").exists());
        assert!(!root.join("saves.importing").exists());

        let mut bytes = Vec::new();
        export_to_stream(&root, &mut bytes).unwrap();
        for invalid in [&bytes[..bytes.len() - 1], b"invalid".as_slice()] {
            assert!(import_from_stream(&root, invalid).is_err());
            assert_eq!(fs::read(root.join("saves/marker")).unwrap(), b"original");
            assert!(!root.join("saves.importing").exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn document_export_reports_flush_failure() {
        struct BrokenProvider(Vec<u8>);
        impl Write for BrokenProvider {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("document provider disconnected"))
            }
        }
        let root = test_root("document-write-error");
        write_save_set(&root.join("saves"), b"original");
        assert!(export_to_stream(&root, BrokenProvider(Vec::new())).is_err());
        assert_eq!(fs::read(root.join("saves/marker")).unwrap(), b"original");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_accounts_for_actual_payloads_within_the_remaining_budget() {
        let root = test_root("budget");
        let first = root.join("first");
        let second = root.join("second");
        fs::write(&first, b"1234").unwrap();
        fs::write(&second, b"567").unwrap();
        let mut total = 0;
        assert_eq!(read_backup_file(&first, &mut total, 6).unwrap(), b"1234");
        assert_eq!(total, 4);
        assert!(read_backup_file(&second, &mut total, 6).is_err());
        assert_eq!(total, 4);
        fs::write(&second, b"56").unwrap();
        assert_eq!(read_backup_file(&second, &mut total, 6).unwrap(), b"56");
        assert_eq!(total, 6);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_backups_do_not_replace_existing_saves() {
        let root = test_root("invalid-names");
        write_save_set(&root.join("saves"), b"old");
        let source = root.join("backup");
        for names in [
            vec!["../outside"],
            vec!["C:outside"],
            vec!["slot:stream"],
            vec!["NUL.sav"],
            vec!["COM¹"],
            vec!["slot."],
            vec!["slot "],
            vec!["slot\0"],
            vec!["slot", "slot"],
            vec!["slot", "SLOT"],
        ] {
            let bytes = postcard::to_stdvec(&BackupBundle {
                version: VERSION,
                files: names
                    .into_iter()
                    .map(|name| BackupFile {
                        name: name.into(),
                        bytes: b"new".to_vec(),
                    })
                    .collect(),
            })
            .unwrap();
            fs::write(&source, bytes).unwrap();
            assert!(import(&root, &source).is_err());
            assert_eq!(fs::read(root.join("saves/marker")).unwrap(), b"old");
            assert!(!root.join("saves.importing").exists());
        }
        let mut bytes = postcard::to_stdvec(&BackupBundle {
            version: VERSION,
            files: vec![],
        })
        .unwrap();
        bytes.push(0);
        fs::write(&source, bytes).unwrap();
        assert!(import(&root, &source).is_err());
        assert_eq!(fs::read(root.join("saves/marker")).unwrap(), b"old");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_keeps_entries_that_overlap_atomic_temporary_names() {
        let root = test_root("temporary-names");
        let source = root.join("backup");
        fs::write(
            &source,
            postcard::to_stdvec(&BackupBundle {
                version: VERSION,
                files: vec![
                    BackupFile {
                        name: "slot.tmp".into(),
                        bytes: b"temporary".to_vec(),
                    },
                    BackupFile {
                        name: "slot".into(),
                        bytes: b"save".to_vec(),
                    },
                ],
            })
            .unwrap(),
        )
        .unwrap();
        import(&root, &source).unwrap();
        assert_eq!(fs::read(root.join("saves/slot.tmp")).unwrap(), b"temporary");
        assert_eq!(fs::read(root.join("saves/slot")).unwrap(), b"save");
        fs::remove_dir_all(root).unwrap();
    }

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
        fs::write(root.join("saves/slot_1.keine.tmp"), b"temporary").unwrap();

        super::export(&root, &export).unwrap();
        fs::remove_dir_all(root.join("saves")).unwrap();
        super::import(&root, &export).unwrap();

        assert_eq!(
            fs::read(root.join("saves/settings.bin")).unwrap(),
            b"settings"
        );
        assert_eq!(fs::read(root.join("saves/slot_1.keine")).unwrap(), b"save");
        assert_eq!(
            fs::read(root.join("saves/slot_1.keine.tmp")).unwrap(),
            b"temporary"
        );
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

    #[test]
    fn recovery_discards_an_uncommitted_incoming_tree() {
        let root = test_root("incoming");
        write_save_set(&root.join("saves"), b"old");
        write_save_set(&root.join("saves.importing"), b"partial");

        recover(&root).unwrap();

        assert_eq!(fs::read(root.join("saves/marker")).unwrap(), b"old");
        assert!(!root.join("saves.importing").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_restores_previous_before_discarding_incoming() {
        let root = test_root("previous-and-incoming");
        write_save_set(&root.join("saves.previous"), b"old");
        write_save_set(&root.join("saves.importing"), b"new");

        recover(&root).unwrap();

        assert_eq!(fs::read(root.join("saves/marker")).unwrap(), b"old");
        assert!(!root.join("saves.previous").exists());
        assert!(!root.join("saves.importing").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_keeps_committed_target_and_cleans_previous() {
        let root = test_root("committed");
        write_save_set(&root.join("saves"), b"new");
        write_save_set(&root.join("saves.previous"), b"old");

        recover(&root).unwrap();

        assert_eq!(fs::read(root.join("saves/marker")).unwrap(), b"new");
        assert!(!root.join("saves.previous").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_does_not_promote_orphaned_incoming_without_a_marker() {
        let root = test_root("orphaned-incoming");
        write_save_set(&root.join("saves.importing"), b"possibly-partial");

        recover(&root).unwrap();

        assert!(!root.join("saves").exists());
        assert!(!root.join("saves.importing").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_second_import_recovers_the_only_old_copy_before_decoding() {
        let root = test_root("second-import");
        write_save_set(&root.join("saves.previous"), b"old");
        write_save_set(&root.join("saves.importing"), b"interrupted-new");
        let invalid = root.join("invalid.keine-backup");
        fs::write(&invalid, b"invalid").unwrap();

        assert!(import(&root, &invalid).is_err());

        assert_eq!(fs::read(root.join("saves/marker")).unwrap(), b"old");
        assert!(!root.join("saves.previous").exists());
        assert!(!root.join("saves.importing").exists());
        let _ = fs::remove_dir_all(root);
    }
}
