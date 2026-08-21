use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "hot-reload")]
use std::sync::Mutex;

use bevy::prelude::*;
use keine_core::State;
use keine_core::config::GameConfig;
#[cfg(feature = "hot-reload")]
use keine_loader::ScriptWatcher;
use keine_loader::{ContentProject, ResourceRef, SceneRef, ScriptLanguageRegistry, StoreAdapter};
use std::collections::{HashMap, HashSet};

#[derive(Resource, Deref, DerefMut)]
pub struct GameState(pub State);

/// Caches the Unicode scalar count for one dialogue payload.
///
/// Dialogue text is normally stable for hundreds of rendered frames. Keeping
/// the owned text here makes cache invalidation content-based, so save restore,
/// hot reload, and same-cursor editor replacement cannot reuse a stale count.
#[derive(Default)]
pub(crate) struct DialogueLengthCache {
    text: String,
    characters: usize,
}

impl DialogueLengthCache {
    pub(crate) fn count(&mut self, text: &str) -> usize {
        if self.text != text {
            self.text.clear();
            self.text.push_str(text);
            self.characters = text.chars().count();
        }
        self.characters
    }
}

#[derive(Resource, Deref)]
pub struct GameConfigResource(pub GameConfig);

#[derive(Resource, Deref)]
pub struct ProjectRoot(pub PathBuf);

#[derive(Resource, Deref)]
pub struct ContentProjectResource(pub ContentProject);

#[derive(Resource, Deref)]
pub struct ScriptLanguages(pub ScriptLanguageRegistry);

#[derive(Resource, Clone)]
pub struct StoreCodec(pub Arc<dyn StoreAdapter>);

#[cfg(feature = "hot-reload")]
#[derive(Resource)]
pub struct ScriptWatcherResource(pub Mutex<ScriptWatcher>);

/// Marks a read-only native-editor preview that follows the adapter's
/// persisted selected-block position instead of entering keine's title screen.
#[derive(Resource, Default)]
pub struct EditorSyncSession;

/// Enables source and asset watching for interactive development sessions.
/// Shipping runtimes and deterministic benchmarks deliberately omit it.
#[cfg(feature = "hot-reload")]
#[derive(Resource, Default)]
pub struct HotReloadSession;

/// Enables diagnostics intended for the `keine dev` and Studio sync UIs.
#[derive(Resource, Default)]
pub struct DevelopmentSession;

/// Disables all project-owned persistence while keeping normal runtime input
/// and scene semantics. Used by deterministic performance captures.
#[derive(Resource, Default)]
pub struct PersistenceDisabled;

#[derive(Resource, Default, Deref, DerefMut)]
pub struct LocalAssetManifest(pub HashMap<String, LocalSceneAssets>);

#[derive(Default)]
pub struct LocalSceneAssets {
    pub resources: Vec<ResourceRef>,
    pub sub_scenes: Vec<SceneRef>,
    pub action_spans: Vec<keine_loader::SourceSpan>,
}

/// Strong handles retained around the current script position.
///
/// `critical` is deliberately separate from the retained handle set. Assets
/// predicted by the lookahead should be loaded and kept warm, but only assets
/// required by the currently presented state are allowed to pause execution.
#[derive(Resource, Default)]
pub struct LocalAssetCache {
    pub(crate) handles: HashMap<String, UntypedHandle>,
    pub(crate) critical: HashSet<String>,
}

impl LocalAssetCache {
    pub(crate) fn blocking_handles(&self) -> impl Iterator<Item = &UntypedHandle> {
        self.critical
            .iter()
            .filter_map(|path| self.handles.get(path))
    }
}

#[derive(Resource)]
pub struct AssetLoadingGate {
    pub blocked: bool,
}

impl Default for AssetLoadingGate {
    fn default() -> Self {
        Self { blocked: true }
    }
}

#[cfg(test)]
mod tests {
    use super::DialogueLengthCache;

    #[test]
    fn dialogue_length_cache_tracks_content_changes() {
        let mut cache = DialogueLengthCache::default();

        assert_eq!(cache.count("我a🙂"), 3);
        assert_eq!(cache.count("我a🙂"), 3);
        assert_eq!(cache.count("same bytes"), 10);
        assert_eq!(cache.count(""), 0);
    }
}
