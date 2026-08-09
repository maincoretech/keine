//! Compiled program scene loader and the `compiled_program` policy.
//!
//! A packaged project can skip source-script parsing by shipping
//! `.keine/compiled/program.bin` (produced by `cargo compiler`). The artifact
//! decodes into the same typed scenes the source path produces, so the runtime
//! pipeline below `load_scenes` is unchanged.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use keine_core::config::CompiledProgramPolicy;

use crate::compiled::{CompiledProgramV1, decode};
use crate::loader::source::HexzArchive;
use crate::{ContentProject, LoadedScene, SourceSpan, StructuredSceneLoader};

/// Logical artifact location inside a project or package.
pub const COMPILED_PROGRAM_PATH: &str = ".keine/compiled/program.bin";

/// Scene source backed by a decoded compiled program.
#[derive(Debug, Clone)]
pub struct CompiledProgramSceneLoader {
    program: CompiledProgramV1,
    fingerprint: u64,
}

impl CompiledProgramSceneLoader {
    pub fn from_program_bin(bytes: &[u8], expected_schema: u32) -> Result<Self> {
        let decoded = decode(bytes, expected_schema)
            .map_err(|error| anyhow::anyhow!("invalid compiled program: {error}"))?;
        Ok(Self {
            program: CompiledProgramV1 {
                scenes: decoded.scenes,
            },
            fingerprint: decoded.fingerprint,
        })
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

impl StructuredSceneLoader for CompiledProgramSceneLoader {
    fn name(&self) -> &'static str {
        "compiled"
    }

    fn load(&self, _project_root: &Path) -> Result<Vec<LoadedScene>> {
        Ok(self
            .program
            .scenes
            .iter()
            .map(|scene| LoadedScene {
                name: scene.name.clone(),
                // Compiled artifacts carry no source positions; the synthetic
                // path keeps diagnostics and editor seek code unchanged.
                path: PathBuf::from(&scene.name),
                actions: scene.actions.clone(),
                action_spans: vec![SourceSpan { line: 1, column: 1 }; scene.actions.len()],
                diagnostics: Vec::new(),
                resources: scene.resources.clone(),
                sub_scenes: scene.sub_scenes.clone(),
            })
            .collect())
    }

    fn watch_roots(&self, _project_root: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    fn accepts_change(&self, _path: &Path) -> bool {
        false
    }
}

/// Read the compiled artifact for a directory project or a packaged file.
/// Returns `None` when the project has no artifact.
pub fn read_compiled_program(root: &Path) -> Result<Option<Vec<u8>>> {
    if root.is_dir() {
        let path = root.join(COMPILED_PROGRAM_PATH);
        if !path.is_file() {
            return Ok(None);
        }
        return std::fs::read(&path)
            .map(Some)
            .with_context(|| format!("failed to read {}", path.display()));
    }
    let archive = HexzArchive::open(root)
        .with_context(|| format!("failed to open package {}", root.display()))?;
    let path = Path::new(COMPILED_PROGRAM_PATH);
    if !archive.contains_file(path) {
        return Ok(None);
    }
    archive
        .read(path)
        .map(Some)
        .with_context(|| format!("failed to read {COMPILED_PROGRAM_PATH}"))
}

/// Replace source-script scenes with the compiled loader when the policy
/// allows it. `auto_trusts_bin` is true for packaged projects; development
/// directories keep source scripts under `Auto` so diagnostics and hot reload
/// stay live. A present-but-invalid artifact always fails loudly.
pub fn attach_compiled_program(
    project: ContentProject,
    policy: CompiledProgramPolicy,
    auto_trusts_bin: bool,
    bin: Option<Vec<u8>>,
    expected_schema: u32,
) -> Result<ContentProject> {
    let bytes = match policy {
        CompiledProgramPolicy::Disable => return Ok(project),
        CompiledProgramPolicy::Require => bin.ok_or_else(|| {
            anyhow::anyhow!("compiled program is required but {COMPILED_PROGRAM_PATH} is missing")
        })?,
        CompiledProgramPolicy::Auto if auto_trusts_bin => {
            let Some(bytes) = bin else {
                return Ok(project);
            };
            bytes
        }
        CompiledProgramPolicy::Auto => return Ok(project),
    };
    let loader = Arc::new(
        CompiledProgramSceneLoader::from_program_bin(&bytes, expected_schema)
            .with_context(|| format!("failed to load {COMPILED_PROGRAM_PATH}"))?,
    );
    Ok(ContentProject::with_structured_scenes(
        project.root,
        project.sources,
        loader,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::compiled::{IR_SCHEMA_VERSION, ProgramMetadataV1, encode};
    use keine_core::config::AssetSourceConfig;
    use keine_core::{Action, Program};

    fn compiled_bytes(action_count: usize) -> Vec<u8> {
        let scenes = vec![crate::CompiledSceneV1 {
            name: "start".to_string(),
            actions: vec![Action::Comment; action_count],
            resources: vec![],
            sub_scenes: vec![],
        }];
        let program = Program::from_scenes(
            scenes
                .iter()
                .map(|scene| (scene.name.clone(), scene.actions.clone())),
        );
        encode(&crate::EncodeInput {
            scenes,
            metadata: ProgramMetadataV1 {
                compiler_version: "test".to_string(),
                engine_version: "test".to_string(),
                source_adapter: "webgal".to_string(),
                scene_count: 1,
                action_count: action_count as u64,
                source_manifest_hash: 0,
            },
            fingerprint: program.fingerprint(),
        })
        .unwrap()
    }

    fn compiled_loader() -> CompiledProgramSceneLoader {
        CompiledProgramSceneLoader::from_program_bin(&compiled_bytes(2), IR_SCHEMA_VERSION).unwrap()
    }

    fn project(root: &Path) -> ContentProject {
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        crate::load_project(
            root,
            &[AssetSourceConfig {
                path: ".".to_string(),
                format: "fs".to_string(),
            }],
        )
        .unwrap()
    }

    #[test]
    fn compiled_loader_returns_typed_loaded_scenes() {
        let loader = compiled_loader();
        assert_eq!(loader.name(), "compiled");
        let scenes = loader.load(Path::new("/unused")).unwrap();
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].name, "start");
        assert_eq!(scenes[0].actions.len(), 2);
        assert_eq!(scenes[0].action_spans.len(), 2);
        assert!(scenes[0].resources.is_empty());
        assert_ne!(loader.fingerprint(), 0);
    }

    #[test]
    fn attach_policy_decides_between_source_and_compiled() {
        let root =
            std::env::temp_dir().join(format!("keine-compiled-loader-{}", std::process::id()));
        let project = project(&root);

        // Disable always keeps source scenes.
        let kept = attach_compiled_program(
            project.clone(),
            CompiledProgramPolicy::Disable,
            true,
            None,
            IR_SCHEMA_VERSION,
        )
        .unwrap();
        assert!(kept.scene_loader().is_none());

        // Auto ignores a missing artifact.
        let kept = attach_compiled_program(
            project.clone(),
            CompiledProgramPolicy::Auto,
            true,
            None,
            IR_SCHEMA_VERSION,
        )
        .unwrap();
        assert!(kept.scene_loader().is_none());

        // Auto with a present artifact attaches the compiled loader.
        let attached = attach_compiled_program(
            project.clone(),
            CompiledProgramPolicy::Auto,
            true,
            Some(compiled_bytes(1)),
            IR_SCHEMA_VERSION,
        )
        .unwrap();
        assert_eq!(attached.scene_loader().unwrap().name(), "compiled");

        // Require fails when the artifact is missing.
        assert!(
            attach_compiled_program(
                project,
                CompiledProgramPolicy::Require,
                true,
                None,
                IR_SCHEMA_VERSION,
            )
            .is_err()
        );
        let _ = fs::remove_dir_all(&root);
    }
}
