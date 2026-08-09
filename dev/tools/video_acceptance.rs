#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    use std::path::Path;

    use hexz_k::cmd::pack::{PackOptions, pack_directory};
    use keine_loader::{ContentBackend, ContentMount, hexz_password, mount_hexz};

    let Some(fixture) = std::env::args_os().nth(1) else {
        eprintln!("usage: keine-video-acceptance <video>");
        return std::process::ExitCode::from(2);
    };
    let fixture = Path::new(&fixture);
    let result = (|| -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        let source_dir = temporary.path().join("source");
        std::fs::create_dir(&source_dir)?;
        std::fs::copy(fixture, source_dir.join("playback.mp4"))?;
        let filesystem = ContentMount::new(ContentBackend::FileSystem(source_dir.clone()), "")?;
        keine::validate_native_video(&[filesystem], Path::new("playback.mp4"))
            .map_err(anyhow::Error::msg)?;
        let archive_path = temporary.path().join("media.hxz");
        pack_directory(&PackOptions {
            input: source_dir.display().to_string(),
            output: archive_path.display().to_string(),
            compression: "zstd".to_owned(),
            encrypt: true,
            block_size: 65_536,
            password: Some(hexz_password().to_owned()),
        })?;
        let mount = ContentMount::new(ContentBackend::Hexz(mount_hexz(&archive_path)?), "")?;
        keine::validate_native_video(&[mount], Path::new("playback.mp4"))
            .map_err(anyhow::Error::msg)
    })();
    match result {
        Ok(()) => {
            println!("AVFoundation decoded encrypted Hexz video successfully");
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
