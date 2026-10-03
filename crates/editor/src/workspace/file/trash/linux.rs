//! freedesktop Trash: paired files/info entries, same-volume moves, no deletion fallback.
use super::super::{NEXT_TEMP, invalid, move_without_overwrite};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::{
    ffi::OsStrExt,
    fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
};
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
pub(in super::super) fn trash(path: &Path) -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| invalid("HOME is unavailable"))?;
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    fs::create_dir_all(&data)?;
    let device = fs::metadata(path)?.dev();
    let (root, original) = if fs::metadata(&data)?.dev() == device {
        (data.join("Trash"), path.to_owned())
    } else {
        // Find this volume's mount root. A private per-user trash is permitted
        // when the shared .Trash directory is absent or unsuitable.
        let mut mount = path.parent().ok_or_else(|| invalid("No source parent"))?;
        while let Some(parent) = mount.parent() {
            if fs::metadata(parent)?.dev() != device {
                break;
            }
            mount = parent;
        }
        // SAFETY: getuid has no arguments or side effects.
        let uid = unsafe { libc::getuid() };
        (
            mount.join(format!(".Trash-{uid}")),
            path.strip_prefix(mount)
                .map_err(|error| invalid(error.to_string()))?
                .to_owned(),
        )
    };
    trash_at(path, &root, &original)
}

fn private_directory(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700))?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(path)?;
    // SAFETY: getuid is a process-local query without pointer arguments.
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::getuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unsafe Trash directory",
        ));
    }
    Ok(())
}

fn trash_at(path: &Path, root: &Path, original: &Path) -> io::Result<PathBuf> {
    private_directory(root)?;
    let files = root.join("files");
    let info = root.join("info");
    private_directory(&files)?;
    private_directory(&info)?;
    let name = path.file_name().ok_or_else(|| invalid("No file name"))?;
    let record = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        escaped(original),
        deletion_date()?
    );
    for _ in 0..128 {
        let nonce = NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut unique = name.to_os_string();
        unique.push(format!(".{}-{nonce}", std::process::id()));
        let destination = files.join(&unique);
        unique.push(".trashinfo");
        let metadata_path = info.join(unique);
        let mut metadata = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&metadata_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let result = metadata
            .write_all(record.as_bytes())
            .and_then(|()| metadata.sync_all())
            .and_then(|()| move_without_overwrite(path, &destination));
        if let Err(error) = result {
            let _ = fs::remove_file(&metadata_path);
            if error.kind() == io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(error);
        }
        return Ok(destination);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "No unused Trash slot",
    ))
}

pub(in super::super) fn restore(receipt: &Path, destination: &Path) -> io::Result<()> {
    let files = receipt
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "files"))
        .ok_or_else(|| invalid("Invalid Trash receipt"))?;
    let root = files
        .parent()
        .ok_or_else(|| invalid("Invalid Trash root"))?;
    private_directory(root)?;
    private_directory(files)?;
    private_directory(&root.join("info"))?;
    let mut name = receipt
        .file_name()
        .ok_or_else(|| invalid("Invalid Trash name"))?
        .to_os_string();
    name.push(".trashinfo");
    let metadata = root.join("info").join(name);
    if !fs::symlink_metadata(&metadata)?.file_type().is_file() {
        return Err(invalid("Invalid Trash metadata"));
    }
    move_without_overwrite(receipt, destination)?;
    // The file is already restored: a metadata cleanup error must not cause
    // the source/manifest transaction to roll back a successful restore.
    if let Err(error) = fs::remove_file(metadata) {
        eprintln!("Trash metadata cleanup failed: {error}");
    }
    Ok(())
}

fn escaped(path: &Path) -> String {
    let mut output = String::new();
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            output.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(output, "%{byte:02X}").expect("write to String");
        }
    }
    output
}

fn deletion_date() -> io::Result<String> {
    // SAFETY: time writes no pointer here; localtime_r receives valid aligned
    // buffers; strftime is bounded by the output array and uses a static format.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut local = std::mem::zeroed();
        if libc::localtime_r(&now, &mut local).is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut result = [0u8; 32];
        let count = libc::strftime(
            result.as_mut_ptr().cast(),
            result.len(),
            c"%Y-%m-%dT%H:%M:%S".as_ptr(),
            &local,
        );
        if count == 0 {
            return Err(io::Error::other("Trash timestamp unavailable"));
        }
        String::from_utf8(result[..count].to_vec()).map_err(|error| invalid(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paired_metadata_collision_and_restore_preserve_content() {
        let root = std::env::temp_dir().join(format!("keine-trash-spec-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("含 空格%.txt");
        fs::write(&source, b"original").unwrap();
        let receipt = trash_at(&source, &root.join("Trash"), &source).unwrap();
        let info = root.join("Trash/info");
        let entry = fs::read_dir(&info).unwrap().next().unwrap().unwrap().path();
        let metadata = fs::read_to_string(&entry).unwrap();
        assert!(metadata.contains("%E5%90%AB%20%E7%A9%BA%E6%A0%BC%25.txt"));
        assert!(metadata.contains("DeletionDate="));
        fs::write(&source, b"replacement").unwrap();
        assert!(restore(&receipt, &source).is_err());
        assert!(entry.is_file());
        assert_eq!(fs::read(&source).unwrap(), b"replacement");
        fs::remove_file(&source).unwrap();
        restore(&receipt, &source).unwrap();
        assert!(!entry.exists());
        assert_eq!(fs::read(&source).unwrap(), b"original");
        let receipt = trash_at(&source, &root.join("Trash"), &source).unwrap();
        restore(&receipt, &source).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn refuses_symlink_or_shared_trash_without_moving_source() {
        let root = std::env::temp_dir().join(format!("keine-trash-safety-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::write(&source, b"keep").unwrap();
        std::os::unix::fs::symlink(&root, root.join("Trash")).unwrap();
        assert!(trash_at(&source, &root.join("Trash"), &source).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"keep");
        fs::remove_file(root.join("Trash")).unwrap();
        fs::create_dir(root.join("Trash")).unwrap();
        fs::set_permissions(root.join("Trash"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(trash_at(&source, &root.join("Trash"), &source).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
