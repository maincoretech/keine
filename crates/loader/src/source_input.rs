use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::ContentMount;

pub const MAX_SOURCE_FILE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SOURCE_FILES: usize = 4_096;
pub const MAX_SOURCE_TOTAL_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct SourceLimits {
    file_bytes: usize,
    files: usize,
    total_bytes: usize,
}

const SOURCE_LIMITS: SourceLimits = SourceLimits {
    file_bytes: MAX_SOURCE_FILE_BYTES,
    files: MAX_SOURCE_FILES,
    total_bytes: MAX_SOURCE_TOTAL_BYTES,
};

/// Per-project source ingress budget. Raw source bytes are not retained here;
/// the counters bound one complete parse operation across all of its inputs.
pub(crate) struct SourceBudget {
    root: Option<PathBuf>,
    files: usize,
    bytes: usize,
    limits: SourceLimits,
}

impl SourceBudget {
    pub(crate) fn for_mounts() -> Self {
        Self {
            root: None,
            files: 0,
            bytes: 0,
            limits: SOURCE_LIMITS,
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

    pub(crate) fn read_mount(&mut self, mount: &ContentMount, path: &Path) -> Result<Vec<u8>> {
        let file = mount
            .open_file(path)
            .with_context(|| format!("failed to open source {}", path.display()))?;
        let length = file
            .len()
            .with_context(|| format!("failed to inspect source {}", path.display()))?;
        self.read_stream(file, length, path)
    }

    pub(crate) fn read_file(&mut self, path: &Path) -> Result<Vec<u8>> {
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

    fn read_stream<R: Read>(&mut self, reader: R, declared: u64, path: &Path) -> Result<Vec<u8>> {
        if self.files >= self.limits.files {
            bail!(
                "source project exceeds the {}-file limit while reading {}",
                self.limits.files,
                path.display()
            );
        }
        let declared = usize::try_from(declared)
            .with_context(|| format!("source size exceeds this platform: {}", path.display()))?;
        self.check_size(declared, path)?;

        let read_limit = self
            .limits
            .file_bytes
            .checked_add(1)
            .context("source read limit overflow")?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(declared.min(self.limits.file_bytes))
            .with_context(|| format!("failed to reserve source input for {}", path.display()))?;
        Read::take(reader, read_limit as u64)
            .read_to_end(&mut bytes)
            .with_context(|| format!("failed to read source {}", path.display()))?;
        self.check_size(bytes.len(), path)?;

        self.files += 1;
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .context("source project byte count overflow")?;
        Ok(bytes)
    }

    fn check_size(&self, length: usize, path: &Path) -> Result<()> {
        if length > self.limits.file_bytes {
            bail!(
                "source {} exceeds the {}-byte per-file limit",
                path.display(),
                self.limits.file_bytes
            );
        }
        let total = self
            .bytes
            .checked_add(length)
            .context("source project byte count overflow")?;
        if total > self.limits.total_bytes {
            bail!(
                "source project exceeds the {}-byte total limit while reading {}",
                self.limits.total_bytes,
                path.display()
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

    fn budget(file_bytes: usize, files: usize, total_bytes: usize) -> SourceBudget {
        SourceBudget {
            root: None,
            files: 0,
            bytes: 0,
            limits: SourceLimits {
                file_bytes,
                files,
                total_bytes,
            },
        }
    }

    #[test]
    fn accepts_exact_source_limits() {
        let mut budget = budget(4, 1, 4);

        assert_eq!(
            budget
                .read_stream(Cursor::new(b"1234"), 4, Path::new("scene.txt"))
                .unwrap(),
            b"1234"
        );
    }

    #[test]
    fn rejects_declared_and_growing_sources_before_unbounded_allocation() {
        let mut declared = budget(4, 2, 8);
        assert!(
            declared
                .read_stream(Cursor::new(b"12345"), 5, Path::new("declared.txt"))
                .unwrap_err()
                .to_string()
                .contains("per-file limit")
        );

        let mut growing = budget(4, 2, 8);
        assert!(
            growing
                .read_stream(Cursor::new(b"12345"), 4, Path::new("growing.txt"))
                .unwrap_err()
                .to_string()
                .contains("per-file limit")
        );
    }

    #[test]
    fn enforces_file_count_and_total_bytes_across_one_load() {
        let mut count = budget(4, 1, 8);
        count
            .read_stream(Cursor::new(b"1"), 1, Path::new("one.txt"))
            .unwrap();
        assert!(
            count
                .read_stream(Cursor::new(b"2"), 1, Path::new("two.txt"))
                .unwrap_err()
                .to_string()
                .contains("file limit")
        );

        let mut total = budget(4, 2, 5);
        total
            .read_stream(Cursor::new(b"123"), 3, Path::new("one.txt"))
            .unwrap();
        assert!(
            total
                .read_stream(Cursor::new(b"456"), 3, Path::new("two.txt"))
                .unwrap_err()
                .to_string()
                .contains("total limit")
        );
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
        let mut budget = SourceBudget::for_filesystem(&root).unwrap();

        assert_eq!(
            budget.read_file(&root.join("inside.json")).unwrap(),
            b"inside"
        );
        assert!(
            budget
                .read_file(&outside)
                .unwrap_err()
                .to_string()
                .contains("escaped project root")
        );
        let _ = std::fs::remove_dir_all(base);
    }
}
