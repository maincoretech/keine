//! `cargo compiler` — build-time compilation of a source project into
//! `.keine/compiled/program.bin` (codec in `keine-loader::compiled`).
//!
//! Compilation reuses the exact scene pipeline behind `cargo validate`:
//! source parsing with full diagnostics, resource existence checks, and the
//! same typed `Program` construction. The artifact is written atomically so a
//! crashed build never leaves a half-written program.bin.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use keine_core::Program;
use keine_core::config::GameConfig;
use keine_loader::compiled::{CompiledSceneV1, EncodeInput, ProgramMetadataV1, encode};
use keine_loader::{
    ContentProject, DiagnosticLevel, LoadedScene, ScriptLanguageRegistry, load_scenes_with,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileReport {
    pub output: PathBuf,
    pub scene_count: usize,
    pub action_count: u64,
    pub fingerprint: u64,
    pub warnings: usize,
}

pub fn compile_project(
    config: &GameConfig,
    content: &ContentProject,
    languages: &ScriptLanguageRegistry,
    output: Option<PathBuf>,
) -> Result<CompileReport> {
    let scenes =
        load_scenes_with(content, languages).context("failed to compile project scenes")?;
    let warnings = validate_scenes(config, content, &scenes)?;
    let action_count = scenes
        .iter()
        .map(|scene| scene.actions.len() as u64)
        .sum::<u64>();

    // The fingerprint is the identity of the source-built program; the runtime
    // compares it after reconstruction instead of trusting the header alone.
    let program = Program::from_scenes(
        scenes
            .iter()
            .map(|scene| (scene.name.clone(), scene.actions.clone())),
    );
    let fingerprint = program.fingerprint();
    let bytes = encode(&EncodeInput {
        scenes: scenes.iter().map(CompiledSceneV1::from_loaded).collect(),
        metadata: ProgramMetadataV1 {
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            source_adapter: config.adapter.script.clone(),
            scene_count: scenes.len() as u32,
            action_count,
            source_manifest_hash: manifest_hash(&scenes),
        },
        fingerprint,
    })
    .context("failed to encode compiled program")?;

    let output = output.unwrap_or_else(|| content.root.join(".keine/compiled/program.bin"));
    write_atomically(&output, &bytes)?;
    println!(
        "compiled · {} scene(s) · {action_count} action(s) · fingerprint {fingerprint:016x} · {warnings} warning(s) → {}",
        scenes.len(),
        output.display()
    );
    Ok(CompileReport {
        output,
        scene_count: scenes.len(),
        action_count,
        fingerprint,
        warnings,
    })
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
            if path.contains('{') || !missing.insert(path.clone()) {
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

/// Stable diagnostic hash over scene identities, independent of build order,
/// timestamps and absolute paths.
fn manifest_hash(scenes: &[LoadedScene]) -> u64 {
    let mut entries = scenes
        .iter()
        .map(|scene| format!("{}|{}", scene.name, scene.path.display()))
        .collect::<Vec<_>>();
    entries.sort();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for entry in entries {
        for byte in entry.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

fn write_atomically(output: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut temporary_name = output.as_os_str().to_os_string();
    temporary_name.push(format!(".tmp-{}", std::process::id()));
    let temporary = PathBuf::from(temporary_name);
    fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, output)
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
        let root = std::env::temp_dir().join(format!("keine-compiler-{}", std::process::id()));
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
        let output = root.join("out/program.bin");

        let report = compile_project(&config, &content, &languages, Some(output.clone())).unwrap();
        assert_eq!(report.scene_count, 1);
        assert_eq!(report.action_count, 2);

        let bytes = fs::read(&output).unwrap();
        let decoded = decode(&bytes, IR_SCHEMA_VERSION).unwrap();
        assert_eq!(decoded.scenes.len(), 1);
        assert_eq!(decoded.scenes[0].name, "start");
        assert_eq!(decoded.scenes[0].actions.len(), 2);
        assert_eq!(decoded.fingerprint, report.fingerprint);
        assert_eq!(
            decoded.metadata.source_adapter,
            config.adapter.script.as_str()
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compiler_rejects_error_diagnostics() {
        let root = std::env::temp_dir().join(format!("keine-compiler-bad-{}", std::process::id()));
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
        assert!(compile_project(&config, &content, &languages, None).is_err());

        let _ = fs::remove_dir_all(&root);
    }
}
