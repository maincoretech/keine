mod overlays;
mod screens;
mod stage;
pub(crate) mod startup_error;
mod support;

pub(crate) use overlays::user_input;
pub use overlays::{backlog, dialog};
pub(crate) use screens::menu;
pub use screens::{extra, save_load, settings_panel, title};
pub use stage::{choice, control_bar, textbox};
#[cfg(feature = "ui-sounds")]
pub(crate) use support::sound;
pub(crate) use support::{activity, foundation, input_scope};
pub use support::{loading, performance, text_style};

use bevy::asset::embedded_asset;
use bevy::prelude::*;

use crate::render::blur;
use crate::runtime::GameSystemSet;

pub(crate) const FULLSCREEN_BLUR_STRENGTH: f32 = 36.0;
pub(crate) const MENU_BACKDROP_ALPHA: f32 = 0.8;
pub(crate) const BACKLOG_BACKDROP_ALPHA: f32 = 0.82;

pub(crate) struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "src", "assets/fonts/MavenPro-CJK.ttf");
        embedded_asset!(app, "src", "assets/fonts/bootstrap-icons.ttf");
        #[cfg(feature = "ui-sounds")]
        {
            embedded_asset!(app, "src", "assets/audio/click.opus");
            embedded_asset!(app, "src", "assets/audio/mouse-enter.opus");
            embedded_asset!(app, "src", "assets/audio/switch.opus");
        }
        text_style::install_renderer(app);
        init_resources(app);
        app.add_systems(PreUpdate, input_scope::sync);
        add_startup_systems(app);
        app.add_systems(
            Update,
            control_bar::auto_hide_tick.in_set(GameSystemSet::Input),
        );
        add_stage_systems(app);
        add_overlay_systems(app);
        add_menu_systems(app);
        app.add_systems(
            PostUpdate,
            (
                control_bar::position_quick_previews,
                blur::update_blur_regions,
            )
                .chain()
                .after(bevy::ui::UiSystems::Layout),
        );
        app.add_systems(
            PostUpdate,
            (dialog::sync_system_message, dialog::spawn_dialog).chain(),
        );
        app.add_systems(
            Last,
            activity::update.before(crate::runtime::platform::update_lifecycle),
        );
    }
}

fn init_resources(app: &mut App) {
    app.init_resource::<activity::UiAnimationActivity>()
        .init_resource::<foundation::UiFonts>()
        .init_resource::<control_bar::ToggleStates>()
        .init_resource::<control_bar::AutoHideTiming>()
        .init_resource::<control_bar::QuickSavePreview>()
        .init_resource::<control_bar::QuickSavePreviewLoad>()
        .init_resource::<dialog::SavePreviewWriter>()
        .init_resource::<textbox::TextboxOverlayFade>()
        .init_resource::<textbox::InitialTextboxFade>()
        .init_resource::<textbox::TextboxLayoutMotion>()
        .init_resource::<overlays::presentation::AdvanceHintState>()
        .init_resource::<user_input::UserInputCaretBlink>()
        .init_resource::<backlog::BacklogUiState>()
        .init_resource::<backlog::BacklogScrollMotion>()
        .init_resource::<save_load::SaveLoadUi>()
        .init_resource::<save_load::SavePreviewCache>()
        .init_resource::<save_load::SaveLoadPageTransition>()
        .init_resource::<menu::MenuRouteTransition>()
        .init_resource::<settings_panel::SettingsUi>()
        .init_resource::<settings_panel::SettingsPageTransition>()
        .init_resource::<settings_panel::PendingWindowMode>()
        .init_resource::<settings_panel::ActiveSettingSlider>()
        .init_resource::<extra::ExtraUi>()
        .init_resource::<input_scope::UiInputScope>()
        .init_resource::<crate::storage::settings::RuntimeSettings>();
    #[cfg(feature = "ui-sounds")]
    app.init_resource::<sound::UiSoundAssets>();
}

fn add_startup_systems(app: &mut App) {
    app.add_systems(
        Startup,
        (
            control_bar::load_quick_save_preview,
            textbox::setup_textbox,
            loading::setup_loading,
        )
            .chain(),
    );
}

fn add_stage_systems(app: &mut App) {
    #[cfg(feature = "ui-sounds")]
    app.add_systems(Update, sound::play_button_sounds.in_set(GameSystemSet::Ui));
    app.add_systems(
        Update,
        (
            (
                textbox::sync_visibility,
                textbox::update_textbox,
                textbox::animate_glyph_reveals,
                textbox::update_mini_avatar,
                textbox::animate_overlay_fade,
                textbox::animate_initial_fade,
                textbox::apply_hide_toggle,
            )
                .chain(),
            control_bar::set_hover_target,
            control_bar::animate_hover,
            foundation::attach_button_feedback,
            foundation::animate_button_feedback,
            foundation::animate_hover_sweeps,
            control_bar::handle_button_click.run_if(loading::assets_ready),
            (
                control_bar::show_quick_preview,
                control_bar::animate_quick_previews,
            )
                .chain(),
            control_bar::sync_quick_preview,
            control_bar::auto_hide_apply,
            control_bar::sync_toggle_highlights,
            control_bar::update_lock_icon,
            loading::update_loading,
            (
                overlays::presentation::ensure_spawned,
                overlays::presentation::sync,
                overlays::presentation::animate_advance_hint,
            )
                .chain(),
            (
                overlays::user_input::ensure_spawned,
                overlays::user_input::handle
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::user_input_allowed),
                overlays::user_input::sync,
                overlays::user_input::animate_caret,
            )
                .chain(),
            text_style::apply_text_shadows,
            support::i18n::sync,
            (
                choice::sync_choice,
                choice::handle_choice_input
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::stage_allowed),
                choice::animate_choice_buttons,
            )
                .chain(),
        )
            .in_set(GameSystemSet::Ui),
    );
}

fn add_overlay_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            (
                backlog::toggle_backlog
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::backlog_allowed),
                backlog::sync_backlog,
                backlog::handle_backlog_action.run_if(loading::assets_ready),
                backlog::animate_backlog,
                backlog::animate_backlog_buttons,
                backlog::scroll_backlog,
            )
                .chain(),
            (
                dialog::sync_modal_backdrop_layer
                    .after(save_load::handle_save_load_slot)
                    .after(control_bar::handle_button_click),
                dialog::handle_dialog_click
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::dialog_allowed),
                dialog::animate_dialog,
            )
                .chain(),
            (
                title::animate_return_to_title,
                title::hydrate_quick_save_preview.run_if(input_scope::title_allowed),
                title::sync_title,
                title::handle_title_input
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::title_allowed),
                title::animate_title_buttons,
            )
                .chain()
                // Let START produce the first dialogue before the stage UI is
                // updated. The title remains for this frame, so the textbox
                // has already begun its fade when the title leaves.
                .before(textbox::sync_visibility)
                .before(textbox::update_textbox),
            (
                extra::handle_navigation.run_if(input_scope::extra_allowed),
                extra::handle_page.run_if(input_scope::extra_allowed),
                extra::handle_cg.run_if(input_scope::extra_allowed),
                extra::handle_bgm.run_if(input_scope::extra_allowed),
                extra::sync_bgm_selection,
                extra::sync_bgm_play_icon,
                extra::handle_bgm_seek.run_if(input_scope::extra_allowed),
                extra::sync,
                extra::update_bgm_progress,
                extra::animate,
            )
                .chain()
                .run_if(extra::active),
        )
            .in_set(GameSystemSet::Ui),
    );
}

fn add_menu_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            (
                save_load::toggle_save_load
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::menu_allowed),
                settings_panel::toggle_settings
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::menu_allowed),
                save_load::handle_save_load_page
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::menu_allowed)
                    .run_if(menu::route_settled),
                save_load::animate_page_transition,
                save_load::handle_save_load_slot
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::menu_allowed)
                    .run_if(input_scope::writable_session)
                    .run_if(menu::route_settled),
                save_load::handle_save_delete
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::menu_allowed)
                    .run_if(input_scope::writable_session)
                    .run_if(menu::route_settled),
                settings_panel::handle_setting_action
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::menu_allowed)
                    .run_if(input_scope::writable_session)
                    .run_if(menu::route_settled),
                settings_panel::handle_about_repository_link
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::menu_allowed)
                    .run_if(menu::route_settled),
                settings_panel::apply_pending_window_mode,
                settings_panel::handle_settings_page
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::menu_allowed)
                    .run_if(menu::route_settled),
                settings_panel::handle_setting_sliders
                    .run_if(loading::assets_ready)
                    .run_if(input_scope::menu_allowed)
                    .run_if(input_scope::writable_session)
                    .run_if(menu::route_settled),
                save_load::sync_save_load,
            )
                .chain(),
            (
                save_load::poll_preview_tasks.run_if(save_load::save_load_open),
                settings_panel::sync_settings,
                menu::sync_header,
                menu::sync_tabs,
                settings_panel::update_setting_visuals.run_if(settings_panel::settings_open),
                settings_panel::update_setting_bubbles.run_if(settings_panel::settings_open),
                settings_panel::update_setting_preview.run_if(settings_panel::settings_open),
                settings_panel::update_settings_pages.run_if(settings_panel::settings_open),
                settings_panel::animate_about_repository_link.run_if(settings_panel::settings_open),
                settings_panel::animate_watermark,
                save_load::animate_save_load_grid_track.run_if(save_load::save_load_open),
                save_load::animate_save_load_pages.run_if(save_load::save_load_open),
                save_load::animate_save_load_slots.run_if(save_load::save_load_open),
                save_load::animate_save_load_content,
                menu::animate,
                menu::fade_header_visuals,
                settings_panel::fade_settings_visuals,
                menu::animate_route_transition,
            )
                .chain(),
        )
            .chain()
            .in_set(GameSystemSet::Ui),
    );
}
