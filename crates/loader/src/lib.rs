// Unified asset/source adapters, script languages and hot reload.

#![warn(unused_crate_dependencies)]

pub mod adapter;
pub mod compiled;
mod language;
mod loader;
mod report;

pub use adapter::{
    AdaptedProject, AdapterCategory, AdapterDescriptor, FormatAdapter, KeineStore, LoaderRegistry,
    ProjectAdapter, ProjectDebugCursor, ProjectInitialState, SavedState, StoreAdapter,
    StoreMetadata, StoreStatus, StructuredSceneLoader, WebGalLanguage, mount_hexz, parse_webgal,
    parse_webgal_report,
};
pub use compiled::{
    CompiledError, CompiledProgramV1, CompiledSceneV1, DecodedProgram, ENVELOPE_VERSION,
    EncodeInput, FIXED_HEADER_LEN, IR_SCHEMA_VERSION, PROGRAM_MAGIC, ProgramMetadataV1, decode,
    encode,
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

// Criterion is a bench-only dev-dependency; the lib-test build sees it as
// available and the crate-level lint would otherwise report it as unused.
#[cfg(test)]
use criterion as _;
