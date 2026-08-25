// Step engine — executes Actions until an interactive point.
//
// Inspired by Ayaka's next_run() and Siglus's step_until_yield().
// The key insight: VN execution pauses at user interaction points (click/choice),
// NOT at every frame. This is how we achieve "frame rate insensitive" execution.

use std::sync::Arc;

use log::debug;
use unicode_segmentation::UnicodeSegmentation;

use crate::action::Action;
use crate::action::ChoiceTarget;
use crate::expression::{evaluate, interpolate};
use crate::state::{
    BgTransition, Dialogue, DialogueRetraction, IntroState, KeyframeAnimation, MenuChoice,
    MenuState, PresetAnimation, SceneFrame, Sprite, State, TransformAnimation, TransitionRule,
};
use crate::types::{Anchor, Transition};

/// Result of a step() call.
#[derive(Debug, Clone, PartialEq)]
pub enum StepResult {
    /// Engine is waiting for the user to click (dialogue shown).
    AwaitClick,
    /// Engine is waiting for the user to choose (menu shown).
    AwaitChoice,
    /// Engine is waiting for a timed presentation action to finish.
    AwaitPresentation,
    /// Engine is waiting for text input confirmation.
    AwaitInput,
    /// No more actions in this scene.
    EndOfScene,
    /// Forward execution exceeded the deterministic safety limit.
    ExecutionLimit,
}

const MAX_FORWARD_ACTIONS: usize = 1024;

/// Execute actions from the current cursor position until we hit
/// an interactive point (Say or Menu) or end of scene.
pub fn step(state: &mut State) -> StepResult {
    step_inner(state, None, true)
}

/// Execute one runtime step while deferring destructive end-of-game cleanup.
///
/// Native frontends use this while fading back to the title screen so the
/// final authored frame remains visible. The frontend must call [`end_game`]
/// after its transition completes.
pub fn step_preserving_presentation(state: &mut State) -> StepResult {
    step_inner(state, None, false)
}

/// Execute like [`step`], but never cross the requested cursor in one scene.
/// Editor adapters use this to reconstruct a deterministic preview state.
pub fn step_until_cursor(
    state: &mut State,
    target_scene: &str,
    target_cursor: usize,
) -> StepResult {
    step_inner(state, Some((target_scene, target_cursor)), true)
}

fn step_inner(state: &mut State, stop: Option<(&str, usize)>, cleanup_on_end: bool) -> StepResult {
    if state.menu.is_some() {
        return StepResult::AwaitChoice;
    }
    if state.user_input.is_some() {
        return StepResult::AwaitInput;
    }
    if state.presentation_blocked() {
        return StepResult::AwaitPresentation;
    }

    // Keep an independent handle to immutable script data. Actions can then be
    // borrowed for dispatch while the rest of `state` is updated in place.
    let program = Arc::clone(&state.program);

    for _ in 0..MAX_FORWARD_ACTIONS {
        if stop
            .is_some_and(|(scene, cursor)| state.current_scene == scene && state.cursor >= cursor)
        {
            return StepResult::EndOfScene;
        }
        let Some(action) = program
            .scene(&state.current_scene)
            .and_then(|scene| scene.get(state.cursor))
        else {
            if let Some(frame) = state.scene_stack.pop() {
                state.current_scene = frame.scene;
                state.cursor = frame.cursor;
                continue;
            }
            return StepResult::EndOfScene;
        };
        state.cursor += 1;

        let (action, next) = match action {
            Action::Flow { action, when, next } => {
                if let Some(condition) = when {
                    match evaluate(condition, &state.vars, &state.global_vars) {
                        Ok(value) if value.truthy() => {}
                        Ok(_) => continue,
                        Err(error) => {
                            log::error!("invalid -when expression {condition:?}: {error}");
                            continue;
                        }
                    }
                }
                (action.as_ref(), *next)
            }
            action => (action, false),
        };

        // Script actions are infrequent relative to rendered frames. One
        // conservative revision bump here covers every sprite/camera mutation
        // branch without scattering an easy-to-miss manual invalidation.
        state.invalidate_stage();
        match action {
            Action::ShowBg {
                image,
                transition,
                transform,
            } => {
                let transition = *transition;
                let transform = *transform;
                let image = interpolate(image, &state.vars, &state.global_vars);
                debug!("ShowBg: {} ({:?})", image, transition);
                let from = state.bg.take();
                state.bg_camera_distance = None;
                state.bg_transform = transform;
                state.bg_transform_animation = None;
                let rule_animation = state.transition_rules.get("bg-main").and_then(|rule| {
                    rule.enter.as_ref().map(|preset| PresetAnimation {
                        preset: preset.clone(),
                        base: transform,
                        elapsed: 0.0,
                        duration: rule.duration.max(f32::EPSILON),
                        blocking: !next,
                        remove_on_finish: false,
                    })
                });
                let rule_blocks = rule_animation.is_some() && !next;
                if let Some(animation) = &rule_animation {
                    state.bg_transform =
                        preset_initial_transform(animation.base, &animation.preset);
                }
                state.bg_animation = rule_animation;
                state.bg_transition = if transition == Transition::Instant {
                    None
                } else {
                    Some(BgTransition {
                        from,
                        to: image.clone(),
                        progress: 0.0,
                        kind: transition,
                        blocking: !next,
                    })
                };
                state.bg = Some(image);
                if rule_blocks || (!next && transition != Transition::Instant) {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::HideBg { transition } => {
                let transition = *transition;
                if state.bg.is_some()
                    && let Some((preset, duration)) =
                        state.transition_rules.get("bg-main").and_then(|rule| {
                            rule.exit
                                .as_ref()
                                .map(|preset| (preset.clone(), rule.duration))
                        })
                {
                    state.bg_animation = Some(PresetAnimation {
                        preset,
                        base: state.bg_transform,
                        elapsed: 0.0,
                        duration: duration.max(f32::EPSILON),
                        blocking: !next,
                        remove_on_finish: true,
                    });
                    if !next {
                        return StepResult::AwaitPresentation;
                    }
                    continue;
                }
                let from = state.bg.take();
                if transition == Transition::Instant {
                    state.bg_camera_distance = None;
                }
                state.bg_transition = match (from, transition) {
                    (Some(from), transition) if transition != Transition::Instant => {
                        Some(BgTransition {
                            from: Some(from),
                            to: String::new(),
                            progress: 0.0,
                            kind: transition,
                            blocking: !next,
                        })
                    }
                    _ => None,
                };
                if !next && transition != Transition::Instant {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::SetTransform {
                id,
                transform: target,
                duration,
                easing,
            } => {
                let duration = *duration;
                let easing = *easing;
                let id = interpolate(id, &state.vars, &state.global_vars);
                debug!("SetTransform: {} -> {:?}", id, target);
                let mut started = false;
                if matches!(id.as_str(), "bg-main" | "background") {
                    started = true;
                    let target = target.apply_to(state.bg_transform);
                    if duration > 0.0 {
                        state.bg_keyframe_animation = None;
                        state.bg_transform_animation = Some(TransformAnimation {
                            from: state.bg_transform,
                            to: target,
                            elapsed: 0.0,
                            duration,
                            easing,
                            blocking: !next,
                        });
                    } else {
                        state.bg_transform = target;
                        state.bg_transform_animation = None;
                        state.bg_keyframe_animation = None;
                    }
                } else if is_character_group_target(&id) {
                    for sprite in state.sprites.values_mut() {
                        started = true;
                        let target = target.apply_to(sprite.transform);
                        if duration > 0.0 {
                            sprite.keyframe_animation = None;
                            sprite.transform_animation = Some(TransformAnimation {
                                from: sprite.transform,
                                to: target,
                                elapsed: 0.0,
                                duration,
                                easing,
                                blocking: !next,
                            });
                        } else {
                            sprite.transform = target;
                            sprite.transform_animation = None;
                            sprite.keyframe_animation = None;
                        }
                    }
                } else if let Some(sprite) = state.sprites.get_mut(&id) {
                    started = true;
                    let target = target.apply_to(sprite.transform);
                    if duration > 0.0 {
                        sprite.keyframe_animation = None;
                        sprite.transform_animation = Some(TransformAnimation {
                            from: sprite.transform,
                            to: target,
                            elapsed: 0.0,
                            duration,
                            easing,
                            blocking: !next,
                        });
                    } else {
                        sprite.transform = target;
                        sprite.transform_animation = None;
                        sprite.keyframe_animation = None;
                    }
                }
                if started && !next && duration > 0.0 {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::ShowSprite {
                id,
                image,
                position,
                layout,
                transition,
                transform,
                z_index,
                blend,
            } => {
                let position = *position;
                let transition = *transition;
                let transform = *transform;
                let z_index = *z_index;
                let blend = *blend;
                let id = interpolate(id, &state.vars, &state.global_vars);
                let image = interpolate(image, &state.vars, &state.global_vars);
                debug!("ShowSprite: {} {} at {:?}", id, image, position);
                let transition_offset_x = match (position.x, transition) {
                    (Anchor::Left(_), Transition::SlideFromLeft(_)) => -400.0,
                    (Anchor::Right(_), Transition::SlideFromRight(_)) => 400.0,
                    _ => 0.0,
                };
                let transition_progress = if transition == Transition::Instant {
                    1.0
                } else {
                    0.0
                };
                let rule_animation = state.transition_rules.get(&id).and_then(|rule| {
                    rule.enter.as_ref().map(|preset| PresetAnimation {
                        preset: preset.clone(),
                        base: transform,
                        elapsed: 0.0,
                        duration: rule.duration.max(f32::EPSILON),
                        blocking: !next,
                        remove_on_finish: false,
                    })
                });
                let rule_blocks = rule_animation.is_some() && !next;
                let initial_transform = rule_animation.as_ref().map_or(transform, |animation| {
                    preset_initial_transform(animation.base, &animation.preset)
                });
                state.sprite_sequences.remove(&id);
                state.sprites.insert(
                    id,
                    Sprite {
                        image,
                        position,
                        layout: *layout,
                        transition_progress,
                        transition,
                        entering: true,
                        transition_offset_x,
                        transition_blocking: !next,
                        transform: initial_transform,
                        transform_animation: None,
                        position_animation: None,
                        keyframe_animation: None,
                        filter: Default::default(),
                        films: Default::default(),
                        animation: rule_animation,
                        z_index,
                        blend,
                        camera_distance: None,
                    },
                );
                if rule_blocks || (!next && transition != Transition::Instant) {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::ConfigureSpriteSequence {
                id,
                frames,
                fps,
                looped,
            } => {
                let id = interpolate(id, &state.vars, &state.global_vars);
                let frames = frames
                    .iter()
                    .map(|frame| interpolate(frame, &state.vars, &state.global_vars))
                    .filter(|frame| !frame.is_empty())
                    .collect::<Vec<_>>();
                if let Some(first) = frames.first()
                    && let Some(sprite) = state.sprites.get_mut(&id)
                {
                    sprite.image.clone_from(first);
                    state.sprite_sequences.insert(
                        id,
                        crate::state::SpriteSequenceState {
                            frames,
                            fps: fps.max(f32::EPSILON),
                            looped: *looped,
                            elapsed: 0.0,
                            frame: 0,
                        },
                    );
                }
            }
            Action::SelectSpriteImage {
                id,
                variable,
                default_image,
                variants,
            } => {
                let id = interpolate(id, &state.vars, &state.global_vars);
                let variable = interpolate(variable, &state.vars, &state.global_vars);
                let selected = state
                    .vars
                    .get(&variable)
                    .or_else(|| state.global_vars.get(&variable))
                    .map(crate::Value::display);
                let image = selected
                    .as_deref()
                    .and_then(|selected| {
                        variants
                            .iter()
                            .find(|(name, _)| name == selected)
                            .map(|(_, image)| image)
                    })
                    .unwrap_or(default_image);
                if let Some(sprite) = state.sprites.get_mut(&id) {
                    sprite.image = interpolate(image, &state.vars, &state.global_vars);
                }
            }
            Action::HideSprite { id, transition } => {
                let transition = *transition;
                let id = interpolate(id, &state.vars, &state.global_vars);
                debug!("HideSprite: {}", id);
                let exit_rule = state
                    .transition_rules
                    .get(&id)
                    .and_then(|rule| rule.exit.as_ref().map(|preset| (preset, rule.duration)));
                if let Some((sprite, (preset, duration))) =
                    state.sprites.get_mut(&id).zip(exit_rule)
                {
                    sprite.animation = Some(PresetAnimation {
                        preset: preset.clone(),
                        base: sprite.transform,
                        elapsed: 0.0,
                        duration: duration.max(f32::EPSILON),
                        blocking: !next,
                        remove_on_finish: true,
                    });
                    // The preset owns the entire exit lifecycle. Keep the
                    // sprite fully present until it finishes so the ordinary
                    // transition cleanup cannot remove it on the first frame
                    // (especially when its original transition was Instant).
                    sprite.entering = true;
                    sprite.transition_progress = 1.0;
                    sprite.transition_blocking = false;
                    if !next {
                        return StepResult::AwaitPresentation;
                    }
                } else if transition == Transition::Instant {
                    state.sprites.remove(&id);
                } else if let Some(sprite) = state.sprites.get_mut(&id) {
                    sprite.transition = transition;
                    sprite.entering = false;
                    sprite.transition_blocking = !next;
                    sprite.transition_offset_x = match transition {
                        Transition::SlideFromLeft(_) => -400.0,
                        Transition::SlideFromRight(_) => 400.0,
                        _ => 0.0,
                    };
                    if !next {
                        return StepResult::AwaitPresentation;
                    }
                }
            }
            Action::HideSprites { prefix, transition } => {
                let transition = *transition;
                let prefix = interpolate(prefix, &state.vars, &state.global_vars);
                debug!("HideSprites: {prefix}*");
                if transition == Transition::Instant {
                    state.sprites.retain(|id, _| !id.starts_with(&prefix));
                } else {
                    let mut matched = false;
                    for (id, sprite) in &mut state.sprites {
                        if !id.starts_with(&prefix) {
                            continue;
                        }
                        matched = true;
                        sprite.animation = None;
                        sprite.transition = transition;
                        sprite.entering = false;
                        sprite.transition_blocking = !next;
                        sprite.transition_offset_x = match transition {
                            Transition::SlideFromLeft(_) => -400.0,
                            Transition::SlideFromRight(_) => 400.0,
                            _ => 0.0,
                        };
                    }
                    if matched && !next {
                        return StepResult::AwaitPresentation;
                    }
                }
            }
            Action::Say {
                speaker,
                text,
                options,
            } => {
                let paragraph = std::mem::take(&mut state.next_text_is_paragraph);
                state.active_text_style = if paragraph {
                    state.paragraph_style.clone()
                } else {
                    state.dialogue_style.clone()
                };
                state.active_typewriter_speed = paragraph
                    .then_some(state.paragraph_typewriter_speed)
                    .flatten();
                state.active_text_reveal =
                    paragraph.then_some(state.paragraph_text_reveal).flatten();
                if state.textbox_auto_hidden {
                    state.textbox_hidden = false;
                    state.textbox_auto_hidden = false;
                }
                let mut speaker = resolve_speaker(speaker, state);
                let source = interpolate(text, &state.vars, &state.global_vars)
                    .replace("<br/>", "\n")
                    .replace("<br>", "\n");
                let (mut text, mut markup, mut pauses) = compile_rich_text(&source);
                if options.inherit_speaker
                    && let Some(previous) =
                        state.dialogue.as_ref().or(state.previous_dialogue.as_ref())
                {
                    speaker.clone_from(&previous.speaker);
                }
                if options.concat
                    && let Some(previous) =
                        state.dialogue.as_ref().or(state.previous_dialogue.as_ref())
                {
                    let pause_offset = previous.text.chars().count();
                    text = previous.text.clone() + &text;
                    markup = previous.markup.clone() + &markup;
                    pauses = previous
                        .pauses
                        .iter()
                        .copied()
                        .chain(pauses.into_iter().map(|mut pause| {
                            pause.at += pause_offset;
                            pause
                        }))
                        .collect();
                    if speaker.is_empty() {
                        speaker.clone_from(&previous.speaker);
                    }
                }
                debug!("Say: {}: {}", speaker, text);
                state.dialogue = Some(Dialogue {
                    speaker,
                    text,
                    markup,
                    visible_chars: 0,
                    pauses,
                    vocal: options
                        .vocal
                        .as_deref()
                        .map(|vocal| interpolate(vocal, &state.vars, &state.global_vars)),
                    volume: options.volume.clamp(0.0, 1.0),
                    auto_advance: options.auto_advance,
                });
                state.record_dialogue(state.cursor - 1);
                state.menu = None;
                if !next {
                    return StepResult::AwaitClick;
                }
            }
            Action::RetractDialogue { source, keep } => {
                let requested_source = interpolate(source, &state.vars, &state.global_vars);
                let keep = interpolate(keep, &state.vars, &state.global_vars);
                let mut dialogue = state
                    .dialogue
                    .take()
                    .or_else(|| state.previous_dialogue.take())
                    .unwrap_or_else(|| Dialogue {
                        speaker: String::new(),
                        text: requested_source.clone(),
                        markup: requested_source.clone(),
                        visible_chars: requested_source.chars().count(),
                        pauses: Vec::new(),
                        vocal: None,
                        volume: 1.0,
                        auto_advance: false,
                    });
                let source = if requested_source.is_empty() {
                    dialogue.text.clone()
                } else {
                    requested_source
                };
                if source.is_empty() || !source.starts_with(&keep) {
                    log::error!(
                        "dialogue retraction target is not a source prefix: \
                         source={source:?}, keep={keep:?}"
                    );
                    state.dialogue = Some(dialogue);
                    continue;
                }
                if dialogue.text != source {
                    dialogue.markup.clone_from(&source);
                }
                dialogue.text = source;
                dialogue.visible_chars = dialogue.text.chars().count();
                dialogue.pauses.clear();
                dialogue.vocal = None;
                dialogue.auto_advance = false;
                state.previous_dialogue = None;
                state.dialogue = Some(dialogue);
                let target_visible_chars = keep.chars().count();
                state.dialogue_retraction = Some(DialogueRetraction {
                    keep,
                    target_visible_chars,
                    fractional_chars: 0.0,
                    awaiting_advance: false,
                });
                if next {
                    finish_dialogue_retraction(state);
                    state.dialogue_retraction = None;
                } else {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::Menu { prompt, choices } => {
                let choices = choices
                    .iter()
                    .filter(|choice| condition_matches(choice.show_when.as_deref(), state))
                    .map(|choice| MenuChoice {
                        text: interpolate(&choice.text, &state.vars, &state.global_vars),
                        target: interpolate_choice_target(&choice.target, state),
                        enabled: condition_matches(choice.enable_when.as_deref(), state),
                    })
                    .collect::<Vec<_>>();
                if choices.is_empty() {
                    log::warn!("choice menu has no visible options; continuing");
                    continue;
                }
                debug!("Menu: {} visible choices", choices.len());
                state.menu = Some(MenuState {
                    prompt: interpolate(prompt, &state.vars, &state.global_vars),
                    choices,
                });
                state.dialogue = None;
                return StepResult::AwaitChoice;
            }
            Action::Jump(label) => {
                let label = interpolate(label, &state.vars, &state.global_vars);
                if let Some(idx) = program.label(&state.current_scene, &label) {
                    debug!("Jump: {} -> {}", label, idx);
                    state.cursor = idx;
                } else {
                    debug!("Jump: {} (label not found, skipping)", label);
                }
            }
            Action::Label(name) => {
                // Labels are pre-indexed; no-op during execution.
                debug!("Label: {}", name);
            }
            Action::ChangeScene(scene) => {
                let scene = interpolate(scene, &state.vars, &state.global_vars);
                debug!("ChangeScene: {}", scene);
                enter_scene(state, &scene);
            }
            Action::CallScene(scene) => {
                let scene = interpolate(scene, &state.vars, &state.global_vars);
                debug!("CallScene: {}", scene);
                if program.contains_scene(&scene) {
                    state.scene_stack.push(SceneFrame {
                        scene: state.current_scene.clone(),
                        cursor: state.cursor,
                    });
                    enter_scene(state, &scene);
                } else {
                    log::warn!("CallScene target does not exist: {scene}");
                }
            }
            Action::End => {
                debug!("End");
                if cleanup_on_end {
                    end_game(state);
                }
                return StepResult::EndOfScene;
            }
            Action::Bgm {
                file,
                volume,
                fade_seconds,
            } => {
                let file = interpolate(file, &state.vars, &state.global_vars);
                state.bgm.file = (file != "none" && !file.is_empty()).then_some(file);
                state.bgm.volume = volume.clamp(0.0, 1.0);
                state.bgm.fade_seconds = fade_seconds.max(0.0);
                state.bgm.revision = state.bgm.revision.wrapping_add(1);
            }
            Action::Effect { file, volume, id } => {
                let id = id
                    .as_deref()
                    .map(|id| interpolate(id, &state.vars, &state.global_vars));
                let file = file
                    .as_deref()
                    .map(|file| interpolate(file, &state.vars, &state.global_vars));
                match (id, file) {
                    (Some(id), Some(file)) => {
                        state.looping_effects.insert(
                            id,
                            crate::state::EffectState {
                                file,
                                volume: volume.clamp(0.0, 1.0),
                            },
                        );
                    }
                    (Some(id), None) => {
                        state.looping_effects.remove(&id);
                    }
                    (None, Some(file)) => {
                        state.effect_queue.push(crate::state::EffectEvent::Play(
                            crate::state::EffectCue {
                                id: None,
                                file,
                                volume: volume.clamp(0.0, 1.0),
                                fade_in: 0.0,
                            },
                        ));
                    }
                    (None, None) => state.effect_queue.push(crate::state::EffectEvent::Stop),
                }
            }
            Action::Vocal { file, volume } => {
                state.vocal_event = Some(crate::state::VocalCue {
                    file: file
                        .as_deref()
                        .map(|file| interpolate(file, &state.vars, &state.global_vars)),
                    volume: volume.clamp(0.0, 1.0),
                    fade_in: 0.0,
                    fade_out: 0.0,
                });
            }
            Action::Set {
                name,
                expression,
                global,
            } => {
                let name = interpolate(name, &state.vars, &state.global_vars);
                match evaluate(expression, &state.vars, &state.global_vars) {
                    Ok(value) => {
                        debug!("Set: {} = {:?}", name, value);
                        if *global {
                            assign_value(&mut state.global_vars, &name, value);
                        } else {
                            assign_value(&mut state.vars, &name, value);
                        }
                    }
                    Err(error) => log::error!("failed to evaluate {name} = {expression}: {error}"),
                }
            }
            Action::MiniAvatar { image } => {
                debug!("MiniAvatar: {}", image);
                state.mini_avatar = Some(image.clone());
                state.mini_avatar_progress = 0.0;
            }
            Action::HideMiniAvatar => {
                debug!("HideMiniAvatar");
                state.mini_avatar = None;
                state.mini_avatar_progress = 0.0;
            }
            Action::Animate {
                target,
                preset,
                duration,
            } => {
                let duration = *duration;
                let target = interpolate(target, &state.vars, &state.global_vars);
                if matches!(
                    preset,
                    crate::types::AnimationPreset::OldFilm
                        | crate::types::AnimationPreset::DotFilm
                        | crate::types::AnimationPreset::ReflectionFilm
                        | crate::types::AnimationPreset::GlitchFilm
                        | crate::types::AnimationPreset::RgbFilm
                        | crate::types::AnimationPreset::GodrayFilm
                        | crate::types::AnimationPreset::RemoveFilm
                ) {
                    let mut applied = false;
                    if is_background_target(&target) {
                        applied = state.bg_films.apply(preset);
                        state.bg_animation = None;
                    } else if is_character_group_target(&target) {
                        for sprite in state.sprites.values_mut() {
                            applied |= sprite.films.apply(preset);
                            sprite.animation = None;
                        }
                    } else if let Some(sprite) = state.sprites.get_mut(&target) {
                        applied = sprite.films.apply(preset);
                        sprite.animation = None;
                    }
                    if !applied {
                        log::warn!("film animation target does not exist: {target}");
                    }
                    continue;
                }
                let animation = |base| PresetAnimation {
                    preset: preset.clone(),
                    base,
                    elapsed: 0.0,
                    duration: duration.max(f32::EPSILON),
                    blocking: !next,
                    remove_on_finish: matches!(preset, crate::types::AnimationPreset::Exit),
                };
                let mut started = false;
                if is_background_target(&target) {
                    started = true;
                    let animation = animation(state.bg_transform);
                    state.bg_transform =
                        preset_initial_transform(animation.base, &animation.preset);
                    state.bg_animation = Some(animation);
                } else if is_character_group_target(&target) {
                    for sprite in state.sprites.values_mut() {
                        started = true;
                        let animation = animation(sprite.transform);
                        sprite.transform =
                            preset_initial_transform(animation.base, &animation.preset);
                        sprite.animation = Some(animation);
                    }
                } else if let Some(sprite) = state.sprites.get_mut(&target) {
                    started = true;
                    let animation = animation(sprite.transform);
                    sprite.transform = preset_initial_transform(animation.base, &animation.preset);
                    sprite.animation = Some(animation);
                } else {
                    log::warn!("animation target does not exist: {target}");
                }
                if started && !next && duration > 0.0 {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::SetTransition {
                target,
                enter,
                exit,
                duration,
            } => {
                let target = interpolate(target, &state.vars, &state.global_vars);
                state.transition_rules.insert(
                    target,
                    TransitionRule {
                        enter: enter.clone(),
                        exit: exit.clone(),
                        duration: duration.max(f32::EPSILON),
                    },
                );
            }
            Action::SetFilter { target, filter } => {
                let target = interpolate(target, &state.vars, &state.global_vars);
                if is_background_target(&target) {
                    state.bg_filter = *filter;
                } else if is_character_group_target(&target) {
                    for sprite in state.sprites.values_mut() {
                        sprite.filter = *filter;
                    }
                } else if let Some(sprite) = state.sprites.get_mut(&target) {
                    sprite.filter = *filter;
                } else {
                    log::warn!("filter target does not exist: {target}");
                }
            }
            Action::Wait { seconds } => {
                state.wait_remaining = seconds.max(0.0);
                state.wait_blocking = !next;
                if !next && state.wait_remaining > 0.0 {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::WaitForAdvance => {
                if !next {
                    state.waiting_for_advance = true;
                    return StepResult::AwaitPresentation;
                }
            }
            Action::Intro { pages, hold } => {
                let pages = pages
                    .iter()
                    .map(|page| interpolate(page, &state.vars, &state.global_vars))
                    .collect();
                state.dialogue = None;
                state.intro = Some(IntroState {
                    pages,
                    page: 0,
                    elapsed: 0.0,
                    hold: *hold,
                    blocking: !next,
                });
                if !next {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::FilmMode { enabled } => state.film_mode = *enabled,
            Action::Curtain {
                visible,
                color,
                duration,
            } => {
                let target = if *visible { 1.0 } else { 0.0 };
                state.curtain.color = *color;
                state.curtain.from = state.curtain.current;
                state.curtain.target = target;
                state.curtain.elapsed = 0.0;
                state.curtain.duration = duration.max(0.0);
                state.curtain.blocking = !next && *duration > 0.0;
                if *duration <= f32::EPSILON {
                    state.curtain.current = target;
                } else if !next {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::FloatingText {
                text,
                position,
                font_size,
                color,
                fade_in,
                hold,
                fade_out,
                blocking,
            } => {
                let state_blocking = *blocking && !next;
                state.floating_text = Some(crate::state::FloatingTextState {
                    id: None,
                    text: interpolate(text, &state.vars, &state.global_vars),
                    position: *position,
                    font_size: *font_size,
                    color: *color,
                    fade_in: fade_in.max(0.0),
                    hold: hold.max(0.0),
                    fade_out: fade_out.max(0.0),
                    elapsed: 0.0,
                    blocking: state_blocking,
                    infinite: false,
                });
                if state_blocking {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::ConfigureFloatingText { id, infinite } => {
                if let Some(active) = &mut state.floating_text {
                    active.id.clone_from(id);
                    active.infinite = *infinite;
                }
            }
            Action::UpdateSprite {
                id,
                image,
                position,
                layout,
                scale,
                duration,
                easing,
                blocking,
            } => {
                let id = interpolate(id, &state.vars, &state.global_vars);
                let image = interpolate(image, &state.vars, &state.global_vars);
                let duration = duration.max(0.0);
                let state_blocking = *blocking && !next;
                let Some(sprite) = state.sprites.get_mut(&id) else {
                    continue;
                };
                let from_position = sprite.position;
                let mut target_transform = sprite.transform;
                target_transform.scale_x = scale.max(f32::EPSILON);
                target_transform.scale_y = scale.max(f32::EPSILON);
                sprite.image = image;
                sprite.layout = *layout;
                sprite.position = *position;
                sprite.keyframe_animation = None;
                sprite.animation = None;
                state.sprite_sequences.remove(&id);
                if duration > f32::EPSILON {
                    sprite.transform_animation = Some(TransformAnimation {
                        from: sprite.transform,
                        to: target_transform,
                        elapsed: 0.0,
                        duration,
                        easing: *easing,
                        blocking: state_blocking,
                    });
                    sprite.position_animation = Some(crate::state::PositionAnimation {
                        from: from_position,
                        to: *position,
                        elapsed: 0.0,
                        duration,
                        easing: *easing,
                        blocking: state_blocking,
                    });
                    if state_blocking {
                        return StepResult::AwaitPresentation;
                    }
                } else {
                    sprite.transform = target_transform;
                    sprite.transform_animation = None;
                    sprite.position_animation = None;
                }
            }
            Action::HideFloatingText { id } => {
                let matches = state.floating_text.as_ref().is_some_and(|active| {
                    id.as_ref().is_none_or(|id| active.id.as_ref() == Some(id))
                });
                if matches && let Some(active) = &mut state.floating_text {
                    if active.fade_out > f32::EPSILON {
                        active.infinite = false;
                        active.elapsed = active.fade_in + active.hold;
                    } else {
                        state.floating_text = None;
                    }
                }
            }
            Action::ConfigurePortraits {
                enabled,
                character_ids,
                speaking,
                others,
                narration,
                duration,
                easing,
            } => {
                state.portrait_rule = Some(crate::state::PortraitRuleState {
                    enabled: *enabled,
                    character_ids: character_ids.iter().cloned().collect(),
                    speaking: *speaking,
                    others: *others,
                    narration: *narration,
                    duration: duration.max(0.0),
                    easing: *easing,
                });
            }
            Action::FocusPortrait { speaker_id } => {
                let Some(rule) = state.portrait_rule.clone().filter(|rule| rule.enabled) else {
                    continue;
                };
                let speaker_id = speaker_id
                    .as_deref()
                    .map(|id| interpolate(id, &state.vars, &state.global_vars));
                for (id, sprite) in &mut state.sprites {
                    if !rule.character_ids.contains(id) {
                        continue;
                    }
                    let style = match speaker_id.as_deref() {
                        None => rule.narration,
                        Some(speaker) if speaker == id => rule.speaking,
                        Some(_) => rule.others,
                    };
                    sprite.filter = crate::VisualFilter {
                        blur: style.blur,
                        brightness: style.brightness,
                        contrast: style.contrast,
                        saturation: style.saturation,
                    };
                    let mut target = sprite.transform;
                    target.scale_x = style.scale;
                    target.scale_y = style.scale;
                    target.alpha = style.alpha;
                    if rule.duration > f32::EPSILON {
                        sprite.transform_animation = Some(TransformAnimation {
                            from: sprite.transform,
                            to: target,
                            elapsed: 0.0,
                            duration: rule.duration,
                            easing: rule.easing,
                            blocking: false,
                        });
                    } else {
                        sprite.transform = target;
                        sprite.transform_animation = None;
                    }
                }
            }
            Action::SetDialogueStyle { style } => {
                state.dialogue_style.clone_from(style);
            }
            Action::SetParagraphStyle {
                style,
                typewriter_speed,
                text_reveal,
            } => {
                state.paragraph_style.clone_from(style);
                state.paragraph_typewriter_speed = *typewriter_speed;
                state.paragraph_text_reveal = *text_reveal;
            }
            Action::SelectTextPresentation { paragraph } => {
                state.next_text_is_paragraph = *paragraph;
            }
            Action::AnimateKeyframes {
                target,
                frames,
                repeat,
                blocking,
            } => {
                let target = interpolate(target, &state.vars, &state.global_vars);
                let state_blocking = *blocking && !next;
                let build = |initial| {
                    let mut from = initial;
                    let frames = frames
                        .iter()
                        .map(|frame| {
                            let to = frame.transform.apply_to(from);
                            let animation = TransformAnimation {
                                from,
                                to,
                                elapsed: 0.0,
                                duration: frame.duration.max(0.0),
                                easing: frame.easing,
                                blocking: false,
                            };
                            from = to;
                            animation
                        })
                        .collect();
                    KeyframeAnimation {
                        initial,
                        frames,
                        index: 0,
                        repeat_remaining: *repeat,
                        blocking: state_blocking,
                    }
                };
                let mut started = false;
                if is_background_target(&target) {
                    state.bg_transform_animation = None;
                    state.bg_keyframe_animation = Some(build(state.bg_transform));
                    started = true;
                } else if let Some(sprite) = state.sprites.get_mut(&target) {
                    sprite.transform_animation = None;
                    sprite.keyframe_animation = Some(build(sprite.transform));
                    started = true;
                }
                if started && state_blocking && !frames.is_empty() {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::ShowParticles { id, effect } => {
                let id = interpolate(id, &state.vars, &state.global_vars);
                let mut effect = effect.clone();
                effect.preset = interpolate(&effect.preset, &state.vars, &state.global_vars);
                effect.texture = effect
                    .texture
                    .as_deref()
                    .map(|texture| interpolate(texture, &state.vars, &state.global_vars));
                state
                    .particle_effects
                    .insert(id, crate::state::ActiveParticleEffect::new(effect));
            }
            Action::HideParticles { id, duration } => {
                let duration = duration.max(0.0);
                let selected = id
                    .as_deref()
                    .map(|id| interpolate(id, &state.vars, &state.global_vars));
                if duration <= f32::EPSILON {
                    if let Some(id) = selected {
                        state.particle_effects.remove(&id);
                    } else {
                        state.particle_effects.clear();
                    }
                } else {
                    for (id, effect) in &mut state.particle_effects {
                        if selected.as_ref().is_none_or(|selected| selected == id) {
                            effect.begin_fade_out(duration);
                        }
                    }
                }
            }
            Action::SetTextbox { visible, auto } => {
                state.textbox_hidden = !*visible;
                state.textbox_auto_hidden = !*visible && *auto;
            }
            Action::UserInput {
                variable,
                title,
                button,
            } => {
                state.user_input = Some(crate::state::UserInputState {
                    variable: interpolate(variable, &state.vars, &state.global_vars),
                    title: interpolate(title, &state.vars, &state.global_vars),
                    button: interpolate(button, &state.vars, &state.global_vars),
                    value: String::new(),
                    value_type: crate::types::InputValueType::String,
                    description: String::new(),
                    placeholder: String::new(),
                    required_text: "请填写后再继续".into(),
                    required: true,
                    min_length: 0,
                    max_length: 64,
                    min_value: None,
                    max_value: None,
                    step: 1.0,
                    true_text: "是".into(),
                    false_text: "否".into(),
                    error: String::new(),
                });
                return StepResult::AwaitInput;
            }
            Action::Comment => {}
            Action::Unlock { kind, file, name } => {
                let file = interpolate(file, &state.vars, &state.global_vars);
                let name = interpolate(name, &state.vars, &state.global_vars);
                match kind {
                    crate::types::UnlockKind::Cg => {
                        state.unlocked_cg.insert(file, name);
                    }
                    crate::types::UnlockKind::Bgm => {
                        state.unlocked_bgm.insert(file, name);
                    }
                }
            }
            Action::SetAutoplay { enabled } => state
                .shell_events
                .push(crate::state::ShellEvent::SetAutoplay(*enabled)),
            Action::SetSystemUi { slot, visible } => {
                state
                    .shell_events
                    .push(crate::state::ShellEvent::SetSystemUi {
                        slot: *slot,
                        visible: *visible,
                    });
            }
            Action::PlayVideo { video } => {
                let mut video = video.clone();
                video.id = interpolate(&video.id, &state.vars, &state.global_vars);
                video.file = interpolate(&video.file, &state.vars, &state.global_vars);
                // `-next` makes this playback genuinely non-blocking for its
                // complete lifetime. Keeping the authored blocking flag in
                // state would make a later `wait` stall behind the video and
                // prevent the following StopVideo action from ever running.
                if next {
                    video.wait_for_finished = false;
                }
                let blocking = video.wait_for_finished && !video.looped;
                state.video_revision_counter = state.video_revision_counter.wrapping_add(1);
                state.videos.insert(
                    video.id.clone(),
                    crate::state::VideoState {
                        opacity: video.alpha.clamp(0.0, 1.0),
                        spec: video,
                        revision: state.video_revision_counter,
                        elapsed: 0.0,
                        stopping: false,
                        fade_out: 0.0,
                    },
                );
                if blocking {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::StopVideo { id, fade_out } => {
                let id = id
                    .as_ref()
                    .map(|id| interpolate(id, &state.vars, &state.global_vars));
                if *fade_out <= f32::EPSILON {
                    state
                        .videos
                        .retain(|video_id, _| id.as_ref().is_some_and(|id| id != video_id));
                } else {
                    for (video_id, video) in &mut state.videos {
                        if id.as_ref().is_none_or(|id| id == video_id) {
                            video.stopping = true;
                            video.fade_out = fade_out.max(0.0);
                        }
                    }
                }
            }
            Action::SetPostProcess {
                targets,
                effect,
                duration,
                easing,
                blocking,
            } => {
                state.camera_effect_targets = *targets;
                let blocking = *blocking && !next;
                let target_effect = effect.apply_to(state.camera_effect.clone());
                if *duration <= f32::EPSILON {
                    state.camera_effect = target_effect;
                    state.camera_effect_animation = None;
                } else {
                    state.camera_effect_animation = Some(crate::state::PostProcessAnimation {
                        from: state.camera_effect.clone(),
                        to: target_effect,
                        elapsed: 0.0,
                        duration: *duration,
                        easing: *easing,
                        blocking,
                    });
                }
                if blocking && *duration > f32::EPSILON {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::SetCameraBinding {
                target,
                bound,
                distance,
            } => {
                let target = interpolate(target, &state.vars, &state.global_vars);
                let distance = distance.max(f32::EPSILON);
                if is_background_target(&target) {
                    state.bg_camera_distance = bound.then_some(distance);
                } else if is_character_group_target(&target) {
                    for (id, sprite) in &mut state.sprites {
                        if !id.starts_with("scene-layer:") {
                            sprite.camera_distance = bound.then_some(distance);
                        }
                    }
                } else if let Some(sprite) = state.sprites.get_mut(&target) {
                    sprite.camera_distance = bound.then_some(distance);
                }
            }
            Action::SetCameraTransform {
                targets,
                transform,
                duration,
                easing,
                blocking,
            } => {
                state.camera_targets = *targets;
                let target = transform.apply_to(state.camera_transform);
                let blocking = *blocking && !next;
                if *duration <= f32::EPSILON {
                    state.camera_transform = target;
                    state.camera_transform_animation = None;
                } else {
                    state.camera_transform_animation = Some(crate::state::TransformAnimation {
                        from: state.camera_transform,
                        to: target,
                        elapsed: 0.0,
                        duration: *duration,
                        easing: *easing,
                        blocking,
                    });
                }
                if blocking && *duration > f32::EPSILON {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::ShakeCamera {
                targets,
                shake,
                blocking,
            } => {
                state.camera_targets = *targets;
                let blocking = *blocking && !next;
                if shake.amplitude > f32::EPSILON
                    && shake.frequency > f32::EPSILON
                    && shake.duration > f32::EPSILON
                {
                    state.camera_shake = Some(crate::state::CameraShakeState {
                        spec: *shake,
                        elapsed: 0.0,
                        offset_x: 0.0,
                        offset_y: 0.0,
                        blocking,
                    });
                    if blocking {
                        return StepResult::AwaitPresentation;
                    }
                } else {
                    state.camera_shake = None;
                }
            }
            Action::HostCommand {
                namespace,
                command,
                payload,
            } => state.host_commands.push(crate::state::HostCommandEvent {
                namespace: namespace.clone(),
                command: command.clone(),
                payload: interpolate(payload, &state.vars, &state.global_vars),
            }),
            Action::RequestInput { spec } => {
                state.user_input = Some(crate::state::UserInputState {
                    variable: interpolate(&spec.variable, &state.vars, &state.global_vars),
                    title: interpolate(&spec.title, &state.vars, &state.global_vars),
                    button: interpolate(&spec.confirm_text, &state.vars, &state.global_vars),
                    value: if spec.value_type == crate::types::InputValueType::Bool {
                        "false".into()
                    } else {
                        String::new()
                    },
                    value_type: spec.value_type,
                    description: interpolate(&spec.description, &state.vars, &state.global_vars),
                    placeholder: interpolate(&spec.placeholder, &state.vars, &state.global_vars),
                    required_text: interpolate(
                        &spec.required_text,
                        &state.vars,
                        &state.global_vars,
                    ),
                    required: spec.required,
                    min_length: spec.min_length,
                    max_length: spec.max_length,
                    min_value: spec.min_value,
                    max_value: spec.max_value,
                    step: spec.step.max(f64::EPSILON),
                    true_text: interpolate(&spec.true_text, &state.vars, &state.global_vars),
                    false_text: interpolate(&spec.false_text, &state.vars, &state.global_vars),
                    error: String::new(),
                });
                return StepResult::AwaitInput;
            }
            Action::SystemMessage { spec } => {
                state.system_message = Some(crate::state::SystemMessageState {
                    mode: spec.mode,
                    title: interpolate(&spec.title, &state.vars, &state.global_vars),
                    message: interpolate(&spec.message, &state.vars, &state.global_vars),
                    confirm_text: interpolate(&spec.confirm_text, &state.vars, &state.global_vars),
                    cancel_text: interpolate(&spec.cancel_text, &state.vars, &state.global_vars),
                    result_variable: spec
                        .result_variable
                        .as_deref()
                        .map(|value| interpolate(value, &state.vars, &state.global_vars))
                        .filter(|value| !value.is_empty()),
                });
                return StepResult::AwaitPresentation;
            }
            Action::StageAnimation { animation } => {
                // A timeline may reference a character expression before a
                // separate show-character block. Prepare only missing targets;
                // visible characters retain their authored position and base.
                for track in &animation.tracks {
                    let crate::action::StageTarget::Character {
                        id,
                        image: Some(image),
                    } = &track.target
                    else {
                        continue;
                    };
                    if image.is_empty() || state.sprites.contains_key(id) {
                        continue;
                    }
                    state.sprites.insert(
                        id.clone(),
                        Sprite {
                            image: interpolate(image, &state.vars, &state.global_vars),
                            position: crate::types::Position::center(0.0),
                            layout: crate::types::SpriteLayout::Natural,
                            transition_progress: 1.0,
                            transition: Transition::Instant,
                            entering: true,
                            transition_offset_x: 0.0,
                            transition_blocking: false,
                            transform: Default::default(),
                            transform_animation: None,
                            position_animation: None,
                            keyframe_animation: None,
                            filter: Default::default(),
                            films: Default::default(),
                            animation: None,
                            z_index: 100,
                            blend: crate::types::BlendMode::Alpha,
                            camera_distance: Some(1.0),
                        },
                    );
                }
                let mut animation = animation.clone();
                animation.playback_rate = animation.playback_rate.max(f32::EPSILON);
                animation.duration = animation.duration.max(0.0);
                let blocking = animation.blocking && !next && !animation.infinite;
                animation.blocking = blocking;
                state.stage_animation = (animation.duration > f32::EPSILON)
                    .then(|| crate::state::StageAnimationState::new(animation, state));
                if blocking && state.stage_animation.is_some() {
                    return StepResult::AwaitPresentation;
                }
            }
            Action::Flow { .. } => unreachable!("flow wrappers are removed before dispatch"),
        }
    }

    log::error!("script exceeded {MAX_FORWARD_ACTIONS} actions without yielding");
    StepResult::ExecutionLimit
}

fn is_background_target(target: &str) -> bool {
    matches!(target, "bg-main" | "background")
}

fn is_character_group_target(target: &str) -> bool {
    matches!(target, "characters" | "character-group")
}

fn preset_initial_transform(
    base: crate::types::SpriteTransform,
    preset: &crate::types::AnimationPreset,
) -> crate::types::SpriteTransform {
    use crate::types::AnimationPreset;
    let mut initial = base;
    match preset {
        AnimationPreset::Enter => initial.alpha = 0.0,
        AnimationPreset::EnterFromBottom => {
            initial.offset_y += 220.0;
            initial.blur += 5.0;
            initial.alpha = 0.0;
        }
        AnimationPreset::EnterFromLeft => {
            initial.offset_x -= 280.0;
            initial.blur += 5.0;
            initial.alpha = 0.0;
        }
        AnimationPreset::EnterFromRight => {
            initial.offset_x += 280.0;
            initial.blur += 5.0;
            initial.alpha = 0.0;
        }
        _ => {}
    }
    initial
}

fn assign_value(
    variables: &mut std::collections::HashMap<String, crate::Value>,
    target: &str,
    value: crate::Value,
) {
    let Some((name, index)) = target
        .strip_suffix(']')
        .and_then(|target| target.rsplit_once('['))
    else {
        variables.insert(target.to_owned(), value);
        return;
    };
    let Ok(index) = index.parse::<usize>() else {
        log::error!("invalid array assignment target {target:?}");
        return;
    };
    let Some(crate::Value::Array(values)) = variables.get_mut(name) else {
        log::error!("array assignment target {name:?} does not exist");
        return;
    };
    if let Some(slot) = values.get_mut(index) {
        *slot = value;
    } else {
        log::error!("array assignment index {index} is out of bounds for {name:?}");
    }
}

fn enter_scene(state: &mut State, scene: &str) -> bool {
    if !state.program.contains_scene(scene) {
        log::warn!("scene target does not exist: {scene}");
        return false;
    }
    state.current_scene = scene.to_owned();
    state.cursor = 0;
    state.dialogue = None;
    state.dialogue_retraction = None;
    state.menu = None;
    state.stage_animation = None;
    true
}

/// Handle user clicking to advance past a dialogue.
pub fn advance(state: &mut State) {
    state.dialogue_retraction = None;
    state.mark_current_dialogue_read();
    state.previous_dialogue = state.dialogue.take();
}

/// Advance the native sentence-tail deletion presentation.
///
/// Returns whether visible or blocking state changed. A click is accepted only
/// after deletion had already completed on a previous tick, matching editor
/// runtimes that install the advance handler after the animation finishes.
pub fn update_dialogue_retraction(
    state: &mut State,
    delta_seconds: f64,
    chars_per_second: f64,
    advance: bool,
    immediate: bool,
) -> bool {
    let Some(retraction) = state.dialogue_retraction.as_mut() else {
        return false;
    };
    if state.dialogue.is_none() {
        state.dialogue_retraction = None;
        return true;
    }
    if immediate {
        finish_dialogue_retraction(state);
        state.dialogue_retraction = None;
        return true;
    }
    if retraction.awaiting_advance {
        if advance {
            state.dialogue_retraction = None;
            return true;
        }
        return false;
    }

    let speed = chars_per_second.max(0.0);
    retraction.fractional_chars += delta_seconds.max(0.0) * speed;
    let remove_graphemes = retraction.fractional_chars.floor() as usize;
    if remove_graphemes == 0 {
        return false;
    }
    retraction.fractional_chars -= remove_graphemes as f64;

    let target = retraction.target_visible_chars;
    let dialogue = state.dialogue.as_mut().expect("checked above");
    let previous = dialogue.visible_chars;
    for _ in 0..remove_graphemes {
        if dialogue.visible_chars <= target {
            break;
        }
        let visible_end = dialogue
            .text
            .char_indices()
            .nth(dialogue.visible_chars)
            .map_or(dialogue.text.len(), |(index, _)| index);
        let grapheme_chars = dialogue.text[..visible_end]
            .graphemes(true)
            .next_back()
            .map_or(1, |grapheme| grapheme.chars().count());
        dialogue.visible_chars = dialogue
            .visible_chars
            .saturating_sub(grapheme_chars)
            .max(target);
    }
    if dialogue.visible_chars == target {
        dialogue.text.clone_from(&retraction.keep);
        dialogue.markup.clone_from(&retraction.keep);
        dialogue.pauses.clear();
        retraction.fractional_chars = 0.0;
        retraction.awaiting_advance = true;
    }
    dialogue.visible_chars != previous || retraction.awaiting_advance
}

fn finish_dialogue_retraction(state: &mut State) {
    let Some(retraction) = state.dialogue_retraction.as_ref() else {
        return;
    };
    if let Some(dialogue) = &mut state.dialogue {
        dialogue.text.clone_from(&retraction.keep);
        dialogue.markup.clone_from(&retraction.keep);
        dialogue.visible_chars = retraction.target_visible_chars;
        dialogue.pauses.clear();
    }
}

pub fn end_game(state: &mut State) {
    state.invalidate_stage();
    state.shell_events.clear();
    state.host_commands.clear();
    state.scene_stack.clear();
    state.dialogue = None;
    state.previous_dialogue = None;
    state.dialogue_retraction = None;
    state.menu = None;
    state.bg = None;
    state.bg_transition = None;
    state.bg_animation = None;
    state.bg_keyframe_animation = None;
    state.bg_filter = Default::default();
    state.bg_films = Default::default();
    state.bg_camera_distance = None;
    state.camera_effect = Default::default();
    state.camera_transform = Default::default();
    state.camera_targets = crate::types::CameraTargets::NONE;
    state.camera_transform_animation = None;
    state.camera_shake = None;
    state.camera_effect_targets = crate::types::CameraTargets::NONE;
    state.camera_effect_animation = None;
    state.stage_animation = None;
    state.videos.clear();
    state.video_revision_counter = 0;
    state.sprites.clear();
    state.sprite_sequences.clear();
    state.mini_avatar = None;
    state.textbox_hidden = false;
    state.textbox_auto_hidden = false;
    state.user_input = None;
    state.system_message = None;
    state.wait_remaining = 0.0;
    state.wait_blocking = false;
    state.waiting_for_advance = false;
    state.intro = None;
    state.film_mode = false;
    state.curtain = Default::default();
    state.floating_text = None;
    state.portrait_rule = None;
    state.dialogue_style = Default::default();
    state.paragraph_style = Default::default();
    state.active_text_style = Default::default();
    state.paragraph_typewriter_speed = None;
    state.paragraph_text_reveal = None;
    state.active_typewriter_speed = None;
    state.active_text_reveal = None;
    state.next_text_is_paragraph = false;
    state.particle_effects.clear();
    state.transition_rules.clear();
    state.bgm.file = None;
    state.bgm.fade_seconds = 0.0;
    state.bgm.revision = state.bgm.revision.wrapping_add(1);
    state.looping_effects.clear();
    state.effect_queue.clear();
    state.vocal_event = Some(crate::state::VocalCue {
        file: None,
        volume: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
    });
    state.vars.clear();
    state.cursor = state.program.scene_len(&state.current_scene).unwrap_or(0);
    state.ended = true;
}

pub fn submit_user_input(state: &mut State) -> bool {
    let Some(input) = state.user_input.as_mut() else {
        return false;
    };
    let trimmed = input.value.trim();
    input.error.clear();
    if input.required && trimmed.is_empty() {
        input.error.clone_from(&input.required_text);
        return false;
    }
    let value = match input.value_type {
        crate::types::InputValueType::String => {
            let length = input.value.chars().count();
            if length < input.min_length {
                input.error = format!("至少输入 {} 个字符", input.min_length);
                return false;
            }
            if input.max_length > 0 && length > input.max_length {
                input.error = format!("最多输入 {} 个字符", input.max_length);
                return false;
            }
            crate::Value::Str(input.value.clone())
        }
        crate::types::InputValueType::Number => {
            let Ok(number) = trimmed.parse::<f64>() else {
                input.error = "请输入有效数字".into();
                return false;
            };
            if input.min_value.is_some_and(|minimum| number < minimum)
                || input.max_value.is_some_and(|maximum| number > maximum)
            {
                input.error = "数值超出允许范围".into();
                return false;
            }
            if let Ok(integer) = trimmed.parse::<i64>() {
                crate::Value::Int(integer)
            } else {
                let Some(value) = crate::Value::from_finite_f64(number) else {
                    input.error = "请输入有效数字".into();
                    return false;
                };
                value
            }
        }
        crate::types::InputValueType::Bool => crate::Value::Bool(trimmed == "true"),
    };
    let variable = input.variable.clone();
    state.user_input = None;
    state.vars.insert(variable, value);
    true
}

/// Resolve the active script-owned alert or confirmation.
pub fn resolve_system_message(state: &mut State, confirmed: bool) -> bool {
    let Some(message) = state.system_message.take() else {
        return false;
    };
    if let Some(variable) = message.result_variable {
        state.vars.insert(variable, crate::Value::Bool(confirmed));
    }
    true
}

/// Handle user selecting a menu choice.
pub fn select_choice(state: &mut State, index: usize) {
    let Some(target) = state
        .menu
        .as_ref()
        .and_then(|menu| menu.choices.get(index))
        .filter(|choice| choice.enabled)
        .map(|choice| choice.target.clone())
    else {
        return;
    };

    state.menu = None;
    match target {
        ChoiceTarget::Label(label) => {
            if let Some(cursor) = state.program.label(&state.current_scene, &label) {
                state.cursor = cursor;
            }
        }
        ChoiceTarget::ChangeScene(scene) => {
            enter_scene(state, &scene);
        }
        ChoiceTarget::CallScene(scene) => {
            if state.program.contains_scene(&scene) {
                state.scene_stack.push(SceneFrame {
                    scene: state.current_scene.clone(),
                    cursor: state.cursor,
                });
                enter_scene(state, &scene);
            }
        }
    }
}

fn condition_matches(condition: Option<&str>, state: &State) -> bool {
    let Some(condition) = condition else {
        return true;
    };
    match evaluate(condition, &state.vars, &state.global_vars) {
        Ok(value) => value.truthy(),
        Err(error) => {
            log::error!("invalid choice condition {condition:?}: {error}");
            false
        }
    }
}

fn interpolate_choice_target(target: &ChoiceTarget, state: &State) -> ChoiceTarget {
    let interpolate_target = |value: &str| interpolate(value, &state.vars, &state.global_vars);
    match target {
        ChoiceTarget::Label(value) => ChoiceTarget::Label(interpolate_target(value)),
        ChoiceTarget::ChangeScene(value) => ChoiceTarget::ChangeScene(interpolate_target(value)),
        ChoiceTarget::CallScene(value) => ChoiceTarget::CallScene(interpolate_target(value)),
    }
}

fn resolve_speaker(source: &str, state: &State) -> String {
    let speaker = interpolate(source, &state.vars, &state.global_vars);
    state
        .vars
        .get(&speaker)
        .or_else(|| state.global_vars.get(&speaker))
        .map_or(speaker, crate::Value::display)
}

fn compile_rich_text(source: &str) -> (String, String, Vec<crate::state::DialoguePause>) {
    let chars = source.chars().collect::<Vec<_>>();
    let mut text = String::new();
    let mut markup = String::new();
    let mut pauses = Vec::new();
    let mut cursor = 0;
    while cursor < chars.len() {
        if chars[cursor] != '[' {
            text.push(chars[cursor]);
            markup.push(chars[cursor]);
            cursor += 1;
            continue;
        }
        let Some(label_end) = chars[cursor + 1..].iter().position(|value| *value == ']') else {
            text.push(chars[cursor]);
            markup.push(chars[cursor]);
            cursor += 1;
            continue;
        };
        let label_end = cursor + 1 + label_end;
        let label = chars[cursor + 1..label_end].iter().collect::<String>();
        if let Some(duration) = parse_inline_wait(&label) {
            pauses.push(crate::state::DialoguePause {
                at: text.chars().count(),
                duration,
            });
            cursor = label_end + 1;
            continue;
        }
        if chars.get(label_end + 1) != Some(&'(') {
            text.push(chars[cursor]);
            markup.push(chars[cursor]);
            cursor += 1;
            continue;
        }
        let Some(argument_end) = chars[label_end + 2..]
            .iter()
            .position(|value| *value == ')')
        else {
            text.push(chars[cursor]);
            markup.push(chars[cursor]);
            cursor += 1;
            continue;
        };
        let argument_end = label_end + 2 + argument_end;
        text.push_str(&label);
        markup.extend(chars[cursor..=argument_end].iter());
        cursor = argument_end + 1;
    }
    (text, markup, pauses)
}

fn parse_inline_wait(label: &str) -> Option<Option<f32>> {
    let label = label.trim();
    if label.eq_ignore_ascii_case("wait") {
        return Some(None);
    }
    let milliseconds = label
        .strip_prefix("wait=")
        .or_else(|| label.strip_prefix("wait time=\""))?
        .trim_end_matches('"')
        .parse::<f32>()
        .ok()?;
    Some(Some(milliseconds.max(0.0) / 1000.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{Choice, SayOptions};
    use crate::types::{BlendMode, Easing, Position, SpriteTransform, TransformPatch};
    use crate::{Value, VisualFilter};

    #[test]
    fn inline_wait_is_removed_from_text_and_retained_as_timing() {
        let (text, markup, pauses) = compile_rich_text("[前](color=#fff)[wait=1000]後[wait]");

        assert_eq!(text, "前後");
        assert_eq!(markup, "[前](color=#fff)後");
        assert_eq!(
            pauses,
            [
                crate::state::DialoguePause {
                    at: 1,
                    duration: Some(1.0),
                },
                crate::state::DialoguePause {
                    at: 2,
                    duration: None,
                },
            ]
        );
    }

    fn state_with(actions: Vec<Action>) -> State {
        let mut state = State::new();
        state.current_scene = "main".into();
        state.insert_scene("main".into(), actions);
        state
    }

    #[test]
    fn executes_until_dialogue() {
        let mut state = state_with(vec![
            Action::ShowBg {
                image: "room.webp".into(),
                transition: Transition::Instant,
                transform: SpriteTransform::default(),
            },
            Action::Say {
                speaker: "A".into(),
                text: "Hello".into(),
                options: SayOptions::default(),
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.bg.as_deref(), Some("room.webp"));
        assert_eq!(
            state.dialogue.as_ref().map(|d| d.text.as_str()),
            Some("Hello")
        );
    }

    #[test]
    fn retracts_dialogue_then_waits_for_a_fresh_click() {
        let mut state = state_with(vec![
            Action::Say {
                speaker: "A".into(),
                text: "我当然来了".into(),
                options: SayOptions::default(),
            },
            Action::RetractDialogue {
                source: "我当然来了".into(),
                keep: "我当然".into(),
            },
            Action::Say {
                speaker: "A".into(),
                text: "我来学校了".into(),
                options: SayOptions::default(),
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        state.dialogue.as_mut().unwrap().visible_chars = 5;
        advance(&mut state);
        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert!(state.presentation_blocked());

        assert!(update_dialogue_retraction(
            &mut state, 0.2, 10.0, false, false
        ));
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 3);
        assert!(
            state
                .dialogue_retraction
                .as_ref()
                .is_some_and(|retraction| retraction.awaiting_advance)
        );
        // The click that coincides with the last deletion tick is not reused.
        assert!(state.presentation_blocked());

        assert!(update_dialogue_retraction(
            &mut state, 0.0, 10.0, true, false
        ));
        assert!(!state.presentation_blocked());
        assert_eq!(step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.dialogue.as_ref().unwrap().text, "我来学校了");
    }

    #[test]
    fn consecutive_retractions_reuse_the_current_dialogue() {
        let mut state = state_with(vec![
            Action::Say {
                speaker: "A".into(),
                text: "我当然来了".into(),
                options: SayOptions::default(),
            },
            Action::RetractDialogue {
                source: "我当然来了".into(),
                keep: "我当然".into(),
            },
            Action::RetractDialogue {
                source: "我当然".into(),
                keep: "我".into(),
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        state.dialogue.as_mut().unwrap().visible_chars = 5;
        advance(&mut state);
        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        update_dialogue_retraction(&mut state, 1.0, 60.0, false, false);
        update_dialogue_retraction(&mut state, 0.0, 60.0, true, false);
        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert_eq!(state.dialogue.as_ref().unwrap().text, "我当然");
        update_dialogue_retraction(&mut state, 1.0, 60.0, false, false);
        assert_eq!(state.dialogue.as_ref().unwrap().text, "我");
    }

    #[test]
    fn dialogue_retraction_can_remove_the_entire_line() {
        let mut state = state_with(vec![
            Action::Say {
                speaker: "A".into(),
                text: "整句话".into(),
                options: SayOptions::default(),
            },
            Action::RetractDialogue {
                source: "整句话".into(),
                keep: String::new(),
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        state.dialogue.as_mut().unwrap().visible_chars = 3;
        advance(&mut state);
        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        update_dialogue_retraction(&mut state, 1.0, 60.0, false, false);
        assert_eq!(state.dialogue.as_ref().unwrap().text, "");
        assert!(
            state
                .dialogue_retraction
                .as_ref()
                .is_some_and(|retraction| retraction.awaiting_advance)
        );
    }

    #[test]
    fn dialogue_retraction_round_trips_mid_animation() {
        let mut state = state_with(vec![
            Action::Say {
                speaker: "A".into(),
                text: "abcdef".into(),
                options: SayOptions::default(),
            },
            Action::RetractDialogue {
                source: "abcdef".into(),
                keep: "ab".into(),
            },
        ]);
        assert_eq!(step(&mut state), StepResult::AwaitClick);
        state.dialogue.as_mut().unwrap().visible_chars = 6;
        advance(&mut state);
        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        update_dialogue_retraction(&mut state, 0.15, 10.0, false, false);

        let bytes = postcard::to_stdvec(&state).unwrap();
        let restored = postcard::from_bytes::<State>(&bytes).unwrap();
        assert_eq!(restored.dialogue, state.dialogue);
        assert_eq!(restored.dialogue_retraction, state.dialogue_retraction);
        assert!(restored.presentation_blocked());
    }

    #[test]
    fn dialogue_retraction_falls_back_to_the_current_line_and_keeps_graphemes_whole() {
        let mut state = state_with(vec![
            Action::Say {
                speaker: "A".into(),
                text: "A👩‍👩‍👧‍👧B".into(),
                options: SayOptions::default(),
            },
            Action::RetractDialogue {
                source: String::new(),
                keep: "A".into(),
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        let source_chars = state.dialogue.as_ref().unwrap().text.chars().count();
        state.dialogue.as_mut().unwrap().visible_chars = source_chars;
        advance(&mut state);
        assert_eq!(step(&mut state), StepResult::AwaitPresentation);

        update_dialogue_retraction(&mut state, 0.1, 10.0, false, false);
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 8);
        update_dialogue_retraction(&mut state, 0.1, 10.0, false, false);
        assert_eq!(state.dialogue.as_ref().unwrap().visible_chars, 1);
        assert_eq!(state.dialogue.as_ref().unwrap().text, "A");
    }

    #[test]
    fn jump_uses_precomputed_label_index() {
        let mut state = state_with(vec![
            Action::Jump("end".into()),
            Action::Say {
                speaker: "".into(),
                text: "skip".into(),
                options: SayOptions::default(),
            },
            Action::Label("end".into()),
            Action::Say {
                speaker: "".into(),
                text: "done".into(),
                options: SayOptions::default(),
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        assert_eq!(
            state.dialogue.as_ref().map(|d| d.text.as_str()),
            Some("done")
        );
    }

    #[test]
    fn selecting_choice_moves_cursor_and_clears_menu() {
        let mut state = state_with(vec![
            Action::Menu {
                prompt: String::new(),
                choices: vec![Choice {
                    text: "Go".into(),
                    target: ChoiceTarget::Label("next".into()),
                    show_when: None,
                    enable_when: None,
                }],
            },
            Action::Label("next".into()),
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitChoice);
        assert_eq!(state.menu.as_ref().unwrap().prompt, "");
        select_choice(&mut state, 0);
        assert!(state.menu.is_none());
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn invalid_choice_keeps_menu_open() {
        let mut state = state_with(vec![Action::Menu {
            prompt: "Pick one".into(),
            choices: vec![Choice {
                text: "Go".into(),
                target: ChoiceTarget::Label("next".into()),
                show_when: None,
                enable_when: None,
            }],
        }]);

        assert_eq!(step(&mut state), StepResult::AwaitChoice);
        select_choice(&mut state, 4);

        assert_eq!(state.menu.as_ref().unwrap().prompt, "Pick one");
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn repeated_step_does_not_advance_past_open_menu() {
        let mut state = state_with(vec![
            Action::Menu {
                prompt: String::new(),
                choices: vec![Choice {
                    text: "Go".into(),
                    target: ChoiceTarget::Label("next".into()),
                    show_when: None,
                    enable_when: None,
                }],
            },
            Action::Say {
                speaker: String::new(),
                text: "must wait".into(),
                options: SayOptions::default(),
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitChoice);
        assert_eq!(step(&mut state), StepResult::AwaitChoice);
        assert_eq!(state.cursor, 1);
        assert!(state.dialogue.is_none());
    }

    #[test]
    fn instant_sprite_is_immediately_visible_and_removable() {
        let mut state = state_with(vec![
            Action::ShowSprite {
                id: "hero".into(),
                image: "hero.webp".into(),
                position: Position::center(0.0),
                layout: crate::SpriteLayout::Natural,
                transition: Transition::Instant,
                transform: SpriteTransform::default(),
                z_index: 0,
                blend: BlendMode::Alpha,
            },
            Action::HideSprite {
                id: "hero".into(),
                transition: Transition::Instant,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::EndOfScene);
        assert!(!state.sprites.contains_key("hero"));
    }

    #[test]
    fn sprite_update_skips_absent_targets() {
        let mut state = state_with(vec![Action::UpdateSprite {
            id: "hero".into(),
            image: "smile.webp".into(),
            position: Position::right(0.0),
            layout: crate::SpriteLayout::Natural,
            scale: 1.2,
            duration: 0.5,
            easing: Easing::EaseOut,
            blocking: true,
        }]);

        assert_eq!(step(&mut state), StepResult::EndOfScene);
        assert!(!state.sprites.contains_key("hero"));
    }

    #[test]
    fn sprite_update_replaces_visible_image_and_animates_placement() {
        let mut state = state_with(vec![
            Action::ShowSprite {
                id: "hero".into(),
                image: "calm.webp".into(),
                position: Position::left(0.0),
                layout: crate::SpriteLayout::Natural,
                transition: Transition::Instant,
                transform: SpriteTransform::default(),
                z_index: 100,
                blend: BlendMode::Alpha,
            },
            Action::UpdateSprite {
                id: "hero".into(),
                image: "smile.webp".into(),
                position: Position::right(20.0),
                layout: crate::SpriteLayout::ViewportHeight(0.8),
                scale: 1.25,
                duration: 0.5,
                easing: Easing::EaseOut,
                blocking: true,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        let sprite = &state.sprites["hero"];
        assert_eq!(sprite.image, "smile.webp");
        assert_eq!(sprite.position, Position::right(20.0));
        assert_eq!(sprite.layout, crate::SpriteLayout::ViewportHeight(0.8));
        assert_eq!(
            sprite.transform_animation.as_ref().unwrap().to.scale_x,
            1.25
        );
        assert_eq!(
            sprite.position_animation.as_ref().unwrap().from,
            Position::left(0.0)
        );
        assert!(sprite.position_animation.as_ref().unwrap().blocking);
    }

    #[test]
    fn transform_animation_targets_a_sparse_patch_of_current_sprite_state() {
        let base = SpriteTransform {
            offset_x: 15.0,
            offset_y: -20.0,
            alpha: 0.7,
            scale_x: 1.2,
            scale_y: 0.85,
            rotation: 0.1,
            blur: 5.0,
            ..SpriteTransform::default()
        };
        let mut patch = TransformPatch::default();
        patch.set_offset_x(160.0);
        patch.set_alpha(0.3);
        let mut state = state_with(vec![
            Action::ShowSprite {
                id: "hero".into(),
                image: "hero.webp".into(),
                position: Position::center(0.0),
                layout: crate::SpriteLayout::Natural,
                transition: Transition::Instant,
                transform: base,
                z_index: 0,
                blend: BlendMode::Alpha,
            },
            Action::SetTransform {
                id: "hero".into(),
                transform: patch,
                duration: 0.5,
                easing: Easing::EaseOut,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        let sprite = &state.sprites["hero"];
        let animation = sprite.transform_animation.as_ref().unwrap();
        assert_eq!(animation.from, base);
        assert_eq!(
            animation.to,
            SpriteTransform {
                offset_x: 160.0,
                alpha: 0.3,
                ..base
            }
        );
        assert_eq!(animation.duration, 0.5);
        assert_eq!(animation.easing, Easing::EaseOut);
    }

    #[test]
    fn character_group_targets_apply_camera_state_to_visible_portraits() {
        let mut patch = TransformPatch::default();
        patch.set_offset_x(80.0);
        patch.set_scale_x(1.1);
        patch.set_scale_y(1.1);
        let filter = VisualFilter {
            blur: 4.0,
            ..VisualFilter::default()
        };
        let mut state = state_with(vec![
            Action::ShowSprite {
                id: "hero".into(),
                image: "hero.webp".into(),
                position: Position::center(0.0),
                layout: crate::SpriteLayout::Natural,
                transition: Transition::Instant,
                transform: SpriteTransform::default(),
                z_index: 0,
                blend: BlendMode::Alpha,
            },
            Action::SetTransform {
                id: "characters".into(),
                transform: patch,
                duration: 0.0,
                easing: Easing::Linear,
            },
            Action::SetFilter {
                target: "characters".into(),
                filter,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::EndOfScene);
        assert_eq!(state.sprites["hero"].transform.offset_x, 80.0);
        assert_eq!(state.sprites["hero"].transform.scale_x, 1.1);
        assert_eq!(state.sprites["hero"].filter, filter);
    }

    #[test]
    fn typed_camera_state_preserves_binding_distance_and_blocking_animation() {
        let mut patch = TransformPatch::default();
        patch.set_offset_x(120.0);
        patch.set_scale_x(1.25);
        patch.set_scale_y(1.25);
        let mut state = state_with(vec![
            Action::ShowSprite {
                id: "hero".into(),
                image: "hero.webp".into(),
                position: Position::center(0.0),
                layout: crate::SpriteLayout::Natural,
                transition: Transition::Instant,
                transform: SpriteTransform::default(),
                z_index: 0,
                blend: BlendMode::Alpha,
            },
            Action::SetCameraBinding {
                target: "hero".into(),
                bound: true,
                distance: 0.8,
            },
            Action::SetCameraTransform {
                targets: crate::CameraTargets::CHARACTERS,
                transform: patch,
                duration: 0.4,
                easing: Easing::EaseInOut,
                blocking: true,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert_eq!(state.sprites["hero"].camera_distance, Some(0.8));
        assert_eq!(state.camera_targets, crate::CameraTargets::CHARACTERS);
        let animation = state.camera_transform_animation.as_ref().unwrap();
        assert_eq!(animation.to.offset_x, 120.0);
        assert_eq!(animation.to.scale_x, 1.25);
        assert!(animation.blocking);
    }

    #[test]
    fn typed_video_blocks_without_losing_playback_options() {
        let spec = crate::VideoSpec {
            id: "opening".into(),
            file: "opening.mp4".into(),
            looped: false,
            muted: false,
            alpha: 0.75,
            skippable: false,
            wait_for_finished: true,
            mode: crate::VideoMode::Fullscreen,
        };
        let mut state = state_with(vec![Action::PlayVideo {
            video: spec.clone(),
        }]);

        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert_eq!(state.videos["opening"].spec, spec);
        assert_eq!(state.videos["opening"].opacity, 0.75);
        assert!(state.presentation_blocked());
    }

    #[test]
    fn next_video_stays_non_blocking_until_an_explicit_stop() {
        let spec = crate::VideoSpec {
            id: "preview".into(),
            file: "preview.mp4".into(),
            looped: false,
            muted: false,
            alpha: 1.0,
            skippable: true,
            wait_for_finished: true,
            mode: crate::VideoMode::Fullscreen,
        };
        let mut state = state_with(vec![Action::Flow {
            action: Box::new(Action::PlayVideo { video: spec }),
            when: None,
            next: true,
        }]);

        assert_eq!(step(&mut state), StepResult::EndOfScene);
        assert!(!state.videos["preview"].spec.wait_for_finished);
        assert!(!state.presentation_blocked());
    }

    #[test]
    fn timed_stop_executes_while_a_next_video_is_still_playing() {
        let spec = crate::VideoSpec {
            id: "preview".into(),
            file: "preview.mp4".into(),
            looped: false,
            muted: false,
            alpha: 1.0,
            skippable: true,
            wait_for_finished: true,
            mode: crate::VideoMode::Fullscreen,
        };
        let mut state = state_with(vec![
            Action::Flow {
                action: Box::new(Action::PlayVideo { video: spec }),
                when: None,
                next: true,
            },
            Action::Wait { seconds: 1.1 },
            Action::StopVideo {
                id: Some("preview".into()),
                fade_out: 0.0,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert!(state.videos.contains_key("preview"));
        state.wait_remaining = 0.0;
        assert_eq!(step(&mut state), StepResult::EndOfScene);
        assert!(!state.videos.contains_key("preview"));
    }

    #[test]
    fn zero_duration_transform_patch_is_applied_immediately_to_background() {
        let base = SpriteTransform {
            offset_y: 40.0,
            alpha: 0.8,
            scale_x: 1.1,
            scale_y: 1.1,
            blur: 7.0,
            ..SpriteTransform::default()
        };
        let mut patch = TransformPatch::default();
        patch.set_rotation(0.4);
        patch.set_blur(0.0);
        let mut state = state_with(vec![
            Action::ShowBg {
                image: "room.webp".into(),
                transition: Transition::Instant,
                transform: base,
            },
            Action::SetTransform {
                id: "bg-main".into(),
                transform: patch,
                duration: 0.0,
                easing: Easing::EaseInOut,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::EndOfScene);
        assert_eq!(
            state.bg_transform,
            SpriteTransform {
                rotation: 0.4,
                blur: 0.0,
                ..base
            }
        );
        assert!(state.bg_transform_animation.is_none());
    }

    #[test]
    fn change_scene_replaces_current_flow() {
        let mut state = state_with(vec![
            Action::ChangeScene("chapter".into()),
            Action::Say {
                speaker: String::new(),
                text: "unreachable".into(),
                options: SayOptions::default(),
            },
        ]);
        state.insert_scene(
            "chapter".into(),
            vec![Action::Say {
                speaker: String::new(),
                text: "chapter".into(),
                options: SayOptions::default(),
            }],
        );

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, "chapter");
        assert_eq!(state.dialogue.as_ref().unwrap().text, "chapter");
        assert!(state.scene_stack.is_empty());
    }

    #[test]
    fn call_scene_returns_to_following_action() {
        let mut state = state_with(vec![
            Action::CallScene("aside".into()),
            Action::Say {
                speaker: String::new(),
                text: "back".into(),
                options: SayOptions::default(),
            },
        ]);
        state.insert_scene(
            "aside".into(),
            vec![Action::Say {
                speaker: String::new(),
                text: "inside".into(),
                options: SayOptions::default(),
            }],
        );

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, "aside");
        assert_eq!(state.scene_stack.len(), 1);
        advance(&mut state);
        assert_eq!(step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, "main");
        assert_eq!(state.dialogue.as_ref().unwrap().text, "back");
        assert!(state.scene_stack.is_empty());
    }

    #[test]
    fn nested_scene_calls_restore_in_lifo_order() {
        let mut state = state_with(vec![Action::CallScene("first".into())]);
        state.insert_scene(
            "first".into(),
            vec![
                Action::CallScene("second".into()),
                Action::Say {
                    speaker: String::new(),
                    text: "first".into(),
                    options: SayOptions::default(),
                },
            ],
        );
        state.insert_scene("second".into(), Vec::new());

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, "first");
        assert_eq!(state.dialogue.as_ref().unwrap().text, "first");
        assert_eq!(state.scene_stack.len(), 1);
        advance(&mut state);
        assert_eq!(step(&mut state), StepResult::EndOfScene);
        assert_eq!(state.current_scene, "main");
        assert!(state.scene_stack.is_empty());
    }

    #[test]
    fn explicit_end_discards_call_stack() {
        let mut state = state_with(vec![
            Action::CallScene("ending".into()),
            Action::Say {
                speaker: String::new(),
                text: "must not return".into(),
                options: SayOptions::default(),
            },
        ]);
        state.insert_scene("ending".into(), vec![Action::End]);

        assert_eq!(step(&mut state), StepResult::EndOfScene);
        assert_eq!(state.current_scene, "ending");
        assert!(state.scene_stack.is_empty());
        assert!(state.dialogue.is_none());
        assert!(state.ended);
        assert_eq!(step(&mut state), StepResult::EndOfScene);
    }

    #[test]
    fn transition_step_defers_destructive_end_cleanup() {
        let mut state = state_with(vec![
            Action::ShowBg {
                image: "ending.webp".into(),
                transition: Transition::Instant,
                transform: SpriteTransform::default(),
            },
            Action::End,
        ]);

        assert_eq!(
            step_preserving_presentation(&mut state),
            StepResult::EndOfScene
        );
        assert_eq!(state.bg.as_deref(), Some("ending.webp"));
        assert!(!state.ended);

        end_game(&mut state);
        assert!(state.bg.is_none());
        assert!(state.ended);
    }

    #[test]
    fn expressions_globals_conditions_and_interpolation_share_one_runtime() {
        let mut state = state_with(vec![
            Action::Set {
                name: "score".into(),
                expression: "2 + 3 * 2".into(),
                global: false,
            },
            Action::Set {
                name: "name".into(),
                expression: "'MainCore'".into(),
                global: true,
            },
            Action::Flow {
                action: Box::new(Action::Say {
                    speaker: "{name}".into(),
                    text: "score={score}".into(),
                    options: SayOptions::default(),
                }),
                when: Some("score == 8".into()),
                next: false,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        let dialogue = state.dialogue.as_ref().unwrap();
        assert_eq!(dialogue.speaker, "MainCore");
        assert_eq!(dialogue.text, "score=8");
        assert_eq!(
            state.global_vars.get("name"),
            Some(&Value::Str("MainCore".into()))
        );
    }

    #[test]
    fn next_and_concat_follow_webgal_flow() {
        let concat = SayOptions {
            concat: true,
            inherit_speaker: true,
            ..SayOptions::default()
        };
        let mut state = state_with(vec![
            Action::Flow {
                action: Box::new(Action::Say {
                    speaker: "A".into(),
                    text: "ignored by next".into(),
                    options: SayOptions::default(),
                }),
                when: None,
                next: true,
            },
            Action::Say {
                speaker: "A".into(),
                text: "Hello".into(),
                options: SayOptions::default(),
            },
            Action::Say {
                speaker: String::new(),
                text: " world".into(),
                options: concat,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.dialogue.as_ref().unwrap().text, "Hello");
        advance(&mut state);
        assert_eq!(step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.dialogue.as_ref().unwrap().speaker, "A");
        assert_eq!(state.dialogue.as_ref().unwrap().text, "Hello world");
    }

    #[test]
    fn choice_filters_disables_and_changes_scene() {
        let mut state = state_with(vec![Action::Menu {
            prompt: String::new(),
            choices: vec![
                Choice {
                    text: "hidden".into(),
                    target: ChoiceTarget::Label("none".into()),
                    show_when: Some("false".into()),
                    enable_when: None,
                },
                Choice {
                    text: "disabled".into(),
                    target: ChoiceTarget::Label("none".into()),
                    show_when: None,
                    enable_when: Some("false".into()),
                },
                Choice {
                    text: "scene".into(),
                    target: ChoiceTarget::ChangeScene("chapter".into()),
                    show_when: None,
                    enable_when: Some("true".into()),
                },
            ],
        }]);
        state.insert_scene(
            "chapter".into(),
            vec![Action::Say {
                speaker: String::new(),
                text: "arrived".into(),
                options: SayOptions::default(),
            }],
        );

        assert_eq!(step(&mut state), StepResult::AwaitChoice);
        assert_eq!(state.menu.as_ref().unwrap().choices.len(), 2);
        select_choice(&mut state, 0);
        assert!(state.menu.is_some());
        select_choice(&mut state, 1);
        assert_eq!(state.current_scene, "chapter");
        assert_eq!(step(&mut state), StepResult::AwaitClick);
    }

    #[test]
    fn runaway_jump_hits_execution_limit() {
        let mut state = state_with(vec![
            Action::Label("loop".into()),
            Action::Jump("loop".into()),
        ]);
        assert_eq!(step(&mut state), StepResult::ExecutionLimit);
    }

    #[test]
    fn audio_commands_update_persistent_and_transient_state() {
        let mut state = state_with(vec![
            Action::Bgm {
                file: "theme.ogg".into(),
                volume: 0.7,
                fade_seconds: 1.5,
            },
            Action::Effect {
                file: Some("rain.ogg".into()),
                volume: 0.4,
                id: Some("weather".into()),
            },
            Action::Effect {
                file: Some("click.wav".into()),
                volume: 0.8,
                id: None,
            },
            Action::Vocal {
                file: Some("line.ogg".into()),
                volume: 0.6,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::EndOfScene);
        assert_eq!(state.bgm.file.as_deref(), Some("theme.ogg"));
        assert_eq!(state.bgm.volume, 0.7);
        assert_eq!(state.bgm.fade_seconds, 1.5);
        assert_eq!(state.looping_effects["weather"].file, "rain.ogg");
        assert!(matches!(
            &state.effect_queue[0],
            crate::state::EffectEvent::Play(cue) if cue.file == "click.wav"
        ));
        assert_eq!(
            state.vocal_event,
            Some(crate::state::VocalCue {
                file: Some("line.ogg".into()),
                volume: 0.6,
                fade_in: 0.0,
                fade_out: 0.0,
            })
        );
    }

    #[test]
    fn presentation_commands_block_and_persist_stage_state() {
        use crate::types::{AnimationPreset, BlendMode, Position, VisualFilter};

        let filter = VisualFilter {
            blur: 4.0,
            brightness: 0.9,
            ..Default::default()
        };
        let mut state = state_with(vec![
            Action::ShowSprite {
                id: "hero".into(),
                image: "hero.webp".into(),
                position: Position::center(0.0),
                layout: crate::SpriteLayout::Natural,
                transition: Transition::Instant,
                transform: Default::default(),
                z_index: 0,
                blend: BlendMode::Screen,
            },
            Action::Animate {
                target: "hero".into(),
                preset: AnimationPreset::Shake,
                duration: 0.4,
            },
            Action::SetFilter {
                target: "hero".into(),
                filter,
            },
            Action::FilmMode { enabled: true },
            Action::ShowParticles {
                id: "weather".into(),
                effect: crate::ParticleEffect::preset("rain"),
            },
            Action::Wait { seconds: 0.5 },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert!(state.sprites["hero"].animation.is_some());
        state.sprites.get_mut("hero").unwrap().animation = None;
        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert_eq!(state.sprites["hero"].filter, filter);
        assert!(state.film_mode);
        assert_eq!(state.particle_effects["weather"].effect.preset, "rain");
        assert_eq!(state.wait_remaining, 0.5);
    }

    #[test]
    fn sprite_group_exit_only_targets_the_requested_namespace() {
        let show = |id: &str| Action::ShowSprite {
            id: id.into(),
            image: format!("{id}.png"),
            position: Position::center(0.0),
            layout: crate::SpriteLayout::Natural,
            transition: Transition::Instant,
            transform: Default::default(),
            z_index: 0,
            blend: crate::BlendMode::Alpha,
        };
        let mut state = state_with(vec![
            show("scene-layer:clouds"),
            show("scene-layer:trees"),
            show("hero"),
            Action::Flow {
                action: Box::new(Action::HideSprites {
                    prefix: "scene-layer:".into(),
                    transition: Transition::Crossfade(0.4),
                }),
                when: None,
                next: true,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::EndOfScene);
        assert!(!state.sprites["scene-layer:clouds"].entering);
        assert!(!state.sprites["scene-layer:trees"].entering);
        assert!(state.sprites["hero"].entering);
    }

    #[test]
    fn named_particle_emitters_can_fade_independently() {
        let mut state = state_with(vec![
            Action::ShowParticles {
                id: "snow".into(),
                effect: crate::ParticleEffect {
                    texture: Some("particles/snow.png".into()),
                    preset: "MODERATE_SNOW".into(),
                    count: 80,
                    wind: None,
                    gravity: None,
                    fade_in: 0.25,
                },
            },
            Action::ShowParticles {
                id: "leaves".into(),
                effect: crate::ParticleEffect::preset("FALLEN_LEAVES"),
            },
            Action::HideParticles {
                id: Some("snow".into()),
                duration: 0.4,
            },
        ]);

        assert_eq!(step(&mut state), StepResult::EndOfScene);
        assert_eq!(state.particle_effects.len(), 2);
        assert!(state.particle_effects["snow"].fading_out);
        assert!(!state.particle_effects["leaves"].fading_out);
        assert_eq!(state.particle_effects["snow"].fade_out, 0.4);
    }

    #[test]
    fn borrowed_non_copy_action_payloads_persist_in_runtime_state() {
        use crate::types::AnimationPreset;

        let mut state = state_with(vec![
            Action::SetTransition {
                target: "hero".into(),
                enter: Some(AnimationPreset::Custom("soft-enter".into())),
                exit: Some(AnimationPreset::Custom("soft-exit".into())),
                duration: 0.3,
            },
            Action::MiniAvatar {
                image: "avatar.webp".into(),
            },
            Action::Animate {
                target: "bg-main".into(),
                preset: AnimationPreset::Custom("ambient-drift".into()),
                duration: 0.4,
            },
        ]);
        let program = Arc::clone(&state.program);

        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert!(Arc::ptr_eq(&program, &state.program));
        assert_eq!(state.mini_avatar.as_deref(), Some("avatar.webp"));
        assert!(matches!(
            state.transition_rules["hero"].enter.as_ref(),
            Some(AnimationPreset::Custom(name)) if name == "soft-enter"
        ));
        assert!(matches!(
            state.bg_animation.as_ref().map(|animation| &animation.preset),
            Some(AnimationPreset::Custom(name)) if name == "ambient-drift"
        ));
    }

    #[test]
    fn rich_text_input_and_textbox_commands_share_runtime_state() {
        let mut state = state_with(vec![
            Action::SetTextbox {
                visible: false,
                auto: true,
            },
            Action::Say {
                speaker: "A".into(),
                text: "[蟹](かに)[色](color=#fff)".into(),
                options: SayOptions::default(),
            },
            Action::UserInput {
                variable: "name".into(),
                title: "Name".into(),
                button: "OK".into(),
            },
        ]);
        assert_eq!(step(&mut state), StepResult::AwaitClick);
        let dialogue = state.dialogue.as_ref().unwrap();
        assert_eq!(dialogue.text, "蟹色");
        assert_eq!(dialogue.markup, "[蟹](かに)[色](color=#fff)");
        assert!(!state.textbox_hidden);
        advance(&mut state);
        assert_eq!(step(&mut state), StepResult::AwaitInput);
        state.user_input.as_mut().unwrap().value = "小夜".into();
        assert!(submit_user_input(&mut state));
        assert_eq!(state.vars["name"], crate::Value::Str("小夜".into()));
    }

    #[test]
    fn typed_editor_input_validates_before_storing_a_native_value() {
        let mut state = state_with(vec![Action::RequestInput {
            spec: crate::UserInputSpec {
                variable: "age".into(),
                value_type: crate::InputValueType::Number,
                title: "Age".into(),
                required_text: "required".into(),
                min_value: Some(12.0),
                max_value: Some(18.0),
                ..Default::default()
            },
        }]);

        assert_eq!(step(&mut state), StepResult::AwaitInput);
        state.user_input.as_mut().unwrap().value = "9".into();
        assert!(!submit_user_input(&mut state));
        assert!(state.user_input.as_ref().unwrap().error.contains("范围"));

        state.user_input.as_mut().unwrap().value = "16".into();
        assert!(submit_user_input(&mut state));
        assert_eq!(state.vars["age"], crate::Value::Int(16));
    }

    #[test]
    fn typed_number_input_preserves_large_integers() {
        let mut state = state_with(vec![Action::RequestInput {
            spec: crate::UserInputSpec {
                variable: "identifier".into(),
                value_type: crate::InputValueType::Number,
                title: "Identifier".into(),
                required_text: "required".into(),
                ..Default::default()
            },
        }]);

        assert_eq!(step(&mut state), StepResult::AwaitInput);
        state.user_input.as_mut().unwrap().value = "9007199254740993".into();
        assert!(submit_user_input(&mut state));
        assert_eq!(
            state.vars["identifier"],
            crate::Value::Int(9_007_199_254_740_993)
        );
    }

    #[test]
    fn editor_step_limit_stops_before_the_next_source_block() {
        let mut state = state_with(vec![
            Action::Set {
                name: "first".into(),
                expression: "1".into(),
                global: false,
            },
            Action::Set {
                name: "second".into(),
                expression: "2".into(),
                global: false,
            },
        ]);

        assert_eq!(
            step_until_cursor(&mut state, "main", 1),
            StepResult::EndOfScene
        );
        assert_eq!(state.cursor, 1);
        assert!(state.vars.contains_key("first"));
        assert!(!state.vars.contains_key("second"));
    }

    #[test]
    fn explicit_advance_wait_does_not_fall_back_to_a_timer() {
        let mut state = state_with(vec![
            Action::WaitForAdvance,
            Action::Say {
                speaker: String::new(),
                text: "continued".into(),
                options: SayOptions::default(),
            },
        ]);

        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert!(state.waiting_for_advance);
        assert_eq!(state.cursor, 1);
        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert_eq!(state.cursor, 1);

        state.waiting_for_advance = false;
        assert_eq!(step(&mut state), StepResult::AwaitClick);
        assert_eq!(
            state
                .dialogue
                .as_ref()
                .map(|dialogue| dialogue.text.as_str()),
            Some("continued")
        );
    }

    #[test]
    fn sprite_image_can_follow_an_adapter_neutral_runtime_variable() {
        let mut state = state_with(vec![
            Action::ShowSprite {
                id: "aya".into(),
                image: "summer.webp".into(),
                position: Position::center(0.0),
                layout: crate::SpriteLayout::ViewportHeight(0.8),
                transition: Transition::Instant,
                transform: SpriteTransform::default(),
                z_index: 0,
                blend: BlendMode::Alpha,
            },
            Action::SelectSpriteImage {
                id: "aya".into(),
                variable: "aya.skin".into(),
                default_image: "summer.webp".into(),
                variants: vec![
                    ("summer".into(), "summer.webp".into()),
                    ("winter".into(), "winter.webp".into()),
                ],
            },
            Action::Say {
                speaker: "Aya".into(),
                text: "ready".into(),
                options: SayOptions::default(),
            },
        ]);
        state
            .vars
            .insert("aya.skin".into(), Value::Str("winter".into()));

        assert_eq!(step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.sprites["aya"].image, "winter.webp");
        assert_eq!(
            state.sprites["aya"].layout,
            crate::SpriteLayout::ViewportHeight(0.8)
        );
    }

    #[test]
    fn curtain_action_blocks_until_the_host_finishes_the_fade() {
        let mut state = state_with(vec![Action::Curtain {
            visible: true,
            color: [0.1, 0.2, 0.3, 1.0],
            duration: 0.4,
        }]);

        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert!(state.curtain.blocking);
        assert_eq!(state.curtain.target, 1.0);
        assert_eq!(state.curtain.color, [0.1, 0.2, 0.3, 1.0]);
    }

    #[test]
    fn system_confirmation_blocks_and_stores_the_native_boolean_result() {
        let mut state = state_with(vec![Action::SystemMessage {
            spec: crate::SystemMessageSpec {
                mode: crate::SystemMessageMode::Confirm,
                title: "Confirm".into(),
                message: "Continue?".into(),
                confirm_text: "Yes".into(),
                cancel_text: "No".into(),
                result_variable: Some("accepted".into()),
            },
        }]);

        assert_eq!(step(&mut state), StepResult::AwaitPresentation);
        assert!(state.presentation_blocked());
        assert!(resolve_system_message(&mut state, true));
        assert_eq!(state.vars["accepted"], Value::Bool(true));
        assert!(!state.presentation_blocked());
    }

    #[test]
    fn named_infinite_floating_text_uses_its_authored_exit() {
        let mut state = state_with(vec![
            Action::FloatingText {
                text: "persistent".into(),
                position: [0.5, 0.5],
                font_size: 40.0,
                color: [1.0; 4],
                fade_in: 0.1,
                hold: 0.0,
                fade_out: 0.2,
                blocking: false,
            },
            Action::ConfigureFloatingText {
                id: Some("notice".into()),
                infinite: true,
            },
            Action::HideFloatingText {
                id: Some("notice".into()),
            },
        ]);

        assert_eq!(step(&mut state), StepResult::EndOfScene);
        let floating = state.floating_text.as_ref().unwrap();
        assert!(!floating.infinite);
        assert_eq!(floating.elapsed, floating.fade_in + floating.hold);
    }
}
