use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use keine_core::config::GameConfig;

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

    pub(crate) fn adopt_saved_contents(&mut self, contents: String) -> io::Result<()> {
        ensure_document_size(&contents)?;
        self.contents = contents.clone();
        self.disk_contents = contents;
        self.revision = self.revision.wrapping_add(1).max(1);
        self.saved_revision = self.revision;
        self.recovery_state = RecoveryState::None;
        remove_if_present(&self.recovery_path)
    }

    pub fn replace_range(
        &mut self,
        range: std::ops::Range<usize>,
        replacement: &str,
    ) -> io::Result<bool> {
        if range.start > range.end
            || !self.contents.is_char_boundary(range.start)
            || !self.contents.is_char_boundary(range.end)
            || self.contents.get(range.clone()).is_none()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source edit range is stale or not on UTF-8 boundaries",
            ));
        }
        let mut contents = self.contents.clone();
        contents.replace_range(range, replacement);
        ensure_document_size(&contents)?;
        Ok(self.replace_contents(contents))
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
    policy: AuthoringPolicy,
}

impl DocumentManager {
    pub fn new(project_root: PathBuf, recovery_root: PathBuf) -> io::Result<Self> {
        let project_root = project_root.canonicalize()?;
        fs::create_dir_all(&recovery_root)?;
        Ok(Self {
            policy: AuthoringPolicy::load(&project_root),
            project_root,
            recovery_root,
            documents: HashMap::new(),
        })
    }

    pub fn open(&mut self, relative_path: impl AsRef<Path>) -> io::Result<DocumentHandle> {
        let relative_path = checked_relative(relative_path.as_ref())?;
        if !self.policy.is_writable(&relative_path) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "only Eiyashou config, configured manifests, and scripts/**/*.shou are writable",
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

    pub fn document(&self, relative_path: &Path) -> Option<DocumentHandle> {
        self.documents.get(relative_path).cloned()
    }

    pub fn source_overrides(&self) -> std::collections::BTreeMap<PathBuf, String> {
        self.documents
            .iter()
            .map(|(path, document)| (path.clone(), document.borrow().contents().to_owned()))
            .collect()
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

#[derive(Clone, Debug, Default)]
struct AuthoringPolicy {
    enabled: bool,
    assets: PathBuf,
    characters: PathBuf,
}

impl AuthoringPolicy {
    fn load(project_root: &Path) -> Self {
        let Ok(source) = fs::read_to_string(project_root.join("config.yaml")) else {
            return Self::default();
        };
        let Ok(config) = GameConfig::from_yaml(&source) else {
            return Self::default();
        };
        if config.adapter.script != "keine" {
            return Self::default();
        }
        Self {
            enabled: true,
            assets: PathBuf::from(config.script.assets),
            characters: PathBuf::from(config.script.characters),
        }
    }

    fn is_writable(&self, path: &Path) -> bool {
        self.enabled
            && (path == Path::new("config.yaml")
                || path == self.assets
                || path == self.characters
                || (path.starts_with("scripts")
                    && path
                        .extension()
                        .is_some_and(|extension| extension == "shou")))
    }
}

pub fn is_eiyashou_authoring_document(project_root: &Path, relative: &Path) -> bool {
    AuthoringPolicy::load(project_root).is_writable(relative)
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

pub(crate) fn atomic_source(path: &Path, bytes: &[u8]) -> io::Result<()> {
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
        // The rename is the commit point. A later directory-sync failure is
        // a durability warning, not a failed write: callers may otherwise
        // remove an imported file while its new manifest is already live.
        if let Err(error) = sync_directory(parent) {
            eprintln!("Kēne Editor: source committed; directory sync failed: {error}");
        }
        Ok(())
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
        if let Err(error) = sync_directory(parent) {
            eprintln!("Kēne Editor: recovery committed; directory sync failed: {error}");
        }
        Ok(())
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
        fs::write(
            project.join("config.yaml"),
            "title: Fixture\nadapter:\n  script: keine\nscript:\n  version: 1\n  entry: opening\n",
        )
        .unwrap();
        fs::write(project.join("assets.yaml"), "backgrounds: {}\n").unwrap();
        fs::write(project.join("characters.yaml"), "characters: {}\n").unwrap();
        fs::write(
            project.join("scripts/main.shou"),
            "scene opening { \"hello\" }\n",
        )
        .unwrap();
        let manager = DocumentManager::new(project.clone(), recovery).unwrap();
        (root, project, manager)
    }

    #[test]
    fn opening_without_changes_preserves_exact_bytes() {
        let (root, project, mut manager) = fixture();
        let path = project.join("scripts/main.shou");
        let before = fs::read(&path).unwrap();
        let document = manager.open("scripts/main.shou").unwrap();
        assert!(!document.borrow().is_dirty());
        drop(document);
        drop(manager);
        assert_eq!(fs::read(path).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manifest_save_preserves_comments_order_and_entry_forms() {
        let (root, project, mut manager) = fixture();
        let source = concat!(
            "# author order stays authoritative\n",
            "voices:\n",
            "  hello:\n",
            "    path: assets/hello.opus\n",
            "    tags: [rin, chapter-1]\n",
            "backgrounds:\n",
            "  room: assets/room.webp\n",
        );
        let path = project.join("assets.yaml");
        fs::write(&path, source).unwrap();
        let document = manager.open("assets.yaml").unwrap();

        document.borrow_mut().save().unwrap();

        assert_eq!(fs::read_to_string(path).unwrap(), source);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_round_trips_unknown_source_and_clears_recovery() {
        let (root, project, mut manager) = fixture();
        let document = manager.open("scripts/main.shou").unwrap();
        let mut contents = document.borrow().contents().to_owned();
        contents.push_str("narrator:追加文本;\n");
        document.borrow_mut().replace_contents(contents.clone());
        document.borrow_mut().persist_recovery().unwrap();
        assert!(document.borrow().recovery_path.is_file());
        document.borrow_mut().save().unwrap();
        assert_eq!(
            fs::read_to_string(project.join("scripts/main.shou")).unwrap(),
            contents
        );
        assert!(!document.borrow().recovery_path.exists());
        assert!(!document.borrow().is_dirty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn external_modification_is_never_overwritten() {
        let (root, project, mut manager) = fixture();
        let document = manager.open("scripts/main.shou").unwrap();
        document
            .borrow_mut()
            .replace_contents("editor version".into());
        fs::write(project.join("scripts/main.shou"), "external version").unwrap();
        assert!(matches!(
            document.borrow_mut().save(),
            Err(SaveError::ExternalModification { .. })
        ));
        assert_eq!(
            fs::read_to_string(project.join("scripts/main.shou")).unwrap(),
            "external version"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_restores_only_against_the_same_disk_base() {
        let (root, project, mut manager) = fixture();
        let document = manager.open("scripts/main.shou").unwrap();
        document.borrow_mut().replace_contents("draft".into());
        document.borrow_mut().persist_recovery().unwrap();
        drop(document);
        drop(manager);

        let mut reopened =
            DocumentManager::new(project.clone(), root.join("app-data/recovery")).unwrap();
        let document = reopened.open("scripts/main.shou").unwrap();
        assert_eq!(document.borrow().contents(), "draft");
        assert_eq!(document.borrow().recovery_state(), RecoveryState::Restored);
        drop(document);
        drop(reopened);

        fs::write(project.join("scripts/main.shou"), "external version").unwrap();
        let mut conflicted = DocumentManager::new(project, root.join("app-data/recovery")).unwrap();
        let document = conflicted.open("scripts/main.shou").unwrap();
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
        let first = manager.open("scripts/main.shou").unwrap();
        let second = manager.open("scripts/main.shou").unwrap();
        assert!(Rc::ptr_eq(&first, &second));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn configured_manifests_share_the_writable_source_path() {
        let (root, project, manager) = fixture();
        drop(manager);
        fs::create_dir_all(project.join("manifests")).unwrap();
        fs::write(
            project.join("manifests/resources.yaml"),
            "backgrounds: {}\n",
        )
        .unwrap();
        fs::write(project.join("manifests/cast.yaml"), "characters: {}\n").unwrap();
        fs::write(
            project.join("config.yaml"),
            "title: Fixture\nadapter:\n  script: keine\nscript:\n  version: 1\n  entry: opening\n  assets: manifests/resources.yaml\n  characters: manifests/cast.yaml\n",
        )
        .unwrap();
        let mut manager = DocumentManager::new(project, root.join("app-data/recovery")).unwrap();
        assert!(manager.open("manifests/resources.yaml").is_ok());
        assert!(manager.open("manifests/cast.yaml").is_ok());
        let error = manager.open("scripts/legacy.txt").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bounded_edits_preserve_source_outside_the_range() {
        let (root, _, mut manager) = fixture();
        let document = manager.open("scripts/main.shou").unwrap();
        let before = document.borrow().contents().to_owned();
        let range = before.find("opening").unwrap()..before.find("opening").unwrap() + 7;
        assert!(document.borrow_mut().replace_range(range, "intro").unwrap());
        assert_eq!(document.borrow().contents(), "scene intro { \"hello\" }\n");
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
        let document = manager.open("scripts/main.shou").unwrap();
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
        let document = reopened.open("scripts/main.shou").unwrap();
        assert_eq!(document.borrow().recovery_state(), RecoveryState::None);
        fs::remove_dir_all(root).unwrap();
    }
}
