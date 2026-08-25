// GlobalDialog — WebGAL-style confirmation overlay with title + two buttons.
use std::path::{Path, PathBuf};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::thread;

use crate::render::blur::DialogCamera;
use crate::render::blur::UiBlurCamera;
use crate::storage::save::QUICK_SAVE_SLOT;
use crate::storage::settings::RuntimeSettings;
use crate::ui::backlog::BacklogRoot;
use crate::ui::control_bar::QuickSavePreview;
use crate::ui::foundation::{HoverSweep, UiFonts, UiSoundStyle, hover_sweep_fill};
use crate::ui::save_load::SaveLoadRoot;
use crate::ui::settings_panel::SettingsRoot;
use crate::ui::support::i18n::{UiText, tr};
use bevy::camera::{
    OrthographicProjection, Projection, RenderTarget, ScalingMode, visibility::RenderLayers,
};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::ui::FocusPolicy;
use bevy::window::{PrimaryWindow, WindowCloseRequested};

const FADE_DURATION: f32 = 0.2;
const OVERLAY_ALPHA: f32 = 0.16;
const PANEL_ALPHA: f32 = 0.78;
const SAVE_PREVIEW_LIMIT: UVec2 = UVec2::new(480, 270);

/// Which action to perform when the user confirms.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DialogAction {
    QuickSave,
    QuickLoad,
    SaveSlot(u32),
    LoadSlot(u32),
    DeleteSlot(u32),
    ClearSaves,
    ResetSettings,
    ClearAll,
    BackToTitle,
    Noop,
    SystemMessage,
    ExitGame,
}

/// Active dialog request. When set, the overlay + dialog UI is shown.
#[derive(Resource, Clone)]
pub(crate) struct DialogRequest {
    pub title: String,
    pub message: String,
    pub confirm_text: Option<String>,
    pub cancel_text: Option<String>,
    pub action: DialogAction,
}

impl DialogRequest {
    pub fn confirmation(title: impl Into<String>, action: DialogAction) -> Self {
        Self {
            title: title.into(),
            message: String::new(),
            confirm_text: None,
            cancel_text: None,
            action,
        }
    }

    fn system_message(message: &keine_core::state::SystemMessageState) -> Self {
        Self {
            title: message.title.clone(),
            message: message.message.clone(),
            confirm_text: Some(message.confirm_text.clone()),
            cancel_text: (message.mode == keine_core::SystemMessageMode::Confirm)
                .then(|| message.cancel_text.clone()),
            action: DialogAction::SystemMessage,
        }
    }
}

pub fn sync_system_message(
    mut commands: Commands,
    state: Res<crate::runtime::resources::GameState>,
    request: Option<Res<DialogRequest>>,
) {
    if request.is_none()
        && let Some(message) = &state.system_message
    {
        commands.insert_resource(DialogRequest::system_message(message));
    }
}

#[derive(Component)]
pub(crate) struct DialogRoot;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogButton {
    Confirm,
    Cancel,
}

#[derive(Component)]
pub(crate) struct DialogFade(f32);

impl DialogFade {
    pub(crate) fn progress(&self) -> f32 {
        self.0
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.0 < 0.999
    }
}

#[derive(Component)]
pub(crate) struct DialogBackground {
    alpha: f32,
}

#[derive(Component)]
pub(crate) struct DialogBorder {
    alpha: f32,
}

#[derive(Component)]
pub(crate) struct DialogText {
    alpha: f32,
}

type ModalBackdropQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut UiTargetCamera, &'static mut RenderLayers),
    Or<(With<BacklogRoot>, With<SaveLoadRoot>, With<SettingsRoot>)>,
>;

#[derive(Component)]
struct SavePreviewCapture {
    camera: Entity,
    slot: u32,
    generation: crate::storage::save::SavePreviewGeneration,
}

struct SavePreviewJob {
    image: Image,
    path: PathBuf,
    slot: u32,
    generation: crate::storage::save::SavePreviewGeneration,
    coordinator: crate::storage::save::SavePreviewCoordinator,
}

#[derive(Resource)]
pub(crate) struct SavePreviewWriter {
    sender: Option<SyncSender<SavePreviewJob>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Default for SavePreviewWriter {
    fn default() -> Self {
        // Keep at most two final-size render targets waiting behind the image
        // currently being encoded. Rapid repeated saves never grow memory
        // without bound or stall the render thread on image compression.
        let (sender, receiver) = sync_channel::<SavePreviewJob>(2);
        let worker = thread::Builder::new()
            .name("keine-save-preview".into())
            .spawn(move || {
                for job in receiver {
                    write_save_preview(job);
                }
            })
            .map_err(|error| log::error!("failed to start save preview writer: {error}"))
            .ok();
        Self {
            sender: Some(sender),
            worker,
        }
    }
}

impl SavePreviewWriter {
    fn enqueue(
        &self,
        image: Image,
        path: PathBuf,
        slot: u32,
        generation: crate::storage::save::SavePreviewGeneration,
        coordinator: &crate::storage::save::SavePreviewCoordinator,
    ) {
        let Some(sender) = &self.sender else {
            return;
        };
        match sender.try_send(SavePreviewJob {
            image,
            path,
            slot,
            generation,
            coordinator: coordinator.clone(),
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(job)) => {
                log::warn!("save preview queue is full; skipped {}", job.path.display())
            }
            Err(TrySendError::Disconnected(job)) => log::error!(
                "save preview writer stopped before writing {}",
                job.path.display()
            ),
        }
    }
}

impl Drop for SavePreviewWriter {
    fn drop(&mut self) {
        drop(self.sender.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn write_save_preview(job: SavePreviewJob) {
    let result = job
        .image
        .data
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("captured preview has no CPU pixel data"))
        .and_then(|rgba| {
            crate::scene::images::encode_preview(rgba, job.image.width(), job.image.height())
                .map_err(anyhow::Error::from)
        })
        .and_then(|bytes| {
            job.coordinator
                .commit_if_current(job.slot, job.generation, || {
                    crate::storage::write_atomically(&job.path, &bytes)
                })
                .map(|_| ())
        });
    if let Err(error) = result {
        log::error!("failed to save slot preview: {error:#}");
    }
}

#[derive(SystemParam)]
pub(crate) struct QuickSaveContext<'w, 's> {
    state: ResMut<'w, crate::runtime::resources::GameState>,
    checkpoint: ResMut<'w, crate::storage::save::ContinuationCheckpoint>,
    project_root: Res<'w, crate::runtime::resources::PersistenceRoot>,
    store: Res<'w, crate::runtime::resources::StoreCodec>,
    preview: ResMut<'w, QuickSavePreview>,
    save_previews: ResMut<'w, crate::ui::save_load::SavePreviewCache>,
    images: ResMut<'w, Assets<Image>>,
    windows: Query<'w, 's, &'static Window>,
    primary_window: Query<'w, 's, Entity, With<PrimaryWindow>>,
    save_load: ResMut<'w, crate::ui::save_load::SaveLoadUi>,
    settings_ui: ResMut<'w, crate::ui::settings_panel::SettingsUi>,
    backlog_ui: ResMut<'w, crate::ui::backlog::BacklogUiState>,
    settings: ResMut<'w, crate::storage::settings::RuntimeSettings>,
    toggles: ResMut<'w, crate::ui::control_bar::ToggleStates>,
    pending_window: ResMut<'w, crate::ui::settings_panel::PendingWindowMode>,
    profile_writer: ResMut<'w, crate::storage::profile::ProfileWriter>,
    read_history_writer: ResMut<'w, crate::storage::read_history::ReadHistoryWriter>,
    gallery_snapshot: ResMut<'w, crate::storage::gallery::GallerySnapshot>,
    preview_coordinator: Res<'w, crate::storage::save::SavePreviewCoordinator>,
    editor_sync: Option<Res<'w, crate::runtime::resources::EditorSyncSession>>,
}

#[derive(SystemParam)]
struct SavePreviewContext<'w, 's> {
    targets: Query<'w, 's, &'static SavePreviewCapture>,
    commands: Commands<'w, 's>,
    images: ResMut<'w, Assets<Image>>,
    preview: ResMut<'w, QuickSavePreview>,
    save_previews: ResMut<'w, crate::ui::save_load::SavePreviewCache>,
    save_load: ResMut<'w, crate::ui::save_load::SaveLoadUi>,
    project_root: Res<'w, crate::runtime::resources::PersistenceRoot>,
    writer: Res<'w, SavePreviewWriter>,
    coordinator: Res<'w, crate::storage::save::SavePreviewCoordinator>,
}

/// Spawn the dialog overlay + centred box when DialogRequest is present.
pub fn spawn_dialog(
    mut commands: Commands,
    dialog_q: Query<Entity, With<DialogRoot>>,
    request: Option<Res<DialogRequest>>,
    fonts: Res<UiFonts>,
    settings: Res<RuntimeSettings>,
    dialog_camera_q: Query<Entity, With<DialogCamera>>,
) {
    // Remove existing dialog when request is gone
    if request
        .as_ref()
        .is_some_and(|request| !request.is_changed())
        && !dialog_q.is_empty()
    {
        return;
    }

    // Clear old dialog
    for e in dialog_q.iter() {
        commands.entity(e).despawn();
    }

    let Some(req) = request else { return };
    let Ok(dialog_camera) = dialog_camera_q.single() else {
        return;
    };

    let font = fonts.text.clone();

    commands
        .spawn((
            Name::new("dialog_overlay"),
            DialogRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
            DialogFade(0.0),
            DialogBackground {
                alpha: OVERLAY_ALPHA,
            },
            FocusPolicy::Block,
            GlobalZIndex(200),
            UiTargetCamera(dialog_camera),
            RenderLayers::layer(2),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(20.0),
                    border: UiRect::top(Val::Px(11.25)),
                    ..default()
                },
                BorderColor::all(Color::NONE),
                DialogBorder { alpha: 0.19 },
            ))
            .with_child((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(60.0), Val::Px(24.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                DialogBackground { alpha: PANEL_ALPHA },
                children![
                    // Title
                    dialog_text(
                        if req.message.is_empty() {
                            req.title.clone()
                        } else {
                            format!("{}\n{}", req.title, req.message)
                        },
                        font.clone(),
                        48.0,
                        0.9,
                    ),
                    // Button row — wide spacing
                    (
                        Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(60.0),
                            ..default()
                        },
                        children![
                            spawn_dialog_button(
                                DialogButton::Confirm,
                                req.confirm_text.clone().unwrap_or_else(|| tr(
                                    settings.locale,
                                    UiText::Confirm
                                )
                                .into()),
                                font.clone(),
                                true,
                            ),
                            spawn_dialog_button(
                                DialogButton::Cancel,
                                req.cancel_text.clone().unwrap_or_else(|| tr(
                                    settings.locale,
                                    UiText::Cancel
                                )
                                .into()),
                                font,
                                req.cancel_text.is_some() || req.confirm_text.is_none(),
                            ),
                        ],
                    ),
                ],
            ));
        });
}

/// Full-screen menus normally render after their own backdrop blur. When a
/// confirmation dialog opens, temporarily render those menus on the UI camera
/// so the dialog's full-screen post-process also blurs the menu beneath it.
pub fn sync_modal_backdrop_layer(
    request: Option<Res<DialogRequest>>,
    ui_camera: Query<Entity, With<UiBlurCamera>>,
    dialog_camera: Query<Entity, (With<DialogCamera>, Without<UiBlurCamera>)>,
    mut roots: ModalBackdropQuery,
) {
    let target = if request.is_some() {
        ui_camera.single().ok().map(|entity| (entity, 1))
    } else {
        dialog_camera.single().ok().map(|entity| (entity, 2))
    };
    let Some((target, layer)) = target else {
        return;
    };
    for (mut current, mut layers) in &mut roots {
        if current.0 != target {
            *current = UiTargetCamera(target);
            *layers = RenderLayers::layer(layer);
        }
    }
}

fn spawn_dialog_button(
    action: DialogButton,
    text: impl Into<String>,
    font: Handle<Font>,
    visible: bool,
) -> impl Bundle {
    (
        Button,
        UiSoundStyle::Click,
        action,
        HoverSweep::default(),
        Node {
            display: if visible {
                Display::Flex
            } else {
                Display::None
            },
            min_width: Val::Px(112.5),
            padding: UiRect::axes(Val::Px(24.0), Val::Px(6.0)),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![hover_sweep_fill(), dialog_text(text, font, 31.5, 0.67)],
    )
}

fn dialog_text(
    content: impl Into<String>,
    font: Handle<Font>,
    size: f32,
    alpha: f32,
) -> impl Bundle {
    (
        Text::new(content.into()),
        TextFont {
            font: font.into(),
            font_size: FontSize::from(size),
            ..default()
        },
        TextColor(Color::NONE),
        DialogText { alpha },
    )
}

pub fn animate_dialog(
    time: Res<Time>,
    mut fade_query: Query<&mut DialogFade>,
    mut backgrounds: Query<(&DialogBackground, &mut BackgroundColor)>,
    mut borders: Query<(&DialogBorder, &mut BorderColor)>,
    mut texts: Query<(&DialogText, &mut TextColor)>,
) {
    let Ok(mut fade) = fade_query.single_mut() else {
        return;
    };
    if fade.0 >= 1.0 {
        return;
    }
    fade.0 = (fade.0 + time.delta_secs() / FADE_DURATION).min(1.0);

    for (visual, mut color) in &mut backgrounds {
        color.0 = Color::srgba(0.0, 0.0, 0.0, visual.alpha * fade.0);
    }
    for (visual, mut color) in &mut borders {
        *color = BorderColor::all(Color::srgba(0.0, 0.0, 0.0, visual.alpha * fade.0));
    }
    for (visual, mut color) in &mut texts {
        color.0 = Color::srgba(1.0, 1.0, 1.0, visual.alpha * fade.0);
    }
}

/// Handle dialog button clicks: execute the action and remove the request.
pub fn handle_dialog_click(
    mut commands: Commands,
    buttons: Query<(&Interaction, &DialogButton), Changed<Interaction>>,
    request: Option<Res<DialogRequest>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut context: QuickSaveContext,
) {
    let left_clicked = buttons.iter().any(|(interaction, button)| {
        matches!(interaction, Interaction::Pressed) && *button == DialogButton::Confirm
    }) || keys.just_pressed(KeyCode::Enter);
    let right_clicked = buttons.iter().any(|(interaction, button)| {
        matches!(interaction, Interaction::Pressed) && *button == DialogButton::Cancel
    }) || keys.just_pressed(KeyCode::Escape)
        || mouse.just_pressed(MouseButton::Right);

    if !left_clicked && !right_clicked {
        return;
    }
    let Some(req) = request else { return };
    commands.remove_resource::<DialogRequest>();

    if left_clicked {
        if context.editor_sync.is_some()
            && !matches!(
                req.action,
                DialogAction::Noop | DialogAction::SystemMessage | DialogAction::ExitGame
            )
        {
            log::debug!("ignored persistent UI action during Studio synchronization");
            return;
        }
        match &req.action {
            DialogAction::QuickSave => {
                if let Err(error) = crate::storage::save::save_game(
                    context.store.0.as_ref(),
                    &context.state,
                    QUICK_SAVE_SLOT,
                    &context.project_root,
                ) {
                    log::error!("quick save failed: {error:#}");
                    if !context.state.persistence_safety().is_exact() {
                        commands.insert_resource(DialogRequest::confirmation(
                            tr(
                                context.settings.locale,
                                UiText::SaveUnavailableDuringPresentation,
                            ),
                            DialogAction::Noop,
                        ));
                    }
                } else {
                    let generation = replace_preview_generation(
                        &context.preview_coordinator,
                        &context.project_root,
                        QUICK_SAVE_SLOT,
                    );
                    context.preview.state = Some(crate::ui::control_bar::QuickSaveSnapshot::from(
                        &**context.state,
                    ));
                    context.preview.image = None;
                    if let Ok(window) = context.windows.single() {
                        let size = Vec2::new(window.width(), window.height());
                        capture_save_preview(
                            &mut commands,
                            &mut context.images,
                            size,
                            QUICK_SAVE_SLOT,
                            generation,
                        );
                    }
                }
            }
            DialogAction::QuickLoad => {
                match crate::storage::save::load_game(
                    context.store.0.as_ref(),
                    QUICK_SAVE_SLOT,
                    &context.project_root,
                ) {
                    Ok(loaded) => match loaded.restore_into(&mut context.state) {
                        Ok(()) => context.checkpoint.reset(&context.state),
                        Err(error) => {
                            log::error!("quick load rejected: {error}");
                            commands.insert_resource(DialogRequest::confirmation(
                                tr(context.settings.locale, UiText::ForeignSave),
                                DialogAction::Noop,
                            ));
                        }
                    },
                    Err(error) => log::error!("quick load failed: {error:#}"),
                }
            }
            DialogAction::BackToTitle => {
                let continuation = context
                    .checkpoint
                    .state_for_continuation(&context.state)
                    .cloned();
                match crate::storage::save::save_continuation(
                    context.store.0.as_ref(),
                    &context.state,
                    &context.checkpoint,
                    &context.project_root,
                ) {
                    Ok(crate::storage::save::ContinuationSave::Skipped) => {
                        log::warn!(
                            "kept the previous continuation because no exact checkpoint exists"
                        )
                    }
                    Ok(_) => {
                        context.preview_coordinator.invalidate_slot(QUICK_SAVE_SLOT);
                        if let Err(error) = crate::storage::save::remove_preview(
                            &context.project_root,
                            QUICK_SAVE_SLOT,
                        ) {
                            log::warn!("failed to invalidate stale quick-save preview: {error:#}");
                        }
                        context.preview.state = continuation
                            .as_ref()
                            .map(crate::ui::control_bar::QuickSaveSnapshot::from);
                        context.preview.image = None;
                    }
                    Err(error) => {
                        log::error!(
                            "failed to save continuation before returning to title: {error:#}"
                        )
                    }
                }
                commands.insert_resource(crate::ui::title::ReturnToTitleTransition::default());
                context.save_load.mode = None;
                context.settings_ui.open = false;
                context.backlog_ui.open = false;
            }
            DialogAction::SaveSlot(slot) => {
                if let Err(error) = crate::storage::save::save_game(
                    context.store.0.as_ref(),
                    &context.state,
                    *slot,
                    &context.project_root,
                ) {
                    log::error!("save slot {slot} failed: {error:#}");
                    if !context.state.persistence_safety().is_exact() {
                        commands.insert_resource(DialogRequest::confirmation(
                            tr(
                                context.settings.locale,
                                UiText::SaveUnavailableDuringPresentation,
                            ),
                            DialogAction::Noop,
                        ));
                    }
                } else {
                    let generation = replace_preview_generation(
                        &context.preview_coordinator,
                        &context.project_root,
                        *slot,
                    );
                    context.save_previews.invalidate(*slot);
                    context.save_load.set_changed();
                    if let Ok(window) = context.windows.single() {
                        let size = Vec2::new(window.width(), window.height());
                        capture_save_preview(
                            &mut commands,
                            &mut context.images,
                            size,
                            *slot,
                            generation,
                        );
                    }
                }
            }
            DialogAction::LoadSlot(slot) => {
                match crate::storage::save::load_game(
                    context.store.0.as_ref(),
                    *slot,
                    &context.project_root,
                ) {
                    Ok(loaded) => match loaded.restore_into(&mut context.state) {
                        Ok(()) => {
                            context.checkpoint.reset(&context.state);
                            context.save_load.mode = None;
                        }
                        Err(error) => {
                            log::error!("load slot {slot} rejected: {error}");
                            commands.insert_resource(DialogRequest::confirmation(
                                tr(context.settings.locale, UiText::ForeignSave),
                                DialogAction::Noop,
                            ));
                        }
                    },
                    Err(error) => log::error!("load slot {slot} failed: {error:#}"),
                }
            }
            DialogAction::DeleteSlot(slot) => {
                context.preview_coordinator.invalidate_slot(*slot);
                context.save_previews.invalidate(*slot);
                match crate::storage::save::delete_game(
                    context.store.0.as_ref(),
                    *slot,
                    &context.project_root,
                ) {
                    Ok(()) => context.save_load.set_changed(),
                    Err(error) => log::error!("delete slot {slot} failed: {error:#}"),
                }
            }
            DialogAction::ClearSaves => {
                context.preview_coordinator.invalidate_all();
                if let Err(error) = crate::storage::save::clear_games(
                    context.store.0.as_ref(),
                    &context.project_root,
                ) {
                    log::error!("failed to clear save slots: {error:#}");
                } else {
                    context.preview.state = None;
                    context.preview.image = None;
                    context.save_previews.clear();
                    context.save_load.set_changed();
                }
            }
            DialogAction::ResetSettings => {
                crate::ui::settings_panel::reset_runtime_settings(
                    &mut context.settings,
                    &mut context.toggles,
                    &mut context.pending_window,
                    &context.project_root,
                );
            }
            DialogAction::ClearAll => {
                context.preview_coordinator.invalidate_all();
                crate::ui::settings_panel::reset_runtime_settings(
                    &mut context.settings,
                    &mut context.toggles,
                    &mut context.pending_window,
                    &context.project_root,
                );
                if let Err(error) = crate::storage::reset_all(
                    &context.project_root,
                    &mut context.state,
                    &mut context.settings,
                    &mut context.profile_writer,
                    &mut context.read_history_writer,
                    &mut context.gallery_snapshot,
                ) {
                    log::error!("failed to clear all persistent data: {error:#}");
                }
                context.preview.state = None;
                context.preview.image = None;
                context.save_previews.clear();
                context.save_load.set_changed();
            }
            DialogAction::Noop => {}
            DialogAction::SystemMessage => {
                if keine_core::step::resolve_system_message(&mut context.state, true) {
                    let outcome = crate::runtime::script_driver::resume(
                        &mut context.state,
                        &mut context.checkpoint,
                    );
                    crate::ui::title::handle_script_outcome(&mut commands, outcome);
                }
            }
            DialogAction::ExitGame => {
                if let Ok(window) = context.primary_window.single() {
                    commands.write_message(WindowCloseRequested { window });
                } else {
                    log::warn!("primary window unavailable; exiting directly");
                    commands.write_message(bevy::app::AppExit::Success);
                }
            }
        }
    }
    if right_clicked
        && matches!(req.action, DialogAction::SystemMessage)
        && keine_core::step::resolve_system_message(&mut context.state, false)
    {
        let outcome =
            crate::runtime::script_driver::resume(&mut context.state, &mut context.checkpoint);
        crate::ui::title::handle_script_outcome(&mut commands, outcome);
    }
}

pub(crate) fn replace_preview_generation(
    coordinator: &crate::storage::save::SavePreviewCoordinator,
    project_root: &Path,
    slot: u32,
) -> crate::storage::save::SavePreviewGeneration {
    let generation = coordinator.invalidate_slot(slot);
    if let Err(error) = crate::storage::save::remove_preview(project_root, slot) {
        log::warn!("failed to remove stale slot preview: {error:#}");
    }
    generation
}

pub(crate) fn capture_save_preview(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    size: Vec2,
    slot: u32,
    generation: crate::storage::save::SavePreviewGeneration,
) {
    let extent = preview_extent(size);
    let target = images.add(Image::new_target_texture(
        extent.x,
        extent.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let camera = commands
        .spawn((
            Name::new("save_preview_camera"),
            Camera2d,
            Camera { ..default() },
            Projection::Orthographic(OrthographicProjection {
                // The small render target must retain the main scene camera's
                // logical viewport. WindowSize would instead zoom the scene to
                // 480x270 world units.
                scaling_mode: ScalingMode::Fixed {
                    width: size.x.max(1.0),
                    height: size.y.max(1.0),
                },
                ..OrthographicProjection::default_2d()
            }),
            RenderTarget::Image(target.clone().into()),
            RenderLayers::layer(0),
        ))
        .id();
    commands
        .spawn((
            Screenshot::image(target),
            SavePreviewCapture {
                camera,
                slot,
                generation,
            },
        ))
        .observe(store_save_preview);
}

fn preview_extent(viewport: Vec2) -> UVec2 {
    let viewport = viewport.max(Vec2::ONE);
    let scale = (SAVE_PREVIEW_LIMIT.x as f32 / viewport.x)
        .min(SAVE_PREVIEW_LIMIT.y as f32 / viewport.y)
        .min(1.0);
    UVec2::new(
        (viewport.x * scale).round().max(1.0) as u32,
        (viewport.y * scale).round().max(1.0) as u32,
    )
}

fn store_save_preview(capture: On<ScreenshotCaptured>, mut context: SavePreviewContext) {
    let Ok(target) = context.targets.get(capture.entity) else {
        return;
    };
    context.commands.entity(target.camera).despawn();
    if !context
        .coordinator
        .is_current(target.slot, target.generation)
    {
        return;
    }
    let mut display_image = capture.image.clone();
    display_image.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
    let captured = context.images.add(display_image);
    if target.slot == QUICK_SAVE_SLOT {
        context.preview.image = Some(captured);
    } else {
        context.save_previews.insert_live(target.slot, captured);
        context.save_load.set_changed();
    }
    let path = crate::storage::save::preview_path(&context.project_root, target.slot);
    context.writer.enqueue(
        capture.image.clone(),
        path,
        target.slot,
        target.generation,
        &context.coordinator,
    );
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn preview_extent_caps_pixels_without_changing_aspect_ratio() {
        assert_eq!(
            preview_extent(Vec2::new(1920.0, 1080.0)),
            UVec2::new(480, 270)
        );
        assert_eq!(
            preview_extent(Vec2::new(1920.0, 1200.0)),
            UVec2::new(432, 270)
        );
        assert_eq!(
            preview_extent(Vec2::new(320.0, 180.0)),
            UVec2::new(320, 180)
        );
    }
}
