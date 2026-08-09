use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
use keine_loader::{
    ContentProject, DiagnosticLevel, LoaderRegistry, ScriptWatcher, load_project_with,
    load_scenes_with,
};

use crate::render::blur::{BlurCamera, BlurPlugin, DialogCamera, SceneBlurCamera, UiBlurCamera};
use crate::runtime::GamePlugin;
use crate::runtime::cli::{
    BenchmarkOptions, CliCommand, InteractiveMode, help_or_version, parse as parse_cli,
    resolve_project_path,
};
use crate::runtime::resources::{
    ContentProjectResource, DevelopmentSession, EditorSyncSession, GameConfigResource, GameState,
    HotReloadSession, LocalAssetCache, LocalAssetManifest, LocalSceneAssets, PersistenceDisabled,
    ProjectRoot, ScriptLanguages, ScriptWatcherResource, StoreCodec,
};

#[derive(Clone, Copy, Default)]
struct LaunchOptions {
    development: bool,
    editor_sync: bool,
    benchmark: Option<BenchmarkOptions>,
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
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if let Some(code) = help_or_version(&args) {
        return code;
    }
    let command = match parse_cli(&args) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error:#}\nrun `keine --help` for usage");
            return std::process::ExitCode::FAILURE;
        }
    };
    let configure_adapters = matches!(&command, CliCommand::Adapters);
    let uses_startup_error_page = command.uses_startup_error_page();
    let mut loader = LoaderRegistry::default();
    let result = if configure_adapters {
        super::adapter_tui::configure(&loader)
    } else {
        super::adapter_tui::apply_saved_selection(&mut loader)
            .and_then(|()| execute_command(loader, command))
    };

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            report_startup_error(
                uses_startup_error_page,
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
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let parsed = parse_cli(&args);
    let uses_startup_error_page = parsed
        .as_ref()
        .is_ok_and(CliCommand::uses_startup_error_page);
    let result = parsed.and_then(|command| execute_command(loader, command));
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

fn execute_command(loader: LoaderRegistry, command: CliCommand) -> Result<()> {
    #[cfg(feature = "hardened")]
    super::platform::apply_hardening();
    let (project_path, action) = match command {
        CliCommand::Adapters => {
            anyhow::bail!("adapter configuration must run before project setup")
        }
        CliCommand::Package { project, output } => {
            return crate::package::package_project(
                &resolve_project_path(project),
                &loader,
                &output,
            );
        }
        CliCommand::Check { project } => (project, ProjectAction::Check),
        CliCommand::Run {
            project,
            mode,
            editor_sync,
        } => (project, ProjectAction::Run { mode, editor_sync }),
    };
    let (project_root, config, content) =
        open_project(&resolve_project_path(project_path), &loader)?;
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
            benchmark: mode.benchmark(),
        },
    );
    app.run();
    Ok(())
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
    .add_plugins((webp, GamePlugin, BlurPlugin))
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
        app.init_resource::<HotReloadSession>();
    }
    if let Some(benchmark) = options.benchmark {
        app.add_plugins(EntityCountDiagnosticsPlugin::default());
        app.init_resource::<PersistenceDisabled>();
        crate::ui::performance::install_runtime_capture(
            &mut app,
            benchmark.seconds,
            benchmark.cursor,
            benchmark.cameras,
        );
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

pub(crate) fn open_project(
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

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
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
    fn only_non_development_interactive_modes_require_a_process_lock() {
        assert!(InteractiveMode::Shipping.requires_single_instance());
        assert!(!InteractiveMode::Development.requires_single_instance());
        assert!(
            !InteractiveMode::Benchmark(BenchmarkOptions {
                seconds: 1.0,
                cursor: None,
                cameras: crate::ui::performance::BenchmarkCameras::Full,
            })
            .requires_single_instance()
        );
    }

    #[test]
    fn only_shipping_runs_use_the_native_startup_error_page() {
        let shipping = parse_cli(&args(&["game.hxz"])).unwrap();
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
        assert_eq!(options.cursor, None);
        assert_eq!(
            options.cameras,
            crate::ui::performance::BenchmarkCameras::Full
        );
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
        assert_eq!(options.cursor, Some(25));
        assert_eq!(
            options.cameras,
            crate::ui::performance::BenchmarkCameras::Full
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
        assert!(parse_cli(&args(&["check", "project", "ignored"])).is_err());
        assert!(parse_cli(&args(&["dev", "project", "--sync", "--sync"])).is_err());
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
