mod compiled;
mod scenes;
mod source;
#[cfg(feature = "hot-reload")]
mod watcher;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use keine_core::Rgba;
use keine_core::config::{
    AssetSourceConfig, EiyashouAssetManifest, EiyashouCharacterManifest, GameConfig,
};

use crate::{LoaderRegistry, ResourceKind, StructuredSceneLoader};

pub(crate) use compiled::{COMPILED_PROGRAM_PATH, with_compiled_program};
pub use scenes::{
    LoadedScene, load_scenes, load_scenes_with, load_startup_scenes_with,
    validate_native_entry_flow,
};
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
    pub(crate) eiyashou: Option<EiyashouProjectData>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EiyashouProjectData {
    pub(crate) characters: HashMap<String, EiyashouCharacterData>,
    pub(crate) assets: HashMap<ResourceKind, HashSet<String>>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct EiyashouCharacterData {
    pub(crate) name: String,
    pub(crate) color: Option<Rgba>,
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
            eiyashou: None,
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

    /// Load Eiyashou's author-facing manifests and normalize their project-root
    /// paths into the engine's existing asset-root aliases.
    pub fn prepare_eiyashou(&mut self, config: &mut GameConfig) -> Result<()> {
        if !config.adapter.script.eq_ignore_ascii_case("keine") {
            self.eiyashou = None;
            return Ok(());
        }
        let assets_path = confined_manifest_path(&self.root, &config.script.assets)?;
        let characters_path = confined_manifest_path(&self.root, &config.script.characters)?;
        let assets_yaml = fs::read_to_string(&assets_path)
            .with_context(|| format!("failed to read {}", assets_path.display()))?;
        let characters_yaml = fs::read_to_string(&characters_path)
            .with_context(|| format!("failed to read {}", characters_path.display()))?;
        let assets = EiyashouAssetManifest::from_yaml(&assets_yaml)
            .with_context(|| format!("invalid Eiyashou manifest {}", assets_path.display()))?;
        let characters = EiyashouCharacterManifest::from_yaml(&characters_yaml)
            .with_context(|| format!("invalid Eiyashou manifest {}", characters_path.display()))?;
        let mut data = EiyashouProjectData::default();
        let asset_roots = self
            .asset_mounts()
            .into_iter()
            .filter_map(|mount| mount.filesystem_root())
            .collect::<Vec<_>>();
        data.warnings.extend(
            assets
                .unknown_namespaces()
                .map(|name| format!("unknown assets.yaml namespace `{name}` is ignored")),
        );
        data.warnings.extend(
            characters
                .unknown_fields()
                .map(|name| format!("unknown characters.yaml field `{name}` is ignored")),
        );
        for (id, character) in characters.characters {
            if character.name.is_empty() {
                bail!("character `{id}` must have a non-empty name");
            }
            if let Some(color) = &character.color
                && !valid_character_color(color)
            {
                bail!("character `{id}` color must use #RRGGBB");
            }
            data.warnings.extend(
                character
                    .unknown_fields()
                    .map(|field| format!("unknown field `{field}` on character `{id}` is ignored")),
            );
            data.characters.insert(
                id,
                EiyashouCharacterData {
                    name: character.name,
                    color: character.color.as_deref().map(parse_character_color),
                },
            );
        }
        install_asset_namespace(
            &self.root,
            &asset_roots,
            ResourceKind::Background,
            assets.backgrounds,
            &mut config.assets.backgrounds,
            &mut data,
        )?;
        install_asset_namespace(
            &self.root,
            &asset_roots,
            ResourceKind::Figure,
            assets.figures,
            &mut config.assets.figures,
            &mut data,
        )?;
        install_asset_namespace(
            &self.root,
            &asset_roots,
            ResourceKind::Voice,
            assets.voices,
            &mut config.assets.voices,
            &mut data,
        )?;
        install_asset_namespace(
            &self.root,
            &asset_roots,
            ResourceKind::Bgm,
            assets.bgm,
            &mut config.assets.bgm,
            &mut data,
        )?;
        install_asset_namespace(
            &self.root,
            &asset_roots,
            ResourceKind::Effect,
            assets.effects,
            &mut config.assets.effects,
            &mut data,
        )?;
        install_asset_namespace(
            &self.root,
            &asset_roots,
            ResourceKind::Video,
            assets.videos,
            &mut config.assets.videos,
            &mut data,
        )?;
        self.eiyashou = Some(data);
        Ok(())
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
        eiyashou: None,
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
        eiyashou: None,
    })
}

fn confined_manifest_path(root: &Path, configured: &str) -> Result<PathBuf> {
    let relative = Path::new(configured);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!("manifest path must stay inside the project: {configured}");
    }
    let path = root.join(relative);
    let resolved = path
        .canonicalize()
        .with_context(|| format!("failed to resolve manifest {}", path.display()))?;
    if !resolved.starts_with(root) {
        bail!("manifest path escapes the project root: {configured}");
    }
    Ok(resolved)
}

fn install_asset_namespace(
    root: &Path,
    asset_roots: &[PathBuf],
    kind: ResourceKind,
    values: HashMap<String, String>,
    target: &mut HashMap<String, String>,
    data: &mut EiyashouProjectData,
) -> Result<()> {
    let ids = data.assets.entry(kind).or_default();
    for (id, configured) in values {
        if id.is_empty() {
            bail!("asset identifiers must not be empty");
        }
        let relative = Path::new(&configured);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            bail!("asset `{id}` must stay inside the project: {configured}");
        }
        let resolved = root
            .join(relative)
            .canonicalize()
            .with_context(|| format!("failed to resolve asset `{id}` at {configured}"))?;
        if !resolved.starts_with(root) || !resolved.is_file() {
            bail!("asset `{id}` must resolve to a project file: {configured}");
        }
        let logical = asset_roots
            .iter()
            .rev()
            .find_map(|asset_root| resolved.strip_prefix(asset_root).ok())
            .filter(|path| !path.as_os_str().is_empty())
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "asset `{id}` at {configured} is not reachable through adapter.asset sources"
                )
            })?;
        ids.insert(id.clone());
        target.insert(id, logical);
    }
    Ok(())
}

fn valid_character_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color.as_bytes()[1..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
}

fn parse_character_color(color: &str) -> Rgba {
    let channel = |range| u8::from_str_radix(&color[range], 16).unwrap() as f32 / 255.0;
    Rgba::new(channel(1..3), channel(3..5), channel(5..7), 1.0)
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

    #[test]
    fn prepares_eiyashou_manifests_without_exposing_yaml_to_runtime() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("keine-eiyashou-manifests-{nonce}"));
        fs::create_dir_all(root.join("assets/background")).unwrap();
        fs::create_dir_all(root.join("packs/voices")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join("assets/background/day.webp"), b"image").unwrap();
        fs::write(root.join("packs/voices/hello.opus"), b"voice").unwrap();
        fs::write(
            root.join("assets.yaml"),
            "backgrounds:\n  day: assets/background/day.webp\nvoices:\n  hello: packs/voices/hello.opus\n",
        )
        .unwrap();
        fs::write(
            root.join("characters.yaml"),
            "characters:\n  rin:\n    name: Rin\n    color: '#BAEBFF'\n",
        )
        .unwrap();
        fs::write(
            root.join("scripts/main.shou"),
            "scene start { rin: \"Hi\", background(day) }",
        )
        .unwrap();

        let mut config = GameConfig::default();
        config.adapter.script = "keine".into();
        config.adapter.asset.push(AssetSourceConfig {
            path: "packs/voices".into(),
            format: "fs".into(),
        });
        let mut project = load_project(&root, &config.adapter.asset).unwrap();
        project.prepare_eiyashou(&mut config).unwrap();

        assert_eq!(config.assets.backgrounds["day"], "background/day.webp");
        assert_eq!(config.assets.voices["hello"], "hello.opus");
        let prepared = project.eiyashou.as_ref().unwrap();
        assert_eq!(prepared.characters["rin"].name, "Rin");
        assert_eq!(
            prepared.characters["rin"].color,
            Some(Rgba::new(186.0 / 255.0, 235.0 / 255.0, 1.0, 1.0))
        );
        assert!(prepared.assets[&ResourceKind::Background].contains("day"));
        let scenes = load_scenes(&project).unwrap();
        assert!(
            scenes[0]
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.level != crate::DiagnosticLevel::Error)
        );
        assert!(matches!(
            &scenes[0].actions[0],
            keine_core::Action::EiyashouSay(dialogue)
                if dialogue.speaker == "Rin"
                    && dialogue.speaker_color == Some(Rgba::new(186.0 / 255.0, 235.0 / 255.0, 1.0, 1.0))
        ));
        let _ = fs::remove_dir_all(root);
    }
}
