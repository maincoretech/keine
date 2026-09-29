#[path = "workspace/app/data.rs"]
pub mod app_data;
pub mod document;
#[path = "workspace/file/ops.rs"]
pub(crate) mod file_ops;
pub mod migration;
pub mod persistence;
#[path = "workspace/project/key.rs"]
pub mod project_key;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::project_key::ProjectKey;

pub(crate) const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceFile {
    pub relative_path: PathBuf,
    pub size: u64,
    pub kind: WorkspaceEntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceEntryKind {
    File,
    Directory,
}

impl WorkspaceFile {
    pub const fn is_dir(&self) -> bool {
        matches!(self.kind, WorkspaceEntryKind::Directory)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextDocument {
    pub relative_path: PathBuf,
    pub contents: String,
}

#[derive(Clone, Debug)]
pub struct WorkspaceSession {
    key: ProjectKey,
    name: String,
    files: Vec<WorkspaceFile>,
    documents: Vec<TextDocument>,
}

impl WorkspaceSession {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let key = ProjectKey::from_path(path)?;
        if !key.path().is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a Kēne workspace must be a directory",
            ));
        }

        let name = key
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("Project")
            .to_owned();
        let mut files = Vec::new();
        discover(key.path(), key.path(), &mut files)?;
        let mut initial = files
            .iter()
            .filter(|file| !file.is_dir())
            .filter(|file| file.size <= MAX_DOCUMENT_BYTES)
            .filter(|file| is_text_document(&file.relative_path))
            .collect::<Vec<_>>();
        initial.sort_by_key(|file| document_rank(&file.relative_path));
        let documents = initial
            .into_iter()
            .take(2)
            .filter_map(|file| {
                fs::read_to_string(key.path().join(&file.relative_path))
                    .ok()
                    .map(|contents| TextDocument {
                        relative_path: file.relative_path.clone(),
                        contents,
                    })
            })
            .collect::<Vec<_>>();

        Ok(Self {
            key,
            name,
            files,
            documents,
        })
    }

    pub fn key(&self) -> &ProjectKey {
        &self.key
    }

    pub fn root(&self) -> &Path {
        self.key.path()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn files(&self) -> &[WorkspaceFile] {
        &self.files
    }

    pub fn documents(&self) -> &[TextDocument] {
        &self.documents
    }
}

fn discover(root: &Path, directory: &Path, files: &mut Vec<WorkspaceFile>) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if should_descend(&name) {
                let relative_path = path
                    .strip_prefix(root)
                    .map_err(io::Error::other)?
                    .to_owned();
                files.push(WorkspaceFile {
                    relative_path,
                    size: 0,
                    kind: WorkspaceEntryKind::Directory,
                });
                discover(root, &path, files)?;
            }
        } else if file_type.is_file() && !name.starts_with('.') {
            let relative_path = path
                .strip_prefix(root)
                .map_err(io::Error::other)?
                .to_owned();
            files.push(WorkspaceFile {
                relative_path,
                size: entry.metadata()?.len(),
                kind: WorkspaceEntryKind::File,
            });
        }
    }
    Ok(())
}

fn should_descend(name: &str) -> bool {
    !matches!(
        name,
        ".git" | "target" | "node_modules" | ".keine" | "saves"
    ) && !name.starts_with('.')
}

pub(crate) fn is_text_document(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("shou" | "txt" | "json" | "yaml" | "yml" | "toml" | "md" | "webgal")
    )
}

fn document_rank(path: &Path) -> (u8, String) {
    let value = path.to_string_lossy().replace('\\', "/");
    let rank = if value.starts_with("scripts/") {
        0
    } else if value.starts_with("chapters/") {
        1
    } else if value == "config.yaml" || value == "project.json" {
        2
    } else {
        3
    };
    (rank, value)
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
        let root = std::env::temp_dir().join(format!("keine-editor-workspace-{nonce}"));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("scripts/02.shou"), "second").unwrap();
        fs::write(root.join("scripts/01.shou"), "first").unwrap();
        fs::write(root.join("project.json"), "{}").unwrap();
        fs::write(root.join("cover.webp"), "media").unwrap();
        fs::write(root.join("target/ignored.txt"), "ignored").unwrap();
        root
    }

    #[test]
    fn discovery_does_not_truncate_large_projects() {
        let root = fixture();
        for number in 0..2100 {
            fs::write(
                root.join(format!("scripts/extra-{number:04}.shou")),
                "scene empty {}",
            )
            .unwrap();
        }
        let session = WorkspaceSession::open(&root).unwrap();
        assert!(
            session
                .files()
                .iter()
                .any(|file| file.relative_path == Path::new("scripts/extra-2099.shou"))
        );
        assert_eq!(
            session
                .files()
                .iter()
                .filter(|file| file
                    .relative_path
                    .extension()
                    .is_some_and(|ext| ext == "shou"))
                .count(),
            2102
        );
        assert_eq!(session.documents().len(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovers_two_real_documents_in_stable_authoring_order() {
        let root = fixture();
        let session = WorkspaceSession::open(&root).unwrap();
        assert_eq!(session.documents().len(), 2);
        assert_eq!(
            session.documents()[0].relative_path,
            Path::new("scripts/01.shou")
        );
        assert_eq!(session.documents()[0].contents, "first");
        assert_eq!(
            session.documents()[1].relative_path,
            Path::new("scripts/02.shou")
        );
        assert!(
            session
                .files()
                .iter()
                .all(|file| !file.relative_path.starts_with("target"))
        );
        assert!(session.files().iter().any(|file| {
            file.relative_path == Path::new("scripts") && file.kind == WorkspaceEntryKind::Directory
        }));
        assert!(session.files().iter().any(|file| {
            file.relative_path == Path::new("cover.webp") && file.kind == WorkspaceEntryKind::File
        }));
        fs::remove_dir_all(root).unwrap();
    }
}
