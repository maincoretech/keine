//! Window, input, lifecycle and diagnostics owned by the native platform.

use std::fmt;
use std::time::Instant;

use anyhow::Error;
use bevy::app::AppExit;
use bevy::audio::{AudioSink, AudioSinkPlayback};
use bevy::camera::Viewport;
use bevy::ecs::system::SystemParam;
use bevy::log::{BoxedFmtLayer, Level, LogPlugin, tracing_subscriber};
use bevy::prelude::*;
use bevy::render::batching::gpu_preprocessing::{GpuPreprocessingMode, GpuPreprocessingSupport};
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::{Render, RenderApp};
use bevy::window::WindowCloseRequested;
use bevy::window::{Monitor, OnMonitor, PrimaryWindow};
use bevy::winit::{UpdateMode, WinitSettings};
use keine_core::{DESIGN_HEIGHT, DESIGN_WIDTH};

use crate::render::blur::{DialogCamera, SceneBlurCamera, UiBlurCamera};
use crate::runtime::resources::{
    AssetLoadingGate, DialogueLengthCache, EditorSyncSession, GameState,
};
use crate::scene::audio::AudioAnimationActivity;
use crate::ui::activity::UiAnimationActivity;
use crate::ui::control_bar::{AutoHideTiming, ButtonAction, QuickPreviewSurface, ToggleStates};
use crate::ui::textbox::{ContentRoot, QuickPreviewLayer};
use crate::ui::user_input::UserInputCaretBlink;

/// Raises the cost of runtime extraction for packaged builds.
///
/// Only the `keine bundle` engine build compiles with the `hardened`
/// feature, so `cargo dev` and CI runner builds remain fully debuggable.
/// None of this is DRM — a determined attacker can patch the binary or dump
/// memory another way — it only closes the trivial "attach a debugger and
/// read the restored key" path.
///
/// Compile-time guard: the call site is also `#[cfg(feature = "hardened")]`,
/// so non-packaged builds compile this function away entirely.
#[cfg(feature = "hardened")]
pub fn apply_hardening() {
    #[cfg(target_os = "macos")]
    deny_attach();
    #[cfg(unix)]
    disable_core_dumps();
    #[cfg(windows)]
    exit_under_debugger();
}

/// Refuse debugger attachment at the kernel level: after `PT_DENY_ATTACH`,
/// lldb `process attach` and DTrace task-port access both fail for this task.
#[cfg(all(feature = "hardened", target_os = "macos"))]
fn deny_attach() {
    unsafe {
        libc::ptrace(libc::PT_DENY_ATTACH, 0, std::ptr::null_mut(), 0);
    }
}

/// Crash dumps would otherwise capture the decrypted key after an unwind.
#[cfg(all(feature = "hardened", unix))]
fn disable_core_dumps() {
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    unsafe {
        libc::setrlimit(libc::RLIMIT_CORE, &limit);
    }
}

/// A packaged game under a user-mode debugger exits immediately. Trivially
/// bypassable, but it stops casual attach-and-inspect sessions.
#[cfg(all(feature = "hardened", windows))]
fn exit_under_debugger() {
    use windows_sys::Win32::System::Diagnostics::Debug::IsDebuggerPresent;
    unsafe {
        if IsDebuggerPresent() != 0 {
            std::process::exit(1);
        }
    }
}

/// Platform-neutral actions consumed by the VN runtime.
#[derive(Resource, Default, Debug)]
pub(crate) struct InputActions {
    pub advance: bool,
    pub pointer_advance: bool,
    pub shortcut: Option<ButtonAction>,
    pub toggle_auto: bool,
    pub toggle_skip: bool,
    pub auto_held: bool,
    pub auto_released: bool,
    pub skip_video: bool,
    pub toggle_fullscreen: bool,
    pub(crate) control_chord_used: bool,
}

#[derive(Resource, Default)]
pub(crate) struct PointerClickHistory {
    last_click: Option<f64>,
}

#[derive(Resource, Default)]
pub(crate) struct GracefulExit {
    requested: bool,
}

/// Convert every native close request into one orderly application exit.
///
/// The window entity deliberately remains alive for this final schedule. This
/// gives save/profile systems a chance to observe `AppExit` and flush their
/// state before winit tears down the native window.
pub(crate) fn request_graceful_exit(
    mut requests: MessageReader<WindowCloseRequested>,
    mut exits: MessageWriter<AppExit>,
    mut shutdown: ResMut<GracefulExit>,
) {
    let requested = requests.read().next().is_some();
    if requested && !shutdown.requested {
        shutdown.requested = true;
        log::info!("shutdown requested · flushing state");
        exits.write(AppExit::Success);
    }
}

#[derive(SystemParam)]
pub(crate) struct InputContext<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    touches: Res<'w, Touches>,
    gamepads: Query<'w, 's, &'static Gamepad>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    scope: Res<'w, crate::ui::input_scope::UiInputScope>,
    time: Res<'w, Time>,
    click_history: ResMut<'w, PointerClickHistory>,
    actions: ResMut<'w, InputActions>,
}

pub(crate) fn collect_input(context: InputContext) {
    let InputContext {
        keys,
        mouse,
        touches,
        gamepads,
        windows,
        scope,
        time,
        mut click_history,
        mut actions,
    } = context;
    let was_held = actions.auto_held;
    if windows.single().is_ok_and(|window| !window.focused) {
        *actions = InputActions {
            auto_released: was_held,
            // Regaining focus while Ctrl is still down must not resume autoplay.
            control_chord_used: true,
            ..default()
        };
        click_history.last_click = None;
        return;
    }
    let gameplay_input = matches!(
        *scope,
        crate::ui::input_scope::UiInputScope::Stage | crate::ui::input_scope::UiInputScope::Title
    );
    let gamepad_advance = gameplay_input
        && gamepads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::South));
    let gamepad_skip = gamepads
        .iter()
        .any(|pad| pad.just_pressed(GamepadButton::RightTrigger2));
    let pointer_pressed =
        gameplay_input && (mouse.just_pressed(MouseButton::Left) || touches.any_just_pressed());
    let control_pressed = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    actions.shortcut = if matches!(
        *scope,
        crate::ui::input_scope::UiInputScope::Stage
            | crate::ui::input_scope::UiInputScope::Title
            | crate::ui::input_scope::UiInputScope::Menu
            | crate::ui::input_scope::UiInputScope::Backlog
    ) {
        keyboard_shortcut(&keys)
            .or_else(|| {
                (*scope == crate::ui::input_scope::UiInputScope::Stage
                    && !control_pressed
                    && !shortcut_modifier_pressed(&keys)
                    && keys.just_pressed(KeyCode::Escape))
                .then_some(ButtonAction::System)
            })
            .or_else(|| {
                (*scope == crate::ui::input_scope::UiInputScope::Stage
                    && mouse.just_pressed(MouseButton::Right))
                .then_some(ButtonAction::Hide)
            })
    } else {
        None
    };
    if *scope != crate::ui::input_scope::UiInputScope::Stage && control_pressed {
        // Do not begin hold-to-autoplay by closing a modal with Ctrl still held.
        actions.control_chord_used = true;
    }
    update_control_hold(&keys, &mut actions);
    actions.auto_held &= *scope == crate::ui::input_scope::UiInputScope::Stage;
    actions.auto_released = was_held && !actions.auto_held;
    actions.pointer_advance = pointer_pressed;
    actions.advance = (gameplay_input
        && !control_pressed
        && !shortcut_modifier_pressed(&keys)
        && keys.any_just_pressed([KeyCode::Space, KeyCode::Enter]))
        || pointer_pressed
        || gamepad_advance;
    actions.skip_video = false;
    if pointer_pressed {
        let now = time.elapsed_secs_f64();
        actions.skip_video = click_history
            .last_click
            .is_some_and(|last| now - last <= 0.35);
        click_history.last_click = Some(now);
    }
    actions.toggle_auto = actions.shortcut == Some(ButtonAction::Auto)
        || (gameplay_input
            && gamepads
                .iter()
                .any(|pad| pad.just_pressed(GamepadButton::West)));
    actions.toggle_skip =
        actions.shortcut == Some(ButtonAction::Skip) || (gameplay_input && gamepad_skip);
    actions.toggle_fullscreen =
        !control_pressed && !shortcut_modifier_pressed(&keys) && keys.just_pressed(KeyCode::F11);
}

fn shortcut_modifier_pressed(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
    ])
}

fn update_control_hold(keys: &ButtonInput<KeyCode>, actions: &mut InputActions) {
    let was_held = actions.auto_held;
    let control_pressed = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let chord_key_pressed = keys
        .get_pressed()
        .any(|key| !matches!(key, KeyCode::ControlLeft | KeyCode::ControlRight));

    if !control_pressed {
        actions.control_chord_used = false;
    } else if chord_key_pressed {
        // Once Ctrl participates in any chord, suppress hold-to-autoplay until the
        // modifier is released. Releasing the letter before Ctrl must not
        // unexpectedly start autoplaying.
        actions.control_chord_used = true;
    }

    actions.auto_held = control_pressed && !actions.control_chord_used;
    actions.auto_released = was_held && !actions.auto_held;
}

fn keyboard_shortcut(keys: &ButtonInput<KeyCode>) -> Option<ButtonAction> {
    if shortcut_modifier_pressed(keys) {
        return None;
    }
    let common = [
        (KeyCode::KeyK, ButtonAction::Skip),
        (KeyCode::KeyB, ButtonAction::Backlog),
        (KeyCode::KeyR, ButtonAction::Replay),
        (KeyCode::KeyH, ButtonAction::Hide),
    ];
    let specific = if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        &[
            (KeyCode::KeyQ, ButtonAction::QuickSave),
            (KeyCode::KeyL, ButtonAction::QuickLoad),
            (KeyCode::KeyS, ButtonAction::Save),
            (KeyCode::KeyO, ButtonAction::Load),
            (KeyCode::Comma, ButtonAction::System),
            (KeyCode::KeyT, ButtonAction::Title),
        ][..]
    } else {
        &[
            (KeyCode::F5, ButtonAction::QuickSave),
            (KeyCode::F9, ButtonAction::QuickLoad),
        ][..]
    };
    common
        .into_iter()
        .chain(specific.iter().copied())
        .find_map(|(key, action)| keys.just_pressed(key).then_some(action))
}

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RuntimeActivity {
    #[default]
    Active,
    Idle,
    Loading,
    Background,
}

type DialogTargetRoots<'w, 's> = Query<
    'w,
    's,
    (
        &'static UiTargetCamera,
        &'static Node,
        &'static InheritedVisibility,
    ),
    (With<Node>, Without<QuickPreviewLayer>),
>;

/// Stop submitting the third UI camera while its layer is empty.
///
/// The normal textbox belongs to the UI camera. The dialog camera is reserved
/// for title/menu/modal/preview overlays, so keeping it alive throughout every
/// line of dialogue wastes a complete camera extraction and render pass.
pub(crate) fn sync_dialog_camera_activity(
    benchmark: Option<Res<crate::ui::performance::RuntimeCaptureConfig>>,
    mut camera: Query<(Entity, &mut Camera), With<DialogCamera>>,
    roots: DialogTargetRoots,
    previews: Query<&Node, With<QuickPreviewSurface>>,
) {
    if benchmark
        .as_ref()
        .is_some_and(|capture| capture.cameras.pins_dialog_activity())
    {
        return;
    }
    let Ok((camera_entity, mut camera)) = camera.single_mut() else {
        return;
    };
    // Use hierarchy visibility rather than ViewVisibility: the latter depends
    // on an active view and would make a sleeping camera unable to wake itself.
    let visible_root = roots.iter().any(|(target, node, visibility)| {
        target.0 == camera_entity && node.display != Display::None && visibility.get()
    });
    let visible_preview = previews.iter().any(|node| node.display != Display::None);
    let needed = visible_root || visible_preview;
    if camera.is_active != needed {
        camera.is_active = needed;
    }
}

#[derive(SystemParam)]
pub(crate) struct LifecycleContext<'w, 's> {
    state: Res<'w, GameState>,
    loading: Res<'w, AssetLoadingGate>,
    ui: Res<'w, UiAnimationActivity>,
    audio: Res<'w, AudioAnimationActivity>,
    toggles: Res<'w, ToggleStates>,
    auto_hide: Res<'w, AutoHideTiming>,
    input_caret: Res<'w, UserInputCaretBlink>,
    real_time: Res<'w, Time<Real>>,
    windows: Query<'w, 's, (&'static Window, Option<&'static OnMonitor>)>,
    monitors: Query<'w, 's, &'static Monitor>,
    benchmark: Option<Res<'w, crate::ui::performance::RuntimeCaptureConfig>>,
    startup_capture: Option<Res<'w, crate::ui::performance::StartupCapture>>,
    editor_sync: Option<Res<'w, EditorSyncSession>>,
    authoring_preview: Option<Res<'w, super::preview::AuthoringPreviewSession>>,
}

// Bevy 0.19's reactive runner uses Instant::checked_add(wait). Duration::MAX
// overflows, leaving the previous animation deadline/control flow in place.
// A finite idle deadline lets it sleep; input and authoring messages still wake
// immediately, and nearer UI deadlines (auto-hide/caret) retain their timing.
const IDLE_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

pub(crate) fn update_lifecycle(
    context: LifecycleContext,
    mut activity: ResMut<RuntimeActivity>,
    mut winit: ResMut<WinitSettings>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut dialogue_length: Local<DialogueLengthCache>,
) {
    let focused = context
        .windows
        .single()
        .is_ok_and(|(window, _)| window.focused);
    let studio_sync = context.editor_sync.is_some();
    let companion_preview = context.authoring_preview.is_some();
    let preview_settling = context
        .authoring_preview
        .as_ref()
        .and_then(|preview| preview.settle_until)
        .is_some_and(|until| Instant::now() < until);
    // A companion Preview keeps playing while the user edits in the other
    // window; winit still sleeps on unchanged frames in both focus states.
    let pause_for_background =
        should_pause_for_background(focused, studio_sync || companion_preview);
    let auto_hide = context
        .auto_hide
        .lifecycle(context.real_time.elapsed_secs(), &context.toggles);
    let reactive_wait = auto_hide.1.min(IDLE_WAIT).min(
        context
            .input_caret
            .next_toggle_in(context.real_time.elapsed_secs()),
    );
    let benchmark_active = context.benchmark.is_some() || context.startup_capture.is_some();
    let next = if benchmark_active || (studio_sync && !focused && !companion_preview) {
        // A benchmark must keep measuring the render loop even when the
        // current visual-novel frame itself is static. Studio synchronization
        // likewise remains fully live while the user works in another window.
        RuntimeActivity::Active
    } else if pause_for_background {
        RuntimeActivity::Background
    } else if context.loading.blocked {
        RuntimeActivity::Loading
    } else if preview_settling
        || core_is_animating(&context.state, &mut dialogue_length)
        || context.ui.0
        || context.audio.0
        || context.toggles.auto
        || context.toggles.skip
        || auto_hide.0
    {
        RuntimeActivity::Active
    } else {
        RuntimeActivity::Idle
    };

    let refresh_rate = context
        .windows
        .single()
        .ok()
        .and_then(|(_, monitor)| monitor)
        .and_then(|monitor| context.monitors.get(monitor.0).ok())
        .and_then(|monitor| monitor.refresh_rate_millihertz);
    let active_mode = active_update_mode(refresh_rate);
    let focused_mode = match next {
        RuntimeActivity::Active | RuntimeActivity::Loading => active_mode,
        RuntimeActivity::Idle | RuntimeActivity::Background => {
            UpdateMode::reactive_low_power(reactive_wait)
        }
    };
    if winit.focused_mode != focused_mode {
        winit.focused_mode = focused_mode;
    }
    let unfocused_mode = if benchmark_active || studio_sync && !companion_preview {
        active_mode
    } else if companion_preview {
        focused_mode
    } else {
        UpdateMode::reactive_low_power(IDLE_WAIT)
    };
    if winit.unfocused_mode != unfocused_mode {
        winit.unfocused_mode = unfocused_mode;
    }
    if *activity != next {
        *activity = next;
    }
    let should_pause_time = matches!(next, RuntimeActivity::Idle | RuntimeActivity::Background);
    if virtual_time.is_paused() != should_pause_time {
        if should_pause_time {
            virtual_time.pause();
        } else {
            virtual_time.unpause();
        }
    }
}

/// AppKit can issue synthetic redraws faster than the display presents them.
/// Schedule active macOS playback on the current monitor's cadence instead of
/// driving extra updates from those events. Input is consumed on the next frame;
/// idle windows still wake directly on input. Other platforms retain vsync pacing.
fn active_update_mode(refresh_rate_millihertz: Option<u32>) -> UpdateMode {
    #[cfg(target_os = "macos")]
    if let Some(rate) = refresh_rate_millihertz.filter(|rate| *rate > 0) {
        return UpdateMode::Reactive {
            wait: std::time::Duration::from_secs_f64(1_000.0 / f64::from(rate)),
            react_to_device_events: false,
            react_to_user_events: false,
            react_to_window_events: false,
        };
    }
    #[cfg(not(target_os = "macos"))]
    let _ = refresh_rate_millihertz;
    UpdateMode::Continuous
}

const fn should_pause_for_background(focused: bool, studio_sync: bool) -> bool {
    !focused && !studio_sync
}

#[derive(Component)]
pub(crate) struct BackgroundPausedAudio;

/// Pause every Bevy/rodio sink when a non-Studio window loses focus.
///
/// The marker distinguishes lifecycle-paused audio from tracks the player or
/// UI had already paused, so focus recovery never starts something it does not
/// own.
pub(crate) fn sync_background_audio(
    activity: Res<RuntimeActivity>,
    sinks: Query<(Entity, &AudioSink, Option<&BackgroundPausedAudio>)>,
    mut commands: Commands,
) {
    let background = *activity == RuntimeActivity::Background;
    if !should_scan_background_audio(*activity, activity.is_changed()) {
        return;
    }
    for (entity, sink, paused_by_lifecycle) in &sinks {
        match (background, paused_by_lifecycle.is_some(), sink.is_paused()) {
            (true, false, false) => {
                sink.pause();
                commands.entity(entity).insert(BackgroundPausedAudio);
            }
            (false, true, _) => {
                sink.play();
                commands.entity(entity).remove::<BackgroundPausedAudio>();
            }
            _ => {}
        }
    }
}

const fn should_scan_background_audio(activity: RuntimeActivity, changed: bool) -> bool {
    changed || matches!(activity, RuntimeActivity::Background)
}

fn core_is_animating(state: &GameState, dialogue_length: &mut DialogueLengthCache) -> bool {
    state
        .dialogue
        .as_ref()
        .is_some_and(|dialogue| dialogue.visible_chars < dialogue_length.count(&dialogue.text))
        || state
            .dialogue_retraction
            .as_ref()
            .is_some_and(|retraction| !retraction.awaiting_advance)
        || state.wait_remaining > f32::EPSILON
        || state.intro.is_some()
        || (state.curtain.current - state.curtain.target).abs() > f32::EPSILON
        || state
            .stage_masks
            .values()
            .any(|mask| (mask.current - mask.target).abs() > f32::EPSILON)
        || state.floating_text.is_some()
        || !state.videos.is_empty()
        || !state.particle_effects.is_empty()
        || state.bg_films.is_time_varying()
        || state.bg_transition.is_some()
        || state.bg_transform_animation.is_some()
        || state.bg_keyframe_animation.is_some()
        || state.bg_animation.is_some()
        || state.camera_effect_animation.is_some()
        || state.camera_shake.is_some()
        || state.stage_animation.is_some()
        || state.camera_effect.is_time_varying()
        || state.sprite_sequences.values().any(|sequence| {
            sequence.frames.len() > 1
                && (sequence.looped || sequence.frame + 1 < sequence.frames.len())
        })
        || state.sprites.values().any(|sprite| {
            sprite.films.is_time_varying()
                || sprite.animation.is_some()
                || sprite.transform_animation.is_some()
                || sprite.position_animation.is_some()
                || sprite.keyframe_animation.is_some()
                || (sprite.entering && sprite.transition_progress < 1.0)
                || (!sprite.entering && sprite.transition_progress > 0.0)
        })
        || (state.mini_avatar.is_some() && state.mini_avatar_progress < 1.0)
        || (state.mini_avatar.is_none() && state.mini_avatar_progress > 0.0)
}

/// Every camera that draws game content must share the same physical viewport.
///
/// Scaling scene entities into the design rectangle is not enough: camera
/// transforms and oversized sprites can still draw into the window letterbox.
/// A real camera viewport is the final, GPU-side scissor boundary for the
/// scene, UI and overlay layers.
type DesignCameraFilter = Or<(
    With<SceneBlurCamera>,
    With<UiBlurCamera>,
    With<DialogCamera>,
)>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignViewport {
    pub scale: f32,
    pub offset: Vec2,
    pub window_size: Vec2,
}

impl DesignViewport {
    pub fn from_window(window: &Window) -> Self {
        let window_size = Vec2::new(window.width(), window.height());
        let scale = (window_size.x / DESIGN_WIDTH)
            .min(window_size.y / DESIGN_HEIGHT)
            .max(f32::EPSILON);
        let content_size = Vec2::new(DESIGN_WIDTH, DESIGN_HEIGHT) * scale;

        Self {
            scale,
            offset: (window_size - content_size) * 0.5,
            window_size,
        }
    }

    pub fn world_from_design(self, point: Vec2) -> Vec2 {
        self.offset + point * self.scale - self.window_size * 0.5
    }

    pub fn content_center(self) -> Vec2 {
        self.world_from_design(Vec2::new(DESIGN_WIDTH, DESIGN_HEIGHT) * 0.5)
    }

    pub fn camera_viewport(self, window: &Window) -> Viewport {
        let scale_factor = window.scale_factor();
        let position = (self.offset * scale_factor).round().as_uvec2();
        let size = (Vec2::new(DESIGN_WIDTH, DESIGN_HEIGHT) * self.scale * scale_factor)
            .round()
            .as_uvec2()
            .max(UVec2::ONE);
        Viewport {
            physical_position: position,
            physical_size: size,
            ..default()
        }
    }
}

/// Keeps the fixed design canvas centered inside the window letterbox.
#[expect(
    clippy::type_complexity,
    reason = "ParamSet keeps added-camera detection disjoint from viewport mutation"
)]
pub(crate) fn resize_viewport(
    mut content_root: Query<&mut Node, (With<ContentRoot>, Without<QuickPreviewLayer>)>,
    mut quick_preview_layer: Query<&mut Node, (With<QuickPreviewLayer>, Without<ContentRoot>)>,
    window_query: Query<&Window>,
    mut cameras: ParamSet<(
        Query<Entity, (DesignCameraFilter, Added<Camera>)>,
        Query<&mut Camera, DesignCameraFilter>,
    )>,
    mut ui_scale: ResMut<UiScale>,
    mut previous: Local<Option<DesignViewport>>,
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let viewport = DesignViewport::from_window(window);
    let camera_added = !cameras.p0().is_empty();
    if !camera_added && previous.as_ref() == Some(&viewport) {
        return;
    }
    *previous = Some(viewport);

    ui_scale.0 = viewport.scale;
    for mut camera in &mut cameras.p1() {
        camera.viewport = Some(viewport.camera_viewport(window));
    }
    if let Ok(mut node) = content_root.single_mut() {
        node.left = Val::ZERO;
        node.top = Val::ZERO;
    }
    if let Ok(mut node) = quick_preview_layer.single_mut() {
        node.left = Val::ZERO;
        node.top = Val::ZERO;
    }
}

const DEFAULT_FILTER: &str = concat!(
    "warn,",
    "keine=info,",
    "keine_core=info,",
    "keine_loader=info,",
    "wgpu=error,",
    "naga=warn"
);
const MACOS_BENCHMARK_FILTER: &str = "bevy_winit::state=error";

pub(super) fn log_plugin(benchmark: bool) -> LogPlugin {
    LogPlugin {
        filter: runtime_log_filter(benchmark),
        level: Level::INFO,
        fmt_layer: compact_layer,
        ..Default::default()
    }
}

fn runtime_log_filter(benchmark: bool) -> String {
    if benchmark && cfg!(target_os = "macos") {
        // macOS can deliver the final native `Destroyed` event after Bevy has
        // removed its window mapping, producing a harmless warning on every
        // automated exit. Restrict the workaround to benchmark launches;
        // normal runs retain every bevy_winit warning.
        // Upstream: https://github.com/bevyengine/bevy/issues/23313
        format!("{DEFAULT_FILTER},{MACOS_BENCHMARK_FILTER}")
    } else {
        DEFAULT_FILTER.into()
    }
}

pub(super) fn install_runtime_diagnostics(app: &mut App) {
    app.add_systems(PostStartup, log_window);
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.add_systems(Render, log_renderer.run_if(run_once));
    }
}

fn compact_layer(_: &mut App) -> Option<BoxedFmtLayer> {
    let layer = tracing_subscriber::fmt::layer()
        .with_timer(ShortUptime::now())
        .compact()
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_writer(std::io::stderr);
    Some(Box::new(layer))
}

struct ShortUptime(Instant);

impl ShortUptime {
    fn now() -> Self {
        Self(Instant::now())
    }
}

impl tracing_subscriber::fmt::time::FormatTime for ShortUptime {
    fn format_time(&self, writer: &mut tracing_subscriber::fmt::format::Writer<'_>) -> fmt::Result {
        write!(writer, "{:>8.3}s", self.0.elapsed().as_secs_f64())
    }
}

pub(super) fn startup_error(stage: &str, error: &Error) {
    eprintln!("ERROR  keine::startup: {stage}");
    for (index, cause) in error.chain().enumerate() {
        eprintln!("       {:>2}. {cause}", index + 1);
    }
}

fn log_window(window: Single<&Window, With<PrimaryWindow>>) {
    let width = window.resolution.width().round() as u32;
    let height = window.resolution.height().round() as u32;
    let scale = window.resolution.scale_factor();
    let resize = if window.resizable {
        "resizable"
    } else {
        "fixed"
    };
    log::info!(
        target: "keine::platform",
        "WINDOW   │ {} · {width}×{height} @{scale:.1}× · {resize}",
        window.title,
    );
}

fn log_renderer(adapter: Res<RenderAdapterInfo>, preprocessing: Res<GpuPreprocessingSupport>) {
    let transient_memory = if adapter.transient_saves_memory {
        " · transient memory ✓"
    } else {
        ""
    };
    log::info!(
        target: "keine::platform",
        "GPU      │ {} · {:?} · {:?} · subgroup {}–{}{transient_memory}",
        adapter.name,
        adapter.device_type,
        adapter.backend,
        adapter.subgroup_min_size,
        adapter.subgroup_max_size,
    );

    let mode = match preprocessing.max_supported_mode {
        GpuPreprocessingMode::None => "CPU fallback",
        GpuPreprocessingMode::PreprocessingOnly => "GPU preprocessing ✓",
        GpuPreprocessingMode::Culling => "GPU preprocessing + culling ✓",
    };
    log::info!(target: "keine::platform", "PIPELINE │ {mode}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{ButtonState, InputPlugin, InputSystems, mouse::MouseButtonInput};
    use bevy::window::WindowResolution;

    fn is_animating(state: &GameState) -> bool {
        core_is_animating(state, &mut DialogueLengthCache::default())
    }

    #[test]
    fn save_shortcuts_require_control_and_gameplay_letters_do_not() {
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyS);
        assert_eq!(keyboard_shortcut(&keys), None);
        keys.press(KeyCode::ControlLeft);
        assert_eq!(keyboard_shortcut(&keys), Some(ButtonAction::Save));
        keys.reset_all();
        keys.press(KeyCode::KeyA);
        assert_eq!(keyboard_shortcut(&keys), None);
        keys.press(KeyCode::ControlLeft);
        assert_eq!(keyboard_shortcut(&keys), None);
    }

    #[test]
    fn common_shortcuts_have_one_central_mapping() {
        let expected = [
            (KeyCode::KeyK, ButtonAction::Skip),
            (KeyCode::KeyB, ButtonAction::Backlog),
            (KeyCode::KeyR, ButtonAction::Replay),
            (KeyCode::KeyH, ButtonAction::Hide),
            (KeyCode::KeyQ, ButtonAction::QuickSave),
            (KeyCode::KeyL, ButtonAction::QuickLoad),
            (KeyCode::KeyS, ButtonAction::Save),
            (KeyCode::KeyO, ButtonAction::Load),
            (KeyCode::Comma, ButtonAction::System),
            (KeyCode::KeyT, ButtonAction::Title),
        ];
        for (key, action) in expected {
            let mut keys = ButtonInput::default();
            keys.press(KeyCode::ControlLeft);
            keys.press(key);
            assert_eq!(keyboard_shortcut(&keys), Some(action));
        }
        for (key, action) in [
            (KeyCode::KeyK, ButtonAction::Skip),
            (KeyCode::KeyB, ButtonAction::Backlog),
            (KeyCode::KeyR, ButtonAction::Replay),
            (KeyCode::KeyH, ButtonAction::Hide),
            (KeyCode::F5, ButtonAction::QuickSave),
            (KeyCode::F9, ButtonAction::QuickLoad),
        ] {
            let mut keys = ButtonInput::default();
            keys.press(key);
            assert_eq!(keyboard_shortcut(&keys), Some(action));
            keys.clear();
            assert_eq!(
                keyboard_shortcut(&keys),
                None,
                "holding a key must not retrigger"
            );
        }
    }

    #[test]
    fn operating_system_chords_do_not_trigger_gameplay_shortcuts() {
        for modifier in [KeyCode::AltLeft, KeyCode::SuperLeft, KeyCode::ShiftLeft] {
            let mut keys = ButtonInput::default();
            keys.press(modifier);
            keys.press(KeyCode::KeyH);
            keys.press(KeyCode::F5);
            assert_eq!(keyboard_shortcut(&keys), None);
        }
    }

    #[test]
    fn right_click_toggles_textbox_only_on_stage_without_advancing() {
        use crate::ui::input_scope::UiInputScope;
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Touches>()
            .init_resource::<Time>()
            .init_resource::<InputActions>()
            .init_resource::<PointerClickHistory>()
            .init_resource::<UiInputScope>()
            .init_resource::<ToggleStates>()
            .init_resource::<crate::storage::settings::RuntimeSettings>()
            .add_systems(
                Update,
                (collect_input, crate::ui::control_bar::handle_button_click).chain(),
            );
        for expected_hidden in [true, false] {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .reset_all();
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Right);
            app.update();
            assert_eq!(app.world().resource::<ToggleStates>().hide, expected_hidden);
            assert!(!app.world().resource::<InputActions>().advance);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .clear();
            app.update();
            assert_eq!(app.world().resource::<ToggleStates>().hide, expected_hidden);
        }
        for scope in [
            UiInputScope::Title,
            UiInputScope::Menu,
            UiInputScope::Backlog,
            UiInputScope::Dialog,
            UiInputScope::UserInput,
            UiInputScope::Loading,
        ] {
            *app.world_mut().resource_mut::<UiInputScope>() = scope;
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .reset_all();
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Right);
            app.update();
            assert!(!app.world().resource::<ToggleStates>().hide);
            assert!(app.world().resource::<InputActions>().shortcut.is_none());
        }
    }

    #[test]
    fn input_modal_and_focus_boundaries_cancel_control_without_restarting_it() {
        use crate::ui::input_scope::UiInputScope;
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Touches>()
            .init_resource::<Time>()
            .init_resource::<InputActions>()
            .init_resource::<PointerClickHistory>()
            .init_resource::<UiInputScope>()
            .add_systems(Update, collect_input);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
        app.update();
        assert!(app.world().resource::<InputActions>().auto_held);

        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        let actions = app.world().resource::<InputActions>();
        assert!(actions.auto_released);
        assert!(!actions.auto_held);
        assert!(actions.shortcut.is_none());
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert!(!app.world().resource::<InputActions>().auto_held);

        for scope in [
            UiInputScope::UserInput,
            UiInputScope::Dialog,
            UiInputScope::Loading,
        ] {
            *app.world_mut().resource_mut::<UiInputScope>() = scope;
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ControlRight);
            keys.press(KeyCode::KeyA);
            keys.press(KeyCode::Space);
            app.update();
            let actions = app.world().resource::<InputActions>();
            assert!(!actions.advance);
            assert!(!actions.toggle_auto);
            assert!(!actions.auto_held);
            assert!(actions.shortcut.is_none());
        }
        *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Stage;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyA);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Space);
        app.update();
        assert!(!app.world().resource::<InputActions>().auto_held);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlRight);
        app.update();
        assert!(app.world().resource::<InputActions>().auto_held);
    }

    #[test]
    fn standalone_control_is_a_level_trigger_and_chords_stay_suppressed() {
        let mut keys = ButtonInput::default();
        let mut actions = InputActions::default();

        keys.press(KeyCode::ControlLeft);
        update_control_hold(&keys, &mut actions);
        assert!(actions.auto_held);
        assert!(!actions.auto_released);

        update_control_hold(&keys, &mut actions);
        assert!(
            actions.auto_held,
            "holding Ctrl must remain active every frame"
        );

        keys.press(KeyCode::KeyA);
        update_control_hold(&keys, &mut actions);
        assert!(!actions.auto_held);
        assert!(actions.auto_released);

        keys.release(KeyCode::KeyA);
        update_control_hold(&keys, &mut actions);
        assert!(
            !actions.auto_held,
            "a completed chord stays suppressed until Ctrl is released"
        );

        keys.release(KeyCode::ControlLeft);
        update_control_hold(&keys, &mut actions);
        keys.press(KeyCode::ControlLeft);
        update_control_hold(&keys, &mut actions);
        assert!(actions.auto_held);
    }

    #[test]
    fn collected_pointer_edge_is_not_replayed_on_the_next_frame() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin))
            .init_resource::<InputActions>()
            .init_resource::<PointerClickHistory>()
            .init_resource::<crate::ui::input_scope::UiInputScope>()
            .add_systems(PreUpdate, collect_input.after(InputSystems));
        let window = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window,
        });

        app.update();
        assert!(app.world().resource::<InputActions>().pointer_advance);

        app.update();
        assert!(!app.world().resource::<InputActions>().pointer_advance);
        assert!(!app.world().resource::<InputActions>().advance);
    }

    #[test]
    fn close_requests_emit_one_exit_and_leave_window_alive_for_flushing() {
        let mut app = App::new();
        app.add_message::<WindowCloseRequested>()
            .add_message::<AppExit>()
            .init_resource::<GracefulExit>()
            .add_systems(Update, request_graceful_exit);
        let window = app.world_mut().spawn(Window::default()).id();

        app.world_mut()
            .write_message(WindowCloseRequested { window });
        app.world_mut()
            .write_message(WindowCloseRequested { window });
        app.update();

        assert!(app.world().get_entity(window).is_ok());
        assert!(app.world().resource::<GracefulExit>().requested);
        let exits = app
            .world_mut()
            .resource_mut::<Messages<AppExit>>()
            .drain()
            .collect::<Vec<_>>();
        assert_eq!(exits, [AppExit::Success]);

        app.world_mut()
            .write_message(WindowCloseRequested { window });
        app.update();
        assert!(
            app.world().resource::<Messages<AppExit>>().is_empty(),
            "duplicate native close events must not start another shutdown"
        );
    }

    #[test]
    fn runtime_capture_uses_playback_cadence_in_both_focus_states() {
        use crate::ui::performance::{BenchmarkCameras, install_runtime_capture};
        use bevy::ecs::system::RunSystemOnce;

        let mut app = App::new();
        app.insert_resource(GameState(keine_core::State::new()))
            .init_resource::<AssetLoadingGate>()
            .init_resource::<UiAnimationActivity>()
            .init_resource::<AudioAnimationActivity>()
            .init_resource::<ToggleStates>()
            .init_resource::<AutoHideTiming>()
            .init_resource::<UserInputCaretBlink>()
            .init_resource::<Time<Real>>()
            .init_resource::<Time<Virtual>>()
            .init_resource::<RuntimeActivity>()
            .insert_resource(WinitSettings::desktop_app());
        install_runtime_capture(&mut app, 12.0, None, BenchmarkCameras::Runtime);
        let monitor = app
            .world_mut()
            .spawn(Monitor {
                name: None,
                physical_height: 1898,
                physical_width: 3024,
                physical_position: IVec2::ZERO,
                refresh_rate_millihertz: Some(120_000),
                scale_factor: 1.0,
                video_modes: Vec::new(),
            })
            .id();
        let entity = app
            .world_mut()
            .spawn((Window::default(), OnMonitor(monitor)))
            .id();
        for focused in [true, false] {
            app.world_mut().get_mut::<Window>(entity).unwrap().focused = focused;
            app.world_mut().run_system_once(update_lifecycle).unwrap();
            let winit = app.world().resource::<WinitSettings>();
            assert_eq!(winit.focused_mode, active_update_mode(Some(120_000)));
            assert_eq!(winit.unfocused_mode, winit.focused_mode);
            assert!(!app.world().resource::<Time<Virtual>>().is_paused());
        }
    }

    #[test]
    fn settled_frames_sleep_with_a_valid_deadline_and_resume_for_work() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        let mut timing = AutoHideTiming::default();
        timing.hide_btn_alpha = 0.0;
        app.insert_resource(GameState(keine_core::State::new()))
            .insert_resource(AssetLoadingGate { blocked: false })
            .init_resource::<UiAnimationActivity>()
            .init_resource::<AudioAnimationActivity>()
            .init_resource::<ToggleStates>()
            .insert_resource(timing)
            .init_resource::<UserInputCaretBlink>()
            .init_resource::<Time<Real>>()
            .init_resource::<Time<Virtual>>()
            .init_resource::<RuntimeActivity>()
            .insert_resource(WinitSettings::desktop_app());
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(10));
        let window = app.world_mut().spawn(Window::default()).id();

        for preview in [false, true] {
            if preview {
                app.init_resource::<super::super::preview::AuthoringPreviewSession>();
            }
            for focused in [true, false] {
                app.world_mut().get_mut::<Window>(window).unwrap().focused = focused;
                app.world_mut().run_system_once(update_lifecycle).unwrap();
                assert!(app.world().resource::<Time<Virtual>>().is_paused());
                let winit = app.world().resource::<WinitSettings>();
                for mode in [winit.focused_mode, winit.unfocused_mode] {
                    let UpdateMode::Reactive {
                        wait,
                        react_to_user_events,
                        react_to_window_events,
                        ..
                    } = mode
                    else {
                        panic!("settled frames must sleep")
                    };
                    assert!(wait > std::time::Duration::ZERO);
                    assert!(Instant::now().checked_add(wait).is_some());
                    assert!(react_to_user_events && react_to_window_events);
                }
            }
        }

        app.world_mut().resource_mut::<GameState>().wait_remaining = 0.5;
        app.world_mut().run_system_once(update_lifecycle).unwrap();
        assert_eq!(
            *app.world().resource::<RuntimeActivity>(),
            RuntimeActivity::Active
        );
        assert!(!app.world().resource::<Time<Virtual>>().is_paused());
        app.world_mut().resource_mut::<GameState>().wait_remaining = 0.0;
        app.world_mut().run_system_once(update_lifecycle).unwrap();
        assert_eq!(
            *app.world().resource::<RuntimeActivity>(),
            RuntimeActivity::Idle
        );
        {
            let mut timing = app.world_mut().resource_mut::<AutoHideTiming>();
            timing.last_move = 9.5;
            timing.hide_btn_alpha = 1.0;
        }
        app.world_mut().run_system_once(update_lifecycle).unwrap();
        assert_eq!(
            app.world().resource::<WinitSettings>().focused_mode,
            UpdateMode::reactive_low_power(std::time::Duration::from_millis(500)),
            "idle sleep must still honor the control-bar fade deadline"
        );
    }

    #[test]
    fn active_cadence_follows_refresh_rate_without_a_sixty_hz_cap() {
        assert_eq!(active_update_mode(None), UpdateMode::Continuous);
        assert_eq!(active_update_mode(Some(0)), UpdateMode::Continuous);
        #[cfg(target_os = "macos")]
        for rate in [60_000, 120_000, 144_000] {
            let UpdateMode::Reactive {
                wait,
                react_to_window_events,
                ..
            } = active_update_mode(Some(rate))
            else {
                panic!("monitor cadence")
            };
            assert!((wait.as_secs_f64() - 1_000.0 / f64::from(rate)).abs() < 1e-9);
            assert!(!react_to_window_events);
        }
    }

    #[test]
    fn only_studio_sync_keeps_running_without_focus() {
        assert!(!should_pause_for_background(false, true));
        assert!(should_pause_for_background(false, false));
        assert!(!should_pause_for_background(true, false));
    }

    #[test]
    fn only_macos_benchmarks_quiet_the_known_winit_teardown_warning() {
        assert_eq!(runtime_log_filter(false), DEFAULT_FILTER);
        let expected = if cfg!(target_os = "macos") {
            format!("{DEFAULT_FILTER},{MACOS_BENCHMARK_FILTER}")
        } else {
            DEFAULT_FILTER.into()
        };
        assert_eq!(runtime_log_filter(true), expected);
    }

    #[test]
    fn background_audio_scan_stays_live_only_while_backgrounded() {
        assert!(!should_scan_background_audio(
            RuntimeActivity::Active,
            false
        ));
        assert!(should_scan_background_audio(RuntimeActivity::Active, true));
        assert!(should_scan_background_audio(
            RuntimeActivity::Background,
            false
        ));
    }

    #[test]
    fn dialog_camera_sleeps_until_its_layer_has_visible_content() {
        let mut app = App::new();
        app.add_systems(Update, sync_dialog_camera_activity);
        let camera = app
            .world_mut()
            .spawn((Camera::default(), DialogCamera))
            .id();
        let root = app
            .world_mut()
            .spawn((
                Node::default(),
                UiTargetCamera(camera),
                InheritedVisibility::HIDDEN,
            ))
            .id();

        app.update();
        assert!(!app.world().get::<Camera>(camera).unwrap().is_active);

        app.world_mut()
            .entity_mut(root)
            .insert(InheritedVisibility::VISIBLE);
        app.update();
        assert!(app.world().get::<Camera>(camera).unwrap().is_active);

        app.world_mut()
            .entity_mut(root)
            .insert(InheritedVisibility::HIDDEN);
        app.world_mut().spawn((
            Node::default(),
            QuickPreviewSurface,
            InheritedVisibility::VISIBLE,
        ));
        app.update();
        assert!(app.world().get::<Camera>(camera).unwrap().is_active);
    }

    #[test]
    fn time_based_film_effects_keep_the_render_loop_active() {
        let mut state = GameState(keine_core::State::new());
        assert!(!is_animating(&state));
        assert!(state.bg_films.apply(&keine_core::AnimationPreset::OldFilm));
        assert!(is_animating(&state));
        state.bg_films.clear();
        assert!(state.bg_films.apply(&keine_core::AnimationPreset::DotFilm));
        assert!(!is_animating(&state));
        state.bg_films.clear();
        state.camera_effect.godray_intensity = 0.8;
        state.camera_effect.godray_speed = 0.2;
        assert!(is_animating(&state));
        state.camera_effect.godray_speed = 0.0;
        assert!(!is_animating(&state));
        state.camera_effect.film_grain_intensity = 0.5;
        assert!(is_animating(&state));
    }

    #[test]
    fn input_waits_sleep_but_timed_presentation_work_stays_active() {
        let mut state = GameState(keine_core::State::new());
        state.waiting_for_advance = true;
        assert!(
            !is_animating(&state),
            "waiting for a player input is script blocking, not an animation"
        );

        state.wait_remaining = 0.5;
        assert!(is_animating(&state));
        state.wait_remaining = 0.0;
        state.dialogue_retraction = Some(keine_core::state::DialogueRetraction {
            keep: "line".into(),
            target_visible_chars: 4,
            fractional_chars: 0.0,
            awaiting_advance: true,
        });
        assert!(!is_animating(&state));
        state.dialogue_retraction.as_mut().unwrap().awaiting_advance = false;
        assert!(is_animating(&state));
    }

    #[test]
    fn video_playback_keeps_the_render_loop_active() {
        let mut state = GameState(keine_core::State::new());
        state.videos.insert(
            "rain".into(),
            keine_core::VideoState {
                spec: keine_core::VideoSpec {
                    id: "rain".into(),
                    file: "video/rain.mp4".into(),
                    looped: true,
                    muted: true,
                    alpha: 1.0,
                    skippable: false,
                    wait_for_finished: false,
                    mode: keine_core::VideoMode::Mixed,
                },
                revision: 1,
                elapsed: 0.0,
                opacity: 1.0,
                stopping: false,
                fade_out: 0.0,
            },
        );

        assert!(is_animating(&state));
    }

    #[test]
    fn wide_window_centers_a_sixteen_by_nine_camera_viewport() {
        let window = Window {
            resolution: WindowResolution::new(2560, 1080),
            ..default()
        };
        let design = DesignViewport::from_window(&window);
        let camera = design.camera_viewport(&window);

        assert_eq!(design.offset, Vec2::new(320.0, 0.0));
        assert_eq!(camera.physical_position, UVec2::new(320, 0));
        assert_eq!(camera.physical_size, UVec2::new(1920, 1080));
    }

    #[test]
    fn tall_window_centers_a_sixteen_by_nine_camera_viewport() {
        let window = Window {
            resolution: WindowResolution::new(1280, 1024),
            ..default()
        };
        let design = DesignViewport::from_window(&window);
        let camera = design.camera_viewport(&window);

        assert_eq!(design.offset, Vec2::new(0.0, 152.0));
        assert_eq!(camera.physical_position, UVec2::new(0, 152));
        assert_eq!(camera.physical_size, UVec2::new(1280, 720));
    }

    #[test]
    fn every_game_camera_receives_the_design_viewport() {
        let mut app = App::new();
        app.insert_resource(UiScale::default())
            .add_systems(Update, resize_viewport)
            .world_mut()
            .spawn(Window {
                resolution: WindowResolution::new(2560, 1080),
                ..default()
            });
        app.world_mut().spawn((Camera::default(), SceneBlurCamera));
        app.world_mut().spawn((Camera::default(), UiBlurCamera));
        app.world_mut().spawn((Camera::default(), DialogCamera));

        app.update();

        let mut cameras = app.world_mut().query::<&Camera>();
        let viewports = cameras
            .iter(app.world())
            .map(|camera| {
                let viewport = camera.viewport.as_ref().expect("design viewport");
                (viewport.physical_position, viewport.physical_size)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            viewports,
            vec![(UVec2::new(320, 0), UVec2::new(1920, 1080)); 3]
        );

        app.update();
        let mut cameras = app.world_mut().query::<Ref<Camera>>();
        assert!(
            cameras.iter(app.world()).all(|camera| !camera.is_changed()),
            "a stable window must not dirty every camera each frame"
        );
        assert!(
            !app.world().resource_ref::<UiScale>().is_changed(),
            "a stable window must not invalidate UI layout each frame"
        );

        app.world_mut().spawn((Camera::default(), SceneBlurCamera));
        app.update();
        let mut cameras = app.world_mut().query::<&Camera>();
        assert_eq!(
            cameras
                .iter(app.world())
                .filter(|camera| camera.viewport.is_some())
                .count(),
            4,
            "a camera spawned after the window settles still needs the viewport"
        );
    }
}
