//! Shared chrome for the fixed SAVE / LOAD / CONFIG shell.

use std::collections::HashMap;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use crabgal_core::{DESIGN_HEIGHT, DESIGN_WIDTH};

use crate::render::blur::DialogCamera;
use crate::ui::control_bar::{BlurStrength, ButtonAction, HoverAlpha};
use crate::ui::foundation::{
    SURFACE_ACTIVE_ALPHA, SURFACE_HOVER_ALPHA, UiFonts, UiSoundStyle, button_surface,
    ease_in_out_cubic, exp_lerp, smoothstep, text,
};
use crate::ui::save_load::{SaveLoadContent, SaveLoadMode, SaveLoadRoot, SaveLoadUi};
use crate::ui::settings_panel::{SettingsContent, SettingsRoot, SettingsUi};
use crate::ui::support::i18n::{LocalizedText, UiText};
use crate::ui::{FULLSCREEN_BLUR_STRENGTH, MENU_BACKDROP_ALPHA};

/// Below this point the blur is no longer perceptible, while opaque child UI
/// would otherwise remain visible during the tail of the exponential fade.
/// Finish both layers on the same frame instead of waiting for numerical zero.
const EXIT_VISUAL_EPSILON: f32 = 0.05;

#[derive(Component)]
pub(crate) struct MenuFade {
    pub(crate) current: f32,
    pub(crate) target: f32,
}

impl MenuFade {
    pub(crate) fn entering() -> Self {
        Self {
            current: 0.0,
            target: 1.0,
        }
    }

    pub(crate) fn visible() -> Self {
        Self {
            current: 1.0,
            target: 1.0,
        }
    }
}

#[derive(Component)]
pub(crate) struct MenuSurface {
    start_scale: f32,
    start_translation: Vec2,
}

impl MenuSurface {
    pub(crate) fn standard() -> Self {
        Self {
            start_scale: 0.99,
            start_translation: Vec2::new(0.0, 9.0),
        }
    }

    pub(crate) fn config() -> Self {
        Self {
            start_scale: 1.0,
            start_translation: Vec2::new(31.5, 0.0),
        }
    }
}

#[derive(Bundle)]
pub(crate) struct MenuSurfaceState {
    surface: MenuSurface,
    fade: MenuFade,
    transform: UiTransform,
    visibility: Visibility,
}

impl MenuSurfaceState {
    pub(crate) fn new(surface: MenuSurface, switching_routes: bool) -> Self {
        let transform = surface_transform(&surface, switching_routes);
        Self {
            surface,
            fade: if switching_routes {
                MenuFade::visible()
            } else {
                MenuFade::entering()
            },
            transform,
            visibility: Visibility::Inherited,
        }
    }
}

#[derive(Component)]
pub(crate) struct MenuBlur;

#[derive(Component)]
pub(crate) struct PersistentMenu;

type MenuSurfaceQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut MenuFade,
        &'static MenuSurface,
        &'static mut BackgroundColor,
        &'static mut UiTransform,
        &'static mut Visibility,
        Option<&'static PersistentMenu>,
    ),
    Without<MenuBlur>,
>;

type MenuBlurQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut MenuFade,
        &'static mut BlurStrength,
        &'static mut BackgroundColor,
        &'static mut Visibility,
        Option<&'static PersistentMenu>,
    ),
    (With<MenuBlur>, Without<MenuSurface>),
>;

#[derive(Component)]
pub(crate) struct MenuBack;

#[derive(Component)]
pub(crate) struct MenuHeader;

#[derive(Component)]
pub(crate) struct MenuHeaderRoot;

#[derive(Component)]
pub(crate) struct MenuHeaderSlot;

#[derive(Component)]
pub(crate) struct MenuTab(ButtonAction);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuHeaderActive {
    Save,
    Load,
    Config,
}

pub(crate) fn active_route(
    save_load: &SaveLoadUi,
    settings: &SettingsUi,
) -> Option<MenuHeaderActive> {
    if settings.open {
        Some(MenuHeaderActive::Config)
    } else {
        save_load.mode.map(|mode| match mode {
            SaveLoadMode::Save => MenuHeaderActive::Save,
            SaveLoadMode::Load => MenuHeaderActive::Load,
        })
    }
}

#[derive(Resource, Default)]
pub(crate) struct MenuRouteTransition {
    from: Option<MenuHeaderActive>,
    to: Option<MenuHeaderActive>,
    elapsed: f32,
    width: f32,
}

impl MenuRouteTransition {
    const SECONDS: f32 = 0.34;

    pub(crate) fn begin(&mut self, from: MenuHeaderActive, to: MenuHeaderActive) {
        if from == to {
            return;
        }
        self.from = Some(from);
        self.to = Some(to);
        self.elapsed = 0.0;
        self.width = 0.0;
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.from.is_some() && self.to.is_some()
    }

    pub(crate) fn involves(&self, route: MenuHeaderActive) -> bool {
        self.is_animating() && (self.from == Some(route) || self.to == Some(route))
    }
}

pub(crate) fn begin_route_change(
    transition: &mut MenuRouteTransition,
    from: Option<MenuHeaderActive>,
    to: Option<MenuHeaderActive>,
) {
    let (Some(from), Some(to)) = (from, to) else {
        return;
    };
    if from == MenuHeaderActive::Config || to == MenuHeaderActive::Config {
        transition.begin(from, to);
    }
}

pub(crate) fn route_settled(transition: Res<MenuRouteTransition>) -> bool {
    !transition.is_animating()
}

pub(crate) fn root_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: Val::Px(DESIGN_WIDTH),
        height: Val::Px(DESIGN_HEIGHT),
        padding: UiRect::axes(Val::Percent(2.5), Val::Percent(2.0)),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Percent(1.0),
        ..default()
    }
}

pub(crate) fn surface_transform(surface: &MenuSurface, visible: bool) -> UiTransform {
    if visible {
        UiTransform::default()
    } else {
        UiTransform {
            translation: Val2::px(surface.start_translation.x, surface.start_translation.y),
            scale: Vec2::splat(surface.start_scale),
            ..default()
        }
    }
}

pub(crate) fn spawn_header(
    slot: &mut ChildSpawnerCommands,
    active: MenuHeaderActive,
    font: &Handle<Font>,
    icon_font: &Handle<Font>,
) {
    slot.spawn((
        MenuHeader,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            padding: UiRect::horizontal(Val::Px(9.0)),
            flex_shrink: 0.0,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        },
    ))
    .with_children(|header| {
        header
            .spawn((Node {
                height: Val::Percent(100.0),
                ..default()
            },))
            .with_children(|left| {
                spawn_button(
                    left,
                    "\u{f7d8}",
                    "SAVE",
                    ButtonAction::Save,
                    active == MenuHeaderActive::Save,
                    font,
                    icon_font,
                );
                spawn_button(
                    left,
                    "\u{f3d8}",
                    "LOAD",
                    ButtonAction::Load,
                    active == MenuHeaderActive::Load,
                    font,
                    icon_font,
                );
                spawn_button(
                    left,
                    "\u{f56b}",
                    "CONFIG",
                    ButtonAction::System,
                    active == MenuHeaderActive::Config,
                    font,
                    icon_font,
                );
            });
        header
            .spawn((Node {
                height: Val::Percent(100.0),
                ..default()
            },))
            .with_children(|right| {
                spawn_button(
                    right,
                    "\u{f423}",
                    "TITLE",
                    ButtonAction::Title,
                    false,
                    font,
                    icon_font,
                );
                right
                    .spawn((
                        Button,
                        UiSoundStyle::Click,
                        MenuBack,
                        HoverAlpha::default(),
                        Node {
                            min_width: Val::Px(112.5),
                            height: Val::Percent(100.0),
                            padding: UiRect::horizontal(Val::Px(21.0)),
                            column_gap: Val::Px(7.5),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|button| {
                        button.spawn(text("\u{f1c3}", icon_font, 21.0, 0.82));
                        button.spawn((LocalizedText(UiText::Back), text("BACK", font, 21.0, 0.82)));
                    });
            });
    });
}

pub(crate) fn spawn_header_slot(root: &mut ChildSpawnerCommands) {
    root.spawn((MenuHeaderSlot, header_slot_node()));
}

fn header_slot_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: Val::Percent(7.0),
        flex_shrink: 0.0,
        ..default()
    }
}

#[derive(SystemParam)]
pub(crate) struct MenuHeaderContext<'w, 's> {
    commands: Commands<'w, 's>,
    fonts: Res<'w, UiFonts>,
    camera: Query<'w, 's, Entity, With<DialogCamera>>,
    roots: Query<
        'w,
        's,
        (
            &'static mut Visibility,
            &'static mut MenuFade,
            &'static mut MenuSurface,
        ),
        With<MenuHeaderRoot>,
    >,
    transition: Res<'w, MenuRouteTransition>,
}

pub(crate) fn sync_header(
    save_load: Res<SaveLoadUi>,
    settings: Res<SettingsUi>,
    mut context: MenuHeaderContext,
) {
    let route = active_route(&save_load, &settings);
    if let Ok((mut visibility, mut fade, mut surface)) = context.roots.single_mut() {
        let Some(route) = route else {
            fade.target = 0.0;
            return;
        };
        *surface = surface_for_route(route);
        *visibility = Visibility::Inherited;
        if context.transition.is_animating() {
            fade.current = 1.0;
        }
        fade.target = 1.0;
        return;
    }
    let (Some(route), Ok(camera)) = (route, context.camera.single()) else {
        return;
    };
    context
        .commands
        .spawn((
            Name::new("menu_header"),
            MenuHeaderRoot,
            PersistentMenu,
            MenuSurfaceState::new(surface_for_route(route), context.transition.is_animating()),
            root_node(),
            BackgroundColor(Color::NONE),
            GlobalZIndex(181),
            UiTargetCamera(camera),
            RenderLayers::layer(2),
        ))
        .with_children(|root| {
            root.spawn((MenuHeaderSlot, header_slot_node()))
                .with_children(|slot| {
                    spawn_header(slot, route, &context.fonts.text, &context.fonts.icons);
                });
        });
}

fn surface_for_route(route: MenuHeaderActive) -> MenuSurface {
    match route {
        MenuHeaderActive::Save | MenuHeaderActive::Load => MenuSurface::standard(),
        MenuHeaderActive::Config => MenuSurface::config(),
    }
}

fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    icon: &str,
    label: &str,
    action: ButtonAction,
    active: bool,
    font: &Handle<Font>,
    icon_font: &Handle<Font>,
) {
    let alpha = if active { SURFACE_ACTIVE_ALPHA } else { 0.0 };
    let sound = if action == ButtonAction::Title {
        UiSoundStyle::Click
    } else {
        UiSoundStyle::Switch
    };
    parent
        .spawn((
            Button,
            sound,
            action,
            MenuTab(action),
            HoverAlpha {
                target: alpha,
                current: alpha,
                active,
                active_alpha: SURFACE_ACTIVE_ALPHA,
                hover_alpha: SURFACE_HOVER_ALPHA,
                ..default()
            },
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
            BackgroundColor(button_surface(alpha)),
        ))
        .with_children(|button| {
            button.spawn(text(icon, icon_font, 21.0, 0.82));
            let key = match action {
                ButtonAction::Save => Some(UiText::Save),
                ButtonAction::Load => Some(UiText::Load),
                ButtonAction::System => Some(UiText::Config),
                ButtonAction::Title => Some(UiText::Title),
                _ => None,
            };
            if let Some(key) = key {
                button.spawn((LocalizedText(key), text(label, font, 21.0, 0.82)));
            } else {
                button.spawn(text(label, font, 21.0, 0.82));
            }
        });
}

pub(crate) fn sync_tabs(
    save_load: Res<SaveLoadUi>,
    settings: Res<SettingsUi>,
    mut tabs: Query<(
        &MenuTab,
        &Interaction,
        &mut HoverAlpha,
        &mut BackgroundColor,
    )>,
) {
    if !save_load.is_changed() && !settings.is_changed() {
        return;
    }
    for (tab, interaction, mut hover, mut background) in &mut tabs {
        let active = match tab.0 {
            ButtonAction::Save => save_load.mode == Some(SaveLoadMode::Save),
            ButtonAction::Load => save_load.mode == Some(SaveLoadMode::Load),
            ButtonAction::System => settings.open,
            _ => false,
        };
        let active_changed = hover.active != active;
        hover.active = active;
        hover.active_alpha = SURFACE_ACTIVE_ALPHA;
        hover.hover_alpha = SURFACE_HOVER_ALPHA;
        hover.target = if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            hover.hover_alpha
        } else if active {
            SURFACE_ACTIVE_ALPHA
        } else {
            hover.idle_alpha
        };
        if active_changed {
            hover.current = hover.target;
            background.0 = if hover.current < 0.002 {
                Color::NONE
            } else {
                button_surface(hover.current)
            };
        }
    }
}

#[derive(Default)]
pub(crate) struct MenuHeaderFadeCache {
    active: bool,
    text: HashMap<Entity, f32>,
    background: HashMap<Entity, f32>,
}

pub(crate) fn fade_header_visuals(
    roots: Query<(Entity, &MenuFade, &Visibility), With<MenuHeaderRoot>>,
    parents: Query<&ChildOf>,
    mut texts: Query<(Entity, &mut TextColor)>,
    mut backgrounds: Query<(Entity, &mut BackgroundColor)>,
    mut cache: Local<MenuHeaderFadeCache>,
) {
    let Ok((root, fade, visibility)) = roots.single() else {
        return;
    };
    if *visibility == Visibility::Hidden {
        return;
    }
    let belongs_to_header = |entity: Entity| {
        let mut current = entity;
        while let Ok(parent) = parents.get(current) {
            current = parent.parent();
            if current == root {
                return true;
            }
        }
        false
    };
    let alpha = smoothstep(fade.current);
    if alpha >= 0.999 {
        if !cache.active {
            return;
        }
        for (entity, mut color) in &mut texts {
            if belongs_to_header(entity)
                && let Some(base) = cache.text.get(&entity)
            {
                color.0 = color.0.with_alpha(*base);
            }
        }
        for (entity, mut color) in &mut backgrounds {
            if belongs_to_header(entity)
                && let Some(base) = cache.background.get(&entity)
            {
                color.0 = color.0.with_alpha(*base);
            }
        }
        cache.active = false;
        cache.text.clear();
        cache.background.clear();
        return;
    }

    cache.active = true;
    for (entity, mut color) in &mut texts {
        if belongs_to_header(entity) {
            let base = *cache.text.entry(entity).or_insert_with(|| color.0.alpha());
            color.0 = color.0.with_alpha(base * alpha);
        }
    }
    for (entity, mut color) in &mut backgrounds {
        if belongs_to_header(entity) {
            let base = *cache
                .background
                .entry(entity)
                .or_insert_with(|| color.0.alpha());
            color.0 = color.0.with_alpha(base * alpha);
        }
    }
}

pub(crate) fn animate(
    time: Res<Time>,
    mut commands: Commands,
    mut surfaces: MenuSurfaceQuery,
    mut blurs: MenuBlurQuery,
) {
    let amount = exp_lerp(time.delta_secs(), 16.0);
    for (entity, mut fade, motion, mut background, mut transform, mut visibility, persistent) in
        &mut surfaces
    {
        if persistent.is_some()
            && *visibility == Visibility::Hidden
            && fade.current == 0.0
            && fade.target == 0.0
        {
            continue;
        }
        fade.current += (fade.target - fade.current) * amount;
        if fade.target == 0.0 && fade.current <= EXIT_VISUAL_EPSILON
            || (fade.target - fade.current).abs() < 0.001
        {
            fade.current = fade.target;
        }
        let eased = smoothstep(fade.current);
        background.0 = Color::NONE;
        transform.scale = Vec2::splat(motion.start_scale + (1.0 - motion.start_scale) * eased);
        transform.translation = Val2::px(
            motion.start_translation.x * (1.0 - eased),
            motion.start_translation.y * (1.0 - eased),
        );
        if fade.target == 0.0 && fade.current == 0.0 {
            if persistent.is_some() {
                *visibility = Visibility::Hidden;
            } else {
                commands.entity(entity).despawn();
            }
        }
    }
    for (entity, mut fade, mut strength, mut background, mut visibility, persistent) in &mut blurs {
        if persistent.is_some()
            && *visibility == Visibility::Hidden
            && fade.current == 0.0
            && fade.target == 0.0
        {
            continue;
        }
        fade.current += (fade.target - fade.current) * amount;
        if fade.target == 0.0 && fade.current <= EXIT_VISUAL_EPSILON
            || (fade.target - fade.current).abs() < 0.001
        {
            fade.current = fade.target;
        }
        strength.0 = FULLSCREEN_BLUR_STRENGTH * smoothstep(fade.current);
        background.0 = Color::srgba(
            0.0,
            0.0,
            0.0,
            MENU_BACKDROP_ALPHA * smoothstep(fade.current),
        );
        if fade.target == 0.0 && fade.current == 0.0 {
            if persistent.is_some() {
                *visibility = Visibility::Hidden;
            } else {
                commands.entity(entity).despawn();
            }
        }
    }
}

type SaveRouteRootQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Visibility, &'static mut MenuFade),
    (With<SaveLoadRoot>, Without<SettingsRoot>),
>;
type SettingsRouteRootQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Visibility, &'static mut MenuFade),
    (With<SettingsRoot>, Without<SaveLoadRoot>),
>;
type SaveRouteContentQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut UiTransform, &'static ComputedNode),
    (With<SaveLoadContent>, Without<SettingsContent>),
>;
type SettingsRouteContentQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut UiTransform, &'static ComputedNode),
    (With<SettingsContent>, Without<SaveLoadContent>),
>;
#[derive(SystemParam)]
pub(crate) struct MenuRouteContext<'w, 's> {
    save_roots: SaveRouteRootQuery<'w, 's>,
    settings_roots: SettingsRouteRootQuery<'w, 's>,
    save_contents: SaveRouteContentQuery<'w, 's>,
    settings_contents: SettingsRouteContentQuery<'w, 's>,
    windows: Query<'w, 's, &'static Window>,
}

pub(crate) fn animate_route_transition(
    time: Res<Time>,
    mut transition: ResMut<MenuRouteTransition>,
    mut context: MenuRouteContext,
) {
    let (Some(from), Some(to)) = (transition.from, transition.to) else {
        return;
    };
    let (
        Ok((mut save_visibility, mut save_fade)),
        Ok((mut settings_visibility, mut settings_fade)),
    ) = (
        context.save_roots.single_mut(),
        context.settings_roots.single_mut(),
    )
    else {
        return;
    };

    let starting = transition.elapsed <= f32::EPSILON;
    transition.elapsed = (transition.elapsed + time.delta_secs()).min(MenuRouteTransition::SECONDS);
    let linear = transition.elapsed / MenuRouteTransition::SECONDS;
    let progress = ease_in_out_cubic(linear);
    let route_index = |route| -> f32 {
        match route {
            MenuHeaderActive::Save => 0.0,
            MenuHeaderActive::Load => 1.0,
            MenuHeaderActive::Config => 2.0,
        }
    };
    let direction = (route_index(to) - route_index(from)).signum();
    if starting {
        transition.width = context
            .save_contents
            .iter()
            .map(|(_, node)| node.size().x)
            .chain(
                context
                    .settings_contents
                    .iter()
                    .map(|(_, node)| node.size().x),
            )
            .fold(0.0_f32, f32::max)
            .max(
                context
                    .windows
                    .single()
                    .map_or(1.0, |window| window.width() * 0.95),
            );
    }
    let width = transition.width.max(1.0);
    let incoming_x = direction * width * (1.0 - progress);
    let outgoing_x = -direction * width * progress;
    let save_is_incoming = matches!(to, MenuHeaderActive::Save | MenuHeaderActive::Load);

    if starting {
        *save_visibility = Visibility::Inherited;
        *settings_visibility = Visibility::Inherited;
        save_fade.current = 1.0;
        save_fade.target = 1.0;
        settings_fade.current = 1.0;
        settings_fade.target = 1.0;
    }

    for (mut transform, _) in &mut context.save_contents {
        transform.translation = Val2::px(
            if save_is_incoming {
                incoming_x
            } else {
                outgoing_x
            },
            0.0,
        );
    }
    for (mut transform, _) in &mut context.settings_contents {
        transform.translation = Val2::px(
            if save_is_incoming {
                outgoing_x
            } else {
                incoming_x
            },
            0.0,
        );
    }
    if transition.elapsed < MenuRouteTransition::SECONDS {
        return;
    }
    if save_is_incoming {
        *settings_visibility = Visibility::Hidden;
        settings_fade.current = 0.0;
        settings_fade.target = 0.0;
    } else {
        *save_visibility = Visibility::Hidden;
        save_fade.current = 0.0;
        save_fade.target = 0.0;
    }
    for (mut transform, _) in &mut context.save_contents {
        transform.translation = Val2::ZERO;
    }
    for (mut transform, _) in &mut context.settings_contents {
        transform.translation = Val2::ZERO;
    }
    transition.from = None;
    transition.to = None;
    transition.elapsed = 0.0;
    transition.width = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_keep_one_fixed_header_root() {
        let mut app = App::new();
        app.init_resource::<SaveLoadUi>()
            .init_resource::<SettingsUi>()
            .init_resource::<MenuRouteTransition>()
            .insert_resource(UiFonts {
                text: Handle::default(),
                icons: Handle::default(),
            })
            .add_systems(Update, (sync_header, sync_tabs).chain());
        app.world_mut().spawn(DialogCamera);
        app.world_mut().resource_mut::<SettingsUi>().open = true;

        app.update();

        let header_root = {
            let world = app.world_mut();
            let mut roots = world.query_filtered::<Entity, With<MenuHeaderRoot>>();
            roots.single(world).expect("one fixed header root")
        };
        let header = {
            let world = app.world_mut();
            let mut headers = world.query_filtered::<Entity, With<MenuHeader>>();
            headers.single(world).expect("one shared header")
        };
        let slot = app
            .world()
            .get::<ChildOf>(header)
            .expect("header slot parent")
            .parent();
        assert_eq!(
            app.world()
                .get::<ChildOf>(slot)
                .expect("fixed root parent")
                .parent(),
            header_root
        );

        app.world_mut().resource_mut::<SettingsUi>().open = false;
        app.world_mut().resource_mut::<SaveLoadUi>().mode = Some(SaveLoadMode::Load);
        app.update();

        let world = app.world_mut();
        let mut roots = world.query_filtered::<Entity, With<MenuHeaderRoot>>();
        assert_eq!(roots.iter(world).collect::<Vec<_>>(), [header_root]);
        let mut headers = world.query_filtered::<Entity, With<MenuHeader>>();
        assert_eq!(headers.iter(world).count(), 1);
        let (load_entity, active, current, target) = {
            let mut tabs = world.query::<(Entity, &MenuTab, &HoverAlpha)>();
            let (entity, _, hover) = tabs
                .iter(world)
                .find(|(_, tab, _)| tab.0 == ButtonAction::Load)
                .expect("load tab");
            (entity, hover.active, hover.current, hover.target)
        };
        assert!(active);
        assert_eq!(current, SURFACE_ACTIVE_ALPHA);
        assert_eq!(target, SURFACE_ACTIVE_ALPHA);
        assert_eq!(
            world
                .get::<BackgroundColor>(load_entity)
                .expect("selected load background")
                .0,
            button_surface(SURFACE_ACTIVE_ALPHA)
        );
    }

    #[test]
    fn only_crossing_the_config_boundary_starts_content_motion() {
        let mut transition = MenuRouteTransition::default();
        begin_route_change(
            &mut transition,
            Some(MenuHeaderActive::Save),
            Some(MenuHeaderActive::Load),
        );
        assert!(!transition.is_animating());

        begin_route_change(
            &mut transition,
            Some(MenuHeaderActive::Config),
            Some(MenuHeaderActive::Load),
        );
        assert!(transition.is_animating());
    }

    #[test]
    fn route_switch_starts_visible_without_replaying_root_entry_motion() {
        let state = MenuSurfaceState::new(MenuSurface::config(), true);

        assert_eq!(state.fade.current, 1.0);
        assert_eq!(state.fade.target, 1.0);
        assert_eq!(state.transform.translation, Val2::ZERO);
        assert_eq!(state.transform.scale, Vec2::ONE);
        assert_eq!(state.visibility, Visibility::Inherited);
    }

    #[test]
    fn standalone_menu_open_keeps_the_surface_entry_motion() {
        let state = MenuSurfaceState::new(MenuSurface::config(), false);

        assert_eq!(state.fade.current, 0.0);
        assert_eq!(state.fade.target, 1.0);
        assert_eq!(state.transform.translation, Val2::px(31.5, 0.0));
        assert_eq!(state.transform.scale, Vec2::ONE);
        assert_eq!(state.visibility, Visibility::Inherited);
    }
}
