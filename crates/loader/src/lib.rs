// Unified asset/source adapters, script languages and hot reload.

#![warn(unused_crate_dependencies)]

pub mod adapter;
mod language;
mod loader;
mod report;

pub use adapter::{
    AdaptedProject, AdapterCategory, AdapterDescriptor, FormatAdapter, KeineStore, LoaderRegistry,
    ProjectAdapter, ProjectDebugCursor, ProjectInitialState, SavedState, StoreAdapter,
    StoreMetadata, StoreStatus, StructuredSceneLoader, WebGalLanguage, mount_hexz, parse_webgal,
    parse_webgal_report,
};
pub use language::{ScriptLanguage, ScriptLanguageRegistry};
pub use loader::{
    ContentBackend, ContentFile, ContentMount, ContentProject, HexzArchive, HexzCursor, HexzFile,
    LoadedScene, ScriptWatcher, SourceMount, hexz_password, load_hexz_project,
    load_hexz_project_from_archive, load_project, load_project_with, load_scenes, load_scenes_with,
};
pub use report::{
    Diagnostic, DiagnosticLevel, ParseReport, ResourceKind, ResourceRef, SceneRef, SourceSpan,
};
