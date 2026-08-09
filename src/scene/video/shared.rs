use std::io;
use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use keine_core::{BlendMode, VideoMode, VisualFilter};
use keine_loader::{ContentFile, ContentMount};

use crate::runtime::platform::DesignViewport;
use crate::scene::effects::material::{StageMaterial, StageQuad};

#[derive(Component)]
pub(super) struct VideoNode;

#[derive(Default)]
pub(super) struct VideoVisual {
    pub(super) image: Option<Handle<Image>>,
    pub(super) material: Option<Handle<StageMaterial>>,
    pub(super) entity: Option<Entity>,
    presentation: Option<VideoPresentation>,
}

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
pub(super) struct VideoFrame {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) pixels: Vec<u8>,
    pub(super) format: TextureFormat,
}

#[derive(Debug)]
pub(super) struct PreparedSource {
    mount: ContentMount,
    logical_path: PathBuf,
    physical_path: Option<PathBuf>,
    length: u64,
}

impl PreparedSource {
    pub(super) fn open(&self) -> io::Result<ContentFile> {
        self.mount
            .open_file(&self.logical_path)
            .map_err(|error| io::Error::other(error.to_string()))
    }

    pub(super) fn physical_path(&self) -> Option<&Path> {
        self.physical_path.as_deref()
    }

    pub(super) const fn len(&self) -> u64 {
        self.length
    }

    pub(super) fn extension(&self) -> Option<&str> {
        self.logical_path
            .extension()
            .and_then(|value| value.to_str())
    }

    #[cfg(all(
        test,
        feature = "video-ffmpeg",
        not(all(feature = "video-native", target_os = "macos"))
    ))]
    pub(super) fn filesystem(path: PathBuf) -> Self {
        use keine_loader::ContentBackend;

        let root = path.parent().unwrap_or_else(|| Path::new(".")).to_owned();
        let logical_path = path
            .file_name()
            .map(PathBuf::from)
            .expect("test video path must name a file");
        let mount = ContentMount::new(ContentBackend::FileSystem(root), "")
            .expect("test video mount must be valid");
        let length = std::fs::metadata(&path)
            .expect("test video must exist")
            .len();
        Self {
            mount,
            logical_path,
            physical_path: Some(path),
            length,
        }
    }
}

pub(super) fn prepare_source(
    mounts: &[ContentMount],
    path: &Path,
) -> Result<PreparedSource, String> {
    for mount in mounts.iter().rev() {
        if !mount.contains_file(path) {
            continue;
        }
        let source = mount.open_file(path).map_err(|error| error.to_string())?;
        let length = source.len().map_err(|error| error.to_string())?;
        return Ok(PreparedSource {
            mount: mount.clone(),
            logical_path: path.to_owned(),
            physical_path: mount.filesystem_root().map(|root| root.join(path)),
            length,
        });
    }
    Err(format!("video asset does not exist: {}", path.display()))
}

pub(super) struct VisualResources<'a> {
    pub(super) images: &'a mut Assets<Image>,
    pub(super) materials: &'a mut Assets<StageMaterial>,
    pub(super) quad: &'a StageQuad,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct VideoPresentation {
    pub(super) mode: VideoMode,
    pub(super) opacity: f32,
    pub(super) viewport: DesignViewport,
}

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
pub(super) fn present_frame(
    id: &str,
    visual: &mut VideoVisual,
    frame: VideoFrame,
    presentation: VideoPresentation,
    commands: &mut Commands,
    resources: VisualResources<'_>,
) {
    let handle = if let Some(handle) = &visual.image {
        if let Some(mut current) = resources.images.get_mut(handle) {
            update_video_image(&mut current, frame);
        }
        handle.clone()
    } else {
        let handle = resources.images.add(video_image(frame));
        visual.image = Some(handle.clone());
        handle
    };
    present_image(id, visual, handle, presentation, commands, resources);
}

pub(super) fn present_image(
    id: &str,
    visual: &mut VideoVisual,
    handle: Handle<Image>,
    presentation: VideoPresentation,
    commands: &mut Commands,
    resources: VisualResources<'_>,
) {
    if visual.entity.is_none() {
        let material = resources.materials.add(StageMaterial::new(
            handle,
            presentation.opacity,
            VisualFilter::default(),
            video_blend(presentation.mode),
            Vec4::ZERO,
            &keine_core::PostProcessEffect::default(),
            None,
        ));
        visual.material = Some(material.clone());
        visual.entity = Some(
            commands
                .spawn((
                    Name::new(format!("video::{id}")),
                    VideoNode,
                    Mesh2d(resources.quad.0.clone()),
                    MeshMaterial2d(material),
                    video_transform(presentation.viewport, presentation.mode),
                    RenderLayers::layer(video_layer(presentation.mode)),
                ))
                .id(),
        );
        visual.presentation = Some(presentation);
    }
}

pub(super) fn update_visual(
    visual: &mut VideoVisual,
    presentation: VideoPresentation,
    materials: &mut Assets<StageMaterial>,
    nodes: &mut Query<
        (
            &MeshMaterial2d<StageMaterial>,
            &mut Transform,
            &mut RenderLayers,
        ),
        With<VideoNode>,
    >,
) {
    if visual.presentation == Some(presentation) {
        return;
    }
    let Some(entity) = visual.entity else {
        return;
    };
    let Ok((material, mut transform, mut layers)) = nodes.get_mut(entity) else {
        return;
    };
    if let Some(mut material) = materials.get_mut(&material.0) {
        material.tint.w = presentation.opacity;
    }
    *transform = video_transform(presentation.viewport, presentation.mode);
    *layers = RenderLayers::layer(video_layer(presentation.mode));
    visual.presentation = Some(presentation);
}

pub(super) fn cleanup_visual(
    visual: &mut VideoVisual,
    commands: &mut Commands,
    images: &mut Assets<Image>,
    materials: &mut Assets<StageMaterial>,
) {
    if let Some(entity) = visual.entity.take() {
        commands.entity(entity).try_despawn();
    }
    if let Some(image) = visual.image.take() {
        images.remove(image.id());
    }
    if let Some(material) = visual.material.take() {
        materials.remove(material.id());
    }
    visual.presentation = None;
}

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
fn video_image(frame: VideoFrame) -> Image {
    Image::new(
        Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        frame.pixels,
        frame.format,
        // The CPU frame is uploaded once and not retained by the main world.
        // Reusing the Image handle also lets Bevy reuse its GPU texture.
        RenderAssetUsages::RENDER_WORLD,
    )
}

pub(super) fn video_image_placeholder(size: Extent3d, format: TextureFormat) -> Image {
    Image::new_uninit(
        size,
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
fn update_video_image(image: &mut Image, frame: VideoFrame) {
    let same_layout = image.texture_descriptor.size.width == frame.width
        && image.texture_descriptor.size.height == frame.height
        && image.texture_descriptor.size.depth_or_array_layers == 1
        && image.texture_descriptor.dimension == TextureDimension::D2
        && image.texture_descriptor.format == frame.format;
    if same_layout {
        // Only pixel storage changes during normal playback. Preserving the
        // descriptor, sampler, view and handle avoids rebuilding asset state
        // for every decoded frame while Bevy reuses the GPU texture.
        image.data = Some(frame.pixels);
    } else {
        *image = video_image(frame);
    }
}

fn video_transform(viewport: DesignViewport, mode: VideoMode) -> Transform {
    Transform::from_translation(viewport.content_center().extend(video_z(mode))).with_scale(
        Vec3::new(
            keine_core::DESIGN_WIDTH * viewport.scale,
            keine_core::DESIGN_HEIGHT * viewport.scale,
            1.0,
        ),
    )
}

const fn video_layer(mode: VideoMode) -> usize {
    match mode {
        VideoMode::Fullscreen => 2,
        VideoMode::Mixed => 0,
    }
}

const fn video_blend(mode: VideoMode) -> BlendMode {
    match mode {
        VideoMode::Fullscreen => BlendMode::Alpha,
        VideoMode::Mixed => BlendMode::Screen,
    }
}

const fn video_z(mode: VideoMode) -> f32 {
    match mode {
        VideoMode::Fullscreen => 1_000.0,
        VideoMode::Mixed => 50.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_video_uses_the_authored_screen_blend() {
        assert_eq!(video_blend(VideoMode::Mixed), BlendMode::Screen);
        assert_eq!(video_blend(VideoMode::Fullscreen), BlendMode::Alpha);
    }

    #[cfg(all(
        feature = "video-ffmpeg",
        not(all(feature = "video-native", target_os = "macos"))
    ))]
    #[test]
    fn video_frames_do_not_keep_a_second_main_world_copy() {
        let image = video_image(VideoFrame {
            width: 1,
            height: 1,
            pixels: vec![0, 0, 0, 255],
            format: TextureFormat::Rgba8UnormSrgb,
        });
        assert_eq!(image.asset_usage, RenderAssetUsages::RENDER_WORLD);
    }

    #[cfg(all(
        feature = "video-ffmpeg",
        not(all(feature = "video-native", target_os = "macos"))
    ))]
    #[test]
    fn same_size_video_frame_preserves_image_configuration() {
        let mut image = video_image(VideoFrame {
            width: 2,
            height: 1,
            pixels: vec![0; 8],
            format: TextureFormat::Rgba8UnormSrgb,
        });
        image.copy_on_resize = true;
        update_video_image(
            &mut image,
            VideoFrame {
                width: 2,
                height: 1,
                pixels: vec![7; 8],
                format: TextureFormat::Rgba8UnormSrgb,
            },
        );

        assert_eq!(image.data.as_deref(), Some([7; 8].as_slice()));
        assert!(image.copy_on_resize);
        assert_eq!(image.texture_descriptor.size.width, 2);
    }
}
