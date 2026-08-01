use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
struct BackupBundle {
    version: u32,
    files: Vec<BackupFile>,
}

#[derive(Serialize, Deserialize)]
struct BackupFile {
    name: String,
    bytes: Vec<u8>,
}

pub(crate) fn export(project_root: &Path, target: &Path) -> Result<()> {
    let directory = project_root.join("saves");
    let mut files = Vec::new();
    match fs::read_dir(&directory) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.context("failed to inspect save data")?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                files.push(BackupFile {
                    name,
                    bytes: fs::read(entry.path())?,
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
    let temporary = target.with_extension("tmp");
    fs::write(&temporary, bytes)?;
    #[cfg(windows)]
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::rename(&temporary, target)?;
    Ok(())
}

pub(crate) fn import(project_root: &Path, source: &Path) -> Result<()> {
    let bytes = fs::read(source)?;
    let bundle: BackupBundle = postcard::from_bytes(&bytes).context("invalid backup file")?;
    if bundle.version != VERSION {
        bail!("unsupported backup version {}", bundle.version);
    }
    if bundle.files.iter().any(|file| !safe_name(&file.name)) {
        bail!("backup contains an unsafe file name");
    }

    let target = project_root.join("saves");
    let incoming = sibling(&target, "saves.importing");
    let previous = sibling(&target, "saves.previous");
    remove_if_present(&incoming)?;
    remove_if_present(&previous)?;
    fs::create_dir_all(&incoming)?;
    for file in bundle.files {
        fs::write(incoming.join(file.name), file.bytes)?;
    }
    if target.exists() {
        fs::rename(&target, &previous)?;
    }
    if let Err(error) = fs::rename(&incoming, &target) {
        if previous.exists() {
            let _ = fs::rename(&previous, &target);
        }
        return Err(error).context("failed to install imported save data");
    }
    remove_if_present(&previous)?;
    Ok(())
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
}
