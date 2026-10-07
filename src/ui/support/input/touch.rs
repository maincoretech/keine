//! One owner per touch: controls and scroll areas cannot become navigation.
use std::collections::HashSet;
use std::time::Instant;

use bevy::ecs::system::SystemParam;
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::ui::{
    ComputedStackIndex, ComputedUiTargetCamera, FocusPolicy, OverflowAxis, OverrideClip,
    clip_check_recursive,
};
use bevy::window::PrimaryWindow;

use super::super::input_scope::UiInputScope;
use crate::runtime::resources::GameState;
use crate::ui::menu::{MenuFade, MenuRouteTransition};
use crate::ui::settings_panel::{SettingsLocaleTransition, SettingsPageTransition, SettingsUi};

const EDGE: f32 = 24.0;
const TAP_SLOP: f32 = 10.0;
const SWIPE_DISTANCE: f32 = 56.0;
const AXIS_RATIO: f32 = 1.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Control(Entity),
    Scroll(Entity),
    Stage,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Swipe {
    Backlog,
}

struct Capture {
    id: u64,
    owner: Owner,
    start: Vec2,
    last: Vec2,
    excursion: f32,
    length: f32,
    started: Instant,
    scope: UiInputScope,
    page: crate::ui::settings_panel::SettingsPage,
    cursor: usize,
    scene: String,
    size: Vec2,
    scale: f32,
    camera: Option<Entity>,
    canceled: bool,
}

impl Capture {
    fn control_tap(&self) -> Option<Entity> {
        match self.owner {
            Owner::Control(entity) if !self.canceled && self.excursion <= TAP_SLOP => Some(entity),
            _ => None,
        }
    }

    fn moved(&mut self, point: Vec2) -> Vec2 {
        let delta = point - self.last;
        self.length += delta.length();
        self.excursion = self.excursion.max(point.distance(self.start));
        self.last = point;
        delta
    }

    fn finish(&self, elapsed: f64) -> (bool, Option<Swipe>) {
        if self.canceled || elapsed > 0.8 {
            return (false, None);
        }
        let delta = self.last - self.start;
        let straight = self.length <= delta.length() * 1.35;
        let swipe = match self.owner {
            Owner::Stage
                if straight
                    && -delta.y >= SWIPE_DISTANCE
                    && delta.y.abs() >= delta.x.abs() * AXIS_RATIO =>
            {
                Some(Swipe::Backlog)
            }
            _ => None,
        };
        (
            self.owner == Owner::Stage && self.excursion <= TAP_SLOP,
            swipe,
        )
    }
}

/// Frame output shared by runtime taps, sliders and lists.
#[derive(Resource, Default)]
pub(crate) struct TouchInputState {
    fingers: HashSet<u64>,
    capture: Option<Capture>,
    owner: Option<Owner>,
    point: Option<Vec2>,
    finished: bool,
    pub(crate) tap: bool,
    pub(crate) swipe: Option<Swipe>,
    pub(crate) scroll: Option<(Entity, f32)>,
}

impl TouchInputState {
    pub(crate) fn control_position(&self, entity: Entity) -> Option<Vec2> {
        (self.owner == Some(Owner::Control(entity)))
            .then_some(self.point)
            .flatten()
    }

    pub(crate) fn control_finished(&self, entity: Entity) -> bool {
        self.finished && self.owner == Some(Owner::Control(entity))
    }
}

type HitNodes<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ComputedNode,
        &'static UiGlobalTransform,
        &'static InheritedVisibility,
        &'static ComputedUiTargetCamera,
        Option<&'static ComputedStackIndex>,
        Has<Interaction>,
        &'static Node,
        &'static FocusPolicy,
    ),
>;

#[derive(SystemParam)]
pub(crate) struct TouchContext<'w, 's> {
    events: MessageReader<'w, 's, TouchInput>,
    windows: Query<'w, 's, (Entity, &'static Window), With<PrimaryWindow>>,
    cameras: Query<'w, 's, &'static Camera>,
    nodes: HitNodes<'w, 's>,
    clipping: Query<
        'w,
        's,
        (
            &'static ComputedNode,
            &'static UiGlobalTransform,
            &'static Node,
        ),
    >,
    parents: Query<'w, 's, &'static ChildOf, Without<OverrideClip>>,
    interactions: Query<'w, 's, (Entity, &'static mut Interaction)>,
    scope: Res<'w, UiInputScope>,
    state: Res<'w, GameState>,
    settings: Res<'w, SettingsUi>,
    route: Res<'w, MenuRouteTransition>,
    pages: Res<'w, SettingsPageTransition>,
    locale: Res<'w, SettingsLocaleTransition>,
    fades: Query<'w, 's, &'static MenuFade>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    input: ResMut<'w, TouchInputState>,
}

fn interior(point: Vec2, size: Vec2) -> bool {
    point.is_finite()
        && point.x >= EDGE
        && point.y >= EDGE
        && point.x <= size.x - EDGE
        && point.y <= size.y - EDGE
}

pub(crate) fn collect(mut context: TouchContext) {
    let events: Vec<_> = context.events.read().copied().collect();
    let input = &mut *context.input;
    // Our release click lasts one frame even when Bevy retains its old cursor.
    if input.finished
        && !context.mouse.just_pressed(MouseButton::Left)
        && let Some(Owner::Control(entity)) = input.owner
        && let Ok((_, mut interaction)) = context.interactions.get_mut(entity)
        && *interaction == Interaction::Pressed
    {
        interaction.set_if_neq(Interaction::None);
    }
    input.owner = None;
    input.point = None;
    input.finished = false;
    input.tap = false;
    input.swipe = None;
    input.scroll = None;
    let mut control_click = None;
    let Ok((window_entity, window)) = context.windows.single() else {
        input.capture = None;
        input.fingers.clear();
        return;
    };
    let navigation_ready = !context.route.is_animating()
        && !context.pages.is_animating()
        && !context.locale.is_animating()
        && context
            .fades
            .iter()
            .all(|fade| (fade.current - fade.target).abs() <= 0.01);
    let hit = |point: Vec2| {
        // Use Bevy's transformed bounds, camera viewport and recursive clipping.
        // Controls win over their containing list or menu surface.
        context
            .nodes
            .iter()
            .filter_map(
                |(entity, node, transform, visible, target, stack, interaction, style, focus)| {
                    let camera = target.get()?;
                    let camera_point = point * window.scale_factor()
                        - context
                            .cameras
                            .get(camera)
                            .ok()?
                            .physical_viewport_rect()
                            .map(|rect| rect.min.as_vec2())
                            .unwrap_or_default();
                    if !visible.get()
                        || node.size().min_element() <= 0.0
                        || !node.contains_point(*transform, camera_point)
                        || !clip_check_recursive(
                            camera_point,
                            entity,
                            &context.clipping,
                            &context.parents,
                        )
                    {
                        return None;
                    }
                    let (priority, owner) = if interaction {
                        (3, Owner::Control(entity))
                    } else if style.overflow.x == OverflowAxis::Scroll
                        || style.overflow.y == OverflowAxis::Scroll
                    {
                        (2, Owner::Scroll(entity))
                    } else if *focus == FocusPolicy::Block {
                        // Match Bevy focus: blocking overlays shield controls,
                        // while a transparent header's Pass area lets them through.
                        (1, Owner::Blocked)
                    } else {
                        return None;
                    };
                    Some((
                        (stack.map_or(0, |stack| stack.0), priority),
                        owner,
                        Some(camera),
                    ))
                },
            )
            .max_by_key(|(order, _, _)| *order)
            .map(|(_, owner, camera)| (owner, camera))
            .unwrap_or((
                if *context.scope == UiInputScope::Stage
                    && context.state.menu.is_none()
                    && context.state.videos.is_empty()
                    && {
                        let viewport =
                            crate::runtime::platform::DesignViewport::from_window(window)
                                .camera_viewport(window);
                        let point = point * window.scale_factor();
                        Rect::from_corners(
                            viewport.physical_position.as_vec2(),
                            (viewport.physical_position + viewport.physical_size).as_vec2(),
                        )
                        .contains(point)
                    }
                {
                    Owner::Stage
                } else {
                    Owner::Blocked
                },
                None,
            ))
    };
    if let Some(capture) = input.capture.as_mut() {
        capture.canceled |= !window.focused
            || capture.scope != *context.scope
            || capture.size != window.size()
            || capture.scale != window.scale_factor()
            || capture.page != context.settings.page
            || capture.cursor != context.state.cursor
            || capture.scene != context.state.current_scene
            || !navigation_ready
            || context.mouse.pressed(MouseButton::Left);
    }
    for event in events {
        if event.window != window_entity {
            continue;
        }
        match event.phase {
            TouchPhase::Started => {
                input.fingers.insert(event.id);
                if input.fingers.len() != 1 || input.capture.is_some() {
                    if let Some(capture) = input.capture.as_mut() {
                        capture.canceled = true;
                    }
                    continue;
                }
                let (owner, camera) = hit(event.position);
                input.capture = Some(Capture {
                    id: event.id,
                    owner,
                    start: event.position,
                    last: event.position,
                    excursion: 0.0,
                    length: 0.0,
                    started: Instant::now(),
                    scope: *context.scope,
                    page: context.settings.page,
                    cursor: context.state.cursor,
                    scene: context.state.current_scene.clone(),
                    size: window.size(),
                    scale: window.scale_factor(),
                    camera,
                    canceled: !window.focused
                        || !navigation_ready
                        || !interior(event.position, window.size())
                        || context.mouse.pressed(MouseButton::Left),
                });
            }
            TouchPhase::Moved | TouchPhase::Ended | TouchPhase::Canceled => {
                if let Some(capture) = input
                    .capture
                    .as_mut()
                    .filter(|capture| capture.id == event.id)
                {
                    let delta = capture.moved(event.position);
                    capture.canceled |= event.phase == TouchPhase::Canceled;
                    if capture.owner == Owner::Stage {
                        capture.canceled |= !interior(event.position, window.size())
                            || hit(event.position).0 != capture.owner;
                    }
                    if let Owner::Scroll(entity) = capture.owner
                        && !capture.canceled
                        && capture.excursion > TAP_SLOP
                    {
                        let amount = &mut input.scroll.get_or_insert((entity, 0.0)).1;
                        *amount += delta.y * window.scale_factor();
                    }
                    if event.phase == TouchPhase::Ended {
                        let elapsed = capture.started.elapsed().as_secs_f64();
                        (input.tap, input.swipe) = capture.finish(elapsed);
                        control_click = capture
                            .control_tap()
                            .filter(|entity| hit(event.position).0 == Owner::Control(*entity));
                    }
                }
                if matches!(event.phase, TouchPhase::Ended | TouchPhase::Canceled) {
                    input.fingers.remove(&event.id);
                    if input
                        .capture
                        .as_ref()
                        .is_some_and(|capture| capture.id == event.id)
                    {
                        input.finished = true;
                    }
                }
            }
        }
    }
    if let Some(capture) = input.capture.as_ref() {
        if capture.canceled {
            control_click = None;
            input.tap = false;
            input.swipe = None;
            input.scroll = None;
        }
        input.owner = Some(capture.owner);
        input.finished |= capture.canceled;
        if !capture.canceled {
            let origin = capture
                .camera
                .and_then(|camera| context.cameras.get(camera).ok())
                .and_then(Camera::physical_viewport_rect)
                .map(|rect| rect.min.as_vec2())
                .unwrap_or_default();
            input.point = Some(capture.last * window.scale_factor() - origin);
        }
        // Ordinary touch buttons commit on release. A swipe starting on a tab,
        // restore button, or menu control must not activate it on finger-down.
        // Sliders read the same captured position continuously, independently.
        for (entity, mut interaction) in &mut context.interactions {
            if control_click == Some(entity) {
                interaction.set_if_neq(Interaction::Pressed);
            } else if *interaction == Interaction::Pressed {
                interaction.set_if_neq(Interaction::None);
            }
        }
    }
    // Retain a canceled capture until every finger is up. A second finger's
    // release cannot re-arm a gesture, even across multiple frames.
    if !window.focused {
        input.fingers.clear();
    }
    if input.fingers.is_empty() {
        input.capture = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::platform::{InputActions, PointerClickHistory, collect_input};
    use crate::ui::menu::MenuSurface;
    use crate::ui::save_load::SaveLoadUi;

    fn capture(owner: Owner) -> Capture {
        Capture {
            id: 1,
            owner,
            start: Vec2::new(300.0, 300.0),
            last: Vec2::new(300.0, 300.0),
            excursion: 0.0,
            length: 0.0,
            started: Instant::now(),
            scope: UiInputScope::Stage,
            page: crate::ui::settings_panel::SettingsPage::System,
            cursor: 0,
            scene: String::new(),
            size: Vec2::new(1280.0, 720.0),
            scale: 1.0,
            camera: None,
            canceled: false,
        }
    }

    #[test]
    fn controls_and_lists_never_become_navigation() {
        for owner in [
            Owner::Control(Entity::PLACEHOLDER),
            Owner::Scroll(Entity::PLACEHOLDER),
            Owner::Blocked,
        ] {
            for point in [Vec2::new(100.0, 300.0), Vec2::new(300.0, 100.0)] {
                let mut touch = capture(owner);
                touch.moved(point);
                assert_eq!(touch.finish(0.3), (false, None));
            }
        }
    }

    #[test]
    fn a_control_swipe_never_activates_its_initial_button() {
        let entity = Entity::PLACEHOLDER;
        let mut touch = capture(Owner::Control(entity));
        assert_eq!(touch.control_tap(), Some(entity));
        touch.moved(Vec2::new(340.0, 300.0));
        touch.moved(Vec2::new(300.0, 300.0));
        assert_eq!(touch.control_tap(), None);
        assert_eq!(touch.finish(0.3), (false, None));
        let mut canceled = capture(Owner::Control(entity));
        canceled.canceled = true;
        assert_eq!(canceled.control_tap(), None);
    }

    #[test]
    fn ui_control_clicks_are_deferred_and_clipped_controls_do_not_capture() {
        use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
        use bevy::ui::{IsDefaultUiCamera, UiScale};
        let (mut app, window) = app();
        app.init_resource::<UiScale>()
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
                PostUpdate,
            ))
            .add_systems(
                PostUpdate,
                bevy::ui::update::propagate_ui_target_cameras
                    .before(PropagateSet::<ComputedUiTargetCamera>::default()),
            );
        app.world_mut()
            .spawn((Camera::default(), IsDefaultUiCamera));
        *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Menu;
        app.world_mut().resource_mut::<SettingsUi>().open = true;
        let surface = app
            .world_mut()
            .spawn((
                Node {
                    overflow: Overflow::clip(),
                    ..default()
                },
                MenuSurface::fade_only(),
                bevy::ui::FocusPolicy::Block,
                ComputedNode {
                    size: Vec2::splat(400.0),
                    ..default()
                },
                UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::splat(300.0))),
                InheritedVisibility::VISIBLE,
                ComputedStackIndex(1),
            ))
            .id();
        let button = app
            .world_mut()
            .spawn((
                Node::default(),
                Interaction::Pressed,
                ChildOf(surface),
                ComputedNode {
                    size: Vec2::splat(80.0),
                    ..default()
                },
                UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::splat(300.0))),
                InheritedVisibility::VISIBLE,
                ComputedStackIndex(2),
            ))
            .id();
        // The shared menu header is a full-page transparent surface above
        // settings, with Pass. Its empty area must not steal child-page taps.
        app.world_mut().spawn((
            Node::default(),
            MenuSurface::fade_only(),
            bevy::ui::FocusPolicy::Pass,
            ComputedNode {
                size: Vec2::splat(400.0),
                ..default()
            },
            UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::splat(300.0))),
            InheritedVisibility::VISIBLE,
            ComputedStackIndex(3),
        ));
        app.update();
        app.world_mut()
            .get_mut::<Interaction>(button)
            .unwrap()
            .set_if_neq(Interaction::Pressed);
        let point = Vec2::splat(300.0);
        send(&mut app, window, 1, TouchPhase::Started, point);
        assert_eq!(
            *app.world().get::<Interaction>(button).unwrap(),
            Interaction::None
        );
        send(&mut app, window, 1, TouchPhase::Ended, point);
        assert_eq!(
            *app.world().get::<Interaction>(button).unwrap(),
            Interaction::Pressed
        );
        app.update();
        assert_eq!(
            *app.world().get::<Interaction>(button).unwrap(),
            Interaction::None
        );
        // Holding a button is still a click on release; only stage navigation
        // gestures expire. This must run through the actual interaction writer.
        send(&mut app, window, 1, TouchPhase::Started, point);
        app.world_mut()
            .resource_mut::<TouchInputState>()
            .capture
            .as_mut()
            .unwrap()
            .started -= std::time::Duration::from_secs(2);
        send(&mut app, window, 1, TouchPhase::Ended, point);
        assert_eq!(
            *app.world().get::<Interaction>(button).unwrap(),
            Interaction::Pressed
        );
        app.update();
        send(&mut app, window, 1, TouchPhase::Started, point);
        send(
            &mut app,
            window,
            1,
            TouchPhase::Ended,
            point - Vec2::X * 100.0,
        );
        assert_eq!(
            *app.world().get::<Interaction>(button).unwrap(),
            Interaction::None
        );
        assert!(app.world().resource::<TouchInputState>().swipe.is_none());

        // The child overlaps this point, but its parent's clip excludes it.
        app.world_mut()
            .get_mut::<ComputedNode>(button)
            .unwrap()
            .size = Vec2::splat(600.0);
        send(
            &mut app,
            window,
            1,
            TouchPhase::Started,
            Vec2::new(550.0, 300.0),
        );
        send(
            &mut app,
            window,
            1,
            TouchPhase::Ended,
            Vec2::new(550.0, 300.0),
        );
        assert_eq!(
            *app.world().get::<Interaction>(button).unwrap(),
            Interaction::None
        );
        assert!(app.world().resource::<TouchInputState>().swipe.is_none());

        app.world_mut()
            .get_mut::<ComputedNode>(button)
            .unwrap()
            .size = Vec2::splat(80.0);
        send(
            &mut app,
            window,
            1,
            TouchPhase::Started,
            Vec2::new(450.0, 300.0),
        );
        send(&mut app, window, 1, TouchPhase::Ended, point);
        assert!(app.world().resource::<TouchInputState>().swipe.is_none());
        assert_eq!(
            *app.world().get::<Interaction>(button).unwrap(),
            Interaction::None
        );

        // A menu's empty area must block lower-layer controls.
        app.world_mut()
            .get_mut::<ComputedNode>(button)
            .unwrap()
            .size = Vec2::ZERO;
        let underlay = app
            .world_mut()
            .spawn((
                Node::default(),
                Interaction::None,
                ComputedNode {
                    size: Vec2::splat(80.0),
                    ..default()
                },
                UiGlobalTransform::from(bevy::math::Affine2::from_translation(point)),
                InheritedVisibility::VISIBLE,
                ComputedStackIndex(0),
            ))
            .id();
        app.update();
        send(&mut app, window, 1, TouchPhase::Started, point);
        assert_eq!(
            app.world().resource::<TouchInputState>().owner,
            Some(Owner::Blocked)
        );
        send(&mut app, window, 1, TouchPhase::Ended, point);
        assert_eq!(
            *app.world().get::<Interaction>(underlay).unwrap(),
            Interaction::None
        );
        send(&mut app, window, 1, TouchPhase::Started, point);
        send(
            &mut app,
            window,
            1,
            TouchPhase::Ended,
            point - Vec2::X * 100.0,
        );
        assert!(app.world().resource::<TouchInputState>().swipe.is_none());
        assert_eq!(
            *app.world().get::<Interaction>(underlay).unwrap(),
            Interaction::None
        );
    }

    #[test]
    fn navigation_requires_a_short_straight_unambiguous_swipe() {
        let mut stage = capture(Owner::Stage);
        stage.moved(Vec2::new(310.0, 220.0));
        assert_eq!(stage.finish(0.3), (false, Some(Swipe::Backlog)));
        assert_eq!(stage.finish(1.0), (false, None));
        stage.canceled = true;
        assert_eq!(stage.finish(0.3), (false, None));

        for point in [
            Vec2::new(250.0, 300.0),
            Vec2::new(220.0, 220.0),
            Vec2::new(300.0, 270.0),
            Vec2::new(300.0, 400.0),
        ] {
            let mut touch = capture(Owner::Stage);
            touch.moved(point);
            assert_eq!(touch.finish(0.3), (false, None));
        }
        let mut curved = capture(Owner::Stage);
        curved.moved(Vec2::new(450.0, 300.0));
        curved.moved(Vec2::new(300.0, 100.0));
        assert_eq!(curved.finish(0.3), (false, None));
        let mut return_to_start = capture(Owner::Stage);
        return_to_start.moved(Vec2::new(350.0, 300.0));
        return_to_start.moved(Vec2::new(300.0, 300.0));
        assert_eq!(return_to_start.finish(0.3), (false, None));
    }

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_message::<TouchInput>()
            .init_resource::<TouchInputState>()
            .init_resource::<UiInputScope>()
            .insert_resource(GameState(keine_core::State::new()))
            .init_resource::<SettingsUi>()
            .init_resource::<SaveLoadUi>()
            .init_resource::<MenuRouteTransition>()
            .init_resource::<SettingsPageTransition>()
            .init_resource::<SettingsLocaleTransition>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<bevy::input::keyboard::Key>>()
            .init_resource::<Time>()
            .init_resource::<Time<Real>>()
            .init_resource::<InputActions>()
            .init_resource::<PointerClickHistory>()
            .add_systems(Update, (collect, collect_input).chain());
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        (app, window)
    }

    fn send(app: &mut App, window: Entity, id: u64, phase: TouchPhase, point: Vec2) {
        app.world_mut().write_message(TouchInput {
            window,
            id,
            phase,
            position: point,
            force: None,
        });
        app.update();
    }

    #[test]
    fn a_tap_advances_on_release_but_an_up_swipe_only_opens_backlog() {
        let (mut app, window) = app();
        let start = Vec2::new(300.0, 300.0);
        send(&mut app, window, 1, TouchPhase::Started, start);
        assert!(!app.world().resource::<InputActions>().advance);
        send(&mut app, window, 1, TouchPhase::Ended, start);
        assert!(app.world().resource::<InputActions>().advance);
        app.update();
        assert!(!app.world().resource::<InputActions>().advance);
        send(&mut app, window, 1, TouchPhase::Started, start);
        send(
            &mut app,
            window,
            1,
            TouchPhase::Ended,
            start - Vec2::Y * 100.0,
        );
        let actions = app.world().resource::<InputActions>();
        assert!(!actions.advance);
        assert_eq!(
            actions.shortcut,
            Some(crate::ui::control_bar::ButtonAction::Backlog)
        );
    }

    #[test]
    fn extra_fingers_route_changes_cancel_and_screen_edges_do_not_navigate() {
        for boundary in 0..7 {
            let (mut app, window) = app();
            let start = Vec2::new(300.0, 300.0);
            send(&mut app, window, 1, TouchPhase::Started, start);
            match boundary {
                0 => {
                    send(&mut app, window, 2, TouchPhase::Started, start);
                    send(&mut app, window, 2, TouchPhase::Ended, start);
                }
                1 => *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Dialog,
                2 => app.world_mut().resource_mut::<GameState>().cursor += 1,
                3 => app.world_mut().get_mut::<Window>(window).unwrap().focused = false,
                4 => send(&mut app, window, 1, TouchPhase::Canceled, start),
                5 => send(
                    &mut app,
                    window,
                    1,
                    TouchPhase::Moved,
                    Vec2::new(10.0, 300.0),
                ),
                _ => {
                    app.world_mut()
                        .resource_mut::<TouchInputState>()
                        .capture
                        .as_mut()
                        .unwrap()
                        .started -= std::time::Duration::from_secs(1)
                }
            }
            send(
                &mut app,
                window,
                1,
                TouchPhase::Ended,
                start - Vec2::Y * 100.0,
            );
            assert!(!app.world().resource::<InputActions>().advance);
            assert!(app.world().resource::<InputActions>().shortcut.is_none());
        }
        assert!(!interior(Vec2::new(23.0, 300.0), Vec2::new(1280.0, 720.0)));
    }

    #[test]
    fn horizontal_swipes_do_not_navigate_settings_or_save_load() {
        use crate::ui::save_load::SaveLoadMode;
        use crate::ui::settings_panel::SettingsPage;
        for mode in [None, Some(SaveLoadMode::Save), Some(SaveLoadMode::Load)] {
            let (mut app, window) = app();
            app.add_systems(Update, crate::ui::settings_panel::handle_settings_page);
            *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Menu;
            app.world_mut().resource_mut::<SettingsUi>().open = mode.is_none();
            app.world_mut().resource_mut::<SaveLoadUi>().mode = mode;
            for direction in [-1.0, 1.0] {
                let point = Vec2::splat(300.0);
                send(&mut app, window, 1, TouchPhase::Started, point);
                send(
                    &mut app,
                    window,
                    1,
                    TouchPhase::Ended,
                    point + Vec2::X * direction * 100.0,
                );
                assert!(app.world().resource::<TouchInputState>().swipe.is_none());
                assert!(app.world().resource::<InputActions>().shortcut.is_none());
                assert_eq!(
                    app.world().resource::<SettingsUi>().page,
                    SettingsPage::System
                );
                assert_eq!(app.world().resource::<SaveLoadUi>().mode, mode);
            }
        }
    }

    #[test]
    fn a_clock_rebase_while_waking_does_not_cancel_a_short_touch() {
        let (mut app, window) = app();
        let point = Vec2::splat(300.0);
        send(&mut app, window, 1, TouchPhase::Started, point);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(2));
        send(&mut app, window, 1, TouchPhase::Ended, point);
        assert!(app.world().resource::<InputActions>().advance);
    }

    #[test]
    fn system_back_uses_existing_menu_and_cancel_actions() {
        for scope in [
            UiInputScope::Stage,
            UiInputScope::Menu,
            UiInputScope::Dialog,
            UiInputScope::Backlog,
            UiInputScope::Extra,
            UiInputScope::Title,
        ] {
            let (mut app, _) = app();
            *app.world_mut().resource_mut::<UiInputScope>() = scope;
            app.world_mut()
                .resource_mut::<ButtonInput<bevy::input::keyboard::Key>>()
                .press(bevy::input::keyboard::Key::BrowserBack);
            app.update();
            let actions = app.world().resource::<InputActions>();
            assert!(!actions.advance);
            assert_eq!(actions.back, scope != UiInputScope::Stage);
            assert_eq!(
                actions.shortcut,
                (scope == UiInputScope::Stage)
                    .then_some(crate::ui::control_bar::ButtonAction::System)
            );
        }
    }

    #[test]
    fn captured_slider_uses_physical_coordinates_and_saves_only_when_finished() {
        use crate::runtime::resources::PersistenceRoot;
        use crate::storage::settings::RuntimeSettings;
        use crate::ui::settings_panel::{
            ActiveSettingSlider, SettingKind, SettingSlider, handle_setting_sliders,
        };
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-touch-slider-{nonce}"));
        let mut app = App::new();
        app.init_resource::<RuntimeSettings>()
            .insert_resource(PersistenceRoot(root.clone()))
            .init_resource::<ActiveSettingSlider>()
            .init_resource::<TouchInputState>()
            .init_resource::<ButtonInput<MouseButton>>()
            .insert_resource(UiInputScope::Menu)
            .insert_resource(SettingsUi {
                open: true,
                ..default()
            })
            .init_resource::<MenuRouteTransition>()
            .add_systems(Update, handle_setting_sliders);
        let mut window = Window::default();
        window.resolution.set_scale_factor_override(Some(2.0));
        app.world_mut().spawn(window);
        let slider = app
            .world_mut()
            .spawn((
                SettingSlider(SettingKind::MasterVolume),
                Interaction::None,
                ComputedNode {
                    size: Vec2::new(400.0, 20.0),
                    ..default()
                },
                UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::new(
                    1000.0, 500.0,
                ))),
            ))
            .id();
        {
            let mut input = app.world_mut().resource_mut::<TouchInputState>();
            input.owner = Some(Owner::Control(slider));
            input.point = Some(Vec2::new(1100.0, 500.0));
        }
        app.update();
        assert_eq!(
            app.world().resource::<RuntimeSettings>().master_volume,
            0.75
        );
        assert!(app.world().resource::<ActiveSettingSlider>().is_active());
        assert!(crate::storage::settings::load(&root).is_none());
        // Leaving the track keeps the same owner and clamps its own value.
        {
            let mut input = app.world_mut().resource_mut::<TouchInputState>();
            input.point = Some(Vec2::new(1300.0, 800.0));
            input.finished = true;
        }
        app.update();
        assert_eq!(app.world().resource::<RuntimeSettings>().master_volume, 1.0);
        assert!(!app.world().resource::<ActiveSettingSlider>().is_active());
        assert_eq!(
            crate::storage::settings::load(&root).unwrap().master_volume,
            1.0
        );
        // A modal interruption finishes the capture without reading its cursor.
        {
            let mut input = app.world_mut().resource_mut::<TouchInputState>();
            input.point = Some(Vec2::new(1100.0, 500.0));
            input.finished = false;
        }
        app.update();
        *app.world_mut().resource_mut::<UiInputScope>() = UiInputScope::Dialog;
        app.world_mut().resource_mut::<TouchInputState>().point = Some(Vec2::ZERO);
        app.update();
        assert!(!app.world().resource::<ActiveSettingSlider>().is_active());
        assert_eq!(
            crate::storage::settings::load(&root).unwrap().master_volume,
            0.75
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
