//! Android button feedback, independent of UI audio volume and feature flags.
use crate::ui::foundation::{Button, Interaction};
use bevy::prelude::*;

use crate::ui::backlog::BacklogUiState;
use crate::ui::foundation::UiSoundStyle;

#[derive(Component)]
pub(crate) struct UiHapticInteraction(Interaction);

type ChangedButtons<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Interaction,
        Option<&'static UiSoundStyle>,
        Option<&'static mut UiHapticInteraction>,
    ),
    (Changed<Interaction>, With<Button>),
>;

pub(crate) fn button_feedback(
    mut buttons: ChangedButtons,
    backlog: Res<BacklogUiState>,
    mut was_open: Local<bool>,
    mut commands: Commands,
) {
    // Also cover gesture/wheel entry and non-button exits, once per state change.
    let mut feedback = (*was_open != backlog.open).then_some(true);
    *was_open = backlog.open;
    for (entity, interaction, style, previous) in &mut buttons {
        let old = previous.as_ref().map_or(Interaction::None, |state| state.0);
        let cue = press_feedback(old, *interaction, style.copied().unwrap_or_default());
        if let Some(mut previous) = previous {
            previous.0 = *interaction;
        } else {
            commands
                .entity(entity)
                .insert(UiHapticInteraction(*interaction));
        }
        feedback = feedback.or(cue);
    }
    if let Some(switch) = feedback {
        #[cfg(target_os = "android")]
        crate::runtime::platform::ui_haptic_feedback(switch);
        #[cfg(not(target_os = "android"))]
        let _ = switch;
    }
}

fn press_feedback(
    previous: Interaction,
    current: Interaction,
    style: UiSoundStyle,
) -> Option<bool> {
    if previous == Interaction::Pressed || current != Interaction::Pressed {
        return None;
    }
    match style {
        UiSoundStyle::Click => Some(false),
        UiSoundStyle::Switch => Some(true),
        // Sliders share this style; dragging must not produce continuous pulses.
        UiSoundStyle::HoverOnly => None,
    }
}

/// Invoked only from user-driven slider/list updates, never from animation sync.
#[cfg(target_os = "android")]
pub(crate) fn drag_feedback(before: f32, after: f32, max: f32, step: f32) {
    if let Some(switch) = motion_cue(before, after, max, step) {
        crate::runtime::platform::ui_haptic_feedback(switch);
    }
}

fn motion_cue(before: f32, after: f32, max: f32, step: f32) -> Option<bool> {
    if ![before, after, max, step].into_iter().all(f32::is_finite)
        || max <= 0.0
        || step <= 0.0
        || before == after
    {
        return None;
    }
    let before = before.clamp(0.0, max);
    let after = after.clamp(0.0, max);
    if before == after {
        return None;
    }
    if after == 0.0 || after == max {
        return Some(false); // A firmer endpoint tap, once when it is reached.
    }
    ((before / step).floor() != (after / step).floor()).then_some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_only_follows_new_button_presses_and_tracks_releases() {
        for (old, current, style, expected) in [
            (
                Interaction::None,
                Interaction::Pressed,
                UiSoundStyle::Click,
                Some(false),
            ),
            (
                Interaction::Hovered,
                Interaction::Pressed,
                UiSoundStyle::Switch,
                Some(true),
            ),
            (
                Interaction::Pressed,
                Interaction::Pressed,
                UiSoundStyle::Click,
                None,
            ),
            (
                Interaction::Pressed,
                Interaction::Hovered,
                UiSoundStyle::Click,
                None,
            ),
            (
                Interaction::None,
                Interaction::Hovered,
                UiSoundStyle::Click,
                None,
            ),
            (
                Interaction::None,
                Interaction::Pressed,
                UiSoundStyle::HoverOnly,
                None,
            ),
        ] {
            assert_eq!(press_feedback(old, current, style), expected);
        }
        for (before, after, max, step, expected) in [
            (128.0, 129.0, 300.0, 64.0, None),
            (127.0, 129.0, 300.0, 64.0, Some(true)),
            (129.0, 127.0, 300.0, 64.0, Some(true)),
            (100.0, 280.0, 300.0, 64.0, Some(true)),
            (280.0, 300.0, 300.0, 64.0, Some(false)),
            (20.0, 0.0, 300.0, 64.0, Some(false)),
            (300.0, 300.0, 300.0, 64.0, None),
            (0.0, 0.0, 0.0, 64.0, None),
            (0.42, 0.44, 1.0, 0.05, None),
            (0.44, 0.46, 1.0, 0.05, Some(true)),
            (0.98, 1.0, 1.0, 0.05, Some(false)),
            (f32::NAN, 1.0, 1.0, 0.05, None),
        ] {
            assert_eq!(motion_cue(before, after, max, step), expected);
        }
        let mut app = App::new();
        app.init_resource::<BacklogUiState>()
            .add_systems(Update, button_feedback);
        let button = app.world_mut().spawn((Button, Interaction::Pressed)).id();
        app.update();
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
        app.update();
        assert_eq!(
            app.world().get::<UiHapticInteraction>(button).unwrap().0,
            Interaction::None
        );
        app.update();
        assert_eq!(
            app.world().get::<UiHapticInteraction>(button).unwrap().0,
            Interaction::None
        );
    }
}
