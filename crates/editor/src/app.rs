use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName as AssetIconName;
use gpui_kit::base::input::RopeExt as _;
use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::base::{InteractiveElementExt as _, Placement, ResizeHandleContext, ScrollbarMode};
use gpui_kit::component::dock::{
    AnyDrag, BasePanel, BasePanelView, DockArea, DockAreaRenderer, DockContext, DockEvent,
    DockLayout, DockPlacement, DockSkin, DragPanel, DropIndicator, DropPlaceholderBounds,
    InsertTarget, NodeId, Panel, PanelBuildContext, PanelEvent, PanelHandle, PanelId, PanelInfo,
    PanelState, PanelStyle, TabGroupContext, TabGroupRenderer, panel_handle, register_panel,
};
use gpui_kit::component::input::{
    Editor, EditorState, Input, InputEvent, InputState, Position, Textarea, TextareaState,
};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::select::{SelectEvent, SelectItem, SelectState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    Disableable as _, Icon, IconName, Root, Sizable as _, Theme, ThemeMode, WindowExt as _,
};
use gpui_kit::{
    Anchor, Animation, AnimationExt as _, AnyElement, AnyView, App, AppContext as _, Axis, Bounds,
    ClickEvent, ClipboardItem, Context, Div, DragMoveEvent, Element, Empty, Entity, EventEmitter,
    ExternalPaths, FocusHandle, Focusable, Global, Hsla, InteractiveElement, IntoElement,
    KeyBinding, MouseButton, MouseDownEvent, ParentElement, PathPromptOptions, Pixels, Point,
    PromptButton, PromptLevel, Render, ScrollAnchor, ScrollHandle, SharedString, Stateful, Styled,
    StyledImage as _, Subscription, WeakEntity, Window, WindowBounds, WindowHandle, WindowOptions,
    actions, anchored, canvas, deferred, div, ease_out_quint, fill, hsla, img, linear_color_stop,
    linear_gradient, prelude::*, px, radians, rgb, size,
};
use serde::{Deserialize, Serialize};

use crate::app_data::APP_ID;
use crate::authoring::{
    AssetKey, AssetKind, AssetQuery, AssetSort, AuthoringIndex, AuthoringSelection, InsertKind,
    ProblemSeverity, UnmappedAsset, append_character, append_scene, confined_existing_file,
    delete_scene, dialogues_for_source, escape_eiyashou_string, insert_statement,
    insertion_statement, move_scene, rename_scene, rename_scene_references, replace_dialogue_text,
    scene_references, valid_identifier,
};
use crate::document::{DocumentHandle, DocumentManager, SaveError, is_eiyashou_authoring_document};
use crate::file_ops::{self, ImportResult};
use crate::instance::{InstanceReceiver, PrimaryInstance, Startup, acquire_or_forward};
use crate::migration::MigrationPlan;
use crate::persistence::{AppPersistence, BlockPickerPreferences};
use crate::preview::{PreviewController, PreviewLifecycle};
use crate::project_key::ProjectKey;
use crate::projection::{
    BlockKind, EiyashouProjection, MoveDirection, SourceField, TextBlockMetadata, TextLifetime,
};
use crate::syntax::editor_highlighter_factory;
use crate::workspace::{WorkspaceEntryKind, WorkspaceFile, WorkspaceSession};

mod blocks;
mod completion;
mod dock;
mod edits;
mod files;
mod inspector;
#[path = "app/inspector/controls.rs"]
mod inspector_controls;
mod minimap;
mod render;
#[path = "app/resource/picker.rs"]
mod resource_picker;
mod search;
#[path = "app/text/minimap.rs"]
mod text_minimap;
mod view;
#[cfg(test)]
use dock::editor_drop_placement;
use dock::{ProjectWorkspace, install_default_layout};
use edits::*;
use files::FileHistory;
use inspector_controls::*;
use resource_picker::*;
use view::*;

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
const LAYOUT_SCHEMA: usize = 2;

gpui_kit::assets::icon_assets!(
    EditorExtraIcons,
    [
        Braces,
        Clipboard,
        Clock,
        Film,
        GitBranch,
        GripVertical,
        Image,
        Images,
        ListFilter,
        MessageSquarePlus,
        MessageSquareText,
        Move,
        Music,
        PersonStanding,
        Repeat2,
        Scissors,
        SlidersHorizontal,
        Square,
        Volume2,
        Workflow,
    ]
);

struct EditorAssets;

impl gpui_kit::AssetSource for EditorAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        gpui_kit::AssetSource::load(&EditorExtraIcons, path).and_then(|extra| {
            extra.map_or_else(
                || gpui_kit::AssetSource::load(&gpui_kit::assets::Assets, path),
                |bytes| Ok(Some(bytes)),
            )
        })
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut paths = gpui_kit::AssetSource::list(&gpui_kit::assets::Assets, path)?;
        paths.extend(gpui_kit::AssetSource::list(&EditorExtraIcons, path)?);
        Ok(paths)
    }
}
// Each Dock group owns one half-gap. Adjacent groups therefore have a 4 px
// gutter, while the window and activity rail contribute the matching other
// half at the workspace edge. Keep this as the single spacing owner instead
// of adding per-panel margins.
const VIEW_INSET_PX: f32 = 2.;
const VIEW_RADIUS_PX: f32 = 14.;
const ACTIVITY_RAIL_WIDTH_PX: f32 = 44.;
const ACTIVITY_ITEM_SIZE_PX: f32 = 34.;
const ACTIVITY_BRAND_SIZE_PX: f32 = 32.;
const ACTIVITY_ICON_SIZE_PX: f32 = 18.;
// GPUI reserves one leading digit plus input padding before the widest visible
// line number. Crop that reserve while keeping the built-in right margin as a
// distinct dark gap before source text.
const EDITOR_GUTTER_TRIM_PX: f32 = 18.;
const TAB_MOTION_DURATION: Duration = Duration::from_millis(140);
const PREVIEW_SOURCE_DEBOUNCE: Duration = Duration::from_millis(100);
const FILE_CONTEXT_MENU_WIDTH_PX: f32 = 144.;
const SCENE_CONTEXT_MENU_WIDTH_PX: f32 = 154.;
const ASSET_FILTER_MENU_WIDTH_PX: f32 = 240.;
const SCENE_CONTEXT_MENU_HEIGHT_PX: f32 = 143.;

const EXPLORER_PANEL: &str = "keine.editor.explorer";
const SEARCH_PANEL: &str = "keine.editor.search";
const DOCUMENT_PANEL: &str = "keine.editor.document";
const INSPECTOR_PANEL: &str = "keine.editor.inspector";
const OUTPUT_PANEL: &str = "keine.editor.output";
const ASSETS_PANEL: &str = "keine.editor.assets";
const ASSET_PREVIEW_PANEL: &str = "keine.editor.asset_preview";
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
        ShowAssets,
        ShowSearch,
        ShowAssetPreview,
        MigrateEiyashou,
        CopyBlocks,
        PasteBlocks,
        DeleteBlocks,
        BeginTextBlock,
        ToggleBlockPicker,
        BlockPickerNext,
        BlockPickerPrevious,
        BlockPickerLeft,
        BlockPickerRight,
        AcceptBlockPicker,
        CloseBlockPicker,
        ResourcePickerNext,
        ResourcePickerPrevious,
        AcceptResourcePicker,
        CloseResourcePicker,
        MoveBlocksUp,
        MoveBlocksDown,
        UndoBlocks,
        RedoBlocks,
        UndoFiles,
        RedoFiles,
        UndoSources,
        RedoSources,
        ReloadDocument
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
    theme.radius = px(9.);
    theme.radius_lg = px(VIEW_RADIUS_PX);
    theme.shadow = false;
    Theme::sync_base(cx);
    // Dock edges retain their resize hit targets without painting a divider.
    let resize_theme = &mut gpui_kit::base::Theme::global_mut(cx).resizable;
    resize_theme.handle = Some(hsla(0., 0., 0., 0.));
    resize_theme.active_handle = Some(hsla(0., 0., 0., 0.));
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
    authoring: Arc<AuthoringIndex>,
    index_epoch: u64,
    pending_index: BTreeMap<PathBuf, String>,
    index_task: Option<gpui_kit::Task<()>>,
    force_index_reload: bool,
    notice: String,
    selection: Option<(PathBuf, usize, usize)>,
    block_selection: Option<(PathBuf, Vec<usize>)>,
    asset_selection: Vec<AssetKey>,
    asset_preview: Option<AssetPreviewSelection>,
    diagnostics: Vec<keine_authoring::Diagnostic>,
    source_history: SourceHistory,
    dock: Option<WeakEntity<DockArea>>,
    document_node: Option<NodeId>,
    panels: HashMap<PathBuf, PanelId>,
    panel_entities: HashMap<PathBuf, WeakEntity<WorkbenchPanel>>,
    editors: HashMap<PathBuf, WeakEntity<EditorState>>,
    preview: Arc<PreviewController>,
    tools: HashMap<&'static str, PanelId>,
    file_operation_active: bool,
}

fn schedule_authoring_refresh(root: &Path, path: Option<&Path>, cx: &mut App) {
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
        background.timer(Duration::from_millis(60)).await;
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
struct AssetPreviewSelection {
    kind: AssetKind,
    label: String,
    path: PathBuf,
    file: Option<PathBuf>,
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

struct EditorDocuments {
    persistence: AppPersistence,
    block_picker_preferences: BlockPickerPreferences,
    workspaces: HashMap<PathBuf, WorkspaceDocuments>,
    resource_picker_epoch: u64,
}

impl Global for EditorDocuments {}

impl EditorDocuments {
    fn new(persistence: AppPersistence) -> Self {
        let block_picker_preferences = persistence.load_block_picker_preferences();
        Self {
            persistence,
            block_picker_preferences,
            workspaces: HashMap::new(),
            resource_picker_epoch: 0,
        }
    }

    fn workspace_mut(&mut self, root: &Path) -> io::Result<&mut WorkspaceDocuments> {
        self.workspaces
            .get_mut(root)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Project is closed"))
    }

    fn ensure_workspace_with_files(
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

    fn open(&mut self, root: &Path, relative: &Path) -> io::Result<DocumentHandle> {
        self.workspace_mut(root)?.manager.open(relative)
    }

    fn has_dirty_documents(&self, root: &Path) -> bool {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .is_some_and(|workspace| workspace.manager.has_dirty_documents())
    }

    fn save_all(&mut self, root: &Path) -> Result<usize, SaveError> {
        let workspace = self.workspace_mut(root).map_err(SaveError::from)?;
        if workspace.file_operation_active {
            return Err(io::Error::other(
                "A file operation is still running; save when it finishes",
            )
            .into());
        }
        workspace.manager.save_all()
    }

    fn set_notice(&mut self, root: &Path, notice: impl Into<String>) {
        if let Ok(workspace) = self.workspace_mut(root) {
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
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.selection = Some((relative, line, column));
        }
    }

    fn set_block_selection(&mut self, root: &Path, relative: PathBuf, mut starts: Vec<usize>) {
        starts.sort_unstable();
        starts.dedup();
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.block_selection = (!starts.is_empty()).then_some((relative, starts));
            workspace.asset_selection.clear();
            workspace.asset_preview = None;
            workspace.preview.stop_audition();
        }
    }

    fn clear_block_selection(&mut self, root: &Path) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.block_selection = None;
        }
    }

    fn asset_selection(&self, root: &Path) -> Vec<AssetKey> {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .map(|workspace| workspace.asset_selection.clone())
            .unwrap_or_default()
    }

    fn set_asset_selection(&mut self, root: &Path, selection: Vec<AssetKey>) {
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

    fn preview_asset(&mut self, root: &Path, kind: AssetKind, label: String, path: PathBuf) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.asset_preview = Some(asset_preview_selection(root, kind, label, path));
        }
    }

    fn set_unmapped_asset_preview(&mut self, root: &Path, asset: &UnmappedAsset) {
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

    fn asset_preview(&self, root: &Path) -> Option<AssetPreviewSelection> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces.get(key.path())?.asset_preview.clone()
    }

    fn clear_asset_selection(&mut self, root: &Path) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.asset_selection.clear();
            workspace.asset_preview = None;
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

    fn asset_manifest_is_clean(&mut self, root: &Path) -> bool {
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

    fn has_open_documents_under(&mut self, root: &Path, path: &Path) -> bool {
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

    fn adopt_manifest_update(
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

    fn authoring(&self, root: &Path) -> Arc<AuthoringIndex> {
        ProjectKey::from_path(root)
            .ok()
            .and_then(|key| self.workspaces.get(key.path()))
            .map(|workspace| workspace.authoring.clone())
            .unwrap_or_default()
    }

    fn authoring_ref(&self, root: &Path) -> Option<&AuthoringIndex> {
        let key = ProjectKey::from_path(root).ok()?;
        Some(&self.workspaces.get(key.path())?.authoring)
    }

    fn authoring_is_current(&self, root: &Path) -> bool {
        self.workspaces.get(root).is_some_and(|workspace| {
            workspace.pending_index.is_empty() && !workspace.force_index_reload
        })
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
            .document(relative)
            .map(|document| document.borrow().contents().to_owned())
            .or_else(|| fs::read_to_string(root.join(relative)).ok())
    }

    fn projection(&self, root: &Path, relative: &Path, source: &str) -> Rc<EiyashouProjection> {
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

    fn selection(&self, root: &Path) -> Option<&(PathBuf, usize, usize)> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces.get(key.path())?.selection.as_ref()
    }

    fn set_diagnostics(&mut self, root: &Path, diagnostics: Vec<keine_authoring::Diagnostic>) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.diagnostics = diagnostics;
        }
    }

    fn set_dock(&mut self, root: &Path, dock: WeakEntity<DockArea>) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.dock = Some(dock);
        }
    }

    fn set_document_node(&mut self, root: &Path, node: NodeId) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.document_node = Some(node);
        }
    }

    fn clear_document_node(&mut self, root: &Path) {
        if let Some(workspace) = self.workspaces.get_mut(root) {
            workspace.document_node = None;
        }
    }

    fn register_panel(
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

    fn panel_entity_for(&self, root: &Path, relative: &Path) -> Option<WeakEntity<WorkbenchPanel>> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces
            .get(key.path())?
            .panel_entities
            .get(relative)
            .cloned()
    }

    fn register_editor(&mut self, root: &Path, relative: PathBuf, editor: WeakEntity<EditorState>) {
        if let Ok(workspace) = self.workspace_mut(root) {
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
        if let Some(workspace) = self.workspaces.get_mut(root)
            && workspace.panels.get(relative) == Some(&panel)
        {
            workspace.panels.remove(relative);
            workspace.panel_entities.remove(relative);
            workspace.editors.remove(relative);
            workspace.manager.release_clean(relative);
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
        self.workspaces
            .get(root)
            .map(|workspace| workspace.preview.clone())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "project session is closed"))
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

    fn tool_panel(&self, root: &Path, name: &'static str) -> Option<PanelId> {
        let key = ProjectKey::from_path(root).ok()?;
        self.workspaces.get(key.path())?.tools.get(name).copied()
    }

    fn set_tool_panel(&mut self, root: &Path, name: &'static str, panel: Option<PanelId>) {
        if let Ok(workspace) = self.workspace_mut(root) {
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
    windows: WindowRegistry<WindowHandle<Root>>,
    empty_window: Option<WindowHandle<Root>>,
    persistence: AppPersistence,
    _instance: PrimaryInstance,
}

struct EditorAppOwner {
    _editor: Entity<EditorApp>,
}

impl Global for EditorAppOwner {}

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
        let options = window_options(0, cx, &persistence);
        let initial_windowed_bounds = options.window_bounds.unwrap().get_bounds();
        let handle = cx
            .open_window(options, move |window, cx| {
                window.set_window_title("Kēne Editor");
                let workbench = cx.new(|cx| {
                    WorkbenchWindow::empty(
                        editor,
                        persistence,
                        recents,
                        initial_windowed_bounds,
                        window,
                        cx,
                    )
                });
                cx.new(|cx| Root::new(workbench, window, cx))
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
        cx.global_mut::<EditorDocuments>()
            .ensure_workspace_with_files(session.root(), session.files())?;
        let persistence = self.persistence.clone();
        let editor = self.this.clone();
        let handle = if let Some(empty) = self.empty_window.take() {
            let session_for_window = session.clone();
            empty
                .update(cx, |root, window, cx| {
                    root.view()
                        .clone()
                        .downcast::<WorkbenchWindow>()
                        .expect("Kēne Editor root must contain the workbench")
                        .update(cx, |workbench, cx| {
                            workbench.open_session(session_for_window, window, cx)
                        })?;
                    window.activate_window();
                    Ok::<(), io::Error>(())
                })
                .map_err(io::Error::other)??;
            empty
        } else {
            let index = self.windows.windows.len();
            let options = window_options(index, cx, &persistence);
            let initial_windowed_bounds = options.window_bounds.unwrap().get_bounds();
            cx.open_window(options, move |window, cx| {
                let workbench = cx.new(|cx| {
                    WorkbenchWindow::project(
                        editor,
                        persistence,
                        session,
                        initial_windowed_bounds,
                        window,
                        cx,
                    )
                });
                cx.new(|cx| Root::new(workbench, window, cx))
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
    Explorer {
        root: PathBuf,
        #[serde(default)]
        expanded: Vec<PathBuf>,
    },
    Document {
        root: PathBuf,
        relative: PathBuf,
    },
    Inspector {
        root: PathBuf,
    },
    Output {
        root: PathBuf,
    },
    Search {
        root: PathBuf,
    },
    Assets {
        root: PathBuf,
    },
    AssetPreview {
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
    },
}

#[derive(Clone, Copy)]
enum ToolKind {
    Explorer,
    Search,
    Assets,
    Characters,
    Problems,
    Performance,
}

impl ToolKind {
    fn panel_name(self) -> &'static str {
        match self {
            Self::Explorer => EXPLORER_PANEL,
            Self::Search => SEARCH_PANEL,
            Self::Assets => ASSETS_PANEL,
            Self::Characters => CHARACTERS_PANEL,
            Self::Problems => PROBLEMS_PANEL,
            Self::Performance => PERFORMANCE_PANEL,
        }
    }

    fn payload(self, root: PathBuf) -> PanelPayload {
        match self {
            Self::Explorer => PanelPayload::Explorer {
                root,
                expanded: Vec::new(),
            },
            Self::Search => PanelPayload::Search { root },
            Self::Assets => PanelPayload::Assets { root },
            Self::Characters => PanelPayload::Characters { root },
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
        expanded: HashSet<PathBuf>,
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
    Search {
        root: PathBuf,
    },
    Assets {
        root: PathBuf,
    },
    AssetPreview {
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
            PanelPayload::Explorer { root, expanded } => {
                let files = cx
                    .global_mut::<EditorDocuments>()
                    .workspace_mut(&root)?
                    .files
                    .clone();
                let directories = files
                    .iter()
                    .filter(|file| file.is_dir())
                    .map(|file| &file.relative_path)
                    .collect::<HashSet<_>>();
                let expanded = expanded
                    .into_iter()
                    .filter(|path| directories.contains(path))
                    .collect();
                Ok(Self::Explorer {
                    root,
                    files,
                    expanded,
                })
            }
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
                        .folding(false)
                        .auto_close(true)
                        .smart_indent(true)
                        .tab_size(gpui_kit::base::input::TabSize {
                            tab_size: 2,
                            hard_tabs: false,
                        });
                    editor.set_highlighter_factory(editor_highlighter_factory(), cx);
                    if document.is_some() && language == "eiyashou" {
                        editor.lsp_mut().completion_provider =
                            Some(Rc::new(completion::ShouCompletion {
                                root: root.clone(),
                                relative: relative.clone(),
                            }));
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
            PanelPayload::Inspector { root } => {
                let file_count = cx
                    .global_mut::<EditorDocuments>()
                    .workspace_mut(&root)?
                    .files
                    .len();
                Ok(Self::Inspector { root, file_count })
            }
            PanelPayload::Output { root } if root.as_os_str().is_empty() => Ok(Self::Output {
                root,
                file_count: 0,
            }),
            PanelPayload::Output { root } => {
                let file_count = cx
                    .global_mut::<EditorDocuments>()
                    .workspace_mut(&root)?
                    .files
                    .len();
                Ok(Self::Output { root, file_count })
            }
            PanelPayload::Search { root } => Ok(Self::Search { root }),
            PanelPayload::Assets { root } => Ok(Self::Assets { root }),
            PanelPayload::AssetPreview { root } => Ok(Self::AssetPreview { root }),
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
            Self::Explorer { root, expanded, .. } => {
                let mut expanded = expanded.iter().cloned().collect::<Vec<_>>();
                expanded.sort();
                PanelPayload::Explorer {
                    root: root.clone(),
                    expanded,
                }
            }
            Self::Document { root, relative, .. } => PanelPayload::Document {
                root: root.clone(),
                relative: relative.clone(),
            },
            Self::Inspector { root, .. } => PanelPayload::Inspector { root: root.clone() },
            Self::Output { root, .. } => PanelPayload::Output { root: root.clone() },
            Self::Search { root } => PanelPayload::Search { root: root.clone() },
            Self::Assets { root } => PanelPayload::Assets { root: root.clone() },
            Self::AssetPreview { root } => PanelPayload::AssetPreview { root: root.clone() },
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
            Self::Search { .. } => SEARCH_PANEL,
            Self::Assets { .. } => ASSETS_PANEL,
            Self::AssetPreview { .. } => ASSET_PREVIEW_PANEL,
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
            Self::Search { .. } => "Search".into(),
            Self::Assets { .. } => "Assets".into(),
            Self::AssetPreview { .. } => "Asset Preview".into(),
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
    last_text_cursor: Option<(usize, usize)>,
    block_text_editors: Vec<BlockTextEditor>,
    collapsed_scenes: HashSet<String>,
    selected_blocks: HashSet<usize>,
    block_selection_anchor: Option<usize>,
    draft_text: Option<DraftTextBlock>,
    block_drop_target: Option<BlockDropTarget>,
    block_dragging: Option<HashSet<usize>>,
    block_settle_source: Option<String>,
    block_picker_open: bool,
    block_picker_index: usize,
    block_picker_category: Option<&'static str>,
    block_picker_customize: bool,
    block_picker_input: Entity<InputState>,
    block_insertion_target: Option<DraftInsertionTarget>,
    block_context_menu: Option<(usize, Point<Pixels>, String)>,
    view_scroll: ScrollHandle,
    block_scroll_anchor: ScrollAnchor,
    block_scroll_pending: bool,
    block_minimap: minimap::BlockMinimap,
    text_minimap: text_minimap::TextMinimap,
    project_search: Option<search::ProjectSearch>,
    tool_inputs: Vec<Entity<InputState>>,
    recovery_epoch: u64,
    syntax_check: Option<gpui_kit::Task<()>>,
    syntax_marks: Option<gpui_kit::base::input::TextDecorationCollection>,
    _subscriptions: Vec<Subscription>,
    visual_subscriptions: Vec<Subscription>,
    inspector_key: Option<InspectorEditKey>,
    inspector_inputs: Vec<Entity<InputState>>,
    text_lifetime_inputs: Vec<Entity<InputState>>,
    inspector_selects: Vec<Entity<SelectState<Vec<SourceOption>>>>,
    inspector_subscriptions: Vec<Subscription>,
    inline_block_controls: HashMap<usize, InlineBlockControl>,
    block_visible: HashSet<usize>,
    block_heights: HashMap<usize, f32>,
    block_height_revision: u64,
    resource_picker: Option<ResourcePicker>,
    source_inspector_key: Option<SourceInspectorKey>,
    source_inspector_inputs: Vec<Entity<InputState>>,
    source_inspector_texts: Vec<Entity<TextareaState>>,
    source_inspector_sliders: HashMap<String, Entity<SliderState>>,
    source_inspector_selects: HashMap<String, Entity<SelectState<Vec<SourceOption>>>>,
    source_inspector_subscriptions: Vec<Subscription>,
    source_inspector_effect: Option<&'static str>,
    source_position_bounds: Rc<RefCell<Bounds<Pixels>>>,
    source_position_draft: Option<(usize, f32, f32)>,
    asset_inspector_key: Option<(AssetKey, PathBuf, Vec<String>)>,
    asset_inspector_inputs: Vec<Entity<InputState>>,
    asset_inspector_subscriptions: Vec<Subscription>,
    file_selection: Option<PathBuf>,
    file_drop_target: Option<(PathBuf, Bounds<Pixels>)>,
    file_clipboard: Option<PathBuf>,
    file_history: FileHistory,
    file_edit: Option<FileEditMode>,
    file_name_input: Entity<InputState>,
    file_commit_requested: bool,
    file_progress: Option<FileProgress>,
    file_context_menu: Option<FileContextMenu>,
    file_context_epoch: u64,
    scene_edit: Option<SceneEditMode>,
    scene_name_input: Entity<InputState>,
    scene_commit_requested: bool,
    scene_context_menu: Option<SceneContextMenu>,
    scene_context_epoch: u64,
    asset_search: Entity<InputState>,
    asset_kind: Option<AssetKind>,
    asset_folder: Option<PathBuf>,
    asset_sort: AssetSort,
    asset_grid: bool,
    asset_unmapped: bool,
    asset_anchor: Option<AssetKey>,
    asset_filter_menu: Option<AssetFilterMenu>,
    asset_filter_epoch: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DocumentMode {
    #[default]
    Text,
    Block,
}

struct BlockTextEditor {
    text_start: usize,
    state: Entity<TextareaState>,
    wait: Option<InlineWaitEdit>,
    _subscription: Subscription,
}

struct InlineWaitEdit {
    ordinal: usize,
    input: Entity<InputState>,
    _subscription: Subscription,
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
    path: Option<PathBuf>,
    position: Point<Pixels>,
    epoch: u64,
    closing: bool,
}

#[derive(Clone, Debug)]
enum SceneEditMode {
    New,
    Rename { start: usize, old_name: String },
}

#[derive(Clone, Debug)]
struct SceneContextMenu {
    start: usize,
    name: String,
    position: Point<Pixels>,
    epoch: u64,
    closing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AssetFilterGroup {
    Type,
    Folder,
    Sort,
    View,
    Show,
}

#[derive(Clone, Debug)]
enum AssetFilterChoice {
    Type(Option<AssetKind>),
    Folder(Option<PathBuf>),
    Sort(AssetSort),
    View(bool),
    Show(bool),
}

#[derive(Clone, Debug)]
struct AssetFilterMenu {
    position: Point<Pixels>,
    epoch: u64,
    closing: bool,
    expanded: Option<AssetFilterGroup>,
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
            .text_size(px(11.))
            .text_color(rgb(INK))
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
    lifetime: TextLifetime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceInspectorKey {
    path: PathBuf,
    block_start: usize,
    kind: BlockKind,
    command: String,
    fields: Vec<SourceField>,
}

#[derive(Clone)]
struct BlockDrag {
    selected: HashSet<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BlockDropTarget {
    row: usize,
    after: bool,
}

struct BlockDragPreview {
    label: String,
    summary: String,
    icon: AssetIconName,
    count: usize,
    width: f32,
    height: f32,
}

impl Render for BlockDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.width))
            .h(px(self.height))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .rounded(px(7.))
            .bg(rgb(SURFACE_HOVER))
            .text_sm()
            .text_color(rgb(INK))
            .child(Icon::new(AssetIconName::GripVertical).xsmall())
            .child(Icon::new(self.icon).xsmall())
            .child(
                div()
                    .w(px(64.))
                    .flex_none()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_xs()
                    .text_color(rgb(PRIMARY))
                    .child(self.label.clone()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .whitespace_normal()
                    .child(if self.count == 1 {
                        self.summary.clone()
                    } else {
                        format!("{} blocks", self.count)
                    }),
            )
    }
}

#[derive(Clone)]
struct AssetDrag {
    root: PathBuf,
    keys: Vec<AssetKey>,
}

impl Render for AssetDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(px(5.))
            .bg(rgb(SURFACE))
            .text_xs()
            .child(if self.keys.len() == 1 {
                self.keys[0].id.clone()
            } else {
                format!("{} assets", self.keys.len())
            })
    }
}

struct DraftTextBlock {
    target: DraftInsertionTarget,
    text_range: Option<Range<usize>>,
    last_escaped: String,
    state: Entity<TextareaState>,
}

#[derive(Clone, Copy)]
enum DraftInsertionTarget {
    Before(usize),
    After(usize),
    SceneEnd(usize),
}

#[derive(Clone, Copy)]
enum BlockMenuAction {
    Run,
    Copy,
    Duplicate,
    Cut,
    Paste,
    SelectAll,
    ToggleDisabled,
    InsertAbove,
    InsertBelow,
    MoveUp,
    MoveDown,
    Delete,
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
        let tool_registration = match &content {
            PanelContent::Explorer { root, .. } => Some((root.clone(), EXPLORER_PANEL)),
            PanelContent::Inspector { root, .. } => Some((root.clone(), INSPECTOR_PANEL)),
            PanelContent::Search { root } => Some((root.clone(), SEARCH_PANEL)),
            PanelContent::Assets { root } => Some((root.clone(), ASSETS_PANEL)),
            PanelContent::AssetPreview { root } => Some((root.clone(), ASSET_PREVIEW_PANEL)),
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
            let scene_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Scene"));
            let asset_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
            let view_scroll = ScrollHandle::new();
            let block_scroll_anchor = ScrollAnchor::for_handle(view_scroll.clone());
            let syntax_marks = match &content {
                PanelContent::Document {
                    relative,
                    document: Some(_),
                    editor,
                    ..
                } if relative
                    .extension()
                    .is_some_and(|extension| extension == "shou") =>
                {
                    Some(editor.update(cx, |editor, cx| {
                        editor.create_decorations_collection(Vec::new(), cx)
                    }))
                }
                _ => None,
            };
            let mut panel = Self {
                content,
                focus: cx.focus_handle(),
                document_mode: DocumentMode::Text,
                last_text_cursor: None,
                block_text_editors: Vec::new(),
                collapsed_scenes: HashSet::new(),
                selected_blocks: HashSet::new(),
                block_selection_anchor: None,
                draft_text: None,
                block_drop_target: None,
                block_dragging: None,
                block_settle_source: None,
                block_picker_open: false,
                block_picker_index: 0,
                block_picker_category: None,
                block_picker_customize: false,
                block_picker_input: block_picker_input.clone(),
                block_insertion_target: None,
                block_context_menu: None,
                view_scroll,
                block_scroll_anchor,
                block_scroll_pending: false,
                block_minimap: minimap::BlockMinimap::default(),
                text_minimap: text_minimap::TextMinimap::default(),
                project_search: None,
                tool_inputs: Vec::new(),
                recovery_epoch: 0,
                syntax_check: None,
                syntax_marks,
                _subscriptions: Vec::new(),
                visual_subscriptions: Vec::new(),
                inspector_key: None,
                inspector_inputs: Vec::new(),
                text_lifetime_inputs: Vec::new(),
                inspector_selects: Vec::new(),
                inspector_subscriptions: Vec::new(),
                inline_block_controls: HashMap::new(),
                block_visible: HashSet::new(),
                block_heights: HashMap::new(),
                block_height_revision: 0,
                resource_picker: None,
                source_inspector_key: None,
                source_inspector_inputs: Vec::new(),
                source_inspector_texts: Vec::new(),
                source_inspector_sliders: HashMap::new(),
                source_inspector_selects: HashMap::new(),
                source_inspector_subscriptions: Vec::new(),
                source_inspector_effect: None,
                source_position_bounds: Rc::new(RefCell::new(Bounds::default())),
                source_position_draft: None,
                asset_inspector_key: None,
                asset_inspector_inputs: Vec::new(),
                asset_inspector_subscriptions: Vec::new(),
                file_selection: None,
                file_drop_target: None,
                file_clipboard: None,
                file_history: FileHistory::default(),
                file_edit: None,
                file_name_input: file_name_input.clone(),
                file_commit_requested: false,
                file_progress: None,
                file_context_menu: None,
                file_context_epoch: 0,
                scene_edit: None,
                scene_name_input: scene_name_input.clone(),
                scene_commit_requested: false,
                scene_context_menu: None,
                scene_context_epoch: 0,
                asset_search: asset_search.clone(),
                asset_kind: None,
                asset_folder: None,
                asset_sort: AssetSort::Name,
                asset_grid: false,
                asset_unmapped: false,
                asset_anchor: None,
                asset_filter_menu: None,
                asset_filter_epoch: 0,
            };
            if matches!(panel.content, PanelContent::Search { .. }) {
                panel.install_search(window, cx);
            }
            panel._subscriptions.push(cx.subscribe(
                &asset_search,
                |_: &mut WorkbenchPanel, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            ));
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
            panel._subscriptions.push(cx.subscribe(
                &scene_name_input,
                |panel: &mut WorkbenchPanel, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        panel.scene_commit_requested = true;
                        cx.notify();
                    }
                },
            ));
            if let PanelContent::Document { editor, .. } = &panel.content {
                panel._subscriptions.push(cx.subscribe(
                    editor,
                    |panel, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            panel.text_minimap.invalidate();
                            cx.notify();
                        }
                    },
                ));
            }
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
                    .push(cx.observe(editor, move |panel, _, cx| {
                        // Blocks mode owns the source selection; the hidden
                        // text editor's stale caret must not seek Preview.
                        if panel.document_mode == DocumentMode::Block {
                            return;
                        }
                        let position = editor_for_selection.read(cx).cursor_position();
                        let cursor = (position.line as usize, position.character as usize);
                        if panel.last_text_cursor == Some(cursor) {
                            return;
                        }
                        panel.last_text_cursor = Some(cursor);
                        cx.global_mut::<EditorDocuments>().set_selection(
                            &root_for_selection,
                            relative_for_selection.clone(),
                            cursor.0,
                            cursor.1,
                        );
                        cx.global_mut::<EditorDocuments>()
                            .clear_block_selection(&root_for_selection);
                        let disabled = cx
                            .global::<EditorDocuments>()
                            .source(&root_for_selection, &relative_for_selection)
                            .and_then(|source| {
                                let projection = cx.global::<EditorDocuments>().projection(
                                    &root_for_selection,
                                    &relative_for_selection,
                                    &source,
                                );
                                block_at_position(&projection, &source, cursor.0, cursor.1)
                            })
                            .is_some_and(|(_, block)| block.disabled);
                        if !disabled
                            && let Ok(preview) = cx
                                .global_mut::<EditorDocuments>()
                                .preview(&root_for_selection)
                        {
                            preview.set_cursor(
                                relative_for_selection.clone(),
                                cursor.0 + 1,
                                cursor.1 + 1,
                            );
                        }
                        cx.refresh_windows();
                    }));
                if let Some(document) = document {
                    if relative
                        .extension()
                        .is_some_and(|extension| extension == "shou")
                    {
                        panel.syntax_check = Some(completion::schedule_syntax_check(
                            editor.clone(),
                            panel.syntax_marks.clone(),
                            window,
                            cx,
                        ));
                    }
                    let document_for_change = document.clone();
                    let syntax_window = window.window_handle();
                    let change_subscription = cx.subscribe(
                        editor,
                        move |panel: &mut WorkbenchPanel, editor, event: &InputEvent, cx| {
                            if !matches!(event, InputEvent::Change) {
                                return;
                            }
                            let editor_entity = editor.clone();
                            let editor = editor.read(cx);
                            let contents = editor.value().to_string();
                            let position = editor.cursor_position();
                            let result =
                                document_for_change.borrow_mut().replace_contents(contents);
                            let changed = match result {
                                Ok(changed) => changed,
                                Err(error) => {
                                    let previous =
                                        document_for_change.borrow().contents().to_owned();
                                    let message = error.to_string();
                                    cx.defer(move |cx| {
                                        let _ = cx.update_window(syntax_window, |_, window, cx| {
                                            editor_entity.update(cx, |editor, cx| {
                                                editor.replace_all(previous, window, cx)
                                            });
                                            window.push_notification(
                                                Notification::warning(message),
                                                cx,
                                            );
                                        });
                                    });
                                    return;
                                }
                            };
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
                                if relative
                                    .extension()
                                    .is_some_and(|extension| extension == "shou")
                                {
                                    let window_handle = syntax_window;
                                    let panel_entity = cx.weak_entity();
                                    cx.defer(move |cx| {
                                        let _ = cx.update_window(window_handle, |_, window, cx| {
                                            let _ = panel_entity.update(cx, |panel, cx| {
                                                panel.syntax_check =
                                                    Some(completion::schedule_syntax_check(
                                                        editor_entity,
                                                        panel.syntax_marks.clone(),
                                                        window,
                                                        cx,
                                                    ));
                                            });
                                        });
                                    });
                                }
                                schedule_authoring_refresh(&root, Some(&relative), cx);
                                panel.recovery_epoch = panel.recovery_epoch.wrapping_add(1);
                                let epoch = panel.recovery_epoch;
                                let document = document_for_change.clone();
                                let root = root.clone();
                                if !document.borrow().is_dirty() {
                                    let cleanup = document.borrow().recovery_write();
                                    cx.background_executor()
                                        .spawn(async move {
                                            if let Ok(write) = cleanup {
                                                let _ = write.execute();
                                            }
                                        })
                                        .detach();
                                }
                                let any_dirty =
                                    cx.global::<EditorDocuments>().has_dirty_documents(&root);
                                let notice = if !any_dirty {
                                    "Ready"
                                } else {
                                    "Unsaved changes"
                                }
                                .to_owned();
                                cx.global_mut::<EditorDocuments>().set_notice(&root, notice);
                                if relative
                                    .extension()
                                    .and_then(|extension| extension.to_str())
                                    == Some("shou")
                                {
                                    let preview_root = root.clone();
                                    let preview_relative = relative.clone();
                                    let preview_document = document.clone();
                                    cx.spawn(async move |panel, cx| {
                                        cx.background_executor()
                                            .timer(PREVIEW_SOURCE_DEBOUNCE)
                                            .await;
                                        let _ = panel.update(cx, |panel, cx| {
                                            if panel.recovery_epoch == epoch
                                                && let Ok(preview) = cx
                                                    .global_mut::<EditorDocuments>()
                                                    .preview(&preview_root)
                                            {
                                                preview.apply_snapshot(
                                                    preview_relative,
                                                    preview_document
                                                        .borrow()
                                                        .contents()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                            }
                                        });
                                    })
                                    .detach();
                                }
                                cx.spawn(async move |panel, cx| {
                                    cx.background_executor()
                                        .timer(Duration::from_millis(350))
                                        .await;
                                    let _ = panel.update(cx, |panel, cx| {
                                        if panel.recovery_epoch == epoch {
                                            let clean = !document.borrow().is_dirty();
                                            let recovery = document.borrow().recovery_write();
                                            let revision = document.borrow().revision();
                                            let write_root = root.clone();
                                            let background = cx.background_executor().clone();
                                            cx.spawn(async move |panel, cx| {
                                                let recovery = match recovery {
                                                    Ok(write) => {
                                                        background
                                                            .spawn(async move { write.execute() })
                                                            .await
                                                    }
                                                    Err(error) => Err(error),
                                                };
                                                let _ = panel.update(cx, |panel, cx| {
                                                    if panel.recovery_epoch != epoch
                                                        || document.borrow().revision() != revision
                                                    {
                                                        return;
                                                    }
                                                    let any_dirty = cx
                                                        .global::<EditorDocuments>()
                                                        .has_dirty_documents(&write_root);
                                                    let notice = match recovery {
                                                        Ok(()) if !any_dirty => "Ready".to_owned(),
                                                        Ok(()) if clean => {
                                                            "Unsaved changes".to_owned()
                                                        }
                                                        Ok(()) => "Recovery draft saved".to_owned(),
                                                        Err(error) => format!(
                                                            "Recovery draft failed: {error}"
                                                        ),
                                                    };
                                                    cx.global_mut::<EditorDocuments>()
                                                        .set_notice(&write_root, notice);
                                                    cx.refresh_windows();
                                                });
                                            })
                                            .detach();
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
            if let PanelContent::Characters { .. } = &panel.content {
                for placeholder in ["Character id", "Display name", "Color (optional)"] {
                    panel
                        .tool_inputs
                        .push(cx.new(|cx| InputState::new(window, cx).placeholder(placeholder)));
                }
            }
            panel.rebuild_visual_editors(window, cx);
            panel
        });
        if let Some((root, relative)) = registration {
            let editor = match &panel.read(cx).content {
                PanelContent::Document { editor, .. } => Some(editor.downgrade()),
                _ => None,
            };
            let documents = cx.global_mut::<EditorDocuments>();
            documents.register_panel(
                &root,
                relative.clone(),
                PanelId::from(panel.entity_id()),
                panel.downgrade(),
            );
            if let Some(editor) = editor {
                documents.register_editor(&root, relative.clone(), editor);
            }
            let reopened = match &panel.read(cx).content {
                PanelContent::Document {
                    document: Some(document),
                    ..
                } => Some(document.borrow().contents().as_bytes().to_vec()),
                _ => None,
            };
            if let Some(source) = reopened {
                if relative.extension().is_some_and(|ext| ext == "shou")
                    && let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(&root)
                {
                    preview.apply_snapshot(relative.clone(), source);
                }
                // Opening a clean file may adopt a newer disk revision without
                // an InputEvent::Change. Update every derived consumer as well.
                schedule_authoring_refresh(&root, Some(&relative), cx);
            }
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

    fn on_removed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_resource_picker(false, window, cx);
        if let PanelContent::AssetPreview { root } = &self.content
            && let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(root)
        {
            preview.stop_audition();
        }
        match &self.content {
            PanelContent::Document { root, relative, .. } => {
                let panel = PanelId::from(cx.entity_id());
                let documents = cx.global_mut::<EditorDocuments>();
                documents.unregister_panel(root, relative, panel);
                documents.clear_document_node(root);
            }
            PanelContent::Inspector { root, .. } => cx
                .global_mut::<EditorDocuments>()
                .set_tool_panel(root, INSPECTOR_PANEL, None),
            PanelContent::Search { root } => {
                cx.global_mut::<EditorDocuments>()
                    .set_tool_panel(root, SEARCH_PANEL, None)
            }
            PanelContent::Assets { root } => {
                cx.global_mut::<EditorDocuments>()
                    .set_tool_panel(root, ASSETS_PANEL, None)
            }
            PanelContent::AssetPreview { root } => cx
                .global_mut::<EditorDocuments>()
                .set_tool_panel(root, ASSET_PREVIEW_PANEL, None),
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

fn language_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("shou") => "eiyashou",
        Some("json") => "json",
        Some("yaml" | "yml") => "yaml",
        Some("md") => "markdown",
        Some("toml") => "toml",
        _ => "plaintext",
    }
}

fn register_workbench_panels(cx: &mut App) {
    for name in [
        EXPLORER_PANEL,
        SEARCH_PANEL,
        DOCUMENT_PANEL,
        INSPECTOR_PANEL,
        OUTPUT_PANEL,
        ASSETS_PANEL,
        ASSET_PREVIEW_PANEL,
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

struct WorkbenchWindow {
    editor: WeakEntity<EditorApp>,
    persistence: AppPersistence,
    workspace: Option<ProjectWorkspace>,
    recents: Vec<PathBuf>,
    allow_close: bool,
    close_prompt_open: bool,
    last_windowed_bounds: Bounds<Pixels>,
    bounds_epoch: u64,
    _bounds_subscription: Subscription,
    preview_lifecycle: PreviewLifecycle,
    preview_position: Option<(PathBuf, usize, usize)>,
    audition_status: (Option<PathBuf>, Option<String>),
    focus: FocusHandle,
}

fn install_close_guard(window: &mut Window, workbench: WeakEntity<WorkbenchWindow>, cx: &App) {
    window.on_window_should_close(cx, move |window, cx| {
        workbench
            .update(cx, |workbench, cx| {
                if workbench.file_operation_active(cx) {
                    window.push_notification(
                        Notification::warning("A file operation is still running"),
                        cx,
                    );
                    return false;
                }
                if workbench.allow_close || !workbench.has_unsaved_documents(cx) {
                    let bounds = if window.is_fullscreen() {
                        WindowBounds::Fullscreen(workbench.last_windowed_bounds)
                    } else if window.is_maximized() {
                        WindowBounds::Maximized(workbench.last_windowed_bounds)
                    } else {
                        WindowBounds::Windowed(windowed_content_bounds(window))
                    };
                    let display_uuid = window
                        .display(cx)
                        .and_then(|display| display.uuid().ok())
                        .map(|uuid| uuid.to_string());
                    if let Err(error) = workbench
                        .persistence
                        .save_window_bounds(bounds, display_uuid)
                    {
                        eprintln!("Kēne Editor could not save window bounds: {error}");
                    }
                    workbench.stop_preview(cx);
                    workbench.release_project(cx);
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
    fn file_operation_active(&self, cx: &App) -> bool {
        self.workspace
            .as_ref()
            .and_then(|workspace| {
                cx.global::<EditorDocuments>()
                    .workspaces
                    .get(workspace.session.root())
            })
            .is_some_and(|workspace| workspace.file_operation_active)
    }
    fn release_project(&mut self, cx: &mut App) {
        if let Some(workspace) = self.workspace.take() {
            workspace.persist_final_layout(&self.persistence, cx);
            if let Some(documents) = cx
                .global::<EditorDocuments>()
                .workspaces
                .get(workspace.session.root())
            {
                for document in documents.manager.documents() {
                    if let Err(error) = document.borrow_mut().persist_recovery() {
                        eprintln!("Kēne Editor could not flush recovery on close: {error}");
                    }
                    if let Err(error) = document.borrow().retire_recovery() {
                        eprintln!("Kēne Editor could not retire recovery jobs: {error}");
                    }
                }
            }
            cx.global_mut::<EditorDocuments>()
                .workspaces
                .remove(workspace.session.root());
        }
    }

    fn finish_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_operation_active(cx) {
            self.close_prompt_open = false;
            window.push_notification(
                Notification::warning("A file operation is still running"),
                cx,
            );
            return;
        }
        self.stop_preview(cx);
        self.allow_close = true;
        self.release_project(cx);
        window.remove_window();
    }
    fn watch_preview(window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            let mut interval = Duration::from_millis(250);
            loop {
                cx.background_executor().timer(interval).await;
                let Ok(running) = this.update_in(cx, |this, window, cx| {
                    let Some(root) = this
                        .workspace
                        .as_ref()
                        .map(|workspace| workspace.session.root().to_owned())
                    else {
                        return false;
                    };
                    let Ok(controller) = cx.global_mut::<EditorDocuments>().preview(&root) else {
                        return false;
                    };
                    let snapshot = controller.snapshot();
                    if this.preview_lifecycle != snapshot.lifecycle {
                        if let PreviewLifecycle::Failed(error) = &snapshot.lifecycle {
                            cx.global_mut::<EditorDocuments>()
                                .set_notice(&root, format!("Preview failed: {error}"));
                            cx.refresh_windows();
                        }
                        this.preview_lifecycle = snapshot.lifecycle;
                        cx.notify();
                    }
                    let audition_status = (snapshot.audition_path, snapshot.audition_error);
                    if this.audition_status != audition_status {
                        if let Some(error) = &audition_status.1 {
                            cx.global_mut::<EditorDocuments>()
                                .set_notice(&root, format!("Audition failed: {error}"));
                        }
                        this.audition_status = audition_status;
                        cx.refresh_windows();
                    }
                    if this.preview_position != snapshot.runtime_position {
                        this.preview_position = snapshot.runtime_position.clone();
                        if let Some((path, line, column)) = snapshot.runtime_position {
                            follow_preview_position(&root, &path, line, column, window, cx);
                        }
                    }
                    cx.global_mut::<EditorDocuments>()
                        .set_diagnostics(&root, snapshot.diagnostics);
                    matches!(
                        this.preview_lifecycle,
                        PreviewLifecycle::Running | PreviewLifecycle::Starting
                    )
                }) else {
                    break;
                };
                interval = if running {
                    Duration::from_millis(50)
                } else {
                    Duration::from_millis(250)
                };
            }
        })
        .detach();
    }

    fn undo_sources(&mut self, _: &UndoSources, window: &mut Window, cx: &mut Context<Self>) {
        self.replay_sources(true, window, cx);
    }

    fn redo_sources(&mut self, _: &RedoSources, window: &mut Window, cx: &mut Context<Self>) {
        self.replay_sources(false, window, cx);
    }

    fn replay_sources(&mut self, undo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.session.root())
        else {
            return;
        };
        match replay_source_history(root, undo, window, cx) {
            Ok(true) => cx.refresh_windows(),
            Ok(false) => {}
            Err(error) => window.push_notification(Notification::warning(error), cx),
        }
    }

    fn empty(
        editor: WeakEntity<EditorApp>,
        persistence: AppPersistence,
        recents: Vec<PathBuf>,
        initial_windowed_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_release(|this, cx| this.release_project(cx)).detach();
        install_close_guard(window, cx.weak_entity(), cx);
        Self::watch_preview(window, cx);
        let bounds_subscription = Self::watch_window_bounds(window, cx);
        Self {
            editor,
            persistence,
            workspace: None,
            recents,
            allow_close: false,
            close_prompt_open: false,
            last_windowed_bounds: initial_windowed_bounds,
            bounds_epoch: 0,
            _bounds_subscription: bounds_subscription,
            preview_lifecycle: PreviewLifecycle::Off,
            preview_position: None,
            audition_status: (None, None),
            focus: cx.focus_handle(),
        }
    }

    fn project(
        editor: WeakEntity<EditorApp>,
        persistence: AppPersistence,
        session: WorkspaceSession,
        initial_windowed_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_release(|this, cx| this.release_project(cx)).detach();
        install_close_guard(window, cx.weak_entity(), cx);
        Self::watch_preview(window, cx);
        let bounds_subscription = Self::watch_window_bounds(window, cx);
        let workspace = ProjectWorkspace::new(session, &persistence, window, cx);
        Self {
            editor,
            persistence,
            workspace: Some(workspace),
            recents: Vec::new(),
            allow_close: false,
            close_prompt_open: false,
            last_windowed_bounds: initial_windowed_bounds,
            bounds_epoch: 0,
            _bounds_subscription: bounds_subscription,
            preview_lifecycle: PreviewLifecycle::Off,
            preview_position: None,
            audition_status: (None, None),
            focus: cx.focus_handle(),
        }
    }

    fn watch_window_bounds(window: &mut Window, cx: &mut Context<Self>) -> Subscription {
        cx.observe_window_bounds(window, |this, window, cx| {
            // macOS Zoom reports intermediate windowed sizes while it animates.
            // Keep only a settled ordinary size as the restore geometry.
            this.bounds_epoch = this.bounds_epoch.wrapping_add(1);
            let epoch = this.bounds_epoch;
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let _ = this.update_in(cx, |this, window, _| {
                    if this.bounds_epoch == epoch
                        && !window.is_fullscreen()
                        && !window.is_maximized()
                    {
                        this.last_windowed_bounds = windowed_content_bounds(window);
                    }
                });
            })
            .detach();
        })
    }

    fn open_session(
        &mut self,
        session: WorkspaceSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        self.stop_preview(cx);
        // A migration replaces the session's inventory and source ownership together.
        // Late panel callbacks may read an existing session, but cannot create one.
        self.release_project(cx);
        cx.global_mut::<EditorDocuments>()
            .ensure_workspace_with_files(session.root(), session.files())?;
        self.workspace = Some(ProjectWorkspace::new(
            session,
            &self.persistence,
            window,
            cx,
        ));
        self.preview_lifecycle = PreviewLifecycle::Off;
        self.preview_position = None;
        self.recents.clear();
        cx.notify();
        Ok(())
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

    fn toggle_engine(&mut self, _: &ToggleEngine, _: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.as_ref() else {
            return;
        };
        let root = workspace.session.root().to_owned();
        let controller = match cx.global_mut::<EditorDocuments>().preview(&root) {
            Ok(controller) => controller,
            Err(error) => {
                cx.global_mut::<EditorDocuments>()
                    .set_notice(&root, format!("Could not start Preview: {error}"));
                cx.refresh_windows();
                return;
            }
        };
        if matches!(
            controller.snapshot().lifecycle,
            PreviewLifecycle::Running | PreviewLifecycle::Starting
        ) {
            controller.stop();
        } else {
            if !controller.apply_sources(cx.global::<EditorDocuments>().preview_documents(&root)) {
                return;
            }
            controller.start();
            self.preview_lifecycle = PreviewLifecycle::Starting;
        }
        cx.refresh_windows();
    }

    fn show_engine(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.as_ref() else {
            return;
        };
        if let Ok(controller) = cx
            .global_mut::<EditorDocuments>()
            .preview(workspace.session.root())
        {
            controller.show();
        }
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
            if matches!(kind, ToolKind::Search)
                && let Some(panel) = workspace.dock.read(cx).panel(panel)
            {
                panel.focus_handle(cx).focus(window, cx);
            }
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
        let search_focus = matches!(kind, ToolKind::Search).then(|| panel.read(cx).focus.clone());
        let is_asset = matches!(kind, ToolKind::Assets | ToolKind::Search);
        let tab_anchor = if is_asset {
            cx.global::<EditorDocuments>()
                .tool_panel(&root, EXPLORER_PANEL)
        } else if matches!(kind, ToolKind::Explorer) {
            None
        } else {
            cx.global::<EditorDocuments>()
                .tool_panel(&root, INSPECTOR_PANEL)
                .or_else(|| {
                    cx.global::<EditorDocuments>()
                        .tool_panel(&root, PROBLEMS_PANEL)
                })
        };
        workspace.dock.update(cx, |dock, cx| {
            let tab_node = tab_anchor.and_then(|anchor| {
                [
                    DockPlacement::Center,
                    DockPlacement::Left,
                    DockPlacement::Right,
                    DockPlacement::Bottom,
                ]
                .into_iter()
                .find_map(|placement| dock.layout(placement)?.find_panel_node(anchor))
            });
            dock.add_panel_view(
                panel_handle(panel),
                if matches!(kind, ToolKind::Explorer) {
                    DockPlacement::Left
                } else if is_asset {
                    if tab_node.is_some() {
                        DockPlacement::Center
                    } else {
                        DockPlacement::Left
                    }
                } else if tab_node.is_some() {
                    // Register inside the existing tree before moving to the
                    // target group; a temporary Right dock leaves an empty column.
                    DockPlacement::Center
                } else {
                    DockPlacement::Right
                },
                Some(px(340.)),
                window,
                cx,
            );
            if let Some(node) = tab_node {
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
        if let Some(focus) = search_focus {
            focus.focus(window, cx);
        }
        cx.refresh_windows();
    }

    fn show_asset_preview(
        &mut self,
        _: &ShowAssetPreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.as_ref() else {
            return;
        };
        let root = workspace.session.root();
        if cx
            .global::<EditorDocuments>()
            .tool_panel(root, ASSET_PREVIEW_PANEL)
            .is_none()
        {
            dock::install_asset_preview(&workspace.dock, root, window, cx);
            cx.refresh_windows();
        }
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
                        Ok(session) => match this.open_session(session, window, cx) {
                            Ok(()) => cx.global_mut::<EditorDocuments>().set_notice(
                                &root,
                                format!("Migrated {count} source file(s) to .shou"),
                            ),
                            Err(error) => window.push_notification(
                                Notification::error(format!(
                                    "Migration applied, but workspace refresh failed: {error}"
                                )),
                                cx,
                            ),
                        },
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
            if answer == Some(1) {
                // Complete the latest recovery write before releasing the
                // document/panel that owns the debounce. Editing during I/O
                // invalidates this attempt and prepares a new snapshot.
                loop {
                    let input = this
                        .update_in(cx, |this, _, cx| {
                            let root = this
                                .workspace
                                .as_ref()
                                .map(|workspace| workspace.session.root())?;
                            let workspace = cx.global::<EditorDocuments>().workspaces.get(root)?;
                            let versions = workspace
                                .manager
                                .documents()
                                .map(|document| {
                                    let document = document.borrow();
                                    (document.relative_path().to_owned(), document.revision())
                                })
                                .collect::<BTreeMap<_, _>>();
                            let writes = workspace
                                .manager
                                .documents()
                                .map(|document| document.borrow().recovery_write())
                                .collect::<io::Result<Vec<_>>>();
                            Some((root.to_owned(), versions, writes))
                        })
                        .ok()
                        .flatten();
                    let Some((root, versions, writes)) = input else {
                        let _ =
                            this.update_in(cx, |this, window, cx| this.finish_close(window, cx));
                        return;
                    };
                    let result = match writes {
                        Ok(writes) => {
                            cx.background_executor()
                                .spawn(async move {
                                    for write in writes {
                                        write.execute()?;
                                    }
                                    Ok::<_, io::Error>(())
                                })
                                .await
                        }
                        Err(error) => Err(error),
                    };
                    let finished = this
                        .update_in(cx, |this, window, cx| {
                            if let Err(error) = result {
                                this.close_prompt_open = false;
                                window.push_notification(
                                    Notification::error(format!(
                                        "Could not preserve recovery draft: {error}"
                                    )),
                                    cx,
                                );
                                return true;
                            }
                            let unchanged = cx
                                .global::<EditorDocuments>()
                                .workspaces
                                .get(&root)
                                .is_some_and(|workspace| {
                                    workspace
                                        .manager
                                        .documents()
                                        .map(|document| {
                                            let document = document.borrow();
                                            (
                                                document.relative_path().to_owned(),
                                                document.revision(),
                                            )
                                        })
                                        .collect::<BTreeMap<_, _>>()
                                        == versions
                                });
                            if unchanged {
                                this.finish_close(window, cx);
                            }
                            unchanged
                        })
                        .unwrap_or(true);
                    if finished {
                        return;
                    }
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.close_prompt_open = false;
                if answer == Some(0) {
                    let Some(root) = this
                        .workspace
                        .as_ref()
                        .map(|workspace| workspace.session.root().to_owned())
                    else {
                        this.finish_close(window, cx);
                        return;
                    };
                    match cx.global_mut::<EditorDocuments>().save_all(&root) {
                        Ok(_) => {
                            this.finish_close(window, cx);
                        }
                        Err(error) => {
                            cx.global_mut::<EditorDocuments>()
                                .set_notice(&root, format!("Save blocked: {error}"));
                            cx.refresh_windows();
                        }
                    }
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
            explorer_open,
            search_open,
            assets_open,
            characters_open,
            problems_open,
            performance_open,
        ) = self
            .workspace
            .as_ref()
            .map(|workspace| {
                let root = workspace.session.root();
                let documents = cx.global::<EditorDocuments>();
                (
                    documents.tool_panel(root, EXPLORER_PANEL).is_some(),
                    documents.tool_panel(root, SEARCH_PANEL).is_some(),
                    documents.tool_panel(root, ASSETS_PANEL).is_some(),
                    documents.tool_panel(root, CHARACTERS_PANEL).is_some(),
                    documents.tool_panel(root, PROBLEMS_PANEL).is_some(),
                    documents.tool_panel(root, PERFORMANCE_PANEL).is_some(),
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
            .when(self.workspace.is_some(), |this| {
                this.child(
                    activity_tool(
                        "activity-explorer",
                        IconName::FileText,
                        explorer_open,
                        "Explorer",
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_tool(ToolKind::Explorer, window, cx)
                    })),
                )
            })
            .when(self.workspace.is_some(), |this| {
                this.child(
                    activity_tool(
                        "activity-search",
                        AssetIconName::Search,
                        search_open,
                        "Search",
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_tool(ToolKind::Search, window, cx)
                    })),
                )
                .child(
                    activity_tool(
                        "activity-assets",
                        AssetIconName::Images,
                        assets_open,
                        "Assets",
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_tool(ToolKind::Assets, window, cx)
                    })),
                )
                .child(
                    activity_tool(
                        "activity-characters",
                        IconName::User,
                        characters_open,
                        "Characters",
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_tool(ToolKind::Characters, window, cx)
                    })),
                )
                .child(activity_divider())
                .child(
                    activity_tool(
                        "activity-problems",
                        IconName::TriangleAlert,
                        problems_open,
                        "Problems",
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_tool(ToolKind::Problems, window, cx)
                    })),
                )
                .child(
                    activity_tool(
                        "activity-performance",
                        IconName::Cpu,
                        performance_open,
                        "Performance",
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_tool(ToolKind::Performance, window, cx)
                    })),
                )
            })
            .child(div().flex_1())
            .when(self.workspace.is_some(), |this| {
                this.child(
                    div()
                        .id("activity-open-folder")
                        .size(px(ACTIVITY_ITEM_SIZE_PX))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(7.))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                        .tooltip(icon_hint("Open folder"))
                        .on_click(move |_, _, cx| prompt_open_folder(editor.clone(), cx))
                        .child(
                            Icon::new(IconName::FolderOpen)
                                .with_size(px(ACTIVITY_ICON_SIZE_PX))
                                .text_color(rgb(MUTED)),
                        ),
                )
            })
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
                            .tooltip(icon_hint("Migrate project"))
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
                            .tooltip(icon_hint("Reset layout"))
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
                                                let editor = editor.clone();
                                                let path = open_path.clone();
                                                cx.spawn(async move |cx| {
                                                    cx.update(|cx| {
                                                        open_paths(&editor, vec![path], cx)
                                                    });
                                                })
                                                .detach();
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
            .on_action(cx.listener(|this, _: &ShowAssets, window, cx| {
                this.show_tool(ToolKind::Assets, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowSearch, window, cx| {
                this.show_tool(ToolKind::Search, window, cx);
            }))
            .on_action(cx.listener(Self::show_asset_preview))
            .on_action(cx.listener(Self::migrate_eiyashou))
            .on_action(cx.listener(Self::undo_sources))
            .on_action(cx.listener(Self::redo_sources))
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
            .when(self.workspace.is_some(), |this| {
                let running = matches!(
                    self.preview_lifecycle,
                    PreviewLifecycle::Running | PreviewLifecycle::Starting
                );
                this.child(
                    div()
                        .id("preview-window-control")
                        .absolute()
                        .top(px(6.))
                        .right(px(4.))
                        .p(px(3.))
                        .rounded(px(10.))
                        .bg(rgb(CHROME))
                        .flex()
                        .items_center()
                        .gap_1()
                        .when(
                            matches!(self.preview_lifecycle, PreviewLifecycle::Failed(_)),
                            |this| {
                                this.child(
                                    div()
                                        .px_2()
                                        .text_xs()
                                        .text_color(rgb(0xdb7780))
                                        .child("Preview failed · see Output"),
                                )
                            },
                        )
                        .when(running, |this| {
                            this.child(
                                div()
                                    .id("preview-show")
                                    .size(px(28.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.))
                                    .bg(rgb(SURFACE))
                                    .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                    .cursor_pointer()
                                    .tooltip(icon_hint("Show engine window"))
                                    .child(
                                        Icon::new(AssetIconName::ExternalLink)
                                            .xsmall()
                                            .text_color(rgb(INK)),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| this.show_engine(cx))),
                            )
                        })
                        .child(preview_transport_button(running).on_click(cx.listener(
                            |this, _, window, cx| this.toggle_engine(&ToggleEngine, window, cx),
                        ))),
                )
            })
    }
}

struct IconHint(SharedString);

impl Render for IconHint {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(px(6.))
            .bg(rgb(SURFACE))
            .text_xs()
            .text_color(rgb(INK))
            .child(self.0.clone())
    }
}

fn icon_hint(
    label: impl Into<SharedString> + 'static,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label = label.into();
    move |_, cx| cx.new(|_| IconHint(label.clone())).into()
}

fn activity_tool(
    id: &'static str,
    icon: impl gpui_kit::assets::IconNamed,
    active: bool,
    hint: &'static str,
) -> Stateful<Div> {
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
        .tooltip(icon_hint(hint))
        .child(
            Icon::new(icon)
                .with_size(px(ACTIVITY_ICON_SIZE_PX))
                .text_color(rgb(if active { PRIMARY } else { MUTED })),
        )
}

fn activity_divider() -> Div {
    div().w(px(22.)).h(px(1.)).my(px(1.)).bg(rgb(BORDER))
}

fn file_action_icon(id: &'static str, icon: AssetIconName, hint: &'static str) -> Stateful<Div> {
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
        .tooltip(icon_hint(hint))
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

fn render_block_context_menu(
    row: usize,
    position: Point<Pixels>,
    source: String,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let block = EiyashouProjection::parse(&source)
        .scenes
        .into_iter()
        .flat_map(|scene| scene.blocks)
        .find(|block| block.source_range.start == row);
    let mut items = vec![
        (
            BlockMenuAction::Run,
            AssetIconName::Play,
            "Run to here",
            false,
        ),
        (BlockMenuAction::Copy, AssetIconName::Copy, "Copy", false),
        (
            BlockMenuAction::Duplicate,
            AssetIconName::Copy,
            "Duplicate",
            false,
        ),
        (BlockMenuAction::Cut, AssetIconName::Scissors, "Cut", false),
        (
            BlockMenuAction::Paste,
            AssetIconName::Clipboard,
            "Paste",
            false,
        ),
        (
            BlockMenuAction::SelectAll,
            AssetIconName::Square,
            "Select all",
            false,
        ),
        (
            BlockMenuAction::InsertAbove,
            AssetIconName::ArrowUp,
            "Insert above",
            false,
        ),
        (
            BlockMenuAction::InsertBelow,
            AssetIconName::ArrowDown,
            "Insert below",
            false,
        ),
        (
            BlockMenuAction::MoveUp,
            AssetIconName::ArrowUp,
            "Move up",
            false,
        ),
        (
            BlockMenuAction::MoveDown,
            AssetIconName::ArrowDown,
            "Move down",
            false,
        ),
        (
            BlockMenuAction::Delete,
            AssetIconName::Delete,
            "Delete",
            true,
        ),
    ];
    if let Some(block) = &block
        && !matches!(
            block.kind,
            BlockKind::Narration | BlockKind::Dialogue { .. } | BlockKind::Else | BlockKind::ElseIf
        )
    {
        items.insert(
            6,
            (
                BlockMenuAction::ToggleDisabled,
                if block.disabled {
                    AssetIconName::Eye
                } else {
                    AssetIconName::EyeOff
                },
                if block.disabled { "Enable" } else { "Disable" },
                false,
            ),
        );
    }
    deferred(
        anchored()
            .anchor(Anchor::TopLeft)
            .position(position)
            .snap_to_window_with_margin(px(6.))
            .child(
                div()
                    .id("block-context-menu")
                    .w(px(188.))
                    .p_1()
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.block_context_menu = None;
                        cx.notify();
                    }))
                    .children(items.into_iter().enumerate().map(
                        |(index, (action, icon, label, danger))| {
                            let source = source.clone();
                            div()
                                .id(("block-context-item", index))
                                .w_full()
                                .h(px(27.))
                                .when(
                                    matches!(
                                        action,
                                        BlockMenuAction::Copy
                                            | BlockMenuAction::ToggleDisabled
                                            | BlockMenuAction::InsertAbove
                                            | BlockMenuAction::MoveUp
                                            | BlockMenuAction::Delete
                                    ),
                                    |this| this.mt_1().border_t_1().border_color(rgb(BORDER)),
                                )
                                .px_2()
                                .rounded(px(3.))
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_size(px(12.))
                                .text_color(rgb(if danger { 0xdb7780 } else { INK }))
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.block_menu_action(row, &source, action, window, cx);
                                }))
                                .child(Icon::new(icon).xsmall())
                                .child(label)
                        },
                    )),
            ),
    )
    .priority(100)
    .into_any_element()
}

fn render_scene_context_menu(
    menu: SceneContextMenu,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let start = menu.start;
    let name = menu.name.clone();
    let closing = menu.closing;
    let motion_duration = if closing { 90 } else { 120 };
    let surface = div()
        .id(("scene-context-surface", menu.epoch))
        .w(px(SCENE_CONTEXT_MENU_WIDTH_PX))
        .h(px(SCENE_CONTEXT_MENU_HEIGHT_PX))
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
            file_context_menu_item("scene-context-new", AssetIconName::Plus, "New", false)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_scene_context_menu(window, cx);
                    this.begin_scene_edit(SceneEditMode::New, window, cx);
                })),
        )
        .child(
            file_context_menu_item(
                "scene-context-rename",
                AssetIconName::Replace,
                "Rename",
                false,
            )
            .on_click(cx.listener({
                let name = name.clone();
                move |this, _, window, cx| {
                    this.close_scene_context_menu(window, cx);
                    this.begin_scene_edit(
                        SceneEditMode::Rename {
                            start,
                            old_name: name.clone(),
                        },
                        window,
                        cx,
                    );
                }
            })),
        )
        .child(
            file_context_menu_item("scene-context-up", AssetIconName::ArrowUp, "Move up", false)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_scene_context_menu(window, cx);
                    this.move_scene_from_menu(start, MoveDirection::Up, window, cx);
                })),
        )
        .child(
            file_context_menu_item(
                "scene-context-down",
                AssetIconName::ArrowDown,
                "Move down",
                false,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.close_scene_context_menu(window, cx);
                this.move_scene_from_menu(start, MoveDirection::Down, window, cx);
            })),
        )
        .child(
            file_context_menu_item(
                "scene-context-delete",
                AssetIconName::Delete,
                "Delete",
                true,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.close_scene_context_menu(window, cx);
                this.confirm_delete_scene(start, name.clone(), window, cx);
            })),
        )
        .with_animation(
            ("scene-context-motion", menu.epoch),
            Animation::new(Duration::from_millis(motion_duration)).with_easing(ease_out_quint()),
            move |surface, delta| {
                let progress = if closing { 1. - delta } else { delta };
                let scale = 0.94 + progress * 0.06;
                surface
                    .opacity(progress)
                    .w(px(SCENE_CONTEXT_MENU_WIDTH_PX * scale))
                    .h(px(SCENE_CONTEXT_MENU_HEIGHT_PX * scale))
            },
        );
    deferred(
        anchored()
            .anchor(Anchor::TopLeft)
            .position(menu.position)
            .snap_to_window_with_margin(px(6.))
            .child(
                div()
                    .w(px(SCENE_CONTEXT_MENU_WIDTH_PX))
                    .h(px(SCENE_CONTEXT_MENU_HEIGHT_PX))
                    .on_mouse_down_out(
                        cx.listener(|this, _, window, cx| {
                            this.close_scene_context_menu(window, cx)
                        }),
                    )
                    .child(surface),
            ),
    )
    .priority(100)
    .into_any_element()
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

fn window_options(index: usize, cx: &App, persistence: &AppPersistence) -> WindowOptions {
    let saved = (index == 0)
        .then(|| persistence.load_window_bounds())
        .flatten();
    let restored = saved.and_then(|(bounds, uuid)| {
        let display = cx.displays().into_iter().find(|display| {
            display
                .uuid()
                .is_ok_and(|candidate| Some(candidate.to_string()) == uuid)
        })?;
        Some((
            visible_window_bounds(bounds, display.visible_bounds()),
            display.id(),
        ))
    });
    WindowOptions {
        window_bounds: Some(
            restored
                .as_ref()
                .map(|(bounds, _)| *bounds)
                .unwrap_or_else(|| WindowBounds::Windowed(offset_bounds(index, cx))),
        ),
        display_id: restored.map(|(_, id)| id),
        window_min_size: Some(size(px(720.), px(480.))),
        app_id: Some(APP_ID.into()),
        ..Default::default()
    }
}

fn windowed_content_bounds(window: &Window) -> Bounds<Pixels> {
    let mut frame = window.window_bounds().get_bounds();
    // WindowOptions expects content size; on macOS frame bounds include the title bar.
    frame.size = window.viewport_size();
    frame
}

fn visible_window_bounds(saved: WindowBounds, area: Bounds<Pixels>) -> WindowBounds {
    let rectangle = saved.get_bounds();
    let width = rectangle.size.width.min(area.size.width);
    let height = rectangle.size.height.min(area.size.height);
    let restored = Bounds {
        origin: Point {
            x: rectangle
                .origin
                .x
                .max(area.origin.x)
                .min(area.origin.x + area.size.width - width),
            y: rectangle
                .origin
                .y
                .max(area.origin.y)
                .min(area.origin.y + area.size.height - height),
        },
        size: size(width, height),
    };
    match saved {
        WindowBounds::Windowed(_) => WindowBounds::Windowed(restored),
        WindowBounds::Maximized(_) => WindowBounds::Maximized(restored),
        WindowBounds::Fullscreen(_) => WindowBounds::Fullscreen(restored),
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

#[derive(Debug, PartialEq, Eq)]
enum StartupArgs {
    Launch(Vec<PathBuf>),
    Help,
    Version,
}

fn parse_startup_args(args: &[OsString]) -> Result<StartupArgs, String> {
    if args.len() == 1 {
        if args[0] == "-h" || args[0] == "--help" {
            return Ok(StartupArgs::Help);
        }
        if args[0] == "-V" || args[0] == "--version" {
            return Ok(StartupArgs::Version);
        }
    }
    let mut paths = Vec::new();
    let mut positional = false;
    for argument in args {
        if !positional && argument == "--" {
            positional = true;
            continue;
        }
        if !positional && argument.to_string_lossy().starts_with('-') {
            return Err(format!(
                "unknown option {argument:?}; usage: editor [project ...]"
            ));
        }
        paths.push(PathBuf::from(argument));
    }
    Ok(StartupArgs::Launch(paths))
}

pub fn run() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let paths = match parse_startup_args(&args) {
        Ok(StartupArgs::Launch(paths)) => paths,
        Ok(StartupArgs::Help) => {
            println!(
                "Kēne Editor {}\n\nUsage: editor [project ...]",
                env!("CARGO_PKG_VERSION")
            );
            println!("Open the workbench or one window per project directory.");
            return ExitCode::SUCCESS;
        }
        Ok(StartupArgs::Version) => {
            println!("Kēne Editor {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let app_data = match crate::app_data::root() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Kēne Editor could not locate app data: {error}");
            return ExitCode::FAILURE;
        }
    };
    let instance = match acquire_or_forward(&app_data, paths.clone()) {
        Ok(Startup::Forwarded) => return ExitCode::SUCCESS,
        Ok(Startup::Primary(instance)) => instance,
        Err(error) => {
            eprintln!("Kēne Editor could not initialize its app instance: {error}");
            return ExitCode::FAILURE;
        }
    };
    let receiver = instance.receiver();
    gpui_kit::application()
        .with_assets(EditorAssets)
        .with_quit_mode(gpui_kit::QuitMode::LastWindowClosed)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            crate::syntax::install_editing_languages(cx);
            configure_dark_theme(cx);
            let persistence = AppPersistence::new(app_data.clone());
            cx.set_global(EditorDocuments::new(persistence.clone()));
            register_workbench_panels(cx);
            cx.bind_keys([
                KeyBinding::new("cmd-o", OpenFolder, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-o", OpenFolder, Some("KeineWorkbench")),
                KeyBinding::new("cmd-s", Save, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-s", Save, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-f", ShowSearch, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-shift-f", ShowSearch, Some("KeineWorkbench")),
                // Native Input binds cmd-shift-f to Replace. Override at its context depth.
                KeyBinding::new("cmd-shift-f", ShowSearch, Some("KeineWorkbench > Input")),
                KeyBinding::new("ctrl-shift-f", ShowSearch, Some("KeineWorkbench > Input")),
                KeyBinding::new("cmd-shift-s", SaveAll, Some("KeineWorkbench")),
                KeyBinding::new("cmd-alt-r", ReloadDocument, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-alt-r", ReloadDocument, Some("KeineWorkbench")),
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
                KeyBinding::new("escape", CloseBlockPicker, Some("KeineBlockView")),
                KeyBinding::new("down", BlockPickerNext, Some("KeineBlockPicker")),
                KeyBinding::new("up", BlockPickerPrevious, Some("KeineBlockPicker")),
                KeyBinding::new("left", BlockPickerLeft, Some("KeineBlockPicker")),
                KeyBinding::new("right", BlockPickerRight, Some("KeineBlockPicker")),
                KeyBinding::new("tab", CloseBlockPicker, Some("KeineBlockPicker")),
                KeyBinding::new("enter", AcceptBlockPicker, Some("KeineBlockPicker")),
                KeyBinding::new("escape", CloseBlockPicker, Some("KeineBlockPicker")),
                KeyBinding::new("down", ResourcePickerNext, Some("KeineResourcePicker")),
                KeyBinding::new("up", ResourcePickerPrevious, Some("KeineResourcePicker")),
                KeyBinding::new("enter", AcceptResourcePicker, Some("KeineResourcePicker")),
                KeyBinding::new("escape", CloseResourcePicker, Some("KeineResourcePicker")),
                KeyBinding::new("tab", CloseResourcePicker, Some("KeineResourcePicker")),
                KeyBinding::new("backspace", DeleteBlocks, Some("KeineBlockView")),
                KeyBinding::new("delete", DeleteBlocks, Some("KeineBlockView")),
                KeyBinding::new("alt-up", MoveBlocksUp, Some("KeineBlockView")),
                KeyBinding::new("alt-down", MoveBlocksDown, Some("KeineBlockView")),
                KeyBinding::new("cmd-z", UndoBlocks, Some("KeineBlockView")),
                KeyBinding::new("cmd-shift-z", RedoBlocks, Some("KeineBlockView")),
                KeyBinding::new("ctrl-z", UndoBlocks, Some("KeineBlockView")),
                KeyBinding::new("ctrl-y", RedoBlocks, Some("KeineBlockView")),
                KeyBinding::new("cmd-z", UndoFiles, Some("KeineExplorer")),
                KeyBinding::new("cmd-shift-z", RedoFiles, Some("KeineExplorer")),
                KeyBinding::new("ctrl-z", UndoFiles, Some("KeineExplorer")),
                KeyBinding::new("ctrl-y", RedoFiles, Some("KeineExplorer")),
                KeyBinding::new("cmd-z", UndoSources, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-z", RedoSources, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-z", UndoSources, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-y", RedoSources, Some("KeineWorkbench")),
            ]);
            let editor = cx.new(|cx| EditorApp::new(cx.weak_entity(), persistence, instance));
            cx.set_global(EditorAppOwner {
                _editor: editor.clone(),
            });
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
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::point;

    #[test]
    fn explorer_layout_restores_expansion_and_old_layouts_default_to_collapsed() {
        let legacy: PanelPayload = serde_json::from_value(serde_json::json!({
            "kind": "explorer",
            "root": "project"
        }))
        .unwrap();
        assert!(matches!(legacy, PanelPayload::Explorer { expanded, .. } if expanded.is_empty()));

        let content = PanelContent::Explorer {
            root: PathBuf::from("project"),
            files: Vec::new(),
            expanded: HashSet::from([
                PathBuf::from("chapters"),
                PathBuf::from("assets/voice"),
                PathBuf::from("assets"),
            ]),
        };
        let saved = serde_json::to_value(content.payload()).unwrap();
        let restored: PanelPayload = serde_json::from_value(saved).unwrap();
        let PanelPayload::Explorer { root, expanded } = restored else {
            panic!("Explorer state must retain its panel kind");
        };
        assert_eq!(root, Path::new("project"));
        assert_eq!(
            expanded,
            ["assets", "assets/voice", "chapters"].map(PathBuf::from)
        );
    }

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
            include_str!("../../../tests/fixtures/native-smoke/config.yaml"),
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

    #[test]
    fn editor_cli_handles_help_and_rejects_unknown_options_before_opening_a_window() {
        assert_eq!(
            parse_startup_args(&["--help".into()]).unwrap(),
            StartupArgs::Help
        );
        assert_eq!(
            parse_startup_args(&["-V".into()]).unwrap(),
            StartupArgs::Version
        );
        assert!(parse_startup_args(&["--unknown".into()]).is_err());
        assert_eq!(
            parse_startup_args(&["project-one".into(), "project-two".into()]).unwrap(),
            StartupArgs::Launch(vec!["project-one".into(), "project-two".into()])
        );
        assert_eq!(
            parse_startup_args(&["--".into(), "-project".into()]).unwrap(),
            StartupArgs::Launch(vec!["-project".into()])
        );
    }

    #[test]
    fn document_extensions_select_the_shared_highlighter_language() {
        for (path, language) in [
            ("scripts/main.shou", "eiyashou"),
            ("config.yaml", "yaml"),
            ("assets.yml", "yaml"),
            ("README.md", "markdown"),
            ("settings.toml", "toml"),
            ("project.json", "json"),
            ("notes.txt", "plaintext"),
        ] {
            assert_eq!(language_for_path(Path::new(path)), language);
        }
    }

    #[test]
    fn asset_range_and_toggle_selection_follow_visible_order() {
        let ordered = ["a", "b", "c"]
            .into_iter()
            .map(|id| AssetKey {
                kind: AssetKind::Background,
                id: id.into(),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            select_asset_keys(&[], &ordered, Some(&ordered[0]), &ordered[2], true, false),
            ordered
        );
        assert_eq!(
            select_asset_keys(&ordered, &ordered, None, &ordered[1], false, true),
            vec![ordered[0].clone(), ordered[2].clone()]
        );
        assert_eq!(
            select_asset_keys(&ordered, &ordered, None, &ordered[1], false, false),
            vec![ordered[1].clone()]
        );
    }

    #[test]
    fn asset_drop_is_bounded_and_voice_requires_text() {
        let source = "scene start {\n  \"Hello\",\n  wait(500ms)\n}\n";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        let text_start = blocks[0].source_range.start;
        let command_start = blocks[1].source_range.start;
        let index = AuthoringIndex {
            assets: vec![
                crate::authoring::AssetEntry {
                    kind: AssetKind::Background,
                    id: "room".into(),
                    path: "assets/room.webp".into(),
                    tags: Vec::new(),
                    exists: true,
                    reference_count: 0,
                },
                crate::authoring::AssetEntry {
                    kind: AssetKind::Voice,
                    id: "hello".into(),
                    path: "assets/hello.ogg".into(),
                    tags: Vec::new(),
                    exists: true,
                    reference_count: 0,
                },
            ],
            ..Default::default()
        };
        let voice = index.assets[1].key();
        let background = index.assets[0].key();
        assert!(
            insert_assets_at_block(source, command_start, std::slice::from_ref(&voice), &index)
                .is_err()
        );
        assert!(
            insert_assets_at_block(
                source,
                text_start,
                &[voice.clone(), background.clone()],
                &index
            )
            .is_err()
        );
        assert!(
            insert_assets_at_block(source, text_start, &[voice], &index)
                .unwrap()
                .contains("\"Hello\", hello")
        );
        let edited = insert_assets_at_block(source, command_start, &[background], &index).unwrap();
        assert!(edited.contains("background(room)"));
        assert!(edited.contains("wait(500ms)"));
        let changed = insert_assets_at_block(
            source,
            command_start,
            &[index.assets[0].key(), index.assets[0].key()],
            &index,
        )
        .unwrap();
        assert_eq!(changed.as_str().matches("background(room)").count(), 1);
    }

    #[test]
    fn asset_rename_preflights_every_reference_before_emitting_edits() {
        let asset = crate::authoring::AssetEntry {
            kind: AssetKind::Background,
            id: "room".into(),
            path: "assets/room.webp".into(),
            tags: Vec::new(),
            exists: true,
            reference_count: 2,
        };
        let script_a = "scene a { background(room) }";
        let script_b = "scene b { background(room) }";
        let index = AuthoringIndex {
            assets_manifest: Some("assets.yaml".into()),
            assets: vec![asset.clone()],
            asset_references: ["scripts/a.shou", "scripts/b.shou"]
                .into_iter()
                .map(|path| crate::authoring::AssetReference {
                    key: asset.key(),
                    path: path.into(),
                    line: 1,
                    column: 22,
                    range: Some(21..25),
                })
                .collect(),
            ..Default::default()
        };
        let source_for = |path: &Path| match path.to_str()? {
            "assets.yaml" => Some("backgrounds:\n  room: assets/room.webp\n".into()),
            "scripts/a.shou" => Some(script_a.into()),
            "scripts/b.shou" => Some(script_b.into()),
            _ => None,
        };
        let edits = prepare_asset_edits(
            Path::new("."),
            &index,
            &asset,
            "hall",
            AssetKind::Background,
            &[],
            source_for,
        )
        .unwrap();
        assert_eq!(edits.len(), 3);
        assert!(
            edits
                .iter()
                .any(|(path, text)| path == Path::new("scripts/a.shou")
                    && text.contains("background(hall)"))
        );
        let mut incomplete = index.clone();
        incomplete.asset_references[1].range = None;
        assert!(
            prepare_asset_edits(
                Path::new("."),
                &incomplete,
                &asset,
                "hall",
                AssetKind::Background,
                &[],
                source_for
            )
            .is_err()
        );
        assert!(
            prepare_asset_edits(
                Path::new("."),
                &index,
                &asset,
                "room",
                AssetKind::Figure,
                &[],
                source_for
            )
            .is_err()
        );
    }

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
    fn empty_text_blocks_can_be_inserted_consecutively() {
        let source = "scene start {\n  \"Hello.\"\n}\n";
        let first = EiyashouProjection::parse(source).scenes[0].blocks[0]
            .source_range
            .start;
        let (source, inserted) = EiyashouProjection::parse(source)
            .insert_block_after(source, first, "\"\"")
            .unwrap();
        let (source, _) = EiyashouProjection::parse(&source)
            .insert_block_after(&source, inserted.start, "\"\"")
            .unwrap();
        assert_eq!(source.as_str().matches("\"\"").count(), 2);
        assert_eq!(EiyashouProjection::parse(&source).scenes[0].blocks.len(), 3);
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
    fn picker_icons_are_present_in_the_editor_asset_source() {
        for kind in InsertKind::ALL {
            let icon = insert_kind_icon(kind);
            assert!(
                gpui_kit::AssetSource::load(&EditorAssets, icon.path().as_ref())
                    .unwrap()
                    .is_some(),
                "missing icon for {}",
                kind.label()
            );
        }
    }

    #[test]
    fn preview_transport_icons_are_present_in_the_editor_asset_source() {
        for running in [false, true] {
            let icon = preview_transport_icon(running);
            assert!(
                gpui_kit::AssetSource::load(&EditorAssets, icon.path().as_ref())
                    .unwrap()
                    .is_some(),
                "missing Preview transport icon for running={running}"
            );
        }
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
