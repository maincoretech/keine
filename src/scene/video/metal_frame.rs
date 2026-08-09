//! Core Video to Bevy GPU upload without a CPU pixel copy.

use std::collections::HashMap;
use std::ptr;
use std::sync::{Arc, Mutex, MutexGuard};

use bevy::asset::AssetId;
use bevy::prelude::*;
use bevy::render::render_asset::{RenderAssets, prepare_assets};
use bevy::render::render_resource::{
    CommandEncoderDescriptor, Extent3d, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use objc2::rc::Retained;
use objc2_core_video::{
    CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBuffer,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    kCVPixelFormatType_32BGRA, kCVReturnSuccess,
};
use objc2_metal::{MTLPixelFormat, MTLTextureType};
use wgpu::hal::{CopyExtent, api::Metal};

use super::shared::{
    VideoPresentation, VideoVisual, VisualResources, present_image, video_image_placeholder,
};

/// A retained AVFoundation frame moving exactly once from the main world to
/// Bevy's render world. Core Video buffers are reference-counted exchange
/// objects; no thread accesses this value concurrently while it is in flight.
pub(super) struct NativeVideoFrame {
    pixel_buffer: Retained<CVPixelBuffer>,
    width: u32,
    height: u32,
}

// SAFETY: ownership is transferred through the mutex below. The producing
// main-world system stops accessing the buffer before the render-world system
// receives it, which only maps and retains it for one submitted GPU copy.
unsafe impl Send for NativeVideoFrame {}

impl NativeVideoFrame {
    pub(super) fn new(pixel_buffer: Retained<CVPixelBuffer>) -> Result<Self, String> {
        let pixel_format = CVPixelBufferGetPixelFormatType(&pixel_buffer);
        if pixel_format != kCVPixelFormatType_32BGRA {
            return Err(format!(
                "AVFoundation returned unsupported pixel format 0x{pixel_format:08x}"
            ));
        }
        let width = u32::try_from(CVPixelBufferGetWidth(&pixel_buffer))
            .map_err(|_| "video width exceeds u32".to_owned())?;
        let height = u32::try_from(CVPixelBufferGetHeight(&pixel_buffer))
            .map_err(|_| "video height exceeds u32".to_owned())?;
        if width == 0 || height == 0 {
            return Err("AVFoundation returned an empty video frame".to_owned());
        }
        Ok(Self {
            pixel_buffer,
            width,
            height,
        })
    }
}

#[derive(Resource, Clone, Default)]
pub(super) struct MetalFrameBridge(Arc<Mutex<HashMap<AssetId<Image>, NativeVideoFrame>>>);

impl MetalFrameBridge {
    fn pending(&self) -> MutexGuard<'_, HashMap<AssetId<Image>, NativeVideoFrame>> {
        self.0.lock().unwrap_or_else(|poisoned| {
            log::warn!("recovering poisoned native video frame queue");
            poisoned.into_inner()
        })
    }

    fn publish(&self, image: AssetId<Image>, frame: NativeVideoFrame) {
        self.pending().insert(image, frame);
    }

    pub(super) fn discard(&self, image: AssetId<Image>) {
        self.pending().remove(&image);
    }

    fn drain(&self) -> HashMap<AssetId<Image>, NativeVideoFrame> {
        std::mem::take(&mut *self.pending())
    }
}

pub(super) fn install(app: &mut App) {
    let bridge = MetalFrameBridge::default();
    app.insert_resource(bridge.clone());
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app.insert_resource(bridge).add_systems(
        Render,
        upload_native_frames
            .after(prepare_assets::<GpuImage>)
            .in_set(RenderSystems::PrepareAssets),
    );
}

pub(super) fn present_native_frame(
    id: &str,
    visual: &mut VideoVisual,
    frame: NativeVideoFrame,
    presentation: VideoPresentation,
    bridge: &MetalFrameBridge,
    commands: &mut Commands,
    resources: VisualResources<'_>,
) {
    let extent = Extent3d {
        width: frame.width,
        height: frame.height,
        depth_or_array_layers: 1,
    };
    let handle = if let Some(handle) = &visual.image {
        if let Some(mut current) = resources.images.get_mut(handle)
            && current.texture_descriptor.size != extent
        {
            *current = video_image_placeholder(extent, TextureFormat::Bgra8UnormSrgb);
        }
        handle.clone()
    } else {
        let handle = resources.images.add(video_image_placeholder(
            extent,
            TextureFormat::Bgra8UnormSrgb,
        ));
        visual.image = Some(handle.clone());
        handle
    };
    bridge.publish(handle.id(), frame);
    present_image(id, visual, handle, presentation, commands, resources);
}

#[derive(Default)]
struct MetalUploadState {
    cache: Option<Retained<CVMetalTextureCache>>,
}

// SAFETY: this is `Local` state owned exclusively by one render system. Bevy
// may move the system between worker threads, but the cache is never used from
// two threads at once and Core Video objects use atomic retain/release.
unsafe impl Send for MetalUploadState {}

struct ImportedFrame {
    _pixel_buffer: NativeVideoFrame,
    _cv_texture: Retained<CVMetalTexture>,
    texture: wgpu::Texture,
}

// SAFETY: the queue completion callback owns and only drops these retained
// Core Video objects after the submitted GPU copy has finished.
unsafe impl Send for ImportedFrame {}

fn upload_native_frames(
    bridge: Res<MetalFrameBridge>,
    images: Res<RenderAssets<GpuImage>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut state: Local<MetalUploadState>,
) {
    let pending = bridge.drain();
    if pending.is_empty() {
        return;
    }
    let cache = match metal_texture_cache(&render_device, &mut state) {
        Ok(cache) => cache,
        Err(error) => {
            log::error!("native video GPU bridge is unavailable: {error}");
            return;
        }
    };
    let mut encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("native-video-upload"),
    });
    let mut imported = Vec::with_capacity(pending.len());
    let mut deferred = Vec::new();
    for (image_id, frame) in pending {
        let Some(destination) = images.get(image_id) else {
            deferred.push((image_id, frame));
            continue;
        };
        let extent = Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        };
        // A resolution change reaches the render world one extraction later
        // than the frame bridge. Never issue an out-of-bounds copy into the
        // previous texture; keep the latest frame until the new image arrives.
        if destination.texture_descriptor.size != extent {
            deferred.push((image_id, frame));
            continue;
        }
        match import_frame(&render_device, cache, frame) {
            Ok(frame) => {
                encoder.copy_texture_to_texture(
                    frame.texture.as_image_copy(),
                    destination.texture.as_image_copy(),
                    extent,
                );
                imported.push(frame);
            }
            Err(error) => log::error!("native video frame import failed: {error}"),
        }
    }
    // Asset extraction can trail main-world publication by one render pass.
    // Retain only the newest frame for any destination that is not ready yet.
    for (image_id, frame) in deferred {
        bridge.publish(image_id, frame);
    }
    if imported.is_empty() {
        return;
    }
    render_queue.submit([encoder.finish()]);
    // Apple requires the CVMetalTexture to remain retained until the GPU is
    // done with it. Registering after submit binds this short drop callback to
    // the copy above instead of guessing a fixed frames-in-flight count.
    render_queue.on_submitted_work_done(move || drop(imported));
    cache.flush(0);
}

fn metal_texture_cache<'a>(
    render_device: &RenderDevice,
    state: &'a mut MetalUploadState,
) -> Result<&'a Retained<CVMetalTextureCache>, String> {
    if state.cache.is_none() {
        let hal_device = unsafe { render_device.wgpu_device().as_hal::<Metal>() }
            .ok_or_else(|| "wgpu is not using the Metal backend".to_owned())?;
        let metal_device = hal_device.raw_device().clone();
        let mut cache = ptr::null_mut();
        let result = unsafe {
            CVMetalTextureCache::create(
                None,
                None,
                &metal_device,
                None,
                std::ptr::NonNull::new(&mut cache)
                    .expect("the address of a local pointer cannot be null"),
            )
        };
        if result != kCVReturnSuccess || cache.is_null() {
            return Err(format!("CVMetalTextureCache creation failed: {result}"));
        }
        state.cache = Some(unsafe {
            Retained::from_raw(cache).expect("Core Video returned a non-null texture cache")
        });
    }
    Ok(state
        .cache
        .as_ref()
        .expect("texture cache was initialized above"))
}

fn import_frame(
    render_device: &RenderDevice,
    cache: &CVMetalTextureCache,
    frame: NativeVideoFrame,
) -> Result<ImportedFrame, String> {
    let mut cv_texture = ptr::null_mut();
    let result = unsafe {
        CVMetalTextureCache::create_texture_from_image(
            None,
            cache,
            &frame.pixel_buffer,
            None,
            MTLPixelFormat::BGRA8Unorm_sRGB,
            frame.width as usize,
            frame.height as usize,
            0,
            std::ptr::NonNull::new(&mut cv_texture)
                .expect("the address of a local pointer cannot be null"),
        )
    };
    if result != kCVReturnSuccess || cv_texture.is_null() {
        return Err(format!("CVPixelBuffer is not Metal-compatible: {result}"));
    }
    let cv_texture = unsafe {
        Retained::from_raw(cv_texture).expect("Core Video returned a non-null Metal texture")
    };
    let metal_texture = CVMetalTextureGetTexture(&cv_texture)
        .ok_or_else(|| "CVMetalTexture has no MTLTexture".to_owned())?;
    let size = Extent3d {
        width: frame.width,
        height: frame.height,
        depth_or_array_layers: 1,
    };
    let descriptor = TextureDescriptor {
        label: Some("native-video-cvpixelbuffer"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Bgra8UnormSrgb,
        usage: TextureUsages::COPY_SRC,
        view_formats: &[],
    };
    let texture = unsafe {
        render_device
            .wgpu_device()
            .create_texture_from_hal::<Metal>(
                <Metal as wgpu::hal::Api>::Device::texture_from_raw(
                    metal_texture,
                    descriptor.format,
                    MTLTextureType::Type2D,
                    1,
                    1,
                    CopyExtent {
                        width: frame.width,
                        height: frame.height,
                        depth: 1,
                    },
                ),
                &descriptor,
            )
    };
    Ok(ImportedFrame {
        _pixel_buffer: frame,
        _cv_texture: cv_texture,
        texture,
    })
}

/// Exercises the same Core Video -> Metal -> wgpu copy used by the render
/// world without opening a window. This keeps the acceptance binary capable
/// of catching an API-compatible decoder that still returns non-importable
/// pixel buffers.
pub(super) fn validate_frame_import(frame: NativeVideoFrame) -> Result<(), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = futures_lite::future::block_on(
        instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
    )
    .map_err(|error| format!("Metal adapter creation failed: {error}"))?;
    let (device, queue) =
        futures_lite::future::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .map_err(|error| format!("Metal device creation failed: {error}"))?;
    let render_device = RenderDevice::from(device.clone());
    let mut state = MetalUploadState::default();
    let cache = metal_texture_cache(&render_device, &mut state)?;
    let size = Extent3d {
        width: frame.width,
        height: frame.height,
        depth_or_array_layers: 1,
    };
    let imported = import_frame(&render_device, cache, frame)?;
    let destination = device.create_texture(&TextureDescriptor {
        label: Some("native-video-acceptance-destination"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Bgra8UnormSrgb,
        usage: TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("native-video-acceptance-copy"),
    });
    encoder.copy_texture_to_texture(
        imported.texture.as_image_copy(),
        destination.as_image_copy(),
        size,
    );
    let submission = queue.submit([encoder.finish()]);
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(10)),
        })
        .map_err(|error| format!("native video GPU copy failed: {error}"))?;
    cache.flush(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_placeholder_has_no_cpu_pixel_allocation() {
        let image = video_image_placeholder(
            Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            TextureFormat::Bgra8UnormSrgb,
        );
        assert!(image.data.is_none());
        assert_eq!(
            image.asset_usage,
            bevy::asset::RenderAssetUsages::RENDER_WORLD
        );
        assert!(
            image
                .texture_descriptor
                .usage
                .contains(TextureUsages::COPY_DST)
        );
    }
}
