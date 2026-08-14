#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
use std::collections::HashMap;

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

#[cfg(any(
    test,
    all(
        feature = "video-ffmpeg",
        not(all(feature = "video-native", target_os = "macos"))
    )
))]
fn send_cancellable<T>(
    sender: &crossbeam_channel::Sender<T>,
    event: T,
    cancelled: &std::sync::atomic::AtomicBool,
    cancel_receiver: &crossbeam_channel::Receiver<()>,
) -> bool {
    use std::sync::atomic::Ordering;

    if cancelled.load(Ordering::Acquire) {
        return false;
    }
    // Prefer cancellation if queue capacity and shutdown become ready at the
    // same instant. Unlike timed try_send polling, this parks the producer
    // without periodic wakeups while the bounded queue is full.
    crossbeam_channel::select_biased! {
        recv(cancel_receiver) -> _ => false,
        send(sender, event) -> result => result.is_ok(),
    }
}

#[cfg(test)]
mod cancellable_channel_tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    use crossbeam_channel::bounded;

    use super::send_cancellable;

    #[test]
    fn full_queue_wakes_immediately_for_cancellation() {
        let (sender, _receiver) = bounded(1);
        sender.send(0_u8).unwrap();
        let (cancel_sender, cancel_receiver) = bounded(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let started = Arc::new(Barrier::new(2));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_started = Arc::clone(&started);
        let worker = thread::spawn(move || {
            worker_started.wait();
            send_cancellable(&sender, 1, &worker_cancelled, &cancel_receiver)
        });

        started.wait();
        cancelled.store(true, Ordering::Release);
        cancel_sender.send(()).unwrap();

        assert!(!worker.join().unwrap());
    }

    #[test]
    fn full_queue_resumes_when_the_consumer_frees_capacity() {
        let (sender, receiver) = bounded(1);
        sender.send(0_u8).unwrap();
        let (_cancel_sender, cancel_receiver) = bounded(1);
        let cancelled = AtomicBool::new(false);
        let worker =
            thread::spawn(move || send_cancellable(&sender, 1, &cancelled, &cancel_receiver));

        assert_eq!(receiver.recv().unwrap(), 0);
        assert!(worker.join().unwrap());
        assert_eq!(receiver.recv().unwrap(), 1);
    }
}

// On macOS the native backend wins when both developer features are enabled,
// but Cargo still links the optional FFmpeg dependency. Keep that intentional
// dependency visible to the workspace's unused-crate lint.
#[cfg(all(
    feature = "video-ffmpeg",
    feature = "video-native",
    target_os = "macos"
))]
use ffmpeg_next as _;

#[cfg(any(
    feature = "video-ffmpeg",
    all(feature = "video-native", target_os = "macos")
))]
#[path = "video/shared.rs"]
mod shared;

#[cfg(all(feature = "video-native", target_os = "macos"))]
#[path = "video/avfoundation.rs"]
mod avfoundation_backend;

#[cfg(all(feature = "video-native", target_os = "macos"))]
#[path = "video/metal_frame.rs"]
mod metal_frame;

#[cfg(all(feature = "video-native", target_os = "macos"))]
pub(crate) use avfoundation_backend::validate_native_video;

#[cfg(all(feature = "video-native", target_os = "macos"))]
use avfoundation_backend::BackendPlugin as SelectedVideoBackendPlugin;

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
use ffmpeg_backend::BackendPlugin as SelectedVideoBackendPlugin;

#[cfg(not(any(
    feature = "video-ffmpeg",
    all(feature = "video-native", target_os = "macos")
)))]
use missing_backend::BackendPlugin as SelectedVideoBackendPlugin;

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
#[path = "video/ffmpeg_io.rs"]
mod ffmpeg_io;

pub(crate) struct VideoPlugin;

impl Plugin for VideoPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SelectedVideoBackendPlugin);
    }
}

#[cfg(not(any(
    feature = "video-ffmpeg",
    all(feature = "video-native", target_os = "macos")
)))]
mod missing_backend {
    use super::*;
    use crate::runtime::resources::GameState;

    pub(super) struct BackendPlugin;

    impl Plugin for BackendPlugin {
        fn build(&self, app: &mut App) {
            app.init_resource::<MissingVideoBackend>().add_systems(
                Update,
                reject_unavailable_video.in_set(crate::runtime::GameSystemSet::Sync),
            );
        }
    }

    #[derive(Resource, Default)]
    struct MissingVideoBackend(bool);

    fn reject_unavailable_video(
        mut state: ResMut<GameState>,
        mut warned: ResMut<MissingVideoBackend>,
    ) {
        if state.videos.is_empty() {
            return;
        }
        if !warned.0 {
            warned.0 = true;
            log::error!(
                "video playback was requested, but this binary has no video backend for this platform"
            );
        }
        state.videos.clear();
    }
}

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
mod ffmpeg_backend {
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use bevy::audio::{
        AudioPlayer, AudioSink, AudioSinkPlayback, Decodable, PlaybackMode, PlaybackSettings,
        Volume,
    };
    use bevy::ecs::system::SystemParam;
    use bevy::prelude::*;
    use bevy::render::render_resource::TextureFormat;
    use crossbeam_channel::{Receiver, Sender, TryRecvError, bounded};
    use ffmpeg_next as ffmpeg;
    use ffmpeg_next::software::scaling::{context::Context as VideoScaler, flag::Flags};
    use keine_core::VideoMode;
    use keine_loader::ContentMount;
    use rodio::{ChannelCount, SampleRate, Source};

    use super::ffmpeg_io::{MediaInput, VideoDecoder};
    use super::shared::{
        PreparedSource, VideoFrame, VideoMemoryBudget, VideoMemoryReservation, VideoNode,
        VideoPresentation, VideoVisual, VisualResources, cleanup_visual, prepare_source,
        present_frame, update_visual, video_frame_bytes,
    };
    use super::{HashMap, RenderLayers};
    use crate::runtime::platform::DesignViewport;
    use crate::runtime::resources::{ContentProjectResource, GameConfigResource, GameState};
    use crate::scene::effects::material::{StageMaterial, StageQuad};
    use crate::storage::settings::RuntimeSettings;

    pub(super) struct BackendPlugin;

    impl Plugin for BackendPlugin {
        fn build(&self, app: &mut App) {
            use bevy::audio::AddAudioSource;

            if let Err(error) = ffmpeg::init() {
                log::error!("failed to initialize FFmpeg video backend: {error}");
            }
            app.init_resource::<VideoPlayback>()
                .init_asset::<FfmpegVideoAudio>()
                .add_audio_source::<FfmpegVideoAudio>()
                .add_systems(
                    Update,
                    sync_video_playback.in_set(crate::runtime::GameSystemSet::Sync),
                );
        }
    }

    #[derive(Resource, Default)]
    struct VideoPlayback {
        sessions: HashMap<String, VideoSession>,
        retired_decoders: Vec<thread::JoinHandle<()>>,
        memory_budget: VideoMemoryBudget,
    }

    struct VideoSession {
        receiver: Mutex<Receiver<DecoderEvent>>,
        cancelled: Arc<AtomicBool>,
        cancel_sender: Sender<()>,
        decoder: Option<thread::JoinHandle<()>>,
        pending: Option<DecodedFrame>,
        visual: VideoVisual,
        audio_entity: Option<Entity>,
        audio_asset: Option<Handle<FfmpegVideoAudio>>,
        source: Option<Arc<PreparedSource>>,
        start_elapsed: Option<f32>,
        has_audio: bool,
        audio_started: bool,
        audio_position: f32,
        audio_state_elapsed: f32,
        audio_volume: f32,
        mode: VideoMode,
        muted: bool,
        revision: u64,
    }

    impl Drop for VideoSession {
        fn drop(&mut self) {
            self.cancel();
        }
    }

    impl VideoSession {
        fn cancel(&self) {
            self.cancelled.store(true, Ordering::Release);
            // The channel wakes a decoder blocked on a full frame queue. It is
            // bounded because cancellation is a level-triggered state; one
            // pending notification is sufficient.
            let _ = self.cancel_sender.try_send(());
        }
    }

    enum DecoderEvent {
        Ready {
            source: Arc<PreparedSource>,
            has_audio: bool,
        },
        Frame(DecodedFrame),
        End,
        Error(String),
    }

    struct DecodedFrame {
        timestamp: f32,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    }

    struct RgbaConverter {
        scaler: VideoScaler,
        target: ffmpeg::frame::Video,
    }

    impl RgbaConverter {
        fn new(source: &ffmpeg::frame::Video) -> Result<Self, String> {
            let width = source.width();
            let height = source.height();
            Ok(Self {
                scaler: VideoScaler::get(
                    source.format(),
                    width,
                    height,
                    ffmpeg::format::Pixel::RGBA,
                    width,
                    height,
                    Flags::FAST_BILINEAR,
                )
                .map_err(|error| error.to_string())?,
                target: ffmpeg::frame::Video::empty(),
            })
        }

        fn accepts(&self, source: &ffmpeg::frame::Video) -> bool {
            let input = self.scaler.input();
            input.format == source.format()
                && input.width == source.width()
                && input.height == source.height()
        }
    }

    #[derive(Asset, TypePath, Clone, Debug)]
    struct FfmpegVideoAudio {
        source: Arc<PreparedSource>,
        looped: bool,
    }

    impl Decodable for FfmpegVideoAudio {
        type Decoder = FfmpegAudioStream;

        fn decoder(&self) -> Self::Decoder {
            FfmpegAudioStream::open(self.source.clone(), self.looped).unwrap_or_else(|error| {
                log::warn!("video audio track is unavailable: {error}");
                FfmpegAudioStream::failed(self.looped)
            })
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
        audio: ResMut<'w, Assets<FfmpegVideoAudio>>,
    }

    fn sync_video_playback(
        mut commands: Commands,
        mut state: ResMut<GameState>,
        windows: Query<Ref<Window>>,
        mut playback: ResMut<VideoPlayback>,
        mut resources: VideoResources,
        mut audio_sinks: Query<&mut AudioSink>,
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
        reap_decoders(&mut playback.retired_decoders);

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
            if let Some(session) = playback.sessions.remove(&id)
                && let Some(decoder) = cleanup_session(
                    session,
                    &mut commands,
                    &mut resources.images,
                    &mut resources.materials,
                    &mut resources.audio,
                )
            {
                playback.retired_decoders.push(decoder);
            }
        }

        let memory_budget = playback.memory_budget.clone();
        for (id, video) in &state.videos {
            playback.sessions.entry(id.clone()).or_insert_with(|| {
                spawn_decoder(
                    resources.content.asset_mounts(),
                    resources.config.video_path(&video.spec.file),
                    video.spec.looped,
                    video.spec.mode,
                    video.spec.muted,
                    video.revision,
                    memory_budget.reservation(),
                )
            });
        }

        let mut ended = Vec::new();
        for (id, session) in &mut playback.sessions {
            let Some(video) = state.videos.get(id) else {
                continue;
            };
            let mut newest = None;
            if let Some(frame) = session.pending.take() {
                if playback_clock(
                    session,
                    video.elapsed,
                    resources.settings.master_volume,
                    &mut audio_sinks,
                )
                .is_some_and(|elapsed| frame.timestamp <= elapsed)
                {
                    newest = Some(frame);
                } else {
                    session.pending = Some(frame);
                }
            }
            while session.pending.is_none() {
                let event = session
                    .receiver
                    .lock()
                    .map_or(Err(TryRecvError::Disconnected), |receiver| {
                        receiver.try_recv()
                    });
                match event {
                    Ok(DecoderEvent::Ready { source, has_audio }) => {
                        session.start_elapsed.get_or_insert(video.elapsed);
                        session.has_audio = has_audio;
                        if has_audio && !session.muted && session.audio_entity.is_none() {
                            let asset = resources.audio.add(FfmpegVideoAudio {
                                source: source.clone(),
                                looped: video.spec.looped,
                            });
                            let entity = commands
                                .spawn((
                                    Name::new(format!("video-audio::{id}")),
                                    AudioPlayer(asset.clone()),
                                    PlaybackSettings {
                                        // The FFmpeg source handles looping by
                                        // seeking its random-access input.
                                        // Rodio's generic loop buffers the
                                        // complete decoded PCM stream.
                                        mode: PlaybackMode::Despawn,
                                        volume: Volume::Linear(resources.settings.master_volume),
                                        ..default()
                                    },
                                ))
                                .id();
                            session.audio_entity = Some(entity);
                            session.audio_asset = Some(asset);
                        }
                        session.source = Some(source);
                    }
                    Ok(DecoderEvent::Frame(frame)) => {
                        let elapsed = playback_clock(
                            session,
                            video.elapsed,
                            resources.settings.master_volume,
                            &mut audio_sinks,
                        );
                        if elapsed.is_some_and(|elapsed| frame.timestamp <= elapsed) {
                            newest = Some(frame);
                        } else {
                            session.pending = Some(frame);
                            break;
                        }
                    }
                    Ok(DecoderEvent::End) => {
                        ended.push(id.clone());
                        break;
                    }
                    Ok(DecoderEvent::Error(error)) => {
                        log::error!("video `{}` failed: {error}", video.spec.file);
                        ended.push(id.clone());
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        ended.push(id.clone());
                        break;
                    }
                }
            }
            if let Some(frame) = newest {
                present_frame(
                    id,
                    &mut session.visual,
                    VideoFrame {
                        width: frame.width,
                        height: frame.height,
                        pixels: frame.rgba,
                        format: TextureFormat::Rgba8UnormSrgb,
                    },
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
                );
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
        }
        for id in ended {
            state.videos.remove(&id);
        }
    }

    fn cleanup_session(
        mut session: VideoSession,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        materials: &mut Assets<StageMaterial>,
        audio_assets: &mut Assets<FfmpegVideoAudio>,
    ) -> Option<thread::JoinHandle<()>> {
        session.cancel();
        cleanup_visual(&mut session.visual, commands, images, materials);
        if let Some(entity) = session.audio_entity {
            commands.entity(entity).try_despawn();
        }
        if let Some(audio) = session.audio_asset.take() {
            audio_assets.remove(audio.id());
        }
        session.decoder.take()
    }

    fn reap_decoders(decoders: &mut Vec<thread::JoinHandle<()>>) {
        let mut index = 0;
        while index < decoders.len() {
            if decoders[index].is_finished() {
                let decoder = decoders.swap_remove(index);
                if decoder.join().is_err() {
                    log::warn!("video decoder thread panicked during shutdown");
                }
            } else {
                index += 1;
            }
        }
    }

    fn spawn_decoder(
        mounts: Vec<ContentMount>,
        path: String,
        looped: bool,
        mode: VideoMode,
        muted: bool,
        revision: u64,
        memory_reservation: VideoMemoryReservation,
    ) -> VideoSession {
        // Two frames cover normal decoder jitter without retaining another
        // 8 MiB 1080p RGBA allocation or adding a visible frame of latency.
        let (sender, receiver) = bounded(2);
        let (cancel_sender, cancel_receiver) = bounded(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let thread_cancelled = cancelled.clone();
        let decoder = thread::Builder::new()
            .name("keine-video-decode".into())
            .spawn(move || {
                decode_video(
                    mounts,
                    &path,
                    looped,
                    thread_cancelled,
                    sender,
                    cancel_receiver,
                    memory_reservation,
                )
            })
            .unwrap_or_else(|error| {
                log::error!("failed to start video decoder thread: {error}");
                thread::spawn(|| {})
            });
        VideoSession {
            receiver: Mutex::new(receiver),
            cancelled,
            cancel_sender,
            decoder: Some(decoder),
            pending: None,
            visual: VideoVisual::default(),
            audio_entity: None,
            audio_asset: None,
            source: None,
            start_elapsed: None,
            has_audio: false,
            audio_started: false,
            audio_position: 0.0,
            audio_state_elapsed: 0.0,
            audio_volume: f32::NAN,
            mode,
            muted,
            revision,
        }
    }

    fn playback_elapsed(state_elapsed: f32, start_elapsed: Option<f32>) -> f32 {
        start_elapsed.map_or(0.0, |start| (state_elapsed - start).max(0.0))
    }

    fn playback_clock(
        session: &mut VideoSession,
        state_elapsed: f32,
        master_volume: f32,
        audio_sinks: &mut Query<&mut AudioSink>,
    ) -> Option<f32> {
        if session.muted || !session.has_audio {
            return Some(playback_elapsed(state_elapsed, session.start_elapsed));
        }
        let entity = session.audio_entity?;
        if let Ok(mut sink) = audio_sinks.get_mut(entity) {
            if session.audio_volume != master_volume {
                sink.set_volume(Volume::Linear(master_volume));
                session.audio_volume = master_volume;
            }
            session.audio_started = true;
            session.audio_position = sink.position().as_secs_f32();
            session.audio_state_elapsed = state_elapsed;
            return Some(session.audio_position);
        }
        session.audio_started.then(|| {
            fallback_audio_clock(
                session.audio_position,
                session.audio_state_elapsed,
                state_elapsed,
            )
        })
    }

    fn fallback_audio_clock(
        last_audio_position: f32,
        last_state_elapsed: f32,
        state_elapsed: f32,
    ) -> f32 {
        last_audio_position + (state_elapsed - last_state_elapsed).max(0.0)
    }

    fn decode_video(
        mounts: Vec<ContentMount>,
        logical_path: &str,
        looped: bool,
        cancelled: Arc<AtomicBool>,
        sender: Sender<DecoderEvent>,
        cancel_receiver: Receiver<()>,
        mut memory_reservation: VideoMemoryReservation,
    ) {
        let source = match prepare_source(&mounts, Path::new(logical_path)) {
            Ok(source) => Arc::new(source),
            Err(error) => {
                let _ = send_event(
                    &sender,
                    DecoderEvent::Error(error),
                    &cancelled,
                    &cancel_receiver,
                );
                return;
            }
        };
        let mut decoder = match open_decoder(&source) {
            Ok(decoder) => decoder,
            Err(error) => {
                let _ = send_event(
                    &sender,
                    DecoderEvent::Error(error.to_string()),
                    &cancelled,
                    &cancel_receiver,
                );
                return;
            }
        };
        let duration = decoder.duration();
        let has_audio = decoder.has_audio();
        if !send_event(
            &sender,
            DecoderEvent::Ready { source, has_audio },
            &cancelled,
            &cancel_receiver,
        ) {
            return;
        }
        let mut loop_offset = 0.0;
        let mut rgba_converter = None;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            match decoder.decode_raw() {
                Ok(Some(frame)) => {
                    let timestamp = frame.timestamp().map_or(0.0, |timestamp| {
                        timestamp as f64 * f64::from(decoder.time_base().numerator())
                            / f64::from(decoder.time_base().denominator())
                    }) as f32;
                    let width = frame.width();
                    let height = frame.height();
                    if let Err(error) = memory_reservation.reserve_frame(width, height) {
                        let _ = send_event(
                            &sender,
                            DecoderEvent::Error(error),
                            &cancelled,
                            &cancel_receiver,
                        );
                        return;
                    }
                    let rgba = convert_to_rgba(&frame, &mut rgba_converter);
                    let rgba = match rgba {
                        Ok(rgba) => rgba,
                        Err(error) => {
                            let _ = send_event(
                                &sender,
                                DecoderEvent::Error(error),
                                &cancelled,
                                &cancel_receiver,
                            );
                            return;
                        }
                    };
                    let frame = DecodedFrame {
                        timestamp: loop_offset + timestamp.max(0.0),
                        width,
                        height,
                        rgba,
                    };
                    if !send_event(
                        &sender,
                        DecoderEvent::Frame(frame),
                        &cancelled,
                        &cancel_receiver,
                    ) {
                        return;
                    }
                }
                Ok(None) if looped => {
                    loop_offset += duration;
                    if let Err(error) = decoder.seek_to_start() {
                        let _ = send_event(
                            &sender,
                            DecoderEvent::Error(error.to_string()),
                            &cancelled,
                            &cancel_receiver,
                        );
                        return;
                    }
                }
                Ok(None) => {
                    let _ = send_event(&sender, DecoderEvent::End, &cancelled, &cancel_receiver);
                    return;
                }
                Err(error) => {
                    let _ = send_event(
                        &sender,
                        DecoderEvent::Error(error.to_string()),
                        &cancelled,
                        &cancel_receiver,
                    );
                    return;
                }
            }
        }
    }

    fn open_decoder(source: &Arc<PreparedSource>) -> Result<VideoDecoder, ffmpeg::Error> {
        log::info!("video decoder · software");
        VideoDecoder::open(source)
    }

    fn convert_to_rgba(
        source: &ffmpeg::frame::Video,
        converter: &mut Option<RgbaConverter>,
    ) -> Result<Vec<u8>, String> {
        let width = source.width();
        let height = source.height();
        let (row_bytes, row_count, frame_bytes) = video_frame_layout(width, height)?;
        if converter
            .as_ref()
            .is_none_or(|converter| !converter.accepts(source))
        {
            *converter = Some(RgbaConverter::new(source)?);
        }
        let converter = converter
            .as_mut()
            .ok_or_else(|| "FFmpeg RGBA converter was not initialized".to_owned())?;
        converter
            .scaler
            .run(source, &mut converter.target)
            .map_err(|error| error.to_string())?;

        let stride = converter.target.stride(0);
        let data = converter.target.data(0);
        if stride < row_bytes {
            return Err(format!(
                "FFmpeg RGBA stride {stride} is smaller than row width {row_bytes}"
            ));
        }
        let data_bytes = stride
            .checked_mul(row_count)
            .ok_or_else(|| "FFmpeg RGBA plane size overflow".to_owned())?;
        if data.len() < data_bytes {
            return Err(format!(
                "FFmpeg RGBA plane is truncated: {} < {data_bytes}",
                data.len()
            ));
        }

        let mut rgba = Vec::new();
        rgba.try_reserve_exact(frame_bytes)
            .map_err(|error| format!("failed to reserve FFmpeg RGBA frame: {error}"))?;
        if stride == row_bytes {
            rgba.extend_from_slice(
                data.get(..frame_bytes)
                    .ok_or_else(|| "FFmpeg RGBA frame is truncated".to_owned())?,
            );
            return Ok(rgba);
        }

        for row in 0..row_count {
            let start = row
                .checked_mul(stride)
                .ok_or_else(|| "FFmpeg RGBA row offset overflow".to_owned())?;
            let end = start
                .checked_add(row_bytes)
                .ok_or_else(|| "FFmpeg RGBA row size overflow".to_owned())?;
            rgba.extend_from_slice(
                data.get(start..end)
                    .ok_or_else(|| "FFmpeg RGBA row is truncated".to_owned())?,
            );
        }
        Ok(rgba)
    }

    fn video_frame_layout(width: u32, height: u32) -> Result<(usize, usize, usize), String> {
        let frame_bytes = video_frame_bytes(width, height)?;
        let width = usize::try_from(width)
            .map_err(|_| "video frame width exceeds this platform".to_owned())?;
        let height = usize::try_from(height)
            .map_err(|_| "video frame height exceeds this platform".to_owned())?;
        let row_bytes = width
            .checked_mul(4)
            .ok_or_else(|| "video frame row size overflow".to_owned())?;
        debug_assert_eq!(row_bytes.checked_mul(height), Some(frame_bytes));
        Ok((row_bytes, height, frame_bytes))
    }

    /// Headless decoder acceptance used by CI and for checking arbitrary local
    /// media without turning machine-specific fixtures into ignored tests.
    pub(crate) fn validate_ffmpeg_video(
        mounts: &[ContentMount],
        path: &Path,
    ) -> Result<(), String> {
        ffmpeg::init().map_err(|error| error.to_string())?;
        let source = Arc::new(prepare_source(mounts, path)?);
        let mut decoder = open_decoder(&source).map_err(|error| error.to_string())?;
        let has_audio = decoder.has_audio();
        let mut converter = None;
        let mut decoded_frames = 0;
        while decoded_frames < 60 {
            let Some(frame) = decoder.decode_raw().map_err(|error| error.to_string())? else {
                break;
            };
            let (_, _, expected_bytes) = video_frame_layout(frame.width(), frame.height())?;
            let rgba = convert_to_rgba(&frame, &mut converter)?;
            if rgba.len() != expected_bytes {
                return Err(format!(
                    "FFmpeg produced {} RGBA bytes for a {}-byte frame",
                    rgba.len(),
                    expected_bytes
                ));
            }
            decoded_frames += 1;
        }
        if decoded_frames == 0 {
            return Err("FFmpeg did not produce a decoded video frame".to_owned());
        }
        if has_audio {
            let samples = FfmpegAudioStream::open(source, false)
                .map_err(|error| error.to_string())?
                .take(4_096)
                .count();
            if samples == 0 {
                return Err("FFmpeg did not produce decoded audio samples".to_owned());
            }
        }
        Ok(())
    }

    fn send_event(
        sender: &Sender<DecoderEvent>,
        event: DecoderEvent,
        cancelled: &AtomicBool,
        cancel_receiver: &Receiver<()>,
    ) -> bool {
        super::send_cancellable(sender, event, cancelled, cancel_receiver)
    }

    struct FfmpegAudioStream {
        input: Option<MediaInput>,
        decoder: Option<ffmpeg::decoder::Audio>,
        resampler: Option<ffmpeg::software::resampling::Context>,
        stream_index: usize,
        stream_time_base: ffmpeg::Rational,
        stream_start_time: i64,
        seek_target: Option<i64>,
        samples: Vec<f32>,
        position: usize,
        sample_rate: SampleRate,
        duration: Option<Duration>,
        looped: bool,
        eof_sent: bool,
        ended: bool,
    }

    impl FfmpegAudioStream {
        fn open(source: Arc<PreparedSource>, looped: bool) -> Result<Self, ffmpeg::Error> {
            let input = MediaInput::open(&source)?;
            let stream = input
                .streams()
                .best(ffmpeg::media::Type::Audio)
                .ok_or(ffmpeg::Error::StreamNotFound)?;
            let stream_index = stream.index();
            let stream_time_base = stream.time_base();
            let stream_start_time = match stream.start_time() {
                ffmpeg::ffi::AV_NOPTS_VALUE => 0,
                value => value,
            };
            let duration = (stream.duration() > 0).then(|| {
                let base = stream.time_base();
                Duration::from_secs_f64(
                    stream.duration() as f64 * f64::from(base.numerator())
                        / f64::from(base.denominator()),
                )
            });
            let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
            let decoder = context.decoder().audio()?;
            let rate = decoder.rate().max(1);
            let resampler = audio_resampler(&decoder)?;
            Ok(Self {
                input: Some(input),
                decoder: Some(decoder),
                resampler: Some(resampler),
                stream_index,
                stream_time_base,
                stream_start_time,
                seek_target: None,
                samples: Vec::new(),
                position: 0,
                sample_rate: SampleRate::new(rate).unwrap_or(SampleRate::MIN),
                duration,
                looped,
                eof_sent: false,
                ended: false,
            })
        }

        fn failed(looped: bool) -> Self {
            Self {
                input: None,
                decoder: None,
                resampler: None,
                stream_index: 0,
                stream_time_base: ffmpeg::Rational(1, 1),
                stream_start_time: 0,
                seek_target: None,
                samples: Vec::new(),
                position: 0,
                sample_rate: SampleRate::new(48_000).unwrap_or(SampleRate::MIN),
                duration: None,
                looped,
                eof_sent: true,
                ended: true,
            }
        }

        fn receive_frames(&mut self) -> Result<bool, ffmpeg::Error> {
            let Some(decoder) = self.decoder.as_mut() else {
                return Ok(false);
            };
            let Some(resampler) = self.resampler.as_mut() else {
                return Ok(false);
            };
            let mut received = false;
            let mut decoded = ffmpeg::frame::Audio::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                let frame_timestamp = decoded.timestamp();
                let mut converted = ffmpeg::frame::Audio::empty();
                resampler.run(&decoded, &mut converted)?;
                self.samples.clear();
                self.position = 0;
                let stereo = converted.plane::<(f32, f32)>(0);
                let skip_frames = self.seek_target.map_or(0, |target| {
                    let skip = seek_preroll_frames(
                        frame_timestamp,
                        target,
                        self.stream_time_base,
                        self.sample_rate.get(),
                        stereo.len(),
                    );
                    if skip < stereo.len() || frame_timestamp.is_none() {
                        self.seek_target = None;
                    }
                    skip
                });
                for &(left, right) in &stereo[skip_frames..] {
                    self.samples.extend_from_slice(&[left, right]);
                }
                if !self.samples.is_empty() {
                    received = true;
                    break;
                }
            }
            Ok(received)
        }

        fn decode_next(&mut self) -> bool {
            loop {
                if self.receive_frames().unwrap_or(false) {
                    return true;
                }
                if self.eof_sent {
                    if self.looped {
                        if self.seek_to(Duration::ZERO).is_ok() {
                            continue;
                        }
                    }
                    self.ended = true;
                    return false;
                }
                let packet = self.input.as_mut().and_then(|input| {
                    input
                        .packets()
                        .find(|(stream, _)| stream.index() == self.stream_index)
                        .map(|(_, packet)| packet)
                });
                if let Some(packet) = packet {
                    if self
                        .decoder
                        .as_mut()
                        .is_none_or(|decoder| decoder.send_packet(&packet).is_err())
                    {
                        self.ended = true;
                        return false;
                    }
                } else {
                    self.eof_sent = true;
                    if self
                        .decoder
                        .as_mut()
                        .is_none_or(|decoder| decoder.send_eof().is_err())
                    {
                        self.ended = true;
                        return false;
                    }
                }
            }
        }

        fn seek_to(&mut self, position: Duration) -> Result<(), ffmpeg::Error> {
            let timestamp = duration_to_stream_timestamp(
                position,
                self.stream_time_base,
                self.stream_start_time,
            )?;
            let input = self.input.as_mut().ok_or(ffmpeg::Error::InvalidData)?;
            input.seek_stream_before(self.stream_index, timestamp)?;
            let decoder = self.decoder.as_mut().ok_or(ffmpeg::Error::InvalidData)?;
            decoder.flush();
            self.resampler = Some(audio_resampler(decoder)?);
            self.samples.clear();
            self.position = 0;
            self.seek_target = Some(timestamp);
            self.eof_sent = false;
            self.ended = false;
            Ok(())
        }
    }

    impl Iterator for FfmpegAudioStream {
        type Item = f32;

        fn next(&mut self) -> Option<Self::Item> {
            loop {
                if let Some(sample) = self.samples.get(self.position).copied() {
                    self.position += 1;
                    return Some(sample);
                }
                if self.ended || !self.decode_next() {
                    return None;
                }
            }
        }
    }

    impl Source for FfmpegAudioStream {
        fn current_span_len(&self) -> Option<usize> {
            self.ended.then_some(0)
        }

        fn channels(&self) -> ChannelCount {
            ChannelCount::new(2).unwrap_or(ChannelCount::MIN)
        }

        fn sample_rate(&self) -> SampleRate {
            self.sample_rate
        }

        fn total_duration(&self) -> Option<Duration> {
            if self.looped { None } else { self.duration }
        }

        fn try_seek(&mut self, position: Duration) -> Result<(), rodio::source::SeekError> {
            self.seek_to(position).map_err(|error| {
                rodio::source::SeekError::Other(Arc::new(std::io::Error::other(error.to_string())))
            })
        }
    }

    fn audio_resampler(
        decoder: &ffmpeg::decoder::Audio,
    ) -> Result<ffmpeg::software::resampling::Context, ffmpeg::Error> {
        let source_layout = if decoder.channel_layout().is_empty() {
            ffmpeg::ChannelLayout::default(i32::from(decoder.channels()))
        } else {
            decoder.channel_layout()
        };
        let rate = decoder.rate().max(1);
        ffmpeg::software::resampling::Context::get(
            decoder.format(),
            source_layout,
            rate,
            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
            ffmpeg::ChannelLayout::STEREO,
            rate,
        )
    }

    fn duration_to_stream_timestamp(
        position: Duration,
        time_base: ffmpeg::Rational,
        start_time: i64,
    ) -> Result<i64, ffmpeg::Error> {
        let numerator =
            u128::try_from(time_base.numerator()).map_err(|_| ffmpeg::Error::InvalidData)?;
        let denominator =
            u128::try_from(time_base.denominator()).map_err(|_| ffmpeg::Error::InvalidData)?;
        if numerator == 0 || denominator == 0 {
            return Err(ffmpeg::Error::InvalidData);
        }
        let divisor = 1_000_000_000_u128
            .checked_mul(numerator)
            .ok_or(ffmpeg::Error::InvalidData)?;
        let ticks = position
            .as_nanos()
            .checked_mul(denominator)
            .and_then(|value| value.checked_div(divisor))
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(ffmpeg::Error::InvalidData)?;
        start_time
            .checked_add(ticks)
            .ok_or(ffmpeg::Error::InvalidData)
    }

    fn seek_preroll_frames(
        frame_timestamp: Option<i64>,
        target: i64,
        time_base: ffmpeg::Rational,
        sample_rate: u32,
        frame_count: usize,
    ) -> usize {
        let Some(delta_ticks) = frame_timestamp.and_then(|timestamp| target.checked_sub(timestamp))
        else {
            return 0;
        };
        if delta_ticks <= 0 || time_base.numerator() <= 0 || time_base.denominator() <= 0 {
            return 0;
        }
        let numerator = (delta_ticks as u128)
            .saturating_mul(time_base.numerator() as u128)
            .saturating_mul(u128::from(sample_rate));
        let denominator = time_base.denominator() as u128;
        let frames = numerator.div_ceil(denominator);
        usize::try_from(frames)
            .unwrap_or(usize::MAX)
            .min(frame_count)
    }

    #[cfg(test)]
    mod tests {
        #[cfg(feature = "publisher")]
        use std::fs;
        use std::path::PathBuf;
        #[cfg(feature = "publisher")]
        use std::time::{SystemTime, UNIX_EPOCH};

        #[cfg(feature = "publisher")]
        use hakutaku_pack::{Identity, PackOptions, pack_directory};
        #[cfg(feature = "publisher")]
        use keine_loader::{ContentBackend, ContentMount, HakutakuArchive};

        use super::*;

        #[test]
        fn rejects_zero_sized_and_oversized_video_frames_before_allocation() {
            assert!(video_frame_layout(4_096, 2_304).is_ok());
            assert!(video_frame_layout(0, 1_080).is_err());
            assert!(video_frame_layout(4_097, 2_160).is_err());
            assert!(video_frame_layout(7_680, 4_320).is_err());
        }

        #[cfg(feature = "publisher")]
        fn playback_fixture() -> PathBuf {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("dev/fixtures/video/playback.mp4")
        }

        #[cfg(feature = "publisher")]
        struct Scratch(PathBuf);

        #[cfg(feature = "publisher")]
        impl Scratch {
            fn new() -> Self {
                let nonce = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                let path = std::env::temp_dir()
                    .join(format!("keine-video-test-{}-{nonce}", std::process::id()));
                fs::create_dir(&path).unwrap();
                Self(path)
            }

            fn path(&self) -> &Path {
                &self.0
            }
        }

        #[cfg(feature = "publisher")]
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        #[cfg(feature = "publisher")]
        fn decodes_encrypted_hakutaku_video_through_random_access() {
            ffmpeg::init().unwrap();
            let temporary = Scratch::new();
            let source_dir = temporary.path().join("source");
            fs::create_dir(&source_dir).unwrap();
            fs::copy(playback_fixture(), source_dir.join("playback.mp4")).unwrap();
            let mount = encrypted_mount(&source_dir, temporary.path());
            let source = Arc::new(prepare_source(&[mount], Path::new("playback.mp4")).unwrap());
            assert!(source.physical_path().is_none());

            let mut video = VideoDecoder::open(&source).unwrap();
            assert!(video.has_audio());
            let video_duration = video.duration();
            let first = video.decode_raw().unwrap().unwrap();
            assert_eq!((first.width(), first.height()), (320, 240));
            video.seek_to_start().unwrap();
            assert!(video.decode_raw().unwrap().is_some());
            video.seek_to_start().unwrap();
            let mut last_timestamp = -1.0_f32;
            for cycle in 0..3 {
                while let Some(frame) = video.decode_raw().unwrap() {
                    let local = frame.timestamp().map_or(0.0, |timestamp| {
                        timestamp as f64 * f64::from(video.time_base().numerator())
                            / f64::from(video.time_base().denominator())
                    }) as f32;
                    let timestamp = cycle as f32 * video_duration + local;
                    assert!(timestamp >= last_timestamp);
                    last_timestamp = timestamp;
                }
                if cycle < 2 {
                    video.seek_to_start().unwrap();
                }
            }

            let mut audio = FfmpegAudioStream::open(source.clone(), false).unwrap();
            assert!(audio.by_ref().take(4_096).any(|sample| sample != 0.0));
            audio.try_seek(Duration::ZERO).unwrap();
            assert!(audio.by_ref().take(4_096).any(|sample| sample != 0.0));

            let mut seeked_audio = FfmpegAudioStream::open(source.clone(), false).unwrap();
            let seek_position = Duration::from_millis(500);
            seeked_audio.try_seek(seek_position).unwrap();
            let sample_rate = seeked_audio.sample_rate().get() as f32;
            let remaining = seeked_audio.count() as f32 / (sample_rate * 2.0);
            assert!((remaining - (video_duration - seek_position.as_secs_f32())).abs() < 0.05);

            let mut looped_audio = FfmpegAudioStream::open(source.clone(), true).unwrap();
            let two_seconds = looped_audio.sample_rate().get() as usize * 2 * 2;
            assert_eq!(looped_audio.by_ref().take(two_seconds).count(), two_seconds);

            let audio = FfmpegAudioStream::open(source, false).unwrap();
            let sample_rate = audio.sample_rate().get() as f32;
            let audio_duration = audio.count() as f32 / (sample_rate * 2.0);
            assert!((audio_duration - video_duration).abs() < 0.05);
        }

        #[cfg(feature = "publisher")]
        fn encrypted_mount(source: &Path, temporary: &Path) -> ContentMount {
            let release = temporary.join("release");
            let identity = Identity::generate().unwrap();
            pack_directory(&PackOptions::new(source, &release), &identity).unwrap();
            let archive = HakutakuArchive::open_with_keys(
                &release.join("game.haku"),
                identity.root_key(),
                identity.public_key(),
            )
            .unwrap();
            ContentMount::new(ContentBackend::Hakutaku(archive), "").unwrap()
        }

        #[test]
        fn audio_clock_fallback_is_pause_stable_and_long_play_monotonic() {
            assert_eq!(fallback_audio_clock(12.5, 100.0, 100.0), 12.5);
            assert_eq!(fallback_audio_clock(12.5, 100.0, 99.0), 12.5);
            assert_eq!(fallback_audio_clock(12.5, 100.0, 3_700.0), 3_612.5);
        }

        #[test]
        fn audio_seek_math_uses_stream_time_base_and_bounds_preroll() {
            let time_base = ffmpeg::Rational(1, 48_000);
            assert_eq!(
                duration_to_stream_timestamp(Duration::from_millis(500), time_base, 1_024).unwrap(),
                25_024
            );
            assert_eq!(
                seek_preroll_frames(Some(24_000), 25_024, time_base, 48_000, 2_048),
                1_024
            );
            assert_eq!(
                seek_preroll_frames(Some(0), 48_000, time_base, 48_000, 960),
                960
            );
            assert_eq!(seek_preroll_frames(None, 48_000, time_base, 48_000, 960), 0);
        }
    }
}

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
pub(crate) use ffmpeg_backend::validate_ffmpeg_video;
