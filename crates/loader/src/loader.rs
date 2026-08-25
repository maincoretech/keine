mod compiled;
mod scenes;
mod source;
#[cfg(feature = "hot-reload")]
mod watcher;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use keine_core::config::{AssetSourceConfig, GameConfig};

use crate::{LoaderRegistry, StructuredSceneLoader};

pub(crate) use compiled::{COMPILED_PROGRAM_PATH, with_compiled_program};
pub use scenes::{LoadedScene, load_scenes, load_scenes_with, load_startup_scenes_with};
pub use source::{ContentBackend, ContentFile, ContentMount, HakutakuArchive};
#[cfg(feature = "hot-reload")]
pub use watcher::ScriptWatcher;

/// Mounted roots produced by one complete format adapter.
#[derive(Debug, Clone)]
pub struct SourceMount {
    pub adapter: String,
    pub origin: String,
    pub asset: Option<ContentMount>,
    pub scripts: Option<ContentMount>,
}

impl SourceMount {
    pub fn project(adapter: impl Into<String>, root: PathBuf) -> Self {
        let backend = ContentBackend::FileSystem(root.clone());
        Self {
            adapter: adapter.into(),
            origin: root.display().to_string(),
            asset: Some(ContentMount::new(backend.clone(), "assets").expect("static path")),
            scripts: Some(ContentMount::new(backend, "scripts").expect("static path")),
        }
    }

    pub fn assets(adapter: impl Into<String>, origin: impl Into<String>, root: PathBuf) -> Self {
        Self {
            adapter: adapter.into(),
            origin: origin.into(),
            asset: Some(
                ContentMount::new(ContentBackend::FileSystem(root), PathBuf::new())
                    .expect("empty path"),
            ),
            scripts: None,
        }
    }

    pub fn hakutaku_project(
        adapter: impl Into<String>,
        archive: HakutakuArchive,
        prefix: impl Into<PathBuf>,
    ) -> Result<Self> {
        let prefix = prefix.into();
        let backend = ContentBackend::Hakutaku(archive.clone());
        Ok(Self {
            adapter: adapter.into(),
            origin: archive.path().display().to_string(),
            asset: Some(ContentMount::new(backend.clone(), prefix.join("assets"))?),
            scripts: Some(ContentMount::new(backend, prefix.join("scripts"))?),
        })
    }

    pub fn hakutaku_assets(
        adapter: impl Into<String>,
        archive: HakutakuArchive,
        prefix: impl Into<PathBuf>,
    ) -> Result<Self> {
        let origin = archive.path().display().to_string();
        Ok(Self {
            adapter: adapter.into(),
            origin,
            asset: Some(ContentMount::new(
                ContentBackend::Hakutaku(archive),
                prefix,
            )?),
            scripts: None,
        })
    }
}

/// Ordered mounted view of a project. Consumers resolve from the end, so a
/// later source deterministically overrides an earlier source.
#[derive(Clone)]
pub struct ContentProject {
    pub root: PathBuf,
    pub sources: Vec<SourceMount>,
    scene_loader: Option<Arc<dyn StructuredSceneLoader>>,
}

impl fmt::Debug for ContentProject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContentProject")
            .field("root", &self.root)
            .field("sources", &self.sources)
            .field(
                "scene_loader",
                &self.scene_loader.as_ref().map(|loader| loader.name()),
            )
            .finish()
    }
}

impl ContentProject {
    pub(crate) fn with_structured_scenes(
        root: PathBuf,
        sources: Vec<SourceMount>,
        loader: Arc<dyn StructuredSceneLoader>,
    ) -> Self {
        Self {
            root,
            sources,
            scene_loader: Some(loader),
        }
    }

    pub(crate) fn scene_loader(&self) -> Option<&Arc<dyn StructuredSceneLoader>> {
        self.scene_loader.as_ref()
    }

    /// Editor-native project adapter currently providing structured scenes.
    /// `None` means scenes come from the configured script language adapter.
    pub fn project_adapter(&self) -> Option<&'static str> {
        self.scene_loader.as_ref().map(|loader| loader.name())
    }

    pub fn is_debug_cursor_change(&self, path: &Path) -> bool {
        self.scene_loader
            .as_ref()
            .is_some_and(|loader| loader.is_debug_cursor_change(path))
    }

    pub fn debug_cursor(&self) -> Result<Option<crate::ProjectDebugCursor>> {
        self.scene_loader
            .as_ref()
            .map_or(Ok(None), |loader| loader.debug_cursor(&self.root))
    }

    pub fn initial_state(&self) -> Result<crate::ProjectInitialState> {
        self.scene_loader
            .as_ref()
            .map_or(Ok(crate::ProjectInitialState::default()), |loader| {
                loader.initial_state(&self.root)
            })
    }

    pub fn reload_config(&self) -> Result<Option<GameConfig>> {
        let config = self
            .scene_loader
            .as_ref()
            .map_or(Ok(None), |loader| loader.load_config(&self.root))?;
        if let Some(config) = &config {
            config.validate()?;
        }
        Ok(config)
    }

    pub fn contains_asset(&self, path: &Path) -> bool {
        self.sources
            .iter()
            .rev()
            .filter_map(|source| source.asset.as_ref())
            .any(|mount| mount.contains_file(path))
    }

    pub fn asset_mounts(&self) -> Vec<ContentMount> {
        self.sources
            .iter()
            .filter_map(|source| source.asset.clone())
            .collect()
    }

    pub fn script_mounts(&self) -> Vec<ContentMount> {
        self.sources
            .iter()
            .filter_map(|source| source.scripts.clone())
            .collect()
    }

    pub fn watched_script_roots(&self) -> Vec<PathBuf> {
        self.script_mounts()
            .into_iter()
            .filter_map(|mount| mount.filesystem_root())
            .collect()
    }
}

pub fn load_project(root: &Path, sources: &[AssetSourceConfig]) -> Result<ContentProject> {
    load_project_with(root, sources, &LoaderRegistry::default())
}

pub fn load_project_with(
    root: &Path,
    sources: &[AssetSourceConfig],
    adapters: &LoaderRegistry,
) -> Result<ContentProject> {
    if sources.is_empty() {
        bail!("project must declare at least one adapter source");
    }
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve project root {}", root.display()))?;
    let mut mounted = Vec::with_capacity(sources.len());
    for source in sources {
        let mount = adapters
            .mount(&source.format, &root, &source.path)
            .with_context(|| format!("failed to mount adapter source {:?}", source.path))?;
        mounted.push(mount);
    }
    Ok(ContentProject {
        root,
        sources: mounted,
        scene_loader: None,
    })
}

/// Open a packaged project without extracting any file. Source paths from the
/// embedded config become logical prefixes inside the same archive, preserving
/// the same low-to-high override order used during development.
pub fn load_hakutaku_project(
    package: &Path,
    sources: &[AssetSourceConfig],
) -> Result<ContentProject> {
    load_hakutaku_project_from_archive(HakutakuArchive::open_packaged(package)?, sources)
}

pub fn load_hakutaku_project_from_archive(
    archive: HakutakuArchive,
    sources: &[AssetSourceConfig],
) -> Result<ContentProject> {
    if sources.is_empty() {
        bail!("project must declare at least one adapter source");
    }
    let mut mounted = Vec::with_capacity(sources.len());
    for source in sources {
        let path = PathBuf::from(&source.path);
        if !matches!(source.format.as_str(), "fs" | "auto") {
            bail!(
                "adapter {:?} cannot be resolved from inside a Hakutaku project",
                source.format
            );
        }
        let project_layout = archive.is_directory(&path.join("assets"))
            || archive.is_directory(&path.join("scripts"));
        mounted.push(if project_layout {
            SourceMount::hakutaku_project("hakutaku", archive.clone(), path)?
        } else {
            SourceMount::hakutaku_assets("hakutaku", archive.clone(), path)?
        });
    }
    Ok(ContentProject {
        root: archive.path().to_owned(),
        sources: mounted,
        scene_loader: None,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct InvalidReloadConfig;

    impl StructuredSceneLoader for InvalidReloadConfig {
        fn name(&self) -> &'static str {
            "invalid-reload"
        }

        fn load(&self, _project_root: &Path) -> Result<Vec<LoadedScene>> {
            Ok(Vec::new())
        }

        fn watch_roots(&self, _project_root: &Path) -> Vec<PathBuf> {
            Vec::new()
        }

        fn accepts_change(&self, _path: &Path) -> bool {
            false
        }

        fn load_config(&self, _project_root: &Path) -> Result<Option<GameConfig>> {
            let mut config = GameConfig::default();
            config.styles.textbox_alpha = f32::INFINITY;
            Ok(Some(config))
        }
    }

    #[test]
    fn mounts_ordered_filesystem_layers() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-content-{nonce}"));
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("packs/voices")).unwrap();
        let sources = vec![
            AssetSourceConfig::default(),
            AssetSourceConfig {
                path: "packs/voices".into(),
                format: "fs".into(),
            },
        ];

        let project = load_project(&root, &sources).unwrap();
        let root = root.canonicalize().unwrap();
        assert_eq!(
            project.asset_mounts()[0].filesystem_root().unwrap(),
            root.join("assets")
        );
        assert_eq!(
            project.asset_mounts()[1].filesystem_root().unwrap(),
            root.join("packs/voices")
        );
        assert_eq!(project.watched_script_roots(), vec![root.join("scripts")]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unknown_adapters() {
        let source = AssetSourceConfig {
            path: ".".into(),
            format: "missing".into(),
        };
        assert!(load_project(Path::new("."), &[source]).is_err());
    }

    #[test]
    fn rejects_invalid_adapter_config_before_hot_reload_can_apply_it() {
        let project = ContentProject::with_structured_scenes(
            PathBuf::from("unused"),
            Vec::new(),
            Arc::new(InvalidReloadConfig),
        );

        let error = project.reload_config().unwrap_err();
        assert!(error.to_string().contains("styles.textbox_alpha"));
    }

    #[test]
    fn rejects_filesystem_sources_outside_the_project_root() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-source-boundary-{nonce}"));
        let project = root.join("project");
        let outside = root.join("outside");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&outside).unwrap();

        let parent_escape = AssetSourceConfig {
            path: "../outside".into(),
            format: "fs".into(),
        };
        let absolute_escape = AssetSourceConfig {
            path: outside.to_string_lossy().into_owned(),
            format: "fs".into(),
        };
        assert!(load_project(&project, &[parent_escape]).is_err());
        assert!(load_project(&project, &[absolute_escape]).is_err());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn rejects_filesystem_source_symlinks_outside_the_project_root() {
        use std::os::unix::fs::symlink;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-source-symlink-{nonce}"));
        let project = root.join("project");
        let outside = root.join("outside");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, project.join("escape")).unwrap();

        let source = AssetSourceConfig {
            path: "escape".into(),
            format: "fs".into(),
        };
        assert!(load_project(&project, &[source]).is_err());

        let _ = fs::remove_dir_all(root);
    }
}
