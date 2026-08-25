use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;

use bevy::asset::{AssetApp, AssetId, AssetLoader, LoadContext, RenderAssetUsages, io::Reader};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use keine_loader::ResourceKind;
use keine_media::{ImageSize, MAX_WEBP_FILE_BYTES};
use serde::{Deserialize, Serialize};

use crate::runtime::resources::{GameConfigResource, LocalAssetCache, LocalAssetManifest};

const BACKGROUND_LIMIT: UVec2 = UVec2::new(
    keine_core::DESIGN_WIDTH as u32,
    keine_core::DESIGN_HEIGHT as u32,
);
const MAX_SPRITE_HEIGHT: f32 = keine_core::DESIGN_HEIGHT * 4.0;

pub(crate) struct NativeWebpPlugin {
    sprite_height: f32,
}

impl NativeWebpPlugin {
    pub(crate) fn new(sprite_height: f32) -> Self {
        Self { sprite_height }
    }
}

impl Plugin for NativeWebpPlugin {
    fn build(&self, app: &mut App) {
        app.register_asset_loader(NativeWebpLoader {
            sprite_height: self.sprite_height,
        });
    }
}

#[derive(TypePath)]
struct NativeWebpLoader {
    sprite_height: f32,
}

/// Semantic decode requirements for one project image.
///
/// Bevy intentionally applies loader settings only from the first load of an
/// asset path. The registry therefore merges every known use before startup so
/// aliases and shared background/figure files always request one stable target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ImageRole {
    background: bool,
    figure: bool,
    raw: bool,
}

impl ImageRole {
    pub(crate) const BACKGROUND: Self = Self {
        background: true,
        figure: false,
        raw: false,
    };
    pub(crate) const FIGURE: Self = Self {
        background: false,
        figure: true,
        raw: false,
    };
    pub(crate) const RAW: Self = Self {
        background: false,
        figure: false,
        raw: true,
    };

    fn for_resource(kind: ResourceKind) -> Option<Self> {
        match kind {
            ResourceKind::Background => Some(Self::BACKGROUND),
            ResourceKind::Figure | ResourceKind::MiniAvatar => Some(Self::FIGURE),
            ResourceKind::Particle | ResourceKind::Lut => Some(Self::RAW),
            ResourceKind::Voice
            | ResourceKind::Bgm
            | ResourceKind::Effect
            | ResourceKind::Video => None,
        }
    }

    fn merge(&mut self, other: Self) {
        self.background |= other.background;
        self.figure |= other.figure;
        self.raw |= other.raw;
    }

    fn is_stage_art(self) -> bool {
        self.background || self.figure
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
struct NativeWebpSettings {
    role: ImageRole,
}

#[derive(Resource, Default)]
pub(crate) struct ImageRoleRegistry(HashMap<String, ImageRole>);

impl ImageRoleRegistry {
    pub(crate) fn rebuild(&mut self, config: &GameConfigResource, manifest: &LocalAssetManifest) {
        self.0.clear();
        self.register(
            config.bg_path(&config.title_background),
            ImageRole::BACKGROUND,
        );
        for scene in manifest.values() {
            for resource in &scene.resources {
                if let Some(role) = ImageRole::for_resource(resource.kind) {
                    self.register(resource.resolved_path(config), role);
                }
            }
        }
    }

    fn register(&mut self, path: String, role: ImageRole) {
        self.0.entry(path).or_default().merge(role);
    }

    fn resolve(&self, path: &str, requested: ImageRole) -> ImageRole {
        self.0.get(path).copied().unwrap_or(requested)
    }
}

impl AssetLoader for NativeWebpLoader {
    type Asset = Image;
    type Settings = NativeWebpSettings;
    type Error = io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Image, Self::Error> {
        let bytes = read_webp_input(reader).await?;
        decode_webp(&bytes, |original| {
            target_size(settings.role, original, self.sprite_height)
        })
    }

    fn extensions(&self) -> &[&str] {
        &["webp"]
    }
}

pub(crate) fn load(
    asset_server: &AssetServer,
    roles: &ImageRoleRegistry,
    path: String,
    requested: ImageRole,
) -> Handle<Image> {
    if !is_webp(&path) {
        return asset_server.load(path);
    }
    let role = roles.resolve(&path, requested);
    asset_server
        .load_builder()
        .with_settings(move |settings: &mut NativeWebpSettings| settings.role = role)
        .load(path)
}

fn is_webp(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("webp"))
}

async fn read_webp_input(reader: &mut dyn Reader) -> io::Result<Vec<u8>> {
    crate::runtime::bounded_input::read_to_end(reader, MAX_WEBP_FILE_BYTES, "WebP").await
}

#[derive(Resource, Default)]
pub(crate) struct ImageDimensions(HashMap<AssetId<Image>, UVec2>);

#[derive(Resource, Default)]
pub(crate) struct PreparedImages(HashSet<AssetId<Image>>);

impl ImageDimensions {
    pub(crate) fn size(&self, handle: &Handle<Image>) -> Option<UVec2> {
        self.0.get(&handle.id()).copied()
    }

    pub(crate) fn aspect(&self, handle: &Handle<Image>) -> Option<f32> {
        let size = self.size(handle)?;
        (size.y > 0).then_some(size.x as f32 / size.y as f32)
    }
}

/// Downsizes immutable VN art to its design-space ceiling, records layout
/// metadata, then releases decoded CPU pixels after render extraction.
pub(crate) fn prepare(
    cache: Res<LocalAssetCache>,
    config: Res<GameConfigResource>,
    roles: Res<ImageRoleRegistry>,
    mut images: ResMut<Assets<Image>>,
    mut dimensions: ResMut<ImageDimensions>,
    mut prepared: ResMut<PreparedImages>,
    mut events: MessageReader<AssetEvent<Image>>,
) {
    for event in events.read() {
        let id = match event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } | AssetEvent::Removed { id } => {
                *id
            }
            AssetEvent::Unused { .. } | AssetEvent::LoadedWithDependencies { .. } => continue,
        };
        prepared.0.remove(&id);
        dimensions.0.remove(&id);
    }
    let has_pending = cache.handles.iter().any(|(path, handle)| {
        roles.resolve(path, ImageRole::default()).is_stage_art()
            && !prepared.0.contains(&handle.id().typed::<Image>())
    });
    if !cache.is_changed() && !has_pending {
        return;
    }

    let active = cache
        .handles
        .iter()
        .filter(|(path, _)| roles.resolve(path, ImageRole::default()).is_stage_art())
        .map(|(_, handle)| handle.id().typed::<Image>())
        .collect::<HashSet<_>>();
    if cache.is_changed() {
        if prepared.0.iter().any(|id| !active.contains(id)) {
            prepared.0.retain(|id| active.contains(id));
        }
        if dimensions.0.keys().any(|id| !active.contains(id)) {
            dimensions.0.retain(|id, _| active.contains(id));
        }
    }

    for (path, handle) in &cache.handles {
        let role = roles.resolve(path, ImageRole::default());
        if !role.is_stage_art() {
            continue;
        }
        let id = handle.id().typed::<Image>();
        if prepared.0.contains(&id) {
            continue;
        }
        let Some(mut image) = images.get_mut(id) else {
            continue;
        };
        let original = image.size();
        let target = target_size(role, original, config.layout.sprite_height);
        dimensions.0.insert(id, target);

        if target != original && is_resizeable(&image) {
            // The image loader guarantees valid tightly packed RGBA8 here, so
            // transfer the pixel allocation instead of cloning a full-size
            // image before resizing it.
            let source = std::mem::take(&mut *image)
                .try_into_dynamic()
                .expect("validated RGBA8 image must convert");
            *image = Image::from_dynamic(
                source.thumbnail(target.x, target.y),
                true,
                RenderAssetUsages::RENDER_WORLD,
            );
        } else {
            if target != original {
                log::debug!(
                    "keeping unsupported immutable image {path} at {}x{}",
                    original.x,
                    original.y
                );
            }
            image.asset_usage = RenderAssetUsages::RENDER_WORLD;
        }
        prepared.0.insert(id);
    }
}

fn is_resizeable(image: &Image) -> bool {
    image.texture_descriptor.dimension == TextureDimension::D2
        && image.texture_descriptor.size.depth_or_array_layers == 1
        && image.texture_descriptor.mip_level_count == 1
        && image.texture_descriptor.format == TextureFormat::Rgba8UnormSrgb
        && image.data.as_ref().is_some_and(|data| {
            data.len()
                == image.width() as usize * image.height() as usize * std::mem::size_of::<u32>()
        })
}

fn target_size(role: ImageRole, original: UVec2, sprite_height: f32) -> UVec2 {
    if role.raw {
        return original;
    }
    let mut target = UVec2::ZERO;
    if role.background {
        target = fit_within(original, BACKGROUND_LIMIT);
    }
    if role.figure {
        let sprite_height = if sprite_height.is_finite() && sprite_height > 0.0 {
            sprite_height.min(MAX_SPRITE_HEIGHT)
        } else {
            keine_core::DESIGN_HEIGHT
        };
        let figure = fit_within(
            original,
            UVec2::new(keine_core::DESIGN_WIDTH as u32, sprite_height.ceil() as u32)
                .max(UVec2::ONE),
        );
        if figure.element_product() > target.element_product() {
            target = figure;
        }
    }
    if target != UVec2::ZERO {
        target
    } else {
        original
    }
}

pub(crate) fn decode_preview(bytes: &[u8]) -> io::Result<Image> {
    decode_webp(bytes, |original| original)
}

fn decode_webp(bytes: &[u8], target: impl FnOnce(UVec2) -> UVec2) -> io::Result<Image> {
    let decoded = keine_media::decode_webp(bytes, |original| {
        let output = target(UVec2::new(original.width, original.height));
        ImageSize::new(output.x, output.y)
    })?;
    let output = decoded.size();
    Ok(Image::new(
        Extent3d {
            width: output.width,
            height: output.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        decoded.into_pixels(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

const PREVIEW_WEBP_QUALITY: f32 = 80.0;

pub(crate) fn encode_preview(rgba: &[u8], width: u32, height: u32) -> io::Result<Vec<u8>> {
    // Save-card previews are small display aids rather than archival artwork.
    // Lossy WebP at a fixed quality avoids spending CPU and disk on pixel-exact
    // output while retaining the alpha channel should a render target need it.
    keine_media::encode_webp_rgba(rgba, width, height, PREVIEW_WEBP_QUALITY)
}

fn fit_within(original: UVec2, limit: UVec2) -> UVec2 {
    if original.x <= limit.x && original.y <= limit.y {
        return original;
    }
    let scale = (limit.x as f64 / original.x as f64).min(limit.y as f64 / original.y as f64);
    UVec2::new(
        (original.x as f64 * scale).round().max(1.0) as u32,
        (original.y as f64 * scale).round().max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::resources::{GameConfigResource, LocalAssetManifest, LocalSceneAssets};
    use keine_core::config::GameConfig;
    use keine_loader::{ResourceRef, SourceSpan};

    fn resource(path: &str, kind: ResourceKind) -> ResourceRef {
        ResourceRef {
            path: path.into(),
            kind,
            action_index: 0,
            span: SourceSpan { line: 1, column: 1 },
        }
    }

    #[test]
    fn preserves_aspect_when_reducing_figure() {
        assert_eq!(
            target_size(ImageRole::FIGURE, UVec2::new(1536, 2742), 825.0),
            UVec2::new(462, 825)
        );
    }

    #[test]
    fn caps_background_textures_at_the_design_resolution() {
        assert_eq!(
            target_size(ImageRole::BACKGROUND, UVec2::new(3840, 2160), 825.0),
            UVec2::new(1920, 1080)
        );
    }

    #[test]
    fn never_upscales_source_art() {
        assert_eq!(
            target_size(ImageRole::BACKGROUND, UVec2::new(1280, 720), 825.0),
            UVec2::new(1280, 720)
        );
    }

    #[test]
    fn bounds_invalid_sprite_heights() {
        assert_eq!(
            target_size(ImageRole::FIGURE, UVec2::new(1920, 1080), f32::INFINITY),
            UVec2::new(1920, 1080)
        );
    }

    #[test]
    fn roles_ignore_directory_names_and_merge_shared_assets() {
        let mut roles = ImageRoleRegistry::default();
        roles.register("art/shared.webp".into(), ImageRole::FIGURE);
        roles.register("art/shared.webp".into(), ImageRole::BACKGROUND);
        let role = roles.resolve("art/shared.webp", ImageRole::RAW);

        assert!(role.figure);
        assert!(role.background);
        assert_eq!(
            target_size(role, UVec2::new(3840, 2160), 825.0),
            UVec2::new(1920, 1080)
        );
    }

    #[test]
    fn manifest_aliases_define_decode_roles() {
        let mut config = GameConfig::default();
        config
            .assets
            .backgrounds
            .insert("sea".into(), "artwork/scenes/sea.webp".into());
        config
            .assets
            .figures
            .insert("hero".into(), "art/hero.webp".into());
        let config = GameConfigResource(config);
        let mut manifest = LocalAssetManifest::default();
        manifest.insert(
            "start".into(),
            LocalSceneAssets {
                resources: vec![
                    resource("sea", ResourceKind::Background),
                    resource("hero", ResourceKind::Figure),
                ],
                ..default()
            },
        );
        let mut roles = ImageRoleRegistry::default();
        roles.rebuild(&config, &manifest);

        let background = roles.resolve("artwork/scenes/sea.webp", ImageRole::RAW);
        let figure = roles.resolve("art/hero.webp", ImageRole::RAW);
        assert_eq!(
            target_size(background, UVec2::new(3840, 2160), 825.0),
            UVec2::new(1920, 1080)
        );
        let figure_size = target_size(figure, UVec2::new(2000, 1000), 825.0);
        assert_eq!(figure_size, UVec2::new(1650, 825));
        assert_eq!(figure_size.x as f32 / figure_size.y as f32, 2.0);
    }

    #[test]
    fn raw_role_keeps_original_pixels() {
        assert_eq!(
            target_size(ImageRole::RAW, UVec2::new(3840, 2160), 825.0),
            UVec2::new(3840, 2160)
        );
    }

    #[test]
    fn native_webp_round_trip_and_scaled_decode() {
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255, // row 1
            0, 0, 0, 255, 64, 64, 64, 255, 128, 128, 128, 255, 192, 192, 192, 255, // row 2
        ];
        let encoded = encode_preview(&rgba, 4, 2).expect("encode preview");
        let decoded = decode_webp(&encoded, |_| UVec2::new(2, 1)).expect("decode preview");
        assert_eq!(decoded.size(), UVec2::new(2, 1));
        assert_eq!(decoded.asset_usage, RenderAssetUsages::RENDER_WORLD);
        assert_eq!(decoded.data.as_ref().map(Vec::len), Some(8));
    }
}
