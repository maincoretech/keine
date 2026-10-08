//! Render capture and live slot-preview projection. Encoding and disk writes live in storage.
use crate::runtime::platform::DesignViewport;
use crate::storage::save::QUICK_SAVE_SLOT;
use crate::storage::save::preview::SavePreviewWriter;
use crate::ui::control_bar::QuickSavePreview;
use bevy::camera::{
    OrthographicProjection, Projection, RenderTarget, ScalingMode, visibility::RenderLayers,
};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
const SAVE_PREVIEW_LIMIT: UVec2 = UVec2::new(480, 270);

#[derive(Component)]
struct SavePreviewCapture {
    camera: Entity,
    slot: u32,
    generation: crate::storage::save::SavePreviewGeneration,
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

pub(crate) fn capture_save_preview(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    window: &Window,
    slot: u32,
    generation: crate::storage::save::SavePreviewGeneration,
) {
    let viewport = DesignViewport::from_window(window);
    let size = Vec2::new(keine_core::DESIGN_WIDTH, keine_core::DESIGN_HEIGHT) * viewport.scale;
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
                // Match the main scene's logical content viewport, excluding
                // the window letterbox. WindowSize would instead zoom to the
                // small render target's world units.
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
    use bevy::window::WindowResolution;

    #[test]
    fn save_preview_captures_the_content_frame_on_wide_tall_and_hidpi_windows() {
        for (width, height, scale_factor) in [
            (1920, 1080, 1.0),
            (3168, 1440, 1.0),
            (3168, 1440, 2.75),
            (1280, 1024, 1.0),
        ] {
            let window = Window {
                resolution: WindowResolution::new(width, height)
                    .with_scale_factor_override(scale_factor),
                ..default()
            };
            let mut world = World::new();
            let mut images = Assets::<Image>::default();
            let generation =
                crate::storage::save::SavePreviewCoordinator::default().invalidate_slot(1);
            capture_save_preview(&mut world.commands(), &mut images, &window, 1, generation);
            world.flush();

            let (projection, target) = world
                .query::<(&Projection, &RenderTarget)>()
                .single(&world)
                .unwrap();
            let Projection::Orthographic(projection) = projection else {
                panic!("save preview must use an orthographic camera");
            };
            let ScalingMode::Fixed { width, height } = projection.scaling_mode else {
                panic!("thumbnail target size must not determine the view's world size");
            };
            let content = DesignViewport::from_window(&window);
            assert!((width - keine_core::DESIGN_WIDTH * content.scale).abs() < 0.001);
            assert!((height - keine_core::DESIGN_HEIGHT * content.scale).abs() < 0.001);
            let RenderTarget::Image(target) = target else {
                panic!("save preview must render to an image");
            };
            let image = images.get(&target.handle).unwrap();
            assert_eq!(
                UVec2::new(image.width(), image.height()),
                SAVE_PREVIEW_LIMIT
            );
        }
    }

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
