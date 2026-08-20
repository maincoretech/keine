use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::ContentMount;

pub const MAX_SOURCE_FILE_BYTES: usize = 32 * 1024 * 1024;

/// Reads one source document without imposing an arbitrary project-wide scale
/// limit. The filesystem variant also keeps direct adapter reads inside the
/// canonical project root.
pub(crate) struct SourceReader {
    root: Option<PathBuf>,
    maximum: usize,
}

impl SourceReader {
    pub(crate) fn for_mounts() -> Self {
        Self {
            root: None,
            maximum: MAX_SOURCE_FILE_BYTES,
        }
    }

    pub(crate) fn for_filesystem(root: &Path) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("failed to resolve source project {}", root.display()))?;
        if !root.is_dir() {
            bail!("source project is not a directory: {}", root.display());
        }
        Ok(Self {
            root: Some(root),
            ..Self::for_mounts()
        })
    }

    pub(crate) fn read_mount(&self, mount: &ContentMount, path: &Path) -> Result<Vec<u8>> {
        let file = mount
            .open_file(path)
            .with_context(|| format!("failed to open source {}", path.display()))?;
        let length = file
            .len()
            .with_context(|| format!("failed to inspect source {}", path.display()))?;
        self.read_stream(file, length, path)
    }

    pub(crate) fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let root = self
            .root
            .as_deref()
            .context("filesystem source budget has no project root")?;
        let resolved = path
            .canonicalize()
            .with_context(|| format!("failed to resolve source {}", path.display()))?;
        if !resolved.starts_with(root) {
            bail!(
                "source path escaped project root {}: {}",
                root.display(),
                resolved.display()
            );
        }
        let file = File::open(&resolved)
            .with_context(|| format!("failed to open source {}", resolved.display()))?;
        let length = file
            .metadata()
            .with_context(|| format!("failed to inspect source {}", resolved.display()))?
            .len();
        self.read_stream(file, length, &resolved)
    }

    fn read_stream<R: Read>(&self, reader: R, declared: u64, path: &Path) -> Result<Vec<u8>> {
        let declared = usize::try_from(declared)
            .with_context(|| format!("source size exceeds this platform: {}", path.display()))?;
        self.check_size(declared, path)?;

        let read_limit = self
            .maximum
            .checked_add(1)
            .context("source read limit overflow")?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(declared.min(self.maximum))
            .with_context(|| format!("failed to reserve source input for {}", path.display()))?;
        Read::take(reader, read_limit as u64)
            .read_to_end(&mut bytes)
            .with_context(|| format!("failed to read source {}", path.display()))?;
        self.check_size(bytes.len(), path)?;
        Ok(bytes)
    }

    fn check_size(&self, length: usize, path: &Path) -> Result<()> {
        if length > self.maximum {
            bail!(
                "source {} exceeds the {}-byte per-file limit",
                path.display(),
                self.maximum
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn reader(maximum: usize) -> SourceReader {
        SourceReader {
            root: None,
            maximum,
        }
    }

    #[test]
    fn accepts_exact_source_limits() {
        let reader = reader(4);

        assert_eq!(
            reader
                .read_stream(Cursor::new(b"1234"), 4, Path::new("scene.txt"))
                .unwrap(),
            b"1234"
        );
    }

    #[test]
    fn rejects_declared_and_growing_sources_before_unbounded_allocation() {
        let declared = reader(4);
        assert!(
            declared
                .read_stream(Cursor::new(b"12345"), 5, Path::new("declared.txt"))
                .unwrap_err()
                .to_string()
                .contains("per-file limit")
        );

        let growing = reader(4);
        assert!(
            growing
                .read_stream(Cursor::new(b"12345"), 4, Path::new("growing.txt"))
                .unwrap_err()
                .to_string()
                .contains("per-file limit")
        );
    }

    #[test]
    fn project_scale_is_not_limited_by_the_document_reader() {
        let reader = reader(4);
        for index in 0..10_000 {
            reader
                .read_stream(Cursor::new(b"1234"), 4, Path::new("scene.txt"))
                .unwrap_or_else(|error| panic!("document {index} was rejected: {error}"));
        }
    }

    #[test]
    fn filesystem_sources_cannot_escape_the_project_root() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("keine-source-budget-{nonce}"));
        let root = base.join("project");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("inside.json"), b"inside").unwrap();
        let outside = base.join("outside.json");
        std::fs::write(&outside, b"outside").unwrap();
        let reader = SourceReader::for_filesystem(&root).unwrap();

        assert_eq!(
            reader.read_file(&root.join("inside.json")).unwrap(),
            b"inside"
        );
        assert!(
            reader
                .read_file(&outside)
                .unwrap_err()
                .to_string()
                .contains("escaped project root")
        );
        let _ = std::fs::remove_dir_all(base);
    }
}
