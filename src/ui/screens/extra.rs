use std::{collections::HashMap, time::Duration};

use bevy::asset::LoadState;
use bevy::audio::{AudioSink, AudioSinkPlayback, PlaybackMode, Volume};
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use crate::render::blur::{DialogCamera, UiBlurCamera};
use crate::runtime::audio::GalleryAudio;
use crate::runtime::resources::{GameConfigResource, GameState};
use crate::scene::audio::BgmPlayer;
use crate::scene::images::{ImageRole, ImageRoleRegistry};
use crate::storage::settings::RuntimeSettings;
use crate::ui::control_bar::{BlurStrength, UiBlurSource};
use crate::ui::foundation::{
    PAGE_SLIDE_SECONDS, SURFACE_PANEL_ALPHA, UI_MOTION_RATE, UiFonts, dark_surface,
    ease_in_out_cubic, exp_lerp, logical_node_width, smoothstep, spawn_slider, text, text_weight,
};
use crate::ui::settings_panel::{
    SettingChoiceFillQuery, SettingChoiceVisual, SettingsWatermark, animate_choice_visual,
    choice_fill, menu_watermark,
};
use crate::ui::support::i18n::{LocalizedText, UiText};
use keine_core::{DESIGN_HEIGHT, DESIGN_WIDTH};

type StageBgmQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static AudioSink), (With<BgmPlayer>, Without<ExtraBgmPlayer>)>;

const CG_PER_PAGE: usize = 8;
const EXTRA_PANEL_PADDING: f32 = 24.0;
const EXTRA_CONTROL_MARGIN: f32 = 4.5;
const EXTRA_SECTION_GAP: f32 = 9.0;

#[derive(Resource)]
pub(crate) struct ExtraUi {
    pub(crate) open: bool,
    page: usize,
    section: ExtraSection,
    paused_stage_bgm: Vec<Entity>,
    selected_bgm: Option<String>,
}

impl Default for ExtraUi {
    fn default() -> Self {
        Self {
            open: false,
            page: 1,
            section: ExtraSection::Cg,
            paused_stage_bgm: Vec::new(),
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
pub(crate) struct ExtraPage(i32);

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ExtraSection {
    #[default]
    Cg,
    Music,
}

#[derive(Component)]
pub(crate) struct ExtraTab {
    section: ExtraSection,
}

#[derive(Component)]
pub(crate) struct ExtraButtonVisual;

#[derive(Component)]
pub(crate) struct ExtraPanel(ExtraSection);
#[derive(Component)]
pub(crate) struct ExtraPageLabel;
#[derive(Resource, Default)]
pub(crate) struct ExtraPageTransition {
    from: Option<ExtraSection>,
    elapsed: f32,
    cg_direction: f32,
}
impl ExtraPageTransition {
    pub(crate) fn is_animating(&self) -> bool {
        self.from.is_some() || self.cg_direction != 0.0
    }
}
#[derive(Component, Default)]
pub(crate) struct ExtraBgmList {
    target: f32,
}
impl ExtraBgmList {
    pub(crate) fn is_animating(&self, position: &ScrollPosition) -> bool {
        (self.target - position.y).abs() > 0.01
    }
}
#[derive(Component)]
pub(crate) struct ExtraBgmStatus;
#[derive(Component)]
pub(crate) struct ExtraImageStatus(bevy::asset::UntypedHandle);

#[derive(Component)]
pub(crate) struct ExtraThumbnail(Handle<crate::scene::images::GalleryThumbnail>);
#[derive(Component)]
pub(crate) struct ExtraBgmBubble;
#[derive(Component)]
pub(crate) struct ExtraBgmBubbleText;

#[derive(Component)]
pub(crate) struct ExtraCg(String);

#[derive(Component)]
pub(crate) struct ExtraCgGrid {
    outgoing: bool,
}

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
    failed: bool,
}

impl ExtraBgmPlayer {
    pub(crate) fn needs_frames(&self, sink: Option<&AudioSink>) -> bool {
        !self.failed && sink.is_none_or(|sink| !sink.is_paused())
    }
}

#[derive(Component)]
pub(crate) struct ExtraBgmTime;

#[derive(Component)]
pub(crate) struct ExtraBgmName;

#[derive(Component)]
pub(crate) struct ExtraBgmProgressThumb(f32);
impl ExtraBgmProgressThumb {
    pub(crate) fn is_animating(&self, active: bool) -> bool {
        (self.0 - if active { 12.0 } else { 10.0 }).abs() > 0.001
    }
}

#[derive(Component)]
pub(crate) struct ExtraBgmPlayIcon;

#[derive(Component, Default)]
pub(crate) struct ExtraBgmSeekBar {
    dragging: bool,
    touch: bool,
    preview: Option<Duration>,
}

impl ExtraBgmSeekBar {
    pub(crate) fn is_dragging(&self) -> bool {
        self.dragging
    }

    fn reset(&mut self) {
        self.dragging = false;
        self.touch = false;
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
    (
        Without<ExtraBlurProxy>,
        Without<ExtraRoot>,
        Without<ExtraFullCg>,
    ),
>;

#[derive(SystemParam)]
pub(crate) struct ExtraFadeContext<'w, 's> {
    parents: Query<'w, 's, &'static ChildOf>,
    texts: Query<'w, 's, (Entity, &'static mut TextColor)>,
    choices: Query<'w, 's, &'static SettingChoiceVisual>,
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
    stage_bgm: StageBgmQuery<'w, 's>,
    fade: ExtraFadeContext<'w, 's>,
}

fn extra_button() -> impl Bundle {
    (ExtraButtonVisual, SettingChoiceVisual::new(false))
}

pub(crate) fn attach_button_visuals(
    mut commands: Commands,
    buttons: Query<(Entity, &Children, &SettingChoiceVisual), Added<ExtraButtonVisual>>,
    mut labels: Query<&mut TextColor>,
) {
    for (entity, children, visual) in &buttons {
        let fill = commands.spawn(choice_fill(false)).id();
        commands.entity(entity).insert_children(0, &[fill]);
        for child in children.iter() {
            if let Ok(mut color) = labels.get_mut(child) {
                color.0.set_alpha(visual.text_alpha);
            }
        }
    }
}

type ExtraButtonQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        Option<&'static ExtraBgm>,
        Option<&'static ExtraTab>,
        &'static mut SettingChoiceVisual,
        &'static Children,
    ),
    With<ExtraButtonVisual>,
>;

pub(crate) fn update_button_visuals(
    time: Res<Time>,
    ui: Res<ExtraUi>,
    mut buttons: ExtraButtonQuery,
    mut fills: SettingChoiceFillQuery,
    mut labels: Query<(&mut TextColor, &mut TextFont)>,
) {
    let amount = exp_lerp(time.delta_secs(), 18.0);
    for (interaction, track, tab, mut visual, children) in &mut buttons {
        let selected = track.is_some_and(|track| ui.selected_bgm.as_deref() == Some(&track.0))
            || tab.is_some_and(|tab| ui.section == tab.section);
        animate_choice_visual(
            *interaction,
            selected,
            &mut visual,
            children,
            &mut fills,
            &mut labels,
            amount,
        );
    }
}

#[derive(SystemParam)]
pub(crate) struct ExtraSyncContext<'w, 's> {
    commands: Commands<'w, 's>,
    ui: ResMut<'w, ExtraUi>,
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    fonts: Res<'w, UiFonts>,
    settings: Res<'w, RuntimeSettings>,
    assets: Res<'w, AssetServer>,
    image_roles: Res<'w, ImageRoleRegistry>,
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

    if !context.roots.is_empty() {
        return;
    }

    let mut cg = context
        .state
        .unlocked_cg
        .iter()
        .map(|(file, name)| (file.clone(), name.clone()))
        .collect::<Vec<_>>();
    cg.sort_unstable_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    context.ui.page = context
        .ui
        .page
        .clamp(1, cg.len().div_ceil(CG_PER_PAGE).max(1));
    let mut bgm = context
        .state
        .unlocked_bgm
        .iter()
        .map(|(file, name)| (file.clone(), name.clone()))
        .collect::<Vec<_>>();
    bgm.sort_unstable_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));

    context
        .commands
        .spawn((
            ExtraRoot,
            ExtraMotion {
                current: 0.0,
                target: 1.0,
            },
            UiTransform::from_translation(Val2::px(31.5, 0.0)),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(DESIGN_WIDTH),
                height: Val::Px(DESIGN_HEIGHT),
                padding: UiRect::axes(Val::Percent(2.5), Val::Percent(2.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            FocusPolicy::Block,
            GlobalZIndex(172),
            UiTargetCamera(dialog_camera),
            RenderLayers::layer(2),
        ))
        .with_children(|root| {
            spawn_header(
                root,
                &context.fonts,
                context.ui.section,
                context.settings.locale,
            );
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(3.0),
                    top: Val::Percent(12.0),
                    width: Val::Percent(94.0),
                    height: Val::Percent(84.0),
                    padding: UiRect::axes(Val::Px(27.0), Val::Px(13.5)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(13.5),
                    ..default()
                },
                BackgroundColor(dark_surface(SURFACE_PANEL_ALPHA)),
            ))
            .with_children(|body| {
                body.spawn(Node {
                    position_type: PositionType::Relative,
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_width: Val::ZERO,
                    min_height: Val::ZERO,
                    flex_basis: Val::ZERO,
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|content| {
                    spawn_cg_panel(
                        content,
                        &cg,
                        &context.ui,
                        &context.config,
                        &context.fonts,
                        &context.assets,
                        &context.image_roles,
                    );
                    spawn_bgm_panel(
                        content,
                        &bgm,
                        &context.ui,
                        &context.fonts,
                        &context.settings,
                    );
                });
            });
        });
}

fn spawn_header(
    root: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    _selected: ExtraSection,
    locale: crate::storage::settings::UiLocale,
) {
    root.spawn((Node {
        width: Val::Percent(100.0),
        height: Val::Percent(7.0),
        padding: UiRect::horizontal(Val::Px(9.0)),
        flex_shrink: 0.0,
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        ..default()
    },))
        .with_children(|header| {
            header
                .spawn(Node {
                    height: Val::Percent(100.0),
                    ..default()
                })
                .with_children(|tabs| {
                    for (section, label, icon) in [
                        (ExtraSection::Cg, UiText::GalleryCg, "\u{f42a}"),
                        (ExtraSection::Music, UiText::GalleryMusic, "\u{f49e}"),
                    ] {
                        tabs.spawn((
                            Button,
                            ExtraTab { section },
                            extra_button(),
                            Node {
                                min_width: Val::Px(123.75),
                                height: Val::Percent(100.0),
                                padding: UiRect::horizontal(Val::Px(21.0)),
                                margin: UiRect::right(Val::Px(9.0)),
                                column_gap: Val::Px(7.5),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|button| {
                            button.spawn(text(icon, &fonts.icons, 21.0, 0.82));
                            button.spawn((
                                LocalizedText(label),
                                text(
                                    crate::ui::support::i18n::tr(locale, label),
                                    &fonts.text,
                                    21.0,
                                    0.82,
                                ),
                            ));
                        });
                    }
                });
            header
                .spawn((
                    Button,
                    ExtraClose,
                    extra_button(),
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
    settings: &RuntimeSettings,
) {
    body.spawn((
        ExtraPanel(ExtraSection::Music),
        UiTransform::default(),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            display: if ui.section == ExtraSection::Music {
                Display::Flex
            } else {
                Display::None
            },
            min_width: Val::Px(0.0),
            height: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(EXTRA_PANEL_PADDING)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(EXTRA_SECTION_GAP),
            ..default()
        },
    ))
    .with_children(|panel| {
        panel
            .spawn((
                ExtraBgmList::default(),
                ScrollPosition::default(),
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::ZERO,
                    flex_grow: 1.0,
                    flex_basis: Val::ZERO,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|list| {
                if tracks.is_empty() {
                    list.spawn((
                        LocalizedText(UiText::NoGalleryMusic),
                        text("NO MUSIC UNLOCKED", &fonts.text, 22.5, 0.58),
                    ));
                }
                for (file, name) in tracks {
                    list.spawn((
                        Button,
                        ExtraBgm(file.clone()),
                        extra_button(),
                        Node {
                            width: Val::Percent(100.0),
                            flex_shrink: 0.0,
                            min_height: Val::Px(63.0),
                            align_items: AlignItems::Center,
                            padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
                            margin: UiRect::all(Val::Px(EXTRA_CONTROL_MARGIN)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                        children![(
                            Node {
                                width: Val::Percent(100.0),
                                ..default()
                            },
                            text(name.clone(), &fonts.text, 24.0, 0.8)
                        )],
                    ));
                }
            });
        panel
            .spawn((
                ExtraBgmName,
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(36.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ))
            .with_child(text(
                ui.selected_bgm
                    .as_ref()
                    .and_then(|file| tracks.iter().find(|track| &track.0 == file))
                    .map_or("", |track| track.1.as_str()),
                &fonts.text,
                27.0,
                0.8,
            ));
        panel.spawn((
            ExtraBgmStatus,
            LocalizedText(UiText::PlaybackStopped),
            Node {
                display: Display::None,
                ..default()
            },
            text("", &fonts.text, 18.75, 0.58),
        ));
        panel
            .spawn(Node {
                width: Val::Percent(100.0),
                height: Val::Px(90.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(9.0),
                padding: UiRect::horizontal(Val::Px(EXTRA_CONTROL_MARGIN)),
                ..default()
            })
            .with_children(|progress| {
                progress
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        ..default()
                    })
                    .with_children(|slider| {
                        let parts =
                            spawn_slider(slider, &fonts.text, 0.0, "00:00", Val::Percent(100.0));
                        let mut commands = slider.commands();
                        commands
                            .entity(parts.control)
                            .insert(ExtraBgmSeekBar::default());
                        commands.entity(parts.thumb).insert((
                            ExtraBgmProgressThumb(10.0),
                            Interaction::None,
                            FocusPolicy::Pass,
                        ));
                        commands.entity(parts.bubble).insert(ExtraBgmBubble);
                        commands.entity(parts.value).insert(ExtraBgmBubbleText);
                    });
                progress.spawn((
                    ExtraBgmTime,
                    Node {
                        width: Val::Percent(100.0),
                        flex_shrink: 0.0,
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    },
                    children![text("00:00 / --:--", &fonts.text, 22.5, 0.58)],
                ));
            });
        panel
            .spawn((Node {
                width: Val::Percent(100.0),
                height: Val::Px(63.0),
                flex_shrink: 0.0,
                margin: UiRect::top(Val::Px(EXTRA_CONTROL_MARGIN)),
                align_items: AlignItems::Center,
                ..default()
            },))
            .with_children(|controls| {
                for (icon, action, label) in [
                    ('\u{f564}', ExtraBgmControl::Previous, UiText::Previous),
                    ('\u{f4f4}', ExtraBgmControl::Play, UiText::Play),
                    ('\u{f558}', ExtraBgmControl::Next, UiText::Next),
                    ('\u{f592}', ExtraBgmControl::Stop, UiText::Stop),
                ] {
                    let mut button = controls.spawn((
                        Button,
                        action,
                        extra_button(),
                        Node {
                            min_width: Val::Px(144.0),
                            height: Val::Px(63.0),
                            padding: UiRect::horizontal(Val::Px(12.0)),
                            column_gap: Val::Px(9.0),
                            margin: UiRect::horizontal(Val::Px(EXTRA_CONTROL_MARGIN)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ));
                    if matches!(action, ExtraBgmControl::Play) {
                        button.insert(ExtraBgmPlayIcon);
                    }
                    button.with_children(|button| {
                        button.spawn(text(icon.to_string(), &fonts.icons, 24.0, 0.8));
                        button.spawn((LocalizedText(label), text("", &fonts.text, 22.5, 0.8)));
                    });
                }
                crate::ui::settings_panel::spawn_gallery_volume_slider(
                    controls,
                    settings,
                    &fonts.text,
                );
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
    image_roles: &ImageRoleRegistry,
) {
    let page_count = images.len().div_ceil(CG_PER_PAGE).max(1);
    let page = ui.page.clamp(1, page_count);
    body.spawn((
        ExtraPanel(ExtraSection::Cg),
        UiTransform::default(),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            display: if ui.section == ExtraSection::Cg {
                Display::Flex
            } else {
                Display::None
            },
            min_width: Val::Px(0.0),
            height: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(EXTRA_PANEL_PADDING)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(EXTRA_SECTION_GAP),
            ..default()
        },
    ))
    .with_children(|panel| {
        panel
            .spawn((Node {
                width: Val::Percent(100.0),
                height: Val::Px(63.0),
                flex_shrink: 0.0,
                padding: UiRect::horizontal(Val::Px(12.0)),
                justify_content: JustifyContent::FlexEnd,
                align_items: AlignItems::Center,
                ..default()
            },))
            .with_children(|pages| {
                pages
                    .spawn((Node {
                        align_items: AlignItems::Center,
                        ..default()
                    },))
                    .with_children(|buttons| {
                        for (step, label) in [(-1, UiText::Previous), (1, UiText::Next)] {
                            buttons.spawn((
                                Button,
                                ExtraPage(step),
                                extra_button(),
                                Node {
                                    min_width: Val::Px(126.0),
                                    height: Val::Px(63.0),
                                    padding: UiRect::horizontal(Val::Px(12.0)),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                                children![(LocalizedText(label), text("", &fonts.text, 22.5, 0.8))],
                            ));
                            if step == -1 {
                                buttons.spawn((
                                    ExtraPageLabel,
                                    text(format!("{page} / {page_count}"), &fonts.text, 22.5, 0.58),
                                ));
                            }
                        }
                    });
            });
        panel
            .spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::ZERO,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|viewport| {
                spawn_cg_grid(viewport, images, page, config, fonts, assets, image_roles);
            });
    });
}

fn spawn_cg_grid(
    viewport: &mut ChildSpawnerCommands,
    images: &[(String, String)],
    page: usize,
    config: &GameConfigResource,
    fonts: &UiFonts,
    assets: &AssetServer,
    image_roles: &ImageRoleRegistry,
) {
    viewport
        .spawn((
            ExtraCgGrid { outgoing: false },
            UiTransform::default(),
            Node {
                position_type: PositionType::Absolute,
                left: Val::ZERO,
                top: Val::ZERO,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                padding: UiRect::top(Val::Px(30.0)),
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                align_content: AlignContent::FlexStart,
                ..default()
            },
        ))
        .with_children(|grid| {
            spawn_cg_cards(grid, images, page, config, fonts, assets, image_roles);
        });
}

fn spawn_cg_cards(
    grid: &mut ChildSpawnerCommands,
    images: &[(String, String)],
    page: usize,
    config: &GameConfigResource,
    fonts: &UiFonts,
    assets: &AssetServer,
    image_roles: &ImageRoleRegistry,
) {
    let first = page.saturating_sub(1) * CG_PER_PAGE;
    let visible = images.iter().skip(first).take(CG_PER_PAGE);
    if visible.clone().next().is_none() {
        grid.spawn((
            LocalizedText(UiText::NoGalleryCg),
            text("NO CG UNLOCKED", &fonts.text, 22.5, 0.58),
        ));
        return;
    }
    for (file, name) in visible {
        grid.spawn((
            Button,
            ExtraCg(file.clone()),
            Node {
                width: Val::Percent(22.5),
                height: Val::Percent(46.0),
                padding: UiRect::all(Val::Px(12.0)),
                margin: UiRect::all(Val::Percent(1.25)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::NONE),
            extra_button(),
        ))
        .with_children(|card| {
            let path = config.bg_path(file);
            let thumbnail = crate::scene::images::is_webp(&path)
                .then(|| assets.load::<crate::scene::images::GalleryThumbnail>(path.clone()));
            let image = if thumbnail.is_some() {
                Handle::default()
            } else {
                crate::scene::images::load(assets, image_roles, path, ImageRole::BACKGROUND)
            };
            let status = thumbnail.as_ref().map_or_else(
                || image.clone().untyped(),
                |handle| handle.clone().untyped(),
            );
            card.spawn(Node {
                width: Val::Percent(100.0),
                min_height: Val::ZERO,
                flex_grow: 1.0,
                flex_basis: Val::ZERO,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|preview| {
                let mut node = preview.spawn((
                    ImageNode::new(image.clone()),
                    Node {
                        max_width: Val::Percent(100.0),
                        max_height: Val::Percent(100.0),
                        ..default()
                    },
                    FocusPolicy::Pass,
                ));
                if let Some(thumbnail) = thumbnail {
                    node.insert(ExtraThumbnail(thumbnail));
                }
                preview.spawn((
                    ExtraImageStatus(status),
                    LocalizedText(UiText::GalleryLoading),
                    text("", &fonts.text, 18.75, 0.58),
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                    FocusPolicy::Pass,
                ));
            });
            card.spawn((
                Node {
                    height: Val::Px(63.0),
                    width: Val::Percent(100.0),
                    overflow: Overflow::clip(),
                    flex_shrink: 0.0,
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
    actions: Res<crate::runtime::platform::InputActions>,
    close: Query<&Interaction, (With<ExtraClose>, Changed<Interaction>)>,
    mut full: Query<&mut ExtraMotion, With<ExtraFullCg>>,
    mut ui: ResMut<ExtraUi>,
) {
    if !ui.open {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || actions.back {
        if !full.is_empty() {
            for mut motion in &mut full {
                motion.target = 0.0;
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
    image_roles: Res<'w, ImageRoleRegistry>,
    grids: Query<'w, 's, (Entity, &'static ChildOf, &'static ExtraCgGrid)>,
    transition: ResMut<'w, ExtraPageTransition>,
    page_labels: Query<'w, 's, &'static mut Text, With<ExtraPageLabel>>,
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
    if !context.ui.open || context.transition.is_animating() {
        return;
    }
    let mut images = ordered_cg(&context.state);
    let count = images.len().div_ceil(CG_PER_PAGE).max(1);
    let page = (context.ui.page as i64 + i64::from(page)).clamp(1, count as i64) as usize;
    if context.ui.page != page {
        context.transition.cg_direction = if page > context.ui.page { 1.0 } else { -1.0 };
        context.transition.elapsed = 0.0;
        context.ui.page = page;
        for mut label in &mut context.page_labels {
            label.0 = format!("{page} / {count}");
        }
        images.sort_unstable_by(|left, right| {
            left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0))
        });
        for (grid, parent, phase) in &context.grids {
            if phase.outgoing {
                continue;
            }
            context
                .commands
                .entity(grid)
                .insert(ExtraCgGrid { outgoing: true });
            context
                .commands
                .entity(parent.parent())
                .with_children(|viewport| {
                    spawn_cg_grid(
                        viewport,
                        &images,
                        page,
                        &context.config,
                        &context.fonts,
                        &context.assets,
                        &context.image_roles,
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
    roots: Query<'w, 's, Entity, With<ExtraRoot>>,
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    fonts: Res<'w, UiFonts>,
    assets: Res<'w, AssetServer>,
    image_roles: Res<'w, ImageRoleRegistry>,
    ui: Res<'w, ExtraUi>,
    transition: Res<'w, ExtraPageTransition>,
    motions: Query<'w, 's, &'static ExtraMotion>,
    camera: Query<'w, 's, Entity, With<DialogCamera>>,
    commands: Commands<'w, 's>,
}

pub(crate) fn handle_cg(mut context: ExtraCgContext) {
    if !context.ui.open
        || context.transition.is_animating()
        || context.motions.iter().any(ExtraMotion::is_animating)
    {
        return;
    }
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
    let images = ordered_cg(&context.state);

    let current = context.full.single().ok();
    let selected = if let Some(file) = card {
        Some(file)
    } else if matches!(control, Some(ExtraCgControl::Close)) {
        for (entity, _) in &context.full {
            context.commands.entity(entity).insert(ExtraMotion {
                current: 1.0,
                target: 0.0,
            });
        }
        return;
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
    let image = crate::scene::images::load(
        &context.assets,
        &context.image_roles,
        context.config.bg_path(&file),
        ImageRole::BACKGROUND,
    );
    let overlay = spawn_full_cg(
        &mut context.commands,
        camera,
        &file,
        &images,
        &context.fonts,
        image,
    );
    if let Ok(root) = context.roots.single() {
        context.commands.entity(root).add_child(overlay);
    }
}

fn spawn_full_cg(
    commands: &mut Commands,
    camera: Entity,
    file: &str,
    images: &[(String, String)],
    fonts: &UiFonts,
    image: Handle<Image>,
) -> Entity {
    let name = images
        .iter()
        .find(|item| item.0 == file)
        .map_or("CG", |item| item.1.as_str());
    commands
        .spawn((
            ExtraFullCg(file.to_owned()),
            ExtraMotion {
                current: 0.0,
                target: 1.0,
            },
            UiTransform::default(),
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
                ImageNode::new(image.clone()),
                Node {
                    max_width: Val::Percent(94.0),
                    max_height: Val::Percent(80.0),
                    ..default()
                },
                FocusPolicy::Pass,
            ));
            overlay.spawn((
                ExtraImageStatus(image.untyped()),
                LocalizedText(UiText::GalleryLoading),
                text("", &fonts.text, 24.0, 0.78),
                Node {
                    position_type: PositionType::Absolute,
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
                    children![(
                        Node {
                            max_width: Val::Percent(60.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        text_weight(name, &fonts.text, 24.0, 0.78, bevy::text::FontWeight::BOLD,)
                    )],
                ))
                .with_children(|chrome| {
                    chrome
                        .spawn((Node {
                            column_gap: Val::Px(9.0),
                            ..default()
                        },))
                        .with_children(|buttons| {
                            for (label, action, key) in [
                                ("‹", ExtraCgControl::Previous, UiText::Previous),
                                ("›", ExtraCgControl::Next, UiText::Next),
                                ("×", ExtraCgControl::Close, UiText::Back),
                            ] {
                                buttons
                                    .spawn((
                                        Button,
                                        action,
                                        extra_button(),
                                        Node {
                                            min_width: Val::Px(144.0),
                                            height: Val::Px(63.0),
                                            column_gap: Val::Px(9.0),
                                            padding: UiRect::horizontal(Val::Px(12.0)),
                                            margin: UiRect::horizontal(Val::Px(
                                                EXTRA_CONTROL_MARGIN,
                                            )),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(Color::NONE),
                                    ))
                                    .with_children(|button| {
                                        button.spawn(text(label, &fonts.text, 27.0, 0.82));
                                        button.spawn((
                                            LocalizedText(key),
                                            text("", &fonts.text, 22.5, 0.82),
                                        ));
                                    });
                            }
                        });
                });
        })
        .id()
}

#[derive(SystemParam)]
pub(crate) struct ExtraBgmContext<'w, 's> {
    full: Query<'w, 's, (), With<ExtraFullCg>>,
    transition: Res<'w, ExtraPageTransition>,
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
            Option<&'static mut AudioSink>,
        ),
    >,
    stage_bgm: StageBgmQuery<'w, 's>,
    seek_bars: Query<'w, 's, &'static mut ExtraBgmSeekBar>,
    commands: Commands<'w, 's>,
}

#[derive(SystemParam)]
pub(crate) struct ExtraAudioAssets<'w> {
    #[cfg(feature = "audio-opus")]
    opus: Res<'w, Assets<crate::runtime::audio::OpusAudio>>,
    #[cfg(feature = "audio-seekable")]
    seekable: Res<'w, Assets<crate::runtime::audio::SeekableAudio>>,
    #[cfg(not(any(feature = "audio-opus", feature = "audio-seekable")))]
    _fallback: Res<'w, AssetServer>,
}

impl ExtraAudioAssets<'_> {
    fn duration(&self, audio: Option<&GalleryAudio>) -> Option<Duration> {
        match audio {
            #[cfg(feature = "audio-opus")]
            Some(GalleryAudio::Opus(handle)) => self
                .opus
                .get(handle)
                .and_then(crate::runtime::audio::OpusAudio::duration),
            #[cfg(feature = "audio-seekable")]
            Some(GalleryAudio::Seekable(handle)) => self
                .seekable
                .get(handle)
                .and_then(crate::runtime::audio::SeekableAudio::duration),
            _ => None,
        }
    }
}

pub(crate) fn handle_bgm(mut context: ExtraBgmContext) {
    if !context.ui.open {
        return;
    }
    let input_allowed = context.full.is_empty()
        && !context.transition.is_animating()
        && context.ui.section == ExtraSection::Music;
    let clicked = context.tracks.iter().find_map(|(interaction, track)| {
        (*interaction == Interaction::Pressed).then_some(track.0.clone())
    });
    let control = context.controls.iter().find_map(|(interaction, control)| {
        (*interaction == Interaction::Pressed).then_some(*control)
    });
    let clicked = input_allowed.then_some(clicked).flatten();
    let control = input_allowed.then_some(control).flatten();
    let volume = context.settings.master_volume * context.settings.bgm_volume;
    let (ended, ready_player) = match context.players.single_mut() {
        Ok((_, mut player, Some(mut sink))) => {
            sink.set_volume(Volume::Linear(volume));
            if !sink.empty() {
                player.observed_audio = true;
            }
            (player.observed_audio && sink.empty(), true)
        }
        Ok((_, player, None)) => (false, !player.failed),
        Err(_) => (false, false),
    };
    let selected = match control {
        Some(ExtraBgmControl::Stop) => {
            for (entity, _, _) in &mut context.players {
                context.commands.entity(entity).despawn();
            }
            resume_stage_bgm(&mut context.ui, &context.stage_bgm);
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
            context.ui.selected_bgm.clone().or_else(|| {
                ordered_bgm(&context.state)
                    .first()
                    .map(|(file, _)| (*file).to_owned())
            })
        }
        Some(ExtraBgmControl::Previous) | Some(ExtraBgmControl::Next) => {
            let ordered = ordered_bgm(&context.state);
            if ordered.is_empty() {
                return;
            }
            let current = context
                .ui
                .selected_bgm
                .as_ref()
                .and_then(|file| ordered.iter().position(|candidate| candidate.0 == file))
                .unwrap_or(0);
            let next = if matches!(control, Some(ExtraBgmControl::Previous)) {
                (current + ordered.len() - 1) % ordered.len()
            } else {
                (current + 1) % ordered.len()
            };
            Some(ordered[next].0.to_owned())
        }
        _ if ended => {
            let ordered = ordered_bgm(&context.state);
            if ordered.is_empty() {
                return;
            }
            let current = context
                .ui
                .selected_bgm
                .as_ref()
                .and_then(|file| ordered.iter().position(|candidate| candidate.0 == file))
                .unwrap_or(0);
            Some(ordered[(current + 1) % ordered.len()].0.to_owned())
        }
        _ => clicked,
    };
    let Some(file) = selected else { return };
    for (entity, _, _) in &mut context.players {
        context.commands.entity(entity).despawn();
    }
    for (entity, sink) in &context.stage_bgm {
        if !sink.is_paused() {
            context.ui.paused_stage_bgm.push(entity);
            sink.pause();
        }
    }
    for mut seek in &mut context.seek_bars {
        seek.reset();
    }
    let mut entity = context.commands.spawn(ExtraBgmPlayer {
        duration: None,
        observed_audio: false,
        failed: false,
    });
    crate::runtime::audio::insert_gallery_player(
        &mut entity,
        &context.assets,
        context.config.bgm_path(&file),
        PlaybackSettings {
            // A one-shot source stays seekable; end detection above advances
            // the gallery playlist and wraps the last track to the first.
            mode: PlaybackMode::Once,
            volume: Volume::Linear(volume),
            ..default()
        },
    );
    context.ui.selected_bgm = Some(file);
}

fn ordered_bgm(state: &GameState) -> Vec<(&str, &str)> {
    let mut ordered = state
        .unlocked_bgm
        .iter()
        .map(|(file, name)| (file.as_str(), name.as_str()))
        .collect::<Vec<_>>();
    ordered.sort_unstable_by(|left, right| left.1.cmp(right.1).then_with(|| left.0.cmp(right.0)));
    ordered
}

pub(crate) fn sync_bgm_selection(
    ui: Res<ExtraUi>,
    state: Res<GameState>,
    names: Query<&Children, With<ExtraBgmName>>,
    mut labels: Query<&mut Text>,
) {
    if !ui.is_changed() && !state.is_changed() {
        return;
    }

    let selected = ui.selected_bgm.as_deref();
    let name = selected
        .and_then(|file| state.unlocked_bgm.get(file))
        .map_or("", String::as_str);
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
    mut labels: Query<(&mut Text, Option<&mut LocalizedText>)>,
) {
    let playing = players
        .single()
        .ok()
        .flatten()
        .is_some_and(|sink| !sink.is_paused() && !sink.empty());
    let icon = if playing { "\u{f4c3}" } else { "\u{f4f4}" };
    for children in &buttons {
        for child in children.iter() {
            if let Ok((mut label, localized)) = labels.get_mut(child) {
                if let Some(mut key) = localized {
                    key.0 = if playing { UiText::Pause } else { UiText::Play };
                } else if label.0 != icon {
                    label.0 = icon.into();
                }
            }
        }
    }
}

pub(crate) fn handle_bgm_seek(
    mut bars: Query<
        (
            Entity,
            &Interaction,
            &ComputedNode,
            &UiGlobalTransform,
            &mut ExtraBgmSeekBar,
        ),
        With<Button>,
    >,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    touch: Option<Res<crate::ui::touch::TouchInputState>>,
    players: Query<(&ExtraBgmPlayer, &AudioSink)>,
) {
    let Ok((entity, interaction, node, transform, mut seek)) = bars.single_mut() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    if !window.focused {
        seek.reset();
        return;
    }
    let touch_point = touch
        .as_ref()
        .and_then(|touch| touch.control_position(entity));
    if touch_point.is_some() {
        seek.dragging = true;
        seek.touch = true;
    } else if mouse.just_pressed(MouseButton::Left) && *interaction != Interaction::None {
        seek.dragging = true;
        seek.touch = false;
    }
    let point = if seek.touch {
        touch_point
    } else if mouse.pressed(MouseButton::Left) {
        window.physical_cursor_position().map(|point| {
            point
                - crate::runtime::platform::DesignViewport::from_window(window)
                    .camera_viewport(window)
                    .physical_position
                    .as_vec2()
        })
    } else {
        None
    };
    if seek.dragging
        && let Ok((player, _sink)) = players.single()
        && let (Some(duration), Some(cursor)) = (player.duration, point)
        && let Some(point) = node.normalize_point(*transform, cursor)
    {
        let ratio = (point.x + 0.5).clamp(0.0, 1.0);
        #[cfg(target_os = "android")]
        if !duration.is_zero() {
            let before = seek
                .preview
                .unwrap_or_else(|| _sink.position())
                .as_secs_f32()
                / duration.as_secs_f32();
            crate::ui::haptics::drag_feedback(before, ratio, 1.0, 0.05);
        }
        seek.preview = Some(duration.mul_f64(f64::from(ratio)));
    }
    let finished = if seek.touch {
        touch
            .as_ref()
            .is_some_and(|touch| touch.control_finished(entity))
    } else {
        mouse.just_released(MouseButton::Left)
    };
    if finished {
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

#[derive(SystemParam)]
pub(crate) struct ExtraBgmProgressUi<'w, 's> {
    time: Res<'w, Time>,
    bars: Query<
        'w,
        's,
        (
            &'static Interaction,
            &'static ExtraBgmSeekBar,
            &'static ComputedNode,
        ),
    >,
    thumbs: Query<
        'w,
        's,
        (
            &'static mut ExtraBgmProgressThumb,
            &'static Interaction,
            &'static mut Node,
            &'static mut BackgroundColor,
        ),
    >,
    bubbles:
        Query<'w, 's, &'static mut Node, (With<ExtraBgmBubble>, Without<ExtraBgmProgressThumb>)>,
    values: Query<'w, 's, Entity, With<ExtraBgmBubbleText>>,
    times: Query<'w, 's, (Entity, &'static Children), With<ExtraBgmTime>>,
    labels: Query<'w, 's, &'static mut Text>,
}

type BgmTimeDisplayKey = (Option<Entity>, u64, Option<u64>);

pub(crate) fn update_bgm_progress(
    mut players: Query<(
        &mut ExtraBgmPlayer,
        Option<&AudioSink>,
        Option<&GalleryAudio>,
    )>,
    audio: ExtraAudioAssets,
    seek: Query<&ExtraBgmSeekBar>,
    mut ui: ExtraBgmProgressUi,
    mut displayed_time: Local<Option<BgmTimeDisplayKey>>,
) {
    let Ok((mut player, sink, source)) = players.single_mut() else {
        set_bgm_progress(&mut ui, Duration::ZERO, None, &mut displayed_time);
        return;
    };
    if player.duration.is_none() {
        player.duration = audio.duration(source);
    }
    let position = seek
        .single()
        .ok()
        .and_then(|seek| seek.preview)
        .unwrap_or_else(|| sink.map_or(Duration::ZERO, AudioSinkPlayback::position));
    set_bgm_progress(&mut ui, position, player.duration, &mut displayed_time);
}

fn set_bgm_progress(
    ui: &mut ExtraBgmProgressUi,
    position: Duration,
    duration: Option<Duration>,
    displayed_time: &mut Option<BgmTimeDisplayKey>,
) {
    let elapsed = duration.map_or(position, |duration| position.min(duration));
    let percent = duration
        .filter(|duration| !duration.is_zero())
        .map_or(0.0, |duration| {
            (elapsed.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
        });
    let ratio = percent / 100.0;
    let (_, dragging, width) =
        ui.bars
            .single()
            .map_or((false, false, 375.0), |(interaction, seek, node)| {
                (
                    *interaction != Interaction::None,
                    seek.dragging,
                    logical_node_width(node).max(45.0),
                )
            });
    for (mut visual, interaction, mut thumb, mut background) in &mut ui.thumbs {
        let hovered = *interaction != Interaction::None;
        let target = if hovered || dragging { 12.0 } else { 10.0 };
        visual.0 += (target - visual.0) * exp_lerp(ui.time.delta_secs(), 18.0);
        let thumb_width = (visual.0 * 3.75).min(width);
        thumb.width = Val::Px(thumb_width);
        thumb.left = Val::Px(ratio * (width - thumb_width));
        background.0 = Color::srgba(1.0, 1.0, 1.0, if hovered { 0.67 } else { 0.5 });
    }
    let thumb_hovered = ui
        .thumbs
        .iter()
        .any(|(_, interaction, _, _)| *interaction != Interaction::None);
    for mut bubble in &mut ui.bubbles {
        bubble.display = if thumb_hovered || dragging {
            Display::Flex
        } else {
            Display::None
        };
        let bubble_width = 67.5_f32.min(width);
        bubble.width = Val::Px(bubble_width);
        bubble.left = Val::Px(ratio * (width - bubble_width));
    }
    for entity in &ui.values {
        if let Ok(mut value) = ui.labels.get_mut(entity) {
            value.0 = format_bgm_time(elapsed);
        }
    }
    let time_key = (
        ui.times.iter().next().map(|(entity, _)| entity),
        elapsed.as_secs(),
        duration.map(|duration| duration.as_secs()),
    );
    if *displayed_time == Some(time_key) {
        return;
    }
    *displayed_time = Some(time_key);
    let value = format!(
        "{} / {}",
        format_bgm_time(elapsed),
        duration.map_or_else(|| "--:--".to_owned(), format_bgm_time)
    );
    for (_, children) in &ui.times {
        for child in children.iter() {
            if let Ok(mut label) = ui.labels.get_mut(child) {
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
    mut ui: ResMut<ExtraUi>,
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
        transform.translation = Val2::px(31.5 * (1.0 - eased), 0.0);
        transform.scale = Vec2::ONE;
        progress = Some((entity, eased));
        if motion.current == 0.0 && motion.target == 0.0 {
            commands.entity(entity).despawn();
            for entity in &context.players {
                commands.entity(entity).despawn();
            }
            resume_stage_bgm(&mut ui, &context.stage_bgm);
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
            // Zero progress also occurs on the first opening frame. Only
            // tear down the backdrop when the screen has finished closing.
            if !ui.open && progress == 0.0 {
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
                let base = context
                    .parents
                    .get(entity)
                    .ok()
                    .and_then(|parent| context.choices.get(parent.parent()).ok())
                    .map_or(base, |visual| visual.text_alpha);
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
            let choice_alpha = context
                .parents
                .get(entity)
                .ok()
                .and_then(|parent| context.choices.get(parent.parent()).ok())
                .map(|visual| visual.text_alpha);
            let base = cache.text.entry(entity).or_insert_with(|| color.0.alpha());
            if let Some(choice_alpha) = choice_alpha {
                *base = choice_alpha;
            }
            color.0 = color.0.with_alpha(*base * alpha);
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

fn ordered_cg(state: &GameState) -> Vec<(String, String)> {
    let mut ordered = state
        .unlocked_cg
        .iter()
        .map(|(file, name)| (file.clone(), name.clone()))
        .collect::<Vec<_>>();
    ordered.sort_unstable_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    ordered
}

fn resume_stage_bgm(ui: &mut ExtraUi, stage: &StageBgmQuery<'_, '_>) {
    for entity in ui.paused_stage_bgm.drain(..) {
        if let Ok((_, sink)) = stage.get(entity) {
            sink.play();
        }
    }
}

pub(crate) fn content_ready(
    ui: Res<ExtraUi>,
    transition: Res<ExtraPageTransition>,
    full: Query<(), With<ExtraFullCg>>,
    roots: Query<&ExtraMotion, With<ExtraRoot>>,
) -> bool {
    ui.open
        && !transition.is_animating()
        && full.is_empty()
        && roots.iter().all(|motion| !motion.is_animating())
}

pub(crate) fn handle_section(
    buttons: Query<(&Interaction, &ExtraTab), Changed<Interaction>>,
    mut ui: ResMut<ExtraUi>,
    mut transition: ResMut<ExtraPageTransition>,
    mut seek: Query<&mut ExtraBgmSeekBar>,
) {
    for (interaction, tab) in &buttons {
        if *interaction == Interaction::Pressed && ui.section != tab.section {
            transition.from = Some(ui.section);
            transition.elapsed = 0.0;
            ui.section = tab.section;
            for mut seek in &mut seek {
                seek.reset();
            }
        }
    }
}

pub(crate) fn update_sections(
    time: Res<Time>,
    ui: Res<ExtraUi>,
    mut transition: ResMut<ExtraPageTransition>,
    mut panels: Query<(&ExtraPanel, &mut Node, &mut UiTransform), Without<ExtraCgGrid>>,
    mut grids: Query<(Entity, &ExtraCgGrid, &mut UiTransform), Without<ExtraPanel>>,
    mut commands: Commands,
) {
    if !ui.open {
        *transition = ExtraPageTransition::default();
    }
    if transition.is_animating() {
        transition.elapsed = (transition.elapsed + time.delta_secs()).min(PAGE_SLIDE_SECONDS);
    }
    let progress = transition.elapsed / PAGE_SLIDE_SECONDS;
    for (panel, mut node, mut transform) in &mut panels {
        let outgoing = transition.from == Some(panel.0);
        let incoming = ui.section == panel.0;
        node.display = if incoming || outgoing {
            Display::Flex
        } else {
            Display::None
        };
        transform.translation = if transition.from.is_some() {
            horizontal_page_offset(
                outgoing,
                if ui.section == ExtraSection::Music {
                    1.0
                } else {
                    -1.0
                },
                progress,
            )
        } else {
            Val2::ZERO
        };
    }
    for (entity, grid, mut transform) in &mut grids {
        if transition.cg_direction != 0.0 && progress < 1.0 {
            transform.translation =
                horizontal_page_offset(grid.outgoing, transition.cg_direction, progress);
        } else if grid.outgoing {
            commands.entity(entity).despawn();
        } else {
            transform.translation = Val2::ZERO;
        }
    }
    if progress >= 1.0 {
        transition.from = None;
        transition.cg_direction = 0.0;
        for (panel, mut node, mut transform) in &mut panels {
            node.display = if ui.section == panel.0 {
                Display::Flex
            } else {
                Display::None
            };
            transform.translation = Val2::ZERO;
        }
    }
}

fn horizontal_page_offset(outgoing: bool, direction: f32, progress: f32) -> Val2 {
    let eased = ease_in_out_cubic(progress);
    Val2::percent(
        direction * 100.0 * if outgoing { -eased } else { 1.0 - eased },
        0.0,
    )
}

#[derive(SystemParam)]
pub(crate) struct ExtraScrollContext<'w, 's> {
    ui: Res<'w, ExtraUi>,
    full: Query<'w, 's, (), With<ExtraFullCg>>,
    transition: Res<'w, ExtraPageTransition>,
}

pub(crate) fn scroll_music(
    mut wheel: MessageReader<MouseWheel>,
    touch: Option<Res<crate::ui::touch::TouchInputState>>,
    time: Res<Time>,
    windows: Query<&Window>,
    mut lists: Query<(
        Entity,
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
        &mut ExtraBgmList,
    )>,
    context: ExtraScrollContext,
) {
    let wheel_delta: f32 = wheel
        .read()
        .map(|event| {
            event.y
                * if event.unit == MouseScrollUnit::Line {
                    36.0
                } else {
                    1.0
                }
        })
        .sum();
    if !context.ui.open
        || context.ui.section != ExtraSection::Music
        || !context.full.is_empty()
        || context.transition.is_animating()
    {
        return;
    }
    let Ok((entity, node, transform, mut position, mut list)) = lists.single_mut() else {
        return;
    };
    let hovered = windows
        .single()
        .ok()
        .and_then(|window| {
            window.physical_cursor_position().map(|point| {
                point
                    - crate::runtime::platform::DesignViewport::from_window(window)
                        .camera_viewport(window)
                        .physical_position
                        .as_vec2()
            })
        })
        .is_some_and(|point| node.contains_point(*transform, point));
    let touch_delta = touch
        .as_ref()
        .and_then(|touch| touch.scroll)
        .filter(|(owner, _)| *owner == entity)
        .map_or(0.0, |(_, amount)| amount * node.inverse_scale_factor());
    let max = (node.content_size().y - node.size().y).max(0.0) * node.inverse_scale_factor();
    #[cfg(target_os = "android")]
    let before = list.target;
    list.target =
        (list.target - if hovered { wheel_delta } else { 0.0 } - touch_delta).clamp(0.0, max);
    #[cfg(target_os = "android")]
    if (hovered && wheel_delta != 0.0) || touch_delta != 0.0 {
        crate::ui::haptics::drag_feedback(before, list.target, max, 64.0);
    }
    position.y += (list.target - position.y) * exp_lerp(time.delta_secs(), UI_MOTION_RATE);
    if (list.target - position.y).abs() < 0.01 {
        position.y = list.target;
    }
}

pub(crate) fn update_image_status(
    server: Res<AssetServer>,
    thumbnails: Res<Assets<crate::scene::images::GalleryThumbnail>>,
    mut cards: Query<(&ExtraThumbnail, &mut ImageNode)>,
    mut labels: Query<(&ExtraImageStatus, &mut LocalizedText, &mut Node)>,
) {
    for (thumbnail, mut image) in &mut cards {
        if let Some(loaded) = thumbnails.get(&thumbnail.0)
            && image.image != loaded.image
        {
            image.image = loaded.image.clone();
        }
    }
    for (image, mut label, mut node) in &mut labels {
        let state = server.load_state(&image.0);
        node.display = if matches!(state, LoadState::Loaded) {
            Display::None
        } else {
            Display::Flex
        };
        label.0 = if matches!(state, LoadState::Failed(_)) {
            UiText::GalleryUnavailable
        } else {
            UiText::GalleryLoading
        };
    }
}

pub(crate) fn update_bgm_status(
    _server: Res<AssetServer>,
    mut players: Query<(
        &mut ExtraBgmPlayer,
        Option<&AudioSink>,
        Option<&GalleryAudio>,
    )>,
    mut labels: Query<(&mut LocalizedText, &mut Node), With<ExtraBgmStatus>>,
    mut ui: ResMut<ExtraUi>,
    stage: StageBgmQuery<'_, '_>,
) {
    let status = if let Ok((mut player, sink, source)) = players.single_mut() {
        let failed = match source {
            #[cfg(feature = "audio-opus")]
            Some(GalleryAudio::Opus(handle)) => {
                matches!(_server.load_state(handle), LoadState::Failed(_))
            }
            #[cfg(feature = "audio-seekable")]
            Some(GalleryAudio::Seekable(handle)) => {
                matches!(_server.load_state(handle), LoadState::Failed(_))
            }
            _ => true,
        };
        player.failed = failed;
        if failed {
            resume_stage_bgm(&mut ui, &stage);
            UiText::GalleryUnavailable
        } else if let Some(sink) = sink {
            if sink.is_paused() {
                UiText::PlaybackPaused
            } else {
                UiText::PlaybackPlaying
            }
        } else {
            UiText::GalleryLoading
        }
    } else {
        UiText::PlaybackStopped
    };
    for (mut label, mut node) in &mut labels {
        label.0 = status;
        node.display = if matches!(status, UiText::GalleryLoading | UiText::GalleryUnavailable) {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub(crate) fn animate_full_cg(
    time: Res<Time>,
    mut full: Query<
        (
            Entity,
            &mut ExtraMotion,
            &mut UiTransform,
            &mut BackgroundColor,
        ),
        With<ExtraFullCg>,
    >,
    mut fade: ExtraFadeContext,
    mut cache: Local<ExtraFadeCache>,
    mut commands: Commands,
) {
    for (entity, mut motion, mut transform, mut background) in &mut full {
        motion.current +=
            (motion.target - motion.current) * exp_lerp(time.delta_secs(), UI_MOTION_RATE);
        if (motion.current - motion.target).abs() < 0.001
            || motion.target == 0.0 && motion.current < 0.05
        {
            motion.current = motion.target;
        }
        let progress = smoothstep(motion.current);
        transform.scale = Vec2::splat(0.99 + progress * 0.01);
        background.0 = Color::srgba(0.0, 0.0, 0.0, 0.72 * progress);
        fade_extra_content(entity, progress, &mut fade, &mut cache);
        if progress == 0.0 && motion.target == 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backdrop_survives_zero_delta_opening_and_is_removed_after_closing() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .insert_resource(ExtraUi {
                open: true,
                ..default()
            })
            .add_systems(Update, animate);
        let root = app
            .world_mut()
            .spawn((
                ExtraRoot,
                ExtraMotion {
                    current: 0.0,
                    target: 1.0,
                },
                UiTransform::default(),
            ))
            .id();
        let proxy = app
            .world_mut()
            .spawn((
                ExtraBlurProxy,
                BlurStrength(0.0),
                BackgroundColor(Color::NONE),
            ))
            .id();
        app.update();
        assert!(app.world().get_entity(proxy).is_ok());
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(1));
        app.update();
        assert_eq!(
            app.world().get::<BlurStrength>(proxy).unwrap().0,
            crate::ui::FULLSCREEN_BLUR_STRENGTH
        );
        assert_eq!(
            app.world().get::<BackgroundColor>(proxy).unwrap().0.alpha(),
            0.6
        );
        app.world_mut().resource_mut::<ExtraUi>().open = false;
        app.world_mut().get_mut::<ExtraMotion>(root).unwrap().target = 0.0;
        app.update();
        assert!(app.world().get_entity(root).is_err());
        assert!(app.world().get_entity(proxy).is_err());
    }

    #[test]
    fn cg_page_slide_retains_outgoing_cards_until_horizontal_transition_finishes() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .insert_resource(ExtraUi {
                open: true,
                ..default()
            })
            .insert_resource(ExtraPageTransition {
                cg_direction: 1.0,
                ..default()
            })
            .add_systems(Update, update_sections);
        let old = app
            .world_mut()
            .spawn((ExtraCgGrid { outgoing: true }, UiTransform::default()))
            .id();
        let new = app
            .world_mut()
            .spawn((ExtraCgGrid { outgoing: false }, UiTransform::default()))
            .id();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(150));
        app.update();
        assert_eq!(
            app.world().get::<UiTransform>(old).unwrap().translation,
            Val2::percent(-50.0, 0.0)
        );
        assert_eq!(
            app.world().get::<UiTransform>(new).unwrap().translation,
            Val2::percent(50.0, 0.0)
        );
        app.update();
        assert!(app.world().get_entity(old).is_err());
        assert_eq!(
            app.world().get::<UiTransform>(new).unwrap().translation,
            Val2::ZERO
        );
        assert!(!app.world().resource::<ExtraPageTransition>().is_animating());
    }

    #[test]
    fn right_click_returns_from_full_cg_before_leaving_extra() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(crate::runtime::platform::InputActions {
                back: true,
                ..default()
            })
            .insert_resource(ExtraUi {
                open: true,
                ..default()
            })
            .init_resource::<Time>()
            .add_systems(Update, (handle_navigation, animate_full_cg).chain());
        let cg = app
            .world_mut()
            .spawn((
                ExtraFullCg("test.webp".into()),
                ExtraMotion {
                    current: 1.0,
                    target: 1.0,
                },
                UiTransform::default(),
                BackgroundColor(Color::BLACK),
            ))
            .id();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(16));
        app.update();
        assert!(app.world().get_entity(cg).is_ok());
        assert!(app.world().resource::<ExtraUi>().open);
        assert_eq!(app.world().get::<ExtraMotion>(cg).unwrap().target, 0.0);
        // Repeated Back during the exit fade cannot also dismiss Extra.
        app.update();
        assert!(app.world().resource::<ExtraUi>().open);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(1));
        app.update();
        assert!(app.world().get_entity(cg).is_err());
        assert!(app.world().resource::<ExtraUi>().open);
        app.update();
        assert!(!app.world().resource::<ExtraUi>().open);
    }

    #[test]
    fn tabs_keep_panels_and_selection_until_the_slide_settles() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<ExtraPageTransition>()
            .insert_resource(ExtraUi {
                open: true,
                section: ExtraSection::Music,
                selected_bgm: Some("chosen.opus".into()),
                ..default()
            })
            .add_systems(Update, update_sections);
        app.world_mut().resource_mut::<ExtraPageTransition>().from = Some(ExtraSection::Cg);
        let old = app
            .world_mut()
            .spawn((
                ExtraPanel(ExtraSection::Cg),
                Node::default(),
                UiTransform::default(),
            ))
            .id();
        let new = app
            .world_mut()
            .spawn((
                ExtraPanel(ExtraSection::Music),
                Node::default(),
                UiTransform::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(150));
        app.update();
        assert_eq!(app.world().get::<Node>(old).unwrap().display, Display::Flex);
        assert_eq!(app.world().get::<Node>(new).unwrap().display, Display::Flex);
        assert!(app.world().resource::<ExtraPageTransition>().is_animating());
        assert_eq!(
            app.world().get::<UiTransform>(old).unwrap().translation,
            Val2::percent(-50.0, 0.0)
        );
        assert_eq!(
            app.world().get::<UiTransform>(new).unwrap().translation,
            Val2::percent(50.0, 0.0)
        );
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(150));
        app.update();
        assert_eq!(app.world().get::<Node>(old).unwrap().display, Display::None);
        assert_eq!(app.world().get::<Node>(new).unwrap().display, Display::Flex);
        assert_eq!(
            app.world().get::<UiTransform>(new).unwrap().translation,
            Val2::ZERO
        );
        assert!(!app.world().resource::<ExtraPageTransition>().is_animating());
        assert_eq!(
            app.world().resource::<ExtraUi>().selected_bgm.as_deref(),
            Some("chosen.opus")
        );
    }

    #[test]
    fn formats_bgm_time_without_fractional_jitter() {
        assert_eq!(format_bgm_time(Duration::from_millis(62_999)), "01:02");
    }

    #[test]
    fn orders_bgm_by_display_name_then_file() {
        let mut state = GameState(keine_core::State::new());
        state.unlocked_bgm.insert("z.opus".into(), "Same".into());
        state.unlocked_bgm.insert("a.opus".into(), "Same".into());
        state
            .unlocked_bgm
            .insert("middle.opus".into(), "Before".into());

        assert_eq!(
            ordered_bgm(&state),
            vec![
                ("middle.opus", "Before"),
                ("a.opus", "Same"),
                ("z.opus", "Same"),
            ]
        );
    }
}
