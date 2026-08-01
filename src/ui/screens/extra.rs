use std::{collections::HashMap, time::Duration};

#[cfg(feature = "audio-opus")]
use bevy::audio::AudioPlayer;
use bevy::audio::{AudioSink, AudioSinkPlayback, PlaybackMode, Volume};
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use crate::render::blur::{DialogCamera, UiBlurCamera};
use crate::runtime::resources::{GameConfigResource, GameState};
use crate::scene::audio::BgmPlayer;
use crate::storage::settings::RuntimeSettings;
use crate::ui::control_bar::{BlurStrength, HoverAlpha, UiBlurSource};
use crate::ui::foundation::{
    SURFACE_ACTIVE_ALPHA, SURFACE_HOVER_ALPHA, SURFACE_IDLE_ALPHA, SURFACE_PANEL_ALPHA,
    UI_MOTION_RATE, UiFonts, UiSoundStyle, button_surface, dark_surface, exp_lerp, smoothstep,
    text, text_weight,
};
use crate::ui::settings_panel::{SettingsWatermark, menu_watermark};
use crate::ui::support::i18n::{LocalizedText, UiText};
use keine_core::{DESIGN_HEIGHT, DESIGN_WIDTH};

const CG_PER_PAGE: usize = 8;
const EXTRA_PANEL_PADDING: f32 = 24.0;
const EXTRA_CONTROL_MARGIN: f32 = 4.5;
const EXTRA_SECTION_GAP: f32 = 9.0;
const BGM_PROGRESS_INSET: f32 = 18.0;

#[derive(Resource)]
pub(crate) struct ExtraUi {
    pub(crate) open: bool,
    page: usize,
    selected_bgm: Option<String>,
}

impl Default for ExtraUi {
    fn default() -> Self {
        Self {
            open: false,
            page: 1,
            selected_bgm: None,
        }
    }
}

pub(crate) fn active(ui: Res<ExtraUi>, roots: Query<(), With<ExtraRoot>>) -> bool {
    ui.open || !roots.is_empty()
}

#[derive(Component)]
pub(crate) struct ExtraRoot;

#[derive(Component)]
pub(crate) struct ExtraBlurProxy;

#[derive(Component)]
pub(crate) struct ExtraWatermark;

#[derive(Component)]
pub(crate) struct ExtraClose;

#[derive(Component)]
pub(crate) struct ExtraPage(usize);

#[derive(Component)]
pub(crate) struct ExtraCg(String);

#[derive(Component)]
pub(crate) struct ExtraCgGrid;

#[derive(Component)]
pub(crate) struct ExtraFullCg(String);

#[derive(Component, Clone, Copy)]
pub(crate) enum ExtraCgControl {
    Previous,
    Next,
    Close,
}

#[derive(Component)]
pub(crate) struct ExtraBgm(String);

#[derive(Component)]
pub(crate) struct ExtraBgmPlayer {
    duration: Option<Duration>,
    observed_audio: bool,
}

#[derive(Component)]
pub(crate) struct ExtraBgmProgress;

#[derive(Component)]
pub(crate) struct ExtraBgmTime;

#[derive(Component)]
pub(crate) struct ExtraBgmName;

#[derive(Component)]
pub(crate) struct ExtraBgmProgressThumb;

#[derive(Component)]
pub(crate) struct ExtraBgmPlayIcon;

#[derive(Component, Default)]
pub(crate) struct ExtraBgmSeekBar {
    dragging: bool,
    preview: Option<Duration>,
}

impl ExtraBgmSeekBar {
    pub(crate) fn is_dragging(&self) -> bool {
        self.dragging
    }

    fn reset(&mut self) {
        self.dragging = false;
        self.preview = None;
    }
}

#[derive(Component, Clone, Copy)]
pub(crate) enum ExtraBgmControl {
    Previous,
    Play,
    Next,
    Stop,
}

#[derive(Component)]
pub(crate) struct ExtraMotion {
    current: f32,
    target: f32,
}

impl ExtraMotion {
    pub(crate) fn is_animating(&self) -> bool {
        (self.current - self.target).abs() > 0.001
    }
}

#[derive(Default)]
pub(crate) struct ExtraFadeCache {
    root: Option<Entity>,
    text: HashMap<Entity, f32>,
    background: HashMap<Entity, f32>,
    image: HashMap<Entity, f32>,
}

impl ExtraFadeCache {
    fn clear(&mut self) {
        self.root = None;
        self.text.clear();
        self.background.clear();
        self.image.clear();
    }
}

type ExtraBackgroundQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut BackgroundColor),
    (Without<ExtraBlurProxy>, Without<ExtraRoot>),
>;

#[derive(SystemParam)]
pub(crate) struct ExtraFadeContext<'w, 's> {
    parents: Query<'w, 's, &'static ChildOf>,
    texts: Query<'w, 's, (Entity, &'static mut TextColor)>,
    backgrounds: ExtraBackgroundQuery<'w, 's>,
    images: Query<'w, 's, (Entity, &'static mut ImageNode)>,
}

#[derive(SystemParam)]
pub(crate) struct ExtraAnimationContext<'w, 's> {
    roots: Query<
        'w,
        's,
        (Entity, &'static mut ExtraMotion, &'static mut UiTransform),
        With<ExtraRoot>,
    >,
    proxies: Query<
        'w,
        's,
        (
            Entity,
            &'static mut BlurStrength,
            &'static mut BackgroundColor,
        ),
        With<ExtraBlurProxy>,
    >,
    watermarks: Query<'w, 's, &'static mut SettingsWatermark, With<ExtraWatermark>>,
    players: Query<'w, 's, Entity, With<ExtraBgmPlayer>>,
    stage_bgm: Query<'w, 's, &'static AudioSink, (With<BgmPlayer>, Without<ExtraBgmPlayer>)>,
    fade: ExtraFadeContext<'w, 's>,
}

fn hover_surface(idle: f32, selected: bool) -> HoverAlpha {
    let resting = if selected { SURFACE_ACTIVE_ALPHA } else { idle };
    HoverAlpha {
        target: resting,
        current: resting,
        idle_alpha: idle,
        active: selected,
        active_alpha: SURFACE_ACTIVE_ALPHA,
        hover_alpha: SURFACE_HOVER_ALPHA,
    }
}

#[derive(SystemParam)]
pub(crate) struct ExtraSyncContext<'w, 's> {
    commands: Commands<'w, 's>,
    ui: ResMut<'w, ExtraUi>,
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    fonts: Res<'w, UiFonts>,
    assets: Res<'w, AssetServer>,
    ui_camera: Query<'w, 's, Entity, With<UiBlurCamera>>,
    dialog_camera: Query<'w, 's, Entity, With<DialogCamera>>,
    roots: Query<'w, 's, &'static mut ExtraMotion, With<ExtraRoot>>,
    proxies: Query<'w, 's, Entity, With<ExtraBlurProxy>>,
}

pub(crate) fn sync(mut context: ExtraSyncContext) {
    if !context.config.features.extra {
        context.ui.open = false;
    }
    if !context.ui.open {
        for mut motion in &mut context.roots {
            motion.target = 0.0;
        }
        return;
    }
    if !context.roots.is_empty() {
        return;
    }
    let (Ok(ui_camera), Ok(dialog_camera)) =
        (context.ui_camera.single(), context.dialog_camera.single())
    else {
        return;
    };
    if context.proxies.is_empty() {
        context
            .commands
            .spawn((
                ExtraBlurProxy,
                UiBlurSource,
                BlurStrength(0.0),
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(DESIGN_WIDTH),
                    height: Val::Px(DESIGN_HEIGHT),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                FocusPolicy::Pass,
                GlobalZIndex(171),
                UiTargetCamera(ui_camera),
                RenderLayers::layer(1),
            ))
            .with_children(|proxy| {
                proxy.spawn((ExtraWatermark, menu_watermark("EXTRA", &context.fonts.text)));
            });
    }

    let mut cg = context
        .state
        .unlocked_cg
        .iter()
        .map(|(file, name)| (file.clone(), name.clone()))
        .collect::<Vec<_>>();
    cg.sort_unstable_by(|left, right| left.1.cmp(&right.1));
    let mut bgm = context
        .state
        .unlocked_bgm
        .iter()
        .map(|(file, name)| (file.clone(), name.clone()))
        .collect::<Vec<_>>();
    bgm.sort_unstable_by(|left, right| left.1.cmp(&right.1));

    context
        .commands
        .spawn((
            ExtraRoot,
            ExtraMotion {
                current: 0.0,
                target: 1.0,
            },
            UiTransform::from_translation(Val2::px(0.0, 9.0)),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(DESIGN_WIDTH),
                height: Val::Px(DESIGN_HEIGHT),
                padding: UiRect::all(Val::Px(24.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            FocusPolicy::Block,
            GlobalZIndex(172),
            UiTargetCamera(dialog_camera),
            RenderLayers::layer(2),
        ))
        .with_children(|root| {
            spawn_header(root, &context.fonts);
            root.spawn((Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(3.0),
                top: Val::Percent(12.0),
                width: Val::Percent(94.0),
                height: Val::Percent(84.0),
                display: Display::Grid,
                grid_template_columns: vec![GridTrack::flex(1.0), GridTrack::flex(1.0)],
                column_gap: Val::Px(24.0),
                ..default()
            },))
                .with_children(|body| {
                    spawn_bgm_panel(body, &bgm, &context.ui, &context.fonts);
                    spawn_cg_panel(
                        body,
                        &cg,
                        &context.ui,
                        &context.config,
                        &context.fonts,
                        &context.assets,
                    );
                });
        });
}

fn spawn_header(root: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    root.spawn((Node {
        width: Val::Percent(100.0),
        height: Val::Percent(8.0),
        padding: UiRect::horizontal(Val::Px(24.0)),
        margin: UiRect::bottom(Val::Px(EXTRA_SECTION_GAP)),
        justify_content: JustifyContent::FlexEnd,
        align_items: AlignItems::FlexStart,
        ..default()
    },))
        .with_children(|header| {
            header
                .spawn((
                    Button,
                    UiSoundStyle::Click,
                    ExtraClose,
                    HoverAlpha::default(),
                    Node {
                        min_width: Val::Px(112.5),
                        height: Val::Percent(100.0),
                        padding: UiRect::horizontal(Val::Px(21.0)),
                        margin: UiRect::horizontal(Val::Px(EXTRA_CONTROL_MARGIN)),
                        column_gap: Val::Px(7.5),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|button| {
                    button.spawn(text("\u{f1c3}", &fonts.icons, 21.0, 0.82));
                    button.spawn((
                        LocalizedText(UiText::Back),
                        text("BACK", &fonts.text, 21.0, 0.82),
                    ));
                });
        });
}

fn spawn_bgm_panel(
    body: &mut ChildSpawnerCommands,
    tracks: &[(String, String)],
    ui: &ExtraUi,
    fonts: &UiFonts,
) {
    body.spawn((
        Node {
            min_width: Val::Px(0.0),
            height: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(EXTRA_PANEL_PADDING)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(EXTRA_SECTION_GAP),
            ..default()
        },
        BackgroundColor(dark_surface(SURFACE_PANEL_ALPHA)),
    ))
    .with_children(|panel| {
        panel.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(48.0),
                align_items: AlignItems::Center,
                ..default()
            },
            children![text_weight(
                "BGM",
                &fonts.text,
                24.0,
                0.72,
                bevy::text::FontWeight::BOLD,
            )],
        ));
        panel
            .spawn((Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                align_content: AlignContent::FlexStart,
                overflow: Overflow::clip_y(),
                ..default()
            },))
            .with_children(|list| {
                if tracks.is_empty() {
                    list.spawn(text("NO BGM", &fonts.text, 22.5, 0.36));
                }
                for (file, name) in tracks {
                    let active = ui.selected_bgm.as_ref() == Some(file);
                    list.spawn((
                        Button,
                        UiSoundStyle::Switch,
                        ExtraBgm(file.clone()),
                        hover_surface(0.0, active),
                        Node {
                            width: Val::Percent(48.0),
                            padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
                            margin: UiRect::all(Val::Px(EXTRA_CONTROL_MARGIN)),
                            ..default()
                        },
                        BackgroundColor(button_surface(if active {
                            SURFACE_ACTIVE_ALPHA
                        } else {
                            0.0
                        })),
                        children![text(name.clone(), &fonts.text, 18.75, 0.8)],
                    ));
                }
            });
        panel
            .spawn((Node {
                width: Val::Percent(100.0),
                height: Val::Px(30.0),
                margin: UiRect::vertical(Val::Px(EXTRA_CONTROL_MARGIN)),
                padding: UiRect::horizontal(Val::Px(BGM_PROGRESS_INSET)),
                column_gap: Val::Px(12.0),
                align_items: AlignItems::Center,
                ..default()
            },))
            .with_children(|progress| {
                progress
                    .spawn((
                        Button,
                        ExtraBgmSeekBar::default(),
                        Node {
                            flex_grow: 1.0,
                            height: Val::Px(24.0),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|hit_area| {
                        hit_area
                            .spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(4.0),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.18)),
                            ))
                            .with_children(|track| {
                                track.spawn((
                                    ExtraBgmProgress,
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::ZERO,
                                        width: Val::Percent(0.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.76)),
                                ));
                                track.spawn((
                                    ExtraBgmProgressThumb,
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Percent(0.0),
                                        top: Val::Px(-4.0),
                                        width: Val::Px(12.0),
                                        height: Val::Px(12.0),
                                        border_radius: BorderRadius::all(Val::Px(6.0)),
                                        ..default()
                                    },
                                    UiTransform::from_translation(Val2::px(-6.0, 0.0)),
                                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                                ));
                            });
                    });
                progress.spawn((
                    ExtraBgmTime,
                    Node {
                        width: Val::Px(105.0),
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    },
                    children![text("00:00 / 00:00", &fonts.text, 15.0, 0.58)],
                ));
            });
        panel
            .spawn((Node {
                height: Val::Px(57.0),
                margin: UiRect::top(Val::Px(EXTRA_CONTROL_MARGIN)),
                align_items: AlignItems::Center,
                ..default()
            },))
            .with_children(|controls| {
                for (icon, action) in [
                    ('\u{f564}', ExtraBgmControl::Previous),
                    ('\u{f4f4}', ExtraBgmControl::Play),
                    ('\u{f558}', ExtraBgmControl::Next),
                    ('\u{f592}', ExtraBgmControl::Stop),
                ] {
                    let mut button = controls.spawn((
                        Button,
                        UiSoundStyle::Switch,
                        action,
                        hover_surface(SURFACE_IDLE_ALPHA, false),
                        Node {
                            width: Val::Px(54.0),
                            height: Val::Px(45.0),
                            margin: UiRect::horizontal(Val::Px(EXTRA_CONTROL_MARGIN)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(button_surface(SURFACE_IDLE_ALPHA)),
                    ));
                    if matches!(action, ExtraBgmControl::Play) {
                        button.insert(ExtraBgmPlayIcon);
                    }
                    button.with_child(text(icon.to_string(), &fonts.icons, 24.0, 0.8));
                }
                let name = ui
                    .selected_bgm
                    .as_ref()
                    .and_then(|file| tracks.iter().find(|track| &track.0 == file))
                    .map_or("NO BGM", |track| track.1.as_str());
                controls.spawn((
                    ExtraBgmName,
                    Node {
                        flex_grow: 1.0,
                        padding: UiRect::left(Val::Px(12.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    children![text(name, &fonts.text, 20.25, 0.8)],
                ));
            });
    });
}

fn spawn_cg_panel(
    body: &mut ChildSpawnerCommands,
    images: &[(String, String)],
    ui: &ExtraUi,
    config: &GameConfigResource,
    fonts: &UiFonts,
    assets: &AssetServer,
) {
    let page_count = images.len().div_ceil(CG_PER_PAGE).max(1);
    let page = ui.page.clamp(1, page_count);
    body.spawn((Node {
        min_width: Val::Px(0.0),
        height: Val::Percent(100.0),
        padding: UiRect::all(Val::Px(EXTRA_PANEL_PADDING)),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(EXTRA_SECTION_GAP),
        ..default()
    },))
        .with_children(|panel| {
            panel
                .spawn((Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(48.0),
                    padding: UiRect::horizontal(Val::Px(12.0)),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                },))
                .with_children(|pages| {
                    pages.spawn(text_weight(
                        "CG",
                        &fonts.text,
                        24.0,
                        0.72,
                        bevy::text::FontWeight::BOLD,
                    ));
                    pages
                        .spawn((Node {
                            align_items: AlignItems::Center,
                            ..default()
                        },))
                        .with_children(|buttons| {
                            for index in 1..=page_count {
                                buttons.spawn((
                                    Button,
                                    UiSoundStyle::Switch,
                                    ExtraPage(index),
                                    hover_surface(0.0, index == page),
                                    Node {
                                        min_width: Val::Px(45.0),
                                        height: Val::Px(39.0),
                                        margin: UiRect::horizontal(Val::Px(EXTRA_CONTROL_MARGIN)),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        ..default()
                                    },
                                    BackgroundColor(button_surface(if index == page {
                                        SURFACE_ACTIVE_ALPHA
                                    } else {
                                        0.0
                                    })),
                                    children![text(index.to_string(), &fonts.text, 21.0, 0.8)],
                                ));
                            }
                        });
                });
            panel
                .spawn((
                    ExtraCgGrid,
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        padding: UiRect::top(Val::Px(30.0)),
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        align_content: AlignContent::FlexStart,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                ))
                .with_children(|grid| {
                    spawn_cg_cards(grid, images, page, config, fonts, assets);
                });
        });
}

fn spawn_cg_cards(
    grid: &mut ChildSpawnerCommands,
    images: &[(String, String)],
    page: usize,
    config: &GameConfigResource,
    fonts: &UiFonts,
    assets: &AssetServer,
) {
    let first = page.saturating_sub(1) * CG_PER_PAGE;
    let visible = images.iter().skip(first).take(CG_PER_PAGE);
    if visible.clone().next().is_none() {
        grid.spawn(text("NO CG", &fonts.text, 22.5, 0.36));
        return;
    }
    for (file, name) in visible {
        grid.spawn((
            Button,
            UiSoundStyle::Click,
            ExtraCg(file.clone()),
            Node {
                width: Val::Percent(22.5),
                height: Val::Percent(46.0),
                padding: UiRect::all(Val::Px(12.0)),
                margin: UiRect::all(Val::Percent(1.25)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(button_surface(SURFACE_IDLE_ALPHA)),
            hover_surface(SURFACE_IDLE_ALPHA, false),
        ))
        .with_children(|card| {
            card.spawn((
                ImageNode::new(assets.load(config.bg_path(file))),
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
            card.spawn((
                Node {
                    height: Val::Px(33.0),
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![text(name.clone(), &fonts.text, 18.75, 0.8)],
            ));
        });
    }
}

pub(crate) fn handle_navigation(
    keys: Res<ButtonInput<KeyCode>>,
    close: Query<&Interaction, (With<ExtraClose>, Changed<Interaction>)>,
    full: Query<Entity, With<ExtraFullCg>>,
    mut ui: ResMut<ExtraUi>,
    mut commands: Commands,
) {
    if !ui.open {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        if !full.is_empty() {
            for entity in &full {
                commands.entity(entity).despawn();
            }
        } else {
            ui.open = false;
        }
    }
    if close
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        ui.open = false;
    }
}

#[derive(SystemParam)]
pub(crate) struct ExtraPageContext<'w, 's> {
    pages: Query<'w, 's, (&'static Interaction, &'static ExtraPage), Changed<Interaction>>,
    ui: ResMut<'w, ExtraUi>,
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    fonts: Res<'w, UiFonts>,
    assets: Res<'w, AssetServer>,
    grids: Query<'w, 's, Entity, With<ExtraCgGrid>>,
    page_visuals: Query<'w, 's, (&'static ExtraPage, &'static mut HoverAlpha)>,
    commands: Commands<'w, 's>,
}

pub(crate) fn handle_page(mut context: ExtraPageContext) {
    let Some(page) = context
        .pages
        .iter()
        .find_map(|(interaction, page)| (*interaction == Interaction::Pressed).then_some(page.0))
    else {
        return;
    };
    if context.ui.page != page {
        context.ui.page = page;
        for (candidate, mut visual) in &mut context.page_visuals {
            let selected = candidate.0 == page;
            visual.active = selected;
            visual.target = if selected {
                SURFACE_ACTIVE_ALPHA
            } else {
                visual.idle_alpha
            };
        }
        let mut images = context
            .state
            .unlocked_cg
            .iter()
            .map(|(file, name)| (file.clone(), name.clone()))
            .collect::<Vec<_>>();
        images.sort_unstable_by(|left, right| left.1.cmp(&right.1));
        for grid in &context.grids {
            context
                .commands
                .entity(grid)
                .despawn_related::<Children>()
                .with_children(|cards| {
                    spawn_cg_cards(
                        cards,
                        &images,
                        page,
                        &context.config,
                        &context.fonts,
                        &context.assets,
                    );
                });
        }
    }
}

#[derive(SystemParam)]
pub(crate) struct ExtraCgContext<'w, 's> {
    cards: Query<'w, 's, (&'static Interaction, &'static ExtraCg), Changed<Interaction>>,
    controls: Query<'w, 's, (&'static Interaction, &'static ExtraCgControl), Changed<Interaction>>,
    full: Query<'w, 's, (Entity, &'static ExtraFullCg)>,
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    fonts: Res<'w, UiFonts>,
    assets: Res<'w, AssetServer>,
    camera: Query<'w, 's, Entity, With<DialogCamera>>,
    commands: Commands<'w, 's>,
}

pub(crate) fn handle_cg(mut context: ExtraCgContext) {
    let card = context.cards.iter().find_map(|(interaction, card)| {
        (*interaction == Interaction::Pressed).then_some(card.0.clone())
    });
    let control = context.controls.iter().find_map(|(interaction, control)| {
        (*interaction == Interaction::Pressed).then_some(*control)
    });
    if card.is_none() && control.is_none() {
        return;
    }
    let Ok(camera) = context.camera.single() else {
        return;
    };
    let mut images = context
        .state
        .unlocked_cg
        .iter()
        .map(|(file, name)| (file.clone(), name.clone()))
        .collect::<Vec<_>>();
    images.sort_unstable_by(|left, right| left.1.cmp(&right.1));

    let current = context.full.single().ok();
    let selected = if let Some(file) = card {
        Some(file)
    } else if matches!(control, Some(ExtraCgControl::Close)) {
        None
    } else {
        let Some((_, current)) = current else { return };
        let Some(index) = images.iter().position(|item| item.0 == current.0) else {
            return;
        };
        let next = if matches!(control, Some(ExtraCgControl::Previous)) {
            (index + images.len() - 1) % images.len()
        } else {
            (index + 1) % images.len()
        };
        Some(images[next].0.clone())
    };
    for (entity, _) in &context.full {
        context.commands.entity(entity).despawn();
    }
    let Some(file) = selected else { return };
    spawn_full_cg(
        &mut context.commands,
        camera,
        &file,
        &images,
        &context.config,
        &context.fonts,
        &context.assets,
    );
}

fn spawn_full_cg(
    commands: &mut Commands,
    camera: Entity,
    file: &str,
    images: &[(String, String)],
    config: &GameConfigResource,
    fonts: &UiFonts,
    assets: &AssetServer,
) {
    let name = images
        .iter()
        .find(|item| item.0 == file)
        .map_or("CG", |item| item.1.as_str());
    commands
        .spawn((
            ExtraFullCg(file.to_owned()),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(DESIGN_WIDTH),
                height: Val::Px(DESIGN_HEIGHT),
                padding: UiRect::all(Val::Px(24.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.72)),
            FocusPolicy::Block,
            GlobalZIndex(190),
            UiTargetCamera(camera),
            RenderLayers::layer(2),
        ))
        .with_children(|overlay| {
            overlay.spawn((
                ImageNode::new(assets.load(config.bg_path(file))),
                Node {
                    max_width: Val::Percent(94.0),
                    max_height: Val::Percent(90.0),
                    ..default()
                },
                FocusPolicy::Pass,
            ));
            overlay
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(42.0),
                        right: Val::Px(42.0),
                        top: Val::Px(30.0),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    children![text_weight(
                        name,
                        &fonts.text,
                        24.0,
                        0.78,
                        bevy::text::FontWeight::BOLD,
                    )],
                ))
                .with_children(|chrome| {
                    chrome
                        .spawn((Node {
                            column_gap: Val::Px(9.0),
                            ..default()
                        },))
                        .with_children(|buttons| {
                            for (label, action, sound) in [
                                ("‹", ExtraCgControl::Previous, UiSoundStyle::Switch),
                                ("›", ExtraCgControl::Next, UiSoundStyle::Switch),
                                ("×", ExtraCgControl::Close, UiSoundStyle::Click),
                            ] {
                                buttons
                                    .spawn((
                                        Button,
                                        sound,
                                        action,
                                        hover_surface(SURFACE_IDLE_ALPHA, false),
                                        Node {
                                            width: Val::Px(48.0),
                                            height: Val::Px(42.0),
                                            margin: UiRect::horizontal(Val::Px(
                                                EXTRA_CONTROL_MARGIN,
                                            )),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(button_surface(SURFACE_IDLE_ALPHA)),
                                    ))
                                    .with_child(text(label, &fonts.text, 27.0, 0.82));
                            }
                        });
                });
        });
}

#[derive(SystemParam)]
pub(crate) struct ExtraBgmContext<'w, 's> {
    tracks: Query<'w, 's, (&'static Interaction, &'static ExtraBgm), Changed<Interaction>>,
    controls: Query<'w, 's, (&'static Interaction, &'static ExtraBgmControl), Changed<Interaction>>,
    ui: ResMut<'w, ExtraUi>,
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    settings: Res<'w, RuntimeSettings>,
    assets: Res<'w, AssetServer>,
    players: Query<
        'w,
        's,
        (
            Entity,
            &'static mut ExtraBgmPlayer,
            Option<&'static AudioSink>,
        ),
    >,
    stage_bgm: Query<'w, 's, &'static AudioSink, (With<BgmPlayer>, Without<ExtraBgmPlayer>)>,
    seek_bars: Query<'w, 's, &'static mut ExtraBgmSeekBar>,
    commands: Commands<'w, 's>,
}

#[cfg(feature = "audio-opus")]
type ExtraOpusPlayerQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut ExtraBgmPlayer,
        Option<&'static AudioSink>,
        Option<&'static AudioPlayer<crate::runtime::audio::OpusAudio>>,
    ),
>;

pub(crate) fn handle_bgm(mut context: ExtraBgmContext) {
    let clicked = context.tracks.iter().find_map(|(interaction, track)| {
        (*interaction == Interaction::Pressed).then_some(track.0.clone())
    });
    let control = context.controls.iter().find_map(|(interaction, control)| {
        (*interaction == Interaction::Pressed).then_some(*control)
    });
    let mut ordered = context
        .state
        .unlocked_bgm
        .iter()
        .map(|(file, name)| (file.clone(), name.clone()))
        .collect::<Vec<_>>();
    ordered.sort_unstable_by(|left, right| left.1.cmp(&right.1));
    let (ended, ready_player) = match context.players.single_mut() {
        Ok((_, mut player, Some(sink))) => {
            if !sink.empty() {
                player.observed_audio = true;
            }
            (player.observed_audio && sink.empty(), true)
        }
        Ok((_, _, None)) => (false, true),
        Err(_) => (false, false),
    };
    let selected = match control {
        Some(ExtraBgmControl::Stop) => {
            for (entity, _, _) in &mut context.players {
                context.commands.entity(entity).despawn();
            }
            for sink in &context.stage_bgm {
                sink.play();
            }
            for mut seek in &mut context.seek_bars {
                seek.reset();
            }
            return;
        }
        Some(ExtraBgmControl::Play) if clicked.is_none() => {
            if let Ok((_, _, Some(sink))) = context.players.single_mut() {
                if sink.is_paused() {
                    sink.play();
                } else {
                    sink.pause();
                }
                return;
            }
            if ready_player {
                return;
            }
            context
                .ui
                .selected_bgm
                .clone()
                .or_else(|| ordered.first().map(|item| item.0.clone()))
        }
        Some(ExtraBgmControl::Previous) | Some(ExtraBgmControl::Next) => {
            if ordered.is_empty() {
                return;
            }
            let current = context
                .ui
                .selected_bgm
                .as_ref()
                .and_then(|file| ordered.iter().position(|candidate| &candidate.0 == file))
                .unwrap_or(0);
            let next = if matches!(control, Some(ExtraBgmControl::Previous)) {
                (current + ordered.len() - 1) % ordered.len()
            } else {
                (current + 1) % ordered.len()
            };
            Some(ordered[next].0.clone())
        }
        _ if ended && !ordered.is_empty() => {
            let current = context
                .ui
                .selected_bgm
                .as_ref()
                .and_then(|file| ordered.iter().position(|candidate| &candidate.0 == file))
                .unwrap_or(0);
            Some(ordered[(current + 1) % ordered.len()].0.clone())
        }
        _ => clicked,
    };
    let Some(file) = selected else { return };
    for (entity, _, _) in &mut context.players {
        context.commands.entity(entity).despawn();
    }
    for sink in &context.stage_bgm {
        sink.pause();
    }
    for mut seek in &mut context.seek_bars {
        seek.reset();
    }
    let volume = context.settings.master_volume * context.settings.bgm_volume;
    let mut entity = context.commands.spawn((
        ExtraBgmPlayer {
            duration: None,
            observed_audio: false,
        },
        PlaybackSettings {
            // Bevy's Loop mode wraps the source in rodio::Buffered, which is
            // intentionally not seekable. The lightweight restart above keeps
            // gallery playback looping while preserving native Opus seeking.
            mode: PlaybackMode::Once,
            volume: Volume::Linear(volume),
            ..default()
        },
    ));
    crate::runtime::audio::insert_player(
        &mut entity,
        &context.assets,
        context.config.bgm_path(&file),
    );
    context.ui.selected_bgm = Some(file);
}

pub(crate) fn sync_bgm_selection(
    ui: Res<ExtraUi>,
    state: Res<GameState>,
    mut tracks: Query<(&ExtraBgm, &mut HoverAlpha)>,
    names: Query<&Children, With<ExtraBgmName>>,
    mut labels: Query<&mut Text>,
) {
    if !ui.is_changed() && !state.is_changed() {
        return;
    }

    let selected = ui.selected_bgm.as_deref();
    for (track, mut visual) in &mut tracks {
        visual.active = selected == Some(track.0.as_str());
        visual.target = if visual.active {
            SURFACE_ACTIVE_ALPHA
        } else {
            visual.idle_alpha
        };
    }

    let name = selected
        .and_then(|file| state.unlocked_bgm.get(file))
        .map_or("NO BGM", String::as_str);
    for children in &names {
        for child in children.iter() {
            if let Ok(mut label) = labels.get_mut(child) {
                label.0.clear();
                label.0.push_str(name);
            }
        }
    }
}

pub(crate) fn sync_bgm_play_icon(
    players: Query<Option<&AudioSink>, With<ExtraBgmPlayer>>,
    buttons: Query<&Children, With<ExtraBgmPlayIcon>>,
    mut labels: Query<&mut Text>,
) {
    let playing = players
        .single()
        .ok()
        .flatten()
        .is_some_and(|sink| !sink.is_paused() && !sink.empty());
    let icon = if playing { "\u{f4c3}" } else { "\u{f4f4}" };
    for children in &buttons {
        for child in children.iter() {
            if let Ok(mut label) = labels.get_mut(child)
                && label.0 != icon
            {
                label.0.clear();
                label.0.push_str(icon);
            }
        }
    }
}

pub(crate) fn handle_bgm_seek(
    mut bars: Query<
        (
            &Interaction,
            &ComputedNode,
            &UiGlobalTransform,
            &mut ExtraBgmSeekBar,
        ),
        With<Button>,
    >,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    players: Query<(&ExtraBgmPlayer, &AudioSink)>,
) {
    let Ok((interaction, node, transform, mut seek)) = bars.single_mut() else {
        return;
    };
    let Ok(window) = windows.single() else { return };
    if mouse.just_pressed(MouseButton::Left)
        && matches!(interaction, Interaction::Hovered | Interaction::Pressed)
    {
        seek.dragging = true;
    }
    if seek.dragging
        && mouse.pressed(MouseButton::Left)
        && let Ok((player, _)) = players.single()
        && let (Some(duration), Some(cursor)) = (player.duration, window.physical_cursor_position())
        && let Some(point) = node.normalize_point(*transform, cursor)
    {
        let ratio = (point.x + 0.5).clamp(0.0, 1.0);
        seek.preview = Some(duration.mul_f64(f64::from(ratio)));
    }
    if mouse.just_released(MouseButton::Left) {
        if seek.dragging
            && let Some(position) = seek.preview
            && let Ok((_, sink)) = players.single()
            && let Err(error) = sink.try_seek(position)
        {
            log::warn!("BGM seek failed: {error}");
        }
        seek.reset();
    }
}

#[cfg(feature = "audio-opus")]
pub(crate) fn update_bgm_progress(
    mut players: ExtraOpusPlayerQuery,
    audio: Res<Assets<crate::runtime::audio::OpusAudio>>,
    seek: Query<&ExtraBgmSeekBar>,
    mut fills: Query<&mut Node, With<ExtraBgmProgress>>,
    mut thumbs: Query<&mut Node, (With<ExtraBgmProgressThumb>, Without<ExtraBgmProgress>)>,
    times: Query<&Children, With<ExtraBgmTime>>,
    mut labels: Query<&mut Text>,
) {
    let Ok((mut player, sink, opus_player)) = players.single_mut() else {
        set_bgm_progress(
            &mut fills,
            &mut thumbs,
            &times,
            &mut labels,
            Duration::ZERO,
            None,
        );
        return;
    };
    if player.duration.is_none() {
        player.duration = opus_player
            .and_then(|source| audio.get(&source.0))
            .and_then(crate::runtime::audio::OpusAudio::duration);
    }
    let position = seek
        .single()
        .ok()
        .and_then(|seek| seek.preview)
        .unwrap_or_else(|| sink.map_or(Duration::ZERO, AudioSinkPlayback::position));
    set_bgm_progress(
        &mut fills,
        &mut thumbs,
        &times,
        &mut labels,
        position,
        player.duration,
    );
}

#[cfg(not(feature = "audio-opus"))]
pub(crate) fn update_bgm_progress(
    players: Query<(&ExtraBgmPlayer, Option<&AudioSink>)>,
    seek: Query<&ExtraBgmSeekBar>,
    mut fills: Query<&mut Node, With<ExtraBgmProgress>>,
    mut thumbs: Query<&mut Node, (With<ExtraBgmProgressThumb>, Without<ExtraBgmProgress>)>,
    times: Query<&Children, With<ExtraBgmTime>>,
    mut labels: Query<&mut Text>,
) {
    let Ok((player, sink)) = players.single() else {
        set_bgm_progress(
            &mut fills,
            &mut thumbs,
            &times,
            &mut labels,
            Duration::ZERO,
            None,
        );
        return;
    };
    let position = seek
        .single()
        .ok()
        .and_then(|seek| seek.preview)
        .unwrap_or_else(|| sink.map_or(Duration::ZERO, AudioSinkPlayback::position));
    set_bgm_progress(
        &mut fills,
        &mut thumbs,
        &times,
        &mut labels,
        position,
        player.duration,
    );
}

fn set_bgm_progress(
    fills: &mut Query<&mut Node, With<ExtraBgmProgress>>,
    thumbs: &mut Query<&mut Node, (With<ExtraBgmProgressThumb>, Without<ExtraBgmProgress>)>,
    times: &Query<&Children, With<ExtraBgmTime>>,
    labels: &mut Query<&mut Text>,
    position: Duration,
    duration: Option<Duration>,
) {
    let elapsed = duration.map_or(position, |duration| position.min(duration));
    let percent = duration
        .filter(|duration| !duration.is_zero())
        .map_or(0.0, |duration| {
            (elapsed.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
        });
    for mut fill in fills.iter_mut() {
        fill.width = Val::Percent(percent);
    }
    for mut thumb in thumbs.iter_mut() {
        thumb.left = Val::Percent(percent);
    }
    let value = format!(
        "{} / {}",
        format_bgm_time(elapsed),
        duration.map_or_else(|| "--:--".to_owned(), format_bgm_time)
    );
    for children in times {
        for child in children.iter() {
            if let Ok(mut label) = labels.get_mut(child) {
                label.0.clone_from(&value);
            }
        }
    }
}

fn format_bgm_time(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

pub(crate) fn animate(
    time: Res<Time>,
    mut context: ExtraAnimationContext,
    mut fade_cache: Local<ExtraFadeCache>,
    mut commands: Commands,
) {
    let amount = exp_lerp(time.delta_secs(), UI_MOTION_RATE);
    let mut progress = None;
    for (entity, mut motion, mut transform) in &mut context.roots {
        motion.current += (motion.target - motion.current) * amount;
        if motion.target == 0.0 && motion.current <= 0.05
            || (motion.target - motion.current).abs() < 0.001
        {
            motion.current = motion.target;
        }
        let eased = smoothstep(motion.current);
        transform.translation = Val2::px(0.0, 9.0 * (1.0 - eased));
        transform.scale = Vec2::splat(0.99 + eased * 0.01);
        progress = Some((entity, eased));
        if motion.current == 0.0 && motion.target == 0.0 {
            commands.entity(entity).despawn();
            for entity in &context.players {
                commands.entity(entity).despawn();
            }
            for sink in &context.stage_bgm {
                sink.play();
            }
        }
    }
    if let Some((root, progress)) = progress {
        fade_extra_content(root, progress, &mut context.fade, &mut fade_cache);
        for mut watermark in &mut context.watermarks {
            watermark.set_progress(progress);
        }
        for (entity, mut strength, mut background) in &mut context.proxies {
            strength.0 = crate::ui::FULLSCREEN_BLUR_STRENGTH * progress;
            background.0 = Color::srgba(0.0, 0.0, 0.0, 0.6 * progress);
            if progress == 0.0 {
                commands.entity(entity).despawn();
            }
        }
    } else {
        fade_cache.clear();
    }
}

fn fade_extra_content(
    root: Entity,
    alpha: f32,
    context: &mut ExtraFadeContext,
    cache: &mut ExtraFadeCache,
) {
    if cache.root != Some(root) {
        cache.clear();
        cache.root = Some(root);
    }
    if alpha >= 0.999 {
        for (entity, base) in cache.text.drain() {
            if let Ok((_, mut color)) = context.texts.get_mut(entity) {
                color.0 = color.0.with_alpha(base);
            }
        }
        for (entity, base) in cache.background.drain() {
            if let Ok((_, mut color)) = context.backgrounds.get_mut(entity) {
                color.0 = color.0.with_alpha(base);
            }
        }
        for (entity, base) in cache.image.drain() {
            if let Ok((_, mut image)) = context.images.get_mut(entity) {
                image.color = image.color.with_alpha(base);
            }
        }
        cache.root = None;
        return;
    }
    let belongs_to_root = |entity: Entity| {
        let mut current = entity;
        while let Ok(parent) = context.parents.get(current) {
            current = parent.parent();
            if current == root {
                return true;
            }
        }
        false
    };
    for (entity, mut color) in &mut context.texts {
        if belongs_to_root(entity) {
            let base = *cache.text.entry(entity).or_insert_with(|| color.0.alpha());
            color.0 = color.0.with_alpha(base * alpha);
        }
    }
    for (entity, mut color) in &mut context.backgrounds {
        if belongs_to_root(entity) {
            let base = *cache
                .background
                .entry(entity)
                .or_insert_with(|| color.0.alpha());
            color.0 = color.0.with_alpha(base * alpha);
        }
    }
    for (entity, mut image) in &mut context.images {
        if belongs_to_root(entity) {
            let base = *cache
                .image
                .entry(entity)
                .or_insert_with(|| image.color.alpha());
            image.color = image.color.with_alpha(base * alpha);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bgm_time_without_fractional_jitter() {
        assert_eq!(format_bgm_time(Duration::from_millis(62_999)), "01:02");
    }
}
