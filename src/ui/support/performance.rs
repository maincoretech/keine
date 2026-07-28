use bevy::app::AppExit;
use bevy::diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::winit::WinitSettings;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Resource)]
pub(crate) struct RuntimeCaptureConfig {
    warmup_seconds: f32,
    sample_seconds: f32,
    pub(crate) cursor: Option<usize>,
    pub(crate) cameras: BenchmarkCameras,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BenchmarkCameras {
    #[default]
    Full,
    SceneUi,
    SceneDialog,
    SceneOnly,
}

impl BenchmarkCameras {
    pub(crate) const fn scene(self) -> bool {
        true
    }

    pub(crate) const fn ui(self) -> bool {
        matches!(self, Self::Full | Self::SceneUi)
    }

    pub(crate) const fn dialog(self) -> bool {
        matches!(self, Self::Full | Self::SceneDialog)
    }
}

#[derive(Resource, Default)]
struct RuntimeCaptureState {
    finished: bool,
}

#[derive(Default)]
struct RenderSampleData {
    first_frame: Option<Instant>,
    previous_frame: Option<Instant>,
    frame_ms: Vec<(f32, f64)>,
}

#[derive(Resource, Clone, Default)]
struct RenderCaptureSamples(Arc<Mutex<RenderSampleData>>);

pub(crate) fn install_runtime_capture(
    app: &mut App,
    sample_seconds: f32,
    cursor: Option<usize>,
    cameras: BenchmarkCameras,
) {
    let render_samples = RenderCaptureSamples::default();
    app.insert_resource(RuntimeCaptureConfig {
        warmup_seconds: 3.0,
        sample_seconds,
        cursor,
        cameras,
    })
    // Captures commonly run behind a terminal or on a second display. Start
    // continuously before winit gets a chance to enter its unfocused wait so
    // the benchmark never depends on mouse or window events.
    .insert_resource(WinitSettings::continuous())
    .insert_resource(render_samples.clone())
    .init_resource::<RuntimeCaptureState>()
    .add_systems(Update, capture_runtime_performance);
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.insert_resource(render_samples).add_systems(
            Render,
            capture_render_frame.in_set(RenderSystems::PostCleanup),
        );
    }
}

fn capture_render_frame(samples: Res<RenderCaptureSamples>) {
    let now = Instant::now();
    let mut samples = samples.0.lock().expect("render capture lock poisoned");
    let first_frame = *samples.first_frame.get_or_insert(now);
    if let Some(previous) = samples.previous_frame {
        samples.frame_ms.push((
            now.duration_since(first_frame).as_secs_f32(),
            now.duration_since(previous).as_secs_f64() * 1_000.0,
        ));
    }
    samples.previous_frame = Some(now);
}

fn capture_runtime_performance(
    config: Res<RuntimeCaptureConfig>,
    samples: Res<RenderCaptureSamples>,
    diagnostics: Res<DiagnosticsStore>,
    images: Res<Assets<Image>>,
    fonts: Res<Assets<Font>>,
    mut state: ResMut<RuntimeCaptureState>,
    mut commands: Commands,
) {
    if state.finished {
        return;
    }
    let now = Instant::now();
    let samples = samples.0.lock().expect("render capture lock poisoned");
    let Some(first_frame) = samples.first_frame else {
        return;
    };
    if now.duration_since(first_frame).as_secs_f32() < config.warmup_seconds + config.sample_seconds
    {
        return;
    }

    let mut frame_ms = samples
        .frame_ms
        .iter()
        .filter_map(|(elapsed, frame_ms)| (*elapsed >= config.warmup_seconds).then_some(*frame_ms))
        .collect::<Vec<_>>();
    drop(samples);
    frame_ms.sort_by(f64::total_cmp);
    let frames = frame_ms.len();
    let average = frame_ms.iter().sum::<f64>() / frames.max(1) as f64;
    let p50 = percentile(&frame_ms, 0.50);
    let p95 = percentile(&frame_ms, 0.95);
    let p99 = percentile(&frame_ms, 0.99);
    let maximum = frame_ms.last().copied().unwrap_or_default();
    let entities = diagnostics
        .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        .and_then(|value| value.smoothed())
        .unwrap_or_default();
    log::info!(
        target: "crabgal::performance",
        "CAPTURE  | {:.1}s · {frames} frames · {:.1} FPS avg · {:.1} FPS 1% low",
        config.sample_seconds,
        if average > 0.0 { 1_000.0 / average } else { 0.0 },
        if p99 > 0.0 { 1_000.0 / p99 } else { 0.0 },
    );
    log::info!(
        target: "crabgal::performance",
        "FRAME    | avg {average:.2} ms · p50 {p50:.2} · p95 {p95:.2} · p99 {p99:.2} · max {maximum:.2}",
    );
    log::info!(
        target: "crabgal::performance",
        "SCENE    | {entities:.0} entities · cameras {:?} · 3.0s warm-up excluded",
        config.cameras,
    );
    let image_bytes = images
        .iter()
        .filter_map(|(_, image)| image.data.as_ref().map(Vec::len))
        .sum::<usize>();
    let font_bytes = fonts.iter().map(|(_, font)| font.data.len()).sum::<usize>();
    log::info!(
        target: "crabgal::performance",
        "ASSETS   | {} images / {:.1} MiB CPU pixels · {} fonts / {:.1} MiB source data",
        images.len(),
        image_bytes as f64 / 1_048_576.0,
        fonts.len(),
        font_bytes as f64 / 1_048_576.0,
    );
    let mut render_passes = diagnostics
        .iter()
        .filter_map(|diagnostic| {
            diagnostic
                .path()
                .as_str()
                .starts_with("render/")
                .then(|| {
                    diagnostic
                        .average()
                        .map(|value| (diagnostic.path().to_string(), value, &diagnostic.suffix))
                })
                .flatten()
        })
        .collect::<Vec<_>>();
    render_passes.sort_by(|left, right| right.1.total_cmp(&left.1));
    for (path, value, suffix) in render_passes.into_iter().take(8) {
        log::info!(
            target: "crabgal::performance",
            "RENDER   | {path} {value:.3}{suffix}",
        );
    }
    state.finished = true;
    commands.write_message(AppExit::Success);
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[index]
}
