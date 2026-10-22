use crate::ui::foundation::{Button, Interaction};
use bevy::audio::AudioSinkPlayback;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use keine_core::State;
use keine_core::runtime::animation::{preset_final_transform, sample_preset};
use keine_core::step;

use crate::runtime::platform::InputActions;
use crate::runtime::resources::{
    AssetLoadingGate, DialogueLengthCache, EditorSyncSession, GameState,
};
#[cfg(feature = "hot-reload")]
use crate::runtime::resources::{
    ContentProjectResource, LocalAssetManifest, ScriptLanguages, ScriptWatcherResource,
};
use crate::storage::settings::RuntimeSettings;
use crate::ui::control_bar::{ButtonAction, SkipMode, ToggleStates};
use crate::ui::input_scope::UiInputScope;

#[path = "script/reload.rs"]
pub(super) mod reload;
#[cfg(feature = "hot-reload")]
use reload::{EditorCursorSync, HotReloadPipeline, reload_scripts_if_changed};

#[path = "preview/seek.rs"]
mod seek;
pub(crate) use seek::{
    restart_after_program_reload, seek_editor_state, sync_editor_cursor,
    sync_editor_source_position,
};

#[derive(Default)]
struct TypewriterClock {
    scene: String,
    cursor: usize,
    dialogue_length: DialogueLengthCache,
    fractional_chars: f64,
    last_visible_chars: usize,
    pauses: Vec<keine_core::DialoguePause>,
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
    vocal_playback: Option<Res<'w, crate::scene::audio::VocalPlayback>>,
    vocals: Query<'w, 's, Option<&'static AudioSink>, With<crate::scene::audio::VocalPlayer>>,
    editor_sync: Option<Res<'w, EditorSyncSession>>,
    authoring_preview: Option<Res<'w, super::preview::AuthoringPreviewSession>>,
    returning_to_title: Option<Res<'w, crate::ui::title::ReturnToTitleTransition>>,
    auto_timer: Local<'s, f64>,
    typewriter_clock: Local<'s, TypewriterClock>,
    checkpoint_generation: Local<'s, u64>,
    #[cfg(feature = "hot-reload")]
    editor_cursor_sync: Local<'s, EditorCursorSync>,
    #[cfg(feature = "hot-reload")]
    hot_reload: Local<'s, HotReloadPipeline>,
    commands: Commands<'w, 's>,
}

/// Advances input, text timing, script hot reload, and transition state.
pub fn tick(mut context: TickContext) {
    let delta_seconds = context.time.delta_secs_f64();
    if *context.checkpoint_generation != context.checkpoint.generation() {
        *context.checkpoint_generation = context.checkpoint.generation();
        *context.typewriter_clock = TypewriterClock::default();
        *context.auto_timer = 0.0;
    }
    // A modal or loading gate can consume the frame on which Ctrl is released.
    if context.actions.auto_released {
        context.toggles.auto = false;
        *context.auto_timer = 0.0;
    }
    if context.returning_to_title.is_some() {
        return;
    }
    // Legacy Studio sync is a view-only cursor follower. The native authoring
    // Preview also owns its source position, but must accept direct input.
    let studio_sync = context.editor_sync.is_some() && context.authoring_preview.is_none();
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
    if !studio_sync {
        update_toggle_shortcuts(
            &context.actions,
            &mut context.toggles,
            &mut context.auto_timer,
        );
    }
    let presentation_was_blocked = context.state.presentation_blocked();
    if !studio_sync && context.actions.skip_video {
        let before = context.state.videos.len();
        context
            .state
            .videos
            .retain(|_, video| !video.spec.skippable || video.spec.looped);
        state_changed |= before != context.state.videos.len();
    }
    let presentation_advance = !studio_sync
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
    if presentation_was_blocked
        || (!studio_sync && crate::runtime::script_driver::needs_resume(&context.state))
    {
        if !studio_sync && !context.state.presentation_blocked() {
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
        // Keep the typewriter running when read-only skip stops at an unread line.
        if context.toggles.skip || progress.changed || progress.return_to_title {
            if state_changed {
                context.state.set_changed();
            }
            return;
        }
    }

    state_changed |= update_typewriter(
        context.state.bypass_change_detection(),
        delta_seconds,
        typewriter_speed,
        &mut context.typewriter_clock,
    );
    if studio_sync {
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
    // One frame may contain both a click and an expired auto/notend timer.
    // Consume one yield only; never apply the same input to the next line.
    let progress = if presentation_advance {
        *context.auto_timer = 0.0;
        advance_once(
            context.state.bypass_change_detection(),
            &mut context.checkpoint,
        )
    } else {
        let target_chars = dialogue_target(&context.state, &mut context.typewriter_clock);
        let notend = update_notend(
            context.state.bypass_change_detection(),
            target_chars,
            &mut context.checkpoint,
        );
        if notend.changed || notend.return_to_title {
            *context.auto_timer = 0.0;
            notend
        } else if context
            .vocal_playback
            .as_ref()
            .is_some_and(|playback| playback.awaiting_sync(&context.state))
            || context
                .vocals
                .iter()
                .any(|sink| sink.is_none_or(|sink| !sink.empty()))
        {
            // Pending loads and paused voices are unfinished too. Begin the
            // normal reading delay only after playback has actually ended.
            *context.auto_timer = 0.0;
            TickProgress::default()
        } else {
            update_auto_mode(
                context.state.bypass_change_detection(),
                context.toggles.auto,
                delta_seconds,
                context.settings.auto_delay,
                &mut context.auto_timer,
                target_chars,
                &mut context.checkpoint,
            )
        }
    };
    state_changed |= progress.changed;
    if progress.return_to_title {
        request_return_to_title(&mut context.commands);
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
    if actions.auto_released {
        toggles.auto = false;
        *auto_timer = 0.0;
    }
    if actions.toggle_auto {
        toggles.auto = !toggles.auto;
        *auto_timer = 0.0;
    }
    if actions.toggle_skip {
        toggles.skip = !toggles.skip;
    }
    if actions.auto_held {
        if !toggles.auto {
            *auto_timer = 0.0;
            toggles.auto = true;
        }
        toggles.skip = false;
    }
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
    let changed = advance_typewriter(state, delta_seconds, chars_per_second, clock);
    clock.last_visible_chars = state.dialogue.as_ref().map_or(0, |line| line.visible_chars);
    changed
}

fn advance_typewriter(
    state: &mut State,
    delta_seconds: f64,
    chars_per_second: f64,
    clock: &mut TypewriterClock,
) -> bool {
    let dialogue_changed = clock.scene != state.current_scene
        || clock.cursor != state.cursor
        || state.dialogue.as_ref().is_some_and(|line| {
            !clock.dialogue_length.matches(&line.text)
                || clock.pauses != line.pauses
                || line.visible_chars < clock.last_visible_chars
        });
    if dialogue_changed {
        clock.scene.clone_from(&state.current_scene);
        clock.cursor = state.cursor;
        clock.fractional_chars = 0.0;
        clock.next_pause = state.dialogue.as_ref().map_or(0, |line| {
            clock.pauses.clone_from(&line.pauses);
            line.pauses
                .partition_point(|pause| pause.at < line.visible_chars)
        });
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
    // Opacity can round to the target before the clock reaches its deadline.
    // Keep ticking the clock so a visually settled fade cannot retain its lock.
    if state.curtain.elapsed < state.curtain.duration
        || state.curtain.blocking
        || (state.curtain.current - state.curtain.target).abs() > f32::EPSILON
    {
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
        if mask.elapsed >= mask.duration
            && !mask.blocking
            && (mask.current - mask.target).abs() <= f32::EPSILON
        {
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
    state.stage_masks.retain(|_, mask| {
        mask.elapsed < mask.duration
            || mask.blocking
            || mask.target > f32::EPSILON
            || mask.current > f32::EPSILON
    });
    if state.stage_masks.len() != mask_count {
        changed = true;
        stage_changed = true;
    }
    // An infinite caption has no timed work after its entrance. HideFloatingText
    // starts its authored exit by resetting elapsed and clearing infinite.
    if let Some(text) = &mut state.floating_text
        && (!text.infinite || text.elapsed < text.fade_in)
    {
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
    state
        .videos
        .retain(|_, video| !video.stopping || video.opacity > 0.0);
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
        if let Some(fields) = &animation.fields {
            for field in fields {
                field.interpolate_effect(
                    &mut state.camera_effect,
                    &animation.from,
                    &animation.to,
                    progress,
                );
            }
        }
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
        changed = true;
        shake.advance(delta_seconds);
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
        let speaking = match &sequence.playback {
            Some(keine_core::SequencePlayback::Talk { speaker }) => {
                state.dialogue.as_ref().is_some_and(|dialogue| {
                    &dialogue.speaker == speaker
                        && dialogue.visible_chars < dialogue.text.chars().count()
                })
            }
            _ => false,
        };
        let frame = sequence.advance(delta_seconds, speaking);
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
                keine_core::StageEventKind::CameraShake(shake)
                | keine_core::StageEventKind::CameraShakeRandomized { shake, .. }
                    if crossed(from, to, at) =>
                {
                    let randomness = match &event.kind {
                        keine_core::StageEventKind::CameraShakeRandomized {
                            randomness, ..
                        } => *randomness,
                        _ => Default::default(),
                    };
                    state.camera_shake = Some(keine_core::CameraShakeState::new(
                        *shake,
                        randomness,
                        state.program_fingerprint ^ at.to_bits() as u64,
                        false,
                    ));
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
                    fade_out: 0.0,
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

#[cfg(test)]
mod tests {
    use super::seek::finish_editor_presentation;
    use crate::runtime::resources::LocalAssetManifest;
    use keine_core::Program;
    use keine_core::state::{Dialogue, KeyframeAnimation, TransformAnimation};
    use keine_core::{
        Action, AnimationPreset, BlendMode, CameraShakeAxis, CameraShakeFalloff, CameraShakeSpec,
        DialoguePause, Easing, Position, PostProcessPatch, SpriteTransform, StageAnimation,
        StageAudioCue, StageAudioKind, StageEvent, StageEventKind, StageKeyframe, StageProperty,
        StageTarget, StageTrack, Transition, VideoMode, VideoSpec,
    };

    use super::*;

    #[test]
    fn selected_effect_fields_tween_through_runtime_and_instant_fields_stay_fixed() {
        let mut state = State::new();
        state.camera_shake = Some(keine_core::CameraShakeState::new(
            CameraShakeSpec {
                amplitude: 2.0,
                frequency: 1.0,
                duration: 2.0,
                axis: CameraShakeAxis::Both,
                falloff: CameraShakeFalloff::Linear,
            },
            Default::default(),
            42,
            false,
        ));
        let spec = keine_core::CameraTweenSpec {
            targets: keine_core::CameraTargets::SCENE,
            transform: None,
            effect: None,
            v2: Some(Box::new(keine_core::PostProcessV2 {
                speed_lines_intensity: 0.8,
                speed_lines_density: 0.95,
                speed_lines_region_width: 2.,
                ..Default::default()
            })),
            shake: Some(keine_core::CameraShakeTweenSpec {
                shake: CameraShakeSpec {
                    amplitude: 4.0,
                    frequency: 2.0,
                    duration: 2.0,
                    axis: CameraShakeAxis::Both,
                    falloff: CameraShakeFalloff::Linear,
                },
                randomness: Default::default(),
            }),
            fields: vec![
                keine_core::CameraTweenField::ShakeAmplitude,
                keine_core::CameraTweenField::ShakeFrequency,
                keine_core::CameraTweenField::SpeedLinesDensity,
                keine_core::CameraTweenField::SpeedLinesRegionWidth,
            ],
            duration: 1.,
            easing: Easing::Linear,
            blocking: true,
        };
        assert!(spec.start(&mut state, true));
        assert_eq!(state.camera_effect.v2.speed_lines_intensity, 0.8);
        assert_eq!(state.camera_effect.v2.speed_lines_region_width, 1.);
        assert!(advance_camera_transitions(&mut state, 0.5));
        assert_eq!(state.camera_effect.v2.speed_lines_intensity, 0.8);
        let shake = state.camera_shake.as_ref().unwrap();
        assert_eq!((shake.spec.amplitude, shake.spec.frequency), (3.0, 1.5));
        assert!((state.camera_effect.v2.speed_lines_density - 0.75).abs() < 0.00001);
        assert_eq!(state.camera_effect.v2.speed_lines_region_width, 1.5);
        assert!(advance_camera_transitions(&mut state, 0.5));
        assert_eq!(state.camera_effect.v2.speed_lines_density, 0.95);
        assert_eq!(state.camera_effect.v2.speed_lines_region_width, 2.);
        assert!(state.camera_effect_animation.is_none());
        assert!(!state.presentation_blocked());
        assert!(state.camera_shake.is_some());
        advance_camera_transitions(&mut state, 1.0);
        assert!(state.camera_shake.is_none());
    }

    #[test]
    fn held_control_autoplays_without_skipping_and_stops_on_release() {
        let mut toggles = ToggleStates {
            skip: true,
            ..default()
        };
        let mut timer = 5.0;
        let held = InputActions {
            auto_held: true,
            ..default()
        };

        update_toggle_shortcuts(&held, &mut toggles, &mut timer);
        assert!(toggles.auto);
        assert!(!toggles.skip);
        assert_eq!(timer, 0.0);
        timer = 0.25;
        update_toggle_shortcuts(&held, &mut toggles, &mut timer);
        assert_eq!(
            timer, 0.25,
            "holding Ctrl must not restart the autoplay timer"
        );

        // Any runtime boundary that clears the effective toggle cannot cancel
        // a physical key that is still held on the next tick.
        toggles.auto = false;
        update_toggle_shortcuts(&held, &mut toggles, &mut 0.0);
        assert!(toggles.auto);
        assert!(!toggles.skip);

        let released = InputActions {
            auto_released: true,
            ..default()
        };
        update_toggle_shortcuts(&released, &mut toggles, &mut timer);
        assert!(!toggles.auto);
        assert_eq!(timer, 0.0);
    }

    #[test]
    fn control_k_toggle_survives_the_modifier_release() {
        let mut toggles = ToggleStates::default();
        let chord = InputActions {
            toggle_skip: true,
            auto_released: true,
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

    pub(super) fn dialogue_state() -> State {
        let mut state = State::new();
        state.current_scene = "main".into();
        state.cursor = 1;
        state.dialogue = Some(Dialogue {
            speaker: String::new(),
            speaker_color: None,
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
    fn native_dialogue_waits_before_and_between_unicode_text() {
        let mut state = State::new();
        state.current_scene = "main".into();
        state.insert_scene(
            "main".into(),
            vec![Action::EiyashouSay(keine_core::EiyashouDialogue {
                speaker: "Rin".into(),
                speaker_color: None,
                text: keine_core::EiyashouText::literal("[wait=500]前[wait=500][wait=1000]後"),
                options: keine_core::SayOptions::default(),
                source_id: "inline-wait".into(),
            })],
        );
        assert_eq!(step::step(&mut state), step::StepResult::AwaitClick);
        assert_eq!(state.dialogue.as_ref().unwrap().text, "前後");
        assert_eq!(state.dialogue.as_ref().unwrap().markup, "前後");
        assert_eq!(
            state.active_dialogue_source_id.as_deref(),
            Some("inline-wait")
        );
        let mut clock = TypewriterClock::default();
        update_typewriter(&mut state, 0., 10., &mut clock);
        update_typewriter(&mut state, 0.4, 10., &mut clock);
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 0);
        update_typewriter(&mut state, 0.2, 10., &mut clock);
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 1);
        update_typewriter(&mut state, 1.4, 10., &mut clock);
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 1);
        update_typewriter(&mut state, 0.3, 10., &mut clock);
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 2);
        assert_eq!(state.dialogue.as_ref().unwrap().text, "前後");
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
    #[cfg(not(feature = "hot-reload"))]
    fn held_autoplay_reveals_unread_text_at_normal_speed_and_cancels_at_a_modal() {
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_millis(50));
        app.insert_resource(time)
            .insert_resource(GameState(dialogue_state()))
            .init_resource::<crate::storage::save::ContinuationCheckpoint>()
            .init_resource::<RuntimeSettings>()
            .insert_resource(InputActions {
                auto_held: true,
                ..default()
            })
            .init_resource::<ToggleStates>()
            .init_resource::<UiInputScope>()
            .insert_resource(AssetLoadingGate { blocked: false })
            .add_systems(Update, tick);
        app.update();
        let first = app
            .world()
            .resource::<GameState>()
            .dialogue
            .as_ref()
            .unwrap()
            .visible_chars;
        assert!(first > 0 && first < 10);
        app.update();
        let second = app
            .world()
            .resource::<GameState>()
            .dialogue
            .as_ref()
            .unwrap()
            .visible_chars;
        assert!(second > first && second < 10);
        assert!(app.world().resource::<ToggleStates>().auto);
        assert!(!app.world().resource::<ToggleStates>().skip);
        *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Dialog;
        *app.world_mut().resource_mut::<InputActions>() = InputActions {
            auto_released: true,
            ..default()
        };
        app.update();
        assert!(!app.world().resource::<ToggleStates>().auto);
        *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Stage;
        *app.world_mut().resource_mut::<InputActions>() = InputActions::default();
        app.update();
        assert!(!app.world().resource::<ToggleStates>().auto);
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
    fn infinite_floating_text_settles_and_keeps_its_authored_exit() {
        let mut state = State::new();
        state.floating_text = Some(keine_core::state::FloatingTextState {
            id: Some("caption".into()),
            text: "persistent".into(),
            position: [960.0, 400.0],
            font_size: 38.0,
            color: [1.0; 4],
            fade_in: 0.5,
            hold: 2.0,
            fade_out: 0.5,
            elapsed: 0.0,
            blocking: false,
            infinite: true,
        });
        assert!(update_transitions(&mut state, 0.25, false));
        assert_eq!(state.floating_text.as_ref().unwrap().alpha(), 0.5);
        assert!(update_transitions(&mut state, 0.25, false));
        let settled = state.floating_text.clone();
        assert!(!update_transitions(&mut state, 60.0, false));
        assert_eq!(state.floating_text, settled);

        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::HideFloatingText {
                    id: Some("caption".into()),
                },
                Action::Say {
                    speaker: String::new(),
                    text: "after caption".into(),
                    options: Default::default(),
                },
            ],
        )]));
        state.current_scene = "main".into();
        step::step(&mut state);
        assert!(!state.floating_text.as_ref().unwrap().infinite);
        assert!(update_transitions(&mut state, 0.25, false));
        assert_eq!(state.floating_text.as_ref().unwrap().alpha(), 0.5);
        assert!(update_transitions(&mut state, 0.25, false));
        assert!(state.floating_text.is_none());
        assert_eq!(state.dialogue.as_ref().unwrap().text, "after caption");
    }

    #[test]
    fn curtain_resumes_the_script_after_a_2200ms_fade_at_60fps() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::Curtain {
                    visible: false,
                    color: [0.0, 0.0, 0.0, 1.0],
                    duration: 2.2,
                },
                Action::Say {
                    speaker: String::new(),
                    text: "after curtain".into(),
                    options: Default::default(),
                },
            ],
        )]));
        state.current_scene = "main".into();
        state.curtain.current = 1.0;
        assert_eq!(
            step::step(&mut state),
            keine_core::StepResult::AwaitPresentation
        );

        let mut checkpoint = Default::default();
        for _ in 0..180 {
            if !state.presentation_blocked() {
                break;
            }
            update_transitions(&mut state, 1.0 / 60.0, false);
            if !state.presentation_blocked() {
                step_once(&mut state, &mut checkpoint);
            }
        }

        assert!(!state.presentation_blocked());
        assert_eq!(state.dialogue.as_ref().unwrap().text, "after curtain");
    }

    pub(super) fn playback_test_app(state: State, auto: bool) -> App {
        use crate::runtime::platform::{RuntimeActivity, update_lifecycle};
        use crate::ui::control_bar::AutoHideTiming;
        use bevy::time::{TimePlugin, TimeUpdateStrategy};
        let mut app = App::new();
        app.add_plugins(TimePlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f64(1.0 / 60.0),
            ))
            .insert_resource(GameState(state))
            .init_resource::<crate::storage::save::ContinuationCheckpoint>()
            .init_resource::<RuntimeSettings>()
            .init_resource::<InputActions>()
            .insert_resource(ToggleStates { auto, ..default() })
            .init_resource::<UiInputScope>()
            .init_resource::<LocalAssetManifest>()
            .insert_resource(AssetLoadingGate { blocked: false })
            .init_resource::<crate::ui::activity::UiAnimationActivity>()
            .init_resource::<crate::scene::audio::AudioAnimationActivity>()
            .init_resource::<AutoHideTiming>()
            .init_resource::<crate::ui::user_input::UserInputCaretBlink>()
            .init_resource::<RuntimeActivity>()
            .insert_resource(bevy::winit::WinitSettings::desktop_app())
            .add_systems(Update, tick)
            .add_systems(Last, update_lifecycle);
        #[cfg(feature = "hot-reload")]
        {
            let opened = crate::runtime::bootstrap::open_project(
                std::path::Path::new(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/native-smoke"
                )),
                &keine_loader::LoaderRegistry::default(),
            )
            .unwrap();
            app.insert_resource(ContentProjectResource(opened.content))
                .insert_resource(crate::runtime::resources::GameConfigResource(opened.config))
                .insert_resource(ScriptLanguages(
                    keine_loader::ScriptLanguageRegistry::default(),
                ))
                .init_resource::<crate::scene::images::ImageRoleRegistry>();
        }
        app.world_mut().spawn(Window {
            focused: true,
            ..default()
        });
        app
    }

    #[test]
    fn settled_presentation_clocks_resume_without_player_input() {
        use crate::runtime::platform::RuntimeActivity;

        for auto in [false, true] {
            for case in [
                "curtain-show",
                "curtain-hide",
                "mask-show",
                "mask-hide",
                "wait",
            ] {
                let mut state = State::new();
                state.install_program(Program::from_scenes([(
                    "main".into(),
                    vec![Action::Say {
                        speaker: String::new(),
                        text: "after presentation".into(),
                        options: Default::default(),
                    }],
                )]));
                state.current_scene = "main".into();
                let target = if case.ends_with("hide") { 0.0 } else { 1.0 };
                if case.starts_with("curtain") {
                    state.curtain = keine_core::state::CurtainState {
                        current: target,
                        from: 1.0 - target,
                        target,
                        elapsed: 0.25999,
                        duration: 0.26,
                        blocking: true,
                        ..default()
                    };
                } else if case.starts_with("mask") {
                    state.stage_masks.insert(
                        "mask".into(),
                        keine_core::state::StageMaskState {
                            mask: Default::default(),
                            current: target,
                            from: 1.0 - target,
                            target,
                            elapsed: 0.25999,
                            duration: 0.26,
                            blocking: true,
                            order: 0,
                        },
                    );
                } else {
                    state.wait_remaining = f32::EPSILON * 0.5;
                    state.wait_blocking = true;
                }
                let mut app = playback_test_app(state, auto);
                // TimePlugin's initial frame has zero delta. A settled visual
                // must still keep virtual time alive for the unfinished clock.
                app.update();
                assert_eq!(
                    *app.world().resource::<RuntimeActivity>(),
                    RuntimeActivity::Active,
                    "{case}, auto={auto}"
                );
                assert!(
                    !app.world().resource::<Time<Virtual>>().is_paused(),
                    "{case}, auto={auto}"
                );
                app.update();
                let state = app.world().resource::<GameState>();
                assert!(!state.presentation_blocked(), "{case}, auto={auto}");
                assert_eq!(
                    state.dialogue.as_ref().unwrap().text,
                    "after presentation",
                    "{case}, auto={auto}"
                );
                assert!(!app.world().resource::<InputActions>().advance);
            }
        }
    }

    #[test]
    fn playback_camera_transform_keeps_time_active_until_completion() {
        for blocking in [false, true] {
            let mut state = dialogue_state();
            state.dialogue.as_mut().unwrap().visible_chars = 100;
            state.camera_transform_animation = Some(TransformAnimation {
                from: SpriteTransform::default(),
                to: SpriteTransform {
                    offset_x: 100.0,
                    ..Default::default()
                },
                elapsed: 0.0,
                duration: 0.05,
                easing: Easing::Linear,
                blocking,
            });
            let mut app = playback_test_app(state, false);
            app.update();
            assert_eq!(
                *app.world()
                    .resource::<crate::runtime::platform::RuntimeActivity>(),
                crate::runtime::platform::RuntimeActivity::Active,
                "blocking={blocking}"
            );
            for _ in 0..6 {
                app.update();
            }
            let state = app.world().resource::<GameState>();
            assert!(
                state.camera_transform_animation.is_none(),
                "blocking={blocking}"
            );
            assert_eq!(state.camera_transform.offset_x, 100.0);
        }
    }

    #[test]
    fn playback_backend_completion_resumes_after_the_loading_or_menu_gate() {
        for caption in [false, true] {
            for scope in [
                UiInputScope::Stage,
                UiInputScope::Menu,
                UiInputScope::Loading,
            ] {
                let mut state = State::new();
                state.current_scene = "main".into();
                state.install_program(Program::from_scenes([(
                    "main".into(),
                    vec![
                        Action::Flow {
                            action: Box::new(Action::Say {
                                speaker: String::new(),
                                text: "caption during video".into(),
                                options: Default::default(),
                            }),
                            when: Some(caption.to_string()),
                            next: true,
                        },
                        Action::PlayVideo {
                            video: VideoSpec {
                                id: "opening".into(),
                                file: "opening.mp4".into(),
                                wait_for_finished: true,
                                looped: false,
                                muted: true,
                                alpha: 1.0,
                                skippable: true,
                                mode: VideoMode::Fullscreen,
                            },
                        },
                        Action::Say {
                            speaker: String::new(),
                            text: "after video".into(),
                            options: Default::default(),
                        },
                    ],
                )]));
                let mut checkpoint = crate::storage::save::ContinuationCheckpoint::default();
                crate::runtime::script_driver::resume(&mut state, &mut checkpoint);
                assert!(state.presentation_blocked());
                let mut app = playback_test_app(state, false);
                *app.world_mut().resource_mut::<UiInputScope>() = scope;
                app.world_mut().resource_mut::<AssetLoadingGate>().blocked =
                    scope == UiInputScope::Loading;
                // Backends run after tick. Completion or rejection clears video state
                // in Sync, including while a menu is covering the stage.
                app.add_systems(PostUpdate, |mut state: ResMut<GameState>| {
                    state.videos.clear();
                });
                app.update();
                if scope != UiInputScope::Stage {
                    app.update();
                    assert_eq!(app.world().resource::<GameState>().cursor, 2);
                    assert_eq!(
                        app.world().resource::<GameState>().dialogue.is_some(),
                        caption
                    );
                }
                *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Stage;
                app.world_mut().resource_mut::<AssetLoadingGate>().blocked = false;
                app.update();
                let state = app.world().resource::<GameState>();
                assert_eq!(
                    state.dialogue.as_ref().map(|d| d.text.as_str()),
                    Some("after video"),
                    "scope={scope:?}"
                );
                assert_eq!(state.cursor, 3);
            }
        }
    }

    #[test]
    fn playback_auto_waits_for_pending_voice_and_releases_after_failed_player_cleanup() {
        let mut app = voiced_auto_test_app();
        app.world_mut().resource_mut::<RuntimeSettings>().auto_delay = 0.0;
        // The first tick precedes sync: even a zero-delay, fully revealed line
        // must not disappear before its voice player can be created.
        app.update();
        assert_eq!(app.world().resource::<GameState>().cursor, 1);
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<crate::scene::audio::VocalPlayer>>()
            .single(app.world())
            .unwrap();
        for _ in 0..120 {
            app.update();
        }
        assert_eq!(app.world().resource::<GameState>().cursor, 1);
        // Restoring the same cursor queues a new cue even though the dialogue
        // key matches. An old finished/absent player cannot satisfy that cue.
        app.world_mut().despawn(entity);
        assert!(
            app.world_mut()
                .resource_mut::<GameState>()
                .restore_backlog(0)
        );
        app.update();
        assert_eq!(app.world().resource::<GameState>().cursor, 1);
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<crate::scene::audio::VocalPlayer>>()
            .single(app.world())
            .unwrap();
        // Failed-source cleanup removes the pending player; the synchronized
        // dialogue key must not cause a permanent wait or replay the source.
        #[cfg(feature = "audio-seekable")]
        app.add_systems(
            PreUpdate,
            crate::scene::audio::retire_failed_players::<crate::runtime::audio::SeekableAudio>,
        );
        #[cfg(not(feature = "audio-seekable"))]
        app.add_systems(
            PreUpdate,
            crate::scene::audio::retire_failed_players::<AudioSource>,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while app.world().resource::<GameState>().cursor == 1 {
            app.update();
            assert!(
                std::time::Instant::now() < deadline,
                "failed voice was not retired"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(app.world().get_entity(entity).is_err());
        assert_eq!(app.world().resource::<GameState>().cursor, 2);
    }

    fn voiced_auto_test_app() -> App {
        use crate::scene::audio::{VocalPlayback, sync_vocal};
        use bevy::asset::AssetApp;
        let mut state = State::new();
        state.current_scene = "main".into();
        state.install_program(Program::from_scenes([(
            "main".into(),
            ["first", "second", "third"]
                .map(|text| Action::Say {
                    speaker: String::new(),
                    text: text.into(),
                    options: keine_core::SayOptions {
                        vocal: Some("voice.wav".into()),
                        ..Default::default()
                    },
                })
                .to_vec(),
        )]));
        crate::runtime::script_driver::resume(&mut state, &mut Default::default());
        state.dialogue.as_mut().unwrap().visible_chars = 5;
        let mut app = playback_test_app(state, true);
        // No device or decoder: sync creates real pending players; tests attach
        // an offline Rodio sink when they need to consume playback samples.
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::asset::AssetPlugin::default(),
        ))
        .init_asset::<AudioSource>()
        .init_resource::<VocalPlayback>()
        .insert_resource(crate::runtime::resources::GameConfigResource(
            Default::default(),
        ))
        .add_systems(Update, sync_vocal.after(tick));
        #[cfg(feature = "audio-seekable")]
        app.init_asset::<crate::runtime::audio::SeekableAudio>();
        app
    }

    #[cfg(any(
        feature = "audio-opus",
        feature = "audio-seekable",
        feature = "video-ffmpeg"
    ))]
    #[test]
    fn playback_auto_waits_for_voice_eof_and_then_reading_delay() {
        let mut app = voiced_auto_test_app();
        app.world_mut().resource_mut::<RuntimeSettings>().auto_delay = 0.1;
        app.update();
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<crate::scene::audio::VocalPlayer>>()
            .single(app.world())
            .unwrap();
        let (player, mut output) = rodio::Player::new();
        player.append(rodio::buffer::SamplesBuffer::new(
            rodio::ChannelCount::new(1).unwrap(),
            rodio::SampleRate::new(48_000).unwrap(),
            vec![0.0; 48_000],
        ));
        app.world_mut()
            .entity_mut(entity)
            .insert(AudioSink::new(player));
        // Paused or muted does not mean finished. Neither simulated time nor
        // volume may bypass the actual sample queue.
        for paused in [false, true] {
            let mut sink = app.world_mut().get_mut::<AudioSink>(entity).unwrap();
            sink.mute();
            if paused {
                sink.pause();
            }
            for _ in 0..120 {
                app.update();
            }
            assert_eq!(app.world().resource::<GameState>().cursor, 1);
        }
        app.world().get::<AudioSink>(entity).unwrap().play();
        for _ in 0..50_000 {
            output.next();
        }
        assert!(app.world().get::<AudioSink>(entity).unwrap().empty());
        *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Menu;
        for _ in 0..120 {
            app.update();
        }
        assert_eq!(app.world().resource::<GameState>().cursor, 1);
        *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Stage;
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(app.world().resource::<GameState>().cursor, 1);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<GameState>().cursor, 2);
        // The same file on the next line still gets a fresh pending player.
        for _ in 0..120 {
            app.update();
        }
        assert_eq!(app.world().resource::<GameState>().cursor, 2);
    }

    #[test]
    fn playback_auto_without_voice_keeps_the_configured_reading_delay() {
        let mut app = voiced_auto_test_app();
        app.world_mut()
            .resource_mut::<GameState>()
            .dialogue
            .as_mut()
            .unwrap()
            .vocal = None;
        app.world_mut().resource_mut::<RuntimeSettings>().auto_delay = 0.1;
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(app.world().resource::<GameState>().cursor, 1);
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(app.world().resource::<GameState>().cursor, 2);
    }

    #[test]
    fn playback_manual_skip_and_authored_notend_can_interrupt_voice() {
        for mode in ["manual", "skip", "notend"] {
            let mut app = voiced_auto_test_app();
            app.update();
            match mode {
                "manual" => app.world_mut().resource_mut::<InputActions>().advance = true,
                "skip" => {
                    let mut toggles = app.world_mut().resource_mut::<ToggleStates>();
                    toggles.skip = true;
                    toggles.skip_mode = SkipMode::All;
                }
                _ => {
                    app.world_mut()
                        .resource_mut::<GameState>()
                        .dialogue
                        .as_mut()
                        .unwrap()
                        .auto_advance = true
                }
            }
            app.update();
            assert_eq!(app.world().resource::<GameState>().cursor, 2, "{mode}");
        }
    }

    #[test]
    fn playback_loading_and_menu_gates_do_not_swallow_auto_release() {
        for loading in [false, true] {
            let mut state = State::new();
            state.current_scene = "main".into();
            state.install_program(Program::from_scenes([(
                "main".into(),
                vec![
                    Action::Say {
                        speaker: String::new(),
                        text: "first".into(),
                        options: Default::default(),
                    },
                    Action::Say {
                        speaker: String::new(),
                        text: "second".into(),
                        options: Default::default(),
                    },
                ],
            )]));
            crate::runtime::script_driver::resume(&mut state, &mut Default::default());
            state.dialogue.as_mut().unwrap().visible_chars = 5;
            let mut app = playback_test_app(state, true);
            app.world_mut().resource_mut::<ToggleStates>().skip = true;
            app.world_mut().resource_mut::<InputActions>().auto_released = true;
            app.world_mut().resource_mut::<RuntimeSettings>().auto_delay = 0.0;
            app.world_mut().resource_mut::<AssetLoadingGate>().blocked = loading;
            *app.world_mut().resource_mut::<UiInputScope>() = if loading {
                UiInputScope::Loading
            } else {
                UiInputScope::Menu
            };
            app.update();
            let toggles = app.world().resource::<ToggleStates>();
            assert!(!toggles.auto && !toggles.skip, "loading={loading}");
            assert_eq!(app.world().resource::<GameState>().cursor, 1);
            *app.world_mut().resource_mut::<InputActions>() = Default::default();
            *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Stage;
            app.world_mut().resource_mut::<AssetLoadingGate>().blocked = false;
            app.update();
            assert_eq!(
                app.world().resource::<GameState>().cursor,
                1,
                "released auto must not resume after gate, loading={loading}"
            );
        }
    }

    #[test]
    fn playback_skip_video_respects_skippable_and_looping_flags() {
        for skippable in [false, true] {
            for looped in [false, true] {
                let mut state = State::new();
                state.current_scene = "main".into();
                state.install_program(Program::from_scenes([(
                    "main".into(),
                    vec![
                        Action::PlayVideo {
                            video: VideoSpec {
                                id: "video".into(),
                                file: "video.mp4".into(),
                                wait_for_finished: true,
                                looped,
                                muted: true,
                                alpha: 1.0,
                                skippable,
                                mode: VideoMode::Fullscreen,
                            },
                        },
                        Action::Say {
                            speaker: String::new(),
                            text: "after".into(),
                            options: Default::default(),
                        },
                        Action::Say {
                            speaker: String::new(),
                            text: "tail".into(),
                            options: Default::default(),
                        },
                    ],
                )]));
                crate::runtime::script_driver::resume(&mut state, &mut Default::default());
                let mut app = playback_test_app(state, false);
                app.world_mut().resource_mut::<InputActions>().skip_video = true;
                app.update();
                let state = app.world().resource::<GameState>();
                assert_eq!(
                    state.videos.contains_key("video"),
                    !skippable || looped,
                    "skippable={skippable}, looped={looped}"
                );
                assert_eq!(state.cursor, if skippable || looped { 2 } else { 1 });
                assert_eq!(state.presentation_blocked(), !skippable && !looped);
            }
        }
    }

    #[test]
    fn playback_manual_and_automatic_advance_cannot_cross_two_yields_in_one_frame() {
        for notend in [false, true] {
            let mut state = State::new();
            state.current_scene = "main".into();
            state.install_program(Program::from_scenes([(
                "main".into(),
                vec![
                    Action::Say {
                        speaker: String::new(),
                        text: "first".into(),
                        options: keine_core::SayOptions {
                            auto_advance: notend,
                            ..Default::default()
                        },
                    },
                    Action::Say {
                        speaker: String::new(),
                        text: String::new(),
                        options: Default::default(),
                    },
                    Action::Say {
                        speaker: String::new(),
                        text: "must remain unread".into(),
                        options: Default::default(),
                    },
                ],
            )]));
            crate::runtime::script_driver::resume(
                &mut state,
                &mut crate::storage::save::ContinuationCheckpoint::default(),
            );
            state.dialogue.as_mut().unwrap().visible_chars = 5;
            let mut app = playback_test_app(state, true);
            app.world_mut().resource_mut::<RuntimeSettings>().auto_delay = 0.0;
            app.world_mut().resource_mut::<InputActions>().advance = true;
            app.update();
            assert_eq!(
                app.world().resource::<GameState>().cursor,
                2,
                "notend={notend}"
            );
            assert_eq!(app.world().resource::<GameState>().backlog.len(), 2);
        }
    }

    #[test]
    fn playback_typewriter_restore_at_the_same_cursor_restarts_inline_timing() {
        let mut state = dialogue_state();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Comment],
        )]));
        state.dialogue.as_mut().unwrap().pauses = vec![DialoguePause {
            at: 2,
            duration: None,
        }];
        let saved = state.clone();
        let mut clock = TypewriterClock::default();
        update_typewriter(&mut state, 0.0, 10.0, &mut clock);
        update_typewriter(&mut state, 0.2, 10.0, &mut clock);
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 2);
        state.restore_saved(saved).unwrap();
        update_typewriter(&mut state, 0.0, 10.0, &mut clock);
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 1);
        update_typewriter(&mut state, 0.2, 10.0, &mut clock);
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 2);
        assert!(matches!(clock.pause, TypewriterPause::Input));
    }

    #[test]
    fn playback_restore_resets_zero_character_wait_and_auto_delay() {
        for wait_at_zero in [false, true] {
            let mut state = State::new();
            state.current_scene = "main".into();
            state.install_program(Program::from_scenes([(
                "main".into(),
                vec![
                    Action::Say {
                        speaker: String::new(),
                        text: "first".into(),
                        options: Default::default(),
                    },
                    Action::Say {
                        speaker: String::new(),
                        text: "second".into(),
                        options: Default::default(),
                    },
                ],
            )]));
            crate::runtime::script_driver::resume(
                &mut state,
                &mut crate::storage::save::ContinuationCheckpoint::default(),
            );
            if wait_at_zero {
                state.dialogue.as_mut().unwrap().pauses = vec![DialoguePause {
                    at: 0,
                    duration: Some(1.0),
                }];
            } else {
                state.dialogue.as_mut().unwrap().visible_chars = 5;
            }
            let saved = state.clone();
            let mut app = playback_test_app(state, !wait_at_zero);
            app.world_mut().resource_mut::<RuntimeSettings>().auto_delay = 0.25;
            for _ in 0..if wait_at_zero { 50 } else { 10 } {
                app.update();
            }
            {
                let mut live = app.world_mut().resource_mut::<GameState>();
                live.restore_saved(saved).unwrap();
            }
            app.world_mut().resource_scope(
                |world, mut checkpoint: Mut<crate::storage::save::ContinuationCheckpoint>| {
                    checkpoint.reset(world.resource::<GameState>());
                },
            );
            for _ in 0..10 {
                app.update();
            }
            let live = app.world().resource::<GameState>();
            assert_eq!(live.cursor, 1, "wait_at_zero={wait_at_zero}");
            if wait_at_zero {
                assert_eq!(live.dialogue.as_ref().unwrap().visible_chars, 0);
            }
        }
    }

    #[test]
    fn playback_return_transition_cannot_restart_or_advance_the_script() {
        let mut state = State::new();
        state.current_scene = "main".into();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Say {
                speaker: String::new(),
                text: "must not run during return".into(),
                options: Default::default(),
            }],
        )]));
        let mut app = playback_test_app(state, true);
        app.insert_resource(crate::ui::title::ReturnToTitleTransition::default());
        app.world_mut().resource_mut::<InputActions>().advance = true;
        app.world_mut().resource_mut::<RuntimeSettings>().auto_delay = 0.0;
        for _ in 0..6 {
            app.update();
        }
        let state = app.world().resource::<GameState>();
        assert_eq!(state.cursor, 0);
        assert!(state.dialogue.is_none());
        assert!(state.backlog.is_empty());
    }

    #[test]
    fn playback_invisible_video_waits_for_completion_until_explicitly_stopped() {
        let mut state = State::new();
        state.current_scene = "main".into();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::PlayVideo {
                video: VideoSpec {
                    id: "audio-only".into(),
                    file: "movie.mp4".into(),
                    looped: false,
                    muted: false,
                    alpha: 0.0,
                    skippable: true,
                    wait_for_finished: true,
                    mode: VideoMode::Fullscreen,
                },
            }],
        )]));
        crate::runtime::script_driver::resume(
            &mut state,
            &mut crate::storage::save::ContinuationCheckpoint::default(),
        );
        update_transitions(&mut state, 0.02, false);
        assert!(state.videos.contains_key("audio-only"));
        assert!(
            state.presentation_blocked(),
            "opacity zero is not a completion signal"
        );
        let video = state.videos.get_mut("audio-only").unwrap();
        video.stopping = true;
        video.fade_out = 1.0;
        update_transitions(&mut state, 0.02, false);
        assert!(state.videos.is_empty());
        assert!(!state.presentation_blocked());
    }

    #[test]
    fn curtain_and_masks_finish_even_when_opacity_has_already_settled() {
        for target in [0.0, 1.0] {
            let mut state = State::new();
            state.curtain = keine_core::state::CurtainState {
                current: target,
                from: 1.0 - target,
                target,
                elapsed: 2.1999,
                duration: 2.2,
                blocking: true,
                ..default()
            };
            state.stage_masks.insert(
                "mask".into(),
                keine_core::state::StageMaskState {
                    mask: Default::default(),
                    current: target,
                    from: 1.0 - target,
                    target,
                    elapsed: 2.1999,
                    duration: 2.2,
                    blocking: true,
                    order: 0,
                },
            );

            update_transitions(&mut state, 1.0 / 60.0, false);

            assert!(!state.presentation_blocked());
            assert_eq!(state.curtain.current, target);
            assert_eq!(state.curtain.elapsed, 2.2);
        }
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

    #[test]
    fn tday_normal_route_reaches_all_chapters_and_restores_live_and_presentation_saves() {
        use crate::storage::save::{
            ContinuationCheckpoint, SavePreviewCoordinator, load_game,
            save_manual_replacing_preview,
        };
        use keine_loader::{KeineStore, StoreAdapter};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("projects/tday");
        let loader = keine_loader::LoaderRegistry::default();
        let project = crate::runtime::bootstrap::open_project(&root, &loader).unwrap();
        assert_eq!(project.config.script.entry, "start");
        let scenes = keine_loader::load_scenes(&project.content).unwrap();
        assert_eq!(scenes.len(), 13);
        assert!(scenes.iter().all(|scene| scene.diagnostics.is_empty()));
        let mut state = State::new();
        state.install_program(Program::from_scenes(
            scenes.into_iter().map(|scene| (scene.name, scene.actions)),
        ));
        state.current_scene = "start".into();
        state.script_entry = Some("start".into());
        let mut checkpoint = ContinuationCheckpoint::default();
        let mut chapters = std::collections::BTreeSet::new();
        let mut saved_chapters = std::collections::BTreeSet::new();
        let mut fallback_saved = false;
        let mut dialogue_count = 0;
        let mut finished = false;
        let save_root =
            std::env::temp_dir().join(format!("keine-tday-route-{}", std::process::id()));
        std::fs::create_dir_all(&save_root).unwrap();
        for _ in 0..10_000 {
            let outcome = crate::runtime::script_driver::resume(&mut state, &mut checkpoint);
            chapters.insert(state.current_scene.clone());
            assert!(
                state.script_diagnostic.is_none(),
                "{:?}",
                state.script_diagnostic
            );
            if outcome.returns_to_title() {
                assert_eq!(
                    outcome,
                    crate::runtime::script_driver::ScriptOutcome::ReturnToTitle(
                        crate::runtime::script_driver::ReturnReason::EndOfScene
                    )
                );
                finished = true;
                break;
            }
            if state.persistence_safety().is_exact()
                && state.dialogue.is_some()
                && saved_chapters.insert(state.current_scene.clone())
            {
                let encoded = KeineStore.encode(&state).unwrap();
                let mut restored = state.clone();
                restored.current_scene = "start".into();
                restored.cursor = 0;
                KeineStore
                    .decode(&encoded)
                    .unwrap()
                    .restore_into(&mut restored)
                    .unwrap();
                assert_eq!(
                    (restored.current_scene.as_str(), restored.cursor),
                    (state.current_scene.as_str(), state.cursor)
                );
                assert_eq!(restored.dialogue, state.dialogue);
                assert_eq!(restored.unlocked_bgm, state.unlocked_bgm);
            } else if !fallback_saved
                && !state.persistence_safety().is_exact()
                && checkpoint.state_for_continuation(&state).is_some()
            {
                let position = (state.current_scene.clone(), state.cursor);
                let saved = save_manual_replacing_preview(
                    &KeineStore,
                    &state,
                    &checkpoint,
                    1,
                    &save_root,
                    &SavePreviewCoordinator::default(),
                )
                .unwrap();
                assert!(saved.used_checkpoint);
                let expected = checkpoint.state_for_continuation(&state).unwrap();
                let mut restored = state.clone();
                load_game(&KeineStore, 1, &save_root)
                    .unwrap()
                    .restore_into(&mut restored)
                    .unwrap();
                assert_eq!(
                    (restored.current_scene.as_str(), restored.cursor),
                    (expected.current_scene.as_str(), expected.cursor)
                );
                assert_eq!(
                    (state.current_scene.clone(), state.cursor),
                    position,
                    "Saving must not advance the live script"
                );
                fallback_saved = true;
            }
            if state.presentation_blocked() {
                assert!(finish_editor_presentation(&mut state));
            } else if state.dialogue.is_some() {
                dialogue_count += 1;
                step::advance(&mut state);
            } else {
                assert!(state.menu.is_none() && state.user_input.is_none());
            }
            // Native audio/UI hosts consume transient events independently.
            state.effect_queue.clear();
            state.shell_events.clear();
            state.host_commands.clear();
        }
        std::fs::remove_dir_all(save_root).unwrap();
        assert!(finished && fallback_saved);
        assert_eq!(chapters.len(), 13);
        assert_eq!(saved_chapters.len(), 13);
        assert!(dialogue_count > 900);
        assert_eq!(state.unlocked_cg.len(), 1);
        assert_eq!(
            state.unlocked_cg.values().next().unwrap(),
            "06_tree_shade_path"
        );
        assert_eq!(
            state
                .unlocked_bgm
                .values()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([
                "bedroom", "bridge", "distance", "dive", "night", "shimmer"
            ])
        );
        assert!(state.bgm.file.is_none() && state.looping_effects.is_empty());
    }
}
