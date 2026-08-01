use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::fs;
use std::io::{Error, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use hexz_k::{ResourceFile, ResourcePack, ResourcePackOptions};

const DEFAULT_HEXZ_PASSWORD: &str = "keine-hexz-resource-v1";
const HEXZ_READ_AHEAD_BYTES: usize = 64 * 1024;

/// Compile-time resource key used for deliberately weak distribution
/// protection. It deters casual extraction but is not DRM: a key embedded in
/// a client executable can always be recovered by a determined user.
pub fn hexz_password() -> &'static str {
    option_env!("KEINE_HEXZ_PASSWORD").unwrap_or(DEFAULT_HEXZ_PASSWORD)
}

/// One immutable physical content backend shared by scripts and Bevy assets.
#[derive(Clone)]
pub enum ContentBackend {
    FileSystem(PathBuf),
    Hexz(HexzArchive),
}

impl fmt::Debug for ContentBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileSystem(root) => formatter.debug_tuple("FileSystem").field(root).finish(),
            Self::Hexz(archive) => formatter.debug_tuple("Hexz").field(archive).finish(),
        }
    }
}

impl ContentBackend {
    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let path = safe_relative(path)?;
        match self {
            Self::FileSystem(root) => fs::read(root.join(&path))
                .with_context(|| format!("failed to read {}", root.join(path).display())),
            Self::Hexz(archive) => archive.read(&path),
        }
    }

    pub fn contains_file(&self, path: &Path) -> bool {
        let Ok(path) = safe_relative(path) else {
            return false;
        };
        match self {
            Self::FileSystem(root) => root.join(path).is_file(),
            Self::Hexz(archive) => archive.contains_file(&path),
        }
    }

    pub fn is_directory(&self, path: &Path) -> bool {
        let Ok(path) = safe_relative(path) else {
            return false;
        };
        match self {
            Self::FileSystem(root) => root.join(path).is_dir(),
            Self::Hexz(archive) => archive.is_directory(&path),
        }
    }

    pub fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let path = safe_relative(path)?;
        match self {
            Self::FileSystem(root) => {
                let directory = root.join(&path);
                let mut entries = fs::read_dir(&directory)
                    .with_context(|| format!("failed to read {}", directory.display()))?
                    .map(|entry| entry.map(|entry| path.join(entry.file_name())))
                    .collect::<std::io::Result<Vec<_>>>()?;
                entries.sort();
                Ok(entries)
            }
            Self::Hexz(archive) => Ok(archive.read_directory(&path)),
        }
    }

    pub fn filesystem_root(&self) -> Option<&Path> {
        match self {
            Self::FileSystem(root) => Some(root),
            Self::Hexz(_) => None,
        }
    }
}

/// A logical directory inside a physical backend.
#[derive(Debug, Clone)]
pub struct ContentMount {
    backend: ContentBackend,
    prefix: PathBuf,
}

impl ContentMount {
    pub fn new(backend: ContentBackend, prefix: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            backend,
            prefix: safe_relative(&prefix.into())?,
        })
    }

    pub fn backend(&self) -> &ContentBackend {
        &self.backend
    }

    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    pub fn resolve(&self, path: &Path) -> Result<PathBuf> {
        Ok(self.prefix.join(safe_relative(path)?))
    }

    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        self.backend.read(&self.resolve(path)?)
    }

    /// Opens one logical file as an adapter-neutral seekable stream.
    pub fn open_file(&self, path: &Path) -> Result<ContentFile> {
        let path = self.resolve(path)?;
        let inner = match &self.backend {
            ContentBackend::FileSystem(root) => {
                let physical = root.join(&path);
                ContentFileInner::FileSystem(
                    fs::File::open(&physical)
                        .with_context(|| format!("failed to open {}", physical.display()))?,
                )
            }
            ContentBackend::Hexz(archive) => {
                ContentFileInner::Archive(archive.open_file(&path)?.cursor())
            }
        };
        Ok(ContentFile { inner })
    }

    pub fn contains_file(&self, path: &Path) -> bool {
        self.resolve(path)
            .is_ok_and(|path| self.backend.contains_file(&path))
    }

    pub fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let resolved = self.resolve(path)?;
        self.backend
            .read_directory(&resolved)?
            .into_iter()
            .map(|entry| {
                entry
                    .strip_prefix(&self.prefix)
                    .map(Path::to_owned)
                    .with_context(|| format!("entry {} escaped mount", entry.display()))
            })
            .collect()
    }

    pub fn is_directory(&self, path: &Path) -> bool {
        self.resolve(path)
            .is_ok_and(|path| self.backend.is_directory(&path))
    }

    /// Recursively collects every file below this mount.
    ///
    /// Hexz mounts filter the archive's in-memory file index once instead of
    /// rescanning the complete package for every directory. Filesystem mounts
    /// follow links only when their canonical target stays inside the mount;
    /// canonical directory identities prevent links from creating cycles.
    pub(crate) fn recursive_files(&self) -> Result<Vec<PathBuf>> {
        match &self.backend {
            ContentBackend::FileSystem(root) => collect_filesystem_files(&root.join(&self.prefix)),
            ContentBackend::Hexz(archive) => Ok(archive.files_under(&self.prefix)),
        }
    }

    pub fn filesystem_root(&self) -> Option<PathBuf> {
        self.backend
            .filesystem_root()
            .map(|root| root.join(&self.prefix))
    }
}

/// Seekable logical content stream exposed without leaking its container
/// implementation to Bevy or other consumers.
pub struct ContentFile {
    inner: ContentFileInner,
}

enum ContentFileInner {
    FileSystem(fs::File),
    Archive(HexzCursor),
}

impl ContentFile {
    pub fn read_remaining_into(&mut self, output: &mut Vec<u8>) -> std::io::Result<usize> {
        match &mut self.inner {
            ContentFileInner::FileSystem(file) => file.read_to_end(output),
            ContentFileInner::Archive(cursor) => cursor.read_remaining_into(output),
        }
    }
}

impl Read for ContentFile {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        match &mut self.inner {
            ContentFileInner::FileSystem(file) => file.read(output),
            ContentFileInner::Archive(cursor) => cursor.read(output),
        }
    }
}

impl Seek for ContentFile {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        match &mut self.inner {
            ContentFileInner::FileSystem(file) => file.seek(position),
            ContentFileInner::Archive(cursor) => cursor.seek(position),
        }
    }
}

/// Shared, indexed Hexz archive. Cloning this handle is O(1).
#[derive(Clone)]
pub struct HexzArchive {
    path: Arc<PathBuf>,
    pack: ResourcePack,
    encrypted: bool,
    files: Arc<HashSet<PathBuf>>,
    directories: Arc<HashSet<PathBuf>>,
}

impl fmt::Debug for HexzArchive {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HexzArchive")
            .field("path", &self.path)
            .field("encrypted", &self.encrypted)
            .field("files", &self.files.len())
            .field("directories", &self.directories.len())
            .finish()
    }
}

impl HexzArchive {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| format!("failed to resolve Hexz package {}", path.display()))?;
        let encrypted = hexz_k::is_encrypted(&path)
            .with_context(|| format!("failed to inspect Hexz package {}", path.display()))?;
        let pack = ResourcePack::open_with_options(
            &path,
            Some(hexz_password()),
            ResourcePackOptions::memory_constrained(),
        )
        .with_context(|| format!("failed to open Hexz package {}", path.display()))?;
        let files = pack
            .iter_files()
            .map(|path| safe_relative(Path::new(path)))
            .collect::<Result<HashSet<_>>>()?;
        let directories = files
            .iter()
            .flat_map(|file| file.ancestors().skip(1).map(Path::to_owned))
            .collect::<HashSet<_>>();
        Ok(Self {
            path: Arc::new(path),
            pack,
            encrypted,
            files: Arc::new(files),
            directories: Arc::new(directories),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        self.pack
            .read_file(&archive_path(path)?)
            .with_context(|| format!("failed to read Hexz entry {}", path.display()))
    }

    pub fn open_file(&self, path: &Path) -> Result<HexzFile> {
        let file = self
            .pack
            .open_file(&archive_path(path)?)
            .with_context(|| format!("failed to open Hexz entry {}", path.display()))?;
        Ok(HexzFile {
            file,
            read_ahead: self.encrypted,
        })
    }

    pub fn contains_file(&self, path: &Path) -> bool {
        safe_relative(path).is_ok_and(|path| self.files.contains(&path))
    }

    pub fn is_directory(&self, path: &Path) -> bool {
        let Ok(path) = safe_relative(path) else {
            return false;
        };
        path.as_os_str().is_empty() || self.directories.contains(&path)
    }

    pub fn read_directory(&self, path: &Path) -> Vec<PathBuf> {
        let Ok(path) = safe_relative(path) else {
            return Vec::new();
        };
        let depth = path.components().count();
        self.files
            .iter()
            .filter(|file| file.starts_with(&path) && file.components().count() > depth)
            .filter_map(|file| {
                let component = file.components().nth(depth)?;
                Some(path.join(component.as_os_str()))
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn files_under(&self, prefix: &Path) -> Vec<PathBuf> {
        relative_files_under(&self.files, prefix)
    }
}

fn relative_files_under(files: &HashSet<PathBuf>, prefix: &Path) -> Vec<PathBuf> {
    files
        .iter()
        .filter_map(|file| file.strip_prefix(prefix).ok())
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_owned)
        .collect()
}

fn collect_filesystem_files(mount_root: &Path) -> Result<Vec<PathBuf>> {
    let mount_root = mount_root
        .canonicalize()
        .with_context(|| format!("failed to resolve content mount {}", mount_root.display()))?;
    if !mount_root.is_dir() {
        bail!("content mount is not a directory: {}", mount_root.display());
    }

    let mut visited = HashSet::from([mount_root.clone()]);
    let mut directories = vec![(PathBuf::new(), mount_root.clone())];
    let mut files = Vec::new();

    while let Some((logical_directory, physical_directory)) = directories.pop() {
        let mut entries = fs::read_dir(&physical_directory)
            .with_context(|| format!("failed to read {}", physical_directory.display()))?
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);

        for entry in entries {
            let file_type = entry.file_type().with_context(|| {
                format!(
                    "failed to inspect directory entry {}",
                    entry.path().display()
                )
            })?;
            let logical_path = logical_directory.join(entry.file_name());

            if file_type.is_file() {
                files.push(logical_path);
                continue;
            }

            if file_type.is_dir() {
                let canonical = entry.path().canonicalize().with_context(|| {
                    format!("failed to resolve directory {}", entry.path().display())
                })?;
                if canonical.starts_with(&mount_root) && visited.insert(canonical.clone()) {
                    directories.push((logical_path, canonical));
                }
                continue;
            }

            if !file_type.is_symlink() {
                continue;
            }

            // Broken links and direct symlink loops are not content entries.
            let Ok(canonical) = entry.path().canonicalize() else {
                continue;
            };
            if !canonical.starts_with(&mount_root) {
                continue;
            }
            let metadata = fs::metadata(&canonical).with_context(|| {
                format!("failed to inspect symlink target {}", canonical.display())
            })?;
            if metadata.is_file() {
                files.push(logical_path);
            } else if metadata.is_dir() && visited.insert(canonical.clone()) {
                directories.push((logical_path, canonical));
            }
        }
    }

    Ok(files)
}

/// Seekable file view used by Bevy without materializing the resource.
#[derive(Clone)]
pub struct HexzFile {
    file: ResourceFile,
    read_ahead: bool,
}

impl HexzFile {
    pub fn len(&self) -> usize {
        self.file.len()
    }

    pub fn is_empty(&self) -> bool {
        self.file.is_empty()
    }

    pub fn read_range_into(&self, offset: usize, buffer: &mut [u8]) -> Result<usize> {
        self.file.read_range_into(offset, buffer)
    }

    pub fn cursor(self) -> HexzCursor {
        HexzCursor::new(self)
    }
}

/// Seekable Hexz file cursor with one-block read-ahead. Asset decoders often
/// request headers and audio frames in small pieces; serving those requests
/// from this private buffer avoids authenticating/decompressing the same Hexz
/// block repeatedly.
pub struct HexzCursor {
    file: HexzFile,
    position: usize,
    cache_start: usize,
    cache: Vec<u8>,
}

impl HexzCursor {
    fn new(file: HexzFile) -> Self {
        Self {
            file,
            position: 0,
            cache_start: 0,
            cache: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.file.len()
    }

    pub fn is_empty(&self) -> bool {
        self.file.is_empty()
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn read_remaining_into(&mut self, output: &mut Vec<u8>) -> std::io::Result<usize> {
        let remaining = self.file.len().saturating_sub(self.position);
        let start = output.len();
        output.resize(start + remaining, 0);
        match self
            .file
            .read_range_into(self.position, &mut output[start..])
        {
            Ok(read) => {
                self.position += read;
                output.truncate(start + read);
                Ok(read)
            }
            Err(error) => {
                output.truncate(start);
                Err(Error::other(error.to_string()))
            }
        }
    }

    fn refill(&mut self) -> std::io::Result<()> {
        let remaining = self.file.len().saturating_sub(self.position);
        let capacity = remaining.min(HEXZ_READ_AHEAD_BYTES);
        self.cache.resize(capacity, 0);
        let read = self
            .file
            .read_range_into(self.position, &mut self.cache)
            .map_err(|error| Error::other(error.to_string()))?;
        self.cache.truncate(read);
        self.cache_start = self.position;
        Ok(())
    }
}

impl Read for HexzCursor {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() || self.position >= self.file.len() {
            return Ok(0);
        }
        if !self.file.read_ahead || output.len() >= HEXZ_READ_AHEAD_BYTES {
            let read = self
                .file
                .read_range_into(self.position, output)
                .map_err(|error| Error::other(error.to_string()))?;
            self.position += read;
            return Ok(read);
        }

        let cache_end = self.cache_start + self.cache.len();
        if self.position < self.cache_start || self.position >= cache_end {
            self.refill()?;
        }
        let offset = self.position - self.cache_start;
        let read = output.len().min(self.cache.len().saturating_sub(offset));
        output[..read].copy_from_slice(&self.cache[offset..offset + read]);
        self.position += read;
        Ok(read)
    }
}

impl Seek for HexzCursor {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let position = match position {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::End(offset) => self.file.len() as i128 + i128::from(offset),
            SeekFrom::Current(offset) => self.position as i128 + i128::from(offset),
        };
        if !(0..=self.file.len() as i128).contains(&position) {
            return Err(Error::other("Hexz seek is outside the resource"));
        }
        self.position = position as usize;
        Ok(self.position as u64)
    }
}

fn archive_path(path: &Path) -> Result<String> {
    let path = safe_relative(path)?;
    Ok(path
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn safe_relative(path: &Path) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => result.push(value),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("content path must be relative: {}", path.display());
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_that_escape_a_source() {
        assert!(safe_relative(Path::new("../secret")).is_err());
        assert!(safe_relative(Path::new("assets/bg.webp")).is_ok());
    }

    #[test]
    fn collects_nested_hexz_files_relative_to_a_mount_in_one_pass() {
        let files = [
            "project/scripts/main.txt",
            "project/scripts/chapter/act/scene.txt",
            "project/scripts/chapter/notes.md",
            "project/assets/background.webp",
            "other/scripts/ignored.txt",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect::<HashSet<_>>();

        let mut relative = relative_files_under(&files, Path::new("project/scripts"));
        relative.sort();

        assert_eq!(
            relative,
            [
                PathBuf::from("chapter/act/scene.txt"),
                PathBuf::from("chapter/notes.md"),
                PathBuf::from("main.txt"),
            ]
        );
    }
}
