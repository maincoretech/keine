use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use keine_loader::ContentMount;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "keine-video-acceptance-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(all(feature = "video-native", target_os = "macos"))]
fn validate_backend(mounts: &[ContentMount], path: &Path) -> Result<(), String> {
    keine::validate_native_video(mounts, path)
}

#[cfg(all(feature = "video-native", target_os = "macos"))]
fn backend_name() -> &'static str {
    "AVFoundation"
}

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
fn validate_backend(mounts: &[ContentMount], path: &Path) -> Result<(), String> {
    keine::validate_ffmpeg_video(mounts, path)
}

#[cfg(all(
    feature = "video-ffmpeg",
    not(all(feature = "video-native", target_os = "macos"))
))]
fn backend_name() -> &'static str {
    "FFmpeg"
}

#[cfg(not(any(
    all(feature = "video-native", target_os = "macos"),
    all(
        feature = "video-ffmpeg",
        not(all(feature = "video-native", target_os = "macos"))
    )
)))]
fn validate_backend(_mounts: &[ContentMount], _path: &Path) -> Result<(), String> {
    Err("enable video-native on macOS or video-ffmpeg on Windows/Linux".to_owned())
}

#[cfg(not(any(
    all(feature = "video-native", target_os = "macos"),
    all(
        feature = "video-ffmpeg",
        not(all(feature = "video-native", target_os = "macos"))
    )
)))]
fn backend_name() -> &'static str {
    "unavailable video backend"
}

fn main() -> std::process::ExitCode {
    use hakutaku_pack::{Identity, PackOptions, pack_directory};
    use keine_loader::{ContentBackend, HakutakuArchive, OpenPolicy};

    let Some(fixture) = std::env::args_os().nth(1) else {
        eprintln!("usage: keine-video-acceptance <video>");
        return std::process::ExitCode::from(2);
    };
    let fixture = Path::new(&fixture);
    let extension = fixture
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("mp4");
    let logical_path = PathBuf::from(format!("playback.{extension}"));
    let result = (|| -> anyhow::Result<()> {
        let temporary = Scratch::new()?;
        let source_dir = temporary.path().join("source");
        std::fs::create_dir(&source_dir)?;
        std::fs::copy(fixture, source_dir.join(&logical_path))?;

        let filesystem = ContentMount::new(ContentBackend::FileSystem(source_dir.clone()), "")?;
        validate_backend(&[filesystem], &logical_path)
            .map_err(|error| anyhow::anyhow!("filesystem source: {error}"))?;

        let release = temporary.path().join("release");
        let identity = Identity::generate()?;
        pack_directory(&PackOptions::new(&source_dir, &release), &identity)?;
        let archive = HakutakuArchive::open_with_keys(
            &release.join("game.haku"),
            identity.root_key(),
            identity.public_key(),
            OpenPolicy::TrustFirstRelease,
        )?;
        let mount = ContentMount::new(ContentBackend::Hakutaku(archive), "")?;
        validate_backend(&[mount], &logical_path)
            .map_err(|error| anyhow::anyhow!("Hakutaku source: {error}"))
    })();
    match result {
        Ok(()) => {
            println!(
                "{} decoded filesystem and Hakutaku video successfully",
                backend_name()
            );
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("video acceptance failed: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
