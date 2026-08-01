//! Adapter categories consumed by the content loader and storage layer.

mod asset;
mod editor;
mod script;
mod store;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};

use keine_core::config::GameConfig;

use crate::loader::SourceMount;
use crate::{ContentProject, ScriptLanguageRegistry};

pub use asset::{FormatAdapter, mount_hexz};
pub use editor::{
    LetsGalProjectAdapter, ProjectDebugCursor, ProjectInitialState, StructuredSceneLoader,
};
pub use script::{WebGalLanguage, parse_webgal, parse_webgal_report};
pub use store::{KeineStore, SavedState, StoreAdapter, StoreMetadata, StoreStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AdapterCategory {
    Asset,
    Script,
    Project,
    Store,
}

impl AdapterCategory {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Script => "script",
            Self::Project => "project",
            Self::Store => "store",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterDescriptor {
    pub category: AdapterCategory,
    pub name: String,
}

impl AdapterDescriptor {
    pub fn id(&self) -> String {
        format!("{}:{}", self.category.id(), self.name.to_ascii_lowercase())
    }
}

/// A complete project opened by any package or editor adapter.
#[derive(Clone)]
pub struct AdaptedProject {
    pub format: &'static str,
    pub root: PathBuf,
    pub config: GameConfig,
    pub content: ContentProject,
}

/// Detects and opens a complete source project.
///
/// Implementations are read-only format translators. They may inspect source
/// files and return normalized mounts/configuration, but must not start
/// watchers, launch processes, or interact with a renderer.
pub trait ProjectAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn detect(&self, project_root: &Path) -> Result<bool>;
    fn open(&self, project_root: &Path) -> Result<AdaptedProject>;
}

/// Registry consumed by project loading, scene parsing and hot reload.
pub struct LoaderRegistry {
    assets: HashMap<String, Arc<dyn FormatAdapter>>,
    languages: ScriptLanguageRegistry,
    projects: Vec<Arc<dyn ProjectAdapter>>,
    stores: HashMap<String, Arc<dyn StoreAdapter>>,
}

impl Default for LoaderRegistry {
    fn default() -> Self {
        let mut registry = Self::empty();
        registry.register_asset(asset::FsFormat);
        registry.register_asset(asset::HexzFormat);
        registry.register_asset(asset::AutoFormat);
        for language in script::builtin() {
            registry.languages.register_shared(language);
        }
        registry.register_project(editor::LetsGalProjectAdapter);
        registry.register_project(asset::HexzProjectAdapter);
        registry.register_store(store::KeineStore);
        registry
    }
}

impl LoaderRegistry {
    /// Creates a registry with no format or host knowledge.
    ///
    /// Embedders can register only the adapters they ship, which keeps the
    /// engine runtime independent from keine's curated default adapter set.
    pub fn empty() -> Self {
        Self {
            assets: HashMap::new(),
            languages: ScriptLanguageRegistry::new(),
            projects: Vec::new(),
            stores: HashMap::new(),
        }
    }

    pub fn register_asset(&mut self, format: impl FormatAdapter + 'static) {
        self.assets
            .insert(format.name().to_owned(), Arc::new(format));
    }

    pub fn register_script(&mut self, language: impl crate::ScriptLanguage + 'static) {
        self.languages.register(language);
    }

    pub fn register_project(&mut self, adapter: impl ProjectAdapter + 'static) {
        self.projects.push(Arc::new(adapter));
    }

    /// Opens a native editor project when one of the registered project
    /// adapters recognizes it. A plain keine project deliberately returns
    /// `None` and continues through `config.yaml` loading in the runtime.
    pub fn open_project(&self, root: &Path) -> Result<Option<AdaptedProject>> {
        for adapter in &self.projects {
            if adapter.detect(root)? {
                return adapter
                    .open(root)
                    .with_context(|| format!("failed to open {} project", adapter.name()))
                    .map(Some);
            }
        }
        Ok(None)
    }

    pub fn languages(&self, name: &str) -> Result<ScriptLanguageRegistry> {
        self.languages
            .select(name)
            .with_context(|| format!("unknown script adapter {name:?}"))
    }

    pub fn register_store(&mut self, store: impl StoreAdapter + 'static) {
        self.stores.insert(store.name().to_owned(), Arc::new(store));
    }

    /// Returns the concrete capabilities currently installed in this registry.
    pub fn adapters(&self) -> Vec<AdapterDescriptor> {
        let mut adapters = Vec::with_capacity(
            self.assets.len()
                + self.languages.names().count()
                + self.projects.len()
                + self.stores.len(),
        );
        adapters.extend(self.assets.keys().map(|name| AdapterDescriptor {
            category: AdapterCategory::Asset,
            name: name.clone(),
        }));
        adapters.extend(self.languages.names().map(|name| AdapterDescriptor {
            category: AdapterCategory::Script,
            name: name.to_ascii_lowercase(),
        }));
        adapters.extend(self.projects.iter().map(|adapter| AdapterDescriptor {
            category: AdapterCategory::Project,
            name: adapter.name().to_owned(),
        }));
        adapters.extend(self.stores.keys().map(|name| AdapterDescriptor {
            category: AdapterCategory::Store,
            name: name.clone(),
        }));
        adapters.sort_by(|left, right| {
            (left.category, left.name.as_str()).cmp(&(right.category, right.name.as_str()))
        });
        adapters
    }

    /// Removes concrete adapters which are disabled by the host application.
    pub fn retain_adapters(&mut self, mut keep: impl FnMut(AdapterCategory, &str) -> bool) {
        self.assets
            .retain(|name, _| keep(AdapterCategory::Asset, name));
        self.languages
            .retain(|name| keep(AdapterCategory::Script, name));
        self.projects
            .retain(|adapter| keep(AdapterCategory::Project, adapter.name()));
        self.stores
            .retain(|name, _| keep(AdapterCategory::Store, name));
    }

    pub fn store(&self, name: &str) -> Result<Arc<dyn StoreAdapter>> {
        self.stores
            .get(name)
            .cloned()
            .with_context(|| format!("unknown store adapter {name:?}"))
    }

    pub(crate) fn mount(
        &self,
        adapter: &str,
        project_root: &Path,
        location: &str,
    ) -> Result<SourceMount> {
        self.assets
            .get(adapter)
            .with_context(|| format!("unknown adapter {adapter:?}"))?
            .mount(project_root, location)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseReport, ScriptLanguage, SourceMount};

    struct CustomFormat;

    impl FormatAdapter for CustomFormat {
        fn name(&self) -> &'static str {
            "custom"
        }

        fn mount(&self, project_root: &Path, location: &str) -> Result<SourceMount> {
            Ok(SourceMount::assets(
                self.name(),
                location,
                project_root.to_owned(),
            ))
        }
    }

    struct CustomScript;

    impl ScriptLanguage for CustomScript {
        fn name(&self) -> &'static str {
            "custom-script"
        }

        fn extensions(&self) -> &'static [&'static str] {
            &["custom"]
        }

        fn parse(&self, _source: &str) -> ParseReport {
            ParseReport::default()
        }
    }

    #[test]
    fn registers_custom_asset_and_script_options() {
        let mut registry = LoaderRegistry::default();
        registry.register_asset(CustomFormat);
        registry.register_script(CustomScript);
        assert!(registry.languages("custom-script").is_ok());
        assert!(registry.mount("custom", Path::new("."), "virtual").is_ok());
    }

    #[test]
    fn empty_registry_has_no_concrete_adapter_or_host_dependency() {
        let registry = LoaderRegistry::empty();
        assert!(registry.assets.is_empty());
        assert!(registry.projects.is_empty());
        assert!(registry.stores.is_empty());
        assert!(registry.languages("webgal").is_err());
    }

    #[test]
    fn exposes_builtin_options_by_category() {
        let registry = LoaderRegistry::default();
        assert!(
            registry
                .languages("webgal")
                .unwrap()
                .supports(Path::new("scene.txt"))
        );
        for name in ["fs", "hexz", "auto"] {
            assert!(registry.assets.contains_key(name));
        }
        assert_eq!(registry.assets.len(), 3);
        assert_eq!(registry.projects.len(), 2);
        assert!(registry.store("keine").is_ok());
    }

    #[test]
    fn registry_can_disable_one_category_without_affecting_the_others() {
        let mut registry = LoaderRegistry::default();
        registry.retain_adapters(|category, name| {
            category != AdapterCategory::Project || name != "letsgal"
        });

        assert!(
            !registry
                .adapters()
                .iter()
                .any(|adapter| adapter.id() == "project:letsgal")
        );
        let adapters = registry.adapters();
        assert!(registry.languages("webgal").is_ok());
        assert!(adapters.iter().any(|adapter| adapter.id() == "asset:fs"));
        assert!(registry.store("keine").is_ok());
    }
}
