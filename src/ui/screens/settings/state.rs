//! Settings state; registered through the parent facade.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct SettingsProjectContext<'a> {
    pub(super) config: &'a GameConfigResource,
    pub(super) content: &'a ContentProjectResource,
}

#[derive(Clone, Copy)]
pub(super) struct SettingsGridCell {
    pub(super) column: i16,
    pub(super) row: i16,
    pub(super) span: u16,
}

impl SettingsGridCell {
    pub(super) const fn at(column: i16, row: i16) -> Self {
        Self {
            column,
            row,
            span: 1,
        }
    }

    pub(super) const fn spanning(column: i16, row: i16, span: u16) -> Self {
        Self { column, row, span }
    }

    pub(super) const fn from_index(index: usize) -> Self {
        Self::at(
            (index % SETTINGS_COLUMNS as usize) as i16 + 1,
            (index / SETTINGS_COLUMNS as usize) as i16 + 1,
        )
    }

    pub(super) fn column(self) -> GridPlacement {
        GridPlacement::start_span(self.column, self.span)
    }

    pub(super) fn row(self) -> GridPlacement {
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
    pub(super) target: Option<bool>,
    pub(super) delay_frames: u8,
}

impl PendingWindowMode {
    pub(crate) fn is_pending(&self) -> bool {
        self.target.is_some()
    }
}

#[derive(Resource, Default)]
pub(crate) struct ActiveSettingSlider {
    pub(super) kind: Option<SettingKind>,
    pub(super) touch: Option<Entity>,
    pub(super) dirty: bool,
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
pub(crate) struct SettingsContent;

#[derive(Component)]
pub(crate) struct SettingsBlurProxy;

#[derive(Component)]
pub(crate) struct SettingsWatermark {
    pub(super) current: f32,
    pub(super) target: f32,
    pub(super) pending_label: Option<String>,
}

impl SettingsWatermark {
    pub(super) fn entering() -> Self {
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

    pub(crate) fn show_label(&mut self, text: &Text, label: &str) {
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

    pub(crate) fn set_progress(&mut self, progress: f32) {
        let progress = progress.clamp(0.0, 1.0);
        self.current = progress;
        self.target = progress;
        self.pending_label = None;
    }
}

#[derive(Default)]
pub(crate) struct SettingsVisualFadeCache {
    pub(super) text_alpha: HashMap<Entity, f32>,
    pub(super) background_alpha: HashMap<Entity, f32>,
    pub(super) outline_alpha: HashMap<Entity, f32>,
    pub(super) settled: bool,
}

pub(super) type SettingsRootVisibilityQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut Visibility),
    (
        With<SettingsRoot>,
        Without<SettingsBlurProxy>,
        Without<crate::ui::save_load::SaveLoadRoot>,
    ),
>;
pub(super) type SettingsProxyVisibilityQuery<'w, 's> = Query<
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
    pub(super) const fn index(self) -> i8 {
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
    pub(super) from: Option<SettingsPage>,
    pub(super) to: Option<SettingsPage>,
    pub(super) elapsed: f32,
}

impl SettingsPageTransition {
    pub(super) const SECONDS: f32 = PAGE_SLIDE_SECONDS;

    pub(super) fn begin(&mut self, from: SettingsPage, to: SettingsPage) {
        self.from = Some(from);
        self.to = Some(to);
        self.elapsed = 0.0;
    }

    pub(super) fn reset(&mut self) {
        self.from = None;
        self.to = None;
        self.elapsed = 0.0;
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.from.is_some() && self.to.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LocaleTransitionPhase {
    FadeOut,
    FadeIn,
}

#[derive(Resource, Default)]
pub(crate) struct SettingsLocaleTransition {
    pub(super) target: Option<UiLocale>,
    pub(super) phase: Option<LocaleTransitionPhase>,
}

impl SettingsLocaleTransition {
    pub(super) fn begin(&mut self, target: UiLocale) {
        self.target = Some(target);
        self.phase = Some(LocaleTransitionPhase::FadeOut);
    }

    pub(super) fn begin_fade_in(&mut self) {
        self.phase = Some(LocaleTransitionPhase::FadeIn);
    }

    pub(super) fn finish(&mut self) {
        self.target = None;
        self.phase = None;
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.phase.is_some()
    }

    pub(super) fn is_fading_in(&self) -> bool {
        self.phase == Some(LocaleTransitionPhase::FadeIn)
    }
}

#[derive(Component)]
pub(crate) struct SettingsPageButton(pub(crate) SettingsPage);

#[derive(Component)]
pub(crate) struct SettingsPageLabel;

#[derive(Component)]
pub(crate) struct SettingsPageButtonVisual(pub(super) f32);

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
    pub(super) page: SettingsPage,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingAction {
    SetSkip(bool),
    SetLanguage(UiLocale),
    SetFullscreen(bool),
    SetTextSize(u8),
    ClearSaves,
    ResetSettings,
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
    pub(super) fn label(self) -> UiText {
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

    pub(super) fn ratio(self, settings: &RuntimeSettings) -> f32 {
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

    pub(super) fn set_ratio(self, settings: &mut RuntimeSettings, ratio: f32) {
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

    pub(super) fn value_text(self, ratio: f32) -> String {
        match self {
            Self::TextSpeed => format!("{:.0}", 10.0 + ratio * 110.0),
            Self::AutoDelay => format!("{:.1}", 0.5 + ratio * 4.5),
            _ => format!("{}", (ratio * 100.0).round()),
        }
    }
}

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
pub(crate) struct AboutRepositoryLink;

#[derive(Component)]
pub(crate) struct AboutRepositoryLabel;

#[derive(Component)]
pub(crate) struct AboutRepositoryUnderline;

#[derive(Component)]
pub(crate) struct AboutRepositoryVisual {
    pub(super) underline_width: f32,
    pub(super) text_alpha: f32,
}

impl AboutRepositoryVisual {
    pub(crate) fn is_animating(&self, interaction: Interaction) -> bool {
        let (underline, text) = about_repository_targets(interaction);
        (self.underline_width - underline).abs() > 0.001 || (self.text_alpha - text).abs() > 0.001
    }
}

pub(super) type AboutRepositoryLinkQuery<'w, 's> =
    Query<'w, 's, &'static Interaction, (With<AboutRepositoryLink>, Changed<Interaction>)>;

pub(super) type AboutRepositoryAnimationQuery<'w, 's> = Query<
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
pub(crate) struct SettingChoiceVisual {
    pub(super) selected: bool,
    pub(super) hovered: bool,
    pub(super) fill: f32,
    pub(super) text_alpha: f32,
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

pub(super) type SettingChoiceFillQuery<'w, 's> = Query<
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
    pub(super) commands: Commands<'w, 's>,
    pub(super) roots: SettingsRootVisibilityQuery<'w, 's>,
    pub(super) proxies: SettingsProxyVisibilityQuery<'w, 's>,
    pub(super) camera: Query<'w, 's, Entity, With<DialogCamera>>,
    pub(super) blur_camera: Query<'w, 's, Entity, With<UiBlurCamera>>,
    pub(super) fonts: Res<'w, UiFonts>,
    pub(super) config: Res<'w, GameConfigResource>,
    pub(super) content: Res<'w, ContentProjectResource>,
    pub(super) development: Option<Res<'w, DevelopmentSession>>,
    pub(super) fades: Query<'w, 's, &'static mut MenuFade>,
    pub(super) watermarks: Query<'w, 's, (&'static mut SettingsWatermark, &'static Text)>,
    pub(super) save_roots:
        Query<'w, 's, (Entity, &'static mut Visibility), With<crate::ui::save_load::SaveLoadRoot>>,
    pub(super) save_proxies: Query<'w, 's, Entity, With<crate::ui::save_load::SaveLoadBlurProxy>>,
    pub(super) route_transition: Res<'w, MenuRouteTransition>,
    pub(super) locale_transition: Res<'w, SettingsLocaleTransition>,
}

#[derive(SystemParam)]
pub(crate) struct SettingsVisualFadeContext<'w, 's> {
    pub(super) roots:
        Query<'w, 's, (Entity, &'static MenuFade, &'static Visibility), With<SettingsRoot>>,
    pub(super) parents: Query<'w, 's, &'static ChildOf>,
    pub(super) texts: Query<'w, 's, (Entity, &'static mut TextColor)>,
    pub(super) backgrounds: Query<'w, 's, (Entity, &'static mut BackgroundColor)>,
    pub(super) outlines: Query<'w, 's, (Entity, &'static mut Outline)>,
}

pub(super) type TitleEntityQuery<'w, 's> = Query<
    'w,
    's,
    Entity,
    Or<(
        With<crate::ui::title::TitleRoot>,
        With<crate::ui::title::TitleBackground>,
    )>,
>;

pub(super) type SettingsLocaleRootQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut MenuFade,
        &'static mut MenuSurface,
        &'static mut Visibility,
    ),
    With<SettingsRoot>,
>;

#[derive(SystemParam)]
pub(crate) struct SettingActionContext<'w, 's> {
    pub(super) commands: Commands<'w, 's>,
    pub(super) actions:
        Query<'w, 's, (&'static Interaction, &'static SettingAction), Changed<Interaction>>,
    pub(super) settings: ResMut<'w, RuntimeSettings>,
    pub(super) toggles: ResMut<'w, ToggleStates>,
    pub(super) pending_window: ResMut<'w, PendingWindowMode>,
    pub(super) project_root: Res<'w, PersistenceRoot>,
    pub(super) store: Res<'w, crate::runtime::resources::StoreCodec>,
    pub(super) state: ResMut<'w, crate::runtime::resources::GameState>,
    pub(super) quick_preview: ResMut<'w, crate::ui::control_bar::QuickSavePreview>,
    pub(super) save_previews: ResMut<'w, crate::ui::save_load::SavePreviewCache>,
    pub(super) preview_coordinator: Res<'w, crate::storage::save::SavePreviewCoordinator>,
    pub(super) title_entities: TitleEntityQuery<'w, 's>,
    pub(super) settings_roots: SettingsLocaleRootQuery<'w, 's>,
    pub(super) locale_transition: ResMut<'w, SettingsLocaleTransition>,
}

#[derive(SystemParam)]
pub(crate) struct SettingsVisualResources<'w> {
    pub(super) time: Res<'w, Time>,
    pub(super) settings: Res<'w, RuntimeSettings>,
    pub(super) drag: Res<'w, ActiveSettingSlider>,
}
