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

// When both desktop backends are enabled, macOS selects AVFoundation at
// runtime while keeping FFmpeg available as the cross-platform build fallback.
#[cfg(all(
    feature = "video-ffmpeg",
    feature = "video-native",
    target_os = "macos"
))]
use video_rs as _;

#[cfg(not(any(
    feature = "video-ffmpeg",
    all(feature = "video-native", target_os = "macos")
)))]
use crate::runtime::resources::GameState;

#[cfg(any(
    feature = "video-ffmpeg",
    all(feature = "video-native", target_os = "macos")
))]
#[path = "video/shared.rs"]
mod shared;

#[cfg(all(feature = "video-native", target_os = "macos"))]
#[path = "video/avfoundation.rs"]
mod avfoundation_backend;

pub(crate) struct VideoPlugin;

impl Plugin for VideoPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(all(feature = "video-native", target_os = "macos"))]
        app.init_non_send::<avfoundation_backend::VideoPlayback>()
            .add_systems(
                Update,
                avfoundation_backend::sync_video_playback
                    .in_set(crate::runtime::GameSystemSet::Sync),
            );

        #[cfg(all(
            feature = "video-ffmpeg",
            not(all(feature = "video-native", target_os = "macos"))
        ))]
        {
            use bevy::audio::AddAudioSource;

            if let Err(error) = video_rs::init() {
                log::error!("failed to initialize FFmpeg video backend: {error}");
            }
            app.init_resource::<VideoPlayback>()
                .init_asset::<ffmpeg_backend::FfmpegVideoAudio>()
                .add_audio_source::<ffmpeg_backend::FfmpegVideoAudio>()
                .add_systems(
                    Update,
                    sync_video_playback.in_set(crate::runtime::GameSystemSet::Sync),
                );
        }
        #[cfg(not(any(
            feature = "video-ffmpeg",
            all(feature = "video-native", target_os = "macos")
        )))]
        app.init_resource::<MissingVideoBackend>().add_systems(
            Update,
            reject_unavailable_video.in_set(crate::runtime::GameSystemSet::Sync),
        );
    }
}

#[cfg(not(any(
    feature = "video-ffmpeg",
    all(feature = "video-native", target_os = "macos")
)))]
#[derive(Resource, Default)]
struct MissingVideoBackend(bool);

#[cfg(not(any(
    feature = "video-ffmpeg",
    all(feature = "video-native", target_os = "macos")
)))]
fn reject_unavailable_video(mut state: ResMut<GameState>, mut warned: ResMut<MissingVideoBackend>) {
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

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
mod ffmpeg_backend {
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use bevy::audio::{AudioPlayer, Decodable, PlaybackMode, PlaybackSettings, Volume};
    use bevy::ecs::system::SystemParam;
    use bevy::prelude::*;
    use bevy::render::render_resource::TextureFormat;
    use keine_core::VideoMode;
    use keine_loader::ContentMount;
    use rodio::{ChannelCount, SampleRate, Source};
    use video_rs::ffmpeg;
    use video_rs::ffmpeg::software::scaling::{context::Context as VideoScaler, flag::Flags};

    use super::shared::{
        PreparedSource, VideoFrame, VideoNode, VideoPresentation, VideoVisual, VisualResources,
        cleanup_visual, prepare_source, present_frame, update_visual,
    };
    use super::{HashMap, RenderLayers};
    use crate::runtime::platform::DesignViewport;
    use crate::runtime::resources::{ContentProjectResource, GameConfigResource, GameState};
    use crate::scene::effects::material::{StageMaterial, StageQuad};
    use crate::storage::settings::RuntimeSettings;

    #[derive(Resource, Default)]
    pub(super) struct VideoPlayback {
        sessions: HashMap<String, VideoSession>,
        retired_decoders: Vec<thread::JoinHandle<()>>,
    }

    struct VideoSession {
        receiver: Mutex<Receiver<DecoderEvent>>,
        cancelled: Arc<AtomicBool>,
        decoder: Option<thread::JoinHandle<()>>,
        pending: Option<DecodedFrame>,
        visual: VideoVisual,
        audio_entity: Option<Entity>,
        audio_asset: Option<Handle<FfmpegVideoAudio>>,
        source: Option<Arc<PreparedSource>>,
        start_elapsed: Option<f32>,
        mode: VideoMode,
        muted: bool,
        revision: u64,
    }

    impl Drop for VideoSession {
        fn drop(&mut self) {
            self.cancelled.store(true, Ordering::Release);
        }
    }

    enum DecoderEvent {
        Ready(Arc<PreparedSource>),
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

    #[derive(Asset, TypePath, Clone, Debug)]
    pub(super) struct FfmpegVideoAudio {
        source: Arc<PreparedSource>,
    }

    impl Decodable for FfmpegVideoAudio {
        type Decoder = FfmpegAudioStream;

        fn decoder(&self) -> Self::Decoder {
            FfmpegAudioStream::open(self.source.clone()).unwrap_or_else(|error| {
                log::warn!("video audio track is unavailable: {error}");
                FfmpegAudioStream::failed(self.source.clone())
            })
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
        audio: ResMut<'w, Assets<FfmpegVideoAudio>>,
    }

    pub(super) fn sync_video_playback(
        mut commands: Commands,
        mut state: ResMut<GameState>,
        windows: Query<Ref<Window>>,
        mut playback: ResMut<VideoPlayback>,
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

        for (id, video) in &state.videos {
            playback.sessions.entry(id.clone()).or_insert_with(|| {
                spawn_decoder(
                    resources.content.asset_mounts(),
                    resources.config.video_path(&video.spec.file),
                    video.spec.looped,
                    video.spec.mode,
                    video.spec.muted,
                    video.revision,
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
                if frame.timestamp <= playback_elapsed(video.elapsed, session.start_elapsed) {
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
                    Ok(DecoderEvent::Ready(source)) => {
                        session.start_elapsed.get_or_insert(video.elapsed);
                        if !session.muted && session.audio_entity.is_none() {
                            let asset = resources.audio.add(FfmpegVideoAudio {
                                source: source.clone(),
                            });
                            let entity = commands
                                .spawn((
                                    Name::new(format!("video-audio::{id}")),
                                    AudioPlayer(asset.clone()),
                                    PlaybackSettings {
                                        mode: if video.spec.looped {
                                            PlaybackMode::Loop
                                        } else {
                                            PlaybackMode::Despawn
                                        },
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
                    Ok(DecoderEvent::Frame(frame))
                        if frame.timestamp
                            <= playback_elapsed(video.elapsed, session.start_elapsed) =>
                    {
                        newest = Some(frame);
                    }
                    Ok(DecoderEvent::Frame(frame)) => {
                        session.pending = Some(frame);
                        break;
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
                &session.visual,
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
        session.cancelled.store(true, Ordering::Release);
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
    ) -> VideoSession {
        // Two frames cover normal decoder jitter without retaining another
        // 8 MiB 1080p RGBA allocation or adding a visible frame of latency.
        let (sender, receiver) = sync_channel(2);
        let cancelled = Arc::new(AtomicBool::new(false));
        let thread_cancelled = cancelled.clone();
        let decoder = thread::Builder::new()
            .name(format!("keine-video-{path}"))
            .spawn(move || decode_video(mounts, &path, looped, thread_cancelled, sender))
            .unwrap_or_else(|error| {
                log::error!("failed to start video decoder thread: {error}");
                thread::spawn(|| {})
            });
        VideoSession {
            receiver: Mutex::new(receiver),
            cancelled,
            decoder: Some(decoder),
            pending: None,
            visual: VideoVisual::default(),
            audio_entity: None,
            audio_asset: None,
            source: None,
            start_elapsed: None,
            mode,
            muted,
            revision,
        }
    }

    fn playback_elapsed(state_elapsed: f32, start_elapsed: Option<f32>) -> f32 {
        start_elapsed.map_or(0.0, |start| (state_elapsed - start).max(0.0))
    }

    fn decode_video(
        mounts: Vec<ContentMount>,
        logical_path: &str,
        looped: bool,
        cancelled: Arc<AtomicBool>,
        sender: SyncSender<DecoderEvent>,
    ) {
        let source = match prepare_source(&mounts, Path::new(logical_path)) {
            Ok(source) => Arc::new(source),
            Err(error) => {
                let _ = sender.send(DecoderEvent::Error(error));
                return;
            }
        };
        let mut decoder = match open_decoder(source.path.as_path()) {
            Ok(decoder) => decoder,
            Err(error) => {
                let _ = sender.send(DecoderEvent::Error(error.to_string()));
                return;
            }
        };
        let duration = decoder
            .duration()
            .map_or(0.0, |duration| duration.as_secs());
        if !send_event(&sender, DecoderEvent::Ready(source), &cancelled) {
            return;
        }
        let mut loop_offset = 0.0;
        let mut rgba_converter = None;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            match decoder.decode_raw() {
                Ok(frame) => {
                    let timestamp =
                        video_rs::Time::new(Some(frame.packet().dts), decoder.time_base());
                    let width = frame.width() as usize;
                    let height = frame.height() as usize;
                    let rgba = convert_to_rgba(&frame, &mut rgba_converter)
                        .map_err(|error| error.to_string());
                    let rgba = match rgba {
                        Ok(rgba) => rgba,
                        Err(error) => {
                            let _ = sender.send(DecoderEvent::Error(error));
                            return;
                        }
                    };
                    let frame = DecodedFrame {
                        timestamp: loop_offset + timestamp.as_secs().max(0.0),
                        width: width as u32,
                        height: height as u32,
                        rgba,
                    };
                    if !send_event(&sender, DecoderEvent::Frame(frame), &cancelled) {
                        return;
                    }
                }
                Err(video_rs::Error::DecodeExhausted | video_rs::Error::ReadExhausted)
                    if looped =>
                {
                    loop_offset += duration;
                    if let Err(error) = decoder.seek_to_start() {
                        let _ = sender.send(DecoderEvent::Error(error.to_string()));
                        return;
                    }
                }
                Err(video_rs::Error::DecodeExhausted | video_rs::Error::ReadExhausted) => {
                    let _ = sender.send(DecoderEvent::End);
                    return;
                }
                Err(error) => {
                    let _ = sender.send(DecoderEvent::Error(error.to_string()));
                    return;
                }
            }
        }
    }

    fn open_decoder(path: &Path) -> Result<video_rs::Decoder, video_rs::Error> {
        log::info!("video decoder · software");
        video_rs::Decoder::new(path)
    }

    fn convert_to_rgba(
        source: &ffmpeg::frame::Video,
        converter: &mut Option<RgbaConverter>,
    ) -> Result<Vec<u8>, ffmpeg::Error> {
        let width = source.width();
        let height = source.height();
        let converter = match converter {
            Some(converter) => converter,
            slot @ None => slot.insert(RgbaConverter {
                scaler: VideoScaler::get(
                    source.format(),
                    width,
                    height,
                    ffmpeg::format::Pixel::RGBA,
                    width,
                    height,
                    Flags::FAST_BILINEAR,
                )?,
                target: ffmpeg::frame::Video::empty(),
            }),
        };
        converter.scaler.run(source, &mut converter.target)?;

        let row_bytes = width as usize * 4;
        let height = height as usize;
        let stride = converter.target.stride(0);
        let data = converter.target.data(0);
        if stride == row_bytes {
            return Ok(data[..row_bytes * height].to_vec());
        }

        let mut rgba = Vec::with_capacity(row_bytes * height);
        for row in data.chunks(stride).take(height) {
            rgba.extend_from_slice(&row[..row_bytes]);
        }
        Ok(rgba)
    }

    fn send_event(
        sender: &SyncSender<DecoderEvent>,
        mut event: DecoderEvent,
        cancelled: &AtomicBool,
    ) -> bool {
        loop {
            if cancelled.load(Ordering::Acquire) {
                return false;
            }
            match sender.try_send(event) {
                Ok(()) => return true,
                Err(TrySendError::Full(returned)) => {
                    event = returned;
                    thread::sleep(Duration::from_millis(2));
                }
                Err(TrySendError::Disconnected(_)) => return false,
            }
        }
    }

    pub(super) struct FfmpegAudioStream {
        source: Arc<PreparedSource>,
        input: Option<ffmpeg::format::context::Input>,
        decoder: Option<ffmpeg::decoder::Audio>,
        resampler: Option<ffmpeg::software::resampling::Context>,
        stream_index: usize,
        samples: Vec<f32>,
        position: usize,
        sample_rate: SampleRate,
        duration: Option<Duration>,
        eof_sent: bool,
        ended: bool,
    }

    impl FfmpegAudioStream {
        fn open(source: Arc<PreparedSource>) -> Result<Self, ffmpeg::Error> {
            let input = ffmpeg::format::input(&source.path)?;
            let stream = input
                .streams()
                .best(ffmpeg::media::Type::Audio)
                .ok_or(ffmpeg::Error::StreamNotFound)?;
            let stream_index = stream.index();
            let duration = (stream.duration() > 0).then(|| {
                let base = stream.time_base();
                Duration::from_secs_f64(
                    stream.duration() as f64 * f64::from(base.numerator())
                        / f64::from(base.denominator()),
                )
            });
            let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
            let decoder = context.decoder().audio()?;
            let source_layout = if decoder.channel_layout().is_empty() {
                ffmpeg::ChannelLayout::default(i32::from(decoder.channels()))
            } else {
                decoder.channel_layout()
            };
            let rate = decoder.rate().max(1);
            let resampler = ffmpeg::software::resampling::Context::get(
                decoder.format(),
                source_layout,
                rate,
                ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
                ffmpeg::ChannelLayout::STEREO,
                rate,
            )?;
            Ok(Self {
                source,
                input: Some(input),
                decoder: Some(decoder),
                resampler: Some(resampler),
                stream_index,
                samples: Vec::new(),
                position: 0,
                sample_rate: SampleRate::new(rate).unwrap_or(SampleRate::MIN),
                duration,
                eof_sent: false,
                ended: false,
            })
        }

        fn failed(source: Arc<PreparedSource>) -> Self {
            Self {
                source,
                input: None,
                decoder: None,
                resampler: None,
                stream_index: 0,
                samples: Vec::new(),
                position: 0,
                sample_rate: SampleRate::new(48_000).unwrap_or(SampleRate::MIN),
                duration: None,
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
                let mut converted = ffmpeg::frame::Audio::empty();
                resampler.run(&decoded, &mut converted)?;
                self.samples.clear();
                self.position = 0;
                for &(left, right) in converted.plane::<(f32, f32)>(0) {
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
            self.duration
        }

        fn try_seek(&mut self, position: Duration) -> Result<(), rodio::source::SeekError> {
            let mut reopened = Self::open(self.source.clone()).map_err(|error| {
                rodio::source::SeekError::Other(Arc::new(std::io::Error::other(error.to_string())))
            })?;
            let samples =
                (position.as_secs_f64() * f64::from(reopened.sample_rate.get()) * 2.0) as usize;
            for _ in 0..samples {
                if reopened.next().is_none() {
                    break;
                }
            }
            *self = reopened;
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use std::path::PathBuf;

        use super::*;

        #[test]
        #[ignore = "set KEINE_TEST_VIDEO to a local video"]
        fn decodes_video_frames_with_the_runtime_pipeline() {
            video_rs::init().unwrap();
            let path = std::env::var_os("KEINE_TEST_VIDEO")
                .map(PathBuf::from)
                .expect("KEINE_TEST_VIDEO is required");
            let mut decoder = open_decoder(path.as_path()).unwrap();
            let mut scaler = None;
            for _ in 0..60 {
                let frame = decoder.decode_raw().unwrap();
                let expected = frame.width() as usize * frame.height() as usize * 4;
                assert_eq!(
                    convert_to_rgba(&frame, &mut scaler).unwrap().len(),
                    expected
                );
            }
        }

        #[test]
        #[ignore = "set KEINE_TEST_VIDEO to a local video with an audio track"]
        fn decodes_video_and_audio_incrementally() {
            let path = std::env::var_os("KEINE_TEST_VIDEO")
                .map(PathBuf::from)
                .expect("KEINE_TEST_VIDEO is required");
            let source = Arc::new(PreparedSource::filesystem(path.clone()));
            let mut video = video_rs::Decoder::new(path.as_path()).unwrap();
            let frame = video.decode_raw().unwrap();
            assert!(frame.width() > 0 && frame.height() > 0);

            let mut audio = FfmpegAudioStream::open(source).unwrap();
            assert!(audio.by_ref().take(4_096).any(|sample| sample != 0.0));
        }
    }
}

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
use ffmpeg_backend::{VideoPlayback, sync_video_playback};
