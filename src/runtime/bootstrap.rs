use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use bevy::asset::io::AssetSourceId;
use bevy::asset::{AssetApp, AssetPlugin, RenderAssetUsages};
use bevy::camera::visibility::RenderLayers;
use bevy::diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::ecs::system::NonSendMarker;
use bevy::ecs::system::SystemParam;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy::winit::WINIT_WINDOWS;
use keine_core::config::{CompiledProgramPolicy, GameConfig};
use keine_core::{Action, DESIGN_HEIGHT, DESIGN_WIDTH, Program, State};
use keine_loader::{
    ContentProject, DiagnosticLevel, IR_SCHEMA_VERSION, LoaderRegistry, ScriptWatcher,
    attach_compiled_program, load_project_with, load_scenes_with, read_compiled_program,
};

use crate::render::blur::{BlurCamera, BlurPlugin, DialogCamera, SceneBlurCamera, UiBlurCamera};
use crate::runtime::GamePlugin;
use crate::runtime::resources::{
    ContentProjectResource, DevelopmentSession, EditorSyncSession, GameConfigResource, GameState,
    HotReloadSession, LocalAssetCache, LocalAssetManifest, LocalSceneAssets, PersistenceDisabled,
    ProjectRoot, ScriptLanguages, ScriptWatcherResource, StoreCodec,
};
use crate::runtime::startup::{SceneLoadPath, StartupTimeline};

#[derive(Clone, Copy)]
struct BenchmarkOptions {
    seconds: f32,
    cursor: Option<usize>,
    cameras: crate::ui::performance::BenchmarkCameras,
}

#[derive(Clone, Copy, Default)]
struct LaunchOptions {
    development: bool,
    editor_sync: bool,
    hot_reload: bool,
    benchmark: Option<BenchmarkOptions>,
    startup: Option<StartupTimeline>,
}

#[derive(SystemParam)]
struct BootstrapMode<'w> {
    editor_sync: Option<Res<'w, EditorSyncSession>>,
    benchmark: Option<Res<'w, crate::ui::performance::RuntimeCaptureConfig>>,
    hot_reload: Option<Res<'w, HotReloadSession>>,
}

pub fn run() {
    run_with_loader(LoaderRegistry::default());
}

pub fn run_cli() -> std::process::ExitCode {
    let configure_adapters = std::env::args_os()
        .nth(1)
        .is_some_and(|command| command == "adapters");
    let startup_requested = std::env::args_os()
        .nth(1)
        .is_some_and(|command| command == "startup");
    let mut startup = startup_requested.then(StartupTimeline::begin);
    let mut loader = LoaderRegistry::default();
    let result = if configure_adapters {
        super::adapter_tui::configure(&loader)
    } else {
        super::adapter_tui::apply_saved_selection(&mut loader)
            .and_then(|()| try_run_with_loader(loader, startup.take()))
    };

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            super::platform::startup_error(
                if configure_adapters {
                    "failed to configure adapters"
                } else {
                    "failed to open project"
                },
                &error,
            );
            std::process::ExitCode::FAILURE
        }
    }
}

pub fn run_with_loader(loader: LoaderRegistry) {
    if let Err(error) = try_run_with_loader(loader, None) {
        super::platform::startup_error("failed to open project", &error);
    }
}

fn try_run_with_loader(loader: LoaderRegistry, mut startup: Option<StartupTimeline>) -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let startup_requested = args.first().is_some_and(|command| command == "startup");
    let check_only = args.first().is_some_and(|command| command == "check");
    let compile_request = args.first().is_some_and(|command| command == "compiler");
    let compile_preview = args
        .get(1)
        .is_some_and(|subcommand| subcommand == "preview");
    let editor_sync = args.first().is_some_and(|command| command == "studio");
    let development = development_requested(&args);
    let hot_reload = hot_reload_requested(&args);
    let benchmark = benchmark_options_from_args(&args)?;
    let compiler_output = compiler_output_from_args(&args)?;
    let project_path = project_root_from_args(args.iter().cloned());
    if let Some(startup) = startup.as_mut() {
        startup.mark_cli_ready();
    }
    let (project_root, config, content) = open_project(&project_path, &loader)?;
    if let Some(startup) = startup.as_mut() {
        startup.mark_project_opened();
    }
    let languages = loader
        .languages(&config.adapter.script)
        .context("failed to select script adapter")?;
    if let Some(startup) = startup.as_mut() {
        startup.mark_languages();
    }
    if check_only {
        return check_project(&config, &content, &languages);
    }
    if compile_request && !compile_preview {
        return crate::compiler::compile_project(&config, &content, &languages, compiler_output)
            .map(|_report| ());
    }
    let force_compiled = startup_requested && args.iter().any(|argument| argument == "--compiled");
    let content = if compile_preview || force_compiled {
        if let Some(startup) = startup.as_mut() {
            startup.set_path(SceneLoadPath::Compiled);
        }
        force_compiled_preview(content)?
    } else {
        content
    };
    let store = loader
        .store(&config.adapter.store)
        .context("failed to select store adapter")?;
    if let Some(startup) = startup.as_mut() {
        startup.mark_store();
    }
    let _instance = single_instance_required(check_only, hot_reload, benchmark).then(|| {
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
            development,
            editor_sync,
            hot_reload,
            benchmark,
            startup,
        },
    );
    app.run();
    Ok(())
}

const fn single_instance_required(
    check_only: bool,
    development: bool,
    benchmark: Option<BenchmarkOptions>,
) -> bool {
    !check_only && !development && benchmark.is_none()
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

fn hot_reload_requested(args: &[std::ffi::OsString]) -> bool {
    args.first()
        .is_some_and(|command| command == "dev" || command == "studio")
}

fn development_requested(args: &[std::ffi::OsString]) -> bool {
    args.first()
        .is_some_and(|command| command == "dev" || command == "studio")
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
    let watch_assets = options.hot_reload
        && asset_mounts
            .iter()
            .any(|mount| mount.filesystem_root().is_some());

    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        crate::runtime::asset_reader::overlay_source(asset_mounts),
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
                    visible: true,
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
    )
    .add_plugins((
        webp,
        GamePlugin,
        BlurPlugin,
        FrameTimeDiagnosticsPlugin::default(),
        EntityCountDiagnosticsPlugin::default(),
    ))
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
    }
    if options.hot_reload {
        app.init_resource::<HotReloadSession>();
    }
    if let Some(benchmark) = options.benchmark {
        app.init_resource::<PersistenceDisabled>();
        crate::ui::performance::install_runtime_capture(
            &mut app,
            benchmark.seconds,
            benchmark.cursor,
            benchmark.cameras,
        );
    }
    super::platform::install_runtime_diagnostics(&mut app);
    if let Some(mut startup) = options.startup {
        startup.mark_app_built();
        super::startup::install(&mut app, startup);
    }
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
            if path.contains('{') || !missing_resources.insert(path.clone()) {
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

fn open_project(
    project_path: &Path,
    loader: &LoaderRegistry,
) -> Result<(PathBuf, GameConfig, ContentProject)> {
    if let Some(project) = loader.open_project(project_path)? {
        return Ok((project.root, project.config, project.content));
    }

    ensure_project_directory(project_path)?;
    let config_path = project_path.join("config.yaml");
    let yaml = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let config = GameConfig::from_yaml(&yaml)
        .with_context(|| format!("invalid project config {}", config_path.display()))?;
    let content = load_project_with(project_path, &config.adapter.asset, loader)?;
    let bin = if config.compiled_program == CompiledProgramPolicy::Require {
        read_compiled_program(&content.root)?
    } else {
        None
    };
    let content = attach_compiled_program(
        content,
        config.compiled_program,
        false,
        bin,
        IR_SCHEMA_VERSION,
    )?;
    Ok((content.root.clone(), config, content))
}

/// Run the compiled-loading path for any project that has a program.bin,
/// mirroring packaged `Require` semantics during development.
fn force_compiled_preview(content: ContentProject) -> Result<ContentProject> {
    let bin = read_compiled_program(&content.root)?.ok_or_else(|| {
        anyhow::anyhow!("no compiled program found; run `cargo compiler <project>` first")
    })?;
    attach_compiled_program(
        content,
        CompiledProgramPolicy::Require,
        true,
        Some(bin),
        IR_SCHEMA_VERSION,
    )
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

fn project_root_from_args(args: impl Iterator<Item = std::ffi::OsString>) -> PathBuf {
    let args = args.collect::<Vec<_>>();
    let relative = match args.as_slice() {
        [command, subcommand, path, ..] if command == "compiler" && subcommand == "preview" => {
            PathBuf::from(path)
        }
        [command, path, ..]
            if command == "dev"
                || command == "check"
                || command == "studio"
                || command == "benchmark"
                || command == "compiler"
                || command == "startup" =>
        {
            PathBuf::from(path)
        }
        [path, ..] => PathBuf::from(path),
        [] => PathBuf::new(),
    };

    std::env::current_dir()
        .unwrap_or_else(|error| {
            log::warn!("failed to read current directory: {error}");
            PathBuf::from(".")
        })
        .join(relative)
}

fn compiler_output_from_args(args: &[std::ffi::OsString]) -> Result<Option<PathBuf>> {
    let mut output = None;
    let mut index = 2;
    while index < args.len() {
        if args[index] == "--output" {
            let value = args
                .get(index + 1)
                .context("--output requires a path argument")?;
            output = Some(PathBuf::from(value));
            index += 2;
        } else {
            index += 1;
        }
    }
    Ok(output)
}

fn benchmark_options_from_args(args: &[std::ffi::OsString]) -> Result<Option<BenchmarkOptions>> {
    if args.first().is_none_or(|command| command != "benchmark") {
        return Ok(None);
    }
    let seconds = match args.get(2) {
        Some(value) => value
            .to_string_lossy()
            .parse::<f32>()
            .context("benchmark duration must be a number of seconds")?,
        None => 15.0,
    };
    if !seconds.is_finite() || seconds < 1.0 {
        anyhow::bail!("benchmark duration must be at least one second");
    }
    let cursor = args
        .get(3)
        .map(|value| {
            value
                .to_string_lossy()
                .parse::<usize>()
                .context("benchmark cursor must be a non-negative action index")
        })
        .transpose()?;
    let cameras = match args.get(4).map(|value| value.to_string_lossy()) {
        None => crate::ui::performance::BenchmarkCameras::Full,
        Some(value) if value == "full" => crate::ui::performance::BenchmarkCameras::Full,
        Some(value) if value == "scene-ui" => crate::ui::performance::BenchmarkCameras::SceneUi,
        Some(value) if value == "scene-dialog" => {
            crate::ui::performance::BenchmarkCameras::SceneDialog
        }
        Some(value) if value == "scene" => crate::ui::performance::BenchmarkCameras::SceneOnly,
        Some(value) => anyhow::bail!(
            "unknown benchmark camera profile {value:?}; expected full, scene-ui, scene-dialog, or scene"
        ),
    };
    Ok(Some(BenchmarkOptions {
        seconds,
        cursor,
        cameras,
    }))
}

fn bootstrap_project(
    mut commands: Commands,
    project_root: Res<ProjectRoot>,
    content: Res<ContentProjectResource>,
    languages: Res<ScriptLanguages>,
    config: Res<GameConfigResource>,
    mode: BootstrapMode,
    mut startup: Option<ResMut<StartupTimeline>>,
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
    if let Some(startup) = startup.as_deref_mut() {
        startup.mark_scenes_start();
    }
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
            if let Some(mut startup) = startup {
                startup.record_scenes(scene_count, action_count as u64);
            }
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
        let timelines = state
            .program
            .scene(&state.current_scene)
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, action)| match action {
                Action::StageAnimation { animation } => Some(format!("{index}:{}", animation.id)),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", ");
        log::info!(target: "keine::performance", "TIMELINE | {timelines}");
        if let Some(cursor) = mode.benchmark.as_ref().and_then(|capture| capture.cursor) {
            let target_scene = state.current_scene.clone();
            let mut preview = State {
                program: state.program.clone(),
                program_fingerprint: state.program_fingerprint,
                vars: state.vars.clone(),
                global_vars: state.global_vars.clone(),
                ..State::new()
            };
            preview.current_scene = crate::scene::entry_scene(&preview);
            preview.ended = false;
            if crate::runtime::tick::seek_editor_state(
                &mut preview,
                &target_scene,
                cursor,
                cursor.saturating_add(1),
            ) {
                state = preview;
            } else {
                log::warn!(
                    target: "keine::performance",
                    "benchmark cursor {cursor} could not be replayed in {target_scene:?}",
                );
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
            "START    | requested cursor {:?} · running cursor {} · timeline {}",
            mode.benchmark.as_ref().and_then(|capture| capture.cursor),
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
    commands.insert_resource(GameState(state));
    commands.insert_resource(crate::storage::read_history::ReadHistoryWriter::loaded(
        read_history_count,
    ));
    commands.insert_resource(profile_writer);
    commands.insert_resource(manifest);
    commands.insert_resource(LocalAssetCache::default());

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
    fn check_command_uses_the_explicit_project_path() {
        let expected = std::env::current_dir().unwrap().join("editor-project");
        let path = project_root_from_args(
            ["check", "editor-project"]
                .into_iter()
                .map(std::ffi::OsString::from),
        );
        assert_eq!(path, expected);
    }

    #[test]
    fn studio_command_uses_the_native_project_path() {
        let expected = std::env::current_dir().unwrap().join("editor-project");
        let args = ["studio", "editor-project"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        assert_eq!(project_root_from_args(args.iter().cloned()), expected);
    }

    #[test]
    fn only_development_commands_enable_hot_reload() {
        let args = |command: &str| vec![std::ffi::OsString::from(command)];
        assert!(hot_reload_requested(&args("dev")));
        assert!(hot_reload_requested(&args("studio")));
        assert!(!hot_reload_requested(&args("benchmark")));
        assert!(!hot_reload_requested(&args("/tmp/release-project")));
    }

    #[test]
    fn dev_and_studio_commands_enable_development_ui() {
        let args = |command: &str| vec![std::ffi::OsString::from(command)];
        assert!(development_requested(&args("dev")));
        assert!(development_requested(&args("studio")));
        assert!(!development_requested(&args("benchmark")));
        assert!(!development_requested(&args("/tmp/release-project")));
    }

    #[test]
    fn only_the_shipping_runtime_requires_a_process_lock() {
        assert!(single_instance_required(false, false, None));
        assert!(!single_instance_required(false, true, None));
        assert!(!single_instance_required(true, false, None));
        assert!(!single_instance_required(
            false,
            false,
            Some(BenchmarkOptions {
                seconds: 1.0,
                cursor: None,
                cameras: crate::ui::performance::BenchmarkCameras::Full,
            })
        ));
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
        let args = ["benchmark", "/tmp/project"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        let options = benchmark_options_from_args(&args).unwrap().unwrap();
        assert_eq!(options.seconds, 15.0);
        assert_eq!(options.cursor, None);
        assert_eq!(
            options.cameras,
            crate::ui::performance::BenchmarkCameras::Full
        );
    }

    #[test]
    fn benchmark_command_accepts_duration_and_cursor() {
        let args = ["benchmark", "/tmp/project", "7.5", "25"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        let options = benchmark_options_from_args(&args).unwrap().unwrap();
        assert_eq!(options.seconds, 7.5);
        assert_eq!(options.cursor, Some(25));
        assert_eq!(
            options.cameras,
            crate::ui::performance::BenchmarkCameras::Full
        );
    }

    #[test]
    fn benchmark_command_accepts_camera_profile() {
        let args = ["benchmark", "/tmp/project", "7.5", "25", "scene-ui"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        let options = benchmark_options_from_args(&args).unwrap().unwrap();
        assert_eq!(
            options.cameras,
            crate::ui::performance::BenchmarkCameras::SceneUi
        );
    }

    #[test]
    fn benchmark_command_rejects_zero_duration() {
        let args = ["benchmark", "/tmp/project", "0"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        assert!(benchmark_options_from_args(&args).is_err());
    }

    #[test]
    fn embedded_window_icon_is_valid_rgba() {
        let (rgba, width, height) = decode_window_icon().unwrap();
        assert_eq!((width, height), (256, 256));
        assert_eq!(rgba.len(), width as usize * height as usize * 4);
    }
}
