use std::time::Duration;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use keine_core::{DESIGN_HEIGHT, DESIGN_WIDTH};

use crate::render::blur::{DialogCamera, UiBlurCamera};
use crate::runtime::resources::GameState;
use crate::ui::FULLSCREEN_BLUR_STRENGTH;
use crate::ui::control_bar::{BlurStrength, UiBlurSource};
use crate::ui::foundation::{HoverSweep, UiFonts, hover_sweep_fill};

const CARET_PERIOD_SECONDS: f32 = 1.0;
const CARET_VISIBLE_SECONDS: f32 = 0.55;
const CARET_ALPHA: f32 = 0.8;

#[derive(Component)]
pub(crate) struct UserInputRoot;
#[derive(Component)]
pub(crate) struct UserInputBlur;
#[derive(Component)]
pub(crate) struct UserInputTitle;
#[derive(Component)]
pub(crate) struct UserInputDescription;
#[derive(Component)]
pub(crate) struct UserInputValue;
#[derive(Component)]
pub(crate) struct UserInputError;
#[derive(Component)]
pub(crate) struct UserInputConfirm;

#[derive(Component)]
pub(crate) struct UserInputCaret;

#[derive(Resource, Default)]
pub(crate) struct UserInputCaretBlink {
    started_at: f32,
    active: bool,
}

impl UserInputCaretBlink {
    fn reset(&mut self, now: f32) {
        self.started_at = now;
        self.active = true;
    }

    fn stop(&mut self) {
        self.active = false;
    }

    fn phase(&self, now: f32) -> f32 {
        (now - self.started_at)
            .max(0.0)
            .rem_euclid(CARET_PERIOD_SECONDS)
    }

    fn alpha(&self, now: f32) -> f32 {
        if self.active && self.phase(now) < CARET_VISIBLE_SECONDS {
            CARET_ALPHA
        } else {
            0.0
        }
    }

    pub(crate) fn next_toggle_in(&self, now: f32) -> Duration {
        if !self.active {
            return Duration::MAX;
        }
        let phase = self.phase(now);
        let seconds = if phase < CARET_VISIBLE_SECONDS {
            CARET_VISIBLE_SECONDS - phase
        } else {
            CARET_PERIOD_SECONDS - phase
        };
        Duration::from_secs_f32(seconds.max(0.001))
    }
}

pub(crate) fn ensure_spawned(
    mut commands: Commands,
    state: Res<GameState>,
    fonts: Res<UiFonts>,
    cameras: Query<Entity, With<DialogCamera>>,
    blur_cameras: Query<Entity, With<UiBlurCamera>>,
    existing: Query<(), With<UserInputRoot>>,
) {
    if state.user_input.is_none() || !existing.is_empty() {
        return;
    }
    let (Ok(camera), Ok(blur_camera)) = (cameras.single(), blur_cameras.single()) else {
        return;
    };
    commands.spawn((
        Name::new("user_input_blur"),
        UserInputBlur,
        UiBlurSource,
        BlurStrength(FULLSCREEN_BLUR_STRENGTH),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(DESIGN_WIDTH),
            height: Val::Px(DESIGN_HEIGHT),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.36)),
        FocusPolicy::Pass,
        UiTargetCamera(blur_camera),
        RenderLayers::layer(1),
        GlobalZIndex(979),
    ));
    commands
        .spawn((
            Name::new("user_input_overlay"),
            UserInputRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(DESIGN_WIDTH),
                height: Val::Px(DESIGN_HEIGHT),
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.05)),
            FocusPolicy::Block,
            UiTargetCamera(camera),
            RenderLayers::layer(2),
            GlobalZIndex(980),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(75.0), Val::Px(24.0)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(7.5),
                    border: UiRect::top(Val::Px(15.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.19)),
                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.19)),
                BoxShadow::new(
                    Color::srgba(0.0, 0.0, 0.0, 0.25),
                    Val::ZERO,
                    Val::ZERO,
                    Val::ZERO,
                    Val::Px(25.0),
                ),
            ))
            .with_children(|panel| {
                panel.spawn((
                    UserInputTitle,
                    Text::new("INPUT"),
                    TextFont {
                        font: fonts.text.clone().into(),
                        font_size: FontSize::Px(45.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
                panel.spawn((
                    UserInputDescription,
                    Text::new(""),
                    TextFont {
                        font: fonts.text.clone().into(),
                        font_size: FontSize::Px(24.0),
                        ..default()
                    },
                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.64)),
                    TextLayout::justify(Justify::Center),
                ));
                panel
                    .spawn((
                        Node {
                            min_width: Val::Percent(50.0),
                            min_height: Val::Px(66.0),
                            padding: UiRect::axes(Val::Px(15.0), Val::Px(3.75)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.19)),
                        BorderColor::all(Color::NONE),
                        BoxShadow::new(
                            Color::srgba(0.0, 0.0, 0.0, 0.25),
                            Val::ZERO,
                            Val::ZERO,
                            Val::ZERO,
                            Val::Px(25.0),
                        ),
                    ))
                    .with_children(|field| {
                        field.spawn((
                            UserInputValue,
                            Text::new(""),
                            TextFont {
                                font: fonts.text.clone().into(),
                                font_size: FontSize::Px(39.75),
                                ..default()
                            },
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.67)),
                            TextLayout::justify(Justify::Center),
                        ));
                        field.spawn((
                            UserInputCaret,
                            Text::new("▌"),
                            TextFont {
                                font: fonts.text.clone().into(),
                                font_size: FontSize::Px(39.75),
                                ..default()
                            },
                            TextColor(Color::srgba(1.0, 1.0, 1.0, CARET_ALPHA)),
                        ));
                    });
                panel.spawn((
                    UserInputError,
                    Text::new(""),
                    TextFont {
                        font: fonts.text.clone().into(),
                        font_size: FontSize::Px(21.0),
                        ..default()
                    },
                    TextColor(Color::srgba(1.0, 0.64, 0.64, 0.9)),
                ));
                panel.spawn((
                    UserInputConfirm,
                    HoverSweep::default(),
                    Button,
                    Node {
                        min_width: Val::Px(112.5),
                        padding: UiRect::axes(Val::Px(15.0), Val::Px(2.25)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    children![
                        hover_sweep_fill(),
                        (
                            Text::new("OK"),
                            TextFont {
                                font: fonts.text.clone().into(),
                                font_size: FontSize::Px(30.0),
                                ..default()
                            },
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
                        )
                    ],
                ));
            });
        });
}

#[derive(SystemParam)]
pub(crate) struct UserInputSyncContext<'w, 's> {
    state: Res<'w, GameState>,
    real_time: Res<'w, Time<Real>>,
    blink: ResMut<'w, UserInputCaretBlink>,
    roots: Query<'w, 's, &'static mut Node, With<UserInputRoot>>,
    blurs: Query<'w, 's, &'static mut Node, (With<UserInputBlur>, Without<UserInputRoot>)>,
    titles: Query<'w, 's, &'static mut Text, (With<UserInputTitle>, Without<UserInputValue>)>,
    descriptions: UserInputDescriptionQuery<'w, 's>,
    values: UserInputValueQuery<'w, 's>,
    errors: UserInputErrorQuery<'w, 's>,
    confirm: Query<'w, 's, &'static Children, With<UserInputConfirm>>,
    texts: UserInputOtherTextQuery<'w, 's>,
}

type UserInputDescriptionQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        With<UserInputDescription>,
        Without<UserInputTitle>,
        Without<UserInputValue>,
        Without<UserInputError>,
    ),
>;

type UserInputValueQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Text, &'static mut TextColor),
    (
        With<UserInputValue>,
        Without<UserInputTitle>,
        Without<UserInputDescription>,
        Without<UserInputError>,
    ),
>;

type UserInputErrorQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        With<UserInputError>,
        Without<UserInputTitle>,
        Without<UserInputDescription>,
        Without<UserInputValue>,
    ),
>;

type UserInputOtherTextQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        Without<UserInputTitle>,
        Without<UserInputDescription>,
        Without<UserInputValue>,
        Without<UserInputError>,
    ),
>;

pub(crate) fn sync(mut context: UserInputSyncContext) {
    if !context.state.is_changed() {
        return;
    }
    if context.state.user_input.is_some() {
        context.blink.reset(context.real_time.elapsed_secs());
    } else {
        context.blink.stop();
    }
    let display = if context.state.user_input.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut root in &mut context.roots {
        root.display = display;
    }
    for mut blur in &mut context.blurs {
        blur.display = display;
    }
    let Some(input) = &context.state.user_input else {
        return;
    };
    for mut title in &mut context.titles {
        title.0.clone_from(&input.title);
    }
    for mut description in &mut context.descriptions {
        description.0.clone_from(&input.description);
    }
    let (display_value, placeholder) = match input.value_type {
        keine_core::InputValueType::Bool => (
            if input.value == "true" {
                input.true_text.as_str()
            } else {
                input.false_text.as_str()
            },
            false,
        ),
        _ if input.value.is_empty() => (input.placeholder.as_str(), true),
        _ => (input.value.as_str(), false),
    };
    for (mut value, mut color) in &mut context.values {
        value.0.clear();
        value.0.push_str(display_value);
        color.0 = if placeholder {
            Color::srgba(1.0, 1.0, 1.0, 0.32)
        } else {
            Color::srgba(1.0, 1.0, 1.0, 0.78)
        };
    }
    for mut error in &mut context.errors {
        error.0.clone_from(&input.error);
    }
    for children in &context.confirm {
        for child in children.iter() {
            if let Ok(mut text) = context.texts.get_mut(child) {
                text.0.clone_from(&input.button);
            }
        }
    }
}

pub(crate) fn animate_caret(
    real_time: Res<Time<Real>>,
    blink: Res<UserInputCaretBlink>,
    mut carets: Query<&mut TextColor, With<UserInputCaret>>,
) {
    let alpha = blink.alpha(real_time.elapsed_secs());
    for mut color in &mut carets {
        color.0 = Color::srgba(1.0, 1.0, 1.0, alpha);
    }
}

pub(crate) fn handle(
    mut commands: Commands,
    mut state: ResMut<GameState>,
    mut checkpoint: ResMut<crate::storage::save::ContinuationCheckpoint>,
    mut keyboard: MessageReader<KeyboardInput>,
    buttons: Query<&Interaction, (With<UserInputConfirm>, Changed<Interaction>)>,
) {
    if state.user_input.is_none() {
        return;
    }
    let mut submit = buttons.iter().any(|value| *value == Interaction::Pressed);
    let changed = {
        let state_value = state.bypass_change_detection();
        let input = state_value
            .user_input
            .as_mut()
            .expect("user input was checked before mutation");
        let mut changed = false;
        for event in keyboard.read() {
            if !event.state.is_pressed() {
                continue;
            }
            match &event.logical_key {
                Key::Character(value) if input.value_type != keine_core::InputValueType::Bool => {
                    let accepted = input.value_type == keine_core::InputValueType::String
                        || value.chars().all(|character| {
                            character.is_ascii_digit() || ".-+".contains(character)
                        });
                    if accepted
                        && append_with_scalar_limit(&mut input.value, value, input.max_length)
                    {
                        input.error.clear();
                        changed = true;
                    }
                }
                Key::Space if input.value_type == keine_core::InputValueType::Bool => {
                    input.value = if input.value == "true" {
                        "false"
                    } else {
                        "true"
                    }
                    .into();
                    input.error.clear();
                    changed = true;
                }
                Key::Space if input.value_type == keine_core::InputValueType::String => {
                    if !append_with_scalar_limit(&mut input.value, " ", input.max_length) {
                        continue;
                    }
                    input.error.clear();
                    changed = true;
                }
                Key::Backspace => {
                    changed |= input.value.pop().is_some();
                    input.error.clear();
                }
                Key::Enter => submit = true,
                _ => {}
            }
        }
        if submit {
            if keine_core::step::submit_user_input(state_value) {
                let outcome = crate::runtime::script_driver::resume(state_value, &mut checkpoint);
                crate::ui::title::handle_script_outcome(&mut commands, outcome);
            }
            changed = true;
        }
        changed
    };
    if changed {
        state.set_changed();
    }
}

fn append_with_scalar_limit(value: &mut String, incoming: &str, maximum: usize) -> bool {
    if incoming.is_empty() {
        return false;
    }
    if maximum == 0 {
        value.push_str(incoming);
        return true;
    }
    let remaining = maximum.saturating_sub(value.chars().count());
    let previous_bytes = value.len();
    value.extend(incoming.chars().take(remaining));
    value.len() != previous_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_blinks_on_real_time_deadlines() {
        let mut blink = UserInputCaretBlink::default();
        blink.reset(10.0);

        assert_eq!(blink.alpha(10.0), CARET_ALPHA);
        assert_eq!(blink.alpha(10.54), CARET_ALPHA);
        assert_eq!(blink.alpha(10.56), 0.0);
        assert_eq!(blink.alpha(11.01), CARET_ALPHA);
        assert!(blink.next_toggle_in(10.2) < Duration::from_millis(400));

        blink.stop();
        assert_eq!(blink.alpha(20.0), 0.0);
        assert_eq!(blink.next_toggle_in(20.0), Duration::MAX);
    }

    #[test]
    fn text_input_never_exceeds_its_unicode_scalar_limit() {
        let mut value = "你".to_string();

        assert!(append_with_scalar_limit(&mut value, "好🙂extra", 3));
        assert_eq!(value, "你好🙂");
        assert_eq!(value.chars().count(), 3);
        assert!(!append_with_scalar_limit(&mut value, "!", 3));
        assert_eq!(value, "你好🙂");
    }

    #[test]
    fn zero_text_limit_remains_unbounded() {
        let mut value = String::new();

        assert!(append_with_scalar_limit(&mut value, "你好🙂", 0));
        assert_eq!(value, "你好🙂");
    }
}
