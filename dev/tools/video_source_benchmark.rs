use std::hint::black_box;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use hakutaku_pack::{Identity, PackOptions, pack_directory};
use keine_loader::{ContentBackend, ContentMount, HakutakuArchive};

const MEDIA_BYTES: usize = 32 * 1024 * 1024;
const DIRECT_ITERATIONS: u32 = 1_000;
const LEGACY_ITERATIONS: u32 = 3;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("video source benchmark failed: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let source_dir = temporary.path().join("source");
    std::fs::create_dir(&source_dir)?;
    let mut media = std::fs::File::create(source_dir.join("large.mp4"))?;
    let mut state = 0x4d59_5df4_d0f3_3173_u64;
    let mut block = [0_u8; 64 * 1024];
    for _ in 0..MEDIA_BYTES / block.len() {
        for chunk in block.chunks_exact_mut(8) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            chunk.copy_from_slice(&state.to_le_bytes());
        }
        media.write_all(&block)?;
    }
    drop(media);

    let release = temporary.path().join("release");
    let identity = Identity::generate()?;
    pack_directory(&PackOptions::new(&source_dir, &release), &identity)?;
    let archive = HakutakuArchive::open_with_keys(
        &release.join("game.haku"),
        identity.root_key(),
        identity.public_key(),
    )?;
    let mount = ContentMount::new(ContentBackend::Hakutaku(archive), "")?;
    let logical_path = Path::new("large.mp4");

    let direct_start = Instant::now();
    for _ in 0..DIRECT_ITERATIONS {
        if !mount.contains_file(logical_path) {
            anyhow::bail!("benchmark source disappeared from its mount");
        }
        let source = mount.open_file(logical_path)?;
        let length = source.len()?;
        if length != MEDIA_BYTES as u64 {
            anyhow::bail!("unexpected direct source length: {length}");
        }
        black_box(source);
    }
    let direct = direct_start.elapsed() / DIRECT_ITERATIONS;

    let legacy_start = Instant::now();
    for iteration in 0..LEGACY_ITERATIONS {
        let mut source = mount.open_file(logical_path)?;
        let mut output =
            std::fs::File::create(temporary.path().join(format!("legacy-{iteration}.bin")))?;
        let copied = std::io::copy(&mut source, &mut output)?;
        if copied != MEDIA_BYTES as u64 {
            anyhow::bail!("unexpected legacy copy length: {copied}");
        }
    }
    let legacy = legacy_start.elapsed() / LEGACY_ITERATIONS;
    println!(
        "32 MiB Hakutaku video source: legacy_copy={legacy:?}/open, direct_random_access={direct:?}/open, plaintext_write=32 MiB -> 0"
    );
    Ok(())
}
