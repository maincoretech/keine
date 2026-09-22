use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::assets::IconName as AssetIconName;
use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::base::{InteractiveElementExt as _, Placement, ResizeHandleContext, ScrollbarMode};
use gpui_kit::component::dock::{
    AnyDrag, BasePanel, BasePanelView, DockArea, DockAreaRenderer, DockContext, DockEvent,
    DockLayout, DockPlacement, DockSkin, DragPanel, DropIndicator, DropPlaceholderBounds,
    InsertTarget, NodeId, Panel, PanelBuildContext, PanelEvent, PanelHandle, PanelId, PanelInfo,
    PanelState, PanelStyle, TabGroupContext, TabGroupRenderer, panel_handle, register_panel,
};
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState, Position};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, Theme, ThemeMode, WindowExt as _,
};
use gpui_kit::{
    Anchor, Animation, AnimationExt as _, AnyElement, AnyView, App, AppContext as _, Axis, Bounds,
    ClickEvent, ClipboardItem, Context, Div, DragMoveEvent, Element, Empty, Entity, EventEmitter,
    ExternalPaths, FocusHandle, Focusable, Global, Hsla, InteractiveElement, IntoElement,
    KeyBinding, MouseButton, MouseDownEvent, ObjectFit, ParentElement, PathPromptOptions, Pixels,
    Point, PromptButton, PromptLevel, Render, RenderImage, ScrollAnchor, ScrollHandle,
    SharedString, Stateful, Styled, Subscription, WeakEntity, Window, WindowBounds, WindowHandle,
    WindowOptions, actions, anchored, canvas, deferred, div, ease_out_quint, fill, hsla, img,
    linear_color_stop, linear_gradient, prelude::*, px, radians, rgb, size,
};
use serde::{Deserialize, Serialize};

use crate::app_data::APP_ID;
use crate::authoring::{
    AuthoringIndex, AuthoringSelection, InsertKind, ProblemSeverity, append_character,
    append_scene, dialogues_for_source, escape_eiyashou_string, insert_statement,
    replace_dialogue_text,
};
use crate::document::{DocumentHandle, DocumentManager, SaveError, is_eiyashou_authoring_document};
use crate::file_ops::{self, ImportResult};
use crate::instance::{InstanceReceiver, PrimaryInstance, Startup, acquire_or_forward};
use crate::migration::MigrationPlan;
use crate::persistence::{AppPersistence, BlockPickerPreferences};
use crate::preview::{PreviewController, PreviewLifecycle, PreviewMode, map_preview_point};
use crate::project_key::ProjectKey;
use crate::projection::{BlockKind, EiyashouProjection, MoveDirection, TextBlockMetadata};
use crate::syntax::eiyashou_highlighter_factory;
use crate::workspace::{WorkspaceEntryKind, WorkspaceFile, WorkspaceSession};

const CANVAS: u32 = 0x070809;
const CHROME: u32 = 0x0b0d10;
const PANEL: u32 = 0x101317;
const SURFACE: u32 = 0x191e23;
const SURFACE_HOVER: u32 = 0x222a31;
const BORDER: u32 = 0x2d343b;
const INK: u32 = 0xd8dadd;
const MUTED: u32 = 0x92979e;
const PRIMARY: u32 = 0xbaebff;
const PRIMARY_DIM: u32 = 0x243138;
const SUCCESS: u32 = 0x69c38d;
const LAYOUT_SCHEMA: usize = 1;
// Each Dock group owns one half-gap. Adjacent groups therefore have a 4 px
// gutter, while the window and activity rail contribute the matching other
// half at the workspace edge. Keep this as the single spacing owner instead
// of adding per-panel margins.
const VIEW_INSET_PX: f32 = 2.;
const VIEW_RADIUS_PX: f32 = 9.;
const ACTIVITY_RAIL_WIDTH_PX: f32 = 50.;
const ACTIVITY_ITEM_SIZE_PX: f32 = 36.;
const ACTIVITY_BRAND_SIZE_PX: f32 = 34.;
const ACTIVITY_ICON_SIZE_PX: f32 = 18.;
// GPUI reserves one leading digit plus input padding before the widest visible
// line number. Crop that reserve while keeping the built-in right margin as a
// distinct dark gap before source text.
const EDITOR_GUTTER_TRIM_PX: f32 = 18.;
const TAB_MOTION_DURATION: Duration = Duration::from_millis(140);
const PREVIEW_POLL_INTERVAL: Duration = Duration::from_millis(16);
const FILE_CONTEXT_MENU_WIDTH_PX: f32 = 144.;
const FILE_CONTEXT_MENU_HEIGHT_PX: f32 = 120.;

const EXPLORER_PANEL: &str = "keine.editor.explorer";
const DOCUMENT_PANEL: &str = "keine.editor.document";
const INSPECTOR_PANEL: &str = "keine.editor.inspector";
const OUTPUT_PANEL: &str = "keine.editor.output";
const PREVIEW_PANEL: &str = "keine.editor.preview";
const ASSETS_PANEL: &str = "keine.editor.assets";
const CHARACTERS_PANEL: &str = "keine.editor.characters";
const SCENES_PANEL: &str = "keine.editor.scenes";
const PROBLEMS_PANEL: &str = "keine.editor.problems";
const PERFORMANCE_PANEL: &str = "keine.editor.performance";

actions!(
    keine_editor,
    [
        OpenFolder,
        ResetLayout,
        Save,
        SaveAll,
        ToggleEngine,
        MigrateEiyashou,
        CopyBlocks,
        PasteBlocks,
        DeleteBlocks,
        BeginTextBlock,
        ToggleBlockPicker,
        BlockPickerNext,
        BlockPickerPrevious,
        AcceptBlockPicker,
        CloseBlockPicker,
        MoveBlocksUp,
        MoveBlocksDown
    ]
);

fn theme_color(value: u32) -> Hsla {
    rgb(value).into()
}

fn configure_dark_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    let theme = Theme::global_mut(cx);
    theme.background = theme_color(CANVAS);
    theme.foreground = theme_color(INK);
    theme.border = theme_color(BORDER);
    theme.muted = theme_color(SURFACE);
    theme.muted_foreground = theme_color(MUTED);
    theme.accent = theme_color(PRIMARY_DIM);
    theme.accent_foreground = theme_color(PRIMARY);
    theme.primary = theme_color(PRIMARY);
    theme.primary_hover = theme_color(0xd6f3ff);
    theme.primary_active = theme_color(0x9dd7ee);
    theme.primary_foreground = theme_color(0x121a1e);
    theme.secondary = theme_color(SURFACE);
    theme.secondary_hover = theme_color(SURFACE_HOVER);
    theme.secondary_active = theme_color(0x202428);
    theme.secondary_foreground = theme_color(0xbfc3c7);
    theme.success = theme_color(SUCCESS);
    theme.success_foreground = theme_color(0x0b1b11);
    theme.warning = theme_color(0xd2aa62);
    theme.warning_foreground = theme_color(0x211707);
    theme.danger = theme_color(0xdb7780);
    theme.danger_foreground = theme_color(0x230d11);
    theme.info = theme_color(PRIMARY);
    theme.info_foreground = theme_color(0x121a1e);
    theme.ring = theme_color(PRIMARY);
    theme.drag_border = theme_color(PRIMARY);
    theme.drop_target = hsla(0.55, 0.38, 0.72, 0.22);
    theme.selection = theme_color(0x29353b);
    theme.sidebar = theme_color(CHROME);
    theme.sidebar_border = theme_color(BORDER);
    theme.sidebar_foreground = theme_color(0x9da2a8);
    theme.sidebar_accent = theme_color(PRIMARY_DIM);
    theme.sidebar_accent_foreground = theme_color(PRIMARY);
    theme.tab = theme_color(PANEL);
    theme.tab_bar = theme_color(CHROME);
    theme.tab_bar_segmented = theme_color(SURFACE);
    theme.tab_foreground = theme_color(0x8e9399);
    theme.tab_active = theme_color(SURFACE);
    theme.tab_active_foreground = theme_color(INK);
    theme.title_bar = theme_color(CHROME);
    theme.title_bar_border = theme_color(BORDER);
    theme.status_bar = theme_color(CHROME);
    theme.status_bar_border = theme_color(BORDER);
    theme.scrollbar_mode = ScrollbarMode::Scrolling;
    theme.scrollbar = hsla(0.58, 0.04, 0.10, 0.08);
    theme.scrollbar_thumb = hsla(0.56, 0.06, 0.56, 0.32);
    theme.scrollbar_thumb_hover = hsla(0.55, 0.14, 0.70, 0.48);
    theme.input = theme_color(BORDER);
    Arc::make_mut(&mut theme.highlight_theme)
        .style
        .editor_gutter_background = Some(theme_color(0x050607));
    theme.popover = theme_color(SURFACE);
    theme.popover_foreground = theme_color(INK);
    theme.button = theme_color(SURFACE);
    theme.button_hover = theme_color(SURFACE_HOVER);
    theme.button_active = theme_color(0x202428);
    theme.button_foreground = theme_color(0xc1c4c8);
    theme.radius = px(7.);
    theme.radius_lg = px(10.);
    theme.shadow = false;
    Theme::sync_base(cx);
}

struct WindowRegistry<W> {
    windows: HashMap<ProjectKey, W>,
}

impl<W> Default for WindowRegistry<W> {
    fn default() -> Self {
        Self {
            windows: HashMap::new(),
        }
    }
}

impl<W: Copy> WindowRegistry<W> {
    fn existing(&self, project: &ProjectKey) -> Option<W> {
        self.windows.get(project).copied()
    }

    fn insert(&mut self, project: ProjectKey, window: W) {
        self.windows.insert(project, window);
    }
}

struct WorkspaceDocuments {
    manager: DocumentManager,
    files: Vec<WorkspaceFile>,
    authoring: AuthoringIndex,
    notice: String,
    selection: Option<(PathBuf, usize, usize)>,
    block_selection: Option<(PathBuf, Vec<usize>)>,
    diagnostics: Vec<keine_authoring::Diagnostic>,
    dock: Option<WeakEntity<DockArea>>,
    document_node: Option<NodeId>,
    panels: HashMap<PathBuf, PanelId>,
    editors: HashMap<PathBuf, WeakEntity<EditorState>>,
    preview: Arc<PreviewController>,
    preview_panel: Option<PanelId>,
    tools: HashMap<&'static str, PanelId>,
}

struct EditorDocuments {
    persistence: AppPersistence,
    block_picker_preferences: BlockPickerPreferences,
    workspaces: HashMap<PathBuf, WorkspaceDocuments>,
}

impl Global for EditorDocuments {}

impl EditorDocuments {
    fn new(persistence: AppPersistence) -> Self {
        let block_picker_preferences = persistence.load_block_picker_preferences();
        Self {
            persistence,
            block_picker_preferences,
            workspaces: HashMap::new(),
        }
    }

    fn ensure_workspace(&mut self, root: &Path) -> io::Result<&mut WorkspaceDocuments> {
        let key = ProjectKey::from_path(root)?;
        let canonical = key.path().to_owned();
        if !self.workspaces.contains_key(&canonical) {
            let manager =
                DocumentManager::new(canonical.clone(), self.persistence.recovery_dir(&key))?;
            let files = WorkspaceSession::open(&canonical)
                .map(|session| session.files().to_vec())
                .unwrap_or_default();
            let authoring = AuthoringIndex::load(&canonical, &files, &BTreeMap::new());
            self.workspaces.insert(
                canonical.clone(),
                WorkspaceDocuments {
                    manager,
                    files,
                    authoring,
                    notice: "Ready".into(),
                    selection: None,
                    block_selection: None,
                    diagnostics: Vec::new(),
                    dock: None,
                    document_node: None,
                    panels: HashMap::new(),
                    editors: HashMap::new(),
                    preview: PreviewController::new(key),
                    preview_panel: None,
                    tools: HashMap::new(),
                },
            );
        }
        Ok(self
            .workspaces
            .get_mut(&canonical)
            .expect("workspace inserted above"))
    }

    fn open(&mut self, root: &Path, relative: &Path) -> io::Result<DocumentHandle> {
        self.ensure_workspace(root)?.manager.open(relative)
    }

    fn has_dirty_documents(&self, root: &Path) -> bool {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .is_some_and(|workspace| workspace.manager.has_dirty_documents())
    }

    fn save_all(&mut self, root: &Path) -> Result<usize, SaveError> {
        self.ensure_workspace(root)
            .map_err(SaveError::from)?
            .manager
            .save_all()
    }

    fn set_notice(&mut self, root: &Path, notice: impl Into<String>) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            workspace.notice = notice.into();
        }
    }

    fn notice(&self, root: &Path) -> Option<&str> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces
            .get(key.path())
            .map(|state| state.notice.as_str())
    }

    fn set_selection(&mut self, root: &Path, relative: PathBuf, line: usize, column: usize) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            workspace.selection = Some((relative, line, column));
        }
    }

    fn set_block_selection(&mut self, root: &Path, relative: PathBuf, mut starts: Vec<usize>) {
        starts.sort_unstable();
        starts.dedup();
        if let Ok(workspace) = self.ensure_workspace(root) {
            workspace.block_selection = (!starts.is_empty()).then_some((relative, starts));
        }
    }

    fn clear_block_selection(&mut self, root: &Path) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            workspace.block_selection = None;
        }
    }

    fn block_selection(&self, root: &Path) -> Option<&(PathBuf, Vec<usize>)> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces.get(key.path())?.block_selection.as_ref()
    }

    fn block_picker_preferences(&self) -> &BlockPickerPreferences {
        &self.block_picker_preferences
    }

    fn update_block_picker_preferences(
        &mut self,
        update: impl FnOnce(&mut BlockPickerPreferences),
    ) -> io::Result<()> {
        update(&mut self.block_picker_preferences);
        self.persistence
            .save_block_picker_preferences(&self.block_picker_preferences)
    }

    fn refresh_authoring(&mut self, root: &Path) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            let overrides = workspace.manager.source_overrides();
            workspace.authoring = AuthoringIndex::load(root, &workspace.files, &overrides);
        }
    }

    fn refresh_files(&mut self, root: &Path) -> io::Result<Vec<WorkspaceFile>> {
        let files = WorkspaceSession::open(root)?.files().to_vec();
        let workspace = self.ensure_workspace(root)?;
        workspace.files.clone_from(&files);
        let overrides = workspace.manager.source_overrides();
        workspace.authoring = AuthoringIndex::load(root, &workspace.files, &overrides);
        Ok(files)
    }

    fn asset_manifest_is_clean(&mut self, root: &Path) -> bool {
        let Ok(workspace) = self.ensure_workspace(root) else {
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

    fn adopt_manifest_update(
        &mut self,
        root: &Path,
        relative: &Path,
        source: String,
    ) -> io::Result<Option<WeakEntity<EditorState>>> {
        let workspace = self.ensure_workspace(root)?;
        if let Some(document) = workspace.manager.document(relative) {
            document.borrow_mut().adopt_saved_contents(source)?;
        }
        Ok(workspace.editors.get(relative).cloned())
    }

    fn authoring(&self, root: &Path) -> AuthoringIndex {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .map(|workspace| workspace.authoring.clone())
            .unwrap_or_default()
    }

    fn explicit_source_ids(&self, root: &Path) -> HashSet<String> {
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

    fn source(&self, root: &Path, relative: &Path) -> Option<String> {
        let key = ProjectKey::from_path(root).ok()?;
        let workspace = self.workspaces.get(key.path())?;
        workspace
            .manager
            .source_overrides()
            .remove(relative)
            .or_else(|| fs::read_to_string(root.join(relative)).ok())
    }

    fn selection(&self, root: &Path) -> Option<&(PathBuf, usize, usize)> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces.get(key.path())?.selection.as_ref()
    }

    fn set_diagnostics(&mut self, root: &Path, diagnostics: Vec<keine_authoring::Diagnostic>) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            workspace.diagnostics = diagnostics;
        }
    }

    fn set_dock(&mut self, root: &Path, dock: WeakEntity<DockArea>) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            workspace.dock = Some(dock);
        }
    }

    fn set_document_node(&mut self, root: &Path, node: NodeId) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            workspace.document_node = Some(node);
        }
    }

    fn clear_document_node(&mut self, root: &Path) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            workspace.document_node = None;
        }
    }

    fn register_panel(&mut self, root: &Path, relative: PathBuf, panel: PanelId) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            workspace.panels.insert(relative, panel);
        }
    }

    fn register_editor(&mut self, root: &Path, relative: PathBuf, editor: WeakEntity<EditorState>) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            workspace.editors.insert(relative, editor);
        }
    }

    fn editor_for(&self, root: &Path, relative: &Path) -> Option<WeakEntity<EditorState>> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces
            .get(key.path())?
            .editors
            .get(relative)
            .cloned()
    }

    fn unregister_panel(&mut self, root: &Path, relative: &Path, panel: PanelId) {
        if let Ok(workspace) = self.ensure_workspace(root)
            && workspace.panels.get(relative) == Some(&panel)
        {
            workspace.panels.remove(relative);
            workspace.editors.remove(relative);
        }
    }

    fn document_dock(&self, root: &Path) -> Option<(WeakEntity<DockArea>, Option<NodeId>)> {
        let key = ProjectKey::from_path(root).ok()?;
        let workspace = self.workspaces.get(key.path())?;
        Some((workspace.dock.clone()?, workspace.document_node))
    }

    fn panel_for(&self, root: &Path, relative: &Path) -> Option<PanelId> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces
            .get(key.path())?
            .panels
            .get(relative)
            .copied()
    }

    fn open_document_count(&self, root: &Path) -> usize {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .map(|workspace| workspace.panels.len())
            .unwrap_or_default()
    }

    fn diagnostics_for<'a>(
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

    fn runtime_diagnostics(&self, root: &Path) -> Vec<keine_authoring::Diagnostic> {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .map(|workspace| workspace.diagnostics.clone())
            .unwrap_or_default()
    }

    fn preview(&mut self, root: &Path) -> io::Result<Arc<PreviewController>> {
        Ok(self.ensure_workspace(root)?.preview.clone())
    }

    fn preview_documents(&self, root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
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

    fn preview_panel(&self, root: &Path) -> Option<PanelId> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces.get(key.path())?.preview_panel
    }

    fn set_preview_panel(&mut self, root: &Path, panel: Option<PanelId>) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            workspace.preview_panel = panel;
        }
    }

    fn tool_panel(&self, root: &Path, name: &'static str) -> Option<PanelId> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces.get(key.path())?.tools.get(name).copied()
    }

    fn set_tool_panel(&mut self, root: &Path, name: &'static str, panel: Option<PanelId>) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            if let Some(panel) = panel {
                workspace.tools.insert(name, panel);
            } else {
                workspace.tools.remove(name);
            }
        }
    }
}

struct EditorApp {
    this: WeakEntity<EditorApp>,
    windows: WindowRegistry<WindowHandle<WorkbenchWindow>>,
    empty_window: Option<WindowHandle<WorkbenchWindow>>,
    persistence: AppPersistence,
    _instance: PrimaryInstance,
}

impl EditorApp {
    fn new(
        this: WeakEntity<EditorApp>,
        persistence: AppPersistence,
        instance: PrimaryInstance,
    ) -> Self {
        Self {
            this,
            windows: WindowRegistry::default(),
            empty_window: None,
            persistence,
            _instance: instance,
        }
    }

    fn open_empty(&mut self, cx: &mut App) {
        if let Some(window) = self.empty_window
            && window
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let editor = self.this.clone();
        let persistence = self.persistence.clone();
        let recents = persistence.recent_projects();
        let handle = cx
            .open_window(window_options(0, cx), move |window, cx| {
                window.set_window_title("Kēne Editor");
                cx.new(|cx| WorkbenchWindow::empty(editor, persistence, recents, window, cx))
            })
            .expect("failed to open Kēne Editor workbench");
        self.empty_window = Some(handle);
    }

    fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut App) {
        if paths.is_empty() {
            if let Some(window) = self.empty_window {
                let _ = window.update(cx, |_, window, _| window.activate_window());
            } else if let Some(window) = self.windows.windows.values().next().copied() {
                let _ = window.update(cx, |_, window, _| window.activate_window());
            } else {
                self.open_empty(cx);
            }
            return;
        }
        for path in paths {
            if let Err(error) = self.open_project(&path, cx) {
                eprintln!("Kēne Editor could not open {}: {error}", path.display());
            }
        }
    }

    fn open_project(&mut self, path: &Path, cx: &mut App) -> io::Result<()> {
        self.windows
            .windows
            .retain(|_, handle| handle.update(cx, |_, _, _| ()).is_ok());
        let session = WorkspaceSession::open(path)?;
        let project = session.key().clone();
        if let Some(existing) = self.windows.existing(&project) {
            let _ = existing.update(cx, |_, window, _| window.activate_window());
            return Ok(());
        }

        self.persistence.prepare_workspace(&project)?;
        let _ = self.persistence.record_recent(&project)?;
        let persistence = self.persistence.clone();
        let editor = self.this.clone();
        let handle = if let Some(empty) = self.empty_window.take() {
            let session_for_window = session.clone();
            empty
                .update(cx, |workbench, window, cx| {
                    workbench.open_session(session_for_window, window, cx);
                    window.activate_window();
                })
                .map_err(io::Error::other)?;
            empty
        } else {
            let index = self.windows.windows.len();
            cx.open_window(window_options(index, cx), move |window, cx| {
                cx.new(|cx| WorkbenchWindow::project(editor, persistence, session, window, cx))
            })
            .map_err(io::Error::other)?
        };
        self.windows.insert(project, handle);
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PanelPayload {
    Explorer { root: PathBuf },
    Document { root: PathBuf, relative: PathBuf },
    Inspector { root: PathBuf },
    Output { root: PathBuf },
    Preview { root: PathBuf },
    Assets { root: PathBuf },
    Characters { root: PathBuf },
    Scenes { root: PathBuf },
    Problems { root: PathBuf },
    Performance { root: PathBuf },
}

#[derive(Clone, Copy)]
enum ToolKind {
    Assets,
    Characters,
    Scenes,
    Problems,
    Performance,
}

impl ToolKind {
    fn panel_name(self) -> &'static str {
        match self {
            Self::Assets => ASSETS_PANEL,
            Self::Characters => CHARACTERS_PANEL,
            Self::Scenes => SCENES_PANEL,
            Self::Problems => PROBLEMS_PANEL,
            Self::Performance => PERFORMANCE_PANEL,
        }
    }

    fn payload(self, root: PathBuf) -> PanelPayload {
        match self {
            Self::Assets => PanelPayload::Assets { root },
            Self::Characters => PanelPayload::Characters { root },
            Self::Scenes => PanelPayload::Scenes { root },
            Self::Problems => PanelPayload::Problems { root },
            Self::Performance => PanelPayload::Performance { root },
        }
    }
}

#[derive(Clone)]
enum PanelContent {
    Explorer {
        root: PathBuf,
        files: Vec<WorkspaceFile>,
    },
    Document {
        root: PathBuf,
        relative: PathBuf,
        document: Option<DocumentHandle>,
        editor: Entity<EditorState>,
    },
    Inspector {
        root: PathBuf,
        file_count: usize,
    },
    Output {
        root: PathBuf,
        file_count: usize,
    },
    Preview {
        root: PathBuf,
        controller: Arc<PreviewController>,
    },
    Assets {
        root: PathBuf,
    },
    Characters {
        root: PathBuf,
    },
    Scenes {
        root: PathBuf,
    },
    Problems {
        root: PathBuf,
    },
    Performance {
        root: PathBuf,
        controller: Arc<PreviewController>,
    },
}

impl PanelContent {
    fn from_payload(payload: PanelPayload, window: &mut Window, cx: &mut App) -> io::Result<Self> {
        match payload {
            PanelPayload::Explorer { root } => WorkspaceSession::open(&root)
                .map(|session| Self::Explorer {
                    root: root.clone(),
                    files: session.files().to_vec(),
                })
                .or_else(|_| {
                    Ok(Self::Explorer {
                        root,
                        files: Vec::new(),
                    })
                }),
            PanelPayload::Document { root, relative } => {
                let document = if is_eiyashou_authoring_document(&root, &relative) {
                    Some(cx.global_mut::<EditorDocuments>().open(&root, &relative)?)
                } else {
                    None
                };
                let contents = match &document {
                    Some(document) => document.borrow().contents().to_owned(),
                    None => std::fs::read_to_string(root.join(&relative))?,
                };
                let language = language_for_path(&relative);
                let editor = cx.new(|cx| {
                    let mut editor = EditorState::new(window, cx)
                        .default_value(contents)
                        .language(language)
                        .folding(false);
                    if language == "eiyashou" {
                        editor.set_highlighter_factory(eiyashou_highlighter_factory(), cx);
                    }
                    editor
                });
                Ok(Self::Document {
                    root,
                    relative,
                    document,
                    editor,
                })
            }
            PanelPayload::Inspector { root } => WorkspaceSession::open(&root)
                .map(|session| Self::Inspector {
                    root: root.clone(),
                    file_count: session.files().len(),
                })
                .or_else(|_| {
                    Ok(Self::Inspector {
                        root,
                        file_count: 0,
                    })
                }),
            PanelPayload::Output { root } => WorkspaceSession::open(&root)
                .map(|session| Self::Output {
                    root: root.clone(),
                    file_count: session.files().len(),
                })
                .or_else(|_| {
                    Ok(Self::Output {
                        root,
                        file_count: 0,
                    })
                }),
            PanelPayload::Preview { root } => {
                let controller = cx.global_mut::<EditorDocuments>().preview(&root)?;
                controller.set_panel_visible(true);
                Ok(Self::Preview { root, controller })
            }
            PanelPayload::Assets { root } => Ok(Self::Assets { root }),
            PanelPayload::Characters { root } => Ok(Self::Characters { root }),
            PanelPayload::Scenes { root } => Ok(Self::Scenes { root }),
            PanelPayload::Problems { root } => Ok(Self::Problems { root }),
            PanelPayload::Performance { root } => {
                let controller = cx.global_mut::<EditorDocuments>().preview(&root)?;
                Ok(Self::Performance { root, controller })
            }
        }
    }

    fn payload(&self) -> PanelPayload {
        match self {
            Self::Explorer { root, .. } => PanelPayload::Explorer { root: root.clone() },
            Self::Document { root, relative, .. } => PanelPayload::Document {
                root: root.clone(),
                relative: relative.clone(),
            },
            Self::Inspector { root, .. } => PanelPayload::Inspector { root: root.clone() },
            Self::Output { root, .. } => PanelPayload::Output { root: root.clone() },
            Self::Preview { root, .. } => PanelPayload::Preview { root: root.clone() },
            Self::Assets { root } => PanelPayload::Assets { root: root.clone() },
            Self::Characters { root } => PanelPayload::Characters { root: root.clone() },
            Self::Scenes { root } => PanelPayload::Scenes { root: root.clone() },
            Self::Problems { root } => PanelPayload::Problems { root: root.clone() },
            Self::Performance { root, .. } => PanelPayload::Performance { root: root.clone() },
        }
    }

    fn panel_name(&self) -> &'static str {
        match self {
            Self::Explorer { .. } => EXPLORER_PANEL,
            Self::Document { .. } => DOCUMENT_PANEL,
            Self::Inspector { .. } => INSPECTOR_PANEL,
            Self::Output { .. } => OUTPUT_PANEL,
            Self::Preview { .. } => PREVIEW_PANEL,
            Self::Assets { .. } => ASSETS_PANEL,
            Self::Characters { .. } => CHARACTERS_PANEL,
            Self::Scenes { .. } => SCENES_PANEL,
            Self::Problems { .. } => PROBLEMS_PANEL,
            Self::Performance { .. } => PERFORMANCE_PANEL,
        }
    }

    fn title(&self) -> SharedString {
        match self {
            Self::Explorer { .. } => "Explorer".into(),
            Self::Document { relative, .. } => relative
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Document")
                .to_owned()
                .into(),
            Self::Inspector { .. } => "Inspector".into(),
            Self::Output { .. } => "Output".into(),
            Self::Preview { .. } => "Preview".into(),
            Self::Assets { .. } => "Assets".into(),
            Self::Characters { .. } => "Characters".into(),
            Self::Scenes { .. } => "Scenes".into(),
            Self::Problems { .. } => "Problems".into(),
            Self::Performance { .. } => "Performance".into(),
        }
    }
}

struct WorkbenchPanel {
    content: PanelContent,
    focus: FocusHandle,
    document_mode: DocumentMode,
    block_text_editors: Vec<BlockTextEditor>,
    collapsed_scenes: HashSet<String>,
    selected_blocks: HashSet<usize>,
    block_selection_anchor: Option<usize>,
    draft_text: Option<DraftTextBlock>,
    block_drop_target: Option<usize>,
    block_picker_open: bool,
    block_picker_index: usize,
    block_picker_category: Option<&'static str>,
    block_picker_customize: bool,
    block_picker_input: Entity<InputState>,
    view_scroll: ScrollHandle,
    block_scroll_anchor: ScrollAnchor,
    block_scroll_pending: bool,
    tool_inputs: Vec<Entity<InputState>>,
    timeline: VecDeque<TimelineSample>,
    recovery_epoch: u64,
    preview_image: Option<Arc<RenderImage>>,
    preview_frame_id: u64,
    preview_lifecycle: PreviewLifecycle,
    preview_bounds: Arc<Mutex<Option<Bounds<Pixels>>>>,
    _subscriptions: Vec<Subscription>,
    visual_subscriptions: Vec<Subscription>,
    inspector_key: Option<InspectorEditKey>,
    inspector_inputs: Vec<Entity<InputState>>,
    inspector_subscriptions: Vec<Subscription>,
    file_selection: Option<PathBuf>,
    file_collapsed: HashSet<PathBuf>,
    file_clipboard: Option<PathBuf>,
    file_edit: Option<FileEditMode>,
    file_name_input: Entity<InputState>,
    file_commit_requested: bool,
    file_progress: Option<FileProgress>,
    file_context_menu: Option<FileContextMenu>,
    file_context_epoch: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DocumentMode {
    #[default]
    Text,
    Block,
}

struct BlockTextEditor {
    text_start: usize,
    state: Entity<InputState>,
}

#[derive(Clone, Debug)]
enum FileEditMode {
    NewFile { parent: PathBuf },
    NewFolder { parent: PathBuf },
    Rename { path: PathBuf },
}

#[derive(Clone, Debug)]
struct FileProgress {
    completed: usize,
    total: usize,
}

#[derive(Clone, Debug)]
struct FileContextMenu {
    path: PathBuf,
    position: Point<Pixels>,
    epoch: u64,
    closing: bool,
}

#[derive(Clone, Debug)]
struct FileDrag {
    relative: PathBuf,
}

impl Render for FileDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(px(5.))
            .bg(rgb(SURFACE))
            .text_xs()
            .child(
                self.relative
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("File")
                    .to_owned(),
            )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InspectorEditKey {
    path: PathBuf,
    block_start: usize,
    metadata: TextBlockMetadata,
}

#[derive(Clone)]
struct BlockDrag {
    selected: HashSet<usize>,
}

struct DraftTextBlock {
    target: DraftInsertionTarget,
    text_range: Option<Range<usize>>,
    last_escaped: String,
    state: Entity<InputState>,
}

#[derive(Clone, Copy)]
enum DraftInsertionTarget {
    After(usize),
    SceneEnd(usize),
}

#[derive(Clone, Copy)]
struct TimelineSample {
    published: u64,
    overwritten: u64,
}

impl WorkbenchPanel {
    fn from_payload(
        payload: PanelPayload,
        window: &mut Window,
        cx: &mut App,
    ) -> io::Result<Entity<Self>> {
        let content = PanelContent::from_payload(payload, window, cx)?;
        let registration = match &content {
            PanelContent::Document { root, relative, .. } => Some((root.clone(), relative.clone())),
            _ => None,
        };
        let preview_registration = match &content {
            PanelContent::Preview { root, .. } => Some(root.clone()),
            _ => None,
        };
        let tool_registration = match &content {
            PanelContent::Assets { root } => Some((root.clone(), ASSETS_PANEL)),
            PanelContent::Characters { root } => Some((root.clone(), CHARACTERS_PANEL)),
            PanelContent::Scenes { root } => Some((root.clone(), SCENES_PANEL)),
            PanelContent::Problems { root } => Some((root.clone(), PROBLEMS_PANEL)),
            PanelContent::Performance { root, .. } => Some((root.clone(), PERFORMANCE_PANEL)),
            _ => None,
        };
        let panel = cx.new(|cx| {
            let block_picker_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Search blocks"));
            let file_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
            let view_scroll = ScrollHandle::new();
            let block_scroll_anchor = ScrollAnchor::for_handle(view_scroll.clone());
            let mut panel = Self {
                content,
                focus: cx.focus_handle(),
                document_mode: DocumentMode::Text,
                block_text_editors: Vec::new(),
                collapsed_scenes: HashSet::new(),
                selected_blocks: HashSet::new(),
                block_selection_anchor: None,
                draft_text: None,
                block_drop_target: None,
                block_picker_open: false,
                block_picker_index: 0,
                block_picker_category: None,
                block_picker_customize: false,
                block_picker_input: block_picker_input.clone(),
                view_scroll,
                block_scroll_anchor,
                block_scroll_pending: false,
                tool_inputs: Vec::new(),
                timeline: VecDeque::with_capacity(60),
                recovery_epoch: 0,
                preview_image: None,
                preview_frame_id: 0,
                preview_lifecycle: PreviewLifecycle::Off,
                preview_bounds: Arc::new(Mutex::new(None)),
                _subscriptions: Vec::new(),
                visual_subscriptions: Vec::new(),
                inspector_key: None,
                inspector_inputs: Vec::new(),
                inspector_subscriptions: Vec::new(),
                file_selection: None,
                file_collapsed: HashSet::new(),
                file_clipboard: None,
                file_edit: None,
                file_name_input: file_name_input.clone(),
                file_commit_requested: false,
                file_progress: None,
                file_context_menu: None,
                file_context_epoch: 0,
            };
            panel._subscriptions.push(cx.subscribe(
                &block_picker_input,
                |panel: &mut WorkbenchPanel, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        panel.block_picker_index = 0;
                        cx.notify();
                    }
                },
            ));
            panel._subscriptions.push(cx.subscribe(
                &file_name_input,
                move |panel: &mut WorkbenchPanel, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        panel.file_commit_requested = true;
                        cx.notify();
                    }
                },
            ));
            if let PanelContent::Document {
                root,
                relative,
                document,
                editor,
            } = &panel.content
            {
                let root = root.clone();
                let relative = relative.clone();
                let editor_for_selection = editor.clone();
                let root_for_selection = root.clone();
                let relative_for_selection = relative.clone();
                panel
                    ._subscriptions
                    .push(cx.observe(editor, move |_, _, cx| {
                        let position = editor_for_selection.read(cx).cursor_position();
                        cx.global_mut::<EditorDocuments>().set_selection(
                            &root_for_selection,
                            relative_for_selection.clone(),
                            position.line as usize,
                            position.character as usize,
                        );
                        cx.global_mut::<EditorDocuments>()
                            .clear_block_selection(&root_for_selection);
                        if let Ok(preview) = cx
                            .global_mut::<EditorDocuments>()
                            .preview(&root_for_selection)
                        {
                            preview.set_cursor(
                                relative_for_selection.clone(),
                                position.line as usize + 1,
                                position.character as usize + 1,
                            );
                        }
                        cx.refresh_windows();
                    }));
                if let Some(document) = document {
                    let document_for_change = document.clone();
                    let change_subscription = cx.subscribe(
                        editor,
                        move |panel: &mut WorkbenchPanel, editor, event: &InputEvent, cx| {
                            if !matches!(event, InputEvent::Change) {
                                return;
                            }
                            let editor = editor.read(cx);
                            let contents = editor.value().to_string();
                            let position = editor.cursor_position();
                            let changed =
                                document_for_change.borrow_mut().replace_contents(contents);
                            document_for_change
                                .borrow_mut()
                                .set_selection(position.line as usize, position.character as usize);
                            cx.global_mut::<EditorDocuments>().set_selection(
                                &root,
                                relative.clone(),
                                position.line as usize,
                                position.character as usize,
                            );
                            if changed {
                                cx.global_mut::<EditorDocuments>().refresh_authoring(&root);
                                panel.recovery_epoch = panel.recovery_epoch.wrapping_add(1);
                                let epoch = panel.recovery_epoch;
                                let document = document_for_change.clone();
                                let root = root.clone();
                                let relative = relative.clone();
                                cx.global_mut::<EditorDocuments>()
                                    .set_notice(&root, "Unsaved changes");
                                cx.spawn(async move |panel, cx| {
                                    cx.background_executor()
                                        .timer(Duration::from_millis(350))
                                        .await;
                                    let _ = panel.update(cx, |panel, cx| {
                                        if panel.recovery_epoch == epoch {
                                            let notice =
                                                match document.borrow_mut().persist_recovery() {
                                                    Ok(()) => "Recovery draft saved".to_owned(),
                                                    Err(error) => {
                                                        format!("Recovery draft failed: {error}")
                                                    }
                                                };
                                            cx.global_mut::<EditorDocuments>()
                                                .set_notice(&root, notice);
                                            if relative
                                                .extension()
                                                .and_then(|extension| extension.to_str())
                                                == Some("shou")
                                                && let Ok(preview) = cx
                                                    .global_mut::<EditorDocuments>()
                                                    .preview(&root)
                                            {
                                                preview.apply_snapshot(
                                                    relative.clone(),
                                                    document
                                                        .borrow()
                                                        .contents()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                            }
                                            cx.refresh_windows();
                                        }
                                    });
                                })
                                .detach();
                            }
                            cx.notify();
                            cx.refresh_windows();
                        },
                    );
                    panel._subscriptions.push(change_subscription);
                }
            }
            match &panel.content {
                PanelContent::Characters { .. } => {
                    for placeholder in ["Character id", "Display name", "Color (optional)"] {
                        panel.tool_inputs.push(
                            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder)),
                        );
                    }
                }
                PanelContent::Scenes { .. } => {
                    panel
                        .tool_inputs
                        .push(cx.new(|cx| InputState::new(window, cx).placeholder("Scene id")));
                }
                _ => {}
            }
            panel.rebuild_visual_editors(window, cx);
            if let PanelContent::Preview { root, controller } = &panel.content {
                let root = root.clone();
                let controller = controller.clone();
                cx.spawn(async move |panel, cx| {
                    loop {
                        let snapshot = controller.snapshot();
                        let interval = if matches!(
                            snapshot.lifecycle,
                            PreviewLifecycle::Starting | PreviewLifecycle::Running
                        ) && snapshot
                            .last_frame_at
                            .is_some_and(|instant| instant.elapsed() < Duration::from_millis(250))
                        {
                            PREVIEW_POLL_INTERVAL
                        } else {
                            Duration::from_millis(250)
                        };
                        cx.background_executor().timer(interval).await;
                        if panel
                            .update(cx, |panel, cx| {
                                if panel.refresh_preview(&root, &controller, cx) {
                                    cx.notify();
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
            if let PanelContent::Performance { controller, .. } = &panel.content {
                let controller = controller.clone();
                cx.spawn(async move |panel, cx| {
                    loop {
                        let interval = match controller.snapshot().lifecycle {
                            PreviewLifecycle::Running | PreviewLifecycle::Starting => {
                                Duration::from_millis(500)
                            }
                            _ => Duration::from_secs(2),
                        };
                        cx.background_executor().timer(interval).await;
                        if panel
                            .update(cx, |panel, cx| {
                                let stats = controller.snapshot().frame_stats;
                                let sample = TimelineSample {
                                    published: stats.published,
                                    overwritten: stats.overwritten,
                                };
                                if panel.timeline.back().is_none_or(|last| {
                                    last.published != sample.published
                                        || last.overwritten != sample.overwritten
                                }) {
                                    if panel.timeline.len() == 60 {
                                        panel.timeline.pop_front();
                                    }
                                    panel.timeline.push_back(sample);
                                    cx.notify();
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
            panel
        });
        if let Some((root, relative)) = registration {
            let editor = match &panel.read(cx).content {
                PanelContent::Document { editor, .. } => Some(editor.downgrade()),
                _ => None,
            };
            let documents = cx.global_mut::<EditorDocuments>();
            documents.register_panel(&root, relative.clone(), PanelId::from(panel.entity_id()));
            if let Some(editor) = editor {
                documents.register_editor(&root, relative, editor);
            }
        }
        if let Some(root) = preview_registration {
            cx.global_mut::<EditorDocuments>()
                .set_preview_panel(&root, Some(PanelId::from(panel.entity_id())));
        }
        if let Some((root, name)) = tool_registration {
            cx.global_mut::<EditorDocuments>().set_tool_panel(
                &root,
                name,
                Some(PanelId::from(panel.entity_id())),
            );
        }
        Ok(panel)
    }

    fn refresh_preview(
        &mut self,
        root: &Path,
        controller: &PreviewController,
        cx: &mut Context<Self>,
    ) -> bool {
        let snapshot = controller.take_snapshot();
        let lifecycle_changed = self.preview_lifecycle != snapshot.lifecycle;
        cx.global_mut::<EditorDocuments>()
            .set_diagnostics(root, snapshot.diagnostics.clone());
        self.preview_lifecycle = snapshot.lifecycle;
        let Some(frame) = snapshot.frame else {
            return lifecycle_changed;
        };
        if frame.metadata.frame_id == self.preview_frame_id {
            return lifecycle_changed;
        }
        let frame = Arc::try_unwrap(frame).unwrap_or_else(|frame| (*frame).clone());
        let metadata = frame.metadata;
        let Some(bytes) = take_tightly_packed_bgra(frame) else {
            return lifecycle_changed;
        };
        let Some(buffer) = image::RgbaImage::from_raw(metadata.width, metadata.height, bytes)
        else {
            return lifecycle_changed;
        };
        self.preview_frame_id = metadata.frame_id;
        self.preview_image = Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])));
        true
    }

    fn refresh_inspector_editors(
        &mut self,
        root: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = cx
            .global::<EditorDocuments>()
            .block_selection(root)
            .filter(|(_, starts)| starts.len() == 1)
            .map(|(path, starts)| (path.clone(), starts[0]))
            .or_else(|| {
                let (path, line, column) = cx.global::<EditorDocuments>().selection(root)?.clone();
                let source = cx.global::<EditorDocuments>().source(root, &path)?;
                projected_block_at(&source, line, column)
                    .map(|(_, block)| (path, block.source_range.start))
            });
        let next = selected.and_then(|(path, block_start)| {
            if path.extension().and_then(|extension| extension.to_str()) != Some("shou") {
                return None;
            }
            let source = cx.global::<EditorDocuments>().source(root, &path)?;
            let metadata =
                EiyashouProjection::parse(&source).text_block_metadata(&source, block_start)?;
            Some(InspectorEditKey {
                path,
                block_start,
                metadata,
            })
        });
        if self.inspector_key == next {
            return;
        }
        self.inspector_key = next.clone();
        self.inspector_inputs.clear();
        self.inspector_subscriptions.clear();
        let Some(key) = next else {
            return;
        };
        let values = [
            key.metadata
                .speaker
                .clone()
                .unwrap_or_else(|| "Narrator".to_owned()),
            key.metadata.voice.clone().unwrap_or_default(),
            key.metadata.stable_id.clone().unwrap_or_default(),
        ];
        let placeholders = ["Narrator / character id", "Voice id", "Stable ID"];
        self.inspector_inputs = values
            .into_iter()
            .zip(placeholders)
            .map(|(value, placeholder)| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(value)
                        .placeholder(placeholder)
                })
            })
            .collect();
        let inputs = self.inspector_inputs.clone();
        let window_handle = window.window_handle();
        for input in &inputs {
            let inputs = inputs.clone();
            let root = root.to_owned();
            let key = key.clone();
            self.inspector_subscriptions.push(cx.subscribe(
                input,
                move |_, _, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. }) {
                        return;
                    }
                    let values = inputs
                        .iter()
                        .map(|input| input.read(cx).value().to_string())
                        .collect::<Vec<_>>();
                    let speaker = values[0].trim();
                    let voice = values[1].trim();
                    let stable_id = values[2].trim();
                    let metadata = TextBlockMetadata {
                        speaker: (!speaker.is_empty() && !speaker.eq_ignore_ascii_case("Narrator"))
                            .then(|| speaker.to_owned()),
                        voice: (!voice.is_empty()).then(|| voice.to_owned()),
                        stable_id: (!stable_id.is_empty()).then(|| stable_id.to_owned()),
                    };
                    if metadata == key.metadata {
                        return;
                    }
                    let Some(source) = cx.global::<EditorDocuments>().source(&root, &key.path)
                    else {
                        return;
                    };
                    if metadata.stable_id != key.metadata.stable_id
                        && metadata.stable_id.as_ref().is_some_and(|id| {
                            cx.global::<EditorDocuments>()
                                .explicit_source_ids(&root)
                                .contains(id)
                        })
                    {
                        cx.global_mut::<EditorDocuments>()
                            .set_notice(&root, "Stable ID already exists");
                        cx.refresh_windows();
                        return;
                    }
                    match EiyashouProjection::parse(&source).replace_text_block_metadata(
                        &source,
                        key.block_start,
                        &metadata,
                    ) {
                        Ok(edited) => {
                            let result = cx.update_window(window_handle, |_, window, cx| {
                                apply_workspace_edit(&root, &key.path, edited, window, cx);
                            });
                            if result.is_ok() {
                                cx.global_mut::<EditorDocuments>().set_block_selection(
                                    &root,
                                    key.path.clone(),
                                    vec![key.block_start],
                                );
                            }
                            cx.global_mut::<EditorDocuments>().set_notice(
                                &root,
                                if result.is_ok() {
                                    "Text properties updated".to_owned()
                                } else {
                                    "Text properties update failed".to_owned()
                                },
                            );
                        }
                        Err(error) => cx
                            .global_mut::<EditorDocuments>()
                            .set_notice(&root, format!("Text properties blocked: {error}")),
                    }
                    cx.refresh_windows();
                },
            ));
        }
    }

    fn rebuild_visual_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.block_text_editors.clear();
        self.visual_subscriptions.clear();
        self.draft_text = None;
        let PanelContent::Document {
            root,
            relative,
            document: Some(document),
            editor,
        } = &self.content
        else {
            return;
        };
        if relative.extension().and_then(|value| value.to_str()) != Some("shou") {
            return;
        }

        let window_handle = window.window_handle();
        let dialogues = dialogues_for_source(relative, document.borrow().contents());
        for dialogue in dialogues.into_iter().filter(|dialogue| dialogue.editable) {
            let line = dialogue.line;
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(dialogue.text)
                    .placeholder("Text")
            });
            let document = document.clone();
            let source_editor = editor.clone();
            let root = root.clone();
            let relative = relative.clone();
            let state_for_change = state.clone();
            let subscription = cx.subscribe(&state, move |_, _, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let value = state_for_change.read(cx).value().to_string();
                let source = document.borrow().contents().to_owned();
                let current = dialogues_for_source(&relative, &source)
                    .into_iter()
                    .find(|dialogue| dialogue.line == line);
                let result = current
                    .as_ref()
                    .ok_or_else(|| "dialogue no longer exists".to_owned())
                    .and_then(|dialogue| {
                        replace_dialogue_text(&source, dialogue, &value)
                            .map_err(|error| error.to_string())
                    });
                match result {
                    Ok(edited) => {
                        let result = cx.update_window(window_handle, |_, window, cx| {
                            source_editor.update(cx, |editor, cx| {
                                editor.replace_all(edited, window, cx);
                            });
                        });
                        let notice = match result {
                            Ok(()) => "Text updated from Blocks".to_owned(),
                            Err(error) => format!("Block text edit failed: {error}"),
                        };
                        cx.global_mut::<EditorDocuments>().set_notice(&root, notice);
                    }
                    Err(error) => cx
                        .global_mut::<EditorDocuments>()
                        .set_notice(&root, format!("Block text edit blocked: {error}")),
                }
                cx.refresh_windows();
            });
            self.visual_subscriptions.push(subscription);
            self.block_text_editors.push(BlockTextEditor {
                text_start: dialogue.text_range.start,
                state,
            });
        }
    }

    fn toggle_block_picker(
        &mut self,
        _: &ToggleBlockPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        self.block_picker_open = !self.block_picker_open;
        self.block_picker_index = 0;
        if self.block_picker_open {
            self.block_picker_input
                .update(cx, |state, cx| state.focus(window, cx));
        } else {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    fn block_picker_next(&mut self, _: &BlockPickerNext, _: &mut Window, cx: &mut Context<Self>) {
        let count = self.filtered_picker_kinds(cx).len();
        if count > 0 {
            self.block_picker_index = (self.block_picker_index + 1).min(count - 1);
            cx.notify();
        }
    }

    fn block_picker_previous(
        &mut self,
        _: &BlockPickerPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.block_picker_index = self.block_picker_index.saturating_sub(1);
        cx.notify();
    }

    fn accept_block_picker(
        &mut self,
        _: &AcceptBlockPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kinds = self.filtered_picker_kinds(cx);
        let Some(kind) = kinds.get(self.block_picker_index).copied() else {
            return;
        };
        self.block_picker_open = false;
        self.insert_from_palette(kind, window, cx);
        self.rebuild_visual_editors(window, cx);
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn close_block_picker(
        &mut self,
        _: &CloseBlockPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.block_picker_open = false;
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn filtered_picker_kinds(&self, cx: &App) -> Vec<InsertKind> {
        let query = self
            .block_picker_input
            .read(cx)
            .value()
            .to_string()
            .to_lowercase();
        picker_kinds(
            cx.global::<EditorDocuments>().block_picker_preferences(),
            &query,
            self.block_picker_category,
            self.block_picker_customize,
        )
    }

    fn change_picker_preferences(
        &self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut BlockPickerPreferences),
    ) {
        let result = cx
            .global_mut::<EditorDocuments>()
            .update_block_picker_preferences(update);
        if let Err(error) = result
            && let PanelContent::Document { root, .. } = &self.content
        {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, format!("Picker preferences not saved: {error}"));
        }
        cx.refresh_windows();
    }

    fn toggle_picker_favorite(&self, kind: InsertKind, cx: &mut Context<Self>) {
        self.change_picker_preferences(cx, move |preferences| {
            toggle_preference(&mut preferences.favorites, kind.label());
        });
    }

    fn toggle_picker_hidden(&self, kind: InsertKind, cx: &mut Context<Self>) {
        self.change_picker_preferences(cx, move |preferences| {
            toggle_preference(&mut preferences.hidden, kind.label());
        });
    }

    fn move_picker_item(&self, kind: InsertKind, delta: isize, cx: &mut Context<Self>) {
        self.change_picker_preferences(cx, move |preferences| {
            let universe = InsertKind::ALL
                .into_iter()
                .map(InsertKind::label)
                .collect::<Vec<_>>();
            let group = InsertKind::ALL
                .into_iter()
                .filter(|candidate| candidate.category() == kind.category())
                .map(InsertKind::label)
                .collect::<Vec<_>>();
            move_group_preference(
                &mut preferences.item_order,
                kind.label(),
                &group,
                &universe,
                delta,
            );
        });
    }

    fn move_picker_category(&self, category: &'static str, delta: isize, cx: &mut Context<Self>) {
        self.change_picker_preferences(cx, move |preferences| {
            move_preference(
                &mut preferences.category_order,
                category,
                &PICKER_CATEGORIES,
                delta,
            );
        });
    }

    fn begin_text_block(
        &mut self,
        _: &BeginTextBlock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        if let Some(draft) = &self.draft_text {
            draft.state.update(cx, |state, cx| state.focus(window, cx));
            return;
        }
        let PanelContent::Document {
            root,
            relative,
            document: Some(document),
            editor,
        } = &self.content
        else {
            return;
        };
        let source = document.borrow().contents().to_owned();
        let projection = EiyashouProjection::parse(&source);
        let selected_line = cx
            .global::<EditorDocuments>()
            .selection(root)
            .filter(|(path, _, _)| path == relative)
            .map_or(0, |(_, line, _)| *line);
        let target = self
            .selected_blocks
            .iter()
            .copied()
            .max()
            .map(DraftInsertionTarget::After)
            .or_else(|| {
                projection
                    .scenes
                    .iter()
                    .filter(|scene| {
                        source
                            .get(..scene.name_range.start)
                            .map(|prefix| prefix.bytes().filter(|byte| *byte == b'\n').count())
                            .is_some_and(|line| line <= selected_line)
                    })
                    .max_by_key(|scene| scene.source_range.start)
                    .map(|scene| {
                        scene.blocks.last().map_or(
                            DraftInsertionTarget::SceneEnd(scene.source_range.start),
                            |block| DraftInsertionTarget::After(block.source_range.start),
                        )
                    })
            })
            .or_else(|| {
                projection.scenes.first().map(|scene| {
                    scene.blocks.last().map_or(
                        DraftInsertionTarget::SceneEnd(scene.source_range.start),
                        |block| DraftInsertionTarget::After(block.source_range.start),
                    )
                })
            });
        let Some(target) = target else {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "No Scene available");
            cx.refresh_windows();
            return;
        };
        if let Some(scene) = projection.scenes.iter().find(|scene| match target {
            DraftInsertionTarget::After(start) => scene
                .blocks
                .iter()
                .any(|block| block.source_range.start == start),
            DraftInsertionTarget::SceneEnd(start) => scene.source_range.start == start,
        }) {
            self.collapsed_scenes.remove(&scene.name);
        }
        let state = cx.new(|cx| InputState::new(window, cx).placeholder("Text"));
        let state_for_change = state.clone();
        let document = document.clone();
        let source_editor = editor.clone();
        let root = root.clone();
        let window_handle = window.window_handle();
        let subscription = cx.subscribe(&state, move |panel, _, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let value = state_for_change.read(cx).value().to_string();
            let escaped = escape_eiyashou_string(&value);
            let source = document.borrow().contents().to_owned();
            let Some(draft) = panel
                .draft_text
                .as_mut()
                .filter(|draft| draft.state.entity_id() == state_for_change.entity_id())
            else {
                return;
            };
            let edit = if let Some(range) = draft.text_range.clone() {
                if source.get(range.clone()) != Some(draft.last_escaped.as_str()) {
                    Err("source changed; refresh the Block view".to_owned())
                } else {
                    let start = range.start;
                    let mut edited = source;
                    edited.replace_range(range, &escaped);
                    draft.text_range = Some(start..start + escaped.len());
                    draft.last_escaped.clone_from(&escaped);
                    Ok(edited)
                }
            } else if value.is_empty() {
                return;
            } else {
                let statement = format!("\"{escaped}\"");
                let projection = EiyashouProjection::parse(&source);
                match draft.target {
                    DraftInsertionTarget::After(start) => {
                        projection.insert_block_after(&source, start, &statement)
                    }
                    DraftInsertionTarget::SceneEnd(start) => {
                        projection.insert_block_in_scene(&source, start, &statement)
                    }
                }
                .map(|(edited, range)| {
                    draft.text_range = Some(range.start + 1..range.end - 1);
                    draft.last_escaped.clone_from(&escaped);
                    panel.selected_blocks.clear();
                    panel.selected_blocks.insert(range.start);
                    panel.block_selection_anchor = Some(range.start);
                    edited
                })
                .map_err(|error| error.to_string())
            };
            match edit {
                Ok(edited) => {
                    let result = cx.update_window(window_handle, |_, window, cx| {
                        source_editor.update(cx, |editor, cx| {
                            editor.replace_all(edited, window, cx);
                        });
                    });
                    let notice = match result {
                        Ok(()) => "Text updated".to_owned(),
                        Err(error) => format!("Text edit failed: {error}"),
                    };
                    cx.global_mut::<EditorDocuments>().set_notice(&root, notice);
                }
                Err(error) => cx
                    .global_mut::<EditorDocuments>()
                    .set_notice(&root, format!("Text edit blocked: {error}")),
            }
            cx.notify();
            cx.refresh_windows();
        });
        self.visual_subscriptions.push(subscription);
        self.draft_text = Some(DraftTextBlock {
            target,
            text_range: None,
            last_escaped: String::new(),
            state: state.clone(),
        });
        state.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn copy_selected_blocks(&mut self, _: &CopyBlocks, _: &mut Window, cx: &mut Context<Self>) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        let PanelContent::Document {
            root,
            document: Some(document),
            ..
        } = &self.content
        else {
            return;
        };
        let source = document.borrow().contents().to_owned();
        match EiyashouProjection::parse(&source).copy_blocks(&source, &self.selected_blocks) {
            Ok(value) => {
                cx.write_to_clipboard(ClipboardItem::new_string(value));
                cx.global_mut::<EditorDocuments>()
                    .set_notice(root, "Blocks copied");
            }
            Err(error) => cx
                .global_mut::<EditorDocuments>()
                .set_notice(root, format!("Copy blocked: {error}")),
        }
        cx.refresh_windows();
    }

    fn paste_blocks(&mut self, _: &PasteBlocks, window: &mut Window, cx: &mut Context<Self>) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        let Some(fragment) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.set_block_notice("Paste blocked: clipboard has no text".into(), cx);
            return;
        };
        let fragment = fragment.trim();
        if fragment.is_empty() {
            self.set_block_notice("Paste blocked: clipboard is empty".into(), cx);
            return;
        }
        let wrapper = format!("scene __paste {{ {fragment} }}");
        let fragment_projection = EiyashouProjection::parse(&wrapper);
        let fragment_blocks = fragment_projection
            .scenes
            .first()
            .map(|scene| scene.blocks.as_slice())
            .unwrap_or_default();
        if fragment_blocks.is_empty()
            || !fragment_projection.read_only.is_empty()
            || fragment_blocks.iter().any(|block| block.read_only)
        {
            self.set_block_notice("Paste blocked: not valid block source".into(), cx);
            return;
        }
        let mut pasted_ids = HashSet::new();
        if fragment_blocks
            .iter()
            .filter_map(|block| block.stable_id.as_deref())
            .any(|id| !pasted_ids.insert(id.to_owned()))
        {
            self.set_block_notice("Paste blocked: duplicate stable ID".into(), cx);
            return;
        }
        let PanelContent::Document {
            root,
            document: Some(document),
            ..
        } = &self.content
        else {
            return;
        };
        let project_ids = cx.global::<EditorDocuments>().explicit_source_ids(root);
        if pasted_ids.iter().any(|id| project_ids.contains(id)) {
            self.set_block_notice("Paste blocked: stable ID already exists".into(), cx);
            return;
        }
        let Some(after_start) = self.selected_blocks.iter().copied().max() else {
            self.set_block_notice("Paste blocked: select an insertion block".into(), cx);
            return;
        };
        let source = document.borrow().contents().to_owned();
        match EiyashouProjection::parse(&source).insert_block_after(&source, after_start, fragment)
        {
            Ok((edited, _)) => self.apply_block_source(edited, "Blocks pasted", window, cx),
            Err(error) => self.set_block_notice(format!("Paste blocked: {error}"), cx),
        }
    }

    fn delete_selected_blocks(
        &mut self,
        _: &DeleteBlocks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        let source = match &self.content {
            PanelContent::Document {
                document: Some(document),
                ..
            } => document.borrow().contents().to_owned(),
            _ => return,
        };
        match EiyashouProjection::parse(&source).delete_blocks(&source, &self.selected_blocks) {
            Ok(edited) => self.apply_block_source(edited, "Blocks deleted", window, cx),
            Err(error) => self.set_block_notice(format!("Delete blocked: {error}"), cx),
        }
    }

    fn move_selected_blocks_up(
        &mut self,
        _: &MoveBlocksUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selected_blocks(MoveDirection::Up, window, cx);
    }

    fn move_selected_blocks_down(
        &mut self,
        _: &MoveBlocksDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selected_blocks(MoveDirection::Down, window, cx);
    }

    fn move_selected_blocks(
        &mut self,
        direction: MoveDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        let source = match &self.content {
            PanelContent::Document {
                document: Some(document),
                ..
            } => document.borrow().contents().to_owned(),
            _ => return,
        };
        match EiyashouProjection::parse(&source).move_blocks(
            &source,
            &self.selected_blocks,
            direction,
        ) {
            Ok(edited) => self.apply_block_source(edited, "Blocks moved", window, cx),
            Err(error) => self.set_block_notice(format!("Move blocked: {error}"), cx),
        }
    }

    fn drop_blocks(
        &mut self,
        drag: &BlockDrag,
        target_start: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        let source = match &self.content {
            PanelContent::Document {
                document: Some(document),
                ..
            } => document.borrow().contents().to_owned(),
            _ => return,
        };
        match EiyashouProjection::parse(&source).move_blocks_to(
            &source,
            &drag.selected,
            target_start,
        ) {
            Ok(edited) if edited != source => {
                self.apply_block_source(edited, "Blocks moved", window, cx)
            }
            Ok(_) => {}
            Err(error) => self.set_block_notice(format!("Move blocked: {error}"), cx),
        }
    }

    fn apply_block_source(
        &mut self,
        edited: String,
        notice: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PanelContent::Document { root, editor, .. } = &self.content else {
            return;
        };
        let root = root.clone();
        let editor = editor.clone();
        editor.update(cx, |editor, cx| editor.replace_all(edited, window, cx));
        self.selected_blocks.clear();
        self.block_selection_anchor = None;
        self.rebuild_visual_editors(window, cx);
        cx.global_mut::<EditorDocuments>()
            .clear_block_selection(&root);
        cx.global_mut::<EditorDocuments>().set_notice(&root, notice);
        cx.notify();
        cx.refresh_windows();
    }

    fn set_block_notice(&self, notice: String, cx: &mut Context<Self>) {
        if let PanelContent::Document { root, .. } = &self.content {
            cx.global_mut::<EditorDocuments>().set_notice(root, notice);
            cx.refresh_windows();
        }
    }

    fn insert_from_palette(
        &mut self,
        kind: InsertKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PanelContent::Document {
            root,
            document: Some(document),
            editor,
            ..
        } = &self.content
        else {
            return;
        };
        let source = document.borrow().contents().to_owned();
        let line = document.borrow().selection().line;
        let index = cx.global::<EditorDocuments>().authoring(root);
        match insert_statement(&source, line, kind, &index) {
            Ok(edited) => {
                editor.update(cx, |editor, cx| editor.replace_all(edited, window, cx));
                cx.global_mut::<EditorDocuments>().set_notice(
                    root,
                    format!("Inserted {} at a source line boundary", kind.label()),
                );
            }
            Err(error) => cx
                .global_mut::<EditorDocuments>()
                .set_notice(root, format!("Insert blocked: {error}")),
        }
        cx.refresh_windows();
    }

    fn add_character(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let PanelContent::Characters { root } = &self.content else {
            return;
        };
        if self.tool_inputs.len() != 3 {
            return;
        }
        let id = self.tool_inputs[0].read(cx).value().to_string();
        let name = self.tool_inputs[1].read(cx).value().to_string();
        let color = self.tool_inputs[2].read(cx).value().to_string();
        let index = cx.global::<EditorDocuments>().authoring(root);
        let Some(path) = index.characters_manifest else {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "Character manifest is unavailable");
            return;
        };
        let result = cx
            .global_mut::<EditorDocuments>()
            .open(root, &path)
            .map_err(|error| error.to_string())
            .and_then(|document| {
                append_character(
                    document.borrow().contents(),
                    id.trim(),
                    name.trim(),
                    Some(color.trim()),
                )
                .map_err(|error| error.to_string())
            });
        match result {
            Ok(edited) => {
                apply_workspace_edit(root, &path, edited, window, cx);
                for input in &self.tool_inputs {
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                }
                cx.global_mut::<EditorDocuments>()
                    .set_notice(root, format!("Added character `{}`", id.trim()));
            }
            Err(error) => cx
                .global_mut::<EditorDocuments>()
                .set_notice(root, format!("Character edit blocked: {error}")),
        }
        cx.refresh_windows();
    }

    fn add_scene(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let PanelContent::Scenes { root } = &self.content else {
            return;
        };
        let Some(input) = self.tool_inputs.first() else {
            return;
        };
        let id = input.read(cx).value().to_string();
        let index = cx.global::<EditorDocuments>().authoring(root);
        let selected = cx
            .global::<EditorDocuments>()
            .selection(root)
            .map(|(path, _, _)| path.clone());
        let path = selected
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("shou"))
            .or_else(|| index.scenes.first().map(|scene| scene.path.clone()));
        let Some(path) = path else {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "No Eiyashou source is available");
            return;
        };
        let result = cx
            .global_mut::<EditorDocuments>()
            .open(root, &path)
            .map_err(|error| error.to_string())
            .and_then(|document| {
                append_scene(document.borrow().contents(), id.trim())
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(edited) => {
                apply_workspace_edit(root, &path, edited, window, cx);
                input.update(cx, |input, cx| input.set_value("", window, cx));
                cx.global_mut::<EditorDocuments>()
                    .set_notice(root, format!("Added scene `{}`", id.trim()));
            }
            Err(error) => cx
                .global_mut::<EditorDocuments>()
                .set_notice(root, format!("Scene edit blocked: {error}")),
        }
        cx.refresh_windows();
    }

    fn explorer_root(&self) -> Option<PathBuf> {
        match &self.content {
            PanelContent::Explorer { root, .. } => Some(root.clone()),
            _ => None,
        }
    }

    fn selected_directory(&self) -> PathBuf {
        let PanelContent::Explorer { files, .. } = &self.content else {
            return PathBuf::new();
        };
        let Some(selected) = self.file_selection.as_ref() else {
            return PathBuf::new();
        };
        if files
            .iter()
            .find(|file| &file.relative_path == selected)
            .is_some_and(WorkspaceFile::is_dir)
        {
            selected.clone()
        } else {
            selected
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_owned()
        }
    }

    fn begin_file_edit(
        &mut self,
        mode: FileEditMode,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_name_input
            .update(cx, |input, cx| input.set_value(initial, window, cx));
        self.file_edit = Some(mode);
        self.file_name_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn open_file_context_menu(
        &mut self,
        path: PathBuf,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.file_context_epoch = self.file_context_epoch.wrapping_add(1);
        self.file_selection = Some(path.clone());
        self.file_context_menu = Some(FileContextMenu {
            path,
            position,
            epoch: self.file_context_epoch,
            closing: false,
        });
        cx.notify();
    }

    fn close_file_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.file_context_menu.as_mut() else {
            return;
        };
        if menu.closing {
            return;
        }
        self.file_context_epoch = self.file_context_epoch.wrapping_add(1);
        menu.epoch = self.file_context_epoch;
        menu.closing = true;
        let epoch = menu.epoch;
        let delay = if cx.reduce_motion() {
            Duration::ZERO
        } else {
            Duration::from_millis(90)
        };
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this
                    .file_context_menu
                    .as_ref()
                    .is_some_and(|menu| menu.epoch == epoch && menu.closing)
                {
                    this.file_context_menu = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn commit_file_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.explorer_root() else {
            return;
        };
        let Some(mode) = self.file_edit.clone() else {
            return;
        };
        let name = self.file_name_input.read(cx).value().trim().to_owned();
        let result = match mode {
            FileEditMode::NewFile { parent } => {
                file_ops::create_file(&root, &parent, &name).map(|_| None)
            }
            FileEditMode::NewFolder { parent } => {
                file_ops::create_directory(&root, &parent, &name).map(|_| None)
            }
            FileEditMode::Rename { path } => {
                if !self.manifest_mutation_ready(&root, window, cx) {
                    return;
                }
                file_ops::rename_entry(&root, &path, &name).map(Some)
            }
        };
        match result {
            Ok(update) => {
                self.file_edit = None;
                if let Some(update) = update {
                    self.accept_file_result(&root, update, window, cx);
                } else {
                    self.refresh_explorer(&root, cx);
                }
            }
            Err(error) => window.push_notification(Notification::error(short_error(&error)), cx),
        }
        cx.notify();
    }

    fn manifest_mutation_ready(
        &self,
        root: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if cx
            .global_mut::<EditorDocuments>()
            .asset_manifest_is_clean(root)
        {
            true
        } else {
            window.push_notification(Notification::warning("Save assets.yaml first"), cx);
            false
        }
    }

    fn accept_file_result(
        &mut self,
        root: &Path,
        result: ImportResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((relative, source)) = result.manifest_update {
            match cx.global_mut::<EditorDocuments>().adopt_manifest_update(
                root,
                &relative,
                source.clone(),
            ) {
                Ok(Some(editor)) => {
                    let _ = editor.update(cx, |editor, cx| {
                        editor.replace_all(source, window, cx);
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    window.push_notification(Notification::error(short_error(&error)), cx)
                }
            }
        }
        self.refresh_explorer(root, cx);
    }

    fn refresh_explorer(&mut self, root: &Path, cx: &mut Context<Self>) {
        match cx.global_mut::<EditorDocuments>().refresh_files(root) {
            Ok(refreshed) => {
                if let PanelContent::Explorer { files, .. } = &mut self.content {
                    *files = refreshed;
                }
            }
            Err(error) => cx
                .global_mut::<EditorDocuments>()
                .set_notice(root, format!("File refresh failed: {error}")),
        }
        cx.refresh_windows();
    }

    fn paste_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.explorer_root() else {
            return;
        };
        let Some(source) = self.file_clipboard.clone() else {
            return;
        };
        if !self.manifest_mutation_ready(&root, window, cx) {
            return;
        }
        let target = self.selected_directory();
        match file_ops::copy_entry(&root, &source, &target) {
            Ok(results) => {
                for result in results {
                    self.accept_file_result(&root, result, window, cx);
                }
                window.push_notification(Notification::success("Copied"), cx);
            }
            Err(error) => window.push_notification(Notification::error(short_error(&error)), cx),
        }
    }

    fn move_file(
        &mut self,
        source: &Path,
        target: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.explorer_root() else {
            return;
        };
        if !self.manifest_mutation_ready(&root, window, cx) {
            return;
        }
        match file_ops::move_entry(&root, source, target) {
            Ok(result) => self.accept_file_result(&root, result, window, cx),
            Err(error) => window.push_notification(Notification::error(short_error(&error)), cx),
        }
    }

    fn confirm_delete_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.explorer_root() else {
            return;
        };
        let Some(path) = self.file_selection.clone() else {
            return;
        };
        let receiver = window.prompt(
            PromptLevel::Warning,
            "Delete selected item?",
            Some(&path.display().to_string()),
            &[
                PromptButton::Other("Delete".into()),
                PromptButton::Cancel("Cancel".into()),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if receiver.await.ok() != Some(0) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                match file_ops::delete_entry(&root, &path) {
                    Ok(()) => {
                        this.file_selection = None;
                        this.refresh_explorer(&root, cx);
                        window.push_notification(Notification::success("Deleted"), cx);
                    }
                    Err(error) => {
                        window.push_notification(Notification::error(short_error(&error)), cx)
                    }
                }
            });
        })
        .detach();
    }

    fn start_external_import(
        &mut self,
        paths: Vec<PathBuf>,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.explorer_root() else {
            return;
        };
        if paths.is_empty() || !self.manifest_mutation_ready(&root, window, cx) {
            return;
        }
        self.file_progress = Some(FileProgress {
            completed: 0,
            total: paths.len(),
        });
        let total = paths.len();
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let mut succeeded = 0;
            let mut failed = 0;
            let mut last_manifest = None;
            for (index, source) in paths.into_iter().enumerate() {
                let root_for_task = root.clone();
                let target_for_task = target.clone();
                let result = background
                    .spawn(async move {
                        file_ops::import_external(&root_for_task, &target_for_task, &source)
                    })
                    .await;
                match result {
                    Ok(result) => {
                        succeeded += 1;
                        if result.manifest_update.is_some() {
                            last_manifest = result.manifest_update;
                        }
                    }
                    Err(_) => failed += 1,
                }
                let _ = this.update_in(cx, |this, _, cx| {
                    this.file_progress = Some(FileProgress {
                        completed: index + 1,
                        total,
                    });
                    cx.notify();
                });
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.file_progress = None;
                if let Some((relative, source)) = last_manifest {
                    let result = ImportResult {
                        destination: PathBuf::new(),
                        manifest_update: Some((relative, source)),
                        registered: true,
                    };
                    this.accept_file_result(&root, result, window, cx);
                } else {
                    this.refresh_explorer(&root, cx);
                }
                if failed == 0 {
                    window.push_notification(
                        Notification::success(format!("Imported {succeeded}")),
                        cx,
                    );
                } else {
                    window.push_notification(
                        Notification::warning(format!("Imported {succeeded} · {failed} failed")),
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl BasePanel for WorkbenchPanel {
    fn panel_name(&self) -> &'static str {
        self.content.panel_name()
    }

    fn dump(&self, _: &App) -> PanelState {
        PanelState {
            panel_name: self.panel_name().to_owned(),
            children: Vec::new(),
            info: PanelInfo::panel(
                serde_json::to_value(self.content.payload()).unwrap_or(serde_json::Value::Null),
            ),
        }
    }

    fn set_active(&mut self, active: bool, _: &mut Window, _: &mut Context<Self>) {
        if let PanelContent::Preview { controller, .. } = &self.content {
            controller.set_panel_visible(active);
        }
    }

    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        match &self.content {
            PanelContent::Document { root, relative, .. } => {
                let panel = PanelId::from(cx.entity_id());
                let documents = cx.global_mut::<EditorDocuments>();
                documents.unregister_panel(root, relative, panel);
                documents.clear_document_node(root);
            }
            PanelContent::Preview { root, controller } => {
                controller.stop();
                controller.set_panel_visible(false);
                cx.global_mut::<EditorDocuments>()
                    .set_preview_panel(root, None);
            }
            PanelContent::Assets { root } => {
                cx.global_mut::<EditorDocuments>()
                    .set_tool_panel(root, ASSETS_PANEL, None)
            }
            PanelContent::Characters { root } => {
                cx.global_mut::<EditorDocuments>()
                    .set_tool_panel(root, CHARACTERS_PANEL, None)
            }
            PanelContent::Scenes { root } => {
                cx.global_mut::<EditorDocuments>()
                    .set_tool_panel(root, SCENES_PANEL, None)
            }
            PanelContent::Problems { root } => {
                cx.global_mut::<EditorDocuments>()
                    .set_tool_panel(root, PROBLEMS_PANEL, None)
            }
            PanelContent::Performance { root, .. } => cx
                .global_mut::<EditorDocuments>()
                .set_tool_panel(root, PERFORMANCE_PANEL, None),
            _ => {}
        }
    }
}

impl Panel for WorkbenchPanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some(self.content.title())
    }
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.content.title()
    }
}

impl EventEmitter<PanelEvent> for WorkbenchPanel {}

impl Focusable for WorkbenchPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for WorkbenchPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.file_commit_requested {
            self.file_commit_requested = false;
            self.commit_file_edit(window, cx);
        }
        if let PanelContent::Inspector { root, .. } = &self.content {
            let root = root.clone();
            self.refresh_inspector_editors(&root, window, cx);
        }
        let mono = Theme::global(cx).mono_font_family.clone();
        let body = match &self.content {
            PanelContent::Explorer { root, files } => {
                let project_root = root.clone();
                let files = files.clone();
                let project_name = project_root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("PROJECT")
                    .to_uppercase();
                let selected = self.file_selection.clone();
                let selected_directory = selected
                    .as_ref()
                    .and_then(|path| {
                        files
                            .iter()
                            .find(|file| &file.relative_path == path)
                            .map(|file| {
                                if file.is_dir() {
                                    path.clone()
                                } else {
                                    path.parent().unwrap_or_else(|| Path::new("")).to_owned()
                                }
                            })
                    })
                    .unwrap_or_default();
                let visible = files
                    .iter()
                    .filter(|file| {
                        let mut ancestor = file.relative_path.parent();
                        while let Some(path) = ancestor {
                            if self.file_collapsed.contains(path) {
                                return false;
                            }
                            ancestor = path.parent();
                        }
                        true
                    })
                    .take(800)
                    .cloned()
                    .collect::<Vec<_>>();
                let new_file_parent = selected_directory.clone();
                let new_folder_parent = selected_directory.clone();
                let refresh_root = project_root.clone();
                let external_root_target = PathBuf::new();
                let internal_root_target = PathBuf::new();
                let header =
                    div()
                        .h(px(30.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pl_3()
                        .pr_1()
                        .text_xs()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(rgb(MUTED))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .child(project_name),
                        )
                        .child(file_action_icon("file-new", AssetIconName::File).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.begin_file_edit(
                                    FileEditMode::NewFile {
                                        parent: new_file_parent.clone(),
                                    },
                                    "",
                                    window,
                                    cx,
                                );
                            }),
                        ))
                        .child(
                            file_action_icon("folder-new", AssetIconName::Folder).on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.begin_file_edit(
                                        FileEditMode::NewFolder {
                                            parent: new_folder_parent.clone(),
                                        },
                                        "",
                                        window,
                                        cx,
                                    );
                                }),
                            ),
                        )
                        .when(self.file_clipboard.is_some(), |this| {
                            this.child(
                                file_action_icon("file-paste", AssetIconName::Copy).on_click(
                                    cx.listener(|this, _, window, cx| this.paste_file(window, cx)),
                                ),
                            )
                        })
                        .child(
                            file_action_icon("file-refresh", AssetIconName::RotateCw).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.refresh_explorer(&refresh_root, cx)
                                }),
                            ),
                        );
                let edit_row =
                    self.file_edit.as_ref().map(|_| {
                        div()
                            .h(px(30.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_1()
                            .mx_2()
                            .px_1()
                            .rounded(px(6.))
                            .bg(rgb(SURFACE))
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&self.file_name_input)
                                        .appearance(false)
                                        .bordered(false)
                                        .size_full()
                                        .text_xs()
                                        .text_color(rgb(INK)),
                                ),
                            )
                            .child(
                                file_action_icon("file-edit-accept", AssetIconName::Check)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.commit_file_edit(window, cx)
                                    })),
                            )
                            .child(
                                file_action_icon("file-edit-cancel", AssetIconName::Close)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.file_edit = None;
                                        cx.notify();
                                    })),
                            )
                    });
                let progress = self.file_progress.as_ref().map(|progress| {
                    div()
                        .h(px(24.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(
                            Icon::new(IconName::LoaderCircle)
                                .xsmall()
                                .text_color(rgb(PRIMARY)),
                        )
                        .child(format!(
                            "Importing {} / {}",
                            progress.completed, progress.total
                        ))
                });
                let context_menu = self.file_context_menu.clone().map(|menu| {
                    let rename_path = menu.path.clone();
                    let rename_name = menu
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_owned();
                    let copy_path = menu.path.clone();
                    let reveal_root = project_root.clone();
                    let reveal_path = menu.path.clone();
                    let delete_path = menu.path.clone();
                    let closing = menu.closing;
                    let motion_duration = if closing { 90 } else { 120 };
                    let menu_surface = div()
                        .id(("file-context-surface", menu.epoch))
                        .w(px(FILE_CONTEXT_MENU_WIDTH_PX))
                        .h(px(FILE_CONTEXT_MENU_HEIGHT_PX))
                        .overflow_hidden()
                        .p_1()
                        .rounded(px(7.))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(SURFACE))
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .child(
                            file_context_menu_item(
                                "file-context-rename",
                                AssetIconName::Replace,
                                "Rename",
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.close_file_context_menu(window, cx);
                                    this.begin_file_edit(
                                        FileEditMode::Rename {
                                            path: rename_path.clone(),
                                        },
                                        &rename_name,
                                        window,
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            file_context_menu_item(
                                "file-context-copy",
                                AssetIconName::Copy,
                                "Copy",
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.file_clipboard = Some(copy_path.clone());
                                    this.close_file_context_menu(window, cx);
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            file_context_menu_item(
                                "file-context-reveal",
                                AssetIconName::ExternalLink,
                                "Reveal",
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    reveal_workspace_path(&reveal_root, &reveal_path);
                                    this.close_file_context_menu(window, cx);
                                },
                            )),
                        )
                        .child(
                            file_context_menu_item(
                                "file-context-delete",
                                AssetIconName::Delete,
                                "Delete",
                                true,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.file_selection = Some(delete_path.clone());
                                    this.close_file_context_menu(window, cx);
                                    this.confirm_delete_file(window, cx);
                                },
                            )),
                        )
                        .with_animation(
                            ("file-context-motion", menu.epoch),
                            Animation::new(Duration::from_millis(motion_duration))
                                .with_easing(ease_out_quint()),
                            move |surface, delta| {
                                let progress = if closing { 1. - delta } else { delta };
                                let scale = 0.94 + progress * 0.06;
                                surface
                                    .opacity(progress)
                                    .w(px(FILE_CONTEXT_MENU_WIDTH_PX * scale))
                                    .h(px(FILE_CONTEXT_MENU_HEIGHT_PX * scale))
                            },
                        );
                    deferred(
                        anchored()
                            .anchor(Anchor::TopLeft)
                            .position(menu.position)
                            .snap_to_window_with_margin(px(6.))
                            .child(
                                div()
                                    .w(px(FILE_CONTEXT_MENU_WIDTH_PX))
                                    .h(px(FILE_CONTEXT_MENU_HEIGHT_PX))
                                    .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                                        this.close_file_context_menu(window, cx)
                                    }))
                                    .child(menu_surface),
                            ),
                    )
                    .priority(100)
                });
                let content = div()
                    .flex()
                    .flex_col()
                    .child(header)
                    .children(edit_row)
                    .children(progress)
                    .child(
                        div()
                            .id("explorer-drop-root")
                            .w_full()
                            .min_h(px(80.))
                            .drag_over::<ExternalPaths>(|style, _, _, _| {
                                style.bg(rgb(SURFACE_HOVER))
                            })
                            .drag_over::<FileDrag>(|style, _, _, _| style.bg(rgb(SURFACE_HOVER)))
                            .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
                                this.start_external_import(
                                    paths.paths().to_vec(),
                                    external_root_target.clone(),
                                    window,
                                    cx,
                                );
                            }))
                            .on_drop(cx.listener(move |this, drag: &FileDrag, window, cx| {
                                this.move_file(&drag.relative, &internal_root_target, window, cx);
                            }))
                            .child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .children(visible.into_iter().enumerate().map(
                                        |(index, file)| {
                                            let root = project_root.clone();
                                            let relative = file.relative_path.clone();
                                            let click_relative = relative.clone();
                                            let context_relative = relative.clone();
                                            let drag = FileDrag {
                                                relative: relative.clone(),
                                            };
                                            let is_dir = file.kind == WorkspaceEntryKind::Directory;
                                            let openable = !is_dir
                                                && matches!(
                                                    relative
                                                        .extension()
                                                        .and_then(|extension| extension.to_str()),
                                                    Some(
                                                        "shou"
                                                            | "txt"
                                                            | "json"
                                                            | "yaml"
                                                            | "yml"
                                                            | "toml"
                                                            | "md"
                                                            | "webgal"
                                                    )
                                                );
                                            let depth =
                                                relative.components().count().saturating_sub(1);
                                            let name = relative
                                                .file_name()
                                                .and_then(|name| name.to_str())
                                                .unwrap_or("File")
                                                .to_owned();
                                            let row_selected = selected.as_ref() == Some(&relative);
                                            let collapsed = self.file_collapsed.contains(&relative);
                                            let drop_target = relative.clone();
                                            let external_target = relative.clone();
                                            div()
                                                .id(("explorer-file", index))
                                                .h(px(25.))
                                                .w_full()
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .pl(px(5. + depth as f32 * 14.))
                                                .pr_2()
                                                .rounded(px(5.))
                                                .whitespace_nowrap()
                                                .text_xs()
                                                .text_color(rgb(if row_selected {
                                                    INK
                                                } else {
                                                    MUTED
                                                }))
                                                .when(row_selected, |style| {
                                                    style.bg(rgb(SURFACE_HOVER))
                                                })
                                                .cursor_pointer()
                                                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.file_selection =
                                                            Some(click_relative.clone());
                                                        if is_dir {
                                                            if !this
                                                                .file_collapsed
                                                                .insert(click_relative.clone())
                                                            {
                                                                this.file_collapsed
                                                                    .remove(&click_relative);
                                                            }
                                                            cx.notify();
                                                        } else if openable {
                                                            open_workspace_document(
                                                                &root,
                                                                &click_relative,
                                                                window,
                                                                cx,
                                                            );
                                                        }
                                                    },
                                                ))
                                                .on_mouse_down(
                                                    MouseButton::Right,
                                                    cx.listener(
                                                        move |this,
                                                              event: &MouseDownEvent,
                                                              _,
                                                              cx| {
                                                            this.open_file_context_menu(
                                                                context_relative.clone(),
                                                                event.position,
                                                                cx,
                                                            );
                                                            cx.stop_propagation();
                                                        },
                                                    ),
                                                )
                                                .on_drag(drag, |drag: &FileDrag, _, _, cx| {
                                                    cx.new(|_| drag.clone())
                                                })
                                                .when(is_dir, |row| {
                                                    row.drag_over::<FileDrag>(|style, _, _, _| {
                                                    style.bg(rgb(PRIMARY_DIM))
                                                })
                                                .drag_over::<ExternalPaths>(|style, _, _, _| {
                                                    style.bg(rgb(PRIMARY_DIM))
                                                })
                                                .on_drop(cx.listener(
                                                    move |this, drag: &FileDrag, window, cx| {
                                                        this.move_file(
                                                            &drag.relative,
                                                            &drop_target,
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                ))
                                                .on_drop(cx.listener(
                                                    move |this,
                                                          paths: &ExternalPaths,
                                                          window,
                                                          cx| {
                                                        this.start_external_import(
                                                            paths.paths().to_vec(),
                                                            external_target.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                ))
                                                })
                                                .child(
                                                    Icon::new(if is_dir {
                                                        if collapsed {
                                                            IconName::ChevronRight
                                                        } else {
                                                            IconName::ChevronDown
                                                        }
                                                    } else {
                                                        IconName::File
                                                    })
                                                    .xsmall()
                                                    .text_color(rgb(if is_dir {
                                                        MUTED
                                                    } else {
                                                        PRIMARY
                                                    })),
                                                )
                                                .when(is_dir, |row| {
                                                    row.child(
                                                        Icon::new(if collapsed {
                                                            IconName::FolderClosed
                                                        } else {
                                                            IconName::FolderOpen
                                                        })
                                                        .xsmall()
                                                        .text_color(rgb(PRIMARY)),
                                                    )
                                                })
                                                .child(name)
                                        },
                                    ))
                                    .overflow_x_scrollbar()
                                    .id("explorer-files"),
                            ),
                    );
                div()
                    .relative()
                    .size_full()
                    .min_h_0()
                    .child(vertical_overflow_view(
                        "explorer-vertical-scroll",
                        &self.view_scroll,
                        content,
                    ))
                    .children(context_menu)
                    .into_any_element()
            }
            PanelContent::Document {
                root,
                relative,
                document,
                editor,
            } => {
                let eiyashou = document.is_some()
                    && relative.extension().and_then(|value| value.to_str()) == Some("shou");
                let mode = self.document_mode;
                let text_root = root.clone();
                let text_relative = relative.clone();
                let text_editor = editor.clone();
                let header = eiyashou.then(|| {
                    let text_selected = mode == DocumentMode::Text;
                    let block_selected = mode == DocumentMode::Block;
                    div()
                        .h(px(34.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_1()
                        .px_2()
                        .bg(rgb(CHROME))
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .child(document_mode_button("Text", text_selected).on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.document_mode = DocumentMode::Text;
                                        if let Some((_, line, column)) = cx
                                            .global::<EditorDocuments>()
                                            .selection(&text_root)
                                            .filter(|(path, _, _)| path == &text_relative)
                                        {
                                            let line = *line;
                                            let column = *column;
                                            text_editor.update(cx, |editor, cx| {
                                                editor.set_cursor_position(
                                                    Position::new(line as u32, column as u32),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }
                                        cx.notify();
                                    }),
                                ))
                                .child(document_mode_button("Blocks", block_selected).on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.rebuild_visual_editors(window, cx);
                                        this.document_mode = DocumentMode::Block;
                                        this.block_scroll_pending = true;
                                        if let PanelContent::Document { root, relative, .. } =
                                            &this.content
                                        {
                                            cx.global_mut::<EditorDocuments>().set_block_selection(
                                                root,
                                                relative.clone(),
                                                this.selected_blocks.iter().copied().collect(),
                                            );
                                        }
                                        cx.notify();
                                    }),
                                )),
                        )
                });
                let body = if eiyashou && mode == DocumentMode::Block {
                    render_block_projection(
                        BlockProjectionView {
                            root,
                            relative,
                            document: document.as_ref().unwrap(),
                            editors: &self.block_text_editors,
                            collapsed_scenes: &self.collapsed_scenes,
                            selected_blocks: &self.selected_blocks,
                            draft_text: self.draft_text.as_ref(),
                            drop_target: self.block_drop_target,
                            scroll_handle: &self.view_scroll,
                            scroll_anchor: &self.block_scroll_anchor,
                            scroll_pending: self.block_scroll_pending,
                        },
                        window,
                        cx,
                    )
                } else {
                    let editor_for_fade = editor.clone();
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .bottom_0()
                        .left(px(-EDITOR_GUTTER_TRIM_PX))
                        .child(
                            Editor::new(editor)
                                .appearance(false)
                                .bordered(false)
                                .readonly(document.is_none())
                                .h(gpui_kit::relative(1.))
                                .w_full()
                                .p_1()
                                .font_family(mono)
                                .text_size(px(13.))
                                .text_color(rgb(0xc8cbd0)),
                        )
                        .child(bottom_overflow_fade(move |cx| {
                            let editor = editor_for_fade.read(cx);
                            let row_count = editor.value().lines().count();
                            editor
                                .visible_row_range()
                                .is_some_and(|visible| visible.end < row_count)
                        }))
                        .into_any_element()
                };
                let picker = (eiyashou && mode == DocumentMode::Block && self.block_picker_open)
                    .then(|| {
                        let query = self
                            .block_picker_input
                            .read(cx)
                            .value()
                            .to_string()
                            .to_lowercase();
                        let preferences = cx
                            .global::<EditorDocuments>()
                            .block_picker_preferences()
                            .clone();
                        let kinds = picker_kinds(
                            &preferences,
                            &query,
                            self.block_picker_category,
                            self.block_picker_customize,
                        );
                        render_block_picker(
                            &kinds,
                            &self.block_picker_input,
                            self.block_picker_index,
                            self.block_picker_category,
                            self.block_picker_customize,
                            &preferences,
                            cx,
                        )
                    });
                if eiyashou && mode == DocumentMode::Block {
                    self.block_scroll_pending = false;
                }
                div()
                    .id("document-content")
                    .size_full()
                    .flex()
                    .flex_col()
                    .rounded_b(px(VIEW_RADIUS_PX))
                    .bg(rgb(CANVAS))
                    .overflow_hidden()
                    .when_some(header, |this, header| this.child(header))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(body)
                            .when_some(picker, |this, picker| this.child(picker)),
                    )
                    .into_any_element()
            }
            PanelContent::Preview { root, controller } => {
                let snapshot = controller.snapshot();
                let running = matches!(
                    snapshot.lifecycle,
                    PreviewLifecycle::Running | PreviewLifecycle::Paused
                );
                let status = match &snapshot.lifecycle {
                    PreviewLifecycle::Off => "Stopped".to_owned(),
                    PreviewLifecycle::Starting => "Starting…".to_owned(),
                    PreviewLifecycle::Running => format!(
                        "Live · frame {} · dropped {}",
                        snapshot.frame_stats.published, snapshot.frame_stats.overwritten
                    ),
                    PreviewLifecycle::Paused => "Paused while hidden".to_owned(),
                    PreviewLifecycle::Failed(error) => format!("Failed · {error}"),
                };
                let start = controller.clone();
                let start_root = root.clone();
                let stop = controller.clone();
                let edit = controller.clone();
                let play = controller.clone();
                let input = controller.clone();
                let keyboard_input = controller.clone();
                let mode = snapshot.mode;
                let bounds = self.preview_bounds.clone();
                let surface_bounds = self.preview_bounds.clone();
                let preview_focus = self.focus.clone();
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .bg(rgb(CANVAS))
                    .child(
                        div()
                            .h(px(38.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .bg(rgb(CHROME))
                            .child(preview_control("Edit", mode == PreviewMode::Edit).on_click(
                                move |_, _, cx| {
                                    edit.set_mode(PreviewMode::Edit);
                                    cx.refresh_windows();
                                },
                            ))
                            .child(preview_control("Play", mode == PreviewMode::Play).on_click(
                                move |_, _, cx| {
                                    play.set_mode(PreviewMode::Play);
                                    cx.refresh_windows();
                                },
                            ))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(match snapshot.lifecycle {
                                        PreviewLifecycle::Failed(_) => 0xdb7780,
                                        PreviewLifecycle::Running => SUCCESS,
                                        _ => MUTED,
                                    }))
                                    .child(status),
                            )
                            .child(if running {
                                preview_control("Stop", false).on_click(move |_, _, cx| {
                                    stop.stop();
                                    cx.refresh_windows();
                                })
                            } else {
                                preview_control("Start", true).on_click(move |_, _, cx| {
                                    for (path, contents) in cx
                                        .global::<EditorDocuments>()
                                        .preview_documents(&start_root)
                                    {
                                        start.apply_snapshot(path, contents);
                                    }
                                    start.start();
                                    cx.refresh_windows();
                                })
                            }),
                    )
                    .child(
                        div()
                            .id("preview-surface")
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgb(0x050607))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                                preview_focus.focus(window, cx);
                                if mode != PreviewMode::Play {
                                    return;
                                }
                                let bounds =
                                    *bounds.lock().expect("preview surface bounds lock poisoned");
                                let Some(bounds) = bounds else {
                                    return;
                                };
                                let local_x = f32::from(event.position.x - bounds.origin.x);
                                let local_y = f32::from(event.position.y - bounds.origin.y);
                                if let Some((x, y)) = map_preview_point(
                                    f32::from(bounds.size.width),
                                    f32::from(bounds.size.height),
                                    local_x,
                                    local_y,
                                ) {
                                    input.input(keine_authoring::PreviewInput::PointerPressed {
                                        x,
                                        y,
                                    });
                                }
                            })
                            .on_key_down(move |event, _, cx| {
                                if mode == PreviewMode::Play
                                    && !event.keystroke.modifiers.control
                                    && !event.keystroke.modifiers.alt
                                    && !event.keystroke.modifiers.platform
                                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                                {
                                    keyboard_input.input(keine_authoring::PreviewInput::Advance);
                                    cx.stop_propagation();
                                }
                            })
                            .when_some(self.preview_image.clone(), |this, image| {
                                this.child(img(image).size_full().object_fit(ObjectFit::Contain))
                            })
                            .when(self.preview_image.is_none(), |this| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(MUTED))
                                        .child("Start Preview to render the current project"),
                                )
                            })
                            .child(
                                canvas(
                                    move |surface, _, _| {
                                        *surface_bounds
                                            .lock()
                                            .expect("preview surface bounds lock poisoned") =
                                            Some(surface);
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .inset_0(),
                            ),
                    )
                    .into_any_element()
            }
            PanelContent::Inspector { root, file_count } => {
                let document_count = cx.global::<EditorDocuments>().open_document_count(root);
                let index = cx.global::<EditorDocuments>().authoring(root);
                let has_selection = cx.global::<EditorDocuments>().selection(root).is_some();
                let inputs = self.inspector_inputs.clone();
                let content = div()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_3()
                    .when(has_selection, |this| {
                        this.child(section_label("SELECTION"))
                            .child(selection_summary(root, &index, cx))
                            .when(inputs.len() == 3, |this| {
                                this.child(section_label("TEXT"))
                                    .child(property_input("Speaker", &inputs[0]))
                                    .child(property_input("Voice", &inputs[1]))
                                    .child(property_input("Stable ID", &inputs[2]))
                            })
                    })
                    .when(!has_selection, |this| {
                        this.child(section_label("WORKSPACE"))
                            .child(property_row("Path", root.display().to_string()))
                            .child(property_row("Text files", file_count.to_string()))
                            .child(property_row("Open documents", document_count.to_string()))
                    });
                vertical_overflow_view("inspector-scroll", &self.view_scroll, content)
            }
            PanelContent::Assets { root } => render_assets(
                root,
                &cx.global::<EditorDocuments>().authoring(root),
                &self.view_scroll,
            ),
            PanelContent::Characters { root } => {
                let index = cx.global::<EditorDocuments>().authoring(root);
                let inputs = self.tool_inputs.clone();
                let content = div()
                    .flex()
                    .flex_col()
                    .p_2()
                    .gap_2()
                    .child(section_label("CHARACTER MANIFEST"))
                    .when(inputs.len() == 3, |this| {
                        this.child(tool_input(&inputs[0]))
                            .child(tool_input(&inputs[1]))
                            .child(tool_input(&inputs[2]))
                            .child(tool_action("Add character").on_click(
                                cx.listener(|this, _, window, cx| this.add_character(window, cx)),
                            ))
                    })
                    .child(div().h(px(1.)).bg(rgb(SURFACE)))
                    .child(
                        div().flex().flex_col().gap_1().children(
                            index
                                .characters
                                .into_iter()
                                .enumerate()
                                .map(|(row, character)| {
                                    div()
                                        .id(("character-row", row))
                                        .p_2()
                                        .rounded(px(7.))
                                        .bg(rgb(PANEL))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(INK))
                                                .child(character.name),
                                        )
                                        .child(div().text_xs().text_color(rgb(MUTED)).child(
                                            format!(
                                                    "{}{}",
                                                    character.id,
                                                    character
                                                        .color
                                                        .map(|color| format!(" · {color}"))
                                                        .unwrap_or_default()
                                                ),
                                        ))
                                }),
                        ),
                    );
                vertical_overflow_view("character-scroll", &self.view_scroll, content)
            }
            PanelContent::Scenes { root } => {
                let index = cx.global::<EditorDocuments>().authoring(root);
                let input = self.tool_inputs.first().cloned();
                let root_for_rows = root.clone();
                let content = div()
                    .flex()
                    .flex_col()
                    .p_2()
                    .gap_2()
                    .child(section_label("SCENE DECLARATIONS"))
                    .when_some(input, |this, input| {
                        this.child(tool_input(&input))
                            .child(tool_action("Add scene").on_click(
                                cx.listener(|this, _, window, cx| this.add_scene(window, cx)),
                            ))
                    })
                    .child(div().h(px(1.)).bg(rgb(SURFACE)))
                    .child(div().flex().flex_col().gap_1().children(
                        index.scenes.into_iter().enumerate().map(|(row, scene)| {
                            let root = root_for_rows.clone();
                            let path = scene.path.clone();
                            let line = scene.line;
                            div()
                                .id(("scene-row", row))
                                .p_2()
                                .rounded(px(7.))
                                .bg(rgb(PANEL))
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                .on_click(move |_, window, cx| {
                                    navigate_source(&root, &path, line, 1, window, cx)
                                })
                                .child(div().text_sm().text_color(rgb(INK)).child(scene.name))
                                .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                                    "{}:{}",
                                    scene.path.display(),
                                    scene.line
                                )))
                        }),
                    ));
                vertical_overflow_view("scene-scroll", &self.view_scroll, content)
            }
            PanelContent::Problems { root } => render_problems(root, &self.view_scroll, cx),
            PanelContent::Performance { controller, .. } => {
                render_performance(controller, &self.timeline, &self.view_scroll)
            }
            PanelContent::Output { root, file_count } => {
                let content = div()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_2()
                    .font_family(mono)
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(output_line("READY", SUCCESS, root.display().to_string()))
                    .child(output_line(
                        "INDEX",
                        PRIMARY,
                        format!("{file_count} text files discovered"),
                    ))
                    .when_some(
                        cx.global::<EditorDocuments>()
                            .notice(root)
                            .map(str::to_owned),
                        |this, notice| this.child(output_line("EDIT", PRIMARY, notice)),
                    );
                vertical_overflow_view("output-scroll", &self.view_scroll, content)
            }
        };
        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::toggle_block_picker))
            .on_action(cx.listener(Self::block_picker_next))
            .on_action(cx.listener(Self::block_picker_previous))
            .on_action(cx.listener(Self::accept_block_picker))
            .on_action(cx.listener(Self::close_block_picker))
            .on_action(cx.listener(Self::begin_text_block))
            .on_action(cx.listener(Self::copy_selected_blocks))
            .on_action(cx.listener(Self::paste_blocks))
            .on_action(cx.listener(Self::delete_selected_blocks))
            .on_action(cx.listener(Self::move_selected_blocks_up))
            .on_action(cx.listener(Self::move_selected_blocks_down))
            .size_full()
            .text_color(rgb(INK))
            .child(body)
    }
}

fn take_tightly_packed_bgra(frame: keine_authoring::OwnedFrame) -> Option<Vec<u8>> {
    let row_bytes = usize::try_from(frame.metadata.width).ok()?.checked_mul(4)?;
    let stride = usize::try_from(frame.metadata.stride).ok()?;
    let height = usize::try_from(frame.metadata.height).ok()?;
    if stride < row_bytes || frame.bytes.len() != stride.checked_mul(height)? {
        return None;
    }
    if stride == row_bytes {
        return Some(frame.bytes);
    }
    let mut packed = Vec::with_capacity(row_bytes.checked_mul(height)?);
    for row in frame.bytes.chunks_exact(stride) {
        packed.extend_from_slice(&row[..row_bytes]);
    }
    Some(packed)
}

fn preview_control(label: &'static str, selected: bool) -> Stateful<Div> {
    div()
        .id(match label {
            "Edit" => "preview-edit",
            "Play" => "preview-play",
            "Start" => "preview-start",
            _ => "preview-stop",
        })
        .h(px(26.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(7.))
        .bg(rgb(if selected { SURFACE } else { CHROME }))
        .text_xs()
        .text_color(rgb(if selected { PRIMARY } else { MUTED }))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)).text_color(rgb(INK)))
        .child(label)
}

fn section_label(label: &'static str) -> impl IntoElement {
    div()
        .pt_1()
        .text_xs()
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(rgb(PRIMARY))
        .child(label)
}

fn property_row(label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .pb_2()
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(div().text_xs().text_color(rgb(MUTED)).child(label))
        .child(div().text_xs().text_color(rgb(INK)).child(value))
}

fn property_input(label: &'static str, state: &Entity<InputState>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .pb_2()
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(div().text_xs().text_color(rgb(MUTED)).child(label))
        .child(
            div().h(px(28.)).child(
                Input::new(state)
                    .appearance(false)
                    .bordered(false)
                    .size_full()
                    .text_xs()
                    .text_color(rgb(INK)),
            ),
        )
}

fn output_line(label: &'static str, color: u32, value: String) -> impl IntoElement {
    div()
        .flex()
        .gap_3()
        .child(div().w(px(44.)).text_color(rgb(color)).child(label))
        .child(value)
}

fn document_mode_button(label: &'static str, selected: bool) -> Stateful<Div> {
    div()
        .id(if label == "Text" {
            "document-mode-text"
        } else {
            "document-mode-block"
        })
        .h(px(25.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(rgb(if selected { SURFACE } else { CHROME }))
        .text_xs()
        .text_color(rgb(if selected { INK } else { MUTED }))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)).text_color(rgb(INK)))
        .child(label)
}

fn tool_input(state: &Entity<InputState>) -> impl IntoElement {
    div()
        .h(px(30.))
        .rounded(px(7.))
        .bg(rgb(SURFACE))
        .px_2()
        .child(
            Input::new(state)
                .appearance(false)
                .bordered(false)
                .size_full()
                .text_sm()
                .text_color(rgb(INK)),
        )
}

fn tool_action(label: &'static str) -> Stateful<Div> {
    div()
        .id(if label == "Add character" {
            "add-character"
        } else {
            "add-scene"
        })
        .h(px(28.))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .bg(rgb(PRIMARY_DIM))
        .text_xs()
        .text_color(rgb(PRIMARY))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)).text_color(rgb(INK)))
        .child(label)
}

const PICKER_CATEGORIES: [&str; 5] = ["Text", "Scene", "Media", "Flow", "Data"];

fn toggle_preference(values: &mut Vec<String>, value: &str) {
    if let Some(index) = values.iter().position(|candidate| candidate == value) {
        values.remove(index);
    } else {
        values.push(value.to_owned());
    }
}

fn move_preference(values: &mut Vec<String>, value: &str, universe: &[&str], delta: isize) {
    let mut ordered = values
        .iter()
        .map(String::as_str)
        .filter(|candidate| universe.contains(candidate))
        .collect::<Vec<_>>();
    for candidate in universe {
        if !ordered.contains(candidate) {
            ordered.push(candidate);
        }
    }
    let Some(index) = ordered.iter().position(|candidate| *candidate == value) else {
        return;
    };
    let target = index.saturating_add_signed(delta).min(ordered.len() - 1);
    ordered.swap(index, target);
    *values = ordered.into_iter().map(str::to_owned).collect();
}

fn move_group_preference(
    values: &mut Vec<String>,
    value: &str,
    group: &[&str],
    universe: &[&str],
    delta: isize,
) {
    let mut ordered = values
        .iter()
        .map(String::as_str)
        .filter(|candidate| universe.contains(candidate))
        .collect::<Vec<_>>();
    for candidate in universe {
        if !ordered.contains(candidate) {
            ordered.push(candidate);
        }
    }
    let group_positions = ordered
        .iter()
        .enumerate()
        .filter(|(_, candidate)| group.contains(candidate))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let Some(group_index) = group_positions
        .iter()
        .position(|index| ordered[*index] == value)
    else {
        return;
    };
    let target = group_index
        .saturating_add_signed(delta)
        .min(group_positions.len() - 1);
    ordered.swap(group_positions[group_index], group_positions[target]);
    *values = ordered.into_iter().map(str::to_owned).collect();
}

fn preference_rank(values: &[String], value: &str, fallback: usize) -> usize {
    values
        .iter()
        .position(|candidate| candidate == value)
        .unwrap_or(values.len() + fallback)
}

fn ordered_picker_categories(preferences: &BlockPickerPreferences) -> Vec<&'static str> {
    let mut categories = PICKER_CATEGORIES.to_vec();
    categories.sort_by_key(|category| {
        preference_rank(
            &preferences.category_order,
            category,
            PICKER_CATEGORIES
                .iter()
                .position(|candidate| candidate == category)
                .unwrap_or(usize::MAX / 2),
        )
    });
    categories
}

fn picker_kinds(
    preferences: &BlockPickerPreferences,
    query: &str,
    selected_category: Option<&str>,
    customize: bool,
) -> Vec<InsertKind> {
    let categories = ordered_picker_categories(preferences);
    let mut kinds = InsertKind::ALL
        .into_iter()
        .filter(|kind| {
            let matches_query = query.is_empty()
                || kind.search_terms().contains(query)
                || kind.label().to_lowercase().contains(query);
            if !matches_query {
                return false;
            }
            if !query.is_empty() {
                return true;
            }
            let visible = customize
                || !preferences
                    .hidden
                    .iter()
                    .any(|candidate| candidate == kind.label());
            visible
                && match selected_category {
                    Some("Favorites") => preferences
                        .favorites
                        .iter()
                        .any(|candidate| candidate == kind.label()),
                    Some(category) => kind.category() == category,
                    None => true,
                }
        })
        .collect::<Vec<_>>();
    kinds.sort_by_key(|kind| {
        let category_rank = categories
            .iter()
            .position(|category| *category == kind.category())
            .unwrap_or(categories.len());
        let item_fallback = InsertKind::ALL
            .iter()
            .position(|candidate| candidate == kind)
            .unwrap_or(usize::MAX / 2);
        (
            category_rank,
            preference_rank(&preferences.item_order, kind.label(), item_fallback),
        )
    });
    kinds
}

fn insert_kind_icon(kind: InsertKind) -> AssetIconName {
    match kind {
        InsertKind::Narration => AssetIconName::MessageSquareText,
        InsertKind::Dialogue => AssetIconName::User,
        InsertKind::Background => AssetIconName::Image,
        InsertKind::Figure => AssetIconName::PersonStanding,
        InsertKind::Choice | InsertKind::Conditional => AssetIconName::GitBranch,
        InsertKind::Loop => AssetIconName::Repeat2,
        InsertKind::Variable => AssetIconName::Braces,
        InsertKind::Goto | InsertKind::Call | InsertKind::Return => AssetIconName::Workflow,
        InsertKind::Wait => AssetIconName::Clock,
        InsertKind::Hide => AssetIconName::EyeOff,
        InsertKind::Move => AssetIconName::Move,
        InsertKind::Bgm => AssetIconName::Music,
        InsertKind::Effect => AssetIconName::Volume2,
        InsertKind::Video => AssetIconName::Film,
    }
}

fn render_block_picker(
    kinds: &[InsertKind],
    input: &Entity<InputState>,
    selected_index: usize,
    selected_category: Option<&'static str>,
    customize: bool,
    preferences: &BlockPickerPreferences,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let mut rows = Vec::new();
    let mut previous_category = None;
    for (index, kind) in kinds.iter().copied().enumerate() {
        let category = kind.category();
        if previous_category != Some(category) {
            rows.push(
                div()
                    .pt_2()
                    .px_2()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(rgb(MUTED))
                    .child(category.to_uppercase())
                    .into_any_element(),
            );
            previous_category = Some(category);
        }
        let icon = insert_kind_icon(kind);
        let favorite = preferences
            .favorites
            .iter()
            .any(|candidate| candidate == kind.label());
        let hidden = preferences
            .hidden
            .iter()
            .any(|candidate| candidate == kind.label());
        let mut row = div()
            .id(("block-picker-item", index))
            .h(px(34.))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .rounded(px(6.))
            .bg(rgb(if index == selected_index {
                SURFACE
            } else {
                PANEL
            }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.block_picker_open = false;
                this.insert_from_palette(kind, window, cx);
                this.rebuild_visual_editors(window, cx);
                this.focus.focus(window, cx);
                cx.notify();
            }))
            .child(
                Icon::new(icon)
                    .xsmall()
                    .text_color(rgb(if index == selected_index {
                        PRIMARY
                    } else {
                        MUTED
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(rgb(if hidden { MUTED } else { INK }))
                    .child(kind.label()),
            );
        if customize {
            row = row
                .child(
                    div()
                        .id(("picker-favorite", index))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .hover(|style| style.bg(rgb(SURFACE)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_picker_favorite(kind, cx);
                            cx.notify();
                        }))
                        .child(
                            Icon::new(if favorite {
                                AssetIconName::StarFill
                            } else {
                                AssetIconName::Star
                            })
                            .xsmall()
                            .text_color(rgb(if favorite {
                                PRIMARY
                            } else {
                                MUTED
                            })),
                        ),
                )
                .child(
                    div()
                        .id(("picker-up", index))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .hover(|style| style.bg(rgb(SURFACE)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.move_picker_item(kind, -1, cx);
                            cx.notify();
                        }))
                        .child(
                            Icon::new(AssetIconName::ArrowUp)
                                .xsmall()
                                .text_color(rgb(MUTED)),
                        ),
                )
                .child(
                    div()
                        .id(("picker-down", index))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .hover(|style| style.bg(rgb(SURFACE)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.move_picker_item(kind, 1, cx);
                            cx.notify();
                        }))
                        .child(
                            Icon::new(AssetIconName::ArrowDown)
                                .xsmall()
                                .text_color(rgb(MUTED)),
                        ),
                )
                .child(
                    div()
                        .id(("picker-visible", index))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .hover(|style| style.bg(rgb(SURFACE)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_picker_hidden(kind, cx);
                            cx.notify();
                        }))
                        .child(
                            Icon::new(if hidden {
                                AssetIconName::EyeOff
                            } else {
                                AssetIconName::Eye
                            })
                            .xsmall()
                            .text_color(rgb(if hidden {
                                MUTED
                            } else {
                                PRIMARY
                            })),
                        ),
                );
        } else if favorite {
            row = row.child(
                Icon::new(AssetIconName::StarFill)
                    .xsmall()
                    .text_color(rgb(PRIMARY)),
            );
        }
        rows.push(row.into_any_element());
    }
    let categories = std::iter::once(None)
        .chain(std::iter::once(Some("Favorites")))
        .chain(ordered_picker_categories(preferences).into_iter().map(Some))
        .collect::<Vec<_>>();
    div()
        .id("block-picker-overlay")
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .p_3()
        .flex()
        .bg(hsla(0., 0., 0.01, 0.84))
        .child(
            div()
                .id("block-picker")
                .key_context("KeineBlockPicker")
                .size_full()
                .min_w_0()
                .min_h_0()
                .flex()
                .flex_col()
                .rounded(px(10.))
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(PANEL))
                .overflow_hidden()
                .child(
                    div()
                        .h(px(44.))
                        .flex_none()
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .bg(rgb(CHROME))
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(input)
                                    .prefix(
                                        Icon::new(AssetIconName::Search)
                                            .xsmall()
                                            .text_color(rgb(MUTED)),
                                    )
                                    .appearance(false)
                                    .bordered(false)
                                    .size_full()
                                    .text_sm()
                                    .text_color(rgb(INK)),
                            ),
                        )
                        .child(
                            div()
                                .id("block-picker-customize")
                                .size(px(28.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(6.))
                                .bg(rgb(if customize { SURFACE } else { CHROME }))
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.block_picker_customize = !this.block_picker_customize;
                                    this.block_picker_index = 0;
                                    cx.notify();
                                }))
                                .child(
                                    Icon::new(AssetIconName::SlidersHorizontal)
                                        .xsmall()
                                        .text_color(rgb(if customize { PRIMARY } else { MUTED })),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(
                            div()
                                .w(px(112.))
                                .flex_none()
                                .p_2()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .bg(rgb(CHROME))
                                .children(categories.into_iter().enumerate().map(
                                    |(index, category)| {
                                        let selected = selected_category == category;
                                        let label = category.unwrap_or("All");
                                        let mut row = div()
                                            .id(("block-picker-category", index))
                                            .h(px(30.))
                                            .px_2()
                                            .flex()
                                            .items_center()
                                            .rounded(px(6.))
                                            .bg(rgb(if selected { SURFACE } else { CHROME }))
                                            .text_xs()
                                            .text_color(rgb(if selected { PRIMARY } else { MUTED }))
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.block_picker_category = category;
                                                this.block_picker_index = 0;
                                                cx.notify();
                                            }))
                                            .child(div().flex_1().child(label));
                                        if let Some(category) = category.filter(|category| {
                                            customize && *category != "Favorites"
                                        }) {
                                            row = row
                                                .child(
                                                    div()
                                                        .id(("picker-category-up", index))
                                                        .size(px(20.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                cx.stop_propagation();
                                                                this.move_picker_category(
                                                                    category, -1, cx,
                                                                );
                                                                cx.notify();
                                                            },
                                                        ))
                                                        .child(
                                                            Icon::new(AssetIconName::ChevronUp)
                                                                .xsmall(),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .id(("picker-category-down", index))
                                                        .size(px(20.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                cx.stop_propagation();
                                                                this.move_picker_category(
                                                                    category, 1, cx,
                                                                );
                                                                cx.notify();
                                                            },
                                                        ))
                                                        .child(
                                                            Icon::new(AssetIconName::ChevronDown)
                                                                .xsmall(),
                                                        ),
                                                );
                                        }
                                        row
                                    },
                                )),
                        )
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .p_2()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .children(rows)
                                .overflow_y_scrollbar()
                                .id("block-picker-results"),
                        ),
                ),
        )
        .into_any_element()
}

fn block_card_label(kind: &BlockKind, source: &str) -> String {
    match kind {
        BlockKind::Dialogue { speaker } => speaker.clone(),
        BlockKind::Command | BlockKind::Control => source
            .split(|character: char| character == '(' || character.is_whitespace())
            .next()
            .filter(|value| !value.is_empty())
            .map(title_case)
            .unwrap_or_else(|| kind.label().to_owned()),
        _ => kind.label().to_owned(),
    }
}

fn block_card_summary(kind: &BlockKind, source: &str, line: usize) -> String {
    match kind {
        BlockKind::Narration | BlockKind::Dialogue { .. } => {
            format!("Dynamic text · L{}", line + 1)
        }
        BlockKind::Choice
        | BlockKind::Conditional
        | BlockKind::ElseIf
        | BlockKind::Else
        | BlockKind::Loop
        | BlockKind::Control => String::new(),
        BlockKind::ChoiceOption => first_quoted_text(source).unwrap_or_default(),
        BlockKind::Declaration => source
            .strip_prefix("let ")
            .and_then(|tail| tail.split_whitespace().next())
            .unwrap_or_default()
            .to_owned(),
        BlockKind::Assignment => source
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned(),
        BlockKind::Command => source
            .split_once('(')
            .and_then(|(_, tail)| tail.split([',', ')']).next())
            .map(str::trim)
            .unwrap_or_default()
            .to_owned(),
        BlockKind::Unsupported => format!("Unsupported syntax · L{}", line + 1),
    }
}

fn first_quoted_text(source: &str) -> Option<String> {
    let start = source.find('"')? + 1;
    let mut escaped = false;
    for (offset, character) in source[start..].char_indices() {
        if character == '"' && !escaped {
            return Some(source[start..start + offset].to_owned());
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    None
}

fn title_case(value: &str) -> String {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    first.to_uppercase().chain(characters).collect()
}

fn bottom_overflow_fade(visible: impl Fn(&App) -> bool + 'static) -> impl IntoElement {
    canvas(
        move |_, _, cx| visible(cx),
        |bounds, visible, window, _| {
            if visible {
                window.paint_quad(fill(
                    bounds,
                    linear_gradient(
                        180.,
                        linear_color_stop(hsla(0., 0., 0.01, 0.), 0.),
                        linear_color_stop(hsla(0., 0., 0.01, 0.22), 1.),
                    ),
                ));
            }
        },
    )
    .absolute()
    .left_0()
    .right_0()
    .bottom_0()
    .h(px(28.))
}

fn vertical_overflow_view<E>(id: &'static str, handle: &ScrollHandle, content: E) -> AnyElement
where
    E: InteractiveElement + Styled + ParentElement + Element + 'static,
{
    let fade_handle = handle.clone();
    let area = div()
        .id(format!("{id}-area"))
        .size_full()
        .min_h_0()
        .track_scroll(handle)
        .overflow_y_scroll()
        .lock_scroll_axis()
        .child(content.w_full().h_auto().min_h_full().flex_none());
    div()
        .id(id)
        .relative()
        .size_full()
        .min_h_0()
        .overflow_hidden()
        .child(area)
        .child(bottom_overflow_fade(move |_| {
            let max = fade_handle.max_offset().y;
            max > px(1.) && max + fade_handle.offset().y > px(1.)
        }))
        .vertical_scrollbar(handle)
        .into_any_element()
}

struct BlockProjectionView<'a> {
    root: &'a Path,
    relative: &'a Path,
    document: &'a DocumentHandle,
    editors: &'a [BlockTextEditor],
    collapsed_scenes: &'a HashSet<String>,
    selected_blocks: &'a HashSet<usize>,
    draft_text: Option<&'a DraftTextBlock>,
    drop_target: Option<usize>,
    scroll_handle: &'a ScrollHandle,
    scroll_anchor: &'a ScrollAnchor,
    scroll_pending: bool,
}

fn draft_text_row(draft: &DraftTextBlock, indent: f32, id: usize) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .pl(px(indent))
        .child(
            div()
                .id(("draft-text-block", id))
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .min_h(px(38.))
                .px_2()
                .rounded(px(7.))
                .bg(rgb(SURFACE))
                .child(
                    Icon::new(AssetIconName::MessageSquarePlus)
                        .xsmall()
                        .text_color(rgb(PRIMARY)),
                )
                .child(
                    div()
                        .w(px(64.))
                        .flex_none()
                        .text_xs()
                        .text_color(rgb(PRIMARY))
                        .child("Text"),
                )
                .child(
                    div().h(px(30.)).flex_1().min_w_0().child(
                        Input::new(&draft.state)
                            .appearance(false)
                            .bordered(false)
                            .size_full()
                            .text_sm()
                            .text_color(rgb(INK)),
                    ),
                ),
        )
        .into_any_element()
}

fn render_block_projection(
    view: BlockProjectionView<'_>,
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let BlockProjectionView {
        root,
        relative,
        document,
        editors,
        collapsed_scenes,
        selected_blocks,
        draft_text,
        drop_target,
        scroll_handle,
        scroll_anchor,
        scroll_pending,
    } = view;
    let source = document.borrow().contents().to_owned();
    let projection = EiyashouProjection::parse(&source);
    let block_order = Arc::new(
        projection
            .scenes
            .iter()
            .flat_map(|scene| scene.blocks.iter().map(|block| block.source_range.start))
            .collect::<Vec<_>>(),
    );
    let selected_position = cx
        .global::<EditorDocuments>()
        .selection(root)
        .filter(|(path, _, _)| path == relative)
        .map(|(_, line, column)| (*line, *column));
    let selected_line = selected_position.map(|(line, _)| line);
    let selected_start = selected_position.and_then(|(line, column)| {
        projected_block_at(&source, line, column).map(|(_, block)| block.source_range.start)
    });
    let root = root.to_owned();
    let relative = relative.to_owned();
    let mut rows = Vec::new();
    for (scene_index, scene) in projection.scenes.into_iter().enumerate() {
        let collapsed = collapsed_scenes.contains(&scene.name);
        let scene_name = scene.name.clone();
        let scene_root = root.clone();
        let scene_relative = relative.clone();
        let collapse_progress = transition(
            (
                format!("block-scene-{}-{scene_index}", relative.display()),
                "collapse",
            ),
            if collapsed { 0. } else { 1. },
            Transition::new(Duration::from_millis(120)),
            window,
            cx,
        );
        let scene_line = document
            .borrow()
            .contents()
            .get(..scene.name_range.start)
            .map(|prefix| prefix.bytes().filter(|byte| *byte == b'\n').count())
            .unwrap_or_default();
        let block_count = scene.blocks.len();
        let header = div()
            .id(("scene-section", scene_index))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .h(px(32.))
            .px_2()
            .rounded(px(7.))
            .bg(rgb(if selected_line == Some(scene_line) {
                SURFACE
            } else {
                PANEL
            }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if !this.collapsed_scenes.remove(&scene_name) {
                    this.collapsed_scenes.insert(scene_name.clone());
                }
                this.selected_blocks.clear();
                this.block_selection_anchor = None;
                cx.global_mut::<EditorDocuments>()
                    .clear_block_selection(&scene_root);
                set_authoring_selection(&scene_root, scene_relative.clone(), scene_line, 0, cx);
                cx.notify();
            }))
            .child(
                Icon::new(IconName::ChevronRight)
                    .xsmall()
                    .rotate(radians(collapse_progress * std::f32::consts::FRAC_PI_2))
                    .text_color(rgb(MUTED)),
            )
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(rgb(INK))
                    .child(scene.name),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(block_count.to_string()),
            )
            .into_any_element();
        let mut scene_rows = Vec::new();
        let mut scene_body_height = 0.;
        for (block_index, block) in scene.blocks.into_iter().enumerate() {
            let row_id = block.source_range.start;
            let line = block.line;
            let column = block.column;
            let root = root.clone();
            let relative = relative.clone();
            let source_root = root.clone();
            let source_relative = relative.clone();
            let selected = selected_blocks.contains(&row_id) || selected_start == Some(row_id);
            let icon = match &block.kind {
                BlockKind::Narration | BlockKind::Dialogue { .. } => {
                    AssetIconName::MessageSquareText
                }
                BlockKind::Choice
                | BlockKind::ChoiceOption
                | BlockKind::Conditional
                | BlockKind::ElseIf => AssetIconName::GitBranch,
                BlockKind::Else => AssetIconName::Workflow,
                BlockKind::Loop => AssetIconName::Repeat2,
                BlockKind::Declaration | BlockKind::Assignment => AssetIconName::Braces,
                BlockKind::Command | BlockKind::Control => AssetIconName::Play,
                BlockKind::Unsupported => AssetIconName::TriangleAlert,
            };
            let label = block_card_label(&block.kind, &block.summary);
            let text_state = block.text_range.as_ref().and_then(|range| {
                editors
                    .iter()
                    .find(|editor| editor.text_start == range.start)
                    .map(|editor| &editor.state)
                    .or_else(|| {
                        draft_text
                            .filter(|draft| {
                                draft
                                    .text_range
                                    .as_ref()
                                    .is_some_and(|draft_range| draft_range.start == range.start)
                            })
                            .map(|draft| &draft.state)
                    })
            });
            let is_text = matches!(
                &block.kind,
                BlockKind::Narration | BlockKind::Dialogue { .. }
            );
            let is_structure = matches!(
                &block.kind,
                BlockKind::Choice
                    | BlockKind::ChoiceOption
                    | BlockKind::Conditional
                    | BlockKind::ElseIf
                    | BlockKind::Else
                    | BlockKind::Loop
            );
            let source_summary = block_card_summary(&block.kind, &block.summary, block.line);
            let order = block_order.clone();
            let drag_selection = if selected_blocks.contains(&row_id) {
                selected_blocks.clone()
            } else {
                HashSet::from([row_id])
            };
            let movable = !matches!(&block.kind, BlockKind::ElseIf | BlockKind::Else);
            let grip = if movable {
                div()
                    .id(("block-grip", row_id))
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_move()
                    .on_drag(
                        BlockDrag {
                            selected: drag_selection,
                        },
                        |_, _, _, cx| cx.new(|_| Empty),
                    )
                    .child(
                        Icon::new(AssetIconName::GripVertical)
                            .xsmall()
                            .text_color(rgb(0x686e75)),
                    )
                    .into_any_element()
            } else {
                div().size(px(18.)).into_any_element()
            };
            let row_height = if is_text {
                38.
            } else if is_structure {
                28.
            } else {
                32.
            };
            scene_body_height += row_height + 4.;
            let drop_line_opacity = transition(
                (format!("block-drop-line-{row_id}"), "opacity"),
                f32::from(drop_target == Some(row_id) && cx.has_active_drag()),
                Transition::new(Duration::from_millis(90)),
                window,
                cx,
            );
            let block_indent = 8. + block.depth as f32 * 18.;
            let mut row = div()
                .id(("block-row", scene_index * 10_000 + block_index))
                .relative()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .min_h(px(row_height))
                .px_2()
                .rounded(px(7.))
                .bg(rgb(if selected {
                    SURFACE
                } else if is_structure {
                    CANVAS
                } else {
                    PANEL
                }))
                .cursor_pointer()
                .anchor_scroll((selected_start == Some(row_id)).then(|| scroll_anchor.clone()))
                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    let modifiers = event.modifiers();
                    if modifiers.shift {
                        let anchor = this.block_selection_anchor.unwrap_or(row_id);
                        if let (Some(anchor_index), Some(row_index)) = (
                            order.iter().position(|candidate| *candidate == anchor),
                            order.iter().position(|candidate| *candidate == row_id),
                        ) {
                            let start = anchor_index.min(row_index);
                            let end = anchor_index.max(row_index);
                            this.selected_blocks.clear();
                            this.selected_blocks
                                .extend(order[start..=end].iter().copied());
                        }
                    } else if modifiers.platform || modifiers.control {
                        if !this.selected_blocks.remove(&row_id) {
                            this.selected_blocks.insert(row_id);
                        }
                        this.block_selection_anchor = Some(row_id);
                    } else {
                        this.selected_blocks.clear();
                        this.selected_blocks.insert(row_id);
                        this.block_selection_anchor = Some(row_id);
                    }
                    cx.global_mut::<EditorDocuments>().set_block_selection(
                        &root,
                        relative.clone(),
                        this.selected_blocks.iter().copied().collect(),
                    );
                    set_authoring_selection(&root, relative.clone(), line, column, cx);
                    cx.notify();
                }))
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if cx.has_active_drag() && this.block_drop_target != Some(row_id) {
                        this.block_drop_target = Some(row_id);
                        cx.notify();
                    }
                }))
                .on_drop(cx.listener(move |this, drag: &BlockDrag, window, cx| {
                    cx.stop_propagation();
                    this.block_drop_target = None;
                    this.drop_blocks(drag, row_id, window, cx);
                }))
                .child(
                    div()
                        .absolute()
                        .top(px(-1.))
                        .left(px(8.))
                        .right(px(8.))
                        .h(px(2.))
                        .rounded_full()
                        .bg(rgb(PRIMARY))
                        .opacity(drop_line_opacity),
                )
                .child(grip)
                .child(Icon::new(icon).xsmall().text_color(rgb(if block.read_only {
                    0xd2aa62
                } else {
                    MUTED
                })))
                .child(
                    div()
                        .w(px(64.))
                        .flex_shrink_1()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_xs()
                        .text_color(rgb(if is_text { PRIMARY } else { MUTED }))
                        .child(label),
                );
            row = if let Some(state) = text_state.filter(|_| !block.read_only) {
                row.child(
                    div().h(px(30.)).flex_1().min_w_0().child(
                        Input::new(state)
                            .appearance(false)
                            .bordered(false)
                            .size_full()
                            .text_sm()
                            .text_color(rgb(INK)),
                    ),
                )
            } else {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_sm()
                        .text_color(rgb(if block.read_only { 0xd2aa62 } else { INK }))
                        .child(source_summary),
                )
            };
            if block.read_only {
                row = row.child(
                    div()
                        .id(("open-source", row_id))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .hover(|style| style.bg(rgb(SURFACE)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.document_mode = DocumentMode::Text;
                            navigate_source(
                                &source_root,
                                &source_relative,
                                line + 1,
                                column + 1,
                                window,
                                cx,
                            );
                            cx.notify();
                        }))
                        .child(Icon::new(IconName::FileText).xsmall()),
                );
            }
            scene_rows.push(
                div()
                    .w_full()
                    .min_w_0()
                    .pl(px(block_indent))
                    .child(row)
                    .into_any_element(),
            );
            if let Some(draft) = draft_text.filter(|draft| {
                matches!(draft.target, DraftInsertionTarget::After(start) if start == row_id)
                    && draft.text_range.is_none()
            }) {
                scene_body_height += 42.;
                scene_rows.push(draft_text_row(draft, block_indent, row_id));
            }
        }
        if let Some(draft) = draft_text.filter(|draft| {
            matches!(
                draft.target,
                DraftInsertionTarget::SceneEnd(start) if start == scene.source_range.start
            ) && draft.text_range.is_none()
        }) {
            scene_body_height += 42.;
            scene_rows.push(draft_text_row(draft, 8., scene.source_range.start));
        }
        scene_body_height = (scene_body_height - 4.).max(0.);
        rows.push(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .child(header)
                .child(
                    div()
                        .w_full()
                        .h(px(scene_body_height * collapse_progress))
                        .opacity(collapse_progress)
                        .overflow_hidden()
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .children(scene_rows),
                        ),
                )
                .into_any_element(),
        );
    }
    rows.extend(
        projection
            .read_only
            .into_iter()
            .enumerate()
            .map(|(index, card)| {
                div()
                    .id(("projection-diagnostic", index))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_h(px(30.))
                    .px_2()
                    .rounded(px(7.))
                    .bg(rgb(SURFACE))
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .xsmall()
                            .text_color(rgb(0xd2aa62)),
                    )
                    .child(div().text_xs().text_color(rgb(MUTED)).child(card.message))
                    .into_any_element()
            }),
    );
    if scroll_pending && selected_start.is_some() {
        scroll_anchor.scroll_to(window, cx);
    }
    let content = div()
        .id("eiyashou-block-content")
        .relative()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .pr_4()
        .children(rows)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.selected_blocks.clear();
            this.block_selection_anchor = None;
            cx.global_mut::<EditorDocuments>()
                .clear_block_selection(&root);
            cx.notify();
        }))
        .key_context("KeineBlockView");
    vertical_overflow_view("eiyashou-block-scroll", scroll_handle, content)
}

fn render_assets(root: &Path, index: &AuthoringIndex, scroll_handle: &ScrollHandle) -> AnyElement {
    let root = root.to_owned();
    let content = div()
        .flex()
        .flex_col()
        .p_2()
        .gap_1()
        .child(section_label("ASSET BROWSER"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(index.assets.iter().enumerate().map(|(row, asset)| {
                    let root = root.clone();
                    let path = index
                        .assets_manifest
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("assets.yaml"));
                    div()
                        .id(("asset-row", row))
                        .flex()
                        .items_center()
                        .gap_2()
                        .p_2()
                        .rounded(px(7.))
                        .bg(rgb(PANEL))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                        .on_click(move |_, window, cx| {
                            navigate_source(&root, &path, 1, 1, window, cx)
                        })
                        .child(
                            div()
                                .w(px(72.))
                                .flex_none()
                                .text_xs()
                                .text_color(rgb(PRIMARY))
                                .child(asset.kind.label()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(div().text_sm().text_color(rgb(INK)).child(asset.id.clone()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(if asset.exists {
                                            MUTED
                                        } else {
                                            0xdb7780
                                        }))
                                        .child(asset.path.display().to_string()),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(MUTED))
                                .child(format!("{} refs", asset.reference_count)),
                        )
                })),
        );
    vertical_overflow_view("asset-scroll", scroll_handle, content)
}

fn render_problems(
    root: &Path,
    scroll_handle: &ScrollHandle,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let index = cx.global::<EditorDocuments>().authoring(root);
    let runtime = cx.global::<EditorDocuments>().runtime_diagnostics(root);
    let root = root.to_owned();
    let authoring_rows = index.problems.into_iter().enumerate().map({
        let root = root.clone();
        move |(row, problem)| {
            let root = root.clone();
            let path = problem.path.clone();
            let line = problem.line;
            let column = problem.column;
            problem_row(
                ("authoring-problem", row),
                match problem.severity {
                    ProblemSeverity::Warning => 0xd2aa62,
                    ProblemSeverity::Error => 0xdb7780,
                },
                problem.path.display().to_string(),
                problem.line,
                problem.column,
                problem.message,
            )
            .on_click(move |_, window, cx| navigate_source(&root, &path, line, column, window, cx))
        }
    });
    let runtime_rows = runtime.into_iter().enumerate().map({
        let root = root.clone();
        move |(row, diagnostic)| {
            let root = root.clone();
            let path = diagnostic.path.clone();
            let line = diagnostic.line;
            let column = diagnostic.column;
            problem_row(
                ("runtime-problem", row),
                match diagnostic.level {
                    keine_authoring::DiagnosticLevel::Warning => 0xd2aa62,
                    keine_authoring::DiagnosticLevel::Error => 0xdb7780,
                },
                diagnostic.path.display().to_string(),
                diagnostic.line,
                diagnostic.column,
                diagnostic.message,
            )
            .on_click(move |_, window, cx| navigate_source(&root, &path, line, column, window, cx))
        }
    });
    let content = div()
        .flex()
        .flex_col()
        .p_2()
        .gap_1()
        .child(section_label("PARSE · VALIDATION · RUNTIME"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(authoring_rows)
                .children(runtime_rows),
        );
    vertical_overflow_view("problem-scroll", scroll_handle, content)
}

fn problem_row(
    id: (&'static str, usize),
    color: u32,
    path: String,
    line: usize,
    column: usize,
    message: String,
) -> Stateful<Div> {
    div()
        .id(id)
        .p_2()
        .rounded(px(7.))
        .bg(rgb(PANEL))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .child(div().text_xs().text_color(rgb(color)).child(message))
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(format!("{path}:{line}:{column}")),
        )
}

fn render_performance(
    controller: &PreviewController,
    timeline: &VecDeque<TimelineSample>,
    scroll_handle: &ScrollHandle,
) -> AnyElement {
    let snapshot = controller.snapshot();
    let latest = timeline.back().copied().unwrap_or(TimelineSample {
        published: snapshot.frame_stats.published,
        overwritten: snapshot.frame_stats.overwritten,
    });
    let deltas = timeline
        .iter()
        .zip(timeline.iter().skip(1))
        .map(|(before, after)| after.published.saturating_sub(before.published))
        .collect::<Vec<_>>();
    let peak = deltas.iter().copied().max().unwrap_or(1).max(1);
    let content = div()
        .flex()
        .flex_col()
        .p_3()
        .gap_3()
        .child(section_label("PREVIEW TRANSPORT"))
        .child(property_row(
            "Published frames",
            latest.published.to_string(),
        ))
        .child(property_row(
            "Overwritten frames",
            latest.overwritten.to_string(),
        ))
        .child(property_row("Samples", timeline.len().to_string()))
        .child(
            div()
                .h(px(84.))
                .flex()
                .items_end()
                .gap(px(2.))
                .px_1()
                .rounded(px(8.))
                .bg(rgb(CANVAS))
                .children(deltas.into_iter().map(|delta| {
                    let height = 4. + (delta as f32 / peak as f32) * 68.;
                    div()
                        .w(px(5.))
                        .h(px(height))
                        .rounded(px(2.))
                        .bg(rgb(PRIMARY))
                })),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child("500 ms transport samples · not CPU/GPU frame time"),
        );
    vertical_overflow_view("performance-scroll", scroll_handle, content)
}

fn set_authoring_selection(
    root: &Path,
    relative: PathBuf,
    line: usize,
    column: usize,
    cx: &mut App,
) {
    cx.global_mut::<EditorDocuments>()
        .set_selection(root, relative.clone(), line, column);
    if let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(root) {
        preview.set_cursor(relative, line + 1, column + 1);
    }
    cx.refresh_windows();
}

fn apply_workspace_edit(
    root: &Path,
    relative: &Path,
    edited: String,
    window: &mut Window,
    cx: &mut App,
) {
    open_workspace_document(root, relative, window, cx);
    if let Some(editor) = cx.global::<EditorDocuments>().editor_for(root, relative) {
        let _ = editor.update(cx, |editor, cx| editor.replace_all(edited, window, cx));
    }
}

fn navigate_source(
    root: &Path,
    relative: &Path,
    line: usize,
    column: usize,
    window: &mut Window,
    cx: &mut App,
) {
    open_workspace_document(root, relative, window, cx);
    if let Some(editor) = cx.global::<EditorDocuments>().editor_for(root, relative) {
        let _ = editor.update(cx, |editor, cx| {
            editor.set_cursor_position(
                Position::new(
                    line.saturating_sub(1) as u32,
                    column.saturating_sub(1) as u32,
                ),
                window,
                cx,
            );
        });
    }
}

fn projected_block_at(
    source: &str,
    line: usize,
    column: usize,
) -> Option<(String, crate::projection::BlockCard)> {
    let line_start = source
        .split_inclusive('\n')
        .take(line)
        .map(str::len)
        .sum::<usize>();
    let offset = (line_start + column).min(source.len());
    let projection = EiyashouProjection::parse(source);
    projection.scenes.into_iter().find_map(|scene| {
        if !scene.source_range.contains(&offset) && offset != scene.source_range.end {
            return None;
        }
        scene
            .blocks
            .iter()
            .filter(|block| block.source_range.start <= offset && block.source_range.end >= offset)
            .max_by_key(|block| block.depth)
            .cloned()
            .map(|block| (scene.name, block))
    })
}

fn parenthesized_value(source: &str) -> Option<String> {
    let start = source.find('(')? + 1;
    let end = source.rfind(')')?;
    (end >= start).then(|| source[start..end].trim().trim_matches('"').to_owned())
}

fn text_voice(source: &str) -> Option<String> {
    let quote = source.rfind('"')?;
    source[quote + 1..]
        .strip_prefix(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn common_value(values: impl IntoIterator<Item = String>) -> String {
    let mut values = values.into_iter();
    let Some(first) = values.next() else {
        return "—".to_owned();
    };
    if values.all(|value| value == first) {
        first
    } else {
        "Mixed".to_owned()
    }
}

fn multi_block_summary(source: &str, starts: &[usize]) -> Option<AnyElement> {
    let selected = EiyashouProjection::parse(source)
        .scenes
        .into_iter()
        .flat_map(|scene| {
            let name = scene.name;
            scene
                .blocks
                .into_iter()
                .filter(|block| starts.contains(&block.source_range.start))
                .map(move |block| (name.clone(), block))
        })
        .collect::<Vec<_>>();
    if selected.len() < 2 {
        return None;
    }
    let mut properties = vec![
        ("Blocks", selected.len().to_string()),
        (
            "Type",
            common_value(
                selected
                    .iter()
                    .map(|(_, block)| block.kind.label().to_owned()),
            ),
        ),
        (
            "Scene",
            common_value(selected.iter().map(|(scene, _)| scene.clone())),
        ),
    ];
    let all_text = selected.iter().all(|(_, block)| {
        matches!(
            block.kind,
            BlockKind::Narration | BlockKind::Dialogue { .. }
        )
    });
    if all_text {
        properties.push((
            "Speaker",
            common_value(selected.iter().map(|(_, block)| match &block.kind {
                BlockKind::Narration => "Narrator".to_owned(),
                BlockKind::Dialogue { speaker } => speaker.clone(),
                _ => unreachable!("guarded above"),
            })),
        ));
        properties.push((
            "Voice",
            common_value(
                selected.iter().map(|(_, block)| {
                    text_voice(&block.summary).unwrap_or_else(|| "None".to_owned())
                }),
            ),
        ));
    }
    properties.push((
        "Stable ID",
        common_value(
            selected
                .iter()
                .map(|(_, block)| block.stable_id.clone().unwrap_or_else(|| "None".to_owned())),
        ),
    ));
    Some(
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(
                properties
                    .into_iter()
                    .map(|(label, value)| property_row(label, value)),
            )
            .into_any_element(),
    )
}

fn selection_summary(root: &Path, index: &AuthoringIndex, cx: &App) -> AnyElement {
    match cx.global::<EditorDocuments>().selection(root) {
        Some((path, line, column)) => {
            let diagnostics = cx
                .global::<EditorDocuments>()
                .diagnostics_for(root, path)
                .take(3)
                .map(|diagnostic| {
                    let color = match diagnostic.level {
                        keine_authoring::DiagnosticLevel::Warning => 0xd2aa62,
                        keine_authoring::DiagnosticLevel::Error => 0xdb7780,
                    };
                    div().text_color(rgb(color)).child(format!(
                        "{}:{}  {}",
                        diagnostic.line, diagnostic.column, diagnostic.message
                    ))
                })
                .collect::<Vec<_>>();
            if let Some((selected_path, starts)) =
                cx.global::<EditorDocuments>().block_selection(root)
                && selected_path == path
                && let Some(source) = cx.global::<EditorDocuments>().source(root, path)
                && let Some(summary) = multi_block_summary(&source, starts)
            {
                return div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(path.display().to_string()),
                    )
                    .child(summary)
                    .children(diagnostics)
                    .into_any_element();
            }
            if path.extension().is_some_and(|value| value == "shou")
                && let Some(source) = cx.global::<EditorDocuments>().source(root, path)
                && let Some((scene, block)) = projected_block_at(&source, *line, *column)
            {
                let mut properties = vec![
                    ("Type", block_card_label(&block.kind, &block.summary)),
                    ("Scene", scene),
                ];
                match &block.kind {
                    BlockKind::Narration | BlockKind::Dialogue { .. } => {}
                    BlockKind::Choice => {
                        if let Some(prompt) = parenthesized_value(&block.summary) {
                            properties.push(("Prompt", prompt));
                        }
                    }
                    BlockKind::ChoiceOption => {
                        if let Some(option) = first_quoted_text(&block.summary) {
                            properties.push(("Option", option));
                        }
                    }
                    BlockKind::Conditional | BlockKind::ElseIf => {
                        if let Some(condition) = parenthesized_value(&block.summary) {
                            properties.push(("Condition", condition));
                        }
                    }
                    BlockKind::Declaration | BlockKind::Assignment => {
                        if let Some((_, expression)) = block.summary.split_once('=') {
                            properties.push(("Value", expression.trim().to_owned()));
                        }
                    }
                    BlockKind::Command => {
                        if let Some(arguments) = parenthesized_value(&block.summary) {
                            properties.push(("Parameters", arguments));
                        }
                    }
                    BlockKind::Else
                    | BlockKind::Loop
                    | BlockKind::Control
                    | BlockKind::Unsupported => {}
                }
                if !matches!(
                    &block.kind,
                    BlockKind::Narration | BlockKind::Dialogue { .. }
                ) && let Some(stable_id) = &block.stable_id
                {
                    properties.push(("Stable ID", stable_id.clone()));
                }
                return div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(path.display().to_string()),
                    )
                    .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                        "Line {}, column {}",
                        line + 1,
                        column + 1
                    )))
                    .children(
                        properties
                            .into_iter()
                            .map(|(label, value)| property_row(label, value)),
                    )
                    .children(diagnostics)
                    .into_any_element();
            }
            let authoring = match index.selection(path, *line) {
                AuthoringSelection::Source => "Source selection".to_owned(),
                AuthoringSelection::Scene(scene) => format!("Scene · {}", scene.name),
                AuthoringSelection::Dialogue(dialogue) => format!(
                    "{} · {}",
                    if dialogue.speaker.is_empty() {
                        "Narration"
                    } else {
                        dialogue.speaker.as_str()
                    },
                    dialogue.text
                ),
            };
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_xs()
                .text_color(rgb(0xb8c4cf))
                .child(path.display().to_string())
                .child(format!("Line {}, column {}", line + 1, column + 1))
                .child(div().text_color(rgb(PRIMARY)).child(authoring))
                .children(diagnostics)
                .into_any_element()
        }
        None => div()
            .text_xs()
            .text_color(rgb(MUTED))
            .child("No source selection")
            .into_any_element(),
    }
}

fn language_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("shou") => "eiyashou",
        Some("json") => "json",
        Some("yaml" | "yml") => "yaml",
        _ => "plaintext",
    }
}

fn register_workbench_panels(cx: &mut App) {
    for name in [
        EXPLORER_PANEL,
        DOCUMENT_PANEL,
        INSPECTOR_PANEL,
        OUTPUT_PANEL,
        PREVIEW_PANEL,
        ASSETS_PANEL,
        CHARACTERS_PANEL,
        SCENES_PANEL,
        PROBLEMS_PANEL,
        PERFORMANCE_PANEL,
    ] {
        register_panel(cx, name, |context, window, cx| {
            let panel = workbench_panel(context, window, cx).unwrap_or_else(|error| {
                WorkbenchPanel::from_payload(
                    PanelPayload::Output {
                        root: PathBuf::new(),
                    },
                    window,
                    cx,
                )
                .unwrap_or_else(|_| panic!("could not construct fallback panel: {error}"))
            });
            panel_handle(panel)
        });
    }
}

fn workbench_panel(
    context: PanelBuildContext<'_>,
    window: &mut Window,
    cx: &mut App,
) -> io::Result<Entity<WorkbenchPanel>> {
    match context.info() {
        PanelInfo::Panel(value) => serde_json::from_value::<PanelPayload>(value.clone())
            .map_err(io::Error::other)
            .and_then(|payload| WorkbenchPanel::from_payload(payload, window, cx)),
        _ => WorkbenchPanel::from_payload(
            PanelPayload::Output {
                root: PathBuf::new(),
            },
            window,
            cx,
        ),
    }
}

struct ProjectWorkspace {
    session: WorkspaceSession,
    dock: Entity<DockArea>,
    _layout_subscription: Subscription,
}

/// Keep gpui-component's dock behavior and visuals, changing only the tab bar
/// so every tab has the compact close affordance expected by an editor.
struct EditorDockSkin {
    inner: Rc<DockSkin>,
    root: PathBuf,
    dock: Rc<RefCell<Option<WeakEntity<DockArea>>>>,
    drop_overlays: Rc<RefCell<HashMap<NodeId, Entity<EditorDropOverlay>>>>,
    tab_motion: Entity<EditorTabMotion>,
}

impl DockAreaRenderer for EditorDockSkin {
    fn frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        DockAreaRenderer::frame(self.inner.as_ref(), window, cx)
    }

    fn center_frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        DockAreaRenderer::center_frame(self.inner.as_ref(), window, cx)
    }

    fn split_frame(&self, node: NodeId, _: Axis, _: &mut Window, _: &mut App) -> Stateful<Div> {
        // The split still owns its resize hit area, but tonal separation behind
        // rounded views replaces the old stack of square panel outlines.
        div()
            .id(("editor-dock-split", node.as_u64()))
            .bg(rgb(CANVAS))
    }

    fn render_split_handle(
        &self,
        _: &ResizeHandleContext,
        _: &mut Window,
        _: &mut App,
    ) -> Option<AnyElement> {
        // Resizing keeps the upstream hit target and cursor. Card spacing and
        // tone already show the boundary, so do not paint a divider.
        Some(Empty.into_any_element())
    }

    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        DockAreaRenderer::render_dock(self.inner.as_ref(), dock, content, window, cx)
    }

    fn build_placeholder(
        &self,
        state: &PanelState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Arc<dyn BasePanelView>> {
        DockAreaRenderer::build_placeholder(self.inner.as_ref(), state, window, cx)
    }

    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(EditorTabGroupSkin {
            inner: DockAreaRenderer::tab_group_renderer(self.inner.as_ref()),
            root: self.root.clone(),
            dock: self.dock.clone(),
            drop_overlays: self.drop_overlays.clone(),
            tab_motion: self.tab_motion.clone(),
        })
    }
}

struct EditorTabGroupSkin {
    inner: Rc<dyn TabGroupRenderer>,
    root: PathBuf,
    dock: Rc<RefCell<Option<WeakEntity<DockArea>>>>,
    drop_overlays: Rc<RefCell<HashMap<NodeId, Entity<EditorDropOverlay>>>>,
    tab_motion: Entity<EditorTabMotion>,
}

impl EditorTabGroupSkin {
    fn drop_overlay(&self, node: NodeId, cx: &mut App) -> Entity<EditorDropOverlay> {
        if let Some(overlay) = self.drop_overlays.borrow().get(&node) {
            return overlay.clone();
        }
        let overlay = cx.new(|_| EditorDropOverlay {
            node,
            epoch: 0,
            target: None,
        });
        self.drop_overlays
            .borrow_mut()
            .insert(node, overlay.clone());
        overlay
    }

    fn singleton_title_bar(&self, group: &TabGroupContext, cx: &mut App) -> Option<AnyElement> {
        let [panel] = group.panels() else {
            return None;
        };
        let panel_name = panel.panel_name(cx);
        if panel_name == DOCUMENT_PANEL {
            return None;
        }
        let is_closable_tool = matches!(
            panel_name,
            PREVIEW_PANEL
                | ASSETS_PANEL
                | CHARACTERS_PANEL
                | SCENES_PANEL
                | PROBLEMS_PANEL
                | PERFORMANCE_PANEL
        );
        let panel_id = panel.panel_id(cx);
        let close_group = group.clone();

        let title = PanelHandle::of(panel)
            .and_then(|handle| handle.tab_name(cx))
            .unwrap_or_else(|| panel.panel_name(cx).into());
        let drag_title = title.clone();
        let drag = group
            .drag_panel(0, cx)
            .map(|panel| EditorPanelDrag { panel });
        let drop_overlays = self.drop_overlays.clone();
        let node = group.node();

        let title = div()
            .id(("editor-view-title-drag", node.as_u64()))
            .h_full()
            .flex_1()
            .flex()
            .items_center()
            .pr_2()
            .child(title)
            .when_some(drag, |this, drag| {
                this.on_drag(drag, move |drag, offset, _, cx| {
                    cx.stop_propagation();
                    clear_all_drop_overlays(&drop_overlays, cx);
                    drag.panel.set_drag_offset(offset);
                    drag.panel.set_preview_size(size(px(180.), px(30.)));
                    cx.new(|_| TabDragPreview {
                        title: drag_title.clone(),
                    })
                })
            });

        Some(
            div()
                .id(("editor-view-title", node.as_u64()))
                .h(px(36.))
                .flex()
                .items_center()
                .px_3()
                .rounded_t(px(VIEW_RADIUS_PX))
                .bg(rgb(CHROME))
                .text_sm()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(rgb(0xb8c2cc))
                .child(title)
                .when(is_closable_tool, |this| {
                    this.child(
                        div()
                            .id(("close-tool-view", node.as_u64()))
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .text_color(rgb(MUTED))
                            .hover(|style| style.bg(rgb(SURFACE_HOVER)).text_color(rgb(INK)))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                close_group.close(panel_id, window, cx);
                            })
                            .child(Icon::new(IconName::Close).xsmall()),
                    )
                })
                .into_any_element(),
        )
    }
}

#[derive(Clone)]
struct EditorPanelDrag {
    panel: DragPanel,
}

#[derive(Default)]
struct EditorTabMotion {
    closing: HashSet<u64>,
}

impl EditorTabMotion {
    fn close(
        &mut self,
        panel_id: gpui_kit::component::dock::PanelId,
        group: TabGroupContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = panel_id.as_u64();
        if !self.closing.insert(key) {
            return;
        }
        let delay = if cx.reduce_motion() {
            Duration::ZERO
        } else {
            TAB_MOTION_DURATION
        };
        cx.notify();
        cx.refresh_windows();
        cx.spawn_in(window, async move |motion, cx| {
            cx.background_executor().timer(delay).await;
            let _ = motion.update_in(cx, |motion, window, cx| {
                motion.closing.remove(&key);
                group.close(panel_id, window, cx);
                cx.refresh_windows();
            });
        })
        .detach();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct EditorDropTarget {
    placement: Option<Placement>,
    bounds: DropPlaceholderBounds,
}

struct EditorDropOverlay {
    node: NodeId,
    epoch: u64,
    target: Option<EditorDropTarget>,
}

impl Render for EditorDropOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(target) = self.target else {
            return Empty.into_any_element();
        };
        let bounds = Bounds::new(target.bounds.origin(), target.bounds.size());
        let bounds = transition(
            (
                "editor-drop-overlay",
                format!("{}-{}", self.node.as_u64(), self.epoch),
            ),
            bounds,
            Transition::new(TAB_MOTION_DURATION),
            window,
            cx,
        );
        drop_target_element(bounds)
    }
}

fn set_drop_overlay_target(
    overlay: &Entity<EditorDropOverlay>,
    target: Option<EditorDropTarget>,
    cx: &mut App,
) {
    overlay.update(cx, |overlay, cx| {
        if overlay.target == target {
            return;
        }
        if overlay.target.is_none() && target.is_some() {
            overlay.epoch = overlay.epoch.wrapping_add(1);
        }
        overlay.target = target;
        cx.notify();
    });
}

fn clear_drop_overlay(overlay: &Entity<EditorDropOverlay>, cx: &mut App) {
    set_drop_overlay_target(overlay, None, cx);
}

fn clear_all_drop_overlays(
    overlays: &Rc<RefCell<HashMap<NodeId, Entity<EditorDropOverlay>>>>,
    cx: &mut App,
) {
    let overlays = overlays.borrow().values().cloned().collect::<Vec<_>>();
    for overlay in overlays {
        clear_drop_overlay(&overlay, cx);
    }
}

fn drop_target_element(target: Bounds<Pixels>) -> AnyElement {
    div()
        .absolute()
        .left(target.origin.x)
        .top(target.origin.y)
        .w(target.size.width)
        .h(target.size.height)
        .rounded(px(8.))
        .bg(hsla(0.55, 0.38, 0.72, 0.20))
        .into_any_element()
}

struct TabDragPreview {
    title: SharedString,
}

impl Render for TabDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(30.))
            .max_w(px(180.))
            .flex()
            .items_center()
            .px_3()
            .rounded(px(7.))
            .bg(rgb(SURFACE))
            .text_xs()
            .text_color(rgb(INK))
            .child(self.title.clone())
    }
}

impl TabGroupRenderer for EditorTabGroupSkin {
    fn frame(&self, group: &TabGroupContext, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.inner
            .frame(group, window, cx)
            .border_0()
            .p(px(VIEW_INSET_PX))
            .rounded(px(VIEW_RADIUS_PX + VIEW_INSET_PX))
            .overflow_hidden()
            .bg(rgb(CANVAS))
    }

    fn content_frame(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        let node = group.node();
        let overlay = self.drop_overlay(node, cx);
        let moving_overlay = overlay.clone();
        let dropped_overlay = overlay.clone();
        let dropped_group = group.clone();
        let dock = self.dock.clone();

        self.inner
            .content_frame(group, window, cx)
            .border_0()
            .pt_0()
            .rounded_b(px(VIEW_RADIUS_PX))
            .overflow_hidden()
            .bg(rgb(PANEL))
            .on_drag_move(move |event: &DragMoveEvent<EditorPanelDrag>, _, cx| {
                let target = event.bounds.contains(&event.event.position).then(|| {
                    let placement = editor_drop_placement(event.bounds, event.event.position);
                    EditorDropTarget {
                        placement,
                        bounds: DropPlaceholderBounds::for_placement(event.bounds, placement),
                    }
                });
                set_drop_overlay_target(&moving_overlay, target, cx);
            })
            .on_drop(move |drag: &EditorPanelDrag, window, cx| {
                cx.stop_propagation();
                let target = dropped_overlay.read(cx).target;
                clear_drop_overlay(&dropped_overlay, cx);
                let Some(target) = target else {
                    return;
                };
                if let Some(placement) = target.placement {
                    let dock = dock.borrow().clone();
                    if let Some(dock) = dock {
                        let _ = dock.update(cx, |dock, cx| {
                            dock.move_panel(
                                drag.panel.panel(),
                                gpui_kit::component::dock::InsertTarget::Split {
                                    node,
                                    placement,
                                    size: None,
                                },
                                window,
                                cx,
                            );
                        });
                    }
                } else {
                    dropped_group.drop_panel(drag.panel.clone(), None, true, window, cx);
                }
            })
    }

    fn render_tab_bar(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        if group
            .panels()
            .iter()
            .any(|panel| panel.panel_name(cx) == DOCUMENT_PANEL)
        {
            cx.global_mut::<EditorDocuments>()
                .set_document_node(&self.root, group.node());
        }
        if let Some(title_bar) = self.singleton_title_bar(group, cx) {
            return title_bar;
        }

        let node = group.node();
        let content_overlay = self.drop_overlay(node, cx);
        let all_content_overlays = self.drop_overlays.clone();
        let displayed = group.active_panel().map(|panel| panel.panel_id(cx));
        let visible_panels = group
            .panels()
            .iter()
            .enumerate()
            .filter(|(_, panel)| panel.visible(cx))
            .map(|(ix, panel)| (ix, panel.clone()))
            .collect::<Vec<_>>();
        let tabs = visible_panels
            .into_iter()
            .map(|(ix, panel)| {
                let panel_id = panel.panel_id(cx);
                let title = PanelHandle::of(&panel)
                    .and_then(|handle| handle.tab_name(cx))
                    .unwrap_or_else(|| panel.panel_name(cx).into());
                let drag_title = title.clone();
                let drag = group
                    .drag_panel(ix, cx)
                    .map(|panel| EditorPanelDrag { panel });
                let closable = group.is_closable() && panel.closable(cx);
                let select_group = group.clone();
                let close_group = group.clone();
                let middle_close_group = group.clone();
                let drop_group = group.clone();
                let item_drop_group = group.clone();
                let drag_content_overlays = all_content_overlays.clone();
                let hover_content_overlay = content_overlay.clone();
                let drop_content_overlay = content_overlay.clone();

                let selected = !group.is_collapsed() && displayed == Some(panel_id);
                let closing = self
                    .tab_motion
                    .read(cx)
                    .closing
                    .contains(&panel_id.as_u64());
                let background = transition(
                    (("editor-tab-motion", panel_id.as_u64()), "background"),
                    theme_color(if selected { SURFACE } else { CHROME }),
                    Transition::new(TAB_MOTION_DURATION),
                    window,
                    cx,
                );
                let foreground = transition(
                    (("editor-tab-motion", panel_id.as_u64()), "foreground"),
                    theme_color(if selected { INK } else { MUTED }),
                    Transition::new(TAB_MOTION_DURATION),
                    window,
                    cx,
                );
                let opacity = transition(
                    (("editor-tab-motion", panel_id.as_u64()), "opacity"),
                    if closing { 0. } else { 1. },
                    Transition::new(TAB_MOTION_DURATION),
                    window,
                    cx,
                );
                let close_motion = self.tab_motion.clone();
                let middle_close_motion = self.tab_motion.clone();
                div()
                    .id(("editor-tab", panel_id.as_u64()))
                    .h(px(28.))
                    .max_w(px(240.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .rounded(px(7.))
                    .bg(background)
                    .opacity(opacity)
                    .text_sm()
                    .text_color(foreground)
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(SURFACE_HOVER)).text_color(rgb(INK)))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(title),
                    )
                    .on_click(move |_, window, cx| select_group.select_tab(ix, window, cx))
                    .when(closable, |this| {
                        this.on_mouse_down(MouseButton::Middle, move |_, window, cx| {
                            cx.stop_propagation();
                            middle_close_motion.update(cx, |motion, cx| {
                                motion.close(panel_id, middle_close_group.clone(), window, cx);
                            });
                        })
                    })
                    .when_some(drag, |this, drag| {
                        this.on_drag(drag, move |drag, offset, _, cx| {
                            cx.stop_propagation();
                            clear_all_drop_overlays(&drag_content_overlays, cx);
                            drag.panel.set_drag_offset(offset);
                            drag.panel.set_preview_size(size(px(180.), px(30.)));
                            cx.new(|_| TabDragPreview {
                                title: drag_title.clone(),
                            })
                        })
                    })
                    .when(group.is_droppable(), |this| {
                        this.drag_over::<EditorPanelDrag>(move |this, _, _, cx| {
                            clear_drop_overlay(&hover_content_overlay, cx);
                            this.border_l_2().border_color(cx.theme().drag_border)
                        })
                        .on_drop(move |drag: &EditorPanelDrag, window, cx| {
                            clear_drop_overlay(&drop_content_overlay, cx);
                            drop_group.drop_panel(drag.panel.clone(), Some(ix), true, window, cx);
                        })
                        .drag_over::<AnyDrag>(|this, _, _, cx| {
                            this.border_l_2().border_color(cx.theme().drag_border)
                        })
                        .on_drop(move |item: &AnyDrag, window, cx| {
                            item_drop_group.drop_item(item.clone(), None, window, cx);
                        })
                    })
                    .when(closable, |this| {
                        this.child(
                            div()
                                .id(("close-tab", panel_id.as_u64()))
                                .size(px(16.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(6.))
                                .cursor_pointer()
                                .text_color(rgb(MUTED))
                                .hover(|style| {
                                    style.bg(rgb(SURFACE_HOVER)).text_color(rgb(PRIMARY))
                                })
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    close_motion.update(cx, |motion, cx| {
                                        motion.close(panel_id, close_group.clone(), window, cx);
                                    });
                                })
                                .child(Icon::new(IconName::Close).xsmall()),
                        )
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        if tabs.is_empty() {
            return Empty.into_any_element();
        }

        let drop_group = group.clone();
        let item_drop_group = group.clone();
        let hover_content_overlay = content_overlay.clone();
        let drop_content_overlay = content_overlay;
        let empty_space = div()
            .id(("editor-tab-empty", node.as_u64()))
            .h_full()
            .flex_1()
            .min_w_8()
            .when(group.is_droppable(), |this| {
                this.drag_over::<EditorPanelDrag>(move |this, _, _, cx| {
                    clear_drop_overlay(&hover_content_overlay, cx);
                    this.bg(cx.theme().tokens.drop_target)
                })
                .on_drop(move |drag: &EditorPanelDrag, window, cx| {
                    clear_drop_overlay(&drop_content_overlay, cx);
                    drop_group.drop_panel(drag.panel.clone(), None, true, window, cx);
                })
                .drag_over::<AnyDrag>(|this, _, _, cx| this.bg(cx.theme().tokens.drop_target))
                .on_drop(move |item: &AnyDrag, window, cx| {
                    item_drop_group.drop_item(item.clone(), None, window, cx);
                })
            });

        div()
            .id(("editor-tab-bar", node.as_u64()))
            .h(px(36.))
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .overflow_x_scroll()
            .rounded_t(px(VIEW_RADIUS_PX))
            .bg(rgb(CHROME))
            .children(tabs)
            .child(empty_space)
            .into_any_element()
    }

    fn render_active_panel(
        &self,
        panel: AnyView,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let active_panel = self.inner.render_active_panel(panel, group, window, cx);
        let overlay = self.drop_overlay(group.node(), cx);
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .child(active_panel),
            )
            .child(overlay)
            .into_any_element()
    }

    fn render_drop_indicator(
        &self,
        indicator: DropIndicator,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<AnyElement> {
        let target = indicator.to();
        Some(drop_target_element(Bounds::new(
            target.origin(),
            target.size(),
        )))
    }

    fn render_empty(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.inner.render_empty(group, window, cx)
    }
}

/// Resolve a content drop like a modern code editor: the broad centre merges
/// into the current tab group, while only narrow edges create a split. At a
/// corner the physically nearest edge wins instead of horizontal priority.
fn editor_drop_placement(bounds: Bounds<Pixels>, position: Point<Pixels>) -> Option<Placement> {
    if !bounds.contains(&position) {
        return None;
    }

    let left = position.x - bounds.left();
    let right = bounds.right() - position.x;
    let top = position.y - bounds.top();
    let bottom = bounds.bottom() - position.y;
    let horizontal = if left < bounds.size.width * 0.20 {
        Some((Placement::Left, left))
    } else if right < bounds.size.width * 0.20 {
        Some((Placement::Right, right))
    } else {
        None
    };
    let vertical = if top < bounds.size.height * 0.20 {
        Some((Placement::Top, top))
    } else if bottom < bounds.size.height * 0.20 {
        Some((Placement::Bottom, bottom))
    } else {
        None
    };

    match (horizontal, vertical) {
        (Some((placement, distance)), Some((other, other_distance))) => {
            Some(if distance <= other_distance {
                placement
            } else {
                other
            })
        }
        (Some((placement, _)), None) | (None, Some((placement, _))) => Some(placement),
        (None, None) => None,
    }
}

impl ProjectWorkspace {
    fn new(
        session: WorkspaceSession,
        persistence: &AppPersistence,
        window: &mut Window,
        cx: &mut Context<WorkbenchWindow>,
    ) -> Self {
        window.set_window_title(&format!("{} — Kēne Editor", session.name()));
        let mut skin = None;
        let dock_ref = Rc::new(RefCell::new(None));
        let drop_overlays = Rc::new(RefCell::new(HashMap::new()));
        let tab_motion = cx.new(|_| EditorTabMotion::default());
        let dock_ref_for_skin = dock_ref.clone();
        let drop_overlays_for_skin = drop_overlays.clone();
        let root_for_skin = session.root().to_owned();
        let dock = cx.new(|cx| {
            let dock_skin = DockSkin::new(cx);
            skin = Some(dock_skin.clone());
            DockArea::new(
                format!("workspace-{}", session.key().workspace_id()),
                Some(LAYOUT_SCHEMA),
                window,
                cx,
            )
            .with_renderer(Rc::new(EditorDockSkin {
                inner: dock_skin,
                root: root_for_skin,
                dock: dock_ref_for_skin,
                drop_overlays: drop_overlays_for_skin,
                tab_motion,
            }))
        });
        *dock_ref.borrow_mut() = Some(dock.downgrade());
        cx.global_mut::<EditorDocuments>()
            .set_dock(session.root(), dock.downgrade());
        let skin = skin.expect("DockSkin::new runs inside DockArea construction");
        skin.set_panel_style(PanelStyle::TabBar, cx);
        skin.set_toggle_button_visible(false, cx);
        let restored = persistence
            .load_layout(session.key())
            .filter(|state| state.version == Some(LAYOUT_SCHEMA));
        let load_succeeded = restored.is_some_and(|state| {
            dock.update(cx, |dock, cx| dock.load(state, window, cx))
                .is_ok()
        });
        if !load_succeeded {
            install_default_layout(&dock, &session, window, cx);
        }

        let project = session.key().clone();
        let persistence = persistence.clone();
        let layout_subscription = cx.subscribe(&dock, move |_, dock, event: &DockEvent, cx| {
            if matches!(event, DockEvent::LayoutChanged) {
                let state = dock.read(cx).dump(cx);
                if let Err(error) = persistence.save_layout(&project, state) {
                    eprintln!("Kēne Editor could not persist layout: {error}");
                }
            }
        });
        Self {
            session,
            dock,
            _layout_subscription: layout_subscription,
        }
    }
}

fn install_default_layout(
    dock: &Entity<DockArea>,
    session: &WorkspaceSession,
    window: &mut Window,
    cx: &mut App,
) {
    let explorer = WorkbenchPanel::from_payload(
        PanelPayload::Explorer {
            root: session.root().to_owned(),
        },
        window,
        cx,
    )
    .expect("workspace explorer must be constructible");
    let mut documents = session
        .documents()
        .iter()
        .map(|document| {
            WorkbenchPanel::from_payload(
                PanelPayload::Document {
                    root: session.root().to_owned(),
                    relative: document.relative_path.clone(),
                },
                window,
                cx,
            )
            .expect("indexed document must remain readable")
        })
        .collect::<Vec<_>>();
    let inspector = WorkbenchPanel::from_payload(
        PanelPayload::Inspector {
            root: session.root().to_owned(),
        },
        window,
        cx,
    )
    .expect("workspace inspector must be constructible");
    let preview = WorkbenchPanel::from_payload(
        PanelPayload::Preview {
            root: session.root().to_owned(),
        },
        window,
        cx,
    )
    .expect("workspace preview must be constructible");
    let output = WorkbenchPanel::from_payload(
        PanelPayload::Output {
            root: session.root().to_owned(),
        },
        window,
        cx,
    )
    .expect("workspace output must be constructible");
    let mut document_tabs = DockLayout::tabs();
    for document in documents.drain(..) {
        document_tabs = document_tabs.panel_view(panel_handle(document), cx);
    }
    let layout = DockLayout::h_split()
        .child(
            DockLayout::tabs().panel_view(panel_handle(explorer), cx),
            Some(px(220.)),
        )
        .child(
            DockLayout::v_split().child(document_tabs, None).child(
                DockLayout::tabs().panel_view(panel_handle(output), cx),
                Some(px(150.)),
            ),
            None,
        )
        .child(
            DockLayout::v_split()
                .child(
                    DockLayout::tabs().panel_view(panel_handle(preview), cx),
                    None,
                )
                .child(
                    DockLayout::tabs().panel_view(panel_handle(inspector), cx),
                    None,
                ),
            Some(px(520.)),
        );
    dock.update(cx, |dock, cx| dock.set_center(layout, window, cx));
}

struct WorkbenchWindow {
    editor: WeakEntity<EditorApp>,
    persistence: AppPersistence,
    workspace: Option<ProjectWorkspace>,
    recents: Vec<PathBuf>,
    allow_close: bool,
    close_prompt_open: bool,
    focus: FocusHandle,
    _window_subscriptions: Vec<Subscription>,
}

fn install_close_guard(window: &mut Window, workbench: WeakEntity<WorkbenchWindow>, cx: &App) {
    window.on_window_should_close(cx, move |window, cx| {
        workbench
            .update(cx, |workbench, cx| {
                if workbench.allow_close || !workbench.has_unsaved_documents(cx) {
                    workbench.stop_preview(cx);
                    true
                } else {
                    workbench.confirm_close(window, cx);
                    false
                }
            })
            .unwrap_or(true)
    });
}

impl WorkbenchWindow {
    fn empty(
        editor: WeakEntity<EditorApp>,
        persistence: AppPersistence,
        recents: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        install_close_guard(window, cx.weak_entity(), cx);
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            this.set_preview_visible(window.is_window_active(), cx);
        });
        Self {
            editor,
            persistence,
            workspace: None,
            recents,
            allow_close: false,
            close_prompt_open: false,
            focus: cx.focus_handle(),
            _window_subscriptions: vec![activation],
        }
    }

    fn project(
        editor: WeakEntity<EditorApp>,
        persistence: AppPersistence,
        session: WorkspaceSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        install_close_guard(window, cx.weak_entity(), cx);
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            this.set_preview_visible(window.is_window_active(), cx);
        });
        let workspace = ProjectWorkspace::new(session, &persistence, window, cx);
        Self {
            editor,
            persistence,
            workspace: Some(workspace),
            recents: Vec::new(),
            allow_close: false,
            close_prompt_open: false,
            focus: cx.focus_handle(),
            _window_subscriptions: vec![activation],
        }
    }

    fn open_session(
        &mut self,
        session: WorkspaceSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_preview(cx);
        self.workspace = Some(ProjectWorkspace::new(
            session,
            &self.persistence,
            window,
            cx,
        ));
        self.recents.clear();
        cx.notify();
    }

    fn stop_preview(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.session.root().to_owned())
        else {
            return;
        };
        if let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(&root) {
            preview.stop();
            preview.set_panel_visible(false);
        }
    }

    fn set_preview_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        let Some(root) = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.session.root().to_owned())
        else {
            return;
        };
        if cx
            .global::<EditorDocuments>()
            .preview_panel(&root)
            .is_some()
            && let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(&root)
        {
            preview.set_window_visible(visible);
        }
    }

    fn open_folder(&mut self, _: &OpenFolder, _: &mut Window, cx: &mut Context<Self>) {
        prompt_open_folder(self.editor.clone(), cx);
    }

    fn reset_layout(&mut self, _: &ResetLayout, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.as_ref() else {
            return;
        };
        if let Err(error) = self.persistence.clear_layout(workspace.session.key()) {
            eprintln!("Kēne Editor could not reset layout: {error}");
            return;
        }
        install_default_layout(&workspace.dock, &workspace.session, window, cx);
        cx.notify();
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        self.save_documents(cx);
    }

    fn save_all(&mut self, _: &SaveAll, _: &mut Window, cx: &mut Context<Self>) {
        self.save_documents(cx);
    }

    fn save_documents(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.session.root().to_owned())
        else {
            return;
        };
        let result = cx.global_mut::<EditorDocuments>().save_all(&root);
        let notice = match result {
            Ok(0) => "No changes to save".to_owned(),
            Ok(1) => "Saved 1 document".to_owned(),
            Ok(count) => format!("Saved {count} documents"),
            Err(error) => format!("Save blocked: {error}"),
        };
        cx.global_mut::<EditorDocuments>().set_notice(&root, notice);
        cx.notify();
        cx.refresh_windows();
    }

    fn toggle_engine(&mut self, _: &ToggleEngine, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.as_ref() else {
            return;
        };
        let root = workspace.session.root().to_owned();
        if let Some(panel) = cx.global::<EditorDocuments>().preview_panel(&root) {
            workspace
                .dock
                .update(cx, |dock, cx| dock.select_panel(panel, window, cx));
            if let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(&root) {
                preview.set_panel_visible(true);
            }
            return;
        }
        let panel = match WorkbenchPanel::from_payload(
            PanelPayload::Preview { root: root.clone() },
            window,
            cx,
        ) {
            Ok(panel) => panel,
            Err(error) => {
                cx.global_mut::<EditorDocuments>()
                    .set_notice(&root, format!("Could not show Preview: {error}"));
                cx.refresh_windows();
                return;
            }
        };
        let panel_id = PanelId::from(panel.entity_id());
        workspace.dock.update(cx, |dock, cx| {
            dock.add_panel_view(
                panel_handle(panel),
                DockPlacement::Right,
                Some(px(520.)),
                window,
                cx,
            );
            dock.select_panel(panel_id, window, cx);
        });
        cx.global_mut::<EditorDocuments>()
            .set_notice(&root, "Preview shown · press Start when ready");
        cx.refresh_windows();
    }

    fn show_tool(&mut self, kind: ToolKind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.as_ref() else {
            return;
        };
        let root = workspace.session.root().to_owned();
        let name = kind.panel_name();
        if let Some(panel) = cx.global::<EditorDocuments>().tool_panel(&root, name) {
            workspace
                .dock
                .update(cx, |dock, cx| dock.select_panel(panel, window, cx));
            return;
        }
        let panel = match WorkbenchPanel::from_payload(kind.payload(root.clone()), window, cx) {
            Ok(panel) => panel,
            Err(error) => {
                cx.global_mut::<EditorDocuments>()
                    .set_notice(&root, format!("Could not show tool: {error}"));
                cx.refresh_windows();
                return;
            }
        };
        let panel_id = PanelId::from(panel.entity_id());
        workspace.dock.update(cx, |dock, cx| {
            dock.add_panel_view(
                panel_handle(panel),
                DockPlacement::Right,
                Some(px(340.)),
                window,
                cx,
            );
            dock.select_panel(panel_id, window, cx);
        });
        cx.refresh_windows();
    }

    fn migrate_eiyashou(
        &mut self,
        _: &MigrateEiyashou,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.session.root().to_owned())
        else {
            return;
        };
        if self.has_unsaved_documents(cx) {
            cx.global_mut::<EditorDocuments>()
                .set_notice(&root, "Save or discard source changes before migration");
            cx.refresh_windows();
            return;
        }
        let plan = match MigrationPlan::preview(&root) {
            Ok(plan) => plan,
            Err(error) => {
                cx.global_mut::<EditorDocuments>()
                    .set_notice(&root, format!("Migration preview failed: {error}"));
                cx.refresh_windows();
                return;
            }
        };
        if plan.changes.is_empty() {
            cx.global_mut::<EditorDocuments>()
                .set_notice(&root, "No eligible legacy Eiyashou sources were found");
            cx.refresh_windows();
            return;
        }
        let detail = plan.diff_preview();
        let receiver = window.prompt(
            PromptLevel::Warning,
            "Apply Eiyashou source migration?",
            Some(&detail),
            &[
                PromptButton::Other("Apply Renames".into()),
                PromptButton::Cancel("Cancel".into()),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = receiver.await.ok();
            let _ = this.update_in(cx, |this, window, cx| {
                if answer != Some(0) {
                    cx.global_mut::<EditorDocuments>()
                        .set_notice(&root, "Migration cancelled after preview");
                    cx.refresh_windows();
                    return;
                }
                match plan.apply() {
                    Ok(count) => match WorkspaceSession::open(&root) {
                        Ok(session) => {
                            this.open_session(session, window, cx);
                            cx.global_mut::<EditorDocuments>().set_notice(
                                &root,
                                format!("Migrated {count} source file(s) to .shou"),
                            );
                        }
                        Err(error) => cx.global_mut::<EditorDocuments>().set_notice(
                            &root,
                            format!("Migration applied, but workspace refresh failed: {error}"),
                        ),
                    },
                    Err(error) => cx
                        .global_mut::<EditorDocuments>()
                        .set_notice(&root, format!("Migration failed: {error}")),
                }
                cx.notify();
                cx.refresh_windows();
            });
        })
        .detach();
    }

    fn has_unsaved_documents(&self, cx: &App) -> bool {
        self.workspace.as_ref().is_some_and(|workspace| {
            cx.global::<EditorDocuments>()
                .has_dirty_documents(workspace.session.root())
        })
    }

    fn confirm_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_prompt_open {
            return;
        }
        self.close_prompt_open = true;
        let receiver = window.prompt(
            PromptLevel::Warning,
            "Save changes before closing?",
            Some("Unsaved source changes have a recovery draft, but are not in the project yet."),
            &[
                PromptButton::Other("Save and Close".into()),
                PromptButton::Other("Close Without Saving".into()),
                PromptButton::Cancel("Cancel".into()),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = receiver.await.ok();
            let _ = this.update_in(cx, |this, window, cx| {
                this.close_prompt_open = false;
                match answer {
                    Some(0) => {
                        let Some(root) = this
                            .workspace
                            .as_ref()
                            .map(|workspace| workspace.session.root().to_owned())
                        else {
                            this.allow_close = true;
                            window.remove_window();
                            return;
                        };
                        match cx.global_mut::<EditorDocuments>().save_all(&root) {
                            Ok(_) => {
                                this.allow_close = true;
                                window.remove_window();
                            }
                            Err(error) => {
                                cx.global_mut::<EditorDocuments>()
                                    .set_notice(&root, format!("Save blocked: {error}"));
                                cx.refresh_windows();
                            }
                        }
                    }
                    Some(1) => {
                        this.allow_close = true;
                        window.remove_window();
                    }
                    _ => {}
                }
            });
        })
        .detach();
    }

    fn render_activity_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.clone();
        let has_unsaved_changes = self.workspace.as_ref().is_some_and(|workspace| {
            cx.global::<EditorDocuments>()
                .has_dirty_documents(workspace.session.root())
        });
        let (
            assets_open,
            characters_open,
            scenes_open,
            problems_open,
            performance_open,
            preview_open,
        ) = self
            .workspace
            .as_ref()
            .map(|workspace| {
                let root = workspace.session.root();
                let documents = cx.global::<EditorDocuments>();
                (
                    documents.tool_panel(root, ASSETS_PANEL).is_some(),
                    documents.tool_panel(root, CHARACTERS_PANEL).is_some(),
                    documents.tool_panel(root, SCENES_PANEL).is_some(),
                    documents.tool_panel(root, PROBLEMS_PANEL).is_some(),
                    documents.tool_panel(root, PERFORMANCE_PANEL).is_some(),
                    documents.preview_panel(root).is_some(),
                )
            })
            .unwrap_or_default();
        let rail = div()
            .id("activity-rail-content")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .py_1()
            .gap_1()
            .bg(rgb(CHROME))
            .rounded(px(VIEW_RADIUS_PX))
            .child(
                div()
                    .id("workspace-status")
                    .size(px(ACTIVITY_BRAND_SIZE_PX))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(7.))
                    .bg(if has_unsaved_changes {
                        rgb(0xdb7780)
                    } else {
                        rgb(PRIMARY)
                    })
                    .text_size(px(17.))
                    .font_weight(gpui_kit::FontWeight::BOLD)
                    .text_color(rgb(CANVAS))
                    .child("K"),
            )
            .child(
                div()
                    .id("activity-explorer")
                    .size(px(ACTIVITY_ITEM_SIZE_PX))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(7.))
                    .bg(rgb(SURFACE))
                    .child(
                        Icon::new(IconName::FileText)
                            .with_size(px(ACTIVITY_ICON_SIZE_PX))
                            .text_color(rgb(PRIMARY)),
                    ),
            )
            .child(activity_divider())
            .when(self.workspace.is_some(), |this| {
                this.child(
                    activity_tool("activity-assets", IconName::Palette, assets_open).on_click(
                        cx.listener(|this, _, window, cx| {
                            this.show_tool(ToolKind::Assets, window, cx)
                        }),
                    ),
                )
                .child(
                    activity_tool("activity-characters", IconName::User, characters_open).on_click(
                        cx.listener(|this, _, window, cx| {
                            this.show_tool(ToolKind::Characters, window, cx)
                        }),
                    ),
                )
                .child(
                    activity_tool("activity-scenes", IconName::Map, scenes_open).on_click(
                        cx.listener(|this, _, window, cx| {
                            this.show_tool(ToolKind::Scenes, window, cx)
                        }),
                    ),
                )
                .child(activity_divider())
                .child(
                    activity_tool("activity-problems", IconName::TriangleAlert, problems_open)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.show_tool(ToolKind::Problems, window, cx)
                        })),
                )
                .child(
                    activity_tool("activity-performance", IconName::Cpu, performance_open)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.show_tool(ToolKind::Performance, window, cx)
                        })),
                )
            })
            .when(self.workspace.is_some(), |this| {
                this.child(
                    div()
                        .id("activity-preview")
                        .size(px(ACTIVITY_ITEM_SIZE_PX))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(7.))
                        .when(preview_open, |style| style.bg(rgb(SURFACE)))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_engine(&ToggleEngine, window, cx)
                        }))
                        .child(
                            Icon::new(IconName::Play)
                                .with_size(px(ACTIVITY_ICON_SIZE_PX))
                                .text_color(rgb(if preview_open { PRIMARY } else { MUTED })),
                        ),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .id("activity-open-folder")
                    .size(px(ACTIVITY_ITEM_SIZE_PX))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                    .on_click(move |_, _, cx| prompt_open_folder(editor.clone(), cx))
                    .child(
                        Icon::new(IconName::FolderOpen)
                            .with_size(px(ACTIVITY_ICON_SIZE_PX))
                            .text_color(rgb(MUTED)),
                    ),
            )
            .when(self.workspace.is_some(), |this| {
                this.child(activity_divider())
                    .child(
                        div()
                            .id("activity-migrate-eiyashou")
                            .size(px(ACTIVITY_ITEM_SIZE_PX))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(7.))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.migrate_eiyashou(&MigrateEiyashou, window, cx)
                            }))
                            .child(
                                Icon::new(IconName::Replace)
                                    .with_size(px(ACTIVITY_ICON_SIZE_PX))
                                    .text_color(rgb(MUTED)),
                            ),
                    )
                    .child(
                        div()
                            .id("activity-reset-layout")
                            .size(px(ACTIVITY_ITEM_SIZE_PX))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(7.))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.reset_layout(&ResetLayout, window, cx)
                            }))
                            .child(
                                Icon::new(IconName::RotateCw)
                                    .with_size(px(ACTIVITY_ICON_SIZE_PX))
                                    .text_color(rgb(MUTED)),
                            ),
                    )
            });

        div()
            .id("activity-rail")
            .w(px(ACTIVITY_RAIL_WIDTH_PX))
            .h_full()
            .flex_none()
            .p(px(VIEW_INSET_PX))
            .child(rail)
    }

    fn render_empty(&self) -> impl IntoElement {
        let editor = self.editor.clone();
        let recents = self.recents.clone();
        div()
            .id("empty-workbench")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(CANVAS))
            .child(
                div()
                    .w(px(520.))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(rgb(INK))
                                    .child("Kēne Editor"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(MUTED))
                                    .child("Native visual-novel workspace"),
                            ),
                    )
                    .child(
                        div()
                            .id("open-folder")
                            .h(px(34.))
                            .w(px(148.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .rounded(px(7.))
                            .bg(rgb(PRIMARY_DIM))
                            .text_sm()
                            .text_color(rgb(PRIMARY))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                            .on_click(move |_, _, cx| prompt_open_folder(editor.clone(), cx))
                            .child(
                                Icon::new(IconName::FolderOpen)
                                    .small()
                                    .text_color(rgb(PRIMARY)),
                            )
                            .child("Open Folder"),
                    )
                    .when(!recents.is_empty(), |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .text_color(rgb(MUTED))
                                        .child("RECENT"),
                                )
                                .children(recents.into_iter().take(6).enumerate().map(
                                    |(index, path)| {
                                        let editor = self.editor.clone();
                                        let open_path = path.clone();
                                        div()
                                            .id(("recent-project", index))
                                            .h(px(34.))
                                            .flex()
                                            .items_center()
                                            .gap_3()
                                            .px_3()
                                            .rounded(px(7.))
                                            .bg(rgb(CHROME))
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                            .on_click(move |_, _, cx| {
                                                open_paths(&editor, vec![open_path.clone()], cx)
                                            })
                                            .child(
                                                Icon::new(IconName::Folder)
                                                    .xsmall()
                                                    .text_color(rgb(PRIMARY)),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_sm()
                                                    .text_color(rgb(INK))
                                                    .child(
                                                        path.file_name()
                                                            .and_then(|name| name.to_str())
                                                            .unwrap_or("Project")
                                                            .to_owned(),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(MUTED))
                                                    .child(path.display().to_string()),
                                            )
                                    },
                                )),
                        )
                    }),
            )
    }
}

impl Render for WorkbenchWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = if let Some(workspace) = &self.workspace {
            div()
                .id("dock-host")
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .child(workspace.dock.clone())
                .into_any_element()
        } else {
            self.render_empty().into_any_element()
        };
        div()
            .id("project-window")
            .key_context("KeineWorkbench")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open_folder))
            .on_action(cx.listener(Self::reset_layout))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_all))
            .on_action(cx.listener(Self::toggle_engine))
            .on_action(cx.listener(Self::migrate_eiyashou))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .p(px(VIEW_INSET_PX))
            .bg(rgb(CANVAS))
            .text_color(rgb(INK))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_activity_rail(cx))
                    .child(main),
            )
            .child(div().id("overlay-host").absolute().inset_0())
    }
}

fn activity_tool(id: &'static str, icon: IconName, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(ACTIVITY_ITEM_SIZE_PX))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .when(active, |style| style.bg(rgb(SURFACE)))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .child(
            Icon::new(icon)
                .with_size(px(ACTIVITY_ICON_SIZE_PX))
                .text_color(rgb(if active { PRIMARY } else { MUTED })),
        )
}

fn activity_divider() -> Div {
    div().w(px(22.)).h(px(1.)).my(px(1.)).bg(rgb(BORDER))
}

fn file_action_icon(id: &'static str, icon: AssetIconName) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(22.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .child(Icon::new(icon).xsmall().text_color(rgb(MUTED)))
}

fn file_context_menu_item(
    id: &'static str,
    icon: AssetIconName,
    label: &'static str,
    danger: bool,
) -> Stateful<Div> {
    let color = if danger { 0xdb7780 } else { INK };
    div()
        .id(id)
        .h(px(27.))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .rounded(px(5.))
        .text_xs()
        .text_color(rgb(color))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .child(Icon::new(icon).xsmall().text_color(rgb(color)))
        .child(label)
}

fn reveal_workspace_path(root: &Path, relative: &Path) {
    let path = root.join(relative);
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(root))
        .spawn();
}

fn short_error(error: &io::Error) -> String {
    error
        .to_string()
        .lines()
        .next()
        .unwrap_or("File operation failed")
        .to_owned()
}

fn prompt_open_folder(editor: WeakEntity<EditorApp>, cx: &mut App) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: true,
        prompt: Some("Open Folder".into()),
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = receiver.await {
            cx.update(|cx| open_paths(&editor, paths, cx));
        }
    })
    .detach();
}

fn open_paths(editor: &WeakEntity<EditorApp>, paths: Vec<PathBuf>, cx: &mut App) {
    let _ = editor.update(cx, |editor, cx| editor.open_paths(paths, cx));
}

fn open_workspace_document(root: &Path, relative: &Path, window: &mut Window, cx: &mut App) {
    let existing = cx.global::<EditorDocuments>().panel_for(root, relative);
    let Some((dock, document_node)) = cx.global::<EditorDocuments>().document_dock(root) else {
        return;
    };
    if let Some(panel) = existing {
        let _ = dock.update(cx, |dock, cx| dock.select_panel(panel, window, cx));
        return;
    }
    let panel = match WorkbenchPanel::from_payload(
        PanelPayload::Document {
            root: root.to_owned(),
            relative: relative.to_owned(),
        },
        window,
        cx,
    ) {
        Ok(panel) => panel,
        Err(error) => {
            cx.global_mut::<EditorDocuments>().set_notice(
                root,
                format!("Could not open {}: {error}", relative.display()),
            );
            cx.refresh_windows();
            return;
        }
    };
    let panel_id = PanelId::from(panel.entity_id());
    let _ = dock.update(cx, |dock, cx| {
        dock.add_panel_view(panel_handle(panel), DockPlacement::Center, None, window, cx);
        if let Some(node) = document_node {
            dock.move_panel(
                panel_id,
                InsertTarget::Tabs {
                    node,
                    ix: None,
                    activate: true,
                },
                window,
                cx,
            );
        }
        dock.select_panel(panel_id, window, cx);
    });
}

fn window_options(index: usize, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(offset_bounds(index, cx))),
        window_min_size: Some(size(px(720.), px(480.))),
        app_id: Some(APP_ID.into()),
        ..Default::default()
    }
}

fn offset_bounds(index: usize, cx: &App) -> Bounds<gpui_kit::Pixels> {
    let mut bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
    let offset = px(index.min(6) as f32 * 34.);
    bounds.origin.x += offset;
    bounds.origin.y += offset;
    bounds
}

fn listen_for_secondary_launches(
    receiver: InstanceReceiver,
    editor: WeakEntity<EditorApp>,
    cx: &mut App,
) {
    let background = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        loop {
            let receiver = receiver.clone();
            let result = background.spawn(async move { receiver.receive() }).await;
            match result {
                Ok(request) => {
                    cx.update(|cx| open_paths(&editor, request.paths, cx));
                }
                Err(error) => eprintln!("Kēne Editor instance route failed: {error}"),
            }
        }
    })
    .detach();
}

pub fn run() {
    let app_data = match crate::app_data::root() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Kēne Editor could not locate app data: {error}");
            return;
        }
    };
    let paths = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let instance = match acquire_or_forward(&app_data, paths.clone()) {
        Ok(Startup::Forwarded) => return,
        Ok(Startup::Primary(instance)) => instance,
        Err(error) => {
            eprintln!("Kēne Editor could not initialize its app instance: {error}");
            return;
        }
    };
    let receiver = instance.receiver();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_quit_mode(gpui_kit::QuitMode::LastWindowClosed)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            configure_dark_theme(cx);
            let persistence = AppPersistence::new(app_data.clone());
            cx.set_global(EditorDocuments::new(persistence.clone()));
            register_workbench_panels(cx);
            cx.bind_keys([
                KeyBinding::new("cmd-o", OpenFolder, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-o", OpenFolder, Some("KeineWorkbench")),
                KeyBinding::new("cmd-s", Save, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-s", Save, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-s", SaveAll, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-shift-s", SaveAll, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-r", ToggleEngine, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-shift-r", ToggleEngine, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-m", MigrateEiyashou, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-shift-m", MigrateEiyashou, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-0", ResetLayout, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-shift-0", ResetLayout, Some("KeineWorkbench")),
                KeyBinding::new("cmd-c", CopyBlocks, Some("KeineBlockView")),
                KeyBinding::new("ctrl-c", CopyBlocks, Some("KeineBlockView")),
                KeyBinding::new("cmd-v", PasteBlocks, Some("KeineBlockView")),
                KeyBinding::new("ctrl-v", PasteBlocks, Some("KeineBlockView")),
                KeyBinding::new("enter", BeginTextBlock, Some("KeineBlockView")),
                KeyBinding::new("tab", ToggleBlockPicker, Some("KeineBlockView")),
                KeyBinding::new("down", BlockPickerNext, Some("KeineBlockPicker")),
                KeyBinding::new("up", BlockPickerPrevious, Some("KeineBlockPicker")),
                KeyBinding::new("tab", AcceptBlockPicker, Some("KeineBlockPicker")),
                KeyBinding::new("enter", AcceptBlockPicker, Some("KeineBlockPicker")),
                KeyBinding::new("escape", CloseBlockPicker, Some("KeineBlockPicker")),
                KeyBinding::new("backspace", DeleteBlocks, Some("KeineBlockView")),
                KeyBinding::new("delete", DeleteBlocks, Some("KeineBlockView")),
                KeyBinding::new("alt-up", MoveBlocksUp, Some("KeineBlockView")),
                KeyBinding::new("alt-down", MoveBlocksDown, Some("KeineBlockView")),
            ]);
            let editor = cx.new(|cx| EditorApp::new(cx.weak_entity(), persistence, instance));
            let weak_editor = editor.downgrade();
            editor.update(cx, |editor, cx| {
                if paths.is_empty() {
                    editor.open_empty(cx);
                } else {
                    editor.open_paths(paths, cx);
                }
            });
            listen_for_secondary_launches(receiver, weak_editor, cx);
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::point;

    #[test]
    fn duplicate_project_routes_to_the_existing_window() {
        let project = ProjectKey::from_path(".").unwrap();
        let duplicate = ProjectKey::from_path(std::env::current_dir().unwrap()).unwrap();
        let mut registry = WindowRegistry::default();
        registry.insert(project, 41);
        assert_eq!(registry.existing(&duplicate), Some(41));
        assert_eq!(registry.windows.len(), 1);
    }

    #[test]
    fn editor_drop_zones_keep_a_large_merge_center() {
        let bounds = Bounds::new(point(px(100.), px(50.)), size(px(1000.), px(500.)));

        assert_eq!(
            editor_drop_placement(bounds, point(px(600.), px(300.))),
            None
        );
        assert_eq!(
            editor_drop_placement(bounds, point(px(200.), px(300.))),
            Some(Placement::Left)
        );
        assert_eq!(
            editor_drop_placement(bounds, point(px(1000.), px(300.))),
            Some(Placement::Right)
        );
        assert_eq!(
            editor_drop_placement(bounds, point(px(600.), px(100.))),
            Some(Placement::Top)
        );
        assert_eq!(
            editor_drop_placement(bounds, point(px(600.), px(500.))),
            Some(Placement::Bottom)
        );
    }

    #[test]
    fn editor_drop_corner_uses_the_nearest_edge() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(1000.), px(500.)));

        assert_eq!(
            editor_drop_placement(bounds, point(px(25.), px(80.))),
            Some(Placement::Left)
        );
        assert_eq!(
            editor_drop_placement(bounds, point(px(90.), px(15.))),
            Some(Placement::Top)
        );
    }

    #[test]
    fn picker_preferences_hide_browse_items_without_removing_search_access() {
        let preferences = BlockPickerPreferences {
            hidden: vec!["Video".into()],
            favorites: vec!["Wait".into()],
            ..BlockPickerPreferences::default()
        };
        assert!(!picker_kinds(&preferences, "", None, false).contains(&InsertKind::Video));
        assert!(picker_kinds(&preferences, "video", None, false).contains(&InsertKind::Video));
        assert_eq!(
            picker_kinds(&preferences, "", Some("Favorites"), false),
            vec![InsertKind::Wait]
        );
    }

    #[test]
    fn picker_item_reorder_is_limited_to_its_category() {
        let universe = InsertKind::ALL
            .into_iter()
            .map(InsertKind::label)
            .collect::<Vec<_>>();
        let group = InsertKind::ALL
            .into_iter()
            .filter(|kind| kind.category() == "Text")
            .map(InsertKind::label)
            .collect::<Vec<_>>();
        let mut order = Vec::new();
        move_group_preference(&mut order, "Dialogue", &group, &universe, -1);
        assert!(
            preference_rank(&order, "Dialogue", usize::MAX / 2)
                < preference_rank(&order, "Narration", usize::MAX / 2)
        );
        assert!(order.contains(&"Background".to_owned()));
    }
}
