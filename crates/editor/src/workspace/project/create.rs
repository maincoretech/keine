//! Minimal native project creation; existing destinations are never overwritten.
use std::{
    fs, io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) fn create(root: &Path) -> io::Result<()> {
    let title = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| io::Error::other("Enter a project name"))?;
    let title = serde_json::to_string(title).map_err(io::Error::other)?;
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    fs::create_dir(root)?;
    let result = (|| {
        fs::create_dir(root.join("scripts"))?;
        for kind in ["background", "figure", "bgm", "vocal", "effect"] {
            fs::create_dir_all(root.join("assets").join(kind))?;
        }
        fs::write(
            root.join("config.yaml"),
            format!(
                "title: {title}\nproject:\n  id: game-{id:x}\nadapter:\n  script: keine\nscript:\n  version: 2\n  entry: start\n"
            ),
        )?;
        fs::write(root.join("assets.yaml"), "{}\n")?;
        fs::write(root.join("characters.yaml"), "{}\n")?;
        fs::write(
            root.join("scripts/main.shou"),
            "scene start {\n  \"Hello!\"\n}\n",
        )
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(root);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creates_native_project_without_overwriting_existing_files() {
        let temp = std::env::temp_dir().join(format!(
            "keine-new-project-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&temp).unwrap();
        let root = temp.join("新游戏");
        create(&root).unwrap();
        let source = fs::read_to_string(root.join("config.yaml")).unwrap();
        let config = keine_core::config::GameConfig::from_yaml(&source).unwrap();
        assert_eq!(config.title, "新游戏");
        assert_eq!(config.script.entry, "start");
        assert!(!config.project.id.is_empty());
        crate::workspace::WorkspaceSession::open(&root).unwrap();
        assert!(create(&root).is_err());
        assert_eq!(
            fs::read_to_string(root.join("config.yaml")).unwrap(),
            source
        );
        fs::remove_dir_all(temp).unwrap();
    }
}
