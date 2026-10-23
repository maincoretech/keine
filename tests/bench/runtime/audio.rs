//! Opt-in, device-free measurement of the production mounted Opus decoder.
use super::*;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[derive(Default)]
struct Reads {
    bytes: AtomicU64,
    calls: AtomicU64,
    nanos: AtomicU64,
}

struct MeasuredSource {
    source: Box<dyn MediaSource>,
    reads: Arc<Reads>,
}

impl Read for MeasuredSource {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let started = Instant::now();
        let read = self.source.read(buffer)?;
        self.reads
            .nanos
            .fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.reads.bytes.fetch_add(read as u64, Ordering::Relaxed);
        self.reads.calls.fetch_add(1, Ordering::Relaxed);
        Ok(read)
    }
}

impl Seek for MeasuredSource {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.source.seek(position)
    }
}

impl MediaSource for MeasuredSource {
    fn is_seekable(&self) -> bool {
        self.source.is_seekable()
    }
    fn byte_len(&self) -> Option<u64> {
        self.source.byte_len()
    }
}

fn open(source: &OpusSource, reads: &Arc<Reads>) -> Box<dyn MediaSource> {
    Box::new(MeasuredSource {
        source: source.open().unwrap(),
        reads: reads.clone(),
    })
}

fn files(root: &Path, result: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            files(&entry.path(), result);
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "opus") {
            result.push(entry.path());
        }
    }
}

#[test]
#[ignore = "set KEINE_VOICE_BENCH to a read-only Opus voice directory"]
fn mounted_voice_reads() {
    use hakutaku_pack::{Identity, PackOptions, pack_directory};
    use keine_loader::{ContentBackend, HakutakuArchive};
    let root = PathBuf::from(std::env::var_os("KEINE_VOICE_BENCH").expect("voice directory"));
    let mut paths = Vec::new();
    files(&root, &mut paths);
    paths.sort_by_key(|path| (std::fs::metadata(path).unwrap().len(), path.clone()));
    assert!(!paths.is_empty());
    let count = paths.len().min(96);
    let selected: Vec<_> = (0..count)
        .map(|index| &paths[index * (paths.len() - 1) / count.max(2).saturating_sub(1)])
        .collect();
    let fixture = tempfile::tempdir().unwrap();
    let input = fixture.path().join("input");
    let release = fixture.path().join("release");
    std::fs::create_dir(&input).unwrap();
    let mut logical = 0;
    for (index, path) in selected.iter().enumerate() {
        logical += std::fs::copy(path, input.join(format!("voice-{index:04}.opus"))).unwrap();
    }
    let identity = Identity::generate().unwrap();
    pack_directory(&PackOptions::new(&input, &release), &identity).unwrap();
    let keys = identity.runtime_key_material().unwrap();
    println!(
        "voice_fixture available={} selected={count} bytes={logical}",
        paths.len()
    );
    let mut expected_pcm = None;
    for backend in ["filesystem", "hakutaku"] {
        for pass in 0..5 {
            // Fresh package caches per pass; the host filesystem cache stays warm.
            let backend_value = if backend == "filesystem" {
                ContentBackend::FileSystem(input.clone())
            } else {
                ContentBackend::Hakutaku(
                    HakutakuArchive::open_with_keys(
                        &release.join("game.haku"),
                        keys.root_key(),
                        keys.public_key,
                        hakutaku_core::OpenPolicy::TrustFirstRelease,
                    )
                    .unwrap(),
                )
            };
            let mounts: Arc<[ContentMount]> =
                vec![ContentMount::new(backend_value, "").unwrap()].into();
            let reads = Arc::new(Reads::default());
            let mut probe_time = Duration::ZERO;
            let mut first_time = Duration::ZERO;
            let mut rest_time = Duration::ZERO;
            let mut first_bytes = 0;
            let mut samples = 0_u64;
            let mut checksum = 0_u64;
            for index in 0..count {
                let samples_before = samples;
                let source = OpusSource::Mounted {
                    mounts: mounts.clone(),
                    path: format!("voice-{index:04}.opus").into(),
                };
                let started = Instant::now();
                // The AssetLoader probes once, then Decodable opens a fresh stream.
                let probe = OpusStream::new(open(&source, &reads), false).unwrap();
                let duration = probe.total_duration();
                drop(probe);
                probe_time += started.elapsed();
                let started = Instant::now();
                let mut stream = OpusStream::new(open(&source, &reads), false).unwrap();
                assert_eq!(stream.total_duration(), duration);
                let first =
                    usize::from(stream.channels().get()) * stream.sample_rate().get() as usize / 50;
                let before_bytes = reads.bytes.load(Ordering::Relaxed);
                for sample in stream.by_ref().take(first) {
                    checksum =
                        checksum.wrapping_add(u64::from(std::hint::black_box(sample).to_bits()));
                    samples += 1;
                }
                first_time += started.elapsed();
                first_bytes += reads.bytes.load(Ordering::Relaxed) - before_bytes;
                let started = Instant::now();
                for sample in stream.by_ref() {
                    checksum =
                        checksum.wrapping_add(u64::from(std::hint::black_box(sample).to_bits()));
                    samples += 1;
                }
                assert!(stream.ended);
                assert!(samples > samples_before, "voice-{index:04} produced no PCM");
                rest_time += started.elapsed();
            }
            let actual_pcm = (samples, checksum);
            if let Some(expected) = expected_pcm {
                assert_eq!(actual_pcm, expected, "PCM changed across backends/passes");
            } else {
                expected_pcm = Some(actual_pcm);
            }
            println!(
                "voice_read backend={backend} pass={pass} probe_ms={:.6} first_ms={:.6} rest_ms={:.6} read_ms={:.6} bytes={} calls={} first_decode_bytes={first_bytes} samples={samples} checksum={checksum}",
                probe_time.as_secs_f64() * 1000.0,
                first_time.as_secs_f64() * 1000.0,
                rest_time.as_secs_f64() * 1000.0,
                reads.nanos.load(Ordering::Relaxed) as f64 / 1e6,
                reads.bytes.load(Ordering::Relaxed),
                reads.calls.load(Ordering::Relaxed)
            );
        }
    }
}
