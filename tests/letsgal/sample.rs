//! Opt-in acceptance for the full official LetsGal Studio sample. The sample
//! contains commercial media and therefore remains a local, ignored project.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use keine_loader::{DiagnosticLevel, LoaderRegistry, load_scenes};

// Selected camera fields now form one atomic Action. Keep counting its original
// transform/effect/shake components so this acceptance still detects dropped sample content.
fn camera_components(action: &keine_core::Action) -> usize {
    match action {
        keine_core::Action::Flow { action, .. } => camera_components(action),
        keine_core::Action::SetCameraTween { spec } => {
            usize::from(spec.transform.is_some())
                + usize::from(spec.effect.is_some())
                + usize::from(spec.v2.is_some())
                + usize::from(spec.shake.is_some())
        }
        _ => 1,
    }
}

#[test]
fn official_sample_compiles_and_resolves_every_static_resource() {
    let Some(root) = std::env::var_os("KEINE_LETSGAL_PROJECT").map(PathBuf::from) else {
        eprintln!(
            "skipping official sample acceptance; set KEINE_LETSGAL_PROJECT to its external directory"
        );
        return;
    };
    assert!(
        root.join("project.json").is_file(),
        "KEINE_LETSGAL_PROJECT does not contain project.json: {}",
        root.display()
    );

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
            .map(|scene| scene.actions.iter().map(camera_components).sum::<usize>())
            .sum::<usize>(),
        1020
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
