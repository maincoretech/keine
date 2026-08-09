use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use keine_core::{DESIGN_HEIGHT, DESIGN_WIDTH};

use crate::render::blur::DialogCamera;
use crate::runtime::resources::GameState;
use crate::ui::foundation::UiFonts;

#[derive(Component)]
pub(crate) struct IntroRoot;

#[derive(Component)]
pub(crate) struct IntroText;

#[derive(Component)]
pub(crate) struct AdvanceHint;

#[derive(Resource, Default)]
pub(crate) struct AdvanceHintState {
    video_revision: Option<u64>,
    video_armed: bool,
}

#[derive(Component)]
pub(crate) struct FilmBar;

#[derive(Component)]
pub(crate) struct CurtainRoot;

#[derive(Component)]
pub(crate) struct FloatingTextRoot;

#[derive(Component)]
pub(crate) struct FloatingTextValue;

type CurtainQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Node, &'static mut BackgroundColor),
    (With<CurtainRoot>, Without<IntroRoot>, Without<FilmBar>),
>;
type FloatingRootQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Node, &'static mut UiTransform),
    (
        With<FloatingTextRoot>,
        Without<IntroRoot>,
        Without<FilmBar>,
        Without<CurtainRoot>,
    ),
>;
type FloatingValueQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Text,
        &'static mut TextFont,
        &'static mut TextColor,
    ),
    (With<FloatingTextValue>, Without<IntroText>),
>;

pub(crate) fn ensure_spawned(
    mut commands: Commands,
    state: Res<GameState>,
    fonts: Res<UiFonts>,
    cameras: Query<Entity, With<DialogCamera>>,
    existing: Query<(), With<IntroRoot>>,
) {
    let has_waiting_video = state.videos.values().any(|video| {
        video.spec.skippable
            && video.spec.wait_for_finished
            && !video.spec.looped
            && !video.stopping
    });
    let needed = state.intro.is_some()
        || state.film_mode
        || state.curtain.current > 0.001
        || state.floating_text.is_some()
        || has_waiting_video;
    if !needed || !existing.is_empty() {
        return;
    }
    let Ok(camera) = cameras.single() else {
        return;
    };
    commands
        .spawn((
            Name::new("intro_overlay"),
            IntroRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(DESIGN_WIDTH),
                height: Val::Px(DESIGN_HEIGHT),
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(Val::Px(180.0)),
                ..default()
            },
            BackgroundColor(Color::BLACK),
            FocusPolicy::Pass,
            UiTargetCamera(camera),
            RenderLayers::layer(2),
            ZIndex(900),
        ))
        .with_child((
            IntroText,
            Text::new(""),
            TextFont {
                font: FontSource::Handle(fonts.text.clone()),
                font_size: FontSize::Px(40.5),
                ..default()
            },
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Center),
        ));

    for (name, top) in [("film_bar_top", true), ("film_bar_bottom", false)] {
        commands.spawn((
            Name::new(name),
            FilmBar,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(DESIGN_WIDTH),
                height: Val::Px(69.0),
                top: if top { Val::Px(0.0) } else { Val::Auto },
                bottom: if top { Val::Auto } else { Val::Px(0.0) },
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::BLACK),
            FocusPolicy::Pass,
            UiTargetCamera(camera),
            RenderLayers::layer(2),
            ZIndex(800),
        ));
    }

    commands.spawn((
        Name::new("curtain_overlay"),
        CurtainRoot,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(DESIGN_WIDTH),
            height: Val::Px(DESIGN_HEIGHT),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::NONE),
        FocusPolicy::Pass,
        UiTargetCamera(camera),
        RenderLayers::layer(2),
        ZIndex(850),
    ));

    commands
        .spawn((
            Name::new("floating_text_overlay"),
            FloatingTextRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(DESIGN_WIDTH),
                height: Val::Px(DESIGN_HEIGHT),
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            UiTransform::default(),
            FocusPolicy::Pass,
            UiTargetCamera(camera),
            RenderLayers::layer(2),
            ZIndex(875),
        ))
        .with_child((
            FloatingTextValue,
            Text::new(""),
            TextFont {
                font: FontSource::Handle(fonts.text.clone()),
                font_size: FontSize::Px(50.0),
                ..default()
            },
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Center),
        ));

    commands.spawn((
        Name::new("advance_hint"),
        AdvanceHint,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(18.0),
            height: Val::Px(18.0),
            right: Val::Px(54.0),
            top: Val::Px(48.0),
            display: Display::None,
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Percent(50.0)),
            ..default()
        },
        BorderColor::all(Color::WHITE),
        BackgroundColor(Color::NONE),
        FocusPolicy::Pass,
        UiTargetCamera(camera),
        RenderLayers::layer(2),
        ZIndex(910),
    ));
}

pub(crate) fn animate_advance_hint(
    time: Res<Time<Real>>,
    state: Res<GameState>,
    actions: Res<crate::runtime::platform::InputActions>,
    mut hint_state: ResMut<AdvanceHintState>,
    mut hints: Query<(&mut Node, &mut BorderColor), With<AdvanceHint>>,
) {
    let video_revision = state
        .videos
        .values()
        .filter(|video| {
            video.spec.skippable
                && video.spec.wait_for_finished
                && !video.spec.looped
                && !video.stopping
        })
        .map(|video| video.revision)
        .max();
    if hint_state.video_revision != video_revision {
        hint_state.video_revision = video_revision;
        hint_state.video_armed = false;
    }
    if video_revision.is_some() && actions.pointer_advance && !actions.skip_video {
        hint_state.video_armed = true;
    }

    let intro_ready = state
        .intro
        .as_ref()
        .is_some_and(|intro| intro.hold && intro.elapsed >= 0.9);
    let visible = intro_ready || (video_revision.is_some() && hint_state.video_armed);
    if !visible && hints.iter().all(|(node, _)| node.display == Display::None) {
        return;
    }
    let pulse = 0.38 + 0.58 * (time.elapsed_secs() * 3.8).sin().abs();
    for (mut node, mut border) in &mut hints {
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if visible {
            *border = BorderColor::all(Color::srgba(1.0, 1.0, 1.0, pulse));
        }
    }
}

pub(crate) fn sync(
    state: Res<GameState>,
    mut intro_roots: Query<&mut Node, (With<IntroRoot>, Without<FilmBar>)>,
    mut intro_texts: Query<(&mut Text, &mut TextColor), With<IntroText>>,
    mut film_bars: Query<&mut Node, (With<FilmBar>, Without<IntroRoot>)>,
    mut curtains: CurtainQuery,
    mut floating_roots: FloatingRootQuery,
    mut floating_values: FloatingValueQuery,
) {
    if !state.is_changed() {
        return;
    }
    let intro = state.intro.as_ref();
    for mut node in &mut intro_roots {
        node.display = if intro.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (mut text, mut color) in &mut intro_texts {
        let Some(intro) = intro else {
            text.0.clear();
            continue;
        };
        text.0 = intro.pages.get(intro.page).cloned().unwrap_or_default();
        let alpha = (intro.elapsed / 0.22).clamp(0.0, 1.0);
        color.0 = Color::srgba(1.0, 1.0, 1.0, alpha);
    }
    for mut node in &mut film_bars {
        node.display = if state.film_mode {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (mut node, mut background) in &mut curtains {
        let [red, green, blue, alpha] = state.curtain.color;
        let alpha = alpha * state.curtain.current;
        node.display = if alpha > 0.001 {
            Display::Flex
        } else {
            Display::None
        };
        background.0 = Color::srgba(red, green, blue, alpha);
    }
    for (mut node, mut transform) in &mut floating_roots {
        let Some(floating) = &state.floating_text else {
            node.display = Display::None;
            continue;
        };
        node.display = Display::Flex;
        transform.translation = Val2::px(
            floating.position[0] - DESIGN_WIDTH * 0.5,
            floating.position[1] - DESIGN_HEIGHT * 0.5,
        );
    }
    for (mut text, mut font, mut color) in &mut floating_values {
        let Some(floating) = &state.floating_text else {
            text.0.clear();
            continue;
        };
        text.0.clone_from(&floating.text);
        font.font_size = FontSize::Px(floating.font_size.max(1.0));
        let [red, green, blue, alpha] = floating.color;
        color.0 = Color::srgba(red, green, blue, alpha * floating.alpha());
    }
}
