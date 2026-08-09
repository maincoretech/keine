//! Minimal native fallback shown when a shipping package cannot be opened.
//!
//! This application deliberately does not install the game runtime. Keeping
//! the failure path independent means a broken project, script, or archive
//! can never prevent the recovery instructions themselves from rendering.

use bevy::app::AppExit;
use bevy::asset::{embedded_asset, load_embedded_asset};
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::window::{WindowResolution, WindowTheme};
use bevy::winit::WinitSettings;

const WINDOW_WIDTH: u32 = 960;
const WINDOW_HEIGHT: u32 = 540;
const CARD_WIDTH: f32 = 600.0;
const TITLE: &str = "游戏资源不可用";
const INSTRUCTION: &str = "请重新下载 game.hxz，并放到游戏可执行文件同目录";

#[derive(Component)]
struct CloseButton;

#[derive(Resource)]
struct InitialRender {
    text_font: Handle<Font>,
    icon_font: Handle<Font>,
    ready_frames: u8,
}

pub(crate) fn show() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Kēne — 无法启动".to_owned(),
            resolution: WindowResolution::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            resizable: false,
            window_theme: Some(WindowTheme::Dark),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::BLACK))
    .insert_resource(WinitSettings::continuous())
    .add_systems(Startup, setup)
    .add_systems(Update, (close, settle_after_initial_render));

    // These are registered after AssetPlugin has created the embedded source.
    // Loading through the matching macro below keeps paths correct even though
    // this module lives one directory below `src/assets`.
    embedded_asset!(&mut app, "../assets/fonts/MavenPro-CJK.ttf");
    embedded_asset!(&mut app, "../assets/fonts/bootstrap-icons.ttf");
    app.run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    let text_font = load_embedded_asset!(assets.as_ref(), "../assets/fonts/MavenPro-CJK.ttf");
    let icon_font = load_embedded_asset!(assets.as_ref(), "../assets/fonts/bootstrap-icons.ttf");

    commands.insert_resource(InitialRender {
        text_font: text_font.clone(),
        icon_font: icon_font.clone(),
        ready_frames: 0,
    });

    commands.spawn(Camera2d);
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::BLACK),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    position_type: PositionType::Relative,
                    width: Val::Px(CARD_WIDTH),
                    min_height: Val::Px(280.0),
                    padding: UiRect::axes(Val::Px(48.0), Val::Px(40.0)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.078, 0.078, 0.078, 0.95)),
            ))
            .with_children(|card| {
                card.spawn((
                    Button,
                    CloseButton,
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(12.0),
                        right: Val::Px(14.0),
                        width: Val::Px(32.0),
                        height: Val::Px(32.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_child(text("×", &text_font, 21.0, 0.34, FontWeight::NORMAL));

                card.spawn((
                    text("\u{f1c7}", &icon_font, 52.0, 0.92, FontWeight::NORMAL),
                    Node {
                        margin: UiRect::bottom(Val::Px(18.0)),
                        ..default()
                    },
                ));
                card.spawn((
                    text(TITLE, &text_font, 23.0, 1.0, FontWeight::MEDIUM),
                    Node {
                        margin: UiRect::bottom(Val::Px(10.0)),
                        ..default()
                    },
                ));
                card.spawn(text(
                    INSTRUCTION,
                    &text_font,
                    15.0,
                    0.54,
                    FontWeight::NORMAL,
                ));
            });
        });
}

fn text(
    content: impl Into<String>,
    font: &Handle<Font>,
    size: f32,
    alpha: f32,
    weight: FontWeight,
) -> impl Bundle {
    (
        Text::new(content.into()),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::from(size),
            weight,
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, alpha)),
    )
}

fn close(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<&Interaction, (Changed<Interaction>, With<CloseButton>)>,
    mut exits: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::Escape)
        || buttons
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed)
    {
        exits.write(AppExit::Success);
    }
}

fn settle_after_initial_render(
    assets: Res<AssetServer>,
    mut initial: ResMut<InitialRender>,
    mut settings: ResMut<WinitSettings>,
) {
    if !assets.is_loaded_with_dependencies(&initial.text_font)
        || !assets.is_loaded_with_dependencies(&initial.icon_font)
    {
        return;
    }
    initial.ready_frames = initial.ready_frames.saturating_add(1);
    if initial.ready_frames == 2 {
        *settings = WinitSettings::desktop_app();
    }
}
