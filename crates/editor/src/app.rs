use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::base::{Placement, ResizeHandleContext};
use gpui_kit::component::dock::{
    AnyDrag, BasePanel, BasePanelView, DockArea, DockAreaRenderer, DockContext, DockEvent,
    DockLayout, DockSkin, DragPanel, DropIndicator, DropPlaceholderBounds, NodeId, Panel,
    PanelBuildContext, PanelEvent, PanelHandle, PanelInfo, PanelState, PanelStyle, TabGroupContext,
    TabGroupRenderer, panel_handle, register_panel,
};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, Theme, ThemeMode};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext as _, Axis, Bounds, Context, Div, DragMoveEvent, Empty,
    Entity, EventEmitter, FocusHandle, Focusable, Hsla, IntoElement, KeyBinding, PathPromptOptions,
    Pixels, Point, Render, SharedString, Stateful, Subscription, WeakEntity, Window, WindowBounds,
    WindowHandle, WindowOptions, actions, div, hsla, linear_color_stop, linear_gradient,
    prelude::*, px, rgb, size,
};
use serde::{Deserialize, Serialize};

use crate::instance::{InstanceReceiver, PrimaryInstance, Startup, acquire_or_forward};
use crate::persistence::AppPersistence;
use crate::project_key::ProjectKey;
use crate::workspace::{TextDocument, WorkspaceFile, WorkspaceSession};

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
const VIEW_GAP_PX: f32 = 2.;
const VIEW_RADIUS_PX: f32 = 9.;
const TAB_MOTION_DURATION: Duration = Duration::from_millis(140);

const EXPLORER_PANEL: &str = "keine.editor.explorer";
const DOCUMENT_PANEL: &str = "keine.editor.document";
const INSPECTOR_PANEL: &str = "keine.editor.inspector";
const OUTPUT_PANEL: &str = "keine.editor.output";

actions!(keine_editor, [OpenFolder, ResetLayout]);

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
                cx.new(|cx| WorkbenchWindow::empty(editor, persistence, recents, cx))
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
}

#[derive(Clone)]
enum PanelContent {
    Explorer {
        root: PathBuf,
        files: Vec<WorkspaceFile>,
    },
    Document {
        root: PathBuf,
        document: TextDocument,
    },
    Inspector {
        root: PathBuf,
        file_count: usize,
        document_count: usize,
    },
    Output {
        root: PathBuf,
        file_count: usize,
    },
}

impl PanelContent {
    fn from_payload(payload: PanelPayload) -> Self {
        match payload {
            PanelPayload::Explorer { root } => WorkspaceSession::open(&root)
                .map(|session| Self::Explorer {
                    root: root.clone(),
                    files: session.files().to_vec(),
                })
                .unwrap_or(Self::Explorer {
                    root,
                    files: Vec::new(),
                }),
            PanelPayload::Document { root, relative } => {
                let contents = std::fs::read_to_string(root.join(&relative))
                    .unwrap_or_else(|error| format!("Unable to read document: {error}"));
                Self::Document {
                    root,
                    document: TextDocument {
                        relative_path: relative,
                        contents,
                    },
                }
            }
            PanelPayload::Inspector { root } => WorkspaceSession::open(&root)
                .map(|session| Self::Inspector {
                    root: root.clone(),
                    file_count: session.files().len(),
                    document_count: session.documents().len(),
                })
                .unwrap_or(Self::Inspector {
                    root,
                    file_count: 0,
                    document_count: 0,
                }),
            PanelPayload::Output { root } => WorkspaceSession::open(&root)
                .map(|session| Self::Output {
                    root: root.clone(),
                    file_count: session.files().len(),
                })
                .unwrap_or(Self::Output {
                    root,
                    file_count: 0,
                }),
        }
    }

    fn payload(&self) -> PanelPayload {
        match self {
            Self::Explorer { root, .. } => PanelPayload::Explorer { root: root.clone() },
            Self::Document { root, document } => PanelPayload::Document {
                root: root.clone(),
                relative: document.relative_path.clone(),
            },
            Self::Inspector { root, .. } => PanelPayload::Inspector { root: root.clone() },
            Self::Output { root, .. } => PanelPayload::Output { root: root.clone() },
        }
    }

    fn panel_name(&self) -> &'static str {
        match self {
            Self::Explorer { .. } => EXPLORER_PANEL,
            Self::Document { .. } => DOCUMENT_PANEL,
            Self::Inspector { .. } => INSPECTOR_PANEL,
            Self::Output { .. } => OUTPUT_PANEL,
        }
    }

    fn title(&self) -> SharedString {
        match self {
            Self::Explorer { .. } => "Explorer".into(),
            Self::Document { document, .. } => document
                .relative_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Document")
                .to_owned()
                .into(),
            Self::Inspector { .. } => "Inspector".into(),
            Self::Output { .. } => "Output".into(),
        }
    }
}

struct WorkbenchPanel {
    content: PanelContent,
    focus: FocusHandle,
}

impl WorkbenchPanel {
    fn new(content: PanelContent, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            content,
            focus: cx.focus_handle(),
        })
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = Theme::global(cx).mono_font_family.clone();
        let body = match &self.content {
            PanelContent::Explorer { root, files } => {
                let project_name = root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("PROJECT")
                    .to_uppercase();
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
                                                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
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
            PanelContent::Document { document, .. } => div()
                .id("document-content")
                .size_full()
                .overflow_scroll()
                .rounded_b(px(VIEW_RADIUS_PX))
                .bg(rgb(0x10151b))
                .p_3()
                .font_family(mono)
                .text_xs()
                .line_height(px(20.))
                .text_color(rgb(0xb8c4cf))
                .child(document_preview(&document.contents))
                .into_any_element(),
            PanelContent::Inspector {
                root,
                file_count,
                document_count,
            } => div()
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
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child("No structured selection"),
                )
                .into_any_element(),
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
                .into_any_element(),
        };
        div()
            .track_focus(&self.focus)
            .size_full()
            .text_color(rgb(INK))
            .child(body)
    }
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

fn document_preview(contents: &str) -> SharedString {
    let mut preview = contents.lines().take(600).collect::<Vec<_>>().join("\n");
    if contents.lines().nth(600).is_some() {
        preview.push_str("\n\n… document preview limited to 600 lines");
    }
    preview.into()
}

fn register_workbench_panels(cx: &mut App) {
    for name in [
        EXPLORER_PANEL,
        DOCUMENT_PANEL,
        INSPECTOR_PANEL,
        OUTPUT_PANEL,
    ] {
        register_panel(cx, name, |context, _, cx| {
            panel_handle(WorkbenchPanel::new(panel_content(context), cx))
        });
    }
}

fn panel_content(context: PanelBuildContext<'_>) -> PanelContent {
    match context.info() {
        PanelInfo::Panel(value) => serde_json::from_value::<PanelPayload>(value.clone())
            .map(PanelContent::from_payload)
            .unwrap_or_else(|_| PanelContent::Output {
                root: PathBuf::new(),
                file_count: 0,
            }),
        _ => PanelContent::Output {
            root: PathBuf::new(),
            file_count: 0,
        },
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
            dock: self.dock.clone(),
            drop_overlays: self.drop_overlays.clone(),
            tab_motion: self.tab_motion.clone(),
        })
    }
}

struct EditorTabGroupSkin {
    inner: Rc<dyn TabGroupRenderer>,
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
        if panel.panel_name(cx) == DOCUMENT_PANEL {
            return None;
        }

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
            .p(px(VIEW_GAP_PX))
            .rounded(px(VIEW_RADIUS_PX + VIEW_GAP_PX))
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
                dock: dock_ref_for_skin,
                drop_overlays: drop_overlays_for_skin,
                tab_motion,
            }))
        });
        *dock_ref.borrow_mut() = Some(dock.downgrade());
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
    let explorer = WorkbenchPanel::new(
        PanelContent::Explorer {
            root: session.root().to_owned(),
            files: session.files().to_vec(),
        },
        cx,
    );
    let mut documents = session
        .documents()
        .iter()
        .cloned()
        .map(|document| {
            WorkbenchPanel::new(
                PanelContent::Document {
                    root: session.root().to_owned(),
                    document,
                },
                cx,
            )
        })
        .collect::<Vec<_>>();
    while documents.len() < 2 {
        let number = documents.len() + 1;
        documents.push(WorkbenchPanel::new(
            PanelContent::Document {
                root: session.root().to_owned(),
                document: TextDocument {
                    relative_path: PathBuf::from(format!("No document {number}")),
                    contents: "No additional readable text document was found.".into(),
                },
            },
            cx,
        ));
    }
    let inspector = WorkbenchPanel::new(
        PanelContent::Inspector {
            root: session.root().to_owned(),
            file_count: session.files().len(),
            document_count: session.documents().len(),
        },
        cx,
    );
    let output = WorkbenchPanel::new(
        PanelContent::Output {
            root: session.root().to_owned(),
            file_count: session.files().len(),
        },
        cx,
    );
    let layout = DockLayout::h_split()
        .child(
            DockLayout::tabs().panel_view(panel_handle(explorer), cx),
            Some(px(220.)),
        )
        .child(
            DockLayout::v_split()
                .child(
                    DockLayout::tabs()
                        .panel_view(panel_handle(documents.remove(0)), cx)
                        .panel_view(panel_handle(documents.remove(0)), cx),
                    None,
                )
                .child(
                    DockLayout::tabs().panel_view(panel_handle(output), cx),
                    Some(px(150.)),
                ),
            None,
        )
        .child(
            DockLayout::tabs().panel_view(panel_handle(inspector), cx),
            Some(px(230.)),
        );
    dock.update(cx, |dock, cx| dock.set_center(layout, window, cx));
}

struct WorkbenchWindow {
    editor: WeakEntity<EditorApp>,
    persistence: AppPersistence,
    workspace: Option<ProjectWorkspace>,
    recents: Vec<PathBuf>,
    has_unsaved_changes: bool,
    focus: FocusHandle,
}

impl WorkbenchWindow {
    fn empty(
        editor: WeakEntity<EditorApp>,
        persistence: AppPersistence,
        recents: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            editor,
            persistence,
            workspace: None,
            recents,
            has_unsaved_changes: false,
            focus: cx.focus_handle(),
        }
    }

    fn project(
        editor: WeakEntity<EditorApp>,
        persistence: AppPersistence,
        session: WorkspaceSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let workspace = ProjectWorkspace::new(session, &persistence, window, cx);
        Self {
            editor,
            persistence,
            workspace: Some(workspace),
            recents: Vec::new(),
            has_unsaved_changes: false,
            focus: cx.focus_handle(),
        }
    }

    fn open_session(
        &mut self,
        session: WorkspaceSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace = Some(ProjectWorkspace::new(
            session,
            &self.persistence,
            window,
            cx,
        ));
        self.recents.clear();
        self.has_unsaved_changes = false;
        cx.notify();
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

    fn render_activity_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.clone();
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
                    .bg(if self.has_unsaved_changes {
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
            .child(activity_icon("activity-search", IconName::Search))
            .child(activity_icon("activity-inspector", IconName::Inspector))
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
                this.child(
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
            .p(px(VIEW_GAP_PX))
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
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .p(px(VIEW_GAP_PX))
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

fn activity_icon(id: &'static str, icon: IconName) -> impl IntoElement {
    div()
        .id(id)
        .size(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .child(Icon::new(icon).small().text_color(rgb(0x74818e)))
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

fn window_options(index: usize, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(offset_bounds(index, cx))),
        window_min_size: Some(size(px(720.), px(480.))),
        app_id: Some("moe.maincore.keine-editor".into()),
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
            register_workbench_panels(cx);
            cx.bind_keys([
                KeyBinding::new("cmd-o", OpenFolder, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-o", OpenFolder, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-0", ResetLayout, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-shift-0", ResetLayout, Some("KeineWorkbench")),
            ]);
            let persistence = AppPersistence::new(app_data.clone());
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
