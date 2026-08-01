use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use crate::runtime::audio::OpusAudio;
use crate::storage::settings::RuntimeSettings;
use crate::ui::foundation::UiSoundStyle;

const CLICK_PATH: &str = "embedded://keine/assets/audio/click.opus";
const HOVER_PATH: &str = "embedded://keine/assets/audio/mouse-enter.opus";
const SWITCH_PATH: &str = "embedded://keine/assets/audio/switch.opus";
// perceived baseline while preserving the full user-facing slider range.
const UI_CUE_GAIN: f32 = 0.1;

#[derive(Component)]
pub(crate) struct UiSoundPlayer;

#[derive(Component)]
pub(crate) struct UiSoundInteraction(Interaction);

#[derive(Resource)]
pub(crate) struct UiSoundAssets {
    click: Handle<OpusAudio>,
    hover: Handle<OpusAudio>,
    switch: Handle<OpusAudio>,
}

impl FromWorld for UiSoundAssets {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            click: assets.load(CLICK_PATH),
            hover: assets.load(HOVER_PATH),
            switch: assets.load(SWITCH_PATH),
        }
    }
}

type ChangedButtonSounds<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Interaction,
        Option<&'static UiSoundStyle>,
        Option<&'static mut UiSoundInteraction>,
    ),
    (Changed<Interaction>, With<Button>),
>;

pub(crate) fn play_button_sounds(
    mut buttons: ChangedButtonSounds,
    sounds: Res<UiSoundAssets>,
    settings: Res<RuntimeSettings>,
    mut commands: Commands,
) {
    let volume = settings.master_volume * settings.ui_se_volume * UI_CUE_GAIN;
    if volume <= f32::EPSILON {
        return;
    }

    for (entity, interaction, style, previous) in &mut buttons {
        let style = style.copied().unwrap_or_default();
        let old = previous.as_ref().map_or(Interaction::None, |state| state.0);
        let cue = match cue_for_transition(old, *interaction, style) {
            Some(UiCue::Hover) => Some(sounds.hover.clone()),
            Some(UiCue::Click) => Some(sounds.click.clone()),
            Some(UiCue::Switch) => Some(sounds.switch.clone()),
            None => None,
        };
        if let Some(mut previous) = previous {
            previous.0 = *interaction;
        } else {
            commands
                .entity(entity)
                .insert(UiSoundInteraction(*interaction));
        }
        let Some(cue) = cue else { continue };
        // WebGAL K creates a short-lived audio element for every cue. Keep the
        // same overlap semantics so a release or a new hover cannot truncate
        // the click that is already playing.
        commands.spawn((
            UiSoundPlayer,
            AudioPlayer::<OpusAudio>(cue),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(volume),
                ..default()
            },
        ));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UiCue {
    Hover,
    Click,
    Switch,
}

fn cue_for_transition(
    previous: Interaction,
    current: Interaction,
    style: UiSoundStyle,
) -> Option<UiCue> {
    match (previous, current, style) {
        (Interaction::None, Interaction::Hovered, _) => Some(UiCue::Hover),
        (_, Interaction::Pressed, UiSoundStyle::Switch) => Some(UiCue::Switch),
        (_, Interaction::Pressed, UiSoundStyle::Click) => Some(UiCue::Click),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releasing_a_button_does_not_retrigger_hover_audio() {
        assert_eq!(
            cue_for_transition(Interaction::None, Interaction::Hovered, UiSoundStyle::Click,),
            Some(UiCue::Hover)
        );
        assert_eq!(
            cue_for_transition(
                Interaction::Hovered,
                Interaction::Pressed,
                UiSoundStyle::Click,
            ),
            Some(UiCue::Click)
        );
        assert_eq!(
            cue_for_transition(
                Interaction::Pressed,
                Interaction::Hovered,
                UiSoundStyle::Click,
            ),
            None
        );
    }
}
