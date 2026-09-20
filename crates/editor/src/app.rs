use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::base::{Placement, ResizeHandleContext};
use gpui_kit::component::dock::{
    AnyDrag, BasePanel, BasePanelView, DockArea, DockAreaRenderer, DockContext, DockEvent,
    DockLayout, DockPlacement, DockSkin, DragPanel, DropIndicator, DropPlaceholderBounds,
    InsertTarget, NodeId, Panel, PanelBuildContext, PanelEvent, PanelHandle, PanelId, PanelInfo,
    PanelState, PanelStyle, TabGroupContext, TabGroupRenderer, panel_handle, register_panel,
};
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState, Position};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, Theme, ThemeMode};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext as _, Axis, Bounds, Context, Div, DragMoveEvent, Empty,
    Entity, EventEmitter, FocusHandle, Focusable, Global, Hsla, IntoElement, KeyBinding,
    MouseButton, ObjectFit, PathPromptOptions, Pixels, Point, PromptButton, PromptLevel, Render,
    RenderImage, SharedString, Stateful, Subscription, WeakEntity, Window, WindowBounds,
    WindowHandle, WindowOptions, actions, canvas, div, hsla, img, linear_color_stop,
    linear_gradient, prelude::*, px, rgb, size,
};
use serde::{Deserialize, Serialize};

use crate::app_data::APP_ID;
use crate::authoring::{
    AuthoringIndex, AuthoringSelection, InsertKind, ProblemSeverity, append_character,
    append_scene, dialogues_for_source, insert_statement, replace_dialogue_text,
};
use crate::document::{DocumentHandle, DocumentManager, SaveError, is_eiyashou_authoring_document};
use crate::instance::{InstanceReceiver, PrimaryInstance, Startup, acquire_or_forward};
use crate::migration::MigrationPlan;
use crate::persistence::AppPersistence;
use crate::preview::{PreviewController, PreviewLifecycle, PreviewMode, map_preview_point};
use crate::project_key::ProjectKey;
use crate::projection::EiyashouProjection;
use crate::syntax::eiyashou_highlighter_factory;
use crate::workspace::{WorkspaceFile, WorkspaceSession};

const CANVAS: u32 = 0x11151b;
const CHROME: u32 = 0x151a21;
const PANEL: u32 = 0x19212a;
const SURFACE: u32 = 0x222d38;
const SURFACE_HOVER: u32 = 0x2b3743;
const BORDER: u32 = 0x3a4855;
const INK: u32 = 0xd7dee6;
const MUTED: u32 = 0x98a3ae;
const PRIMARY: u32 = 0xbaebff;
const PRIMARY_DIM: u32 = 0x30434d;
const SUCCESS: u32 = 0x69c38d;
const LAYOUT_SCHEMA: usize = 1;
// Each Dock group owns one half-gap. Adjacent groups therefore have a 4 px
// gutter, while the window and activity rail contribute the matching other
// half at the workspace edge. Keep this as the single spacing owner instead
// of adding per-panel margins.
const VIEW_INSET_PX: f32 = 2.;
const VIEW_RADIUS_PX: f32 = 9.;
// GPUI reserves one leading digit plus input padding before the widest visible
// line number. Crop that reserve while keeping the built-in right margin as a
// distinct dark gap before source text.
const EDITOR_GUTTER_TRIM_PX: f32 = 18.;
const TAB_MOTION_DURATION: Duration = Duration::from_millis(140);
const PREVIEW_POLL_INTERVAL: Duration = Duration::from_millis(16);

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
        MigrateEiyashou
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
    theme.secondary_active = theme_color(0x202a34);
    theme.secondary_foreground = theme_color(0xbec7d0);
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
    theme.selection = theme_color(0x344752);
    theme.sidebar = theme_color(CHROME);
    theme.sidebar_border = theme_color(BORDER);
    theme.sidebar_foreground = theme_color(0xa2adb8);
    theme.sidebar_accent = theme_color(PRIMARY_DIM);
    theme.sidebar_accent_foreground = theme_color(PRIMARY);
    theme.tab = theme_color(PANEL);
    theme.tab_bar = theme_color(CHROME);
    theme.tab_bar_segmented = theme_color(SURFACE);
    theme.tab_foreground = theme_color(0x929da8);
    theme.tab_active = theme_color(SURFACE);
    theme.tab_active_foreground = theme_color(INK);
    theme.title_bar = theme_color(CHROME);
    theme.title_bar_border = theme_color(BORDER);
    theme.status_bar = theme_color(CHROME);
    theme.status_bar_border = theme_color(BORDER);
    theme.scrollbar = hsla(0.57, 0.12, 0.12, 0.08);
    theme.scrollbar_thumb = hsla(0.55, 0.18, 0.72, 0.22);
    theme.scrollbar_thumb_hover = hsla(0.55, 0.24, 0.78, 0.38);
    theme.input = theme_color(BORDER);
    Arc::make_mut(&mut theme.highlight_theme)
        .style
        .editor_gutter_background = Some(theme_color(0x090c10));
    theme.popover = theme_color(SURFACE);
    theme.popover_foreground = theme_color(INK);
    theme.button = theme_color(SURFACE);
    theme.button_hover = theme_color(SURFACE_HOVER);
    theme.button_active = theme_color(0x202a34);
    theme.button_foreground = theme_color(0xc0c9d2);
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
    workspaces: HashMap<PathBuf, WorkspaceDocuments>,
}

impl Global for EditorDocuments {}

impl EditorDocuments {
    fn new(persistence: AppPersistence) -> Self {
        Self {
            persistence,
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

    fn refresh_authoring(&mut self, root: &Path) {
        if let Ok(workspace) = self.ensure_workspace(root) {
            let overrides = workspace.manager.source_overrides();
            workspace.authoring = AuthoringIndex::load(root, &workspace.files, &overrides);
        }
    }

    fn authoring(&self, root: &Path) -> AuthoringIndex {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .map(|workspace| workspace.authoring.clone())
            .unwrap_or_default()
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
    card_editors: Vec<CardNameEditor>,
    dialogue_editors: Vec<DialogueTextEditor>,
    tool_inputs: Vec<Entity<InputState>>,
    timeline: VecDeque<TimelineSample>,
    recovery_epoch: u64,
    preview_image: Option<Arc<RenderImage>>,
    preview_frame_id: u64,
    preview_lifecycle: PreviewLifecycle,
    preview_bounds: Arc<Mutex<Option<Bounds<Pixels>>>>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DocumentMode {
    #[default]
    Text,
    Card,
    Dialogue,
}

struct CardNameEditor {
    scene_index: usize,
    state: Entity<InputState>,
}

struct DialogueTextEditor {
    line: usize,
    state: Entity<InputState>,
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
            let mut panel = Self {
                content,
                focus: cx.focus_handle(),
                document_mode: DocumentMode::Text,
                card_editors: Vec::new(),
                dialogue_editors: Vec::new(),
                tool_inputs: Vec::new(),
                timeline: VecDeque::with_capacity(60),
                recovery_epoch: 0,
                preview_image: None,
                preview_frame_id: 0,
                preview_lifecycle: PreviewLifecycle::Off,
                preview_bounds: Arc::new(Mutex::new(None)),
                _subscriptions: Vec::new(),
            };
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
            panel.rebuild_card_editors(window, cx);
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

    fn rebuild_card_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.card_editors.clear();
        self.dialogue_editors.clear();
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

        let projection = EiyashouProjection::parse(document.borrow().contents());
        let window_handle = window.window_handle();
        for (scene_index, scene) in projection.scenes.into_iter().enumerate() {
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(scene.name)
                    .placeholder("Scene name")
            });
            let document = document.clone();
            let source_editor = editor.clone();
            let root = root.clone();
            let state_for_change = state.clone();
            let subscription = cx.subscribe(&state, move |_, _, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let new_name = state_for_change.read(cx).value().to_string();
                let source = document.borrow().contents().to_owned();
                let projection = EiyashouProjection::parse(&source);
                match projection.rename_scene(&source, scene_index, &new_name) {
                    Ok(edited) => {
                        let result = cx.update_window(window_handle, |_, window, cx| {
                            source_editor.update(cx, |editor, cx| {
                                editor.replace_all(edited, window, cx);
                            });
                        });
                        let notice = match result {
                            Ok(()) => "Scene name updated from Card view".to_owned(),
                            Err(error) => format!("Card edit failed to reach Text view: {error}"),
                        };
                        cx.global_mut::<EditorDocuments>().set_notice(&root, notice);
                    }
                    Err(error) => cx
                        .global_mut::<EditorDocuments>()
                        .set_notice(&root, format!("Card edit blocked: {error}")),
                }
                cx.refresh_windows();
            });
            self._subscriptions.push(subscription);
            self.card_editors
                .push(CardNameEditor { scene_index, state });
        }

        let dialogues = dialogues_for_source(relative, document.borrow().contents());
        for dialogue in dialogues.into_iter().filter(|dialogue| dialogue.editable) {
            let line = dialogue.line;
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(dialogue.text)
                    .placeholder("Dialogue")
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
                            Ok(()) => "Dialogue updated from visual view".to_owned(),
                            Err(error) => format!("Dialogue edit failed: {error}"),
                        };
                        cx.global_mut::<EditorDocuments>().set_notice(&root, notice);
                    }
                    Err(error) => cx
                        .global_mut::<EditorDocuments>()
                        .set_notice(&root, format!("Dialogue edit blocked: {error}")),
                }
                cx.refresh_windows();
            });
            self._subscriptions.push(subscription);
            self.dialogue_editors
                .push(DialogueTextEditor { line, state });
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = Theme::global(cx).mono_font_family.clone();
        let body = match &self.content {
            PanelContent::Explorer { root, files } => {
                let project_name = root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("PROJECT")
                    .to_uppercase();
                let project_root = root.clone();
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .px_3()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(rgb(MUTED))
                            .child(project_name),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(
                                div()
                                    .size_full()
                                    .flex()
                                    .flex_col()
                                    .children(files.iter().take(400).enumerate().map(
                                        |(index, file)| {
                                            let root = project_root.clone();
                                            let relative = file.relative_path.clone();
                                            div()
                                                .id(("explorer-file", index))
                                                .h(px(25.))
                                                .w_auto()
                                                .min_w_full()
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .px_2()
                                                .rounded(px(5.))
                                                .whitespace_nowrap()
                                                .text_xs()
                                                .text_color(rgb(0xa9b5c1))
                                                .cursor_pointer()
                                                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                                .on_click(move |_, window, cx| {
                                                    open_workspace_document(
                                                        &root, &relative, window, cx,
                                                    );
                                                })
                                                .child(
                                                    Icon::new(IconName::FileText)
                                                        .xsmall()
                                                        .text_color(rgb(0x76bfdc)),
                                                )
                                                .child(file.relative_path.display().to_string())
                                        },
                                    ))
                                    .overflow_scrollbar()
                                    .id("explorer-files"),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .right_0()
                                    .bottom(px(10.))
                                    .w(px(14.))
                                    .bg(linear_gradient(
                                        90.,
                                        linear_color_stop(hsla(0.57, 0.18, 0.12, 0.), 0.),
                                        linear_color_stop(hsla(0.57, 0.18, 0.12, 0.72), 1.),
                                    )),
                            ),
                    )
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
                let authoring = cx.global::<EditorDocuments>().authoring(root);
                let palette_kinds = [
                    InsertKind::Narration,
                    InsertKind::Dialogue,
                    InsertKind::Background,
                    InsertKind::Figure,
                    InsertKind::Choice,
                ]
                .into_iter()
                .filter(|kind| match kind {
                    InsertKind::Narration => true,
                    InsertKind::Dialogue => !authoring.characters.is_empty(),
                    InsertKind::Background => authoring
                        .assets
                        .iter()
                        .any(|asset| asset.kind == crate::authoring::AssetKind::Background),
                    InsertKind::Figure => authoring
                        .assets
                        .iter()
                        .any(|asset| asset.kind == crate::authoring::AssetKind::Figure),
                    InsertKind::Choice => !authoring.scenes.is_empty(),
                })
                .collect::<Vec<_>>();
                let header = eiyashou.then(|| {
                    let text_selected = mode == DocumentMode::Text;
                    let card_selected = mode == DocumentMode::Card;
                    let dialogue_selected = mode == DocumentMode::Dialogue;
                    div()
                        .h(px(34.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_1()
                        .px_2()
                        .bg(rgb(CHROME))
                        .child(div().flex().gap_1().children(palette_kinds.into_iter().map(
                            |kind| {
                                palette_button(kind.label()).on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.insert_from_palette(kind, window, cx)
                                    },
                                ))
                            },
                        )))
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .child(document_mode_button("Text", text_selected).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.document_mode = DocumentMode::Text;
                                        cx.notify();
                                    }),
                                ))
                                .child(document_mode_button("Cards", card_selected).on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.rebuild_card_editors(window, cx);
                                        this.document_mode = DocumentMode::Card;
                                        cx.notify();
                                    }),
                                ))
                                .child(
                                    document_mode_button("Dialogue", dialogue_selected).on_click(
                                        cx.listener(move |this, _, window, cx| {
                                            this.rebuild_card_editors(window, cx);
                                            this.document_mode = DocumentMode::Dialogue;
                                            cx.notify();
                                        }),
                                    ),
                                ),
                        )
                });
                let body = if eiyashou && mode == DocumentMode::Card {
                    render_card_projection(
                        root,
                        relative,
                        document.as_ref().unwrap(),
                        &self.card_editors,
                        cx,
                    )
                } else if eiyashou && mode == DocumentMode::Dialogue {
                    render_dialogue_projection(
                        root,
                        relative,
                        document.as_ref().unwrap(),
                        &self.dialogue_editors,
                        cx,
                    )
                } else {
                    Editor::new(editor)
                        .appearance(false)
                        .bordered(false)
                        .readonly(document.is_none())
                        .size_full()
                        .relative()
                        .left(px(-EDITOR_GUTTER_TRIM_PX))
                        .p_1()
                        .font_family(mono)
                        .text_sm()
                        .text_color(rgb(0xc4ced8))
                        .into_any_element()
                };
                div()
                    .id("document-content")
                    .size_full()
                    .flex()
                    .flex_col()
                    .rounded_b(px(VIEW_RADIUS_PX))
                    .bg(rgb(0x10151b))
                    .overflow_hidden()
                    .when_some(header, |this, header| this.child(header))
                    .child(div().flex_1().min_h_0().child(body))
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
                    .bg(rgb(0x10151b))
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
                            .bg(rgb(0x090c10))
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
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_3()
                    .child(section_label("WORKSPACE"))
                    .child(property_row("Path", root.display().to_string()))
                    .child(property_row("Text files", file_count.to_string()))
                    .child(property_row("Open documents", document_count.to_string()))
                    .child(section_label("SELECTION"))
                    .child(selection_summary(root, &index, cx))
                    .into_any_element()
            }
            PanelContent::Assets { root } => {
                render_assets(root, &cx.global::<EditorDocuments>().authoring(root))
            }
            PanelContent::Characters { root } => {
                let index = cx.global::<EditorDocuments>().authoring(root);
                let inputs = self.tool_inputs.clone();
                div()
                    .size_full()
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
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(index.characters.into_iter().enumerate().map(
                                |(row, character)| {
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
                                },
                            ))
                            .overflow_scrollbar()
                            .id("character-list"),
                    )
                    .into_any_element()
            }
            PanelContent::Scenes { root } => {
                let index = cx.global::<EditorDocuments>().authoring(root);
                let input = self.tool_inputs.first().cloned();
                let root_for_rows = root.clone();
                div()
                    .size_full()
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
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(index.scenes.into_iter().enumerate().map(|(row, scene)| {
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
                            }))
                            .overflow_scrollbar()
                            .id("scene-list"),
                    )
                    .into_any_element()
            }
            PanelContent::Problems { root } => render_problems(root, cx),
            PanelContent::Performance { controller, .. } => {
                render_performance(controller, &self.timeline)
            }
            PanelContent::Output { root, file_count } => div()
                .size_full()
                .flex()
                .flex_col()
                .p_3()
                .gap_2()
                .font_family(mono)
                .text_xs()
                .text_color(rgb(0xaebbc7))
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
                )
                .into_any_element(),
        };
        div()
            .track_focus(&self.focus)
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
        .child(div().text_xs().text_color(rgb(0xb8c4cf)).child(value))
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
        .id(match label {
            "Text" => "document-mode-text",
            "Cards" => "document-mode-card",
            _ => "document-mode-dialogue",
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

fn palette_button(label: &'static str) -> Stateful<Div> {
    div()
        .id(match label {
            "Narration" => "insert-narration",
            "Dialogue" => "insert-dialogue",
            "Background" => "insert-background",
            "Figure" => "insert-figure",
            _ => "insert-choice",
        })
        .h(px(25.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(rgb(PANEL))
        .text_xs()
        .text_color(rgb(0xafbac5))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)).text_color(rgb(PRIMARY)))
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

fn render_card_projection(
    root: &Path,
    relative: &Path,
    document: &DocumentHandle,
    editors: &[CardNameEditor],
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let projection = EiyashouProjection::parse(document.borrow().contents());
    let selected_line = cx
        .global::<EditorDocuments>()
        .selection(root)
        .filter(|(path, _, _)| path == relative)
        .map(|(_, line, _)| *line);
    let root = root.to_owned();
    let relative = relative.to_owned();
    let scene_cards = editors.iter().filter_map(|editor| {
        let scene = projection.scenes.get(editor.scene_index)?;
        let line = document
            .borrow()
            .contents()
            .get(..scene.name_range.start)
            .map(|prefix| prefix.bytes().filter(|byte| *byte == b'\n').count())
            .unwrap_or_default();
        let root = root.clone();
        let relative = relative.clone();
        Some(
            div()
                .id(("scene-card", editor.scene_index))
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .rounded(px(9.))
                .bg(rgb(if selected_line == Some(line) {
                    SURFACE
                } else {
                    PANEL
                }))
                .cursor_pointer()
                .on_click(cx.listener(move |_, _, _, cx| {
                    set_authoring_selection(&root, relative.clone(), line, 0, cx);
                }))
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(rgb(PRIMARY))
                        .child(format!("SCENE {}", editor.scene_index + 1)),
                )
                .child(
                    div()
                        .h(px(30.))
                        .rounded(px(7.))
                        .bg(rgb(SURFACE))
                        .px_2()
                        .child(
                            Input::new(&editor.state)
                                .appearance(false)
                                .bordered(false)
                                .size_full()
                                .text_sm()
                                .text_color(rgb(INK)),
                        ),
                )
                .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                    "Source bytes {}..{}",
                    scene.source_range.start, scene.source_range.end
                ))),
        )
    });
    let read_only_cards = projection
        .read_only
        .into_iter()
        .enumerate()
        .map(|(index, card)| {
            div()
                .id(("read-only-card", index))
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .rounded(px(9.))
                .bg(rgb(0x1f242a))
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(rgb(0xd2aa62))
                        .child("READ-ONLY SOURCE"),
                )
                .child(div().text_xs().text_color(rgb(MUTED)).child(card.message))
        });
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .children(scene_cards)
        .children(read_only_cards)
        .overflow_scrollbar()
        .id("eiyashou-card-view")
        .into_any_element()
}

fn render_dialogue_projection(
    root: &Path,
    relative: &Path,
    document: &DocumentHandle,
    editors: &[DialogueTextEditor],
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let dialogues = dialogues_for_source(relative, document.borrow().contents());
    let selected_line = cx
        .global::<EditorDocuments>()
        .selection(root)
        .filter(|(path, _, _)| path == relative)
        .map(|(_, line, _)| *line);
    let root = root.to_owned();
    let relative = relative.to_owned();
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .children(editors.iter().enumerate().filter_map(|(row, editor)| {
            let dialogue = dialogues
                .iter()
                .find(|dialogue| dialogue.line == editor.line)?;
            let root = root.clone();
            let relative = relative.clone();
            let line = dialogue.line.saturating_sub(1);
            Some(
                div()
                    .id(("dialogue-row", row))
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .rounded(px(8.))
                    .bg(rgb(if selected_line == Some(line) {
                        SURFACE
                    } else {
                        PANEL
                    }))
                    .cursor_pointer()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        set_authoring_selection(&root, relative.clone(), line, 0, cx);
                    }))
                    .child(
                        div()
                            .w(px(92.))
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(PRIMARY))
                            .child(if dialogue.speaker.is_empty() {
                                "Narration".to_owned()
                            } else {
                                dialogue.speaker.clone()
                            }),
                    )
                    .child(
                        div()
                            .h(px(30.))
                            .flex_1()
                            .rounded(px(7.))
                            .bg(rgb(SURFACE))
                            .px_2()
                            .child(
                                Input::new(&editor.state)
                                    .appearance(false)
                                    .bordered(false)
                                    .size_full()
                                    .text_sm()
                                    .text_color(rgb(INK)),
                            ),
                    ),
            )
        }))
        .when(editors.is_empty(), |this| {
            this.child(
                div()
                    .p_3()
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child("No editable dialogue in this source"),
            )
        })
        .overflow_scrollbar()
        .id("eiyashou-dialogue-view")
        .into_any_element()
}

fn render_assets(root: &Path, index: &AuthoringIndex) -> AnyElement {
    let root = root.to_owned();
    div()
        .size_full()
        .flex()
        .flex_col()
        .p_2()
        .gap_1()
        .child(section_label("ASSET BROWSER"))
        .child(
            div()
                .flex_1()
                .min_h_0()
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
                }))
                .overflow_scrollbar()
                .id("asset-list"),
        )
        .into_any_element()
}

fn render_problems(root: &Path, cx: &mut Context<WorkbenchPanel>) -> AnyElement {
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
    div()
        .size_full()
        .flex()
        .flex_col()
        .p_2()
        .gap_1()
        .child(section_label("PARSE · VALIDATION · RUNTIME"))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_1()
                .children(authoring_rows)
                .children(runtime_rows)
                .overflow_scrollbar()
                .id("problem-list"),
        )
        .into_any_element()
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
    div()
        .size_full()
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
                .bg(rgb(0x10151b))
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
        )
        .into_any_element()
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
                    theme_color(if selected { INK } else { 0x8794a2 }),
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
                                .text_color(rgb(0x7f8b98))
                                .hover(|style| style.bg(rgb(0x3a4651)).text_color(rgb(PRIMARY)))
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
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(7.))
                    .bg(if has_unsaved_changes {
                        rgb(0xdb7780)
                    } else {
                        rgb(PRIMARY)
                    })
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::BOLD)
                    .text_color(rgb(0x111a1e))
                    .child("K"),
            )
            .child(
                div()
                    .id("activity-explorer")
                    .size(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(7.))
                    .bg(rgb(SURFACE))
                    .child(
                        Icon::new(IconName::FileText)
                            .small()
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
                        .size(px(30.))
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
                                .small()
                                .text_color(rgb(if preview_open { PRIMARY } else { 0x84919d })),
                        ),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .id("activity-open-folder")
                    .size(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                    .on_click(move |_, _, cx| prompt_open_folder(editor.clone(), cx))
                    .child(
                        Icon::new(IconName::FolderOpen)
                            .small()
                            .text_color(rgb(0x98a3ae)),
                    ),
            )
            .when(self.workspace.is_some(), |this| {
                this.child(activity_divider())
                    .child(
                        div()
                            .id("activity-migrate-eiyashou")
                            .size(px(30.))
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
                                    .small()
                                    .text_color(rgb(0x74818e)),
                            ),
                    )
                    .child(
                        div()
                            .id("activity-reset-layout")
                            .size(px(30.))
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
                                    .small()
                                    .text_color(rgb(0x74818e)),
                            ),
                    )
            });

        div()
            .id("activity-rail")
            .w(px(42.))
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
                            .hover(|style| style.bg(rgb(0x3a4b55)))
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
                                                    .text_color(rgb(0xb9c5d0))
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
        .size(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .when(active, |style| style.bg(rgb(SURFACE)))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .child(
            Icon::new(icon)
                .small()
                .text_color(rgb(if active { PRIMARY } else { 0x84919d })),
        )
}

fn activity_divider() -> Div {
    div().w(px(18.)).h(px(1.)).my(px(1.)).bg(rgb(0x27313b))
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
}
