//! Shared workbench controls and overflow containers.
use super::*;

/// Reversible disclosure motion; stable IDs also preserve measurement and mid-flight reversal.
pub(super) fn disclosure_progress(
    id: impl Into<SharedString>,
    expanded: bool,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    transition(
        (id.into(), "disclosure"),
        if expanded { 1. } else { 0. },
        Transition::new(Duration::from_millis(160)),
        window,
        cx,
    )
}

pub(super) fn disclosure_chevron(progress: f32) -> impl IntoElement {
    Icon::new(IconName::ChevronRight)
        .xsmall()
        .rotate(radians(progress * std::f32::consts::FRAC_PI_2))
        .text_color(rgb(MUTED))
}

pub(super) fn disclosure_content(
    id: impl Into<SharedString>,
    progress: f32,
    content: impl IntoElement,
) -> impl IntoElement {
    gpui_kit::base::Collapsible::new()
        .reveal(id.into(), progress)
        .content(div().w_full().opacity(progress).child(content))
}

/// Prefix geometry keeps virtualized lists correct while disclosure rows change height.
pub(super) fn row_offsets(heights: impl IntoIterator<Item = f32>) -> Vec<f32> {
    let mut offsets = vec![0.];
    for height in heights {
        offsets.push(offsets.last().unwrap() + height.max(0.));
    }
    offsets
}

pub(super) fn visible_row_range(offsets: &[f32], top: f32, height: f32) -> Range<usize> {
    let count = offsets.len().saturating_sub(1);
    let start = offsets
        .partition_point(|position| *position <= top.max(0.))
        .saturating_sub(1)
        .min(count);
    let end = offsets
        .partition_point(|position| *position < top.max(0.) + height.max(0.))
        .min(count)
        .max(start);
    start..end
}

pub(super) fn preview_transport_icon(running: bool) -> AssetIconName {
    if running {
        AssetIconName::Square
    } else {
        AssetIconName::Play
    }
}

pub(super) fn preview_transport_button(running: bool) -> Stateful<Div> {
    let icon = preview_transport_icon(running);
    div()
        .id(if running {
            "preview-stop"
        } else {
            "preview-start"
        })
        .group("preview-transport")
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .bg(rgb(SURFACE))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .tooltip(icon_hint(if running {
            "Stop preview"
        } else {
            "Start preview"
        }))
        .child(
            div()
                .relative()
                .size(px(12.))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .when(!running, |this| {
                            this.group_hover("preview-transport", |style| style.invisible())
                        })
                        .child(Icon::new(icon).xsmall().text_color(rgb(if running {
                            0xdb7780
                        } else {
                            INK
                        }))),
                )
                .when(!running, |this| {
                    this.child(
                        div()
                            .absolute()
                            .inset_0()
                            .invisible()
                            .group_hover("preview-transport", |style| style.visible())
                            .child(Icon::new(icon).xsmall().text_color(rgb(PRIMARY))),
                    )
                }),
        )
}

pub(super) fn section_label(label: &'static str) -> impl IntoElement {
    div()
        .pt_1()
        .text_xs()
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(rgb(PRIMARY))
        .child(label)
}

pub(super) fn property_row(label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .pb_1()
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(div().text_xs().text_color(rgb(MUTED)).child(label))
        .child(div().text_xs().text_color(rgb(INK)).child(value))
}

pub(super) fn property_input(
    label: impl Into<SharedString>,
    state: &Entity<InputState>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .pb_1()
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(div().text_xs().text_color(rgb(MUTED)).child(label.into()))
        .child(
            div().h(px(28.)).child(
                Input::new(state)
                    .appearance(false)
                    .bordered(false)
                    .size_full()
                    .text_xs()
                    .text_color(rgb(INK)),
            ),
        )
}

pub(super) fn output_line(label: &'static str, color: u32, value: String) -> impl IntoElement {
    div()
        .flex()
        .gap_3()
        .child(
            div()
                .w(px(44.))
                .flex_none()
                .text_color(rgb(color))
                .child(label),
        )
        .child(div().min_w_0().flex_1().whitespace_normal().child(value))
}

pub(super) fn document_mode_button(label: &'static str, selected: bool) -> Stateful<Div> {
    div()
        .id(if label == "Text" {
            "document-mode-text"
        } else {
            "document-mode-block"
        })
        .when(cfg!(test), |button| {
            button.debug_selector(move || format!("document-mode-{label}"))
        })
        .h(px(25.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(rgb(if selected { SURFACE } else { CHROME }))
        .text_xs()
        .text_color(rgb(if selected { INK } else { MUTED }))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)).text_color(rgb(INK)))
        .child(label)
}

pub(super) fn tool_input(state: &Entity<InputState>) -> impl IntoElement {
    div()
        .h(px(30.))
        .rounded(px(7.))
        .bg(rgb(SURFACE))
        .px_2()
        .child(
            Input::new(state)
                .appearance(false)
                .bordered(false)
                .size_full()
                .text_sm()
                .text_color(rgb(INK)),
        )
}

pub(super) fn tool_action(label: &'static str) -> Stateful<Div> {
    div()
        .id(label)
        .h(px(28.))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .bg(rgb(PRIMARY_DIM))
        .text_xs()
        .text_color(rgb(PRIMARY))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)).text_color(rgb(INK)))
        .child(label)
}

pub(super) fn bottom_overflow_fade(visible: impl Fn(&App) -> bool + 'static) -> impl IntoElement {
    canvas(
        move |_, _, cx| visible(cx),
        |bounds, visible, window, _| {
            if visible {
                window.paint_quad(fill(
                    bounds,
                    linear_gradient(
                        180.,
                        linear_color_stop(hsla(0., 0., 0.01, 0.), 0.),
                        linear_color_stop(hsla(0., 0., 0.01, 0.22), 1.),
                    ),
                ));
            }
        },
    )
    .absolute()
    .left_0()
    .right_0()
    .bottom_0()
    .h(px(28.))
}

pub(super) fn vertical_overflow_view<E>(
    id: &'static str,
    handle: &ScrollHandle,
    content: E,
) -> AnyElement
where
    E: InteractiveElement + Styled + ParentElement + Element + 'static,
{
    let fade_handle = handle.clone();
    let area = div()
        .id(format!("{id}-area"))
        .size_full()
        .min_h_0()
        .track_scroll(handle)
        .overflow_y_scroll()
        .lock_scroll_axis()
        .child(content.w_full().h_auto().min_h_full().flex_none());
    div()
        .id(id)
        .relative()
        .size_full()
        .min_h_0()
        .overflow_hidden()
        .child(area)
        .child(bottom_overflow_fade(move |_| {
            let max = fade_handle.max_offset().y;
            max > px(1.) && max + fade_handle.offset().y > px(1.)
        }))
        .vertical_scrollbar(handle)
        .into_any_element()
}

pub(super) fn preview_window_controls(root: &Path, cx: &mut App) -> Option<Stateful<Div>> {
    use crate::preview::PreviewLifecycle;
    let controller = cx.global_mut::<EditorDocuments>().preview(root).ok()?;
    let lifecycle = controller.snapshot().lifecycle;
    let running = matches!(
        lifecycle,
        PreviewLifecycle::Running | PreviewLifecycle::Starting
    );
    Some(
        div()
            .id("preview-window-control")
            .when(cfg!(test), |controls| {
                controls.debug_selector(|| "preview-window-control".into())
            })
            .flex_none()
            .p(px(3.))
            .rounded(px(10.))
            .bg(rgb(CHROME))
            .flex()
            .items_center()
            .gap_1()
            .when(matches!(lifecycle, PreviewLifecycle::Failed(_)), |this| {
                this.child(
                    div()
                        .id("preview-failure-hint")
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .tooltip(icon_hint("Preview failed · see Output"))
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .with_size(px(16.))
                                .text_color(rgb(0xdb7780)),
                        ),
                )
            })
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
                        .on_click(move |_, _, _| controller.show()),
                )
            })
            .child(preview_transport_button(running).on_click(|_, window, cx| {
                window.dispatch_action(Box::new(ToggleEngine), cx);
            })),
    )
}

#[cfg(test)]
mod disclosure_tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn virtual_rows_follow_fractional_heights_and_scroll_boundaries() {
        let offsets = row_offsets([25., 12.5, 12.5, 25.]);
        assert_eq!(offsets, [0., 25., 37.5, 50., 75.]);
        assert_eq!(visible_row_range(&offsets, 25., 25.), 1..3);
        assert_eq!(visible_row_range(&offsets, 75., 10.), 4..4);
        assert_eq!(visible_row_range(&row_offsets([]), 0., 100.), 0..0);
        assert_eq!(
            visible_row_range(&row_offsets([25., 0., 25.]), 25., 25.),
            2..3
        );
    }

    struct DisclosureHarness {
        width: f32,
        open: bool,
        progress: Rc<Cell<f32>>,
    }

    impl Render for DisclosureHarness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let progress = disclosure_progress("disclosure-test", self.open, window, cx);
            self.progress.set(progress);
            div()
                .w(px(self.width))
                .flex()
                .flex_col()
                .child(div().h(px(28.)).flex_none())
                .child(disclosure_content(
                    "disclosure-test-body",
                    progress,
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .child(div().h(px(40.)).flex_none())
                        .child(
                            div().text_sm().whitespace_normal().child(
                                "Ready 70 Pending 0 Unused 0 Missing 0 Undefined 0 Unmapped 0",
                            ),
                        ),
                ))
                .child(
                    div()
                        .debug_selector(|| "after-disclosure".into())
                        .h(px(20.))
                        .flex_none(),
                )
        }
    }

    #[gpui_kit::test]
    fn measured_disclosure_reverses_without_jumping_and_honors_reduced_motion(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let progress = Rc::new(Cell::new(0.));
        let (panel, cx) = cx.add_window_view(|_, _| DisclosureHarness {
            width: 160.,
            open: false,
            progress: progress.clone(),
        });
        let draw = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        };
        draw(cx);
        let closed = cx.debug_bounds("after-disclosure").unwrap().top();
        panel.update(cx, |panel, cx| {
            panel.open = true;
            cx.notify();
        });
        draw(cx);
        assert_eq!(cx.debug_bounds("after-disclosure").unwrap().top(), closed);
        cx.executor().advance_clock(Duration::from_millis(60));
        draw(cx);
        let halfway = cx.debug_bounds("after-disclosure").unwrap().top();
        let before = progress.get();
        assert!(before > 0. && before < 1.);
        assert!(halfway > closed);
        panel.update(cx, |panel, cx| {
            panel.open = false;
            cx.notify();
        });
        draw(cx);
        assert_eq!(progress.get(), before);
        assert_eq!(cx.debug_bounds("after-disclosure").unwrap().top(), halfway);
        cx.executor().advance_clock(Duration::from_millis(200));
        draw(cx);
        assert_eq!(cx.debug_bounds("after-disclosure").unwrap().top(), closed);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        panel.update(cx, |panel, cx| {
            panel.open = true;
            cx.notify();
        });
        draw(cx);
        assert_eq!(progress.get(), 1.);
        assert!(cx.debug_bounds("after-disclosure").unwrap().top() > halfway);
        // Width changes remeasure wrapped content, so no guessed height clips the final row.
        let wide_height = cx.debug_bounds("after-disclosure").unwrap().top();
        panel.update(cx, |panel, cx| {
            panel.width = 80.;
            cx.notify();
        });
        draw(cx);
        draw(cx);
        assert!(cx.debug_bounds("after-disclosure").unwrap().top() > wide_height);
        panel.update(cx, |panel, cx| {
            panel.open = false;
            cx.notify();
        });
        draw(cx);
        assert_eq!(progress.get(), 0.);
        assert_eq!(cx.debug_bounds("after-disclosure").unwrap().top(), closed);
    }
}
