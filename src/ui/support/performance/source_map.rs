//! Benchmark-only source positions; the shipping Program schema stays unchanged.
use std::path::{Component, Path, PathBuf};

#[cfg(any(feature = "startup-metrics", test))]
use anyhow::Context;
#[cfg(feature = "startup-metrics")]
use anyhow::bail;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub(crate) const MAP_PATH: &str = "__keine_benchmark__/source-map.bin";
const MAX_MAP_BYTES: usize = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct SourceMap {
    version: u8,
    fingerprint: u64,
    scenes: Vec<SceneMap>,
}

#[derive(Serialize, Deserialize)]
struct SceneMap {
    name: String,
    source: PathBuf,
    spans: Vec<keine_loader::SourceSpan>,
}

#[cfg(feature = "publisher")]
pub(crate) fn encode(root: &Path, scenes: &[keine_loader::LoadedScene]) -> Result<Vec<u8>> {
    let map = SourceMap {
        version: 1,
        fingerprint: keine_core::Program::fingerprint_scenes(
            scenes
                .iter()
                .map(|s| (s.name.as_str(), s.actions.as_slice())),
        ),
        scenes: scenes
            .iter()
            .map(|s| SceneMap {
                name: s.name.clone(),
                source: s.path.strip_prefix(root).unwrap_or(&s.path).to_path_buf(),
                spans: s.action_spans.clone(),
            })
            .collect(),
    };
    for scene in &map.scenes {
        validate_path(&scene.source)?;
    }
    let bytes = postcard::to_stdvec(&map)?;
    ensure!(
        bytes.len() <= MAX_MAP_BYTES,
        "benchmark source map exceeds 4 MiB"
    );
    Ok(bytes)
}

fn validate_path(source: &Path) -> Result<()> {
    ensure!(
        source
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
        "benchmark source map requires logical project paths"
    );
    Ok(())
}

#[cfg(any(feature = "startup-metrics", test))]
fn apply(
    bytes: &[u8],
    fingerprint: u64,
    manifest: &mut crate::runtime::resources::LocalAssetManifest,
) -> Result<()> {
    ensure!(
        bytes.len() <= MAX_MAP_BYTES,
        "benchmark source map exceeds 4 MiB"
    );
    let (map, remaining): (SourceMap, _) = postcard::take_from_bytes(bytes)?;
    ensure!(
        remaining.is_empty() && map.version == 1,
        "invalid benchmark source map envelope"
    );
    ensure!(
        map.fingerprint == fingerprint,
        "benchmark source map Program fingerprint mismatch"
    );
    ensure!(
        map.scenes.len() == manifest.len(),
        "benchmark source map scene count mismatch"
    );
    let mut names = std::collections::HashSet::new();
    for scene in &map.scenes {
        validate_path(&scene.source)?;
        ensure!(
            names.insert(&scene.name),
            "duplicate benchmark source map scene"
        );
        let loaded = manifest
            .get(&scene.name)
            .context("benchmark source map scene missing")?;
        ensure!(
            loaded.action_spans.len() == scene.spans.len(),
            "benchmark source map action count mismatch"
        );
    }
    // Validate the whole table before changing any positions.
    for scene in map.scenes {
        let loaded = manifest.get_mut(&scene.name).expect("validated scene");
        loaded.source_path = scene.source;
        loaded.action_spans = scene.spans;
    }
    Ok(())
}

#[cfg(feature = "startup-metrics")]
pub(crate) fn restore(
    content: &keine_loader::ContentProject,
    fingerprint: u64,
    manifest: &mut crate::runtime::resources::LocalAssetManifest,
) -> Result<()> {
    use std::io::Read;
    let path = Path::new(MAP_PATH);
    let mount = content
        .asset_mounts()
        .into_iter()
        .rev()
        .find(|m| m.contains_file(path));
    let Some(mount) = mount else {
        if content.project_adapter() == Some("compiled") {
            // Compiled loader uses synthetic line 1: it is not source evidence.
            for scene in manifest.values_mut() {
                scene.source_path.clear();
                scene
                    .action_spans
                    .fill(keine_loader::SourceSpan { line: 0, column: 0 });
            }
        }
        return Ok(());
    };
    let file = mount.open_file(path)?;
    if file.len()? > MAX_MAP_BYTES as u64 {
        bail!("benchmark source map exceeds 4 MiB");
    }
    let mut bytes = Vec::new();
    file.take(MAX_MAP_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    apply(&bytes, fingerprint, manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::resources::{LocalAssetManifest, LocalSceneAssets};

    #[test]
    fn rejects_wrong_program_and_partial_maps_before_mutating_positions() {
        let mut manifest = LocalAssetManifest::default();
        manifest.insert(
            "start".into(),
            LocalSceneAssets {
                source_path: "synthetic".into(),
                action_spans: vec![keine_loader::SourceSpan { line: 1, column: 1 }],
                ..Default::default()
            },
        );
        let mut map = SourceMap {
            version: 1,
            fingerprint: 7,
            scenes: vec![SceneMap {
                name: "start".into(),
                source: "scripts/start.shou".into(),
                spans: vec![keine_loader::SourceSpan {
                    line: 12,
                    column: 3,
                }],
            }],
        };
        let bytes = postcard::to_stdvec(&map).unwrap();
        assert!(apply(&bytes, 8, &mut manifest).is_err());
        assert_eq!(manifest["start"].source_path, Path::new("synthetic"));
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(apply(&trailing, 7, &mut manifest).is_err());
        map.scenes[0].spans.clear();
        assert!(apply(&postcard::to_stdvec(&map).unwrap(), 7, &mut manifest).is_err());
        apply(&bytes, 7, &mut manifest).unwrap();
        assert_eq!(
            manifest["start"].source_path,
            Path::new("scripts/start.shou")
        );
        assert_eq!(manifest["start"].action_spans[0].line, 12);
    }
}
