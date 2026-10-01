//! Shared source documents, derived authoring indexes and selections.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::{fs, io};

use gpui_kit::component::dock::{DockArea, NodeId, PanelId};
use gpui_kit::component::input::EditorState;
use gpui_kit::{App, Global, WeakEntity};

use crate::authoring::{
    AssetKey, AssetKind, AuthoringIndex, UnmappedAsset, confined_existing_file,
};
use crate::document::{DocumentHandle, DocumentManager, SaveError};
use crate::persistence::{AppPersistence, BlockPickerPreferences};
use crate::preview::PreviewController;
use crate::project_key::ProjectKey;
use crate::projection::EiyashouProjection;
use crate::workspace::WorkspaceFile;

use super::edits::SourceHistory;
use super::panel::WorkbenchPanel;

pub(super) struct WorkspaceDocuments {
    pub(super) manager: DocumentManager,
    pub(super) files: Vec<WorkspaceFile>,
    pub(super) authoring: Arc<AuthoringIndex>,
    pub(super) index_epoch: u64,
    pending_index: BTreeMap<PathBuf, String>,
    index_task: Option<gpui_kit::Task<()>>,
    force_index_reload: bool,
    notice: String,
    selection: Option<(PathBuf, usize, usize)>,
    pub(super) block_selection: Option<(PathBuf, Vec<usize>)>,
    asset_selection: Vec<AssetKey>,
    asset_preview: Option<AssetPreviewSelection>,
    diagnostics: Vec<keine_authoring::Diagnostic>,
    pub(super) source_history: SourceHistory,
    pub(super) dock: Option<WeakEntity<DockArea>>,
    document_node: Option<NodeId>,
    panels: HashMap<PathBuf, PanelId>,
    pub(super) panel_entities: HashMap<PathBuf, WeakEntity<WorkbenchPanel>>,
    editors: HashMap<PathBuf, WeakEntity<EditorState>>,
    pub(super) preview: Arc<PreviewController>,
    tools: HashMap<&'static str, PanelId>,
    pub(super) file_operation_active: bool,
}

pub(super) fn schedule_authoring_refresh(root: &Path, path: Option<&Path>, cx: &mut App) {
    let root = root.to_owned();
    let Some(workspace) = cx.global_mut::<EditorDocuments>().workspaces.get_mut(&root) else {
        return;
    };
    if let Some(path) = path {
        let Some(document) = workspace.manager.document(path) else {
            return;
        };
        workspace
            .pending_index
            .insert(path.to_owned(), document.borrow().contents().to_owned());
    } else {
        workspace.force_index_reload = true;
    }
    workspace.index_epoch = workspace.index_epoch.wrapping_add(1);
    let epoch = workspace.index_epoch;
    workspace.index_task.take();
    let task_root = root.clone();
    let background = cx.background_executor().clone();
    let task = cx.spawn(async move |cx| {
        let Some(input) = cx.update(|cx| {
            let workspace = cx.global::<EditorDocuments>().workspaces.get(&task_root)?;
            let pending = workspace.pending_index.clone();
            let full = workspace.force_index_reload
                || pending
                    .keys()
                    .any(|path| path.extension().is_none_or(|ext| ext != "shou"));
            Some((
                workspace.authoring.clone(),
                workspace.files.clone(),
                pending,
                full.then(|| workspace.manager.source_overrides()),
            ))
        }) else {
            return;
        };
        let calculation_root = task_root.clone();
        let index = background
            .spawn(async move {
                let (previous, files, pending, full) = input;
                if let Some(overrides) = full {
                    AuthoringIndex::load(&calculation_root, &files, &overrides)
                } else {
                    previous.with_sources(&pending)
                }
            })
            .await;
        cx.update(|cx| {
            if let Some(workspace) = cx
                .global_mut::<EditorDocuments>()
                .workspaces
                .get_mut(&task_root)
                && workspace.index_epoch == epoch
            {
                workspace.authoring = Arc::new(index);
                workspace.pending_index.clear();
                workspace.force_index_reload = false;
                refresh_asset_preview(&task_root, workspace);
                cx.refresh_windows();
            }
        });
    });
    if let Some(workspace) = cx.global_mut::<EditorDocuments>().workspaces.get_mut(&root) {
        workspace.index_task = Some(task);
    }
}

#[derive(Clone)]
pub(super) struct AssetPreviewSelection {
    pub(super) kind: AssetKind,
    pub(super) label: String,
    pub(super) path: PathBuf,
    pub(super) file: Option<PathBuf>,
}

fn asset_preview_selection(
    root: &Path,
    kind: AssetKind,
    label: String,
    path: PathBuf,
) -> AssetPreviewSelection {
    let file = confined_existing_file(root, &path);
    AssetPreviewSelection {
        kind,
        label,
        path,
        file,
    }
}

fn refresh_asset_preview(root: &Path, workspace: &mut WorkspaceDocuments) {
    if let [key] = workspace.asset_selection.as_slice() {
        workspace.asset_preview = workspace
            .authoring
            .assets
            .iter()
            .find(|asset| asset.key() == *key)
            .map(|asset| {
                asset_preview_selection(root, asset.kind, asset.id.clone(), asset.path.clone())
            });
    } else if workspace.asset_selection.is_empty() {
        let previous = workspace.asset_preview.take();
        workspace.asset_preview = previous.and_then(|previous| {
            if let Some(asset) = workspace
                .authoring
                .assets
                .iter()
                .find(|asset| asset.kind == previous.kind && asset.path == previous.path)
            {
                return Some(asset_preview_selection(
                    root,
                    asset.kind,
                    previous.label,
                    asset.path.clone(),
                ));
            }
            workspace
                .authoring
                .unmapped
                .iter()
                .find(|asset| asset.kind == previous.kind && asset.path == previous.path)
                .map(|asset| {
                    asset_preview_selection(root, asset.kind, previous.label, asset.path.clone())
                })
        });
    } else {
        workspace.asset_preview = None;
    }
}

pub(super) struct EditorDocuments {
    persistence: AppPersistence,
    block_picker_preferences: BlockPickerPreferences,
    pub(super) workspaces: HashMap<PathBuf, WorkspaceDocuments>,
    pub(super) resource_picker_epoch: u64,
}

impl Global for EditorDocuments {}

impl EditorDocuments {
    pub(super) fn new(persistence: AppPersistence) -> Self {
        let block_picker_preferences = persistence.load_block_picker_preferences();
        Self {
            persistence,
            block_picker_preferences,
            workspaces: HashMap::new(),
            resource_picker_epoch: 0,
        }
    }

    pub(super) fn workspace_mut(&mut self, root: &Path) -> io::Result<&mut WorkspaceDocuments> {
        self.workspaces
            .get_mut(root)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Project is closed"))
    }

    pub(super) fn ensure_workspace_with_files(
        &mut self,
        root: &Path,
        discovered: &[WorkspaceFile],
    ) -> io::Result<&mut WorkspaceDocuments> {
        let key = ProjectKey::from_path(root)?;
        let canonical = key.path().to_owned();
        if !self.workspaces.contains_key(&canonical) {
            let manager =
                DocumentManager::new(canonical.clone(), self.persistence.recovery_dir(&key))?;
            let files = discovered.to_vec();
            let authoring = AuthoringIndex::load(&canonical, &files, &BTreeMap::new());
            self.workspaces.insert(
                canonical.clone(),
                WorkspaceDocuments {
                    manager,
                    files,
                    authoring: Arc::new(authoring),
                    index_epoch: 0,
                    pending_index: BTreeMap::new(),
                    index_task: None,
                    force_index_reload: false,
                    notice: "Ready".into(),
                    selection: None,
                    block_selection: None,
                    asset_selection: Vec::new(),
                    asset_preview: None,
                    diagnostics: Vec::new(),
                    source_history: SourceHistory::default(),
                    dock: None,
                    document_node: None,
                    panels: HashMap::new(),
                    panel_entities: HashMap::new(),
                    editors: HashMap::new(),
                    preview: PreviewController::new(key),
                    tools: HashMap::new(),
                    file_operation_active: false,
                },
            );
        }
        Ok(self
            .workspaces
            .get_mut(&canonical)
            .expect("workspace inserted above"))
    }

    pub(super) fn open(&mut self, root: &Path, relative: &Path) -> io::Result<DocumentHandle> {
        self.workspace_mut(root)?.manager.open(relative)
    }

    pub(super) fn has_dirty_documents(&self, root: &Path) -> bool {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .is_some_and(|workspace| workspace.manager.has_dirty_documents())
    }

    pub(super) fn save_all(&mut self, root: &Path) -> Result<usize, SaveError> {
        let workspace = self.workspace_mut(root).map_err(SaveError::from)?;
        if workspace.file_operation_active {
            return Err(io::Error::other(
                "A file operation is still running; save when it finishes",
            )
            .into());
        }
        workspace.manager.save_all()
    }

    pub(super) fn set_notice(&mut self, root: &Path, notice: impl Into<String>) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.notice = notice.into();
        }
    }

    pub(super) fn notice(&self, root: &Path) -> Option<&str> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces
            .get(key.path())
            .map(|state| state.notice.as_str())
    }

    pub(super) fn set_selection(
        &mut self,
        root: &Path,
        relative: PathBuf,
        line: usize,
        column: usize,
    ) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.selection = Some((relative, line, column));
        }
    }

    pub(super) fn set_block_selection(
        &mut self,
        root: &Path,
        relative: PathBuf,
        mut starts: Vec<usize>,
    ) {
        starts.sort_unstable();
        starts.dedup();
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.block_selection = (!starts.is_empty()).then_some((relative, starts));
            workspace.asset_selection.clear();
            workspace.asset_preview = None;
            workspace.preview.stop_audition();
        }
    }

    pub(super) fn clear_block_selection(&mut self, root: &Path) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.block_selection = None;
        }
    }

    pub(super) fn asset_selection(&self, root: &Path) -> Vec<AssetKey> {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .map(|workspace| workspace.asset_selection.clone())
            .unwrap_or_default()
    }

    pub(super) fn set_asset_selection(&mut self, root: &Path, selection: Vec<AssetKey>) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.asset_preview = selection
                .first()
                .filter(|_| selection.len() == 1)
                .and_then(|key| {
                    workspace
                        .authoring
                        .assets
                        .iter()
                        .find(|asset| asset.key() == *key)
                })
                .map(|asset| {
                    asset_preview_selection(root, asset.kind, asset.id.clone(), asset.path.clone())
                });
            workspace.preview.stop_audition();
            workspace.asset_selection = selection;
        }
    }

    pub(super) fn preview_asset(
        &mut self,
        root: &Path,
        kind: AssetKind,
        label: String,
        path: PathBuf,
    ) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.asset_preview = Some(asset_preview_selection(root, kind, label, path));
        }
    }

    pub(super) fn set_unmapped_asset_preview(&mut self, root: &Path, asset: &UnmappedAsset) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.preview.stop_audition();
            workspace.asset_selection.clear();
            workspace.asset_preview = Some(asset_preview_selection(
                root,
                asset.kind,
                asset
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                asset.path.clone(),
            ));
        }
    }

    pub(super) fn asset_preview(&self, root: &Path) -> Option<AssetPreviewSelection> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces.get(key.path())?.asset_preview.clone()
    }

    pub(super) fn clear_asset_selection(&mut self, root: &Path) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.asset_selection.clear();
            workspace.asset_preview = None;
        }
    }

    pub(super) fn block_selection(&self, root: &Path) -> Option<&(PathBuf, Vec<usize>)> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces.get(key.path())?.block_selection.as_ref()
    }

    pub(super) fn block_picker_preferences(&self) -> &BlockPickerPreferences {
        &self.block_picker_preferences
    }

    pub(super) fn update_block_picker_preferences(
        &mut self,
        update: impl FnOnce(&mut BlockPickerPreferences),
    ) -> io::Result<()> {
        update(&mut self.block_picker_preferences);
        self.persistence
            .save_block_picker_preferences(&self.block_picker_preferences)
    }

    pub(super) fn asset_manifest_is_clean(&mut self, root: &Path) -> bool {
        let Ok(workspace) = self.workspace_mut(root) else {
            return false;
        };
        let Some(path) = workspace.authoring.assets_manifest.as_ref() else {
            return false;
        };
        workspace
            .manager
            .document(path)
            .is_none_or(|document| !document.borrow().is_dirty())
    }

    pub(super) fn has_open_documents_under(&mut self, root: &Path, path: &Path) -> bool {
        self.workspace_mut(root).is_ok_and(|workspace| {
            workspace.editors.iter().any(|(relative, editor)| {
                (relative == path || relative.starts_with(path)) && editor.upgrade().is_some()
            }) || workspace.manager.documents().any(|document| {
                let document = document.borrow();
                (document.relative_path() == path || document.relative_path().starts_with(path))
                    && document.is_dirty()
            })
        })
    }

    pub(super) fn adopt_manifest_update(
        &mut self,
        root: &Path,
        relative: &Path,
        source: String,
    ) -> io::Result<Option<WeakEntity<EditorState>>> {
        let workspace = self.workspace_mut(root)?;
        if let Some(document) = workspace.manager.document(relative) {
            if document.borrow().is_dirty() {
                return Err(io::Error::other(
                    "Asset manifest changed while files were imported; your draft is preserved. Reload or reconcile before saving.",
                ));
            }
            document.borrow_mut().adopt_saved_contents(source)?;
        }
        Ok(workspace.editors.get(relative).cloned())
    }

    pub(super) fn authoring(&self, root: &Path) -> Arc<AuthoringIndex> {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .map(|workspace| workspace.authoring.clone())
            .unwrap_or_default()
    }

    pub(super) fn authoring_ref(&self, root: &Path) -> Option<&AuthoringIndex> {
        let key = ProjectKey::from_path(root).ok()?;
        Some(&self.workspaces.get(key.path())?.authoring)
    }

    pub(super) fn authoring_is_current(&self, root: &Path) -> bool {
        self.workspaces.get(root).is_some_and(|workspace| {
            workspace.pending_index.is_empty() && !workspace.force_index_reload
        })
    }

    pub(super) fn explicit_source_ids(&self, root: &Path) -> HashSet<String> {
        let Ok(key) = ProjectKey::from_path(root) else {
            return HashSet::new();
        };
        let Some(workspace) = self.workspaces.get(key.path()) else {
            return HashSet::new();
        };
        let overrides = workspace.manager.source_overrides();
        workspace
            .files
            .iter()
            .filter(|file| {
                file.relative_path
                    .extension()
                    .is_some_and(|value| value == "shou")
            })
            .filter_map(|file| {
                overrides
                    .get(&file.relative_path)
                    .cloned()
                    .or_else(|| fs::read_to_string(root.join(&file.relative_path)).ok())
            })
            .flat_map(|source| {
                EiyashouProjection::parse(&source)
                    .scenes
                    .into_iter()
                    .flat_map(|scene| scene.blocks)
                    .filter_map(|block| block.stable_id)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub(super) fn source(&self, root: &Path, relative: &Path) -> Option<String> {
        let key = ProjectKey::from_path(root).ok()?;
        let workspace = self.workspaces.get(key.path())?;
        workspace
            .manager
            .document(relative)
            .map(|document| document.borrow().contents().to_owned())
            .or_else(|| fs::read_to_string(root.join(relative)).ok())
    }

    pub(super) fn projection(
        &self,
        root: &Path,
        relative: &Path,
        source: &str,
    ) -> Rc<EiyashouProjection> {
        if let Some(document) = self
            .workspaces
            .get(root)
            .and_then(|workspace| workspace.manager.document(relative))
            && document.borrow().contents() == source
        {
            return document.borrow().projection();
        }
        Rc::new(EiyashouProjection::parse(source))
    }

    pub(super) fn selection(&self, root: &Path) -> Option<&(PathBuf, usize, usize)> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces.get(key.path())?.selection.as_ref()
    }

    pub(super) fn set_diagnostics(
        &mut self,
        root: &Path,
        diagnostics: Vec<keine_authoring::Diagnostic>,
    ) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.diagnostics = diagnostics;
        }
    }

    pub(super) fn set_dock(&mut self, root: &Path, dock: WeakEntity<DockArea>) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.dock = Some(dock);
        }
    }

    pub(super) fn set_document_node(&mut self, root: &Path, node: NodeId) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.document_node = Some(node);
        }
    }

    pub(super) fn clear_document_node(&mut self, root: &Path) {
        if let Some(workspace) = self.workspaces.get_mut(root) {
            workspace.document_node = None;
        }
    }

    pub(super) fn register_panel(
        &mut self,
        root: &Path,
        relative: PathBuf,
        panel: PanelId,
        entity: WeakEntity<WorkbenchPanel>,
    ) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.panels.insert(relative.clone(), panel);
            workspace.panel_entities.insert(relative, entity);
        }
    }

    pub(super) fn panel_entity_for(
        &self,
        root: &Path,
        relative: &Path,
    ) -> Option<WeakEntity<WorkbenchPanel>> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces
            .get(key.path())?
            .panel_entities
            .get(relative)
            .cloned()
    }

    pub(super) fn register_editor(
        &mut self,
        root: &Path,
        relative: PathBuf,
        editor: WeakEntity<EditorState>,
    ) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.editors.insert(relative, editor);
        }
    }

    pub(super) fn editor_for(
        &self,
        root: &Path,
        relative: &Path,
    ) -> Option<WeakEntity<EditorState>> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces
            .get(key.path())?
            .editors
            .get(relative)
            .cloned()
    }

    pub(super) fn unregister_panel(&mut self, root: &Path, relative: &Path, panel: PanelId) {
        if let Some(workspace) = self.workspaces.get_mut(root)
            && workspace.panels.get(relative) == Some(&panel)
        {
            workspace.panels.remove(relative);
            workspace.panel_entities.remove(relative);
            workspace.editors.remove(relative);
            workspace.manager.release_clean(relative);
        }
    }

    pub(super) fn document_dock(
        &self,
        root: &Path,
    ) -> Option<(WeakEntity<DockArea>, Option<NodeId>)> {
        let key = ProjectKey::from_path(root).ok()?;
        let workspace = self.workspaces.get(key.path())?;
        Some((workspace.dock.clone()?, workspace.document_node))
    }

    pub(super) fn document_panels(&self, root: &Path) -> Vec<PanelId> {
        let Ok(key) = ProjectKey::from_path(root) else {
            return Vec::new();
        };
        self.workspaces
            .get(key.path())
            .map(|workspace| workspace.panels.values().copied().collect())
            .unwrap_or_default()
    }

    pub(super) fn panel_for(&self, root: &Path, relative: &Path) -> Option<PanelId> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces
            .get(key.path())?
            .panels
            .get(relative)
            .copied()
    }

    pub(super) fn open_document_count(&self, root: &Path) -> usize {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .map(|workspace| workspace.panels.len())
            .unwrap_or_default()
    }

    pub(super) fn diagnostics_for<'a>(
        &'a self,
        root: &Path,
        relative: &Path,
    ) -> impl Iterator<Item = &'a keine_authoring::Diagnostic> {
        let diagnostics = ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .map(|workspace| workspace.diagnostics.as_slice())
            .unwrap_or_default();
        diagnostics.iter().filter(move |diagnostic| {
            diagnostic.path == relative || diagnostic.path.ends_with(relative)
        })
    }

    pub(super) fn runtime_diagnostics(&self, root: &Path) -> Vec<keine_authoring::Diagnostic> {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .map(|workspace| workspace.diagnostics.clone())
            .unwrap_or_default()
    }

    pub(super) fn preview(&mut self, root: &Path) -> io::Result<Arc<PreviewController>> {
        self.workspaces
            .get(root)
            .map(|workspace| workspace.preview.clone())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "project session is closed"))
    }

    pub(super) fn preview_documents(&self, root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let Ok(key) = ProjectKey::from_path(root) else {
            return Vec::new();
        };
        let Some(workspace) = self.workspaces.get(key.path()) else {
            return Vec::new();
        };
        workspace
            .manager
            .documents()
            .filter_map(|document| {
                let document = document.borrow();
                (document
                    .relative_path()
                    .extension()
                    .and_then(|value| value.to_str())
                    == Some("shou"))
                .then(|| {
                    (
                        document.relative_path().to_owned(),
                        document.contents().as_bytes().to_vec(),
                    )
                })
            })
            .collect()
    }

    pub(super) fn tool_panel(&self, root: &Path, name: &'static str) -> Option<PanelId> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces.get(key.path())?.tools.get(name).copied()
    }

    pub(super) fn set_tool_panel(
        &mut self,
        root: &Path,
        name: &'static str,
        panel: Option<PanelId>,
    ) {
        if let Ok(workspace) = self.workspace_mut(root) {
            if let Some(panel) = panel {
                workspace.tools.insert(name, panel);
            } else {
                workspace.tools.remove(name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::WorkspaceSession;

    #[test]
    fn late_project_notifications_do_not_recreate_a_released_session() {
        let temporary = std::env::temp_dir().join(format!(
            "keine-editor-closed-session-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = temporary.join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("config.yaml"),
            include_str!("../../../../tests/fixtures/native-smoke/config.yaml"),
        )
        .unwrap();
        let session = WorkspaceSession::open(&project).unwrap();
        let mut documents = EditorDocuments::new(AppPersistence::new(temporary.join("app-data")));
        documents
            .ensure_workspace_with_files(session.root(), session.files())
            .unwrap();
        assert_eq!(documents.workspaces.len(), 1);
        documents.workspaces.remove(session.root());

        documents.set_notice(session.root(), "Late worker result");
        documents.set_selection(session.root(), "scripts/main.shou".into(), 0, 0);
        documents.set_diagnostics(session.root(), Vec::new());
        documents.record_source_edit(session.root(), Path::new("scripts/main.shou"), "a", "b");
        assert!(documents.preview(session.root()).is_err());
        assert!(documents.workspaces.is_empty());
        fs::remove_dir_all(temporary).unwrap();
    }
}
