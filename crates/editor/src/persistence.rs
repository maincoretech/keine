use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::component::dock::DockAreaState;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::project_key::ProjectKey;

const STATE_SCHEMA: u32 = 1;
const BLOCK_PICKER_SCHEMA: u32 = 1;
const MAX_RECENTS: usize = 12;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct AppPersistence {
    root: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct RecentProjectsFile {
    schema: u32,
    paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LayoutFile {
    schema: u32,
    project_path: PathBuf,
    dock: DockAreaState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct IdentityFile {
    schema: u32,
    project_path: PathBuf,
    workspace_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockPickerPreferences {
    pub favorites: Vec<String>,
    pub hidden: Vec<String>,
    pub category_order: Vec<String>,
    pub item_order: Vec<String>,
}

impl Default for BlockPickerPreferences {
    fn default() -> Self {
        Self {
            favorites: Vec::new(),
            hidden: Vec::new(),
            category_order: ["Text", "Scene", "Media", "Flow", "Data"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            item_order: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct BlockPickerPreferencesFile {
    schema: u32,
    preferences: BlockPickerPreferences,
}

impl AppPersistence {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn recent_projects(&self) -> Vec<PathBuf> {
        read_json::<RecentProjectsFile>(&self.root.join("recent-projects.json"))
            .filter(|state| state.schema == STATE_SCHEMA)
            .map(|state| state.paths)
            .unwrap_or_default()
    }

    pub fn record_recent(&self, project: &ProjectKey) -> io::Result<Vec<PathBuf>> {
        let path = project.path().to_owned();
        let mut paths = self.recent_projects();
        paths.retain(|candidate| {
            ProjectKey::from_path(candidate)
                .map(|key| key != *project)
                .unwrap_or(candidate != &path)
        });
        paths.insert(0, path);
        paths.truncate(MAX_RECENTS);
        atomic_json(
            &self.root.join("recent-projects.json"),
            &RecentProjectsFile {
                schema: STATE_SCHEMA,
                paths: paths.clone(),
            },
        )?;
        Ok(paths)
    }

    pub fn load_block_picker_preferences(&self) -> BlockPickerPreferences {
        read_json::<BlockPickerPreferencesFile>(&self.root.join("block-picker.json"))
            .filter(|state| state.schema == BLOCK_PICKER_SCHEMA)
            .map(|state| state.preferences)
            .unwrap_or_default()
    }

    pub fn save_block_picker_preferences(
        &self,
        preferences: &BlockPickerPreferences,
    ) -> io::Result<()> {
        atomic_json(
            &self.root.join("block-picker.json"),
            &BlockPickerPreferencesFile {
                schema: BLOCK_PICKER_SCHEMA,
                preferences: preferences.clone(),
            },
        )
    }

    pub fn prepare_workspace(&self, project: &ProjectKey) -> io::Result<PathBuf> {
        let directory = self.workspace_dir(project);
        fs::create_dir_all(directory.join("recovery"))?;
        atomic_json(
            &directory.join("identity.json"),
            &IdentityFile {
                schema: STATE_SCHEMA,
                project_path: project.path().to_owned(),
                workspace_id: project.workspace_id(),
            },
        )?;
        Ok(directory)
    }

    pub fn load_layout(&self, project: &ProjectKey) -> Option<DockAreaState> {
        read_json::<LayoutFile>(&self.workspace_dir(project).join("layout.json"))
            .filter(|state| state.schema == STATE_SCHEMA)
            .filter(|state| {
                ProjectKey::from_path(&state.project_path)
                    .map(|saved| saved == *project)
                    .unwrap_or(false)
            })
            .map(|state| state.dock)
    }

    pub fn save_layout(&self, project: &ProjectKey, dock: DockAreaState) -> io::Result<()> {
        let directory = self.prepare_workspace(project)?;
        atomic_json(
            &directory.join("layout.json"),
            &LayoutFile {
                schema: STATE_SCHEMA,
                project_path: project.path().to_owned(),
                dock,
            },
        )
    }

    pub fn clear_layout(&self, project: &ProjectKey) -> io::Result<()> {
        let path = self.workspace_dir(project).join("layout.json");
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub fn layout_path(&self, project: &ProjectKey) -> PathBuf {
        self.workspace_dir(project).join("layout.json")
    }

    pub fn recovery_dir(&self, project: &ProjectKey) -> PathBuf {
        self.workspace_dir(project).join("recovery")
    }

    fn workspace_dir(&self, project: &ProjectKey) -> PathBuf {
        self.root.join("projects").join(project.workspace_id())
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn atomic_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "state path has no parent"))?;
    fs::create_dir_all(parent)?;
    let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    let temporary = parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        serde_json::to_writer_pretty(&mut file, value).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        replace(&temporary, path)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(target_os = "windows"))]
fn replace(temporary: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(target_os = "windows")]
fn replace(temporary: &Path, destination: &Path) -> io::Result<()> {
    let previous = destination.with_extension("json.previous");
    let had_destination = destination.exists();
    if had_destination {
        let _ = fs::remove_file(&previous);
        fs::rename(destination, &previous)?;
    }
    match fs::rename(temporary, destination) {
        Ok(()) => {
            let _ = fs::remove_file(previous);
            Ok(())
        }
        Err(error) => {
            if had_destination {
                let _ = fs::rename(previous, destination);
            }
            Err(error)
        }
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::dock::{PanelInfo, PanelState};

    use super::*;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> (PathBuf, PathBuf, AppPersistence, ProjectKey) {
        let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "keine-editor-persistence-{}-{nonce}",
            std::process::id()
        ));
        let project = root.join("project");
        let app_data = root.join("app-data");
        fs::create_dir_all(&project).unwrap();
        let key = ProjectKey::from_path(&project).unwrap();
        (root, project, AppPersistence::new(app_data), key)
    }

    fn layout() -> DockAreaState {
        DockAreaState {
            version: Some(1),
            center: PanelState {
                panel_name: "TabPanel".into(),
                children: Vec::new(),
                info: PanelInfo::tabs(0),
            },
            ..Default::default()
        }
    }

    #[test]
    fn layout_is_atomic_and_never_written_in_the_project() {
        let (root, project, persistence, key) = fixture();
        persistence.save_layout(&key, layout()).unwrap();
        assert!(
            persistence
                .layout_path(&key)
                .starts_with(persistence.root())
        );
        assert!(!persistence.layout_path(&key).starts_with(&project));
        assert!(persistence.load_layout(&key).is_some());
        assert!(
            fs::read_dir(persistence.layout_path(&key).parent().unwrap())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".tmp-"))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_or_wrong_schema_layout_falls_back() {
        let (root, _, persistence, key) = fixture();
        persistence.prepare_workspace(&key).unwrap();
        fs::write(persistence.layout_path(&key), b"not json").unwrap();
        assert!(persistence.load_layout(&key).is_none());
        fs::write(
            persistence.layout_path(&key),
            br#"{"schema":99,"project_path":"/missing","dock":{}}"#,
        )
        .unwrap();
        assert!(persistence.load_layout(&key).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recent_projects_are_most_recent_first_and_deduplicated() {
        let (root, _, persistence, first) = fixture();
        let second_path = root.join("second");
        fs::create_dir_all(&second_path).unwrap();
        let second = ProjectKey::from_path(&second_path).unwrap();
        persistence.record_recent(&first).unwrap();
        persistence.record_recent(&second).unwrap();
        let recents = persistence.record_recent(&first).unwrap();
        assert_eq!(recents, vec![first.path(), second.path()]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clearing_layout_leaves_project_source_unchanged() {
        let (root, project, persistence, key) = fixture();
        let source = project.join("story.txt");
        fs::write(&source, "unchanged").unwrap();
        persistence.save_layout(&key, layout()).unwrap();
        persistence.clear_layout(&key).unwrap();
        assert_eq!(fs::read_to_string(source).unwrap(), "unchanged");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn block_picker_preferences_are_global_and_atomic() {
        let (root, project, persistence, _) = fixture();
        let preferences = BlockPickerPreferences {
            favorites: vec!["Dialogue".into()],
            hidden: vec!["Video".into()],
            category_order: vec!["Flow".into(), "Text".into()],
            item_order: vec!["Wait".into(), "Goto".into()],
        };
        persistence
            .save_block_picker_preferences(&preferences)
            .unwrap();
        assert_eq!(persistence.load_block_picker_preferences(), preferences);
        assert!(!persistence.root().starts_with(&project));
        assert!(fs::read_dir(persistence.root()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")
        }));
        fs::remove_dir_all(root).unwrap();
    }
}
