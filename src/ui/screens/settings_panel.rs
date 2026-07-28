use std::collections::HashMap;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::FocusPolicy;
use bevy::window::{MonitorSelection, WindowMode};

use crate::render::blur::{DialogCamera, UiBlurCamera};
use crate::runtime::resources::{ContentProjectResource, GameConfigResource, ProjectRoot};
use crate::storage::settings::{RuntimeSettings, UiLocale};
use crate::ui::control_bar::{
    BlurStrength, ButtonAction, ControlInput, SkipMode, ToggleStates, UiBlurSource,
};
use crate::ui::foundation::{
    UiFonts, UiSoundStyle, ease_in_out_cubic, exp_lerp, fill_node, smoothstep, text_weight,
};
use crate::ui::menu::{
    MenuBack, MenuBlur, MenuFade, MenuHeaderActive, MenuRouteTransition, MenuSurface,
    PersistentMenu, active_route, root_node, spawn_header, surface_transform,
};
use crate::ui::save_load::SaveLoadUi;
use crate::ui::support::i18n::{UiText, tr};

const OPTION_TRANSITION_RATE: f32 = 18.0;
const OPTION_TEXT_IDLE: f32 = 0.376;
const OPTION_TEXT_ACTIVE: f32 = 0.667;
const OPTION_FILL_ALPHA: f32 = 0.188;
const PAGE_TEXT_IDLE: f32 = 0.175;
const PAGE_TEXT_HOVER: f32 = 0.5;
const PAGE_TEXT_ACTIVE: f32 = 0.8;
const SETTINGS_COLUMNS: u16 = 3;
const SETTINGS_COLUMN_GAP: f32 = 30.0;
const SETTINGS_ROW_GAP: f32 = 24.0;
const SETTING_LABEL_SIZE: f32 = 30.0;
const SETTING_OPTION_SIZE: f32 = 24.0;

#[derive(Clone, Copy)]
struct SettingsProjectContext<'a> {
    config: &'a GameConfigResource,
    content: &'a ContentProjectResource,
}

#[derive(Clone, Copy)]
struct SettingsGridCell {
    column: i16,
    row: i16,
    span: u16,
}

impl SettingsGridCell {
    const fn at(column: i16, row: i16) -> Self {
        Self {
            column,
            row,
            span: 1,
        }
    }

    const fn spanning(column: i16, row: i16, span: u16) -> Self {
        Self { column, row, span }
    }

    const fn from_index(index: usize) -> Self {
        Self::at(
            (index % SETTINGS_COLUMNS as usize) as i16 + 1,
            (index / SETTINGS_COLUMNS as usize) as i16 + 1,
        )
    }

    fn column(self) -> GridPlacement {
        GridPlacement::start_span(self.column, self.span)
    }

    fn row(self) -> GridPlacement {
        GridPlacement::start(self.row)
    }
}

#[derive(Resource)]
pub(crate) struct SettingsUi {
    pub(crate) open: bool,
    pub(crate) page: SettingsPage,
}

#[derive(Resource, Default)]
pub(crate) struct PendingWindowMode {
    target: Option<bool>,
    delay_frames: u8,
}

impl PendingWindowMode {
    pub(crate) fn is_pending(&self) -> bool {
        self.target.is_some()
    }
}

#[derive(Resource, Default)]
pub(crate) struct ActiveSettingSlider {
    kind: Option<SettingKind>,
    dirty: bool,
}

impl ActiveSettingSlider {
    pub(crate) fn is_active(&self) -> bool {
        self.kind.is_some()
    }
}

impl Default for SettingsUi {
    fn default() -> Self {
        Self {
            open: false,
            page: SettingsPage::System,
        }
    }
}

#[derive(Component)]
pub(crate) struct SettingsRoot;

#[derive(Component)]
pub(crate) struct SettingsEntryPending;

#[derive(Component)]
pub(crate) struct SettingsContent;

#[derive(Component)]
pub(crate) struct SettingsBlurProxy;

#[derive(Component)]
pub(crate) struct SettingsWatermark {
    current: f32,
    target: f32,
    pending_label: Option<String>,
}

impl SettingsWatermark {
    fn entering() -> Self {
        Self {
            current: 0.0,
            target: 1.0,
            pending_label: None,
        }
    }

    pub(crate) fn show(&mut self) {
        self.target = 1.0;
    }

    pub(crate) fn hide(&mut self) {
        self.target = 0.0;
    }

    pub(crate) fn show_label(&mut self, text: &mut Text, label: &str) {
        if text.0 == label && self.pending_label.is_none() {
            self.show();
        } else {
            self.pending_label = Some(label.to_owned());
            self.target = 0.0;
        }
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.pending_label.is_some() || (self.current - self.target).abs() > 0.001
    }
}

pub(crate) fn spawn_menu_watermark(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    font: &Handle<Font>,
) {
    parent.spawn((
        Name::new("menu_watermark"),
        SettingsWatermark::entering(),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(39.0),
            bottom: Val::Px(18.0),
            ..default()
        },
        FocusPolicy::Pass,
        crate::ui::text_style::NoTextShadow,
        Text::new(label),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::from(240.0),
            weight: FontWeight::BOLD,
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.0)),
    ));
}

#[derive(Default)]
pub(crate) struct SettingsVisualFadeCache {
    text_alpha: HashMap<Entity, f32>,
    background_alpha: HashMap<Entity, f32>,
    outline_alpha: HashMap<Entity, f32>,
    settled: bool,
}

type SettingsRootVisibilityQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut Visibility),
    (
        With<SettingsRoot>,
        Without<SettingsBlurProxy>,
        Without<crate::ui::save_load::SaveLoadRoot>,
    ),
>;
type SettingsProxyVisibilityQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut Visibility),
    (
        With<SettingsBlurProxy>,
        Without<SettingsRoot>,
        Without<crate::ui::save_load::SaveLoadRoot>,
    ),
>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SettingsPage {
    #[default]
    System,
    Display,
    Audio,
    About,
}

impl SettingsPage {
    const fn index(self) -> i8 {
        match self {
            Self::System => 0,
            Self::Display => 1,
            Self::Audio => 2,
            Self::About => 3,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct SettingsPageTransition {
    from: Option<SettingsPage>,
    to: Option<SettingsPage>,
    elapsed: f32,
}

impl SettingsPageTransition {
    const SECONDS: f32 = 0.3;

    fn begin(&mut self, from: SettingsPage, to: SettingsPage) {
        self.from = Some(from);
        self.to = Some(to);
        self.elapsed = 0.0;
    }

    fn reset(&mut self) {
        self.from = None;
        self.to = None;
        self.elapsed = 0.0;
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.from.is_some() && self.to.is_some()
    }
}

#[derive(Component)]
pub(crate) struct SettingsPageButton(pub(crate) SettingsPage);

#[derive(Component)]
pub(crate) struct SettingsPageLabel;

#[derive(Component)]
pub(crate) struct SettingsPageButtonVisual(f32);

impl SettingsPageButtonVisual {
    pub(crate) fn is_animating(
        &self,
        interaction: Interaction,
        page: SettingsPage,
        active: SettingsPage,
    ) -> bool {
        let target = if page == active {
            PAGE_TEXT_ACTIVE
        } else if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            PAGE_TEXT_HOVER
        } else {
            PAGE_TEXT_IDLE
        };
        (self.0 - target).abs() > 0.001
    }
}

#[derive(Component)]
pub(crate) struct SettingsPagePanel {
    page: SettingsPage,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingAction {
    SetSkip(bool),
    SetFullscreen(bool),
    SetTextSize(u8),
    ClearSaves,
    ResetSettings,
    ClearAll,
    ExportData,
    ImportData,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingKind {
    MasterVolume,
    VocalVolume,
    BgmVolume,
    SeVolume,
    UiSeVolume,
    TextSpeed,
    AutoDelay,
    TextboxOpacity,
}

impl SettingKind {
    fn label(self) -> UiText {
        match self {
            Self::MasterVolume => UiText::MasterVolume,
            Self::VocalVolume => UiText::VoiceVolume,
            Self::BgmVolume => UiText::BgmVolume,
            Self::SeVolume => UiText::SoundEffectVolume,
            Self::UiSeVolume => UiText::UiSoundVolume,
            Self::TextSpeed => UiText::TextSpeed,
            Self::AutoDelay => UiText::AutoPlaySpeed,
            Self::TextboxOpacity => UiText::TextboxOpacity,
        }
    }

    fn ratio(self, settings: &RuntimeSettings) -> f32 {
        match self {
            Self::MasterVolume => settings.master_volume,
            Self::VocalVolume => settings.vocal_volume,
            Self::BgmVolume => settings.bgm_volume,
            Self::SeVolume => settings.se_volume,
            Self::UiSeVolume => settings.ui_se_volume,
            Self::TextSpeed => ((settings.typewriter_speed - 10.0) / 110.0) as f32,
            Self::AutoDelay => ((settings.auto_delay - 0.5) / 4.5) as f32,
            Self::TextboxOpacity => settings.textbox_opacity,
        }
    }

    fn set_ratio(self, settings: &mut RuntimeSettings, ratio: f32) {
        match self {
            Self::MasterVolume => settings.master_volume = ratio,
            Self::VocalVolume => settings.vocal_volume = ratio,
            Self::BgmVolume => settings.bgm_volume = ratio,
            Self::SeVolume => settings.se_volume = ratio,
            Self::UiSeVolume => settings.ui_se_volume = ratio,
            Self::TextSpeed => settings.typewriter_speed = 10.0 + f64::from(ratio) * 110.0,
            Self::AutoDelay => settings.auto_delay = 0.5 + f64::from(ratio) * 4.5,
            Self::TextboxOpacity => settings.textbox_opacity = ratio,
        }
    }

    fn value_text(self, ratio: f32) -> String {
        match self {
            Self::TextSpeed => format!("{:.0}", 10.0 + ratio * 110.0),
            Self::AutoDelay => format!("{:.1}", 0.5 + ratio * 4.5),
            _ => format!("{}", (ratio * 100.0).round()),
        }
    }
}

#[derive(Clone, Copy)]
struct SliderSpec {
    kind: SettingKind,
}

const SYSTEM_SLIDERS: &[SliderSpec] = &[SliderSpec {
    kind: SettingKind::AutoDelay,
}];

const DISPLAY_SLIDERS: &[SliderSpec] = &[
    SliderSpec {
        kind: SettingKind::TextSpeed,
    },
    SliderSpec {
        kind: SettingKind::TextboxOpacity,
    },
];

const AUDIO_SLIDERS: &[SliderSpec] = &[
    SliderSpec {
        kind: SettingKind::MasterVolume,
    },
    SliderSpec {
        kind: SettingKind::VocalVolume,
    },
    SliderSpec {
        kind: SettingKind::BgmVolume,
    },
    SliderSpec {
        kind: SettingKind::SeVolume,
    },
    SliderSpec {
        kind: SettingKind::UiSeVolume,
    },
];

#[derive(Component)]
pub(crate) struct SettingSlider(pub(crate) SettingKind);

#[derive(Component)]
pub(crate) struct SettingSliderThumb(pub(crate) SettingKind);

#[derive(Component)]
pub(crate) struct SettingSliderThumbVisual(pub(crate) f32);

#[derive(Component)]
pub(crate) struct SettingValueText(pub(crate) SettingKind);

#[derive(Component)]
pub(crate) struct SettingValueBubble(pub(crate) SettingKind);

#[derive(Component)]
pub(crate) struct SettingChoice(pub(crate) SettingAction);

#[derive(Component)]
pub(crate) struct LanguageDropdownButton;

#[derive(Component)]
pub(crate) struct LanguageDropdownOption(pub(crate) UiLocale);

#[derive(Component)]
pub(crate) struct LanguageDropdownOptions;

#[derive(Component)]
pub(crate) struct LanguageDropdownIcon;

#[derive(SystemParam)]
pub(crate) struct LanguageDropdownContext<'w, 's> {
    menus: Query<'w, 's, (&'static mut LanguageDropdownAnimation, &'static mut Node)>,
    icons: Query<'w, 's, &'static mut Text, With<LanguageDropdownIcon>>,
    roots: Query<'w, 's, Entity, With<SettingsRoot>>,
    commands: Commands<'w, 's>,
    settings: ResMut<'w, RuntimeSettings>,
    project_root: Res<'w, ProjectRoot>,
    ui: ResMut<'w, SettingsUi>,
}

#[derive(Component)]
pub(crate) struct AboutRepositoryLink;

#[derive(Component)]
pub(crate) struct AboutRepositoryLabel;

#[derive(Component)]
pub(crate) struct AboutRepositoryUnderline;

#[derive(Component)]
pub(crate) struct AboutRepositoryVisual {
    underline_width: f32,
    text_alpha: f32,
}

impl AboutRepositoryVisual {
    pub(crate) fn is_animating(&self, interaction: Interaction) -> bool {
        let (underline, text) = about_repository_targets(interaction);
        (self.underline_width - underline).abs() > 0.001 || (self.text_alpha - text).abs() > 0.001
    }
}

type AboutRepositoryLinkQuery<'w, 's> =
    Query<'w, 's, &'static Interaction, (With<AboutRepositoryLink>, Changed<Interaction>)>;

type AboutRepositoryAnimationQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        &'static mut AboutRepositoryVisual,
        &'static Children,
    ),
    With<AboutRepositoryLink>,
>;

#[derive(Component)]
pub(crate) struct LanguageDropdownAnimation {
    progress: f32,
    target: f32,
}

impl LanguageDropdownAnimation {
    pub(crate) fn is_animating(&self) -> bool {
        (self.progress - self.target).abs() > 0.001
    }
}

#[derive(Component)]
pub(crate) struct SettingChoiceVisual {
    selected: bool,
    hovered: bool,
    fill: f32,
    text_alpha: f32,
}

impl SettingChoiceVisual {
    pub(crate) fn is_animating(
        &self,
        interaction: Interaction,
        action: SettingAction,
        settings: &RuntimeSettings,
    ) -> bool {
        let selected = choice_is_selected(settings, action);
        let hovered = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        let target_fill = if selected || hovered { 100.0 } else { 0.0 };
        let target_text = if selected || hovered {
            OPTION_TEXT_ACTIVE
        } else {
            OPTION_TEXT_IDLE
        };
        self.selected != selected
            || self.hovered != hovered
            || (self.fill - target_fill).abs() > 0.001
            || (self.text_alpha - target_text).abs() > 0.001
    }
}

#[derive(Component)]
pub(crate) struct SettingChoiceFill;

type SettingChoiceFillQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Node, &'static mut BackgroundColor),
    (With<SettingChoiceFill>, Without<SettingSliderThumb>),
>;

#[derive(Component)]
pub(crate) struct SettingPreviewSurface;

#[derive(Component)]
pub(crate) struct SettingPreviewText;

#[derive(SystemParam)]
pub(crate) struct SettingsSyncContext<'w, 's> {
    commands: Commands<'w, 's>,
    roots: SettingsRootVisibilityQuery<'w, 's>,
    proxies: SettingsProxyVisibilityQuery<'w, 's>,
    camera: Query<'w, 's, Entity, With<DialogCamera>>,
    blur_camera: Query<'w, 's, Entity, With<UiBlurCamera>>,
    fonts: Res<'w, UiFonts>,
    config: Res<'w, GameConfigResource>,
    content: Res<'w, ContentProjectResource>,
    fades: Query<'w, 's, &'static mut MenuFade>,
    watermarks: Query<'w, 's, (&'static mut SettingsWatermark, &'static mut Text)>,
    save_roots:
        Query<'w, 's, (Entity, &'static mut Visibility), With<crate::ui::save_load::SaveLoadRoot>>,
    save_proxies: Query<'w, 's, Entity, With<crate::ui::save_load::SaveLoadBlurProxy>>,
    route_transition: Res<'w, MenuRouteTransition>,
}

pub fn toggle_settings(
    keys: Res<ButtonInput<KeyCode>>,
    input: ControlInput,
    back: Query<&Interaction, (With<MenuBack>, Changed<Interaction>)>,
    mut ui: ResMut<SettingsUi>,
    mut save_load: ResMut<SaveLoadUi>,
    mut route_transition: ResMut<MenuRouteTransition>,
    mut page_transition: ResMut<SettingsPageTransition>,
) {
    let previous_route = active_route(&save_load, &ui);
    if input.pressed(ButtonAction::System) {
        ui.open = !ui.open;
        if ui.open {
            save_load.mode = None;
        }
    }
    let next_route = active_route(&save_load, &ui);
    if let (Some(from), Some(to)) = (previous_route, next_route)
        && (from == MenuHeaderActive::Config || to == MenuHeaderActive::Config)
    {
        route_transition.begin(from, to);
    }
    if ui.open
        && (keys.just_pressed(KeyCode::Escape)
            || back
                .iter()
                .any(|interaction| *interaction == Interaction::Pressed))
    {
        ui.open = false;
    }
    if !ui.open {
        page_transition.reset();
    }
}

pub fn settings_open(ui: Res<SettingsUi>) -> bool {
    ui.open
}

pub fn handle_settings_page(
    buttons: Query<(&Interaction, &SettingsPageButton), Changed<Interaction>>,
    mut ui: ResMut<SettingsUi>,
    mut transition: ResMut<SettingsPageTransition>,
) {
    for (interaction, page) in &buttons {
        if *interaction == Interaction::Pressed && ui.page != page.0 {
            transition.begin(ui.page, page.0);
            ui.page = page.0;
        }
    }
}

pub fn sync_settings(
    ui: Res<SettingsUi>,
    settings: Res<RuntimeSettings>,
    save_load: Res<SaveLoadUi>,
    mut context: SettingsSyncContext,
) {
    if !ui.is_changed() {
        return;
    }
    for (entity, _) in &mut context.roots {
        if !ui.open
            && !context.route_transition.involves(MenuHeaderActive::Config)
            && let Ok(mut fade) = context.fades.get_mut(entity)
        {
            fade.target = 0.0;
        }
    }
    if !ui.open {
        if save_load.mode.is_none() && !context.route_transition.involves(MenuHeaderActive::Config)
        {
            for (mut watermark, _) in &mut context.watermarks {
                watermark.hide();
            }
        }
        for (entity, _) in &mut context.proxies {
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                fade.target = f32::from(save_load.mode.is_some());
            }
        }
        return;
    }
    if !context.roots.is_empty() {
        let switching = context.route_transition.is_animating();
        if !switching {
            for (_, mut visibility) in &mut context.save_roots {
                *visibility = Visibility::Hidden;
            }
        }
        for (mut watermark, mut label) in &mut context.watermarks {
            watermark.show_label(&mut label, "CONFIG");
        }
        for (entity, mut visibility) in &mut context.roots {
            *visibility = Visibility::Inherited;
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                if switching {
                    fade.current = 1.0;
                }
                fade.target = 1.0;
            }
        }
        for (entity, mut visibility) in &mut context.proxies {
            *visibility = Visibility::Inherited;
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                fade.target = 1.0;
            }
        }
        return;
    }
    let (Ok(camera), Ok(blur_camera)) = (context.camera.single(), context.blur_camera.single())
    else {
        return;
    };
    let switching = context.route_transition.is_animating();
    if !switching {
        for (_, mut visibility) in &mut context.save_roots {
            *visibility = Visibility::Hidden;
        }
    }
    let font = context.fonts.text.clone();
    let icon_font = context.fonts.icons.clone();
    if context.proxies.is_empty() {
        context
            .commands
            .spawn((
                Name::new("settings_blur"),
                SettingsBlurProxy,
                crate::ui::save_load::SaveLoadBlurProxy,
                MenuBlur,
                PersistentMenu,
                if switching {
                    MenuFade::visible()
                } else {
                    MenuFade::entering()
                },
                UiBlurSource,
                BlurStrength(if switching {
                    crate::ui::FULLSCREEN_BLUR_STRENGTH
                } else {
                    0.0
                }),
                fill_node(),
                BackgroundColor(Color::srgba(
                    0.0,
                    0.0,
                    0.0,
                    if switching {
                        crate::ui::MENU_BACKDROP_ALPHA
                    } else {
                        0.0
                    },
                )),
                GlobalZIndex(179),
                UiTargetCamera(blur_camera),
                RenderLayers::layer(1),
            ))
            .with_children(|proxy| spawn_menu_watermark(proxy, "CONFIG", &font));
    } else {
        for (entity, mut visibility) in &mut context.proxies {
            *visibility = Visibility::Inherited;
            if let Ok(mut fade) = context.fades.get_mut(entity) {
                fade.target = 1.0;
            }
        }
        if context.watermarks.is_empty()
            && let Some(proxy) = context.save_proxies.iter().next()
        {
            context
                .commands
                .entity(proxy)
                .with_children(|proxy| spawn_menu_watermark(proxy, "CONFIG", &font));
        }
    }
    context
        .commands
        .spawn((
            Name::new("system_settings"),
            SettingsRoot,
            SettingsEntryPending,
            MenuSurface::config(),
            PersistentMenu,
            if switching {
                MenuFade::visible()
            } else {
                MenuFade::entering()
            },
            surface_transform(&MenuSurface::config(), switching),
            root_node(),
            BackgroundColor(Color::NONE),
            FocusPolicy::Block,
            GlobalZIndex(180),
            UiTargetCamera(camera),
            RenderLayers::layer(2),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            spawn_header(root, MenuHeaderActive::Config, &font, &icon_font);
            spawn_options_content(
                root,
                &ui,
                &settings,
                &context.config,
                &context.content,
                &font,
                &icon_font,
            );
        });
}

pub fn begin_settings_entry(
    mut commands: Commands,
    mut roots: Query<
        (
            Entity,
            &mut Visibility,
            &mut MenuFade,
            &MenuSurface,
            &mut UiTransform,
        ),
        With<SettingsEntryPending>,
    >,
) {
    for (entity, mut visibility, mut fade, surface, mut transform) in &mut roots {
        fade.current = 0.0;
        fade.target = 1.0;
        *transform = surface_transform(surface, false);
        *visibility = Visibility::Inherited;
        commands.entity(entity).remove::<SettingsEntryPending>();
    }
}

fn spawn_options_content(
    root: &mut ChildSpawnerCommands,
    ui: &SettingsUi,
    settings: &RuntimeSettings,
    config: &GameConfigResource,
    project: &ContentProjectResource,
    font: &Handle<Font>,
    icon_font: &Handle<Font>,
) {
    root.spawn((
        SettingsContent,
        UiTransform::default(),
        Node {
            position_type: PositionType::Relative,
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            ..default()
        },
    ))
    .with_children(|options| {
        options
            .spawn((Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                padding: UiRect::axes(Val::Px(27.0), Val::Px(13.5)),
                ..default()
            },))
            .with_children(|body| {
                body.spawn((Node {
                    width: Val::Percent(12.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    ..default()
                },))
                    .with_children(|pages| {
                        let locale = settings.locale;
                        spawn_page_button(
                            pages,
                            tr(locale, UiText::System),
                            SettingsPage::System,
                            ui,
                            font,
                        );
                        spawn_page_button(
                            pages,
                            tr(locale, UiText::Display),
                            SettingsPage::Display,
                            ui,
                            font,
                        );
                        spawn_page_button(
                            pages,
                            tr(locale, UiText::Audio),
                            SettingsPage::Audio,
                            ui,
                            font,
                        );
                        spawn_page_button(
                            pages,
                            tr(locale, UiText::About),
                            SettingsPage::About,
                            ui,
                            font,
                        );
                    });
                body.spawn((Node {
                    position_type: PositionType::Relative,
                    flex_grow: 1.0,
                    height: Val::Percent(100.0),
                    padding: UiRect::left(Val::Px(24.0)),
                    overflow: Overflow::visible(),
                    ..default()
                },))
                    .with_children(|content| {
                        for page in [
                            SettingsPage::System,
                            SettingsPage::Display,
                            SettingsPage::Audio,
                            SettingsPage::About,
                        ] {
                            spawn_settings_page(
                                content,
                                page,
                                ui,
                                settings,
                                SettingsProjectContext {
                                    config,
                                    content: project,
                                },
                                font,
                                icon_font,
                            );
                        }
                    });
            });
    });
}

pub fn animate_watermark(
    time: Res<Time>,
    mut watermarks: Query<(&mut SettingsWatermark, &mut Text, &mut TextColor)>,
) {
    let amount = exp_lerp(time.delta_secs(), 34.0);
    for (mut watermark, mut text, mut color) in &mut watermarks {
        if (watermark.current - watermark.target).abs() >= 0.001 {
            watermark.current += (watermark.target - watermark.current) * amount;
            if (watermark.target - watermark.current).abs() < 0.001 {
                watermark.current = watermark.target;
            }
        }
        if watermark.current == 0.0
            && watermark.target == 0.0
            && let Some(label) = watermark.pending_label.take()
        {
            text.0 = label;
            watermark.target = 1.0;
        }
        color.0 = Color::srgba(1.0, 1.0, 1.0, 0.075 * watermark.current);
    }
}

#[derive(SystemParam)]
pub(crate) struct SettingsVisualFadeContext<'w, 's> {
    roots: Query<'w, 's, (Entity, &'static MenuFade, &'static Visibility), With<SettingsRoot>>,
    parents: Query<'w, 's, &'static ChildOf>,
    texts: Query<'w, 's, (Entity, &'static mut TextColor)>,
    backgrounds: Query<'w, 's, (Entity, &'static mut BackgroundColor)>,
    outlines: Query<'w, 's, (Entity, &'static mut Outline)>,
}

pub fn fade_settings_visuals(
    ui: Res<SettingsUi>,
    route_transition: Res<MenuRouteTransition>,
    mut context: SettingsVisualFadeContext,
    mut cache: Local<SettingsVisualFadeCache>,
) {
    let Ok((root, fade, visibility)) = context.roots.single() else {
        return;
    };
    let belongs_to_settings = |entity: Entity| {
        let mut current = entity;
        while let Ok(parent) = context.parents.get(current) {
            current = parent.parent();
            if current == root {
                return true;
            }
        }
        false
    };

    if *visibility == Visibility::Hidden {
        return;
    }
    let fading =
        (!ui.open && !route_transition.involves(MenuHeaderActive::Config)) || fade.current < 0.999;
    let alpha = smoothstep(fade.current);
    let text_alpha = if fade.target > fade.current {
        smoothstep(fade.current.powf(0.72))
    } else {
        alpha
    };

    if fading {
        if cache.settled {
            cache.text_alpha.clear();
            cache.background_alpha.clear();
            cache.outline_alpha.clear();
        }
        cache.settled = false;
        for (entity, mut color) in &mut context.texts {
            if belongs_to_settings(entity) {
                let base = *cache
                    .text_alpha
                    .entry(entity)
                    .or_insert_with(|| color.0.alpha());
                color.0 = color.0.with_alpha(base * text_alpha);
            }
        }
        for (entity, mut background) in &mut context.backgrounds {
            if belongs_to_settings(entity) {
                let base = *cache
                    .background_alpha
                    .entry(entity)
                    .or_insert_with(|| background.0.alpha());
                background.0 = background.0.with_alpha(base * alpha);
            }
        }
        for (entity, mut outline) in &mut context.outlines {
            if belongs_to_settings(entity) {
                let base = *cache
                    .outline_alpha
                    .entry(entity)
                    .or_insert_with(|| outline.color.alpha());
                outline.color = outline.color.with_alpha(base * alpha);
            }
        }
        return;
    }

    if cache.settled {
        return;
    }

    for (entity, mut color) in &mut context.texts {
        if !belongs_to_settings(entity) {
            continue;
        }
        if let Some(base) = cache.text_alpha.get(&entity) {
            color.0 = color.0.with_alpha(*base);
        }
    }
    for (entity, mut background) in &mut context.backgrounds {
        if !belongs_to_settings(entity) {
            continue;
        }
        if let Some(base) = cache.background_alpha.get(&entity) {
            background.0 = background.0.with_alpha(*base);
        }
    }
    for (entity, mut outline) in &mut context.outlines {
        if !belongs_to_settings(entity) {
            continue;
        }
        if let Some(base) = cache.outline_alpha.get(&entity) {
            outline.color = outline.color.with_alpha(*base);
        }
    }
    cache.text_alpha.clear();
    cache.background_alpha.clear();
    cache.outline_alpha.clear();
    cache.settled = true;
}

fn spawn_page_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    page: SettingsPage,
    ui: &SettingsUi,
    font: &Handle<Font>,
) {
    let active = ui.page == page;
    parent.spawn((
        Button,
        UiSoundStyle::Switch,
        SettingsPageButton(page),
        SettingsPageButtonVisual(if active {
            PAGE_TEXT_ACTIVE
        } else {
            PAGE_TEXT_IDLE
        }),
        Node {
            height: Val::Px(63.0),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(4.5)),
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![(
            SettingsPageLabel,
            Text::new(label),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::from(36.0),
                weight: FontWeight::BOLD,
                ..default()
            },
            TextColor(Color::srgba(
                1.0,
                1.0,
                1.0,
                if active {
                    PAGE_TEXT_ACTIVE
                } else {
                    PAGE_TEXT_IDLE
                },
            )),
        )],
    ));
}

fn spawn_settings_page(
    parent: &mut ChildSpawnerCommands,
    page: SettingsPage,
    ui: &SettingsUi,
    settings: &RuntimeSettings,
    project: SettingsProjectContext<'_>,
    font: &Handle<Font>,
    icon_font: &Handle<Font>,
) {
    let active = ui.page == page;
    parent
        .spawn((
            SettingsPagePanel { page },
            UiTransform::default(),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(78.0),
                height: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(21.0), Val::Px(9.0)),
                grid_template_columns: RepeatedGridTrack::flex(SETTINGS_COLUMNS, 1.0),
                column_gap: Val::Px(SETTINGS_COLUMN_GAP),
                row_gap: Val::Px(SETTINGS_ROW_GAP),
                align_items: AlignItems::FlexStart,
                align_content: AlignContent::FlexStart,
                // All pages use the same available width. Reserving a scroll
                // gutter only on DISPLAY/AUDIO shifted their column tracks.
                overflow: Overflow::visible(),
                display: if active {
                    settings_page_display(page)
                } else {
                    Display::None
                },
                ..default()
            },
        ))
        .with_children(|content| match page {
            SettingsPage::System => {
                let auto_play = SYSTEM_SLIDERS[0];
                spawn_row(
                    content,
                    font,
                    tr(settings.locale, auto_play.kind.label()),
                    auto_play.kind,
                    auto_play.kind.ratio(settings),
                    SettingsGridCell::at(1, 1),
                );
                spawn_skip_row(content, settings, font, SettingsGridCell::at(2, 1));
                spawn_language_row(
                    content,
                    settings,
                    font,
                    icon_font,
                    SettingsGridCell::at(3, 1),
                );
                spawn_choice_row(
                    content,
                    tr(settings.locale, UiText::ClearOrRestore),
                    &[
                        (
                            tr(settings.locale, UiText::ClearSaves),
                            SettingAction::ClearSaves,
                        ),
                        (
                            tr(settings.locale, UiText::ResetSettings),
                            SettingAction::ResetSettings,
                        ),
                        (
                            tr(settings.locale, UiText::ClearAll),
                            SettingAction::ClearAll,
                        ),
                    ],
                    usize::MAX,
                    true,
                    font,
                    SettingsGridCell::spanning(1, 2, 2),
                );
                spawn_choice_row(
                    content,
                    tr(settings.locale, UiText::ImportExport),
                    &[
                        (
                            tr(settings.locale, UiText::Export),
                            SettingAction::ExportData,
                        ),
                        (
                            tr(settings.locale, UiText::Import),
                            SettingAction::ImportData,
                        ),
                    ],
                    usize::MAX,
                    false,
                    font,
                    SettingsGridCell::at(3, 2),
                );
            }
            SettingsPage::Display => {
                spawn_fullscreen_row(content, settings, font, SettingsGridCell::at(1, 1));
                spawn_text_size_row(content, settings, font, SettingsGridCell::at(2, 1));
                spawn_sliders(content, DISPLAY_SLIDERS, settings, font, 2);
                spawn_text_preview(content, settings, font, SettingsGridCell::spanning(1, 3, 3));
            }
            SettingsPage::Audio => spawn_sliders(content, AUDIO_SLIDERS, settings, font, 0),
            SettingsPage::About => spawn_about_page(content, project, settings.locale, font),
        });
}

fn settings_page_display(page: SettingsPage) -> Display {
    if page == SettingsPage::About {
        Display::Flex
    } else {
        Display::Grid
    }
}

fn spawn_sliders(
    content: &mut ChildSpawnerCommands,
    specs: &[SliderSpec],
    settings: &RuntimeSettings,
    font: &Handle<Font>,
    start_index: usize,
) {
    for (offset, spec) in specs.iter().enumerate() {
        spawn_row(
            content,
            font,
            tr(settings.locale, spec.kind.label()),
            spec.kind,
            spec.kind.ratio(settings),
            SettingsGridCell::from_index(start_index + offset),
        );
    }
}

fn spawn_about_page(
    content: &mut ChildSpawnerCommands,
    project: SettingsProjectContext<'_>,
    locale: UiLocale,
    font: &Handle<Font>,
) {
    let config = project.config;
    let project_description = if config.project.description.trim().is_empty() {
        tr(locale, UiText::NoProjectDescription)
    } else {
        config.project.description.trim()
    };
    let loader_tree = loader_tree(project);
    let runtime = format!(
        "crabgal {}  ·  {} / {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );

    content
        .spawn((Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            padding: UiRect::new(Val::Px(9.0), Val::Px(36.0), Val::Px(9.0), Val::Px(18.0)),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(60.0),
            ..default()
        },))
        .with_children(|page| {
            page.spawn((Node {
                width: Val::Percent(48.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                ..default()
            },))
                .with_children(|engine| {
                    engine.spawn(about_title(tr(locale, UiText::AboutCrabgal), font, 48.0));
                    engine.spawn(about_copy(
                        tr(locale, UiText::EngineDescription),
                        font,
                        22.5,
                        0.58,
                    ));
                    spawn_about_section(
                        engine,
                        tr(locale, UiText::VersionSystem),
                        &runtime,
                        font,
                        Val::Px(42.0),
                    );
                    spawn_repository_link(engine, locale, font);
                });

            page.spawn((Node {
                width: Val::Percent(46.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                ..default()
            },))
                .with_children(|project| {
                    project.spawn(about_title(tr(locale, UiText::CurrentProject), font, 39.0));
                    project.spawn(about_title(config.title.clone(), font, 31.5));
                    project.spawn(about_copy(project_description, font, 22.5, 0.62));
                    spawn_about_section(
                        project,
                        tr(locale, UiText::Loader),
                        &loader_tree,
                        font,
                        Val::Px(36.0),
                    );
                });
        });
}

fn loader_tree(project: SettingsProjectContext<'_>) -> String {
    let config = project.config;
    let mut lines = vec!["ASSET".to_owned()];
    for source in &config.adapter.asset {
        lines.push(format!(
            "  {}",
            format_loader_source(&source.format, &source.path)
        ));
    }
    let script = project
        .content
        .project_adapter()
        .unwrap_or(config.adapter.script.as_str());
    lines.push(format!("SCRIPT\n  [{script}]"));
    lines.push(format!("STORE\n  [{}]", config.adapter.store));
    lines.join("\n")
}

fn format_loader_source(adapter: &str, path: &str) -> String {
    let path = path.trim().trim_end_matches('/');
    if path.is_empty() || path == "." {
        return format!("[{adapter}]");
    }
    let path = path.strip_prefix("./").unwrap_or(path);
    format!("[{adapter}]/{path}")
}

fn spawn_repository_link(parent: &mut ChildSpawnerCommands, locale: UiLocale, font: &Handle<Font>) {
    parent
        .spawn((Node {
            width: Val::Percent(100.0),
            margin: UiRect::top(Val::Px(27.0)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(7.5),
            ..default()
        },))
        .with_children(|section| {
            section.spawn(about_title(tr(locale, UiText::Repository), font, 25.5));
            section
                .spawn((
                    Button,
                    AboutRepositoryLink,
                    AboutRepositoryVisual {
                        underline_width: 0.0,
                        text_alpha: 0.58,
                    },
                    Node {
                        position_type: PositionType::Relative,
                        align_self: AlignSelf::FlexStart,
                        padding: UiRect::vertical(Val::Px(6.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|link| {
                    link.spawn((
                        AboutRepositoryUnderline,
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::ZERO,
                            bottom: Val::ZERO,
                            width: Val::ZERO,
                            height: Val::Px(1.5),
                            ..default()
                        },
                        FocusPolicy::Pass,
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
                    ));
                    link.spawn((
                        AboutRepositoryLabel,
                        ZIndex(1),
                        text_weight(
                            "github.com/shiftz300/crabgal",
                            font,
                            20.25,
                            0.58,
                            FontWeight::NORMAL,
                        ),
                    ));
                });
        });
}

pub fn handle_about_repository_link(links: AboutRepositoryLinkQuery) {
    for interaction in &links {
        if *interaction == Interaction::Pressed {
            bevy::tasks::IoTaskPool::get()
                .spawn(async {
                    if let Err(error) = webbrowser::open("https://github.com/shiftz300/crabgal") {
                        log::error!("failed to open crabgal repository: {error}");
                    }
                })
                .detach();
        }
    }
}

pub fn animate_about_repository_link(
    time: Res<Time>,
    mut links: AboutRepositoryAnimationQuery,
    mut labels: Query<&mut TextColor, With<AboutRepositoryLabel>>,
    mut underlines: Query<&mut Node, With<AboutRepositoryUnderline>>,
) {
    let amount = exp_lerp(time.delta_secs(), OPTION_TRANSITION_RATE);
    for (interaction, mut visual, children) in &mut links {
        if !visual.is_animating(*interaction) {
            continue;
        }
        let (target_underline, target_text) = about_repository_targets(*interaction);
        visual.underline_width += (target_underline - visual.underline_width) * amount;
        visual.text_alpha += (target_text - visual.text_alpha) * amount;
        for child in children.iter() {
            if let Ok(mut node) = underlines.get_mut(child) {
                node.width = Val::Percent(visual.underline_width);
            }
            if let Ok(mut color) = labels.get_mut(child) {
                color.0 = Color::srgba(1.0, 1.0, 1.0, visual.text_alpha);
            }
        }
    }
}

fn about_repository_targets(interaction: Interaction) -> (f32, f32) {
    match interaction {
        Interaction::None => (0.0, 0.58),
        Interaction::Hovered => (100.0, 0.84),
        Interaction::Pressed => (100.0, 1.0),
    }
}

fn spawn_about_section(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    value: &str,
    font: &Handle<Font>,
    top: Val,
) {
    parent
        .spawn((Node {
            width: Val::Percent(100.0),
            margin: UiRect::top(top),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(7.5),
            ..default()
        },))
        .with_children(|section| {
            section.spawn(about_title(label, font, 25.5));
            section.spawn(about_copy(value, font, 20.25, 0.52));
        });
}

fn about_title(text: impl Into<String>, font: &Handle<Font>, size: f32) -> impl Bundle {
    text_weight(text, font, size, 0.76, FontWeight::BOLD)
}

fn about_copy(text: impl Into<String>, font: &Handle<Font>, size: f32, alpha: f32) -> impl Bundle {
    (
        Node {
            max_width: Val::Percent(100.0),
            ..default()
        },
        text_weight(text, font, size, alpha, FontWeight::NORMAL),
    )
}

fn spawn_skip_row(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
    cell: SettingsGridCell,
) {
    spawn_choice_row(
        content,
        tr(settings.locale, UiText::SkipMode),
        &[
            (
                tr(settings.locale, UiText::Read),
                SettingAction::SetSkip(false),
            ),
            (
                tr(settings.locale, UiText::All),
                SettingAction::SetSkip(true),
            ),
        ],
        usize::from(settings.skip_all),
        false,
        font,
        cell,
    );
}

fn spawn_language_row(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
    icon_font: &Handle<Font>,
    cell: SettingsGridCell,
) {
    content
        .spawn((
            Node {
                width: Val::Percent(100.0),
                grid_column: cell.column(),
                grid_row: cell.row(),
                min_height: Val::Px(84.0),
                margin: UiRect::vertical(Val::Px(4.5)),
                padding: UiRect::all(Val::Px(4.5)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                ..default()
            },
            ZIndex(30),
        ))
        .with_children(|row| {
            row.spawn(setting_text(
                tr(settings.locale, UiText::Language),
                font,
                SETTING_LABEL_SIZE,
                true,
            ));
            row.spawn((
                Node {
                    position_type: PositionType::Relative,
                    width: Val::Px(180.0),
                    height: Val::Px(43.5),
                    margin: UiRect::top(Val::Px(7.5)),
                    ..default()
                },
                ZIndex(30),
            ))
            .with_children(|dropdown| {
                dropdown.spawn((
                    Button,
                    UiSoundStyle::Switch,
                    LanguageDropdownButton,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        padding: UiRect::horizontal(Val::Px(15.0)),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(15.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
                    children![
                        setting_text(
                            settings.locale.native_name(),
                            font,
                            SETTING_OPTION_SIZE,
                            false,
                        ),
                        (
                            LanguageDropdownIcon,
                            text_weight("\u{f282}", icon_font, 18.0, 0.78, FontWeight::NORMAL,),
                        ),
                    ],
                ));
                dropdown
                    .spawn((
                        LanguageDropdownOptions,
                        LanguageDropdownAnimation {
                            progress: 0.0,
                            target: 0.0,
                        },
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::ZERO,
                            top: Val::Px(43.5),
                            width: Val::Percent(100.0),
                            height: Val::ZERO,
                            display: Display::None,
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        FocusPolicy::Block,
                        BackgroundColor(Color::srgb(0.12, 0.12, 0.135)),
                        BoxShadow::new(
                            Color::srgba(0.0, 0.0, 0.0, 0.55),
                            Val::Px(0.0),
                            Val::Px(9.0),
                            Val::Px(0.0),
                            Val::Px(18.0),
                        ),
                        GlobalZIndex(181),
                    ))
                    .with_children(|options| {
                        for locale in UiLocale::ALL {
                            options.spawn((
                                Button,
                                UiSoundStyle::Switch,
                                LanguageDropdownOption(locale),
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(43.5),
                                    padding: UiRect::horizontal(Val::Px(15.0)),
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(if locale == settings.locale {
                                    Color::srgba(1.0, 1.0, 1.0, 0.1)
                                } else {
                                    Color::NONE
                                }),
                                children![setting_text(
                                    locale.native_name(),
                                    font,
                                    SETTING_OPTION_SIZE,
                                    false,
                                )],
                            ));
                        }
                    });
            });
        });
}

pub fn handle_language_dropdown(
    toggles: Query<&Interaction, (With<LanguageDropdownButton>, Changed<Interaction>)>,
    options: Query<(&Interaction, &LanguageDropdownOption), Changed<Interaction>>,
    mut context: LanguageDropdownContext,
) {
    let toggle = toggles
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed);
    let selected = options.iter().find_map(|(interaction, option)| {
        (*interaction == Interaction::Pressed).then_some(option.0)
    });
    if !toggle && selected.is_none() {
        return;
    }
    if let Some(locale) = selected
        && locale != context.settings.locale
    {
        context.settings.locale = locale;
        if let Err(error) =
            crate::storage::settings::persist(&context.settings, &context.project_root)
        {
            log::error!("failed to persist UI language: {error:#}");
        }
        for entity in &context.roots {
            context.commands.entity(entity).despawn();
        }
        context.ui.set_changed();
    }
    for (mut animation, mut node) in &mut context.menus {
        animation.target = if selected.is_some() || animation.target > 0.5 {
            0.0
        } else {
            node.display = Display::Flex;
            1.0
        };
    }
    for mut icon in &mut context.icons {
        icon.0 = if selected.is_some() || icon.0 == "\u{f286}" {
            "\u{f282}".into()
        } else {
            "\u{f286}".into()
        };
    }
}

pub fn animate_language_dropdown(
    time: Res<Time>,
    mut menus: Query<(&mut LanguageDropdownAnimation, &mut Node)>,
) {
    for (mut animation, mut node) in &mut menus {
        if !animation.is_animating() {
            if animation.target == 0.0 {
                node.display = Display::None;
            }
            continue;
        }
        node.display = Display::Flex;
        animation.progress +=
            (animation.target - animation.progress) * exp_lerp(time.delta_secs(), 20.0);
        if (animation.target - animation.progress).abs() < 0.001 {
            animation.progress = animation.target;
        }
        node.height = Val::Px(43.5 * UiLocale::ALL.len() as f32 * smoothstep(animation.progress));
    }
}

fn spawn_fullscreen_row(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
    cell: SettingsGridCell,
) {
    spawn_choice_row(
        content,
        tr(settings.locale, UiText::Fullscreen),
        &[
            (
                tr(settings.locale, UiText::On),
                SettingAction::SetFullscreen(true),
            ),
            (
                tr(settings.locale, UiText::Off),
                SettingAction::SetFullscreen(false),
            ),
        ],
        if settings.fullscreen { 0 } else { 1 },
        false,
        font,
        cell,
    );
}

fn spawn_text_size_row(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
    cell: SettingsGridCell,
) {
    spawn_choice_row(
        content,
        tr(settings.locale, UiText::TextSize),
        &[
            (
                tr(settings.locale, UiText::Small),
                SettingAction::SetTextSize(0),
            ),
            (
                tr(settings.locale, UiText::Medium),
                SettingAction::SetTextSize(1),
            ),
            (
                tr(settings.locale, UiText::Large),
                SettingAction::SetTextSize(2),
            ),
        ],
        usize::from(settings.text_size),
        false,
        font,
        cell,
    );
}

fn spawn_choice_row(
    content: &mut ChildSpawnerCommands,
    label: &str,
    choices: &[(&str, SettingAction)],
    selected: usize,
    vertical: bool,
    font: &Handle<Font>,
    cell: SettingsGridCell,
) {
    content
        .spawn((Node {
            width: Val::Percent(100.0),
            grid_column: cell.column(),
            grid_row: cell.row(),
            min_height: Val::Px(84.0),
            margin: UiRect::vertical(Val::Px(4.5)),
            padding: UiRect::all(Val::Px(4.5)),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexStart,
            ..default()
        },))
        .with_children(|row| {
            row.spawn(setting_text(label, font, SETTING_LABEL_SIZE, true));
            row.spawn((Node {
                margin: UiRect::top(Val::Px(7.5)),
                flex_direction: if vertical {
                    FlexDirection::Column
                } else {
                    FlexDirection::Row
                },
                column_gap: Val::Px(if vertical { 0.0 } else { 9.0 }),
                row_gap: Val::Px(if vertical { 9.0 } else { 0.0 }),
                align_items: AlignItems::FlexStart,
                ..default()
            },))
                .with_children(|buttons| {
                    for (index, (text, action)) in choices.iter().copied().enumerate() {
                        spawn_choice(buttons, text, action, index == selected, font);
                    }
                });
        });
}

fn spawn_text_preview(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
    cell: SettingsGridCell,
) {
    let size = match settings.text_size {
        0 => 23.25,
        2 => 31.5,
        _ => 27.0,
    };
    content
        .spawn((Node {
            width: Val::Percent(100.0),
            grid_column: cell.column(),
            grid_row: cell.row(),
            min_height: Val::Px(247.5),
            margin: UiRect::vertical(Val::Px(4.5)),
            padding: UiRect::all(Val::Px(4.5)),
            flex_direction: FlexDirection::Column,
            ..default()
        },))
        .with_children(|preview| {
            preview.spawn(setting_text(
                tr(settings.locale, UiText::TextPreview),
                font,
                SETTING_LABEL_SIZE,
                true,
            ));
            preview
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        margin: UiRect::top(Val::Px(9.0)),
                        padding: UiRect::axes(Val::Px(27.0), Val::Px(21.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(7.5),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(
                        0.0,
                        0.0,
                        0.03,
                        settings.textbox_opacity * 0.72,
                    )),
                    SettingPreviewSurface,
                ))
                .with_children(|box_| {
                    box_.spawn((
                        Node {
                            width: Val::Px(225.0),
                            height: Val::Px(43.5),
                            margin: UiRect::new(
                                Val::Px(-13.5),
                                Val::ZERO,
                                Val::Px(-9.0),
                                Val::ZERO,
                            ),
                            padding: UiRect::horizontal(Val::Px(16.5)),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.02, 0.7)),
                    ))
                    .with_child(setting_text(
                        tr(settings.locale, UiText::TextPreview),
                        font,
                        SETTING_LABEL_SIZE,
                        true,
                    ));
                    box_.spawn((
                        SettingPreviewText,
                        setting_text(
                            tr(settings.locale, UiText::PreviewDialogue),
                            font,
                            size,
                            false,
                        ),
                    ));
                });
        });
}

fn spawn_row(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    kind: SettingKind,
    ratio: f32,
    cell: SettingsGridCell,
) {
    root.spawn((Node {
        width: Val::Percent(100.0),
        grid_column: cell.column(),
        grid_row: cell.row(),
        min_height: Val::Px(96.0),
        margin: UiRect::vertical(Val::Px(4.5)),
        padding: UiRect::all(Val::Px(4.5)),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::FlexStart,
        ..default()
    },))
        .with_children(|row| {
            row.spawn(setting_text(label, font, SETTING_LABEL_SIZE, true));
            row.spawn((
                Button,
                UiSoundStyle::HoverOnly,
                SettingSlider(kind),
                Node {
                    position_type: PositionType::Relative,
                    width: Val::Percent(100.0),
                    max_width: Val::Px(375.0),
                    height: Val::Px(37.5),
                    margin: UiRect::top(Val::Px(7.5)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|slider| {
                slider.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(15.0),
                        width: Val::Percent(100.0),
                        height: Val::Px(7.5),
                        ..default()
                    },
                    BackgroundColor(Color::BLACK),
                    Outline::new(Val::Px(3.75), Val::ZERO, Color::srgba(1.0, 1.0, 1.0, 0.19)),
                ));
                slider.spawn((
                    SettingSliderThumb(kind),
                    SettingSliderThumbVisual(10.0),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(ratio.clamp(0.0, 1.0) * 90.0),
                        top: Val::Px(3.75),
                        width: Val::Percent(10.0),
                        height: Val::Px(30.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.5)),
                ));
                slider
                    .spawn((
                        SettingValueBubble(kind),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(ratio.clamp(0.0, 1.0) * 90.0),
                            top: Val::Px(-31.5),
                            width: Val::Percent(10.0),
                            height: Val::Px(27.0),
                            display: Display::None,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border_radius: BorderRadius::all(Val::Px(4.5)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
                    ))
                    .with_child((
                        SettingValueText(kind),
                        Text::new(kind.value_text(ratio)),
                        TextFont {
                            font: font.clone().into(),
                            font_size: FontSize::from(16.5),
                            weight: FontWeight::BOLD,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
            });
        });
}

fn spawn_choice(
    parent: &mut ChildSpawnerCommands,
    text: &str,
    action: SettingAction,
    selected: bool,
    font: &Handle<Font>,
) {
    parent.spawn((
        Button,
        UiSoundStyle::Switch,
        action,
        SettingChoice(action),
        SettingChoiceVisual {
            selected,
            hovered: false,
            fill: if selected { 100.0 } else { 0.0 },
            text_alpha: if selected {
                OPTION_TEXT_ACTIVE
            } else {
                OPTION_TEXT_IDLE
            },
        },
        Node {
            min_width: Val::Px(96.0),
            height: Val::Px(43.5),
            padding: UiRect::horizontal(Val::Px(15.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![
            (
                SettingChoiceFill,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::ZERO,
                    top: Val::ZERO,
                    width: Val::Percent(if selected { 100.0 } else { 0.0 }),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, OPTION_FILL_ALPHA)),
                FocusPolicy::Pass,
            ),
            text_weight(
                text,
                font,
                SETTING_OPTION_SIZE,
                if selected {
                    OPTION_TEXT_ACTIVE
                } else {
                    OPTION_TEXT_IDLE
                },
                if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                },
            )
        ],
    ));
}

fn setting_text(
    text: impl Into<String>,
    font: &Handle<Font>,
    size: f32,
    bold: bool,
) -> impl Bundle {
    text_weight(
        text,
        font,
        size,
        0.78,
        if bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        },
    )
}

type TitleEntityQuery<'w, 's> = Query<
    'w,
    's,
    Entity,
    Or<(
        With<crate::ui::title::TitleRoot>,
        With<crate::ui::title::TitleBackground>,
    )>,
>;

#[derive(SystemParam)]
pub(crate) struct SettingActionContext<'w, 's> {
    commands: Commands<'w, 's>,
    actions: Query<'w, 's, (&'static Interaction, &'static SettingAction), Changed<Interaction>>,
    settings: ResMut<'w, RuntimeSettings>,
    toggles: ResMut<'w, ToggleStates>,
    pending_window: ResMut<'w, PendingWindowMode>,
    project_root: Res<'w, ProjectRoot>,
    store: Res<'w, crate::runtime::resources::StoreCodec>,
    state: ResMut<'w, crate::runtime::resources::GameState>,
    quick_preview: ResMut<'w, crate::ui::control_bar::QuickSavePreview>,
    save_previews: ResMut<'w, crate::ui::save_load::SavePreviewCache>,
    title_entities: TitleEntityQuery<'w, 's>,
}

pub fn handle_setting_action(context: SettingActionContext) {
    let SettingActionContext {
        mut commands,
        actions,
        mut settings,
        mut toggles,
        mut pending_window,
        project_root,
        store,
        mut state,
        mut quick_preview,
        mut save_previews,
        title_entities,
    } = context;
    let Some(action) = actions.iter().find_map(|(interaction, action)| {
        (*interaction == Interaction::Pressed).then_some(*action)
    }) else {
        return;
    };
    match action {
        SettingAction::SetSkip(value) => {
            settings.skip_all = value;
            toggles.skip_mode = if settings.skip_all {
                SkipMode::All
            } else {
                SkipMode::Read
            };
            toggles.skip = false;
        }
        SettingAction::SetFullscreen(value) => {
            settings.fullscreen = value;
            pending_window.target = Some(value);
            pending_window.delay_frames = 1;
        }
        SettingAction::SetTextSize(value) => settings.text_size = value.min(2),
        SettingAction::ClearSaves => {
            commands.insert_resource(crate::ui::dialog::DialogRequest::confirmation(
                tr(settings.locale, UiText::ConfirmClearSaves),
                crate::ui::dialog::DialogAction::ClearSaves,
            ));
            return;
        }
        SettingAction::ResetSettings => {
            commands.insert_resource(crate::ui::dialog::DialogRequest::confirmation(
                tr(settings.locale, UiText::ConfirmResetSettings),
                crate::ui::dialog::DialogAction::ResetSettings,
            ));
            return;
        }
        SettingAction::ClearAll => {
            commands.insert_resource(crate::ui::dialog::DialogRequest::confirmation(
                tr(settings.locale, UiText::ConfirmClearAll),
                crate::ui::dialog::DialogAction::ClearAll,
            ));
            return;
        }
        SettingAction::ExportData => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("crabgal backup", &["crabgal-backup"])
                .set_file_name("crabgal.crabgal-backup")
                .save_file()
            else {
                return;
            };
            if let Err(error) = crate::storage::backup::export(&project_root, &path) {
                log::error!("failed to export save data: {error:#}");
            }
            return;
        }
        SettingAction::ImportData => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("crabgal backup", &["crabgal-backup"])
                .pick_file()
            else {
                return;
            };
            if let Err(error) = crate::storage::backup::import(&project_root, &path) {
                log::error!("failed to import save data: {error:#}");
                return;
            }
            if let Some(mut imported) = crate::storage::settings::load(&project_root) {
                crate::storage::settings::sanitize(&mut imported);
                *settings = imported;
                toggles.skip = false;
                toggles.skip_mode = if settings.skip_all {
                    SkipMode::All
                } else {
                    SkipMode::Read
                };
                pending_window.target = Some(settings.fullscreen);
                pending_window.delay_frames = 1;
            }
            state.global_vars = crate::storage::profile::load(&project_root);
            state.read_dialogues = crate::storage::read_history::load(&project_root);
            state.unlocked_cg.clear();
            state.unlocked_bgm.clear();
            crate::storage::gallery::load(&mut state, &project_root);
            quick_preview.state = crate::storage::save::load_game(
                store.0.as_ref(),
                crate::storage::save::QUICK_SAVE_SLOT,
                &project_root,
            )
            .ok()
            .filter(|saved| saved.snapshot().program_fingerprint == state.program_fingerprint)
            .map(|saved| crate::ui::control_bar::QuickSaveSnapshot::from(saved.snapshot()));
            quick_preview.image = None;
            save_previews.clear();
            for entity in &title_entities {
                commands.entity(entity).despawn();
            }
            return;
        }
    }
    if let Err(error) = crate::storage::settings::persist(&settings, &project_root) {
        log::error!("failed to persist settings: {error:#}");
    }
}

pub(crate) fn reset_runtime_settings(
    settings: &mut RuntimeSettings,
    toggles: &mut ToggleStates,
    pending_window: &mut PendingWindowMode,
    project_root: &ProjectRoot,
) {
    *settings = RuntimeSettings::default();
    toggles.skip = false;
    toggles.skip_mode = SkipMode::Read;
    pending_window.target = Some(settings.fullscreen);
    pending_window.delay_frames = 1;
    if let Err(error) = crate::storage::settings::persist(settings, project_root) {
        log::error!("failed to restore default settings: {error:#}");
    }
}

pub fn apply_pending_window_mode(
    mut pending: ResMut<PendingWindowMode>,
    mut windows: Query<&mut Window>,
) {
    let Some(value) = pending.target else {
        return;
    };
    if pending.delay_frames > 0 {
        pending.delay_frames -= 1;
        return;
    }
    if let Ok(mut window) = windows.single_mut() {
        window.mode = if value {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };
    }
    pending.target = None;
}

pub fn handle_setting_sliders(
    sliders: Query<(
        &Interaction,
        &SettingSlider,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut settings: ResMut<RuntimeSettings>,
    project_root: Res<ProjectRoot>,
    mut drag: ResMut<ActiveSettingSlider>,
) {
    let Ok(window) = windows.single() else { return };
    // UI layout and UiGlobalTransform are expressed in physical pixels.
    // Using the logical cursor position on HiDPI displays offsets the hit
    // calculation and commonly clamps the slider to zero.
    if mouse.just_pressed(MouseButton::Left) {
        drag.kind = sliders
            .iter()
            .find(|(interaction, _, _, _)| {
                matches!(interaction, Interaction::Hovered | Interaction::Pressed)
            })
            .map(|(_, slider, _, _)| slider.0);
    }
    if mouse.pressed(MouseButton::Left)
        && let (Some(kind), Some(cursor)) = (drag.kind, window.physical_cursor_position())
    {
        for (_, slider, node, transform) in &sliders {
            if slider.0 != kind {
                continue;
            }
            let size = node.size();
            if size.x <= 0.0 {
                continue;
            }
            let left = transform.translation.x - size.x * 0.5;
            let ratio = ((cursor.x - left) / size.x).clamp(0.0, 1.0);
            if (slider.0.ratio(&settings) - ratio).abs() > 0.0005 {
                slider.0.set_ratio(&mut settings, ratio);
                drag.dirty = true;
            }
        }
    }
    if mouse.just_released(MouseButton::Left) {
        drag.kind = None;
        if drag.dirty {
            if let Err(error) = crate::storage::settings::persist(&settings, &project_root) {
                log::error!("failed to persist settings: {error:#}");
            }
            drag.dirty = false;
        }
    }
}

#[derive(SystemParam)]
pub(crate) struct SettingsVisualResources<'w> {
    time: Res<'w, Time>,
    settings: Res<'w, RuntimeSettings>,
    drag: Res<'w, ActiveSettingSlider>,
}

pub fn update_setting_visuals(
    resources: SettingsVisualResources,
    sliders: Query<(&Interaction, &SettingSlider)>,
    mut thumbs: Query<
        (
            &SettingSliderThumb,
            &mut SettingSliderThumbVisual,
            &mut Node,
            &mut BackgroundColor,
        ),
        Without<SettingChoice>,
    >,
    mut choices: Query<
        (
            &Interaction,
            &SettingChoice,
            &mut SettingChoiceVisual,
            &Children,
        ),
        Without<SettingSliderThumb>,
    >,
    mut fills: SettingChoiceFillQuery,
    mut choice_text: Query<(&mut TextColor, &mut TextFont)>,
) {
    let amount = exp_lerp(resources.time.delta_secs(), OPTION_TRANSITION_RATE);
    for (kind, mut visual, mut node, mut background) in &mut thumbs {
        let dragging = resources.drag.kind == Some(kind.0);
        let hovered = sliders.iter().any(|(interaction, slider)| {
            slider.0 == kind.0 && matches!(interaction, Interaction::Hovered | Interaction::Pressed)
        });
        let target_width = if hovered || dragging { 12.0 } else { 10.0 };
        if (visual.0 - target_width).abs() < 0.001
            && node.left
                == Val::Percent(
                    kind.0.ratio(&resources.settings).clamp(0.0, 1.0) * (100.0 - visual.0),
                )
        {
            continue;
        }
        if dragging {
            // Pointer motion owns the thumb while captured. Do not interpolate
            // its geometry underneath the cursor.
            visual.0 = target_width;
        } else {
            visual.0 += (target_width - visual.0) * amount;
        }
        node.width = Val::Percent(visual.0);
        node.left =
            Val::Percent(kind.0.ratio(&resources.settings).clamp(0.0, 1.0) * (100.0 - visual.0));
        background.0 = Color::srgba(1.0, 1.0, 1.0, if hovered { 0.67 } else { 0.5 });
    }
    for (interaction, choice, mut visual, children) in &mut choices {
        if !visual.is_animating(*interaction, choice.0, &resources.settings) {
            continue;
        }
        let selected = choice_is_selected(&resources.settings, choice.0);
        let hovered = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        visual.selected = selected;
        visual.hovered = hovered;
        let target_fill = if selected || hovered { 100.0 } else { 0.0 };
        let target_text = if selected || hovered {
            OPTION_TEXT_ACTIVE
        } else {
            OPTION_TEXT_IDLE
        };
        visual.fill += (target_fill - visual.fill) * amount;
        visual.text_alpha += (target_text - visual.text_alpha) * amount;
        for child in children.iter() {
            if let Ok((mut node, mut background)) = fills.get_mut(child) {
                node.width = Val::Percent(visual.fill);
                background.0 = Color::srgba(1.0, 1.0, 1.0, OPTION_FILL_ALPHA);
            }
            if let Ok((mut color, mut font)) = choice_text.get_mut(child) {
                color.0 = Color::srgba(1.0, 1.0, 1.0, visual.text_alpha);
                font.weight = if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                };
            }
        }
    }
}

pub fn update_setting_bubbles(
    settings: Res<RuntimeSettings>,
    sliders: Query<(&Interaction, &SettingSlider)>,
    mut values: Query<(&SettingValueText, &mut Text)>,
    mut bubbles: Query<(&SettingValueBubble, &mut Node), Without<SettingSliderThumb>>,
) {
    for (kind, mut text) in &mut values {
        let value = kind.0.value_text(kind.0.ratio(&settings));
        if text.0 != value {
            text.0 = value;
        }
    }
    for (kind, mut node) in &mut bubbles {
        let active = sliders.iter().any(|(interaction, slider)| {
            slider.0 == kind.0 && matches!(interaction, Interaction::Hovered | Interaction::Pressed)
        });
        let display = if active { Display::Flex } else { Display::None };
        let left = Val::Percent(kind.0.ratio(&settings).clamp(0.0, 1.0) * 90.0);
        if node.display != display {
            node.display = display;
        }
        if node.left != left {
            node.left = left;
        }
    }
}

pub fn update_setting_preview(
    settings: Res<RuntimeSettings>,
    mut surfaces: Query<&mut BackgroundColor, With<SettingPreviewSurface>>,
    mut texts: Query<&mut TextFont, With<SettingPreviewText>>,
) {
    if !settings.is_changed() {
        return;
    }
    for mut background in &mut surfaces {
        background.0 = Color::srgba(0.0, 0.0, 0.03, settings.textbox_opacity * 0.72);
    }
    let size = match settings.text_size {
        0 => 23.25,
        2 => 31.5,
        _ => 27.0,
    };
    for mut font in &mut texts {
        font.font_size = FontSize::from(size);
    }
}

fn choice_is_selected(settings: &RuntimeSettings, action: SettingAction) -> bool {
    match action {
        SettingAction::SetSkip(value) => settings.skip_all == value,
        SettingAction::SetFullscreen(value) => settings.fullscreen == value,
        SettingAction::SetTextSize(value) => settings.text_size == value,
        SettingAction::ClearSaves
        | SettingAction::ResetSettings
        | SettingAction::ClearAll
        | SettingAction::ExportData
        | SettingAction::ImportData => false,
    }
}

pub fn update_settings_pages(
    time: Res<Time>,
    ui: Res<SettingsUi>,
    mut transition: ResMut<SettingsPageTransition>,
    mut buttons: Query<(
        &Interaction,
        &SettingsPageButton,
        &mut SettingsPageButtonVisual,
        &Children,
    )>,
    mut labels: Query<&mut TextColor, With<SettingsPageLabel>>,
    mut panels: Query<(&mut SettingsPagePanel, &mut Node, &mut UiTransform)>,
) {
    let amount = exp_lerp(time.delta_secs(), OPTION_TRANSITION_RATE);
    for (interaction, page, mut visual, children) in &mut buttons {
        if !visual.is_animating(*interaction, page.0, ui.page) {
            continue;
        }
        let active = ui.page == page.0;
        let hovered = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        let target = if active {
            PAGE_TEXT_ACTIVE
        } else if hovered {
            PAGE_TEXT_HOVER
        } else {
            PAGE_TEXT_IDLE
        };
        visual.0 += (target - visual.0) * amount;
        for child in children.iter() {
            if let Ok(mut color) = labels.get_mut(child) {
                color.0 = Color::srgba(1.0, 1.0, 1.0, visual.0);
            }
        }
    }

    let (Some(from), Some(to)) = (transition.from, transition.to) else {
        for (panel, mut node, mut transform) in &mut panels {
            node.display = if panel.page == ui.page {
                settings_page_display(panel.page)
            } else {
                Display::None
            };
            transform.translation = Val2::ZERO;
            transform.scale = Vec2::ONE;
        }
        return;
    };

    transition.elapsed =
        (transition.elapsed + time.delta_secs()).min(SettingsPageTransition::SECONDS);
    let progress = transition.elapsed / SettingsPageTransition::SECONDS;
    let eased = ease_in_out_cubic(progress);
    let direction = (to.index() - from.index()).signum() as f32;

    for (panel, mut node, mut transform) in &mut panels {
        let offset = if panel.page == from {
            Some(-direction * 100.0 * eased)
        } else if panel.page == to {
            Some(direction * 100.0 * (1.0 - eased))
        } else {
            None
        };
        if let Some(offset) = offset {
            node.display = settings_page_display(panel.page);
            transform.translation = Val2::percent(0.0, offset);
            transform.scale = Vec2::ONE;
        } else {
            node.display = Display::None;
            transform.translation = Val2::ZERO;
        }
    }

    if transition.elapsed >= SettingsPageTransition::SECONDS {
        transition.reset();
        for (panel, mut node, mut transform) in &mut panels {
            node.display = if panel.page == ui.page {
                settings_page_display(panel.page)
            } else {
                Display::None
            };
            transform.translation = Val2::ZERO;
        }
    }
}
