//! Palette ordering is a preferences transaction; it never edits a script.
use super::*;
use crate::app::reorder::{PositionMotion, edge_scroll_velocity};
use std::time::Instant;

#[derive(Clone)]
pub(in crate::app) struct PickerDrag {
    pub token: Rc<()>,
    pub kind: InsertKind,
}

pub(in crate::app) fn picker_item_id(kind: InsertKind) -> usize {
    InsertKind::ALL
        .iter()
        .position(|candidate| *candidate == kind)
        .expect("catalogue item")
}

pub(in crate::app) struct PickerDragSession {
    pub token: Rc<()>,
    source: usize,
    kinds: Vec<InsertKind>,
    baseline: BlockPickerPreferences,
    category: Option<&'static str>,
    target: Option<(usize, bool)>,
    bases: HashMap<usize, f32>,
    motion: PositionMotion,
    pub settling: bool,
    pointer: Option<Point<Pixels>>,
    last_scroll_tick: Instant,
}

impl PickerDragSession {
    pub fn offset(&self, id: usize, reduce_motion: bool) -> f32 {
        self.motion.position(id, reduce_motion).unwrap_or(0.)
            - self.bases.get(&id).copied().unwrap_or(0.)
    }

    pub fn hidden(&self, id: usize) -> bool {
        !self.settling && self.source == id
    }

    pub fn placeholder_top(&self, reduce_motion: bool) -> Option<f32> {
        (!self.settling && self.target.is_some())
            .then(|| self.motion.position(self.source, reduce_motion))
            .flatten()
    }

    fn ordered(&self) -> Vec<usize> {
        let ids = self
            .kinds
            .iter()
            .copied()
            .map(picker_item_id)
            .collect::<Vec<_>>();
        let Some((target, after)) = self.target else {
            return ids;
        };
        reordered_ids(&ids, self.source, target, after)
    }

    fn destinations(&self) -> HashMap<usize, f32> {
        self.ordered()
            .into_iter()
            .zip(
                self.kinds
                    .iter()
                    .map(|kind| self.bases[&picker_item_id(*kind)]),
            )
            .collect()
    }
}

fn reordered_ids(ids: &[usize], source: usize, target: usize, after: bool) -> Vec<usize> {
    if source == target || !ids.contains(&source) || !ids.contains(&target) {
        return ids.to_vec();
    }
    let mut ordered = ids
        .iter()
        .copied()
        .filter(|id| *id != source)
        .collect::<Vec<_>>();
    let at = ordered.iter().position(|id| *id == target).unwrap() + usize::from(after);
    ordered.insert(at, source);
    ordered
}

impl WorkbenchPanel {
    pub(in crate::app) fn begin_picker_drag(
        &mut self,
        drag: &PickerDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.picker.block_picker_customize
            || self.picker.block_picker_closing
            || !self.picker.block_picker_input.read(cx).value().is_empty()
        {
            return;
        }
        let kinds = self.filtered_picker_kinds(cx);
        let viewport = self.picker.picker_scroll.bounds();
        let bounds = self.picker.picker_row_bounds.borrow();
        let bases = kinds
            .iter()
            .filter_map(|kind| {
                let id = picker_item_id(*kind);
                bounds.get(&id).map(|bounds| {
                    (
                        id,
                        f32::from(
                            bounds.top() - viewport.top() - self.picker.picker_scroll.offset().y,
                        ),
                    )
                })
            })
            .collect::<HashMap<_, _>>();
        // Start only after every row has an actual layout, just like Blocks.
        if bases.len() != kinds.len() {
            return;
        }
        drop(bounds);
        self.picker.picker_drag = Some(PickerDragSession {
            token: drag.token.clone(),
            source: picker_item_id(drag.kind),
            kinds,
            baseline: cx
                .global::<EditorDocuments>()
                .block_picker_preferences()
                .clone(),
            category: self.picker.block_picker_category,
            target: None,
            motion: PositionMotion::new(bases.clone()),
            bases,
            settling: false,
            pointer: None,
            last_scroll_tick: Instant::now(),
        });
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(in crate::app) fn update_picker_drag(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let viewport = self.picker.picker_scroll.bounds();
        let Some(session) = self
            .picker
            .picker_drag
            .as_mut()
            .filter(|session| !session.settling)
        else {
            return;
        };
        session.pointer = Some(position);
        let y = f32::from(position.y - viewport.top() - self.picker.picker_scroll.offset().y);
        let source_kind = InsertKind::ALL[session.source];
        let target = viewport
            .contains(&position)
            .then(|| {
                session
                    .kinds
                    .iter()
                    .copied()
                    .find(|kind| {
                        let top = session.bases[&picker_item_id(*kind)];
                        (top - 2. ..top + 36.).contains(&y)
                    })
                    .filter(|kind| {
                        session.category == Some("Favorites")
                            || kind.category() == source_kind.category()
                    })
                    .map(|kind| {
                        let id = picker_item_id(kind);
                        (id, y >= session.bases[&id] + 17.)
                    })
            })
            .flatten();
        if session.target == target {
            return;
        }
        session.target = target;
        session
            .motion
            .retarget(session.destinations(), cx.reduce_motion());
        cx.notify();
    }

    pub(in crate::app) fn cancel_picker_drag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .picker
            .picker_drag
            .as_mut()
            .filter(|session| !session.settling)
        else {
            return;
        };
        session.target = None;
        session
            .motion
            .retarget(session.bases.clone(), cx.reduce_motion());
        session.settling = true;
        cx.stop_active_drag(window);
        cx.notify();
    }

    pub(in crate::app) fn refresh_picker_drag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.picker.picker_drag.as_ref() else {
            return;
        };
        if session.settling {
            if cx.reduce_motion() || !session.motion.animating() {
                self.picker.picker_drag = None;
            } else {
                window.request_animation_frame();
            }
            return;
        }
        let stale = !self.picker.block_picker_open
            || self.picker.block_picker_closing
            || !self.picker.block_picker_customize
            || self.document.document_mode != DocumentMode::Block
            || self.picker.block_picker_category != session.category
            || !self.picker.block_picker_input.read(cx).value().is_empty()
            || cx.global::<EditorDocuments>().block_picker_preferences() != &session.baseline;
        if stale {
            self.cancel_picker_drag(window, cx);
            self.picker.picker_drag = None;
            return;
        }
        if !cx.has_active_drag() || !window.is_window_active() {
            self.cancel_picker_drag(window, cx);
            return;
        }
        let session = self.picker.picker_drag.as_mut().unwrap();
        let now = Instant::now();
        let dt = now
            .duration_since(session.last_scroll_tick)
            .as_secs_f32()
            .min(0.05);
        session.last_scroll_tick = now;
        let pointer = session.pointer;
        let animating = session.motion.animating();
        if let Some(pointer) = pointer {
            let viewport = self.picker.picker_scroll.bounds();
            if viewport.contains(&pointer) {
                let speed = edge_scroll_velocity(
                    f32::from(pointer.y - viewport.top()),
                    f32::from(viewport.size.height),
                );
                let mut offset = self.picker.picker_scroll.offset();
                let next = (offset.y + px(speed * dt))
                    .clamp(-self.picker.picker_scroll.max_offset().y, px(0.));
                if next != offset.y {
                    offset.y = next;
                    self.picker.picker_scroll.set_offset(offset);
                    window.request_animation_frame();
                }
            }
            self.update_picker_drag(pointer, cx);
        }
        if animating && !cx.reduce_motion() {
            window.request_animation_frame();
        }
    }

    pub(in crate::app) fn drop_picker_drag(
        &mut self,
        drag: &PickerDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .picker
            .picker_drag
            .as_ref()
            .filter(|session| !session.settling && Rc::ptr_eq(&session.token, &drag.token))
        else {
            return;
        };
        if !self.picker.block_picker_open
            || self.picker.block_picker_closing
            || !self.picker.block_picker_customize
            || self.document.document_mode != DocumentMode::Block
            || self.picker.block_picker_category != session.category
            || !self.picker.block_picker_input.read(cx).value().is_empty()
            || cx.global::<EditorDocuments>().block_picker_preferences() != &session.baseline
            || session.target.is_none()
        {
            self.cancel_picker_drag(window, cx);
            return;
        }
        let ordered = session.ordered();
        let source = session.source;
        let favorites = session.category == Some("Favorites");
        let group = session
            .kinds
            .iter()
            .filter(|kind| favorites || kind.category() == drag.kind.category())
            .map(|kind| picker_item_id(*kind))
            .collect::<Vec<_>>();
        let reordered_group = ordered
            .into_iter()
            .filter(|id| group.contains(id))
            .collect::<Vec<_>>();
        if group == reordered_group {
            self.cancel_picker_drag(window, cx);
            return;
        }
        let before = group.iter().position(|id| *id == source).unwrap();
        let after = reordered_group.iter().position(|id| *id == source).unwrap();
        self.move_picker_item(drag.kind, after as isize - before as isize, cx);
        let saved = cx.global::<EditorDocuments>().block_picker_preferences();
        let session = self.picker.picker_drag.as_mut().unwrap();
        // Preference write is the only commit; finish visuals at the new bases.
        if saved == &session.baseline {
            self.cancel_picker_drag(window, cx);
            return;
        }
        let destinations = session.destinations();
        session
            .motion
            .retarget(destinations.clone(), cx.reduce_motion());
        session.bases = destinations;
        session.settling = true;
        cx.notify();
    }
}

pub(in crate::app) fn render_picker_grip(
    kind: InsertKind,
    cx: &mut Context<WorkbenchPanel>,
) -> impl IntoElement {
    let panel = cx.entity().downgrade();
    div()
        .id(("picker-grip", picker_item_id(kind)))
        .size(px(18.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor_move()
        .on_drag(
            PickerDrag {
                token: Rc::new(()),
                kind,
            },
            move |drag, _, window, cx| {
                let width = panel
                    .update(cx, |panel, cx| {
                        let width = panel
                            .picker
                            .picker_row_bounds
                            .borrow()
                            .get(&picker_item_id(kind))
                            .map_or(300., |bounds| f32::from(bounds.size.width));
                        panel.begin_picker_drag(drag, window, cx);
                        width
                    })
                    .unwrap_or(300.);
                cx.new(|_| crate::app::panel::BlockDragPreview {
                    label: kind.label().into(),
                    summary: String::new(),
                    icon: insert_kind_icon(kind),
                    count: 1,
                    width,
                    height: 34.,
                    color: PRIMARY,
                    grip_top: 8.,
                })
            },
        )
        .child(
            Icon::new(AssetIconName::GripVertical)
                .xsmall()
                .text_color(rgb(MUTED)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui_kit::test]
    fn palette_drag_commits_on_drop_and_cancel_keeps_source(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::{Modifiers, VisualTestContext, point};
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/native-smoke");
        let relative = PathBuf::from("scripts/main.shou");
        let source = fs::read_to_string(root.join(&relative)).unwrap();
        let temporary =
            std::env::temp_dir().join(format!("keine-picker-drag-{}", std::process::id()));
        let persistence = crate::persistence::AppPersistence::new(temporary.clone());
        let window = cx.update(|cx| {
            gpui_kit::init(cx);
            let session = crate::workspace::WorkspaceSession::open(&root).unwrap();
            let mut documents = EditorDocuments::new(persistence.clone());
            documents
                .ensure_workspace_with_files(session.root(), session.files())
                .unwrap();
            cx.set_global(documents);
            cx.open_window(gpui_kit::WindowOptions::default(), |window, cx| {
                WorkbenchPanel::from_payload(
                    crate::app::panel::PanelPayload::Document {
                        root: session.root().to_owned(),
                        relative: relative.clone(),
                        view: crate::app::panel::DocumentView::default(),
                    },
                    window,
                    cx,
                )
                .unwrap()
            })
            .unwrap()
        });
        cx.simulate_window_resize(window.into(), size(px(640.), px(420.)));
        let panel = window.root(cx).unwrap();
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        cx.update(|window, cx| {
            window.activate_window();
            panel.update(cx, |panel, cx| {
                panel.switch_document_mode(DocumentMode::Block, window, cx);
                panel.toggle_block_picker(&ToggleBlockPicker, window, cx);
                panel.picker.block_picker_customize = true;
                cx.notify();
            })
        });
        cx.run_until_parked();
        let baseline = cx.read(|cx| {
            cx.global::<EditorDocuments>()
                .block_picker_preferences()
                .clone()
        });
        let (start, target) = panel.read_with(cx, |panel, _| {
            let bounds = panel.picker.picker_row_bounds.borrow();
            let first = bounds[&picker_item_id(InsertKind::Narration)];
            let target = bounds[&picker_item_id(InsertKind::Bgm)];
            (
                point(first.left() + px(17.), first.top() + px(17.)),
                point(target.left() + px(17.), target.top() + px(28.)),
            )
        });
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            start + point(px(12.), px(0.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.run_until_parked();
        cx.simulate_mouse_move(target, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        assert!(panel.read_with(cx, |panel, _| {
            panel
                .picker
                .picker_drag
                .as_ref()
                .is_some_and(|drag| drag.target.is_some())
        }));
        assert_eq!(
            cx.read(|cx| cx
                .global::<EditorDocuments>()
                .block_picker_preferences()
                .clone()),
            baseline
        );
        cx.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        let saved = cx.read(|cx| {
            cx.global::<EditorDocuments>()
                .block_picker_preferences()
                .clone()
        });
        assert_eq!(
            &saved.favorites[..5],
            &["Dialogue", "Background", "Figure", "BGM", "Narration"]
        );
        assert_eq!(persistence.load_block_picker_preferences(), saved);
        // Customization must never invoke the script insertion action.
        cx.simulate_click(target, Modifiers::default());
        cx.run_until_parked();
        let start = panel.read_with(cx, |panel, _| {
            let bounds =
                panel.picker.picker_row_bounds.borrow()[&picker_item_id(InsertKind::Dialogue)];
            point(bounds.left() + px(17.), bounds.top() + px(17.))
        });
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            start + point(px(12.), px(0.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.run_until_parked();
        let outside = point(px(1.), px(1.));
        cx.simulate_mouse_move(outside, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| cx
                .global::<EditorDocuments>()
                .block_picker_preferences()
                .clone()),
            saved
        );
        // Changing the category immediately before a drop invalidates the transaction.
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            start + point(px(12.), px(0.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.run_until_parked();
        cx.simulate_mouse_move(target, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let token = panel.picker.picker_drag.as_ref().unwrap().token.clone();
                panel.picker.block_picker_category = Some("Text");
                panel.drop_picker_drag(
                    &PickerDrag {
                        token,
                        kind: InsertKind::Dialogue,
                    },
                    window,
                    cx,
                );
            })
        });
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| cx
                .global::<EditorDocuments>()
                .block_picker_preferences()
                .clone()),
            saved
        );
        // Esc-style dismissal fades the Block context menu; an old timer cannot close a newly opened one.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.picker.block_picker_open = false;
                panel.open_block_context_menu(0, gpui_kit::point(px(200.), px(80.)), window, cx);
                panel.close_block_picker(&CloseBlockPicker, window, cx);
                assert!(panel.document.block_context_menu.as_ref().unwrap().closing);
                panel.open_block_context_menu(0, gpui_kit::point(px(200.), px(80.)), window, cx);
            })
        });
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        assert!(panel.read_with(cx, |panel, _| {
            panel
                .document
                .block_context_menu
                .as_ref()
                .is_some_and(|menu| !menu.closing)
        }));
        cx.update(|window, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, cx| {
                panel.close_block_context_menu(window, cx);
                assert!(panel.document.block_context_menu.is_none());
            });
        });
        panel.read_with(cx, |panel, _| {
            let PanelContent::Document {
                document: Some(document),
                ..
            } = &panel.content
            else {
                unreachable!()
            };
            assert_eq!(document.borrow().contents(), source);
        });
        assert_eq!(fs::read_to_string(root.join(relative)).unwrap(), source);
        fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn reorder_keeps_one_identity_and_handles_both_directions_and_noop() {
        assert_eq!(reordered_ids(&[0, 1, 2, 3], 0, 2, true), [1, 2, 0, 3]);
        assert_eq!(reordered_ids(&[0, 1, 2, 3], 3, 1, false), [0, 3, 1, 2]);
        assert_eq!(reordered_ids(&[0, 1, 2], 1, 1, true), [0, 1, 2]);
        assert_eq!(reordered_ids(&[0, 1, 2], 5, 1, true), [0, 1, 2]);
    }
}
