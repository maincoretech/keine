#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "macos")]
struct Scratch(PathBuf);

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    use hakutaku_pack::{Identity, PackOptions, pack_directory};
    use keine_loader::{ContentBackend, ContentMount, HakutakuArchive};

    let Some(fixture) = std::env::args_os().nth(1) else {
        eprintln!("usage: keine-video-acceptance <video>");
        return std::process::ExitCode::from(2);
    };
    let fixture = Path::new(&fixture);
    let result = (|| -> anyhow::Result<()> {
        let temporary = Scratch::new()?;
        let source_dir = temporary.path().join("source");
        std::fs::create_dir(&source_dir)?;
        std::fs::copy(fixture, source_dir.join("playback.mp4"))?;
        let filesystem = ContentMount::new(ContentBackend::FileSystem(source_dir.clone()), "")?;
        keine::validate_native_video(&[filesystem], Path::new("playback.mp4"))
            .map_err(anyhow::Error::msg)?;
        let release = temporary.path().join("release");
        let identity = Identity::generate()?;
        pack_directory(&PackOptions::new(&source_dir, &release), &identity)?;
        let archive = HakutakuArchive::open_with_keys(
            &release.join("game.haku"),
            identity.root_key(),
            identity.public_key(),
        )?;
        let mount = ContentMount::new(ContentBackend::Hakutaku(archive), "")?;
        keine::validate_native_video(&[mount], Path::new("playback.mp4"))
            .map_err(anyhow::Error::msg)
    })();
    match result {
        Ok(()) => {
            println!("AVFoundation decoded FS/Hakutaku video frames successfully");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("native video acceptance failed: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    eprintln!("native video acceptance is available only on macOS");
    std::process::ExitCode::from(2)
}
