//! Timing view over the document projection; drag commits through normal source history.
use crate::app::*;
use crate::authoring::timeline::{DurationField, Segment};
use std::cell::Cell;

#[derive(Default)]
pub(in crate::app) struct TimelineState {
    revision: Option<u64>,
    segments: Rc<Vec<Segment>>,
    drag: Option<DurationDrag>,
    pointer_down: Rc<Cell<Pixels>>,
}
#[derive(Clone)]
struct DurationDrag {
    owner: gpui_kit::EntityId,
    document: DocumentHandle,
    revision: u64,
    field: DurationField,
    origin: Rc<Cell<Pixels>>,
    seconds: Rc<Cell<f64>>,
    scale: f64,
}

pub(in crate::app) fn render(
    panel: &mut WorkbenchPanel,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let PanelContent::Document {
        root,
        relative,
        document: Some(document),
        ..
    } = &panel.content
    else {
        return Empty.into_any_element();
    };
    let root = root.clone();
    let path = relative.clone();
    let document = document.clone();
    let source = document.borrow();
    let revision = source.revision();
    let state = &mut panel.document.timeline;
    if state.revision != Some(revision) {
        state.segments = Rc::new(crate::authoring::timeline::segments(
            source.contents(),
            &source.projection(),
        ));
        state.revision = Some(revision);
        state.drag = None;
    }
    drop(source);
    if !cx.has_active_drag() {
        state.drag = None;
    }
    let mut content =
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .child(div().text_xs().text_color(rgb(MUTED)).child(
            crate::i18n::tr(
                "Declared timing · drag the right edge to resize; select to edit in Inspector.",
            ),
        ));
    let viewport_width = panel.view_scroll.bounds().size.width;
    let lane = (f32::from(viewport_width) - 232.).max(200.) as f64;
    for (segment_index, segment) in state.segments.iter().enumerate() {
        let scale = lane / segment.end.max(1.);
        let mut group = div().flex().flex_col().gap_1().child(
            div().text_xs().text_color(rgb(MUTED)).child(format!(
                "{} · {}",
                segment.scene,
                segment_index + 1
            )),
        );
        if segment.end > 0. {
            group = group.child(
                div()
                    .ml(px(180.))
                    .w(px(lane as f32))
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .children((0..=4).map(|i| {
                        div().child(format!("{:.2}s", segment.end.max(1.) * i as f64 / 4.))
                    })),
            );
        }
        for row in &segment.rows {
            let id = row.block.source_range.start;
            let selected = panel.document.selected_blocks.contains(&id);
            let root = root.clone();
            let path = path.clone();
            let line = row.block.line;
            let column = row.block.column;
            let mut body = div()
                .id(("timeline-row", id))
                .flex()
                .items_center()
                .gap_2()
                .h(px(38.))
                .rounded(px(6.))
                .px_1()
                .cursor_pointer()
                .bg(rgb(if selected { PRIMARY_DIM } else { SURFACE }))
                .hover(|s| s.bg(rgb(SURFACE_HOVER)))
                .child(
                    div()
                        .w(px(168.))
                        .flex_none()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_xs()
                        .child(row.block.summary.clone()),
                )
                .on_click(cx.listener(move |panel, _, window, cx| {
                    panel.focus.focus(window, cx);
                    panel.document.selected_blocks = HashSet::from([id]);
                    panel.document.block_selection_anchor = Some(id);
                    let documents = cx.global_mut::<EditorDocuments>();
                    documents.clear_asset_selection(&root);
                    documents.set_selection(&root, path.clone(), line, column);
                    documents.set_block_selection(&root, path.clone(), vec![id]);
                    cx.refresh_windows();
                }));
            if let Some(field) = &row.duration {
                let seconds = state
                    .drag
                    .as_ref()
                    .filter(|d| d.field.range == field.range)
                    .map_or(field.seconds, |d| d.seconds.get());
                let drag = DurationDrag {
                    owner: cx.entity_id(),
                    document: document.clone(),
                    revision,
                    field: field.clone(),
                    origin: state.pointer_down.clone(),
                    seconds: Rc::new(Cell::new(field.seconds)),
                    scale,
                };
                let owner = cx.entity().downgrade();
                let origin = drag.origin.clone();
                let width = (seconds * scale).clamp(12., lane);
                body = body.child(
                    div().relative().h(px(28.)).w(px(lane as f32)).child(
                        div()
                            .absolute()
                            .left(px((row.start * scale) as f32))
                            .h_full()
                            .w(px(width as f32))
                            .rounded(px(4.))
                            .bg(rgb(PRIMARY_DIM))
                            .border_1()
                            .border_color(rgb(PRIMARY))
                            .child(
                                div()
                                    .px_1()
                                    .text_xs()
                                    .overflow_hidden()
                                    .child(format!("{seconds:.2}s")),
                            )
                            .child(
                                div()
                                    .id(("timeline-resize", id))
                                    .absolute()
                                    .right_0()
                                    .top_0()
                                    .h_full()
                                    .w(px(12.))
                                    .cursor_col_resize()
                                    .bg(rgb(PRIMARY))
                                    .hover(|s| s.opacity(0.7))
                                    .on_mouse_down(MouseButton::Left, move |event, _, _| {
                                        origin.set(event.position.x);
                                    })
                                    .on_drag(drag, move |drag, _, window, cx| {
                                        let _ = owner.update(cx, |panel, cx| {
                                            panel.document.timeline.drag = Some(drag.clone());
                                            panel.focus.focus(window, cx);
                                            cx.notify();
                                        });
                                        cx.new(|_| BlockDragPreview {
                                            label: crate::i18n::tr("Duration").into(),
                                            summary: String::new(),
                                            icon: AssetIconName::Clock,
                                            count: 1,
                                            width: 100.,
                                            height: 30.,
                                            color: PRIMARY,
                                            grip_top: 6.,
                                        })
                                    }),
                            ),
                    ),
                );
            } else {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(crate::i18n::tr("Segment boundary")),
                );
            }
            group = group.child(body);
        }
        content = content.child(group);
    }
    let owner = cx.entity().downgrade();
    div()
        .id("timeline-scroll")
        .relative()
        .size_full()
        .overflow_y_scroll()
        .track_scroll(&panel.view_scroll)
        .on_drag_move(
            cx.listener(|_, event: &DragMoveEvent<DurationDrag>, _, cx| {
                let drag = event.drag(cx);
                if drag.owner == cx.entity_id() {
                    let delta =
                        f32::from(event.event.position.x - drag.origin.get()) as f64 / drag.scale;
                    drag.seconds
                        .set(((drag.field.seconds + delta).max(0.) * 100.).round() / 100.);
                    cx.notify();
                }
            }),
        )
        .on_drop(cx.listener(move |panel, drag: &DurationDrag, window, cx| {
            if drag.owner != cx.entity_id() {
                return;
            }
            panel.document.timeline.drag = None;
            let source = drag.document.borrow();
            if source.revision() != drag.revision {
                return;
            }
            let expected = source.contents().to_owned();
            let mut edited = expected.clone();
            edited.replace_range(
                drag.field.range.clone(),
                &drag.field.replacement(drag.seconds.get()),
            );
            drop(source);
            if edited != expected
                && let Err(error) = super::super::edits::apply_checked_edits(
                    &root,
                    &[(path.clone(), edited)],
                    &BTreeMap::from([(path.clone(), expected)]),
                    window,
                    cx,
                )
            {
                window.push_notification(Notification::error(error), cx);
            }
            cx.notify();
        }))
        .child(content)
        .child(
            canvas(
                move |bounds, _, cx| {
                    if bounds.size.width != viewport_width {
                        let _ = owner.update(cx, |_, cx| cx.notify());
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .into_any_element()
}

pub(in crate::app) fn cancel_drag(
    panel: &mut WorkbenchPanel,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    if panel.document.timeline.drag.take().is_some() {
        cx.stop_active_drag(window);
        true
    } else {
        false
    }
}
