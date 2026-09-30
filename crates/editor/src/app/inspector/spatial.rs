//! Position pad and keyframe timeline interactions.
use super::*;
use crate::authoring::fields::number_text;

impl WorkbenchPanel {
    fn move_source_position(
        &mut self,
        root: &Path,
        start: usize,
        point: Point<Pixels>,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = *self.source_position_bounds.borrow();
        if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
            return;
        }
        let Some(mut key) = self
            .source_inspector_key
            .clone()
            .filter(|key| key.block_start == start)
        else {
            return;
        };
        // Several pointer events can arrive before Inspector is rendered again.
        // Read this same command's current bounded fields for each atomic X/Y edit.
        let Some(source) = cx.global::<EditorDocuments>().source(root, &key.path) else {
            return;
        };
        let projection = EiyashouProjection::parse(&source);
        let Some(block) = projection
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .find(|block| {
                block.source_range.start == start
                    && block.kind == key.kind
                    && block.summary.split('(').next().unwrap_or_default().trim() == key.command
            })
        else {
            return;
        };
        let Some(fields) = projection.source_fields_for_block(&source, block) else {
            return;
        };
        key.fields = fields;
        let percent = |value: f32| {
            let value = value.clamp(0., 100.);
            if shift {
                (value / 25.).round() * 25.
            } else {
                (value * 10.).round() / 10.
            }
        };
        let x = ((percent(((point.x - bounds.origin.x) / bounds.size.width) * 100.) / 100. - 0.5)
            * keine_core::DESIGN_WIDTH)
            .round();
        let y = ((percent(((point.y - bounds.origin.y) / bounds.size.height) * 100.) / 100. - 0.5)
            * keine_core::DESIGN_HEIGHT)
            .round();
        self.source_position_draft = Some((start, x, y));
        self.commit_source_fields(
            root,
            &key,
            &[
                ("x".into(), Some(number_text(x))),
                ("y".into(), Some(number_text(y))),
            ],
            window,
            cx,
        );
        cx.notify();
    }
}

impl SourceInspectorView<'_> {
    pub(super) fn position_pad(&self, cx: &mut Context<WorkbenchPanel>) -> Option<AnyElement> {
        if !matches!(
            self.key.command.as_str(),
            "camera.move" | "sprite.transform" | "background.transform"
        ) {
            return None;
        }
        let coordinate = |name: &str| {
            let field = self.key.fields.iter().find(|field| field.key == name)?;
            let number = source_number(self.key, field)?;
            Some(number.parse(&field.value).unwrap_or(number.default))
        };
        let (mut x, mut y) = (coordinate("x")?, coordinate("y")?);
        if let Some((start, draft_x, draft_y)) = self.position_draft
            && start == self.key.block_start
        {
            (x, y) = (draft_x, draft_y);
        }
        let start = self.key.block_start;
        let down_root = self.root.to_owned();
        let move_root = self.root.to_owned();
        let panel = cx.weak_entity();
        let bounds = self.position_bounds.clone();
        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id("source-position-pad")
                        .relative()
                        .w(px(200.))
                        .h(px(112.5))
                        .max_w_full()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(CANVAS))
                        .overflow_hidden()
                        .cursor_crosshair()
                        .child(
                            canvas(
                                move |bounds_now, _, _| *bounds.borrow_mut() = bounds_now,
                                move |_, _, window, _| {
                                    let panel = panel.clone();
                                    let root = move_root.clone();
                                    // GPUI requires window listeners to be registered
                                    // during paint. Keep tracking outside the pad.
                                    let release_panel = panel.clone();
                                    let release_root = root.clone();
                                    window.on_mouse_event(
                                        move |event: &gpui_kit::MouseUpEvent, phase, window, cx| {
                                            if !phase.capture() || event.button != MouseButton::Left
                                            {
                                                return;
                                            }
                                            let Some(panel) = release_panel.upgrade() else {
                                                return;
                                            };
                                            if !panel
                                                .read(cx)
                                                .source_position_draft
                                                .is_some_and(|(active, _, _)| active == start)
                                            {
                                                return;
                                            }
                                            panel.update(cx, |panel, cx| {
                                                panel.move_source_position(
                                                    &release_root,
                                                    start,
                                                    event.position,
                                                    event.modifiers.shift,
                                                    window,
                                                    cx,
                                                );
                                                panel.source_position_draft = None;
                                                cx.notify();
                                            });
                                        },
                                    );
                                    window.on_mouse_event(
                                        move |event: &gpui_kit::MouseMoveEvent,
                                              phase,
                                              window,
                                              cx| {
                                            if !phase.bubble() {
                                                return;
                                            }
                                            let Some(panel) = panel.upgrade() else {
                                                return;
                                            };
                                            if !panel
                                                .read(cx)
                                                .source_position_draft
                                                .is_some_and(|(active, _, _)| active == start)
                                            {
                                                return;
                                            }
                                            panel.update(cx, |panel, cx| {
                                                if event.pressed_button == Some(MouseButton::Left) {
                                                    panel.move_source_position(
                                                        &root,
                                                        start,
                                                        event.position,
                                                        event.modifiers.shift,
                                                        window,
                                                        cx,
                                                    );
                                                } else {
                                                    panel.source_position_draft = None;
                                                    cx.notify();
                                                }
                                            });
                                        },
                                    );
                                },
                            )
                            .absolute()
                            .size_full(),
                        )
                        .children([0.25, 0.5, 0.75].into_iter().map(|at| {
                            div()
                                .absolute()
                                .left(gpui_kit::relative(at))
                                .top_0()
                                .bottom_0()
                                .w(px(1.))
                                .bg(rgb(BORDER))
                        }))
                        .children([0.25, 0.5, 0.75].into_iter().map(|at| {
                            div()
                                .absolute()
                                .top(gpui_kit::relative(at))
                                .left_0()
                                .right_0()
                                .h(px(1.))
                                .bg(rgb(BORDER))
                        }))
                        .child(
                            div()
                                .absolute()
                                .left(gpui_kit::relative(
                                    (x / keine_core::DESIGN_WIDTH + 0.5).clamp(0., 1.),
                                ))
                                .top(gpui_kit::relative(
                                    (y / keine_core::DESIGN_HEIGHT + 0.5).clamp(0., 1.),
                                ))
                                .ml(px(-5.))
                                .mt(px(-5.))
                                .size(px(10.))
                                .rounded_full()
                                .border_2()
                                .border_color(rgb(PRIMARY))
                                .bg(rgb(CANVAS)),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                this.move_source_position(
                                    &down_root,
                                    start,
                                    event.position,
                                    event.modifiers.shift,
                                    window,
                                    cx,
                                );
                            }),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.source_position_draft = None;
                                cx.notify();
                            }),
                        )
                        .on_mouse_up_out(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.source_position_draft = None;
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(MUTED))
                        .child(format!(
                            "X {} · Y {} px · Shift to snap",
                            number_text(x),
                            number_text(y)
                        )),
                )
                .into_any_element(),
        )
    }

    pub(super) fn timeline(&self, cx: &mut Context<WorkbenchPanel>) -> Option<AnyElement> {
        let source = cx
            .global::<EditorDocuments>()
            .source(self.root, &self.key.path)?;
        let projection = EiyashouProjection::parse(&source);
        let blocks = projection
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .collect::<Vec<_>>();
        let stage = blocks.iter().find(|block| {
            block.summary.starts_with("stage.animate(")
                && block.source_range.contains(&self.key.block_start)
        })?;
        let time = SourceNumber {
            min: 0.,
            max: 5000.,
            step: 100.,
            default: 0.,
            unit: "ms",
        };
        let duration = projection
            .source_fields_for_block(&source, stage)?
            .iter()
            .find(|field| field.key == "duration")
            .and_then(|field| time.parse(&field.value))
            .unwrap_or(0.);
        let tracks = blocks
            .iter()
            .filter(|block| {
                block.summary.starts_with("track(")
                    && stage.source_range.contains(&block.source_range.start)
            })
            .collect::<Vec<_>>();
        let last_time = blocks
            .iter()
            .filter(|block| {
                block.summary.starts_with("key(")
                    && stage.source_range.contains(&block.source_range.start)
            })
            .filter_map(|block| projection.source_fields_for_block(&source, block))
            .flatten()
            .filter(|field| field.key == "time")
            .filter_map(|field| time.parse(&field.value))
            .fold(duration, f32::max)
            .max(1.);
        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label("Timeline"))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .justify_between()
                        .text_size(px(10.))
                        .text_color(rgb(MUTED))
                        .child("0ms")
                        .child(format!("{}ms", number_text(last_time))),
                )
                .children(tracks.into_iter().map(|track| {
                    let fields = projection
                        .source_fields_for_block(&source, track)
                        .unwrap_or_default();
                    let title = fields
                        .iter()
                        .filter(|field| matches!(field.key.as_str(), "0" | "1"))
                        .map(|field| field.value.as_str())
                        .collect::<Vec<_>>()
                        .join(" → ");
                    let keys = blocks
                        .iter()
                        .filter(|block| {
                            block.summary.starts_with("key(")
                                && track.source_range.contains(&block.source_range.start)
                        })
                        .filter_map(|block| {
                            let fields = projection.source_fields_for_block(&source, block)?;
                            let at = fields
                                .iter()
                                .find(|field| field.key == "time")
                                .and_then(|field| time.parse(&field.value))?;
                            Some((*block, at))
                        })
                        .collect::<Vec<_>>();
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(title))
                        .child(
                            div()
                                .relative()
                                .w_full()
                                .h(px(30.))
                                .rounded(px(3.))
                                .border_1()
                                .border_color(rgb(BORDER))
                                .bg(rgb(CANVAS))
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .top(px(14.))
                                        .h(px(1.))
                                        .bg(rgb(BORDER)),
                                )
                                .children(keys.into_iter().map(|(block, at)| {
                                    let root = self.root.to_owned();
                                    let path = self.key.path.clone();
                                    let start = block.source_range.start;
                                    let line = block.line;
                                    let column = block.column;
                                    div()
                                        .id(("timeline-key", start))
                                        .absolute()
                                        .left(gpui_kit::relative((at / last_time).clamp(0., 1.)))
                                        .ml(px(-6.))
                                        .top(px(8.))
                                        .size(px(12.))
                                        .rounded(px(2.))
                                        .border_1()
                                        .border_color(rgb(PRIMARY))
                                        .bg(rgb(if start == self.key.block_start {
                                            PRIMARY
                                        } else {
                                            PRIMARY_DIM
                                        }))
                                        .cursor_pointer()
                                        .tooltip(icon_hint(format!("{}ms", number_text(at))))
                                        .on_click(move |_, window, cx| {
                                            follow_preview_position(
                                                &root,
                                                &path,
                                                line + 1,
                                                column + 1,
                                                window,
                                                cx,
                                            );
                                            set_authoring_selection(
                                                &root,
                                                path.clone(),
                                                line,
                                                column,
                                                cx,
                                            );
                                        })
                                })),
                        )
                }))
                .into_any_element(),
        )
    }
}
