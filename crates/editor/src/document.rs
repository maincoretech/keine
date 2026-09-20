use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

const RECOVERY_SCHEMA: u32 = 1;
const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;
const MAX_RECOVERY_BYTES: u64 = MAX_DOCUMENT_BYTES * 2 + 4096;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

pub type DocumentHandle = Rc<RefCell<SourceDocument>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryState {
    None,
    Restored,
    Conflicted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSelection {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug)]
pub struct SourceDocument {
    relative_path: PathBuf,
    absolute_path: PathBuf,
    recovery_path: PathBuf,
    contents: String,
    disk_contents: String,
    revision: u64,
    saved_revision: u64,
    recovery_state: RecoveryState,
    selection: SourceSelection,
}

impl SourceDocument {
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    pub fn contents(&self) -> &str {
        &self.contents
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub const fn recovery_state(&self) -> RecoveryState {
        self.recovery_state
    }

    pub fn selection(&self) -> &SourceSelection {
        &self.selection
    }

    pub fn set_selection(&mut self, line: usize, column: usize) {
        self.selection = SourceSelection { line, column };
    }

    pub fn replace_contents(&mut self, contents: String) -> bool {
        if self.contents == contents {
            return false;
        }
        self.contents = contents;
        self.revision = self.revision.wrapping_add(1).max(1);
        true
    }

    pub fn persist_recovery(&mut self) -> io::Result<()> {
        if !self.is_dirty() {
            remove_if_present(&self.recovery_path)?;
            self.recovery_state = RecoveryState::None;
            return Ok(());
        }
        ensure_document_size(&self.contents)?;
        let draft = RecoveryDraft {
            schema: RECOVERY_SCHEMA,
            relative_path: self.relative_path.clone(),
            base_contents: self.disk_contents.clone(),
            revision: self.revision,
            contents: self.contents.clone(),
        };
        let bytes = postcard::to_stdvec(&draft).map_err(io::Error::other)?;
        atomic_bytes(&self.recovery_path, &bytes)?;
        self.recovery_state = RecoveryState::Restored;
        Ok(())
    }

    pub fn save(&mut self) -> Result<(), SaveError> {
        let current = read_source(&self.absolute_path)?;
        if current != self.disk_contents.as_bytes() {
            return Err(SaveError::ExternalModification {
                path: self.absolute_path.clone(),
            });
        }
        if !self.is_dirty() {
            remove_if_present(&self.recovery_path)?;
            self.recovery_state = RecoveryState::None;
            return Ok(());
        }

        ensure_document_size(&self.contents)?;
        atomic_source(&self.absolute_path, self.contents.as_bytes())?;
        self.disk_contents.clone_from(&self.contents);
        self.saved_revision = self.revision;
        self.recovery_state = RecoveryState::None;
        remove_if_present(&self.recovery_path)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum SaveError {
    ExternalModification { path: PathBuf },
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExternalModification { path } => write!(
                formatter,
                "{} changed outside Kēne Editor; reload or reconcile it before saving",
                path.display()
            ),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ExternalModification { .. } => None,
            Self::Io(error) => Some(error),
        }
    }
}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug)]
pub struct DocumentManager {
    project_root: PathBuf,
    recovery_root: PathBuf,
    documents: HashMap<PathBuf, DocumentHandle>,
}

impl DocumentManager {
    pub fn new(project_root: PathBuf, recovery_root: PathBuf) -> io::Result<Self> {
        let project_root = project_root.canonicalize()?;
        fs::create_dir_all(&recovery_root)?;
        Ok(Self {
            project_root,
            recovery_root,
            documents: HashMap::new(),
        })
    }

    pub fn open(&mut self, relative_path: impl AsRef<Path>) -> io::Result<DocumentHandle> {
        let relative_path = checked_relative(relative_path.as_ref())?;
        if !is_native_authoring_path(&relative_path) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "only native config.yaml and scripts/*.txt sources are writable",
            ));
        }
        if let Some(document) = self.documents.get(&relative_path) {
            return Ok(document.clone());
        }

        let absolute_path = self.project_root.join(&relative_path);
        let canonical = absolute_path.canonicalize()?;
        if !canonical.starts_with(&self.project_root) || !canonical.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "document escapes the project root",
            ));
        }
        let bytes = read_source(&canonical)?;
        let disk_contents = String::from_utf8(bytes).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "document is not valid UTF-8")
        })?;
        let recovery_path = self.recovery_path(&relative_path);
        let (contents, revision, recovery_state) =
            load_recovery(&recovery_path, &relative_path, &disk_contents);
        let saved_revision = if recovery_state == RecoveryState::Restored {
            0
        } else {
            revision
        };
        let document = Rc::new(RefCell::new(SourceDocument {
            relative_path: relative_path.clone(),
            absolute_path: canonical,
            recovery_path,
            contents,
            disk_contents,
            revision,
            saved_revision,
            recovery_state,
            selection: SourceSelection { line: 0, column: 0 },
        }));
        self.documents.insert(relative_path, document.clone());
        Ok(document)
    }

    pub fn has_dirty_documents(&self) -> bool {
        self.documents
            .values()
            .any(|document| document.borrow().is_dirty())
    }

    pub fn save_all(&mut self) -> Result<usize, SaveError> {
        let mut saved = 0;
        for document in self.documents.values() {
            let mut document = document.borrow_mut();
            if document.is_dirty() {
                document.save()?;
                saved += 1;
            }
        }
        Ok(saved)
    }

    pub fn documents(&self) -> impl Iterator<Item = &DocumentHandle> {
        self.documents.values()
    }

    fn recovery_path(&self, relative_path: &Path) -> PathBuf {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in relative_path.to_string_lossy().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        self.recovery_root.join(format!("{hash:016x}.draft"))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RecoveryDraft {
    schema: u32,
    relative_path: PathBuf,
    base_contents: String,
    revision: u64,
    contents: String,
}

fn checked_relative(path: &Path) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "document path must be a confined relative path",
        ));
    }
    Ok(path.to_owned())
}

fn is_native_authoring_path(path: &Path) -> bool {
    path == Path::new("config.yaml")
        || (path.starts_with("scripts")
            && path.extension().is_some_and(|extension| extension == "txt"))
}

fn read_source(path: &Path) -> io::Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_DOCUMENT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "document exceeds the 1 MiB editor limit",
        ));
    }
    fs::read(path)
}

fn ensure_document_size(contents: &str) -> io::Result<()> {
    if contents.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "document exceeds the 1 MiB editor limit",
        ));
    }
    Ok(())
}

fn load_recovery(
    path: &Path,
    relative_path: &Path,
    disk_contents: &str,
) -> (String, u64, RecoveryState) {
    let Some(draft) = read_recovery(path)
        .ok()
        .flatten()
        .and_then(|bytes| postcard::from_bytes::<RecoveryDraft>(&bytes).ok())
        .filter(|draft| draft.schema == RECOVERY_SCHEMA && draft.relative_path == relative_path)
    else {
        return (disk_contents.to_owned(), 0, RecoveryState::None);
    };
    if draft.base_contents != disk_contents {
        return (disk_contents.to_owned(), 0, RecoveryState::Conflicted);
    }
    if draft.contents == disk_contents {
        let _ = remove_if_present(path);
        return (
            disk_contents.to_owned(),
            draft.revision,
            RecoveryState::None,
        );
    }
    (
        draft.contents,
        draft.revision.max(1),
        RecoveryState::Restored,
    )
}

fn read_recovery(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.len() > MAX_RECOVERY_BYTES {
        return Ok(None);
    }
    fs::read(path).map(Some)
}

fn atomic_source(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "source path has no parent"))?;
    let permissions = fs::metadata(path)?.permissions();
    let temporary = temporary_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.set_permissions(permissions)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        replace(&temporary, path)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn atomic_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "recovery path has no parent")
    })?;
    fs::create_dir_all(parent)?;
    let temporary = temporary_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        replace(&temporary, path)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document");
    let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(".{name}.tmp-{}-{nonce}", std::process::id()))
}

#[cfg(not(target_os = "windows"))]
fn replace(temporary: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(target_os = "windows")]
fn replace(temporary: &Path, destination: &Path) -> io::Result<()> {
    let previous = destination.with_extension("keine-editor-previous");
    let _ = fs::remove_file(&previous);
    fs::rename(destination, &previous)?;
    match fs::rename(temporary, destination) {
        Ok(()) => {
            let _ = fs::remove_file(previous);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(previous, destination);
            Err(error)
        }
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_: &Path) -> io::Result<()> {
    Ok(())
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, PathBuf, DocumentManager) {
        let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "keine-editor-document-{}-{nonce}",
            std::process::id()
        ));
        let project = root.join("project");
        let recovery = root.join("app-data/recovery");
        fs::create_dir_all(project.join("scripts")).unwrap();
        fs::write(project.join("config.yaml"), "title: Fixture\n").unwrap();
        fs::write(
            project.join("scripts/main.txt"),
            "intro:hello;\nfutureCommand:opaque payload;\n",
        )
        .unwrap();
        let manager = DocumentManager::new(project.clone(), recovery).unwrap();
        (root, project, manager)
    }

    #[test]
    fn opening_without_changes_preserves_exact_bytes() {
        let (root, project, mut manager) = fixture();
        let path = project.join("scripts/main.txt");
        let before = fs::read(&path).unwrap();
        let document = manager.open("scripts/main.txt").unwrap();
        assert!(!document.borrow().is_dirty());
        drop(document);
        drop(manager);
        assert_eq!(fs::read(path).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_round_trips_unknown_source_and_clears_recovery() {
        let (root, project, mut manager) = fixture();
        let document = manager.open("scripts/main.txt").unwrap();
        let mut contents = document.borrow().contents().to_owned();
        contents.push_str("narrator:追加文本;\n");
        document.borrow_mut().replace_contents(contents.clone());
        document.borrow_mut().persist_recovery().unwrap();
        assert!(document.borrow().recovery_path.is_file());
        document.borrow_mut().save().unwrap();
        assert_eq!(
            fs::read_to_string(project.join("scripts/main.txt")).unwrap(),
            contents
        );
        assert!(!document.borrow().recovery_path.exists());
        assert!(!document.borrow().is_dirty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn external_modification_is_never_overwritten() {
        let (root, project, mut manager) = fixture();
        let document = manager.open("scripts/main.txt").unwrap();
        document
            .borrow_mut()
            .replace_contents("editor version".into());
        fs::write(project.join("scripts/main.txt"), "external version").unwrap();
        assert!(matches!(
            document.borrow_mut().save(),
            Err(SaveError::ExternalModification { .. })
        ));
        assert_eq!(
            fs::read_to_string(project.join("scripts/main.txt")).unwrap(),
            "external version"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_restores_only_against_the_same_disk_base() {
        let (root, project, mut manager) = fixture();
        let document = manager.open("scripts/main.txt").unwrap();
        document.borrow_mut().replace_contents("draft".into());
        document.borrow_mut().persist_recovery().unwrap();
        drop(document);
        drop(manager);

        let mut reopened =
            DocumentManager::new(project.clone(), root.join("app-data/recovery")).unwrap();
        let document = reopened.open("scripts/main.txt").unwrap();
        assert_eq!(document.borrow().contents(), "draft");
        assert_eq!(document.borrow().recovery_state(), RecoveryState::Restored);
        drop(document);
        drop(reopened);

        fs::write(project.join("scripts/main.txt"), "external version").unwrap();
        let mut conflicted = DocumentManager::new(project, root.join("app-data/recovery")).unwrap();
        let document = conflicted.open("scripts/main.txt").unwrap();
        assert_eq!(document.borrow().contents(), "external version");
        assert_eq!(
            document.borrow().recovery_state(),
            RecoveryState::Conflicted
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manager_reuses_one_authoritative_document() {
        let (root, _, mut manager) = fixture();
        let first = manager.open("scripts/main.txt").unwrap();
        let second = manager.open("scripts/main.txt").unwrap();
        assert!(Rc::ptr_eq(&first, &second));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compatibility_json_is_not_a_writable_document() {
        let (root, project, mut manager) = fixture();
        fs::write(project.join("project.json"), "{}\n").unwrap();
        let error = manager.open("project.json").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn edited_sources_and_recovery_inputs_remain_bounded() {
        let (root, project, mut manager) = fixture();
        let document = manager.open("scripts/main.txt").unwrap();
        let recovery_path = document.borrow().recovery_path.clone();
        document
            .borrow_mut()
            .replace_contents("x".repeat(MAX_DOCUMENT_BYTES as usize + 1));
        assert_eq!(
            document.borrow_mut().persist_recovery().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(matches!(
            document.borrow_mut().save(),
            Err(SaveError::Io(error)) if error.kind() == io::ErrorKind::InvalidData
        ));
        drop(document);
        drop(manager);

        fs::write(&recovery_path, vec![0; MAX_RECOVERY_BYTES as usize + 1]).unwrap();
        let mut reopened = DocumentManager::new(project, root.join("app-data/recovery")).unwrap();
        let document = reopened.open("scripts/main.txt").unwrap();
        assert_eq!(document.borrow().recovery_state(), RecoveryState::None);
        fs::remove_dir_all(root).unwrap();
    }
}
