//! Confirmation dialog view.
use super::*;

/// Spawn the dialog overlay + centred box when DialogRequest is present.
pub fn spawn_dialog(
    mut commands: Commands,
    dialog_q: Query<Entity, With<DialogRoot>>,
    request: Option<Res<DialogRequest>>,
    fonts: Res<UiFonts>,
    settings: Res<RuntimeSettings>,
    dialog_camera_q: Query<Entity, With<DialogCamera>>,
) {
    // Remove existing dialog when request is gone
    if request
        .as_ref()
        .is_some_and(|request| !request.is_changed())
        && !dialog_q.is_empty()
    {
        return;
    }

    // Clear old dialog
    for e in dialog_q.iter() {
        commands.entity(e).despawn();
    }

    let Some(req) = request else { return };
    let Ok(dialog_camera) = dialog_camera_q.single() else {
        return;
    };

    let font = fonts.text.clone();

    commands
        .spawn((
            Name::new("dialog_overlay"),
            DialogRoot,
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
            BackgroundColor(Color::NONE),
            DialogFade(0.0),
            DialogBackground {
                alpha: OVERLAY_ALPHA,
            },
            FocusPolicy::Block,
            GlobalZIndex(200),
            UiTargetCamera(dialog_camera),
            RenderLayers::layer(2),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(20.0),
                    border: UiRect::top(Val::Px(11.25)),
                    ..default()
                },
                BorderColor::all(Color::NONE),
                DialogBorder { alpha: 0.19 },
            ))
            .with_child((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(60.0), Val::Px(24.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                DialogBackground { alpha: PANEL_ALPHA },
                children![
                    // Title
                    dialog_text(
                        if req.message.is_empty() {
                            req.title.clone()
                        } else {
                            format!("{}\n{}", req.title, req.message)
                        },
                        font.clone(),
                        48.0,
                        0.9,
                    ),
                    // Button row — wide spacing
                    (
                        Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(60.0),
                            ..default()
                        },
                        children![
                            spawn_dialog_button(
                                DialogButton::Confirm,
                                req.confirm_text.clone().unwrap_or_else(|| tr(
                                    settings.locale,
                                    UiText::Confirm
                                )
                                .into()),
                                font.clone(),
                                true,
                            ),
                            spawn_dialog_button(
                                DialogButton::Cancel,
                                req.cancel_text.clone().unwrap_or_else(|| tr(
                                    settings.locale,
                                    UiText::Cancel
                                )
                                .into()),
                                font,
                                req.cancel_text.is_some() || req.confirm_text.is_none(),
                            ),
                        ],
                    ),
                ],
            ));
        });
}

fn spawn_dialog_button(
    action: DialogButton,
    text: impl Into<String>,
    font: Handle<Font>,
    visible: bool,
) -> impl Bundle {
    (
        Button,
        UiSoundStyle::Click,
        action,
        HoverSweep::default(),
        Node {
            display: if visible {
                Display::Flex
            } else {
                Display::None
            },
            min_width: Val::Px(112.5),
            padding: UiRect::axes(Val::Px(24.0), Val::Px(6.0)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![hover_sweep_fill(), dialog_text(text, font, 31.5, 0.67)],
    )
}

fn dialog_text(
    content: impl Into<String>,
    font: Handle<Font>,
    size: f32,
    alpha: f32,
) -> impl Bundle {
    (
        Text::new(content.into()),
        TextFont {
            font: font.into(),
            font_size: FontSize::from(size),
            ..default()
        },
        TextColor(Color::NONE),
        DialogText { alpha },
    )
}
