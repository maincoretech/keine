// Isolate stable Block preparation; excludes GPUI layout and drawing.
use super::*;
#[test]
#[ignore = "release microbenchmark; run explicitly"]
fn block_preparation() {
    for count in [1000, 5000] {
        let source = format!("scene demo {{\n{}\n}}", "  wait(10ms),\n".repeat(count));
        let (document, root) = document(&source);
        let mut cache = Cache::default();
        let snapshot = cache.snapshot(&document, None);
        let collapsed = HashSet::new();
        let heights = HashMap::new();
        let geometry = cache.geometry(&snapshot, &collapsed, &heights);
        let mut before = Vec::new();
        let mut after = Vec::new();
        for _ in 0..11 {
            let now = std::time::Instant::now();
            for _ in 0..100 {
                // Original render's source, line index, order, endings, row positions and marks.
                let source = document.borrow().contents().to_owned();
                let lines = keine_loader::SourceLineIndex::new(&source);
                let order = snapshot
                    .projection
                    .scenes
                    .iter()
                    .flat_map(|scene| &scene.blocks)
                    .filter(|block| !block.is_textbox_ending())
                    .map(|block| block.source_range.start)
                    .collect::<Vec<_>>();
                let endings = snapshot
                    .projection
                    .scenes
                    .iter()
                    .flat_map(|scene| &scene.blocks)
                    .filter(|block| block.is_textbox_ending())
                    .filter_map(|block| block.lifetime_owner)
                    .collect::<HashSet<_>>();
                let mut positions = HashMap::new();
                let mut marks = Vec::new();
                let mut top = 76.;
                for block in snapshot
                    .projection
                    .scenes
                    .iter()
                    .flat_map(|scene| &scene.blocks)
                {
                    positions.insert(block.source_range.start, top);
                    marks.push(minimap::Mark::block(block, top, 38., false));
                    top += 42.;
                }
                std::hint::black_box((source, lines, order, endings, positions, marks));
            }
            before.push(now.elapsed().as_secs_f64() * 10.);
            let now = std::time::Instant::now();
            for _ in 0..100 {
                let snapshot = cache.snapshot(&document, None);
                let layout = cache.geometry(&snapshot, &collapsed, &heights);
                let first = layout
                    .rows
                    .partition_point(|(_, top, height)| *top + *height < 20000.);
                let visible = layout.rows[first..]
                    .iter()
                    .take_while(|(_, top, _)| *top < 20800.)
                    .map(|row| row.0)
                    .collect::<Vec<_>>();
                let marks = cache.marks(&snapshot, &geometry, &HashSet::new(), None);
                std::hint::black_box((layout, visible, marks));
            }
            after.push(now.elapsed().as_secs_f64() * 10.);
        }
        before.sort_by(f64::total_cmp);
        after.sort_by(f64::total_cmp);
        println!(
            "blocks={count} preparation before={:.6}ms cached={:.6}ms",
            before[5], after[5]
        );
        fs::remove_dir_all(root).unwrap();
    }
}
