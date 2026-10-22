//! Background script rebuilds and legacy Editor cursor polling.
use super::restart_after_program_reload;
/// Preserve the loader location through anyhow so Preview can navigate errors.
#[derive(Debug)]
pub(crate) struct ReloadDiagnostic {
    pub path: std::path::PathBuf,
    pub diagnostic: Diagnostic,
}
impl std::fmt::Display for ReloadDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.path.display(),
            self.diagnostic.span.line,
            self.diagnostic.span.column,
            self.diagnostic.message
        )
    }
}
impl std::error::Error for ReloadDiagnostic {}

#[cfg(feature = "hot-reload")]
use super::seek::try_sync_editor_cursor;
#[cfg(feature = "hot-reload")]
use super::{TickContext, TypewriterClock};
use crate::runtime::resources::{LocalAssetManifest, LocalSceneAssets};
use anyhow::Context as ContextExt;
use bevy::prelude::*;
use keine_core::{Program, State, config::GameConfig};
use keine_loader::{Diagnostic, DiagnosticLevel};

#[cfg(feature = "hot-reload")]
#[derive(Default)]
pub(super) struct EditorCursorSync {
    remaining_frames: u8,
    poll_elapsed: f32,
    last: Option<keine_loader::ProjectDebugCursor>,
    force: bool,
}

#[cfg(feature = "hot-reload")]
#[derive(Default)]
pub(super) struct HotReloadPipeline {
    running: Option<RunningHotReload>,
    pending_change_count: usize,
}

#[cfg(feature = "hot-reload")]
struct RunningHotReload {
    change_count: usize,
    worker: std::thread::JoinHandle<anyhow::Result<HotReloadBuild>>,
}

pub(crate) struct HotReloadBuild {
    config: Option<keine_core::config::GameConfig>,
    manifest: LocalAssetManifest,
    program: Program,
    diagnostics: Vec<(std::path::PathBuf, Diagnostic)>,
    initial: keine_loader::ProjectInitialState,
}

#[cfg(feature = "hot-reload")]
const EDITOR_CURSOR_POLL_SECONDS: f32 = 0.2;

#[cfg(feature = "hot-reload")]
pub(super) fn reload_scripts_if_changed(
    context: &mut TickContext<'_, '_>,
    delta_seconds: f32,
) -> bool {
    let changes = context
        .watcher
        .as_ref()
        .and_then(|watcher| watcher.0.lock().ok().map(|watcher| watcher.drain()))
        .unwrap_or_default();

    let cursor_changed = changes
        .iter()
        .any(|path| context.content.is_debug_cursor_change(path));
    let source_change_count = changes
        .iter()
        .filter(|path| !context.content.is_debug_cursor_change(path))
        .count();
    let editor_sync = context.editor_sync.is_some();

    let mut changed = false;
    if source_change_count > 0 {
        context.hot_reload.pending_change_count = context
            .hot_reload
            .pending_change_count
            .saturating_add(source_change_count);
    }
    if let Some((change_count, result)) = take_completed_reload(&mut context.hot_reload) {
        if context.hot_reload.pending_change_count > 0 {
            log::debug!(
                "discarded a completed hot reload because newer source changes are pending"
            );
        } else {
            match result {
                Ok(build) => {
                    let restarted = apply_hot_reload(
                        build,
                        context.state.bypass_change_detection(),
                        context.asset_manifest.reborrow(),
                        context.config.reborrow(),
                        context.image_roles.reborrow(),
                        false,
                    );
                    changed |= restarted;
                    log::info!("reloaded {change_count} changed project source(s)");
                    if editor_sync && restarted {
                        context.editor_cursor_sync.force = true;
                        context.editor_cursor_sync.remaining_frames = 8;
                    }
                }
                Err(error) => log::error!("failed to reload project sources: {error:#}"),
            }
        }
    }
    if let Err(error) = start_pending_reload(
        &mut context.hot_reload,
        &context.content,
        &context.languages,
        &context.config.0,
    ) {
        log::error!("failed to start project hot reload: {error:#}");
    }
    if cursor_changed && editor_sync {
        context.editor_cursor_sync.remaining_frames = 8;
    }
    if editor_sync {
        context.editor_cursor_sync.poll_elapsed += delta_seconds.max(0.0);
        if context.editor_cursor_sync.poll_elapsed >= EDITOR_CURSOR_POLL_SECONDS {
            context.editor_cursor_sync.poll_elapsed %= EDITOR_CURSOR_POLL_SECONDS;
            context.editor_cursor_sync.remaining_frames =
                context.editor_cursor_sync.remaining_frames.max(1);
        }
    }
    let reload_in_flight =
        context.hot_reload.running.is_some() || context.hot_reload.pending_change_count > 0;
    if context.editor_cursor_sync.remaining_frames > 0 && !reload_in_flight {
        let force = context.editor_cursor_sync.force;
        match try_sync_editor_cursor(
            &context.content,
            context.state.bypass_change_detection(),
            &context.asset_manifest,
            &mut context.editor_cursor_sync.last,
            force,
        ) {
            Ok(Some(cursor_changed)) => {
                changed |= cursor_changed;
                context.editor_cursor_sync.remaining_frames = 0;
                context.editor_cursor_sync.force = false;
            }
            Ok(None) => {
                context.editor_cursor_sync.remaining_frames = 0;
                context.editor_cursor_sync.force = false;
            }
            Err(error) => {
                context.editor_cursor_sync.remaining_frames -= 1;
                if context.editor_cursor_sync.remaining_frames == 0 {
                    log::warn!("failed to synchronize Studio debug position: {error:#}");
                }
            }
        }
    }
    if changed {
        context.checkpoint.reset(&context.state);
        *context.typewriter_clock = TypewriterClock::default();
        *context.auto_timer = 0.0;
        *context.checkpoint_generation = context.checkpoint.generation();
    }
    changed
}

#[cfg(feature = "hot-reload")]
fn start_pending_reload(
    pipeline: &mut HotReloadPipeline,
    content: &keine_loader::ContentProject,
    languages: &keine_loader::ScriptLanguageRegistry,
    config: &GameConfig,
) -> anyhow::Result<()> {
    if pipeline.running.is_some() || pipeline.pending_change_count == 0 {
        return Ok(());
    }
    let content = content.clone();
    let languages = languages.clone();
    let config = config.clone();
    install_pending_reload(pipeline, || {
        std::thread::Builder::new()
            .name("keine-hot-reload".to_owned())
            .spawn(move || build_hot_reload(&content, &languages, &config))
            .context("failed to spawn hot-reload worker")
    })
}

#[cfg(feature = "hot-reload")]
fn install_pending_reload(
    pipeline: &mut HotReloadPipeline,
    spawn_worker: impl FnOnce()
        -> anyhow::Result<std::thread::JoinHandle<anyhow::Result<HotReloadBuild>>>,
) -> anyhow::Result<()> {
    let change_count = pipeline.pending_change_count;
    let worker = spawn_worker()?;
    pipeline.pending_change_count = 0;
    pipeline.running = Some(RunningHotReload {
        change_count,
        worker,
    });
    Ok(())
}

#[cfg(feature = "hot-reload")]
fn take_completed_reload(
    pipeline: &mut HotReloadPipeline,
) -> Option<(usize, anyhow::Result<HotReloadBuild>)> {
    // `JoinHandle::is_finished` is the standard-library non-blocking join
    // probe. Only consume and join the worker after it reports completion, so
    // source parsing never returns to Bevy's frame-critical Update path.
    if !pipeline
        .running
        .as_ref()
        .is_some_and(|running| running.worker.is_finished())
    {
        return None;
    }
    let running = pipeline.running.take()?;
    let result = running
        .worker
        .join()
        .unwrap_or_else(|_| Err(anyhow::anyhow!("hot-reload worker panicked")));
    Some((running.change_count, result))
}

pub(crate) fn build_hot_reload(
    content: &keine_loader::ContentProject,
    languages: &keine_loader::ScriptLanguageRegistry,
    active_config: &GameConfig,
) -> anyhow::Result<HotReloadBuild> {
    let config = content
        .reload_config()
        .context("failed to reload project config")?;
    let mut scenes = keine_loader::load_scenes_with(content, languages)
        .context("failed to reload scripts from configured content sources")?;
    let effective_config = config.as_ref().unwrap_or(active_config);
    if effective_config
        .adapter
        .script
        .eq_ignore_ascii_case("keine")
    {
        keine_loader::validate_native_entry_flow(&mut scenes, &effective_config.script.entry);
        // A successful loader call may still contain syntax/semantic errors.
        // Reject before constructing or installing any part of the candidate.
        if let Some((scene, diagnostic)) = scenes.iter().find_map(|scene| {
            scene
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.level == DiagnosticLevel::Error)
                .map(|diagnostic| (scene, diagnostic))
        }) {
            return Err(ReloadDiagnostic {
                path: scene.path.clone(),
                diagnostic: diagnostic.clone(),
            }
            .into());
        }
        anyhow::ensure!(
            scenes
                .iter()
                .any(|scene| scene.name == effective_config.script.entry),
            "entry scene {:?} is missing",
            effective_config.script.entry
        );
    }
    let initial = content
        .initial_state()
        .context("failed to reload initial state")?;
    let mut manifest = LocalAssetManifest::default();
    let mut program_scenes = Vec::with_capacity(scenes.len());
    let mut diagnostics = Vec::new();
    for scene in scenes {
        diagnostics.extend(
            scene
                .diagnostics
                .into_iter()
                .map(|diagnostic| (scene.path.clone(), diagnostic)),
        );
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
    Ok(HotReloadBuild {
        config,
        manifest,
        program: Program::from_scenes(program_scenes),
        diagnostics,
        initial,
    })
}

pub(crate) fn apply_hot_reload(
    build: HotReloadBuild,
    state: &mut State,
    mut asset_manifest: Mut<LocalAssetManifest>,
    mut config: Mut<crate::runtime::resources::GameConfigResource>,
    mut image_roles: Mut<crate::scene::images::ImageRoleRegistry>,
    fresh_preview: bool,
) -> bool {
    // Only source spans may change on this path. Adapter config refreshes are
    // conservatively treated as changes; no serialization/hash work per frame.
    let unchanged = build.config.is_none()
        && state.program.fingerprint() == build.program.fingerprint()
        && state.program.as_ref() == &build.program
        && asset_manifest.len() == build.manifest.len()
        && asset_manifest.iter().all(|(name, old)| {
            build.manifest.get(name).is_some_and(|new| {
                old.resources
                    .iter()
                    .map(|r| (&r.path, r.kind, r.action_index))
                    .eq(new
                        .resources
                        .iter()
                        .map(|r| (&r.path, r.kind, r.action_index)))
                    && old
                        .sub_scenes
                        .iter()
                        .map(|r| (&r.scene, r.action_index))
                        .eq(new.sub_scenes.iter().map(|r| (&r.scene, r.action_index)))
            })
        });
    for (path, diagnostic) in build.diagnostics {
        let message = format!(
            "{}:{}:{}: {}",
            path.display(),
            diagnostic.span.line,
            diagnostic.span.column,
            diagnostic.message
        );
        match diagnostic.level {
            DiagnosticLevel::Warning => log::warn!("{message}"),
            DiagnosticLevel::Error => log::error!("{message}"),
        }
    }
    if let Some(refreshed_config) = build.config {
        config.0 = refreshed_config;
    }
    if unchanged {
        *asset_manifest.bypass_change_detection() = build.manifest;
        log::info!("source positions updated; execution unchanged");
        return false;
    }
    *asset_manifest = build.manifest;
    image_roles.rebuild(&config, &asset_manifest);
    restart_after_program_reload(
        state,
        build.program,
        (fresh_preview && config.adapter.script.eq_ignore_ascii_case("keine"))
            .then_some((config.script.entry.clone(), build.initial)),
    );
    true
}

#[cfg(all(test, feature = "hot-reload"))]
mod tests {
    #[cfg(feature = "hot-reload")]
    use super::super::TypewriterPause;
    #[cfg(feature = "hot-reload")]
    use super::super::tests::{dialogue_state, playback_test_app};
    use super::*;
    #[cfg(feature = "hot-reload")]
    use keine_core::Action;
    #[cfg(feature = "hot-reload")]
    use keine_core::config::AssetSourceConfig;
    #[cfg(feature = "hot-reload")]
    use std::sync::{Arc, Mutex};

    #[cfg(feature = "hot-reload")]
    struct ThreadRecordingLanguage(Arc<Mutex<Option<std::thread::ThreadId>>>);

    #[cfg(feature = "hot-reload")]
    impl keine_loader::ScriptLanguage for ThreadRecordingLanguage {
        fn name(&self) -> &'static str {
            "thread-recording"
        }

        fn extensions(&self) -> &'static [&'static str] {
            &["reload"]
        }

        fn parse(&self, _source: &str) -> keine_loader::ParseReport {
            *self.0.lock().unwrap() = Some(std::thread::current().id());
            keine_loader::ParseReport {
                diagnostics: vec![Diagnostic {
                    level: DiagnosticLevel::Error,
                    span: keine_loader::SourceSpan { line: 1, column: 1 },
                    message: "legacy diagnostic remains nonfatal".into(),
                }],
                ..Default::default()
            }
        }
    }

    #[test]
    #[cfg(feature = "hot-reload")]
    fn hot_reload_build_is_send_and_runs_off_the_calling_thread() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-hot-reload-{nonce}"));
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(root.join("scripts/main.reload"), "reload").unwrap();
        let content = keine_loader::load_project(
            &root,
            &[AssetSourceConfig {
                path: ".".into(),
                format: "fs".into(),
            }],
        )
        .unwrap();
        let parsed_on = Arc::new(Mutex::new(None));
        let languages = keine_loader::ScriptLanguageRegistry::new()
            .with(ThreadRecordingLanguage(Arc::clone(&parsed_on)));
        let caller = std::thread::current().id();
        let mut pipeline = HotReloadPipeline {
            pending_change_count: 1,
            ..default()
        };

        let mut config = GameConfig::default();
        config.adapter.script = "webgal".into();
        start_pending_reload(&mut pipeline, &content, &languages, &config).unwrap();
        let worker = pipeline.running.as_ref().unwrap().worker.thread().id();
        let (_, result) = loop {
            if let Some(completed) = take_completed_reload(&mut pipeline) {
                break completed;
            }
            std::thread::yield_now();
        };
        let build = result.unwrap();

        assert_eq!(build.program.scene_count(), 1);
        assert_ne!(worker, caller);
        assert_eq!(parsed_on.lock().unwrap().unwrap(), worker);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(feature = "hot-reload")]
    fn failed_hot_reload_worker_spawn_keeps_pending_changes() {
        let mut pipeline = HotReloadPipeline {
            pending_change_count: 3,
            ..default()
        };

        let error = install_pending_reload(&mut pipeline, || {
            Err(anyhow::anyhow!("simulated worker creation failure"))
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("simulated worker creation failure")
        );
        assert_eq!(pipeline.pending_change_count, 3);
        assert!(pipeline.running.is_none());
    }

    #[test]
    fn hot_reload_discards_invalid_and_superseded_candidates_without_touching_live_state() {
        use bevy::ecs::system::RunSystemOnce;
        for superseded in [false, true] {
            let mut app = playback_test_app(dialogue_state(), true);
            let worker = std::thread::spawn(move || {
                if !superseded {
                    return Err(anyhow::anyhow!("main.shou:2:3: invalid expression"));
                }
                Ok(HotReloadBuild {
                    config: None,
                    manifest: LocalAssetManifest::default(),
                    program: Program::default(),
                    diagnostics: Vec::new(),
                    initial: Default::default(),
                })
            });
            while !worker.is_finished() {
                std::thread::yield_now();
            }
            let mut worker = Some(worker);
            app.world_mut()
                .run_system_once(move |mut context: TickContext| {
                    let before = context.state.0.clone();
                    let generation = context.checkpoint.generation();
                    context.hot_reload.running = Some(RunningHotReload {
                        change_count: 1,
                        worker: worker.take().unwrap(),
                    });
                    context.hot_reload.pending_change_count = usize::from(superseded);
                    assert!(!reload_scripts_if_changed(&mut context, 0.0));
                    assert_eq!(context.state.0, before);
                    assert_eq!(context.checkpoint.generation(), generation);
                })
                .unwrap();
        }
    }

    #[cfg(feature = "hot-reload")]
    #[test]
    fn playback_hot_reload_resets_same_cursor_clocks_and_checkpoint() {
        use bevy::ecs::system::RunSystemOnce;
        let mut state = dialogue_state();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Comment],
        )]));
        let mut app = playback_test_app(state, true);
        let worker = std::thread::spawn(|| {
            Ok(HotReloadBuild {
                config: None,
                manifest: LocalAssetManifest::default(),
                program: Program::from_scenes([(
                    "main".into(),
                    vec![Action::Say {
                        speaker: String::new(),
                        text: "[wait=1000]abcdefghij".into(),
                        options: Default::default(),
                    }],
                )]),
                diagnostics: Vec::new(),
                initial: Default::default(),
            })
        });
        while !worker.is_finished() {
            std::thread::yield_now();
        }
        let mut worker = Some(worker);
        app.world_mut()
            .run_system_once(move |mut context: TickContext| {
                context.hot_reload.running = Some(RunningHotReload {
                    change_count: 1,
                    worker: worker.take().unwrap(),
                });
                context.typewriter_clock.scene = "main".into();
                context.typewriter_clock.cursor = 1;
                context.typewriter_clock.pause = TypewriterPause::Timed(0.1);
                *context.auto_timer = 0.2;
                assert!(reload_scripts_if_changed(&mut context, 0.0));
                assert!(matches!(
                    context.typewriter_clock.pause,
                    TypewriterPause::Idle
                ));
                assert_eq!(*context.auto_timer, 0.0);
                assert_eq!(
                    context.checkpoint.generation(),
                    *context.checkpoint_generation
                );
                assert!(context.checkpoint.generation() > 0);
                assert_eq!(
                    context
                        .checkpoint
                        .state_for_continuation(&context.state)
                        .unwrap()
                        .cursor,
                    context.state.cursor
                );
            })
            .unwrap();
    }
}
