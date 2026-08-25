//! Native editor project adapters.
//!
//! Unlike script adapters, project adapters may need several related JSON
//! files to resolve IDs and compile one adapter-neutral program. They remain
//! inside the loader crate so neither the core VM nor the Bevy runtime needs
//! to know which editor authored the project.

mod letsgal;

pub use letsgal::LetsGalProjectAdapter;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use keine_core::Value;
use keine_core::config::GameConfig;

use crate::{ContentProject, LoadedScene};

/// Editor-selected debug position expressed as a stable scene id and one-based
/// source step used by a structured compiler's [`crate::SourceSpan`].
///
/// Some editor file formats call this a cursor; it is not a mouse pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDebugCursor {
    pub scene: String,
    pub source_step: usize,
}

/// Adapter-neutral defaults used when a native editor preview deterministically
/// rebuilds its VM state at the selected source block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectInitialState {
    pub variables: HashMap<String, Value>,
    pub shared_variables: HashMap<String, Value>,
}

/// Multi-file scene compiler retained by [`ContentProject`] for startup and
/// hot reload. Keeping this interface narrow makes editor formats replaceable
/// without leaking their data model into the runtime.
pub trait StructuredSceneLoader: Send + Sync {
    fn name(&self) -> &'static str;
    fn load(&self, project_root: &Path) -> Result<Vec<LoadedScene>>;

    /// Loads the initial runtime program. Reloadable source adapters reuse
    /// [`Self::load`]; packaged adapters may transfer a decoded payload once
    /// instead of cloning it into a second action tree.
    fn load_startup(&self, project_root: &Path) -> Result<Vec<LoadedScene>> {
        self.load(project_root)
    }

    fn watch_roots(&self, project_root: &Path) -> Vec<PathBuf>;
    fn accepts_change(&self, path: &Path) -> bool;

    /// Rebuilds adapter-derived configuration such as asset aliases after an
    /// editor manifest changes. Script-only adapters keep the startup config.
    fn load_config(&self, _project_root: &Path) -> Result<Option<GameConfig>> {
        Ok(None)
    }

    fn is_debug_cursor_change(&self, _path: &Path) -> bool {
        false
    }

    fn debug_cursor(&self, _project_root: &Path) -> Result<Option<ProjectDebugCursor>> {
        Ok(None)
    }

    fn initial_state(&self, _project_root: &Path) -> Result<ProjectInitialState> {
        Ok(ProjectInitialState::default())
    }
}

pub(super) fn structured_project(
    root: PathBuf,
    sources: Vec<crate::SourceMount>,
    loader: Arc<dyn StructuredSceneLoader>,
) -> ContentProject {
    ContentProject::with_structured_scenes(root, sources, loader)
}
