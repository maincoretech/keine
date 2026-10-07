//! One owner for a Block drag: snapshot → preview → source write → settling.
use super::*;
use crate::app::reorder::{PositionMotion, edge_scroll_velocity};
use std::time::Instant;

#[derive(Default)]
pub(in crate::app) enum BlockDragState {
    #[default]
    Idle,
    Dragging(DragSession),
    Committing {
        session: DragSession,
        source: String,
        edited: String,
        motion: Option<BlockReorderMotion>,
    },
    Settling(BlockReorderMotion),
}

pub(in crate::app) struct DragRow {
    pub id: usize,
    pub end: usize,
    pub top: f32,
    pub height: f32,
}

pub(in crate::app) struct DragSession {
    pub token: Rc<()>,
    pub document: DocumentHandle,
    pub revision: u64,
    pub selected: HashSet<usize>,
    pub rows: Vec<DragRow>,
    pub moved: HashSet<usize>,
    pub target: Option<BlockDropTarget>,
    pub width: f32,
    pub height: f32,
    pub indent: f32,
    pub assets: Option<Vec<AssetKey>>,
    pub insertion: bool,
    hit_offset: f32,
    motion: PositionMotion,
    pointer: Option<Point<Pixels>>,
    last_candidate: Option<BlockDropTarget>,
    last_scroll_tick: Instant,
}

impl DragSession {
    pub fn new(
        drag: &BlockDrag,
        rows: Vec<DragRow>,
        ranges: &[Range<usize>],
        width: f32,
        indent: f32,
    ) -> Self {
        let moved: HashSet<_> = rows
            .iter()
            .filter(|row| ranges.iter().any(|range| range.contains(&row.id)))
            .map(|row| row.id)
            .collect();
        let height = (rows
            .iter()
            .filter(|row| moved.contains(&row.id))
            .map(|row| row.height + 4.)
            .sum::<f32>()
            - 4.)
            .max(0.);
        let positions = rows
            .iter()
            .map(|row| (row.id, row.top))
            .collect::<HashMap<_, _>>();
        Self {
            token: drag.token.clone(),
            document: drag.document.clone(),
            revision: drag.revision,
            selected: drag.selected.clone(),
            rows,
            moved,
            target: None,
            width,
            height,
            indent,
            assets: None,
            insertion: false,
            hit_offset: 0.,
            motion: PositionMotion::new(positions),
            pointer: None,
            last_candidate: None,
            last_scroll_tick: Instant::now(),
        }
    }

    pub fn is_current(&self, document: &DocumentHandle) -> bool {
        Rc::ptr_eq(document, &self.document) && document.borrow().revision() == self.revision
    }

    pub fn row(&self, id: usize) -> Option<&DragRow> {
        self.rows
            .binary_search_by_key(&id, |row| row.id)
            .ok()
            .map(|index| &self.rows[index])
    }

    pub fn position(&self, id: usize, reduce_motion: bool) -> Option<f32> {
        self.motion.position(id, reduce_motion)
    }

    pub fn animating(&self) -> bool {
        self.motion.animating()
    }

    pub fn retarget(
        &mut self,
        target: Option<BlockDropTarget>,
        insertion: bool,
        reduce_motion: bool,
    ) -> bool {
        if self.target == target && self.insertion == insertion {
            return false;
        }
        self.insertion = insertion;
        let destinations = if self.assets.is_some() {
            let top = target
                .filter(|_| self.insertion)
                .and_then(|target| self.insertion_top(target));
            self.rows
                .iter()
                .map(|row| {
                    (
                        row.id,
                        row.top
                            + if top.is_some_and(|top| row.top >= top) {
                                self.height + 4.
                            } else {
                                0.
                            },
                    )
                })
                .collect()
        } else {
            preview_positions(&self.rows, &self.moved, target)
        };
        self.target = target;
        self.motion.retarget(destinations, reduce_motion);
        true
    }

    pub fn painted_positions(&self, reduce_motion: bool) -> HashMap<usize, f32> {
        self.rows
            .iter()
            .map(|row| {
                (
                    row.id,
                    self.position(row.id, reduce_motion).unwrap_or(row.top),
                )
            })
            .collect()
    }

    fn insertion_top(&self, target: BlockDropTarget) -> Option<f32> {
        let row = self.row(target.row)?;
        if !target.after {
            return Some(row.top);
        }
        Some(
            self.rows
                .iter()
                .find(|next| next.id >= row.end)
                .map_or_else(
                    || {
                        self.rows
                            .last()
                            .map_or(row.top, |last| last.top + last.height + 4.)
                    },
                    |next| next.top,
                ),
        )
    }

    pub fn preview_top(&self, reduce_motion: bool) -> Option<f32> {
        if self.assets.is_some() {
            self.target
                .filter(|_| self.insertion)
                .and_then(|target| self.insertion_top(target))
        } else {
            self.target?;
            let first = self.rows.iter().find(|row| self.moved.contains(&row.id))?;
            self.position(first.id, reduce_motion)
        }
    }

    // Hit testing uses the original layout, never animated/transformed card bounds.
    pub fn candidate(&self, y: f32) -> Option<BlockDropTarget> {
        let first = self.rows.first()?;
        let last = self.rows.last()?;
        if y < first.top - 4. || y > last.top + last.height + 4. {
            return None;
        }
        let index = self
            .rows
            .partition_point(|row| y >= row.top + row.height + 4.);
        let row = self.rows.get(index).unwrap_or(last);
        Some(BlockDropTarget {
            row: row.id,
            after: y >= row.top + row.height * 0.5,
        })
    }
}

fn preview_positions(
    rows: &[DragRow],
    moved: &HashSet<usize>,
    target: Option<BlockDropTarget>,
) -> HashMap<usize, f32> {
    let Some(target) = target else {
        return rows.iter().map(|row| (row.id, row.top)).collect();
    };
    let Some(target_row) = rows.iter().find(|row| row.id == target.row) else {
        return rows.iter().map(|row| (row.id, row.top)).collect();
    };
    let insertion = if target.after {
        target_row.end
    } else {
        target_row.id
    };
    let (moving, mut remaining): (Vec<_>, Vec<_>) =
        rows.iter().partition(|row| moved.contains(&row.id));
    let at = remaining
        .iter()
        .position(|row| row.id >= insertion)
        .unwrap_or(remaining.len());
    remaining.splice(at..at, moving);
    let mut top = rows.first().map_or(0., |row| row.top);
    remaining
        .into_iter()
        .map(|row| {
            let position = (row.id, top);
            top += row.height + 4.;
            position
        })
        .collect()
}

impl BlockDragState {
    pub fn session(&self) -> Option<&DragSession> {
        match self {
            Self::Dragging(session) | Self::Committing { session, .. } => Some(session),
            _ => None,
        }
    }
    pub fn committing(&self) -> bool {
        matches!(self, Self::Committing { .. })
    }
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::Committing { source, .. } => Some(source),
            _ => None,
        }
    }
    pub fn motion(&self) -> Option<&BlockReorderMotion> {
        match self {
            Self::Settling(motion) => Some(motion),
            _ => None,
        }
    }
}

impl WorkbenchPanel {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::app) fn begin_block_drag(
        &mut self,
        drag: &BlockDrag,
        row_id: usize,
        fallback_width: f32,
        fallback_height: f32,
        indent: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (f32, f32) {
        self.focus.focus(window, cx);
        let bounds = self.document.block_row_bounds.borrow();
        let width = bounds
            .get(&row_id)
            .map_or(fallback_width, |bounds| f32::from(bounds.size.width));
        let height = bounds
            .get(&row_id)
            .map_or(fallback_height, |bounds| f32::from(bounds.size.height));
        if let PanelContent::Document {
            document: Some(document),
            ..
        } = &self.content
            && Rc::ptr_eq(document, &drag.document)
            && document.borrow().revision() == drag.revision
            && !self.document.block_drag.committing()
        {
            let projection = document.borrow().projection();
            if let Some(scene) = projection.scenes.iter().find(|scene| {
                scene
                    .blocks
                    .iter()
                    .any(|block| block.source_range.start == row_id)
            }) {
                let ranges = scene
                    .blocks
                    .iter()
                    .filter(|block| drag.selected.contains(&block.source_range.start))
                    .map(|block| block.source_range.clone())
                    .collect::<Vec<_>>();
                let positions = self.document.block_row_positions.borrow();
                let rows = scene
                    .blocks
                    .iter()
                    .filter(|block| !block.is_textbox_ending())
                    .filter_map(|block| {
                        positions.get(&block.source_range.start).map(|top| DragRow {
                            id: block.source_range.start,
                            end: block.source_range.end,
                            top: *top,
                            height: bounds.get(&block.source_range.start).map_or_else(
                                || {
                                    block_row_height(
                                        block,
                                        &self.document.block_text_editors,
                                        self.document.draft_text.as_ref(),
                                        &self.document.block_heights,
                                        cx,
                                    )
                                },
                                |bounds| f32::from(bounds.size.height),
                            ),
                        })
                    })
                    .collect();
                let mut session = DragSession::new(drag, rows, &ranges, width, indent);
                // The scroll container includes its own inset and virtualized
                // preceding scenes. Anchor hit coordinates to the painted row,
                // while animation positions stay in the projection's space.
                session.hit_offset =
                    bounds
                        .get(&row_id)
                        .zip(positions.get(&row_id))
                        .map_or(0., |(bounds, top)| {
                            f32::from(
                                bounds.top()
                                    - self.view_scroll.bounds().top()
                                    - self.view_scroll.offset().y,
                            ) - top
                        });
                self.document.block_drag = BlockDragState::Dragging(session);
            }
        }
        drop(bounds);
        self.update_block_drag_target(window.mouse_position(), cx);
        cx.notify();
        (width, height)
    }

    pub(in crate::app) fn refresh_block_drag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = match &self.content {
            PanelContent::Document {
                document: Some(document),
                ..
            } => Some(document),
            _ => None,
        };
        match &self.document.block_drag {
            BlockDragState::Dragging(session) => {
                let stale = current.is_none_or(|document| !session.is_current(document))
                    || self.document.document_mode != DocumentMode::Block;
                if stale {
                    cx.stop_active_drag(window);
                    self.document.block_drag = BlockDragState::Idle;
                } else if !cx.has_active_drag() || !window.is_window_active() {
                    self.cancel_block_drag(window, cx);
                }
            }
            BlockDragState::Settling(motion)
                if cx.reduce_motion()
                    || motion.is_finished()
                    || current.map(|document| document.borrow().revision()) != motion.revision =>
            {
                self.document.block_drag = BlockDragState::Idle
            }
            _ => {}
        }
        self.scroll_block_drag(window, cx);
    }

    fn scroll_block_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let BlockDragState::Dragging(session) = &mut self.document.block_drag else {
            return;
        };
        let Some(pointer) = session.pointer else {
            return;
        };
        let now = Instant::now();
        let elapsed = now
            .duration_since(session.last_scroll_tick)
            .as_secs_f32()
            .min(0.05);
        session.last_scroll_tick = now;
        let viewport = self.view_scroll.bounds();
        if viewport.contains(&pointer) {
            let speed = edge_scroll_velocity(
                f32::from(pointer.y - viewport.top()),
                f32::from(viewport.size.height),
            );
            let mut offset = self.view_scroll.offset();
            let next =
                (offset.y + px(speed * elapsed)).clamp(-self.view_scroll.max_offset().y, px(0.));
            if next != offset.y {
                offset.y = next;
                self.view_scroll.set_offset(offset);
                window.request_animation_frame();
            }
        }
        // Also re-evaluate a stationary pointer after a wheel/edge scroll.
        self.update_block_drag_target(pointer, cx);
    }

    pub(in crate::app) fn cancel_block_drag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.document.block_drag, BlockDragState::Dragging(_)) {
            return;
        }
        let BlockDragState::Dragging(session) = std::mem::take(&mut self.document.block_drag)
        else {
            unreachable!()
        };
        cx.stop_active_drag(window);
        self.document.block_drag = BlockDragState::Settling(BlockReorderMotion {
            positions: session.painted_positions(cx.reduce_motion()),
            heights: session
                .rows
                .iter()
                .map(|row| (row.id, row.height))
                .collect(),
            started_at: Some(Instant::now()),
            revision: Some(session.revision),
        });
        cx.notify();
    }

    pub(in crate::app) fn track_block_drag(
        &mut self,
        event: &DragMoveEvent<BlockDrag>,
        cx: &mut Context<Self>,
    ) {
        let drag = event.drag(cx);
        let BlockDragState::Dragging(session) = &self.document.block_drag else {
            return;
        };
        if !Rc::ptr_eq(&session.token, &drag.token) {
            return;
        }
        self.update_block_drag_target(event.event.position, cx);
    }

    pub(in crate::app) fn track_asset_drag(
        &mut self,
        event: &DragMoveEvent<AssetDrag>,
        row_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let drag = event.drag(cx);
        let PanelContent::Document {
            root,
            document: Some(document),
            ..
        } = &self.content
        else {
            return;
        };
        if *root != drag.root
            || self.document.document_mode != DocumentMode::Block
            || self.document.block_drag.committing()
        {
            return;
        }
        let same = self.document.block_drag.session().is_some_and(|session| {
            Rc::ptr_eq(&session.token, &drag.token) && session.row(row_id).is_some()
        });
        if !same {
            let keys = drag.keys.clone();
            let index = cx.global::<EditorDocuments>().authoring(root);
            if keys.is_empty()
                || keys.iter().any(|key| {
                    !index
                        .assets
                        .iter()
                        .any(|asset| asset.key() == *key && asset.exists)
                })
            {
                return;
            }
            let payload = BlockDrag {
                token: drag.token.clone(),
                document: document.clone(),
                revision: document.borrow().revision(),
                selected: HashSet::new(),
            };
            let projection = document.borrow().projection();
            let indent = projection
                .scenes
                .iter()
                .flat_map(|scene| &scene.blocks)
                .find(|block| block.source_range.start == row_id)
                .map_or(0., |block| block.depth as f32 * 18.);
            self.begin_block_drag(&payload, row_id, 300., 38., indent, window, cx);
            if let BlockDragState::Dragging(session) = &mut self.document.block_drag {
                session.height = keys.iter().collect::<HashSet<_>>().len() as f32 * 42. - 4.;
                session.assets = Some(keys);
                session.last_candidate = None;
            }
        }
        self.update_block_drag_target(event.event.position, cx);
    }

    pub(in crate::app) fn update_block_drag_target(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let BlockDragState::Dragging(session) = &mut self.document.block_drag else {
            return;
        };
        session.pointer = Some(position);
        let viewport = self.view_scroll.bounds();
        let mut y = f32::from(position.y - viewport.origin.y - self.view_scroll.offset().y)
            - session.hit_offset;
        if viewport.contains(&position)
            && session.assets.is_some()
            && let Some(top) = session.preview_top(cx.reduce_motion())
        {
            let bottom = top + session.height + 4.;
            // The external asset adds space to the layout. Keep its gap stable
            // under the pointer, then map rows below it back to the snapshot.
            if (top..bottom).contains(&y) {
                return;
            }
            if y >= bottom {
                y -= session.height + 4.;
            }
        }
        let mut candidate = viewport
            .contains(&position)
            .then(|| session.candidate(y))
            .flatten();
        let mut insertion = false;
        let mut indent = session.indent;
        if let Some(keys) = &session.assets
            && let Some(target) = candidate.as_mut()
            && let Some(row) = session.row(target.row)
        {
            let projection = session.document.borrow().projection();
            if let Some(block) = projection
                .scenes
                .iter()
                .flat_map(|scene| &scene.blocks)
                .find(|block| block.source_range.start == target.row)
            {
                let voice = keys.iter().any(|key| key.kind == AssetKind::Voice);
                insertion = !voice
                    && (keys.len() > 1
                        || !matches!(block.kind, BlockKind::Command)
                        || y <= row.top + 6.
                        || y >= row.top + row.height - 6.);
                if !matches!(block.kind, BlockKind::Command) && keys.len() == 1 && y > row.top + 6.
                {
                    target.after = true;
                }
                indent = block.depth as f32 * 18.;
                if block.read_only
                    || voice
                        && (!matches!(
                            block.kind,
                            BlockKind::Narration | BlockKind::Dialogue { .. }
                        ) || keys.len() != 1)
                    || insertion
                        && !target.after
                        && matches!(block.kind, BlockKind::ElseIf | BlockKind::Else)
                {
                    candidate = None;
                }
            }
        }
        if session.last_candidate == candidate && session.insertion == insertion {
            return;
        }
        session.last_candidate = candidate;
        let target = candidate.filter(|target| matches!(&self.content, PanelContent::Document {document: Some(document), ..} if session.is_current(document) && (session.assets.is_some() || document.borrow().projection().accepts_block_drop(&session.selected, target.row))));
        if let BlockDragState::Dragging(session) = &mut self.document.block_drag {
            session.indent = indent;
            if session.retarget(target, insertion, cx.reduce_motion()) {
                cx.notify();
            }
        }
    }

    pub(in crate::app) fn finish_block_drag(
        &mut self,
        drag: &BlockDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let BlockDragState::Dragging(session) = &self.document.block_drag else {
            return;
        };
        if !Rc::ptr_eq(&session.token, &drag.token) {
            return;
        }
        // GPUI starts a drag on the first move beyond its threshold. A short
        // gesture can release without another DragMoveEvent; resolve the final
        // pointer position even in that case.
        self.update_block_drag_target(window.mouse_position(), cx);
        let BlockDragState::Dragging(session) = &self.document.block_drag else {
            return;
        };
        let Some(target) = session.target else {
            self.cancel_block_drag(window, cx);
            return;
        };
        let source = match &self.content {
            PanelContent::Document {
                document: Some(document),
                ..
            } if session.is_current(document)
                && self.document.document_mode == DocumentMode::Block =>
            {
                document.borrow().contents().to_owned()
            }
            _ => {
                self.document.block_drag = BlockDragState::Idle;
                cx.notify();
                return;
            }
        };
        let projection = EiyashouProjection::parse(&source);
        match projection.move_blocks_to(&source, &session.selected, target.row, target.after) {
            Ok(edited) if edited != source => {
                let heights = session
                    .rows
                    .iter()
                    .map(|row| (row.id, row.height))
                    .collect();
                let motion = BlockReorderMotion::for_drop(
                    &projection,
                    &EiyashouProjection::parse(&edited),
                    &session.selected,
                    target.row,
                    target.after,
                    &session.painted_positions(cx.reduce_motion()),
                    &heights,
                );
                let BlockDragState::Dragging(session) =
                    std::mem::take(&mut self.document.block_drag)
                else {
                    return;
                };
                self.document.block_drag = BlockDragState::Committing {
                    session,
                    source,
                    edited: edited.clone(),
                    motion,
                };
                self.apply_block_source(edited, "Blocks moved", window, cx);
            }
            Ok(_) => self.cancel_block_drag(window, cx),
            Err(error) => {
                self.cancel_block_drag(window, cx);
                self.set_block_notice(format!("Move blocked: {error}"), cx);
            }
        }
    }

    pub(in crate::app) fn finish_asset_drag(
        &mut self,
        drag: &AssetDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document.block_drag.session().is_some_and(|session| {
            session.assets.is_some() && Rc::ptr_eq(&session.token, &drag.token)
        }) {
            return;
        }
        self.update_block_drag_target(window.mouse_position(), cx);
        let BlockDragState::Dragging(session) = &self.document.block_drag else {
            return;
        };
        let Some(target) = session.target else {
            self.cancel_block_drag(window, cx);
            return;
        };
        let PanelContent::Document {
            root,
            document: Some(document),
            ..
        } = &self.content
        else {
            return;
        };
        if !session.is_current(document)
            || self.document.document_mode != DocumentMode::Block
            || *root != drag.root
        {
            self.cancel_block_drag(window, cx);
            return;
        }
        let source = document.borrow().contents().to_owned();
        let index = cx.global::<EditorDocuments>().authoring(root);
        match super::super::edits::asset_drop_edit_at(
            &source,
            target.row,
            &drag.keys,
            &index,
            session.insertion.then_some(target.after),
        ) {
            Ok(edited) if edited != source => {
                let motion = session
                    .insertion
                    .then(|| {
                        BlockReorderMotion::for_insert(
                            &EiyashouProjection::parse(&source),
                            &EiyashouProjection::parse(&edited),
                            target,
                            &session.painted_positions(cx.reduce_motion()),
                            &session
                                .rows
                                .iter()
                                .map(|row| (row.id, row.height))
                                .collect(),
                            session.preview_top(cx.reduce_motion()).unwrap_or(0.),
                        )
                    })
                    .flatten();
                let BlockDragState::Dragging(session) =
                    std::mem::take(&mut self.document.block_drag)
                else {
                    return;
                };
                self.document.block_drag = BlockDragState::Committing {
                    session,
                    source,
                    edited: edited.clone(),
                    motion,
                };
                self.apply_block_source(edited, "Assets inserted", window, cx);
            }
            Ok(_) => self.cancel_block_drag(window, cx),
            Err(error) => {
                self.cancel_block_drag(window, cx);
                self.set_block_notice(format!("Asset drop blocked: {error}"), cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rows() -> Vec<DragRow> {
        vec![
            DragRow {
                id: 0,
                end: 2,
                top: 76.,
                height: 38.,
            },
            DragRow {
                id: 1,
                end: 2,
                top: 118.,
                height: 58.,
            },
            DragRow {
                id: 2,
                end: 3,
                top: 180.,
                height: 38.,
            },
            DragRow {
                id: 3,
                end: 4,
                top: 222.,
                height: 38.,
            },
        ]
    }
    #[test]
    fn structure_preview_preserves_total_height_and_nested_spacing() {
        let rows = rows();
        let positions = preview_positions(
            &rows,
            &HashSet::from([0, 1]),
            Some(BlockDropTarget {
                row: 3,
                after: true,
            }),
        );
        assert_eq!(
            [positions[&2], positions[&3], positions[&0], positions[&1]],
            [76., 118., 160., 202.]
        );
        assert_eq!(positions[&1] + rows[1].height, 260.);
        assert_eq!(rows.last().unwrap().top + rows.last().unwrap().height, 260.);
    }
    #[test]
    fn after_a_structure_inserts_below_all_its_children() {
        let positions = preview_positions(
            &rows(),
            &HashSet::from([3]),
            Some(BlockDropTarget {
                row: 0,
                after: true,
            }),
        );
        assert_eq!(
            [positions[&0], positions[&1], positions[&3], positions[&2]],
            [76., 118., 180., 222.]
        );
        assert_eq!(
            preview_positions(&rows(), &HashSet::from([3]), None)[&3],
            222.
        );
    }
    #[test]
    fn snapshot_hit_testing_and_cancellation_do_not_follow_animated_rows() {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/native-smoke");
        let recovery = std::env::temp_dir().join(format!("keine-drag-unit-{}", std::process::id()));
        let mut documents = crate::document::DocumentManager::new(root, recovery.clone()).unwrap();
        let document = documents.open("scripts/main.shou").unwrap();
        let drag = BlockDrag {
            token: Rc::new(()),
            document: document.clone(),
            revision: document.borrow().revision(),
            selected: HashSet::from([0]),
        };
        let range = 0..2;
        let mut session = DragSession::new(&drag, rows(), std::slice::from_ref(&range), 312., 18.);
        let target = Some(BlockDropTarget {
            row: 3,
            after: true,
        });
        assert!(session.retarget(target, false, true));
        let started = session.motion.started;
        assert!(!session.retarget(target, false, true));
        assert_eq!(session.motion.started, started);
        assert_eq!(session.position(2, true), Some(76.));
        assert_eq!(
            session.candidate(200.),
            Some(BlockDropTarget {
                row: 2,
                after: true
            })
        );
        assert_eq!((session.width, session.height), (312., 100.));
        assert!(session.retarget(None, false, true));
        assert_eq!(session.position(2, true), Some(180.));
        assert_eq!(session.candidate(300.), None);
        assert!(session.is_current(&document));
        // Incoming assets use the same fixed geometry and animation, but leave
        // existing rows in place and open an insertion slot after descendants.
        let mut incoming = DragSession::new(&drag, rows(), &[], 312., 18.);
        incoming.assets = Some(vec![AssetKey {
            kind: AssetKind::Background,
            id: "room".into(),
        }]);
        incoming.height = 38.;
        assert!(incoming.retarget(
            Some(BlockDropTarget {
                row: 0,
                after: true
            }),
            true,
            true
        ));
        assert_eq!(incoming.preview_top(true), Some(180.));
        assert_eq!(incoming.position(1, true), Some(118.));
        assert_eq!(incoming.position(2, true), Some(222.));
        assert!(incoming.retarget(
            Some(BlockDropTarget {
                row: 2,
                after: false
            }),
            true,
            true
        ));
        assert_eq!(incoming.preview_top(true), Some(180.));
        assert!(incoming.retarget(None, false, true));
        assert_eq!(incoming.position(2, true), Some(180.));
        assert_eq!(incoming.preview_top(true), None);
        let foreign = crate::document::DocumentManager::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/native-smoke"),
            recovery.join("foreign"),
        );
        // Cloning a document manager's handle preserves identity; reloading into
        // a separate manager must not inherit an in-flight drag at revision 0.
        let mut foreign = foreign.unwrap();
        let foreign_document = foreign.open("scripts/main.shou").unwrap();
        assert_eq!(foreign_document.borrow().revision(), session.revision);
        assert!(!session.is_current(&foreign_document));
        document
            .borrow_mut()
            .replace_contents("scene changed { wait(1s) }".into())
            .unwrap();
        assert!(!session.is_current(&document));
        std::fs::remove_dir_all(recovery).unwrap();
    }

    #[test]
    fn edge_scroll_stops_in_the_middle_and_bounds_its_speed() {
        assert_eq!(edge_scroll_velocity(0., 600.), 420.);
        assert_eq!(edge_scroll_velocity(600., 600.), -420.);
        assert_eq!(edge_scroll_velocity(300., 600.), 0.);
        assert_eq!(edge_scroll_velocity(-100., 600.), 420.);
    }
}
