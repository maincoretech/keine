use bevy::app::AppExit;
use bevy::diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin};
use bevy::ecs::system::{NonSendMarker, SystemParam};
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResourcePlugin;
use bevy::render::renderer::{RenderAdapterInfo, RenderDevice};
use bevy::render::settings::WgpuFeatures;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::window::{PrimaryWindow, WindowCloseRequested};
use bevy::winit::{WINIT_WINDOWS, WinitSettings};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use crate::render::blur::{DialogCamera, SceneBlurCamera, UiBlurCamera};
use crate::runtime::GameSystemSet;
use crate::runtime::resources::AssetLoadingGate;
use crate::ui::title::TitleRoot;

mod frame;
pub(crate) use frame::TRACE_FIELDS;
#[cfg(any(feature = "publisher", feature = "startup-metrics"))]
pub(crate) mod source_map;
use frame::{BudgetSummary, CapturedFrame, FrameContext};
const MAX_CAPTURE_FRAMES: usize = 240_000;

#[derive(Resource)]
pub(crate) struct InvalidCaptureTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BenchmarkTarget {
    Cursor(usize),
    SceneCursor(String, usize),
    Timeline(String),
}

#[derive(Resource)]
pub(crate) struct RuntimeCaptureConfig {
    warmup_seconds: f32,
    sample_seconds: f32,
    machine_output: bool,
    pub(crate) target: Option<BenchmarkTarget>,
    pub(crate) cameras: BenchmarkCameras,
    pub(crate) continuous: bool,
    refresh_hz: Option<f64>,
    samples: RenderCaptureSamples,
}

impl RuntimeCaptureConfig {
    /// Wake only at warm-up/sample boundaries, retaining normal idle sleep.
    pub(crate) fn next_wake(&self, idle: bool) -> std::time::Duration {
        let mut samples = self.samples.0.lock().expect("render capture lock poisoned");
        if !idle {
            samples.idle_wake = None;
            return std::time::Duration::from_secs(60);
        }
        let Some(first) = samples.first_frame else {
            return std::time::Duration::from_secs(60);
        };
        let elapsed = first.elapsed().as_secs_f32();
        let boundary = if samples.sample_started {
            self.warmup_seconds + self.sample_seconds
        } else {
            self.warmup_seconds
        };
        let wait = std::time::Duration::from_secs_f32((boundary - elapsed).max(0.001));
        // Changing Reactive.wait forces another redraw in Bevy. Hold the wait
        // stable; only shorten it after meaningful progress. Winit starts its
        // timer before app.update(), so its boundary wake can arrive slightly
        // early. Reusing the entire old wait then would double the sample.
        if let Some((phase, scheduled)) = samples.idle_wake
            && phase == samples.sample_started
            && wait >= scheduled / 2
        {
            return scheduled;
        }
        samples.idle_wake = Some((samples.sample_started, wait));
        wait
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BenchmarkCameras {
    #[default]
    Runtime,
    SceneUi,
    SceneDialog,
    SceneOnly,
}

impl BenchmarkCameras {
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::SceneUi => "scene-ui",
            Self::SceneDialog => "scene-dialog",
            Self::SceneOnly => "scene",
        }
    }

    /// Whether a benchmark profile deliberately pins the dialog camera.
    ///
    /// The runtime profile must retain production sleep/wake behavior; the
    /// decomposition profiles instead hold an explicit camera set so their
    /// costs remain attributable.
    pub(crate) const fn pins_dialog_activity(self) -> bool {
        !matches!(self, Self::Runtime)
    }

    pub(crate) const fn scene(self) -> bool {
        true
    }

    pub(crate) const fn ui(self) -> bool {
        matches!(self, Self::Runtime | Self::SceneUi)
    }

    pub(crate) const fn dialog(self) -> bool {
        matches!(self, Self::Runtime | Self::SceneDialog)
    }
}

#[derive(Resource, Default)]
struct RuntimeCaptureState {
    finished: bool,
    cpu_baseline: Option<(Instant, f64)>,
    render_timings: std::collections::HashMap<String, PassSamples>,
}

#[derive(Default)]
struct PassSamples {
    last: Option<Instant>,
    suffix: String,
    values: Vec<f64>,
}

impl PassSamples {
    fn record(&mut self, time: Instant, value: f64) {
        if self.last == Some(time) || !value.is_finite() || value < 0.0 {
            return;
        }
        self.last = Some(time);
        if self.values.len() < MAX_CAPTURE_FRAMES {
            self.values.push(value);
        }
    }
}

type CaptureCameraQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Camera,
        Option<&'static SceneBlurCamera>,
        Option<&'static UiBlurCamera>,
        Option<&'static DialogCamera>,
    ),
>;

#[derive(SystemParam)]
struct RuntimeCaptureDiagnostics<'w, 's> {
    diagnostics: Res<'w, DiagnosticsStore>,
    render_device: Res<'w, RenderDevice>,
    images: Res<'w, Assets<Image>>,
    fonts: Res<'w, Assets<Font>>,
    cameras: CaptureCameraQuery<'w, 's>,
    windows: Query<'w, 's, (Entity, &'static Window), With<PrimaryWindow>>,
    _main_thread: NonSendMarker,
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
    sample_started: bool,
    idle_wake: Option<(bool, std::time::Duration)>,
    previous_frame: Option<Instant>,
    frame_ms: Vec<CapturedFrame>,
    previous_context: Option<FrameContext>,
    continuous: bool,
    omitted: usize,
    adapter: Option<String>,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderSummary {
    pub(crate) frames: usize,
    pub(crate) average_ms: f64,
    pub(crate) p50_ms: f64,
    pub(crate) p95_ms: f64,
    pub(crate) p99_ms: f64,
    pub(crate) maximum_ms: f64,
    pub(crate) average_fps: f64,
    pub(crate) one_percent_low_fps: f64,
    pub(crate) p99_equivalent_fps: f64,
}

impl RenderSummary {
    fn from_sorted_frame_ms(frame_ms: &[f64]) -> Self {
        let frames = frame_ms.len();
        let average_ms = frame_ms.iter().sum::<f64>() / frames.max(1) as f64;
        let reciprocal = |frame_ms: f64| {
            if frame_ms > 0.0 {
                1_000.0 / frame_ms
            } else {
                0.0
            }
        };
        let p99_ms = percentile(frame_ms, 0.99);
        Self {
            frames,
            average_ms,
            p50_ms: percentile(frame_ms, 0.50),
            p95_ms: percentile(frame_ms, 0.95),
            p99_ms,
            maximum_ms: frame_ms.last().copied().unwrap_or_default(),
            average_fps: reciprocal(average_ms),
            one_percent_low_fps: slowest_percent_average_fps(frame_ms, 0.01),
            p99_equivalent_fps: reciprocal(p99_ms),
        }
    }

    pub(crate) fn machine_line(self) -> String {
        format!(
            "KEINE_RENDER_SAMPLE frames={} average_ms={} p50_ms={} p95_ms={} p99_ms={} maximum_ms={} average_fps={} one_percent_low_fps={} p99_equivalent_fps={}",
            self.frames,
            self.average_ms,
            self.p50_ms,
            self.p95_ms,
            self.p99_ms,
            self.maximum_ms,
            self.average_fps,
            self.one_percent_low_fps,
            self.p99_equivalent_fps,
        )
    }

    pub(crate) fn parse(output: &str) -> Option<Self> {
        let line = output
            .lines()
            .find(|line| line.starts_with("KEINE_RENDER_SAMPLE "))?;
        let mut values = [None; 9];
        for field in line.split_whitespace().skip(1) {
            let (key, value) = field.split_once('=')?;
            let slot = match key {
                "frames" => 0,
                "average_ms" => 1,
                "p50_ms" => 2,
                "p95_ms" => 3,
                "p99_ms" => 4,
                "maximum_ms" => 5,
                "average_fps" => 6,
                "one_percent_low_fps" => 7,
                "p99_equivalent_fps" => 8,
                _ => return None,
            };
            values[slot] = Some(value.parse::<f64>().ok()?);
        }
        Some(Self {
            frames: values[0]? as usize,
            average_ms: values[1]?,
            p50_ms: values[2]?,
            p95_ms: values[3]?,
            p99_ms: values[4]?,
            maximum_ms: values[5]?,
            average_fps: values[6]?,
            one_percent_low_fps: values[7]?,
            p99_equivalent_fps: values[8]?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameSample {
    pub(crate) elapsed_seconds: f32,
    pub(crate) frame_ms: f64,
}

impl FrameSample {
    fn machine_line(self) -> String {
        format!(
            "KEINE_FRAME_SAMPLE elapsed_seconds={} frame_ms={}",
            self.elapsed_seconds, self.frame_ms,
        )
    }

    #[cfg(test)]
    pub(crate) fn parse(line: &str) -> Option<Self> {
        let line = line.strip_prefix("KEINE_FRAME_SAMPLE ")?;
        let mut elapsed_seconds = None;
        let mut frame_ms = None;
        for field in line.split_whitespace() {
            let (key, value) = field.split_once('=')?;
            match key {
                "elapsed_seconds" => elapsed_seconds = value.parse().ok(),
                "frame_ms" => frame_ms = value.parse().ok(),
                _ => return None,
            }
        }
        Some(Self {
            elapsed_seconds: elapsed_seconds?,
            frame_ms: frame_ms?,
        })
    }
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

// OS contracts: getrusage(2), GetProcessTimes; aggregate all process threads.
#[cfg(all(feature = "startup-metrics", unix))]
fn process_cpu_seconds() -> Option<f64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage writes this ABI structure on a successful return.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: the successful call initialized the complete structure.
    let usage = unsafe { usage.assume_init() };
    Some(
        (usage.ru_utime.tv_sec + usage.ru_stime.tv_sec) as f64
            + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 / 1_000_000.0,
    )
}

#[cfg(all(feature = "startup-metrics", windows))]
fn process_cpu_seconds() -> Option<f64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: this process handle and all writable FILETIME buffers are valid.
    if unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return None;
    }
    let ticks =
        |t: FILETIME| u128::from((u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime));
    Some((ticks(kernel) + ticks(user)) as f64 / 10_000_000.0)
}

#[cfg(not(any(
    all(feature = "startup-metrics", unix),
    all(feature = "startup-metrics", windows)
)))]
fn process_cpu_seconds() -> Option<f64> {
    None
}

pub(crate) fn install_runtime_capture(
    app: &mut App,
    sample_seconds: f32,
    target: Option<BenchmarkTarget>,
    cameras: BenchmarkCameras,
    continuous: bool,
    refresh_hz: Option<f64>,
    raw: bool,
) {
    log::info!(target: "keine::performance", "build identity · commit {} · built {} · features {}",
        env!("KEINE_BUILD_COMMIT"), env!("KEINE_BUILD_TIME"), env!("KEINE_BUILD_FEATURES"));
    let render_samples = RenderCaptureSamples::default();
    render_samples
        .0
        .lock()
        .expect("render capture lock poisoned")
        .continuous = continuous;
    app.insert_resource(RuntimeCaptureConfig {
        warmup_seconds: 3.0,
        sample_seconds,
        machine_output: raw
            || std::env::var_os(crate::runtime::bootstrap::RUNTIME_BENCHMARK_CHILD_ENV).is_some(),
        target,
        cameras,
        continuous,
        refresh_hz,
        samples: render_samples.clone(),
    })
    .insert_resource(render_samples.clone())
    .init_resource::<FrameContext>()
    .add_plugins(ExtractResourcePlugin::<FrameContext>::default())
    .add_systems(First, frame::begin_update)
    .add_systems(
        Last,
        frame::attribute_frame.after(crate::runtime::platform::update_lifecycle),
    )
    .init_resource::<RuntimeCaptureState>()
    .add_systems(Update, capture_runtime_performance);
    if continuous {
        app.insert_resource(WinitSettings::continuous());
    }
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.insert_resource(render_samples).add_systems(
            Render,
            capture_render_frame.in_set(RenderSystems::PostCleanup),
        );
    }
}

fn capture_render_frame(
    samples: Res<RenderCaptureSamples>,
    adapter: Res<RenderAdapterInfo>,
    context: Res<FrameContext>,
) {
    let now = Instant::now();
    let mut samples = samples.0.lock().expect("render capture lock poisoned");
    samples.adapter.get_or_insert_with(|| {
        format!(
            "{} · {:?} · driver {} {}",
            adapter.name, adapter.backend, adapter.driver, adapter.driver_info,
        )
    });
    let first_frame = *samples.first_frame.get_or_insert(now);
    if let (Some(previous), Some(previous_context)) =
        (samples.previous_frame, &samples.previous_context)
    {
        let frame = CapturedFrame {
            elapsed_seconds: now.duration_since(first_frame).as_secs_f32(),
            frame_ms: now.duration_since(previous).as_secs_f64() * 1000.0,
            latency_ms: now
                .saturating_duration_since(context.update_started)
                .as_secs_f64()
                * 1000.0,
            exclusion: CapturedFrame::exclusion(previous_context, &context, samples.continuous),
            context: context.clone(),
        };
        if samples.frame_ms.len() < MAX_CAPTURE_FRAMES {
            samples.frame_ms.push(frame);
        } else {
            samples.omitted += 1;
        }
    }
    samples.previous_frame = Some(now);
    samples.previous_context = Some(context.clone());
}

fn capture_runtime_performance(
    config: Res<RuntimeCaptureConfig>,
    samples: Res<RenderCaptureSamples>,
    capture: RuntimeCaptureDiagnostics,
    mut state: ResMut<RuntimeCaptureState>,
    mut exit: BenchmarkExit,
    invalid_target: Option<Res<InvalidCaptureTarget>>,
) {
    if invalid_target.is_some() && !state.finished {
        state.finished = true;
        exit.exits.write(AppExit::error());
        return;
    }
    if state.finished {
        return;
    }
    let now = Instant::now();
    let mut samples = samples.0.lock().expect("render capture lock poisoned");
    let Some(first_frame) = samples.first_frame else {
        return;
    };
    if now.duration_since(first_frame).as_secs_f32() >= config.warmup_seconds
        && !samples.sample_started
    {
        samples.sample_started = true;
        state.cpu_baseline = process_cpu_seconds().map(|cpu| (now, cpu));
    }
    if now.duration_since(first_frame).as_secs_f32() >= config.warmup_seconds {
        // Bevy's recorder requires both encoder and pass timestamp writes.
        // Encoder-only zero results cannot establish shader/pass cost.
        let gpu_pass_timing = capture.render_device.features().contains(
            WgpuFeatures::TIMESTAMP_QUERY
                | WgpuFeatures::TIMESTAMP_QUERY_INSIDE_ENCODERS
                | WgpuFeatures::TIMESTAMP_QUERY_INSIDE_PASSES,
        );
        for diagnostic in capture.diagnostics.iter() {
            let path = diagnostic.path().as_str();
            if !path.starts_with("render/")
                || !(path.ends_with("/elapsed_cpu") || path.ends_with("/elapsed_gpu"))
            {
                continue;
            }
            if path.ends_with("/elapsed_gpu") && !gpu_pass_timing {
                continue;
            }
            if let Some(measurement) = diagnostic.measurement() {
                // Do not import the last warm-up result into the sample window.
                if measurement
                    .time
                    .saturating_duration_since(first_frame)
                    .as_secs_f32()
                    < config.warmup_seconds
                {
                    continue;
                }
                if !state.render_timings.contains_key(path) {
                    state.render_timings.insert(
                        path.to_owned(),
                        PassSamples {
                            suffix: diagnostic.suffix.to_string(),
                            ..Default::default()
                        },
                    );
                }
                let pass = state.render_timings.get_mut(path).expect("pass installed");
                pass.record(measurement.time, measurement.value);
            }
        }
    }
    if now.duration_since(first_frame).as_secs_f32() < config.warmup_seconds + config.sample_seconds
    {
        return;
    }

    let mut sampled_frames = samples
        .frame_ms
        .iter()
        .filter(|frame| frame.elapsed_seconds >= config.warmup_seconds)
        .cloned()
        .collect::<Vec<_>>();
    for frame in &mut sampled_frames {
        frame.exclude_sample_boundary(
            config.warmup_seconds,
            config.warmup_seconds + config.sample_seconds,
        );
    }
    let adapter = samples.adapter.clone();
    let omitted = samples.omitted;
    drop(samples);
    let mut frame_ms = sampled_frames
        .iter()
        .filter(|frame| frame.exclusion == "none")
        .map(|frame| frame.frame_ms)
        .collect::<Vec<_>>();
    frame_ms.sort_by(f64::total_cmp);
    let summary = RenderSummary::from_sorted_frame_ms(&frame_ms);
    let entities = capture
        .diagnostics
        .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        .and_then(|value| value.smoothed())
        .unwrap_or_default();
    if summary.frames == 0 {
        log::info!(target: "keine::performance", "CAPTURE  | {:.1}s · no eligible active render intervals", config.sample_seconds);
        log::info!(target: "keine::performance", "FRAME    | FPS and frame-time statistics unavailable; inspect excluded intervals");
    } else {
        log::info!(
            target: "keine::performance",
            "CAPTURE  | {:.1}s · {} frames · {:.1} FPS avg · {:.1} FPS 1% low · {:.1} FPS p99-equivalent",
            config.sample_seconds,
            summary.frames,
            summary.average_fps,
            summary.one_percent_low_fps,
            summary.p99_equivalent_fps,
        );
        log::info!(
            target: "keine::performance",
            "FRAME    | avg {:.2} ms · p50 {:.2} · p95 {:.2} · p99 {:.2} · max {:.2}",
            summary.average_ms,
            summary.p50_ms,
            summary.p95_ms,
            summary.p99_ms,
            summary.maximum_ms,
        );
    }
    let budget = BudgetSummary::from_frames(&sampled_frames);
    log::info!(target: "keine::performance", "SAMPLING | mode {} · render submission intervals, not display presentation · retained {} · omitted {omitted}",
        if config.continuous { "continuous" } else { "runtime" }, sampled_frames.len());
    log::info!(target: "keine::performance", "BUDGET   | measured {} · unknown {} · over budget {} · estimated missed refresh slots {} · longest over-budget run {}",
        budget.measured, budget.unknown, budget.over_budget, budget.missed_slots, budget.longest_run);
    for reason in ["sleep", "unfocused", "display-change", "sample-boundary"] {
        log::info!(target: "keine::performance", "EXCLUDED | {reason}: {}", sampled_frames.iter().filter(|f| f.exclusion == reason).count());
    }
    if let Some((at, before)) = state.cpu_baseline
        && let Some(after) = process_cpu_seconds()
        && after >= before
        && now > at
    {
        log::info!(target: "keine::performance", "PROCESS  | {:.3} CPU seconds / {:.3} wall seconds · {:.2}% CPU (one core = 100%)",
            after - before, now.duration_since(at).as_secs_f64(), (after - before) / now.duration_since(at).as_secs_f64() * 100.0);
    } else {
        log::info!(target: "keine::performance", "PROCESS  | CPU counters unavailable");
    }
    let mut slow_frames = sampled_frames
        .iter()
        .filter(|f| {
            f.exclusion == "none"
                && f.context
                    .budget_ms
                    .is_some_and(|budget| f.frame_ms > budget)
        })
        .collect::<Vec<_>>();
    slow_frames.sort_by(|left, right| right.frame_ms.total_cmp(&left.frame_ms));
    for frame in slow_frames.into_iter().take(10) {
        log::info!(target: "keine::performance", "SLOW     | t={:.3}s · {:.2} ms · scene {:?} next-cursor {} · {}:{} · {:?}",
            frame.elapsed_seconds, frame.frame_ms, frame.context.scene, frame.context.cursor, frame.context.source, frame.context.line, frame.context.activity);
    }
    log::info!(
        target: "keine::performance",
        "SCENE    | {entities:.0} entities · profile {:?} · active cameras {} · 3.0s warm-up excluded",
        config.cameras,
        active_camera_label(&capture.cameras),
    );
    if let Some(adapter) = adapter {
        log::info!(target: "keine::performance", "GPUINFO  | {adapter}");
    }
    if let Ok((entity, window)) = capture.windows.single() {
        let refresh_hz = WINIT_WINDOWS.with_borrow(|windows| {
            windows
                .get_window(entity)
                .and_then(|window| window.current_monitor())
                .and_then(|monitor| monitor.refresh_rate_millihertz())
                .map(|millihertz| millihertz as f64 / 1_000.0)
        });
        log::info!(
            target: "keine::performance",
            "DISPLAY  | {}x{} physical · scale {:.2} · present {:?} · refresh {} · hidden {}",
            window.resolution.physical_width(),
            window.resolution.physical_height(),
            window.resolution.scale_factor(),
            window.present_mode,
            refresh_hz.map_or_else(|| "unknown".to_owned(), |hz| format!("{hz:.2} Hz")),
            !window.visible,
        );
    }
    let image_bytes = capture
        .images
        .iter()
        .filter_map(|(_, image)| image.data.as_ref().map(Vec::len))
        .sum::<usize>();
    let font_bytes = capture
        .fonts
        .iter()
        .map(|(_, font)| font.data.len())
        .sum::<usize>();
    log::info!(
        target: "keine::performance",
        "ASSETS   | {} images / {:.1} MiB CPU pixels · {} fonts / {:.1} MiB source data",
        capture.images.len(),
        image_bytes as f64 / 1_048_576.0,
        capture.fonts.len(),
        font_bytes as f64 / 1_048_576.0,
    );
    if let Some(bytes) = peak_rss_bytes() {
        log::info!(
            target: "keine::performance",
            "MEMORY   | peak RSS {:.1} MiB",
            bytes as f64 / 1_048_576.0,
        );
    }
    let mut render_passes = state
        .render_timings
        .iter_mut()
        .filter_map(|(path, pass)| {
            if pass.values.is_empty() {
                return None;
            }
            pass.values.sort_by(f64::total_cmp);
            let average = pass.values.iter().sum::<f64>() / pass.values.len() as f64;
            Some((
                path,
                average,
                percentile(&pass.values, 0.99),
                *pass.values.last().unwrap(),
                pass.values.len(),
                &pass.suffix,
            ))
        })
        .collect::<Vec<_>>();
    render_passes.sort_by(|left, right| right.1.total_cmp(&left.1));
    for kind in ["elapsed_cpu", "elapsed_gpu"] {
        let passes = render_passes
            .iter()
            .filter(|(path, ..)| path.ends_with(kind))
            .collect::<Vec<_>>();
        if kind == "elapsed_gpu" && passes.is_empty() {
            log::info!(target: "keine::performance", "GPU_TIME | no samples recorded (backend support/activity); unavailable is not zero");
        }
        for (path, average, p99, maximum, count, suffix) in passes.into_iter().take(8) {
            log::info!(target: "keine::performance", "RENDER   | {path} avg {average:.3}{suffix} · p99 {p99:.3}{suffix} · max {maximum:.3}{suffix} · {count} samples");
        }
    }
    if config.machine_output {
        eprintln!("{}", summary.machine_line());
        for frame in &sampled_frames {
            eprintln!("{}", frame.raw_line());
            if frame.exclusion == "none" {
                eprintln!(
                    "{}",
                    FrameSample {
                        elapsed_seconds: frame.elapsed_seconds,
                        frame_ms: frame.frame_ms
                    }
                    .machine_line()
                );
            }
        }
    }
    state.finished = true;
    exit.request();
}

fn active_camera_label(cameras: &CaptureCameraQuery) -> String {
    [
        (
            "scene",
            cameras
                .iter()
                .any(|(camera, marker, _, _)| camera.is_active && marker.is_some()),
        ),
        (
            "ui",
            cameras
                .iter()
                .any(|(camera, _, marker, _)| camera.is_active && marker.is_some()),
        ),
        (
            "dialog",
            cameras
                .iter()
                .any(|(camera, _, _, marker)| camera.is_active && marker.is_some()),
        ),
    ]
    .into_iter()
    .filter_map(|(name, active)| active.then_some(name))
    .collect::<Vec<_>>()
    .join("+")
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[index]
}

fn slowest_percent_average_fps(sorted_frame_ms: &[f64], fraction: f64) -> f64 {
    if sorted_frame_ms.is_empty() {
        return 0.0;
    }
    let count =
        ((sorted_frame_ms.len() as f64 * fraction).ceil() as usize).clamp(1, sorted_frame_ms.len());
    let mean_ms = sorted_frame_ms.iter().rev().take(count).sum::<f64>() / count as f64;
    1_000.0 / mean_ms.max(f64::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_warmup_does_not_skip_the_cpu_baseline_wake() {
        let samples = RenderCaptureSamples::default();
        samples.0.lock().unwrap().first_frame =
            Some(Instant::now() - std::time::Duration::from_secs(4));
        let config = RuntimeCaptureConfig {
            warmup_seconds: 3.0,
            sample_seconds: 10.0,
            machine_output: false,
            target: None,
            cameras: BenchmarkCameras::Runtime,
            continuous: false,
            refresh_hz: None,
            samples: samples.clone(),
        };
        assert!(config.next_wake(true) <= std::time::Duration::from_millis(1));
        samples.0.lock().unwrap().sample_started = true;
        let wake = config.next_wake(true);
        assert!(wake > std::time::Duration::from_secs(1));
        samples.0.lock().unwrap().first_frame =
            Some(Instant::now() - std::time::Duration::from_secs(5));
        assert_eq!(config.next_wake(true), wake);
        samples.0.lock().unwrap().first_frame =
            Some(Instant::now() - std::time::Duration::from_millis(12_998));
        assert!(config.next_wake(true) < std::time::Duration::from_millis(10));
        config.next_wake(false);
        assert!(config.next_wake(true) < wake);
    }

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
    fn only_decomposition_profiles_pin_dialog_camera_activity() {
        assert!(!BenchmarkCameras::Runtime.pins_dialog_activity());
        for cameras in [
            BenchmarkCameras::SceneUi,
            BenchmarkCameras::SceneDialog,
            BenchmarkCameras::SceneOnly,
        ] {
            assert!(cameras.pins_dialog_activity());
        }
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

    #[test]
    fn render_summary_distinguishes_one_percent_low_from_p99_equivalent_fps() {
        let mut frame_ms = vec![10.0; 99];
        frame_ms.push(100.0);
        let summary = RenderSummary::from_sorted_frame_ms(&frame_ms);

        assert_eq!(summary.one_percent_low_fps, 10.0);
        assert_eq!(summary.p99_equivalent_fps, 100.0);
        assert_eq!(RenderSummary::parse(&summary.machine_line()), Some(summary));
    }

    #[test]
    fn tail_fps_uses_mean_duration_and_render_passes_deduplicate_idle_polls() {
        assert_eq!(slowest_percent_average_fps(&[50.0, 150.0], 1.0), 10.0);
        let mut pass = PassSamples::default();
        let time = Instant::now();
        pass.record(time, 1.0);
        pass.record(time, 1.0);
        pass.record(time + std::time::Duration::from_millis(16), 3.0);
        pass.record(time + std::time::Duration::from_millis(32), f64::NAN);
        assert_eq!(pass.values, [1.0, 3.0]);
    }

    #[test]
    fn frame_sample_machine_line_round_trips() {
        let sample = FrameSample {
            elapsed_seconds: 3.25,
            frame_ms: 16.75,
        };
        assert_eq!(FrameSample::parse(&sample.machine_line()), Some(sample));
    }
}
