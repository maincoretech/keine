//! Workbench panel state and dock lifecycle.
mod construction;

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
use gpui_kit::base::input::RopeExt;
use gpui_kit::component::dock::{
    BasePanel, Panel, PanelBuildContext, PanelEvent, PanelId, PanelInfo, PanelState, panel_handle,
    register_panel,
};
use gpui_kit::component::input::{EditorState, InputEvent, InputState, TextareaState};
use gpui_kit::component::notification::Notification;
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
use crate::projection::{EiyashouProjection, TextBlockMetadata, TextLifetime};
use crate::syntax::editor_highlighter_factory;
use crate::workspace::WorkspaceFile;

use super::documents::{EditorDocuments, schedule_authoring_refresh};
use super::edits::block_at_position;
use super::files::FileHistory;
use super::resource::ResourcePicker;
use super::{
    ASSET_PREVIEW_PANEL, ASSETS_PANEL, BUILD_PANEL, CHARACTERS_PANEL, DOCUMENT_PANEL,
    EXPLORER_PANEL, INK, INSPECTOR_PANEL, OUTPUT_PANEL, PERFORMANCE_PANEL, PREVIEW_SOURCE_DEBOUNCE,
    PRIMARY, PROBLEMS_PANEL, SCENES_PANEL, SEARCH_PANEL, SURFACE, SURFACE_HOVER, completion,
    minimap, search, text_minimap,
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
        #[serde(default)]
        view: DocumentView,
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
    Build {
        root: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct DocumentView {
    mode: DocumentMode,
    line: usize,
    column: usize,
}

#[derive(Clone, Copy)]
pub(super) enum ToolKind {
    Explorer,
    Search,
    Assets,
    Characters,
    Inspector,
    Problems,
    Performance,
    Output,
    Build,
}

impl ToolKind {
    pub(super) fn panel_name(self) -> &'static str {
        match self {
            Self::Explorer => EXPLORER_PANEL,
            Self::Search => SEARCH_PANEL,
            Self::Assets => ASSETS_PANEL,
            Self::Characters => CHARACTERS_PANEL,
            Self::Inspector => INSPECTOR_PANEL,
            Self::Problems => PROBLEMS_PANEL,
            Self::Performance => PERFORMANCE_PANEL,
            Self::Output => OUTPUT_PANEL,
            Self::Build => BUILD_PANEL,
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
            Self::Inspector => PanelPayload::Inspector { root },
            Self::Problems => PanelPayload::Problems { root },
            Self::Performance => PanelPayload::Performance { root },
            Self::Output => PanelPayload::Output { root },
            Self::Build => PanelPayload::Build { root },
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
    Build {
        root: PathBuf,
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
            PanelPayload::Document { root, relative, .. } => {
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
                        let provider = Rc::new(completion::ShouCompletion {
                            root: root.clone(),
                            relative: relative.clone(),
                        });
                        editor.lsp_mut().completion_provider = Some(provider.clone());
                        editor.lsp_mut().hover_provider = Some(provider);
                        editor.lsp_mut().completion_menu.max_width = px(400.);
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
            PanelPayload::Build { root } => Ok(Self::Build { root }),
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
                view: DocumentView::default(),
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
            Self::Build { root } => PanelPayload::Build { root: root.clone() },
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
            Self::Build { .. } => BUILD_PANEL,
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
            Self::Build { .. } => "Build".into(),
        }
    }
}

pub(super) struct WorkbenchPanel {
    pub(super) content: PanelContent,
    pub(super) focus: FocusHandle,
    pub(super) view_scroll: ScrollHandle,
    pub(super) project_search: Option<search::ProjectSearch>,
    pub(super) _subscriptions: Vec<Subscription>,
    pub(super) resource_picker: Option<ResourcePicker>,
    pub(super) picker: super::blocks::PickerState,
    pub(super) inspector: super::inspector::InspectorState,
    pub(super) assets: super::resource::AssetsState,
    pub(super) explorer: super::files::ExplorerState,
    pub(super) characters: super::characters::CharactersState,
    pub(super) document: super::document::DocumentState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
    pub(super) reveal_epoch: u64,
    pub(super) epoch: u64,
    pub(super) closing: bool,
    pub(super) expanded: Option<AssetFilterGroup>,
}

#[derive(Clone)]
pub(super) struct BlockContextMenu {
    pub(super) row: usize,
    pub(super) position: Point<Pixels>,
    pub(super) source: String,
    pub(super) epoch: usize,
    pub(super) closing: bool,
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
    pub(super) token: Rc<()>,
    pub(super) root: PathBuf,
    pub(super) keys: Vec<AssetKey>,
    pub(super) preview_offset: Point<Pixels>,
}

impl Render for AssetDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .left(self.preview_offset.x + px(12.))
            .top(self.preview_offset.y + px(12.))
            .flex()
            .items_center()
            .gap_2()
            .max_w(px(240.))
            .px_2()
            .py_1()
            .rounded(px(5.))
            .bg(rgb(SURFACE))
            .text_xs()
            .text_color(rgb(INK))
            .child(
                Icon::new(match self.keys.first().map(|key| key.kind) {
                    Some(AssetKind::Background | AssetKind::Figure | AssetKind::Particle) => {
                        AssetIconName::Image
                    }
                    Some(AssetKind::Bgm | AssetKind::Voice | AssetKind::Effect) => {
                        AssetIconName::Music
                    }
                    Some(AssetKind::Video) => AssetIconName::Film,
                    None => AssetIconName::File,
                })
                .xsmall()
                .text_color(rgb(PRIMARY)),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(if self.keys.len() == 1 {
                        self.keys[0].id.clone()
                    } else {
                        format!("{} assets", self.keys.len())
                    }),
            )
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

impl BasePanel for WorkbenchPanel {
    fn panel_name(&self) -> &'static str {
        self.content.panel_name()
    }

    fn dump(&self, cx: &App) -> PanelState {
        let mut payload = self.content.payload();
        if let PanelPayload::Document { view, .. } = &mut payload
            && let PanelContent::Document { editor, .. } = &self.content
        {
            let editor = editor.read(cx);
            let position = if self.document.document_mode == DocumentMode::Block {
                self.document
                    .block_selection_anchor
                    .map(|offset| {
                        editor
                            .text()
                            .offset_to_position(offset.min(editor.text().len()))
                    })
                    .unwrap_or_else(|| editor.cursor_position())
            } else {
                editor.cursor_position()
            };
            *view = DocumentView {
                mode: self.document.document_mode,
                line: position.line as usize,
                column: position.character as usize,
            };
        }
        PanelState {
            panel_name: self.panel_name().to_owned(),
            children: Vec::new(),
            info: PanelInfo::panel(
                serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
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
            PanelContent::Build { root } => {
                cx.global_mut::<EditorDocuments>()
                    .set_tool_panel(root, BUILD_PANEL, None)
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
        BUILD_PANEL,
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
    fn document_layout_restores_mode_and_position_and_accepts_old_layouts() {
        let legacy: PanelPayload = serde_json::from_value(serde_json::json!({
            "kind": "document", "root": "project", "relative": "scripts/main.shou"
        }))
        .unwrap();
        assert!(
            matches!(legacy, PanelPayload::Document { view, .. } if view == DocumentView::default())
        );
        let payload = PanelPayload::Document {
            root: "project".into(),
            relative: "scripts/main.shou".into(),
            view: DocumentView {
                mode: DocumentMode::Block,
                line: 120,
                column: 4,
            },
        };
        let restored: PanelPayload =
            serde_json::from_value(serde_json::to_value(payload).unwrap()).unwrap();
        assert!(matches!(restored, PanelPayload::Document { view, .. }
            if view == DocumentView { mode: DocumentMode::Block, line: 120, column: 4 }));
    }

    #[gpui_kit::test]
    fn asset_filter_reset_preserves_view_and_selection_and_enter_commits_once(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::VisualTestContext;
        let temporary =
            std::env::temp_dir().join(format!("keine-panel-events-{}", std::process::id()));
        let root = temporary.join("project");
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        for name in [
            "config.yaml",
            "assets.yaml",
            "characters.yaml",
            "scripts/main.shou",
        ] {
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/native-smoke")
                .join(name);
            std::fs::copy(source, root.join(name)).unwrap();
        }
        let window = cx.update(|cx| {
            gpui_kit::init(cx);
            let session = crate::workspace::WorkspaceSession::open(&root).unwrap();
            let mut documents = EditorDocuments::new(crate::persistence::AppPersistence::new(
                temporary.join("app-data"),
            ));
            documents
                .ensure_workspace_with_files(session.root(), session.files())
                .unwrap();
            cx.set_global(documents);
            cx.open_window(gpui_kit::WindowOptions::default(), |window, cx| {
                WorkbenchPanel::from_payload(
                    PanelPayload::Explorer {
                        root: session.root().to_owned(),
                        expanded: vec![],
                    },
                    window,
                    cx,
                )
                .unwrap()
            })
            .unwrap()
        });
        let panel = window.root(cx).unwrap();
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let root = root.canonicalize().unwrap();
        let selection = vec![AssetKey {
            kind: AssetKind::Background,
            id: "keep_selection".into(),
        }];
        cx.update(|window, cx| {
            cx.global_mut::<EditorDocuments>()
                .set_asset_selection(&root, selection.clone());
            panel.update(cx, |panel, cx| {
                panel.assets.asset_sort = AssetSort::Size;
                panel.assets.asset_grid = Some(true);
                panel.assets.asset_large = true;
                panel.assets.asset_kind = Some(AssetKind::Background);
                panel.assets.asset_folder = Some("assets/backgrounds".into());
                panel.assets.asset_tag = Some("outdoors".into());
                panel.assets.asset_status = crate::authoring::AssetStatus::Unused;
                panel.assets.asset_size = crate::authoring::AssetSize::Large;
                panel.assets.asset_modified = Some(Duration::from_secs(86400));
                panel.assets.asset_unmapped = true;
                panel
                    .assets
                    .asset_search
                    .update(cx, |input, cx| input.set_value("missing", window, cx));
                assert!(panel.assets.has_filters(cx));
                panel.clear_asset_filters(window, cx);
                assert!(!panel.assets.has_filters(cx));
                assert_eq!(panel.assets.asset_sort, AssetSort::Size);
                assert_eq!(panel.assets.asset_grid, Some(true));
                assert!(panel.assets.asset_large);
                assert_eq!(
                    cx.global::<EditorDocuments>().asset_selection(&root),
                    selection
                );
                panel.begin_file_edit(
                    FileEditMode::NewFile {
                        parent: "scripts".into(),
                    },
                    "enter.shou",
                    window,
                    cx,
                );
            });
        });
        cx.run_until_parked();
        // Rendering and field changes do not create the file. Submission owns the transaction.
        assert!(!root.join("scripts/enter.shou").exists());
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.explorer.file_name_input.update(cx, |_, cx| {
                    cx.emit(InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    })
                });
            });
        });
        cx.run_until_parked();
        assert!(root.join("scripts/enter.shou").is_file());
        panel.read_with(cx, |panel, _| assert!(panel.explorer.file_edit.is_none()));
        let original = std::fs::read(root.join("scripts/main.shou")).unwrap();
        assert_eq!(
            original,
            std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/fixtures/native-smoke/scripts/main.shou")
            )
            .unwrap()
        );
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[gpui_kit::test]
    fn composition_and_text_continuation_keep_source_focus_and_position(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::base::input::Position;
        use gpui_kit::{EntityInputHandler, VisualTestContext};
        let temporary =
            std::env::temp_dir().join(format!("keine-text-composition-{}", std::process::id()));
        let root = temporary.join("project");
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(
            root.join("config.yaml"),
            include_str!("../../../../tests/fixtures/native-smoke/config.yaml"),
        )
        .unwrap();
        let path = PathBuf::from("scripts/main.shou");
        let source = "scene start {\n  \"before\",\n  \"middle\",\n  \"after\",\n}";
        std::fs::write(root.join(&path), source).unwrap();
        let window = cx.update(|cx| {
            gpui_kit::init(cx);
            let session = crate::workspace::WorkspaceSession::open(&root).unwrap();
            let mut documents = EditorDocuments::new(crate::persistence::AppPersistence::new(
                temporary.join("app-data"),
            ));
            documents
                .ensure_workspace_with_files(session.root(), session.files())
                .unwrap();
            cx.set_global(documents);
            cx.open_window(gpui_kit::WindowOptions::default(), |window, cx| {
                WorkbenchPanel::from_payload(
                    PanelPayload::Document {
                        root: session.root().to_owned(),
                        relative: path.clone(),
                        view: DocumentView {
                            mode: DocumentMode::Text,
                            line: 2,
                            column: 3,
                        },
                    },
                    window,
                    cx,
                )
                .unwrap()
            })
            .unwrap()
        });
        let panel = window.root(cx).unwrap();
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let (editor, document, root) = panel.read_with(cx, |panel, _| {
            let PanelContent::Document {
                editor,
                document: Some(document),
                root,
                ..
            } = &panel.content
            else {
                unreachable!()
            };
            (editor.clone(), document.clone(), root.clone())
        });
        let selection = cx.read(|cx| cx.global::<EditorDocuments>().selection(&root).cloned());
        for preedit in ["n", "ni", "你"] {
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.replace_and_mark_text_in_range(None, preedit, None, window, cx);
                })
            });
            cx.run_until_parked();
            assert_eq!(document.borrow().contents(), source);
            assert_eq!(
                cx.read(|cx| cx.global::<EditorDocuments>().selection(&root).cloned()),
                selection
            );
        }
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.replace_text_in_range(None, "你", window, cx);
            })
        });
        cx.run_until_parked();
        assert!(document.borrow().contents().contains("\"你middle\""));
        let saved = panel.read_with(cx, |panel, cx| panel.dump(cx));
        let payload = serde_json::from_value::<PanelPayload>(match saved.info {
            PanelInfo::Panel(value) => value,
            _ => unreachable!(),
        })
        .unwrap();
        cx.update(|window, cx| {
            let restored = WorkbenchPanel::from_payload(payload, window, cx).unwrap();
            let PanelContent::Document {
                editor: restored_editor,
                ..
            } = &restored.read(cx).content
            else {
                unreachable!()
            };
            assert_eq!(
                restored_editor.read(cx).cursor_position(),
                Position::new(2, 4)
            );
        });
        cx.update(|window, cx| {
            cx.bind_keys([gpui_kit::KeyBinding::new(
                "enter",
                super::super::BeginTextBlock,
                Some("KeineBlockView"),
            )]);
            panel.update(cx, |panel, cx| {
                panel.switch_document_mode(DocumentMode::Block, window, cx);
                panel.sync_visual_editors(window, cx);
                let start = document.borrow().contents().find("你middle").unwrap();
                let row = panel
                    .document
                    .block_text_editors
                    .iter()
                    .find(|row| row.text_start == start)
                    .unwrap();
                row.state.update(cx, |state, cx| state.focus(window, cx));
            });
        });
        cx.run_until_parked();
        let row = panel.read_with(cx, |panel, cx| {
            panel
                .document
                .block_text_editors
                .iter()
                .find(|row| row.state.read(cx).value() == "你middle")
                .unwrap()
                .state
                .clone()
        });
        cx.update(|_, cx| {
            row.update(cx, |state, cx| {
                let end = state.value().len();
                state.set_selected_range(end..end, cx);
            })
        });
        let selection = cx.read(|cx| cx.global::<EditorDocuments>().selection(&root).cloned());
        let index_epoch =
            cx.read(|cx| cx.global::<EditorDocuments>().workspaces[&root].index_epoch);
        let source_selection = editor.read_with(cx, |editor, _| editor.selected_range());
        for expected in ["你middl", "你midd", "你mid"] {
            cx.simulate_keystrokes("backspace");
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(row.read(cx).focus_handle(cx).is_focused(window));
                assert_eq!(row.read(cx).value(), expected);
                assert!(
                    document
                        .borrow()
                        .contents()
                        .contains(&format!("\"{expected}\""))
                );
                assert_eq!(editor.read(cx).selected_range(), source_selection);
                let documents = cx.global::<EditorDocuments>();
                assert_eq!(documents.selection(&root).cloned(), selection);
                assert_eq!(documents.workspaces[&root].index_epoch, index_epoch);
                let panel = panel.read(cx);
                assert!(panel.document.block_text_refresh_pending);
                assert!(
                    panel
                        .document
                        .block_text_editors
                        .iter()
                        .any(|editor| editor.state == row)
                );
            });
        }
        cx.simulate_input("dle");
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert!(!panel.read_with(cx, |panel, _| panel.document.block_text_refresh_pending));
        assert!(
            cx.read(|cx| cx.global::<EditorDocuments>().workspaces[&root].index_epoch)
                > index_epoch
        );
        let draft = panel.read_with(cx, |panel, _| {
            panel.document.draft_text.as_ref().unwrap().state.clone()
        });
        cx.simulate_input("inserted");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(draft.read(cx).focus_handle(cx).is_focused(window));
            assert!(
                document
                    .borrow()
                    .contents()
                    .contains("\"你middle\",\n  \"inserted\",\n  \"after\""),
                "{}",
                document.borrow().contents()
            );
        });
        cx.simulate_keystrokes("shift-enter");
        cx.simulate_input("second line");
        cx.run_until_parked();
        assert_eq!(
            draft.read_with(cx, |state, _| state.value().to_string()),
            "inserted\nsecond line"
        );
        cx.update(|window, cx| {
            let documents = cx.global_mut::<EditorDocuments>();
            documents.register_panel(
                &root,
                path.clone(),
                PanelId::from(panel.entity_id()),
                panel.downgrade(),
            );
            documents.register_editor(&root, path.clone(), editor.downgrade());
            assert!(panel.read(cx).document.block_text_refresh_pending);
            super::super::edits::format_and_save(&root, window, cx).unwrap();
            assert!(!panel.read(cx).document.block_text_refresh_pending);
            assert!(draft.read(cx).focus_handle(cx).is_focused(window));
            assert!(!document.borrow().is_dirty());
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            assert_ne!(panel.document.draft_text.as_ref().unwrap().state, draft);
            assert_eq!(document.borrow().contents().matches("inserted").count(), 1);
        });
        // Saving on close can reformat the file before the layout is dumped.
        // Position memory must follow the same block through that rewrite.
        let unformatted = document.borrow().contents().replace("\n  ", "\n    ");
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.replace_all(unformatted.clone(), window, cx)
            });
            let documents = cx.global_mut::<EditorDocuments>();
            documents.register_panel(
                &root,
                path.clone(),
                PanelId::from(panel.entity_id()),
                panel.downgrade(),
            );
            documents.register_editor(&root, path.clone(), editor.downgrade());
            panel.update(cx, |panel, _| {
                let start = unformatted.find("\"inserted").unwrap();
                panel.document.selected_blocks = HashSet::from([start]);
                panel.document.block_selection_anchor = Some(start);
                panel.document.draft_text = None;
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let offset = unformatted.find("inserted").unwrap();
                editor.set_selected_range(offset..offset, cx);
            });
            let focus = panel.read(cx).focus.clone();
            focus.focus(window, cx);
            assert!(!editor.read(cx).focus_handle(cx).is_focused(window));
            assert_eq!(
                super::super::edits::format_and_save(&root, window, cx).unwrap(),
                // Formatting restores the version already saved above.
                0
            );
            assert_eq!(
                panel.read(cx).document.block_selection_anchor,
                document.borrow().contents().find("\"inserted")
            );
            assert_eq!(
                editor.read(cx).cursor(),
                document.borrow().contents().find("inserted").unwrap()
            );
            assert!(!editor.read(cx).focus_handle(cx).is_focused(window));
        });
        let saved = panel.read_with(cx, |panel, cx| panel.dump(cx));
        let payload = serde_json::from_value::<PanelPayload>(match saved.info {
            PanelInfo::Panel(value) => value,
            _ => unreachable!(),
        })
        .unwrap();
        cx.update(|window, cx| {
            let restored = WorkbenchPanel::from_payload(payload, window, cx).unwrap();
            let restored = restored.read(cx);
            assert_eq!(restored.document.document_mode, DocumentMode::Block);
            assert_eq!(
                restored.document.block_selection_anchor,
                panel.read(cx).document.block_selection_anchor
            );
            assert!(restored.document.block_scroll_pending);
        });
        std::fs::remove_dir_all(temporary).unwrap();
    }

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
