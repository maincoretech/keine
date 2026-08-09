//! `keine package` — the single command-based packaging pipeline shared by
//! local builds and CI. It stages a project (native or LetsGal), compiles
//! `.keine/compiled/program.bin`, builds a content-trimmed release engine,
//! packs an encrypted `.hxz`, and assembles a runnable output directory.

use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use hexz_k::cmd::pack::{PackOptions, pack_directory};
use tempfile::{Builder, TempDir, tempdir};

use crate::compiler::compile_project;
use crate::runtime::bootstrap::open_project;

pub const DEFAULT_OUTPUT: &str = "target/release-package";

/// Both video backends are compiled by default so packaged games behave like
/// `cargo dev` on every platform. `video-native` only activates on macOS (its
/// dependencies are target-gated); elsewhere it is a no-op and FFmpeg carries
/// video.
const VIDEO_FEATURES: &str = "video-native,video-ffmpeg";

fn project_manifest_error(project: &Path) -> String {
    format!(
        "release packaging requires a native project (config.yaml) or a LetsGal \
         project (project.json) at its root ({} has neither)",
        project.display()
    )
}

pub fn package_project(
    project: &Path,
    loader: &keine_loader::LoaderRegistry,
    output: &Path,
) -> Result<()> {
    let password = env::var("HEXZ_PASSWORD").context("HEXZ_PASSWORD must be set")?;
    if !project.join("config.yaml").is_file() && !project.join("project.json").is_file() {
        bail!("{}", project_manifest_error(project));
    }
    if !project.is_dir() {
        bail!("project directory does not exist: {}", project.display());
    }
    let output = release_output_path(output)?;

    let staging = tempdir().context("failed to create staging directory")?;
    let staged = staging.path().join("project");
    copy_tree(project, &staged)?;

    let (_root, config, content) = open_project(&staged, loader)?;
    let config_path = staged.join("config.yaml");
    if !config_path.is_file() {
        // LetsGal source: materialize the adapter-derived config (asset
        // aliases, layout, styles) so the packaged archive can be opened
        // through config.yaml with the same resolution as the editor.
        let yaml = noyalib::to_string(&config)
            .context("failed to serialize the project config to YAML")?;
        fs::write(&config_path, yaml)?;
    }

    // The compiler must parse source scenes, so the policy starts neutralized
    // to `auto`; the packaged config is pinned to `require` afterwards, which
    // makes startup fail loudly if the compiled artifact is missing.
    set_compiled_policy(&config_path, "auto")?;
    let languages = loader
        .languages(&config.adapter.script)
        .context("failed to select script adapter")?;
    compile_project(&config, &content, &languages, None)?;
    set_compiled_policy(&config_path, "require")?;

    let features = detect_features(&staged)?;
    println!("content features: {features}");
    let engine = build_engine(&features)?;

    let output_parent = output.parent().context("release output has no parent")?;
    fs::create_dir_all(output_parent)?;
    let assembled = Builder::new()
        .prefix(".keine-package-")
        .tempdir_in(output_parent)
        .context("failed to create release assembly directory")?;
    pack_staging(&staged, assembled.path(), &password)?;
    assemble(assembled.path(), &features, &engine)?;
    publish_directory(assembled, &output)?;
    println!("{}", output.display());
    Ok(())
}

fn release_output_path(output: &Path) -> Result<PathBuf> {
    let mut relative = PathBuf::new();
    for component in output.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => relative.push(part),
            _ => bail!(
                "release output must be a relative child of target/: {}",
                output.display()
            ),
        }
    }
    let mut components = relative.components();
    let below_target = components.next() == Some(Component::Normal("target".as_ref()));
    let first_directory = components.next();
    if !below_target || first_directory.is_none() {
        bail!(
            "release output must be a named directory below target/, not {}",
            output.display()
        );
    }
    if matches!(
        first_directory,
        Some(Component::Normal(name))
            if matches!(name.to_str(), Some("debug" | "release" | "package-runner" | "runner"))
    ) {
        bail!(
            "release output overlaps a Cargo build directory: {}",
            output.display()
        );
    }
    Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative))
}

fn publish_directory(assembled: TempDir, output: &Path) -> Result<()> {
    let parent = output.parent().context("release output has no parent")?;
    let name = output
        .file_name()
        .context("release output has no directory name")?
        .to_string_lossy();
    let backup = parent.join(format!(".{name}.backup-{}", std::process::id()));
    if backup.exists() {
        bail!(
            "stale release backup blocks publication: {}",
            backup.display()
        );
    }

    let had_previous = output.exists();
    if had_previous {
        if !output.is_dir() {
            bail!("release output is not a directory: {}", output.display());
        }
        fs::rename(output, &backup)
            .with_context(|| format!("failed to preserve {}", output.display()))?;
    }
    let assembled = assembled.keep();
    if let Err(error) = fs::rename(&assembled, output) {
        let _ = fs::remove_dir_all(&assembled);
        if had_previous && let Err(restore_error) = fs::rename(&backup, output) {
            return Err(anyhow::anyhow!(error)).context(format!(
                "failed to publish {}; restoring the previous release also failed: {restore_error}; it remains at {}",
                output.display(),
                backup.display()
            ));
        }
        return Err(error).with_context(|| format!("failed to publish {}", output.display()));
    }
    if had_previous {
        fs::remove_dir_all(&backup)
            .with_context(|| format!("failed to remove old release {}", backup.display()))?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if file_type.is_symlink() {
            bail!(
                "release projects cannot contain symbolic links: {}",
                entry.path().display()
            );
        }
        if (file_type.is_dir() && is_ignored_directory(&name))
            || (!file_type.is_dir() && is_ignored_file(&name))
        {
            continue;
        }
        let target = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
        } else {
            bail!(
                "release projects cannot contain special files: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn is_ignored_directory(name: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "saves" | "imported_assets" | ".keine"
    )
}

fn is_ignored_file(name: &str) -> bool {
    name == ".DS_Store" || name.ends_with(".meta")
}

/// Replace or append the top-level `compiled_program` policy in a config.yaml.
fn set_compiled_policy(config: &Path, policy: &str) -> Result<()> {
    let text = fs::read_to_string(config)?;
    let mut replaced = false;
    let mut lines = Vec::with_capacity(text.lines().count() + 1);
    for line in text.lines() {
        if line.starts_with("compiled_program:") {
            lines.push(format!("compiled_program: {policy}"));
            replaced = true;
        } else {
            lines.push(line.to_owned());
        }
    }
    if !replaced {
        lines.push(format!("compiled_program: {policy}"));
    }
    fs::write(config, lines.join("\n").into_bytes())?;
    Ok(())
}

/// Smallest comma-separated feature set required by a project, mirroring
/// `dev/scripts/audio-features.sh`. `KEINE_AUDIO_FEATURES` overrides detection.
fn detect_features(project: &Path) -> Result<String> {
    if let Some(features) = env::var("KEINE_AUDIO_FEATURES")
        .ok()
        .filter(|features| !features.is_empty())
    {
        return Ok(features);
    }
    let mut wav = false;
    let mut mp3 = false;
    let mut vorbis = false;
    let mut flac = false;
    let mut video = false;
    for file in walk_files(project)? {
        let lower = file.to_string_lossy().to_ascii_lowercase();
        if lower.ends_with(".hxz") {
            return Ok(format!("audio-all,ui-sounds,{VIDEO_FEATURES}"));
        }
        let extension = lower.rsplit('.').next().unwrap_or("");
        match extension {
            "opus" => {}
            "wav" | "wave" => wav = true,
            "mp3" => mp3 = true,
            "ogg" | "oga" | "spx" => vorbis = true,
            "flac" => flac = true,
            "mp4" | "m4v" | "mov" | "webm" | "mkv" => video = true,
            _ => {}
        }
    }
    let mut features = vec!["ui-sounds".to_owned()];
    if wav {
        features.push("audio-wav".to_owned());
    }
    if mp3 {
        features.push("audio-mp3".to_owned());
    }
    if vorbis {
        features.push("audio-vorbis".to_owned());
    }
    if flac {
        features.push("audio-flac".to_owned());
    }
    if video {
        features.push(VIDEO_FEATURES.to_owned());
    }
    Ok(features.join(","))
}

fn walk_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    Ok(files)
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// Content-trimmed release engine build, reusing the repo's default target
/// directory so the same binary the user develops with is rebuilt.
fn build_engine(features: &str) -> Result<PathBuf> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    let mut all_features = String::from("hardened");
    if !features.is_empty() {
        all_features.push(',');
        all_features.push_str(features);
    }
    command
        .current_dir(repo_root)
        .args(["build", "--release", "--locked", "--no-default-features"])
        .args(["--features", &all_features])
        .arg("--target-dir")
        .arg(repo_root.join("target"));
    let status = command.status().context("failed to run cargo build")?;
    if !status.success() {
        bail!("engine build failed with status {status}");
    }
    Ok(repo_root
        .join("target/release")
        .join(format!("keine{}", env::consts::EXE_SUFFIX)))
}

fn pack_staging(staged: &Path, output: &Path, password: &str) -> Result<()> {
    pack_directory(&PackOptions {
        input: staged.display().to_string(),
        output: output.join("game.hxz").display().to_string(),
        compression: "zstd".to_owned(),
        encrypt: true,
        block_size: 65_536,
        password: Some(password.to_owned()),
    })
    .context("hexz pack failed")
}

fn assemble(output: &Path, _features: &str, engine: &Path) -> Result<()> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    #[cfg(windows)]
    {
        fs::copy(engine, output.join("keine.exe"))?;
        if has_feature(_features, "video-ffmpeg") {
            bundle_ffmpeg_runtime(output)?;
        }
        fs::write(output.join("run.bat"), RUN_BAT)?;
    }
    #[cfg(not(windows))]
    {
        fs::copy(engine, output.join("keine"))?;
        fs::write(output.join("run.sh"), RUN_SH)?;
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["keine", "run.sh"] {
                fs::set_permissions(output.join(name), fs::Permissions::from_mode(0o755))?;
            }
        }
    }
    fs::copy(
        repo_root.join("assets/icons/keine-256.png"),
        output.join("keine.png"),
    )?;
    Ok(())
}

#[cfg(windows)]
fn bundle_ffmpeg_runtime(output: &Path) -> Result<()> {
    let vcpkg_root = env::var("VCPKG_ROOT")
        .context("VCPKG_ROOT is required to bundle the Windows FFmpeg runtime")?;
    let ffmpeg_bin = Path::new(&vcpkg_root).join("installed/x64-windows/bin");
    let mut copied = 0;
    for entry in fs::read_dir(&ffmpeg_bin).with_context(|| {
        format!(
            "Windows FFmpeg runtime DLLs were not found in {}",
            ffmpeg_bin.display()
        )
    })? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().ends_with(".dll") {
            fs::copy(entry.path(), output.join(entry.file_name()))?;
            copied += 1;
        }
    }
    if copied == 0 {
        bail!("no FFmpeg runtime DLLs found in {}", ffmpeg_bin.display());
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn has_feature(features: &str, wanted: &str) -> bool {
    features.split(',').any(|feature| feature == wanted)
}

#[cfg(windows)]
const RUN_BAT: &str = "@echo off\n\"%~dp0keine.exe\" \"%~dp0game.hxz\"\n";
#[cfg(not(windows))]
const RUN_SH: &str = "#!/usr/bin/env bash\nset -euo pipefail\nroot=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\nexec \"$root/keine\" \"$root/game.hxz\"\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_content_enables_both_backends_by_default() {
        let root = tempdir().unwrap();
        let assets = root.path().join("assets");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("intro.mp4"), b"video").unwrap();
        let features = detect_features(root.path()).unwrap();
        assert!(has_feature(&features, "video-native"));
        assert!(has_feature(&features, "video-ffmpeg"));
        assert!(has_feature(&features, "ui-sounds"));
    }

    #[test]
    fn audio_only_content_stays_without_video_features() {
        let root = tempdir().unwrap();
        let assets = root.path().join("assets");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("bgm.opus"), b"audio").unwrap();
        let features = detect_features(root.path()).unwrap();
        assert_eq!(features, "ui-sounds");
    }

    #[test]
    fn release_output_cannot_select_target_itself_or_escape_it() {
        assert!(release_output_path(Path::new("target/release-package")).is_ok());
        assert!(release_output_path(Path::new("target")).is_err());
        assert!(release_output_path(Path::new("target/../outside")).is_err());
        assert!(release_output_path(Path::new("/tmp/release")).is_err());
        assert!(release_output_path(Path::new("target/release")).is_err());
        assert!(release_output_path(Path::new("target/debug/package")).is_err());
    }

    #[test]
    fn staging_copy_omits_generated_and_private_directories() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir_all(source.join("assets")).unwrap();
        for ignored in [".git", "target", "saves", "imported_assets", ".keine"] {
            fs::create_dir_all(source.join(ignored)).unwrap();
            fs::write(source.join(ignored).join("private"), b"private").unwrap();
        }
        fs::write(source.join("assets/kept.txt"), b"kept").unwrap();
        fs::write(source.join("assets/ignored.meta"), b"ignored").unwrap();

        copy_tree(&source, &destination).unwrap();

        assert!(destination.join("assets/kept.txt").is_file());
        assert!(!destination.join("assets/ignored.meta").exists());
        assert!(!destination.join(".git").exists());
        assert!(!destination.join("target").exists());
        assert!(!destination.join("saves").exists());
        assert!(!destination.join("imported_assets").exists());
        assert!(!destination.join(".keine").exists());
    }
}
