//! Natural one-shot tail fades run on decoded samples, without waking the UI.
use super::*;

#[derive(Clone)]
enum TailSource {
    #[cfg(feature = "audio-opus")]
    Opus(Handle<OpusAudio>),
    #[cfg(feature = "audio-seekable")]
    Seekable(Handle<SeekableAudio>),
    #[cfg(not(feature = "audio-seekable"))]
    Fallback(Handle<AudioSource>),
}

#[derive(Component)]
struct PendingTail {
    source: TailSource,
    seconds: f32,
}

#[derive(Asset, TypePath)]
struct TailAudio {
    source: TailInput,
    // Retain the original asset while its decoder is in use; the cloned input
    // shares compressed bytes rather than taking ownership out of AssetServer.
    _source_handle: TailSource,
    seconds: f32,
}

#[derive(TypePath)]
enum TailInput {
    #[cfg(feature = "audio-opus")]
    Opus(OpusAudio),
    #[cfg(feature = "audio-seekable")]
    Seekable(SeekableAudio),
    #[cfg(not(feature = "audio-seekable"))]
    Fallback(AudioSource),
}

impl Decodable for TailAudio {
    type Decoder = Box<dyn Source + Send>;

    fn decoder(&self) -> Self::Decoder {
        let input: Box<dyn Source + Send> = match &self.source {
            #[cfg(feature = "audio-opus")]
            TailInput::Opus(audio) => Box::new(Decodable::decoder(audio)),
            #[cfg(feature = "audio-seekable")]
            TailInput::Seekable(audio) => Decodable::decoder(audio),
            #[cfg(not(feature = "audio-seekable"))]
            TailInput::Fallback(audio) => Box::new(audio.decoder()),
        };
        Box::new(TailFade::new(input, self.seconds))
    }
}

pub(super) fn configure(app: &mut App) {
    app.add_audio_source::<TailAudio>()
        .add_systems(PreUpdate, prepare);
}

pub(super) fn insert(
    entity: &mut EntityCommands<'_>,
    server: &AssetServer,
    path: String,
    settings: PlaybackSettings,
    seconds: f32,
) {
    #[cfg(feature = "audio-opus")]
    if is_opus(&path) {
        entity.insert((
            PendingTail {
                source: TailSource::Opus(server.load(path)),
                seconds,
            },
            settings,
        ));
        return;
    }
    #[cfg(feature = "audio-seekable")]
    let source = TailSource::Seekable(server.load(path));
    #[cfg(not(feature = "audio-seekable"))]
    let source = TailSource::Fallback(server.load(path));
    entity.insert((PendingTail { source, seconds }, settings));
}

fn prepare(
    mut commands: Commands,
    server: Res<AssetServer>,
    pending: Query<(Entity, &PendingTail)>,
    mut tails: ResMut<Assets<TailAudio>>,
    #[cfg(feature = "audio-opus")] opus: Res<Assets<OpusAudio>>,
    #[cfg(feature = "audio-seekable")] seekable: Res<Assets<SeekableAudio>>,
    #[cfg(not(feature = "audio-seekable"))] fallback: Res<Assets<AudioSource>>,
) {
    for (entity, request) in &pending {
        let input = match &request.source {
            #[cfg(feature = "audio-opus")]
            TailSource::Opus(handle) => opus.get(handle).cloned().map(TailInput::Opus),
            #[cfg(feature = "audio-seekable")]
            TailSource::Seekable(handle) => seekable.get(handle).cloned().map(TailInput::Seekable),
            #[cfg(not(feature = "audio-seekable"))]
            TailSource::Fallback(handle) => fallback.get(handle).cloned().map(TailInput::Fallback),
        };
        let handle = match &request.source {
            #[cfg(feature = "audio-opus")]
            TailSource::Opus(handle) => handle.id().untyped(),
            #[cfg(feature = "audio-seekable")]
            TailSource::Seekable(handle) => handle.id().untyped(),
            #[cfg(not(feature = "audio-seekable"))]
            TailSource::Fallback(handle) => handle.id().untyped(),
        };
        if matches!(server.load_state(handle), bevy::asset::LoadState::Failed(_)) {
            commands.entity(entity).despawn();
            continue;
        }
        if let Some(source) = input {
            commands
                .entity(entity)
                .remove::<PendingTail>()
                .insert(AudioPlayer(tails.add(TailAudio {
                    source,
                    _source_handle: request.source.clone(),
                    seconds: request.seconds,
                })));
        }
    }
}

struct TailFade<S> {
    input: S,
    frame: u64,
    channel: u16,
    frames: Option<f64>,
    fade_frames: f64,
}

impl<S: Source> TailFade<S> {
    fn new(input: S, seconds: f32) -> Self {
        let rate = f64::from(input.sample_rate().get());
        let frames = input
            .total_duration()
            .map(|duration| duration.as_secs_f64() * rate);
        if frames.is_none() {
            log::warn!("audio duration unavailable; natural tail fade cannot be scheduled");
        }
        Self {
            input,
            frame: 0,
            channel: 0,
            frames,
            fade_frames: f64::from(seconds.max(0.0)) * rate,
        }
    }
}

impl<S: Source> Iterator for TailFade<S> {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.input.next()?;
        let gain = self
            .frames
            .filter(|frames| *frames > 0.0 && self.fade_frames > 0.0)
            .map_or(1.0, |frames| {
                ((frames - self.frame as f64) / self.fade_frames.min(frames)).clamp(0.0, 1.0) as f32
            });
        self.channel += 1;
        if self.channel >= self.input.channels().get() {
            self.channel = 0;
            self.frame += 1;
        }
        Some(sample * gain)
    }
}

impl<S: Source> Source for TailFade<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.input.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.input.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
    fn try_seek(&mut self, position: Duration) -> Result<(), rodio::source::SeekError> {
        self.input.try_seek(position)?;
        self.frame = (position.as_secs_f64() * f64::from(self.input.sample_rate().get())) as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_tail_preserves_length_and_fades_only_the_end_including_seek() {
        let input = rodio::buffer::SamplesBuffer::new(
            2.try_into().unwrap(),
            100.try_into().unwrap(),
            vec![1.0; 200],
        );
        let mut faded = TailFade::new(input, 0.25);
        let samples: Vec<_> = faded.by_ref().collect();
        assert_eq!(samples.len(), 200);
        assert!(samples[..150].iter().all(|sample| *sample == 1.0));
        for pair in samples.chunks_exact(2) {
            assert_eq!(pair[0], pair[1]);
        }
        assert!((samples[180] - 0.4).abs() < 0.001);
        assert!(samples[198] < 0.05);
        faded.try_seek(Duration::from_millis(900)).unwrap();
        assert!((faded.next().unwrap() - 0.4).abs() < 0.001);
        assert!((faded.next().unwrap() - 0.4).abs() < 0.001);
        faded.try_seek(Duration::ZERO).unwrap();
        assert_eq!(faded.next(), Some(1.0));
    }

    #[test]
    fn short_clip_and_zero_tail_are_finite_and_keep_all_samples() {
        for fade in [0.0, 10.0] {
            let input = rodio::buffer::SamplesBuffer::new(
                1.try_into().unwrap(),
                100.try_into().unwrap(),
                vec![1.0; 10],
            );
            let samples: Vec<_> = TailFade::new(input, fade).collect();
            assert_eq!(samples.len(), 10);
            assert_eq!(samples[0], 1.0);
            assert!(samples.iter().all(|sample| sample.is_finite()));
            if fade == 0.0 {
                assert!(samples.iter().all(|sample| *sample == 1.0));
            } else {
                assert!(samples[9] < 0.11);
            }
        }
    }
}
