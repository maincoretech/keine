use std::collections::HashSet;
use std::io;
use std::path::Path;

use bevy::camera::RenderTarget;
use bevy::input::InputSystems;
use bevy::math::DVec2;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::ui::UiSystems;
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy::winit::WinitSettings;
use keine_authoring::{PreviewInput, SharedFrameProducer};

use super::platform::{InputActions, RuntimeActivity};
use super::resources::{GameState, LocalAssetManifest};

const PREVIEW_WIDTH: u32 = keine_core::DESIGN_WIDTH as u32;
const PREVIEW_HEIGHT: u32 = keine_core::DESIGN_HEIGHT as u32;
// A preview is latest-frame-wins. Queuing readbacks cannot improve what the
// editor displays, but it can make the render thread and audio mixer fight
// several old 1080p captures at once.
const MAX_IN_FLIGHT_CAPTURES: usize = 1;

pub(crate) struct AuthoringPreviewConfig {
    pub(crate) producer: SharedFrameProducer,
    pub(crate) document_revision: u64,
}

#[derive(Resource)]
struct PreviewFramePublisher(SharedFrameProducer);

#[derive(Resource)]
struct PreviewDocumentRevision(u64);

#[derive(Resource, Default)]
struct PreviewInputQueue(Vec<PreviewInput>);

#[derive(Resource, Default)]
struct PreviewDirectInputQueue(Vec<PreviewInput>);

#[derive(Resource, Default)]
struct PreviewPointerState {
    pressed_last_frame: bool,
}

#[derive(Resource)]
struct PreviewTarget(Handle<Image>);

#[derive(Resource, Default)]
struct CaptureState {
    requested: HashSet<Entity>,
    force: bool,
}

#[derive(Component)]
struct PreviewPausedAudio;

#[derive(Resource, Default)]
struct PreviewVisibility {
    paused: bool,
}

pub(crate) struct AuthoringPreviewPlugin {
    config: std::sync::Mutex<Option<AuthoringPreviewConfig>>,
}

impl AuthoringPreviewPlugin {
    pub(crate) fn new(config: AuthoringPreviewConfig) -> Self {
        Self {
            config: std::sync::Mutex::new(Some(config)),
        }
    }
}

impl Plugin for AuthoringPreviewPlugin {
    fn build(&self, app: &mut App) {
        let config = self
            .config
            .lock()
            .expect("authoring preview configuration lock poisoned")
            .take()
            .expect("authoring preview plugin may only be built once");
        app.insert_resource(PreviewFramePublisher(config.producer))
            .insert_resource(PreviewDocumentRevision(config.document_revision))
            .init_resource::<PreviewInputQueue>()
            .init_resource::<PreviewDirectInputQueue>()
            .init_resource::<PreviewPointerState>()
            .init_resource::<PreviewVisibility>()
            .insert_resource(CaptureState {
                requested: HashSet::new(),
                force: true,
            })
            // Runtime lifecycle code normally receives this from WinitPlugin.
            // Authoring preview has a virtual Window but no OS event loop.
            .insert_resource(WinitSettings::default())
            .add_systems(PreStartup, spawn_virtual_window)
            .add_systems(PostStartup, setup_target)
            .add_systems(
                PreUpdate,
                inject_preview_input
                    .after(InputSystems)
                    .before(UiSystems::Focus),
            )
            .add_systems(
                PreUpdate,
                apply_input
                    .after(UiSystems::Focus)
                    .after(super::platform::collect_input),
            )
            .add_systems(Update, (retarget_new_cameras, request_capture))
            .add_systems(Last, sync_paused_audio);
    }
}

fn spawn_virtual_window(mut commands: Commands) {
    let mut resolution = WindowResolution::new(PREVIEW_WIDTH, PREVIEW_HEIGHT);
    resolution.set_scale_factor_override(Some(1.0));
    commands.spawn((
        Window {
            title: "Kēne embedded preview".into(),
            resolution,
            visible: false,
            focused: true,
            ..default()
        },
        PrimaryWindow,
    ));
}

fn setup_target(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(Entity, &mut Camera)>,
) {
    let target = images.add(Image::new_target_texture(
        PREVIEW_WIDTH,
        PREVIEW_HEIGHT,
        // GPUI uploads RenderImage bytes as BGRA. Capturing that layout here
        // removes an 8 MiB per-frame channel-swap pass in the editor.
        TextureFormat::Bgra8UnormSrgb,
        None,
    ));
    for (entity, mut camera) in &mut cameras {
        camera.viewport = None;
        commands
            .entity(entity)
            .insert(RenderTarget::Image(target.clone().into()));
    }
    commands.insert_resource(PreviewTarget(target));
}

fn retarget_new_cameras(
    mut commands: Commands,
    target: Option<Res<PreviewTarget>>,
    mut cameras: Query<(Entity, &mut Camera), Added<Camera>>,
) {
    let Some(target) = target else {
        return;
    };
    for (entity, mut camera) in &mut cameras {
        camera.viewport = None;
        commands
            .entity(entity)
            .insert(RenderTarget::Image(target.0.clone().into()));
    }
}

fn request_capture(
    mut commands: Commands,
    target: Option<Res<PreviewTarget>>,
    activity: Res<RuntimeActivity>,
    visibility: Res<PreviewVisibility>,
    mut capture: ResMut<CaptureState>,
) {
    let Some(target) = target else {
        return;
    };
    if visibility.paused || capture.requested.len() >= MAX_IN_FLIGHT_CAPTURES {
        return;
    }
    if !capture.force
        && !matches!(
            *activity,
            RuntimeActivity::Active | RuntimeActivity::Loading
        )
    {
        return;
    }
    capture.force = false;
    let entity = commands
        .spawn(Screenshot::image(target.0.clone()))
        .observe(publish_capture)
        .id();
    capture.requested.insert(entity);
}

fn publish_capture(
    event: On<ScreenshotCaptured>,
    revision: Res<PreviewDocumentRevision>,
    mut publisher: ResMut<PreviewFramePublisher>,
    mut capture: ResMut<CaptureState>,
) {
    capture.requested.remove(&event.entity);
    let Some(bytes) = event.image.data.as_deref() else {
        return;
    };
    let width = event.image.texture_descriptor.size.width;
    let height = event.image.texture_descriptor.size.height;
    let stride = width.saturating_mul(4);
    if let Err(error) = publisher
        .0
        .publish(revision.0, width, height, stride, bytes)
        && error.kind() != io::ErrorKind::WouldBlock
    {
        log::warn!("failed to publish embedded preview frame: {error}");
    }
}

fn inject_preview_input(
    mut queue: ResMut<PreviewInputQueue>,
    mut direct: ResMut<PreviewDirectInputQueue>,
    mut pointer: ResMut<PreviewPointerState>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    if pointer.pressed_last_frame {
        mouse.release(MouseButton::Left);
        pointer.pressed_last_frame = false;
    }
    for event in queue.0.drain(..) {
        match event {
            PreviewInput::PointerPressed { x, y } => {
                window.set_physical_cursor_position(Some(DVec2::new(x as f64, y as f64)));
                mouse.press(MouseButton::Left);
                pointer.pressed_last_frame = true;
            }
            event => direct.0.push(event),
        }
    }
}

fn apply_input(
    mut queue: ResMut<PreviewDirectInputQueue>,
    mut actions: ResMut<InputActions>,
    mut state: ResMut<GameState>,
) {
    for event in queue.0.drain(..) {
        match event {
            PreviewInput::Advance => {
                actions.advance = true;
                actions.pointer_advance = false;
            }
            PreviewInput::Choice { index } => keine_core::step::select_choice(&mut state, index),
            PreviewInput::PointerPressed { .. } => unreachable!("pointer input is injected above"),
        }
    }
}

fn sync_paused_audio(
    visibility: Res<PreviewVisibility>,
    sinks: Query<(Entity, &AudioSink, Option<&PreviewPausedAudio>)>,
    mut commands: Commands,
) {
    if !visibility.is_changed() {
        return;
    }
    for (entity, sink, paused_by_preview) in &sinks {
        match (
            visibility.paused,
            paused_by_preview.is_some(),
            sink.is_paused(),
        ) {
            (true, false, false) => {
                sink.pause();
                commands.entity(entity).insert(PreviewPausedAudio);
            }
            (false, true, _) => {
                sink.play();
                commands.entity(entity).remove::<PreviewPausedAudio>();
            }
            _ => {}
        }
    }
}

pub(crate) fn set_paused(app: &mut App, paused: bool) {
    if let Some(mut visibility) = app.world_mut().get_resource_mut::<PreviewVisibility>() {
        visibility.paused = paused;
    }
}

pub(crate) fn set_document_revision(app: &mut App, revision: u64) {
    if let Some(mut current) = app
        .world_mut()
        .get_resource_mut::<PreviewDocumentRevision>()
    {
        current.0 = revision;
    }
    if let Some(mut capture) = app.world_mut().get_resource_mut::<CaptureState>() {
        capture.force = true;
    }
}

pub(crate) fn queue_input(app: &mut App, event: PreviewInput) {
    if let Some(mut queue) = app.world_mut().get_resource_mut::<PreviewInputQueue>() {
        queue.0.push(event);
    }
}

pub(crate) fn seek_source(app: &mut App, path: &Path, line: usize) -> bool {
    let world = app.world_mut();
    let Some(content) = world
        .get_resource::<super::resources::ContentProjectResource>()
        .map(|content| content.0.clone())
    else {
        return false;
    };
    let Some(manifest) = world
        .get_resource::<LocalAssetManifest>()
        .map(|manifest| manifest.0.clone())
    else {
        return false;
    };
    let changed = {
        let Some(mut state) = world.get_resource_mut::<GameState>() else {
            return false;
        };
        super::tick::sync_editor_source_position(
            &content,
            &mut state,
            &LocalAssetManifest(manifest),
            path,
            line,
        )
    };
    if changed {
        let revision = app.world().resource::<PreviewDocumentRevision>().0;
        set_document_revision(app, revision);
    }
    changed
}

pub(crate) fn wants_continuous_updates(app: &App) -> bool {
    app.world()
        .get_resource::<RuntimeActivity>()
        .is_none_or(|activity| {
            matches!(
                *activity,
                RuntimeActivity::Active | RuntimeActivity::Loading
            )
        })
}
