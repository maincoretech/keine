use std::collections::{HashMap, HashSet};
use std::io;

use bevy::asset::{AssetApp, AssetId, AssetLoader, LoadContext, RenderAssetUsages, io::Reader};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use futures_lite::io::AsyncReadExt;
use libwebp_sys::{
    VP8StatusCode, WEBP_CSP_MODE, WebPDecode, WebPDecoderConfig, WebPEncodeRGBA, WebPFree,
    WebPFreeDecBuffer, WebPGetFeatures, WebPInitDecoderConfig, WebPRGBABuffer,
};

use crate::runtime::resources::{GameConfigResource, LocalAssetCache};

const BACKGROUND_LIMIT: UVec2 = UVec2::new(
    keine_core::DESIGN_WIDTH as u32,
    keine_core::DESIGN_HEIGHT as u32,
);
const MAX_WEBP_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_WEBP_SOURCE_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_WEBP_OUTPUT_PIXELS: u64 = 16 * 1024 * 1024;
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

impl AssetLoader for NativeWebpLoader {
    type Asset = Image;
    type Settings = ();
    type Error = io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Image, Self::Error> {
        let bytes = read_webp_input(reader).await?;
        let path = load_context.path().path().to_string_lossy();
        decode_webp(&bytes, |original| {
            target_size(&path, original, self.sprite_height)
        })
    }

    fn extensions(&self) -> &[&str] {
        &["webp"]
    }
}

async fn read_webp_input(reader: &mut dyn Reader) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Ok(bytes);
        }
        let new_len = bytes
            .len()
            .checked_add(read)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "WebP size overflow"))?;
        if new_len > MAX_WEBP_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("WebP exceeds the {MAX_WEBP_FILE_BYTES}-byte input limit"),
            ));
        }
        bytes
            .try_reserve_exact(read)
            .map_err(|error| io::Error::other(format!("failed to reserve WebP input: {error}")))?;
        bytes.extend_from_slice(&chunk[..read]);
    }
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
    let is_image = |path: &str| is_background_path(path) || is_figure_path(path);
    let has_pending = cache.handles.iter().any(|(path, handle)| {
        is_image(path) && !prepared.0.contains(&handle.id().typed::<Image>())
    });
    if !cache.is_changed() && !has_pending {
        return;
    }

    let active = cache
        .handles
        .iter()
        .filter(|(path, _)| is_image(path))
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
        if !is_background_path(path) && !is_figure_path(path) {
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
        let target = target_size(path, original, config.layout.sprite_height);
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

fn target_size(path: &str, original: UVec2, sprite_height: f32) -> UVec2 {
    let limit = if is_figure_path(path) {
        let sprite_height = if sprite_height.is_finite() && sprite_height > 0.0 {
            sprite_height.min(MAX_SPRITE_HEIGHT)
        } else {
            keine_core::DESIGN_HEIGHT
        };
        UVec2::new(keine_core::DESIGN_WIDTH as u32, sprite_height.ceil() as u32)
    } else if is_background_path(path) {
        BACKGROUND_LIMIT
    } else {
        return original;
    };
    fit_within(original, limit.max(UVec2::ONE))
}

fn is_background_path(path: &str) -> bool {
    matches!(
        path.split('/').next().unwrap_or_default(),
        "background" | "backgrounds" | "cg"
    )
}

fn is_figure_path(path: &str) -> bool {
    matches!(
        path.split('/').next().unwrap_or_default(),
        "figure" | "figures" | "character" | "characters"
    )
}

pub(crate) fn decode_preview(bytes: &[u8]) -> io::Result<Image> {
    decode_webp(bytes, |original| original)
}

fn decode_webp(bytes: &[u8], target: impl FnOnce(UVec2) -> UVec2) -> io::Result<Image> {
    if bytes.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty WebP"));
    }
    if bytes.len() > MAX_WEBP_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("WebP exceeds the {MAX_WEBP_FILE_BYTES}-byte input limit"),
        ));
    }

    // SAFETY: libwebp initializes every field before it is read. Input and
    // output buffers remain alive for the full native call, and all sizes are
    // checked before their pointers are exposed to C.
    unsafe {
        let mut config = std::mem::MaybeUninit::<WebPDecoderConfig>::zeroed().assume_init();
        if !WebPInitDecoderConfig(&mut config) {
            return Err(io::Error::other("libwebp ABI mismatch"));
        }
        let status = WebPGetFeatures(bytes.as_ptr(), bytes.len(), &mut config.input);
        if status != VP8StatusCode::VP8_STATUS_OK
            || config.input.width <= 0
            || config.input.height <= 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid WebP header ({status:?})"),
            ));
        }

        let original = UVec2::new(config.input.width as u32, config.input.height as u32);
        check_pixel_budget("source", original, MAX_WEBP_SOURCE_PIXELS)?;
        let output = target(original).max(UVec2::ONE);
        check_pixel_budget("output", output, MAX_WEBP_OUTPUT_PIXELS)?;
        let stride = output
            .x
            .checked_mul(4)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "WebP row too wide"))?;
        let output_len = (stride as usize)
            .checked_mul(output.y as usize)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "WebP output too large"))?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(output_len).map_err(|error| {
            io::Error::other(format!("failed to reserve WebP output buffer: {error}"))
        })?;
        rgba.resize(output_len, 0);

        config.options.use_threads = 1;
        if output != original {
            config.options.use_scaling = 1;
            config.options.scaled_width = output.x as i32;
            config.options.scaled_height = output.y as i32;
        }
        config.output.colorspace = WEBP_CSP_MODE::MODE_RGBA;
        config.output.is_external_memory = 1;
        config.output.u.RGBA = WebPRGBABuffer {
            rgba: rgba.as_mut_ptr(),
            stride,
            size: rgba.len(),
        };

        let status = WebPDecode(bytes.as_ptr(), bytes.len(), &mut config);
        WebPFreeDecBuffer(&mut config.output);
        if status != VP8StatusCode::VP8_STATUS_OK {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("libwebp decode failed ({status:?})"),
            ));
        }

        Ok(Image::new(
            Extent3d {
                width: output.x,
                height: output.y,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            rgba,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        ))
    }
}

fn check_pixel_budget(label: &str, size: UVec2, maximum: u64) -> io::Result<()> {
    let pixels = u64::from(size.x)
        .checked_mul(u64::from(size.y))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "WebP pixel count overflow"))?;
    if pixels > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "WebP {label} dimensions {}x{} exceed the {maximum}-pixel limit",
                size.x, size.y
            ),
        ));
    }
    Ok(())
}

const PREVIEW_WEBP_QUALITY: f32 = 80.0;

pub(crate) fn encode_preview(rgba: &[u8], width: u32, height: u32) -> io::Result<Vec<u8>> {
    let stride = width
        .checked_mul(4)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "preview row too wide"))?;
    let expected = (stride as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "preview too large"))?;
    if rgba.len() != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "preview RGBA buffer has an invalid length",
        ));
    }

    let mut encoded = std::ptr::null_mut();
    // Save-card previews are small display aids rather than archival artwork.
    // Lossy WebP at a fixed quality avoids spending CPU and disk on pixel-exact
    // output while retaining the alpha channel should a render target need it.
    // SAFETY: `rgba` is validated as tightly packed RGBA8 and remains alive
    // for the call. libwebp owns `encoded` until it is copied and freed below.
    let len = unsafe {
        WebPEncodeRGBA(
            rgba.as_ptr(),
            width as i32,
            height as i32,
            stride,
            PREVIEW_WEBP_QUALITY,
            &mut encoded,
        )
    };
    if len == 0 || encoded.is_null() {
        return Err(io::Error::other("libwebp preview encoding failed"));
    }
    // SAFETY: libwebp returned `len` initialized bytes at `encoded`.
    let bytes = unsafe { std::slice::from_raw_parts(encoded, len).to_vec() };
    // SAFETY: The allocation was returned by libwebp and has been copied.
    unsafe { WebPFree(encoded.cast()) };
    Ok(bytes)
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

    #[test]
    fn preserves_aspect_when_reducing_figure() {
        assert_eq!(
            target_size("figure/stand.webp", UVec2::new(1536, 2742), 825.0),
            UVec2::new(462, 825)
        );
    }

    #[test]
    fn caps_background_textures_at_the_design_resolution() {
        assert_eq!(
            target_size("background/bg.webp", UVec2::new(3840, 2160), 825.0),
            UVec2::new(1920, 1080)
        );
    }

    #[test]
    fn never_upscales_source_art() {
        assert_eq!(
            target_size("background/bg.webp", UVec2::new(1280, 720), 825.0),
            UVec2::new(1280, 720)
        );
    }

    #[test]
    fn bounds_invalid_sprite_heights_and_image_pixel_budgets() {
        assert_eq!(
            target_size("figure/stand.webp", UVec2::new(1920, 1080), f32::INFINITY),
            UVec2::new(1920, 1080)
        );
        assert!(
            check_pixel_budget("source", UVec2::new(8_192, 8_192), MAX_WEBP_SOURCE_PIXELS).is_ok()
        );
        assert!(
            check_pixel_budget("source", UVec2::new(8_193, 8_192), MAX_WEBP_SOURCE_PIXELS).is_err()
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
