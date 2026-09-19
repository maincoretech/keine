use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::project_key::ProjectKey;

const MAX_DISCOVERED_FILES: usize = 2_000;
const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceFile {
    pub relative_path: PathBuf,
    pub size: u64,
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
        files.sort_by_key(|file| document_rank(&file.relative_path));

        let documents = files
            .iter()
            .filter(|file| file.size <= MAX_DOCUMENT_BYTES)
            .filter_map(|file| {
                fs::read_to_string(key.path().join(&file.relative_path))
                    .ok()
                    .map(|contents| TextDocument {
                        relative_path: file.relative_path.clone(),
                        contents,
                    })
            })
            .take(2)
            .collect();

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
    if files.len() >= MAX_DISCOVERED_FILES {
        return Ok(());
    }
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if files.len() >= MAX_DISCOVERED_FILES {
            break;
        }
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if should_descend(&name) {
                discover(root, &path, files)?;
            }
        } else if file_type.is_file() && is_text_document(&path) {
            let relative_path = path
                .strip_prefix(root)
                .map_err(io::Error::other)?
                .to_owned();
            files.push(WorkspaceFile {
                relative_path,
                size: entry.metadata()?.len(),
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

fn is_text_document(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("txt" | "json" | "yaml" | "yml" | "toml" | "md" | "webgal")
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
        fs::write(root.join("scripts/02.txt"), "second").unwrap();
        fs::write(root.join("scripts/01.txt"), "first").unwrap();
        fs::write(root.join("project.json"), "{}").unwrap();
        fs::write(root.join("target/ignored.txt"), "ignored").unwrap();
        root
    }

    #[test]
    fn discovers_two_real_documents_in_stable_authoring_order() {
        let root = fixture();
        let session = WorkspaceSession::open(&root).unwrap();
        assert_eq!(session.documents().len(), 2);
        assert_eq!(
            session.documents()[0].relative_path,
            Path::new("scripts/01.txt")
        );
        assert_eq!(session.documents()[0].contents, "first");
        assert_eq!(
            session.documents()[1].relative_path,
            Path::new("scripts/02.txt")
        );
        assert!(
            session
                .files()
                .iter()
                .all(|file| !file.relative_path.starts_with("target"))
        );
        fs::remove_dir_all(root).unwrap();
    }
}
