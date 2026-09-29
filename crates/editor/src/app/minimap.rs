//! A disposable, painted overview of the Blocks layout. It never selects or edits source.
use super::*;

const WIDTH: f32 = 72.;
const INSET: f32 = 6.;
const MIN_THUMB: f32 = 16.;

#[derive(Default)]
pub(super) struct BlockMinimap {
    bounds: Rc<RefCell<Bounds<Pixels>>>,
    grab: Option<f32>,
}

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
            view::block_type_color(&block.kind, &block.summary)
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
struct Geometry {
    height: f32,
    content: f32,
    max_scroll: f32,
    thumb: f32,
    top: f32,
}

impl Geometry {
    fn new(height: f32, viewport: f32, max_scroll: f32, scroll: f32) -> Self {
        let height = height.max(0.);
        let max_scroll = max_scroll.max(0.);
        let content = (viewport + max_scroll).max(1.);
        let thumb = (height * viewport / content).max(MIN_THUMB).min(height);
        let top = if max_scroll > 0. {
            scroll.clamp(0., max_scroll) / max_scroll * (height - thumb)
        } else {
            0.
        };
        Self {
            height,
            content,
            max_scroll,
            thumb,
            top,
        }
    }

    fn scroll_at(&self, y: f32, grab: f32) -> f32 {
        let travel = self.height - self.thumb;
        if travel <= 0. {
            return 0.;
        }
        ((y - grab * self.thumb) / travel).clamp(0., 1.) * self.max_scroll
    }
}

impl WorkbenchPanel {
    fn scroll_minimap(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let bounds = *self.block_minimap.bounds.borrow();
        let height = f32::from(bounds.size.height) - INSET * 2.;
        if height <= 0. {
            return;
        }
        let geometry = geometry(&self.view_scroll, height);
        let y = f32::from(position.y - bounds.origin.y) - INSET;
        let grab = *self.block_minimap.grab.get_or_insert_with(|| {
            if y >= geometry.top && y <= geometry.top + geometry.thumb && geometry.thumb > 0. {
                (y - geometry.top) / geometry.thumb
            } else {
                0.5
            }
        });
        let offset = self.view_scroll.offset();
        self.view_scroll
            .set_offset(gpui_kit::point(offset.x, px(-geometry.scroll_at(y, grab))));
        self.block_scroll_pending = false;
        self.block_context_menu = None;
        cx.notify();
    }
}

fn geometry(handle: &ScrollHandle, height: f32) -> Geometry {
    Geometry::new(
        height,
        f32::from(handle.bounds().size.height),
        f32::from(handle.max_offset().y),
        -f32::from(handle.offset().y),
    )
}

pub(super) fn render(
    marks: Vec<Mark>,
    handle: &ScrollHandle,
    state: &BlockMinimap,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let bounds_cell = state.bounds.clone();
    let handle = handle.clone();
    let panel = cx.entity().downgrade();
    div()
        .id("block-minimap")
        .relative()
        .w(px(WIDTH))
        .h_full()
        .flex_none()
        .overflow_hidden()
        .cursor_pointer()
        .tooltip(icon_hint("Overview"))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.block_minimap.grab = None;
                this.scroll_minimap(event.position, cx);
            }),
        )
        .on_click(|_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(
            cx.listener(|this, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                let offset = this.view_scroll.offset();
                let delta = event.delta.pixel_delta(px(20.));
                this.view_scroll.set_offset(gpui_kit::point(
                    offset.x,
                    (offset.y + delta.y).clamp(-this.view_scroll.max_offset().y, px(0.)),
                ));
                this.block_scroll_pending = false;
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
                    let geometry = geometry(&handle, height);
                    let origin = bounds.origin + gpui_kit::point(px(INSET), px(INSET));
                    let width = (f32::from(bounds.size.width) - INSET * 2.).max(0.);
                    for mark in &marks {
                        let y = (mark.top / geometry.content * height).clamp(0., height);
                        let h = (mark.height / geometry.content * height)
                            .clamp(0.5, 2.)
                            .min(height - y);
                        if h <= 0. {
                            continue;
                        }
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
                            gpui_kit::rgba((color << 8) | if mark.selected { 0xff } else { 0x60 }),
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
                        let y = (mark.top / geometry.content * height).clamp(0., height);
                        window.paint_quad(fill(
                            Bounds::new(
                                origin + gpui_kit::point(px(width - 5.), px(y)),
                                size(px(5.), px(3.).min(px(height - y))),
                            ),
                            rgb(PRIMARY),
                        ));
                    }
                    let viewport = Bounds::new(
                        origin + gpui_kit::point(px(0.), px(geometry.top)),
                        size(px(width), px(geometry.thumb)),
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

                    // Window listeners retain a drag outside the narrow overview; no polling task.
                    let moving_panel = panel.clone();
                    window.on_mouse_event(move |event: &gpui_kit::MouseMoveEvent, phase, _, cx| {
                        if !phase.bubble() {
                            return;
                        }
                        let Some(panel) = moving_panel.upgrade() else {
                            return;
                        };
                        if panel.read(cx).block_minimap.grab.is_none() {
                            return;
                        }
                        panel.update(cx, |this, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                this.scroll_minimap(event.position, cx);
                            } else {
                                this.block_minimap.grab = None;
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
                        if panel.read(cx).block_minimap.grab.is_some() {
                            panel.update(cx, |this, cx| {
                                this.scroll_minimap(event.position, cx);
                                this.block_minimap.grab = None;
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
        let geometry = Geometry::new(600., 500., 99500., 30000.);
        assert_eq!(geometry.thumb, MIN_THUMB);
        let y = geometry.top + geometry.thumb * 0.25;
        assert!((geometry.scroll_at(y, 0.25) - 30000.).abs() < 0.01);
        assert_eq!(geometry.scroll_at(-100., 0.25), 0.);
        assert_eq!(geometry.scroll_at(700., 0.25), 99500.);
    }

    #[test]
    fn short_and_zero_height_overviews_cannot_scroll() {
        let geometry = Geometry::new(600., 700., 0., 10.);
        assert_eq!(geometry.thumb, 600.);
        assert_eq!(geometry.scroll_at(500., 0.5), 0.);
        assert_eq!(Geometry::new(0., 100., 1000., 50.).scroll_at(100., 0.5), 0.);
    }
}
