use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use keine_core::config::GameConfig;

use crate::loader::{
    COMPILED_PROGRAM_PATH, HakutakuArchive, SourceMount, load_hakutaku_project_from_archive,
    with_compiled_program,
};
use crate::{AdaptedProject, IR_SCHEMA_VERSION, ProjectAdapter};

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

/// Complete packaged-project opener kept beside the filesystem asset formats.
pub(crate) struct HakutakuProjectAdapter;

impl ProjectAdapter for HakutakuProjectAdapter {
    fn name(&self) -> &'static str {
        "hakutaku"
    }

    fn detect(&self, project_root: &Path) -> Result<bool> {
        Ok(project_root.is_file()
            && project_root.extension().and_then(|value| value.to_str()) == Some("haku"))
    }

    fn open(&self, project_root: &Path) -> Result<AdaptedProject> {
        let archive = HakutakuArchive::open_packaged(project_root)?;
        let yaml = archive.read(Path::new("config.yaml"))?;
        let yaml = std::str::from_utf8(&yaml).context("Hakutaku config.yaml is not UTF-8")?;
        let config = GameConfig::from_yaml(yaml).context("invalid Hakutaku config.yaml")?;
        let path = Path::new(COMPILED_PROGRAM_PATH);
        if !archive.contains_file(path) {
            bail!("packaged project is missing required {COMPILED_PROGRAM_PATH}");
        }
        let bin = archive
            .read(path)
            .context("failed to read packaged program")?;
        let content = load_hakutaku_project_from_archive(archive, &config.adapter.asset)?;
        let content = with_compiled_program(content, &bin, IR_SCHEMA_VERSION)?;
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

/// Convenience selector for development inputs; concrete formats keep their
/// own adapter modules below this category.
pub(crate) struct AutoFormat;

impl FormatAdapter for AutoFormat {
    fn name(&self) -> &'static str {
        "auto"
    }

    fn mount(&self, project_root: &Path, location: &str) -> Result<SourceMount> {
        FsFormat.mount(project_root, location)
    }
}
