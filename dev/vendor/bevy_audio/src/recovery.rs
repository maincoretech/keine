//! Persistent logical mixer with a replaceable default-device stream.
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::time::Duration;
use std::sync::{Mutex, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use bevy_ecs::prelude::Resource;
use rodio::cpal::{
    self,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rodio::{
    ChannelCount, SampleRate,
    conversions::{ChannelCountConverter, SampleRateConverter},
    mixer::{Mixer, MixerSource},
};

const CHANNELS: ChannelCount = ChannelCount::new(2).unwrap();
const RATE: SampleRate = SampleRate::new(48_000).unwrap();
const POLL: Duration = Duration::from_millis(250);
const ROUTE_POLL: Duration = Duration::from_secs(1);
const STALL: Duration = Duration::from_secs(3);

type Converted = ChannelCountConverter<SampleRateConverter<IdleMixer>>;

// MixerSource returns None while idle. The hardware stream must remain alive
// and be able to play sources queued later without recreating any Players.
struct IdleMixer(MixerSource);
impl Iterator for IdleMixer {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        Some(self.0.next().unwrap_or(0.0))
    }
}

struct Reader {
    samples: Option<Converted>,
    channels: u16,
    rate: u32,
}
impl Reader {
    fn new(source: MixerSource) -> Self {
        Self {
            samples: Some(ChannelCountConverter::new(
                SampleRateConverter::new(IdleMixer(source), RATE, RATE, CHANNELS),
                CHANNELS,
                CHANNELS,
            )),
            channels: CHANNELS.get(),
            rate: RATE.get(),
        }
    }
    fn configure(&mut self, config: &cpal::StreamConfig) -> Result<(), String> {
        let channels = ChannelCount::new(config.channels).ok_or("zero output channels")?;
        let rate = SampleRate::new(config.sample_rate).ok_or("zero output sample rate")?;
        if self.channels != config.channels || self.rate != config.sample_rate {
            // Preserve the MixerSource and all Player/decoder state. Only a few
            // resampler look-ahead samples may be discarded when the physical
            // format changes; hardware latency cannot be recovered either.
            let source = self
                .samples
                .take()
                .expect("reader retains source")
                .into_inner()
                .into_inner();
            self.samples = Some(ChannelCountConverter::new(
                SampleRateConverter::new(source, RATE, rate, CHANNELS),
                CHANNELS,
                channels,
            ));
            self.channels = config.channels;
            self.rate = config.sample_rate;
        }
        Ok(())
    }
}

struct Signals {
    available: AtomicBool,
    failed: AtomicBool,
    callbacks: AtomicU64,
    played_nanos: AtomicU64,
    wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}
impl Signals {
    fn new() -> Self {
        Self {
            available: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            callbacks: AtomicU64::new(0),
            played_nanos: AtomicU64::new(0),
            wake: Mutex::new(None),
        }
    }
    fn notify(&self) {
        let wake = self
            .wake
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(wake) = wake {
            wake();
        }
    }
}

struct Owner {
    mixer: Mixer,
    signals: Arc<Signals>,
    stop: mpsc::Sender<()>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self
            .worker
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = worker.join();
        }
    }
}

/// Shared output owner. Player controls and decoders survive hardware changes.
///
/// The worker closes and reopens streams outside CPAL's realtime/error callbacks.
/// No default endpoint is replaced by an arbitrary fallback device.
#[derive(Resource, Clone)]
pub struct AudioOutput(Arc<Owner>);
impl AudioOutput {
    /// Stable logical mixer shared by game playback and authoring audition.
    pub fn mixer(&self) -> &Mixer {
        &self.0.mixer
    }
    /// Whether a default output is currently consuming audio samples.
    pub fn is_available(&self) -> bool {
        self.0.signals.available.load(Ordering::Acquire)
    }
    /// Time actually submitted to a healthy stream. Suspended streams do not
    /// consume fade duration, even while the game or host event loop is idle.
    pub fn playback_time(&self) -> Duration {
        Duration::from_nanos(self.0.signals.played_nanos.load(Ordering::Relaxed))
    }
    /// Wake a sleeping native event loop when availability changes. Invoked on
    /// the recovery worker, never from a realtime or error callback.
    pub fn set_wake_callback(&self, wake: impl Fn() + Send + Sync + 'static) {
        *self
            .0
            .signals
            .wake
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(wake));
        self.0.signals.notify();
    }
}
impl Default for AudioOutput {
    fn default() -> Self {
        let (mixer, source) = rodio::mixer::mixer(CHANNELS, RATE);
        let reader = Arc::new(Mutex::new(Reader::new(source)));
        let signals = Arc::new(Signals::new());
        let (stop, stopped) = mpsc::channel();
        let worker_signals = Arc::clone(&signals);
        let worker = thread::Builder::new()
            .name("audio-output".into())
            .spawn(move || {
                run(reader, worker_signals, stopped);
            })
            .inspect_err(|error| tracing::warn!("Unable to start audio output worker: {error}"))
            .ok();
        Self(Arc::new(Owner {
            mixer,
            signals,
            stop,
            worker: Mutex::new(worker),
        }))
    }
}

#[derive(Default)]
struct Retry {
    failures: u32,
    after: Option<Instant>,
}
impl Retry {
    fn ready(&self, now: Instant) -> bool {
        self.after.is_none_or(|after| now >= after)
    }
    fn failed(&mut self, now: Instant) {
        let millis = (250u64 << self.failures.min(5)).min(5_000);
        self.failures = self.failures.saturating_add(1);
        self.after = Some(now + Duration::from_millis(millis));
    }
    fn recovered(&mut self) {
        *self = Self::default();
    }
}

fn run(reader: Arc<Mutex<Reader>>, signals: Arc<Signals>, stopped: mpsc::Receiver<()>) {
    let host = cpal::default_host();
    let mut stream = None;
    let mut route = None;
    let mut retry = Retry::default();
    let mut next_route_poll = Instant::now();
    let mut last_progress = Instant::now();
    let mut callbacks = 0;
    let mut opened_at = Instant::now();
    loop {
        let now = Instant::now();
        let count = signals.callbacks.load(Ordering::Relaxed);
        if count != callbacks {
            callbacks = count;
            last_progress = now;
        }
        let failure = signals.failed.swap(false, Ordering::AcqRel);
        let stalled = stream.is_some() && now.duration_since(last_progress) >= STALL;
        let mut device = None;
        let mut route_changed = false;
        if now >= next_route_poll || stream.is_none() && retry.ready(now) {
            device = host.default_output_device();
            let current_route = device.as_ref().and_then(|d| d.id().ok());
            // CoreAudio DefaultOutput follows system routing itself. Rebuilding
            // a healthy stream on an endpoint-name change causes a needless gap.
            // Android's unbound AAudio device reports disconnect via callback.
            route_changed = stream.is_some()
                && default_route_changed(
                    cfg!(target_os = "windows"),
                    device.is_some(),
                    route.as_ref(),
                    current_route.as_ref(),
                );
            next_route_poll = now + ROUTE_POLL;
        }
        if failure || stalled || route_changed {
            signals.available.store(false, Ordering::Release);
            signals.notify();
            drop(stream.take()); // never stop/close in an error callback
            if route_changed {
                retry.recovered();
            } else {
                retry.failed(now);
            }
        }
        if stream.is_some()
            && !failure
            && !stalled
            && now.duration_since(last_progress) < STALL
            && retry.after.is_none()
            && now.duration_since(opened_at) > STALL
        {
            retry.recovered();
        }
        if stream.is_none() && retry.ready(now) {
            signals.failed.store(false, Ordering::Release);
            let device = device.or_else(|| host.default_output_device());
            let result = device
                .as_ref()
                .ok_or_else(|| "no default audio output".to_string())
                .and_then(|device| open(device, Arc::clone(&reader), Arc::clone(&signals)));
            match result {
                Ok(opened) => {
                    route = device.as_ref().and_then(|d| d.id().ok());
                    stream = Some(opened);
                    last_progress = now;
                    opened_at = now;
                    signals.available.store(true, Ordering::Release);
                    if retry.failures != 0 {
                        tracing::info!("Default audio output recovered");
                    }
                    retry.after = None;
                    signals.notify();
                }
                Err(error) => {
                    if retry.failures == 0 {
                        tracing::warn!("Audio output unavailable; retaining playback: {error}");
                    }
                    retry.failed(now);
                }
            }
        }
        match stopped.recv_timeout(POLL) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => break,
        }
    }
    signals.available.store(false, Ordering::Release);
    drop(stream);
}

fn open(
    device: &cpal::Device,
    reader: Arc<Mutex<Reader>>,
    signals: Arc<Signals>,
) -> Result<cpal::Stream, String> {
    let supported = device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    let config = supported.config();
    reader
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .configure(&config)?;
    macro_rules! formats {
        ($($variant:ident => $sample:ty),+ $(,)?) => {
            match supported.sample_format() {
                $(cpal::SampleFormat::$variant => build::<$sample>(device, &config, reader, signals),)+
                format => Err(format!("unsupported output sample format {format}")),
            }
        };
    }
    let stream = formats! {
        F32 => f32, F64 => f64, I8 => i8, I16 => i16, I24 => cpal::I24,
        I32 => i32, I64 => i64, U8 => u8, U16 => u16, U24 => cpal::U24, U32 => u32, U64 => u64,
    }?;
    stream.play().map_err(|error| error.to_string())?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    reader: Arc<Mutex<Reader>>,
    signals: Arc<Signals>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let error_signals = Arc::clone(&signals);
    let channels = u64::from(config.channels);
    let sample_rate = u64::from(config.sample_rate);
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data, _| render(data, &reader, &signals, channels, sample_rate),
            move |_error| {
                // Coalesce error bursts; no logging, allocation, close or reopen here.
                error_signals.available.store(false, Ordering::Release);
                error_signals.failed.store(true, Ordering::Release);
            },
            None,
        )
        .map_err(|error| error.to_string())
}

fn render<T: cpal::SizedSample + cpal::FromSample<f32>>(
    data: &mut [T],
    reader: &Mutex<Reader>,
    signals: &Signals,
    channels: u64,
    rate: u64,
) {
    signals.callbacks.fetch_add(1, Ordering::Relaxed);
    if signals.available.load(Ordering::Acquire)
        && !signals.failed.load(Ordering::Acquire)
        && let Ok(mut reader) = reader.try_lock()
    {
        // One uncontended lock per hardware buffer, never one per sample.
        let source = reader.samples.as_mut().expect("reader retains source");
        signals.played_nanos.fetch_add(
            data.len() as u64 / channels * 1_000_000_000 / rate,
            Ordering::Relaxed,
        );
        for sample in data {
            *sample = T::from_sample(source.next().unwrap_or(0.0));
        }
        return;
    }
    data.fill(T::EQUILIBRIUM);
}

fn default_route_changed(
    follow_endpoint: bool,
    device_exists: bool,
    before: Option<&cpal::DeviceId>,
    now: Option<&cpal::DeviceId>,
) -> bool {
    !device_exists
        || (follow_endpoint && before.zip(now).is_some_and(|(before, now)| before != now))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_is_bounded_and_recovery_resets_it() {
        let now = Instant::now();
        let mut retry = Retry::default();
        for index in 0..20 {
            retry.failed(now);
            let expected = (250u64 << index.min(5)).min(5_000);
            assert_eq!(retry.after, Some(now + Duration::from_millis(expected)));
            assert!(!retry.ready(now));
            assert!(retry.ready(now + Duration::from_millis(expected)));
        }
        retry.recovered();
        assert!(retry.ready(now));
        assert_eq!(retry.failures, 0);
    }
    #[test]
    fn persistent_mixer_survives_idle_and_format_change_without_replacing_players() {
        let (mixer, source) = rodio::mixer::mixer(CHANNELS, RATE);
        let mut reader = Reader::new(source);
        assert_eq!(reader.samples.as_mut().unwrap().next(), Some(0.0));
        let player = rodio::Player::connect_new(&mixer);
        player.append(rodio::buffer::SamplesBuffer::new(
            CHANNELS,
            RATE,
            vec![0.5; 48_000],
        ));
        player.set_volume(0.5);
        assert!((0..4).any(|_| reader.samples.as_mut().unwrap().next() == Some(0.25)));
        player.pause();
        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: 44_100,
            buffer_size: cpal::BufferSize::Default,
        };
        reader.configure(&config).unwrap();
        assert!(player.is_paused());
        assert_eq!(player.volume(), 0.5);
        assert!(!player.empty());
        player.play();
        // The same decoder resumes; switching did not detach its Player.
        assert!((0..1_000).any(|_| reader.samples.as_mut().unwrap().next() == Some(0.25)));
    }
    #[test]
    fn outage_stops_decoding_and_same_player_resumes_after_device_returns() {
        let (mixer, source) = rodio::mixer::mixer(CHANNELS, RATE);
        let reader = Mutex::new(Reader::new(source));
        let player = rodio::Player::connect_new(&mixer);
        player.append(rodio::buffer::SamplesBuffer::new(
            CHANNELS,
            RATE,
            vec![0.5; 480_000],
        ));
        let signals = Signals::new();
        let mut buffer = [1.0f32; 9_600];
        render(&mut buffer, &reader, &signals, 2, 48_000);
        assert!(buffer.iter().all(|sample| *sample == 0.0));
        assert_eq!(player.get_pos(), Duration::ZERO);
        signals.available.store(true, Ordering::Release);
        render(&mut buffer, &reader, &signals, 2, 48_000);
        let position = player.get_pos();
        assert!(position > Duration::ZERO);
        signals.failed.store(true, Ordering::Release);
        let submitted = signals.played_nanos.load(Ordering::Relaxed);
        for _ in 0..20 {
            render(&mut buffer, &reader, &signals, 2, 48_000);
        }
        assert_eq!(player.get_pos(), position);
        assert_eq!(signals.played_nanos.load(Ordering::Relaxed), submitted);
        player.pause();
        signals.failed.store(false, Ordering::Release);
        render(&mut buffer, &reader, &signals, 2, 48_000);
        let paused_position = player.get_pos();
        // Rodio applies Player controls on its existing 5ms control boundary.
        assert!(paused_position <= position + Duration::from_millis(5));
        render(&mut buffer, &reader, &signals, 2, 48_000);
        assert_eq!(player.get_pos(), paused_position);
        player.play();
        render(&mut buffer, &reader, &signals, 2, 48_000);
        assert!(player.get_pos() > paused_position);
        assert!(buffer.contains(&0.5));
    }

    #[test]
    fn default_endpoint_change_without_error_reopens_windows_but_not_healthy_coreaudio() {
        let old = cpal::DeviceId(cpal::default_host().id(), "old".into());
        let new = cpal::DeviceId(cpal::default_host().id(), "new".into());
        assert!(default_route_changed(true, true, Some(&old), Some(&new)));
        assert!(!default_route_changed(false, true, Some(&old), Some(&new)));
        assert!(!default_route_changed(true, true, Some(&old), Some(&old)));
        assert!(!default_route_changed(true, true, Some(&old), None));
        assert!(default_route_changed(false, false, Some(&old), None));
    }
}
