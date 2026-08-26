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
use bevy::window::PrimaryWindow;
use bevy::window::WindowCloseRequested;
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
    pub skip_held: bool,
    pub skip_released: bool,
    pub skip_video: bool,
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

pub(crate) fn collect_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    mut click_history: ResMut<PointerClickHistory>,
    mut actions: ResMut<InputActions>,
) {
    let gamepad_advance = gamepads
        .iter()
        .any(|pad| pad.just_pressed(GamepadButton::South));
    let gamepad_skip = gamepads
        .iter()
        .any(|pad| pad.just_pressed(GamepadButton::RightTrigger2));
    let pointer_pressed = mouse.just_pressed(MouseButton::Left) || touches.any_just_pressed();
    let control_pressed = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    actions.shortcut = keyboard_shortcut(&keys);
    update_control_hold(&keys, &mut actions);
    actions.pointer_advance = pointer_pressed;
    actions.advance = (!control_pressed && keys.any_just_pressed([KeyCode::Space, KeyCode::Enter]))
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
        || gamepads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::West));
    actions.toggle_skip = actions.shortcut == Some(ButtonAction::Skip) || gamepad_skip;
}

fn update_control_hold(keys: &ButtonInput<KeyCode>, actions: &mut InputActions) {
    let was_held = actions.skip_held;
    let control_pressed = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let chord_key_pressed = keys
        .get_pressed()
        .any(|key| !matches!(key, KeyCode::ControlLeft | KeyCode::ControlRight));

    if !control_pressed {
        actions.control_chord_used = false;
    } else if chord_key_pressed {
        // Once Ctrl participates in any chord, suppress hold-to-skip until the
        // modifier is released. Releasing the letter before Ctrl must not
        // unexpectedly start fast-forwarding.
        actions.control_chord_used = true;
    }

    actions.skip_held = control_pressed && !actions.control_chord_used;
    actions.skip_released = was_held && !actions.skip_held;
}

fn keyboard_shortcut(keys: &ButtonInput<KeyCode>) -> Option<ButtonAction> {
    if !keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        return None;
    }
    [
        (KeyCode::KeyA, ButtonAction::Auto),
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
    ]
    .into_iter()
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
    windows: Query<'w, 's, &'static Window>,
    benchmark: Option<Res<'w, crate::ui::performance::RuntimeCaptureConfig>>,
    startup_capture: Option<Res<'w, crate::ui::performance::StartupCapture>>,
    editor_sync: Option<Res<'w, EditorSyncSession>>,
}

pub(crate) fn update_lifecycle(
    context: LifecycleContext,
    mut activity: ResMut<RuntimeActivity>,
    mut winit: ResMut<WinitSettings>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut dialogue_length: Local<DialogueLengthCache>,
) {
    let focused = context.windows.single().is_ok_and(|window| window.focused);
    let studio_sync = context.editor_sync.is_some();
    let pause_for_background = should_pause_for_background(focused, studio_sync);
    let auto_hide = context
        .auto_hide
        .lifecycle(context.real_time.elapsed_secs(), &context.toggles);
    let reactive_wait = auto_hide.1.min(
        context
            .input_caret
            .next_toggle_in(context.real_time.elapsed_secs()),
    );
    let benchmark_active = context.benchmark.is_some() || context.startup_capture.is_some();
    let next = if benchmark_active || (studio_sync && !focused) {
        // A benchmark must keep measuring the render loop even when the
        // current visual-novel frame itself is static. Studio synchronization
        // likewise remains fully live while the user works in another window.
        RuntimeActivity::Active
    } else if pause_for_background {
        RuntimeActivity::Background
    } else if context.loading.blocked {
        RuntimeActivity::Loading
    } else if core_is_animating(&context.state, &mut dialogue_length)
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

    let benchmark_mode = benchmark_active
        .then(|| UpdateMode::reactive_low_power(std::time::Duration::from_secs_f64(1.0 / 60.0)));
    let focused_mode = match (benchmark_mode, next) {
        (Some(mode), _) => mode,
        (None, RuntimeActivity::Active | RuntimeActivity::Loading) => UpdateMode::Continuous,
        (None, RuntimeActivity::Idle | RuntimeActivity::Background) => {
            UpdateMode::reactive_low_power(reactive_wait)
        }
    };
    if winit.focused_mode != focused_mode {
        winit.focused_mode = focused_mode;
    }
    let unfocused_mode = if let Some(mode) = benchmark_mode {
        mode
    } else if studio_sync {
        // Only Studio synchronization needs an unfocused live render loop.
        // Ordinary `dev` hot reload is event-driven and follows release focus
        // semantics, avoiding a permanent background CPU cost.
        UpdateMode::Continuous
    } else {
        UpdateMode::reactive_low_power(std::time::Duration::MAX)
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
    fn application_shortcuts_require_control() {
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyA);
        assert_eq!(keyboard_shortcut(&keys), None);

        keys.press(KeyCode::ControlLeft);
        assert_eq!(keyboard_shortcut(&keys), Some(ButtonAction::Auto));
    }

    #[test]
    fn common_shortcuts_have_one_central_mapping() {
        let expected = [
            (KeyCode::KeyA, ButtonAction::Auto),
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
    }

    #[test]
    fn standalone_control_is_a_level_trigger_and_chords_stay_suppressed() {
        let mut keys = ButtonInput::default();
        let mut actions = InputActions::default();

        keys.press(KeyCode::ControlLeft);
        update_control_hold(&keys, &mut actions);
        assert!(actions.skip_held);
        assert!(!actions.skip_released);

        update_control_hold(&keys, &mut actions);
        assert!(
            actions.skip_held,
            "holding Ctrl must remain active every frame"
        );

        keys.press(KeyCode::KeyA);
        update_control_hold(&keys, &mut actions);
        assert!(!actions.skip_held);
        assert!(actions.skip_released);

        keys.release(KeyCode::KeyA);
        update_control_hold(&keys, &mut actions);
        assert!(
            !actions.skip_held,
            "a completed chord stays suppressed until Ctrl is released"
        );

        keys.release(KeyCode::ControlLeft);
        update_control_hold(&keys, &mut actions);
        keys.press(KeyCode::ControlLeft);
        update_control_hold(&keys, &mut actions);
        assert!(actions.skip_held);
    }

    #[test]
    fn collected_pointer_edge_is_not_replayed_on_the_next_frame() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin))
            .init_resource::<InputActions>()
            .init_resource::<PointerClickHistory>()
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
