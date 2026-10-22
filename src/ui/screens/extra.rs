//! Extra screen facade; registration and public entry points stay stable.
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

mod state;
pub(crate) use state::*;
mod view;
pub(crate) use view::attach_button_visuals;
use view::*;
mod actions;
pub(crate) use actions::*;
mod music;
pub(crate) use music::*;
mod motion;
pub(crate) use motion::*;
mod sync;
pub(crate) use sync::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct VisualWrites(usize);

    type VisualChanges = Or<(
        Changed<Node>,
        Changed<UiTransform>,
        Changed<BlurStrength>,
        Changed<BackgroundColor>,
        Changed<ExtraMotion>,
    )>;

    fn count_visual_writes(
        changed: Query<Entity, VisualChanges>,
        mut writes: ResMut<VisualWrites>,
    ) {
        writes.0 = changed.iter().count();
    }

    #[test]
    fn backdrop_survives_zero_delta_opening_and_is_removed_after_closing() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .insert_resource(ExtraUi {
                open: true,
                ..default()
            })
            .init_resource::<VisualWrites>()
            .add_systems(Update, (animate, count_visual_writes).chain());
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
        app.update();
        assert_eq!(app.world().resource::<VisualWrites>().0, 0);
        // A newly created backdrop still gets the settled fade in its first update.
        let replacement = app
            .world_mut()
            .spawn((
                ExtraBlurProxy,
                BlurStrength(0.0),
                BackgroundColor(Color::NONE),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<BlurStrength>(replacement).unwrap().0,
            crate::ui::FULLSCREEN_BLUR_STRENGTH
        );
        assert_eq!(
            app.world()
                .get::<BackgroundColor>(replacement)
                .unwrap()
                .0
                .alpha(),
            0.6
        );
        app.world_mut().resource_mut::<ExtraUi>().open = false;
        app.world_mut().get_mut::<ExtraMotion>(root).unwrap().target = 0.0;
        app.update();
        assert!(app.world().get_entity(root).is_err());
        assert!(app.world().get_entity(proxy).is_err());
        assert!(app.world().get_entity(replacement).is_err());
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
    fn full_cg_settles_and_back_closes_it_before_leaving_extra() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::runtime::platform::InputActions>()
            .insert_resource(ExtraUi {
                open: true,
                ..default()
            })
            .init_resource::<Time>()
            .init_resource::<VisualWrites>()
            .add_systems(
                Update,
                (handle_navigation, animate_full_cg, count_visual_writes).chain(),
            );
        let cg = app
            .world_mut()
            .spawn((
                ExtraFullCg("test.webp".into()),
                ExtraMotion {
                    current: 1.0,
                    target: 1.0,
                },
                UiTransform::default(),
                BackgroundColor(Color::NONE),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<BackgroundColor>(cg).unwrap().0.alpha(),
            0.72
        );
        app.update();
        assert_eq!(app.world().resource::<VisualWrites>().0, 0, "settled CG");
        app.world_mut()
            .resource_mut::<crate::runtime::platform::InputActions>()
            .back = true;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(16));
        app.update();
        assert!(app.world().get_entity(cg).is_ok());
        assert!(app.world().resource::<ExtraUi>().open);
        assert_eq!(app.world().get::<ExtraMotion>(cg).unwrap().target, 0.0);
        assert!(app.world().get::<ExtraMotion>(cg).unwrap().current < 1.0);
        assert!(app.world().get::<BackgroundColor>(cg).unwrap().0.alpha() < 0.72);
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
            .init_resource::<VisualWrites>()
            .add_systems(Update, (update_sections, count_visual_writes).chain());
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
        app.update();
        assert_eq!(app.world().resource::<VisualWrites>().0, 0);
        let grid = app
            .world_mut()
            .spawn((
                ExtraCgGrid { outgoing: false },
                UiTransform {
                    translation: Val2::percent(100.0, 0.0),
                    ..default()
                },
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<UiTransform>(grid).unwrap().translation,
            Val2::ZERO
        );
        app.world_mut().resource_mut::<ExtraUi>().section = ExtraSection::Cg;
        *app.world_mut().resource_mut::<ExtraPageTransition>() = ExtraPageTransition {
            from: Some(ExtraSection::Music),
            ..default()
        };
        app.update();
        assert_eq!(
            app.world().get::<UiTransform>(new).unwrap().translation,
            Val2::percent(50.0, 0.0)
        );
        assert_eq!(
            app.world().get::<UiTransform>(old).unwrap().translation,
            Val2::percent(-50.0, 0.0)
        );
        app.update();
        assert_eq!(app.world().get::<Node>(new).unwrap().display, Display::None);
        assert_eq!(app.world().get::<Node>(old).unwrap().display, Display::Flex);
    }
}
