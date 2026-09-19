use std::collections::HashMap;
use std::path::Path;

use gpui_kit::component::dock::{
    BasePanel, DockArea, DockLayout, DockSkin, Panel, PanelEvent, panel_handle,
};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{
    App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla, IntoElement, Render,
    SharedString, Window, WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, rgb,
    size,
};

use crate::project_key::ProjectKey;

const CANVAS: u32 = 0x141414;
const PANEL: u32 = 0x1a1a1a;
const SURFACE: u32 = 0x212121;
const INK: u32 = 0xc6c9d0;

fn theme_color(value: u32) -> Hsla {
    rgb(value).into()
}

fn configure_dark_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    let theme = Theme::global_mut(cx);

    // Material-style surface elevation: color is reserved for meaning and focus.
    theme.background = theme_color(0x121416);
    theme.foreground = theme_color(0xc6c9d0);
    theme.border = theme_color(0x30343a);
    theme.muted = theme_color(0x22262b);
    theme.muted_foreground = theme_color(0x7f8793);
    theme.accent = theme_color(0x20333b);
    theme.accent_foreground = theme_color(0xa3e4ff);
    theme.primary = theme_color(0xa3e4ff);
    theme.primary_hover = theme_color(0xc7efff);
    theme.primary_active = theme_color(0x7ccde9);
    theme.primary_foreground = theme_color(0x003544);
    theme.secondary = theme_color(0x26363d);
    theme.secondary_hover = theme_color(0x304650);
    theme.secondary_active = theme_color(0x1f2e34);
    theme.secondary_foreground = theme_color(0xb9d8e5);
    theme.success = theme_color(0x58b879);
    theme.success_foreground = theme_color(0x0d1811);
    theme.warning = theme_color(0xd6a756);
    theme.warning_foreground = theme_color(0x1c1508);
    theme.danger = theme_color(0xd66a72);
    theme.danger_foreground = theme_color(0x1c0d0f);
    theme.info = theme_color(0xa3e4ff);
    theme.info_foreground = theme_color(0x0d1320);
    theme.ring = theme_color(0xa3e4ff);
    theme.drag_border = theme_color(0xa3e4ff);
    theme.drop_target = theme_color(0x20333b);
    theme.selection = theme_color(0x294a57);
    theme.sidebar = theme_color(0x15181b);
    theme.sidebar_border = theme_color(0x30343a);
    theme.sidebar_foreground = theme_color(0x9da4ae);
    theme.sidebar_accent = theme_color(0x20333b);
    theme.sidebar_accent_foreground = theme_color(0xa3e4ff);
    theme.tab = theme_color(0x171a1e);
    theme.tab_bar = theme_color(0x101214);
    theme.tab_bar_segmented = theme_color(0x1a1e22);
    theme.tab_foreground = theme_color(0x858d98);
    theme.tab_active = theme_color(0x23282f);
    theme.tab_active_foreground = theme_color(0xc6c9d0);
    theme.title_bar = theme_color(0x101214);
    theme.title_bar_border = theme_color(0x30343a);
    theme.status_bar = theme_color(0x15181b);
    theme.status_bar_border = theme_color(0x30343a);
    theme.input = theme_color(0x30343a);
    theme.popover = theme_color(0x1b1f23);
    theme.popover_foreground = theme_color(0xc6c9d0);
    theme.button = theme_color(0x24292f);
    theme.button_hover = theme_color(0x2d333b);
    theme.button_active = theme_color(0x1d2227);
    theme.button_foreground = theme_color(0xb5bbc4);
    theme.radius = px(6.);
    theme.radius_lg = px(10.);
    theme.shadow = true;

    Theme::sync_base(cx);
}

#[derive(Debug, PartialEq, Eq)]
enum OpenRoute<W> {
    Created(W),
    FocusExisting(W),
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
    fn route(&mut self, project: ProjectKey, create: impl FnOnce() -> W) -> OpenRoute<W> {
        if let Some(window) = self.windows.get(&project).copied() {
            return OpenRoute::FocusExisting(window);
        }
        let window = create();
        self.windows.insert(project, window);
        OpenRoute::Created(window)
    }
}

#[derive(Clone, Copy)]
enum PanelKind {
    Explorer,
    ScriptA,
    ScriptB,
    Inspector,
}

impl PanelKind {
    const fn title(self) -> &'static str {
        match self {
            Self::Explorer => "Explorer",
            Self::ScriptA => "main.txt",
            Self::ScriptB => "chapter-2.txt",
            Self::Inspector => "Inspector",
        }
    }

    const fn accent(self) -> u32 {
        match self {
            Self::Explorer => 0x7ccde9,
            Self::ScriptA => 0xa3e4ff,
            Self::ScriptB => 0x8bd8f7,
            Self::Inspector => 0x6fabc2,
        }
    }
}

struct SpikePanel {
    kind: PanelKind,
    focus: FocusHandle,
}

impl SpikePanel {
    fn new(kind: PanelKind, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            kind,
            focus: cx.focus_handle(),
        })
    }
}

impl BasePanel for SpikePanel {
    fn panel_name(&self) -> &'static str {
        self.kind.title()
    }
}

impl Panel for SpikePanel {
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some(self.kind.title().into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.kind.title()
    }
}

impl EventEmitter<PanelEvent> for SpikePanel {}

impl Focusable for SpikePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SpikePanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(rgb(PANEL))
            .text_color(rgb(INK))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(self.kind.accent()))
                    .child(self.kind.title()),
            )
    }
}

struct ProjectWindow {
    project: ProjectKey,
    dock: Entity<DockArea>,
}

impl ProjectWindow {
    fn new(project: ProjectKey, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let explorer = SpikePanel::new(PanelKind::Explorer, cx);
        let script_a = SpikePanel::new(PanelKind::ScriptA, cx);
        let script_b = SpikePanel::new(PanelKind::ScriptB, cx);
        let inspector = SpikePanel::new(PanelKind::Inspector, cx);

        let (dock, _skin) = DockSkin::dock_area("phase0", Some(1), window, cx);
        dock.update(cx, |dock, cx| {
            dock.set_center(
                DockLayout::h_split()
                    .child(
                        DockLayout::tabs().panel_view(panel_handle(explorer), cx),
                        Some(px(220.)),
                    )
                    .child(
                        DockLayout::v_split()
                            .child(
                                DockLayout::tabs()
                                    .panel_view(panel_handle(script_a), cx)
                                    .panel_view(panel_handle(script_b), cx),
                                None,
                            )
                            .child(
                                DockLayout::tabs().panel_view(panel_handle(inspector), cx),
                                Some(px(180.)),
                            ),
                        None,
                    ),
                window,
                cx,
            );
        });

        Self { project, dock }
    }
}

impl Render for ProjectWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(CANVAS))
            .text_color(rgb(INK))
            .child(
                div()
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .px_3()
                    .bg(rgb(SURFACE))
                    .text_sm()
                    .child(self.project.path().display().to_string()),
            )
            .child(div().flex_1().min_h_0().child(self.dock.clone()))
    }
}

pub fn run() {
    gpui_kit::application()
        .with_quit_mode(gpui_kit::QuitMode::LastWindowClosed)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            configure_dark_theme(cx);
            let cwd = std::env::current_dir().expect("failed to locate the working directory");
            let projects = [cwd.clone(), cwd.join("projects/test-project")];
            let mut registry = WindowRegistry::<WindowHandle<ProjectWindow>>::default();

            for (index, path) in projects.iter().enumerate() {
                let project =
                    ProjectKey::from_path(path).expect("failed to identify dummy project");
                let bounds = offset_bounds(index, cx);
                let route = registry.route(project.clone(), || {
                    cx.open_window(
                        WindowOptions {
                            window_bounds: Some(WindowBounds::Windowed(bounds)),
                            ..Default::default()
                        },
                        move |window, cx| cx.new(|cx| ProjectWindow::new(project, window, cx)),
                    )
                    .expect("failed to open Phase 0 window")
                });
                debug_assert!(matches!(route, OpenRoute::Created(_)));
            }

            let duplicate = ProjectKey::from_path(Path::new(".")).expect("failed to identify cwd");
            if let OpenRoute::FocusExisting(window) = registry.route(duplicate, || unreachable!()) {
                let _ = window.update(cx, |_, window, _| window.activate_window());
            }
            cx.activate(true);
        });
}

fn offset_bounds(index: usize, cx: &App) -> Bounds<gpui_kit::Pixels> {
    let mut bounds = Bounds::centered(None, size(px(960.), px(640.)), cx);
    let offset = px(index as f32 * 56.);
    bounds.origin.x += offset;
    bounds.origin.y += offset;
    bounds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_project_routes_to_the_existing_window() {
        let project = ProjectKey::from_path(".").unwrap();
        let duplicate = ProjectKey::from_path(std::env::current_dir().unwrap()).unwrap();
        let mut registry = WindowRegistry::default();

        assert_eq!(registry.route(project, || 41), OpenRoute::Created(41));
        assert_eq!(
            registry.route(duplicate, || 99),
            OpenRoute::FocusExisting(41)
        );
        assert_eq!(registry.windows.len(), 1);
    }
}
