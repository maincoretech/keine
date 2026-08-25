//! Portable Hakutaku I/O coverage for self-running benchmark bundles.
//!
//! The render suite exercises valid project WebP/Opus assets. This module adds
//! an unreferenced deterministic payload only to benchmark packages so a single
//! download can also expose storage, authentication, cache-admission and
//! concurrent-reader bottlenecks without affecting normal bundles.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use keine_loader::{ContentBackend, ContentMount, ContentProject, LoaderRegistry};

const FIXTURE_DIRECTORY: &str = "__keine_benchmark__";
const IO_BUFFER_BYTES: usize = 256 * 1024;
const RANDOM_READ_BYTES: usize = 4 * 1024;
const RANDOM_READS: usize = 512;

#[cfg(feature = "publisher")]
const HOT_FILES: usize = 32;
#[cfg(feature = "publisher")]
const HOT_FILE_BYTES: usize = 8 * 1024;
#[cfg(feature = "publisher")]
const NORMAL_FILES: usize = 32;
#[cfg(feature = "publisher")]
const NORMAL_FILE_BYTES: usize = 256 * 1024;
#[cfg(feature = "publisher")]
const TRANSIENT_FILES: usize = 16;
#[cfg(feature = "publisher")]
const TRANSIENT_FILE_BYTES: usize = 256 * 1024;
#[cfg(feature = "publisher")]
const STREAMING_FILES: usize = 6;
#[cfg(feature = "publisher")]
const STREAMING_FILE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
struct AssetEntry {
    mount: ContentMount,
    path: PathBuf,
    len: u64,
}

#[derive(Clone, Copy)]
struct ReadSample {
    bytes: u64,
    elapsed: Duration,
}

impl ReadSample {
    fn mib_per_second(self) -> f64 {
        self.bytes as f64 / 1_048_576.0 / self.elapsed.as_secs_f64().max(f64::EPSILON)
    }
}

/// Adds a stable, incompressible four-access-class payload to benchmark builds.
/// The files are intentionally not valid media because codec performance is
/// measured by the real project workloads; only Hakutaku layout and I/O are in
/// scope here.
#[cfg(feature = "publisher")]
pub(crate) fn stage_payload(staged_project: &Path) -> Result<()> {
    use std::io::{BufWriter, Write};

    let root = staged_project.join("assets").join(FIXTURE_DIRECTORY);
    if root.exists() {
        bail!(
            "project assets reserve {} for Kēne benchmark data",
            root.display()
        );
    }

    let groups = [
        ("hot", HOT_FILES, HOT_FILE_BYTES, "blob"),
        ("normal", NORMAL_FILES, NORMAL_FILE_BYTES, "blob"),
        ("transient", TRANSIENT_FILES, TRANSIENT_FILE_BYTES, "opus"),
        ("streaming", STREAMING_FILES, STREAMING_FILE_BYTES, "webm"),
    ];
    for (group_index, (group, count, bytes, extension)) in groups.into_iter().enumerate() {
        let directory = root.join(group);
        fs::create_dir_all(&directory)?;
        for index in 0..count {
            let path = directory.join(format!("{group}-{index:02}.{extension}"));
            let file = fs::File::create(&path)
                .with_context(|| format!("failed to create {}", path.display()))?;
            let mut writer = BufWriter::with_capacity(1024 * 1024, file);
            let mut state =
                0x6a09_e667_f3bc_c909_u64 ^ ((group_index as u64 + 1) << 48) ^ index as u64;
            let mut remaining = bytes;
            let mut buffer = vec![0_u8; (1024 * 1024).min(bytes)];
            while remaining > 0 {
                let chunk_len = remaining.min(buffer.len());
                fill_deterministic(&mut buffer[..chunk_len], &mut state);
                writer.write_all(&buffer[..chunk_len])?;
                remaining -= chunk_len;
            }
            writer.flush()?;
        }
    }
    let total = HOT_FILES * HOT_FILE_BYTES
        + NORMAL_FILES * NORMAL_FILE_BYTES
        + TRANSIENT_FILES * TRANSIENT_FILE_BYTES
        + STREAMING_FILES * STREAMING_FILE_BYTES;
    println!(
        "benchmark Hakutaku payload: {:.1} MiB across hot, normal, transient, and streaming classes",
        total as f64 / 1_048_576.0,
    );
    Ok(())
}

#[cfg(feature = "publisher")]
fn fill_deterministic(output: &mut [u8], state: &mut u64) {
    for chunk in output.chunks_mut(8) {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        chunk.copy_from_slice(&state.to_le_bytes()[..chunk.len()]);
    }
}

pub(super) fn run(project_path: &Path, loader: &LoaderRegistry) -> Result<String> {
    let opened_at = Instant::now();
    let content = super::bootstrap::open_project(project_path, loader)?.content;
    let open_elapsed = opened_at.elapsed();
    storage_report(project_path, &content, open_elapsed)
}

fn storage_report(
    project_path: &Path,
    content: &ContentProject,
    open_elapsed: Duration,
) -> Result<String> {
    let inventory_at = Instant::now();
    let entries = effective_assets(content.asset_mounts())?;
    let inventory_elapsed = inventory_at.elapsed();
    let mut report = String::new();
    let package_bytes = physical_package_bytes(project_path)?;
    let backend = if content
        .asset_mounts()
        .iter()
        .all(|mount| matches!(mount.backend(), ContentBackend::Hakutaku(_)))
    {
        "Hakutaku"
    } else {
        "mixed/filesystem"
    };
    let logical_bytes = entries.iter().map(|entry| entry.len).sum::<u64>();
    line(
        &mut report,
        format!(
            "PACKAGE  | {backend} · {} effective asset(s) / {} logical · {} physical · open {:.2} ms · inventory {:.2} ms",
            entries.len(),
            mib(logical_bytes),
            package_bytes.map_or_else(|| "n/a".to_owned(), mib),
            milliseconds(open_elapsed),
            milliseconds(inventory_elapsed),
        ),
    );
    line(
        &mut report,
        format!("LOCATION | {}", project_path.display()),
    );

    let (fixture, real): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .partition(|entry| entry.path.starts_with(FIXTURE_DIRECTORY));
    if !real.is_empty() {
        let first = timed_read(&real)?;
        let repeat = timed_read(&real)?;
        line(
            &mut report,
            format!(
                "REAL     | {} project asset(s) / {} · first {:.1} MiB/s · repeat {:.1} MiB/s",
                real.len(),
                mib(first.bytes),
                first.mib_per_second(),
                repeat.mib_per_second(),
            ),
        );
    }

    if fixture.is_empty() {
        line(
            &mut report,
            "I/O      | benchmark payload absent · storage/cache stress skipped",
        );
        return Ok(report);
    }

    let group = |name| {
        fixture
            .iter()
            .filter(|entry| fixture_group(&entry.path) == Some(name))
            .cloned()
            .collect::<Vec<_>>()
    };
    let hot = group("hot");
    let normal = group("normal");
    let transient = group("transient");
    let streaming = group("streaming");
    if hot.is_empty() || normal.is_empty() || transient.is_empty() || streaming.len() < 6 {
        bail!("benchmark Hakutaku payload is incomplete");
    }
    line(
        &mut report,
        format!(
            "I/O      | isolated encrypted payload · {} · Hot/Normal/Transient/Streaming · codec decode excluded",
            mib(fixture.iter().map(|entry| entry.len).sum()),
        ),
    );

    let hot_first = timed_read(&hot)?;
    let hot_repeat = timed_read(&hot)?;
    line(
        &mut report,
        format!(
            "HOT      | {} files / {} · first {:.1} MiB/s · CLOCK repeat {:.1} MiB/s",
            hot.len(),
            mib(hot_first.bytes),
            hot_first.mib_per_second(),
            hot_repeat.mib_per_second(),
        ),
    );

    let normal_first = timed_read(&normal)?;
    let normal_admit = timed_read(&normal)?;
    let normal_resident = timed_read(&normal)?;
    line(
        &mut report,
        format!(
            "NORMAL   | {} files / {} · probation {:.1} · admission {:.1} · CLOCK resident {:.1} MiB/s",
            normal.len(),
            mib(normal_first.bytes),
            normal_first.mib_per_second(),
            normal_admit.mib_per_second(),
            normal_resident.mib_per_second(),
        ),
    );

    let transient_first = timed_read(&transient)?;
    let transient_repeat = timed_read(&transient)?;
    line(
        &mut report,
        format!(
            "TRANSIENT | {} files / {} · first {:.1} MiB/s · repeat {:.1} MiB/s",
            transient.len(),
            mib(transient_first.bytes),
            transient_first.mib_per_second(),
            transient_repeat.mib_per_second(),
        ),
    );

    let sequential = &streaming[..1];
    let sequential_first = timed_read(sequential)?;
    let sequential_repeat = timed_read(sequential)?;
    line(
        &mut report,
        format!(
            "STREAM   | {} sequential · first-touch {:.1} MiB/s · repeat {:.1} MiB/s",
            mib(sequential_first.bytes),
            sequential_first.mib_per_second(),
            sequential_repeat.mib_per_second(),
        ),
    );

    let random_first = timed_random_reads(&streaming[1], RANDOM_READS)?;
    let random_repeat = timed_random_reads(&streaming[1], RANDOM_READS)?;
    line(
        &mut report,
        format!(
            "RANDOM   | {RANDOM_READS} × 4 KiB seeks · first-touch {:.0} IOPS · repeat {:.0} IOPS",
            operations_per_second(RANDOM_READS, random_first.elapsed),
            operations_per_second(RANDOM_READS, random_repeat.elapsed),
        ),
    );

    let concurrent_first = timed_concurrent_read(&streaming[2..6])?;
    let concurrent_repeat = timed_concurrent_read(&streaming[2..6])?;
    line(
        &mut report,
        format!(
            "PARALLEL | 4 independent streams / {} · first-touch {:.1} MiB/s · repeat {:.1} MiB/s",
            mib(concurrent_first.bytes),
            concurrent_first.mib_per_second(),
            concurrent_repeat.mib_per_second(),
        ),
    );
    line(
        &mut report,
        "CACHE    | first-touch uses disjoint payload ranges; OS/drive caches are observed, not forcibly cleared",
    );
    Ok(report)
}

fn effective_assets(mounts: Vec<ContentMount>) -> Result<Vec<AssetEntry>> {
    let mut effective = BTreeMap::<PathBuf, ContentMount>::new();
    for mount in mounts {
        for path in mount_files(&mount)? {
            effective.insert(path, mount.clone());
        }
    }
    effective
        .into_iter()
        .map(|(path, mount)| {
            let len = mount
                .open_file(&path)
                .with_context(|| format!("failed to open benchmark asset {}", path.display()))?
                .len()?;
            Ok(AssetEntry { mount, path, len })
        })
        .collect()
}

fn mount_files(mount: &ContentMount) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![PathBuf::new()];
    let mut visited = HashSet::new();
    while let Some(directory) = pending.pop() {
        if !visited.insert(directory.clone()) {
            continue;
        }
        for entry in mount.read_directory(&directory)? {
            if mount.is_directory(&entry) {
                pending.push(entry);
            } else if mount.contains_file(&entry) {
                files.push(entry);
            }
        }
    }
    files.sort_unstable();
    Ok(files)
}

fn timed_read(entries: &[AssetEntry]) -> Result<ReadSample> {
    let started = Instant::now();
    let mut bytes = 0_u64;
    let mut buffer = vec![0_u8; IO_BUFFER_BYTES];
    for entry in entries {
        let mut file = entry
            .mount
            .open_file(&entry.path)
            .with_context(|| format!("failed to open {}", entry.path.display()))?;
        loop {
            let read = file
                .read(&mut buffer)
                .with_context(|| format!("failed to read {}", entry.path.display()))?;
            if read == 0 {
                break;
            }
            bytes = bytes.saturating_add(read as u64);
        }
    }
    Ok(ReadSample {
        bytes,
        elapsed: started.elapsed(),
    })
}

fn timed_random_reads(entry: &AssetEntry, operations: usize) -> Result<ReadSample> {
    if entry.len < RANDOM_READ_BYTES as u64 {
        bail!("random-read benchmark asset is too small");
    }
    let started = Instant::now();
    let mut file = entry.mount.open_file(&entry.path)?;
    let mut output = [0_u8; RANDOM_READ_BYTES];
    let slots = (entry.len - RANDOM_READ_BYTES as u64) / RANDOM_READ_BYTES as u64 + 1;
    let mut state = 0x243f_6a88_85a3_08d3_u64;
    for _ in 0..operations {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let offset = state % slots * RANDOM_READ_BYTES as u64;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut output)?;
    }
    Ok(ReadSample {
        bytes: (operations * RANDOM_READ_BYTES) as u64,
        elapsed: started.elapsed(),
    })
}

fn timed_concurrent_read(entries: &[AssetEntry]) -> Result<ReadSample> {
    let started = Instant::now();
    let bytes = thread::scope(|scope| -> Result<u64> {
        let handles = entries
            .iter()
            .cloned()
            .map(|entry| scope.spawn(move || timed_read(std::slice::from_ref(&entry))))
            .collect::<Vec<_>>();
        let mut bytes = 0_u64;
        for handle in handles {
            let sample = handle
                .join()
                .map_err(|_| anyhow::anyhow!("parallel benchmark reader panicked"))??;
            bytes = bytes.saturating_add(sample.bytes);
        }
        Ok(bytes)
    })?;
    Ok(ReadSample {
        bytes,
        elapsed: started.elapsed(),
    })
}

fn fixture_group(path: &Path) -> Option<&str> {
    path.strip_prefix(FIXTURE_DIRECTORY)
        .ok()?
        .components()
        .next()?
        .as_os_str()
        .to_str()
}

fn physical_package_bytes(project_path: &Path) -> Result<Option<u64>> {
    if !project_path.is_file() {
        return Ok(None);
    }
    let mut bytes = project_path.metadata()?.len();
    let data = project_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("data");
    if data.is_dir() {
        for entry in fs::read_dir(data)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                bytes = bytes.saturating_add(entry.metadata()?.len());
            }
        }
    }
    Ok(Some(bytes))
}

fn line(report: &mut String, value: impl AsRef<str>) {
    report.push_str(value.as_ref());
    report.push('\n');
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn operations_per_second(operations: usize, duration: Duration) -> f64 {
    operations as f64 / duration.as_secs_f64().max(f64::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("keine-benchmark-{name}-{}", std::process::id()))
    }

    #[cfg(feature = "publisher")]
    #[test]
    fn deterministic_payload_bytes_are_repeatable_and_seeded() {
        let mut first = [0_u8; 32];
        let mut repeated = [0_u8; 32];
        let mut different = [0_u8; 32];
        let mut seed = 7;
        fill_deterministic(&mut first, &mut seed);
        let mut seed = 7;
        fill_deterministic(&mut repeated, &mut seed);
        let mut seed = 8;
        fill_deterministic(&mut different, &mut seed);
        assert_eq!(first, repeated);
        assert_ne!(first, different);
    }

    #[test]
    fn effective_inventory_uses_highest_priority_mount() {
        let root = scratch("overlay");
        let _ = fs::remove_dir_all(&root);
        let low = root.join("low");
        let high = root.join("high");
        fs::create_dir_all(low.join("nested")).unwrap();
        fs::create_dir_all(high.join("nested")).unwrap();
        fs::write(low.join("nested/shared.bin"), b"low").unwrap();
        fs::write(high.join("nested/shared.bin"), b"higher").unwrap();
        fs::write(low.join("only-low.bin"), b"low").unwrap();
        let mounts = vec![
            ContentMount::new(ContentBackend::FileSystem(low), "").unwrap(),
            ContentMount::new(ContentBackend::FileSystem(high), "").unwrap(),
        ];

        let entries = effective_assets(mounts).unwrap();
        let shared = entries
            .iter()
            .find(|entry| entry.path == Path::new("nested/shared.bin"))
            .unwrap();
        assert_eq!(shared.len, 6);
        assert_eq!(entries.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fixture_group_requires_the_reserved_root() {
        assert_eq!(
            fixture_group(Path::new("__keine_benchmark__/normal/a.blob")),
            Some("normal")
        );
        assert_eq!(fixture_group(Path::new("normal/a.blob")), None);
    }
}
