use super::*;

pub(super) struct ProjectWorkspace {
    pub(super) session: WorkspaceSession,
    pub(super) dock: Entity<DockArea>,
    _layout_subscription: Subscription,
}

/// Keep gpui-component's dock behavior and visuals, changing only the tab bar
/// so every tab has the compact close affordance expected by an editor.
struct EditorDockSkin {
    inner: Rc<DockSkin>,
    root: PathBuf,
    dock: Rc<RefCell<Option<WeakEntity<DockArea>>>>,
    drop_overlays: Rc<RefCell<HashMap<NodeId, Entity<EditorDropOverlay>>>>,
    tab_scrolls: Rc<RefCell<HashMap<NodeId, ScrollHandle>>>,
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
            tab_scrolls: self.tab_scrolls.clone(),
            tab_motion: self.tab_motion.clone(),
        })
    }
}

struct EditorTabGroupSkin {
    inner: Rc<dyn TabGroupRenderer>,
    root: PathBuf,
    dock: Rc<RefCell<Option<WeakEntity<DockArea>>>>,
    drop_overlays: Rc<RefCell<HashMap<NodeId, Entity<EditorDropOverlay>>>>,
    tab_scrolls: Rc<RefCell<HashMap<NodeId, ScrollHandle>>>,
    tab_motion: Entity<EditorTabMotion>,
}

impl EditorTabGroupSkin {
    fn tab_scroll(&self, node: NodeId) -> ScrollHandle {
        self.tab_scrolls
            .borrow_mut()
            .entry(node)
            .or_default()
            .clone()
    }

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
                            .tooltip(icon_hint("Close view"))
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
    seen: HashSet<u64>,
    opening: HashSet<u64>,
    closing: HashSet<u64>,
    dragging: Option<u64>,
    hover_target: Option<u64>,
    hover_anchor_x: Option<f32>,
}

impl EditorTabMotion {
    fn begin_drag(&mut self, panel_id: PanelId, cx: &mut Context<Self>) {
        self.dragging = Some(panel_id.as_u64());
        self.hover_target = None;
        self.hover_anchor_x = None;
        cx.refresh_windows();
    }

    fn hover_at(&mut self, panel_id: Option<PanelId>, x: f32, cx: &mut Context<Self>) {
        let target = panel_id
            .map(|id| id.as_u64())
            .filter(|id| Some(*id) != self.dragging);
        // A target shifts when its insertion space opens. Keep it selected until
        // the pointer actually moves, rather than chasing the shifted hitbox.
        if target != self.hover_target
            && self
                .hover_anchor_x
                .is_some_and(|anchor| (x - anchor).abs() < 36.)
        {
            return;
        }
        if self.hover_target != target {
            self.hover_target = target;
            self.hover_anchor_x = Some(x);
            cx.refresh_windows();
        }
    }

    fn clear_hover(&mut self, cx: &mut Context<Self>) {
        if self.hover_target.take().is_some() {
            self.hover_anchor_x = None;
            cx.refresh_windows();
        }
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.dragging.take().is_some() || self.hover_target.take().is_some() {
            self.hover_anchor_x = None;
            cx.refresh_windows();
        }
    }

    fn register(
        &mut self,
        panel_id: gpui_kit::component::dock::PanelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (bool, bool) {
        let key = panel_id.as_u64();
        if self.seen.insert(key) && !cx.reduce_motion() {
            self.opening.insert(key);
            cx.spawn_in(window, async move |motion, cx| {
                cx.background_executor().timer(Duration::ZERO).await;
                let _ = motion.update_in(cx, |motion, _, cx| {
                    motion.opening.remove(&key);
                    cx.refresh_windows();
                });
            })
            .detach();
        }
        (self.opening.contains(&key), self.closing.contains(&key))
    }

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
                motion.opening.remove(&key);
                motion.seen.remove(&key);
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

fn tab_overflow_fade(scroll: ScrollHandle, left: bool) -> AnyElement {
    let fade = canvas(
        move |_, _, _| {
            if left {
                scroll.offset().x < -px(1.)
            } else {
                let max = scroll.max_offset().x;
                max > px(1.) && max + scroll.offset().x > px(1.)
            }
        },
        move |bounds, visible, window, _| {
            if visible {
                window.paint_quad(fill(
                    bounds,
                    if left {
                        linear_gradient(
                            90.,
                            linear_color_stop(hsla(0., 0., 0., 0.96), 0.),
                            linear_color_stop(hsla(0., 0., 0., 0.), 1.),
                        )
                    } else {
                        linear_gradient(
                            90.,
                            linear_color_stop(hsla(0., 0., 0., 0.), 0.),
                            linear_color_stop(hsla(0., 0., 0., 0.96), 1.),
                        )
                    },
                ));
            }
        },
    )
    .absolute()
    .top_0()
    .h_full()
    .w(px(48.));
    if left {
        fade.left_0().into_any_element()
    } else {
        fade.right_0().into_any_element()
    }
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
        let hover_motion = self.tab_motion.clone();
        let drop_motion = self.tab_motion.clone();

        self.inner
            .content_frame(group, window, cx)
            .border_0()
            .pt_0()
            .rounded_b(px(VIEW_RADIUS_PX))
            .overflow_hidden()
            .bg(rgb(PANEL))
            .on_drag_move(move |event: &DragMoveEvent<EditorPanelDrag>, _, cx| {
                hover_motion.update(cx, |motion, cx| motion.clear_hover(cx));
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
                drop_motion.update(cx, |motion, cx| motion.end_drag(cx));
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
        if !cx.has_active_drag() {
            let stale_drag = {
                let motion = self.tab_motion.read(cx);
                motion.dragging.is_some() || motion.hover_target.is_some()
            };
            if stale_drag {
                self.tab_motion.update(cx, |motion, cx| motion.end_drag(cx));
            }
        }
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
                let key = panel_id.as_u64();
                let (opening, closing) = if self.tab_motion.read(cx).seen.contains(&key) {
                    let motion = self.tab_motion.read(cx);
                    (motion.opening.contains(&key), motion.closing.contains(&key))
                } else {
                    self.tab_motion
                        .update(cx, |motion, cx| motion.register(panel_id, window, cx))
                };
                let hovering = self.tab_motion.read(cx).hover_target == Some(key);
                let dragging = self.tab_motion.read(cx).dragging == Some(key);
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
                    if opening || closing {
                        0.
                    } else if dragging {
                        0.32
                    } else {
                        1.
                    },
                    Transition::new(TAB_MOTION_DURATION),
                    window,
                    cx,
                );
                let max_width = transition(
                    (("editor-tab-motion", panel_id.as_u64()), "width"),
                    if opening || closing {
                        px(0.)
                    } else if hovering {
                        px(280.)
                    } else {
                        px(240.)
                    },
                    Transition::new(TAB_MOTION_DURATION),
                    window,
                    cx,
                );
                let leading_space = transition(
                    (("editor-tab-motion", panel_id.as_u64()), "leading-space"),
                    if hovering { px(44.) } else { px(8.) },
                    Transition::new(TAB_MOTION_DURATION),
                    window,
                    cx,
                );
                let close_motion = self.tab_motion.clone();
                let middle_close_motion = self.tab_motion.clone();
                let drag_motion = self.tab_motion.clone();
                let hover_motion = self.tab_motion.clone();
                let drop_motion = self.tab_motion.clone();
                div()
                    .id(("editor-tab", panel_id.as_u64()))
                    .h(px(28.))
                    .min_w_0()
                    .max_w(max_width)
                    .flex_none()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl(leading_space)
                    .pr_2()
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
                            drag_motion.update(cx, |motion, cx| motion.begin_drag(panel_id, cx));
                            clear_all_drop_overlays(&drag_content_overlays, cx);
                            drag.panel.set_drag_offset(offset);
                            drag.panel.set_preview_size(size(px(180.), px(30.)));
                            cx.new(|_| TabDragPreview {
                                title: drag_title.clone(),
                            })
                        })
                    })
                    .when(group.is_droppable(), |this| {
                        this.on_drag_move(move |event: &DragMoveEvent<EditorPanelDrag>, _, cx| {
                            clear_drop_overlay(&hover_content_overlay, cx);
                            let x = f32::from(event.event.position.x);
                            hover_motion
                                .update(cx, |motion, cx| motion.hover_at(Some(panel_id), x, cx));
                        })
                        .on_drop(move |drag: &EditorPanelDrag, window, cx| {
                            clear_drop_overlay(&drop_content_overlay, cx);
                            drop_motion.update(cx, |motion, cx| motion.end_drag(cx));
                            drop_group.drop_panel(drag.panel.clone(), Some(ix), true, window, cx);
                        })
                        .drag_over::<AnyDrag>(|this, _, _, _| this.bg(rgb(SURFACE_HOVER)))
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
                                .tooltip(icon_hint("Close tab"))
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
        let empty_hover_motion = self.tab_motion.clone();
        let empty_drop_motion = self.tab_motion.clone();
        let empty_space = div()
            .id(("editor-tab-empty", node.as_u64()))
            .h_full()
            .flex_1()
            .min_w_8()
            .when(group.is_droppable(), |this| {
                this.on_drag_move(move |event: &DragMoveEvent<EditorPanelDrag>, _, cx| {
                    clear_drop_overlay(&hover_content_overlay, cx);
                    let x = f32::from(event.event.position.x);
                    empty_hover_motion.update(cx, |motion, cx| motion.hover_at(None, x, cx));
                })
                .on_drop(move |drag: &EditorPanelDrag, window, cx| {
                    clear_drop_overlay(&drop_content_overlay, cx);
                    empty_drop_motion.update(cx, |motion, cx| motion.end_drag(cx));
                    drop_group.drop_panel(drag.panel.clone(), None, true, window, cx);
                })
                .drag_over::<AnyDrag>(|this, _, _, cx| this.bg(cx.theme().tokens.drop_target))
                .on_drop(move |item: &AnyDrag, window, cx| {
                    item_drop_group.drop_item(item.clone(), None, window, cx);
                })
            });

        let scroll = self.tab_scroll(node);
        let bar = div()
            .id(("editor-tab-bar-scroll", node.as_u64()))
            .h(px(36.))
            .w_full()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .track_scroll(&scroll)
            .overflow_x_scroll()
            .bg(rgb(CHROME))
            .children(tabs)
            .child(empty_space);
        div()
            .id(("editor-tab-bar", node.as_u64()))
            .relative()
            .h(px(36.))
            .w_full()
            .overflow_hidden()
            .rounded_t(px(VIEW_RADIUS_PX))
            .child(bar)
            .child(tab_overflow_fade(scroll.clone(), true))
            .child(tab_overflow_fade(scroll, false))
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
pub(super) fn editor_drop_placement(
    bounds: Bounds<Pixels>,
    position: Point<Pixels>,
) -> Option<Placement> {
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
    pub(super) fn new(
        session: WorkspaceSession,
        persistence: &AppPersistence,
        window: &mut Window,
        cx: &mut Context<WorkbenchWindow>,
    ) -> Self {
        window.set_window_title(&format!("{} — Kēne Editor", session.name()));
        let mut skin = None;
        let dock_ref = Rc::new(RefCell::new(None));
        let drop_overlays = Rc::new(RefCell::new(HashMap::new()));
        let tab_scrolls = Rc::new(RefCell::new(HashMap::new()));
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
                tab_scrolls,
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

pub(super) fn install_default_layout(
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
