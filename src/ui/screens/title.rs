use bevy::asset::LoadState;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use crate::render::blur::{DialogCamera, UiBlurCamera};
use crate::runtime::platform::DesignViewport;
use crate::runtime::platform::InputActions;
use crate::runtime::resources::{GameConfigResource, GameState, PersistenceRoot};
use crate::scene::images::{ImageRole, ImageRoleRegistry};
use crate::ui::control_bar::{BlurSource, BlurStrength, QuickSavePreview, QuickSavePreviewLoad};
use crate::ui::dialog::{DialogAction, DialogRequest};
use crate::ui::foundation::{
    SURFACE_HOVER_ALPHA, UiFonts, button_surface, dark_surface, exp_lerp, smoothstep, text,
};
use keine_core::{DESIGN_HEIGHT, DESIGN_WIDTH};

const RETURN_COVER_SECONDS: f32 = 0.24;
const RETURN_REVEAL_SECONDS: f32 = 0.34;

#[derive(Component)]
pub struct TitleRoot;

#[derive(Component)]
pub struct TitleBackground;

#[derive(Component, Clone, Copy)]
pub enum TitleAction {
    Start,
    Continue,
    Load,
    Extra,
    Options,
    Exit,
}

#[derive(Resource)]
pub struct PendingTitleAction {
    action: TitleAction,
    remaining: f32,
}

#[derive(Clone, Copy, Default)]
enum ReturnToTitlePhase {
    #[default]
    Cover,
    Reveal,
}

#[derive(Resource, Default)]
pub(crate) struct ReturnToTitleTransition {
    elapsed: f32,
    phase: ReturnToTitlePhase,
    title_background: Option<Handle<Image>>,
}

pub(crate) fn handle_script_outcome(
    commands: &mut Commands,
    outcome: crate::runtime::script_driver::ScriptOutcome,
) {
    if outcome.returns_to_title() {
        commands.insert_resource(ReturnToTitleTransition::default());
    }
}

#[derive(Component)]
pub(crate) struct ReturnToTitleOverlay;

#[derive(SystemParam)]
pub(crate) struct ReturnToTitleContext<'w, 's> {
    time: Res<'w, Time<Real>>,
    config: Res<'w, GameConfigResource>,
    asset_server: Res<'w, AssetServer>,
    image_roles: Res<'w, ImageRoleRegistry>,
    state: ResMut<'w, GameState>,
    camera: Query<'w, 's, Entity, With<DialogCamera>>,
    overlays: Query<'w, 's, (Entity, &'static mut BackgroundColor), With<ReturnToTitleOverlay>>,
    commands: Commands<'w, 's>,
}

pub(crate) fn animate_return_to_title(
    transition: Option<ResMut<ReturnToTitleTransition>>,
    mut context: ReturnToTitleContext,
) {
    let Some(mut transition) = transition else {
        return;
    };
    if transition.title_background.is_none() {
        transition.title_background = Some(crate::scene::images::load(
            &context.asset_server,
            &context.image_roles,
            context.config.bg_path(&context.config.title_background),
            ImageRole::BACKGROUND,
        ));
    }
    if context.overlays.is_empty() {
        if let Ok(camera) = context.camera.single() {
            context.commands.spawn((
                ReturnToTitleOverlay,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(DESIGN_WIDTH),
                    height: Val::Px(DESIGN_HEIGHT),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                FocusPolicy::Block,
                GlobalZIndex(1000),
                UiTargetCamera(camera),
                RenderLayers::layer(2),
            ));
        }
        // Do not consume transition time before the covering surface actually
        // participates in a rendered frame.
        return;
    }

    transition.elapsed += context.time.delta_secs();
    let alpha = match transition.phase {
        ReturnToTitlePhase::Cover => {
            let alpha = transition_alpha(transition.phase, transition.elapsed);
            let background_ready = transition.title_background.as_ref().is_some_and(|handle| {
                context.asset_server.is_loaded_with_dependencies(handle)
                    || matches!(
                        context.asset_server.get_load_state(handle),
                        Some(LoadState::Failed(_))
                    )
            });
            if transition.elapsed >= RETURN_COVER_SECONDS && background_ready {
                keine_core::step::end_game(&mut context.state);
                transition.phase = ReturnToTitlePhase::Reveal;
                transition.elapsed = 0.0;
            }
            alpha
        }
        ReturnToTitlePhase::Reveal => transition_alpha(transition.phase, transition.elapsed),
    };
    for (_, mut background) in &mut context.overlays {
        background.0 = Color::srgba(0.0, 0.0, 0.0, alpha);
    }
    if matches!(transition.phase, ReturnToTitlePhase::Reveal)
        && transition.elapsed >= RETURN_REVEAL_SECONDS
    {
        for (entity, _) in &mut context.overlays {
            context.commands.entity(entity).despawn();
        }
        context
            .commands
            .remove_resource::<ReturnToTitleTransition>();
    }
}

fn transition_alpha(phase: ReturnToTitlePhase, elapsed: f32) -> f32 {
    let ratio = match phase {
        ReturnToTitlePhase::Cover => elapsed / RETURN_COVER_SECONDS,
        ReturnToTitlePhase::Reveal => 1.0 - elapsed / RETURN_REVEAL_SECONDS,
    };
    crate::ui::foundation::smoothstep(ratio.clamp(0.0, 1.0))
}

#[derive(Component)]
pub struct TitleButtonMotion {
    width: f32,
    padding: f32,
    press: f32,
    hover: f32,
}

impl TitleButtonMotion {
    pub(crate) fn is_animating(&self, interaction: Interaction) -> bool {
        let (target_width, target_padding) = match interaction {
            Interaction::None => (100.0, 22.5),
            Interaction::Hovered => (96.25, 22.5),
            Interaction::Pressed => (92.0, 20.25),
        };
        let target_hover = if interaction == Interaction::None {
            0.0
        } else {
            SURFACE_HOVER_ALPHA
        };
        (self.width - target_width).abs() > 0.001
            || (self.padding - target_padding).abs() > 0.001
            || self.press > 0.001
            || (self.hover - target_hover).abs() > 0.001
    }
}

const CONTINUE_PREVIEW_ALPHA: f32 = 0.78;
const CONTINUE_PREVIEW_GAP_PERCENT: f32 = 5.0;
pub(crate) const TITLE_GLASS_BLUR: f32 = 36.0;
const DISABLED_BUTTON_SURFACE_ALPHA: f32 = 0.28;
const DISABLED_BUTTON_TEXT_ALPHA: f32 = 0.36;

#[derive(Component, Default)]
pub struct TitleContinuePreview {
    progress: f32,
    target: f32,
}

impl TitleContinuePreview {
    pub(crate) fn is_animating(&self) -> bool {
        (self.progress - self.target).abs() > 0.001
    }
}

#[derive(Component)]
pub(crate) struct TitleContinuePreviewAlpha(f32);

#[derive(Component)]
pub(crate) struct TitleButtonVisual;

type TitleButtonAnimationQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        &'static mut TitleButtonMotion,
        &'static Children,
        Option<&'static TitleAction>,
    ),
>;

type TitleButtonVisualQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Node,
        &'static mut UiTransform,
        &'static mut BackgroundColor,
    ),
    (With<TitleButtonVisual>, Without<TitleContinuePreview>),
>;

#[derive(SystemParam)]
pub struct TitleSyncContext<'w, 's> {
    config: Res<'w, GameConfigResource>,
    roots: Query<'w, 's, (Entity, &'static mut Node), With<TitleRoot>>,
    backgrounds:
        Query<'w, 's, (Entity, &'static mut Sprite, &'static mut Transform), With<TitleBackground>>,
    camera: Query<'w, 's, Entity, With<UiBlurCamera>>,
    fonts: Res<'w, UiFonts>,
    asset_server: Res<'w, AssetServer>,
    image_roles: Res<'w, ImageRoleRegistry>,
    preview: Res<'w, QuickSavePreview>,
    windows: Query<'w, 's, &'static Window>,
    commands: Commands<'w, 's>,
}

#[derive(SystemParam)]
pub struct TitleInputContext<'w, 's> {
    buttons: Query<'w, 's, (&'static Interaction, &'static TitleAction), Changed<Interaction>>,
    keys: ResMut<'w, ButtonInput<KeyCode>>,
    actions: ResMut<'w, InputActions>,
    state: ResMut<'w, GameState>,
    checkpoint: ResMut<'w, crate::storage::save::ContinuationCheckpoint>,
    config: Res<'w, GameConfigResource>,
    preview: Res<'w, QuickSavePreview>,
    project_root: Res<'w, PersistenceRoot>,
    store: Res<'w, crate::runtime::resources::StoreCodec>,
    time: Res<'w, Time>,
    pending: Option<ResMut<'w, PendingTitleAction>>,
    save_load: ResMut<'w, crate::ui::save_load::SaveLoadUi>,
    settings: ResMut<'w, crate::ui::settings_panel::SettingsUi>,
    runtime_settings: Res<'w, crate::storage::settings::RuntimeSettings>,
    extra: ResMut<'w, crate::ui::extra::ExtraUi>,
    commands: Commands<'w, 's>,
}

pub fn sync_title(state: Res<GameState>, mut context: TitleSyncContext) {
    let Ok(window) = context.windows.single() else {
        return;
    };
    let viewport = DesignViewport::from_window(window);
    for (_, mut node) in &mut context.roots {
        layout_title_root(&mut node, viewport);
    }
    for (_, mut sprite, mut transform) in &mut context.backgrounds {
        layout_title_background(&mut sprite, &mut transform, viewport);
    }
    let title_present = !context.roots.is_empty();
    let title_config_changed = state.ended && title_present && context.config.is_changed();
    let continue_state_changed = state.ended && title_present && context.preview.is_changed();
    if state.ended == title_present && !title_config_changed && !continue_state_changed {
        return;
    }
    for (entity, _) in &context.roots {
        context.commands.entity(entity).despawn();
    }
    for (entity, _, _) in &context.backgrounds {
        context.commands.entity(entity).despawn();
    }
    if !state.ended {
        return;
    }
    let Ok(camera) = context.camera.single() else {
        return;
    };
    let font = context.fonts.text.clone();
    let background = crate::scene::images::load(
        &context.asset_server,
        &context.image_roles,
        context.config.bg_path(&context.config.title_background),
        ImageRole::BACKGROUND,
    );
    context.commands.spawn((
        Name::new("title_background"),
        TitleBackground,
        Sprite {
            image: background,
            custom_size: Some(Vec2::new(DESIGN_WIDTH, DESIGN_HEIGHT) * viewport.scale),
            ..default()
        },
        Transform::from_translation(viewport.content_center().extend(0.0)),
        RenderLayers::layer(0),
    ));
    context
        .commands
        .spawn((
            Name::new("title"),
            TitleRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::ZERO,
                top: Val::ZERO,
                width: Val::Px(DESIGN_WIDTH),
                height: Val::Px(DESIGN_HEIGHT),
                ..default()
            },
            BackgroundColor(Color::NONE),
            FocusPolicy::Block,
            GlobalZIndex(160),
            UiTargetCamera(camera),
            RenderLayers::layer(1),
        ))
        .with_children(|title| {
            title.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.01, 0.015, 0.04, 0.2)),
            ));
            title
                .spawn((Node {
                    position_type: PositionType::Absolute,
                    right: Val::Percent(10.0),
                    top: Val::Percent(17.0),
                    width: Val::Percent(20.5),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(30.0),
                    ..default()
                },))
                .with_children(|menu| {
                    spawn_title_button(menu, "START", Some(TitleAction::Start), &font, None);
                    if context.preview.can_continue(state.program_fingerprint) {
                        spawn_title_button(
                            menu,
                            "CONTINUE",
                            Some(TitleAction::Continue),
                            &font,
                            Some(&context.preview),
                        );
                    } else {
                        spawn_disabled_button(menu, "CONTINUE", &font);
                    }
                    spawn_title_button(menu, "LOAD", Some(TitleAction::Load), &font, None);
                    spawn_extra_button(menu, &font, context.config.features.extra);
                    spawn_title_button(menu, "OPTIONS", Some(TitleAction::Options), &font, None);
                    spawn_title_button(menu, "EXIT", Some(TitleAction::Exit), &font, None);
                });
        });
}

pub fn hydrate_quick_save_preview(
    buttons: Query<(&Interaction, &TitleAction), Changed<Interaction>>,
    project_root: Res<PersistenceRoot>,
    mut load: ResMut<QuickSavePreviewLoad>,
    mut preview: ResMut<QuickSavePreview>,
    mut images: ResMut<Assets<Image>>,
) {
    if let Some(details) = load.poll() {
        preview.image = details.image.map(|image| images.add(image));
    }
    let requested = buttons.iter().any(|(interaction, action)| {
        *interaction == Interaction::Hovered && matches!(action, TitleAction::Continue)
    });
    if requested && preview.state.is_some() && preview.image.is_none() {
        load.request(&project_root);
    }
}

fn spawn_extra_button(menu: &mut ChildSpawnerCommands, font: &Handle<Font>, extra_enabled: bool) {
    if extra_enabled {
        spawn_title_button(menu, "EXTRA", Some(TitleAction::Extra), font, None);
    }
}

fn layout_title_root(node: &mut Node, _viewport: DesignViewport) {
    node.left = Val::ZERO;
    node.top = Val::ZERO;
    node.width = Val::Px(DESIGN_WIDTH);
    node.height = Val::Px(DESIGN_HEIGHT);
}

fn layout_title_background(
    sprite: &mut Sprite,
    transform: &mut Transform,
    viewport: DesignViewport,
) {
    sprite.custom_size = Some(Vec2::new(DESIGN_WIDTH, DESIGN_HEIGHT) * viewport.scale);
    transform.translation = viewport.content_center().extend(0.0);
}

fn spawn_title_button(
    menu: &mut ChildSpawnerCommands,
    label: &str,
    action: Option<TitleAction>,
    font: &Handle<Font>,
    preview: Option<&QuickSavePreview>,
) {
    let hit_width = if preview.is_some() {
        100.0 + CONTINUE_PREVIEW_GAP_PERCENT
    } else {
        100.0
    };
    // Continue's fixed hit area also covers the intentional visual gap before
    // its preview. Only the inner surface animates, so hover never falls into
    // an uncovered seam.
    menu.spawn((
        Node {
            position_type: PositionType::Relative,
            width: Val::Percent(100.0),
            height: Val::Px(94.5),
            ..default()
        },
        BackgroundColor(Color::NONE),
    ))
    .with_children(|surface| {
        let mut entity = surface.spawn((
            Button,
            TitleButtonMotion {
                width: 100.0,
                padding: 22.5,
                press: 0.0,
                hover: 0.0,
            },
            Node {
                position_type: PositionType::Absolute,
                right: Val::ZERO,
                top: Val::ZERO,
                width: Val::Percent(hit_width),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ));
        if let Some(action) = action {
            entity.insert(action);
        }
        entity.with_child((
            TitleButtonVisual,
            UiTransform::default(),
            BlurSource,
            BlurStrength(TITLE_GLASS_BLUR),
            Node {
                position_type: PositionType::Absolute,
                right: Val::ZERO,
                top: Val::ZERO,
                width: Val::Percent(100.0 / hit_width * 100.0),
                height: Val::Percent(100.0),
                padding: UiRect::left(Val::Px(22.5)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(button_surface(0.0)),
            children![
                (
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::ZERO,
                        top: Val::ZERO,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(dark_surface(0.64)),
                    FocusPolicy::Pass,
                ),
                text(label, font, 37.5, 0.9)
            ],
        ));
        if let Some(preview) = preview {
            spawn_continue_preview(surface, preview, font);
        }
    });
}

fn spawn_continue_preview(
    surface: &mut ChildSpawnerCommands,
    preview: &QuickSavePreview,
    font: &Handle<Font>,
) {
    surface
        .spawn((
            TitleContinuePreview::default(),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Percent(100.0 + CONTINUE_PREVIEW_GAP_PERCENT),
                top: Val::Px(0.0),
                width: Val::Px(675.0),
                height: Val::Px(172.5),
                padding: UiRect::all(Val::Px(9.0)),
                display: Display::None,
                column_gap: Val::Px(10.5),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
            BlurSource,
            BlurStrength(36.0),
        ))
        .with_children(|panel| {
            if let Some(image) = &preview.image {
                let mut image = ImageNode::new(image.clone());
                image.color = image.color.with_alpha(0.0);
                panel.spawn((
                    image,
                    TitleContinuePreviewAlpha(1.0),
                    Node {
                        width: Val::Px(270.0),
                        height: Val::Percent(100.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
            }
            let (speaker, dialogue) = preview
                .state
                .as_ref()
                .map_or(("NO SAVE DATA", ""), |state| {
                    (state.speaker.as_str(), state.dialogue.as_str())
                });
            panel
                .spawn((Node {
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(9.0),
                    ..default()
                },))
                .with_children(|copy| {
                    copy.spawn((
                        text(speaker, font, 22.5, 0.0),
                        TitleContinuePreviewAlpha(0.8),
                    ));
                    copy.spawn((
                        text(dialogue, font, 18.75, 0.0),
                        TitleContinuePreviewAlpha(0.67),
                    ));
                });
        });
}

fn spawn_disabled_button(menu: &mut ChildSpawnerCommands, label: &str, font: &Handle<Font>) {
    menu.spawn((
        BlurSource,
        BlurStrength(TITLE_GLASS_BLUR),
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(94.5),
            padding: UiRect::left(Val::Px(25.5)),
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(dark_surface(DISABLED_BUTTON_SURFACE_ALPHA)),
        children![text(label, font, 37.5, DISABLED_BUTTON_TEXT_ALPHA)],
    ));
}

pub fn handle_title_input(mut context: TitleInputContext) {
    let can_continue = context
        .preview
        .can_continue(context.state.program_fingerprint);
    let action = context
        .buttons
        .iter()
        .find_map(|(interaction, action)| (*interaction == Interaction::Pressed).then_some(*action))
        .or_else(|| title_keyboard_action(&context.keys, &context.actions, can_continue));
    if !context.state.ended {
        context.commands.remove_resource::<PendingTitleAction>();
        return;
    }
    if let Some(outcome) = start_game_from_keyboard(
        &mut context.keys,
        &mut context.actions,
        &mut context.state,
        &mut context.checkpoint,
    ) {
        handle_script_outcome(&mut context.commands, outcome);
        return;
    }
    let action = if let Some(mut pending) = context.pending {
        pending.remaining -= context.time.delta_secs();
        if pending.remaining > 0.0 {
            return;
        }
        let action = pending.action;
        context.commands.remove_resource::<PendingTitleAction>();
        let allowed = match action {
            TitleAction::Continue => can_continue,
            TitleAction::Extra => context.config.features.extra,
            _ => true,
        };
        allowed.then_some(action)
    } else if let Some(action) = action {
        context.commands.insert_resource(PendingTitleAction {
            action,
            remaining: 0.1,
        });
        return;
    } else {
        None
    };
    match action {
        Some(TitleAction::Continue) => {
            if let Ok(loaded) = crate::storage::save::load_game(
                context.store.0.as_ref(),
                crate::storage::save::QUICK_SAVE_SLOT,
                &context.project_root,
            ) {
                if let Err(error) = restore_continuation(loaded, &mut context.state) {
                    log::error!("continue rejected: {error}");
                    context
                        .commands
                        .insert_resource(DialogRequest::confirmation(
                            crate::ui::support::i18n::tr(
                                context.runtime_settings.locale,
                                crate::ui::support::i18n::UiText::ForeignSave,
                            ),
                            DialogAction::Noop,
                        ));
                } else {
                    context.checkpoint.reset(&context.state);
                    *context.actions = InputActions::default();
                    log::info!(
                        "continued quick save · {}:{}",
                        context.state.current_scene,
                        context.state.cursor
                    );
                }
            } else {
                context
                    .commands
                    .insert_resource(DialogRequest::confirmation(
                        crate::ui::support::i18n::tr(
                            context.runtime_settings.locale,
                            crate::ui::support::i18n::UiText::NoSaveData,
                        ),
                        DialogAction::Noop,
                    ));
            }
        }
        Some(TitleAction::Exit) => {
            context
                .commands
                .insert_resource(DialogRequest::confirmation(
                    crate::ui::support::i18n::tr(
                        context.runtime_settings.locale,
                        crate::ui::support::i18n::UiText::ConfirmExit,
                    ),
                    DialogAction::ExitGame,
                ));
        }
        Some(TitleAction::Load) => {
            context.settings.open = false;
            context.save_load.mode = Some(crate::ui::save_load::SaveLoadMode::Load);
        }
        Some(TitleAction::Options) => {
            context.save_load.mode = None;
            context.settings.open = true;
        }
        Some(TitleAction::Extra) => {
            context.save_load.mode = None;
            context.settings.open = false;
            context.extra.open = true;
        }
        Some(TitleAction::Start) => {
            let outcome = start_game(&mut context.state, &mut context.checkpoint);
            handle_script_outcome(&mut context.commands, outcome);
        }
        _ => {}
    }
}

fn title_keyboard_action(
    keys: &ButtonInput<KeyCode>,
    actions: &InputActions,
    can_continue: bool,
) -> Option<TitleAction> {
    if keys.just_pressed(KeyCode::Escape) {
        return Some(TitleAction::Exit);
    }
    match actions.shortcut {
        Some(crate::ui::control_bar::ButtonAction::QuickLoad) if can_continue => {
            Some(TitleAction::Continue)
        }
        Some(crate::ui::control_bar::ButtonAction::Load) => Some(TitleAction::Load),
        Some(crate::ui::control_bar::ButtonAction::System) => Some(TitleAction::Options),
        _ => None,
    }
}

pub fn animate_title_buttons(
    time: Res<Time>,
    mut buttons: TitleButtonAnimationQuery,
    mut button_visuals: TitleButtonVisualQuery,
    mut previews: Query<
        (
            &mut TitleContinuePreview,
            &mut Node,
            &mut BackgroundColor,
            &mut BlurStrength,
        ),
        Without<TitleButtonMotion>,
    >,
    mut preview_texts: Query<(&TitleContinuePreviewAlpha, &mut TextColor)>,
    mut preview_images: Query<(&TitleContinuePreviewAlpha, &mut ImageNode), Without<TextColor>>,
) {
    let delta = time.delta_secs();
    let amount = exp_lerp(delta, 12.0);
    let mut continue_visible = false;
    for (interaction, mut motion, children, action) in &mut buttons {
        if matches!(action, Some(TitleAction::Continue)) {
            continue_visible = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        }
        if *interaction == Interaction::Pressed {
            motion.press = 1.0;
        } else {
            motion.press *= (-delta * 16.0).exp();
            if motion.press < 0.001 {
                motion.press = 0.0;
            }
        }
        let (target_width, target_padding) = match interaction {
            Interaction::None => (100.0, 22.5),
            Interaction::Hovered => (96.25, 22.5),
            Interaction::Pressed => (92.0, 20.25),
        };
        let target_hover = if *interaction == Interaction::None {
            0.0
        } else {
            SURFACE_HOVER_ALPHA
        };
        if (motion.width - target_width).abs() < 0.001
            && (motion.padding - target_padding).abs() < 0.001
            && motion.press == 0.0
            && (motion.hover - target_hover).abs() < 0.001
        {
            continue;
        }
        let visual = children
            .iter()
            .find(|child| button_visuals.contains(*child));
        let Some(visual) = visual else {
            continue;
        };
        let Ok((mut node, mut transform, mut background)) = button_visuals.get_mut(visual) else {
            continue;
        };
        motion.width += (target_width - motion.width) * amount;
        motion.padding += (target_padding - motion.padding) * amount;
        motion.hover += (target_hover - motion.hover) * amount;
        let hit_width = if matches!(action, Some(TitleAction::Continue)) {
            100.0 + CONTINUE_PREVIEW_GAP_PERCENT
        } else {
            100.0
        };
        node.width = Val::Percent(motion.width / hit_width * 100.0);
        node.padding.left = Val::Px(motion.padding);
        transform.scale = Vec2::splat(1.0 - 0.045 * motion.press);
        background.0 = button_surface(motion.hover);
    }

    let Ok((mut preview, mut node, mut background, mut blur)) = previews.single_mut() else {
        return;
    };
    preview.target = f32::from(continue_visible);
    if preview.target > 0.0 {
        node.display = Display::Flex;
    }

    let rate = if preview.target > preview.progress {
        15.0
    } else {
        18.0
    };
    preview.progress += (preview.target - preview.progress) * exp_lerp(delta, rate);
    if (preview.progress - preview.target).abs() <= 0.001 {
        preview.progress = preview.target;
    }

    let opacity = smoothstep(preview.progress);
    background.0 = Color::srgba(0.0, 0.0, 0.0, CONTINUE_PREVIEW_ALPHA * opacity);
    blur.0 = TITLE_GLASS_BLUR * opacity;
    for (base, mut color) in &mut preview_texts {
        color.0 = color.0.with_alpha(base.0 * opacity);
    }
    for (base, mut image) in &mut preview_images {
        image.color = image.color.with_alpha(base.0 * opacity);
    }

    if preview.progress == 0.0 && preview.target == 0.0 {
        node.display = Display::None;
    }
}

#[cfg(test)]
mod continue_preview_tests {
    use super::smoothstep;

    #[test]
    fn preview_curve_is_not_linear() {
        assert!(smoothstep(0.25) < 0.25);
        assert!(smoothstep(0.75) > 0.75);
    }
}

fn restore_continuation(
    saved: keine_loader::SavedState,
    current: &mut GameState,
) -> anyhow::Result<()> {
    // Restore into a candidate first. A script change can reconcile a formerly
    // valid save to an ended state; the live title state must remain intact if
    // that happens.
    let mut candidate = current.0.clone();
    saved.restore_into(&mut candidate)?;
    anyhow::ensure!(
        !candidate.ended,
        "quick save no longer points to a playable scene"
    );
    current.0 = candidate;
    Ok(())
}

fn start_game(
    state: &mut GameState,
    checkpoint: &mut crate::storage::save::ContinuationCheckpoint,
) -> crate::runtime::script_driver::ScriptOutcome {
    state.ended = false;
    state.backlog.clear();
    state.current_scene = crate::scene::entry_scene(state);
    state.cursor = 0;
    checkpoint.reset(state);
    crate::runtime::script_driver::resume(state, checkpoint)
}

fn start_game_from_keyboard(
    keys: &mut ButtonInput<KeyCode>,
    actions: &mut InputActions,
    state: &mut GameState,
    checkpoint: &mut crate::storage::save::ContinuationCheckpoint,
) -> Option<crate::runtime::script_driver::ScriptOutcome> {
    const START_KEYS: [KeyCode; 2] = [KeyCode::Enter, KeyCode::Space];
    if !keys.any_just_pressed(START_KEYS) {
        return None;
    }

    // `InputActions` was collected earlier in this frame. Clear both the raw
    // edge and its translated action so the key that leaves the title cannot
    // also advance the first stage presentation. The held state is preserved;
    // releasing and pressing again still creates a normal intro advance edge.
    for key in START_KEYS {
        keys.clear_just_pressed(key);
    }
    actions.advance = false;
    Some(start_game(state, checkpoint))
}

#[cfg(test)]
mod tests {
    use super::*;
    use keine_core::{Action, Program, State};
    use keine_loader::KeineStore;

    #[test]
    fn title_button_keeps_hover_surface_right_anchored() {
        fn spawn(mut commands: Commands) {
            let font = Handle::<Font>::default();
            commands.spawn_empty().with_children(|root| {
                spawn_title_button(root, "START", Some(TitleAction::Start), &font, None)
            });
        }
        let mut app = App::new();
        app.add_systems(Update, spawn);
        app.update();
        let world = app.world_mut();

        let button = {
            let mut buttons = world.query_filtered::<Entity, With<Button>>();
            buttons.single(world).expect("one title button")
        };
        assert_eq!(
            world
                .get::<BackgroundColor>(button)
                .expect("button background")
                .0,
            Color::NONE
        );

        let visual = world
            .get::<Children>(button)
            .expect("button children")
            .iter()
            .find(|child| world.get::<TitleButtonVisual>(*child).is_some())
            .expect("scaled title-button visual");
        let visual_node = world.get::<Node>(visual).expect("visual node");
        assert_eq!(visual_node.position_type, PositionType::Absolute);
        assert_eq!(visual_node.right, Val::ZERO);
        assert_eq!(
            world
                .get::<BackgroundColor>(visual)
                .expect("visual hover surface")
                .0,
            button_surface(0.0)
        );
    }

    #[test]
    fn continue_and_regular_title_buttons_share_one_blur_strength() {
        fn spawn(mut commands: Commands) {
            let font = Handle::<Font>::default();
            let preview = QuickSavePreview::default();
            commands.spawn_empty().with_children(|root| {
                spawn_title_button(root, "START", Some(TitleAction::Start), &font, None);
                spawn_title_button(
                    root,
                    "CONTINUE",
                    Some(TitleAction::Continue),
                    &font,
                    Some(&preview),
                );
            });
        }
        let mut app = App::new();
        app.add_systems(Update, spawn);
        app.update();
        let world = app.world_mut();

        let strengths = world
            .query_filtered::<&BlurStrength, With<TitleButtonVisual>>()
            .iter(world)
            .map(|strength| strength.0)
            .collect::<Vec<_>>();
        assert_eq!(strengths, vec![TITLE_GLASS_BLUR; 2]);
    }

    #[test]
    fn return_to_title_fully_covers_before_it_reveals() {
        assert_eq!(transition_alpha(ReturnToTitlePhase::Cover, 0.0), 0.0);
        assert_eq!(
            transition_alpha(ReturnToTitlePhase::Cover, RETURN_COVER_SECONDS),
            1.0
        );
        assert_eq!(transition_alpha(ReturnToTitlePhase::Reveal, 0.0), 1.0);
        assert_eq!(
            transition_alpha(ReturnToTitlePhase::Reveal, RETURN_REVEAL_SECONDS),
            0.0
        );
        assert!(
            transition_alpha(ReturnToTitlePhase::Cover, RETURN_COVER_SECONDS * 0.75)
                > transition_alpha(ReturnToTitlePhase::Cover, RETURN_COVER_SECONDS * 0.25)
        );
        assert!(
            transition_alpha(ReturnToTitlePhase::Reveal, RETURN_REVEAL_SECONDS * 0.75)
                < transition_alpha(ReturnToTitlePhase::Reveal, RETURN_REVEAL_SECONDS * 0.25)
        );
    }

    #[test]
    fn keyboard_start_consumes_only_the_launch_edge_before_a_held_intro() {
        let mut state = GameState(State::new());
        state.install_program(Program::from_scenes([(
            "start".into(),
            vec![Action::Intro {
                pages: vec!["first".into(), "second".into()],
                hold: true,
            }],
        )]));
        state.ended = true;

        let mut keys = ButtonInput::default();
        keys.press(KeyCode::Enter);
        let mut actions = InputActions {
            advance: true,
            ..Default::default()
        };

        assert!(
            start_game_from_keyboard(
                &mut keys,
                &mut actions,
                &mut state,
                &mut crate::storage::save::ContinuationCheckpoint::default(),
            )
            .is_some()
        );
        assert_eq!(state.intro.as_ref().unwrap().page, 0);
        assert!(state.intro.as_ref().unwrap().hold);
        assert!(!state.ended);
        assert!(!actions.advance);
        assert!(!keys.just_pressed(KeyCode::Enter));
        assert!(keys.pressed(KeyCode::Enter));

        actions.advance = keys.any_just_pressed([KeyCode::Space, KeyCode::Enter]);
        assert!(
            !actions.advance,
            "collecting input again must not replay the title launch edge"
        );

        keys.release(KeyCode::Enter);
        keys.clear();
        keys.press(KeyCode::Enter);
        actions.advance = keys.just_pressed(KeyCode::Enter);

        assert!(
            actions.advance,
            "a later Enter press must advance the intro"
        );
        assert_eq!(state.intro.as_ref().unwrap().page, 0);
    }

    #[test]
    fn escape_requests_the_existing_title_exit_action() {
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::Escape);

        assert!(matches!(
            title_keyboard_action(&keys, &InputActions::default(), false),
            Some(TitleAction::Exit)
        ));
    }

    #[test]
    fn unavailable_continue_has_no_button_or_action() {
        fn spawn(mut commands: Commands) {
            let font = Handle::<Font>::default();
            commands.spawn_empty().with_children(|root| {
                spawn_disabled_button(root, "CONTINUE", &font);
            });
        }
        let mut app = App::new();
        app.add_systems(Update, spawn);
        app.update();
        let world = app.world_mut();

        assert_eq!(
            world
                .query_filtered::<Entity, With<Button>>()
                .iter(world)
                .count(),
            0
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<TitleAction>>()
                .iter(world)
                .count(),
            0
        );
        assert_eq!(
            world
                .query_filtered::<&BackgroundColor, Without<Text>>()
                .single(world)
                .expect("disabled surface")
                .0,
            dark_surface(DISABLED_BUTTON_SURFACE_ALPHA)
        );
        assert_eq!(
            world
                .query_filtered::<&TextColor, With<Text>>()
                .single(world)
                .expect("disabled label")
                .0,
            Color::srgba(1.0, 1.0, 1.0, DISABLED_BUTTON_TEXT_ALPHA)
        );
    }

    #[test]
    fn quick_load_shortcut_cannot_bypass_unavailable_continue() {
        let actions = InputActions {
            shortcut: Some(crate::ui::control_bar::ButtonAction::QuickLoad),
            ..Default::default()
        };

        assert!(title_keyboard_action(&ButtonInput::default(), &actions, false).is_none());
        assert!(matches!(
            title_keyboard_action(&ButtonInput::default(), &actions, true),
            Some(TitleAction::Continue)
        ));
    }

    #[test]
    fn extra_title_action_only_exists_after_project_opt_in() {
        fn spawn(mut commands: Commands) {
            let font = Handle::<Font>::default();
            commands.spawn_empty().with_children(|disabled| {
                spawn_extra_button(disabled, &font, false);
            });
            commands.spawn_empty().with_children(|enabled| {
                spawn_extra_button(enabled, &font, true);
            });
        }
        let mut app = App::new();
        app.add_systems(Update, spawn);
        app.update();
        let world = app.world_mut();

        let actions = world
            .query_filtered::<&TitleAction, With<Button>>()
            .iter(world)
            .filter(|action| matches!(action, TitleAction::Extra))
            .count();
        assert_eq!(actions, 1);
    }

    #[test]
    fn continue_restores_the_saved_scene_cursor_and_dialogue() {
        let program = Program::from_scenes([(
            "start".into(),
            vec![
                Action::Say {
                    speaker: "小夜".into(),
                    text: "第一句".into(),
                    options: Default::default(),
                },
                Action::Say {
                    speaker: "小夜".into(),
                    text: "继续的位置".into(),
                    options: Default::default(),
                },
            ],
        )]);
        let mut saved = State::new();
        saved.install_program(program.clone());
        saved.current_scene = "start".into();
        saved.ended = false;
        keine_core::step::step(&mut saved);
        keine_core::step::step(&mut saved);

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("keine-continue-{}-{nonce}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::storage::save::save_game(
            &KeineStore,
            &saved,
            crate::storage::save::QUICK_SAVE_SLOT,
            &root,
        )
        .unwrap();

        let loaded = crate::storage::save::load_game(
            &KeineStore,
            crate::storage::save::QUICK_SAVE_SLOT,
            &root,
        )
        .unwrap();
        let mut current = GameState(State::new());
        current.install_program(program);
        current.ended = true;

        restore_continuation(loaded, &mut current).unwrap();

        assert!(!current.ended);
        assert_eq!(current.current_scene, saved.current_scene);
        assert_eq!(current.cursor, saved.cursor);
        assert_eq!(current.dialogue, saved.dialogue);
        let _ = std::fs::remove_dir_all(root);
    }
}
