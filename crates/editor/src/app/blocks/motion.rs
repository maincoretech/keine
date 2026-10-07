//! Presentation-only identity mapping across a completed source reorder.
use super::*;
use std::time::Instant;

use crate::app::reorder::REORDER_DURATION;

pub(in crate::app) struct BlockReorderMotion {
    pub(in crate::app) positions: HashMap<usize, f32>,
    pub(in crate::app) heights: HashMap<usize, f32>,
    pub(in crate::app) started_at: Option<Instant>,
    pub(in crate::app) revision: Option<u64>,
}

impl BlockReorderMotion {
    pub(in crate::app) fn progress(&self) -> f32 {
        self.started_at.map_or(1., |started| {
            ease_out_quint()(
                (started.elapsed().as_secs_f32() / REORDER_DURATION.as_secs_f32()).min(1.),
            )
        })
    }

    pub(in crate::app) fn is_finished(&self) -> bool {
        self.started_at
            .is_some_and(|started| started.elapsed() >= REORDER_DURATION)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::app) fn for_drop(
        before: &EiyashouProjection,
        after: &EiyashouProjection,
        selected: &HashSet<usize>,
        target: usize,
        below: bool,
        positions: &HashMap<usize, f32>,
        heights: &HashMap<usize, f32>,
    ) -> Option<Self> {
        let old = before
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .collect::<Vec<_>>();
        let new = after
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .collect::<Vec<_>>();
        if old.len() != new.len() {
            return None;
        }
        let target = old
            .iter()
            .find(|block| block.source_range.start == target)?;
        let insertion = if below {
            target.source_range.end
        } else {
            target.source_range.start
        };
        // Moving a structure carries all descendants, including hidden text endings.
        // Use the explicit selected ranges, never text matching (duplicate rows are valid).
        let mut ranges: Vec<Range<usize>> = Vec::new();
        for block in old
            .iter()
            .filter(|block| selected.contains(&block.source_range.start))
        {
            if ranges
                .last()
                .is_none_or(|parent| parent.end < block.source_range.end)
            {
                ranges.push(block.source_range.clone());
            }
        }
        let mut range_index = 0;
        let (moved, mut remaining): (Vec<_>, Vec<_>) = old.into_iter().partition(|block| {
            while ranges
                .get(range_index)
                .is_some_and(|range| range.end <= block.source_range.start)
            {
                range_index += 1;
            }
            ranges
                .get(range_index)
                .is_some_and(|range| range.contains(&block.source_range.start))
        });
        let at = remaining
            .iter()
            .position(|block| block.source_range.start >= insertion)
            .unwrap_or(remaining.len());
        remaining.splice(at..at, moved);
        let mut motion = Self {
            positions: HashMap::new(),
            heights: HashMap::new(),
            started_at: None,
            revision: None,
        };
        for (old, new) in remaining.into_iter().zip(new) {
            // Source editing is authoritative. If a projection differs, omit motion.
            if old.kind != new.kind || old.summary != new.summary || old.depth != new.depth {
                return None;
            }
            if let Some(position) = positions.get(&old.source_range.start) {
                motion.positions.insert(new.source_range.start, *position);
            }
            if let Some(height) = heights.get(&old.source_range.start) {
                motion.heights.insert(new.source_range.start, *height);
            }
        }
        Some(motion)
    }

    pub(in crate::app) fn for_insert(
        before: &EiyashouProjection,
        after: &EiyashouProjection,
        target: BlockDropTarget,
        positions: &HashMap<usize, f32>,
        heights: &HashMap<usize, f32>,
        gap_top: f32,
    ) -> Option<Self> {
        let old = before
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .collect::<Vec<_>>();
        let new = after
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .collect::<Vec<_>>();
        let added = new
            .len()
            .checked_sub(old.len())
            .filter(|added| *added > 0)?;
        let block = old
            .iter()
            .find(|block| block.source_range.start == target.row)?;
        let insertion = if target.after {
            block.source_range.end
        } else {
            block.source_range.start
        };
        let at = old.partition_point(|block| block.source_range.start < insertion);
        let mut motion = Self {
            positions: HashMap::new(),
            heights: HashMap::new(),
            started_at: None,
            revision: None,
        };
        for (index, old) in old.iter().enumerate() {
            let new = new[index + if index >= at { added } else { 0 }];
            if old.kind != new.kind || old.summary != new.summary || old.depth != new.depth {
                return None;
            }
            if let Some(position) = positions.get(&old.source_range.start) {
                motion.positions.insert(new.source_range.start, *position);
            }
            if let Some(height) = heights.get(&old.source_range.start) {
                motion.heights.insert(new.source_range.start, *height);
            }
        }
        for (index, block) in new[at..at + added].iter().enumerate() {
            motion
                .positions
                .insert(block.source_range.start, gap_top + index as f32 * 42.);
            motion.heights.insert(block.source_range.start, 38.);
        }
        Some(motion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_insertion_preserves_duplicate_row_origins() {
        let source = "scene start { wait(1s), wait(1s), wait(1s) }";
        let before = EiyashouProjection::parse(source);
        let blocks = &before.scenes[0].blocks;
        let target = BlockDropTarget {
            row: blocks[1].source_range.start,
            after: false,
        };
        let positions = blocks
            .iter()
            .enumerate()
            .map(|(i, block)| (block.source_range.start, [0., 84., 126.][i]))
            .collect();
        let (edited, _) = before
            .insert_block_before(source, target.row, "background(room)")
            .unwrap();
        let after = EiyashouProjection::parse(&edited);
        let motion = BlockReorderMotion::for_insert(
            &before,
            &after,
            target,
            &positions,
            &HashMap::new(),
            42.,
        )
        .unwrap();
        assert_eq!(
            after.scenes[0]
                .blocks
                .iter()
                .map(|block| motion.positions[&block.source_range.start])
                .collect::<Vec<_>>(),
            [0., 42., 84., 126.]
        );
    }

    #[test]
    fn duplicate_rows_keep_their_own_origins_after_a_multiselection_drop() {
        let source = "scene start { wait(1s), wait(1s), wait(2s), wait(3s) }";
        let before = EiyashouProjection::parse(source);
        let blocks = &before.scenes[0].blocks;
        let selected = HashSet::from([blocks[0].source_range.start, blocks[2].source_range.start]);
        let target = blocks[3].source_range.start;
        let positions = blocks
            .iter()
            .enumerate()
            .map(|(i, block)| (block.source_range.start, i as f32 * 42.))
            .collect();
        let edited = before
            .move_blocks_to(source, &selected, target, true)
            .unwrap();
        let after = EiyashouProjection::parse(&edited);
        let motion = BlockReorderMotion::for_drop(
            &before,
            &after,
            &selected,
            target,
            true,
            &positions,
            &HashMap::new(),
        )
        .unwrap();
        let origins = after.scenes[0]
            .blocks
            .iter()
            .map(|block| motion.positions[&block.source_range.start])
            .collect::<Vec<_>>();
        assert_eq!(origins, [42., 126., 0., 84.]);
    }

    #[test]
    fn moving_a_structure_carries_its_descendants_and_measured_heights() {
        let source = "scene start { loop { \"one\", break }, wait(2s), wait(3s) }";
        let before = EiyashouProjection::parse(source);
        let blocks = &before.scenes[0].blocks;
        let selected = HashSet::from([blocks[0].source_range.start]);
        let target = blocks.last().unwrap().source_range.start;
        let positions = blocks
            .iter()
            .enumerate()
            .map(|(i, block)| (block.source_range.start, i as f32 * 42.))
            .collect();
        let heights = HashMap::from([(blocks[1].source_range.start, 78.)]);
        let edited = before
            .move_blocks_to(source, &selected, target, true)
            .unwrap();
        let after = EiyashouProjection::parse(&edited);
        let motion = BlockReorderMotion::for_drop(
            &before, &after, &selected, target, true, &positions, &heights,
        )
        .unwrap();
        let rows = &after.scenes[0].blocks;
        let origins = rows
            .iter()
            .map(|block| motion.positions[&block.source_range.start])
            .collect::<Vec<_>>();
        assert_eq!(origins, [126., 168., 0., 42., 84.]);
        assert_eq!(rows[4].kind, crate::projection::BlockKind::Control);
        assert_eq!(rows[4].depth, 1);
        assert_eq!(motion.heights[&rows[3].source_range.start], 78.);
    }
}
