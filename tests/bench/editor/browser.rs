//! Isolate GPUI overview replay cost; this is not a whole-window FPS test.
use gpui_kit::{Bounds, ContentMask, Quad, ScaledPixels, Scene, point, size};
use std::time::Instant;
fn overview(strokes: usize, layer: bool) -> f64 {
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(72.), ScaledPixels(660.)),
    );
    let mut source = Scene::default();
    if layer {
        source.push_layer(bounds);
    }
    for index in 0..strokes {
        let x = (index % 12) as f32 * 4.;
        let y = (index / 12) as f32 * (650. / (strokes / 12) as f32);
        source.insert_primitive(Quad {
            bounds: Bounds::new(
                point(ScaledPixels(x), ScaledPixels(y)),
                size(ScaledPixels(3.), ScaledPixels(1.)),
            ),
            content_mask: ContentMask { bounds },
            ..Default::default()
        });
    }
    if layer {
        source.pop_layer();
    }
    let mut scene = Scene::default();
    let start = Instant::now();
    for _ in 0..30 {
        scene.clear();
        scene.replay(0..source.len(), &source);
        scene.finish();
        std::hint::black_box(&scene);
    }
    start.elapsed().as_secs_f64() * 1000. / 30.
}
fn main() {
    for strokes in [1200, 4800, 19200] {
        let mut before = Vec::new();
        let mut after = Vec::new();
        for _ in 0..5 {
            before.push(overview(strokes, false));
            after.push(overview(strokes, true));
        }
        before.sort_by(f64::total_cmp);
        after.sort_by(f64::total_cmp);
        println!(
            "strokes={strokes} replay median: before={:.3}ms after={:.3}ms",
            before[2], after[2]
        );
    }
}
