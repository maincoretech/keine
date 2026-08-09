use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use keine_core::config::{CompiledProgramPolicy, GameConfig};

use crate::loader::{HexzArchive, SourceMount, load_hexz_project_from_archive};
use crate::{
    AdaptedProject, COMPILED_PROGRAM_PATH, IR_SCHEMA_VERSION, ProjectAdapter,
    attach_compiled_program,
};

/// Physical layout/container rules owned by one asset adapter.
pub trait FormatAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn mount(&self, project_root: &Path, location: &str) -> Result<SourceMount>;
}

fn resolve_local(project_root: &Path, location: &str) -> Result<PathBuf> {
    let unresolved = project_root.join(location);
    unresolved
        .canonicalize()
        .with_context(|| format!("failed to resolve adapter source {}", unresolved.display()))
}

/// Development filesystem source with direct logical-path access and no unpack step.
pub(crate) struct FsFormat;

impl FormatAdapter for FsFormat {
    fn name(&self) -> &'static str {
        "fs"
    }

    fn mount(&self, project_root: &Path, location: &str) -> Result<SourceMount> {
        let root = resolve_local(project_root, location)?;
        if !root.is_dir() {
            bail!(
                "filesystem asset source is not a directory: {}",
                root.display()
            );
        }
        if root.join("assets").is_dir() || root.join("scripts").is_dir() {
            Ok(SourceMount::project(self.name(), root))
        } else {
            Ok(SourceMount::assets(
                self.name(),
                root.display().to_string(),
                root,
            ))
        }
    }
}

/// Standard Hexz asset archive backed by `hexz_k`.
pub(crate) struct HexzFormat;

/// Complete packaged-project opener kept next to the Hexz asset format.
pub(crate) struct HexzProjectAdapter;

impl ProjectAdapter for HexzProjectAdapter {
    fn name(&self) -> &'static str {
        "hexz"
    }

    fn detect(&self, project_root: &Path) -> Result<bool> {
        Ok(project_root.is_file()
            && project_root.extension().and_then(|value| value.to_str()) == Some("hxz"))
    }

    fn open(&self, project_root: &Path) -> Result<AdaptedProject> {
        let archive = mount_hexz(project_root)?;
        let yaml = archive.read(Path::new("config.yaml"))?;
        let yaml = std::str::from_utf8(&yaml).context("Hexz config.yaml is not UTF-8")?;
        let config = GameConfig::from_yaml(yaml).context("invalid Hexz config.yaml")?;
        let path = Path::new(COMPILED_PROGRAM_PATH);
        let bin = (config.compiled_program != CompiledProgramPolicy::Disable
            && archive.contains_file(path))
        .then(|| {
            archive
                .read(path)
                .context("failed to read compiled program")
        })
        .transpose()?;
        let content = load_hexz_project_from_archive(archive, &config.adapter.asset)?;
        let content = attach_compiled_program(
            content,
            config.compiled_program,
            true,
            bin,
            IR_SCHEMA_VERSION,
        )?;
        let root = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_owned())
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_owned();
        Ok(AdaptedProject {
            format: self.name(),
            root,
            config,
            content,
        })
    }
}

impl FormatAdapter for HexzFormat {
    fn name(&self) -> &'static str {
        "hexz"
    }

    fn mount(&self, project_root: &Path, location: &str) -> Result<SourceMount> {
        let package = resolve_local(project_root, location)?;
        let archive = mount_hexz(&package)?;
        if archive.is_directory(Path::new("assets")) || archive.is_directory(Path::new("scripts")) {
            SourceMount::hexz_project(self.name(), archive, "")
        } else {
            SourceMount::hexz_assets(self.name(), archive, "")
        }
    }
}

/// Open a standard Hexz archive without extracting it.
pub fn mount_hexz(package: &Path) -> Result<HexzArchive> {
    HexzArchive::open(package)
}

/// Convenience selector for development inputs; concrete formats keep their
/// own adapter modules below this category.
pub(crate) struct AutoFormat;

impl FormatAdapter for AutoFormat {
    fn name(&self) -> &'static str {
        "auto"
    }

    fn mount(&self, project_root: &Path, location: &str) -> Result<SourceMount> {
        let path = resolve_local(project_root, location)?;
        if path.extension().and_then(|value| value.to_str()) == Some("hxz") {
            HexzFormat.mount(project_root, location)
        } else {
            FsFormat.mount(project_root, location)
        }
    }
}
