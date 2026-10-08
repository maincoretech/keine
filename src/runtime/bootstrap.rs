use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(feature = "hot-reload")]
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use bevy::asset::io::AssetSourceId;
use bevy::asset::{AssetApp, AssetPlugin, RenderAssetUsages};
use bevy::camera::visibility::RenderLayers;
use bevy::diagnostic::EntityCountDiagnosticsPlugin;
use bevy::ecs::system::NonSendMarker;
use bevy::ecs::system::SystemParam;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy::winit::WINIT_WINDOWS;
use keine_core::config::GameConfig;
use keine_core::{Action, DESIGN_HEIGHT, DESIGN_WIDTH, Program, State};
#[cfg(feature = "hot-reload")]
use keine_loader::ScriptWatcher;
use keine_loader::{
    ContentProject, DiagnosticLevel, LoaderRegistry, SourceMount, load_project_with,
    load_scenes_with, load_startup_scenes_with,
};

use crate::render::blur::{BlurCamera, BlurPlugin, DialogCamera, SceneBlurCamera, UiBlurCamera};
use crate::render::camera_blur::{CameraEffectsPlugin, CompositedCameraEffects};
use crate::runtime::GamePlugin;
use crate::runtime::cli::{
    BenchmarkOptions, BenchmarkWindow, CliCommand, InteractiveMode, help_or_version,
    packaged_benchmark_command, parse as parse_cli, resolve_project_path,
};
use crate::runtime::resources::{
    ContentProjectResource, DevelopmentSession, EditorSyncSession, GameConfigResource, GameState,
    LocalAssetCache, LocalAssetManifest, LocalSceneAssets, PersistenceDisabled, PersistenceRoot,
    ProjectRoot, ScriptLanguages, StoreCodec,
};
#[cfg(feature = "hot-reload")]
use crate::runtime::resources::{HotReloadSession, ScriptWatcherResource};
use crate::ui::performance::{BenchmarkCameras, BenchmarkTarget};

pub(crate) const MAX_PROJECT_CONFIG_BYTES: usize = keine_loader::MAX_PROJECT_CONFIG_BYTES;
type BenchmarkWorkload = (&'static str, &'static str);
type BenchmarkSection = (&'static str, &'static [BenchmarkWorkload]);

const BASELINE_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] =
    &[("reference composition", "bench_baseline")];
const ISOLATED_EFFECT_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    ("effect bloom", "bench_effect_bloom"),
    ("effect depth", "bench_effect_depth"),
    ("effect blur", "bench_effect_blur"),
    ("effect distortion", "bench_effect_distortion"),
    ("effect vignette", "bench_effect_vignette"),
    ("effect color", "bench_effect_color"),
    ("effect tone", "bench_effect_tone"),
    ("effect old_film", "bench_effect_old_film"),
    ("effect shock", "bench_effect_shock"),
    ("effect godray", "bench_effect_godray"),
    ("effect lut", "bench_effect_lut"),
    ("effect chromatic", "bench_effect_chromatic"),
    ("effect pixelate", "bench_effect_pixelate"),
    ("effect glitch", "bench_effect_glitch"),
    ("effect crt", "bench_effect_crt"),
    ("effect sharpen", "bench_effect_sharpen"),
    ("effect radial_blur", "bench_effect_radial_blur"),
    ("effect motion_blur", "bench_effect_motion_blur"),
    ("effect zoom_blur", "bench_effect_zoom_blur"),
    ("effect light_leak", "bench_effect_light_leak"),
    ("effect lens_flare", "bench_effect_lens_flare"),
    ("effect grain", "bench_effect_grain"),
    ("effect heat_haze", "bench_effect_heat_haze"),
    ("effect water", "bench_effect_water"),
    ("effect fog", "bench_effect_fog"),
    ("effect vhs", "bench_effect_vhs"),
    ("effect halftone", "bench_effect_halftone"),
    ("effect dither", "bench_effect_dither"),
    ("effect outline", "bench_effect_outline"),
    ("effect eyelid", "bench_effect_eyelid"),
    ("effect shatter", "bench_effect_shatter"),
    ("effect speed_lines", "bench_effect_speed_lines"),
];
const PARTICLE_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    ("particle snow_80", "bench_particle_snow_80"),
    ("particle snow_256", "bench_particle_snow_256"),
    ("particle rain_256", "bench_particle_rain_256"),
    ("particle petals_128", "bench_particle_petals_128"),
    ("particle texture_256", "bench_particle_texture_256"),
    ("particle large_256", "bench_particle_large_256"),
    ("particle layers_768", "bench_particle_layers_768"),
];
const MEDIA_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    ("audio_loop", "bench_audio_loop"),
    ("audio_crossfade", "bench_audio_crossfade"),
    ("video_fullscreen", "bench_video_fullscreen"),
    ("video_mixed", "bench_video_mixed"),
];
const UI_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    ("text_layers", "bench_text_layers"),
    ("curtain", "bench_curtain"),
    ("sprite_filter", "bench_sprite_filter"),
    ("light_off", "bench_light_off"),
    ("choice", "bench_choice"),
    ("input", "bench_input"),
    ("settings", "bench_settings"),
    ("save panel", "bench_save"),
    ("load panel", "bench_load"),
    ("history", "bench_history"),
    ("gallery", "bench_gallery"),
];

const DAILY_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    (
        "representative dialogue · runtime composition",
        "bench_representative_dialogue",
    ),
    (
        "representative portrait motion · runtime composition",
        "bench_representative_portrait_motion",
    ),
    (
        "representative scene transition · runtime composition",
        "bench_representative_scene_transition",
    ),
];
const FEATURE_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    ("shared transforms", "bench_shared_transform_clock"),
    ("classic camera", "bench_classic_camera_properties"),
    ("optical effects", "bench_optical_effects"),
    ("blur family", "bench_blur_family"),
    ("atmosphere effects", "bench_atmosphere_effects"),
    ("retro and mask effects", "bench_retro_and_eyelid_mask"),
    ("timed event types", "bench_all_event_types"),
    ("playback controls", "bench_playback_options"),
];
const CLASSIC_ATTRIBUTION_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    (
        "classic sampling · depth blur and shock",
        "bench_classic_sampling",
    ),
    ("classic godray math", "bench_classic_godray"),
    ("classic film noise", "bench_classic_film_noise"),
    (
        "classic color and lens math",
        "bench_classic_color_and_lens",
    ),
];
const STRESS_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] =
    &[("stress composition", "bench_stress_composition")];
const REPEATED_BENCHMARK_TARGETS: &[&str] = &[
    "bench_classic_camera_properties",
    "bench_optical_effects",
    "bench_blur_family",
    "bench_atmosphere_effects",
    "bench_retro_and_eyelid_mask",
    "bench_all_event_types",
    "bench_classic_sampling",
    "bench_classic_godray",
    "bench_classic_film_noise",
    "bench_classic_color_and_lens",
    "bench_stress_composition",
];
const HOTSPOT_BENCHMARK_RUNS: usize = 3;
const CAMERA_BENCHMARK_WORKLOADS: &[(&str, BenchmarkCameras)] = &[
    (
        "opening composition · scene + UI",
        BenchmarkCameras::SceneUi,
    ),
    (
        "opening composition · scene + dialog",
        BenchmarkCameras::SceneDialog,
    ),
    (
        "opening composition · scene only",
        BenchmarkCameras::SceneOnly,
    ),
];
const PORTABLE_BENCHMARK_SECTIONS: &[BenchmarkSection] = &[
    (
        "control · same background, portraits and dialogue; no effects",
        BASELINE_BENCHMARK_WORKLOADS,
    ),
    (
        "isolated effects · one family at a time",
        ISOLATED_EFFECT_BENCHMARK_WORKLOADS,
    ),
    (
        "particles · density, texture, fill and emitter scaling",
        PARTICLE_BENCHMARK_WORKLOADS,
    ),
    (
        "media · Opus loop/crossfade and 1080p H.264 playback",
        MEDIA_BENCHMARK_WORKLOADS,
    ),
    (
        "UI and sprites · overlays, filter and environment light control",
        UI_BENCHMARK_WORKLOADS,
    ),
    (
        "daily workloads · representative player-facing actions",
        DAILY_BENCHMARK_WORKLOADS,
    ),
    (
        "feature coverage · authored property and event combinations",
        FEATURE_BENCHMARK_WORKLOADS,
    ),
    (
        "classic camera attribution · isolated shader cost groups",
        CLASSIC_ATTRIBUTION_BENCHMARK_WORKLOADS,
    ),
    (
        "stress workload · intentionally combined peak load",
        STRESS_BENCHMARK_WORKLOADS,
    ),
];

/// Android shares the desktop workload inventory; unsupported presentation/video
/// cases remain explicit skips rather than silently disappearing from coverage.
#[cfg(feature = "publisher")]
pub(crate) fn android_benchmark_plan() -> serde_json::Value {
    use serde_json::json;
    let mut samples = Vec::new();
    let mut add = |label: &str,
                   target: Option<&str>,
                   camera: &str,
                   mode: &str,
                   runs: usize,
                   kind: &str,
                   skip: Option<&str>,
                   required: bool| {
        let mut args = vec!["perf".to_owned(), "embedded".to_owned()];
        if kind == "startup" {
            args.extend(["--startup".into(), "--runs".into(), "1".into()]);
        } else if kind == "render" {
            args.extend([
                "--seconds".into(),
                "5".into(),
                "--mode".into(),
                mode.into(),
                "--raw".into(),
            ]);
            if let Some(target) = target {
                args.extend(["--timeline".into(), target.into()]);
            }
            if camera != "runtime" {
                args.extend(["--camera".into(), camera.into()]);
            }
        }
        samples.push(
            json!({"label":label,"target":target,"camera":camera,"mode":mode,
            "runs":runs,"kind":kind,"args":args,"skip":skip,"required":required}),
        );
    };
    add(
        "startup", None, "runtime", "runtime", 7, "startup", None, false,
    );
    add(
        "opening composition · runtime sleep/wake",
        None,
        "runtime",
        "runtime",
        1,
        "render",
        None,
        false,
    );
    add(
        "opening composition · runtime composition",
        None,
        "runtime",
        "continuous",
        1,
        "render",
        None,
        false,
    );
    for (label, cameras) in CAMERA_BENCHMARK_WORKLOADS {
        add(
            label,
            None,
            cameras.id(),
            "continuous",
            1,
            "render",
            None,
            false,
        );
    }
    for (_, workloads) in PORTABLE_BENCHMARK_SECTIONS {
        for (label, target) in *workloads {
            let skip = target
                .starts_with("bench_video_")
                .then_some("Android runtime does not support video");
            let android_target = if *target == "bench_stress_composition" {
                "bench_stress_android"
            } else {
                target
            };
            let label = if *target == "bench_stress_composition" {
                "stress composition · Android, without video"
            } else {
                label
            };
            add(
                label,
                Some(android_target),
                "runtime",
                "continuous",
                benchmark_workload_runs(target),
                "render",
                skip,
                true,
            );
        }
    }
    for (label, target, skip) in [
        (
            "720p particles",
            "bench_particle_snow_256",
            Some("Android uses the actual device surface; desktop window resizing is unsupported"),
        ),
        (
            "1080p particles",
            "bench_particle_snow_256",
            Some("Android uses the actual device surface; desktop window resizing is unsupported"),
        ),
        ("runtime dialogue", "bench_representative_dialogue", None),
        ("runtime particles", "bench_particle_snow_256", None),
        ("runtime audio", "bench_audio_loop", None),
        (
            "runtime video",
            "bench_video_fullscreen",
            Some("Android runtime does not support video"),
        ),
        (
            "fullscreen stress · Android, without video",
            "bench_stress_android",
            None,
        ),
    ] {
        add(
            label,
            Some(target),
            "runtime",
            if label.starts_with("runtime ") {
                "runtime"
            } else {
                "continuous"
            },
            1,
            "render",
            skip,
            true,
        );
    }
    add(
        "packaged APK I/O",
        None,
        "runtime",
        "runtime",
        1,
        "package",
        None,
        false,
    );
    json!({"schema":1,"application_id":"moe.maincore.keine.benchmark", "engine_version":env!("CARGO_PKG_VERSION"),
        "commit":env!("KEINE_BUILD_COMMIT"), "profile":"profiling", "samples":samples})
}

#[derive(Default)]
struct LaunchOptions {
    development: bool,
    editor_sync: bool,
    benchmark: Option<BenchmarkOptions>,
    startup_capture: Option<crate::ui::performance::StartupCapture>,
    hidden_window: bool,
    authoring_preview: Option<super::preview::AuthoringPreviewConfig>,
}

#[derive(SystemParam)]
struct BootstrapMode<'w> {
    editor_sync: Option<Res<'w, EditorSyncSession>>,
    authoring_preview: Option<Res<'w, super::preview::AuthoringPreviewSession>>,
    benchmark: Option<Res<'w, crate::ui::performance::RuntimeCaptureConfig>>,
    #[cfg(feature = "hot-reload")]
    hot_reload: Option<Res<'w, HotReloadSession>>,
}

pub fn run() {
    run_with_loader(LoaderRegistry::default());
}

pub fn run_cli() -> std::process::ExitCode {
    let process_started = Instant::now();
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if let Some(code) = help_or_version(&args) {
        return code;
    }
    let parsed = if args.is_empty() {
        match packaged_benchmark_command() {
            Ok(Some(command)) => Ok(command),
            Ok(None) => parse_cli(&args),
            Err(error) => Err(error),
        }
    } else {
        parse_cli(&args)
    };
    let command = match parsed {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error:#}\nrun `keine --help` for usage");
            return std::process::ExitCode::FAILURE;
        }
    };
    let uses_startup_error_page = command.uses_startup_error_page();
    let result = execute_command(LoaderRegistry::default(), command, process_started);

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            report_startup_error(uses_startup_error_page, "failed to open project", &error);
            std::process::ExitCode::FAILURE
        }
    }
}

pub fn run_with_loader(loader: LoaderRegistry) {
    let process_started = Instant::now();
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let parsed = parse_cli(&args);
    let uses_startup_error_page = parsed
        .as_ref()
        .is_ok_and(CliCommand::uses_startup_error_page);
    let result = parsed.and_then(|command| execute_command(loader, command, process_started));
    if let Err(error) = result {
        report_startup_error(uses_startup_error_page, "failed to open project", &error);
    }
}

fn report_startup_error(show_page: bool, stage: &str, error: &anyhow::Error) {
    super::platform::startup_error(stage, error);
    if show_page {
        crate::ui::startup_error::show();
    }
}

fn execute_command(
    loader: LoaderRegistry,
    command: CliCommand,
    process_started: Instant,
) -> Result<()> {
    #[cfg(feature = "hardened")]
    super::platform::apply_hardening();
    let (project_path, action) = match command {
        CliCommand::AuthoringHost { endpoint, token } => {
            return super::authoring::run(&endpoint, &token, loader);
        }
        CliCommand::Pack { project, output } => {
            #[cfg(feature = "publisher")]
            return crate::publisher::pack_project(
                &resolve_project_path(project),
                &loader,
                &output,
            );
            #[cfg(not(feature = "publisher"))]
            {
                let _ = (project, output);
                anyhow::bail!("publisher tools are not compiled; run `cargo pack <project>`");
            }
        }
        CliCommand::Icons { source, output } => {
            #[cfg(feature = "publisher")]
            {
                if output.exists() {
                    anyhow::bail!("choose a fresh icon output directory");
                }
                let icons = keine_media::icons::IconSet::read(std::fs::File::open(source)?)?;
                return icons
                    .write(&output)
                    .context("failed to derive application icons");
            }
            #[cfg(not(feature = "publisher"))]
            {
                let _ = (source, output);
                anyhow::bail!("icon tools are not compiled; enable publisher");
            }
        }
        CliCommand::Bundle {
            project,
            output,
            benchmark,
        } => {
            #[cfg(feature = "publisher")]
            return crate::publisher::bundle_project(
                &resolve_project_path(project),
                &loader,
                &output,
                benchmark,
            );
            #[cfg(not(feature = "publisher"))]
            {
                let _ = (project, output, benchmark);
                anyhow::bail!("publisher tools are not compiled; run `cargo bundle <project>`");
            }
        }
        CliCommand::BenchmarkReport {
            project,
            runs,
            report_path,
        } => return run_benchmark_report(&project, runs, &report_path),
        CliCommand::PackageBenchmark { project: _project } => {
            #[cfg(any(feature = "publisher", feature = "startup-metrics"))]
            {
                print!("{}", super::package_benchmark::run(&_project, &loader)?);
                return Ok(());
            }
            #[cfg(not(any(feature = "publisher", feature = "startup-metrics")))]
            anyhow::bail!("package benchmark support is not compiled");
        }
        CliCommand::Remap {
            project,
            rules,
            yes,
        } => {
            #[cfg(feature = "publisher")]
            return crate::resource_migration::run(
                &resolve_project_path(project),
                &loader,
                &rules,
                yes,
            );
            #[cfg(not(feature = "publisher"))]
            {
                let _ = (project, rules, yes);
                anyhow::bail!(
                    "resource remapping tools are not compiled; run `cargo remap --help`"
                );
            }
        }
        CliCommand::Migrate { source, target } => {
            #[cfg(feature = "publisher")]
            return crate::project_migration::run(
                &resolve_project_path(source),
                &resolve_project_path(target),
                &loader,
            );
            #[cfg(not(feature = "publisher"))]
            {
                let _ = (source, target);
                anyhow::bail!("migration tools are not compiled; run `cargo migrate --help`");
            }
        }
        CliCommand::Validate { project } => (project, ProjectAction::Check),
        CliCommand::Run {
            project,
            mode,
            editor_sync,
        } => (project, ProjectAction::Run { mode, editor_sync }),
    };
    let project_path = resolve_project_path(project_path);
    if let ProjectAction::Run { mode, editor_sync } = &action
        && let Some(options) = mode.startup_benchmark()
        && std::env::var_os(STARTUP_BENCHMARK_CHILD_ENV).is_none()
    {
        if *editor_sync {
            anyhow::bail!("startup benchmark cannot run in editor-sync mode");
        }
        run_startup_suite(&project_path, options.runs)?;
        return Ok(());
    }
    let OpenedProject {
        root: project_root,
        config,
        content,
        packaged,
    } = open_project(&project_path, &loader)?;
    let project_opened = Instant::now();
    let languages = loader
        .languages(&config.adapter.script)
        .context("failed to select script adapter")?;
    let (mode, editor_sync) = match action {
        ProjectAction::Check => return check_project(&config, &content, &languages),
        ProjectAction::Run { mode, editor_sync } => (mode, editor_sync),
    };
    let store = loader
        .store(&config.adapter.store)
        .context("failed to select store adapter")?;
    let startup_capture = mode
        .startup_benchmark()
        .map(|_| crate::ui::performance::StartupCapture::new(process_started, project_opened));
    let persistence_root =
        crate::storage::persistence_root(&project_root, &config.project, packaged)?;
    let _instance = mode.requires_single_instance().then(|| {
        SingleInstanceGuard::acquire(&persistence_root)
            .context("another instance of this project is already running")
    });
    let _instance = _instance.transpose()?;
    if mode.benchmark().is_none() && mode.startup_benchmark().is_none() {
        crate::storage::prepare_persistence(&project_root, &persistence_root)
            .context("failed to prepare persistent game data")?;
    }
    let mut app = build_opened_app(
        project_root,
        persistence_root,
        config,
        content,
        languages,
        store,
        LaunchOptions {
            development: mode.development(),
            editor_sync,
            benchmark: mode.benchmark().cloned(),
            startup_capture: startup_capture.clone(),
            hidden_window: startup_capture.is_some()
                || std::env::var_os(RUNTIME_BENCHMARK_CHILD_ENV).is_some(),
            authoring_preview: None,
        },
    );
    if let Some(capture) = startup_capture {
        capture.mark_app_built();
    }
    app.run();
    Ok(())
}

const STARTUP_BENCHMARK_CHILD_ENV: &str = "KEINE_STARTUP_BENCHMARK_CHILD";
pub(crate) const RUNTIME_BENCHMARK_CHILD_ENV: &str = "KEINE_RUNTIME_BENCHMARK_CHILD";

fn run_startup_suite(project_path: &Path, runs: usize) -> Result<String> {
    let executable =
        std::env::current_exe().context("failed to locate the benchmark executable")?;
    let mut samples = Vec::with_capacity(runs);
    let logical_threads = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    let profile = if cfg!(debug_assertions) {
        "development"
    } else {
        "release"
    };
    let mut report = String::new();
    emit_report_line(
        &mut report,
        format!(
            "startup baseline · Kēne {} · {profile} · {} / {} · {logical_threads} logical thread(s) · {runs} isolated process run(s) · hidden surface-backed window",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
        ),
    );
    let features = env!("KEINE_BUILD_FEATURES");
    emit_report_line(
        &mut report,
        format!(
            "build identity · commit {} · built {} · features {}",
            env!("KEINE_BUILD_COMMIT"),
            env!("KEINE_BUILD_TIME"),
            if features.is_empty() {
                "none"
            } else {
                features
            },
        ),
    );
    append_host_environment(&mut report, logical_threads);
    for run in 1..=runs {
        let output = Command::new(&executable)
            .arg("perf")
            .arg(project_path)
            .args(["--startup", "--runs", "1"])
            .env(STARTUP_BENCHMARK_CHILD_ENV, "1")
            .output()
            .with_context(|| format!("failed to start benchmark child {run}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if run == 1
            && let Some(gpu) = stderr.lines().find(|line| line.contains("GPU      │"))
        {
            emit_report_line(&mut report, gpu.trim());
        }
        let sample = crate::ui::performance::StartupSample::parse(&format!("{stdout}\n{stderr}"));
        if !output.status.success()
            || sample.is_none()
            || stderr.lines().any(|line| line.contains(" ERROR "))
        {
            anyhow::bail!(
                "startup child {run} failed with {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                output.status,
            );
        }
        let sample = sample.expect("sample presence was checked");
        let peak_rss = sample
            .peak_rss_mib
            .map_or_else(|| "n/a".to_owned(), |value| format!("{value:.1} MiB"));
        emit_report_line(
            &mut report,
            format!(
                "run {run:>2} · project {:>7.2} ms · app {:>7.2} ms · first frame {:>7.2} ms · interactive {:>7.2} ms · peak RSS {peak_rss}",
                sample.project_ms, sample.app_ms, sample.first_frame_ms, sample.interactive_ms,
            ),
        );
        samples.push(sample);
        if run != runs {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    append_startup_summary(&samples, &mut report);
    Ok(report)
}

fn append_host_environment(report: &mut String, logical_threads: usize) {
    let memory = physical_memory_bytes().map_or_else(
        || "unknown RAM".to_owned(),
        |bytes| format!("{:.1} GiB RAM", bytes as f64 / 1_073_741_824.0),
    );
    emit_report_line(
        report,
        format!(
            "host environment · OS {} · CPU {} · {logical_threads} logical thread(s) · {memory}",
            host_os_version(),
            host_cpu_model(),
        ),
    );
    if let Some(power) = host_power_context() {
        emit_report_line(report, format!("power context · {power}"));
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn clean_command_output(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    output
        .status
        .success()
        .then(|| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|output| !output.is_empty())
}

#[cfg(windows)]
fn host_cpu_model() -> String {
    std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown".to_owned())
}

#[cfg(target_os = "macos")]
fn host_cpu_model() -> String {
    clean_command_output("sysctl", &["-n", "machdep.cpu.brand_string"])
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn host_cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim() == "model name").then(|| value.trim().to_owned())
            })
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(not(any(windows, unix)))]
fn host_cpu_model() -> String {
    "unknown".to_owned()
}

#[cfg(windows)]
fn host_os_version() -> String {
    clean_command_output("cmd", &["/C", "ver"]).unwrap_or_else(|| "Windows".to_owned())
}

#[cfg(target_os = "macos")]
fn host_os_version() -> String {
    clean_command_output("sw_vers", &["-productVersion"])
        .map_or_else(|| "macOS".to_owned(), |version| format!("macOS {version}"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn host_os_version() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("PRETTY_NAME=")
                    .map(|value| value.trim_matches('"').to_owned())
            })
        })
        .unwrap_or_else(|| std::env::consts::OS.to_owned())
}

#[cfg(not(any(windows, unix)))]
fn host_os_version() -> String {
    std::env::consts::OS.to_owned()
}

#[cfg(windows)]
fn host_power_context() -> Option<String> {
    clean_command_output("powercfg", &["/getactivescheme"])
}

#[cfg(not(windows))]
fn host_power_context() -> Option<String> {
    None
}

#[cfg(all(feature = "startup-metrics", windows))]
fn physical_memory_bytes() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` has the required size and remains writable for the call.
    let success = unsafe { GlobalMemoryStatusEx(&mut status) };
    (success != 0).then_some(status.ullTotalPhys)
}

#[cfg(all(feature = "startup-metrics", unix))]
fn physical_memory_bytes() -> Option<u64> {
    // SAFETY: `sysconf` has no pointer arguments and these names query stable
    // process-global system values.
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    // SAFETY: See above; the page-size query has the same contract.
    let page_bytes = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    (pages > 0 && page_bytes > 0)
        .then(|| (pages as u64).checked_mul(page_bytes as u64))
        .flatten()
}

#[cfg(not(any(
    all(feature = "startup-metrics", windows),
    all(feature = "startup-metrics", unix)
)))]
fn physical_memory_bytes() -> Option<u64> {
    None
}

fn run_benchmark_report(project_path: &Path, runs: usize, report_path: &Path) -> Result<()> {
    let mut report = String::new();
    let mut raw_frames = format!(
        "RAWFRAME\tworkload\trun\ttotal_runs\ttarget\tcamera_profile\t{}\n",
        crate::ui::performance::TRACE_FIELDS,
    );
    let outcome = (|| -> Result<()> {
        let executable =
            std::env::current_exe().context("failed to locate benchmark executable")?;
        report.push_str(&run_startup_suite(project_path, runs)?);
        // Measure real sleep/wake behavior separately from continuous throughput.
        let mut preliminary_failed = false;
        if let Err(error) = run_benchmark_workload(
            BenchmarkWorkloadCapture {
                executable: &executable,
                project_path,
                label: "opening composition · runtime sleep/wake",
                target: None,
                cameras: BenchmarkCameras::Runtime,
                window: BenchmarkWindow::Default,
                continuous: false,
                run: 1,
                total_runs: 1,
            },
            &mut report,
            &mut raw_frames,
        ) {
            preliminary_failed = true;
            emit_report_line(
                &mut report,
                format!("FAILED   | runtime opening · {error:#}"),
            );
        }
        emit_report_line(&mut report, "");
        emit_report_line(
            &mut report,
            "project workload · actual packaged opening composition",
        );
        emit_report_line(
            &mut report,
            "portable render coverage · continuous window loop · runtime cameras auto-disable empty overlays · decomposition pins cameras",
        );
        let timeline_inventory = run_benchmark_workload(
            BenchmarkWorkloadCapture {
                executable: &executable,
                project_path,
                label: "opening composition · runtime composition",
                target: None,
                cameras: BenchmarkCameras::Runtime,
                window: BenchmarkWindow::Default,
                continuous: true,
                run: 1,
                total_runs: 1,
            },
            &mut report,
            &mut raw_frames,
        )?
        .stderr;
        emit_report_line(&mut report, "");
        emit_report_line(
            &mut report,
            "camera decomposition · opening composition render attribution",
        );
        for (label, cameras) in CAMERA_BENCHMARK_WORKLOADS {
            run_benchmark_workload(
                BenchmarkWorkloadCapture {
                    executable: &executable,
                    project_path,
                    label,
                    target: None,
                    cameras: *cameras,
                    window: BenchmarkWindow::Default,
                    continuous: true,
                    run: 1,
                    total_runs: 1,
                },
                &mut report,
                &mut raw_frames,
            )?;
        }
        let mut completed = 0;
        let mut missing = 0;
        let mut failed = 0;
        let mut ranked = Vec::new();
        for (section, workloads) in PORTABLE_BENCHMARK_SECTIONS {
            emit_report_line(&mut report, "");
            emit_report_line(&mut report, section);
            for (label, target) in *workloads {
                if !timeline_is_available(&timeline_inventory, target) {
                    missing += 1;
                    emit_report_line(
                        &mut report,
                        format!("MISSING  | {label} · target {target:?}"),
                    );
                    continue;
                }
                let total_runs = benchmark_workload_runs(target);
                let mut summaries = Vec::with_capacity(total_runs);
                for run in 1..=total_runs {
                    match run_benchmark_workload(
                        BenchmarkWorkloadCapture {
                            executable: &executable,
                            project_path,
                            label,
                            target: Some(target),
                            cameras: BenchmarkCameras::Runtime,
                            window: BenchmarkWindow::Default,
                            continuous: true,
                            run,
                            total_runs,
                        },
                        &mut report,
                        &mut raw_frames,
                    ) {
                        Ok(output) => summaries.push(output.summary),
                        Err(error) => {
                            emit_report_line(
                                &mut report,
                                format!("FAILED   | {label} · run {run}/{total_runs} · {error:#}"),
                            );
                            break;
                        }
                    }
                }
                if summaries.len() != total_runs {
                    failed += 1;
                    continue;
                }
                completed += 1;
                if total_runs > 1 {
                    append_render_repeat_summary(label, &summaries, &mut report);
                }
                summaries.sort_by(|a, b| a.average_ms.total_cmp(&b.average_ms));
                ranked.push((*label, *target, summaries[total_runs / 2]));
            }
        }
        // Visible windows exercise actual presentation and fill cost, separately
        // from the fixed-size hidden throughput samples used for shader attribution.
        for (label, window, target) in [
            (
                "720p particles",
                BenchmarkWindow::Size(1280, 720),
                "bench_particle_snow_256",
            ),
            (
                "1080p particles",
                BenchmarkWindow::Size(1920, 1080),
                "bench_particle_snow_256",
            ),
            (
                "runtime dialogue",
                BenchmarkWindow::Default,
                "bench_representative_dialogue",
            ),
            (
                "runtime particles",
                BenchmarkWindow::Default,
                "bench_particle_snow_256",
            ),
            (
                "runtime audio",
                BenchmarkWindow::Default,
                "bench_audio_loop",
            ),
            (
                "runtime video",
                BenchmarkWindow::Default,
                "bench_video_fullscreen",
            ),
            (
                "fullscreen stress",
                BenchmarkWindow::Fullscreen,
                "bench_stress_composition",
            ),
        ] {
            if !timeline_is_available(&timeline_inventory, target) {
                missing += 1;
                emit_report_line(
                    &mut report,
                    format!("MISSING  | {label} · target {target:?}"),
                );
                continue;
            }
            match run_benchmark_workload(
                BenchmarkWorkloadCapture {
                    executable: &executable,
                    project_path,
                    label,
                    target: Some(target),
                    cameras: BenchmarkCameras::Runtime,
                    window,
                    continuous: !label.starts_with("runtime "),
                    run: 1,
                    total_runs: 1,
                },
                &mut report,
                &mut raw_frames,
            ) {
                Ok(_) => completed += 1,
                Err(error) => {
                    failed += 1;
                    emit_report_line(&mut report, format!("FAILED   | {label} · {error:#}"));
                }
            }
        }
        append_hotspot_ranking(&ranked, &mut report);
        emit_report_line(
            &mut report,
            format!(
                "COVERAGE | {completed} completed · {missing} missing · {failed} failed · {} required render workloads · {}",
                PORTABLE_BENCHMARK_SECTIONS
                    .iter()
                    .map(|(_, w)| w.len())
                    .sum::<usize>()
                    + 7,
                if missing == 0 && failed == 0 {
                    "complete"
                } else {
                    "INCOMPLETE: not a full benchmark"
                }
            ),
        );
        emit_report_line(&mut report, "");
        emit_report_line(
            &mut report,
            "warm Hakutaku/cache throughput · real assets and isolated access-class stress",
        );
        emit_report_line(
            &mut report,
            "scope · package open/decrypt/cache/memory path; not a cold-cache or physical-device benchmark",
        );
        let mut package_failed = false;
        let package = (|| -> Result<String> {
            let output = Command::new(&executable)
                .arg("__benchmark-package")
                .arg(project_path)
                .output()
                .context("failed to start package I/O benchmark")?;
            if !output.status.success() {
                anyhow::bail!(
                    "package I/O failed: {}\n{}\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            String::from_utf8(output.stdout).context("package I/O output is not UTF-8")
        })();
        match package {
            Ok(output) => {
                for line in output.lines() {
                    emit_report_line(&mut report, line);
                }
            }
            Err(error) => {
                package_failed = true;
                emit_report_line(&mut report, format!("FAILED   | package I/O · {error:#}"));
            }
        }
        emit_report_line(&mut report, "");
        emit_report_line(
            &mut report,
            "raw frame appendix · bounded tab-separated samples for offline analysis",
        );
        if missing != 0 || failed != 0 || package_failed || preliminary_failed {
            anyhow::bail!(
                "benchmark incomplete ({missing} missing, {failed} failed, package failed: {package_failed}, runtime opening failed: {preliminary_failed}); partial evidence retained in {}",
                report_path.display()
            );
        }
        Ok(())
    })();
    if let Err(error) = &outcome {
        emit_report_line(
            &mut report,
            format!("FAILED   | suite incomplete · {error:#}"),
        );
    }
    report.push_str(&raw_frames);
    crate::storage::write_atomically(report_path, report.as_bytes())?;
    println!("benchmark report written to {}", report_path.display());
    outcome
}

struct BenchmarkWorkloadOutput {
    stderr: String,
    summary: crate::ui::performance::RenderSummary,
}

struct BenchmarkWorkloadCapture<'a> {
    executable: &'a Path,
    project_path: &'a Path,
    label: &'a str,
    target: Option<&'a str>,
    cameras: BenchmarkCameras,
    window: BenchmarkWindow,
    continuous: bool,
    run: usize,
    total_runs: usize,
}

fn run_benchmark_workload(
    capture: BenchmarkWorkloadCapture<'_>,
    report: &mut String,
    raw_frames: &mut String,
) -> Result<BenchmarkWorkloadOutput> {
    let BenchmarkWorkloadCapture {
        executable,
        project_path,
        label,
        target,
        cameras,
        window,
        continuous,
        run,
        total_runs,
    } = capture;
    emit_report_line(report, "");
    emit_report_line(
        report,
        if total_runs == 1 {
            format!("settled render · {label} · 3.0s warm-up + 5.0s sample")
        } else {
            format!(
                "settled render · {label} · run {run}/{total_runs} · 3.0s warm-up + 5.0s sample"
            )
        },
    );
    let mut command = Command::new(executable);
    let mode = if continuous { "continuous" } else { "runtime" };
    command
        .arg("perf")
        .arg(project_path)
        .args(["--seconds", "5", "--mode", mode, "--raw"]);
    // Reactive redraws require a real window; a hidden window cannot measure idle behavior.
    if continuous && window == BenchmarkWindow::Default {
        command.env(RUNTIME_BENCHMARK_CHILD_ENV, "1");
    } else {
        command.env_remove(RUNTIME_BENCHMARK_CHILD_ENV);
    }
    match window {
        BenchmarkWindow::Default => {}
        BenchmarkWindow::Size(width, height) => {
            command.args(["--window", &format!("{width}x{height}")]);
        }
        BenchmarkWindow::Fullscreen => {
            command.args(["--window", "fullscreen"]);
        }
    }
    if let Some(target) = target {
        command.arg("--timeline").arg(target);
    }
    if cameras != BenchmarkCameras::Runtime {
        command.arg("--camera").arg(cameras.id());
    }
    let output = command
        .output()
        .with_context(|| format!("failed to start {label} benchmark"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        anyhow::bail!(
            "{label} benchmark failed with {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
            output.status,
        );
    }
    if let Some(target) = target
        && !stderr.contains("resolved cursor Some(")
    {
        anyhow::bail!("{label} benchmark did not resolve timeline {target:?}\n{stderr}");
    }
    let mut captured = 0;
    let mut focused_intervals = 0;
    let mut summary = None;
    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim();
        if let Some(frame) = line.strip_prefix("KEINE_TRACE\t") {
            focused_intervals += usize::from(frame.split('\t').nth(10) == Some("true"));
            append_raw_frame(raw_frames, label, run, total_runs, target, cameras, frame);
            continue;
        }
        if let Some(sample) = crate::ui::performance::RenderSummary::parse(line) {
            summary = Some(sample);
            continue;
        }
        if benchmark_report_line(line) {
            emit_report_line(report, line);
            captured += 1;
        }
    }
    if captured == 0 {
        anyhow::bail!("{label} benchmark completed without performance output");
    }
    let summary =
        summary.context("benchmark completed without a machine-readable render sample")?;
    if stderr.lines().any(|line| line.contains(" ERROR ")) {
        anyhow::bail!("{label} reported an engine error\n{stderr}");
    }
    if !continuous && focused_intervals == 0 {
        anyhow::bail!(
            "{label} has no focused runtime intervals; leave the benchmark window foreground"
        );
    }
    if (continuous || label.starts_with("runtime ")) && summary.frames == 0 {
        anyhow::bail!("{label} has no eligible active render intervals");
    }
    Ok(BenchmarkWorkloadOutput {
        stderr: stderr.into_owned(),
        summary,
    })
}

fn benchmark_workload_runs(target: &str) -> usize {
    if REPEATED_BENCHMARK_TARGETS.contains(&target)
        || target == "bench_baseline"
        || target.starts_with("bench_particle_")
    {
        HOTSPOT_BENCHMARK_RUNS
    } else {
        1
    }
}

fn append_hotspot_ranking(
    results: &[(&str, &str, crate::ui::performance::RenderSummary)],
    report: &mut String,
) {
    emit_report_line(report, "");
    emit_report_line(
        report,
        "optimization candidates · controlled workload deltas, not exclusive function cost",
    );
    let Some((_, _, baseline)) = results
        .iter()
        .find(|(_, target, _)| *target == "bench_baseline")
    else {
        emit_report_line(
            report,
            "HOTSPOT  | reference unavailable; no fabricated deltas",
        );
        return;
    };
    let mut candidates = results
        .iter()
        .filter(|(_, target, s)| *target != "bench_baseline" && s.frames > 0)
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.2.average_ms.total_cmp(&a.2.average_ms));
    for (label, target, summary) in candidates.into_iter().take(15) {
        emit_report_line(
            report,
            format!(
                "HOTSPOT  | {label} · Δavg {:+.2} ms · avg {:.2} · p99 {:.2} · max {:.2} ms · 1% low {:.1} FPS · rerun: perf <package> --timeline {target:?} --mode continuous --raw",
                summary.average_ms - baseline.average_ms,
                summary.average_ms,
                summary.p99_ms,
                summary.maximum_ms,
                summary.one_percent_low_fps
            ),
        );
    }
    emit_report_line(
        report,
        "ATTRIBUTION | compare identical size/backend/cache/pacing; inspect each workload's PROCESS, MEMORY, UPDATE, RENDER and SLOW lines; frame intervals include driver/presentation/scheduling, CPU/GPU spans must not be summed; use a separate native stack capture to locate functions",
    );
}

fn append_render_repeat_summary(
    label: &str,
    summaries: &[crate::ui::performance::RenderSummary],
    report: &mut String,
) {
    let median = |select: fn(&crate::ui::performance::RenderSummary) -> f64| {
        let mut values = summaries.iter().map(select).collect::<Vec<_>>();
        values.sort_by(f64::total_cmp);
        values[values.len() / 2]
    };
    let minimum_fps = summaries
        .iter()
        .map(|summary| summary.average_fps)
        .min_by(f64::total_cmp)
        .unwrap_or_default();
    let maximum_fps = summaries
        .iter()
        .map(|summary| summary.average_fps)
        .max_by(f64::total_cmp)
        .unwrap_or_default();
    let worst_frame = summaries
        .iter()
        .map(|summary| summary.maximum_ms)
        .max_by(f64::total_cmp)
        .unwrap_or_default();
    emit_report_line(
        report,
        format!(
            "REPEAT   | {label} · {} runs · avg FPS median {:.1} ({minimum_fps:.1}..{maximum_fps:.1}) · 1% low median {:.1} · p99 median {:.2} ms · worst frame {worst_frame:.2} ms",
            summaries.len(),
            median(|summary| summary.average_fps),
            median(|summary| summary.one_percent_low_fps),
            median(|summary| summary.p99_ms),
        ),
    );
}

fn append_raw_frame(
    output: &mut String,
    label: &str,
    run: usize,
    total_runs: usize,
    target: Option<&str>,
    cameras: BenchmarkCameras,
    frame: &str,
) {
    output.push_str("RAWFRAME\t");
    output.push_str(&tsv_text(label));
    output.push_str(&format!("\t{run}\t{total_runs}\t"));
    output.push_str(&tsv_text(target.unwrap_or("opening")));
    output.push('\t');
    output.push_str(&tsv_text(cameras.id()));
    // Already sanitized by the shared capture owner. Preserve all fields,
    // including excluded intervals, so a report can be independently audited.
    output.push('\t');
    output.push_str(frame);
    output.push('\n');
}

fn tsv_text(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

fn timeline_is_available(inventory: &str, wanted: &str) -> bool {
    let suffix = format!(":{wanted}");
    inventory.lines().any(|line| {
        line.split_once("TIMELINE | ")
            .is_some_and(|(_, timelines)| {
                timelines
                    .split(", ")
                    .any(|timeline| timeline.ends_with(&suffix))
            })
    })
}

fn benchmark_report_line(line: &str) -> bool {
    [
        "GPU      │",
        "GPUINFO  |",
        "WINDOWSYS |",
        "DISPLAY  |",
        "START    |",
        "CAPTURE  |",
        "FRAME    |",
        "BUDGET   |",
        "PROCESS  |",
        "SAMPLING |",
        "EXCLUDED |",
        "GPU_TIME |",
        "SLOW     |",
        "SCENE    |",
        "ASSETS   |",
        "MEMORY   |",
        "RENDER   |",
        "UPDATE   |",
        " ERROR ",
    ]
    .iter()
    .any(|marker| line.contains(marker))
}

fn benchmark_timelines(state: &State) -> Vec<(String, usize, String)> {
    let mut timelines = state
        .program
        .scene_names()
        .flat_map(|scene| {
            state
                .program
                .scene(scene)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(move |(index, action)| match action {
                    Action::StageAnimation { animation } => {
                        Some((scene.to_owned(), index, animation.id.clone()))
                    }
                    _ => None,
                })
        })
        .collect::<Vec<_>>();
    timelines.sort();
    timelines
}

fn resolve_benchmark_target(state: &State, target: &BenchmarkTarget) -> Option<(String, usize)> {
    match target {
        BenchmarkTarget::Cursor(cursor) => state
            .program
            .scene_len(&state.current_scene)
            .filter(|len| *cursor < *len)
            .map(|_| (state.current_scene.clone(), *cursor)),
        BenchmarkTarget::SceneCursor(scene, cursor) => state
            .program
            .scene_len(scene)
            .filter(|len| *cursor < *len)
            .map(|_| (scene.clone(), *cursor)),
        BenchmarkTarget::Timeline(wanted) => {
            let mut matches = benchmark_timelines(state)
                .into_iter()
                .filter(|(_, _, timeline)| timeline == wanted)
                .map(|(scene, cursor, _)| (scene, cursor));
            let resolved = matches.next();
            if matches.next().is_some() {
                log::error!(
                    target: "keine::performance",
                    "benchmark timeline {wanted:?} is ambiguous across fragments",
                );
                None
            } else {
                resolved
            }
        }
    }
}

fn emit_report_line(report: &mut String, line: impl AsRef<str>) {
    let line = plain_report_line(line.as_ref());
    println!("{line}");
    report.push_str(&line);
    report.push('\n');
}

fn plain_report_line(line: &str) -> String {
    let mut plain = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' && characters.next_if_eq(&'[').is_some() {
            for code in characters.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
            continue;
        }
        plain.push(character);
    }
    plain
}

fn append_startup_summary(samples: &[crate::ui::performance::StartupSample], report: &mut String) {
    let Some(first) = samples.first() else {
        return;
    };
    let repeat_median = |select: fn(&crate::ui::performance::StartupSample) -> f64| {
        let mut values = samples.iter().skip(1).map(select).collect::<Vec<_>>();
        if values.is_empty() {
            return None;
        }
        values.sort_by(f64::total_cmp);
        Some(values[(values.len() - 1) / 2])
    };
    let format_pair = |first: f64, repeat: Option<f64>| {
        repeat.map_or_else(
            || format!("{first:.2} / n/a ms"),
            |repeat| format!("{first:.2} / {repeat:.2} ms"),
        )
    };
    let peak_rss = samples
        .iter()
        .filter_map(|sample| sample.peak_rss_mib)
        .max_by(f64::total_cmp);
    emit_report_line(
        report,
        "first run / repeat median (cumulative from process entry)",
    );
    emit_report_line(
        report,
        format!(
            "project     · {}",
            format_pair(first.project_ms, repeat_median(|sample| sample.project_ms))
        ),
    );
    emit_report_line(
        report,
        format!(
            "app built   · {}",
            format_pair(first.app_ms, repeat_median(|sample| sample.app_ms))
        ),
    );
    emit_report_line(
        report,
        format!(
            "first frame · {}",
            format_pair(
                first.first_frame_ms,
                repeat_median(|sample| sample.first_frame_ms),
            )
        ),
    );
    emit_report_line(
        report,
        format!(
            "interactive · {}",
            format_pair(
                first.interactive_ms,
                repeat_median(|sample| sample.interactive_ms),
            )
        ),
    );
    if let Some(peak_rss) = peak_rss {
        emit_report_line(
            report,
            format!("peak RSS    · {peak_rss:.1} MiB maximum across runs"),
        );
    }
    emit_report_line(
        report,
        "cache note  · every sample is a new process; filesystem/GPU caches are intentionally not claimed cold",
    );
}

enum ProjectAction {
    Check,
    Run {
        mode: InteractiveMode,
        editor_sync: bool,
    },
}

struct SingleInstanceGuard {
    _file: File,
}

impl SingleInstanceGuard {
    fn acquire(project_root: &Path) -> Result<Self> {
        let path = instance_lock_path(project_root);
        let directory = path.parent().context("instance lock path has no parent")?;
        std::fs::create_dir_all(directory).with_context(|| {
            format!(
                "failed to create runtime data directory {}",
                directory.display()
            )
        })?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open instance lock {}", path.display()))?;
        fs2::FileExt::try_lock_exclusive(&file)
            .with_context(|| format!("failed to lock {}", path.display()))?;
        Ok(Self { _file: file })
    }
}

fn instance_lock_path(project_root: &Path) -> PathBuf {
    let canonical = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_owned());
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    std::env::temp_dir()
        .join("keine")
        .join(format!("{:016x}.lock", hasher.finish()))
}

/// Builds a customizable Bevy application for one project without running it.
/// Extension plugins can claim and consume [`crate::HostCommandMessage`] before
/// calling `App::run`, while built-in adapter semantics stay on typed actions.
pub fn build_app_with_loader(
    project_path: impl AsRef<Path>,
    loader: LoaderRegistry,
) -> Result<App> {
    build_project_app(open_project(project_path.as_ref(), &loader)?, loader)
}

pub(crate) fn build_project_app(project: OpenedProject, loader: LoaderRegistry) -> Result<App> {
    let OpenedProject {
        root: project_root,
        config,
        content,
        packaged,
    } = project;
    let languages = loader
        .languages(&config.adapter.script)
        .context("failed to select script adapter")?;
    let store = loader
        .store(&config.adapter.store)
        .context("failed to select store adapter")?;
    let persistence_root =
        crate::storage::persistence_root(&project_root, &config.project, packaged)?;
    crate::storage::prepare_persistence(&project_root, &persistence_root)
        .context("failed to prepare persistent game data")?;
    Ok(build_opened_app(
        project_root,
        persistence_root,
        config,
        content,
        languages,
        store,
        LaunchOptions::default(),
    ))
}

#[cfg(all(target_os = "android", feature = "startup-metrics"))]
pub(super) fn build_android_benchmark_app(
    project: OpenedProject,
    mode: InteractiveMode,
    started: Instant,
) -> Result<App> {
    let opened = Instant::now();
    let loader = LoaderRegistry::default();
    let languages = loader.languages(&project.config.adapter.script)?;
    let store = loader.store(&project.config.adapter.store)?;
    let capture = mode
        .startup_benchmark()
        .map(|_| crate::ui::performance::StartupCapture::new(started, opened));
    let persistence =
        crate::storage::persistence_root(&project.root, &project.config.project, project.packaged)?;
    let app = build_opened_app(
        project.root,
        persistence,
        project.config,
        project.content,
        languages,
        store,
        LaunchOptions {
            benchmark: mode.benchmark().cloned(),
            startup_capture: capture.clone(),
            ..Default::default()
        },
    );
    if let Some(capture) = capture {
        capture.mark_app_built();
    }
    Ok(app)
}

pub(crate) fn build_authoring_preview_app(
    project_path: &Path,
    overlay_root: &Path,
    loader: &LoaderRegistry,
    preview: super::preview::AuthoringPreviewConfig,
) -> Result<App> {
    let OpenedProject {
        root: project_root,
        config,
        mut content,
        packaged: _,
    } = open_project(project_path, loader)?;
    let mut overlay = SourceMount::project("authoring-preview", overlay_root.to_owned());
    overlay.asset = None;
    content.sources.push(overlay);
    let languages = loader
        .languages(&config.adapter.script)
        .context("failed to select script adapter")?;
    let store = loader
        .store(&config.adapter.store)
        .context("failed to select store adapter")?;
    // Authoring saves/settings are isolated from both editable source and
    // shipping per-user data; the overlay owner removes them on clean exit.
    let persistence_root = overlay_root.join("preview-data");
    Ok(build_opened_app(
        project_root,
        persistence_root,
        config,
        content,
        languages,
        store,
        LaunchOptions {
            editor_sync: true,
            hidden_window: false,
            authoring_preview: Some(preview),
            ..default()
        },
    ))
}

fn build_opened_app(
    project_root: PathBuf,
    persistence_root: PathBuf,
    config: GameConfig,
    content: ContentProject,
    languages: keine_loader::ScriptLanguageRegistry,
    store: std::sync::Arc<dyn keine_loader::StoreAdapter>,
    options: LaunchOptions,
) -> App {
    let webp = crate::scene::images::NativeWebpPlugin::new(config.layout.sprite_height);
    let asset_mounts = content.asset_mounts();
    let watch_assets = options.development
        && asset_mounts
            .iter()
            .any(|mount| mount.filesystem_root().is_some());

    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        crate::runtime::asset_reader::overlay_source(asset_mounts.clone()),
    );
    let mut initial_resolution = WindowResolution::new(DESIGN_WIDTH as u32, DESIGN_HEIGHT as u32);
    // Keep the native runtime on the engine's 1920x1080 design grid even on
    // Retina/HiDPI monitors. Studio sync is a normal independent window; no
    // host overlay or focus interception is involved.
    initial_resolution.set_scale_factor_override(Some(1.0));
    let authoring_preview = options.authoring_preview.is_some();
    let remember_window = !cfg!(target_os = "android")
        && options.benchmark.is_none()
        && options.startup_capture.is_none()
        && !options.hidden_window;
    let window_plugin = WindowPlugin {
        primary_window: Some(Window {
            name: Some(
                config
                    .project
                    .application_identifier()
                    .unwrap_or_else(|| "moe.maincore.keine".into()),
            ),
            title: if authoring_preview {
                format!("{} — Kēne Preview", config.title)
            } else {
                config.title.clone()
            },
            mode: if options
                .benchmark
                .as_ref()
                .is_some_and(|b| b.window == BenchmarkWindow::Fullscreen)
            {
                bevy::window::WindowMode::BorderlessFullscreen(
                    bevy::window::MonitorSelection::Primary,
                )
            } else {
                bevy::window::WindowMode::Windowed
            },
            resolution: if let Some(BenchmarkWindow::Size(width, height)) =
                options.benchmark.as_ref().map(|b| b.window)
            {
                WindowResolution::new(width, height).with_scale_factor_override(1.0)
            } else if authoring_preview {
                WindowResolution::new(1280, 720)
            } else {
                initial_resolution
            },
            // Startup reports keep the real winit window and wgpu
            // surface but hide them from the desktop/taskbar. A truly
            // headless render target would omit the startup costs this
            // benchmark is intended to measure.
            visible: !options.hidden_window && !remember_window,
            ..default()
        }),
        // Keep the native window alive until the shutdown pipeline has
        // flushed persistence. Despawning it immediately can race the
        // final winit `Destroyed` event and produce an unknown-window
        // warning during an otherwise successful exit.
        close_when_requested: false,
        ..default()
    };
    let plugins = super::platform::default_plugins()
        .set(AssetPlugin {
            watch_for_changes_override: Some(watch_assets),
            ..default()
        })
        .set(window_plugin)
        .set(ImagePlugin::default())
        .set(super::platform::log_plugin(
            options.benchmark.is_some() || options.startup_capture.is_some(),
        ));
    app.add_plugins(plugins)
        .insert_resource(ClearColor(Color::BLACK));
    if remember_window {
        app.add_plugins(super::window::WindowMemoryPlugin);
    }
    crate::runtime::audio::configure_audio(&mut app, asset_mounts);
    app.add_plugins((webp, GamePlugin, CameraEffectsPlugin, BlurPlugin))
        .insert_resource(ProjectRoot(project_root))
        .insert_resource(PersistenceRoot(persistence_root))
        .insert_resource(ContentProjectResource(content))
        .insert_resource(ScriptLanguages(languages))
        .insert_resource(StoreCodec(store))
        .insert_resource(GameConfigResource(config))
        .add_systems(PreStartup, bootstrap_project);
    app.add_systems(PostStartup, set_primary_window_icon);
    if options.editor_sync {
        app.init_resource::<EditorSyncSession>();
    }
    if let Some(preview) = options.authoring_preview {
        app.add_plugins(super::preview::AuthoringPreviewPlugin::new(preview));
    }
    if options.development {
        app.init_resource::<DevelopmentSession>();
        #[cfg(feature = "hot-reload")]
        app.init_resource::<HotReloadSession>();
    }
    if let Some(benchmark) = options.benchmark {
        // Render diagnostics are benchmark-only. Vulkan and DX12 expose both
        // CPU/GPU pass time and pipeline statistics; other backends still
        // provide CPU pass time without affecting normal game execution.
        app.add_plugins((
            EntityCountDiagnosticsPlugin::default(),
            RenderDiagnosticsPlugin,
        ));
        app.init_resource::<PersistenceDisabled>();
        crate::ui::performance::install_runtime_capture(
            &mut app,
            benchmark.seconds,
            benchmark.target.clone(),
            benchmark.cameras,
            benchmark.continuous,
            benchmark.refresh_hz,
            benchmark.raw,
        );
    }
    if let Some(capture) = options.startup_capture {
        app.init_resource::<PersistenceDisabled>();
        crate::ui::performance::install_startup_capture(&mut app, capture);
    }
    super::platform::install_runtime_diagnostics(&mut app);
    app
}

fn set_primary_window_icon(
    window: Query<Entity, With<PrimaryWindow>>,
    config: Res<GameConfigResource>,
    project: Res<ContentProjectResource>,
    _main_thread: NonSendMarker,
) {
    #[cfg(target_os = "macos")]
    if let Err(error) = set_macos_application_icon() {
        log::warn!("failed to set macOS application icon: {error:#}");
    }

    let Ok(window_entity) = window.single() else {
        return;
    };
    let icon = match load_project_window_icon(&config.0, &project.0).or_else(|error| {
        log::warn!("failed to load project application icon: {error:#}");
        load_window_icon()
    }) {
        Ok(icon) => icon,
        Err(error) => {
            log::warn!("failed to load application icon: {error:#}");
            return;
        }
    };

    WINIT_WINDOWS.with_borrow(|windows| {
        if let Some(window) = windows.get_window(window_entity) {
            window.set_window_icon(Some(icon));
        }
    });
}

#[cfg(target_os = "macos")]
fn set_macos_application_icon() -> Result<()> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    // Native bundles already declare their own project/Editor icon. Do not
    // replace it with the embedded Engine fallback, including playtest Apps.
    if std::env::current_exe()?.parent().is_some_and(|parent| {
        parent.file_name().is_some_and(|name| name == "MacOS")
            && parent
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "Contents")
    }) {
        return Ok(());
    }

    let main_thread =
        MainThreadMarker::new().context("application icon must be set on main thread")?;
    // Preserve all representations when launched directly; a 256 px PNG would
    // replace the packaged application's high-resolution Dock icon.
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/keine.icns"));
    // SAFETY: `NSData` copies exactly `bytes.len()` readable bytes from this
    // process-owned static buffer before returning.
    let data = unsafe { NSData::dataWithBytes_length(bytes.as_ptr().cast(), bytes.len()) };
    let image = NSImage::initWithData(main_thread.alloc(), &data)
        .context("AppKit rejected the embedded ICNS application icon")?;
    let application = NSApplication::sharedApplication(main_thread);
    // SAFETY: This setter is called on AppKit's main thread and retains the
    // supplied NSImage for the application's Dock lifetime.
    unsafe { application.setApplicationIconImage(Some(&image)) };
    Ok(())
}

fn load_window_icon() -> Result<winit::window::Icon> {
    let (rgba, width, height) = decode_window_icon()?;
    winit::window::Icon::from_rgba(rgba, width, height)
        .context("embedded application icon has invalid RGBA data")
}

fn load_project_window_icon(
    config: &GameConfig,
    project: &ContentProject,
) -> Result<winit::window::Icon> {
    use std::io::Read;
    if config.project.icon.is_empty() {
        return load_window_icon();
    }
    let mount = keine_loader::ContentMount::new(
        keine_loader::ContentBackend::FileSystem(project.root.clone()),
        "",
    )?;
    let mut bytes = Vec::new();
    mount
        .open_file(Path::new(&config.project.icon))?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "application icon exceeds 64 MiB"
    );
    let (rgba, width, height) = if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        let mut valid = false;
        let decoded = keine_media::decode_webp(&bytes, |size| {
            valid = size.width == size.height && (32..=4096).contains(&size.width);
            keine_media::ImageSize::new(256, 256)
        })?;
        anyhow::ensure!(valid, "invalid application icon dimensions");
        (decoded.into_pixels(), 256, 256)
    } else {
        anyhow::ensure!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24,
            "application icon must be PNG or WebP"
        );
        let width = u32::from_be_bytes(bytes[16..20].try_into()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into()?);
        anyhow::ensure!(
            width == height && (32..=4096).contains(&width),
            "invalid application icon dimensions"
        );
        let image = Image::from_buffer(
            &bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::default(),
            RenderAssetUsages::MAIN_WORLD,
        )?;
        let pixels = image.try_into_dynamic()?.thumbnail(256, 256).to_rgba8();
        let (width, height) = pixels.dimensions();
        (pixels.into_raw(), width, height)
    };
    winit::window::Icon::from_rgba(rgba, width, height).context("invalid application icon RGBA")
}

fn decode_window_icon() -> Result<(Vec<u8>, u32, u32)> {
    let image = Image::from_buffer(
        include_bytes!(concat!(env!("OUT_DIR"), "/keine-256.png")),
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::MAIN_WORLD,
    )
    .context("failed to decode embedded application icon")?;
    let width = image.texture_descriptor.size.width;
    let height = image.texture_descriptor.size.height;
    let rgba = image
        .data
        .context("embedded application icon has no CPU pixel data")?;
    Ok((rgba, width, height))
}

fn check_project(
    config: &GameConfig,
    content: &ContentProject,
    languages: &keine_loader::ScriptLanguageRegistry,
) -> Result<()> {
    let report = validate_project(config, content, languages)?;
    for diagnostic in &report.diagnostics {
        eprintln!(
            "{}: {}:{}:{}: {}",
            match diagnostic.level {
                keine_authoring::DiagnosticLevel::Warning => "warning",
                keine_authoring::DiagnosticLevel::Error => "error",
            },
            diagnostic.path.display(),
            diagnostic.line,
            diagnostic.column,
            diagnostic.message
        );
    }
    if report.errors > 0 {
        anyhow::bail!(
            "project check failed with {} error(s) and {} warning(s)",
            report.errors,
            report.warnings
        );
    }
    println!(
        "project valid · {} · {} scene(s) · {} action(s) · {} source(s) · {} warning(s)",
        report.title, report.scenes, report.actions, report.sources, report.warnings,
    );
    Ok(())
}

pub(crate) fn validate_project(
    config: &GameConfig,
    content: &ContentProject,
    languages: &keine_loader::ScriptLanguageRegistry,
) -> Result<keine_authoring::ValidationReport> {
    let mut scenes =
        load_scenes_with(content, languages).context("failed to compile project scenes")?;
    if config.adapter.script.eq_ignore_ascii_case("keine") {
        keine_loader::validate_native_entry_flow(&mut scenes, &config.script.entry);
    }
    let mut actions = 0usize;
    let mut warnings = 0usize;
    let mut errors = 0usize;
    let mut diagnostics = Vec::new();
    let mut missing_resources = HashSet::new();
    for scene in &scenes {
        actions += scene.actions.len();
        for diagnostic in &scene.diagnostics {
            let level = match diagnostic.level {
                DiagnosticLevel::Warning => {
                    warnings += 1;
                    keine_authoring::DiagnosticLevel::Warning
                }
                DiagnosticLevel::Error => {
                    errors += 1;
                    keine_authoring::DiagnosticLevel::Error
                }
            };
            diagnostics.push(keine_authoring::Diagnostic {
                level,
                path: scene.path.clone(),
                line: diagnostic.span.line,
                column: diagnostic.span.column,
                message: diagnostic.message.clone(),
            });
        }
        for resource in &scene.resources {
            let path = resource.resolved_path(config);
            if resource.is_dynamic() {
                continue;
            }
            if !missing_resources.insert(path.clone()) {
                continue;
            }
            if !content.contains_asset(Path::new(&path)) {
                errors += 1;
                diagnostics.push(keine_authoring::Diagnostic {
                    level: keine_authoring::DiagnosticLevel::Error,
                    path: scene.path.clone(),
                    line: resource.span.line,
                    column: resource.span.column,
                    message: format!("resource does not exist: {path}"),
                });
            }
        }
    }
    if config.adapter.script.eq_ignore_ascii_case("keine")
        && !scenes.iter().any(|scene| scene.name == config.script.entry)
    {
        errors += 1;
        diagnostics.push(keine_authoring::Diagnostic {
            level: keine_authoring::DiagnosticLevel::Error,
            path: content.root.join("config.yaml"),
            line: 1,
            column: 1,
            message: format!(
                "native script entry scene {:?} does not exist",
                config.script.entry
            ),
        });
    }
    Ok(keine_authoring::ValidationReport {
        title: config.title.clone(),
        scenes: scenes.len(),
        actions,
        sources: content.sources.len(),
        warnings,
        errors,
        diagnostics,
    })
}

#[derive(Debug)]
pub(crate) struct OpenedProject {
    pub(crate) root: PathBuf,
    pub(crate) config: GameConfig,
    pub(crate) content: ContentProject,
    pub(crate) packaged: bool,
}

pub(crate) fn open_project(project_path: &Path, loader: &LoaderRegistry) -> Result<OpenedProject> {
    if let Some(project) = loader.open_project(project_path)? {
        return Ok(OpenedProject {
            root: project.root,
            config: project.config,
            content: project.content,
            packaged: project.format == "hakutaku",
        });
    }

    ensure_project_directory(project_path)?;
    let config_path = project_path.join("config.yaml");
    let bytes = crate::storage::read_limited(&config_path, MAX_PROJECT_CONFIG_BYTES)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let yaml = std::str::from_utf8(&bytes)
        .with_context(|| format!("project config is not UTF-8: {}", config_path.display()))?;
    let mut config = GameConfig::from_yaml(yaml)
        .with_context(|| format!("invalid project config {}", config_path.display()))?;
    let mut content = load_project_with(project_path, &config.adapter.asset, loader)?;
    content.prepare_eiyashou(&mut config)?;
    Ok(OpenedProject {
        root: content.root.clone(),
        config,
        content,
        packaged: false,
    })
}

fn ensure_project_directory(project_path: &Path) -> Result<()> {
    if !project_path.is_dir() {
        anyhow::bail!(
            "project directory does not exist: {}",
            project_path.display()
        );
    }
    let config_path = project_path.join("config.yaml");
    if !config_path.is_file() {
        anyhow::bail!("project config does not exist: {}", config_path.display());
    }
    Ok(())
}

fn bootstrap_project(
    mut commands: Commands,
    persistence_root: Res<PersistenceRoot>,
    content: Res<ContentProjectResource>,
    languages: Res<ScriptLanguages>,
    config: Res<GameConfigResource>,
    mode: BootstrapMode,
) {
    spawn_cameras(
        &mut commands,
        mode.benchmark.as_ref().map_or(
            crate::ui::performance::BenchmarkCameras::Runtime,
            |capture| capture.cameras,
        ),
    );

    let mut state = State::new();
    if config.adapter.script.eq_ignore_ascii_case("keine") {
        state.script_entry = Some(config.script.entry.clone());
    }
    match content.initial_state() {
        Ok(initial) => {
            state.vars = initial.variables;
            state.session_variable_names = initial.session_variables.keys().cloned().collect();
            state.vars.extend(initial.session_variables);
            state.global_vars = initial.shared_variables;
        }
        Err(error) => log::error!("failed to load project variable defaults: {error:#}"),
    }
    if mode.editor_sync.is_none() || mode.authoring_preview.is_some() {
        state
            .global_vars
            .extend(crate::storage::profile::load(&persistence_root));
        crate::storage::gallery::load(&mut state, &persistence_root);
        state.read_dialogues = crate::storage::read_history::load(&persistence_root);
    }
    let read_history_count = state.read_dialogues.len();
    let mut scene_count = 0;
    let mut action_count = 0;
    let mut manifest = LocalAssetManifest::default();
    match load_startup_scenes_with(&content, &languages) {
        Ok(mut scenes) => {
            if config.adapter.script.eq_ignore_ascii_case("keine") {
                keine_loader::validate_native_entry_flow(&mut scenes, &config.script.entry);
            }
            let reject_native_program = config.adapter.script.eq_ignore_ascii_case("keine")
                && (!scenes.iter().any(|scene| scene.name == config.script.entry)
                    || scenes.iter().any(|scene| {
                        scene
                            .diagnostics
                            .iter()
                            .any(|diagnostic| diagnostic.level == DiagnosticLevel::Error)
                    }));
            let mut program_scenes = Vec::with_capacity(scenes.len());
            for scene in scenes {
                scene_count += 1;
                action_count += scene.actions.len();
                for diagnostic in &scene.diagnostics {
                    let message = format!(
                        "{}:{}:{}: {}",
                        scene.path.display(),
                        diagnostic.span.line,
                        diagnostic.span.column,
                        diagnostic.message
                    );
                    match diagnostic.level {
                        DiagnosticLevel::Warning => log::warn!("{message}"),
                        DiagnosticLevel::Error => log::error!("{message}"),
                    }
                }
                manifest.insert(
                    scene.name.clone(),
                    LocalSceneAssets {
                        source_path: scene.path.clone(),
                        resources: scene.resources,
                        sub_scenes: scene.sub_scenes,
                        action_spans: scene.action_spans,
                    },
                );
                program_scenes.push((scene.name, scene.actions));
            }
            if reject_native_program {
                log::error!(
                    target: "keine::runtime",
                    "native script diagnostics contain errors; refusing to install the authored Program"
                );
            } else {
                state.install_program(Program::from_scenes(program_scenes));
            }
        }
        Err(error) => log::error!("failed to load scripts: {error:#}"),
    }
    ensure_playable_scene(&mut state);
    #[cfg(feature = "startup-metrics")]
    if mode.benchmark.is_some()
        && let Err(error) = crate::ui::performance::source_map::restore(
            &content,
            state.program_fingerprint,
            &mut manifest,
        )
    {
        log::error!(target: "keine::performance", "invalid benchmark source map: {error:#}");
        commands.insert_resource(crate::ui::performance::InvalidCaptureTarget);
    }
    if mode.editor_sync.is_some() {
        // An editor is already the outer shell. Enter its current selected block directly
        // so the native overlay never flashes keine's title screen first.
        state.ended = false;
        if !crate::runtime::tick::sync_editor_cursor(&content, &mut state, &manifest) {
            crate::runtime::script_driver::resume_for_tooling(&mut state);
        }
    } else if mode.benchmark.is_some() {
        // Runtime captures start on the actual stage, not the comparatively
        // cheap title screen, and never require synthetic keyboard input.
        state.ended = false;
        crate::runtime::script_driver::resume_for_tooling(&mut state);
        let timelines = benchmark_timelines(&state)
            .into_iter()
            .map(|(scene, index, timeline)| format!("{scene}:{index}:{timeline}"))
            .collect::<Vec<_>>()
            .join(", ");
        log::info!(target: "keine::performance", "TIMELINE | {timelines}");
        let requested_target = mode
            .benchmark
            .as_ref()
            .and_then(|capture| capture.target.as_ref());
        let resolved_target =
            requested_target.and_then(|target| resolve_benchmark_target(&state, target));
        if requested_target.is_some() && resolved_target.is_none() {
            log::error!(target: "keine::performance", "benchmark target {requested_target:?} does not exist or is ambiguous; available timelines: {timelines}");
            commands.insert_resource(crate::ui::performance::InvalidCaptureTarget);
        }
        if let Some((target_scene, cursor)) = &resolved_target {
            let new_preview = || State {
                program: state.program.clone(),
                program_fingerprint: state.program_fingerprint,
                script_entry: state.script_entry.clone(),
                vars: state.vars.clone(),
                global_vars: state.global_vars.clone(),
                ..State::new()
            };
            let mut preview = new_preview();
            preview.current_scene = crate::scene::entry_scene(&preview);
            preview.ended = false;
            if crate::runtime::tick::seek_editor_state(
                &mut preview,
                target_scene,
                *cursor,
                cursor.saturating_add(1),
            ) || {
                // Benchmark fixtures may live in a dedicated fragment that is
                // deliberately unreachable from the playable acceptance flow.
                // Reconstruct that fragment directly, matching editor preview
                // behavior, so benchmark-only content stays out of the story.
                preview = new_preview();
                preview.current_scene.clone_from(target_scene);
                preview.ended = false;
                crate::runtime::tick::seek_editor_state(
                    &mut preview,
                    target_scene,
                    *cursor,
                    cursor.saturating_add(1),
                )
            } {
                state = preview;
            } else {
                log::error!(target: "keine::performance", "benchmark cursor {cursor} could not be replayed in {target_scene:?}");
                commands.insert_resource(crate::ui::performance::InvalidCaptureTarget);
            }
            if mode.benchmark.as_ref().is_some_and(|c| c.continuous)
                && let Some(animation) = state.stage_animation.as_mut()
            {
                // A selected timeline is looped only inside the benchmark so
                // the sample measures its sustained cost instead of mostly
                // measuring the static frame after a short authored clip.
                animation.animation.infinite = true;
                animation.animation.repeat = 0;
            }
        }
        log::info!(
            target: "keine::performance",
            "START    | requested target {:?} · resolved cursor {:?} · running cursor {} · timeline {}",
            requested_target,
            resolved_target
                .as_ref()
                .map(|(scene, cursor)| format!("{scene}:{cursor}")),
            state.cursor,
            state
                .stage_animation
                .as_ref()
                .map_or("none", |animation| animation.animation.id.as_str()),
        );
    } else {
        // Normal binaries prepare the entry scene, but execution belongs to
        // the title screen's START action.
        state.ended = true;
    }
    log::info!(
        "project ready · {} · {scene_count} scene(s) · {action_count} action(s) · {} source(s)",
        config.title,
        content.sources.len(),
    );
    let profile_writer = crate::storage::profile::ProfileWriter::loaded(&state.global_vars);
    let gallery_snapshot = crate::storage::gallery::GallerySnapshot::loaded(&state);
    let mut image_roles = crate::scene::images::ImageRoleRegistry::default();
    image_roles.rebuild(&config, &manifest);
    commands.insert_resource(GameState(state));
    commands.insert_resource(crate::storage::read_history::ReadHistoryWriter::loaded(
        read_history_count,
    ));
    commands.insert_resource(profile_writer);
    commands.insert_resource(gallery_snapshot);
    commands.insert_resource(manifest);
    commands.insert_resource(image_roles);
    commands.insert_resource(LocalAssetCache::default());

    #[cfg(feature = "hot-reload")]
    if mode.hot_reload.is_some() {
        match ScriptWatcher::start_for_project(&content, languages.0.clone()) {
            Ok(watcher) => {
                commands.insert_resource(ScriptWatcherResource(Mutex::new(watcher)));
            }
            Err(error) => log::warn!("script hot reload disabled: {error:#}"),
        }
    }
}

fn spawn_cameras(commands: &mut Commands, cameras: crate::ui::performance::BenchmarkCameras) {
    // All three layers share one single-sample target. A later camera's MSAA
    // resolve would overwrite the regional blur and UI already composited there.
    commands.spawn((
        Name::new("scene_camera"),
        Camera2d,
        Msaa::Off,
        Camera {
            order: 0,
            is_active: cameras.scene(),
            ..default()
        },
        RenderLayers::layer(0),
        BlurCamera::default(),
        CompositedCameraEffects::default(),
        SceneBlurCamera,
    ));
    commands.spawn((
        Name::new("ui_camera"),
        Camera2d,
        Msaa::Off,
        Camera {
            order: 1,
            is_active: cameras.ui(),
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(1),
        BlurCamera::default(),
        UiBlurCamera,
    ));
    commands.spawn((
        Name::new("dialog_camera"),
        Camera2d,
        Msaa::Off,
        Camera {
            order: 2,
            is_active: cameras.dialog(),
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(2),
        DialogCamera,
    ));
}

fn ensure_playable_scene(state: &mut State) {
    if state.program.is_empty() {
        state.insert_scene(
            "main".into(),
            vec![
                Action::ShowBg {
                    image: "bg.webp".into(),
                    transition: Default::default(),
                    transform: Default::default(),
                },
                Action::Say {
                    speaker: "keine".into(),
                    text: "No script found.".into(),
                    options: Default::default(),
                },
            ],
        );
    }

    state.current_scene = crate::scene::entry_scene(state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn layered_cameras_preserve_the_single_sample_post_process_target() {
        for profile in [
            BenchmarkCameras::Runtime,
            BenchmarkCameras::SceneUi,
            BenchmarkCameras::SceneDialog,
            BenchmarkCameras::SceneOnly,
        ] {
            let mut app = App::new();
            // Match the renderer's implicit default so an omitted override
            // cannot pass merely because this test has no RenderPlugin.
            app.register_required_components::<Camera, Msaa>();
            app.add_systems(Startup, move |mut commands: Commands| {
                spawn_cameras(&mut commands, profile);
            });
            app.update();
            let world = app.world_mut();
            let mut cameras = world.query::<(&Camera, &Msaa)>();
            assert_eq!(cameras.iter(world).count(), 3);
            for (camera, msaa) in cameras.iter(world) {
                assert_eq!(*msaa, Msaa::Off, "{profile:?}, layer {}", camera.order);
            }
        }
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("keine-{name}-{}-{nonce}", std::process::id()))
    }

    #[test]
    #[ignore = "constructs the real native Preview renderer; run on a local GPU"]
    fn authoring_preview_uses_an_isolated_persistence_root() {
        let overlay = unique_temp_path("preview-data-root");
        std::fs::create_dir_all(&overlay).unwrap();
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-smoke");
        let app = build_authoring_preview_app(
            &project,
            &overlay,
            &LoaderRegistry::default(),
            super::super::preview::AuthoringPreviewConfig {
                document_revision: 0,
            },
        )
        .unwrap();

        assert_eq!(
            app.world().resource::<PersistenceRoot>().0,
            overlay.join("preview-data")
        );
        assert!(app.world().contains_resource::<EditorSyncSession>());
        assert!(
            app.world()
                .contains_resource::<super::super::preview::AuthoringPreviewSession>()
        );
        assert!(!app.world().contains_resource::<PersistenceDisabled>());
        assert!(!project.join("preview-data").exists());

        drop(app);
        std::fs::remove_dir_all(overlay).unwrap();
    }

    #[test]
    fn missing_project_is_rejected_without_creating_it() {
        let path = unique_temp_path("missing-project");
        assert!(!path.exists());

        let error = open_project(&path, &LoaderRegistry::default()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("project directory does not exist")
        );
        assert!(!path.exists());
    }

    #[test]
    fn project_without_config_is_rejected_without_scaffolding() {
        let path = unique_temp_path("missing-config");
        std::fs::create_dir_all(&path).unwrap();

        let error = open_project(&path, &LoaderRegistry::default()).unwrap_err();

        assert!(error.to_string().contains("project config does not exist"));
        assert!(!path.join("scripts").exists());
        assert!(!path.join("assets").exists());
        std::fs::remove_dir(&path).unwrap();
    }

    #[test]
    fn oversized_project_config_is_rejected_before_reading_its_payload() {
        let path = unique_temp_path("oversized-config");
        std::fs::create_dir_all(&path).unwrap();
        let config = std::fs::File::create(path.join("config.yaml")).unwrap();
        config.set_len(MAX_PROJECT_CONFIG_BYTES as u64 + 1).unwrap();

        let error = open_project(&path, &LoaderRegistry::default()).unwrap_err();

        assert!(format!("{error:#}").contains("exceeding the 262144-byte limit"));
        std::fs::remove_dir_all(&path).unwrap();
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn startup_summary_separates_first_launch_from_repeat_median() {
        let sample = |app_ms, interactive_ms| crate::ui::performance::StartupSample {
            project_ms: 1.0,
            app_ms,
            first_frame_ms: interactive_ms - 5.0,
            interactive_ms,
            peak_rss_mib: Some(200.0),
        };
        let mut report = String::new();

        append_startup_summary(
            &[
                sample(1_400.0, 1_600.0),
                sample(120.0, 280.0),
                sample(140.0, 300.0),
                sample(130.0, 290.0),
            ],
            &mut report,
        );

        assert!(report.contains("first run / repeat median"));
        assert!(report.contains("app built   · 1400.00 / 130.00 ms"));
        assert!(report.contains("interactive · 1600.00 / 290.00 ms"));
    }

    #[test]
    fn portable_benchmark_keeps_daily_coverage_and_stress_groups_distinct() {
        let targets = PORTABLE_BENCHMARK_SECTIONS
            .iter()
            .flat_map(|(_, workloads)| workloads.iter().map(|(_, target)| *target))
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(DAILY_BENCHMARK_WORKLOADS.len(), 3);
        assert_eq!(FEATURE_BENCHMARK_WORKLOADS.len(), 8);
        assert_eq!(CLASSIC_ATTRIBUTION_BENCHMARK_WORKLOADS.len(), 4);
        assert_eq!(STRESS_BENCHMARK_WORKLOADS.len(), 1);
        assert_eq!(REPEATED_BENCHMARK_TARGETS.len(), 11);
        assert_eq!(HOTSPOT_BENCHMARK_RUNS, 3);
        assert_eq!(benchmark_workload_runs("bench_blur_family"), 3);
        assert_eq!(benchmark_workload_runs("bench_classic_godray"), 3);
        assert_eq!(benchmark_workload_runs("bench_shared_transform_clock"), 1);
        assert_eq!(targets.len(), 71);
        assert_eq!(CAMERA_BENCHMARK_WORKLOADS.len(), 3);
        assert!(
            CAMERA_BENCHMARK_WORKLOADS
                .iter()
                .all(|(_, cameras)| *cameras != BenchmarkCameras::Runtime)
        );
        assert_eq!(
            CAMERA_BENCHMARK_WORKLOADS
                .iter()
                .map(|(_, cameras)| cameras.id())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            CAMERA_BENCHMARK_WORKLOADS.len()
        );
    }

    #[test]
    fn portable_benchmark_fixture_resolves_every_required_workload() {
        let loader = LoaderRegistry::default();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-benchmark");
        let project = open_project(&root, &loader).unwrap();
        let languages = loader.languages(&project.config.adapter.script).unwrap();
        let report = validate_project(&project.config, &project.content, &languages).unwrap();
        assert_eq!(
            (report.errors, report.warnings),
            (0, 0),
            "{:?}",
            report.diagnostics
        );
        assert!(project.config.assets.luts.contains_key("cinematic"));
        assert!(project.content.contains_asset(Path::new(
            &project.config.bg_path(&project.config.title_background)
        )));
        let scenes = load_scenes_with(&project.content, &languages).unwrap();
        let mut state = State::new();
        state.install_program(Program::from_scenes(
            scenes.into_iter().map(|s| (s.name, s.actions)),
        ));
        let required = PORTABLE_BENCHMARK_SECTIONS
            .iter()
            .flat_map(|(_, w)| w.iter().map(|(_, t)| *t));
        for target in required {
            let (scene, cursor) =
                resolve_benchmark_target(&state, &BenchmarkTarget::Timeline(target.into()))
                    .unwrap_or_else(|| panic!("missing or duplicate target: {target}"));
            let mut preview = State::new();
            preview.program = state.program.clone();
            preview.program_fingerprint = state.program_fingerprint;
            preview.current_scene = scene.clone();
            preview.ended = false;
            assert!(
                crate::runtime::tick::seek_editor_state(&mut preview, &scene, cursor, cursor + 1),
                "{target}"
            );
            assert!(
                preview.stage_animation.is_some(),
                "{target} is not an active native workload"
            );
        }
        // Transparent calibration art must exercise sprites, not invisible placeholders.
        let image = keine_media::decode_webp(
            &std::fs::read(root.join("assets/portrait.webp")).unwrap(),
            |size| size,
        )
        .unwrap();
        assert_eq!(image.size(), keine_media::ImageSize::new(720, 1440));
        let pixels = image.into_pixels();
        assert!(pixels.chunks_exact(4).any(|p| p[3] == 0));
        assert!(pixels.chunks_exact(4).any(|p| p[3] == 255));
    }

    #[test]
    fn portable_benchmark_ranking_does_not_fabricate_a_reference() {
        let summary = |ms| crate::ui::performance::RenderSummary {
            frames: 100,
            average_ms: ms,
            p50_ms: ms,
            p95_ms: ms,
            p99_ms: ms,
            maximum_ms: ms,
            average_fps: 1000.0 / ms,
            one_percent_low_fps: 1000.0 / ms,
            p99_equivalent_fps: 1000.0 / ms,
        };
        let mut report = String::new();
        append_hotspot_ranking(
            &[("particle", "bench_particle_snow_256", summary(20.0))],
            &mut report,
        );
        assert!(report.contains("reference unavailable"));
        assert!(!report.contains("Δavg"));
        report.clear();
        append_hotspot_ranking(
            &[
                ("baseline", "bench_baseline", summary(5.0)),
                ("particle", "bench_particle_snow_256", summary(20.0)),
                ("blur", "bench_effect_blur", summary(10.0)),
            ],
            &mut report,
        );
        assert!(report.contains("Δavg +15.00 ms"));
        assert!(
            report.find("HOTSPOT  | particle").unwrap() < report.find("HOTSPOT  | blur").unwrap()
        );
        assert!(report.contains("--timeline \"bench_particle_snow_256\""));
    }

    #[test]
    fn benchmark_report_lines_are_plain_utf8() {
        assert_eq!(
            plain_report_line("\u{1b}[2m0.1s\u{1b}[0m \u{1b}[32mINFO\u{1b}[0m Kēne"),
            "0.1s INFO Kēne"
        );
        assert_eq!(plain_report_line("普通文本"), "普通文本");
        assert!(benchmark_report_line(
            "0.1s INFO keine::performance: SLOW     | t=3.400s · 393.01 ms"
        ));
        for system in ["Wayland", "X11 (native or XWayland)", "unavailable"] {
            assert!(benchmark_report_line(&format!(
                "0.1s INFO keine::platform: WINDOWSYS | {system} · actual primary window handle"
            )));
        }
    }

    #[test]
    fn raw_frame_appendix_sanitizes_tab_separators() {
        let mut output = String::new();
        append_raw_frame(
            &mut output,
            "blur\tfamily",
            2,
            3,
            Some("bench_blur_family"),
            BenchmarkCameras::Runtime,
            "3.500000\t121\t16.750000\t6\t8.33\tchapter2\t8\tscripts/2.shou\tIdle\t12\tfalse\t1920\t1080\tsleep",
        );
        assert_eq!(
            output,
            "RAWFRAME\tblur family\t2\t3\tbench_blur_family\truntime\t3.500000\t121\t16.750000\t6\t8.33\tchapter2\t8\tscripts/2.shou\tIdle\t12\tfalse\t1920\t1080\tsleep\n"
        );
    }

    #[test]
    fn build_identity_is_available_to_portable_reports() {
        assert!(!env!("KEINE_BUILD_COMMIT").is_empty());
        assert!(!env!("KEINE_BUILD_TIME").is_empty());
    }

    #[test]
    fn portable_benchmark_matches_only_complete_authored_timeline_ids() {
        let inventory = "0.1s INFO keine::performance: TIMELINE | intro:2:bench_representative_dialogue, coverage:8:bench_blur_family\n";

        assert!(timeline_is_available(
            inventory,
            "bench_representative_dialogue"
        ));
        assert!(timeline_is_available(inventory, "bench_blur_family"));
        assert!(!timeline_is_available(inventory, "blur family"));
        assert!(!timeline_is_available("TIMELINE | ", "anything"));
    }

    #[test]
    #[cfg(feature = "hot-reload")]
    fn parser_keeps_each_commands_project_and_options_together() {
        let CliCommand::Validate { project } = parse_cli(&args(&["validate", "project"])).unwrap()
        else {
            panic!("expected validate command");
        };
        assert_eq!(project, Path::new("project"));

        let CliCommand::Run {
            project,
            mode: InteractiveMode::Development,
            editor_sync,
        } = parse_cli(&args(&["dev", "editor-project", "--sync"])).unwrap()
        else {
            panic!("expected development command");
        };
        assert_eq!(project, Path::new("editor-project"));
        assert!(editor_sync);
    }

    #[test]
    #[cfg(not(feature = "hot-reload"))]
    fn release_surface_rejects_the_uncompiled_development_watcher() {
        let error = parse_cli(&args(&["dev", "project"])).unwrap_err();
        assert!(error.to_string().contains("not compiled"));
    }

    #[test]
    #[cfg(feature = "hot-reload")]
    fn only_non_development_interactive_modes_require_a_process_lock() {
        assert!(InteractiveMode::Shipping.requires_single_instance());
        assert!(!InteractiveMode::Development.requires_single_instance());
        assert!(
            !InteractiveMode::Benchmark(BenchmarkOptions {
                seconds: 1.0,
                window: BenchmarkWindow::Default,
                continuous: false,
                raw: false,
                refresh_hz: None,
                target: None,
                cameras: crate::ui::performance::BenchmarkCameras::Runtime,
            })
            .requires_single_instance()
        );
        assert!(
            !InteractiveMode::StartupBenchmark(crate::runtime::cli::StartupBenchmarkOptions {
                runs: 7
            })
            .requires_single_instance()
        );
    }

    #[test]
    #[cfg(feature = "hot-reload")]
    fn only_shipping_runs_use_the_native_startup_error_page() {
        let shipping = parse_cli(&args(&["game.haku"])).unwrap();
        let development = parse_cli(&args(&["dev", "project"])).unwrap();
        let check = parse_cli(&args(&["validate", "project"])).unwrap();

        assert!(shipping.uses_startup_error_page());
        assert!(!development.uses_startup_error_page());
        assert!(!check.uses_startup_error_page());
    }

    #[test]
    fn instance_lock_is_released_with_its_guard() {
        let root = unique_temp_path("instance-lock");
        std::fs::create_dir_all(&root).unwrap();
        let path = instance_lock_path(&root);
        let first = SingleInstanceGuard::acquire(&root).unwrap();
        assert!(SingleInstanceGuard::acquire(&root).is_err());
        drop(first);
        assert!(SingleInstanceGuard::acquire(&root).is_ok());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn benchmark_command_has_repeatable_defaults() {
        let CliCommand::Run {
            mode: InteractiveMode::Benchmark(options),
            ..
        } = parse_cli(&args(&["perf", "/tmp/project"])).unwrap()
        else {
            panic!("expected benchmark command");
        };
        assert_eq!(options.seconds, 15.0);
        assert_eq!(options.target, None);
        assert_eq!(
            options.cameras,
            crate::ui::performance::BenchmarkCameras::Runtime
        );
    }

    #[test]
    fn startup_benchmark_command_has_bounded_repeatable_runs() {
        let CliCommand::Run {
            mode: InteractiveMode::StartupBenchmark(options),
            ..
        } = parse_cli(&args(&["perf", "/tmp/project", "--startup"])).unwrap()
        else {
            panic!("expected startup benchmark command");
        };
        assert_eq!(options.runs, 7);
        assert!(parse_cli(&args(&["perf", "/tmp/project", "--startup", "--runs", "0"])).is_err());
        assert!(
            parse_cli(&args(&[
                "perf",
                "/tmp/project",
                "--startup",
                "--runs",
                "51"
            ]))
            .is_err()
        );
    }

    #[test]
    fn benchmark_runtime_defaults_and_invalid_modes_do_not_change_workload() {
        let CliCommand::Run {
            mode: InteractiveMode::Benchmark(options),
            ..
        } = parse_cli(&args(&[
            "perf",
            "/tmp/project",
            "--scene",
            "chapter2",
            "--cursor",
            "7",
            "--hz",
            "120",
        ]))
        .unwrap()
        else {
            panic!("benchmark");
        };
        assert!(!options.continuous);
        assert_eq!(options.refresh_hz, Some(120.0));
        assert_eq!(
            options.target,
            Some(BenchmarkTarget::SceneCursor("chapter2".into(), 7))
        );
        for invalid in [
            vec!["--window", "0x1080"],
            vec!["--window", "1920x9000"],
            vec!["--window", "1080"],
            vec!["--startup", "--window", "fullscreen"],
            vec!["--hz", "NaN"],
            vec!["--hz", "0"],
            vec!["--seconds", "3601"],
            vec!["--mode", "unknown"],
            vec!["--camera", "scene"],
            vec!["--startup", "--mode", "runtime"],
            vec!["--scene", "x", "--timeline", "y"],
        ] {
            let mut command = vec!["perf", "/tmp/project"];
            command.extend(invalid);
            assert!(parse_cli(&args(&command)).is_err(), "{command:?}");
        }
    }

    #[test]
    fn benchmark_window_accepts_size_and_fullscreen() {
        for (value, expected) in [
            ("1920x1080", BenchmarkWindow::Size(1920, 1080)),
            ("fullscreen", BenchmarkWindow::Fullscreen),
        ] {
            let CliCommand::Run {
                mode: InteractiveMode::Benchmark(options),
                ..
            } = parse_cli(&args(&["perf", "/tmp/project", "--window", value])).unwrap()
            else {
                panic!("benchmark");
            };
            assert_eq!(options.window, expected);
        }
    }

    #[test]
    fn benchmark_missing_scene_or_out_of_range_cursor_is_rejected() {
        let mut state = State::new();
        state.insert_scene("first".into(), vec![Action::Wait { seconds: 1.0 }]);
        state.current_scene = "first".into();
        assert_eq!(
            resolve_benchmark_target(&state, &BenchmarkTarget::SceneCursor("first".into(), 0)),
            Some(("first".into(), 0))
        );
        assert_eq!(
            resolve_benchmark_target(&state, &BenchmarkTarget::SceneCursor("missing".into(), 0)),
            None
        );
        assert_eq!(
            resolve_benchmark_target(&state, &BenchmarkTarget::Cursor(1)),
            None
        );
    }

    #[test]
    fn benchmark_command_accepts_duration_and_cursor() {
        let CliCommand::Run {
            mode: InteractiveMode::Benchmark(options),
            ..
        } = parse_cli(&args(&[
            "perf",
            "/tmp/project",
            "--seconds",
            "7.5",
            "--cursor",
            "25",
        ]))
        .unwrap()
        else {
            panic!("expected benchmark command");
        };
        assert_eq!(options.seconds, 7.5);
        assert_eq!(options.target, Some(BenchmarkTarget::Cursor(25)));
        assert_eq!(
            options.cameras,
            crate::ui::performance::BenchmarkCameras::Runtime
        );
    }

    #[test]
    fn benchmark_command_accepts_stable_timeline_name() {
        let CliCommand::Run {
            mode: InteractiveMode::Benchmark(options),
            ..
        } = parse_cli(&args(&[
            "perf",
            "/tmp/project",
            "--seconds",
            "7.5",
            "--timeline",
            "10-04-blur-family",
        ]))
        .unwrap()
        else {
            panic!("expected benchmark command");
        };
        assert_eq!(
            options.target,
            Some(BenchmarkTarget::Timeline("10-04-blur-family".into()))
        );
    }

    #[test]
    fn benchmark_command_accepts_camera_profile() {
        let CliCommand::Run {
            mode: InteractiveMode::Benchmark(options),
            ..
        } = parse_cli(&args(&[
            "perf",
            "/tmp/project",
            "--seconds",
            "7.5",
            "--cursor",
            "25",
            "--mode",
            "continuous",
            "--camera",
            "scene-ui",
        ]))
        .unwrap()
        else {
            panic!("expected benchmark command");
        };
        assert_eq!(
            options.cameras,
            crate::ui::performance::BenchmarkCameras::SceneUi
        );
    }

    #[test]
    fn benchmark_command_accepts_camera_profile_without_a_target() {
        let CliCommand::Run {
            mode: InteractiveMode::Benchmark(options),
            ..
        } = parse_cli(&args(&[
            "perf",
            "/tmp/project",
            "--seconds",
            "7.5",
            "--mode",
            "continuous",
            "--camera",
            "scene-dialog",
        ]))
        .unwrap()
        else {
            panic!("expected benchmark command");
        };
        assert_eq!(options.target, None);
        assert_eq!(
            options.cameras,
            crate::ui::performance::BenchmarkCameras::SceneDialog
        );
    }

    #[test]
    fn benchmark_command_rejects_zero_duration() {
        assert!(parse_cli(&args(&["perf", "/tmp/project", "--seconds", "0"])).is_err());
    }

    #[test]
    fn parser_rejects_removed_commands_and_ignored_arguments() {
        assert!(parse_cli(&args(&["assets", "--pack", "project"])).is_err());
        assert!(parse_cli(&args(&["configure", "project"])).is_err());
        assert!(parse_cli(&args(&["remap-assets", "project", "wav=opus"])).is_err());
        assert!(parse_cli(&args(&["validate", "project", "ignored"])).is_err());
        assert!(parse_cli(&args(&["dev", "project", "--sync", "--sync"])).is_err());
        assert!(parse_cli(&args(&["perf", "project", "--startup", "--seconds", "5"])).is_err());
        assert!(parse_cli(&args(&["perf", "project", "--runs", "2"])).is_err());
    }

    #[test]
    fn remap_accepts_rules_and_explicit_yes() {
        let CliCommand::Remap {
            project,
            rules,
            yes,
        } = parse_cli(&args(&[
            "remap",
            "/tmp/project",
            "wav=opus",
            "png=webp",
            "-y",
        ]))
        .unwrap()
        else {
            panic!("expected asset remap command");
        };
        assert_eq!(project, PathBuf::from("/tmp/project"));
        assert_eq!(
            rules,
            vec![
                ("wav".to_owned(), "opus".to_owned()),
                ("png".to_owned(), "webp".to_owned())
            ]
        );
        assert!(yes);
    }

    #[test]
    fn remap_requires_rules_and_rejects_duplicate_yes() {
        assert!(parse_cli(&args(&["remap", "/tmp/project"])).is_err());
        assert!(parse_cli(&args(&["remap", "/tmp/project", "wav=opus", "-y", "-y",])).is_err());
    }

    #[test]
    fn version_keeps_the_established_uppercase_short_option() {
        assert!(help_or_version(&args(&["-V"])).is_some());
        assert!(help_or_version(&args(&["--version"])).is_some());
        assert!(help_or_version(&args(&["-v"])).is_none());
        assert!(parse_cli(&args(&["-v"])).is_err());
    }

    #[test]
    fn embedded_window_icon_is_valid_rgba() {
        let (rgba, width, height) = decode_window_icon().unwrap();
        assert_eq!((width, height), (256, 256));
        assert_eq!(rgba.len(), width as usize * height as usize * 4);
    }
}
