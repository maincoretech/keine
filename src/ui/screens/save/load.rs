//! Save_Load screen facade. State, layout, interaction and motion have separate owners.
use std::collections::HashMap;
use std::time::SystemTime;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use bevy::text::FontWeight;
use bevy::ui::FocusPolicy;

use crate::render::blur::{DialogCamera, UiBlurCamera};
use crate::runtime::resources::PersistenceRoot;
use crate::ui::control_bar::{BlurStrength, ButtonAction, ControlInput, HoverAlpha, UiBlurSource};
use crate::ui::dialog::{DialogAction, DialogRequest};
use crate::ui::foundation::{
    SHELL_TRANSITION_WIDTH, SURFACE_ACTIVE_ALPHA, SURFACE_HOVER_ALPHA, UiFonts, UiSoundStyle,
    button_surface, ease_in_out_cubic, empty_slot_surface, exp_lerp, fill_node, logical_node_width,
    smoothstep, text, text_weight,
};
use crate::ui::menu::{
    MenuBack, MenuBlur, MenuFade, MenuRouteTransition, MenuSurface, MenuSurfaceState,
    PersistentMenu, active_route, begin_route_change, root_node, spawn_header_slot,
};

pub(crate) const PAGE_COUNT: u32 = 20;
pub(crate) const SLOTS_PER_PAGE: u32 = 10;

const PAGE_HIGHLIGHT_TEXT_ALPHA: f32 = 0.67;

#[path = "state.rs"]
mod state;
pub(crate) use state::*;
#[path = "view.rs"]
mod view;
use view::*;
#[path = "actions.rs"]
mod actions;
pub use actions::*;
#[path = "motion.rs"]
mod motion;
pub use motion::*;
#[path = "sync.rs"]
mod sync;
pub use sync::*;
#[path = "previews.rs"]
mod previews;
pub use previews::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn visibility_query_contract(
        _roots: SaveLoadRootVisibilityQuery,
        _proxies: SaveLoadProxyVisibilityQuery,
        _settings_roots: SettingsRootVisibilityQuery,
        _settings_proxies: SettingsProxyVisibilityQuery,
    ) {
    }

    #[test]
    fn rebuilt_grid_at_unchanged_fade_does_not_flash_opaque_for_one_frame() {
        let mut app = App::new();
        app.add_systems(Update, animate_save_load_content);
        let root = app
            .world_mut()
            .spawn((SaveLoadRoot, MenuFade::entering(), Visibility::Inherited))
            .id();
        let content = app.world_mut().spawn((SaveLoadContent, ChildOf(root))).id();
        let original = app
            .world_mut()
            .spawn((TextColor(Color::WHITE), ChildOf(content)))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<TextColor>(original).unwrap().0.alpha(),
            0.0
        );
        app.world_mut().entity_mut(original).despawn();
        let new_slot = app
            .world_mut()
            .spawn((
                BackgroundColor(Color::srgba(0.5, 0.5, 0.5, 0.7)),
                TextColor(Color::WHITE),
                ImageNode::default(),
                ChildOf(content),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<TextColor>(new_slot).unwrap().0.alpha(),
            0.0
        );
        assert_eq!(
            app.world()
                .get::<BackgroundColor>(new_slot)
                .unwrap()
                .0
                .alpha(),
            0.0
        );
        assert_eq!(
            app.world()
                .get::<ImageNode>(new_slot)
                .unwrap()
                .color
                .alpha(),
            0.0
        );
        app.world_mut().get_mut::<MenuFade>(root).unwrap().current = 1.0;
        app.update();
        assert_eq!(
            app.world().get::<TextColor>(new_slot).unwrap().0.alpha(),
            1.0
        );
        assert_eq!(
            app.world()
                .get::<BackgroundColor>(new_slot)
                .unwrap()
                .0
                .alpha(),
            0.7
        );
        assert_eq!(
            app.world()
                .get::<ImageNode>(new_slot)
                .unwrap()
                .color
                .alpha(),
            1.0
        );
    }

    #[test]
    fn right_click_leaves_both_slot_pages() {
        for mode in [SaveLoadMode::Save, SaveLoadMode::Load] {
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>()
                .insert_resource(crate::runtime::platform::InputActions {
                    back: true,
                    ..default()
                })
                .insert_resource(crate::ui::input_scope::UiInputScope::Menu)
                .insert_resource(SaveLoadUi {
                    mode: Some(mode),
                    ..default()
                })
                .init_resource::<crate::ui::settings_panel::SettingsUi>()
                .init_resource::<SaveLoadPageTransition>()
                .init_resource::<MenuRouteTransition>()
                .add_systems(Update, toggle_save_load);
            app.update();
            assert!(app.world().resource::<SaveLoadUi>().mode.is_none());
        }
    }

    #[test]
    fn menu_visibility_queries_are_disjoint() {
        let mut app = App::new();
        app.add_systems(Update, visibility_query_contract);
        app.update();
    }

    #[test]
    fn page_hover_and_selection_share_color_without_bottom_highlight() {
        fn spawn(mut commands: Commands) {
            let font = Handle::<Font>::default();
            commands.spawn_empty().with_children(|pages| {
                spawn_page_button(pages, 1, 1, &font);
            });
        }

        assert_eq!(
            page_highlight_alpha(true, false),
            page_highlight_alpha(false, true)
        );

        let mut app = App::new();
        app.add_systems(Update, spawn);
        app.update();
        let world = app.world_mut();
        let page = {
            let mut pages = world.query_filtered::<Entity, With<SaveLoadPage>>();
            pages.single(world).expect("one page button")
        };
        assert_eq!(
            world.get::<Node>(page).expect("page button node").border,
            UiRect::ZERO
        );
        assert_eq!(
            world
                .get::<BackgroundColor>(page)
                .expect("page button background")
                .0,
            button_surface(SURFACE_ACTIVE_ALPHA)
        );
        let hover = world
            .get::<HoverAlpha>(page)
            .expect("shared page hover surface");
        assert_eq!(
            (
                hover.current,
                hover.target,
                hover.active_alpha,
                hover.hover_alpha
            ),
            (
                SURFACE_ACTIVE_ALPHA,
                SURFACE_ACTIVE_ALPHA,
                SURFACE_ACTIVE_ALPHA,
                SURFACE_HOVER_ALPHA
            )
        );
    }
}

pub(crate) mod capture;
