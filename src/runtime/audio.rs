#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
use std::io::{self, Cursor};
#[cfg(feature = "audio-opus")]
use std::io::{Read, Seek, SeekFrom};
#[cfg(feature = "audio-opus")]
use std::path::PathBuf;
#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
use std::sync::Arc;
#[cfg(feature = "audio-opus")]
use std::sync::Mutex;
#[cfg(feature = "audio-seekable")]
use std::sync::OnceLock;
#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
use std::time::Duration;

#[cfg(feature = "audio-opus")]
use bevy::asset::io::AssetSourceId;
#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
use bevy::asset::{AssetApp, AssetLoader, LoadContext, io::Reader};
#[cfg(not(feature = "audio-seekable"))]
use bevy::audio::AudioSource;
#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
use bevy::audio::PlaybackMode;
#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
use bevy::audio::{AddAudioSource, Decodable};
use bevy::audio::{AudioPlayer, PlaybackSettings};
use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;
#[cfg(feature = "audio-opus")]
use keine_loader::{ContentFile, ContentMount};
#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
use rodio::Source;
#[cfg(feature = "audio-opus")]
use rodio::{ChannelCount, SampleRate};
#[cfg(feature = "audio-opus")]
use symphonia::core::audio::sample::Sample;
#[cfg(feature = "audio-opus")]
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
#[cfg(feature = "audio-opus")]
use symphonia::core::codecs::registry::CodecRegistry;
#[cfg(feature = "audio-opus")]
use symphonia::core::errors::Error as SymphoniaError;
#[cfg(feature = "audio-opus")]
use symphonia::core::formats::{
    FormatOptions, FormatReader, SeekMode, SeekTo, TrackType, probe::Hint,
};
#[cfg(feature = "audio-opus")]
use symphonia::core::io::{MediaSource, MediaSourceStream};
#[cfg(feature = "audio-opus")]
use symphonia::core::meta::MetadataOptions;
#[cfg(feature = "audio-opus")]
use symphonia::core::units::{Time as SymphoniaTime, TimeBase, Timestamp};
#[cfg(feature = "audio-opus")]
use symphonia_adapter_libopus::OpusDecoder;

pub(crate) fn configure_audio(app: &mut App, _mounts: Vec<keine_loader::ContentMount>) {
    // Bevy only registers AudioSource when one of its decoder features is enabled.
    // Unsupported-format placeholders still need a registered handle type.
    #[cfg(not(feature = "audio-seekable"))]
    if !app.world().contains_resource::<Assets<AudioSource>>() {
        app.init_asset::<AudioSource>();
    }
    #[cfg(feature = "audio-opus")]
    app.add_plugins(OpusAudioPlugin::new(_mounts));
    #[cfg(feature = "audio-seekable")]
    app.add_plugins(SeekableAudioPlugin);
}

/// Compatibility formats are retained in compressed form for seek/loop.
/// Opus assets from the project mount stay streaming and do not consume this
/// allowance; this bounds only paths that must become one in-memory asset.
#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
pub(crate) const MAX_IN_MEMORY_AUDIO_BYTES: usize = 128 * 1024 * 1024;

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct CachedAudioSizes<'w> {
    #[cfg(feature = "audio-opus")]
    opus: Res<'w, Assets<OpusAudio>>,
    #[cfg(feature = "audio-seekable")]
    seekable: Res<'w, Assets<SeekableAudio>>,
    #[cfg(not(feature = "audio-seekable"))]
    fallback: Res<'w, Assets<AudioSource>>,
}

impl CachedAudioSizes<'_> {
    pub(crate) fn is_changed(&self) -> bool {
        let mut changed = false;
        #[cfg(feature = "audio-opus")]
        {
            changed |= self.opus.is_changed();
        }
        #[cfg(feature = "audio-seekable")]
        {
            changed |= self.seekable.is_changed();
        }
        #[cfg(not(feature = "audio-seekable"))]
        {
            changed |= self.fallback.is_changed();
        }
        changed
    }

    pub(crate) fn bytes(&self, handle: &UntypedHandle) -> Option<usize> {
        #[cfg(feature = "audio-opus")]
        if handle.type_id() == std::any::TypeId::of::<OpusAudio>() {
            return self
                .opus
                .get(handle.id().typed::<OpusAudio>())
                .map(|audio| match &audio.source {
                    OpusSource::Mounted { .. } => 0,
                    OpusSource::Memory(bytes) => bytes.len(),
                });
        }
        #[cfg(feature = "audio-seekable")]
        if handle.type_id() == std::any::TypeId::of::<SeekableAudio>() {
            return self
                .seekable
                .get(handle.id().typed::<SeekableAudio>())
                .map(|audio| audio.bytes.len());
        }
        #[cfg(not(feature = "audio-seekable"))]
        if handle.type_id() == std::any::TypeId::of::<AudioSource>() {
            return self
                .fallback
                .get(handle.id().typed::<AudioSource>())
                .map(|audio| audio.bytes.len());
        }
        None
    }
}

#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
async fn read_in_memory_audio(reader: &mut dyn Reader, maximum: usize) -> io::Result<Vec<u8>> {
    crate::runtime::bounded_input::read_to_end(reader, maximum, "audio").await
}

/// Reopenable Ogg Opus data. Project assets retain only their logical source;
/// each playback decoder reads and seeks the underlying file incrementally.
#[derive(Asset, Clone, Debug, TypePath)]
#[cfg(feature = "audio-opus")]
pub(crate) struct OpusAudio {
    source: OpusSource,
    duration: Option<Duration>,
}

/// Ogg Opus source that rewinds its demuxer at EOF instead of asking Rodio to
/// retain the first decoded pass in `Buffered`.
#[derive(Asset, Clone, Debug, TypePath)]
#[cfg(feature = "audio-opus")]
struct LoopingOpusAudio {
    source: OpusSource,
}

#[derive(Clone, Debug)]
#[cfg(feature = "audio-opus")]
enum OpusSource {
    Mounted {
        mounts: Arc<[ContentMount]>,
        path: PathBuf,
    },
    Memory(Arc<[u8]>),
}

#[cfg(feature = "audio-opus")]
impl OpusSource {
    fn open(&self) -> io::Result<Box<dyn MediaSource>> {
        match self {
            Self::Mounted { mounts, path } => {
                let file = mounts
                    .iter()
                    .rev()
                    .find(|mount| mount.contains_file(path))
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::NotFound, path.display().to_string())
                    })?
                    .open_file(path)
                    .map_err(|error| io::Error::other(error.to_string()))?;
                Ok(Box::new(StreamingAudioFile::new(file)?))
            }
            Self::Memory(bytes) => Ok(Box::new(Cursor::new(bytes.clone()))),
        }
    }
}

#[cfg(feature = "audio-opus")]
struct StreamingAudioFile {
    file: Mutex<ContentFile>,
    len: u64,
}

#[cfg(feature = "audio-opus")]
impl StreamingAudioFile {
    fn new(file: ContentFile) -> io::Result<Self> {
        let len = file.len()?;
        Ok(Self {
            file: Mutex::new(file),
            len,
        })
    }

    fn file(&self) -> std::sync::MutexGuard<'_, ContentFile> {
        self.file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(feature = "audio-opus")]
impl Read for StreamingAudioFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file().read(buffer)
    }
}

#[cfg(feature = "audio-opus")]
impl Seek for StreamingAudioFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file().seek(position)
    }
}

#[cfg(feature = "audio-opus")]
impl MediaSource for StreamingAudioFile {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

#[cfg(feature = "audio-opus")]
impl OpusAudio {
    pub(crate) fn duration(&self) -> Option<Duration> {
        self.duration
    }
}

#[derive(TypePath)]
#[cfg(feature = "audio-opus")]
struct OpusAudioLoader {
    mounts: Arc<[ContentMount]>,
}

#[cfg(feature = "audio-opus")]
impl AssetLoader for OpusAudioLoader {
    type Asset = OpusAudio;
    type Settings = ();
    type Error = io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let source = if matches!(load_context.path().source(), AssetSourceId::Default) {
            OpusSource::Mounted {
                mounts: self.mounts.clone(),
                path: load_context.path().path().to_owned(),
            }
        } else {
            let bytes = read_in_memory_audio(reader, MAX_IN_MEMORY_AUDIO_BYTES).await?;
            if bytes.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "empty Opus asset",
                ));
            }
            OpusSource::Memory(bytes.into())
        };
        let stream = OpusStream::new(source.open()?, false)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        load_context.add_labeled_asset(
            "looping".to_owned(),
            LoopingOpusAudio {
                source: source.clone(),
            },
        );
        Ok(OpusAudio {
            duration: stream.total_duration(),
            source,
        })
    }

    fn extensions(&self) -> &[&str] {
        &[
            "opus", "opuS", "opUs", "opUS", "oPus", "oPuS", "oPUs", "oPUS", "Opus", "OpuS", "OpUs",
            "OpUS", "OPus", "OPuS", "OPUs", "OPUS",
        ]
    }
}

#[cfg(feature = "audio-opus")]
pub(crate) struct OpusAudioPlugin {
    mounts: Arc<[ContentMount]>,
}

#[cfg(feature = "audio-opus")]
impl OpusAudioPlugin {
    pub(crate) fn new(mounts: Vec<ContentMount>) -> Self {
        Self {
            mounts: mounts.into(),
        }
    }
}

#[cfg(feature = "audio-opus")]
impl Plugin for OpusAudioPlugin {
    fn build(&self, app: &mut App) {
        app.register_asset_loader(OpusAudioLoader {
            mounts: self.mounts.clone(),
        })
        .add_audio_source::<OpusAudio>()
        .add_audio_source::<LoopingOpusAudio>();
    }
}

/// Shared in-memory asset for non-Opus project audio. Every playback decoder
/// receives the byte length Rodio requires for duration and reliable seeking;
/// the gallery therefore reuses the same compressed allocation as the story.
#[derive(Asset, Clone, Debug, TypePath)]
#[cfg(feature = "audio-seekable")]
pub(crate) struct SeekableAudio {
    bytes: Arc<[u8]>,
    duration: OnceLock<Option<Duration>>,
    hint: &'static str,
}

#[derive(Asset, Clone, Debug, TypePath)]
#[cfg(feature = "audio-seekable")]
struct LoopingSeekableAudio {
    bytes: Arc<[u8]>,
    hint: &'static str,
}

#[cfg(feature = "audio-seekable")]
impl SeekableAudio {
    pub(crate) fn duration(&self) -> Option<Duration> {
        *self.duration.get_or_init(|| match self.decoder() {
            Ok(decoder) => decoder.total_duration(),
            Err(error) => {
                log::error!("failed to read audio duration: {error}");
                None
            }
        })
    }

    fn decoder(&self) -> Result<rodio::Decoder<Cursor<Arc<[u8]>>>, rodio::decoder::DecoderError> {
        seekable_decoder(self.bytes.clone(), self.hint)
    }
}

#[derive(Default, TypePath)]
#[cfg(feature = "audio-seekable")]
struct SeekableAudioLoader;

#[cfg(feature = "audio-seekable")]
impl AssetLoader for SeekableAudioLoader {
    type Asset = SeekableAudio;
    type Settings = ();
    type Error = io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let bytes = read_in_memory_audio(reader, MAX_IN_MEMORY_AUDIO_BYTES).await?;
        if bytes.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "empty audio asset",
            ));
        }
        let hint = audio_hint(load_context.path().path())?;
        let bytes: Arc<[u8]> = bytes.into();
        load_context.add_labeled_asset(
            "looping".to_owned(),
            LoopingSeekableAudio {
                bytes: bytes.clone(),
                hint,
            },
        );
        Ok(SeekableAudio {
            bytes,
            duration: OnceLock::new(),
            hint,
        })
    }

    fn extensions(&self) -> &[&str] {
        // Labeled assets are resolved by extension rather than parent asset
        // type. Preserve the filename while accepting every case variant.
        static EXTENSIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
        EXTENSIONS.get_or_init(|| {
            let mut extensions = Vec::new();
            #[cfg(feature = "audio-wav")]
            extensions.extend(["wav", "waV", "wAv", "wAV", "Wav", "WaV", "WAv", "WAV"]);
            #[cfg(feature = "audio-mp3")]
            extensions.extend(["mp3", "mP3", "Mp3", "MP3"]);
            #[cfg(feature = "audio-vorbis")]
            extensions.extend([
                "ogg", "ogG", "oGg", "oGG", "Ogg", "OgG", "OGg", "OGG", "oga", "ogA", "oGa", "oGA",
                "Oga", "OgA", "OGa", "OGA", "spx", "spX", "sPx", "sPX", "Spx", "SpX", "SPx", "SPX",
            ]);
            #[cfg(feature = "audio-flac")]
            extensions.extend([
                "flac", "flaC", "flAc", "flAC", "fLac", "fLaC", "fLAc", "fLAC", "Flac", "FlaC",
                "FlAc", "FlAC", "FLac", "FLaC", "FLAc", "FLAC",
            ]);
            extensions
        })
    }
}

#[cfg(feature = "audio-seekable")]
fn audio_hint(path: &std::path::Path) -> io::Result<&'static str> {
    let extension = path.extension().and_then(|extension| extension.to_str());
    match extension.map(str::to_ascii_lowercase).as_deref() {
        #[cfg(feature = "audio-wav")]
        Some("wav") => Ok("wav"),
        #[cfg(feature = "audio-mp3")]
        Some("mp3") => Ok("mp3"),
        #[cfg(feature = "audio-vorbis")]
        Some("ogg" | "oga" | "spx") => Ok("ogg"),
        #[cfg(feature = "audio-flac")]
        Some("flac") => Ok("flac"),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported seekable audio path: {}", path.display()),
        )),
    }
}

#[cfg(feature = "audio-seekable")]
fn seekable_decoder(
    bytes: Arc<[u8]>,
    hint: &str,
) -> Result<rodio::Decoder<Cursor<Arc<[u8]>>>, rodio::decoder::DecoderError> {
    let byte_len = bytes.len() as u64;
    rodio::Decoder::builder()
        .with_data(Cursor::new(bytes))
        .with_byte_len(byte_len)
        .with_hint(hint)
        .build()
}

#[cfg(feature = "audio-seekable")]
fn looping_seekable_decoder(
    bytes: Arc<[u8]>,
    hint: &str,
) -> Result<rodio::decoder::LoopedDecoder<Cursor<Arc<[u8]>>>, rodio::decoder::DecoderError> {
    let byte_len = bytes.len() as u64;
    rodio::Decoder::builder()
        .with_data(Cursor::new(bytes))
        .with_byte_len(byte_len)
        .with_hint(hint)
        .build_looped()
}

#[cfg(feature = "audio-seekable")]
impl Decodable for SeekableAudio {
    type Decoder = Box<dyn Source + Send>;

    fn decoder(&self) -> Self::Decoder {
        match self.decoder() {
            Ok(decoder) => Box::new(decoder),
            Err(error) => {
                // Keep a malformed project asset from taking down the audio
                // thread; Bevy will naturally retire the empty source.
                log::error!("failed to recreate seekable audio decoder: {error}");
                Box::new(rodio::source::Empty::new())
            }
        }
    }
}

#[cfg(feature = "audio-seekable")]
impl Decodable for LoopingSeekableAudio {
    type Decoder = Box<dyn Source + Send>;

    fn decoder(&self) -> Self::Decoder {
        match looping_seekable_decoder(self.bytes.clone(), self.hint) {
            Ok(decoder) => Box::new(decoder),
            Err(error) => {
                log::error!("failed to recreate looping audio decoder: {error}");
                Box::new(rodio::source::Empty::new())
            }
        }
    }
}

#[derive(Default)]
#[cfg(feature = "audio-seekable")]
pub(crate) struct SeekableAudioPlugin;

#[cfg(feature = "audio-seekable")]
impl Plugin for SeekableAudioPlugin {
    fn build(&self, app: &mut App) {
        app.register_asset_loader(SeekableAudioLoader)
            .add_audio_source::<SeekableAudio>()
            .add_audio_source::<LoopingSeekableAudio>();
    }
}

#[cfg(feature = "audio-opus")]
impl Decodable for OpusAudio {
    type Decoder = OpusStream;

    fn decoder(&self) -> Self::Decoder {
        self.source
            .open()
            .and_then(|source| {
                OpusStream::new(source, false)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
            })
            .unwrap_or_else(|error| {
                log::error!("failed to decode Ogg Opus asset: {error}");
                OpusStream::failed()
            })
    }
}

#[cfg(feature = "audio-opus")]
impl Decodable for LoopingOpusAudio {
    type Decoder = OpusStream;

    fn decoder(&self) -> Self::Decoder {
        self.source
            .open()
            .and_then(|source| {
                OpusStream::new(source, true)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
            })
            .unwrap_or_else(|error| {
                log::error!("failed to decode looping Ogg Opus asset: {error}");
                OpusStream::failed()
            })
    }
}

/// Audition uses the gameplay decoder with an already confined logical asset path.
#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
pub(crate) fn authoring_audio_source(
    mounts: Arc<[keine_loader::ContentMount]>,
    path: &std::path::Path,
) -> io::Result<Box<dyn Source + Send>> {
    #[cfg(feature = "audio-opus")]
    if is_opus(&path.to_string_lossy()) {
        let source = OpusSource::Mounted {
            mounts,
            path: path.to_owned(),
        };
        return OpusStream::new(source.open()?, false)
            .map(|source| Box::new(source) as Box<dyn Source + Send>)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()));
    }
    #[cfg(feature = "audio-seekable")]
    {
        use std::io::Read;
        let hint = audio_hint(path)?;
        let mut file = mounts
            .iter()
            .rev()
            .find(|mount| mount.contains_file(path))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "audio asset not found"))?
            .open_file(path)
            .map_err(|error| io::Error::other(error.to_string()))?;
        if file.len()? > MAX_IN_MEMORY_AUDIO_BYTES as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "audio asset exceeds memory limit",
            ));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_IN_MEMORY_AUDIO_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_IN_MEMORY_AUDIO_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "audio asset exceeds memory limit",
            ));
        }
        seekable_decoder(bytes.into(), hint)
            .map(|source| Box::new(source) as Box<dyn Source + Send>)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
    }
    #[cfg(not(feature = "audio-seekable"))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "audio format is unavailable in this Engine build",
    ))
}

pub(crate) fn insert_player(
    entity: &mut EntityCommands<'_>,
    asset_server: &AssetServer,
    path: String,
    settings: PlaybackSettings,
) {
    #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
    let mut settings = settings;
    if is_opus(&path) {
        #[cfg(feature = "audio-opus")]
        {
            if matches!(settings.mode, PlaybackMode::Loop) {
                settings.mode = PlaybackMode::Once;
                entity.insert(AudioPlayer::<LoopingOpusAudio>(
                    asset_server.load(format!("{path}#looping")),
                ));
            } else {
                entity.insert(AudioPlayer::<OpusAudio>(asset_server.load(path)));
            }
            entity.insert(settings);
            return;
        }
        #[cfg(not(feature = "audio-opus"))]
        log::error!("Opus asset `{path}` requires the `audio-opus` feature");
    }
    #[cfg(feature = "audio-seekable")]
    {
        if matches!(settings.mode, PlaybackMode::Loop) {
            settings.mode = PlaybackMode::Once;
            entity.insert(AudioPlayer::<LoopingSeekableAudio>(
                asset_server.load(format!("{path}#looping")),
            ));
        } else {
            entity.insert(AudioPlayer::<SeekableAudio>(asset_server.load(path)));
        }
        entity.insert(settings);
    }
    #[cfg(not(feature = "audio-seekable"))]
    entity.insert((
        AudioPlayer::new(asset_server.load::<AudioSource>(path)),
        settings,
    ));
}

/// Typed handle retained by the gallery so the UI can read format-independent
/// duration metadata without inspecting player component types.
#[derive(Component)]
pub(crate) enum GalleryAudio {
    #[cfg(feature = "audio-opus")]
    Opus(Handle<OpusAudio>),
    #[cfg(feature = "audio-seekable")]
    Seekable(Handle<SeekableAudio>),
    #[cfg(any(not(feature = "audio-opus"), not(feature = "audio-seekable")))]
    Unavailable,
}

/// Adds a one-shot player whose decoder supports duration queries and random
/// access. Opus retains its mount-backed streaming implementation; Bevy's
/// other formats share their compressed bytes with a byte-length-aware decoder.
pub(crate) fn insert_gallery_player(
    entity: &mut EntityCommands<'_>,
    asset_server: &AssetServer,
    path: String,
    settings: PlaybackSettings,
) {
    if is_opus(&path) {
        #[cfg(feature = "audio-opus")]
        {
            let handle = asset_server.load::<OpusAudio>(path);
            entity.insert((
                AudioPlayer::<OpusAudio>(handle.clone()),
                GalleryAudio::Opus(handle),
                settings,
            ));
        }
        #[cfg(not(feature = "audio-opus"))]
        {
            log::error!("Opus asset `{path}` requires the `audio-opus` feature");
            insert_unavailable_gallery_player(entity, asset_server, path, settings);
        }
    } else {
        #[cfg(feature = "audio-seekable")]
        {
            let handle = asset_server.load::<SeekableAudio>(path);
            entity.insert((
                AudioPlayer::<SeekableAudio>(handle.clone()),
                GalleryAudio::Seekable(handle),
                settings,
            ));
        }
        #[cfg(not(feature = "audio-seekable"))]
        insert_unavailable_gallery_player(entity, asset_server, path, settings);
    }
}

#[cfg(any(not(feature = "audio-opus"), not(feature = "audio-seekable")))]
fn insert_unavailable_gallery_player(
    entity: &mut EntityCommands<'_>,
    asset_server: &AssetServer,
    path: String,
    settings: PlaybackSettings,
) {
    entity.insert((
        AudioPlayer::new(asset_server.load::<AudioSource>(path)),
        GalleryAudio::Unavailable,
        settings,
    ));
}

pub(crate) fn load_untyped(asset_server: &AssetServer, path: String) -> UntypedHandle {
    if is_opus(&path) {
        #[cfg(feature = "audio-opus")]
        return asset_server.load::<OpusAudio>(path).untyped();
        #[cfg(not(feature = "audio-opus"))]
        log::error!("Opus asset `{path}` requires the `audio-opus` feature");
    }
    #[cfg(feature = "audio-seekable")]
    return asset_server.load::<SeekableAudio>(path).untyped();
    #[cfg(not(feature = "audio-seekable"))]
    asset_server.load::<AudioSource>(path).untyped()
}

fn is_opus(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("opus"))
}

#[cfg(all(test, feature = "audio-opus", feature = "audio-seekable"))]
mod cached_size_tests {
    use super::*;

    #[test]
    fn cache_accounts_for_memory_audio_without_charging_streamed_file_size() {
        let mut opus = Assets::<OpusAudio>::default();
        let streamed = opus
            .add(OpusAudio {
                source: OpusSource::Mounted {
                    mounts: [].into(),
                    path: "large.opus".into(),
                },
                duration: None,
            })
            .untyped();
        let memory = opus
            .add(OpusAudio {
                source: OpusSource::Memory([1, 2, 3].into()),
                duration: None,
            })
            .untyped();
        let mut seekable = Assets::<SeekableAudio>::default();
        let compatibility = seekable
            .add(SeekableAudio {
                bytes: [1, 2, 3, 4].into(),
                duration: OnceLock::new(),
                hint: "wav",
            })
            .untyped();
        let mut world = World::new();
        world.insert_resource(opus);
        world.insert_resource(seekable);
        let mut state = bevy::ecs::system::SystemState::<CachedAudioSizes>::new(&mut world);
        let sizes = state.get(&world).unwrap();
        assert_eq!(sizes.bytes(&streamed), Some(0));
        assert_eq!(sizes.bytes(&memory), Some(3));
        assert_eq!(sizes.bytes(&compatibility), Some(4));
    }
}

#[cfg(all(test, not(feature = "audio-seekable")))]
mod unavailable_tests {
    use super::*;

    #[test]
    fn configuration_preserves_an_existing_audio_registry() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        app.init_asset::<AudioSource>();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<AudioSource>>()
            .add(AudioSource { bytes: [].into() });

        configure_audio(&mut app, Vec::new());

        assert!(
            app.world()
                .resource::<Assets<AudioSource>>()
                .contains(&handle)
        );
    }

    #[test]
    fn unsupported_audio_can_be_loaded_and_queued_without_a_decoder() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        configure_audio(&mut app, Vec::new());
        let server = app.world().resource::<AssetServer>().clone();
        let handle = load_untyped(&server, "missing.wav".into());
        assert_eq!(handle.type_id(), std::any::TypeId::of::<AudioSource>());
        let (player, gallery) = {
            let mut commands = app.world_mut().commands();
            let mut player = commands.spawn_empty();
            insert_player(
                &mut player,
                &server,
                "missing.wav".into(),
                PlaybackSettings::LOOP,
            );
            let player = player.id();
            let mut gallery = commands.spawn_empty();
            insert_gallery_player(
                &mut gallery,
                &server,
                "missing.wav".into(),
                PlaybackSettings::ONCE,
            );
            (player, gallery.id())
        };
        app.world_mut().flush();
        assert!(
            app.world()
                .get::<AudioPlayer<AudioSource>>(player)
                .is_some()
        );
        assert!(matches!(
            app.world().get::<GalleryAudio>(gallery),
            Some(GalleryAudio::Unavailable)
        ));
    }
}

#[cfg(all(test, feature = "audio-seekable", feature = "audio-wav"))]
mod seekable_tests {
    use super::*;

    #[test]
    fn uppercase_wav_loads_for_looping_story_playback() {
        use bevy::asset::{
            AssetPlugin, LoadState,
            io::{
                AssetSourceBuilder,
                memory::{Dir, MemoryAssetReader},
            },
        };

        let dir = Dir::default();
        dir.insert_asset(
            std::path::Path::new("track.WAV"),
            silent_pcm_wav(8_000, 80).to_vec(),
        );
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .register_asset_source(
                "test",
                AssetSourceBuilder::new(move || Box::new(MemoryAssetReader { root: dir.clone() })),
            )
            .add_plugins(AssetPlugin::default())
            .init_asset::<SeekableAudio>()
            .init_asset::<LoopingSeekableAudio>()
            .register_asset_loader(SeekableAudioLoader);
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<LoopingSeekableAudio>("test://track.WAV#looping");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.update();
            match app
                .world()
                .resource::<AssetServer>()
                .load_state(handle.id())
            {
                LoadState::Loaded => break,
                LoadState::Failed(error) => panic!("looping WAV did not load: {error}"),
                _ => assert!(std::time::Instant::now() < deadline, "audio load timed out"),
            }
            std::thread::yield_now();
        }
        let assets = app.world().resource::<Assets<LoopingSeekableAudio>>();
        assert_eq!(
            assets.get(&handle).unwrap().decoder().take(200).count(),
            200
        );
    }

    fn silent_pcm_wav(sample_rate: u32, sample_count: u32) -> Arc<[u8]> {
        let data_len = sample_count * 2;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.resize(44 + data_len as usize, 0);
        bytes.into()
    }

    #[test]
    fn byte_length_aware_decoder_reports_duration_and_seeks() {
        let bytes = silent_pcm_wav(8_000, 8_000);
        let mut decoder = seekable_decoder(bytes, "wav").expect("test WAV should decode");

        assert_eq!(decoder.total_duration(), Some(Duration::from_secs(1)));
        decoder
            .try_seek(Duration::from_millis(500))
            .expect("test WAV should seek");
        assert_eq!(decoder.next(), Some(0.0));
    }

    #[test]
    fn looped_decoder_rewinds_without_buffering_a_decoded_pass() {
        let bytes = silent_pcm_wav(8_000, 8_000);
        let samples = looping_seekable_decoder(bytes, "wav")
            .expect("test WAV should loop")
            .take(10_000)
            .count();

        assert_eq!(samples, 10_000);
    }

    #[test]
    fn audio_hint_is_case_insensitive() {
        assert_eq!(
            audio_hint(std::path::Path::new("BGM/TRACK.WAV")).unwrap(),
            "wav"
        );
    }
}

#[cfg(feature = "audio-opus")]
pub(crate) struct OpusStream {
    format: Option<Box<dyn FormatReader>>,
    decoder: Option<Box<dyn AudioDecoder>>,
    track_id: u32,
    samples: Vec<f32>,
    position: usize,
    channels: ChannelCount,
    sample_rate: SampleRate,
    ended: bool,
    duration: Option<Duration>,
    time_base: Option<TimeBase>,
    looping: bool,
    decoded_since_rewind: bool,
}

#[cfg(feature = "audio-opus")]
impl OpusStream {
    fn new(source: Box<dyn MediaSource>, looping: bool) -> Result<Self, SymphoniaError> {
        let stream = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        hint.with_extension("opus");
        let format_options = FormatOptions::default();
        let metadata_options = MetadataOptions::default();
        let format = symphonia::default::get_probe().probe(
            &hint,
            stream,
            format_options,
            metadata_options,
        )?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or(SymphoniaError::Unsupported("opus: no audio track"))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or(SymphoniaError::Unsupported("opus: invalid audio track"))?;
        let channels = ChannelCount::new(
            params
                .channels
                .as_ref()
                .map_or(2, |channels| channels.count() as u16),
        )
        .unwrap_or(ChannelCount::MIN);
        let sample_rate =
            SampleRate::new(params.sample_rate.unwrap_or(48_000)).unwrap_or(SampleRate::MIN);
        let track_id = track.id;
        let duration = track_duration(track);
        let time_base = track.time_base;

        let mut codecs = CodecRegistry::new();
        codecs.register_audio_decoder::<OpusDecoder>();
        let decoder = codecs.make_audio_decoder(params, &AudioDecoderOptions::default())?;

        Ok(Self {
            format: Some(format),
            decoder: Some(decoder),
            track_id,
            samples: Vec::new(),
            position: 0,
            channels,
            sample_rate,
            ended: false,
            duration,
            time_base,
            looping,
            decoded_since_rewind: false,
        })
    }

    fn failed() -> Self {
        Self {
            format: None,
            decoder: None,
            track_id: 0,
            samples: Vec::new(),
            position: 0,
            channels: ChannelCount::new(2).expect("stereo channel count is non-zero"),
            sample_rate: SampleRate::new(48_000).expect("Opus sample rate is non-zero"),
            ended: true,
            duration: None,
            time_base: None,
            looping: false,
            decoded_since_rewind: false,
        }
    }

    fn decode_next_packet(&mut self) -> bool {
        if self.format.is_none() || self.decoder.is_none() {
            self.ended = true;
            return false;
        }
        loop {
            let packet = match self
                .format
                .as_mut()
                .expect("format checked above")
                .next_packet()
            {
                Ok(Some(packet)) => packet,
                Ok(None) => {
                    if self.looping && self.decoded_since_rewind {
                        if let Err(error) = self.rewind() {
                            log::warn!("Ogg Opus loop rewind failed: {error}");
                            self.ended = true;
                            return false;
                        }
                        continue;
                    }
                    self.ended = true;
                    return false;
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.decoder
                        .as_mut()
                        .expect("decoder checked above")
                        .reset();
                    continue;
                }
                Err(error) => {
                    if !matches!(error, SymphoniaError::IoError(_)) {
                        log::warn!("Ogg Opus packet read failed: {error}");
                    }
                    self.ended = true;
                    return false;
                }
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self
                .decoder
                .as_mut()
                .expect("decoder checked above")
                .decode(&packet)
            {
                Ok(decoded) => {
                    self.channels = ChannelCount::new(decoded.spec().channels().count() as u16)
                        .unwrap_or(ChannelCount::MIN);
                    self.sample_rate =
                        SampleRate::new(decoded.spec().rate()).unwrap_or(SampleRate::MIN);
                    self.samples.resize(decoded.samples_interleaved(), f32::MID);
                    decoded.copy_to_slice_interleaved(&mut self.samples);
                    self.position = 0;
                    if !self.samples.is_empty() {
                        self.decoded_since_rewind = true;
                        return true;
                    }
                }
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(error) => {
                    log::warn!("Ogg Opus decode failed: {error}");
                    self.ended = true;
                    return false;
                }
            }
        }
    }

    fn rewind(&mut self) -> Result<(), SymphoniaError> {
        let format = self
            .format
            .as_mut()
            .ok_or(SymphoniaError::Unsupported("opus: missing format reader"))?;
        format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: SymphoniaTime::ZERO,
                track_id: Some(self.track_id),
            },
        )?;
        self.recreate_decoder()?;
        self.samples.clear();
        self.position = 0;
        self.ended = false;
        self.decoded_since_rewind = false;
        Ok(())
    }

    fn recreate_decoder(&mut self) -> Result<(), SymphoniaError> {
        // reset() clears codec history but does not restore the Ogg Opus
        // pre-skip, so returning to time zero needs a fresh decoder.
        let params = self
            .decoder
            .as_ref()
            .ok_or(SymphoniaError::Unsupported("opus: missing decoder"))?
            .codec_params()
            .clone();
        let mut codecs = CodecRegistry::new();
        codecs.register_audio_decoder::<OpusDecoder>();
        self.decoder = Some(codecs.make_audio_decoder(&params, &AudioDecoderOptions::default())?);
        Ok(())
    }
}

#[cfg(feature = "audio-opus")]
impl Iterator for OpusStream {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(sample) = self.samples.get(self.position).copied() {
                self.position += 1;
                return Some(sample);
            }
            if self.ended || !self.decode_next_packet() {
                return None;
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let buffered = self.samples.len().saturating_sub(self.position);
        (buffered, None)
    }
}

#[cfg(feature = "audio-opus")]
impl Source for OpusStream {
    fn current_span_len(&self) -> Option<usize> {
        self.ended.then_some(0)
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        (!self.looping).then_some(self.duration).flatten()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), rodio::source::SeekError> {
        let active_channel = self.position % usize::from(self.channels.get());
        let target = self
            .duration
            .map_or(position, |duration| position.min(duration));
        let seconds = i64::try_from(target.as_secs()).unwrap_or(i64::MAX);
        let time =
            SymphoniaTime::try_new(seconds, target.subsec_nanos()).unwrap_or(SymphoniaTime::MAX);
        let seeked = self
            .format
            .as_mut()
            .ok_or(rodio::source::SeekError::NotSupported {
                underlying_source: std::any::type_name::<Self>(),
            })?
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(opus_seek_error)?;

        let time_base = self.time_base;
        if target.is_zero() {
            self.recreate_decoder().map_err(opus_seek_error)?;
        } else {
            self.decoder
                .as_mut()
                .ok_or(rodio::source::SeekError::NotSupported {
                    underlying_source: std::any::type_name::<Self>(),
                })?
                .reset();
        }
        self.samples.clear();
        self.position = 0;
        self.ended = false;
        self.decoded_since_rewind = false;

        // The demuxer seeks to the nearest packet before the target. Decode
        // and discard that short remainder so dragging lands on the requested
        // sample rather than merely on the preceding Opus packet.
        let frames_to_skip = time_base
            .zip(seeked.required_ts.duration_from(seeked.actual_ts))
            .and_then(|(time_base, delta)| {
                Timestamp::try_from(delta.get())
                    .ok()
                    .map(|timestamp| (time_base, timestamp))
            })
            .and_then(|(time_base, delta)| time_base.calc_time(delta))
            .map_or(0usize, |delta| {
                (delta.as_secs_f64().max(0.0) * f64::from(self.sample_rate.get())).ceil() as usize
            });
        let samples_to_skip = frames_to_skip.saturating_mul(usize::from(self.channels.get()));
        for _ in 0..samples_to_skip.saturating_add(active_channel) {
            if self.next().is_none() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(feature = "audio-opus")]
fn opus_seek_error(error: SymphoniaError) -> rodio::source::SeekError {
    rodio::source::SeekError::Other(Arc::new(io::Error::other(error.to_string())))
}

#[cfg(feature = "audio-opus")]
fn track_duration(track: &symphonia::core::formats::Track) -> Option<Duration> {
    let time_base = track.time_base?;
    let ticks = i64::try_from(track.duration?.get()).ok()?;
    let nanos = time_base
        .calc_time(symphonia::core::units::Timestamp::new(ticks))?
        .as_nanos();
    u64::try_from(nanos).ok().map(Duration::from_nanos)
}

#[cfg(all(test, feature = "audio-opus"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn opus_stream(bytes: Arc<[u8]>) -> Result<OpusStream, SymphoniaError> {
        OpusStream::new(Box::new(Cursor::new(bytes)), false)
    }

    #[test]
    fn decodes_project_ogg_opus_incrementally() {
        let bytes: Arc<[u8]> = include_bytes!("../assets/audio/click.opus")
            .as_slice()
            .into();
        let mut stream = opus_stream(bytes).expect("test Opus asset should open");
        let duration = stream
            .total_duration()
            .expect("seekable Ogg Opus should expose its duration");
        let samples = stream.by_ref().take(4_800).collect::<Vec<_>>();

        assert_eq!(stream.channels().get(), 2);
        assert_eq!(stream.sample_rate().get(), 48_000);
        assert!(duration > Duration::from_millis(100));
        assert_eq!(samples.len(), 4_800);
        assert!(samples.iter().any(|sample| sample.abs() > f32::EPSILON));
    }

    #[test]
    fn decodes_embedded_webgal_k_ui_cues() {
        let cues: [(&[u8], f32); 3] = [
            (include_bytes!("../assets/audio/click.opus"), 0.25),
            (include_bytes!("../assets/audio/mouse-enter.opus"), 0.08),
            (include_bytes!("../assets/audio/switch.opus"), 0.25),
        ];
        for (cue, minimum_seconds) in cues {
            let bytes: Arc<[u8]> = cue.into();
            let mut stream = opus_stream(bytes).expect("UI cue should open");
            assert_eq!(stream.channels().get(), 2);
            assert_eq!(stream.sample_rate().get(), 48_000);
            let channels = stream.channels().get() as usize;
            let sample_rate = stream.sample_rate().get() as usize;
            let samples = stream.by_ref().collect::<Vec<_>>();
            assert!(samples.iter().any(|sample| *sample != 0.0));
            let seconds = samples.len() as f32 / channels as f32 / sample_rate as f32;
            assert!(seconds >= minimum_seconds, "decoded only {seconds:.3}s");
        }
    }

    #[test]
    fn opus_stream_can_seek_forward_and_back_to_the_start() {
        let bytes: Arc<[u8]> = include_bytes!("../assets/audio/click.opus")
            .as_slice()
            .into();
        let mut fresh = opus_stream(bytes.clone()).expect("test Opus asset should open");
        let duration = fresh.total_duration().expect("test Opus has a duration");
        let first_samples = fresh.by_ref().take(2_400).collect::<Vec<_>>();
        let full_sample_count = first_samples.len() + fresh.by_ref().count();

        let mut stream = opus_stream(bytes).expect("test Opus asset should open");
        stream
            .try_seek(duration / 2)
            .expect("forward seek should be supported");
        let remaining_sample_count = stream.by_ref().count();
        assert!(remaining_sample_count > full_sample_count / 4);
        assert!(remaining_sample_count < full_sample_count * 3 / 4);
        stream
            .try_seek(Duration::ZERO)
            .expect("rewind should be supported");
        let actual = stream.by_ref().take(2_400).collect::<Vec<_>>();
        assert_eq!(actual.len(), 2_400);
        assert!(
            actual
                .iter()
                .zip(first_samples)
                .all(|(seeked, first)| (seeked - first).abs() < 1e-6),
            "seeking to zero must restore the initial Opus pre-skip"
        );
    }

    #[test]
    fn looping_opus_stream_rewinds_without_buffering_a_decoded_pass() {
        let bytes: Arc<[u8]> = include_bytes!("../assets/audio/click.opus")
            .as_slice()
            .into();
        let first_pass = opus_stream(bytes.clone())
            .expect("test Opus asset should open")
            .collect::<Vec<_>>();
        let one_pass = first_pass.len();
        let mut looping = OpusStream::new(Box::new(Cursor::new(bytes)), true)
            .expect("looping test Opus asset should open");
        let samples = looping.by_ref().take(one_pass + 2_400).collect::<Vec<_>>();

        assert_eq!(samples.len(), one_pass + 2_400);
        assert!(
            samples[one_pass..]
                .iter()
                .zip(&first_pass[..2_400])
                .all(|(looped, first)| (looped - first).abs() < 1e-6),
            "looped Opus playback must restart at the same decoded sample"
        );
        assert_eq!(looping.total_duration(), None);
    }

    struct CountingSource {
        cursor: Cursor<Vec<u8>>,
        read: Arc<AtomicUsize>,
    }

    impl Read for CountingSource {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = self.cursor.read(buffer)?;
            self.read.fetch_add(count, Ordering::Relaxed);
            Ok(count)
        }
    }

    impl Seek for CountingSource {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.cursor.seek(position)
        }
    }

    impl MediaSource for CountingSource {
        fn is_seekable(&self) -> bool {
            true
        }

        fn byte_len(&self) -> Option<u64> {
            Some(self.cursor.get_ref().len() as u64)
        }
    }

    #[test]
    fn initial_opus_playback_does_not_read_the_complete_asset() {
        const MINIMUM_LEN: usize = 16 * 1024 * 1024;
        let cue = include_bytes!("../assets/audio/click.opus");
        let mut bytes = Vec::with_capacity(MINIMUM_LEN + cue.len());
        while bytes.len() < MINIMUM_LEN {
            // Ogg explicitly permits chained logical bitstreams. Repeating a
            // complete valid stream produces a large valid container without
            // checking a generated binary fixture into the repository.
            bytes.extend_from_slice(cue);
        }
        let total_len = bytes.len();
        let read = Arc::new(AtomicUsize::new(0));
        let source = CountingSource {
            cursor: Cursor::new(bytes),
            read: read.clone(),
        };
        let mut stream =
            OpusStream::new(Box::new(source), false).expect("test Opus asset should open");
        assert_eq!(stream.by_ref().take(4_800).count(), 4_800);
        let bytes_read = read.load(Ordering::Relaxed);
        eprintln!("large Opus startup read: {bytes_read} / {total_len} bytes");
        assert!(
            bytes_read < 512 * 1024,
            "startup read the complete padded asset"
        );
    }
}
