//! Source overview, built off-thread with the Editor's GPUI soft-wrap rules.
use super::*;
use gpui_kit::base::input::Rope;
use minimap::{Geometry, INSET, WIDTH};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub(super) struct TextMinimap {
    cache: Option<Arc<Rows>>,
    dirty: bool,
    requested: Option<(Pixels, gpui_kit::Font)>,
    epoch: u64,
    cancelled: Arc<AtomicBool>,
    task: Option<gpui_kit::Task<()>>,
}

struct Rows {
    source: Arc<str>,
    styles: Arc<Vec<(Range<usize>, Hsla)>>,
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
    wrap_width: Pixels,
    font: gpui_kit::Font,
    language: &str,
    text_system: Arc<gpui_kit::TextSystem>,
    previous: Option<Arc<Rows>>,
    cancelled: &AtomicBool,
) -> Option<Rows> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let source: Arc<str> = text.to_string().into();
    // Resizing changes wrapping, not syntax. Keep the lexer result for the same source.
    let styles = previous
        .as_ref()
        .filter(|previous| previous.source == source)
        .map_or_else(
            || Arc::new(crate::syntax::overview_styles(&source, language)),
            |previous| previous.styles.clone(),
        );
    let mut wrapper = text_system.line_wrapper(font.clone(), px(13.));
    let mut offsets = Vec::new();
    for row in 0..text.lines_len() {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let line = text.slice_line(row).to_string();
        let start = text.line_start_offset(row);
        // Use the same GPUI LineWrapper and WrappingIndent::Same as DisplayMap.
        let mut previous = start;
        for boundary in wrapper.wrap_line(&[gpui_kit::LineFragment::text(&line)], wrap_width) {
            let end = start + boundary.ix;
            offsets.push(previous..end);
            previous = end;
        }
        // Preserve the original navigation/selection ranges, including the newline.
        let end = if row + 1 < text.lines_len() {
            text.line_start_offset(row + 1)
        } else {
            source.len()
        };
        offsets.push(previous..end);
    }
    let mut strokes = Vec::new();
    for range in offsets.iter().cloned() {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let start = range.start;
        let end = range.end;
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
    Some(Rows {
        source,
        styles,
        strokes,
        offsets,
    })
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

impl Drop for TextMinimap {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl TextMinimap {
    fn publish(&mut self, epoch: u64, rows: Rows) -> bool {
        if self.epoch != epoch {
            return false;
        }
        self.cache = Some(Arc::new(rows));
        true
    }
    pub(super) fn invalidate(&mut self) {
        self.dirty = true;
        self.epoch = self.epoch.wrapping_add(1);
        self.cancelled.store(true, Ordering::Relaxed);
        self.task.take();
    }
}

impl WorkbenchPanel {
    pub(super) fn scroll_text_minimap(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let PanelContent::Document { editor, .. } = &self.content else {
            return;
        };
        let Some(rows) = &self.document.text_minimap.cache else {
            return;
        };
        let bounds = *self.document.minimap_navigation.bounds.borrow();
        let height = (f32::from(bounds.size.height) - INSET * 2.).max(0.);
        if height <= 0. {
            return;
        }
        let y = f32::from(position.y - bounds.origin.y) - INSET;
        let mut pan = self.document.minimap_navigation.pan.borrow_mut();
        let mapped = geometry(editor.read(cx), rows.strokes.len(), height, &mut pan);
        let scroll = pan.drag(mapped, y, &mut self.document.minimap_navigation.grab);
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
    use gpui_kit::EntityInputHandler;
    let composing = editor.update(cx, |editor, cx| {
        editor.marked_text_range(window, cx).is_some()
    });
    let width = editor
        .read(cx)
        .text_bounds()
        .map_or(px(600.), |bounds| bounds.size.width);
    let font = gpui_kit::font(Theme::global(cx).mono_font_family.clone());
    if !composing && (state.dirty || state.requested.as_ref() != Some(&(width, font.clone()))) {
        state.cancelled.store(true, Ordering::Relaxed);
        state.task.take();
        state.cancelled = Arc::new(AtomicBool::new(false));
        state.epoch = state.epoch.wrapping_add(1);
        let epoch = state.epoch;
        state.requested = Some((width, font.clone()));
        state.dirty = false;
        let text = editor.read(cx).text().clone();
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
        // Native Editor reserves 18 px beside line numbers for folding controls.
        let fold_gutter = if language_for_path(relative) == "eiyashou" {
            px(18.)
        } else {
            px(0.)
        };
        let wrap_width = (width - gutter - px(20.) - fold_gutter).max(px(1.));
        let text_system = cx.text_system().clone();
        let previous = state.cache.clone();
        let cancelled = state.cancelled.clone();
        let language = language_for_path(relative).to_owned();
        let worker = cx.background_executor().spawn(async move {
            rows(
                text,
                wrap_width,
                font,
                &language,
                text_system,
                previous,
                &cancelled,
            )
        });
        state.task = Some(cx.spawn(async move |panel, cx| {
            if let Some(rows) = worker.await {
                let _ = panel.update(cx, |panel, cx| {
                    if panel.document.text_minimap.publish(epoch, rows) {
                        cx.notify();
                    }
                });
            }
        }));
    }
    let Some(cache) = state.cache.clone() else {
        return div().w(px(WIDTH)).h_full().flex_none().into_any_element();
    };
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
                panel.document.minimap_navigation.grab = None;
                panel.scroll_minimap(event.position, cx);
            }),
        )
        .on_click(|_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(
            cx.listener(|panel, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                let PanelContent::Document { editor, .. } = &panel.content else {
                    return;
                };
                let Some(cache) = &panel.document.text_minimap.cache else {
                    return;
                };
                let height = (f32::from(
                    panel
                        .document
                        .minimap_navigation
                        .bounds
                        .borrow()
                        .size
                        .height,
                ) - INSET * 2.)
                    .max(0.);
                let mut pan = panel.document.minimap_navigation.pan.borrow_mut();
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
                    // One ordered layer avoids a bounds-tree insertion for every stroke.
                    window.paint_layer(bounds, |window| {
                        for row in first.min(last)..last {
                            let y = row as f32 * stride - geometry.pan;
                            let Some((y, h)) = geometry.visible_span(y, stride.clamp(1., 3.))
                            else {
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
                    });
                    let moving = panel.clone();
                    window.on_mouse_event(move |event: &gpui_kit::MouseMoveEvent, phase, _, cx| {
                        if !phase.bubble() {
                            return;
                        }
                        let Some(panel) = moving.upgrade() else {
                            return;
                        };
                        if panel.read(cx).document.minimap_navigation.grab.is_none() {
                            return;
                        }
                        panel.update(cx, |panel, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                panel.scroll_minimap(event.position, cx);
                            } else {
                                panel.document.minimap_navigation.grab = None;
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
                                panel.document.minimap_navigation.grab = None;
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
    use gpui_kit::base::input::{DisplayMap, DisplayPoint};

    #[gpui_kit::test]
    fn worker_mapping_matches_native_wrapping(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            let font = gpui_kit::font("monospace");
            let cancelled = AtomicBool::new(false);
            for source in [
                "",
                "\n",
                "a\r\nb\r\n",
                "\t  你好🙂 a long indented line to wrap\n\nend",
                "scene demo {\n  hero: \"正文[wait=1000]继续\",\n}",
            ] {
                for width in [1., 30., 80., 600.] {
                    let text = Rope::from(source);
                    let actual = rows(
                        text.clone(),
                        px(width),
                        font.clone(),
                        "shou",
                        cx.text_system().clone(),
                        None,
                        &cancelled,
                    )
                    .unwrap();
                    let mut map = DisplayMap::new(font.clone(), px(13.), Some(px(width)));
                    map.set_text(&text, cx);
                    let expected = (0..map.display_row_count())
                        .map(|row| {
                            let point = map.display_pos_to_buffer_pos(DisplayPoint::new(row, 0));
                            let start = text.line_start_offset(point.line) + point.col;
                            let end = if row + 1 < map.display_row_count() {
                                let next =
                                    map.display_pos_to_buffer_pos(DisplayPoint::new(row + 1, 0));
                                text.line_start_offset(next.line) + next.col
                            } else {
                                source.len()
                            };
                            start..end
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(actual.offsets, expected, "source={source:?} width={width}");
                    let actual = Arc::new(actual);
                    let resized = rows(
                        text,
                        px(width + 10.),
                        font.clone(),
                        "shou",
                        cx.text_system().clone(),
                        Some(actual.clone()),
                        &cancelled,
                    )
                    .unwrap();
                    assert!(Arc::ptr_eq(&actual.styles, &resized.styles));
                }
            }
            let build = || {
                rows(
                    Rope::from("latest"),
                    px(80.),
                    font.clone(),
                    "shou",
                    cx.text_system().clone(),
                    None,
                    &cancelled,
                )
                .unwrap()
            };
            let mut state = TextMinimap::default();
            assert!(state.publish(0, build()));
            let previous = state.cache.as_ref().unwrap().clone();
            state.invalidate();
            assert!(!state.publish(0, build()));
            assert!(Arc::ptr_eq(&previous, state.cache.as_ref().unwrap()));
            assert!(state.publish(state.epoch, build()));
            cancelled.store(true, Ordering::Relaxed);
            assert!(
                rows(
                    Rope::from("cancelled"),
                    px(80.),
                    font,
                    "shou",
                    cx.text_system().clone(),
                    None,
                    &cancelled
                )
                .is_none()
            );
        });
    }

    mod benchmark {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/bench/editor/text.rs"
        ));
    }
}
