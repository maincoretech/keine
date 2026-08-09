use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::ptr;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::thread;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use dispatch2::{DispatchQueue, DispatchRetained};
use keine_core::VideoMode;
use keine_loader::ContentMount;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_av_foundation::{
    AVAssetResourceLoader, AVAssetResourceLoaderDelegate, AVAssetResourceLoadingRequest, AVPlayer,
    AVPlayerItem, AVPlayerItemStatus, AVPlayerItemVideoOutput, AVURLAsset,
};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelFormatType_32BGRA, kCVReturnSuccess,
};
use objc2_foundation::{
    NSData, NSDate, NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSRunLoop, NSString, NSURL,
    ns_string,
};

use super::shared::{
    PreparedSource, VideoFrame, VideoNode, VideoPresentation, VideoVisual, VisualResources,
    cleanup_visual, prepare_source, present_frame, update_visual,
};
use crate::runtime::platform::DesignViewport;
use crate::runtime::resources::{ContentProjectResource, GameConfigResource, GameState};
use crate::scene::effects::material::{StageMaterial, StageQuad};
use crate::storage::settings::RuntimeSettings;

#[derive(Default)]
pub(super) struct VideoPlayback {
    sessions: HashMap<String, VideoSession>,
    retired_sources: Vec<thread::JoinHandle<()>>,
}

struct VideoSession {
    source_receiver: Receiver<Result<Arc<PreparedSource>, String>>,
    source_worker: Option<thread::JoinHandle<()>>,
    player: Option<NativePlayer>,
    visual: VideoVisual,
    mode: VideoMode,
    muted: bool,
    looped: bool,
    revision: u64,
}

struct NativePlayer {
    _source: Arc<PreparedSource>,
    _asset: Option<Retained<AVURLAsset>>,
    _resource_delegate: Option<Retained<ResourceLoaderDelegate>>,
    _resource_queue: Option<DispatchRetained<DispatchQueue>>,
    player: Retained<AVPlayer>,
    item: Retained<AVPlayerItem>,
    output: Retained<AVPlayerItemVideoOutput>,
    volume: f32,
    muted: bool,
}

struct ResourceLoaderIvars {
    source: Arc<PreparedSource>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements. The only ivar is an
    // immutable, Send + Sync source factory used on one serial dispatch queue.
    #[unsafe(super(NSObject))]
    #[name = "KeineResourceLoaderDelegate"]
    #[ivars = ResourceLoaderIvars]
    struct ResourceLoaderDelegate;

    unsafe impl NSObjectProtocol for ResourceLoaderDelegate {}

    unsafe impl AVAssetResourceLoaderDelegate for ResourceLoaderDelegate {
        #[unsafe(method(resourceLoader:shouldWaitForLoadingOfRequestedResource:))]
        fn should_load(
            &self,
            _resource_loader: &AVAssetResourceLoader,
            request: &AVAssetResourceLoadingRequest,
        ) -> bool {
            if let Err(error) = self.fulfill(request) {
                log::error!("AVFoundation resource read failed: {error}");
                unsafe { request.finishLoadingWithError(None) };
            } else {
                unsafe { request.finishLoading() };
            }
            true
        }
    }
);

impl ResourceLoaderDelegate {
    fn new(source: Arc<PreparedSource>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(ResourceLoaderIvars { source });
        unsafe { msg_send![super(this), init] }
    }

    fn fulfill(&self, request: &AVAssetResourceLoadingRequest) -> Result<(), String> {
        let source = &self.ivars().source;
        if let Some(info) = unsafe { request.contentInformationRequest() } {
            let allowed = unsafe { info.allowedContentTypes() };
            let content_type = allowed
                .as_ref()
                .and_then(|types| types.firstObject())
                .unwrap_or_else(|| NSString::from_str(content_type(source.extension())));
            unsafe {
                info.setContentType(Some(&content_type));
                info.setContentLength(
                    i64::try_from(source.len())
                        .map_err(|_| "video source is too large for AVFoundation".to_owned())?,
                );
                info.setByteRangeAccessSupported(true);
                info.setEntireLengthAvailableOnDemand(true);
            }
        }
        let Some(data_request) = (unsafe { request.dataRequest() }) else {
            return Ok(());
        };
        let offset = unsafe { data_request.currentOffset() };
        let offset = u64::try_from(offset).map_err(|_| "negative video byte offset".to_owned())?;
        let available = source.len().saturating_sub(offset);
        let requested = if unsafe { data_request.requestsAllDataToEndOfResource() } {
            available
        } else {
            u64::try_from(unsafe { data_request.requestedLength() })
                .unwrap_or(0)
                .min(available)
        };
        let mut stream = source.open().map_err(|error| error.to_string())?;
        stream
            .seek(SeekFrom::Start(offset))
            .map_err(|error| error.to_string())?;
        let mut remaining = requested;
        let mut buffer = vec![0; 256 * 1024];
        while remaining > 0 && !unsafe { request.isCancelled() } {
            let chunk = remaining.min(buffer.len() as u64) as usize;
            let read = stream
                .read(&mut buffer[..chunk])
                .map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            let data = NSData::with_bytes(&buffer[..read]);
            unsafe { data_request.respondWithData(&data) };
            remaining -= read as u64;
        }
        Ok(())
    }
}

fn content_type(extension: Option<&str>) -> &'static str {
    match extension.map(str::to_ascii_lowercase).as_deref() {
        Some("mov") => "com.apple.quicktime-movie",
        Some("webm") => "org.webmproject.webm",
        _ => "public.mpeg-4",
    }
}

impl Drop for NativePlayer {
    fn drop(&mut self) {
        // AVFoundation playback is owned by this session and must not outlive
        // its Bevy state entry.
        unsafe { self.player.pause() };
    }
}

#[derive(SystemParam)]
pub(super) struct VideoResources<'w> {
    content: Res<'w, ContentProjectResource>,
    config: Res<'w, GameConfigResource>,
    settings: Res<'w, RuntimeSettings>,
    images: ResMut<'w, Assets<Image>>,
    materials: ResMut<'w, Assets<StageMaterial>>,
    quad: Res<'w, StageQuad>,
}

pub(super) fn sync_video_playback(
    mut commands: Commands,
    mut state: ResMut<GameState>,
    windows: Query<Ref<Window>>,
    mut playback: NonSendMut<VideoPlayback>,
    mut resources: VideoResources,
    mut nodes: Query<
        (
            &MeshMaterial2d<StageMaterial>,
            &mut Transform,
            &mut RenderLayers,
        ),
        With<VideoNode>,
    >,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let viewport = DesignViewport::from_window(&window);
    reap_source_workers(&mut playback.retired_sources);

    let removed = playback
        .sessions
        .keys()
        .filter(|id| {
            state.videos.get(*id).is_none_or(|video| {
                playback
                    .sessions
                    .get(*id)
                    .is_some_and(|session| session.revision != video.revision)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    for id in removed {
        if let Some(mut session) = playback.sessions.remove(&id) {
            cleanup_visual(
                &mut session.visual,
                &mut commands,
                &mut resources.images,
                &mut resources.materials,
            );
            if let Some(worker) = session.source_worker.take() {
                playback.retired_sources.push(worker);
            }
        }
    }

    for (id, video) in &state.videos {
        playback.sessions.entry(id.clone()).or_insert_with(|| {
            spawn_source_worker(
                resources.content.asset_mounts(),
                resources.config.video_path(&video.spec.file),
                video.spec.mode,
                video.spec.muted,
                video.spec.looped,
                video.revision,
            )
        });
    }

    let mut ended = Vec::new();
    for (id, session) in &mut playback.sessions {
        let Some(video) = state.videos.get(id) else {
            continue;
        };

        if session.player.is_none() {
            match session.source_receiver.try_recv() {
                Ok(Ok(source)) => match NativePlayer::open(
                    source,
                    session.muted,
                    resources.settings.master_volume,
                ) {
                    Ok(player) => session.player = Some(player),
                    Err(error) => {
                        log::error!("video `{}` failed: {error}", video.spec.file);
                        ended.push(id.clone());
                    }
                },
                Ok(Err(error)) => {
                    log::error!("video `{}` failed: {error}", video.spec.file);
                    ended.push(id.clone());
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    log::error!("video `{}` source worker disconnected", video.spec.file);
                    ended.push(id.clone());
                }
            }
        }

        let Some(player) = session.player.as_mut() else {
            continue;
        };
        player.sync_audio_settings(resources.settings.master_volume, session.muted);
        match player.next_frame() {
            Ok(Some(frame)) => present_frame(
                id,
                &mut session.visual,
                frame,
                VideoPresentation {
                    mode: session.mode,
                    opacity: video.opacity,
                    viewport,
                },
                &mut commands,
                VisualResources {
                    images: &mut resources.images,
                    materials: &mut resources.materials,
                    quad: &resources.quad,
                },
            ),
            Ok(None) => {}
            Err(error) => {
                log::error!("video `{}` frame output failed: {error}", video.spec.file);
                ended.push(id.clone());
            }
        }
        update_visual(
            &mut session.visual,
            VideoPresentation {
                mode: session.mode,
                opacity: video.opacity,
                viewport,
            },
            &mut resources.materials,
            &mut nodes,
        );

        match player.playback_end() {
            Ok(false) => {}
            Ok(true) if session.looped => player.restart(),
            Ok(true) => ended.push(id.clone()),
            Err(error) => {
                log::error!("video `{}` failed: {error}", video.spec.file);
                ended.push(id.clone());
            }
        }
    }
    for id in ended {
        state.videos.remove(&id);
    }
}

impl NativePlayer {
    fn open(source: Arc<PreparedSource>, muted: bool, volume: f32) -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            "AVFoundation video must be initialized on the main thread".to_owned()
        })?;
        let pixel_format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
        let attributes =
            NSDictionary::from_slices(&[ns_string!("PixelFormatType")], &[&*pixel_format]);
        // Objective-C lightweight generics are erased at runtime. NSNumber is
        // an Objective-C object, so this widens only the Rust dictionary view.
        let attributes: Retained<NSDictionary<NSString, AnyObject>> =
            unsafe { Retained::cast_unchecked(attributes) };

        let output = unsafe {
            AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
                AVPlayerItemVideoOutput::alloc(),
                Some(&attributes),
            )
        };
        unsafe { output.setSuppressesPlayerRendering(true) };
        let (item, asset, resource_delegate, resource_queue) = if let Some(path) =
            source.physical_path()
        {
            let path = NSString::from_str(path.to_string_lossy().as_ref());
            let url = NSURL::fileURLWithPath(&path);
            (
                unsafe { AVPlayerItem::initWithURL(AVPlayerItem::alloc(mtm), &url) },
                None,
                None,
                None,
            )
        } else {
            let url = NSURL::URLWithString(&NSString::from_str(&format!(
                "keine-resource://local/video.{}",
                source.extension().unwrap_or("mp4")
            )))
            .ok_or_else(|| "could not create AVFoundation resource URL".to_owned())?;
            let asset = unsafe { AVURLAsset::initWithURL_options(AVURLAsset::alloc(), &url, None) };
            let resource_delegate = ResourceLoaderDelegate::new(source.clone());
            let resource_queue = DispatchQueue::new("tech.maincore.keine.video-resource", None);
            let delegate = ProtocolObject::from_ref(&*resource_delegate);
            unsafe {
                asset
                    .resourceLoader()
                    .setDelegate_queue(Some(delegate), Some(&resource_queue));
            }
            let item = unsafe { AVPlayerItem::initWithAsset(AVPlayerItem::alloc(mtm), &asset) };
            (
                item,
                Some(asset),
                Some(resource_delegate),
                Some(resource_queue),
            )
        };
        unsafe { item.addOutput(&output) };
        let player = unsafe { AVPlayer::initWithPlayerItem(AVPlayer::alloc(mtm), Some(&item)) };
        unsafe {
            if asset.is_some() {
                player.setAutomaticallyWaitsToMinimizeStalling(false);
            }
            player.setMuted(muted);
            player.setVolume(volume);
            player.play();
        }
        log::info!("video decoder · AVFoundation");
        Ok(Self {
            _source: source,
            _asset: asset,
            _resource_delegate: resource_delegate,
            _resource_queue: resource_queue,
            player,
            item,
            output,
            volume,
            muted,
        })
    }

    fn sync_audio_settings(&mut self, volume: f32, muted: bool) {
        unsafe {
            if self.volume != volume {
                self.player.setVolume(volume);
                self.volume = volume;
            }
            if self.muted != muted {
                self.player.setMuted(muted);
                self.muted = muted;
            }
        }
    }

    fn next_frame(&self) -> Result<Option<VideoFrame>, String> {
        let item_time = unsafe { self.player.currentTime() };
        if !unsafe { self.output.hasNewPixelBufferForItemTime(item_time) } {
            return Ok(None);
        }
        let Some(pixel_buffer) = (unsafe {
            self.output
                .copyPixelBufferForItemTime_itemTimeForDisplay(item_time, ptr::null_mut())
        }) else {
            return Ok(None);
        };
        copy_bgra_frame(&pixel_buffer).map(Some)
    }

    fn playback_end(&self) -> Result<bool, String> {
        let status = unsafe { self.item.status() };
        if status == AVPlayerItemStatus::Failed {
            let detail = unsafe { self.item.error() }.map_or_else(
                || "unknown AVFoundation error".to_owned(),
                |error| format!("{error:?}"),
            );
            return Err(detail);
        }
        if status != AVPlayerItemStatus::ReadyToPlay {
            return Ok(false);
        }
        let duration = unsafe { self.item.duration().seconds() };
        let current = unsafe { self.player.currentTime().seconds() };
        let paused = unsafe { self.player.rate() }.abs() <= f32::EPSILON;
        Ok(duration.is_finite()
            && duration > 0.0
            && current.is_finite()
            && current >= duration - (1.0 / 120.0)
            && paused)
    }

    fn restart(&self) {
        unsafe {
            self.player.seekToTime(objc2_core_media::kCMTimeZero);
            self.player.play();
        }
    }
}

fn copy_bgra_frame(pixel_buffer: &CVPixelBuffer) -> Result<VideoFrame, String> {
    let pixel_format = CVPixelBufferGetPixelFormatType(pixel_buffer);
    if pixel_format != kCVPixelFormatType_32BGRA {
        return Err(format!(
            "AVFoundation returned unsupported pixel format 0x{pixel_format:08x}"
        ));
    }
    let width = CVPixelBufferGetWidth(pixel_buffer);
    let height = CVPixelBufferGetHeight(pixel_buffer);
    let row_bytes = width
        .checked_mul(4)
        .ok_or_else(|| "video row size overflow".to_owned())?;
    let stride = CVPixelBufferGetBytesPerRow(pixel_buffer);
    if stride < row_bytes {
        return Err(format!(
            "AVFoundation pixel stride {stride} is smaller than row size {row_bytes}"
        ));
    }

    let flags = CVPixelBufferLockFlags::ReadOnly;
    let lock_result = unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, flags) };
    if lock_result != kCVReturnSuccess {
        return Err(format!("could not lock CVPixelBuffer: {lock_result}"));
    }
    let copied = (|| {
        let base = CVPixelBufferGetBaseAddress(pixel_buffer).cast::<u8>();
        if base.is_null() {
            return Err("CVPixelBuffer has no CPU base address".to_owned());
        }
        let allocation = stride
            .checked_mul(height)
            .ok_or_else(|| "video frame size overflow".to_owned())?;
        let source = unsafe { std::slice::from_raw_parts(base, allocation) };
        let pixels = copy_packed_rows(source, stride, row_bytes, height)?;
        Ok(VideoFrame {
            width: u32::try_from(width).map_err(|_| "video width exceeds u32".to_owned())?,
            height: u32::try_from(height).map_err(|_| "video height exceeds u32".to_owned())?,
            pixels,
            format: TextureFormat::Bgra8UnormSrgb,
        })
    })();
    let unlock_result = unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, flags) };
    if unlock_result != kCVReturnSuccess {
        return Err(format!("could not unlock CVPixelBuffer: {unlock_result}"));
    }
    copied
}

fn copy_packed_rows(
    source: &[u8],
    stride: usize,
    row_bytes: usize,
    height: usize,
) -> Result<Vec<u8>, String> {
    let source_len = stride
        .checked_mul(height)
        .ok_or_else(|| "video frame size overflow".to_owned())?;
    let packed_len = row_bytes
        .checked_mul(height)
        .ok_or_else(|| "video frame size overflow".to_owned())?;
    if row_bytes > stride || source.len() < source_len {
        return Err("video frame rows do not fit the source buffer".to_owned());
    }
    let mut pixels = Vec::with_capacity(packed_len);
    for row in source[..source_len].chunks(stride) {
        pixels.extend_from_slice(&row[..row_bytes]);
    }
    Ok(pixels)
}

fn spawn_source_worker(
    mounts: Vec<ContentMount>,
    path: String,
    mode: VideoMode,
    muted: bool,
    looped: bool,
    revision: u64,
) -> VideoSession {
    let (sender, receiver) = sync_channel(1);
    let worker_path = path.clone();
    let worker = thread::Builder::new()
        .name(format!("keine-video-source-{path}"))
        .spawn(move || {
            let source = prepare_source(&mounts, Path::new(&worker_path)).map(Arc::new);
            let _ = sender.send(source);
        })
        .unwrap_or_else(|error| {
            log::error!("failed to start video source worker: {error}");
            thread::spawn(|| {})
        });
    VideoSession {
        source_receiver: receiver,
        source_worker: Some(worker),
        player: None,
        visual: VideoVisual::default(),
        mode,
        muted,
        looped,
        revision,
    }
}

fn reap_source_workers(workers: &mut Vec<thread::JoinHandle<()>>) {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let worker = workers.swap_remove(index);
            if worker.join().is_err() {
                log::warn!("video source worker panicked during shutdown");
            }
        } else {
            index += 1;
        }
    }
}

/// Headless backend acceptance used by macOS CI. It must be called from the
/// process main thread so AVFoundation observes the same ownership rules as
/// the engine runtime.
pub(crate) fn validate_native_video(mounts: &[ContentMount], path: &Path) -> Result<(), String> {
    let source = Arc::new(prepare_source(mounts, path)?);
    let player = NativePlayer::open(source, false, 0.0)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let run_loop = NSRunLoop::currentRunLoop();
    while std::time::Instant::now() < deadline {
        if player.next_frame()?.is_some() {
            return Ok(());
        }
        player.playback_end()?;
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
    Err("AVFoundation did not produce a decoded frame within 10 seconds".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_core_video_row_padding_without_reordering_bgra() {
        let source = [1, 2, 3, 4, 99, 99, 5, 6, 7, 8, 88, 88];
        assert_eq!(
            copy_packed_rows(&source, 6, 4, 2).unwrap(),
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
    }

    #[test]
    fn rejects_short_core_video_rows() {
        assert!(copy_packed_rows(&[0; 7], 4, 4, 2).is_err());
    }
}
