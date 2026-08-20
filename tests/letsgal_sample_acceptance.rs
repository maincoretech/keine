//! Opt-in acceptance for the full official LetsGal Studio sample. The sample
//! contains commercial media and therefore remains a local, ignored project.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use keine_loader::{DiagnosticLevel, LoaderRegistry, load_scenes};

fn project_root() -> PathBuf {
    std::env::var_os("KEINE_LETSGAL_PROJECT")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("projects/letsgal"))
}

#[test]
fn official_sample_compiles_and_resolves_every_static_resource() {
    let root = project_root();
    if !root.join("project.json").is_file() {
        assert!(
            std::env::var_os("KEINE_LETSGAL_PROJECT").is_none(),
            "KEINE_LETSGAL_PROJECT does not contain project.json: {}",
            root.display()
        );
        eprintln!(
            "skipping local LetsGal sample acceptance; copy the official Studio template to {}",
            root.display()
        );
        return;
    }

    let project = LoaderRegistry::default()
        .open_project(&root)
        .expect("LetsGal project detection should not fail")
        .expect("the official sample should be recognized as LetsGal");
    assert_eq!(project.format, "letsgal");
    assert_eq!(project.config.title, "letsgal");

    let scenes = load_scenes(&project.content).expect("official sample scenes should compile");
    assert_eq!(scenes.len(), 9);
    assert_eq!(
        scenes
            .iter()
            .map(|scene| scene.actions.len())
            .sum::<usize>(),
        896
    );

    let diagnostics = scenes
        .iter()
        .flat_map(|scene| {
            scene
                .diagnostics
                .iter()
                .map(move |diagnostic| (scene, diagnostic))
        })
        .collect::<Vec<_>>();
    assert!(
        diagnostics
            .iter()
            .all(|(_, diagnostic)| diagnostic.level != DiagnosticLevel::Error),
        "official sample emitted error diagnostics: {diagnostics:?}"
    );

    let mut extensions = BTreeSet::new();
    let mut resources = 0usize;
    for scene in &scenes {
        for resource in &scene.resources {
            let path = resource.resolved_path(&project.config);
            if path.contains('{') {
                continue;
            }
            assert!(
                project.content.contains_asset(Path::new(&path)),
                "{} references missing asset {path:?}",
                scene.path.display()
            );
            if let Some(extension) = Path::new(&path)
                .extension()
                .and_then(|value| value.to_str())
            {
                extensions.insert(extension.to_ascii_lowercase());
            }
            resources += 1;
        }
    }
    assert!(
        resources >= 100,
        "unexpectedly narrow resource coverage: {resources}"
    );
    for expected in ["jpg", "png", "mp3", "wav", "mp4"] {
        assert!(
            extensions.contains(expected),
            "official sample no longer exercises {expected}: {extensions:?}"
        );
    }

    project
        .content
        .initial_state()
        .expect("LetsGal initial state should load");
    let reloaded = project
        .content
        .reload_config()
        .expect("LetsGal config reload should succeed")
        .expect("LetsGal adapter should provide a reloaded config");
    assert_eq!(reloaded.title, "letsgal");
}
