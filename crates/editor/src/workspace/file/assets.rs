//! Resource file changes paired with source transactions; never permanently delete.
use super::*;

#[derive(Clone, Debug)]
pub struct AssetMediaInfo {
    pub bytes: u64,
    pub modified: Option<std::time::SystemTime>,
    pub duration: Option<f64>,
    /// Bounded header probe, not a full decode or a conversion history claim.
    pub canonical_format: Option<&'static str>,
}

pub fn asset_media_info(root: &Path, path: &Path) -> Option<AssetMediaInfo> {
    use std::io::{Seek, SeekFrom};
    let absolute = confined_existing(root, path).ok()?;
    let metadata = fs::metadata(&absolute).ok()?;
    let format_path = mapped_path(path).unwrap_or_else(|| path.to_owned());
    let extension = extension(&format_path);
    if !matches!(
        extension.as_str(),
        "webp" | "mp4" | "m4v" | "ogg" | "opus" | "wav"
    ) {
        return Some(AssetMediaInfo {
            bytes: metadata.len(),
            modified: metadata.modified().ok(),
            duration: None,
            canonical_format: None,
        });
    }
    let mut file = File::open(absolute).ok()?;
    let limit = match extension.as_str() {
        "webp" => 32,
        "mp4" | "m4v" => PROBE_BYTES as u64,
        _ => 65_536,
    };
    let mut head = vec![0; metadata.len().min(limit) as usize];
    file.read_exact(&mut head).ok()?;
    let canonical_format = match extension.as_str() {
        "webp"
            if head.starts_with(b"RIFF")
                && head.get(8..12) == Some(b"WEBP")
                && matches!(head.get(12..16), Some(b"VP8 " | b"VP8L" | b"VP8X")) =>
        {
            Some("WebP")
        }
        "ogg" | "opus" if head.starts_with(b"OggS") && contains_marker(&head, b"OpusHead") => {
            Some("Opus")
        }
        "mp4" | "m4v"
            if head.get(4..8) == Some(b"ftyp")
                && (contains_marker(&head, b"avc1") || contains_marker(&head, b"avc3")) =>
        {
            Some("H.264 MP4")
        }
        _ => None,
    };
    let duration = if head.starts_with(b"OggS") && head.get(28..36) == Some(b"OpusHead") {
        let skip = u16::from_le_bytes(head.get(38..40)?.try_into().ok()?) as u64;
        let serial = head.get(14..18)?;
        let count = metadata.len().min(65_536) as usize;
        file.seek(SeekFrom::End(-(count as i64))).ok()?;
        let mut tail = vec![0; count];
        file.read_exact(&mut tail).ok()?;
        // Only accept a complete EOS page of the same stream; chained/unknown audio stays unavailable.
        tail.windows(4)
            .enumerate()
            .rev()
            .find_map(|(offset, magic)| {
                if magic != b"OggS" {
                    return None;
                }
                let page = tail.get(offset..)?;
                if page.get(4) != Some(&0) || page.get(5)? & 4 == 0 || page.get(14..18)? != serial {
                    return None;
                }
                let segments = *page.get(26)? as usize;
                let body = page
                    .get(27..27 + segments)?
                    .iter()
                    .map(|n| *n as usize)
                    .sum::<usize>();
                if offset + 27 + segments + body != tail.len() {
                    return None;
                }
                let granule = u64::from_le_bytes(page.get(6..14)?.try_into().ok()?);
                if granule == u64::MAX {
                    return None;
                }
                granule
                    .checked_sub(skip)
                    .map(|samples| samples as f64 / 48_000.)
            })
    } else if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WAVE") {
        let mut offset = 12usize;
        let mut rate = None;
        let mut duration = None;
        while let Some(chunk) = head.get(offset..offset + 8) {
            let length = u32::from_le_bytes(chunk[4..8].try_into().ok()?) as usize;
            if &chunk[..4] == b"fmt " && length >= 16 {
                rate = head
                    .get(offset + 16..offset + 20)
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(u32::from_le_bytes);
            }
            if &chunk[..4] == b"data" {
                duration = rate
                    .filter(|rate| *rate > 0)
                    .filter(|_| offset as u64 + 8 + length as u64 <= metadata.len())
                    .map(|rate| length as f64 / rate as f64);
                break;
            }
            offset = offset.checked_add(8 + length + length % 2)?;
        }
        duration
    } else {
        None
    };
    Some(AssetMediaInfo {
        bytes: metadata.len(),
        modified: metadata.modified().ok(),
        duration,
        canonical_format,
    })
}

#[derive(Clone, Debug)]
pub struct AssetFileChange {
    pub from: PathBuf,
    pub to: Option<PathBuf>,
    trash: Option<PathBuf>,
}

impl AssetFileChange {
    pub fn relocate(from: PathBuf, to: PathBuf) -> Self {
        Self {
            from,
            to: Some(to),
            trash: None,
        }
    }

    pub fn delete(from: PathBuf) -> Self {
        Self {
            from,
            to: None,
            trash: None,
        }
    }

    pub fn apply(&mut self, root: &Path, undo: bool) -> io::Result<()> {
        if let Some(to) = &self.to {
            let (from, to) = if undo {
                (to, &self.from)
            } else {
                (&self.from, to)
            };
            let from = checked_relative(from)?;
            if fs::symlink_metadata(root.join(&from))?
                .file_type()
                .is_symlink()
            {
                return Err(invalid("Resource symlinks cannot be moved"));
            }
            let from = confined_existing(root, &from)?;
            let to = asset_destination(root, to)?;
            move_without_overwrite(&from, &to)
        } else if undo {
            let trash = self
                .trash
                .as_ref()
                .ok_or_else(|| invalid("Trash location unavailable"))?;
            let to = asset_destination(root, &self.from)?;
            restore_trash(trash, &to)
        } else {
            self.trash = Some(trash_entry(root, &self.from)?);
            Ok(())
        }
    }
}

fn move_without_overwrite(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Destination already exists",
        ));
    }
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let from = from
            .to_str()
            .ok_or_else(|| invalid("File path is not UTF-8"))?;
        let to = to
            .to_str()
            .ok_or_else(|| invalid("File path is not UTF-8"))?;
        NSFileManager::defaultManager()
            .moveItemAtURL_toURL_error(
                &NSURL::fileURLWithPath(&NSString::from_str(from)),
                &NSURL::fileURLWithPath(&NSString::from_str(to)),
            )
            .map_err(|error| io::Error::other(error.to_string()))
    }
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let from = CString::new(from.as_os_str().as_bytes())
            .map_err(|error| invalid(error.to_string()))?;
        let to =
            CString::new(to.as_os_str().as_bytes()).map_err(|error| invalid(error.to_string()))?;
        // SAFETY: both paths are NUL-terminated and live for the call. The
        // kernel prevents a concurrent destination from being overwritten.
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        fs::rename(from, to)
    }
}

fn asset_destination(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let relative = checked_relative(relative)?;
    let root = root.canonicalize()?;
    let mut parent = root.clone();
    for part in relative.parent().unwrap_or(Path::new("")).components() {
        parent.push(part);
        match fs::create_dir(&parent) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        parent = parent.canonicalize()?;
        if !parent.starts_with(&root) {
            return Err(invalid("Destination escapes workspace"));
        }
    }
    confined_destination(&root, &relative)
}

pub fn asset_edit_path(
    asset: &AssetEntry,
    id: &str,
    kind: AssetKind,
    rename_file: bool,
) -> PathBuf {
    let mut path = if kind == asset.kind {
        asset.path.clone()
    } else {
        Path::new("assets")
            .join(namespace(kind))
            .join(asset.path.file_name().unwrap_or_default())
    };
    if rename_file && id != asset.id {
        let ext = asset.path.extension().unwrap_or_default().to_string_lossy();
        path.set_file_name(format!("{id}.{ext}"));
    }
    path
}

pub fn unmapped_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(".unmapped");
    PathBuf::from(value)
}

pub fn mapped_path(path: &Path) -> Option<PathBuf> {
    path.to_str()?.strip_suffix(".unmapped").map(PathBuf::from)
}

pub fn remap_manifest(source: &str, kind: AssetKind, path: &Path) -> io::Result<String> {
    let id = identifier_from_filename(path)?;
    let manifest =
        EiyashouAssetManifest::from_yaml(source).map_err(|error| invalid(error.to_string()))?;
    reject_manifest_conflict(&manifest, kind, &id, path)?;
    insert_manifest_entry(source, kind, &id, path)
}

pub fn edit_manifest_asset_path(source: &str, old: &Path, new: &Path) -> io::Result<String> {
    rewrite_manifest_paths(source, &[(old.to_owned(), new.to_owned())])
}

pub fn remove_manifest_asset(source: &str, asset: &AssetEntry) -> io::Result<String> {
    let manifest =
        EiyashouAssetManifest::from_yaml(source).map_err(|error| invalid(error.to_string()))?;
    let entry = entries_for_kind(&manifest, asset.kind)
        .get(&asset.id)
        .ok_or_else(|| invalid("Asset entry changed"))?;
    if entry.path() != slash_path(&asset.path) || entry.tags() != asset.tags {
        return Err(invalid("Asset entry changed"));
    }
    let mut active = false;
    let mut range = None;
    for (start, end) in line_ranges(source) {
        let line = source[start..end].trim_end_matches(['\r', '\n']);
        let indent = line.len() - line.trim_start().len();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if (indent == 0 || indent == 2)
            && let Some((begin, _)) = range
        {
            range = Some((begin, start));
            break;
        }
        if indent == 0 {
            active = line
                .split_once(':')
                .is_some_and(|(key, _)| key.trim() == namespace(asset.kind));
        } else if active
            && indent == 2
            && line
                .split_once(':')
                .is_some_and(|(key, _)| parse_yaml_scalar(key) == Some(asset.id.as_str()))
        {
            range = Some((start, source.len()));
        }
    }
    let (start, end) = range.ok_or_else(|| invalid("Asset entry cannot be located safely"))?;
    let comments = source[start..end]
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    let mut edited = source.to_owned();
    edited.replace_range(start..end, &comments);
    validate_manifest(restore_empty_namespace(edited, namespace(asset.kind)))
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
#[path = "trash.rs"]
mod trash;

pub fn trash_entry(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let relative = checked_relative(relative)?;
    if fs::symlink_metadata(root.join(&relative))?
        .file_type()
        .is_symlink()
    {
        return Err(invalid("Resource symlinks cannot be trashed"));
    }
    let path = confined_existing(root, &relative)?;
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    return trash::trash(&path);
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "System Trash is unavailable",
        ))
    }
}

fn restore_trash(receipt: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    return trash::restore(receipt, destination);
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    move_without_overwrite(receipt, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_status_checks_headers_instead_of_extensions() {
        let root = std::env::temp_dir().join(format!("keine-format-probe-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let cases: &[(&str, &[u8], Option<&str>)] = &[
            ("valid.webp", b"RIFF\x08\0\0\0WEBPVP8Lpayload", Some("WebP")),
            ("fake.webp", b"not an image", None),
            ("valid.opus", b"OggS\0OpusHead", Some("Opus")),
            ("vorbis.ogg", b"OggS\0vorbis", None),
            ("valid.mp4", b"\0\0\0\x18ftypisom\0avc1", Some("H.264 MP4")),
            ("other.mp4", b"\0\0\0\x18ftypisom\0hvc1", None),
            ("source.png", b"source", None),
        ];
        for (name, bytes, format) in cases {
            fs::write(root.join(name), bytes).unwrap();
            let info = asset_media_info(&root, Path::new(name)).unwrap();
            assert_eq!(info.canonical_format, *format, "{name}");
            assert_eq!(info.bytes, bytes.len() as u64);
        }
        assert!(asset_media_info(&root, Path::new("missing.opus")).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unmap_remap_preserves_manifest_neighbors_and_rejects_collisions() {
        let source = "# assets\nbackgrounds:\n  room: 'assets/backgrounds/room.webp'\n  next: 'assets/backgrounds/next.webp'\nfigures: {}\n";
        let asset = AssetEntry {
            kind: AssetKind::Background,
            id: "room".into(),
            path: "assets/backgrounds/room.webp".into(),
            tags: Vec::new(),
            exists: true,
            reference_count: 4,
        };
        let unmapped = remove_manifest_asset(source, &asset).unwrap();
        assert!(unmapped.contains("# assets"));
        assert!(unmapped.contains("  next:"));
        assert!(!unmapped.contains("  room:"));
        let path = unmapped_path(&asset.path);
        assert_eq!(unmapped_candidate_kind(&path), Some(AssetKind::Background));
        assert!(unmapped_candidate_kind(&asset.path).is_none());
        let remapped = remap_manifest(&unmapped, asset.kind, &mapped_path(&path).unwrap()).unwrap();
        assert_eq!(
            EiyashouAssetManifest::from_yaml(&remapped).unwrap(),
            EiyashouAssetManifest::from_yaml(source).unwrap()
        );
        assert!(remap_manifest(source, asset.kind, &asset.path).is_err());
    }

    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[test]
    fn system_trash_is_recoverable_and_restoration_does_not_overwrite() {
        let root = std::env::temp_dir().join(format!("keine-trash-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("owned.txt"), b"owned test file").unwrap();
        let mut change = AssetFileChange::delete("owned.txt".into());
        change.apply(&root, false).unwrap();
        assert!(!root.join("owned.txt").exists());
        #[cfg(not(windows))]
        assert_eq!(
            fs::read(change.trash.as_ref().unwrap()).unwrap(),
            b"owned test file"
        );
        fs::write(root.join("owned.txt"), b"new file").unwrap();
        assert!(change.apply(&root, true).is_err());
        assert_eq!(fs::read(root.join("owned.txt")).unwrap(), b"new file");
        fs::remove_file(root.join("owned.txt")).unwrap();
        change.apply(&root, true).unwrap();
        assert_eq!(
            fs::read(root.join("owned.txt")).unwrap(),
            b"owned test file"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
