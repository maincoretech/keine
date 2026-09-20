use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use keine_loader::{DiagnosticLevel, parse_native_document};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationChange {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MigrationPlan {
    project_root: PathBuf,
    pub changes: Vec<MigrationChange>,
}

impl MigrationPlan {
    /// Preview the only automatic v1 source migration: an already-valid
    /// Eiyashou source with the legacy `.txt` suffix may be renamed to
    /// `.shou`. WebGAL/LetsGal text is never translated or rewritten.
    pub fn preview(project_root: &Path) -> io::Result<Self> {
        let project_root = project_root.canonicalize()?;
        let scripts = project_root.join("scripts");
        let mut changes = Vec::new();
        if scripts.is_dir() {
            collect(&project_root, &scripts, &mut changes)?;
        }
        changes.sort_by(|left, right| left.from.cmp(&right.from));
        Ok(Self {
            project_root,
            changes,
        })
    }

    pub fn diff_preview(&self) -> String {
        if self.changes.is_empty() {
            return "No eligible legacy Eiyashou sources were found.".into();
        }
        self.changes
            .iter()
            .map(|change| {
                format!(
                    "rename {}\n    -> {}",
                    change.from.display(),
                    change.to.display()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn apply(self) -> io::Result<usize> {
        let mut renames = Vec::with_capacity(self.changes.len());
        for change in &self.changes {
            let from = confined_existing(&self.project_root, &change.from)?;
            let to = self.project_root.join(&change.to);
            if to.exists() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("migration target already exists: {}", to.display()),
                ));
            }
            let parent = to.parent().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "migration target has no parent",
                )
            })?;
            let parent = parent.canonicalize()?;
            if !parent.starts_with(&self.project_root) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "migration target escapes the project root",
                ));
            }
            renames.push((from, to));
        }
        let mut applied = Vec::new();
        for (from, to) in &renames {
            if let Err(error) = fs::rename(from, to) {
                for (previous_from, previous_to) in applied.into_iter().rev() {
                    let _ = fs::rename(previous_to, previous_from);
                }
                return Err(error);
            }
            applied.push((from, to));
        }
        Ok(self.changes.len())
    }
}

fn collect(root: &Path, directory: &Path, changes: &mut Vec<MigrationChange>) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect(root, &path, changes)?;
            continue;
        }
        if !file_type.is_file() || path.extension().is_none_or(|value| value != "txt") {
            continue;
        }
        let source = fs::read_to_string(&path)?;
        let document = parse_native_document(&source);
        if document.scenes.is_empty()
            || document
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.level == DiagnosticLevel::Error)
        {
            continue;
        }
        let relative = path.strip_prefix(root).map_err(io::Error::other)?;
        let to = relative.with_extension("shou");
        if !root.join(&to).exists() {
            changes.push(MigrationChange {
                from: relative.to_owned(),
                to,
            });
        }
    }
    Ok(())
}

fn confined_existing(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "migration source escapes the project root",
        ));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn fixture() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-editor-migration-{nonce}"));
        fs::create_dir_all(root.join("scripts/nested")).unwrap();
        root
    }

    #[test]
    fn preview_is_read_only_and_apply_only_renames_valid_eiyashou() {
        let root = fixture();
        let source = "// retained\nscene opening { \"hello\" }\n";
        fs::write(root.join("scripts/main.txt"), source).unwrap();
        fs::write(root.join("scripts/nested/webgal.txt"), "intro:hello;").unwrap();

        let plan = MigrationPlan::preview(&root).unwrap();
        assert_eq!(plan.changes.len(), 1);
        assert!(root.join("scripts/main.txt").is_file());
        assert!(plan.diff_preview().contains("scripts/main.shou"));
        assert_eq!(plan.apply().unwrap(), 1);
        assert_eq!(
            fs::read_to_string(root.join("scripts/main.shou")).unwrap(),
            source
        );
        assert!(root.join("scripts/nested/webgal.txt").is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
