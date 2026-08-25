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
use objc2_core_video::{kCVPixelBufferMetalCompatibilityKey, kCVPixelFormatType_32BGRA};
use objc2_foundation::{
    NSData, NSDate, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSRunLoop,
    NSString, NSURL, ns_string,
};

use super::metal_frame::{
    MetalFrameBridge, NativeVideoFrame, present_native_frame, validate_frame_import,
};
use super::shared::{
    PreparedSource, VideoMemoryBudget, VideoMemoryReservation, VideoNode, VideoPresentation,
    VideoVisual, VisualResources, cleanup_visual, prepare_source, update_visual,
};
use crate::runtime::platform::DesignViewport;
use crate::runtime::resources::{ContentProjectResource, GameConfigResource, GameState};
use crate::scene::effects::material::{StageMaterial, StageQuad};
use crate::storage::settings::RuntimeSettings;

pub(super) struct BackendPlugin;

impl Plugin for BackendPlugin {
    fn build(&self, app: &mut App) {
        app.init_non_send::<VideoPlayback>();
        super::metal_frame::install(app);
        app.add_systems(
            Update,
            sync_video_playback.in_set(crate::runtime::GameSystemSet::Sync),
        );
    }
}

#[derive(Default)]
struct VideoPlayback {
    sessions: HashMap<String, VideoSession>,
    retired_sources: Vec<thread::JoinHandle<()>>,
    memory_budget: VideoMemoryBudget,
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
    memory_reservation: VideoMemoryReservation,
}

struct ResourceLoaderIvars {
    source: Arc<PreparedSource>,
}

enum ResourceLoadOutcome {
    Completed,
    Cancelled,
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
            match self.fulfill(request) {
                Ok(ResourceLoadOutcome::Completed) => unsafe { request.finishLoading() },
                Ok(ResourceLoadOutcome::Cancelled) => {}
                Err(error) => {
                    log::error!("AVFoundation resource read failed: {error}");
                    let error = NSError::new(1, ns_string!("moe.maincore.keine.video-source"));
                    unsafe { request.finishLoadingWithError(Some(&error)) };
                }
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

    fn fulfill(
        &self,
        request: &AVAssetResourceLoadingRequest,
    ) -> Result<ResourceLoadOutcome, String> {
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
            return Ok(ResourceLoadOutcome::Completed);
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
                return Err(format!(
                    "video source ended with {remaining} requested bytes remaining"
                ));
            }
            let data = NSData::with_bytes(&buffer[..read]);
            unsafe { data_request.respondWithData(&data) };
            remaining -= read as u64;
        }
        if unsafe { request.isCancelled() } {
            Ok(ResourceLoadOutcome::Cancelled)
        } else {
            Ok(ResourceLoadOutcome::Completed)
        }
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
struct VideoResources<'w> {
    content: Res<'w, ContentProjectResource>,
    config: Res<'w, GameConfigResource>,
    settings: Res<'w, RuntimeSettings>,
    images: ResMut<'w, Assets<Image>>,
    materials: ResMut<'w, Assets<StageMaterial>>,
    quad: Res<'w, StageQuad>,
    frame_bridge: Res<'w, MetalFrameBridge>,
}

fn sync_video_playback(
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
            if let Some(image) = session.visual.image.as_ref() {
                resources.frame_bridge.discard(image.id());
            }
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

    let memory_budget = playback.memory_budget.clone();
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
                    memory_budget.reservation(),
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
                    log::error!("video `{}` I/O worker disconnected", video.spec.file);
                    ended.push(id.clone());
                }
            }
        }

        let Some(player) = session.player.as_mut() else {
            continue;
        };
        player.sync_audio_settings(resources.settings.master_volume, session.muted);
        match player.next_frame() {
            Ok(Some(frame)) => present_native_frame(
                id,
                &mut session.visual,
                frame,
                VideoPresentation {
                    mode: session.mode,
                    opacity: video.opacity,
                    viewport,
                },
                &resources.frame_bridge,
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
    fn open(
        source: Arc<PreparedSource>,
        muted: bool,
        volume: f32,
        memory_reservation: VideoMemoryReservation,
    ) -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            "AVFoundation video must be initialized on the main thread".to_owned()
        })?;
        let pixel_format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
        let metal_compatible = NSNumber::new_bool(true);
        // Core Foundation strings and NSString are toll-free bridged. Use the
        // framework constant rather than depending on its private string value.
        let metal_compatibility_key: &NSString =
            unsafe { &*std::ptr::from_ref(kCVPixelBufferMetalCompatibilityKey).cast::<NSString>() };
        let attributes = NSDictionary::from_slices(
            &[ns_string!("PixelFormatType"), metal_compatibility_key],
            &[&*pixel_format, &*metal_compatible],
        );
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
                "keine-video://local/stream.{}",
                source.extension().unwrap_or("mp4")
            )))
            .ok_or_else(|| "could not create AVFoundation resource URL".to_owned())?;
            let asset = unsafe { AVURLAsset::initWithURL_options(AVURLAsset::alloc(), &url, None) };
            let resource_delegate = ResourceLoaderDelegate::new(source.clone());
            let resource_queue = DispatchQueue::new("moe.maincore.keine.video-io", None);
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
            memory_reservation,
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

    fn next_frame(&mut self) -> Result<Option<NativeVideoFrame>, String> {
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
        let frame = NativeVideoFrame::new(pixel_buffer)?;
        let (width, height) = frame.dimensions();
        self.memory_reservation.reserve_frame(width, height)?;
        Ok(Some(frame))
    }

    fn playback_end(&self) -> Result<bool, String> {
        let status = unsafe { self.item.status() };
        if status == AVPlayerItemStatus::Failed {
            let detail = unsafe { self.item.error() }.map_or_else(
                || "unknown AVFoundation error".to_owned(),
                |error| {
                    let reason = error.localizedFailureReason().map_or_else(
                        || "no failure reason".to_owned(),
                        |reason| reason.to_string(),
                    );
                    format!(
                        "{} (domain={}, code={}, reason={reason})",
                        error.localizedDescription(),
                        error.domain(),
                        error.code()
                    )
                },
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
        .name("keine-video-io".into())
        .spawn(move || {
            let source = prepare_source(&mounts, Path::new(&worker_path)).map(Arc::new);
            let _ = sender.send(source);
        })
        .unwrap_or_else(|error| {
            log::error!("failed to start video I/O worker: {error}");
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
                log::warn!("video I/O worker panicked during shutdown");
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
    let mut player = NativePlayer::open(
        source,
        true,
        0.0,
        VideoMemoryBudget::default().reservation(),
    )?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let run_loop = NSRunLoop::currentRunLoop();
    let mut decoded_first_cycle = false;
    let mut restarted = false;
    while std::time::Instant::now() < deadline {
        if let Some(frame) = player.next_frame()? {
            if restarted {
                validate_frame_import(frame)?;
                return Ok(());
            }
            if !decoded_first_cycle {
                validate_frame_import(frame)?;
                decoded_first_cycle = true;
            }
        }
        let ended = player.playback_end().map_err(|error| {
            format!(
                "AVFoundation failed after first_frame={decoded_first_cycle}, rewound={restarted}: {error}"
            )
        })?;
        if ended && !restarted {
            if !decoded_first_cycle {
                return Err("AVFoundation reached EOF without a decoded frame".to_owned());
            }
            player.restart();
            restarted = true;
        }
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
    Err("AVFoundation did not complete decode and rewind within 20 seconds".to_owned())
}
