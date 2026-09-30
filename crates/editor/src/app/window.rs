//! Project windows, close/save protection and Engine child lifecycle.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::assets::IconName as AssetIconName;
use gpui_kit::component::dock::{DockPlacement, InsertTarget, PanelId, panel_handle};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Icon, IconName, Root, Sizable as _, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, Entity, FocusHandle, Global, IntoElement, PathPromptOptions, Pixels,
    Point, PromptButton, PromptLevel, Render, Subscription, WeakEntity, Window, WindowBounds,
    WindowHandle, WindowOptions, div, px, rgb, size,
};

use crate::app_data::APP_ID;
use crate::instance::{InstanceReceiver, PrimaryInstance};
use crate::migration::MigrationPlan;
use crate::persistence::AppPersistence;
use crate::preview::PreviewLifecycle;
use crate::project_key::ProjectKey;
use crate::workspace::WorkspaceSession;

use super::controls::preview_transport_button;
use super::dock::{ProjectWorkspace, install_default_layout};
use super::documents::EditorDocuments;
use super::edits::{follow_preview_position, replay_source_history};
use super::panel::{ToolKind, WorkbenchPanel};
use super::{
    ACTIVITY_BRAND_SIZE_PX, ACTIVITY_ICON_SIZE_PX, ACTIVITY_ITEM_SIZE_PX, ACTIVITY_RAIL_WIDTH_PX,
    ASSET_PREVIEW_PANEL, ASSETS_PANEL, CANVAS, CHARACTERS_PANEL, CHROME, EXPLORER_PANEL, INK,
    INSPECTOR_PANEL, MUTED, MigrateEiyashou, OpenFolder, PERFORMANCE_PANEL, PRIMARY, PRIMARY_DIM,
    PROBLEMS_PANEL, RedoSources, ResetLayout, SEARCH_PANEL, SURFACE, SURFACE_HOVER, Save, SaveAll,
    ShowAssetPreview, ShowAssets, ShowSearch, ToggleEngine, UndoSources, VIEW_INSET_PX,
    VIEW_RADIUS_PX, activity_divider, activity_tool, dock, icon_hint,
};

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

pub(super) struct EditorApp {
    this: WeakEntity<EditorApp>,
    windows: WindowRegistry<WindowHandle<Root>>,
    empty_window: Option<WindowHandle<Root>>,
    persistence: AppPersistence,
    _instance: PrimaryInstance,
}

pub(super) struct EditorAppOwner {
    pub(super) _editor: Entity<EditorApp>,
}

impl Global for EditorAppOwner {}

impl EditorApp {
    pub(super) fn new(
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

    pub(super) fn open_empty(&mut self, cx: &mut App) {
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

    pub(super) fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut App) {
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

pub(super) struct WorkbenchWindow {
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

pub(super) fn listen_for_secondary_launches(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_project_routes_to_the_existing_window() {
        let project = ProjectKey::from_path(".").unwrap();
        let duplicate = ProjectKey::from_path(std::env::current_dir().unwrap()).unwrap();
        let mut registry = WindowRegistry::default();
        registry.insert(project, 41);
        assert_eq!(registry.existing(&duplicate), Some(41));
        assert_eq!(registry.windows.len(), 1);
    }
}
