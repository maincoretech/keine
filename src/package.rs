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
use tempfile::tempdir;

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
    if output.components().next() != Some(Component::Normal("target".as_ref())) {
        bail!(
            "release output must stay under target/: {}",
            output.display()
        );
    }
    if !project.is_dir() {
        bail!("project directory does not exist: {}", project.display());
    }

    let staging = tempdir().context("failed to create staging directory")?;
    let staged = staging.path().join("project");
    copy_tree(project, &staged)?;
    cleanup_staging(&staged)?;

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
    build_engine(&features)?;

    if output.exists() {
        fs::remove_dir_all(output)
            .with_context(|| format!("failed to clear {}", output.display()))?;
    }
    fs::create_dir_all(output)?;
    pack_staging(&staged, output, &password)?;
    assemble(output, &features)?;
    println!("{}", output.display());
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Runtime state and generated caches must never enter the encrypted
/// artifact. The staging copy regenerates `.keine/compiled/program.bin`, so
/// any source-project `.keine` contents are dropped as well.
fn cleanup_staging(root: &Path) -> Result<()> {
    let mut remove_dirs = Vec::new();
    let mut remove_files = Vec::new();
    collect_cleanup(root, &mut remove_dirs, &mut remove_files)?;
    for directory in remove_dirs {
        fs::remove_dir_all(directory)?;
    }
    for file in remove_files {
        fs::remove_file(file)?;
    }
    Ok(())
}

fn collect_cleanup(
    root: &Path,
    remove_dirs: &mut Vec<PathBuf>,
    remove_files: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() {
            if matches!(name.as_str(), "saves" | "imported_assets" | ".keine") {
                remove_dirs.push(path);
            } else {
                collect_cleanup(&path, remove_dirs, remove_files)?;
            }
        } else if name == ".DS_Store" || name.ends_with(".meta") {
            remove_files.push(path);
        }
    }
    Ok(())
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
fn build_engine(features: &str) -> Result<()> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut command = Command::new("cargo");
    let mut all_features = String::from("hardened");
    if !features.is_empty() {
        all_features.push(',');
        all_features.push_str(features);
    }
    command
        .current_dir(repo_root)
        .args(["build", "--release", "--locked", "--no-default-features"])
        .args(["--features", &all_features]);
    let status = command.status().context("failed to run cargo build")?;
    if !status.success() {
        bail!("engine build failed with status {status}");
    }
    Ok(())
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

fn assemble(output: &Path, features: &str) -> Result<()> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let windows_engine = repo_root.join("target/release/keine.exe");
    if windows_engine.is_file() {
        fs::copy(&windows_engine, output.join("keine.exe"))?;
        if has_feature(features, "video-ffmpeg") {
            bundle_ffmpeg_runtime(output)?;
        }
        fs::write(output.join("run.bat"), RUN_BAT)?;
    } else {
        let engine = repo_root.join("target/release/keine");
        fs::copy(&engine, output.join("keine"))?;
        fs::write(output.join("run.sh"), RUN_SH)?;
        #[cfg(unix)]
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

fn has_feature(features: &str, wanted: &str) -> bool {
    features.split(',').any(|feature| feature == wanted)
}

const RUN_BAT: &str = "@echo off\n\"%~dp0keine.exe\" \"%~dp0game.hxz\"\n";
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
}
