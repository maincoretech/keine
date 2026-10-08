//! Read-only, native playtest exports using the installed development Engine.
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use keine_core::config::{EiyashouAssetManifest, GameConfig};
use keine_loader::{ContentBackend, ContentMount, MAX_PROJECT_CONFIG_BYTES, load_project};

use super::WorkspaceSession;

#[derive(Clone, Debug)]
pub(crate) struct ExportedGame {
    pub directory: PathBuf,
    pub bytes: u64,
}

pub(crate) fn export_game(root: &Path, parent: &Path, engine: &Path) -> Result<ExportedGame> {
    let root = root.canonicalize()?;
    let parent = parent.canonicalize()?;
    if parent.starts_with(&root) {
        bail!("Choose an export folder outside the project");
    }
    let mount = ContentMount::new(ContentBackend::FileSystem(root.clone()), "")?;
    let config = GameConfig::from_yaml(&read_text(
        &mount,
        "config.yaml",
        MAX_PROJECT_CONFIG_BYTES as u64,
    )?)?;
    if config.adapter.script != "keine" {
        bail!("Migrate this project to Eiyashou before exporting a playtest");
    }
    let content = load_project(&root, &config.adapter.asset)?;
    for mount in content
        .asset_mounts()
        .into_iter()
        .chain(content.script_mounts())
    {
        let source = mount
            .filesystem_root()
            .context("Playtest export requires local resource sources")?;
        if !source.starts_with(&root) {
            bail!("Resource sources must stay inside the project");
        }
    }
    let files = export_files(&root, &mount, &config)?;
    // Validate the exact disk revision before copying; validate the exported
    // revision again before publishing success. No publisher keys are used.
    validate(engine, &root)?;
    let name = root.file_name().and_then(|s| s.to_str()).unwrap_or("game");
    let directory = reserve_directory(&parent, name)?;
    let result = assemble(&directory, engine, &mount, &files, &content, &config);
    match result {
        Ok(export) => Ok(export),
        Err(error) => {
            if let Err(cleanup) = fs::remove_dir_all(&directory) {
                return Err(error.context(format!(
                    "Incomplete export remains at {}: {cleanup}",
                    directory.display()
                )));
            }
            Err(error)
        }
    }
}

fn export_files(
    root: &Path,
    mount: &ContentMount,
    config: &GameConfig,
) -> Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::from([PathBuf::from("config.yaml")]);
    if !config.project.icon.is_empty() {
        let path = Path::new(&config.project.icon);
        safe_relative(path)?;
        // Validate before allocating an export directory, and retain the source
        // for the installed Engine's window icon in the playtest copy.
        keine_media::icons::IconSet::read(mount.open_file(path)?)?;
        files.insert(path.to_owned());
    }
    for path in [
        &config.script.assets,
        &config.script.characters,
        &config.script.objects,
    ] {
        if path.is_empty() {
            continue;
        }
        safe_relative(Path::new(path))?;
        if mount.contains_file(Path::new(path)) {
            files.insert(PathBuf::from(path));
        }
    }
    let manifest = EiyashouAssetManifest::from_yaml(&read_text(
        mount,
        &config.script.assets,
        super::MAX_DOCUMENT_BYTES,
    )?)?;
    for namespace in [
        manifest.backgrounds,
        manifest.figures,
        manifest.voices,
        manifest.bgm,
        manifest.effects,
        manifest.videos,
        manifest.particles,
    ] {
        for entry in namespace.values() {
            let path = PathBuf::from(entry.path());
            safe_relative(&path)?;
            files.insert(path);
        }
    }
    // Additional configured media (e.g. LUTs) use ordered mount lookup.
    let content = load_project(root, &config.adapter.asset)?;
    for namespace in [
        &config.assets.backgrounds,
        &config.assets.figures,
        &config.assets.bgm,
        &config.assets.voices,
        &config.assets.effects,
        &config.assets.videos,
        &config.assets.luts,
    ] {
        for logical in namespace.values() {
            let logical = Path::new(logical);
            safe_relative(logical)?;
            let source = content
                .asset_mounts()
                .into_iter()
                .rev()
                .find(|m| m.contains_file(logical))
                .with_context(|| format!("Missing configured resource: {}", logical.display()))?;
            let physical = source
                .filesystem_root()
                .context("Resource source is not a directory")?
                .join(logical);
            files.insert(physical.strip_prefix(root)?.to_owned());
        }
    }
    for file in WorkspaceSession::open(root)?.files() {
        if !file.is_dir()
            && file
                .relative_path
                .extension()
                .is_some_and(|ext| ext == "shou")
        {
            files.insert(file.relative_path.clone());
        }
    }
    Ok(files)
}

fn assemble(
    directory: &Path,
    engine: &Path,
    mount: &ContentMount,
    files: &BTreeSet<PathBuf>,
    content: &keine_loader::ContentProject,
    config: &GameConfig,
) -> Result<ExportedGame> {
    let icons = if config.project.icon.is_empty() {
        None
    } else {
        Some(keine_media::icons::IconSet::read(
            mount.open_file(Path::new(&config.project.icon))?,
        )?)
    };
    let (executable, project) = export_layout(directory);
    let notices = if cfg!(target_os = "macos") {
        directory.join("Game.app/Contents/Resources")
    } else {
        directory.to_owned()
    };
    fs::create_dir_all(&notices)?;
    for (name, text) in [
        ("LICENSE", include_str!("../../../../LICENSE")),
        ("NOTICE", include_str!("../../../../NOTICE")),
        (
            "FONT-LICENSES.txt",
            include_str!("../../../../src/assets/fonts/FONT-LICENSES.txt"),
        ),
    ] {
        fs::write(notices.join(name), text)?;
    }
    fs::create_dir_all(executable.parent().context("Engine has no parent")?)?;
    fs::create_dir_all(&project)?;
    for source in content
        .asset_mounts()
        .into_iter()
        .chain(content.script_mounts())
    {
        let source = source
            .filesystem_root()
            .context("Resource source is not a directory")?;
        fs::create_dir_all(project.join(source.strip_prefix(&content.root)?))?;
    }
    let mut bytes = include_str!("../../../../LICENSE").len() as u64
        + include_str!("../../../../NOTICE").len() as u64
        + include_str!("../../../../src/assets/fonts/FONT-LICENSES.txt").len() as u64;
    for relative in files {
        safe_relative(relative)?;
        let target = project.join(relative);
        fs::create_dir_all(target.parent().context("Resource has no parent")?)?;
        let mut input = mount.open_file(relative)?;
        bytes += io::copy(&mut input, &mut File::create(&target)?)?;
    }
    bytes += fs::copy(engine, &executable)?;
    // Installed Windows/Linux engines can carry adjacent dynamic libraries.
    copy_runtime_libraries(engine, executable.parent().unwrap(), &mut bytes)?;
    validate(&executable, &project)?;
    #[cfg(target_os = "macos")]
    {
        let contents = directory.join("Game.app/Contents");
        let icon = icons
            .as_ref()
            .map(|icons| icons.icns())
            .unwrap_or_else(|| include_bytes!("../../../../src/assets/icons/keine.icns").to_vec());
        fs::write(notices.join("keine.icns"), &icon)?;
        bytes += icon.len() as u64;
        let identifier = format!("moe.maincore.keine.playtest.{}", config.project.id);
        fs::write(
            contents.join("Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>keine</string><key>CFBundleIdentifier</key><string>{}</string><key>CFBundleName</key><string>{}</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleIconFile</key><string>keine.icns</string><key>CFBundleShortVersionString</key><string>{}</string></dict></plist>",
                xml(&identifier),
                xml(&config.title),
                env!("CARGO_PKG_VERSION")
            ),
        )?;
        let status = Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(directory.join("Game.app"))
            .status()?;
        if !status.success() {
            bail!("Could not sign the playtest app");
        }
    }
    #[cfg(target_os = "linux")]
    {
        let icon = icons
            .as_ref()
            .map(|icons| icons.png(512))
            .unwrap_or(include_bytes!("../../../../src/assets/icons/keine-512.png"));
        let installer = include_str!("../../../../dev/scripts/install-desktop.py");
        fs::write(directory.join("keine.png"), icon)?;
        fs::write(directory.join("install-desktop.py"), installer)?;
        let desktop = serde_json::to_vec_pretty(&serde_json::json!({
            "id": config.project.application_identifier().unwrap_or_else(|| "moe.maincore.keine".into()),
            "name": config.title, "executable": "keine", "arguments": ["project"], "category": "Game",
        }))?;
        fs::write(directory.join("DESKTOP.json"), &desktop)?;
        bytes += (icon.len() + installer.len() + desktop.len()) as u64;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = (config, icons);
    fs::write(
        directory.join("PLAYTEST.txt"),
        "Temporary playtest for this operating system. Launch Game.app (macOS) or keine/keine.exe (Linux/Windows).\nContains readable scripts and registered resources; not a Hakutaku release.\nLinux optional launcher: python3 install-desktop.py --install (after unpacking at its final location).\n",
    )?;
    Ok(ExportedGame {
        directory: directory.to_owned(),
        bytes,
    })
}

fn export_layout(directory: &Path) -> (PathBuf, PathBuf) {
    if cfg!(target_os = "macos") {
        (
            directory.join("Game.app/Contents/MacOS/keine"),
            directory.join("Game.app/Contents/Resources/project"),
        )
    } else {
        (
            directory.join(format!("keine{}", std::env::consts::EXE_SUFFIX)),
            directory.join("project"),
        )
    }
}

pub(crate) fn launch_game(game: &ExportedGame) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = Command::new("open");
        c.arg(game.directory.join("Game.app"));
        c
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = Command::new(export_layout(&game.directory).0);
    command
        .current_dir(&game.directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

fn validate(engine: &Path, project: &Path) -> Result<()> {
    let output = Command::new(engine)
        .arg("validate")
        .arg(project)
        .env_remove("KEINE_HAKUTAKU_IDENTITY")
        .env_remove("HAKUTAKU_IDENTITY_BASE64")
        .stdin(Stdio::null())
        .output()
        .context("Could not run Engine validation")?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Project validation failed: {}",
            message.chars().take(4000).collect::<String>()
        );
    }
    Ok(())
}

fn reserve_directory(parent: &Path, name: &str) -> io::Result<PathBuf> {
    for index in 0..1000 {
        let suffix = if index == 0 {
            String::new()
        } else {
            format!("-{index}")
        };
        let path = parent.join(format!("{name}-playtest{suffix}"));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Too many playtest exports in this folder",
    ))
}

fn safe_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        bail!(
            "Export file must stay inside the project: {}",
            path.display()
        );
    }
    if path
        .components()
        .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
    {
        bail!(
            "Private project files cannot be exported: {}",
            path.display()
        );
    }
    Ok(())
}

fn read_text(mount: &ContentMount, path: &str, limit: u64) -> Result<String> {
    let mut input = mount.open_file(Path::new(path))?;
    if input.len()? > limit {
        bail!("Project document exceeds its size limit: {path}");
    }
    let mut bytes = Vec::new();
    input.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("Project document exceeds its size limit: {path}");
    }
    Ok(String::from_utf8(bytes)?)
}

fn copy_runtime_libraries(engine: &Path, output: &Path, bytes: &mut u64) -> Result<()> {
    if cfg!(target_os = "macos") {
        return Ok(());
    }
    let directory = engine.parent().context("Engine has no parent")?;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dll"))
        {
            *bytes += fs::copy(entry.path(), output.join(entry.file_name()))?;
        }
    }
    if directory.join("lib").is_dir() {
        copy_library_tree(&directory.join("lib"), &output.join("lib"), bytes)?;
    }
    Ok(())
}

fn copy_library_tree(source: &Path, output: &Path, bytes: &mut u64) -> Result<()> {
    fs::create_dir_all(output)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::metadata(&path)?;
        if metadata.is_dir() && !entry.file_type()?.is_symlink() {
            copy_library_tree(&path, &output.join(entry.file_name()), bytes)?;
        } else if metadata.is_file() {
            *bytes += fs::copy(&path, output.join(entry.file_name()))?;
        } else {
            bail!("Unsupported runtime library: {}", path.display());
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("keine-export-{nonce}"));
            fs::create_dir_all(root.join("assets")).unwrap();
            fs::create_dir_all(root.join("scripts")).unwrap();
            fs::write(
                root.join("config.yaml"),
                "adapter:\n  script: keine\nscript:\n  version: 2\n  entry: start\n",
            )
            .unwrap();
            fs::write(
                root.join("assets.yaml"),
                "backgrounds:\n  day: assets/day.webp\n",
            )
            .unwrap();
            fs::write(root.join("characters.yaml"), "characters: {}\n").unwrap();
            fs::write(root.join("assets/day.webp"), b"registered").unwrap();
            fs::write(root.join("assets/day.png"), b"source original").unwrap();
            fs::write(root.join("scripts/one.shou"), "scene start { goto(next), }").unwrap();
            fs::write(root.join("scripts/two.shou"), "scene next { story.end(), }").unwrap();
            fs::create_dir_all(root.join(".keine")).unwrap();
            fs::write(root.join(".keine/publisher.key"), b"must not export").unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn custom_icon_is_retained_and_missing_or_escaping_icon_fails() {
        let fixture = Fixture::new();
        let root = fixture.0.canonicalize().unwrap();
        let mount = ContentMount::new(ContentBackend::FileSystem(root.clone()), "").unwrap();
        let mut config =
            GameConfig::from_yaml(&fs::read_to_string(root.join("config.yaml")).unwrap()).unwrap();
        config.project.icon = "app.png".into();
        fs::write(
            root.join("app.png"),
            include_bytes!("../../../../src/assets/icons/keine-256.png"),
        )
        .unwrap();
        assert!(
            export_files(&root, &mount, &config)
                .unwrap()
                .contains(Path::new("app.png"))
        );
        config.project.icon = "missing.png".into();
        assert!(export_files(&root, &mount, &config).is_err());
        config.project.icon = "../app.png".into();
        assert!(export_files(&root, &mount, &config).is_err());
    }

    #[test]
    fn copies_registered_resources_and_all_chapters_without_keys_or_originals() {
        let fixture = Fixture::new();
        let root = fixture.0.canonicalize().unwrap();
        let mount = ContentMount::new(ContentBackend::FileSystem(root.clone()), "").unwrap();
        let config =
            GameConfig::from_yaml(&fs::read_to_string(root.join("config.yaml")).unwrap()).unwrap();
        let files = export_files(&root, &mount, &config).unwrap();
        assert_eq!(
            files,
            BTreeSet::from(
                [
                    "config.yaml",
                    "assets.yaml",
                    "characters.yaml",
                    "scripts/one.shou",
                    "scripts/two.shou",
                    "assets/day.webp"
                ]
                .map(PathBuf::from)
            )
        );
        assert!(safe_relative(Path::new(".keine/publisher.key")).is_err());
        assert!(safe_relative(Path::new("../outside.webp")).is_err());
        assert!(safe_relative(Path::new("/outside.webp")).is_err());
    }

    #[test]
    fn rejects_in_project_output_and_escaping_resource_paths() {
        let fixture = Fixture::new();
        let parent = fixture.0.join("output");
        fs::create_dir(&parent).unwrap();
        assert!(export_game(&fixture.0, &parent, Path::new("missing-engine")).is_err());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        fs::write(
            fixture.0.join("assets.yaml"),
            "backgrounds:\n  day: ../outside.webp\n",
        )
        .unwrap();
        let root = fixture.0.canonicalize().unwrap();
        let mount = ContentMount::new(ContentBackend::FileSystem(root.clone()), "").unwrap();
        let config =
            GameConfig::from_yaml(&fs::read_to_string(root.join("config.yaml")).unwrap()).unwrap();
        assert!(export_files(&root, &mount, &config).is_err());
    }

    #[test]
    fn repeated_exports_reserve_new_directories_and_preserve_existing_files() {
        let fixture = Fixture::new();
        let first = reserve_directory(&fixture.0, "game").unwrap();
        fs::write(first.join("keep.txt"), b"previous export").unwrap();
        let second = reserve_directory(&fixture.0, "game").unwrap();
        assert_ne!(first, second);
        assert_eq!(
            fs::read(first.join("keep.txt")).unwrap(),
            b"previous export"
        );
    }

    #[cfg(unix)]
    #[test]
    fn failed_export_validation_removes_only_the_new_export() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new();
        let engine = fixture.0.join("engine");
        fs::write(
            &engine,
            "#!/bin/sh\ncase \"$2\" in *-playtest*) echo invalid >&2; exit 1;; *) exit 0;; esac\n",
        )
        .unwrap();
        fs::set_permissions(&engine, fs::Permissions::from_mode(0o755)).unwrap();
        let parent = fixture.0.parent().unwrap().join(format!(
            "{}-exports",
            fixture.0.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir(&parent).unwrap();
        let previous =
            reserve_directory(&parent, fixture.0.file_name().unwrap().to_str().unwrap()).unwrap();
        fs::write(previous.join("keep"), b"existing").unwrap();
        let error = export_game(&fixture.0, &parent, &engine).unwrap_err();
        assert!(error.to_string().contains("invalid"), "{error:#}");
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        assert_eq!(fs::read(previous.join("keep")).unwrap(), b"existing");
        assert_eq!(
            fs::read(fixture.0.join("assets/day.webp")).unwrap(),
            b"registered"
        );
        fs::remove_dir_all(parent).unwrap();
    }
}
