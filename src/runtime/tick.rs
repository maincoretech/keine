use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
#[cfg(any(test, feature = "hot-reload"))]
use keine_core::Program;
use keine_core::State;
use keine_core::step;
#[cfg(feature = "hot-reload")]
use keine_loader::{Diagnostic, DiagnosticLevel};

use crate::runtime::platform::InputActions;
#[cfg(any(test, feature = "hot-reload"))]
use crate::runtime::resources::LocalSceneAssets;
use crate::runtime::resources::{
    AssetLoadingGate, DialogueLengthCache, EditorSyncSession, GameState, LocalAssetManifest,
};
#[cfg(feature = "hot-reload")]
use crate::runtime::resources::{ContentProjectResource, ScriptLanguages, ScriptWatcherResource};
use crate::storage::settings::RuntimeSettings;
use crate::ui::control_bar::{ButtonAction, SkipMode, ToggleStates};
use crate::ui::input_scope::UiInputScope;

#[derive(Default)]
struct TypewriterClock {
    scene: String,
    cursor: usize,
    dialogue_length: DialogueLengthCache,
    fractional_chars: f64,
    next_pause: usize,
    pause: TypewriterPause,
}

#[derive(Default)]
enum TypewriterPause {
    #[default]
    Idle,
    Timed(f64),
    Input,
}

#[cfg(feature = "hot-reload")]
#[derive(Default)]
struct EditorCursorSync {
    remaining_frames: u8,
    poll_elapsed: f32,
    last: Option<keine_loader::ProjectDebugCursor>,
    force: bool,
}

#[cfg(feature = "hot-reload")]
#[derive(Default)]
struct HotReloadPipeline {
    running: Option<RunningHotReload>,
    pending_change_count: usize,
}

#[cfg(feature = "hot-reload")]
struct RunningHotReload {
    change_count: usize,
    worker: std::thread::JoinHandle<anyhow::Result<HotReloadBuild>>,
}

#[cfg(feature = "hot-reload")]
struct HotReloadBuild {
    config: Option<keine_core::config::GameConfig>,
    manifest: LocalAssetManifest,
    program: Program,
    diagnostics: Vec<(std::path::PathBuf, Diagnostic)>,
}

#[cfg(feature = "hot-reload")]
const EDITOR_CURSOR_POLL_SECONDS: f32 = 0.2;
/// Backspace is deliberately more deliberate than forward text reveal.
/// Preserve a response to the user's text-speed preference, but keep a hard
/// visual range so a short suffix cannot disappear in one or two frames.
const DIALOGUE_RETRACTION_SPEED_SCALE: f64 = 0.30;
const DIALOGUE_RETRACTION_MIN_CPS: f64 = 6.0;
const DIALOGUE_RETRACTION_MAX_CPS: f64 = 12.0;

fn dialogue_retraction_speed(typewriter_speed: f64) -> f64 {
    (typewriter_speed * DIALOGUE_RETRACTION_SPEED_SCALE)
        .clamp(DIALOGUE_RETRACTION_MIN_CPS, DIALOGUE_RETRACTION_MAX_CPS)
}

type StageButtonQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        Option<&'static ButtonAction>,
        &'static ComputedNode,
        &'static UiGlobalTransform,
        &'static InheritedVisibility,
    ),
    With<Button>,
>;

#[derive(SystemParam)]
pub struct TickContext<'w, 's> {
    time: Res<'w, Time>,
    state: ResMut<'w, GameState>,
    checkpoint: ResMut<'w, crate::storage::save::ContinuationCheckpoint>,
    settings: ResMut<'w, RuntimeSettings>,
    #[cfg(feature = "hot-reload")]
    content: Res<'w, ContentProjectResource>,
    #[cfg(feature = "hot-reload")]
    config: ResMut<'w, crate::runtime::resources::GameConfigResource>,
    #[cfg(feature = "hot-reload")]
    languages: Res<'w, ScriptLanguages>,
    actions: Res<'w, InputActions>,
    #[cfg(feature = "hot-reload")]
    watcher: Option<Res<'w, ScriptWatcherResource>>,
    #[cfg(feature = "hot-reload")]
    asset_manifest: ResMut<'w, LocalAssetManifest>,
    #[cfg(feature = "hot-reload")]
    image_roles: ResMut<'w, crate::scene::images::ImageRoleRegistry>,
    toggles: ResMut<'w, ToggleStates>,
    buttons: StageButtonQuery<'w, 's>,
    windows: Query<'w, 's, &'static Window>,
    input_scope: Res<'w, UiInputScope>,
    loading: Res<'w, AssetLoadingGate>,
    editor_sync: Option<Res<'w, EditorSyncSession>>,
    auto_timer: Local<'s, f64>,
    typewriter_clock: Local<'s, TypewriterClock>,
    #[cfg(feature = "hot-reload")]
    editor_cursor_sync: Local<'s, EditorCursorSync>,
    #[cfg(feature = "hot-reload")]
    hot_reload: Local<'s, HotReloadPipeline>,
    commands: Commands<'w, 's>,
}

/// Advances input, text timing, script hot reload, and transition state.
pub fn tick(mut context: TickContext) {
    let delta_seconds = context.time.delta_secs_f64();
    #[cfg(feature = "hot-reload")]
    let mut state_changed = reload_scripts_if_changed(&mut context, delta_seconds as f32);
    #[cfg(not(feature = "hot-reload"))]
    let mut state_changed = false;
    if context.loading.blocked {
        if context.toggles.skip {
            context.toggles.skip = false;
        }
        if state_changed {
            context.state.set_changed();
        }
        return;
    }
    if *context.input_scope != UiInputScope::Stage {
        if context.toggles.skip {
            context.toggles.skip = false;
        }
        if state_changed {
            context.state.set_changed();
        }
        return;
    }
    if context.editor_sync.is_none() {
        update_toggle_shortcuts(
            &context.actions,
            &mut context.toggles,
            &mut context.auto_timer,
        );
    }
    let presentation_was_blocked = context.state.presentation_blocked();
    if context.editor_sync.is_none() && context.actions.skip_video {
        let before = context.state.videos.len();
        context
            .state
            .videos
            .retain(|_, video| !video.spec.skippable || video.spec.looped);
        state_changed |= before != context.state.videos.len();
    }
    let presentation_advance = context.editor_sync.is_none()
        && advance_requested(
            &context.actions,
            &context.buttons,
            &context.windows,
            context.toggles.hide,
        );
    let typewriter_speed = context
        .state
        .active_typewriter_speed
        .unwrap_or(context.settings.typewriter_speed);
    state_changed |= step::update_dialogue_retraction(
        context.state.bypass_change_detection(),
        delta_seconds,
        dialogue_retraction_speed(typewriter_speed),
        presentation_advance,
        context.toggles.skip,
    );
    state_changed |= update_transitions(
        context.state.bypass_change_detection(),
        delta_seconds as f32,
        presentation_advance,
    );
    if presentation_was_blocked {
        if context.editor_sync.is_none() && !context.state.presentation_blocked() {
            let progress = step_once(
                context.state.bypass_change_detection(),
                &mut context.checkpoint,
            );
            state_changed |= progress.changed;
            if progress.return_to_title {
                request_return_to_title(&mut context.commands);
            }
        }
        if state_changed {
            context.state.set_changed();
        }
        return;
    }
    if context.toggles.skip {
        let progress = skip_once(
            context.state.bypass_change_detection(),
            &mut context.toggles,
            &mut context.checkpoint,
        );
        state_changed |= progress.changed;
        if progress.return_to_title {
            request_return_to_title(&mut context.commands);
        }
        if state_changed {
            context.state.set_changed();
        }
        return;
    }

    state_changed |= update_typewriter(
        context.state.bypass_change_detection(),
        delta_seconds,
        typewriter_speed,
        &mut context.typewriter_clock,
    );
    if context.editor_sync.is_some() {
        if context.toggles.auto {
            context.toggles.auto = false;
        }
        if context.toggles.skip {
            context.toggles.skip = false;
        }
        if state_changed {
            context.state.set_changed();
        }
        return;
    }
    let target_chars = dialogue_target(&context.state, &mut context.typewriter_clock);
    let notend = update_notend(
        context.state.bypass_change_detection(),
        target_chars,
        &mut context.checkpoint,
    );
    state_changed |= notend.changed;
    let target_chars = dialogue_target(&context.state, &mut context.typewriter_clock);
    let auto = update_auto_mode(
        context.state.bypass_change_detection(),
        context.toggles.auto,
        delta_seconds,
        context.settings.auto_delay,
        &mut context.auto_timer,
        target_chars,
        &mut context.checkpoint,
    );
    state_changed |= auto.changed;
    if notend.return_to_title || auto.return_to_title {
        request_return_to_title(&mut context.commands);
    }

    if advance_requested(
        &context.actions,
        &context.buttons,
        &context.windows,
        context.toggles.hide,
    ) {
        let progress = advance_once(
            context.state.bypass_change_detection(),
            &mut context.checkpoint,
        );
        state_changed |= progress.changed;
        if progress.return_to_title {
            request_return_to_title(&mut context.commands);
        }
        *context.auto_timer = 0.0;
    }

    if state_changed {
        context.state.set_changed();
    }
}

#[derive(Clone, Copy, Default)]
struct TickProgress {
    changed: bool,
    return_to_title: bool,
}

fn request_return_to_title(commands: &mut Commands) {
    commands.insert_resource(crate::ui::title::ReturnToTitleTransition::default());
}

fn dialogue_target(state: &State, clock: &mut TypewriterClock) -> usize {
    state
        .dialogue
        .as_ref()
        .map_or(0, |dialogue| clock.dialogue_length.count(&dialogue.text))
}

fn update_notend(
    state: &mut State,
    target_chars: usize,
    checkpoint: &mut crate::storage::save::ContinuationCheckpoint,
) -> TickProgress {
    let should_advance = state
        .dialogue
        .as_ref()
        .is_some_and(|dialogue| dialogue.auto_advance && dialogue.visible_chars >= target_chars);
    if should_advance {
        return advance_once(state, checkpoint);
    }
    TickProgress::default()
}

fn update_toggle_shortcuts(
    actions: &InputActions,
    toggles: &mut ToggleStates,
    auto_timer: &mut f64,
) {
    if actions.skip_held && !toggles.skip {
        toggles.skip = true;
    }
    if actions.skip_released && toggles.skip {
        toggles.skip = false;
    }
    if actions.toggle_auto {
        toggles.auto = !toggles.auto;
        *auto_timer = 0.0;
    }
    if actions.toggle_skip {
        toggles.skip = !toggles.skip;
    }
}

#[cfg(feature = "hot-reload")]
fn reload_scripts_if_changed(context: &mut TickContext<'_, '_>, delta_seconds: f32) -> bool {
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
                    apply_hot_reload(
                        build,
                        context.state.bypass_change_detection(),
                        &mut context.asset_manifest,
                        &mut context.config,
                        &mut context.image_roles,
                    );
                    changed = true;
                    log::info!("reloaded {change_count} changed project source(s)");
                    if editor_sync {
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
    changed
}

#[cfg(feature = "hot-reload")]
fn start_pending_reload(
    pipeline: &mut HotReloadPipeline,
    content: &keine_loader::ContentProject,
    languages: &keine_loader::ScriptLanguageRegistry,
) -> anyhow::Result<()> {
    use anyhow::Context as _;

    if pipeline.running.is_some() || pipeline.pending_change_count == 0 {
        return Ok(());
    }
    let content = content.clone();
    let languages = languages.clone();
    install_pending_reload(pipeline, || {
        std::thread::Builder::new()
            .name("keine-hot-reload".to_owned())
            .spawn(move || build_hot_reload(&content, &languages))
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

#[cfg(feature = "hot-reload")]
fn build_hot_reload(
    content: &keine_loader::ContentProject,
    languages: &keine_loader::ScriptLanguageRegistry,
) -> anyhow::Result<HotReloadBuild> {
    use anyhow::Context as _;

    let config = content
        .reload_config()
        .context("failed to reload project config")?;
    let scenes = keine_loader::load_scenes_with(content, languages)
        .context("failed to reload scripts from configured content sources")?;
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
    })
}

#[cfg(feature = "hot-reload")]
fn apply_hot_reload(
    build: HotReloadBuild,
    state: &mut State,
    asset_manifest: &mut LocalAssetManifest,
    config: &mut crate::runtime::resources::GameConfigResource,
    image_roles: &mut crate::scene::images::ImageRoleRegistry,
) {
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
    *asset_manifest = build.manifest;
    image_roles.rebuild(config, asset_manifest);
    restart_after_program_reload(state, build.program);
}

pub(crate) fn sync_editor_cursor(
    content: &keine_loader::ContentProject,
    state: &mut State,
    asset_manifest: &LocalAssetManifest,
) -> bool {
    match read_and_sync_editor_cursor(content, state, asset_manifest) {
        Ok(Some(changed)) => changed,
        Ok(None) => false,
        Err(error) => {
            log::warn!("failed to synchronize Studio debug position: {error:#}");
            false
        }
    }
}

#[cfg(feature = "hot-reload")]
fn try_sync_editor_cursor(
    content: &keine_loader::ContentProject,
    state: &mut State,
    asset_manifest: &LocalAssetManifest,
    last: &mut Option<keine_loader::ProjectDebugCursor>,
    force: bool,
) -> anyhow::Result<Option<bool>> {
    let Some(cursor) = content.debug_cursor()? else {
        *last = None;
        return Ok(None);
    };
    if !force && last.as_ref() == Some(&cursor) {
        return Ok(Some(false));
    }
    let changed = sync_editor_cursor_at(content, state, asset_manifest, &cursor)?;
    *last = Some(cursor);
    Ok(Some(changed))
}

fn read_and_sync_editor_cursor(
    content: &keine_loader::ContentProject,
    state: &mut State,
    asset_manifest: &LocalAssetManifest,
) -> anyhow::Result<Option<bool>> {
    let Some(cursor) = content.debug_cursor()? else {
        return Ok(None);
    };
    Ok(Some(sync_editor_cursor_at(
        content,
        state,
        asset_manifest,
        &cursor,
    )?))
}

fn sync_editor_cursor_at(
    content: &keine_loader::ContentProject,
    state: &mut State,
    asset_manifest: &LocalAssetManifest,
    cursor: &keine_loader::ProjectDebugCursor,
) -> anyhow::Result<bool> {
    let initial = content.initial_state()?;
    Ok(sync_editor_position(
        state,
        asset_manifest,
        &cursor.scene,
        cursor.source_step,
        initial,
    ))
}

fn sync_editor_position(
    state: &mut State,
    asset_manifest: &LocalAssetManifest,
    scene_name: &str,
    source_step: usize,
    initial: keine_loader::ProjectInitialState,
) -> bool {
    let Some(scene) = asset_manifest.get(scene_name) else {
        log::warn!("editor selected unknown fragment {scene_name:?}");
        return false;
    };
    let selected_start = scene
        .action_spans
        .iter()
        .position(|span| span.line >= source_step)
        .unwrap_or(scene.action_spans.len());
    let target = scene
        .action_spans
        .iter()
        .position(|span| span.line > source_step)
        .unwrap_or(scene.action_spans.len());
    let new_preview = || State {
        program: state.program.clone(),
        program_fingerprint: state.program_fingerprint,
        vars: initial.variables.clone(),
        global_vars: initial.shared_variables.clone(),
        ..State::new()
    };
    let mut preview = new_preview();
    preview.current_scene = crate::scene::entry_scene(&preview);
    preview.ended = false;

    // Editor previews reconstruct state from the project entry through the
    // selected block. Replaying only the selected fragment loses the scene,
    // characters and audio inherited from earlier chapters and is the reason
    // later chapters intermittently appeared to have missing resources.
    if !seek_editor_state(&mut preview, scene_name, selected_start, target) {
        // Some editor-only/title fragments are deliberately unreachable from
        // keine's normal entry. They still need direct block inspection.
        preview = new_preview();
        preview.current_scene = scene_name.to_owned();
        preview.ended = false;
        let _ = seek_editor_state(&mut preview, scene_name, selected_start, target);
    }
    log::info!(
        "editor seek · fragment {} · block {}",
        scene_name,
        source_step
    );
    preview.stage_revision = state.stage_revision.wrapping_add(1);
    *state = preview;
    true
}

const MAX_EDITOR_REPLAY_STEPS: usize = 65_536;
const MAX_EDITOR_PRESENTATION_STEPS: usize = 1_024;

pub(crate) fn seek_editor_state(
    preview: &mut State,
    target_scene: &str,
    selected_start: usize,
    target: usize,
) -> bool {
    for _ in 0..MAX_EDITOR_REPLAY_STEPS {
        if preview.current_scene == target_scene && preview.cursor >= target {
            return true;
        }
        let result = step::step_until_cursor(preview, target_scene, target);
        // A dialogue block may contain post-confirmation cleanup after `Say`
        // (for example LetsGal's `keepDialogue: false` textbox hide). Studio
        // selecting that block previews the line before confirmation, so do
        // not synthesize the click merely to reach the block's final action.
        if matches!(result, keine_core::StepResult::AwaitClick)
            && preview.current_scene == target_scene
            && preview.cursor > selected_start
            && preview.cursor <= target
        {
            return true;
        }
        // Replay prior blocks to completion, but preserve the selected block's
        // own yield. This lets its dialogue/typewriter, transition, timeline,
        // particle or video presentation run exactly once in the preview.
        if preview.current_scene == target_scene && preview.cursor >= target {
            return true;
        }
        match result {
            keine_core::StepResult::AwaitClick => step::advance(preview),
            keine_core::StepResult::AwaitPresentation => {
                if !finish_editor_presentation(preview) {
                    log::warn!("editor seek could not finish a prior presentation");
                    return false;
                }
            }
            keine_core::StepResult::AwaitInput => {
                let _ = step::submit_user_input(preview);
            }
            keine_core::StepResult::AwaitChoice => {
                let direct = preview.menu.as_ref().and_then(|menu| {
                    menu.choices.iter().position(|choice| {
                        choice.enabled
                            && match &choice.target {
                                keine_core::ChoiceTarget::ChangeScene(scene)
                                | keine_core::ChoiceTarget::CallScene(scene) => {
                                    scene == target_scene
                                }
                                keine_core::ChoiceTarget::Label(_) => false,
                            }
                    })
                });
                let fallback = preview
                    .menu
                    .as_ref()
                    .and_then(|menu| menu.choices.iter().position(|choice| choice.enabled));
                let Some(index) = direct.or(fallback) else {
                    return false;
                };
                step::select_choice(preview, index);
            }
            keine_core::StepResult::EndOfScene => {
                return preview.current_scene == target_scene && preview.cursor >= target;
            }
            keine_core::StepResult::ExecutionLimit => return false,
        }
    }
    log::warn!("editor seek exceeded the deterministic replay limit");
    false
}

fn finish_editor_presentation(preview: &mut State) -> bool {
    for _ in 0..MAX_EDITOR_PRESENTATION_STEPS {
        if !preview.presentation_blocked() {
            return true;
        }
        // A headless editor replay has no decoder callback that can finish a
        // blocking video. The selected block returns before this helper, so
        // only playback belonging to earlier source blocks is retired here.
        preview.videos.retain(|_, video| {
            !video.spec.wait_for_finished || video.spec.looped || video.stopping
        });
        // Prior sentence-tail deletions are deterministic editor history, so
        // finish them without manufacturing their separate player click.
        step::update_dialogue_retraction(preview, 0.0, 0.0, false, true);
        update_transitions(preview, 86_400.0, true);
    }
    !preview.presentation_blocked()
}

/// Re-enter one scene against the new Program without carrying presentation
/// or interaction state produced by the previous script fingerprint.
///
/// Development reload keeps local/global variables and durable gallery
/// unlocks so authors can iterate near the current branch. Execution frames,
/// read positions, backlog, stage, audio and open UI interactions are rebuilt
/// from the beginning of the selected scene.
#[cfg(feature = "hot-reload")]
fn restart_after_program_reload(state: &mut State, program: Program) {
    let next_stage_revision = state.stage_revision.wrapping_add(1);
    let previous_scene = state.current_scene.clone();
    let was_ended = state.ended;
    let vars = std::mem::take(&mut state.vars);
    let global_vars = std::mem::take(&mut state.global_vars);
    let unlocked_cg = std::mem::take(&mut state.unlocked_cg);
    let unlocked_bgm = std::mem::take(&mut state.unlocked_bgm);

    let mut restarted = State {
        vars,
        global_vars,
        unlocked_cg,
        unlocked_bgm,
        ..State::new()
    };
    restarted.install_program(program);
    restarted.current_scene = if restarted.program.contains_scene(&previous_scene) {
        previous_scene
    } else {
        crate::scene::entry_scene(&restarted)
    };
    restarted.ended = was_ended || restarted.current_scene.is_empty();
    restarted.effect_queue.push(keine_core::EffectEvent::Stop);
    restarted.stage_revision = next_stage_revision;
    if !restarted.ended {
        crate::runtime::script_driver::resume_for_tooling(&mut restarted);
    }
    *state = restarted;
}

fn skip_once(
    state: &mut State,
    toggles: &mut ToggleStates,
    checkpoint: &mut crate::storage::save::ContinuationCheckpoint,
) -> TickProgress {
    if toggles.skip_mode == SkipMode::Read && !state.current_dialogue_is_read() {
        toggles.skip = false;
        return TickProgress::default();
    }
    advance_once(state, checkpoint)
}

fn update_typewriter(
    state: &mut State,
    delta_seconds: f64,
    chars_per_second: f64,
    clock: &mut TypewriterClock,
) -> bool {
    let dialogue_changed = clock.scene != state.current_scene || clock.cursor != state.cursor;
    if dialogue_changed {
        clock.scene.clone_from(&state.current_scene);
        clock.cursor = state.cursor;
        clock.fractional_chars = 0.0;
        clock.next_pause = 0;
        clock.pause = TypewriterPause::Idle;
    }

    let Some(dialogue) = &mut state.dialogue else {
        clock.fractional_chars = 0.0;
        return false;
    };
    let target = clock.dialogue_length.count(&dialogue.text);
    if dialogue.visible_chars >= target {
        clock.next_pause = dialogue.pauses.len();
        clock.pause = TypewriterPause::Idle;
        return false;
    }
    // WebGAL K starts the first glyph at delay 0. Avoid making a new line
    // feel unresponsive while waiting for the first full character period.
    if dialogue_changed && target > 0 {
        if dialogue
            .pauses
            .get(clock.next_pause)
            .is_some_and(|pause| pause.at == 0)
        {
            start_inline_pause(dialogue, clock);
            return false;
        }
        let previous = dialogue.visible_chars;
        let pause_at = dialogue
            .pauses
            .get(clock.next_pause)
            .map_or(target, |pause| pause.at);
        dialogue.visible_chars = dialogue.visible_chars.max(1).min(pause_at);
        if dialogue.visible_chars == pause_at && pause_at < target {
            start_inline_pause(dialogue, clock);
        }
        return dialogue.visible_chars != previous;
    }

    let speed = chars_per_second.max(0.0);
    let mut remaining = delta_seconds.max(0.0);
    let mut changed = false;
    let iteration_limit = dialogue.pauses.len().saturating_mul(2).saturating_add(2);
    for _ in 0..iteration_limit {
        match &mut clock.pause {
            TypewriterPause::Timed(wait) if remaining < *wait => {
                *wait -= remaining;
                break;
            }
            TypewriterPause::Timed(wait) => {
                remaining -= *wait;
                clock.pause = TypewriterPause::Idle;
                clock.next_pause += 1;
                continue;
            }
            TypewriterPause::Input => break,
            TypewriterPause::Idle => {}
        }

        if dialogue
            .pauses
            .get(clock.next_pause)
            .is_some_and(|pause| pause.at <= dialogue.visible_chars)
        {
            start_inline_pause(dialogue, clock);
            continue;
        }
        if remaining <= 0.0 || speed <= 0.0 {
            break;
        }

        let pause_at = dialogue
            .pauses
            .get(clock.next_pause)
            .map_or(target, |pause| pause.at.min(target));
        let capacity = pause_at.saturating_sub(dialogue.visible_chars);
        if capacity == 0 {
            start_inline_pause(dialogue, clock);
            continue;
        }
        let exact_chars = clock.fractional_chars + remaining * speed;
        let added = exact_chars.floor() as usize;
        if added < capacity {
            if added > 0 {
                dialogue.visible_chars += added;
                changed = true;
            }
            clock.fractional_chars = exact_chars.fract();
            break;
        }

        let seconds_used = (capacity as f64 - clock.fractional_chars).max(0.0) / speed;
        remaining = (remaining - seconds_used).max(0.0);
        dialogue.visible_chars = pause_at;
        clock.fractional_chars = 0.0;
        changed = true;
        if pause_at >= target {
            break;
        }
        start_inline_pause(dialogue, clock);
    }
    changed
}

fn start_inline_pause(dialogue: &keine_core::state::Dialogue, clock: &mut TypewriterClock) {
    clock.pause = match dialogue.pauses[clock.next_pause].duration {
        Some(seconds) => TypewriterPause::Timed(f64::from(seconds.max(0.0))),
        None => TypewriterPause::Input,
    };
}

fn update_auto_mode(
    state: &mut State,
    enabled: bool,
    delta_seconds: f64,
    delay: f64,
    timer: &mut f64,
    target_chars: usize,
    checkpoint: &mut crate::storage::save::ContinuationCheckpoint,
) -> TickProgress {
    if !enabled {
        *timer = 0.0;
        return TickProgress::default();
    }

    let ready = state
        .dialogue
        .as_ref()
        .is_none_or(|dialogue| dialogue.visible_chars >= target_chars);
    if !ready {
        *timer = 0.0;
        return TickProgress::default();
    }

    *timer += delta_seconds;
    if *timer >= delay {
        *timer = 0.0;
        return advance_once(state, checkpoint);
    }
    TickProgress::default()
}

fn advance_requested(
    actions: &InputActions,
    buttons: &StageButtonQuery<'_, '_>,
    windows: &Query<&Window>,
    content_hidden: bool,
) -> bool {
    if !actions.advance {
        return false;
    }
    if actions.pointer_advance
        && buttons.iter().any(|(interaction, _, _, _, visibility)| {
            visibility.get() && !matches!(interaction, Interaction::None)
        })
    {
        return false;
    }
    if actions.pointer_advance
        && windows
            .single()
            .ok()
            .and_then(Window::physical_cursor_position)
            .is_some_and(|cursor| {
                buttons.iter().any(|(_, _, node, transform, visibility)| {
                    visibility.get()
                        && point_inside_rect(cursor, transform.translation, node.size())
                })
            })
    {
        return false;
    }
    !buttons
        .iter()
        .any(|(interaction, action, _, _, visibility)| {
            visibility.get()
                && matches!(interaction, Interaction::Pressed)
                && (!content_hidden || matches!(action, Some(ButtonAction::Hide)))
        })
}

fn point_inside_rect(point: Vec2, center: Vec2, size: Vec2) -> bool {
    size.x > 0.0
        && size.y > 0.0
        && (point.x - center.x).abs() <= size.x * 0.5
        && (point.y - center.y).abs() <= size.y * 0.5
}

fn advance_once(
    state: &mut State,
    checkpoint: &mut crate::storage::save::ContinuationCheckpoint,
) -> TickProgress {
    if let Some(dialogue) = &mut state.dialogue {
        let target = dialogue.text.chars().count();
        if dialogue.visible_chars < target {
            dialogue.visible_chars = target;
            return TickProgress {
                changed: true,
                return_to_title: false,
            };
        }
    }
    let advanced_dialogue = state.dialogue.is_some();
    if advanced_dialogue {
        step::advance(state);
    }
    finish_step(state, checkpoint, advanced_dialogue)
}

fn step_once(
    state: &mut State,
    checkpoint: &mut crate::storage::save::ContinuationCheckpoint,
) -> TickProgress {
    finish_step(state, checkpoint, false)
}

fn finish_step(
    state: &mut State,
    checkpoint: &mut crate::storage::save::ContinuationCheckpoint,
    restore_previous_dialogue: bool,
) -> TickProgress {
    let outcome = crate::runtime::script_driver::resume(state, checkpoint);
    let return_to_title = outcome.returns_to_title();
    if return_to_title && restore_previous_dialogue && state.dialogue.is_none() {
        // `advance` moves the settled line here before stepping. Moving it
        // back retains the final text without cloning the complete State.
        state.dialogue = state.previous_dialogue.take();
    }
    TickProgress {
        changed: true,
        return_to_title,
    }
}

fn update_transitions(state: &mut State, delta_seconds: f32, advance_intro: bool) -> bool {
    let mut changed = false;
    let mut stage_changed = false;
    if state.waiting_for_advance && advance_intro {
        state.waiting_for_advance = false;
        changed = true;
    }
    if state.wait_remaining > 0.0 {
        state.wait_remaining = (state.wait_remaining - delta_seconds).max(0.0);
        changed = true;
    }
    if let Some(intro) = &mut state.intro {
        intro.elapsed += delta_seconds;
        changed = true;
        let advance = advance_intro || (!intro.hold && intro.elapsed >= 1.6);
        if advance {
            if intro.page + 1 < intro.pages.len() {
                intro.page += 1;
                intro.elapsed = 0.0;
            } else {
                state.intro = None;
            }
        }
    }
    if (state.curtain.current - state.curtain.target).abs() > f32::EPSILON {
        changed = true;
        state.curtain.elapsed = (state.curtain.elapsed + delta_seconds).min(state.curtain.duration);
        let progress = if state.curtain.duration <= f32::EPSILON {
            1.0
        } else {
            (state.curtain.elapsed / state.curtain.duration).clamp(0.0, 1.0)
        };
        let eased = progress * progress * (3.0 - 2.0 * progress);
        state.curtain.current =
            state.curtain.from + (state.curtain.target - state.curtain.from) * eased;
        if progress >= 1.0 {
            state.curtain.current = state.curtain.target;
            state.curtain.blocking = false;
        }
    }
    for mask in state.stage_masks.values_mut() {
        if (mask.current - mask.target).abs() <= f32::EPSILON {
            continue;
        }
        changed = true;
        stage_changed = true;
        mask.elapsed = (mask.elapsed + delta_seconds).min(mask.duration);
        let progress = if mask.duration <= f32::EPSILON {
            1.0
        } else {
            (mask.elapsed / mask.duration).clamp(0.0, 1.0)
        };
        let eased = progress * progress * (3.0 - 2.0 * progress);
        mask.current = mask.from + (mask.target - mask.from) * eased;
        if progress >= 1.0 {
            mask.current = mask.target;
            mask.blocking = false;
        }
    }
    let mask_count = state.stage_masks.len();
    state
        .stage_masks
        .retain(|_, mask| mask.target > f32::EPSILON || mask.current > f32::EPSILON);
    if state.stage_masks.len() != mask_count {
        changed = true;
        stage_changed = true;
    }
    if let Some(text) = &mut state.floating_text {
        changed = true;
        text.elapsed += delta_seconds;
        if !text.infinite && text.elapsed >= text.duration() {
            state.floating_text = None;
        }
    }
    for effect in state.particle_effects.values_mut() {
        if effect.fading_out || effect.elapsed < effect.effect.fade_in {
            effect.elapsed += delta_seconds;
            changed = true;
        }
    }
    let effect_count = state.particle_effects.len();
    state
        .particle_effects
        .retain(|_, effect| !effect.finished());
    changed |= state.particle_effects.len() != effect_count;

    if advance_camera_transitions(state, delta_seconds) {
        changed = true;
        stage_changed = true;
    }

    for video in state.videos.values_mut() {
        video.elapsed += delta_seconds;
        if video.stopping {
            changed = true;
            let fade = video.fade_out.max(f32::EPSILON);
            video.opacity = (video.opacity - video.spec.alpha * delta_seconds / fade).max(0.0);
        }
    }
    let video_count = state.videos.len();
    state.videos.retain(|_, video| video.opacity > 0.0);
    changed |= state.videos.len() != video_count;

    if advance_sprite_transitions(state, delta_seconds) {
        changed = true;
        stage_changed = true;
    }

    changed |= advance_background_transitions(state, delta_seconds);

    if state.stage_animation.is_some() {
        changed = true;
        stage_changed = true;
        advance_stage_animation(state, delta_seconds);
    }

    let avatar_delta = delta_seconds * 3.0;
    if state.mini_avatar.is_some() {
        if state.mini_avatar_progress < 1.0 {
            changed = true;
            state.mini_avatar_progress = (state.mini_avatar_progress + avatar_delta).min(1.0);
        }
    } else {
        if state.mini_avatar_progress > 0.0 {
            changed = true;
            state.mini_avatar_progress = (state.mini_avatar_progress - avatar_delta).max(0.0);
        }
    }
    if stage_changed {
        state.invalidate_stage();
    }
    changed
}

fn advance_camera_transitions(state: &mut State, delta_seconds: f32) -> bool {
    let mut changed = false;
    if let Some(mut animation) = state.camera_effect_animation.take() {
        changed = true;
        animation.elapsed = (animation.elapsed + delta_seconds).min(animation.duration);
        let progress = animation
            .easing
            .sample(animation.elapsed / animation.duration.max(f32::EPSILON));
        state.camera_effect = animation.from.interpolate(&animation.to, progress);
        if animation.elapsed < animation.duration {
            state.camera_effect_animation = Some(animation);
        } else {
            state.camera_effect = animation.to;
        }
    }

    if let Some(mut animation) = state.camera_transform_animation.take() {
        changed = true;
        animation.elapsed = (animation.elapsed + delta_seconds).min(animation.duration);
        let progress = animation
            .easing
            .sample(animation.elapsed / animation.duration.max(f32::EPSILON));
        state.camera_transform = animation.from.lerp(animation.to, progress);
        if animation.elapsed < animation.duration {
            state.camera_transform_animation = Some(animation);
        } else {
            state.camera_transform = animation.to;
        }
    }

    let shake_finished = if let Some(shake) = &mut state.camera_shake {
        use keine_core::{CameraShakeAxis, CameraShakeFalloff};

        changed = true;
        shake.elapsed = (shake.elapsed + delta_seconds).min(shake.spec.duration);
        let progress = shake.elapsed / shake.spec.duration.max(f32::EPSILON);
        let envelope = match shake.spec.falloff {
            CameraShakeFalloff::Linear => 1.0 - progress,
            CameraShakeFalloff::Exponential => (1.0 - progress).powi(2),
        };
        let phase = std::f32::consts::TAU * shake.spec.frequency * shake.elapsed;
        let amplitude = shake.spec.amplitude * envelope;
        shake.offset_x = if shake.spec.axis == CameraShakeAxis::Y {
            0.0
        } else {
            amplitude * phase.sin()
        };
        shake.offset_y = if shake.spec.axis == CameraShakeAxis::X {
            0.0
        } else {
            amplitude * (phase + std::f32::consts::FRAC_PI_3).sin()
        };
        shake.elapsed >= shake.spec.duration
    } else {
        false
    };
    if shake_finished {
        state.camera_shake = None;
    }
    changed
}

fn advance_sprite_transitions(state: &mut State, delta_seconds: f32) -> bool {
    let mut changed = false;
    for (id, sequence) in &mut state.sprite_sequences {
        if sequence.frames.len() < 2 || !state.sprites.contains_key(id) {
            continue;
        }
        sequence.elapsed += delta_seconds;
        let sampled = (sequence.elapsed * sequence.fps).floor() as usize;
        let frame = if sequence.looped {
            sampled % sequence.frames.len()
        } else {
            sampled.min(sequence.frames.len() - 1)
        };
        if frame != sequence.frame {
            sequence.frame = frame;
            if let Some(sprite) = state.sprites.get_mut(id) {
                sprite.image.clone_from(&sequence.frames[frame]);
                changed = true;
            }
        }
    }

    for sprite in state.sprites.values_mut() {
        let keyframes_active = sprite.keyframe_animation.is_some();
        changed |= keyframes_active;
        let keyframes_finished = sprite.keyframe_animation.as_mut().is_some_and(|animation| {
            advance_keyframes(&mut sprite.transform, animation, delta_seconds)
        });
        if keyframes_finished {
            sprite.keyframe_animation = None;
        }
        if let Some(animation) = &mut sprite.transform_animation {
            changed = true;
            animation.elapsed = (animation.elapsed + delta_seconds).min(animation.duration);
            let progress = animation
                .easing
                .sample(animation.elapsed / animation.duration);
            sprite.transform = animation.from.lerp(animation.to, progress);
            if animation.elapsed >= animation.duration {
                sprite.transform_animation = None;
            }
        }
        if let Some(animation) = &mut sprite.position_animation {
            changed = true;
            animation.elapsed = (animation.elapsed + delta_seconds).min(animation.duration);
            if animation.elapsed >= animation.duration {
                sprite.position_animation = None;
            }
        }
        if let Some(animation) = &mut sprite.animation {
            changed = true;
            animation.elapsed = (animation.elapsed + delta_seconds).min(animation.duration);
            let progress = (animation.elapsed / animation.duration).clamp(0.0, 1.0);
            sprite.transform = sample_preset(animation.base, &animation.preset, progress);
            if animation.elapsed >= animation.duration {
                let exiting = animation.remove_on_finish;
                sprite.transform = if exiting {
                    let mut transform = animation.base;
                    transform.alpha = 0.0;
                    transform
                } else {
                    preset_final_transform(animation.base, &animation.preset)
                };
                sprite.animation = None;
                if exiting {
                    sprite.entering = false;
                    sprite.transition_progress = 0.0;
                }
            }
        }
        let delta = sprite
            .transition
            .duration()
            .map_or(1.0, |duration| delta_seconds / duration.max(f32::EPSILON));
        if sprite.entering && sprite.transition_progress < 1.0 {
            changed = true;
            sprite.transition_progress = (sprite.transition_progress + delta).min(1.0);
        } else if !sprite.entering && sprite.transition_progress > 0.0 {
            changed = true;
            sprite.transition_progress = (sprite.transition_progress - delta).max(0.0);
        }
    }
    let sprite_count = state.sprites.len();
    state
        .sprites
        .retain(|_, sprite| sprite.entering || sprite.transition_progress > 0.0);
    let sprites_removed = state.sprites.len() != sprite_count;
    state
        .sprite_sequences
        .retain(|id, _| state.sprites.contains_key(id));
    changed || sprites_removed
}

fn advance_background_transitions(state: &mut State, delta_seconds: f32) -> bool {
    let mut changed = false;
    let transition_finished = if let Some(transition) = &mut state.bg_transition {
        changed = true;
        let delta = transition
            .kind
            .duration()
            .map_or(1.0, |duration| delta_seconds / duration.max(f32::EPSILON));
        transition.progress = (transition.progress + delta).min(1.0);
        transition.progress >= 1.0
    } else {
        false
    };
    if transition_finished {
        if state
            .bg_transition
            .as_ref()
            .is_some_and(|transition| transition.to.is_empty())
        {
            state.bg_camera_distance = None;
        }
        state.bg_transition = None;
    }

    changed |= state.bg_keyframe_animation.is_some();
    let bg_keyframes_finished = state
        .bg_keyframe_animation
        .as_mut()
        .is_some_and(|animation| {
            advance_keyframes(&mut state.bg_transform, animation, delta_seconds)
        });
    if bg_keyframes_finished {
        state.bg_keyframe_animation = None;
    }

    if let Some(animation) = &mut state.bg_transform_animation {
        changed = true;
        animation.elapsed = (animation.elapsed + delta_seconds).min(animation.duration);
        let progress = animation
            .easing
            .sample(animation.elapsed / animation.duration);
        state.bg_transform = animation.from.lerp(animation.to, progress);
        if animation.elapsed >= animation.duration {
            state.bg_transform_animation = None;
        }
    }

    if let Some(animation) = &mut state.bg_animation {
        changed = true;
        animation.elapsed = (animation.elapsed + delta_seconds).min(animation.duration);
        let progress = (animation.elapsed / animation.duration).clamp(0.0, 1.0);
        state.bg_transform = sample_preset(animation.base, &animation.preset, progress);
        if animation.elapsed >= animation.duration {
            let exiting = animation.remove_on_finish;
            state.bg_transform = preset_final_transform(animation.base, &animation.preset);
            state.bg_animation = None;
            if exiting {
                state.bg = None;
            }
        }
    }
    changed
}

fn advance_stage_animation(state: &mut State, delta_seconds: f32) {
    let Some(mut runtime) = state.stage_animation.take() else {
        return;
    };
    let duration = runtime.animation.duration.max(f32::EPSILON);
    let total = if runtime.animation.infinite {
        f32::INFINITY
    } else {
        duration * (runtime.animation.repeat.saturating_add(1) as f32)
    };
    runtime.previous_elapsed = runtime.elapsed;
    runtime.elapsed = (runtime.elapsed
        + delta_seconds.max(0.0) * runtime.animation.playback_rate.max(f32::EPSILON))
    .min(total);
    let finished = runtime.elapsed >= total;
    let local_time = if finished {
        duration
    } else {
        runtime.elapsed % duration
    };

    reset_stage_camera_patches(state, &runtime);
    apply_stage_tracks(state, &mut runtime, local_time);
    apply_stage_camera_patches(state, &runtime, local_time);
    trigger_stage_events(state, &runtime);

    if !finished {
        state.stage_animation = Some(runtime);
    }
}

fn apply_stage_tracks(
    state: &mut State,
    runtime: &mut keine_core::StageAnimationState,
    local_time: f32,
) {
    for index in 0..runtime.animation.tracks.len() {
        let track = &runtime.animation.tracks[index];
        if track.muted || track.keyframes.is_empty() {
            continue;
        }
        if runtime.initial_values[index].is_none() {
            let Some(value) = read_stage_value(state, &track.target, track.property) else {
                // Scene layers can be introduced by a later scene cue.
                continue;
            };
            runtime.initial_values[index] = Some(value);
            runtime.track_start_times[index] = local_time;
        }
        let value = sample_stage_track(
            track,
            local_time,
            runtime.initial_values[index].unwrap_or_default(),
            runtime.track_start_times[index],
        );
        write_stage_value(state, &track.target, track.property, value);
    }
}

fn sample_stage_track(
    track: &keine_core::StageTrack,
    time: f32,
    initial: f32,
    start_time: f32,
) -> f32 {
    let frames = &track.keyframes;
    let first = frames[0];
    let time = time.max(0.0);
    if time <= first.time {
        let start = start_time.clamp(0.0, first.time);
        if first.time <= start {
            return first.value;
        }
        let progress = ((time.max(start) - start) / (first.time - start)).clamp(0.0, 1.0);
        return initial + (first.value - initial) * first.easing.sample(progress);
    }
    for pair in frames.windows(2) {
        let from = pair[0];
        let to = pair[1];
        if time > to.time {
            continue;
        }
        let span = to.time - from.time;
        if span <= f32::EPSILON {
            return to.value;
        }
        let progress = ((time - from.time) / span).clamp(0.0, 1.0);
        return from.value + (to.value - from.value) * to.easing.sample(progress);
    }
    frames.last().map_or(initial, |frame| frame.value)
}

fn read_stage_value(
    state: &State,
    target: &keine_core::StageTarget,
    property: keine_core::StageProperty,
) -> Option<f32> {
    use keine_core::{StageProperty as P, StageTarget};
    let id = match target {
        StageTarget::Camera => {
            return Some(match property {
                P::X => state.camera_transform.offset_x,
                P::Y => state.camera_transform.offset_y,
                P::Zoom | P::ScaleX => state.camera_transform.scale_x,
                P::ScaleY => state.camera_transform.scale_y,
                property => read_stage_effect(&state.camera_effect, property)?,
            });
        }
        StageTarget::Character { id, .. } | StageTarget::SceneLayer { id } => id,
    };
    let sprite = state.sprites.get(id)?;
    Some(match property {
        P::X => sprite.transform.offset_x,
        P::Y => sprite.transform.offset_y,
        P::ScaleX | P::Zoom => sprite.transform.scale_x,
        P::ScaleY => sprite.transform.scale_y,
        P::Alpha => sprite.transform.alpha,
        P::Rotation => sprite.transform.rotation,
        P::Width => sprite.transform.width,
        P::Height => sprite.transform.height,
        _ => return None,
    })
}

fn write_stage_value(
    state: &mut State,
    target: &keine_core::StageTarget,
    property: keine_core::StageProperty,
    value: f32,
) {
    use keine_core::{StageProperty as P, StageTarget};
    let id = match target {
        StageTarget::Camera => {
            match property {
                P::X => state.camera_transform.offset_x = value,
                P::Y => state.camera_transform.offset_y = value,
                P::Zoom => {
                    state.camera_transform.scale_x = value;
                    state.camera_transform.scale_y = value;
                }
                P::ScaleX => state.camera_transform.scale_x = value,
                P::ScaleY => state.camera_transform.scale_y = value,
                property => write_stage_effect(&mut state.camera_effect, property, value),
            }
            return;
        }
        StageTarget::Character { id, .. } | StageTarget::SceneLayer { id } => id,
    };
    let Some(sprite) = state.sprites.get_mut(id) else {
        return;
    };
    match property {
        P::X => sprite.transform.offset_x = value,
        P::Y => sprite.transform.offset_y = value,
        P::ScaleX | P::Zoom => sprite.transform.scale_x = value,
        P::ScaleY => sprite.transform.scale_y = value,
        P::Alpha => sprite.transform.alpha = value.clamp(0.0, 1.0),
        P::Rotation => sprite.transform.rotation = value,
        P::Width => sprite.transform.width = value.max(0.0),
        P::Height => sprite.transform.height = value.max(0.0),
        _ => {}
    }
}

fn read_stage_effect(
    effect: &keine_core::PostProcessEffect,
    property: keine_core::StageProperty,
) -> Option<f32> {
    use keine_core::StageProperty as P;
    Some(match property {
        P::FocalDistance => effect.focal_distance.unwrap_or(0.0),
        P::BlurStrength => effect.blur_strength,
        P::DistortionStrength => effect.distortion_strength,
        P::VignetteIntensity => effect.vignette_intensity,
        P::VignetteSize => effect.vignette_size,
        P::BlurAmount => effect.blur_amount,
        P::ColorToneIntensity => effect.color_tone_intensity,
        P::ColorExposure => effect.color_exposure,
        P::ColorBrightness => effect.color_brightness,
        P::ColorContrast => effect.color_contrast,
        P::ColorSaturation => effect.color_saturation,
        P::ColorTemperature => effect.color_temperature,
        P::OldFilmIntensity => effect.old_film_intensity,
        P::ShockIntensity => effect.shock_intensity,
        P::GodrayIntensity => effect.godray_intensity,
        P::GodrayAngle => effect.godray_angle,
        P::GodrayGain => effect.godray_gain,
        P::GodrayLacunarity => effect.godray_lacunarity,
        P::GodraySpeed => effect.godray_speed,
        P::GodrayCenterX => effect.godray_center_x,
        P::GodrayCenterY => effect.godray_center_y,
        P::LutIntensity => effect.lut_intensity,
        P::BloomIntensity => effect.bloom_intensity,
        P::ChromaticAberration => effect.chromatic_aberration,
        P::PixelateSize => effect.pixelate_size,
        P::GlitchIntensity => effect.glitch_intensity,
        P::CrtIntensity => effect.crt_intensity,
        P::SharpenStrength => effect.sharpen_strength,
        P::RadialBlurStrength => effect.radial_blur_strength,
        P::RadialBlurCenterX => effect.radial_blur_center_x,
        P::RadialBlurCenterY => effect.radial_blur_center_y,
        P::MotionBlurStrength => effect.motion_blur_strength,
        P::MotionBlurAngle => effect.motion_blur_angle,
        P::ZoomBlurStrength => effect.zoom_blur_strength,
        P::ZoomBlurCenterX => effect.zoom_blur_center_x,
        P::ZoomBlurCenterY => effect.zoom_blur_center_y,
        P::LightLeakIntensity => effect.light_leak_intensity,
        P::LightLeakAngle => effect.light_leak_angle,
        P::LensFlareIntensity => effect.lens_flare_intensity,
        P::LensFlareCenterX => effect.lens_flare_center_x,
        P::LensFlareCenterY => effect.lens_flare_center_y,
        P::FilmGrainIntensity => effect.film_grain_intensity,
        P::FilmGrainSize => effect.film_grain_size,
        P::HeatHazeIntensity => effect.heat_haze_intensity,
        P::HeatHazeSpeed => effect.heat_haze_speed,
        P::HeatHazeScale => effect.heat_haze_scale,
        P::WaterRippleIntensity => effect.water_ripple_intensity,
        P::WaterRippleFrequency => effect.water_ripple_frequency,
        P::WaterRippleSpeed => effect.water_ripple_speed,
        P::WaterRippleCenterX => effect.water_ripple_center_x,
        P::WaterRippleCenterY => effect.water_ripple_center_y,
        P::FogIntensity => effect.fog_intensity,
        P::FogSpeed => effect.fog_speed,
        P::FogScale => effect.fog_scale,
        P::VhsIntensity => effect.vhs_intensity,
        P::VhsJitter => effect.vhs_jitter,
        P::VhsNoise => effect.vhs_noise,
        P::HalftoneIntensity => effect.halftone_intensity,
        P::HalftoneScale => effect.halftone_scale,
        P::HalftoneAngle => effect.halftone_angle,
        P::DitherIntensity => effect.dither_intensity,
        P::DitherLevels => effect.dither_levels,
        P::OutlineIntensity => effect.outline_intensity,
        P::OutlineThickness => effect.outline_thickness,
        P::EyelidOpenness => effect.eyelid_openness,
        P::EyelidWidth => effect.eyelid_width,
        P::EyelidCurvature => effect.eyelid_curvature,
        P::EyelidSoftness => effect.eyelid_softness,
        P::EyelidCenterX => effect.eyelid_center_x,
        P::EyelidCenterY => effect.eyelid_center_y,
        _ => return None,
    })
}

fn write_stage_effect(
    effect: &mut keine_core::PostProcessEffect,
    property: keine_core::StageProperty,
    value: f32,
) {
    use keine_core::StageProperty as P;
    match property {
        P::FocalDistance => effect.focal_distance = Some(value),
        P::BlurStrength => effect.blur_strength = value,
        P::DistortionStrength => effect.distortion_strength = value,
        P::VignetteIntensity => effect.vignette_intensity = value,
        P::VignetteSize => effect.vignette_size = value,
        P::BlurAmount => effect.blur_amount = value,
        P::ColorToneIntensity => effect.color_tone_intensity = value,
        P::ColorExposure => effect.color_exposure = value,
        P::ColorBrightness => effect.color_brightness = value,
        P::ColorContrast => effect.color_contrast = value,
        P::ColorSaturation => effect.color_saturation = value,
        P::ColorTemperature => effect.color_temperature = value,
        P::OldFilmIntensity => effect.old_film_intensity = value,
        P::ShockIntensity => effect.shock_intensity = value,
        P::GodrayIntensity => effect.godray_intensity = value,
        P::GodrayAngle => effect.godray_angle = value,
        P::GodrayGain => effect.godray_gain = value,
        P::GodrayLacunarity => effect.godray_lacunarity = value,
        P::GodraySpeed => effect.godray_speed = value,
        P::GodrayCenterX => effect.godray_center_x = value,
        P::GodrayCenterY => effect.godray_center_y = value,
        P::LutIntensity => effect.lut_intensity = value,
        P::BloomIntensity => effect.bloom_intensity = value,
        P::ChromaticAberration => effect.chromatic_aberration = value,
        P::PixelateSize => effect.pixelate_size = value,
        P::GlitchIntensity => effect.glitch_intensity = value,
        P::CrtIntensity => effect.crt_intensity = value,
        P::SharpenStrength => effect.sharpen_strength = value,
        P::RadialBlurStrength => effect.radial_blur_strength = value,
        P::RadialBlurCenterX => effect.radial_blur_center_x = value,
        P::RadialBlurCenterY => effect.radial_blur_center_y = value,
        P::MotionBlurStrength => effect.motion_blur_strength = value,
        P::MotionBlurAngle => effect.motion_blur_angle = value,
        P::ZoomBlurStrength => effect.zoom_blur_strength = value,
        P::ZoomBlurCenterX => effect.zoom_blur_center_x = value,
        P::ZoomBlurCenterY => effect.zoom_blur_center_y = value,
        P::LightLeakIntensity => effect.light_leak_intensity = value,
        P::LightLeakAngle => effect.light_leak_angle = value,
        P::LensFlareIntensity => effect.lens_flare_intensity = value,
        P::LensFlareCenterX => effect.lens_flare_center_x = value,
        P::LensFlareCenterY => effect.lens_flare_center_y = value,
        P::FilmGrainIntensity => effect.film_grain_intensity = value,
        P::FilmGrainSize => effect.film_grain_size = value,
        P::HeatHazeIntensity => effect.heat_haze_intensity = value,
        P::HeatHazeSpeed => effect.heat_haze_speed = value,
        P::HeatHazeScale => effect.heat_haze_scale = value,
        P::WaterRippleIntensity => effect.water_ripple_intensity = value,
        P::WaterRippleFrequency => effect.water_ripple_frequency = value,
        P::WaterRippleSpeed => effect.water_ripple_speed = value,
        P::WaterRippleCenterX => effect.water_ripple_center_x = value,
        P::WaterRippleCenterY => effect.water_ripple_center_y = value,
        P::FogIntensity => effect.fog_intensity = value,
        P::FogSpeed => effect.fog_speed = value,
        P::FogScale => effect.fog_scale = value,
        P::VhsIntensity => effect.vhs_intensity = value,
        P::VhsJitter => effect.vhs_jitter = value,
        P::VhsNoise => effect.vhs_noise = value,
        P::HalftoneIntensity => effect.halftone_intensity = value,
        P::HalftoneScale => effect.halftone_scale = value,
        P::HalftoneAngle => effect.halftone_angle = value,
        P::DitherIntensity => effect.dither_intensity = value,
        P::DitherLevels => effect.dither_levels = value,
        P::OutlineIntensity => effect.outline_intensity = value,
        P::OutlineThickness => effect.outline_thickness = value,
        P::EyelidOpenness => effect.eyelid_openness = value,
        P::EyelidWidth => effect.eyelid_width = value,
        P::EyelidCurvature => effect.eyelid_curvature = value,
        P::EyelidSoftness => effect.eyelid_softness = value,
        P::EyelidCenterX => effect.eyelid_center_x = value,
        P::EyelidCenterY => effect.eyelid_center_y = value,
        _ => {}
    }
}

fn apply_stage_camera_patches(
    state: &mut State,
    runtime: &keine_core::StageAnimationState,
    local_time: f32,
) {
    use keine_core::StageEventKind;
    let patches = runtime.animation.events.iter().filter_map(|event| {
        if let StageEventKind::CameraPatch { targets, effect } = &event.kind {
            Some((event.time, targets, effect))
        } else {
            None
        }
    });
    for (time, targets, patch) in patches {
        if time > local_time {
            continue;
        }
        if let Some(targets) = targets {
            state.camera_targets = *targets;
            state.camera_effect_targets = *targets;
        }
        state.camera_effect = patch.apply_to(state.camera_effect.clone());
    }
}

fn reset_stage_camera_patches(state: &mut State, runtime: &keine_core::StageAnimationState) {
    use keine_core::StageEventKind;
    for event in &runtime.animation.events {
        let StageEventKind::CameraPatch { targets, effect } = &event.kind else {
            continue;
        };
        if targets.is_some() {
            state.camera_targets = runtime.initial_camera_targets;
            state.camera_effect_targets = runtime.initial_camera_effect_targets;
        }
        effect.restore_affected_from(&mut state.camera_effect, &runtime.initial_camera_effect);
    }
}

fn trigger_stage_events(state: &mut State, runtime: &keine_core::StageAnimationState) {
    let duration = runtime.animation.duration.max(f32::EPSILON);
    let from = runtime.previous_elapsed;
    let to = runtime.elapsed;
    let first_cycle = (from.max(0.0) / duration).floor() as u32;
    let last_cycle = (to / duration).ceil().max(1.0) as u32;
    for cycle in first_cycle..last_cycle {
        let base = cycle as f32 * duration;
        for event in &runtime.animation.events {
            let at = base + event.time.clamp(0.0, duration);
            match &event.kind {
                keine_core::StageEventKind::Particle {
                    id,
                    effect,
                    duration,
                    fade_out,
                } => {
                    let runtime_id = format!("{}:particle:{id}", runtime.animation.id);
                    if crossed(from, to, at) {
                        state.particle_effects.insert(
                            runtime_id.clone(),
                            keine_core::ActiveParticleEffect::new(effect.clone()),
                        );
                    }
                    if crossed(from, to, at + duration.max(0.0)) {
                        if *fade_out <= f32::EPSILON {
                            state.particle_effects.remove(&runtime_id);
                        } else if let Some(effect) = state.particle_effects.get_mut(&runtime_id) {
                            effect.begin_fade_out(*fade_out);
                        }
                    }
                }
                keine_core::StageEventKind::CameraShake(shake) if crossed(from, to, at) => {
                    state.camera_shake = Some(keine_core::CameraShakeState {
                        spec: *shake,
                        elapsed: 0.0,
                        offset_x: 0.0,
                        offset_y: 0.0,
                        blocking: false,
                    });
                }
                keine_core::StageEventKind::Scene(cue) if crossed(from, to, at) => {
                    apply_stage_scene_cue(state, cue);
                }
                keine_core::StageEventKind::Audio(cue) => {
                    let runtime_id = format!("{}:audio:{}", runtime.animation.id, cue.id);
                    if crossed(from, to, at) {
                        start_stage_audio(state, cue, &runtime_id);
                    }
                    if cue.duration > 0.0 && crossed(from, to, at + cue.duration) {
                        stop_stage_audio(state, cue, &runtime_id);
                    }
                }
                _ => {}
            }
        }
    }
}

fn crossed(from: f32, to: f32, event: f32) -> bool {
    event > from && event <= to
}

fn start_stage_audio(state: &mut State, cue: &keine_core::StageAudioCue, runtime_id: &str) {
    use keine_core::StageAudioKind;

    match cue.kind {
        StageAudioKind::Bgm => {
            state.bgm.file = (!cue.file.is_empty()).then(|| cue.file.clone());
            state.bgm.volume = cue.volume.clamp(0.0, 1.0);
            state.bgm.fade_seconds = cue.fade_in.max(0.0);
            state.bgm.revision = state.bgm.revision.wrapping_add(1);
        }
        StageAudioKind::Effect if cue.looped => {
            state.looping_effects.insert(
                runtime_id.to_owned(),
                keine_core::EffectState {
                    file: cue.file.clone(),
                    volume: cue.volume.clamp(0.0, 1.0),
                },
            );
            state.effect_queue.push(keine_core::EffectEvent::StartLoop {
                id: runtime_id.to_owned(),
                fade_in: cue.fade_in.max(0.0),
            });
        }
        StageAudioKind::Effect => {
            state
                .effect_queue
                .push(keine_core::EffectEvent::Play(keine_core::EffectCue {
                    id: Some(runtime_id.to_owned()),
                    file: cue.file.clone(),
                    volume: cue.volume.clamp(0.0, 1.0),
                    fade_in: cue.fade_in.max(0.0),
                }));
        }
        StageAudioKind::Vocal => {
            state.vocal_event = Some(keine_core::VocalCue {
                file: (!cue.file.is_empty()).then(|| cue.file.clone()),
                volume: cue.volume.clamp(0.0, 1.0),
                fade_in: cue.fade_in.max(0.0),
                fade_out: 0.0,
            });
        }
    }
}

fn stop_stage_audio(state: &mut State, cue: &keine_core::StageAudioCue, runtime_id: &str) {
    use keine_core::StageAudioKind;

    match cue.kind {
        StageAudioKind::Bgm => {
            if state.bgm.file.as_deref() == Some(cue.file.as_str()) {
                state.bgm.file = None;
                state.bgm.fade_seconds = cue.fade_out.max(0.0);
                state.bgm.revision = state.bgm.revision.wrapping_add(1);
            }
        }
        StageAudioKind::Effect if cue.looped => {
            state.looping_effects.remove(runtime_id);
            state.effect_queue.push(keine_core::EffectEvent::StopLoop {
                id: runtime_id.to_owned(),
                fade_out: cue.fade_out.max(0.0),
            });
        }
        StageAudioKind::Effect => {
            state
                .effect_queue
                .push(keine_core::EffectEvent::StopOneShot {
                    id: runtime_id.to_owned(),
                    fade_out: cue.fade_out.max(0.0),
                });
        }
        StageAudioKind::Vocal => {
            state.vocal_event = Some(keine_core::VocalCue {
                file: None,
                volume: 0.0,
                fade_in: 0.0,
                fade_out: cue.fade_out.max(0.0),
            });
        }
    }
}

fn apply_stage_scene_cue(state: &mut State, cue: &keine_core::StageSceneCue) {
    use keine_core::state::Sprite;
    use keine_core::{BlendMode, Position, SpriteLayout, SpriteTransform, Transition};

    state.bg = None;
    state.bg_transition = None;
    state
        .sprites
        .retain(|id, _| !id.starts_with("scene-layer:"));
    for (index, layer) in cue
        .layers
        .iter()
        .filter(|layer| !layer.image.is_empty())
        .enumerate()
    {
        let id = format!("scene-layer:{}", layer.id);
        let transform = SpriteTransform {
            offset_x: layer.offset[0],
            offset_y: -layer.offset[1],
            ..SpriteTransform::default()
        };
        state.sprites.insert(
            id,
            Sprite {
                image: layer.image.clone(),
                position: Position::left(0.0),
                layout: SpriteLayout::Scene(cue.layout),
                transition_progress: if cue.transition == Transition::Instant {
                    1.0
                } else {
                    0.0
                },
                transition: cue.transition,
                entering: true,
                transition_offset_x: 0.0,
                transition_blocking: false,
                transform,
                transform_animation: None,
                position_animation: None,
                keyframe_animation: None,
                filter: Default::default(),
                films: Default::default(),
                animation: None,
                z_index: index as i32,
                blend: BlendMode::Alpha,
                camera_distance: Some(layer.distance.max(f32::EPSILON)),
            },
        );
    }
    if cue.reset_camera {
        state.camera_transform = SpriteTransform::default();
        state.camera_effect = Default::default();
        state.camera_shake = None;
    }
}

fn advance_keyframes(
    transform: &mut keine_core::SpriteTransform,
    animation: &mut keine_core::state::KeyframeAnimation,
    delta_seconds: f32,
) -> bool {
    if animation.frames.is_empty() {
        return true;
    }
    let mut remaining = delta_seconds.max(0.0);
    // A large frame delta can cross several short segments. The bound also
    // prevents a malformed zero-duration looping timeline from spinning.
    let limit = animation.frames.len().saturating_mul(2).max(1);
    for _ in 0..limit {
        let frame = &mut animation.frames[animation.index];
        let available = (frame.duration - frame.elapsed).max(0.0);
        let consumed = remaining.min(available);
        frame.elapsed += consumed;
        remaining -= consumed;
        let progress = if frame.duration <= f32::EPSILON {
            1.0
        } else {
            frame.easing.sample(frame.elapsed / frame.duration)
        };
        *transform = frame.from.lerp(frame.to, progress);
        if frame.elapsed + f32::EPSILON < frame.duration {
            return false;
        }

        animation.index += 1;
        if animation.index == animation.frames.len() {
            if animation.repeat_remaining == 0 {
                return true;
            }
            animation.repeat_remaining -= 1;
            animation.index = 0;
            *transform = animation.initial;
            for frame in &mut animation.frames {
                frame.elapsed = 0.0;
            }
        }
        if remaining <= f32::EPSILON && animation.frames[animation.index].duration > f32::EPSILON {
            return false;
        }
    }
    false
}

fn sample_preset(
    base: keine_core::SpriteTransform,
    preset: &keine_core::AnimationPreset,
    progress: f32,
) -> keine_core::SpriteTransform {
    use keine_core::AnimationPreset;
    let progress = progress.clamp(0.0, 1.0);
    let mut result = base;
    let eased = 1.0 - (1.0 - progress).powi(3);
    match preset {
        AnimationPreset::Enter => result.alpha *= eased,
        AnimationPreset::Exit => result.alpha *= 1.0 - progress * progress,
        AnimationPreset::EnterFromBottom => {
            result.offset_y += 220.0 * (1.0 - eased);
            result.blur += 5.0 * (1.0 - eased);
            result.alpha *= eased;
        }
        AnimationPreset::EnterFromLeft => {
            result.offset_x -= 280.0 * (1.0 - eased);
            result.blur += 5.0 * (1.0 - eased);
            result.alpha *= eased;
        }
        AnimationPreset::EnterFromRight => {
            result.offset_x += 280.0 * (1.0 - eased);
            result.blur += 5.0 * (1.0 - eased);
            result.alpha *= eased;
        }
        AnimationPreset::Shake => {
            let offset = if progress < 0.25 {
                -100.0 * (progress / 0.25)
            } else if progress < 0.75 {
                -100.0 + 200.0 * ((progress - 0.25) / 0.5)
            } else {
                100.0 * (1.0 - (progress - 0.75) / 0.25)
            };
            result.offset_x += offset;
        }
        AnimationPreset::MoveFrontAndBack => {
            let scale = 1.0 + (progress * std::f32::consts::PI).sin() * 0.15;
            result.scale_x *= scale;
            result.scale_y *= scale;
        }
        AnimationPreset::Blur => {
            result.blur += (progress * std::f32::consts::PI).sin() * 4.0;
        }
        AnimationPreset::ShockwaveIn
        | AnimationPreset::ShockwaveOut
        | AnimationPreset::OldFilm
        | AnimationPreset::DotFilm
        | AnimationPreset::ReflectionFilm
        | AnimationPreset::GlitchFilm
        | AnimationPreset::RgbFilm
        | AnimationPreset::GodrayFilm
        | AnimationPreset::RemoveFilm
        | AnimationPreset::Custom(_) => {}
    }
    result
}

fn preset_final_transform(
    base: keine_core::SpriteTransform,
    _preset: &keine_core::AnimationPreset,
) -> keine_core::SpriteTransform {
    base
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "hot-reload")]
    use std::sync::{Arc, Mutex};

    #[cfg(feature = "hot-reload")]
    use keine_core::config::AssetSourceConfig;
    use keine_core::state::{Dialogue, KeyframeAnimation, TransformAnimation};
    use keine_core::{
        Action, AnimationPreset, BlendMode, DialoguePause, Easing, Position, PostProcessPatch,
        SpriteTransform, StageAnimation, StageAudioCue, StageAudioKind, StageEvent, StageEventKind,
        StageKeyframe, StageProperty, StageTarget, StageTrack, Transition, Value, VideoMode,
        VideoSpec,
    };

    use super::*;

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
            keine_loader::ParseReport::default()
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

        start_pending_reload(&mut pipeline, &content, &languages).unwrap();
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
    fn held_control_reasserts_skip_until_release() {
        let mut toggles = ToggleStates::default();
        let held = InputActions {
            skip_held: true,
            ..default()
        };

        update_toggle_shortcuts(&held, &mut toggles, &mut 0.0);
        assert!(toggles.skip);

        // Any runtime boundary that clears the effective toggle cannot cancel
        // a physical key that is still held on the next tick.
        toggles.skip = false;
        update_toggle_shortcuts(&held, &mut toggles, &mut 0.0);
        assert!(toggles.skip);

        let released = InputActions {
            skip_released: true,
            ..default()
        };
        update_toggle_shortcuts(&released, &mut toggles, &mut 0.0);
        assert!(!toggles.skip);
    }

    #[test]
    fn control_k_toggle_survives_the_modifier_release() {
        let mut toggles = ToggleStates::default();
        let chord = InputActions {
            toggle_skip: true,
            skip_released: true,
            ..default()
        };
        update_toggle_shortcuts(&chord, &mut toggles, &mut 0.0);
        assert!(toggles.skip);

        update_toggle_shortcuts(&InputActions::default(), &mut toggles, &mut 0.0);
        assert!(toggles.skip);
    }

    #[test]
    fn dialogue_retraction_stays_in_a_visible_speed_range() {
        assert_eq!(dialogue_retraction_speed(10.0), 6.0);
        assert_eq!(dialogue_retraction_speed(30.0), 9.0);
        assert_eq!(dialogue_retraction_speed(50.0), 12.0);
        assert_eq!(dialogue_retraction_speed(120.0), 12.0);
    }

    fn dialogue_state() -> State {
        let mut state = State::new();
        state.current_scene = "main".into();
        state.cursor = 1;
        state.dialogue = Some(Dialogue {
            speaker: String::new(),
            text: "abcdefghij".into(),
            markup: "abcdefghij".into(),
            visible_chars: 0,
            pauses: Vec::new(),
            vocal: None,
            volume: 1.0,
            auto_advance: false,
        });
        state
    }

    #[test]
    fn typewriter_preserves_fractional_progress() {
        let mut state = dialogue_state();
        let mut clock = TypewriterClock::default();

        for _ in 0..4 {
            update_typewriter(&mut state, 0.05, 10.0, &mut clock);
        }

        assert_eq!(state.dialogue.unwrap().visible_chars, 2);
    }

    #[test]
    fn pointer_hit_test_consumes_ui_clicks_before_interaction_updates() {
        assert!(point_inside_rect(
            Vec2::new(110.0, 85.0),
            Vec2::new(100.0, 100.0),
            Vec2::new(80.0, 40.0),
        ));
        assert!(!point_inside_rect(
            Vec2::new(145.0, 85.0),
            Vec2::new(100.0, 100.0),
            Vec2::new(80.0, 40.0),
        ));
    }

    #[test]
    fn editor_seek_keeps_resources_inherited_from_an_earlier_fragment() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([
            (
                "start".into(),
                vec![
                    Action::ShowBg {
                        image: "backgrounds/inherited.webp".into(),
                        transition: Transition::Instant,
                        transform: SpriteTransform::default(),
                    },
                    Action::ChangeScene("chapter-two".into()),
                ],
            ),
            (
                "chapter-two".into(),
                vec![Action::Say {
                    speaker: String::new(),
                    text: "continued".into(),
                    options: Default::default(),
                }],
            ),
        ]));
        state.current_scene = "start".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "chapter-two", 0, 0));
        assert_eq!(state.current_scene, "chapter-two");
        assert_eq!(state.bg.as_deref(), Some("backgrounds/inherited.webp"));
    }

    #[test]
    fn editor_seek_keeps_the_selected_dialogue_visible_after_replay() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Say {
                speaker: "小夜".into(),
                text: "被选中的对白".into(),
                options: Default::default(),
            }],
        )]));
        state.current_scene = "main".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "main", 0, 1));
        let retained = state
            .dialogue
            .as_ref()
            .expect("selected dialogue must remain at its native yield");
        assert_eq!(retained.speaker, "小夜");
        assert_eq!(retained.text, "被选中的对白");
        assert!(!state.textbox_hidden);
    }

    #[test]
    fn editor_seek_retires_a_prior_blocking_video_without_spinning() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::PlayVideo {
                    video: VideoSpec {
                        id: "opening".into(),
                        file: "opening.mp4".into(),
                        looped: false,
                        muted: false,
                        alpha: 1.0,
                        skippable: true,
                        wait_for_finished: true,
                        mode: VideoMode::Fullscreen,
                    },
                },
                Action::Say {
                    speaker: "小夜".into(),
                    text: "视频之后".into(),
                    options: Default::default(),
                },
            ],
        )]));
        state.current_scene = "main".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "main", 1, 2));
        assert!(state.videos.is_empty());
        assert_eq!(
            state
                .dialogue
                .as_ref()
                .map(|dialogue| dialogue.text.as_str()),
            Some("视频之后")
        );
    }

    #[test]
    fn editor_seek_preserves_the_selected_block_animation() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::ShowBg {
                image: "backgrounds/selected.webp".into(),
                transition: Transition::Fade(1.0),
                transform: SpriteTransform::default(),
            }],
        )]));
        state.current_scene = "main".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "main", 0, 1));
        let transition = state
            .bg_transition
            .as_ref()
            .expect("selected transition must not be fast-forwarded");
        assert_eq!(transition.progress, 0.0);
        assert!(transition.blocking);
    }

    #[test]
    fn editor_seek_preserves_a_selected_dialogue_retraction() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::Say {
                    speaker: "小夜".into(),
                    text: "我当然来了".into(),
                    options: Default::default(),
                },
                Action::RetractDialogue {
                    source: "我当然来了".into(),
                    keep: "我当然".into(),
                },
            ],
        )]));
        state.current_scene = "main".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "main", 1, 2));
        assert_eq!(
            state.dialogue.as_ref().map(|line| line.text.as_str()),
            Some("我当然来了")
        );
        assert!(state.dialogue_retraction.is_some());
    }

    #[test]
    fn editor_seek_completes_prior_retractions_before_the_selected_block() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::Say {
                    speaker: "小夜".into(),
                    text: "我当然来了".into(),
                    options: Default::default(),
                },
                Action::RetractDialogue {
                    source: "我当然来了".into(),
                    keep: "我当然".into(),
                },
                Action::Say {
                    speaker: "小夜".into(),
                    text: "下一句".into(),
                    options: Default::default(),
                },
            ],
        )]));
        state.current_scene = "main".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "main", 2, 3));
        assert_eq!(
            state.dialogue.as_ref().map(|line| line.text.as_str()),
            Some("下一句")
        );
        assert!(state.dialogue_retraction.is_none());
    }

    #[test]
    fn editor_seek_rebuilds_variables_from_adapter_defaults() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "start".into(),
            vec![Action::Say {
                speaker: String::new(),
                text: "{route}/{ending}".into(),
                options: Default::default(),
            }],
        )]));
        state
            .vars
            .insert("route".into(), Value::Str("stale".into()));
        state.global_vars.insert("ending".into(), Value::Int(99));

        let manifest = LocalAssetManifest(std::collections::HashMap::from([(
            "start".into(),
            LocalSceneAssets {
                action_spans: vec![keine_loader::SourceSpan { line: 1, column: 1 }],
                ..default()
            },
        )]));
        let initial = keine_loader::ProjectInitialState {
            variables: std::collections::HashMap::from([(
                "route".into(),
                Value::Str("fresh".into()),
            )]),
            shared_variables: std::collections::HashMap::from([("ending".into(), Value::Int(2))]),
        };

        assert!(sync_editor_position(
            &mut state, &manifest, "start", 1, initial,
        ));
        assert_eq!(state.dialogue.as_ref().unwrap().text, "fresh/2");
        assert_eq!(state.vars["route"], Value::Str("fresh".into()));
        assert_eq!(state.global_vars["ending"], Value::Int(2));
    }

    #[test]
    fn editor_seek_does_not_confirm_the_selected_dialogue_cleanup() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::Say {
                    speaker: String::new(),
                    text: "应当显示的旁白".into(),
                    options: Default::default(),
                },
                Action::SetTextbox {
                    visible: false,
                    auto: true,
                },
            ],
        )]));
        let manifest = LocalAssetManifest(std::collections::HashMap::from([(
            "main".into(),
            LocalSceneAssets {
                action_spans: vec![
                    keine_loader::SourceSpan { line: 1, column: 1 },
                    keine_loader::SourceSpan { line: 1, column: 1 },
                ],
                ..default()
            },
        )]));

        assert!(sync_editor_position(
            &mut state,
            &manifest,
            "main",
            1,
            keine_loader::ProjectInitialState::default(),
        ));
        assert_eq!(
            state
                .dialogue
                .as_ref()
                .map(|dialogue| dialogue.text.as_str()),
            Some("应当显示的旁白")
        );
        assert!(!state.textbox_hidden);
    }

    #[test]
    fn typewriter_reveals_first_character_immediately() {
        let mut state = dialogue_state();
        let mut clock = TypewriterClock::default();

        update_typewriter(&mut state, 0.0, 10.0, &mut clock);

        assert_eq!(state.dialogue.unwrap().visible_chars, 1);
    }

    #[test]
    fn natural_end_keeps_the_final_frame_for_the_title_transition() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::ShowBg {
                    image: "final.webp".into(),
                    transition: Transition::Instant,
                    transform: SpriteTransform::default(),
                },
                Action::Say {
                    speaker: "续".into(),
                    text: "最后一句".into(),
                    options: Default::default(),
                },
                Action::End,
            ],
        )]));
        state.current_scene = "main".into();
        state.ended = false;
        assert_eq!(step::step(&mut state), keine_core::StepResult::AwaitClick);
        state.dialogue.as_mut().unwrap().visible_chars = 4;

        let progress = advance_once(
            &mut state,
            &mut crate::storage::save::ContinuationCheckpoint::default(),
        );

        assert!(progress.changed);
        assert!(progress.return_to_title);
        assert!(!state.ended);
        assert_eq!(state.bg.as_deref(), Some("final.webp"));
        assert_eq!(
            state.dialogue.as_ref().map(|line| line.text.as_str()),
            Some("最后一句")
        );
    }

    #[test]
    fn execution_limit_fails_closed_to_the_title_transition() {
        let mut actions = vec![Action::Comment; 1_025];
        actions.push(Action::Say {
            speaker: "unreachable".into(),
            text: "must not require another click".into(),
            options: Default::default(),
        });
        let mut state = State::new();
        state.install_program(Program::from_scenes([("main".into(), actions)]));
        state.current_scene = "main".into();
        state.ended = false;

        let progress = step_once(
            &mut state,
            &mut crate::storage::save::ContinuationCheckpoint::default(),
        );

        assert!(progress.changed);
        assert!(progress.return_to_title);
        assert_eq!(state.cursor, 1_024);
        assert!(state.dialogue.is_none());
        assert!(!state.ended, "the title fade owns destructive cleanup");
    }

    #[test]
    fn typewriter_waits_at_zero_width_inline_markers() {
        let mut state = dialogue_state();
        state.dialogue.as_mut().unwrap().pauses = vec![DialoguePause {
            at: 2,
            duration: Some(1.0),
        }];
        let mut clock = TypewriterClock::default();

        update_typewriter(&mut state, 0.0, 10.0, &mut clock);
        update_typewriter(&mut state, 0.2, 10.0, &mut clock);
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 2);

        update_typewriter(&mut state, 0.8, 10.0, &mut clock);
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 2);

        update_typewriter(&mut state, 0.2, 10.0, &mut clock);
        assert!(state.dialogue.unwrap().visible_chars > 2);
    }

    #[test]
    fn skip_read_stops_at_unread_dialogue() {
        let mut state = dialogue_state();
        let mut toggles = ToggleStates {
            skip: true,
            ..default()
        };

        skip_once(
            &mut state,
            &mut toggles,
            &mut crate::storage::save::ContinuationCheckpoint::default(),
        );

        assert!(!toggles.skip);
        assert_eq!(state.dialogue.unwrap().visible_chars, 0);
    }

    #[test]
    fn skip_all_reveals_unread_dialogue() {
        let mut state = dialogue_state();
        let mut toggles = ToggleStates {
            skip: true,
            skip_mode: SkipMode::All,
            ..default()
        };

        skip_once(
            &mut state,
            &mut toggles,
            &mut crate::storage::save::ContinuationCheckpoint::default(),
        );

        assert!(toggles.skip);
        assert_eq!(state.dialogue.unwrap().visible_chars, 10);
    }

    #[test]
    fn keyframe_timeline_consumes_large_frame_deltas_without_rate_dependence() {
        let initial = SpriteTransform::default();
        let mut first = initial;
        first.offset_x = 100.0;
        let mut second = first;
        second.offset_x = 160.0;
        let mut timeline = KeyframeAnimation {
            initial,
            frames: vec![
                TransformAnimation {
                    from: initial,
                    to: first,
                    elapsed: 0.0,
                    duration: 1.0,
                    easing: Easing::Linear,
                    blocking: false,
                },
                TransformAnimation {
                    from: first,
                    to: second,
                    elapsed: 0.0,
                    duration: 0.5,
                    easing: Easing::Linear,
                    blocking: false,
                },
            ],
            index: 0,
            repeat_remaining: 0,
            blocking: true,
        };
        let mut transform = initial;

        assert!(!advance_keyframes(&mut transform, &mut timeline, 0.75));
        assert_eq!(transform.offset_x, 75.0);
        assert!(advance_keyframes(&mut transform, &mut timeline, 0.75));
        assert_eq!(transform.offset_x, 160.0);
    }

    #[test]
    fn stage_timeline_samples_shared_clock_and_resets_event_patches_each_loop() {
        let mut state = State::new();
        let animation = StageAnimation {
            id: "fixture".into(),
            duration: 1.0,
            tracks: vec![StageTrack {
                target: StageTarget::Camera,
                property: StageProperty::Zoom,
                keyframes: vec![
                    StageKeyframe {
                        time: 0.0,
                        value: 1.0,
                        easing: Easing::Linear,
                    },
                    StageKeyframe {
                        time: 1.0,
                        value: 2.0,
                        easing: Easing::Linear,
                    },
                ],
                muted: false,
            }],
            events: vec![StageEvent {
                time: 0.5,
                kind: StageEventKind::CameraPatch {
                    targets: None,
                    effect: Box::new(PostProcessPatch {
                        color_brightness: Some(0.4),
                        ..default()
                    }),
                },
            }],
            repeat: 1,
            infinite: false,
            playback_rate: 1.0,
            blocking: true,
        };
        state.stage_animation = Some(keine_core::StageAnimationState::new(animation, &state));

        advance_stage_animation(&mut state, 0.25);
        assert!((state.camera_transform.scale_x - 1.25).abs() < 0.001);
        assert_eq!(state.camera_effect.color_brightness, 0.0);

        advance_stage_animation(&mut state, 0.5);
        assert!((state.camera_transform.scale_x - 1.75).abs() < 0.001);
        assert_eq!(state.camera_effect.color_brightness, 0.4);

        // The second play starts from its authored time zero and must not keep
        // a camera patch that occurred in the previous play.
        advance_stage_animation(&mut state, 0.3);
        assert!((state.camera_transform.scale_x - 1.05).abs() < 0.001);
        assert_eq!(state.camera_effect.color_brightness, 0.0);

        advance_stage_animation(&mut state, 0.95);
        assert_eq!(state.camera_transform.scale_x, 2.0);
        assert!(state.stage_animation.is_none());
    }

    #[test]
    fn stage_timeline_audio_uses_the_shared_clock_and_authored_duration() {
        let mut state = State::new();
        let animation = StageAnimation {
            id: "audio-fixture".into(),
            duration: 0.6,
            tracks: Vec::new(),
            events: vec![StageEvent {
                time: 0.1,
                kind: StageEventKind::Audio(StageAudioCue {
                    id: "rain".into(),
                    kind: StageAudioKind::Effect,
                    file: "audio/rain.opus".into(),
                    volume: 0.35,
                    looped: true,
                    duration: 0.3,
                    fade_in: 0.1,
                    fade_out: 0.2,
                }),
            }],
            repeat: 0,
            infinite: false,
            playback_rate: 1.0,
            blocking: true,
        };
        state.stage_animation = Some(keine_core::StageAnimationState::new(animation, &state));

        advance_stage_animation(&mut state, 0.2);
        assert_eq!(
            state.looping_effects["audio-fixture:audio:rain"].file,
            "audio/rain.opus"
        );
        assert!(state.effect_queue.iter().any(|event| matches!(
            event,
            keine_core::EffectEvent::StartLoop { id, fade_in }
                if id == "audio-fixture:audio:rain" && (*fade_in - 0.1).abs() < f32::EPSILON
        )));
        advance_stage_animation(&mut state, 0.25);
        assert!(
            !state
                .looping_effects
                .contains_key("audio-fixture:audio:rain")
        );
        assert!(state.effect_queue.iter().any(|event| matches!(
            event,
            keine_core::EffectEvent::StopLoop { id, fade_out }
                if id == "audio-fixture:audio:rain" && (*fade_out - 0.2).abs() < f32::EPSILON
        )));
    }

    #[test]
    fn custom_exit_animation_keeps_sprite_until_its_last_frame() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::SetTransition {
                    target: "hero".into(),
                    enter: None,
                    exit: Some(AnimationPreset::Exit),
                    duration: 1.0,
                },
                Action::ShowSprite {
                    id: "hero".into(),
                    image: "hero.webp".into(),
                    position: Position::center(0.0),
                    layout: keine_core::SpriteLayout::Natural,
                    transition: Transition::Instant,
                    transform: SpriteTransform::default(),
                    z_index: 0,
                    blend: BlendMode::Alpha,
                },
                Action::HideSprite {
                    id: "hero".into(),
                    transition: Transition::Instant,
                },
            ],
        )]));
        state.current_scene = "main".into();
        let initial_revision = state.stage_revision;

        assert_eq!(
            step::step(&mut state),
            keine_core::StepResult::AwaitPresentation
        );
        assert!(state.sprites.contains_key("hero"));
        let started_revision = state.stage_revision;
        assert_ne!(started_revision, initial_revision);

        update_transitions(&mut state, 0.5, false);
        let halfway = &state.sprites["hero"];
        assert!(halfway.animation.is_some());
        assert!(halfway.transform.alpha > 0.0);
        let halfway_revision = state.stage_revision;
        assert_ne!(halfway_revision, started_revision);

        update_transitions(&mut state, 0.5, false);
        assert!(!state.sprites.contains_key("hero"));
        assert!(!state.presentation_blocked());
        assert_ne!(state.stage_revision, halfway_revision);
    }

    #[test]
    #[cfg(feature = "hot-reload")]
    fn program_reload_rebuilds_interaction_state_from_the_new_scene() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Say {
                speaker: "old".into(),
                text: "old line".into(),
                options: Default::default(),
            }],
        )]));
        state.current_scene = "main".into();
        state.vars.insert("route".into(), Value::Str("kept".into()));
        state.global_vars.insert("chapter".into(), Value::Int(2));
        state.unlocked_cg.insert("old.webp".into(), "Old".into());
        assert_eq!(step::step(&mut state), keine_core::StepResult::AwaitClick);
        state.record_dialogue(0);
        state.mark_current_dialogue_read();

        restart_after_program_reload(
            &mut state,
            Program::from_scenes([(
                "main".into(),
                vec![
                    Action::ShowBg {
                        image: "new.webp".into(),
                        transition: Transition::Instant,
                        transform: SpriteTransform::default(),
                    },
                    Action::Say {
                        speaker: "new".into(),
                        text: "new line".into(),
                        options: Default::default(),
                    },
                ],
            )]),
        );

        assert_eq!(state.current_scene, "main");
        assert_eq!(state.bg.as_deref(), Some("new.webp"));
        assert_eq!(
            state.dialogue.as_ref().map(|line| line.text.as_str()),
            Some("new line")
        );
        assert_eq!(state.vars.get("route"), Some(&Value::Str("kept".into())));
        assert!(state.global_vars.contains_key("chapter"));
        assert!(state.unlocked_cg.contains_key("old.webp"));
        assert!(state.scene_stack.is_empty());
        assert_eq!(state.backlog.len(), 1);
        assert_eq!(state.backlog[0].text, "new line");
        assert_eq!(
            state.backlog[0].snapshot.program_fingerprint,
            state.program_fingerprint
        );
        assert!(state.read_dialogues.is_empty());
        assert_eq!(state.effect_queue, [keine_core::EffectEvent::Stop]);
    }

    #[test]
    fn sprite_sequence_samples_authored_fps_and_loops() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::ShowSprite {
                    id: "hero".into(),
                    image: "frame-1.webp".into(),
                    position: Position::center(0.0),
                    layout: Default::default(),
                    transition: Transition::Instant,
                    transform: SpriteTransform::default(),
                    z_index: 0,
                    blend: BlendMode::Alpha,
                },
                Action::ConfigureSpriteSequence {
                    id: "hero".into(),
                    frames: vec!["frame-1.webp".into(), "frame-2.webp".into()],
                    fps: 8.0,
                    looped: true,
                },
                Action::Say {
                    speaker: String::new(),
                    text: "wait".into(),
                    options: Default::default(),
                },
            ],
        )]));
        state.current_scene = "main".into();
        assert_eq!(step::step(&mut state), keine_core::StepResult::AwaitClick);

        update_transitions(&mut state, 0.13, false);
        assert_eq!(state.sprites["hero"].image, "frame-2.webp");
        update_transitions(&mut state, 0.13, false);
        assert_eq!(state.sprites["hero"].image, "frame-1.webp");
    }
}
