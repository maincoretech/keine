use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use keine_core::Action;

use crate::{
    ContentMount, ContentProject, Diagnostic, DiagnosticLevel, ResourceRef, SceneRef,
    ScriptLanguageRegistry, source_input::SourceReader,
};

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedScene {
    pub name: String,
    pub path: PathBuf,
    pub actions: Vec<Action>,
    /// Source position for every action, kept parallel to `actions` for
    /// editor-driven seek and diagnostics.
    pub action_spans: Vec<crate::SourceSpan>,
    pub diagnostics: Vec<Diagnostic>,
    pub resources: Vec<ResourceRef>,
    pub sub_scenes: Vec<SceneRef>,
}

/// Loads every script layer in stable order. A scene in a later content source
/// replaces one with the same name from an earlier source.
pub fn load_scenes(project: &ContentProject) -> Result<Vec<LoadedScene>> {
    load_scenes_with(project, &ScriptLanguageRegistry::default())
}

pub fn load_scenes_with(
    project: &ContentProject,
    languages: &ScriptLanguageRegistry,
) -> Result<Vec<LoadedScene>> {
    if let Some(loader) = project.scene_loader() {
        return loader.load(&project.root);
    }
    let mut merged = BTreeMap::new();
    let source_reader = SourceReader::for_mounts();
    for script_mount in project.script_mounts() {
        for mut scene in load_directory(&script_mount, languages, &source_reader)? {
            if let Some(previous) = merged.insert(scene.name.clone(), scene.clone()) {
                scene.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Warning,
                    span: crate::SourceSpan { line: 1, column: 1 },
                    message: format!(
                        "scene {:?} overrides {}",
                        scene.name,
                        previous.path.display()
                    ),
                });
                merged.insert(scene.name.clone(), scene);
            }
        }
    }
    let mut scenes = merged.into_values().collect::<Vec<_>>();
    validate_scene_references(&mut scenes);
    Ok(scenes)
}

fn load_directory(
    scripts: &ContentMount,
    languages: &ScriptLanguageRegistry,
    source_reader: &SourceReader,
) -> Result<Vec<LoadedScene>> {
    let paths = script_paths(scripts, languages)?;

    let mut scenes = Vec::with_capacity(paths.len());
    for path in paths {
        scenes.push(load_scene(scripts, path, languages, source_reader)?);
    }
    let mut names = HashSet::with_capacity(scenes.len());
    for scene in &scenes {
        if !names.insert(scene.name.clone()) {
            anyhow::bail!(
                "duplicate scene name {:?} in {}",
                scene.name,
                scripts.prefix().display()
            );
        }
    }
    Ok(scenes)
}

fn script_paths(
    scripts: &ContentMount,
    languages: &ScriptLanguageRegistry,
) -> Result<Vec<PathBuf>> {
    let mut paths = scripts.recursive_files()?;
    paths.retain(|path| languages.supports(path));
    paths.sort();
    Ok(paths)
}

fn validate_scene_references(scenes: &mut [LoadedScene]) {
    let names = scenes
        .iter()
        .map(|scene| scene.name.clone())
        .collect::<HashSet<_>>();
    for scene in scenes {
        for reference in &scene.sub_scenes {
            if !reference.scene.contains('{') && !names.contains(&reference.scene) {
                scene.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Error,
                    span: reference.span,
                    message: format!("referenced scene {:?} does not exist", reference.scene),
                });
            }
        }
    }
}

fn load_scene(
    scripts: &ContentMount,
    path: PathBuf,
    languages: &ScriptLanguageRegistry,
    source_reader: &SourceReader,
) -> Result<LoadedScene> {
    let language = languages
        .language_for(&path)
        .with_context(|| format!("unsupported script format: {}", path.display()))?;
    let bytes = source_reader
        .read_mount(scripts, &path)
        .with_context(|| format!("failed to read script {}", path.display()))?;
    let source = String::from_utf8(bytes)
        .with_context(|| format!("script is not UTF-8: {}", path.display()))?;
    let mut name_path = path.clone();
    name_path.set_extension("");
    let name = name_path
        .to_str()
        .with_context(|| format!("script has no valid UTF-8 path: {}", path.display()))?
        .replace('\\', "/");

    let report = language.parse(&source);
    Ok(LoadedScene {
        name,
        path,
        actions: report.actions,
        action_spans: report.spans,
        diagnostics: report.diagnostics,
        resources: report.resources,
        sub_scenes: report.sub_scenes,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use keine_core::{Program, State, StepResult, step};

    fn project(root: &Path) -> ContentProject {
        ContentProject {
            root: root.to_owned(),
            sources: vec![crate::SourceMount::project("project", root.to_owned())],
            scene_loader: None,
        }
    }

    #[test]
    fn detects_supported_languages() {
        let languages = ScriptLanguageRegistry::default();
        assert!(languages.supports(Path::new("scene.txt")));
        assert!(!languages.supports(Path::new("scene.md")));
    }

    #[test]
    fn loads_scenes_in_filename_order() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-scenes-{nonce}"));
        let root = project_root.join("scripts");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("b.txt"), "B:second;").unwrap();
        fs::write(root.join("a.txt"), "A:first;").unwrap();
        fs::write(root.join("ignored.md"), "not a scene").unwrap();

        let scenes = load_scenes(&project(&project_root)).unwrap();

        assert_eq!(
            scenes
                .iter()
                .map(|scene| scene.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        let _ = fs::remove_dir_all(project_root);
    }

    #[test]
    fn loaded_scenes_execute_call_and_return_end_to_end() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-scene-flow-{nonce}"));
        let root = project_root.join("scripts");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("main.txt"),
            "callScene:aside.txt;\nMain:returned;",
        )
        .unwrap();
        fs::write(root.join("aside.txt"), "Aside: inside;").unwrap();

        let mut state = State::new();
        state.install_program(Program::from_scenes(
            load_scenes(&project(&project_root))
                .unwrap()
                .into_iter()
                .map(|scene| (scene.name, scene.actions)),
        ));
        state.current_scene = "main".into();
        step::index_labels(&mut state);

        assert_eq!(step::step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, "aside");
        assert_eq!(state.dialogue.as_ref().unwrap().text, "inside");
        step::advance(&mut state);
        assert_eq!(step::step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, "main");
        assert_eq!(state.dialogue.as_ref().unwrap().text, "returned");

        let _ = fs::remove_dir_all(project_root);
    }

    #[test]
    fn recursively_loads_and_executes_deeply_nested_scene_paths() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-nested-scenes-{nonce}"));
        let root = project_root.join("scripts");
        let nested_scene = "chapter_01/act_02/branch_03/part_04";
        fs::create_dir_all(root.join("chapter_01/act_02/branch_03")).unwrap();
        fs::write(
            root.join("main.txt"),
            format!("callScene:{nested_scene}.txt;\nMain:returned;"),
        )
        .unwrap();
        fs::write(
            root.join(format!("{nested_scene}.txt")),
            "Guide:inside nested scene;",
        )
        .unwrap();

        let scenes = load_scenes(&project(&project_root)).unwrap();
        assert_eq!(
            scenes
                .iter()
                .map(|scene| scene.name.as_str())
                .collect::<Vec<_>>(),
            [nested_scene, "main"]
        );
        assert!(scenes.iter().all(|scene| scene.diagnostics.is_empty()));

        let mut state = State::new();
        state.install_program(Program::from_scenes(
            scenes.into_iter().map(|scene| (scene.name, scene.actions)),
        ));
        state.current_scene = "main".into();

        assert_eq!(step::step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, nested_scene);
        assert_eq!(state.dialogue.as_ref().unwrap().text, "inside nested scene");
        step::advance(&mut state);
        assert_eq!(step::step(&mut state), StepResult::AwaitClick);
        assert_eq!(state.current_scene, "main");
        assert_eq!(state.dialogue.as_ref().unwrap().text, "returned");

        let _ = fs::remove_dir_all(project_root);
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_scan_ignores_symlink_cycles_and_mount_escapes() {
        use std::os::unix::fs::symlink;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-scene-links-{nonce}"));
        let outside_root = std::env::temp_dir().join(format!("keine-scene-links-outside-{nonce}"));
        let scripts = project_root.join("scripts");
        let nested = scripts.join("chapter");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&outside_root).unwrap();
        fs::write(nested.join("inside.txt"), "Guide:inside mount;").unwrap();
        fs::write(outside_root.join("outside.txt"), "Intruder:outside mount;").unwrap();

        symlink(&scripts, nested.join("cycle")).unwrap();
        symlink(&outside_root, scripts.join("escaped-directory")).unwrap();
        symlink(
            outside_root.join("outside.txt"),
            scripts.join("escaped-file.txt"),
        )
        .unwrap();

        let scenes = load_scenes(&project(&project_root)).unwrap();

        assert_eq!(
            scenes
                .iter()
                .map(|scene| scene.name.as_str())
                .collect::<Vec<_>>(),
            ["chapter/inside"]
        );

        let _ = fs::remove_dir_all(project_root);
        let _ = fs::remove_dir_all(outside_root);
    }

    #[test]
    fn same_stem_in_different_directories_has_distinct_scene_names() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project_root = std::env::temp_dir().join(format!("keine-scene-stems-{nonce}"));
        let root = project_root.join("scripts");
        fs::create_dir_all(root.join("chapter_a")).unwrap();
        fs::create_dir_all(root.join("chapter_b")).unwrap();
        fs::write(root.join("chapter_a/part.txt"), "A:first;").unwrap();
        fs::write(root.join("chapter_b/part.txt"), "B:second;").unwrap();

        let scenes = load_scenes(&project(&project_root)).unwrap();
        assert_eq!(
            scenes
                .iter()
                .map(|scene| scene.name.as_str())
                .collect::<Vec<_>>(),
            ["chapter_a/part", "chapter_b/part"]
        );

        let _ = fs::remove_dir_all(project_root);
    }

    #[test]
    fn later_script_sources_override_earlier_scenes() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-scene-layers-{nonce}"));
        let base_project = root.join("base-project");
        let patch_project = root.join("patch-project");
        let base = base_project.join("scripts");
        let patch = patch_project.join("scripts");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&patch).unwrap();
        fs::write(base.join("main.txt"), "Base:old;").unwrap();
        fs::write(patch.join("main.txt"), "Patch:new;").unwrap();
        let project = ContentProject {
            root: root.clone(),
            sources: vec![
                crate::SourceMount::project("project", base_project),
                crate::SourceMount::project("project", patch_project),
            ],
            scene_loader: None,
        };

        let scenes = load_scenes(&project).unwrap();
        assert_eq!(scenes.len(), 1);
        assert!(
            scenes[0]
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("overrides"))
        );
        let keine_core::Action::Say { text, .. } = &scenes[0].actions[0] else {
            panic!("expected dialogue");
        };
        assert_eq!(text, "new");
        let _ = fs::remove_dir_all(root);
    }
}
