//! Shared overview navigation and the painted Blocks layout. Never owns source selection.
use super::*;

pub(super) const WIDTH: f32 = 72.;
pub(super) const INSET: f32 = 6.;
pub(super) const MIN_THUMB: f32 = 32.;
pub(super) const VERTICAL_SCALE: f32 = 2.;

#[derive(Default)]
pub(super) struct Navigation {
    pub(super) bounds: Rc<RefCell<Bounds<Pixels>>>,
    pub(super) grab: Option<f32>,
    pub(super) pan: Rc<RefCell<Pan>>,
}

#[derive(Default)]
pub(super) struct Pan {
    pub(super) offset: f32,
    pub(super) last_scroll: Option<f32>,
}

impl Pan {
    fn for_blocks(&mut self, handle: &ScrollHandle, height: f32) -> Geometry {
        self.geometry(
            height,
            f32::from(handle.bounds().size.height),
            f32::from(handle.max_offset().y),
            -f32::from(handle.offset().y),
        )
    }

    pub(super) fn geometry(
        &mut self,
        height: f32,
        viewport: f32,
        max_scroll: f32,
        scroll: f32,
    ) -> Geometry {
        let mut geometry = Geometry::new(height, viewport, max_scroll, scroll, self.offset);
        // Main-view navigation follows the cursor; wheel browsing in the overview stays local.
        if self.last_scroll != Some(scroll) {
            self.offset = if geometry.max_scroll > 0. {
                scroll / geometry.max_scroll * (geometry.map_height - geometry.height)
            } else {
                0.
            };
            geometry.pan = self.offset.clamp(0., geometry.map_height - geometry.height);
        }
        self.offset = geometry.pan;
        self.last_scroll = Some(scroll);
        geometry
    }

    /// Preserve the initial grab point, and pan when a drag crosses either overview edge.
    pub(super) fn drag(&mut self, mut geometry: Geometry, y: f32, grab: &mut Option<f32>) -> f32 {
        if grab.is_some() {
            self.offset += y - y.clamp(0., geometry.height);
            geometry.pan = self.offset.clamp(0., geometry.map_height - geometry.height);
            self.offset = geometry.pan;
        }
        let grab = *grab.get_or_insert_with(|| {
            let top = geometry.viewport_top();
            if y >= top && y <= top + geometry.thumb && geometry.thumb > 0. {
                (y - top) / geometry.thumb
            } else {
                0.5
            }
        });
        let scroll = geometry.scroll_at(y, grab);
        self.last_scroll = Some(scroll);
        scroll
    }
}

#[derive(Clone)]
pub(super) struct Mark {
    pub top: f32,
    pub height: f32,
    pub depth: usize,
    pub width: f32,
    pub color: u32,
    pub selected: bool,
    pub error: bool,
}

impl Mark {
    pub(super) fn block(
        block: &crate::projection::BlockCard,
        top: f32,
        height: f32,
        selected: bool,
    ) -> Self {
        let color = if block.disabled {
            BORDER
        } else {
            blocks::block_type_color(&block.kind, &block.summary)
        };
        Self {
            top,
            height,
            depth: block.depth,
            width: (block.summary.chars().take(60).count() as f32 * 0.75).clamp(12., 48.),
            color,
            selected,
            error: block.read_only,
        }
    }
}

/// ScrollHandle::set_offset does not clamp or notify (GPUI 0.3.5 div.rs).
/// Both the painted viewport and pointer navigation use this bounded geometry.
pub(super) struct Geometry {
    pub(super) height: f32,
    pub(super) map_height: f32,
    pub(super) content: f32,
    pub(super) max_scroll: f32,
    pub(super) thumb: f32,
    pub(super) top: f32,
    pub(super) pan: f32,
}

impl Geometry {
    pub(super) fn new(height: f32, viewport: f32, max_scroll: f32, scroll: f32, pan: f32) -> Self {
        let height = height.max(0.);
        let max_scroll = max_scroll.max(0.);
        let content = (viewport + max_scroll).max(1.);
        let map_height = if max_scroll > 0. {
            height * VERTICAL_SCALE
        } else {
            height
        };
        let pan = pan.clamp(0., map_height - height);
        let thumb = (map_height * viewport / content).max(MIN_THUMB).min(height);
        let top = if max_scroll > 0. {
            scroll.clamp(0., max_scroll) / max_scroll * (map_height - thumb)
        } else {
            0.
        };
        Self {
            height,
            map_height,
            content,
            max_scroll,
            thumb,
            top,
            pan,
        }
    }

    pub(super) fn scroll_at(&self, y: f32, grab: f32) -> f32 {
        let travel = self.map_height - self.thumb;
        if travel <= 0. {
            return 0.;
        }
        ((y + self.pan - grab * self.thumb) / travel).clamp(0., 1.) * self.max_scroll
    }

    pub(super) fn mark_y(&self, top: f32) -> f32 {
        top / self.content * self.map_height - self.pan
    }

    pub(super) fn viewport_top(&self) -> f32 {
        self.top - self.pan
    }

    pub(super) fn visible_span(&self, top: f32, height: f32) -> Option<(f32, f32)> {
        let bottom = (top + height).min(self.height);
        let top = top.max(0.);
        (bottom > top).then_some((top, bottom - top))
    }
}

impl WorkbenchPanel {
    pub(super) fn scroll_minimap(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.document.document_mode == DocumentMode::Text {
            self.scroll_text_minimap(position, cx);
            return;
        }
        let bounds = *self.document.minimap_navigation.bounds.borrow();
        let height = f32::from(bounds.size.height) - INSET * 2.;
        if height <= 0. {
            return;
        }
        let y = f32::from(position.y - bounds.origin.y) - INSET;
        let mut pan = self.document.minimap_navigation.pan.borrow_mut();
        let geometry = pan.for_blocks(&self.view_scroll, height);
        let scroll = pan.drag(geometry, y, &mut self.document.minimap_navigation.grab);
        let offset = self.view_scroll.offset();
        self.view_scroll
            .set_offset(gpui_kit::point(offset.x, px(-scroll)));
        self.document.block_scroll_pending = false;
        self.document.block_context_menu = None;
        cx.notify();
    }
}

pub(super) fn render(
    marks: Rc<Vec<Mark>>,
    handle: &ScrollHandle,
    state: &Navigation,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let bounds_cell = state.bounds.clone();
    let handle = handle.clone();
    let pan_cell = state.pan.clone();
    let panel = cx.entity().downgrade();
    div()
        .id("block-minimap")
        .relative()
        .w(px(WIDTH))
        .h_full()
        .flex_none()
        .overflow_hidden()
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.document.minimap_navigation.grab = None;
                this.scroll_minimap(event.position, cx);
            }),
        )
        .on_click(|_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(
            cx.listener(|this, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                let bounds = *this.document.minimap_navigation.bounds.borrow();
                let height = (f32::from(bounds.size.height) - INSET * 2.).max(0.);
                let delta = event.delta.pixel_delta(px(20.));
                let mut pan = this.document.minimap_navigation.pan.borrow_mut();
                pan.for_blocks(&this.view_scroll, height);
                pan.offset -= f32::from(delta.y);
                pan.for_blocks(&this.view_scroll, height);
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .child(
            canvas(
                move |bounds, _, _| {
                    *bounds_cell.borrow_mut() = bounds;
                },
                move |bounds, _, window, _| {
                    let height = (f32::from(bounds.size.height) - INSET * 2.).max(0.);
                    let geometry = pan_cell.borrow_mut().for_blocks(&handle, height);
                    let origin = bounds.origin + gpui_kit::point(px(INSET), px(INSET));
                    let width = (f32::from(bounds.size.width) - INSET * 2.).max(0.);
                    // One ordered layer avoids a bounds-tree insertion for every stroke.
                    window.paint_layer(bounds, |window| {
                        for mark in marks.iter() {
                            let y = geometry.mark_y(mark.top);
                            let h = (mark.height / geometry.content * geometry.map_height)
                                .clamp(1., 4.);
                            let Some((y, h)) = geometry.visible_span(y, h) else {
                                continue;
                            };
                            if mark.selected {
                                window.paint_quad(fill(
                                    Bounds::new(
                                        origin + gpui_kit::point(px(0.), px(y)),
                                        size(px(width), px(h.max(3.).min(height - y))),
                                    ),
                                    rgb(PRIMARY_DIM),
                                ));
                            }
                            let indent = (mark.depth as f32 * 3.).min(width / 2.);
                            let color = if mark.selected { PRIMARY } else { mark.color };
                            window.paint_quad(fill(
                                Bounds::new(
                                    origin + gpui_kit::point(px(indent), px(y)),
                                    size(px(mark.width.min(width - indent)), px(h)),
                                ),
                                gpui_kit::rgba(
                                    (color << 8) | if mark.selected { 0xff } else { 0x60 },
                                ),
                            ));
                            if mark.error {
                                window.paint_quad(fill(
                                    Bounds::new(
                                        origin + gpui_kit::point(px(width - 3.), px(y)),
                                        size(px(3.), px(h.max(3.).min(height - y))),
                                    ),
                                    rgb(0xdb7780),
                                ));
                            }
                        }
                        // Dense rows may overlap a stroke; keep selection markers above all rows.
                        for mark in marks.iter().filter(|mark| mark.selected) {
                            let Some((y, h)) = geometry.visible_span(geometry.mark_y(mark.top), 3.)
                            else {
                                continue;
                            };
                            window.paint_quad(fill(
                                Bounds::new(
                                    origin + gpui_kit::point(px(width - 5.), px(y)),
                                    size(px(5.), px(h)),
                                ),
                                rgb(PRIMARY),
                            ));
                        }
                        if let Some((top, h)) =
                            geometry.visible_span(geometry.viewport_top(), geometry.thumb)
                        {
                            let viewport = Bounds::new(
                                origin + gpui_kit::point(px(0.), px(top)),
                                size(px(width), px(h)),
                            );
                            window.paint_quad(fill(viewport, gpui_kit::rgba(0xbaebff20)));
                            window.paint_quad(fill(
                                Bounds::new(viewport.origin, size(px(2.), viewport.size.height)),
                                gpui_kit::rgba(0xbaebff90),
                            ));
                            for y in [viewport.top(), viewport.bottom() - px(1.)] {
                                window.paint_quad(fill(
                                    Bounds::new(
                                        gpui_kit::point(viewport.left(), y),
                                        size(viewport.size.width, px(1.)),
                                    ),
                                    gpui_kit::rgba(0xbaebff60),
                                ));
                            }
                        }
                    });
                    // Window listeners retain a drag outside the narrow overview; no polling task.
                    let moving_panel = panel.clone();
                    window.on_mouse_event(move |event: &gpui_kit::MouseMoveEvent, phase, _, cx| {
                        if !phase.bubble() {
                            return;
                        }
                        let Some(panel) = moving_panel.upgrade() else {
                            return;
                        };
                        if panel.read(cx).document.minimap_navigation.grab.is_none() {
                            return;
                        }
                        panel.update(cx, |this, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                this.scroll_minimap(event.position, cx);
                            } else {
                                this.document.minimap_navigation.grab = None;
                            }
                        });
                    });
                    let release_panel = panel.clone();
                    window.on_mouse_event(move |event: &gpui_kit::MouseUpEvent, phase, _, cx| {
                        if !phase.capture() || event.button != MouseButton::Left {
                            return;
                        }
                        let Some(panel) = release_panel.upgrade() else {
                            return;
                        };
                        if panel.read(cx).document.minimap_navigation.grab.is_some() {
                            panel.update(cx, |this, cx| {
                                this.document.minimap_navigation.grab = None;
                                cx.notify();
                            });
                        }
                    });
                },
            )
            .absolute()
            .size_full(),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_preserves_grab_position_and_reaches_both_ends() {
        let geometry = Geometry::new(600., 500., 99500., 30000., 200.);
        assert_eq!(geometry.thumb, MIN_THUMB);
        let y = geometry.viewport_top() + geometry.thumb * 0.25;
        assert!((geometry.scroll_at(y, 0.25) - 30000.).abs() < 0.01);
        assert_eq!(geometry.scroll_at(-300., 0.25), 0.);
        assert_eq!(geometry.scroll_at(1300., 0.25), 99500.);
    }

    #[test]
    fn short_and_zero_height_overviews_cannot_scroll() {
        let geometry = Geometry::new(600., 700., 0., 10., 200.);
        assert_eq!(geometry.thumb, 600.);
        assert_eq!(geometry.scroll_at(500., 0.5), 0.);
        assert_eq!(
            Geometry::new(0., 100., 1000., 50., 0.).scroll_at(100., 0.5),
            0.
        );
    }

    #[test]
    fn overview_browsing_stays_local_until_main_view_scrolls() {
        let mut pan = Pan::default();
        let initial = pan.geometry(600., 500., 10000., 2500.);
        assert_eq!(initial.pan, 150.);
        pan.offset += 75.;
        let browsed = pan.geometry(600., 500., 10000., 2500.);
        assert_eq!(browsed.pan, 225.);
        assert_eq!(browsed.top, initial.top);
        assert_eq!(pan.geometry(600., 500., 10000., 5000.).pan, 300.);
        assert_eq!(pan.geometry(600., 700., 0., 0.).pan, 0.);
    }

    #[test]
    fn navigation_preserves_grab_and_pans_to_both_document_ends() {
        let mut pan = Pan::default();
        let mut grab = None;
        let geometry = pan.geometry(600., 500., 10000., 5000.);
        let y = geometry.viewport_top() + geometry.thumb * 0.25;
        let scroll = pan.drag(geometry, y, &mut grab);
        assert!((scroll - 5000.).abs() < 0.01);
        assert!((grab.unwrap() - 0.25).abs() < 0.001);
        let geometry = pan.geometry(600., 500., 10000., scroll);
        assert_eq!(pan.drag(geometry, 1800., &mut grab), 10000.);
        assert_eq!(pan.offset, 600.);
        let geometry = pan.geometry(600., 500., 10000., 10000.);
        assert_eq!(pan.drag(geometry, -1200., &mut grab), 0.);
        assert_eq!(pan.offset, 0.);
    }

    #[test]
    fn enlarged_overview_pans_without_changing_document_position() {
        let top = Geometry::new(600., 500., 99500., 30000., 0.);
        let panned = Geometry::new(600., 500., 99500., 30000., 150.);
        assert_eq!(top.map_height, 1200.);
        assert_eq!(top.thumb, 32.);
        assert_eq!(top.viewport_top() - panned.viewport_top(), 150.);
        assert_eq!(top.mark_y(25000.) - panned.mark_y(25000.), 150.);
        assert_eq!(panned.visible_span(-5., 10.), Some((0., 5.)));
        assert_eq!(panned.visible_span(650., 10.), None);
        assert_eq!(Geometry::new(600., 500., 99500., 0., 900.).pan, 600.);
    }
}
