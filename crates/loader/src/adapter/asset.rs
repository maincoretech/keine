use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use keine_core::config::GameConfig;

use crate::loader::{
    COMPILED_PROGRAM_PATH, HakutakuArchive, SourceMount, load_hakutaku_project_from_archive,
    with_compiled_program,
};
use crate::source_input::{MAX_PROJECT_CONFIG_BYTES, SourceReader};
use crate::{AdaptedProject, ContentBackend, ContentMount, IR_SCHEMA_VERSION, ProjectAdapter};

/// Physical layout/container rules owned by one asset adapter.
pub trait FormatAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn mount(&self, project_root: &Path, location: &str) -> Result<SourceMount>;
}

fn resolve_local(project_root: &Path, location: &str) -> Result<PathBuf> {
    let project_root = project_root
        .canonicalize()
        .with_context(|| format!("failed to resolve project root {}", project_root.display()))?;
    let unresolved = project_root.join(location);
    let resolved = unresolved
        .canonicalize()
        .with_context(|| format!("failed to resolve adapter source {}", unresolved.display()))?;
    if !resolved.starts_with(&project_root) {
        bail!(
            "adapter source must stay inside project root {}: {}",
            project_root.display(),
            resolved.display()
        );
    }
    Ok(resolved)
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
        open_archive(archive, project_root)
    }
}

fn open_archive(archive: HakutakuArchive, project_root: &Path) -> Result<AdaptedProject> {
    let mount = ContentMount::new(ContentBackend::Hakutaku(archive.clone()), "")?;
    let yaml = SourceReader::with_limit(MAX_PROJECT_CONFIG_BYTES)
        .read_mount(&mount, Path::new("config.yaml"))?;
    let yaml = std::str::from_utf8(&yaml).context("Hakutaku config.yaml is not UTF-8")?;
    let config = GameConfig::from_yaml(yaml).context("invalid Hakutaku config.yaml")?;
    let path = Path::new(COMPILED_PROGRAM_PATH);
    if !archive.contains_file(path) {
        bail!("packaged project is missing required {COMPILED_PROGRAM_PATH}");
    }
    let file = mount.open_file(path)?;
    let length = file.len()?;
    let bin = crate::compiled::read_encoded(file, length, IR_SCHEMA_VERSION)
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
        format: "hakutaku",
        root,
        config,
        content,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use hakutaku_core::OpenPolicy;
    use hakutaku_pack::{Identity, PackOptions, pack_directory};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn packaged_config_obeys_the_same_limit_as_directory_config() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-package-config-limit-{nonce}"));
        let input = root.join("input");
        fs::create_dir_all(input.join(".keine/compiled")).unwrap();
        let program = crate::compiled::encode(&crate::compiled::EncodeInput {
            scenes: vec![crate::CompiledSceneV1 {
                name: "start".into(),
                actions: vec![],
                resources: vec![],
                sub_scenes: vec![],
            }],
            metadata: crate::ProgramMetadataV1 {
                compiler_version: env!("CARGO_PKG_VERSION").into(),
                engine_version: env!("CARGO_PKG_VERSION").into(),
                source_adapter: "keine".into(),
                scene_count: 1,
                action_count: 0,
                source_manifest_hash: 0,
            },
            fingerprint: keine_core::Program::from_scenes([("start".into(), vec![])]).fingerprint(),
        })
        .unwrap();
        fs::write(input.join(COMPILED_PROGRAM_PATH), &program).unwrap();
        let identity = Identity::generate().unwrap();
        for length in [MAX_PROJECT_CONFIG_BYTES, MAX_PROJECT_CONFIG_BYTES + 1] {
            let mut config = b"title: Fixture\n".to_vec();
            config.resize(length, b' ');
            fs::write(input.join("config.yaml"), config).unwrap();
            let release = root.join(format!("release-{length}"));
            pack_directory(&PackOptions::new(&input, &release), &identity).unwrap();
            let snapshot = release.join("game.haku");
            let archive = HakutakuArchive::open_with_keys(
                &snapshot,
                identity.root_key(),
                identity.public_key(),
                OpenPolicy::TrustFirstRelease,
            )
            .unwrap();
            match open_archive(archive, &snapshot) {
                Ok(project) => {
                    assert_eq!(length, MAX_PROJECT_CONFIG_BYTES);
                    assert_eq!(project.config.title, "Fixture");
                    assert_eq!(
                        crate::load_startup_scenes_with(
                            &project.content,
                            &crate::ScriptLanguageRegistry::default()
                        )
                        .unwrap()[0]
                            .name,
                        "start"
                    );
                }
                Err(error) => {
                    assert_eq!(length, MAX_PROJECT_CONFIG_BYTES + 1, "{error:#}");
                    assert!(format!("{error:#}").contains("per-file limit"), "{error:#}");
                }
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
}
