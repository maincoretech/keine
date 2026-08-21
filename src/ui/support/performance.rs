use bevy::app::AppExit;
use bevy::diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::window::{PrimaryWindow, WindowCloseRequested};
use bevy::winit::WinitSettings;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use crate::runtime::GameSystemSet;
use crate::runtime::resources::AssetLoadingGate;
use crate::ui::title::TitleRoot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BenchmarkTarget {
    Cursor(usize),
    Timeline(String),
}

#[derive(Resource)]
pub(crate) struct RuntimeCaptureConfig {
    warmup_seconds: f32,
    sample_seconds: f32,
    pub(crate) target: Option<BenchmarkTarget>,
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
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::SceneUi => "scene-ui",
            Self::SceneDialog => "scene-dialog",
            Self::SceneOnly => "scene",
        }
    }

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

#[derive(SystemParam)]
struct BenchmarkExit<'w, 's> {
    window: Query<'w, 's, Entity, With<PrimaryWindow>>,
    close_requests: MessageWriter<'w, WindowCloseRequested>,
    exits: MessageWriter<'w, AppExit>,
}

impl BenchmarkExit<'_, '_> {
    fn request(&mut self) {
        if let Ok(window) = self.window.single() {
            self.close_requests.write(WindowCloseRequested { window });
        } else {
            log::warn!("benchmark primary window unavailable; exiting directly");
            self.exits.write(AppExit::Success);
        }
    }
}

#[derive(Default)]
struct RenderSampleData {
    first_frame: Option<Instant>,
    previous_frame: Option<Instant>,
    frame_ms: Vec<(f32, f64)>,
}

#[derive(Resource, Clone, Default)]
struct RenderCaptureSamples(Arc<Mutex<RenderSampleData>>);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct StartupSample {
    pub(crate) project_ms: f64,
    pub(crate) app_ms: f64,
    pub(crate) first_frame_ms: f64,
    pub(crate) interactive_ms: f64,
    pub(crate) peak_rss_mib: Option<f64>,
}

struct StartupCaptureData {
    process_started: Instant,
    project_opened: Instant,
    app_built: Option<Instant>,
    first_frame: Option<Instant>,
    latest_frame: Option<Instant>,
    ready_after: Option<Instant>,
    finished: bool,
}

#[derive(Resource, Clone)]
pub(crate) struct StartupCapture(Arc<Mutex<StartupCaptureData>>);

impl StartupCapture {
    pub(crate) fn new(process_started: Instant, project_opened: Instant) -> Self {
        Self(Arc::new(Mutex::new(StartupCaptureData {
            process_started,
            project_opened,
            app_built: None,
            first_frame: None,
            latest_frame: None,
            ready_after: None,
            finished: false,
        })))
    }

    pub(crate) fn mark_app_built(&self) {
        self.0
            .lock()
            .expect("startup capture lock poisoned")
            .app_built = Some(Instant::now());
    }
}

pub(crate) fn install_startup_capture(app: &mut App, capture: StartupCapture) {
    app.insert_resource(WinitSettings::continuous())
        .insert_resource(capture.clone())
        .add_systems(Update, capture_startup_performance.after(GameSystemSet::Ui));
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.insert_resource(capture).add_systems(
            Render,
            capture_startup_frame.in_set(RenderSystems::PostCleanup),
        );
    }
}

fn capture_startup_frame(capture: Res<StartupCapture>) {
    let now = Instant::now();
    let mut capture = capture.0.lock().expect("startup capture lock poisoned");
    capture.first_frame.get_or_insert(now);
    capture.latest_frame = Some(now);
}

fn capture_startup_performance(
    capture: Res<StartupCapture>,
    gate: Res<AssetLoadingGate>,
    titles: Query<(), With<TitleRoot>>,
    mut exit: BenchmarkExit,
) {
    let now = Instant::now();
    let mut capture = capture.0.lock().expect("startup capture lock poisoned");
    if capture.finished {
        return;
    }
    if capture.ready_after.is_none() {
        if !gate.blocked && !titles.is_empty() {
            capture.ready_after = Some(now);
        }
        return;
    }
    let ready_after = capture.ready_after.expect("ready timestamp was checked");
    let Some(interactive) = capture.latest_frame.filter(|frame| *frame > ready_after) else {
        return;
    };
    let sample = StartupSample::from_capture(&capture, interactive);
    capture.finished = true;
    drop(capture);
    eprintln!("{}", sample.machine_line());
    exit.request();
}

impl StartupSample {
    fn from_capture(capture: &StartupCaptureData, interactive: Instant) -> Self {
        let started = capture.process_started;
        let elapsed_ms = |instant: Instant| instant.duration_since(started).as_secs_f64() * 1_000.0;
        Self {
            project_ms: elapsed_ms(capture.project_opened),
            app_ms: elapsed_ms(capture.app_built.expect("app build must be recorded")),
            first_frame_ms: elapsed_ms(capture.first_frame.expect("first frame must be recorded")),
            interactive_ms: elapsed_ms(interactive),
            peak_rss_mib: peak_rss_bytes().map(|bytes| bytes as f64 / 1_048_576.0),
        }
    }

    pub(crate) fn machine_line(self) -> String {
        format!(
            "KEINE_STARTUP_SAMPLE project_ms={:.3} app_ms={:.3} first_frame_ms={:.3} interactive_ms={:.3} peak_rss_mib={:.3}",
            self.project_ms,
            self.app_ms,
            self.first_frame_ms,
            self.interactive_ms,
            self.peak_rss_mib.unwrap_or_default(),
        )
    }

    pub(crate) fn parse(output: &str) -> Option<Self> {
        let line = output
            .lines()
            .find(|line| line.starts_with("KEINE_STARTUP_SAMPLE "))?;
        let mut values = [None; 5];
        for field in line.split_whitespace().skip(1) {
            let (key, value) = field.split_once('=')?;
            let value = value.parse::<f64>().ok()?;
            match key {
                "project_ms" => values[0] = Some(value),
                "app_ms" => values[1] = Some(value),
                "first_frame_ms" => values[2] = Some(value),
                "interactive_ms" => values[3] = Some(value),
                "peak_rss_mib" => values[4] = Some(value),
                _ => return None,
            }
        }
        Some(Self {
            project_ms: values[0]?,
            app_ms: values[1]?,
            first_frame_ms: values[2]?,
            interactive_ms: values[3]?,
            peak_rss_mib: values[4].filter(|value| *value > 0.0),
        })
    }
}

#[cfg(all(feature = "startup-metrics", unix))]
fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: `getrusage` initializes the provided `rusage` when it returns 0.
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    // SAFETY: A successful `getrusage` call initialized the complete value.
    let usage = unsafe { usage.assume_init() };
    #[cfg(target_os = "macos")]
    return u64::try_from(usage.ru_maxrss).ok();
    #[cfg(not(target_os = "macos"))]
    u64::try_from(usage.ru_maxrss)
        .ok()
        .and_then(|kib| kib.checked_mul(1_024))
}

#[cfg(all(feature = "startup-metrics", windows))]
fn peak_rss_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: The pseudo-handle is valid for this process and `counters`
    // points to a correctly sized writable structure for the duration of the call.
    let result = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    (result != 0).then_some(counters.PeakWorkingSetSize as u64)
}

#[cfg(not(any(
    all(feature = "startup-metrics", unix),
    all(feature = "startup-metrics", windows)
)))]
fn peak_rss_bytes() -> Option<u64> {
    None
}

pub(crate) fn install_runtime_capture(
    app: &mut App,
    sample_seconds: f32,
    target: Option<BenchmarkTarget>,
    cameras: BenchmarkCameras,
) {
    let render_samples = RenderCaptureSamples::default();
    app.insert_resource(RuntimeCaptureConfig {
        warmup_seconds: 3.0,
        sample_seconds,
        target,
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
    mut exit: BenchmarkExit,
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
        target: "keine::performance",
        "CAPTURE  | {:.1}s · {frames} frames · {:.1} FPS avg · {:.1} FPS 1% low",
        config.sample_seconds,
        if average > 0.0 { 1_000.0 / average } else { 0.0 },
        if p99 > 0.0 { 1_000.0 / p99 } else { 0.0 },
    );
    log::info!(
        target: "keine::performance",
        "FRAME    | avg {average:.2} ms · p50 {p50:.2} · p95 {p95:.2} · p99 {p99:.2} · max {maximum:.2}",
    );
    log::info!(
        target: "keine::performance",
        "SCENE    | {entities:.0} entities · cameras {:?} · 3.0s warm-up excluded",
        config.cameras,
    );
    let image_bytes = images
        .iter()
        .filter_map(|(_, image)| image.data.as_ref().map(Vec::len))
        .sum::<usize>();
    let font_bytes = fonts.iter().map(|(_, font)| font.data.len()).sum::<usize>();
    log::info!(
        target: "keine::performance",
        "ASSETS   | {} images / {:.1} MiB CPU pixels · {} fonts / {:.1} MiB source data",
        images.len(),
        image_bytes as f64 / 1_048_576.0,
        fonts.len(),
        font_bytes as f64 / 1_048_576.0,
    );
    if let Some(bytes) = peak_rss_bytes() {
        log::info!(
            target: "keine::performance",
            "MEMORY   | peak RSS {:.1} MiB",
            bytes as f64 / 1_048_576.0,
        );
    }
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
            target: "keine::performance",
            "RENDER   | {path} {value:.3}{suffix}",
        );
    }
    state.finished = true;
    exit.request();
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish_capture(mut exit: BenchmarkExit) {
        exit.request();
    }

    #[test]
    fn benchmark_exit_uses_the_native_window_close_pipeline() {
        let mut app = App::new();
        app.add_message::<WindowCloseRequested>()
            .add_message::<AppExit>()
            .add_systems(Update, finish_capture);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();

        app.update();

        let requests = app
            .world_mut()
            .resource_mut::<Messages<WindowCloseRequested>>()
            .drain()
            .map(|request| request.window)
            .collect::<Vec<_>>();
        assert_eq!(requests, [window]);
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
    }

    #[test]
    fn startup_sample_line_round_trips_with_and_without_memory_metrics() {
        let sample = StartupSample {
            project_ms: 1.25,
            app_ms: 132.5,
            first_frame_ms: 263.75,
            interactive_ms: 267.0,
            peak_rss_mib: Some(205.5),
        };
        assert_eq!(StartupSample::parse(&sample.machine_line()), Some(sample));

        let without_memory = "KEINE_STARTUP_SAMPLE project_ms=1 app_ms=2 first_frame_ms=3 interactive_ms=4 peak_rss_mib=0";
        assert_eq!(
            StartupSample::parse(without_memory)
                .expect("valid startup sample")
                .peak_rss_mib,
            None
        );
    }
}
