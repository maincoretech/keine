//! Internal bundle stage that builds `.keine/compiled/program.bin` from a
//! source project (codec in `keine-loader::compiled`).
//!
//! Compilation reuses the exact scene pipeline behind `cargo validate`:
//! source parsing with full diagnostics, resource existence checks, and the
//! same typed action model. The artifact is written atomically so a
//! crashed build never leaves a half-written program.bin.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use keine_core::Program;
use keine_core::config::GameConfig;
use keine_loader::compiled::{CompiledSceneV1, EncodeInput, ProgramMetadataV1, encode};
use keine_loader::{
    ContentProject, DiagnosticLevel, LoadedScene, ResourceKind, ScriptLanguageRegistry,
    load_scenes_with,
};

pub(crate) fn build_program(
    config: &GameConfig,
    content: &ContentProject,
    languages: &ScriptLanguageRegistry,
) -> Result<()> {
    let scenes =
        load_scenes_with(content, languages).context("failed to compile project scenes")?;
    let warnings = validate_scenes(config, content, &scenes)?;
    let action_count = scenes
        .iter()
        .map(|scene| scene.actions.len() as u64)
        .sum::<u64>();

    // Compute the source program identity without cloning the complete action
    // tree solely to construct temporary label indexes.
    let fingerprint = Program::fingerprint_scenes(
        scenes
            .iter()
            .map(|scene| (scene.name.as_str(), scene.actions.as_slice())),
    );
    let compiled_scenes = scenes
        .iter()
        .map(CompiledSceneV1::from_loaded)
        .collect::<Vec<_>>();
    let bytes = encode(&EncodeInput {
        metadata: ProgramMetadataV1 {
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            source_adapter: config.adapter.script.clone(),
            scene_count: scenes.len() as u32,
            action_count,
            source_manifest_hash: manifest_hash(&compiled_scenes)?,
        },
        scenes: compiled_scenes,
        fingerprint,
    })
    .context("failed to encode compiled program")?;

    let output = content.root.join(".keine/compiled/program.bin");
    write_atomically(&output, &bytes)?;
    println!(
        "compiled · {} scene(s) · {action_count} action(s) · fingerprint {fingerprint:016x} · {warnings} warning(s) → {}",
        scenes.len(),
        output.display()
    );
    Ok(())
}

/// Reject the same conditions as `cargo validate`: error diagnostics and
/// statically missing resources. Dynamic paths (containing `{`) are skipped.
fn validate_scenes(
    config: &GameConfig,
    content: &ContentProject,
    scenes: &[LoadedScene],
) -> Result<usize> {
    let mut warnings = 0usize;
    let mut errors = Vec::new();
    let mut missing = HashSet::new();
    let mut legacy_effects = HashSet::new();
    for scene in scenes {
        for diagnostic in &scene.diagnostics {
            let location = format!(
                "{}:{}:{}",
                scene.path.display(),
                diagnostic.span.line,
                diagnostic.span.column
            );
            match diagnostic.level {
                DiagnosticLevel::Error => {
                    errors.push(format!("{location}: {}", diagnostic.message))
                }
                DiagnosticLevel::Warning => {
                    warnings += 1;
                    eprintln!("warning: {location}: {}", diagnostic.message);
                }
            }
        }
        for resource in &scene.resources {
            let path = resource.resolved_path(config);
            if path.contains('{') {
                continue;
            }
            if resource.kind == ResourceKind::Effect
                && config.uses_legacy_effect_fallback(&resource.path)
                && legacy_effects.insert(resource.path.clone())
            {
                warnings += 1;
                eprintln!(
                    "warning: {}:{}:{}: bare sound effect {:?} uses the deprecated vocal/ fallback; use se/... or an assets.effects alias",
                    scene.path.display(),
                    resource.span.line,
                    resource.span.column,
                    resource.path,
                );
            }
            if !missing.insert(path.clone()) {
                continue;
            }
            if !content.contains_asset(Path::new(&path)) {
                errors.push(format!("resource does not exist: {path}"));
            }
        }
    }
    if !errors.is_empty() {
        for error in &errors {
            eprintln!("error: {error}");
        }
        bail!("project compilation failed with {} error(s)", errors.len());
    }
    Ok(warnings)
}

/// Stable hash over the compiled source manifest. Source paths are excluded:
/// LetsGal reports absolute paths, which would make temporary staging builds
/// produce different artifacts from the same project.
fn manifest_hash(scenes: &[CompiledSceneV1]) -> Result<u64> {
    let mut writer = ManifestHasher::default();
    postcard::to_io(scenes, &mut writer).context("failed to hash compiled scene manifest")?;
    Ok(writer.0)
}

struct ManifestHasher(u64);

impl Default for ManifestHasher {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Write for ManifestHasher {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn write_atomically(output: &Path, bytes: &[u8]) -> Result<()> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary file in {}", parent.display()))?;
    temporary
        .write_all(bytes)
        .with_context(|| format!("failed to write temporary file for {}", output.display()))?;
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use keine_core::config::AdapterConfig;
    use keine_loader::compiled::{IR_SCHEMA_VERSION, decode};
    use keine_loader::{LoaderRegistry, load_project};

    #[test]
    fn compiles_a_directory_project_into_a_decodeable_program_bin() {
        let root = std::env::temp_dir().join(format!("keine-program-build-{}", std::process::id()));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("scripts/start.txt"),
            "comment:first;\ncomment:second;\n",
        )
        .unwrap();

        let config = GameConfig {
            adapter: AdapterConfig {
                asset: vec![keine_core::config::AssetSourceConfig {
                    path: ".".to_string(),
                    format: "fs".to_string(),
                }],
                ..AdapterConfig::default()
            },
            ..GameConfig::default()
        };
        let content = load_project(
            &root,
            &[keine_core::config::AssetSourceConfig {
                path: ".".to_string(),
                format: "fs".to_string(),
            }],
        )
        .unwrap();
        let languages = LoaderRegistry::default()
            .languages("webgal")
            .unwrap()
            .clone();
        build_program(&config, &content, &languages).unwrap();

        let output = root.join(".keine/compiled/program.bin");
        let bytes = fs::read(&output).unwrap();
        let decoded = decode(&bytes, IR_SCHEMA_VERSION).unwrap();
        assert_eq!(decoded.scenes.len(), 1);
        assert_eq!(decoded.scenes[0].name, "start");
        assert_eq!(decoded.scenes[0].actions.len(), 2);
        assert_ne!(decoded.fingerprint, 0);
        assert_eq!(
            decoded.metadata.source_adapter,
            config.adapter.script.as_str()
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bundle_stage_rejects_error_diagnostics() {
        let root =
            std::env::temp_dir().join(format!("keine-program-build-bad-{}", std::process::id()));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("scripts/start.txt"), "callScene:missing;\n").unwrap();

        let config = GameConfig::default();
        let content = load_project(
            &root,
            &[keine_core::config::AssetSourceConfig {
                path: ".".to_string(),
                format: "fs".to_string(),
            }],
        )
        .unwrap();
        let languages = LoaderRegistry::default()
            .languages("webgal")
            .unwrap()
            .clone();
        assert!(build_program(&config, &content, &languages).is_err());

        let _ = fs::remove_dir_all(&root);
    }
}
