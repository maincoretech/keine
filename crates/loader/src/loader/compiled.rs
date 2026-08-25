//! Compiled program scene loader for packaged games.
//!
//! Every packaged project ships `.keine/compiled/program.bin`, produced by the
//! bundle pipeline. Directory projects always use source scripts, while Hakutaku
//! projects always use this loader, so there is no runtime policy to resolve.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::compiled::{CompiledProgramV1, decode};
use crate::{ContentProject, LoadedScene, SourceSpan, StructuredSceneLoader};
use anyhow::{Context, Result};

/// Logical artifact location inside a project or package.
pub(crate) const COMPILED_PROGRAM_PATH: &str = ".keine/compiled/program.bin";

/// Scene source backed by a decoded compiled program.
#[derive(Debug)]
pub(crate) struct CompiledProgramSceneLoader {
    program: Mutex<Option<CompiledProgramV1>>,
}

impl CompiledProgramSceneLoader {
    fn from_program_bin(bytes: &[u8], expected_schema: u32) -> Result<Self> {
        let decoded = decode(bytes, expected_schema)
            .map_err(|error| anyhow::anyhow!("invalid compiled program: {error}"))?;
        Ok(Self {
            program: Mutex::new(Some(CompiledProgramV1 {
                scenes: decoded.scenes,
            })),
        })
    }

    fn loaded_scene(scene: crate::CompiledSceneV1) -> LoadedScene {
        let action_count = scene.actions.len();
        LoadedScene {
            name: scene.name.clone(),
            // Compiled artifacts carry no source positions; the synthetic
            // path keeps diagnostics and editor seek code unchanged.
            path: PathBuf::from(&scene.name),
            actions: scene.actions,
            action_spans: vec![SourceSpan { line: 1, column: 1 }; action_count],
            diagnostics: Vec::new(),
            resources: scene.resources,
            sub_scenes: scene.sub_scenes,
        }
    }

    fn lock_program(&self) -> std::sync::MutexGuard<'_, Option<CompiledProgramV1>> {
        self.program
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl StructuredSceneLoader for CompiledProgramSceneLoader {
    fn name(&self) -> &'static str {
        "compiled"
    }

    fn load(&self, _project_root: &Path) -> Result<Vec<LoadedScene>> {
        let program = self.lock_program();
        let program = program
            .as_ref()
            .context("compiled startup scenes were already transferred")?;
        Ok(program
            .scenes
            .iter()
            .cloned()
            .map(Self::loaded_scene)
            .collect())
    }

    fn load_startup(&self, _project_root: &Path) -> Result<Vec<LoadedScene>> {
        let program = self
            .lock_program()
            .take()
            .context("compiled startup scenes were already transferred")?;
        Ok(program.scenes.into_iter().map(Self::loaded_scene).collect())
    }

    fn watch_roots(&self, _project_root: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    fn accepts_change(&self, _path: &Path) -> bool {
        false
    }
}

/// Replace packaged source scripts with their required compiled program.
pub(crate) fn with_compiled_program(
    project: ContentProject,
    bytes: &[u8],
    expected_schema: u32,
) -> Result<ContentProject> {
    let loader = Arc::new(
        CompiledProgramSceneLoader::from_program_bin(bytes, expected_schema)
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
    }

    #[test]
    fn compiled_loader_transfers_and_releases_its_action_tree_once() {
        let loader = compiled_loader();
        let scenes = loader.load_startup(Path::new("/unused")).unwrap();

        assert_eq!(scenes[0].actions.len(), 2);
        assert!(loader.lock_program().is_none());
        assert!(loader.load(Path::new("/unused")).is_err());
        assert!(loader.load_startup(Path::new("/unused")).is_err());
    }

    #[test]
    fn packaged_project_uses_compiled_scenes() {
        let root =
            std::env::temp_dir().join(format!("keine-compiled-loader-{}", std::process::id()));
        let project = project(&root);
        let attached =
            with_compiled_program(project, &compiled_bytes(1), IR_SCHEMA_VERSION).unwrap();
        assert_eq!(attached.scene_loader().unwrap().name(), "compiled");
        let _ = fs::remove_dir_all(&root);
    }
}
