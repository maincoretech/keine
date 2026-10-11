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
        &'static mut BoxShadow,
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
            Node {
                padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BoxShadow::new(
                Color::NONE,
                Val::ZERO,
                Val::Px(2.0),
                Val::Px(8.0),
                Val::Px(24.0),
            ),
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
        let display = if intro.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (mut text, mut color) in &mut intro_texts {
        let Some(intro) = intro else {
            if !text.0.is_empty() {
                text.0.clear();
            }
            continue;
        };
        let page = intro
            .pages
            .get(intro.page)
            .map(String::as_str)
            .unwrap_or_default();
        if text.0 != page {
            text.0.clear();
            text.0.push_str(page);
        }
        let alpha = (intro.elapsed / 0.22).clamp(0.0, 1.0);
        color.set_if_neq(TextColor(Color::srgba(1.0, 1.0, 1.0, alpha)));
    }
    for mut node in &mut film_bars {
        let display = if state.film_mode {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (mut node, mut background) in &mut curtains {
        let [red, green, blue, alpha] = state.curtain.color;
        let alpha = alpha * state.curtain.current;
        let display = if alpha > 0.001 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        background.set_if_neq(BackgroundColor(Color::srgba(red, green, blue, alpha)));
    }
    for (mut node, mut transform) in &mut floating_roots {
        let Some(floating) = &state.floating_text else {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        if node.display != Display::Flex {
            node.display = Display::Flex;
        }
        let translation = Val2::px(
            floating.position[0] - DESIGN_WIDTH * 0.5,
            floating.position[1] - DESIGN_HEIGHT * 0.5,
        );
        if transform.translation != translation {
            transform.translation = translation;
        }
    }
    for (mut text, mut font, mut color, mut shadow) in &mut floating_values {
        let Some(floating) = &state.floating_text else {
            if !text.0.is_empty() {
                text.0.clear();
            }
            continue;
        };
        if text.0 != floating.text {
            text.0.clone_from(&floating.text);
        }
        let size = FontSize::Px(floating.font_size.max(1.0));
        if font.font_size != size {
            font.font_size = size;
        }
        let [red, green, blue, alpha] = floating.color;
        let opacity = alpha * floating.alpha();
        color.set_if_neq(TextColor(Color::srgba(red, green, blue, opacity)));
        let shadow_color = Color::srgba(0.0, 0.0, 0.0, 0.55 * opacity);
        if shadow.0.iter().any(|style| style.color != shadow_color) {
            for style in &mut shadow.0 {
                style.color = shadow_color;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keine_core::state::{FloatingTextState, IntroState};

    #[derive(Resource, Default, Debug, PartialEq)]
    struct Changes {
        text: usize,
        font: usize,
        color: usize,
        node: usize,
        position: usize,
        shadow: usize,
    }

    fn record(
        text: Query<(), Changed<Text>>,
        font: Query<(), Changed<TextFont>>,
        color: Query<(), Changed<TextColor>>,
        node: Query<(), Changed<Node>>,
        position: Query<(), Changed<UiTransform>>,
        shadow: Query<(), Changed<BoxShadow>>,
        mut changes: ResMut<Changes>,
    ) {
        *changes = Changes {
            text: text.iter().count(),
            font: font.iter().count(),
            color: color.iter().count(),
            node: node.iter().count(),
            position: position.iter().count(),
            shadow: shadow.iter().count(),
        };
    }

    #[test]
    fn presentation_fades_do_not_relayout_unchanged_text() {
        let mut state = keine_core::State::new();
        state.intro = Some(IntroState {
            pages: vec!["first".into(), "second".into()],
            page: 0,
            elapsed: 0.05,
            hold: true,
            blocking: false,
        });
        state.floating_text = Some(FloatingTextState {
            id: None,
            text: "floating".into(),
            position: [960.0, 400.0],
            font_size: 38.0,
            color: [1.0; 4],
            fade_in: 1.0,
            hold: 1.0,
            fade_out: 1.0,
            elapsed: 0.1,
            blocking: false,
            infinite: false,
        });
        let mut app = App::new();
        app.insert_resource(GameState(state))
            .init_resource::<Changes>()
            .add_systems(Update, (sync, record).chain());
        app.world_mut().spawn((IntroRoot, Node::default()));
        let intro = app
            .world_mut()
            .spawn((IntroText, Text::default(), TextColor::default()))
            .id();
        let root = app
            .world_mut()
            .spawn((FloatingTextRoot, Node::default(), UiTransform::default()))
            .id();
        let floating = app
            .world_mut()
            .spawn((
                FloatingTextValue,
                Text::default(),
                TextFont::default(),
                TextColor::default(),
                BoxShadow(vec![ShadowStyle::default()]),
            ))
            .id();
        app.update();

        app.world_mut()
            .resource_mut::<GameState>()
            .invalidate_stage();
        app.update();
        assert_eq!(*app.world().resource::<Changes>(), Changes::default());

        {
            let mut state = app.world_mut().resource_mut::<GameState>();
            state.intro.as_mut().unwrap().elapsed = 0.1;
            state.floating_text.as_mut().unwrap().elapsed = 0.2;
        }
        app.update();
        assert_eq!(
            *app.world().resource::<Changes>(),
            Changes {
                color: 2,
                shadow: 1,
                ..default()
            }
        );

        {
            let mut state = app.world_mut().resource_mut::<GameState>();
            state.intro.as_mut().unwrap().page = 1;
            let floating = state.floating_text.as_mut().unwrap();
            floating.text = "changed".into();
            floating.font_size = 42.0;
            floating.position[0] += 20.0;
        }
        app.update();
        let changes = app.world().resource::<Changes>();
        assert_eq!(changes.text, 2);
        assert_eq!(changes.font, 1);
        assert_eq!(app.world().get::<Text>(intro).unwrap().0, "second");
        assert_eq!(app.world().get::<Text>(floating).unwrap().0, "changed");
        assert_eq!(
            app.world().get::<UiTransform>(root).unwrap().translation,
            Val2::px(20.0, -140.0)
        );

        {
            let mut state = app.world_mut().resource_mut::<GameState>();
            state.intro = None;
            state.floating_text = None;
        }
        app.update();
        assert_eq!(
            app.world().get::<Node>(root).unwrap().display,
            Display::None
        );
        assert!(app.world().get::<Text>(floating).unwrap().0.is_empty());
        app.world_mut()
            .resource_mut::<GameState>()
            .invalidate_stage();
        app.update();
        assert_eq!(*app.world().resource::<Changes>(), Changes::default());
    }
}
