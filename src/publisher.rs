//! Publisher asset-pack and distributable-bundle pipelines.
//!
//! Asset packing writes only compiled Hakutaku content. Bundling remains a
//! separate operation that builds a content-trimmed engine and assembles it
//! with those resources.

use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use hakutaku_core::SEGMENT_FILE_EXTENSION;
use hakutaku_pack::{Identity, PackOptions, pack_directory};
use tempfile::{Builder, TempDir, tempdir};

use crate::compiler::build_program;
use crate::runtime::bootstrap::{OpenedProject, open_project};

#[cfg(target_os = "macos")]
const VIDEO_FEATURE: &str = "video-native";
#[cfg(not(target_os = "macos"))]
const VIDEO_FEATURE: &str = "video-ffmpeg";

fn project_manifest_error(project: &Path) -> String {
    format!(
        "release packaging requires a native project (config.yaml) or a LetsGal \
         project (project.json) at its root ({} has neither)",
        project.display()
    )
}

struct PreparedProject {
    _staging: TempDir,
    staged: PathBuf,
    identity: Identity,
}

pub fn pack_project(
    project: &Path,
    loader: &keine_loader::LoaderRegistry,
    output: &Path,
) -> Result<()> {
    let output = publisher_output_path(output)?;
    let prepared = prepare_project(project, loader)?;
    publish_prepared(&prepared.staged, &prepared.identity, &output, |_| Ok(()))?;
    println!("{}", output.display());
    Ok(())
}

pub fn bundle_project(
    project: &Path,
    loader: &keine_loader::LoaderRegistry,
    output: &Path,
    benchmark: bool,
) -> Result<()> {
    let output = publisher_output_path(output)?;
    let prepared = prepare_project(project, loader)?;
    let mut features = detect_features(&prepared.staged)?;
    if benchmark {
        crate::runtime::package_benchmark::stage_payload(&prepared.staged)?;
        if !features.is_empty() {
            features.push(',');
        }
        features.push_str("startup-metrics");
    }
    println!("content features: {features}");
    let runtime_keys = prepared.identity.runtime_key_material()?;
    let key_share_a = prepared._staging.path().join("hakutaku-key-share-a.bin");
    let key_share_b = prepared._staging.path().join("hakutaku-key-share-b.bin");
    let public_key = prepared._staging.path().join("hakutaku-public-key.bin");
    fs::write(&key_share_a, runtime_keys.key_share_a)?;
    fs::write(&key_share_b, runtime_keys.key_share_b)?;
    fs::write(&public_key, runtime_keys.public_key)?;
    let engine = build_engine(&features, &key_share_a, &key_share_b, &public_key)?;
    publish_prepared(&prepared.staged, &prepared.identity, &output, |assembled| {
        assemble(assembled, &features, &engine, benchmark)
    })?;
    println!("{}", output.display());
    Ok(())
}

fn prepare_project(
    project: &Path,
    loader: &keine_loader::LoaderRegistry,
) -> Result<PreparedProject> {
    if !project.join("config.yaml").is_file() && !project.join("project.json").is_file() {
        bail!("{}", project_manifest_error(project));
    }
    if !project.is_dir() {
        bail!("project directory does not exist: {}", project.display());
    }
    let staging = tempdir().context("failed to create staging directory")?;
    let staged = staging.path().join("project");
    copy_tree(project, &staged)?;

    let OpenedProject {
        config, content, ..
    } = open_project(&staged, loader)?;
    validate_shipping_identity(&config.project)?;
    validate_shipping_media(&content)?;
    let config_path = staged.join("config.yaml");
    if !config_path.is_file() {
        // LetsGal source: materialize the adapter-derived config (asset
        // aliases, layout, styles) so the packaged archive can be opened
        // through config.yaml with the same resolution as the editor.
        let yaml = serialize_config_deterministically(&config)?;
        fs::write(&config_path, yaml)?;
    }

    let languages = loader
        .languages(&config.adapter.script)
        .context("failed to select script adapter")?;
    build_program(&config, &content, &languages)?;
    // Do not create or load publisher secrets until every project-owned
    // validation and compilation step has succeeded.
    let identity = load_or_create_identity(project)?;
    Ok(PreparedProject {
        _staging: staging,
        staged,
        identity,
    })
}

fn serialize_config_deterministically(config: &keine_core::config::GameConfig) -> Result<String> {
    let mut value = noyalib::to_value(config)
        .context("failed to serialize the project config to YAML values")?;
    sort_yaml_mappings(&mut value);
    noyalib::to_string_value(&value).context("failed to serialize the project config to YAML")
}

fn sort_yaml_mappings(value: &mut noyalib::Value) {
    match value {
        noyalib::Value::Sequence(sequence) => {
            for value in sequence {
                sort_yaml_mappings(value);
            }
        }
        noyalib::Value::Mapping(mapping) => {
            for (_, value) in mapping.iter_mut() {
                sort_yaml_mappings(value);
            }
            mapping.sort_keys();
        }
        noyalib::Value::Tagged(tagged) => sort_yaml_mappings(tagged.value_mut()),
        _ => {}
    }
}

fn validate_shipping_identity(project: &keine_core::config::ProjectMetadata) -> Result<()> {
    if project.valid_id().is_none() {
        bail!(
            "release packaging requires project.id to be a lowercase ASCII slug (letters, digits and hyphens; maximum 64 bytes)"
        );
    }
    if project.application_identifier().is_none() {
        bail!(
            "project.bundle_identifier must be a valid reverse-DNS identifier, or be omitted to derive one from project.id"
        );
    }
    Ok(())
}

fn validate_shipping_media(content: &keine_loader::ContentProject) -> Result<()> {
    let mut violations = Vec::new();
    for root in content
        .asset_mounts()
        .into_iter()
        .filter_map(|mount| mount.filesystem_root())
    {
        for file in walk_files(&root)? {
            let Some(kind) = noncanonical_shipping_media(&file) else {
                continue;
            };
            let path = file.strip_prefix(&content.root).unwrap_or(&file);
            violations.push((path.to_owned(), kind));
        }
    }
    violations.sort_unstable();
    violations.dedup();
    if violations.is_empty() {
        return Ok(());
    }

    for (path, kind) in violations.iter().take(16) {
        eprintln!(
            "error: non-canonical {kind} shipping resource: {}",
            path.display()
        );
    }
    if violations.len() > 16 {
        eprintln!(
            "error: and {} more non-canonical resource(s)",
            violations.len() - 16
        );
    }
    bail!(
        "release resources must use WebP images and Ogg Opus (.opus) standalone audio; convert the files first, then migrate their references with `cargo assets --remap <project> png=webp jpg=webp jpeg=webp wav=opus mp3=opus ogg=opus flac=opus -y` before packing"
    )
}

fn noncanonical_shipping_media(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" => Some("image"),
        "wav" | "wave" | "mp3" | "ogg" | "oga" | "spx" | "flac" => Some("standalone audio"),
        _ => None,
    }
}

fn publish_prepared(
    staged: &Path,
    identity: &Identity,
    output: &Path,
    finish: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let output_parent = output.parent().context("publisher output has no parent")?;
    fs::create_dir_all(output_parent)?;
    let assembled = Builder::new()
        .prefix(".keine-publisher-")
        .tempdir_in(output_parent)
        .context("failed to create publisher assembly directory")?;
    seed_previous_release(output, assembled.path())?;
    pack_staging(staged, assembled.path(), identity)?;
    finish(assembled.path())?;
    publish_directory(assembled, output)?;
    Ok(())
}

fn load_or_create_identity(project: &Path) -> Result<Identity> {
    let path = env::var_os("KEINE_HAKUTAKU_IDENTITY")
        .map(PathBuf::from)
        .unwrap_or_else(|| project.join(".keine/publisher.hakutaku-key"));
    load_or_create_identity_at(&path)
}

fn load_or_create_identity_at(path: &Path) -> Result<Identity> {
    if path.is_file() {
        return Identity::load(path)
            .with_context(|| format!("failed to load publisher identity {}", path.display()));
    }
    let parent = path.parent().context("publisher identity has no parent")?;
    fs::create_dir_all(parent)?;
    let identity = Identity::generate()?;
    identity
        .save(path)
        .with_context(|| format!("failed to save publisher identity {}", path.display()))?;
    println!("created publisher identity: {}", path.display());
    Ok(identity)
}

fn publisher_output_path(output: &Path) -> Result<PathBuf> {
    let mut relative = PathBuf::new();
    for component in output.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => relative.push(part),
            _ => bail!(
                "publisher output must be a relative child of target/: {}",
                output.display()
            ),
        }
    }
    let mut components = relative.components();
    let below_target = components.next() == Some(Component::Normal("target".as_ref()));
    let first_directory = components.next();
    if !below_target || first_directory.is_none() {
        bail!(
            "publisher output must be a named directory below target/, not {}",
            output.display()
        );
    }
    if matches!(
        first_directory,
        Some(Component::Normal(name))
            if matches!(
                name.to_str(),
                Some("debug" | "release" | "package-runner" | "publisher-runner" | "runner")
            )
    ) {
        bail!(
            "publisher output overlaps a Cargo build directory: {}",
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
        cleanup_old_release_after_commit(&backup);
    }
    Ok(())
}

fn cleanup_old_release_after_commit(backup: &Path) {
    if let Err(error) = fs::remove_dir_all(backup) {
        eprintln!(
            "warning: release committed, but the previous release could not be removed from {}: {error}",
            backup.display()
        );
    }
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

/// Canonical shipping feature set: embedded/project Opus plus the selected
/// platform video backend when the project contains a video container.
fn detect_features(project: &Path) -> Result<String> {
    let mut video = false;
    for file in walk_files(project)? {
        let lower = file.to_string_lossy().to_ascii_lowercase();
        let extension = lower.rsplit('.').next().unwrap_or("");
        match extension {
            "mp4" | "m4v" | "mov" | "webm" | "mkv" => video = true,
            _ => {}
        }
    }
    let mut features = vec!["ui-sounds".to_owned()];
    if video {
        features.push(VIDEO_FEATURE.to_owned());
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
fn build_engine(
    features: &str,
    key_share_a: &Path,
    key_share_b: &Path,
    public_key: &Path,
) -> Result<PathBuf> {
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
    configure_engine_environment(&mut command, key_share_a, key_share_b, public_key);
    let build_target = env::var("KEINE_BUILD_TARGET")
        .ok()
        .filter(|target| !target.is_empty());
    if let Some(target) = build_target.as_deref() {
        command.args(["--target", target]);
    }
    #[cfg(target_os = "linux")]
    if has_feature(features, "video-ffmpeg") {
        configure_linux_bundle_rpath(&mut command);
    }
    let status = command.status().context("failed to run cargo build")?;
    if !status.success() {
        bail!("engine build failed with status {status}");
    }
    let release = build_target.map_or_else(
        || repo_root.join("target/release"),
        |target| repo_root.join("target").join(target).join("release"),
    );
    Ok(release.join(format!("keine{}", env::consts::EXE_SUFFIX)))
}

#[cfg(any(target_os = "linux", test))]
fn configure_linux_bundle_rpath(command: &mut Command) {
    const LINKER_FLAG: &str = "link-arg=-Wl,--disable-new-dtags,-rpath,$ORIGIN/lib";
    if let Some(mut flags) = env::var_os("CARGO_ENCODED_RUSTFLAGS") {
        if !flags.is_empty() {
            flags.push("\u{1f}");
        }
        flags.push("-C\u{1f}");
        flags.push(LINKER_FLAG);
        command.env("CARGO_ENCODED_RUSTFLAGS", flags);
        return;
    }
    let mut flags = env::var_os("RUSTFLAGS").unwrap_or_default();
    if !flags.is_empty() {
        flags.push(" ");
    }
    flags.push("-C ");
    flags.push(LINKER_FLAG);
    command.env("RUSTFLAGS", flags);
}

fn configure_engine_environment(
    command: &mut Command,
    key_share_a: &Path,
    key_share_b: &Path,
    public_key: &Path,
) {
    // The publisher identity signs the archive in this process. The nested
    // Cargo build only needs the derived runtime shares embedded by loader's
    // build script, so do not expose the signing key to dependencies/build.rs.
    command
        .env_remove("HAKUTAKU_IDENTITY_BASE64")
        .env_remove("KEINE_HAKUTAKU_IDENTITY")
        .env("KEINE_HAKUTAKU_KEY_SHARE_A", key_share_a)
        .env("KEINE_HAKUTAKU_KEY_SHARE_B", key_share_b)
        .env("KEINE_HAKUTAKU_PUBLIC_KEY", public_key);
}

fn pack_staging(staged: &Path, output: &Path, identity: &Identity) -> Result<()> {
    pack_directory(&PackOptions::new(staged, output), identity).context("Hakutaku pack failed")?;
    Ok(())
}

fn seed_previous_release(previous: &Path, assembled: &Path) -> Result<()> {
    let snapshot = previous.join("game.haku");
    if !snapshot.is_file() {
        return Ok(());
    }
    fs::create_dir_all(assembled.join("data"))?;
    link_or_copy(&snapshot, &assembled.join("game.haku"))?;
    let data = previous.join("data");
    if data.is_dir() {
        for entry in fs::read_dir(data)? {
            let entry = entry?;
            let name = entry.file_name();
            if entry.file_type()?.is_file()
                && Path::new(&name)
                    .extension()
                    .and_then(|value| value.to_str())
                    == Some(SEGMENT_FILE_EXTENSION)
            {
                link_or_copy(&entry.path(), &assembled.join("data").join(name))?;
            }
        }
    }
    Ok(())
}

fn link_or_copy(source: &Path, target: &Path) -> Result<()> {
    if fs::hard_link(source, target).is_err() {
        fs::copy(source, target)?;
    }
    Ok(())
}

fn assemble(output: &Path, _features: &str, engine: &Path, benchmark: bool) -> Result<()> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    #[cfg(windows)]
    {
        fs::copy(engine, output.join("keine.exe"))?;
        if has_feature(_features, "video-ffmpeg") {
            bundle_ffmpeg_runtime(output)?;
        }
    }
    #[cfg(not(windows))]
    {
        fs::copy(engine, output.join("keine"))?;
        #[cfg(target_os = "linux")]
        if has_feature(_features, "video-ffmpeg") {
            bundle_linux_runtime(output, engine)?;
        }
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(output.join("keine"), fs::Permissions::from_mode(0o755))?;
        }
    }
    fs::copy(
        repo_root.join("assets/icons/keine-256.png"),
        output.join("keine.png"),
    )?;
    if benchmark {
        fs::write(output.join(crate::runtime::BENCHMARK_MARKER), b"7\n")?;
        fs::write(output.join("BENCHMARK.txt"), BENCHMARK_README)?;
    }
    Ok(())
}

#[cfg(windows)]
fn bundle_ffmpeg_runtime(output: &Path) -> Result<()> {
    let vcpkg_root = env::var("VCPKG_ROOT")
        .context("VCPKG_ROOT is required to bundle the Windows FFmpeg runtime")?;
    let triplet = env::var("VCPKG_TARGET_TRIPLET").unwrap_or_else(|_| match env::consts::ARCH {
        "aarch64" => "arm64-windows".to_owned(),
        _ => "x64-windows".to_owned(),
    });
    let ffmpeg_bin = Path::new(&vcpkg_root)
        .join("installed")
        .join(&triplet)
        .join("bin");
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

#[cfg(target_os = "linux")]
fn bundle_linux_runtime(output: &Path, engine: &Path) -> Result<()> {
    use std::collections::{HashSet, VecDeque};

    let lib_dir = output.join("lib");
    fs::create_dir_all(&lib_dir)?;
    let mut queue = VecDeque::from([engine.to_owned()]);
    let mut visited = HashSet::new();
    let mut copied = 0;
    while let Some(binary) = queue.pop_front() {
        for library in linked_libraries(&binary)? {
            let name = library
                .file_name()
                .context("linked library path has no file name")?
                .to_owned();
            let canonical = library.canonicalize().unwrap_or(library);
            if is_linux_abi_library(&canonical) {
                continue;
            }
            let bundled = lib_dir.join(name);
            if !bundled.exists() {
                fs::copy(&canonical, bundled)?;
                copied += 1;
            }
            if visited.insert(canonical.clone()) {
                queue.push_back(canonical);
            }
        }
    }
    if copied == 0 {
        bail!("Linux video runtime has no dynamic libraries to bundle");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linked_libraries(binary: &Path) -> Result<Vec<PathBuf>> {
    let output = Command::new("ldd")
        .arg(binary)
        .output()
        .with_context(|| format!("failed to inspect {} with ldd", binary.display()))?;
    if !output.status.success() {
        bail!("ldd failed for {}", binary.display());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.lines().any(|line| line.contains("=> not found")) {
        bail!(
            "{} has an unresolved dynamic dependency:\n{stdout}",
            binary.display()
        );
    }
    Ok(parse_ldd(&stdout))
}

#[cfg(any(target_os = "linux", test))]
fn parse_ldd(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| {
            let dependency = line
                .split_once("=>")
                .map_or(line, |(_, target)| target)
                .split_whitespace()
                .next()?;
            dependency
                .starts_with('/')
                .then(|| PathBuf::from(dependency))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn is_linux_abi_library(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    [
        "ld-linux",
        "libc.so",
        "libdl.so",
        "libm.so",
        "libpthread.so",
        "librt.so",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

#[cfg(any(windows, target_os = "linux", test))]
fn has_feature(features: &str, wanted: &str) -> bool {
    features.split(',').any(|feature| feature == wanted)
}

const BENCHMARK_README: &str = "Kēne performance benchmark\n\nWindows: double-click keine.exe once.\nmacOS/Linux: run ./keine once in a terminal.\n\nFor an external-drive first-touch sample, extract or copy the complete directory\nto that drive, safely eject and reconnect it, then run the executable once from\nthe drive. The report never claims that operating-system or drive caches are\nforcibly cold. Package I/O is reported as warm Hakutaku/cache throughput, not\nas the physical throughput of the selected disk.\n\nThe package measures seven isolated startup runs, the actual packaged opening\ncomposition, a three-pass camera decomposition of that opening, three daily\nworkloads, all eight feature-coverage timelines, and a combined render stress\nworkload. The seven known low-end hotspots run in three isolated processes;\nordinary 60 Hz acceptance paths run once. The runtime samples preserve\nproduction camera sleep/wake behavior; only decomposition samples pin explicit\ncameras. It then reads real project assets and an isolated 204.2 MiB encrypted\npayload covering Hakutaku Hot, Normal/CLOCK admission, Transient, sequential\nStreaming, random seeks, and four parallel streams. The synthetic payload\nmeasures package I/O rather than codec decoding; valid project WebP/Opus\nresources remain part of the rendered workloads.\n\nIt uses an invisible real window and GPU surface, not a headless renderer, so\nrendering costs remain in the results without interrupting normal desktop use.\nPersistence is disabled. The single keine-benchmark-report.txt records the\nsource commit, build time, compiled Cargo features, host/display metadata, true\nslowest-1% average FPS, P99-equivalent FPS, benchmark-only render-pass\ndiagnostics, and a bounded tab-separated raw-frame appendix. When complete,\nsend that report from this directory to the developer. Do not move the\nexecutable away from game.haku or the data directory.\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_content_enables_the_platform_backend() {
        let root = tempdir().unwrap();
        let assets = root.path().join("assets");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("intro.mp4"), b"video").unwrap();
        let features = detect_features(root.path()).unwrap();
        assert!(has_feature(&features, VIDEO_FEATURE));
        assert!(has_feature(&features, "ui-sounds"));
    }

    #[test]
    fn parses_direct_and_resolved_ldd_dependencies() {
        let output = "\
            libavcodec.so.60 => /opt/keine/libavcodec.so.60 (0x1)\n\
            /lib64/ld-linux-x86-64.so.2 (0x2)\n\
            libmissing.so => not found\n";
        assert_eq!(
            parse_ldd(output),
            [
                PathBuf::from("/opt/keine/libavcodec.so.60"),
                PathBuf::from("/lib64/ld-linux-x86-64.so.2"),
            ]
        );
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
    fn compatibility_audio_never_expands_the_shipping_feature_set() {
        let root = tempdir().unwrap();
        let assets = root.path().join("assets");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("legacy.wav"), b"audio").unwrap();

        assert_eq!(detect_features(root.path()).unwrap(), "ui-sounds");
    }

    #[test]
    fn shipping_media_validation_is_scoped_to_asset_mounts() {
        let root = tempdir().unwrap();
        let assets = root.path().join("assets");
        fs::create_dir_all(&assets).unwrap();
        fs::write(root.path().join("thumbnail.png"), b"studio metadata").unwrap();
        fs::write(assets.join("background.webp"), b"image").unwrap();
        fs::write(assets.join("voice.opus"), b"audio").unwrap();
        let content = keine_loader::load_project(
            root.path(),
            &[keine_core::config::AssetSourceConfig {
                path: "assets".into(),
                format: "fs".into(),
            }],
        )
        .unwrap();

        validate_shipping_media(&content).unwrap();
        fs::write(assets.join("legacy.png"), b"image").unwrap();
        assert!(validate_shipping_media(&content).is_err());
        fs::remove_file(assets.join("legacy.png")).unwrap();
        fs::write(assets.join("legacy.mp3"), b"audio").unwrap();
        assert!(validate_shipping_media(&content).is_err());
    }

    #[test]
    fn publisher_output_cannot_select_target_itself_or_escape_it() {
        assert!(publisher_output_path(Path::new("target/bundle")).is_ok());
        assert!(publisher_output_path(Path::new("target/package")).is_ok());
        assert!(publisher_output_path(Path::new("target")).is_err());
        assert!(publisher_output_path(Path::new("target/../outside")).is_err());
        assert!(publisher_output_path(Path::new("/tmp/release")).is_err());
        assert!(publisher_output_path(Path::new("target/release")).is_err());
        assert!(publisher_output_path(Path::new("target/debug/package")).is_err());
        assert!(publisher_output_path(Path::new("target/publisher-runner/output")).is_err());
    }

    #[test]
    fn shipping_requires_a_stable_game_identity() {
        assert!(validate_shipping_identity(&Default::default()).is_err());
        assert!(
            validate_shipping_identity(&keine_core::config::ProjectMetadata {
                id: "example-game".into(),
                ..keine_core::config::ProjectMetadata::default()
            })
            .is_ok()
        );
    }

    #[test]
    fn engine_build_receives_only_derived_runtime_keys() {
        let mut command = Command::new("cargo");
        configure_engine_environment(
            &mut command,
            Path::new("share-a"),
            Path::new("share-b"),
            Path::new("public-key"),
        );
        let value = |name: &str| {
            command
                .get_envs()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value)
        };

        assert_eq!(value("HAKUTAKU_IDENTITY_BASE64"), Some(None));
        assert_eq!(value("KEINE_HAKUTAKU_IDENTITY"), Some(None));
        assert_eq!(
            value("KEINE_HAKUTAKU_KEY_SHARE_A"),
            Some(Some("share-a".as_ref()))
        );
        assert_eq!(
            value("KEINE_HAKUTAKU_KEY_SHARE_B"),
            Some(Some("share-b".as_ref()))
        );
        assert_eq!(
            value("KEINE_HAKUTAKU_PUBLIC_KEY"),
            Some(Some("public-key".as_ref()))
        );
    }

    #[test]
    fn linux_bundle_rpath_preserves_direct_executable_launches() {
        let mut command = Command::new("cargo");
        configure_linux_bundle_rpath(&mut command);
        let configured = command
            .get_envs()
            .filter_map(|(key, value)| {
                if matches!(key.to_str(), Some("RUSTFLAGS" | "CARGO_ENCODED_RUSTFLAGS")) {
                    value.map(|value| value.to_string_lossy().into_owned())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(configured.contains("--disable-new-dtags,-rpath,$ORIGIN/lib"));
    }

    #[test]
    fn assembled_release_has_no_launcher_scripts() {
        let root = tempdir().unwrap();
        let output = root.path().join("release");
        let engine = root.path().join("engine");
        fs::create_dir(&output).unwrap();
        fs::write(&engine, b"engine").unwrap();

        assemble(&output, "", &engine, true).unwrap();

        assert!(!output.join("run.sh").exists());
        assert!(!output.join("run.bat").exists());
        assert!(output.join(crate::runtime::BENCHMARK_MARKER).is_file());
    }

    #[test]
    fn asset_pack_contains_no_engine_or_bundle_metadata() {
        let root = tempdir().unwrap();
        let staged = root.path().join("staged");
        let output = root.path().join("package");
        fs::create_dir(&staged).unwrap();
        fs::write(staged.join("asset.txt"), b"asset").unwrap();
        let identity = Identity::generate().unwrap();

        publish_prepared(&staged, &identity, &output, |_| Ok(())).unwrap();

        assert!(output.join("game.haku").is_file());
        assert!(!output.join("keine").exists());
        assert!(!output.join("keine.exe").exists());
        assert!(!output.join("keine.png").exists());
        assert!(!output.join(crate::runtime::BENCHMARK_MARKER).exists());
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

    #[test]
    fn publisher_identity_is_created_once_and_reused() {
        let project = tempdir().unwrap();
        let path = project.path().join(".keine/publisher.hakutaku-key");
        let first = load_or_create_identity_at(&path).unwrap();
        let second = load_or_create_identity_at(&path).unwrap();
        assert_eq!(first.project_id(), second.project_id());
        assert!(
            project
                .path()
                .join(".keine/publisher.hakutaku-key")
                .is_file()
        );
    }

    #[test]
    fn generated_project_config_is_deterministic() {
        let mut first = keine_core::config::GameConfig::default();
        first
            .assets
            .backgrounds
            .insert("second".into(), "backgrounds/second.webp".into());
        first
            .assets
            .backgrounds
            .insert("first".into(), "backgrounds/first.webp".into());
        let mut second = keine_core::config::GameConfig::default();
        second
            .assets
            .backgrounds
            .insert("first".into(), "backgrounds/first.webp".into());
        second
            .assets
            .backgrounds
            .insert("second".into(), "backgrounds/second.webp".into());

        assert_eq!(
            serialize_config_deterministically(&first).unwrap(),
            serialize_config_deterministically(&second).unwrap()
        );
    }

    #[test]
    fn invalid_project_does_not_create_a_default_publisher_identity() {
        let project = tempdir().unwrap();
        fs::create_dir_all(project.path().join("assets")).unwrap();
        fs::create_dir_all(project.path().join("scripts")).unwrap();
        fs::write(project.path().join("scripts/start.txt"), "comment:test;\n").unwrap();
        let mut config = keine_core::config::GameConfig::default();
        config.project.id = "Invalid.Project".into();
        fs::write(
            project.path().join("config.yaml"),
            noyalib::to_string(&config).unwrap(),
        )
        .unwrap();

        assert!(prepare_project(project.path(), &keine_loader::LoaderRegistry::default()).is_err());
        assert!(
            !project
                .path()
                .join(".keine/publisher.hakutaku-key")
                .exists()
        );
    }

    #[test]
    fn previous_hakutaku_segments_seed_incremental_output() {
        let root = tempdir().unwrap();
        let previous = root.path().join("previous");
        let assembled = root.path().join("assembled");
        fs::create_dir_all(previous.join("data")).unwrap();
        fs::create_dir_all(&assembled).unwrap();
        fs::write(previous.join("game.haku"), b"snapshot").unwrap();
        fs::write(previous.join("data/kept.taku"), b"segment").unwrap();
        fs::write(previous.join("data/ignored.txt"), b"not a segment").unwrap();

        seed_previous_release(&previous, &assembled).unwrap();

        assert_eq!(fs::read(assembled.join("game.haku")).unwrap(), b"snapshot");
        assert_eq!(
            fs::read(assembled.join("data/kept.taku")).unwrap(),
            b"segment"
        );
        assert!(!assembled.join("data/ignored.txt").exists());
    }

    #[test]
    fn failed_assembly_preserves_the_previous_runnable_release() {
        let root = tempdir().unwrap();
        let staged = root.path().join("staged");
        let output = root.path().join("release");
        fs::create_dir(&staged).unwrap();
        fs::write(staged.join("asset.txt"), b"previous").unwrap();
        let identity = Identity::generate().unwrap();
        publish_prepared(&staged, &identity, &output, |_| Ok(())).unwrap();
        let previous_snapshot = fs::read(output.join("game.haku")).unwrap();
        let previous_segments = release_segments(&output);

        fs::write(staged.join("asset.txt"), b"replacement").unwrap();
        let error = publish_prepared(&staged, &identity, &output, |_| {
            bail!("injected assembly failure")
        })
        .unwrap_err();

        assert!(error.to_string().contains("injected assembly failure"));
        assert_eq!(
            fs::read(output.join("game.haku")).unwrap(),
            previous_snapshot
        );
        assert_eq!(release_segments(&output), previous_segments);
        assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".keine-publisher-")
        }));
    }

    fn release_segments(release: &Path) -> Vec<(std::ffi::OsString, Vec<u8>)> {
        let mut segments = fs::read_dir(release.join("data"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), fs::read(entry.path()).unwrap())
            })
            .collect::<Vec<_>>();
        segments.sort_by(|left, right| left.0.cmp(&right.0));
        segments
    }

    #[test]
    fn cleanup_failure_does_not_turn_a_committed_release_into_an_error() {
        let root = tempdir().unwrap();
        let backup = root.path().join("old-release");
        fs::write(&backup, b"not a directory").unwrap();

        cleanup_old_release_after_commit(&backup);

        assert!(backup.is_file());
    }
}
