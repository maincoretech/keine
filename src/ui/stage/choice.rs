use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use keine_core::{MenuState, step};

use crate::render::blur::UiBlurCamera;
use crate::runtime::resources::GameState;
use crate::ui::control_bar::{BlurSource, BlurStrength};
use crate::ui::foundation::{UiFonts, button_surface_over_dark, exp_lerp, text};

const NORMAL_ALPHA: f32 = 0.44;
const BORDER_ALPHA: f32 = 0.14;

#[derive(Component)]
pub(crate) struct ChoiceRoot {
    menu: MenuState,
    selected: usize,
    /// The key/click that advanced into a choice must not immediately confirm
    /// its first option in the same frame.
    input_armed: bool,
}

impl ChoiceRoot {
    pub(crate) fn selected(&self) -> usize {
        self.selected
    }
}

#[derive(Component)]
pub(crate) struct ChoiceButton {
    index: usize,
    enabled: bool,
    highlight: f32,
}

impl ChoiceButton {
    pub(crate) fn is_animating(&self, interaction: Interaction, selected: usize) -> bool {
        let target = if self.enabled
            && (self.index == selected
                || matches!(interaction, Interaction::Hovered | Interaction::Pressed))
        {
            1.0
        } else {
            0.0
        };
        (self.highlight - target).abs() > 0.001
    }
}

pub fn sync_choice(
    mut commands: Commands,
    state: Res<GameState>,
    roots: Query<(Entity, &ChoiceRoot)>,
    fonts: Res<UiFonts>,
    ui_camera_query: Query<Entity, With<UiBlurCamera>>,
) {
    let current_menu = state.menu.as_ref();
    if roots
        .iter()
        .any(|(_, root)| current_menu.is_some_and(|menu| root.menu == *menu))
    {
        return;
    }

    for (entity, _) in &roots {
        commands.entity(entity).despawn();
    }

    let Some(menu) = current_menu.cloned() else {
        return;
    };
    let Ok(ui_camera) = ui_camera_query.single() else {
        log::error!("choice UI requires exactly one UI camera");
        return;
    };

    let font = fonts.text.clone();
    commands
        .spawn((
            Name::new("choice_overlay"),
            ChoiceRoot {
                menu: menu.clone(),
                selected: menu
                    .choices
                    .iter()
                    .position(|choice| choice.enabled)
                    .unwrap_or(0),
                input_armed: false,
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            FocusPolicy::Block,
            BackgroundColor(Color::NONE),
            ZIndex(150),
            UiTargetCamera(ui_camera),
            RenderLayers::layer(1),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Name::new("choice_list"),
                    Node {
                        width: Val::Percent(56.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Stretch,
                        row_gap: Val::Px(12.0),
                        ..default()
                    },
                ))
                .with_children(|list| {
                    for (index, choice) in menu.choices.iter().enumerate() {
                        list.spawn((
                            Name::new(format!("choice::{index}")),
                            Button,
                            BlurSource,
                            BlurStrength(31.5),
                            ChoiceButton {
                                index,
                                enabled: choice.enabled,
                                highlight: 0.0,
                            },
                            Node {
                                width: Val::Percent(100.0),
                                padding: UiRect::axes(Val::Px(24.0), Val::Px(10.5)),
                                border: UiRect::all(Val::Px(2.25)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(if choice.enabled {
                                Color::srgba(0.0, 0.0, 0.0, NORMAL_ALPHA)
                            } else {
                                Color::srgba(1.0, 1.0, 1.0, NORMAL_ALPHA)
                            }),
                            BorderColor::all(Color::NONE),
                            BoxShadow::new(
                                Color::srgba(0.0, 0.0, 0.0, 0.25),
                                Val::Px(0.0),
                                Val::Px(0.0),
                                Val::Px(0.0),
                                Val::Px(18.75),
                            ),
                        ))
                        .with_child((
                            text(
                                choice.text.clone(),
                                &font,
                                48.0,
                                if choice.enabled { 0.88 } else { 0.45 },
                            ),
                            TextLayout::justify(Justify::Center),
                        ));
                    }
                });
        });
}

pub fn handle_choice_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GameState>,
    mut root_query: Query<(Entity, &mut ChoiceRoot)>,
    interactions: Query<(&Interaction, &ChoiceButton), Changed<Interaction>>,
) {
    let Ok((root_entity, mut root)) = root_query.single_mut() else {
        return;
    };
    let choice_count = root.menu.choices.len();
    if choice_count == 0 {
        return;
    }
    if !root.input_armed {
        root.input_armed = true;
        return;
    }

    let mut confirmed = None;
    for (interaction, button) in &interactions {
        if !button.enabled {
            continue;
        }
        match interaction {
            Interaction::Hovered => root.selected = button.index,
            Interaction::Pressed => {
                root.selected = button.index;
                confirmed = Some(button.index);
            }
            Interaction::None => {}
        }
    }

    if keys.just_pressed(KeyCode::ArrowUp)
        && let Some(index) = next_enabled_choice(&root.menu, root.selected, -1)
    {
        root.selected = index;
    }
    if keys.just_pressed(KeyCode::ArrowDown)
        && let Some(index) = next_enabled_choice(&root.menu, root.selected, 1)
    {
        root.selected = index;
    }
    if let Some(index) = numeric_choice(&keys).filter(|index| {
        *index < choice_count
            && root
                .menu
                .choices
                .get(*index)
                .is_some_and(|choice| choice.enabled)
    }) {
        root.selected = index;
        confirmed = Some(index);
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
        confirmed = Some(root.selected);
    }

    let Some(index) = confirmed else {
        return;
    };
    step::select_choice(&mut state, index);
    if state.menu.is_some() {
        return;
    }
    step::step(&mut state);
    commands.entity(root_entity).despawn();
}

pub fn animate_choice_buttons(
    time: Res<Time>,
    root_query: Query<&ChoiceRoot>,
    mut buttons: Query<(
        &Interaction,
        &mut ChoiceButton,
        &Children,
        &mut BackgroundColor,
        &mut BorderColor,
        &mut BoxShadow,
    )>,
    mut texts: Query<&mut TextColor>,
) {
    let Ok(root) = root_query.single() else {
        return;
    };

    for (interaction, mut button, children, mut background, mut border, mut shadow) in &mut buttons
    {
        let target = if button.enabled
            && (button.index == root.selected
                || matches!(interaction, Interaction::Hovered | Interaction::Pressed))
        {
            1.0
        } else {
            0.0
        };
        button.highlight += (target - button.highlight) * exp_lerp(time.delta_secs(), 10.0);
        let highlight = button.highlight;
        background.0 = if button.enabled {
            button_surface_over_dark(NORMAL_ALPHA, highlight)
        } else {
            Color::srgba(1.0, 1.0, 1.0, NORMAL_ALPHA)
        };
        *border = BorderColor::all(Color::srgba(1.0, 1.0, 1.0, BORDER_ALPHA * highlight));
        if let Some(style) = shadow.0.first_mut() {
            style.color = Color::srgba(0.0, 0.0, 0.0, 0.25 + 0.25 * highlight);
        }
        for child in children.iter() {
            if let Ok(mut color) = texts.get_mut(child) {
                let text_alpha = if button.enabled {
                    0.88 + 0.08 * highlight
                } else {
                    0.38
                };
                color.0 = Color::srgba(1.0, 1.0, 1.0, text_alpha);
            }
        }
    }
}

fn next_enabled_choice(menu: &MenuState, selected: usize, direction: isize) -> Option<usize> {
    let count = menu.choices.len();
    (1..=count)
        .map(|offset| {
            (selected as isize + direction * offset as isize).rem_euclid(count as isize) as usize
        })
        .find(|index| menu.choices[*index].enabled)
}

fn numeric_choice(keys: &ButtonInput<KeyCode>) -> Option<usize> {
    [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ]
    .into_iter()
    .position(|key| keys.just_pressed(key))
}
