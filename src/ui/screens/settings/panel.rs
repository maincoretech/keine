//! Settings screen facade. State, layout, interaction and motion have separate owners.
use std::collections::HashMap;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::FocusPolicy;
use bevy::window::{MonitorSelection, WindowMode};

use crate::render::blur::{DialogCamera, UiBlurCamera};
use crate::runtime::resources::{
    ContentProjectResource, DevelopmentSession, GameConfigResource, PersistenceRoot,
};
use crate::storage::settings::{RuntimeSettings, UiLocale};
use crate::ui::control_bar::{
    BlurStrength, ButtonAction, ControlInput, SkipMode, ToggleStates, UiBlurSource,
};
use crate::ui::foundation::{
    UiFonts, UiSoundStyle, button_surface, ease_in_out_cubic, exp_lerp, fill_node, smoothstep,
    text_weight,
};
use crate::ui::menu::{
    MenuBack, MenuBlur, MenuFade, MenuHeaderActive, MenuRouteTransition, MenuSurface,
    MenuSurfaceState, PersistentMenu, active_route, begin_route_change, root_node,
    spawn_header_slot,
};
use crate::ui::save_load::SaveLoadUi;
use crate::ui::support::i18n::{UiText, tr};

const OPTION_TRANSITION_RATE: f32 = 18.0;
const OPTION_TEXT_IDLE: f32 = 0.376;
const OPTION_TEXT_ACTIVE: f32 = 0.667;
const OPTION_FILL_ALPHA: f32 = crate::ui::foundation::SURFACE_HOVER_ALPHA;
const PAGE_TEXT_IDLE: f32 = 0.175;
const PAGE_TEXT_HOVER: f32 = 0.5;
const PAGE_TEXT_ACTIVE: f32 = 0.8;
const SETTINGS_COLUMNS: u16 = 3;
const SETTINGS_COLUMN_GAP: f32 = 30.0;
const SETTINGS_ROW_GAP: f32 = 24.0;
const SETTING_LABEL_SIZE: f32 = 30.0;
const SETTING_OPTION_SIZE: f32 = 24.0;

const SYSTEM_PLAYBACK_CELL: SettingsGridCell = SettingsGridCell::at(1, 1);
const SYSTEM_LANGUAGE_CELL: SettingsGridCell = SettingsGridCell::at(2, 1);
const SYSTEM_DATA_CELL: SettingsGridCell = SettingsGridCell::at(1, 2);
const SYSTEM_TRANSFER_CELL: SettingsGridCell = SettingsGridCell::at(2, 2);

const DISPLAY_SLIDERS: &[SettingKind] = &[SettingKind::TextSpeed, SettingKind::TextboxOpacity];

const AUDIO_SLIDERS: &[SettingKind] = &[
    SettingKind::MasterVolume,
    SettingKind::VocalVolume,
    SettingKind::BgmVolume,
    SettingKind::SeVolume,
    SettingKind::UiSeVolume,
];

#[path = "state.rs"]
mod state;
pub(crate) use state::*;
#[path = "view.rs"]
mod view;
pub(crate) use view::*;
#[path = "actions.rs"]
mod actions;
pub use actions::*;
#[path = "motion.rs"]
mod motion;
pub use motion::*;
#[path = "sync.rs"]
mod sync;
pub use sync::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::input_scope::UiInputScope;

    #[test]
    fn fullscreen_shortcut_toggles_the_actual_window_and_pending_target() {
        let mut app = App::new();
        app.init_resource::<PendingWindowMode>()
            .init_resource::<RuntimeSettings>()
            .insert_resource(crate::runtime::platform::InputActions {
                toggle_fullscreen: true,
                ..default()
            })
            .add_systems(Update, apply_pending_window_mode);
        let window = app.world_mut().spawn(Window::default()).id();
        app.update();
        assert_eq!(
            app.world().get::<Window>(window).unwrap().mode,
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        );
        assert!(app.world().resource::<RuntimeSettings>().fullscreen);
        app.world_mut()
            .resource_mut::<crate::runtime::platform::InputActions>()
            .toggle_fullscreen = false;
        app.update();
        assert!(app.world().resource::<RuntimeSettings>().fullscreen);
        app.world_mut()
            .resource_mut::<crate::runtime::platform::InputActions>()
            .toggle_fullscreen = true;
        app.update();
        assert_eq!(
            app.world().get::<Window>(window).unwrap().mode,
            WindowMode::Windowed
        );
        assert!(!app.world().resource::<RuntimeSettings>().fullscreen);
        // A pending UI request is the state the shortcut should invert.
        app.world_mut().resource_mut::<PendingWindowMode>().target = Some(true);
        app.update();
        assert_eq!(
            app.world().get::<Window>(window).unwrap().mode,
            WindowMode::Windowed
        );
        assert!(app.world().resource::<PendingWindowMode>().target.is_none());
    }

    #[test]
    fn stage_escape_opens_settings_and_the_next_escape_closes_it() {
        let mut app = App::new();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::Escape);
        app.insert_resource(keys)
            .insert_resource(crate::runtime::platform::InputActions {
                shortcut: Some(ButtonAction::System),
                ..default()
            })
            .init_resource::<UiInputScope>()
            .init_resource::<SettingsUi>()
            .init_resource::<SaveLoadUi>()
            .init_resource::<MenuRouteTransition>()
            .init_resource::<SettingsPageTransition>()
            .add_systems(Update, toggle_settings);
        app.update();
        assert!(app.world().resource::<SettingsUi>().open);
        *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Menu;
        app.world_mut()
            .resource_mut::<crate::runtime::platform::InputActions>()
            .shortcut = None;
        app.update();
        assert!(!app.world().resource::<SettingsUi>().open);
    }

    #[test]
    fn right_click_closes_settings_without_opening_another_menu() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(crate::runtime::platform::InputActions {
                back: true,
                ..default()
            })
            .insert_resource(UiInputScope::Menu)
            .insert_resource(SettingsUi {
                open: true,
                ..default()
            })
            .init_resource::<SaveLoadUi>()
            .init_resource::<MenuRouteTransition>()
            .init_resource::<SettingsPageTransition>()
            .add_systems(Update, toggle_settings);
        app.update();
        assert!(!app.world().resource::<SettingsUi>().open);
        assert!(app.world().resource::<SaveLoadUi>().mode.is_none());
    }

    #[test]
    fn language_refresh_has_distinct_fade_out_and_fade_in_phases() {
        let mut transition = SettingsLocaleTransition::default();

        transition.begin(UiLocale::Ja);
        assert!(transition.is_animating());
        assert_eq!(transition.phase, Some(LocaleTransitionPhase::FadeOut));

        transition.begin_fade_in();
        assert_eq!(transition.phase, Some(LocaleTransitionPhase::FadeIn));

        transition.finish();
        assert!(!transition.is_animating());
    }

    #[test]
    fn system_page_groups_keep_their_column_relationships() {
        assert_eq!(SYSTEM_PLAYBACK_CELL.column, SYSTEM_DATA_CELL.column);
        assert_eq!(SYSTEM_LANGUAGE_CELL.column, SYSTEM_TRANSFER_CELL.column);
        assert_eq!(SYSTEM_PLAYBACK_CELL.row, SYSTEM_LANGUAGE_CELL.row);
        assert_eq!(SYSTEM_DATA_CELL.row, SYSTEM_TRANSFER_CELL.row);
    }

    #[test]
    fn nested_setting_groups_do_not_carry_outer_grid_coordinates() {
        let node = setting_group_node(None, 84.0);
        assert_eq!(node.grid_column, GridPlacement::default());
        assert_eq!(node.grid_row, GridPlacement::default());
    }
}
