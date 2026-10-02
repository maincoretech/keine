use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::FocusPolicy;
use keine_core::{DESIGN_HEIGHT, DESIGN_WIDTH};

use crate::render::blur::{DialogCamera, UiBlurCamera};
use crate::runtime::resources::{GameConfigResource, GameState};
use crate::scene::audio::VocalPlayer;
use crate::storage::settings::RuntimeSettings;
use crate::ui::control_bar::{BlurStrength, ButtonAction, UiBlurSource};
use crate::ui::foundation::{
    SURFACE_HOVER_ALPHA, SURFACE_IDLE_ALPHA, UiFonts, button_surface, exp_lerp, smoothstep,
};
use crate::ui::input_scope::UiInputScope;
use crate::ui::{BACKLOG_BACKDROP_ALPHA, FULLSCREEN_BLUR_STRENGTH};

const PANEL_ENTER_SECONDS: f32 = 0.2;
const PANEL_EXIT_SECONDS: f32 = 0.32;
const ITEM_ANIMATION_SECONDS: f32 = 0.5;
const ITEM_STAGGER_SECONDS: f32 = 0.02;
const ANIMATED_ITEM_LIMIT: usize = 14;
const BACKLOG_SETTLE_SECONDS: f32 =
    ITEM_ANIMATION_SECONDS + (ANIMATED_ITEM_LIMIT - 1) as f32 * ITEM_STAGGER_SECONDS + 0.05;

#[derive(Resource, Default)]
pub(crate) struct BacklogUiState {
    pub(crate) open: bool,
}

#[derive(Resource, Default)]
pub(crate) struct BacklogScrollMotion {
    current: f32,
    target: f32,
    overscroll: f32,
    initialized: bool,
}

impl BacklogScrollMotion {
    pub(crate) fn is_animating(&self) -> bool {
        self.initialized
            && ((self.current - self.target).abs() > 0.1 || self.overscroll.abs() > 0.1)
    }

    fn advance(&mut self, delta: f32, max: f32, seconds: f32) {
        let desired = self.target - delta;
        self.target = desired.clamp(0.0, max);
        self.overscroll *= (-14.0 * seconds).exp();
        if delta != 0.0 {
            self.overscroll = (self.overscroll - (desired - self.target) * 0.35).clamp(-72.0, 72.0);
        }
        self.current += (self.target - self.current) * exp_lerp(seconds, 24.0);
        if (self.current - self.target).abs() <= 0.1 {
            self.current = self.target;
        }
        if self.overscroll.abs() <= 0.1 {
            self.overscroll = 0.0;
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Component)]
pub(crate) struct BacklogRoot {
    elapsed: f32,
    was_open: bool,
}

impl BacklogRoot {
    pub(crate) fn is_animating(&self, open: bool) -> bool {
        self.was_open != open
            || self.elapsed
                < if open {
                    BACKLOG_SETTLE_SECONDS
                } else {
                    PANEL_EXIT_SECONDS
                }
    }
}

#[derive(Component)]
pub(crate) struct BacklogBlurProxy;

#[derive(Component)]
pub(crate) struct BacklogScroll;

#[derive(Component)]
struct BacklogList;

#[derive(Component)]
pub(crate) struct BacklogClose;

#[derive(Component, Clone, Copy)]
pub(crate) enum BacklogAction {
    Restore(usize),
    Replay(usize),
}

#[derive(Component)]
pub(crate) struct BacklogItemAnimation {
    order: usize,
}

#[derive(Component)]
pub(crate) struct BacklogItemText {
    order: usize,
    base_alpha: f32,
}

#[derive(Component)]
pub(crate) struct BacklogChromeText {
    base_alpha: f32,
}

type BacklogButtonQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        &'static mut BacklogButtonVisual,
        &'static mut BackgroundColor,
        Option<&'static BacklogClose>,
    ),
    (Or<(With<BacklogAction>, With<BacklogClose>)>,),
>;

#[derive(Component, Default)]
pub(crate) struct BacklogButtonVisual {
    current: f32,
}

impl BacklogButtonVisual {
    pub(crate) fn is_animating(&self, interaction: Interaction, close: bool) -> bool {
        let base = if close { 0.0 } else { SURFACE_IDLE_ALPHA };
        let target = if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            SURFACE_HOVER_ALPHA
        } else {
            base
        };
        (self.current - target).abs() > 0.001
    }
}

type BacklogItemTextQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static BacklogItemText,
        &'static mut TextColor,
        Option<&'static mut TextShadow>,
    ),
    (With<BacklogItemText>, Without<BacklogChromeText>),
>;

type BacklogChromeQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static BacklogChromeText,
        &'static mut TextColor,
        Option<&'static mut TextShadow>,
    ),
    (With<BacklogChromeText>, Without<BacklogItemText>),
>;

#[derive(SystemParam)]
pub(crate) struct BacklogAnimationContext<'w, 's> {
    commands: Commands<'w, 's>,
    time: Res<'w, Time>,
    ui: Res<'w, BacklogUiState>,
}

#[derive(SystemParam)]
pub(crate) struct BacklogActionContext<'w, 's> {
    commands: Commands<'w, 's>,
    interactions:
        Query<'w, 's, (&'static Interaction, &'static BacklogAction), Changed<Interaction>>,
    state: ResMut<'w, GameState>,
    ui: ResMut<'w, BacklogUiState>,
    config: Res<'w, GameConfigResource>,
    settings: Res<'w, RuntimeSettings>,
    asset_server: Res<'w, AssetServer>,
    vocals: Query<'w, 's, Entity, With<VocalPlayer>>,
}

pub fn toggle_backlog(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    actions: Res<crate::runtime::platform::InputActions>,
    controls: Query<(&Interaction, &ButtonAction), Changed<Interaction>>,
    close: Query<&Interaction, (With<BacklogClose>, Changed<Interaction>)>,
    mut ui: ResMut<BacklogUiState>,
) {
    let control_pressed = controls.iter().any(|(interaction, action)| {
        *interaction == Interaction::Pressed && *action == ButtonAction::Backlog
    });
    let close_pressed = close
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed);
    if control_pressed || actions.shortcut == Some(ButtonAction::Backlog) {
        ui.open = !ui.open;
    } else if ui.open
        && (close_pressed
            || keys.just_pressed(KeyCode::Escape)
            || mouse.just_pressed(MouseButton::Right))
    {
        ui.open = false;
    }
}

pub fn sync_backlog(
    mut commands: Commands,
    ui: Res<BacklogUiState>,
    state: Res<GameState>,
    roots: Query<Entity, With<BacklogRoot>>,
    ui_camera: Query<Entity, With<UiBlurCamera>>,
    dialog_camera: Query<Entity, With<DialogCamera>>,
    fonts: Res<UiFonts>,
) {
    if !ui.open || !roots.is_empty() || state.ended {
        return;
    }
    let (Ok(ui_camera), Ok(dialog_camera)) = (ui_camera.single(), dialog_camera.single()) else {
        return;
    };
    commands.spawn((
        Name::new("backlog_blur_proxy"),
        BacklogBlurProxy,
        UiBlurSource,
        BlurStrength(0.0),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(DESIGN_WIDTH),
            height: Val::Px(DESIGN_HEIGHT),
            ..default()
        },
        FocusPolicy::Pass,
        GlobalZIndex(169),
        UiTargetCamera(ui_camera),
        RenderLayers::layer(1),
    ));
    commands
        .spawn((
            Name::new("backlog"),
            BacklogRoot {
                elapsed: 0.0,
                was_open: true,
            },
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(DESIGN_WIDTH),
                height: Val::Px(DESIGN_HEIGHT),
                padding: UiRect::axes(Val::ZERO, Val::Px(24.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            FocusPolicy::Block,
            GlobalZIndex(170),
            UiTargetCamera(dialog_camera),
            RenderLayers::layer(2),
        ))
        .with_children(|root| {
            spawn_header(root, &fonts);
            spawn_content(root, &state, &fonts);
        });
}

fn spawn_header(root: &mut ChildSpawnerCommands, assets: &UiFonts) {
    root.spawn((
        Name::new("backlog_top"),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(24.0),
            top: Val::Px(24.0),
            width: Val::Percent(96.0),
            height: Val::Percent(8.0),
            align_items: AlignItems::FlexStart,
            ..default()
        },
    ))
    .with_children(|header| {
        header
            .spawn((
                Button,
                BacklogClose,
                BacklogButtonVisual::default(),
                Node {
                    width: Val::Px(54.0),
                    height: Val::Px(54.0),
                    margin: UiRect::right(Val::Px(9.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_child(chrome_text("\u{f659}", &assets.icons, 45.0, 0.8, false));
        header.spawn(chrome_text("BACKLOG", &assets.text, 43.5, 1.0, true));
    });
}

fn spawn_content(root: &mut ChildSpawnerCommands, state: &GameState, assets: &UiFonts) {
    root.spawn((
        Name::new("backlog_content"),
        BacklogScroll,
        ScrollPosition::default(),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(12.0),
            width: Val::Percent(100.0),
            height: Val::Percent(80.0),
            padding: UiRect::axes(Val::Px(120.0), Val::Px(12.0)),
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            ..default()
        },
    ))
    .with_children(|viewport| {
        viewport
            .spawn((
                BacklogList,
                UiTransform::default(),
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::FlexEnd,
                    flex_shrink: 0.0,
                    ..default()
                },
            ))
            .with_children(|list| {
                for (index, entry) in state.backlog.iter().enumerate() {
                    let order = state.backlog.len() - 1 - index;
                    let mut item = list.spawn((
                        Name::new(format!("backlog_item::{index}")),
                        Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(51.0),
                            margin: UiRect::top(Val::Px(15.0)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    if order < ANIMATED_ITEM_LIMIT {
                        item.insert((
                            BacklogItemAnimation { order },
                            UiTransform::from_xy(Val::Px(-11.25), Val::Px(7.5)),
                        ));
                    } else {
                        item.insert(UiTransform::default());
                    }
                    item.with_children(|row| {
                        spawn_item_functions(row, index, order, entry, assets);
                        row.spawn((
                            Node {
                                width: Val::Percent(70.0),
                                padding: UiRect::left(Val::Px(12.0)),
                                ..default()
                            },
                            children![item_text(
                                entry.text.clone(),
                                &assets.text,
                                26.25,
                                order,
                                false,
                            )],
                        ));
                    });
                }
            });
    });
}

fn spawn_item_functions(
    row: &mut ChildSpawnerCommands,
    index: usize,
    order: usize,
    entry: &keine_core::state::BacklogEntry,
    assets: &UiFonts,
) {
    row.spawn((Node {
        width: Val::Percent(30.0),
        max_width: Val::Percent(30.0),
        min_width: Val::Percent(30.0),
        align_items: AlignItems::FlexStart,
        ..default()
    },))
        .with_children(|area| {
            area.spawn((Node {
                margin: UiRect::top(Val::Px(7.5)),
                ..default()
            },))
                .with_children(|buttons| {
                    spawn_item_button(
                        buttons,
                        BacklogAction::Restore(index),
                        "\u{f138}",
                        order,
                        assets,
                    );
                    if entry.vocal.is_some() {
                        spawn_item_button(
                            buttons,
                            BacklogAction::Replay(index),
                            "\u{f57c}",
                            order,
                            assets,
                        );
                    }
                });
            area.spawn((
                Node {
                    width: Val::Percent(50.0),
                    margin: UiRect::left(Val::Auto),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(4.5)),
                    ..default()
                },
                children![item_text(
                    entry.speaker.clone(),
                    &assets.text,
                    29.25,
                    order,
                    true,
                )],
            ));
        });
}

fn spawn_item_button(
    buttons: &mut ChildSpawnerCommands,
    action: BacklogAction,
    icon: &str,
    order: usize,
    assets: &UiFonts,
) {
    buttons
        .spawn((
            Button,
            action,
            BacklogButtonVisual::default(),
            Node {
                margin: UiRect::left(Val::Px(6.0)),
                width: Val::Px(45.0),
                height: Val::Px(45.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(button_surface(SURFACE_IDLE_ALPHA)),
        ))
        .with_child(item_text(icon, &assets.icons, 21.0, order, false));
}

fn chrome_text(
    content: impl Into<String>,
    font: &Handle<Font>,
    size: f32,
    base_alpha: f32,
    bold: bool,
) -> impl Bundle {
    (
        BacklogChromeText { base_alpha },
        animated_text(content, font, size, bold, 0.0),
    )
}

fn item_text(
    content: impl Into<String>,
    font: &Handle<Font>,
    size: f32,
    order: usize,
    bold: bool,
) -> impl Bundle {
    (
        BacklogItemText {
            order,
            base_alpha: 1.0,
        },
        animated_text(
            content,
            font,
            size,
            bold,
            if order < ANIMATED_ITEM_LIMIT {
                0.0
            } else {
                1.0
            },
        ),
    )
}

fn animated_text(
    content: impl Into<String>,
    font: &Handle<Font>,
    size: f32,
    bold: bool,
    initial_alpha: f32,
) -> impl Bundle {
    (
        Text::new(content.into()),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::from(size),
            weight: if bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            },
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, initial_alpha)),
    )
}

pub fn animate_backlog(
    mut context: BacklogAnimationContext,
    mut roots: Query<(Entity, &mut BacklogRoot, &mut BackgroundColor)>,
    mut proxies: Query<(Entity, &mut BlurStrength), With<BacklogBlurProxy>>,
    mut items: Query<(&BacklogItemAnimation, &mut UiTransform)>,
    mut item_texts: BacklogItemTextQuery,
    mut chrome: BacklogChromeQuery,
) {
    let Ok((entity, mut root, mut background)) = roots.single_mut() else {
        return;
    };
    if root.was_open != context.ui.open {
        root.was_open = context.ui.open;
        root.elapsed = 0.0;
    } else {
        root.elapsed += context.time.delta_secs();
    }
    if context.ui.open && root.elapsed >= BACKLOG_SETTLE_SECONDS {
        return;
    }
    let panel = if context.ui.open {
        (root.elapsed / PANEL_ENTER_SECONDS).clamp(0.0, 1.0)
    } else {
        1.0 - (root.elapsed / PANEL_EXIT_SECONDS).clamp(0.0, 1.0)
    };
    let panel = smoothstep(panel);
    background.0 = Color::srgba(0.0, 0.0, 0.0, BACKLOG_BACKDROP_ALPHA * panel);
    for (_, mut blur) in &mut proxies {
        blur.0 = FULLSCREEN_BLUR_STRENGTH * panel;
    }
    for (marker, mut color, shadow) in &mut chrome {
        let alpha = marker.base_alpha * panel;
        color.0 = Color::srgba(1.0, 1.0, 1.0, alpha);
        if let Some(mut shadow) = shadow {
            shadow.color = Color::srgba(0.0, 0.0, 0.0, 0.9 * alpha);
        }
    }

    for (marker, mut transform) in &mut items {
        let delay = marker.order as f32 * ITEM_STAGGER_SECONDS;
        let progress = if context.ui.open {
            ((root.elapsed - delay) / ITEM_ANIMATION_SECONDS).clamp(0.0, 1.0)
        } else {
            panel
        };
        let progress = smoothstep(progress);
        transform.scale = Vec2::splat(1.05 - 0.05 * progress);
        transform.translation = Val2::new(
            Val::Px(-15.0 * (1.0 - progress)),
            Val::Px(10.0 * (1.0 - progress)),
        );
    }
    for (marker, mut color, shadow) in &mut item_texts {
        let delay = marker.order as f32 * ITEM_STAGGER_SECONDS;
        let progress = if context.ui.open {
            ((root.elapsed - delay) / ITEM_ANIMATION_SECONDS).clamp(0.0, 1.0)
        } else {
            panel
        };
        let alpha = marker.base_alpha * smoothstep(progress);
        color.0 = Color::srgba(1.0, 1.0, 1.0, alpha);
        if let Some(mut shadow) = shadow {
            shadow.color = Color::srgba(0.0, 0.0, 0.0, 0.9 * alpha);
        }
    }
    if !context.ui.open && root.elapsed >= PANEL_EXIT_SECONDS {
        context.commands.entity(entity).despawn();
        for (proxy, _) in &mut proxies {
            context.commands.entity(proxy).despawn();
        }
    }
}

pub fn animate_backlog_buttons(time: Res<Time>, mut buttons: BacklogButtonQuery) {
    let amount = exp_lerp(time.delta_secs(), 10.0);
    for (interaction, mut visual, mut background, close) in &mut buttons {
        let base = if close.is_some() {
            0.0
        } else {
            SURFACE_IDLE_ALPHA
        };
        let target = if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            SURFACE_HOVER_ALPHA
        } else {
            base
        };
        if (visual.current - target).abs() < 0.001 {
            continue;
        }
        visual.current += (target - visual.current) * amount;
        background.0 = button_surface(visual.current);
    }
}

#[derive(SystemParam)]
pub(crate) struct BacklogScrollContext<'w, 's> {
    wheel: MessageReader<'w, 's, MouseWheel>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    time: Res<'w, Time>,
    loading: Res<'w, crate::runtime::resources::AssetLoadingGate>,
    scope: Res<'w, UiInputScope>,
    ui: ResMut<'w, BacklogUiState>,
    motion: ResMut<'w, BacklogScrollMotion>,
    scroll:
        Query<'w, 's, (&'static mut ScrollPosition, &'static ComputedNode), With<BacklogScroll>>,
    list: Query<'w, 's, &'static mut UiTransform, With<BacklogList>>,
}

pub fn scroll_backlog(mut context: BacklogScrollContext) {
    if context.loading.blocked || !context.scope.allows_backlog() {
        context.wheel.read().for_each(drop);
        context.motion.reset();
        return;
    }
    let control_pressed = context
        .keys
        .any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let mut delta = 0.0;
    for event in context.wheel.read() {
        if control_pressed {
            continue;
        }
        if !context.ui.open {
            if event.y > 0.0 {
                context.ui.open = true;
            }
            continue;
        }
        let amount = event.y
            * match event.unit {
                MouseScrollUnit::Line => 36.0,
                MouseScrollUnit::Pixel => 1.0,
            };
        delta += amount;
    }
    let Ok((mut position, computed)) = context.scroll.single_mut() else {
        context.motion.reset();
        return;
    };
    let max =
        (computed.content_size().y - computed.size().y).max(0.0) * computed.inverse_scale_factor();
    if !context.motion.initialized {
        if computed.size().y <= 0.0 {
            return;
        }
        // Chronological layout uses positive offsets; open at the newest line.
        context.motion.current = max;
        context.motion.target = max;
        context.motion.initialized = true;
    }
    context
        .motion
        .advance(delta, max, context.time.delta_secs());
    position.y = context.motion.current.clamp(0.0, max);
    if let Ok(mut transform) = context.list.single_mut() {
        // Bevy clamps ScrollPosition, so apply edge movement to the list while
        // keeping its viewport and clipping bounds fixed.
        *transform = UiTransform::from_xy(Val::Px(0.0), Val::Px(context.motion.overscroll));
    }
}

pub fn handle_backlog_action(mut context: BacklogActionContext) {
    let Some(action) = context
        .interactions
        .iter()
        .find_map(|(interaction, action)| {
            (*interaction == Interaction::Pressed).then_some(*action)
        })
    else {
        return;
    };
    match action {
        BacklogAction::Restore(index) => {
            if context.state.restore_backlog(index) {
                context.ui.open = false;
            }
        }
        BacklogAction::Replay(index) => {
            let Some(dialogue) = context.state.backlog.get(index).and_then(|entry| {
                entry
                    .snapshot
                    .dialogue
                    .vocal
                    .as_ref()
                    .map(|v| (v, entry.snapshot.dialogue.volume))
            }) else {
                return;
            };
            for entity in &context.vocals {
                context.commands.entity(entity).despawn();
            }
            crate::scene::audio::spawn_vocal(
                &mut context.commands,
                &context.asset_server,
                &context.config,
                &context.settings,
                dialogue.1,
                dialogue.0,
                0.0,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_up_moves_towards_older_lines_and_down_returns_to_newest() {
        let mut motion = BacklogScrollMotion {
            current: 300.0,
            target: 300.0,
            initialized: true,
            ..default()
        };
        motion.advance(36.0, 300.0, 1.0 / 60.0);
        assert_eq!(motion.target, 264.0);
        assert!(motion.current < 300.0);
        motion.advance(-36.0, 300.0, 1.0 / 60.0);
        assert_eq!(motion.target, 300.0);
    }

    #[test]
    fn both_edges_resist_and_return_without_moving_the_scroll_bounds() {
        for (edge, delta, sign) in [(0.0, 120.0, 1.0), (300.0, -120.0, -1.0)] {
            let mut motion = BacklogScrollMotion {
                current: edge,
                target: edge,
                initialized: true,
                ..default()
            };
            motion.advance(delta, 300.0, 1.0 / 60.0);
            assert_eq!(motion.target, edge);
            assert!(motion.overscroll * sign > 0.0);
            assert!(motion.is_animating());
            for _ in 0..120 {
                motion.advance(0.0, 300.0, 1.0 / 60.0);
            }
            assert_eq!(motion.overscroll, 0.0);
            assert!(!motion.is_animating());
        }
    }
}
