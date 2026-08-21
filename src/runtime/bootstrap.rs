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
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy::winit::WINIT_WINDOWS;
use keine_core::config::GameConfig;
use keine_core::{Action, DESIGN_HEIGHT, DESIGN_WIDTH, Program, State};
#[cfg(feature = "hot-reload")]
use keine_loader::ScriptWatcher;
use keine_loader::{
    ContentProject, DiagnosticLevel, LoaderRegistry, ResourceKind, load_project_with,
    load_scenes_with,
};

use crate::render::blur::{BlurCamera, BlurPlugin, DialogCamera, SceneBlurCamera, UiBlurCamera};
use crate::runtime::GamePlugin;
use crate::runtime::cli::{
    BenchmarkOptions, CliCommand, InteractiveMode, help_or_version, packaged_benchmark_command,
    parse as parse_cli, resolve_project_path,
};
use crate::runtime::resources::{
    ContentProjectResource, DevelopmentSession, EditorSyncSession, GameConfigResource, GameState,
    LocalAssetCache, LocalAssetManifest, LocalSceneAssets, PersistenceDisabled, ProjectRoot,
    ScriptLanguages, StoreCodec,
};
#[cfg(feature = "hot-reload")]
use crate::runtime::resources::{HotReloadSession, ScriptWatcherResource};
use crate::ui::performance::BenchmarkTarget;

pub(crate) const MAX_PROJECT_CONFIG_BYTES: usize = 256 * 1024;
type BenchmarkWorkload = (&'static str, &'static str);
type BenchmarkSection = (&'static str, &'static [BenchmarkWorkload]);

const DAILY_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    (
        "representative dialogue · full composition",
        "benchmark representative dialogue",
    ),
    (
        "representative portrait motion · full composition",
        "benchmark representative portrait motion",
    ),
    (
        "representative scene transition · full composition",
        "benchmark representative scene transition",
    ),
];
const FEATURE_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    ("shared transforms", "10-01 shared transform clock"),
    ("classic camera", "10-02 classic camera properties"),
    ("optical effects", "10-03 optical effects"),
    ("blur family", "10-04 blur family"),
    ("atmosphere effects", "10-05 atmosphere effects"),
    ("retro and mask effects", "10-06 retro and eyelid mask"),
    ("timed event types", "10-07 all event types"),
    ("playback controls", "10-08 playback options"),
];
const STRESS_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] =
    &[("stress composition", "benchmark stress composition")];
const PORTABLE_BENCHMARK_SECTIONS: &[BenchmarkSection] = &[
    (
        "daily workloads · representative player-facing actions",
        DAILY_BENCHMARK_WORKLOADS,
    ),
    (
        "feature coverage · every authored timeline property and event family",
        FEATURE_BENCHMARK_WORKLOADS,
    ),
    (
        "stress workload · intentionally combined peak load",
        STRESS_BENCHMARK_WORKLOADS,
    ),
];

#[derive(Clone, Default)]
struct LaunchOptions {
    development: bool,
    editor_sync: bool,
    benchmark: Option<BenchmarkOptions>,
    startup_capture: Option<crate::ui::performance::StartupCapture>,
    hidden_window: bool,
    video: crate::scene::video::VideoSelection,
}

#[derive(SystemParam)]
struct BootstrapMode<'w> {
    editor_sync: Option<Res<'w, EditorSyncSession>>,
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
    let loader = LoaderRegistry::default();
    #[cfg(feature = "configure")]
    let mut loader = loader;
    #[cfg(feature = "configure")]
    let configure_engine = matches!(&command, CliCommand::Configure);
    #[cfg(feature = "configure")]
    let result = if configure_engine {
        super::configure::configure(&loader)
    } else {
        super::configure::apply_saved_configuration(&mut loader)
            .and_then(|video| execute_command(loader, command, video, process_started))
    };
    #[cfg(not(feature = "configure"))]
    let result = execute_command(
        loader,
        command,
        crate::scene::video::VideoSelection::Automatic,
        process_started,
    );

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            #[cfg(feature = "configure")]
            let stage = if configure_engine {
                "failed to configure engine"
            } else {
                "failed to open project"
            };
            #[cfg(not(feature = "configure"))]
            let stage = "failed to open project";
            report_startup_error(uses_startup_error_page, stage, &error);
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
    let result = parsed.and_then(|command| {
        execute_command(
            loader,
            command,
            crate::scene::video::VideoSelection::Automatic,
            process_started,
        )
    });
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
    video: crate::scene::video::VideoSelection,
    process_started: Instant,
) -> Result<()> {
    #[cfg(feature = "hardened")]
    super::platform::apply_hardening();
    let (project_path, action) = match command {
        #[cfg(feature = "configure")]
        CliCommand::Configure => {
            anyhow::bail!("engine configuration must run before project setup")
        }
        CliCommand::AssetsPack { project, output } => {
            #[cfg(feature = "publisher")]
            return crate::publisher::pack_project(
                &resolve_project_path(project),
                &loader,
                &output,
            );
            #[cfg(not(feature = "publisher"))]
            {
                let _ = (project, output);
                anyhow::bail!(
                    "publisher tools are not compiled; run `cargo assets --pack <project>`"
                );
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
        CliCommand::RemapAssets {
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
                anyhow::bail!("asset migration tools are not compiled; run `cargo assets --help`");
            }
        }
        CliCommand::Check { project } => (project, ProjectAction::Check),
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
    let (project_root, config, content) = open_project(&project_path, &loader)?;
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
    let _instance = mode.requires_single_instance().then(|| {
        SingleInstanceGuard::acquire(&project_root)
            .context("another instance of this project is already running")
    });
    let _instance = _instance.transpose()?;
    let mut app = build_opened_app(
        project_root,
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
            video,
        },
    );
    if let Some(capture) = startup_capture {
        capture.mark_app_built();
    }
    app.run();
    Ok(())
}

const STARTUP_BENCHMARK_CHILD_ENV: &str = "KEINE_STARTUP_BENCHMARK_CHILD";
const RUNTIME_BENCHMARK_CHILD_ENV: &str = "KEINE_RUNTIME_BENCHMARK_CHILD";

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
    for run in 1..=runs {
        let output = Command::new(&executable)
            .arg("benchmark-startup")
            .arg(project_path)
            .arg("1")
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
        if !output.status.success() || sample.is_none() {
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

fn run_benchmark_report(project_path: &Path, runs: usize, report_path: &Path) -> Result<()> {
    let executable = std::env::current_exe().context("failed to locate benchmark executable")?;
    let mut report = run_startup_suite(project_path, runs)?;
    emit_report_line(&mut report, "");
    emit_report_line(
        &mut report,
        "project workload · actual packaged opening composition",
    );
    let timeline_inventory = run_benchmark_workload(
        &executable,
        project_path,
        "opening composition · full composition",
        None,
        &mut report,
    )?;
    for (section, workloads) in PORTABLE_BENCHMARK_SECTIONS {
        emit_report_line(&mut report, "");
        emit_report_line(&mut report, section);
        let mut authored = 0;
        for (label, target) in *workloads {
            if timeline_is_available(&timeline_inventory, target) {
                run_benchmark_workload(
                    &executable,
                    project_path,
                    label,
                    Some(target),
                    &mut report,
                )?;
                authored += 1;
            }
        }
        if authored == 0 {
            emit_report_line(
                &mut report,
                "not authored by this project · skipped without substituting unrelated content",
            );
        }
    }
    emit_report_line(&mut report, "");
    emit_report_line(
        &mut report,
        "package I/O · real assets and isolated Hakutaku access-class stress",
    );
    let output = Command::new(&executable)
        .arg("__benchmark-package")
        .arg(project_path)
        .output()
        .context("failed to start package I/O benchmark")?;
    if !output.status.success() {
        anyhow::bail!(
            "package I/O benchmark failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    let package_report =
        String::from_utf8(output.stdout).context("package I/O benchmark output is not UTF-8")?;
    for line in package_report.lines() {
        emit_report_line(&mut report, line);
    }
    crate::storage::write_atomically(report_path, report.as_bytes())?;
    println!("benchmark report written to {}", report_path.display());
    Ok(())
}

fn run_benchmark_workload(
    executable: &Path,
    project_path: &Path,
    label: &str,
    target: Option<&str>,
    report: &mut String,
) -> Result<String> {
    emit_report_line(report, "");
    emit_report_line(
        report,
        format!("settled render · {label} · 3.0s warm-up + 5.0s sample"),
    );
    let mut command = Command::new(executable);
    command.arg("benchmark").arg(project_path).arg("5");
    if let Some(target) = target {
        command.arg(target).arg("full");
    }
    let output = command
        .env(RUNTIME_BENCHMARK_CHILD_ENV, "1")
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
    for line in stdout.lines().chain(stderr.lines()) {
        if benchmark_report_line(line) {
            emit_report_line(report, line.trim());
            captured += 1;
        }
    }
    if captured == 0 {
        anyhow::bail!("{label} benchmark completed without performance output");
    }
    Ok(stderr.into_owned())
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
        "START    |",
        "CAPTURE  |",
        "FRAME    |",
        "SCENE    |",
        "ASSETS   |",
        "MEMORY   |",
        "RENDER   |",
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
        BenchmarkTarget::Cursor(cursor) => Some((state.current_scene.clone(), *cursor)),
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
    let line = line.as_ref();
    println!("{line}");
    report.push_str(line);
    report.push('\n');
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
    let (project_root, config, content) = open_project(project_path.as_ref(), &loader)?;
    let languages = loader
        .languages(&config.adapter.script)
        .context("failed to select script adapter")?;
    let store = loader
        .store(&config.adapter.store)
        .context("failed to select store adapter")?;
    Ok(build_opened_app(
        project_root,
        config,
        content,
        languages,
        store,
        LaunchOptions::default(),
    ))
}

fn build_opened_app(
    project_root: PathBuf,
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
    app.add_plugins(
        DefaultPlugins
            .build()
            .set(AssetPlugin {
                watch_for_changes_override: Some(watch_assets),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: config.title.clone(),
                    resolution: initial_resolution,
                    // Startup reports keep the real winit window and wgpu
                    // surface but hide them from the desktop/taskbar. A truly
                    // headless render target would omit the startup costs this
                    // benchmark is intended to measure.
                    visible: !options.hidden_window,
                    ..default()
                }),
                // Keep the native window alive until the shutdown pipeline has
                // flushed persistence. Despawning it immediately can race the
                // final winit `Destroyed` event and produce an unknown-window
                // warning during an otherwise successful exit.
                close_when_requested: false,
                ..default()
            })
            .set(ImagePlugin::default())
            .set(super::platform::log_plugin()),
    );
    #[cfg(feature = "audio-opus")]
    app.add_plugins(crate::runtime::audio::OpusAudioPlugin::new(
        asset_mounts.clone(),
    ));
    #[cfg(feature = "audio-seekable")]
    app.add_plugins(crate::runtime::audio::SeekableAudioPlugin);
    app.add_plugins((webp, GamePlugin::new(options.video), BlurPlugin))
        .insert_resource(ProjectRoot(project_root))
        .insert_resource(ContentProjectResource(content))
        .insert_resource(ScriptLanguages(languages))
        .insert_resource(StoreCodec(store))
        .insert_resource(GameConfigResource(config))
        .add_systems(PreStartup, bootstrap_project)
        .add_systems(PostStartup, set_primary_window_icon);
    if options.editor_sync {
        app.init_resource::<EditorSyncSession>();
    }
    if options.development {
        app.init_resource::<DevelopmentSession>();
        #[cfg(feature = "hot-reload")]
        app.init_resource::<HotReloadSession>();
    }
    if let Some(benchmark) = options.benchmark {
        app.add_plugins(EntityCountDiagnosticsPlugin::default());
        app.init_resource::<PersistenceDisabled>();
        crate::ui::performance::install_runtime_capture(
            &mut app,
            benchmark.seconds,
            benchmark.target.clone(),
            benchmark.cameras,
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
    _main_thread: NonSendMarker,
) {
    #[cfg(target_os = "macos")]
    if let Err(error) = set_macos_application_icon() {
        log::warn!("failed to set macOS application icon: {error:#}");
    }

    let Ok(window_entity) = window.single() else {
        return;
    };
    let icon = match load_window_icon() {
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

    let main_thread =
        MainThreadMarker::new().context("application icon must be set on main thread")?;
    let bytes = include_bytes!("../../assets/icons/keine-256.png");
    // SAFETY: `NSData` copies exactly `bytes.len()` readable bytes from this
    // process-owned static buffer before returning.
    let data = unsafe { NSData::dataWithBytes_length(bytes.as_ptr().cast(), bytes.len()) };
    let image = NSImage::initWithData(main_thread.alloc(), &data)
        .context("AppKit rejected the embedded PNG application icon")?;
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

fn decode_window_icon() -> Result<(Vec<u8>, u32, u32)> {
    let image = Image::from_buffer(
        include_bytes!("../../assets/icons/keine-256.png"),
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
    let scenes =
        load_scenes_with(content, languages).context("failed to compile project scenes")?;
    let mut actions = 0usize;
    let mut warnings = 0usize;
    let mut errors = 0usize;
    let mut missing_resources = HashSet::new();
    let mut legacy_effects = HashSet::new();
    for scene in &scenes {
        actions += scene.actions.len();
        for diagnostic in &scene.diagnostics {
            let level = match diagnostic.level {
                DiagnosticLevel::Warning => {
                    warnings += 1;
                    "warning"
                }
                DiagnosticLevel::Error => {
                    errors += 1;
                    "error"
                }
            };
            eprintln!(
                "{level}: {}:{}:{}: {}",
                scene.path.display(),
                diagnostic.span.line,
                diagnostic.span.column,
                diagnostic.message
            );
        }
        for resource in &scene.resources {
            let path = resource.resolved_path(config);
            if path.contains('{') {
                continue;
            }
            if resource.kind == ResourceKind::Effect
                && config.uses_legacy_effect_fallback(&resource.path)
                && legacy_effects.insert(resource.path.clone())
            {
                warnings += 1;
                eprintln!(
                    "warning: {}:{}:{}: bare sound effect {:?} uses the deprecated vocal/ fallback; use se/... or an assets.effects alias",
                    scene.path.display(),
                    resource.span.line,
                    resource.span.column,
                    resource.path,
                );
            }
            if !missing_resources.insert(path.clone()) {
                continue;
            }
            if !content.contains_asset(Path::new(&path)) {
                errors += 1;
                eprintln!(
                    "error: {}:{}:{}: resource does not exist: {path}",
                    scene.path.display(),
                    resource.span.line,
                    resource.span.column,
                );
            }
        }
    }
    if errors > 0 {
        anyhow::bail!("project check failed with {errors} error(s) and {warnings} warning(s)");
    }
    println!(
        "project valid · {} · {} scene(s) · {actions} action(s) · {} source(s) · {warnings} warning(s)",
        config.title,
        scenes.len(),
        content.sources.len(),
    );
    Ok(())
}

pub(crate) fn open_project(
    project_path: &Path,
    loader: &LoaderRegistry,
) -> Result<(PathBuf, GameConfig, ContentProject)> {
    if let Some(project) = loader.open_project(project_path)? {
        return Ok((project.root, project.config, project.content));
    }

    ensure_project_directory(project_path)?;
    let config_path = project_path.join("config.yaml");
    let bytes = crate::storage::read_limited(&config_path, MAX_PROJECT_CONFIG_BYTES)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let yaml = std::str::from_utf8(&bytes)
        .with_context(|| format!("project config is not UTF-8: {}", config_path.display()))?;
    let config = GameConfig::from_yaml(yaml)
        .with_context(|| format!("invalid project config {}", config_path.display()))?;
    let content = load_project_with(project_path, &config.adapter.asset, loader)?;
    Ok((content.root.clone(), config, content))
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
    project_root: Res<ProjectRoot>,
    content: Res<ContentProjectResource>,
    languages: Res<ScriptLanguages>,
    config: Res<GameConfigResource>,
    mode: BootstrapMode,
) {
    spawn_cameras(
        &mut commands,
        mode.benchmark
            .as_ref()
            .map_or(crate::ui::performance::BenchmarkCameras::Full, |capture| {
                capture.cameras
            }),
    );

    let mut state = State::new();
    match content.initial_state() {
        Ok(initial) => {
            state.vars = initial.variables;
            state.global_vars = initial.shared_variables;
        }
        Err(error) => log::error!("failed to load project variable defaults: {error:#}"),
    }
    if mode.editor_sync.is_none() {
        state
            .global_vars
            .extend(crate::storage::profile::load(&project_root));
        crate::storage::gallery::load(&mut state, &project_root);
        state.read_dialogues = crate::storage::read_history::load(&project_root);
    }
    let read_history_count = state.read_dialogues.len();
    let mut scene_count = 0;
    let mut action_count = 0;
    let mut manifest = LocalAssetManifest::default();
    match load_scenes_with(&content, &languages) {
        Ok(scenes) => {
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
                        resources: scene.resources,
                        sub_scenes: scene.sub_scenes,
                        action_spans: scene.action_spans,
                    },
                );
                program_scenes.push((scene.name, scene.actions));
            }
            state.install_program(Program::from_scenes(program_scenes));
        }
        Err(error) => log::error!("failed to load scripts: {error:#}"),
    }
    ensure_playable_scene(&mut state);
    if mode.editor_sync.is_some() {
        // An editor is already the outer shell. Enter its current selected block directly
        // so the native overlay never flashes keine's title screen first.
        state.ended = false;
        if !crate::runtime::tick::sync_editor_cursor(&content, &mut state, &manifest) {
            keine_core::step::step(&mut state);
        }
    } else if mode.benchmark.is_some() {
        // Runtime captures start on the actual stage, not the comparatively
        // cheap title screen, and never require synthetic keyboard input.
        state.ended = false;
        keine_core::step::step(&mut state);
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
        if let Some(BenchmarkTarget::Timeline(name)) = requested_target
            && resolved_target.is_none()
        {
            log::error!(
                target: "keine::performance",
                "benchmark timeline {name:?} does not exist; available timelines: {timelines}",
            );
        }
        if let Some((target_scene, cursor)) = &resolved_target {
            let new_preview = || State {
                program: state.program.clone(),
                program_fingerprint: state.program_fingerprint,
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
                log::warn!(target: "keine::performance", "benchmark cursor {cursor} could not be replayed in {target_scene:?}");
            }
            if let Some(animation) = state.stage_animation.as_mut() {
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
    commands.insert_resource(GameState(state));
    commands.insert_resource(crate::storage::read_history::ReadHistoryWriter::loaded(
        read_history_count,
    ));
    commands.insert_resource(profile_writer);
    commands.insert_resource(gallery_snapshot);
    commands.insert_resource(manifest);
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
    commands.spawn((
        Name::new("scene_camera"),
        Camera2d,
        Camera {
            order: 0,
            is_active: cameras.scene(),
            ..default()
        },
        RenderLayers::layer(0),
        BlurCamera::default(),
        SceneBlurCamera,
    ));
    commands.spawn((
        Name::new("ui_camera"),
        Camera2d,
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

    fn unique_temp_path(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("keine-{name}-{}-{nonce}", std::process::id()))
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
        assert_eq!(STRESS_BENCHMARK_WORKLOADS.len(), 1);
        assert_eq!(targets.len(), 12);
    }

    #[test]
    fn portable_benchmark_matches_only_complete_authored_timeline_ids() {
        let inventory = "0.1s INFO keine::performance: TIMELINE | intro:2:benchmark representative dialogue, coverage:8:10-04 blur family\n";

        assert!(timeline_is_available(
            inventory,
            "benchmark representative dialogue"
        ));
        assert!(timeline_is_available(inventory, "10-04 blur family"));
        assert!(!timeline_is_available(inventory, "blur family"));
        assert!(!timeline_is_available("TIMELINE | ", "anything"));
    }

    #[test]
    #[cfg(feature = "hot-reload")]
    fn parser_keeps_each_commands_project_and_options_together() {
        let CliCommand::Check { project } = parse_cli(&args(&["check", "project"])).unwrap() else {
            panic!("expected check command");
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
    #[cfg(feature = "configure")]
    fn configure_replaces_the_adapter_command_without_breaking_compatibility() {
        assert!(matches!(
            parse_cli(&args(&["configure"])).unwrap(),
            CliCommand::Configure
        ));
        assert!(matches!(
            parse_cli(&args(&["adapters"])).unwrap(),
            CliCommand::Configure
        ));
        assert!(parse_cli(&args(&["configure", "ignored"])).is_err());
    }

    #[test]
    #[cfg(not(feature = "configure"))]
    fn release_surface_rejects_the_uncompiled_configuration_tui() {
        let error = parse_cli(&args(&["configure"])).unwrap_err();
        assert!(error.to_string().contains("not compiled"));
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
                target: None,
                cameras: crate::ui::performance::BenchmarkCameras::Full,
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
        let check = parse_cli(&args(&["check", "project"])).unwrap();

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
        } = parse_cli(&args(&["benchmark", "/tmp/project"])).unwrap()
        else {
            panic!("expected benchmark command");
        };
        assert_eq!(options.seconds, 15.0);
        assert_eq!(options.target, None);
        assert_eq!(
            options.cameras,
            crate::ui::performance::BenchmarkCameras::Full
        );
    }

    #[test]
    fn startup_benchmark_command_has_bounded_repeatable_runs() {
        let CliCommand::Run {
            mode: InteractiveMode::StartupBenchmark(options),
            ..
        } = parse_cli(&args(&["benchmark-startup", "/tmp/project"])).unwrap()
        else {
            panic!("expected startup benchmark command");
        };
        assert_eq!(options.runs, 7);
        assert!(parse_cli(&args(&["benchmark-startup", "/tmp/project", "0"])).is_err());
        assert!(parse_cli(&args(&["benchmark-startup", "/tmp/project", "51"])).is_err());
    }

    #[test]
    fn benchmark_command_accepts_duration_and_cursor() {
        let CliCommand::Run {
            mode: InteractiveMode::Benchmark(options),
            ..
        } = parse_cli(&args(&["benchmark", "/tmp/project", "7.5", "25"])).unwrap()
        else {
            panic!("expected benchmark command");
        };
        assert_eq!(options.seconds, 7.5);
        assert_eq!(options.target, Some(BenchmarkTarget::Cursor(25)));
        assert_eq!(
            options.cameras,
            crate::ui::performance::BenchmarkCameras::Full
        );
    }

    #[test]
    fn benchmark_command_accepts_stable_timeline_name() {
        let CliCommand::Run {
            mode: InteractiveMode::Benchmark(options),
            ..
        } = parse_cli(&args(&[
            "benchmark",
            "/tmp/project",
            "7.5",
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
            "benchmark",
            "/tmp/project",
            "7.5",
            "25",
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
    fn benchmark_command_rejects_zero_duration() {
        assert!(parse_cli(&args(&["benchmark", "/tmp/project", "0"])).is_err());
    }

    #[test]
    fn parser_rejects_alias_names_and_ignored_arguments() {
        assert!(parse_cli(&args(&["validate", "project"])).is_err());
        assert!(parse_cli(&args(&["perf", "project"])).is_err());
        assert!(parse_cli(&args(&["remap-assets", "project", "wav=opus"])).is_err());
        assert!(parse_cli(&args(&["check", "project", "ignored"])).is_err());
        assert!(parse_cli(&args(&["dev", "project", "--sync", "--sync"])).is_err());
    }

    #[test]
    fn assets_remap_accepts_rules_and_explicit_yes() {
        let CliCommand::RemapAssets {
            project,
            rules,
            yes,
        } = parse_cli(&args(&[
            "assets",
            "--remap",
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
    fn assets_remap_requires_rules_and_rejects_duplicate_yes() {
        assert!(parse_cli(&args(&["assets", "--remap", "/tmp/project"])).is_err());
        assert!(
            parse_cli(&args(&[
                "assets",
                "--remap",
                "/tmp/project",
                "wav=opus",
                "-y",
                "-y",
            ]))
            .is_err()
        );
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
