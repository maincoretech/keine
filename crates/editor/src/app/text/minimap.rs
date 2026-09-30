//! Source overview, using GPUI's DisplayMap for the same soft-wrap coordinates as Editor.
use super::*;
use gpui_kit::base::input::{DisplayMap, DisplayPoint, Rope};
use minimap::{Geometry, INSET, WIDTH};

#[derive(Default)]
pub(super) struct TextMinimap {
    cache: Option<Rc<Rows>>,
    dirty: bool,
}

struct Rows {
    width: Pixels,
    font: gpui_kit::Font,
    strokes: Vec<Vec<Stroke>>,
    offsets: Vec<Range<usize>>,
}

struct Stroke {
    x: f32,
    width: f32,
    color: Hsla,
}

fn rows(
    text: Rope,
    width: Pixels,
    font: gpui_kit::Font,
    language: &str,
    window: &mut Window,
    cx: &mut App,
) -> Rows {
    // gpui-base 0.6.4 InputElement::layout_line_numbers: one extra digit,
    // measured in the active font, plus the two native 10 px margins. Folding is disabled.
    let digits = text.lines_len().max(1).ilog10() as usize + 2;
    let gutter = window
        .text_system()
        .shape_line(
            "+".repeat(digits).into(),
            px(13.),
            &[gpui_kit::TextRun {
                len: digits,
                font: font.clone(),
                color: rgb(INK).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        )
        .width;
    let mut map = DisplayMap::new(
        font.clone(),
        px(13.),
        Some((width - gutter - px(20.)).max(px(1.))),
    );
    map.set_text(&text, cx);
    let source = text.to_string();
    let styles = crate::syntax::overview_styles(&source, language);
    let mut strokes = Vec::new();
    let mut offsets = Vec::new();
    for row in 0..map.display_row_count() {
        let point = map.display_pos_to_buffer_pos(DisplayPoint::new(row, 0));
        let start = text.line_start_offset(point.line) + point.col;
        let end = if row + 1 < map.display_row_count() {
            let next = map.display_pos_to_buffer_pos(DisplayPoint::new(row + 1, 0));
            text.line_start_offset(next.line) + next.col
        } else {
            source.len()
        };
        let start = start.min(source.len());
        let end = end.min(source.len());
        offsets.push(start..end);
        let mut runs: Vec<Stroke> = Vec::new();
        let first = styles.partition_point(|(range, _)| range.end <= start);
        let mut span = first;
        let mut x = 0.;
        for (offset, character) in source[start..end].char_indices() {
            if x >= WIDTH - INSET * 2. {
                break;
            }
            let offset = start + offset;
            while span < styles.len() && styles[span].0.end <= offset {
                span += 1;
            }
            let color = styles
                .get(span)
                .filter(|(range, _)| range.contains(&offset))
                .map_or(rgb(0xc8cbd0).into(), |(_, color)| *color);
            let advance = if character == '\t' {
                2.8
            } else if character.is_ascii() {
                0.7
            } else {
                1.4
            };
            if !character.is_whitespace() {
                if let Some(last) = runs
                    .last_mut()
                    .filter(|run| run.color == color && (run.x + run.width - x).abs() < 0.1)
                {
                    last.width += advance;
                } else {
                    runs.push(Stroke {
                        x,
                        width: advance,
                        color,
                    });
                }
            }
            x += advance;
        }
        strokes.push(runs);
    }
    Rows {
        width,
        font,
        strokes,
        offsets,
    }
}

fn geometry(editor: &EditorState, rows: usize, height: f32, pan: &mut minimap::Pan) -> Geometry {
    let line_height = f32::from(editor.line_height().unwrap_or(px(19.5)));
    let viewport = editor
        .text_bounds()
        .map_or(0., |bounds| f32::from(bounds.size.height));
    // Native editor defaults to three trailing scroll rows; its public setter clamps offsets.
    let content = (rows + 3) as f32 * line_height;
    let scroll = -f32::from(editor.scroll_offset().y);
    pan.geometry(height, viewport, (content - viewport).max(0.), scroll)
}

impl TextMinimap {
    pub(super) fn invalidate(&mut self) {
        self.dirty = true;
    }
}

impl WorkbenchPanel {
    pub(super) fn scroll_text_minimap(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let PanelContent::Document { editor, .. } = &self.content else {
            return;
        };
        let Some(rows) = &self.text_minimap.cache else {
            return;
        };
        let bounds = *self.minimap_navigation.bounds.borrow();
        let height = (f32::from(bounds.size.height) - INSET * 2.).max(0.);
        if height <= 0. {
            return;
        }
        let y = f32::from(position.y - bounds.origin.y) - INSET;
        let mut pan = self.minimap_navigation.pan.borrow_mut();
        let mapped = geometry(editor.read(cx), rows.strokes.len(), height, &mut pan);
        let scroll = pan.drag(mapped, y, &mut self.minimap_navigation.grab);
        let offset = editor.read(cx).scroll_offset();
        editor.update(cx, |editor, cx| {
            editor.set_scroll_offset(gpui_kit::point(offset.x, px(-scroll)), cx)
        });
        cx.notify();
    }
}

pub(super) fn render(
    editor: &Entity<EditorState>,
    relative: &Path,
    state: &mut TextMinimap,
    navigation: &minimap::Navigation,
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let width = editor
        .read(cx)
        .text_bounds()
        .map_or(px(600.), |bounds| bounds.size.width);
    let font = gpui_kit::font(Theme::global(cx).mono_font_family.clone());
    if state
        .cache
        .as_ref()
        .is_none_or(|cache| state.dirty || cache.width != width || cache.font != font)
    {
        state.cache = Some(Rc::new(rows(
            editor.read(cx).text().clone(),
            width,
            font,
            language_for_path(relative),
            window,
            cx,
        )));
        state.dirty = false;
    }
    let cache = state.cache.as_ref().unwrap().clone();
    let editor = editor.clone();
    let bounds_cell = navigation.bounds.clone();
    let pan_cell = navigation.pan.clone();
    let panel = cx.entity().downgrade();
    div()
        .id("text-minimap")
        .relative()
        .w(px(WIDTH))
        .h_full()
        .flex_none()
        .overflow_hidden()
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|panel, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                panel.minimap_navigation.grab = None;
                panel.scroll_minimap(event.position, cx);
            }),
        )
        .on_click(|_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(
            cx.listener(|panel, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                let PanelContent::Document { editor, .. } = &panel.content else {
                    return;
                };
                let Some(cache) = &panel.text_minimap.cache else {
                    return;
                };
                let height = (f32::from(panel.minimap_navigation.bounds.borrow().size.height)
                    - INSET * 2.)
                    .max(0.);
                let mut pan = panel.minimap_navigation.pan.borrow_mut();
                geometry(editor.read(cx), cache.strokes.len(), height, &mut pan);
                pan.offset -= f32::from(event.delta.pixel_delta(px(20.)).y);
                geometry(editor.read(cx), cache.strokes.len(), height, &mut pan);
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .child(
            canvas(
                move |bounds, _, _| *bounds_cell.borrow_mut() = bounds,
                move |bounds, _, window, cx| {
                    let height = (f32::from(bounds.size.height) - INSET * 2.).max(0.);
                    let geometry = geometry(
                        editor.read(cx),
                        cache.strokes.len(),
                        height,
                        &mut pan_cell.borrow_mut(),
                    );
                    let origin = bounds.origin + gpui_kit::point(px(INSET), px(INSET));
                    let width = (f32::from(bounds.size.width) - INSET * 2.).max(0.);
                    let stride = geometry.map_height / (cache.strokes.len() + 3).max(1) as f32;
                    let first = (geometry.pan / stride.max(0.01)) as usize;
                    let last = (((geometry.pan + height) / stride.max(0.01)).ceil() as usize + 1)
                        .min(cache.strokes.len());
                    let selection = editor.read(cx).selected_range();
                    for row in first.min(last)..last {
                        let y = row as f32 * stride - geometry.pan;
                        let Some((y, h)) = geometry.visible_span(y, stride.clamp(1., 3.)) else {
                            continue;
                        };
                        for stroke in &cache.strokes[row] {
                            window.paint_quad(fill(
                                Bounds::new(
                                    origin + gpui_kit::point(px(stroke.x), px(y)),
                                    size(px(stroke.width.min(width - stroke.x)), px(h)),
                                ),
                                stroke.color.opacity(0.5),
                            ));
                        }
                        if cache.offsets[row].contains(&selection.start)
                            || (!selection.is_empty()
                                && cache.offsets[row].start < selection.end
                                && selection.start < cache.offsets[row].end)
                        {
                            window.paint_quad(fill(
                                Bounds::new(
                                    origin + gpui_kit::point(px(width - 3.), px(y)),
                                    size(px(3.), px(h.max(3.).min(height - y))),
                                ),
                                rgb(PRIMARY),
                            ));
                        }
                    }
                    if let Some((top, h)) =
                        geometry.visible_span(geometry.viewport_top(), geometry.thumb)
                    {
                        let bounds = Bounds::new(
                            origin + gpui_kit::point(px(0.), px(top)),
                            size(px(width), px(h)),
                        );
                        window.paint_quad(fill(bounds, gpui_kit::rgba(0xbaebff20)));
                        window.paint_quad(fill(
                            Bounds::new(bounds.origin, size(px(2.), bounds.size.height)),
                            gpui_kit::rgba(0xbaebff90),
                        ));
                    }
                    let moving = panel.clone();
                    window.on_mouse_event(move |event: &gpui_kit::MouseMoveEvent, phase, _, cx| {
                        if !phase.bubble() {
                            return;
                        }
                        let Some(panel) = moving.upgrade() else {
                            return;
                        };
                        if panel.read(cx).minimap_navigation.grab.is_none() {
                            return;
                        }
                        panel.update(cx, |panel, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                panel.scroll_minimap(event.position, cx);
                            } else {
                                panel.minimap_navigation.grab = None;
                            }
                        });
                    });
                    let release = panel.clone();
                    window.on_mouse_event(move |event: &gpui_kit::MouseUpEvent, phase, _, cx| {
                        if !phase.capture() || event.button != MouseButton::Left {
                            return;
                        }
                        if let Some(panel) = release.upgrade() {
                            panel.update(cx, |panel, cx| {
                                panel.minimap_navigation.grab = None;
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
