//! Workbench panel state, construction and dock registration.

pub(super) use crate::authoring::fields::SourceContext as SourceInspectorKey;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName as AssetIconName;
use gpui_kit::component::dock::{
    BasePanel, Panel, PanelBuildContext, PanelEvent, PanelId, PanelInfo, PanelState, panel_handle,
    register_panel,
};
use gpui_kit::component::input::{EditorState, InputEvent, InputState, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::slider::SliderState;
use gpui_kit::component::{Icon, Sizable as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Pixels, Point,
    Render, ScrollAnchor, ScrollHandle, SharedString, Subscription, Window, div, px, rgb,
};
use serde::{Deserialize, Serialize};

use crate::authoring::{AssetKey, AssetKind, AssetSort};
use crate::document::{DocumentHandle, is_eiyashou_authoring_document};
use crate::preview::PreviewController;
use crate::projection::{TextBlockMetadata, TextLifetime};
use crate::syntax::editor_highlighter_factory;
use crate::workspace::WorkspaceFile;

use super::documents::{EditorDocuments, schedule_authoring_refresh};
use super::edits::block_at_position;
use super::files::FileHistory;
use super::inspector::{InlineBlockControl, SourceOption};
use super::resource::ResourcePicker;
use super::{
    ASSET_PREVIEW_PANEL, ASSETS_PANEL, CHARACTERS_PANEL, DOCUMENT_PANEL, EXPLORER_PANEL, INK,
    INSPECTOR_PANEL, OUTPUT_PANEL, PERFORMANCE_PANEL, PREVIEW_SOURCE_DEBOUNCE, PROBLEMS_PANEL,
    SCENES_PANEL, SEARCH_PANEL, SURFACE, SURFACE_HOVER, completion, minimap, search, text_minimap,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum PanelPayload {
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
pub(super) enum ToolKind {
    Explorer,
    Search,
    Assets,
    Characters,
    Problems,
    Performance,
}

impl ToolKind {
    pub(super) fn panel_name(self) -> &'static str {
        match self {
            Self::Explorer => EXPLORER_PANEL,
            Self::Search => SEARCH_PANEL,
            Self::Assets => ASSETS_PANEL,
            Self::Characters => CHARACTERS_PANEL,
            Self::Problems => PROBLEMS_PANEL,
            Self::Performance => PERFORMANCE_PANEL,
        }
    }

    pub(super) fn payload(self, root: PathBuf) -> PanelPayload {
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
pub(super) enum PanelContent {
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
                        .folding(language == "eiyashou")
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

    pub(super) fn payload(&self) -> PanelPayload {
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

pub(super) struct WorkbenchPanel {
    pub(super) content: PanelContent,
    pub(super) focus: FocusHandle,
    pub(super) document_mode: DocumentMode,
    pub(super) last_text_cursor: Option<(usize, usize)>,
    pub(super) block_text_editors: Vec<BlockTextEditor>,
    pub(super) collapsed_scenes: HashSet<String>,
    pub(super) selected_blocks: HashSet<usize>,
    pub(super) block_selection_anchor: Option<usize>,
    pub(super) draft_text: Option<DraftTextBlock>,
    pub(super) block_drag: super::blocks::BlockDragState,
    pub(super) block_row_bounds: Rc<RefCell<HashMap<usize, Bounds<Pixels>>>>,
    pub(super) block_row_positions: RefCell<HashMap<usize, f32>>,
    pub(super) block_picker_open: bool,
    pub(super) block_picker_index: usize,
    pub(super) block_picker_category: Option<&'static str>,
    pub(super) block_picker_customize: bool,
    pub(super) block_picker_input: Entity<InputState>,
    pub(super) block_insertion_target: Option<DraftInsertionTarget>,
    pub(super) block_context_menu: Option<(usize, Point<Pixels>, String)>,
    pub(super) view_scroll: ScrollHandle,
    pub(super) block_scroll_anchor: ScrollAnchor,
    pub(super) block_scroll_pending: bool,
    pub(super) minimap_navigation: minimap::Navigation,
    pub(super) text_minimap: text_minimap::TextMinimap,
    pub(super) project_search: Option<search::ProjectSearch>,
    pub(super) tool_inputs: Vec<Entity<InputState>>,
    pub(super) recovery_epoch: u64,
    pub(super) syntax_check: Option<gpui_kit::Task<()>>,
    pub(super) syntax_marks: Option<gpui_kit::base::input::TextDecorationCollection>,
    pub(super) _subscriptions: Vec<Subscription>,
    pub(super) visual_subscriptions: Vec<Subscription>,
    pub(super) inspector_key: Option<InspectorEditKey>,
    pub(super) inspector_inputs: Vec<Entity<InputState>>,
    pub(super) text_lifetime_inputs: Vec<Entity<InputState>>,
    pub(super) inspector_selects: Vec<Entity<SelectState<Vec<SourceOption>>>>,
    pub(super) inspector_subscriptions: Vec<Subscription>,
    pub(super) inline_block_controls: HashMap<usize, InlineBlockControl>,
    pub(super) block_visible: HashSet<usize>,
    pub(super) block_heights: HashMap<usize, f32>,
    pub(super) block_layout: RefCell<super::blocks::layout::Cache>,
    pub(super) block_height_revision: u64,
    pub(super) resource_picker: Option<ResourcePicker>,
    pub(super) source_inspector_key: Option<SourceInspectorKey>,
    pub(super) source_inspector_inputs: Vec<Entity<InputState>>,
    pub(super) source_inspector_texts: Vec<Entity<TextareaState>>,
    pub(super) source_inspector_sliders: HashMap<String, Entity<SliderState>>,
    pub(super) source_inspector_selects: HashMap<String, Entity<SelectState<Vec<SourceOption>>>>,
    pub(super) source_inspector_subscriptions: Vec<Subscription>,
    pub(super) source_inspector_effect: Option<&'static str>,
    pub(super) source_position_bounds: Rc<RefCell<Bounds<Pixels>>>,
    pub(super) source_position_draft: Option<(usize, f32, f32)>,
    pub(super) asset_inspector_key: Option<(AssetKey, PathBuf, Vec<String>)>,
    pub(super) asset_inspector_inputs: Vec<Entity<InputState>>,
    pub(super) asset_rename_file: bool,
    pub(super) asset_batch_tags: Entity<InputState>,
    pub(super) batch_block_field: Option<String>,
    pub(super) batch_block_input: Entity<InputState>,
    pub(super) asset_inspector_subscriptions: Vec<Subscription>,
    pub(super) file_selection: Option<PathBuf>,
    pub(super) file_drop_target: Option<(PathBuf, Bounds<Pixels>)>,
    pub(super) file_clipboard: Option<PathBuf>,
    pub(super) file_history: FileHistory,
    pub(super) file_edit: Option<FileEditMode>,
    pub(super) file_name_input: Entity<InputState>,
    pub(super) file_commit_requested: bool,
    pub(super) file_progress: Option<FileProgress>,
    pub(super) file_context_menu: Option<FileContextMenu>,
    pub(super) file_context_epoch: u64,
    pub(super) scene_edit: Option<SceneEditMode>,
    pub(super) scene_name_input: Entity<InputState>,
    pub(super) scene_commit_requested: bool,
    pub(super) scene_context_menu: Option<SceneContextMenu>,
    pub(super) scene_context_epoch: u64,
    pub(super) asset_search: Entity<InputState>,
    pub(super) asset_kind: Option<AssetKind>,
    pub(super) asset_folder: Option<PathBuf>,
    pub(super) asset_sort: AssetSort,
    pub(super) asset_tag: Option<String>,
    pub(super) asset_status: crate::authoring::AssetStatus,
    pub(super) asset_size: crate::authoring::AssetSize,
    pub(super) asset_modified: Option<Duration>,
    pub(super) asset_grid: Option<bool>,
    pub(super) asset_large: bool,
    pub(super) asset_browser: RefCell<super::resource::browse::Cache>,
    pub(super) asset_thumbnails: Entity<super::resource::thumbnail::Thumbnails>,
    pub(super) asset_unmapped: bool,
    pub(super) asset_anchor: Option<AssetKey>,
    pub(super) asset_filter_menu: Option<AssetFilterMenu>,
    pub(super) asset_filter_epoch: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum DocumentMode {
    #[default]
    Text,
    Block,
}

pub(super) struct BlockTextEditor {
    pub(super) text_start: usize,
    pub(super) state: Entity<TextareaState>,
    pub(super) wait: Option<InlineWaitEdit>,
    pub(super) _subscription: Subscription,
}

pub(super) struct InlineWaitEdit {
    pub(super) ordinal: usize,
    pub(super) input: Entity<InputState>,
    pub(super) _subscription: Subscription,
}

#[derive(Clone, Debug)]
pub(super) enum FileEditMode {
    NewFile { parent: PathBuf },
    NewFolder { parent: PathBuf },
    Rename { path: PathBuf },
}

#[derive(Clone, Debug)]
pub(super) struct FileProgress {
    pub(super) completed: usize,
    pub(super) total: usize,
}

#[derive(Clone, Debug)]
pub(super) struct FileContextMenu {
    pub(super) path: Option<PathBuf>,
    pub(super) position: Point<Pixels>,
    pub(super) epoch: u64,
    pub(super) closing: bool,
}

#[derive(Clone, Debug)]
pub(super) enum SceneEditMode {
    New,
    Rename { start: usize, old_name: String },
}

#[derive(Clone, Debug)]
pub(super) struct SceneContextMenu {
    pub(super) start: usize,
    pub(super) name: String,
    pub(super) position: Point<Pixels>,
    pub(super) epoch: u64,
    pub(super) closing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AssetFilterGroup {
    Tags,
    Status,
    FileSize,
    Modified,
    Type,
    Folder,
    Sort,
    View,
    Size,
    Show,
}

#[derive(Clone, Debug)]
pub(super) enum AssetFilterChoice {
    Tags(Option<String>),
    Status(crate::authoring::AssetStatus),
    FileSize(crate::authoring::AssetSize),
    Modified(Option<Duration>),
    Type(Option<AssetKind>),
    Folder(Option<PathBuf>),
    Sort(AssetSort),
    View(Option<bool>),
    Size(bool),
    Show(bool),
}

#[derive(Clone, Debug)]
pub(super) struct AssetFilterMenu {
    pub(super) position: Point<Pixels>,
    pub(super) epoch: u64,
    pub(super) closing: bool,
    pub(super) expanded: Option<AssetFilterGroup>,
}

#[derive(Clone, Debug)]
pub(super) struct FileDrag {
    pub(super) relative: PathBuf,
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
pub(super) struct InspectorEditKey {
    pub(super) path: PathBuf,
    pub(super) block_start: usize,
    pub(super) metadata: TextBlockMetadata,
    pub(super) lifetime: TextLifetime,
}

#[derive(Clone)]
pub(super) struct BlockDrag {
    pub(super) token: Rc<()>,
    pub(super) document: DocumentHandle,
    pub(super) revision: u64,
    pub(super) selected: HashSet<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BlockDropTarget {
    pub(super) row: usize,
    pub(super) after: bool,
}

pub(super) struct BlockDragPreview {
    pub(super) label: String,
    pub(super) summary: String,
    pub(super) icon: AssetIconName,
    pub(super) count: usize,
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) color: u32,
    pub(super) grip_top: f32,
}

impl Render for BlockDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.width))
            .h(px(self.height))
            .relative()
            .left(px(-8.))
            .top(px(-self.grip_top))
            .overflow_hidden()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .rounded(px(4.))
            .bg(rgb(SURFACE_HOVER))
            .text_sm()
            .text_color(rgb(INK))
            .child(
                div()
                    .size(px(18.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(AssetIconName::GripVertical)
                            .xsmall()
                            .text_color(rgb(0x686e75)),
                    ),
            )
            .child(
                div()
                    .h(px(24.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(6.))
                    .rounded(px(3.))
                    .bg(gpui_kit::rgba((self.color << 8) | 0x20))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(13.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(rgb(self.color))
                    .child(Icon::new(self.icon).xsmall())
                    .child(self.label.clone()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(if self.count == 1 {
                        self.summary.clone()
                    } else {
                        format!("{} blocks", self.count)
                    }),
            )
    }
}

#[derive(Clone)]
pub(super) struct AssetDrag {
    pub(super) root: PathBuf,
    pub(super) keys: Vec<AssetKey>,
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

pub(super) struct DraftTextBlock {
    pub(super) target: DraftInsertionTarget,
    pub(super) text_range: Option<Range<usize>>,
    pub(super) last_escaped: String,
    pub(super) state: Entity<TextareaState>,
}

#[derive(Clone, Copy)]
pub(super) enum DraftInsertionTarget {
    Before(usize),
    After(usize),
    SceneEnd(usize),
}

#[derive(Clone, Copy)]
pub(super) enum BlockMenuAction {
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
    pub(super) fn from_payload(
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
            PanelContent::Output { root, .. } => Some((root.clone(), OUTPUT_PANEL)),
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
                block_drag: Default::default(),
                block_row_bounds: Rc::new(RefCell::new(HashMap::new())),
                block_row_positions: RefCell::new(HashMap::new()),
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
                minimap_navigation: minimap::Navigation::default(),
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
                block_layout: RefCell::new(super::blocks::layout::Cache::default()),
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
                asset_rename_file: true,
                batch_block_field: None,
                batch_block_input: cx.new(|cx| InputState::new(window, cx).placeholder("Value")),
                asset_batch_tags: cx
                    .new(|cx| InputState::new(window, cx).placeholder("Tags, separated by commas")),
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
                asset_tag: None,
                asset_status: crate::authoring::AssetStatus::All,
                asset_size: crate::authoring::AssetSize::All,
                asset_modified: None,
                asset_grid: None,
                asset_large: false,
                asset_browser: RefCell::new(super::resource::browse::Cache::default()),
                asset_thumbnails: super::resource::thumbnail::Thumbnails::new(cx),
                asset_unmapped: false,
                asset_anchor: None,
                asset_filter_menu: None,
                asset_filter_epoch: 0,
            };
            if matches!(panel.content, PanelContent::Search { .. }) {
                panel.install_search(window, cx);
            }
            if let PanelContent::Performance { controller, .. } = &panel.content {
                let controller = controller.clone();
                cx.spawn_in(window, async move |this, cx| {
                    let mut revision = controller.performance().revision;
                    loop {
                        cx.background_executor()
                            .timer(crate::preview::performance::SAMPLE_INTERVAL)
                            .await;
                        let latest = controller.performance().revision;
                        if this
                            .update_in(cx, |_, _, cx| {
                                if latest != revision {
                                    cx.notify();
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                        revision = latest;
                    }
                })
                .detach();
            }
            panel._subscriptions.push(cx.subscribe(
                &asset_search,
                |panel: &mut WorkbenchPanel, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        panel
                            .view_scroll
                            .set_offset(gpui_kit::point(px(0.), px(0.)));
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
            PanelContent::Output { root, .. } => {
                cx.global_mut::<EditorDocuments>()
                    .set_tool_panel(root, OUTPUT_PANEL, None)
            }
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

pub(super) fn language_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("shou") => "eiyashou",
        Some("json") => "json",
        Some("yaml" | "yml") => "yaml",
        Some("md") => "markdown",
        Some("toml") => "toml",
        _ => "plaintext",
    }
}

pub(super) fn register_workbench_panels(cx: &mut App) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
