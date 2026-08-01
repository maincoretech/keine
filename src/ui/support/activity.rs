use bevy::audio::{AudioSink, AudioSinkPlayback};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::storage::settings::RuntimeSettings;
use crate::ui::backlog::{
    BacklogButtonVisual, BacklogClose, BacklogRoot, BacklogScrollMotion, BacklogUiState,
};
use crate::ui::choice::{ChoiceButton, ChoiceRoot};
use crate::ui::control_bar::{HoverAlpha, QuickPreviewFade};
use crate::ui::dialog::DialogFade;
use crate::ui::extra::{ExtraBgmPlayer, ExtraBgmSeekBar, ExtraMotion};
use crate::ui::foundation::{ButtonPressFeedback, HoverSweep};
use crate::ui::menu::{MenuFade, MenuRouteTransition};
use crate::ui::save_load::{
    SaveLoadPage, SaveLoadPageTransition, SaveLoadPageVisual, SaveLoadSlotMotion, SaveLoadUi,
};
use crate::ui::settings_panel::{
    AboutRepositoryLink, AboutRepositoryVisual, ActiveSettingSlider, PendingWindowMode,
    SettingChoice, SettingChoiceVisual, SettingSlider, SettingSliderThumb,
    SettingSliderThumbVisual, SettingsPageButton, SettingsPageButtonVisual, SettingsPageTransition,
    SettingsUi, SettingsWatermark,
};
use crate::ui::textbox::{
    DialogueGlyphReveal, InitialTextboxFade, TextboxLayoutMotion, TextboxOverlayFade,
};
use crate::ui::title::{
    PendingTitleAction, ReturnToTitleTransition, TitleButtonMotion, TitleContinuePreview,
};

const SETTLED_EPSILON: f32 = 0.001;

#[derive(Resource, Default)]
pub(crate) struct UiAnimationActivity(pub(crate) bool);

#[derive(SystemParam)]
pub(crate) struct UiActivityContext<'w, 's> {
    backlog: Res<'w, BacklogUiState>,
    backlog_scroll: Res<'w, BacklogScrollMotion>,
    save_load: Res<'w, SaveLoadUi>,
    settings_ui: Res<'w, SettingsUi>,
    runtime_settings: Res<'w, RuntimeSettings>,
    textbox_fade: Res<'w, TextboxOverlayFade>,
    textbox_initial_fade: Res<'w, InitialTextboxFade>,
    textbox_layout: Res<'w, TextboxLayoutMotion>,
    dialogue_reveals: Query<'w, 's, (&'static DialogueGlyphReveal, &'static Visibility)>,
    hovers: Query<'w, 's, &'static HoverAlpha>,
    previews: Query<'w, 's, &'static QuickPreviewFade>,
    menu_fades: Query<'w, 's, &'static MenuFade>,
    button_feedback: Query<'w, 's, (&'static Interaction, &'static ButtonPressFeedback)>,
    title_buttons: Query<'w, 's, (&'static Interaction, &'static TitleButtonMotion)>,
    title_previews: Query<'w, 's, &'static TitleContinuePreview>,
    choice_roots: Query<'w, 's, &'static ChoiceRoot>,
    choices: Query<'w, 's, (&'static Interaction, &'static ChoiceButton)>,
    backlog_roots: Query<'w, 's, &'static BacklogRoot>,
    backlog_buttons: Query<
        'w,
        's,
        (
            &'static Interaction,
            &'static BacklogButtonVisual,
            Option<&'static BacklogClose>,
        ),
    >,
    dialog_fades: Query<'w, 's, &'static DialogFade>,
    hover_sweeps: Query<'w, 's, (&'static Interaction, &'static HoverSweep)>,
    watermarks: Query<'w, 's, &'static SettingsWatermark>,
    save_slots: Query<'w, 's, (&'static Interaction, &'static SaveLoadSlotMotion)>,
    save_pages: Query<
        'w,
        's,
        (
            &'static Interaction,
            &'static SaveLoadPage,
            &'static SaveLoadPageVisual,
        ),
    >,
    settings_page_buttons: Query<
        'w,
        's,
        (
            &'static Interaction,
            &'static SettingsPageButton,
            &'static SettingsPageButtonVisual,
        ),
    >,
    settings_page_transition: Res<'w, SettingsPageTransition>,
    setting_choices: Query<
        'w,
        's,
        (
            &'static Interaction,
            &'static SettingChoice,
            &'static SettingChoiceVisual,
        ),
    >,
    setting_sliders: Query<'w, 's, (&'static Interaction, &'static SettingSlider)>,
    setting_thumbs: Query<
        'w,
        's,
        (
            &'static SettingSliderThumb,
            &'static SettingSliderThumbVisual,
        ),
    >,
    about_links: Query<
        'w,
        's,
        (&'static Interaction, &'static AboutRepositoryVisual),
        With<AboutRepositoryLink>,
    >,
    extra_motions: Query<'w, 's, &'static ExtraMotion>,
    extra_players: Query<'w, 's, Option<&'static AudioSink>, With<ExtraBgmPlayer>>,
    extra_seek: Query<'w, 's, &'static ExtraBgmSeekBar>,
    save_load_transition: Res<'w, SaveLoadPageTransition>,
    menu_route_transition: Res<'w, MenuRouteTransition>,
    pending_title: Option<Res<'w, PendingTitleAction>>,
    return_to_title: Option<Res<'w, ReturnToTitleTransition>>,
    pending_window: Res<'w, PendingWindowMode>,
    active_slider: Res<'w, ActiveSettingSlider>,
}

pub(crate) fn update(context: UiActivityContext, mut activity: ResMut<UiAnimationActivity>) {
    activity.0 = context
        .hovers
        .iter()
        .any(|hover| (hover.current - hover.target).abs() > SETTLED_EPSILON)
        || context
            .previews
            .iter()
            .any(|fade| (fade.current - fade.target).abs() > SETTLED_EPSILON)
        || context
            .menu_fades
            .iter()
            .any(|fade| (fade.current - fade.target).abs() > SETTLED_EPSILON)
        || context
            .button_feedback
            .iter()
            .any(|(interaction, feedback)| feedback.is_animating(*interaction))
        || context
            .title_buttons
            .iter()
            .any(|(interaction, motion)| motion.is_animating(*interaction))
        || context
            .title_previews
            .iter()
            .any(TitleContinuePreview::is_animating)
        || choices_are_animating(&context)
        || context
            .backlog_roots
            .iter()
            .any(|root| root.is_animating(context.backlog.open))
        || context
            .backlog_buttons
            .iter()
            .any(|(interaction, visual, close)| visual.is_animating(*interaction, close.is_some()))
        || context.backlog_scroll.is_animating()
        || context.dialog_fades.iter().any(DialogFade::is_animating)
        || context
            .hover_sweeps
            .iter()
            .any(|(interaction, sweep)| sweep.is_animating(*interaction))
        || context
            .watermarks
            .iter()
            .any(SettingsWatermark::is_animating)
        || menu_controls_are_animating(&context)
        || context.save_load_transition.is_animating()
        || context.menu_route_transition.is_animating()
        || context.pending_title.is_some()
        || context.return_to_title.is_some()
        || context.extra_motions.iter().any(ExtraMotion::is_animating)
        || context
            .extra_players
            .iter()
            .any(|sink| sink.is_none_or(|sink| !sink.is_paused()))
        || context.extra_seek.iter().any(ExtraBgmSeekBar::is_dragging)
        || context.pending_window.is_pending()
        || context.active_slider.is_active()
        || context.textbox_initial_fade.is_animating()
        || context.textbox_layout.is_animating()
        || context
            .dialogue_reveals
            .iter()
            .any(|(reveal, visibility)| reveal.is_animating(*visibility))
        || !is_endpoint(context.textbox_fade.alpha);
}

fn menu_controls_are_animating(context: &UiActivityContext<'_, '_>) -> bool {
    context
        .save_slots
        .iter()
        .any(|(interaction, motion)| motion.is_animating(*interaction))
        || context
            .save_pages
            .iter()
            .any(|(interaction, page, visual)| {
                visual.is_animating(*interaction, page.0, context.save_load.page)
            })
        || context
            .settings_page_buttons
            .iter()
            .any(|(interaction, page, visual)| {
                visual.is_animating(*interaction, page.0, context.settings_ui.page)
            })
        || context.settings_page_transition.is_animating()
        || context
            .about_links
            .iter()
            .any(|(interaction, visual)| visual.is_animating(*interaction))
        || context
            .setting_choices
            .iter()
            .any(|(interaction, choice, visual)| {
                visual.is_animating(*interaction, choice.0, &context.runtime_settings)
            })
        || context.setting_thumbs.iter().any(|(thumb, visual)| {
            let hovered = context.setting_sliders.iter().any(|(interaction, slider)| {
                slider.0 == thumb.0
                    && matches!(interaction, Interaction::Hovered | Interaction::Pressed)
            });
            (visual.0 - if hovered { 12.0 } else { 10.0 }).abs() > SETTLED_EPSILON
        })
}

fn choices_are_animating(context: &UiActivityContext<'_, '_>) -> bool {
    let Ok(root) = context.choice_roots.single() else {
        return false;
    };
    context
        .choices
        .iter()
        .any(|(interaction, button)| button.is_animating(*interaction, root.selected()))
}

fn is_endpoint(value: f32) -> bool {
    value <= SETTLED_EPSILON || value >= 1.0 - SETTLED_EPSILON
}
