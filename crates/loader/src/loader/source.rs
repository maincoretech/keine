use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use hakutaku_core::{AssetCursor, Package, ResourceBudget};

fn packaged_hakutaku_keys() -> Result<([u8; 32], [u8; 32])> {
    let share_a = *include_bytes!(concat!(env!("OUT_DIR"), "/hakutaku-key-share-a.bin"));
    let share_b = *include_bytes!(concat!(env!("OUT_DIR"), "/hakutaku-key-share-b.bin"));
    let public_key = *include_bytes!(concat!(env!("OUT_DIR"), "/hakutaku-public-key.bin"));
    if share_a == [0; 32] && share_b == [0; 32] && public_key == [0; 32] {
        bail!("this development engine does not embed Hakutaku release keys");
    }
    Ok((
        std::array::from_fn(|index| share_a[index] ^ share_b[index]),
        public_key,
    ))
}

/// One immutable physical content backend shared by scripts and Bevy assets.
#[derive(Clone)]
pub enum ContentBackend {
    FileSystem(PathBuf),
    Hakutaku(HakutakuArchive),
}

impl fmt::Debug for ContentBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileSystem(root) => formatter.debug_tuple("FileSystem").field(root).finish(),
            Self::Hakutaku(archive) => formatter.debug_tuple("Hakutaku").field(archive).finish(),
        }
    }
}

impl ContentBackend {
    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let path = safe_relative(path)?;
        match self {
            Self::FileSystem(root) => fs::read(root.join(&path))
                .with_context(|| format!("failed to read {}", root.join(path).display())),
            Self::Hakutaku(archive) => archive.read(&path),
        }
    }

    pub fn contains_file(&self, path: &Path) -> bool {
        let Ok(path) = safe_relative(path) else {
            return false;
        };
        match self {
            Self::FileSystem(root) => root.join(path).is_file(),
            Self::Hakutaku(archive) => archive.contains_file(&path),
        }
    }

    pub fn is_directory(&self, path: &Path) -> bool {
        let Ok(path) = safe_relative(path) else {
            return false;
        };
        match self {
            Self::FileSystem(root) => root.join(path).is_dir(),
            Self::Hakutaku(archive) => archive.is_directory(&path),
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
            Self::Hakutaku(archive) => Ok(archive.read_directory(&path)),
        }
    }

    pub fn filesystem_root(&self) -> Option<&Path> {
        match self {
            Self::FileSystem(root) => Some(root),
            Self::Hakutaku(_) => None,
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
            ContentBackend::Hakutaku(archive) => {
                ContentFileInner::Archive(Box::new(archive.open_file(&path)?))
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
    /// Hakutaku mounts filter the package's in-memory file index once instead of
    /// rescanning the complete package for every directory. Filesystem mounts
    /// follow links only when their canonical target stays inside the mount;
    /// canonical directory identities prevent links from creating cycles.
    pub(crate) fn recursive_files(&self) -> Result<Vec<PathBuf>> {
        match &self.backend {
            ContentBackend::FileSystem(root) => collect_filesystem_files(&root.join(&self.prefix)),
            ContentBackend::Hakutaku(archive) => Ok(archive.files_under(&self.prefix)),
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
    Archive(Box<AssetCursor>),
}

impl ContentFile {
    /// Total logical length without changing the current cursor position.
    pub fn len(&self) -> std::io::Result<u64> {
        match &self.inner {
            ContentFileInner::FileSystem(file) => file.metadata().map(|metadata| metadata.len()),
            ContentFileInner::Archive(cursor) => Ok(cursor.len()),
        }
    }

    pub fn is_empty(&self) -> std::io::Result<bool> {
        self.len().map(|length| length == 0)
    }

    pub fn read_remaining_into(&mut self, output: &mut Vec<u8>) -> std::io::Result<usize> {
        match &mut self.inner {
            ContentFileInner::FileSystem(file) => file.read_to_end(output),
            ContentFileInner::Archive(cursor) => cursor.read_to_end(output),
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

/// Shared, indexed Hakutaku snapshot. Cloning this handle is O(1).
#[derive(Clone)]
pub struct HakutakuArchive {
    path: Arc<PathBuf>,
    package: Package,
    files: Arc<HashSet<PathBuf>>,
    directories: Arc<HashSet<PathBuf>>,
}

impl fmt::Debug for HakutakuArchive {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HakutakuArchive")
            .field("path", &self.path)
            .field("files", &self.files.len())
            .field("directories", &self.directories.len())
            .finish()
    }
}

impl HakutakuArchive {
    pub fn open(path: &Path) -> Result<Self> {
        let (root_key, public_key) = packaged_hakutaku_keys()?;
        Self::open_with_keys(path, root_key, public_key)
    }

    pub(crate) fn open_packaged(path: &Path) -> Result<Self> {
        Self::open(path)
    }

    pub fn open_with_keys(path: &Path, root_key: [u8; 32], public_key: [u8; 32]) -> Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| format!("failed to resolve Hakutaku snapshot {}", path.display()))?;
        let data = path.parent().unwrap_or_else(|| Path::new(".")).join("data");
        let package = Package::open_directory(
            &path,
            data,
            root_key,
            public_key,
            ResourceBudget::memory_constrained(),
        )
        .with_context(|| format!("failed to open Hakutaku snapshot {}", path.display()))?;
        let files = package
            .list_assets()?
            .into_iter()
            .map(|asset| safe_relative(Path::new(&asset.path)))
            .collect::<Result<HashSet<_>>>()?;
        let directories = files
            .iter()
            .flat_map(|file| file.ancestors().skip(1).map(Path::to_owned))
            .collect::<HashSet<_>>();
        Ok(Self {
            path: Arc::new(path),
            package,
            files: Arc::new(files),
            directories: Arc::new(directories),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let path = archive_path(path)?;
        self.package
            .asset(&path)?
            .read()
            .with_context(|| format!("failed to read Hakutaku entry {path}"))
    }

    pub fn open_file(&self, path: &Path) -> Result<AssetCursor> {
        let path = archive_path(path)?;
        Ok(self
            .package
            .asset(&path)
            .with_context(|| format!("failed to open Hakutaku entry {path}"))?
            .cursor())
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
    use hakutaku_pack::{Identity, PackOptions, pack_directory};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn rejects_paths_that_escape_a_source() {
        assert!(safe_relative(Path::new("../secret")).is_err());
        assert!(safe_relative(Path::new("assets/bg.webp")).is_ok());
    }

    #[test]
    fn collects_nested_hakutaku_files_relative_to_a_mount_in_one_pass() {
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

    #[test]
    fn streams_hakutaku_assets_without_a_loader_read_ahead_layer() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-hakutaku-{nonce}"));
        let input = root.join("input");
        let release = root.join("release");
        fs::create_dir_all(&input).unwrap();
        let expected: Vec<u8> = (0..700_000).map(|index| index as u8).collect();
        fs::write(input.join("video.mp4"), &expected).unwrap();
        let identity = Identity::generate().unwrap();
        pack_directory(&PackOptions::new(&input, &release), &identity).unwrap();
        let archive = HakutakuArchive::open_with_keys(
            &release.join("game.haku"),
            identity.root_key(),
            identity.public_key(),
        )
        .unwrap();
        let mount = ContentMount::new(ContentBackend::Hakutaku(archive), "").unwrap();
        let mut file = mount.open_file(Path::new("video.mp4")).unwrap();
        file.seek(SeekFrom::Start(255_900)).unwrap();
        let mut actual = vec![0; 500];
        file.read_exact(&mut actual).unwrap();
        assert_eq!(actual, expected[255_900..256_400]);
        fs::remove_dir_all(root).unwrap();
    }
}
