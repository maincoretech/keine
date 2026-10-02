// Compare the original owner function with the worker builder on GPUI's test platform.
use super::*;
fn before(text: Rope, width: Pixels, font: gpui_kit::Font, language: &str, cx: &mut App) -> Rows {
    let mut map = DisplayMap::new(font.clone(), px(13.), Some(width));
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
        source: source.into(),
        styles: Arc::new(styles),
        strokes,
        offsets,
    }
}

#[gpui_kit::test]
#[ignore = "release microbenchmark; run explicitly"]
fn text_rebuild(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        let font = gpui_kit::font("monospace");
        let cancelled = AtomicBool::new(false);
        for lines in [1000, 5000] {
            let source = format!(
                "scene demo {{\n{}\n}}",
                "  hero: \"对白[wait=1000]继续\",\n".repeat(lines)
            );
            let text = Rope::from(source.as_str());
            let mut old = Vec::new();
            let mut new = Vec::new();
            let mut resize = Vec::new();
            let previous = Arc::new(
                rows(
                    text.clone(),
                    px(300.),
                    font.clone(),
                    "shou",
                    cx.text_system().clone(),
                    None,
                    &cancelled,
                )
                .unwrap(),
            );
            for _ in 0..11 {
                let now = std::time::Instant::now();
                std::hint::black_box(before(text.clone(), px(300.), font.clone(), "shou", cx));
                old.push(now.elapsed().as_secs_f64() * 1000.);
                let now = std::time::Instant::now();
                std::hint::black_box(
                    rows(
                        text.clone(),
                        px(300.),
                        font.clone(),
                        "shou",
                        cx.text_system().clone(),
                        None,
                        &cancelled,
                    )
                    .unwrap(),
                );
                new.push(now.elapsed().as_secs_f64() * 1000.);
                let now = std::time::Instant::now();
                std::hint::black_box(
                    rows(
                        text.clone(),
                        px(320.),
                        font.clone(),
                        "shou",
                        cx.text_system().clone(),
                        Some(previous.clone()),
                        &cancelled,
                    )
                    .unwrap(),
                );
                resize.push(now.elapsed().as_secs_f64() * 1000.);
            }
            old.sort_by(f64::total_cmp);
            new.sort_by(f64::total_cmp);
            resize.sort_by(f64::total_cmp);
            println!(
                "text lines={lines} before-main={:.3}ms worker={:.3}ms resize-worker={:.3}ms",
                old[5], new[5], resize[5]
            );
        }
    });
}
