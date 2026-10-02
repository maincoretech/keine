// Unified asset/source adapters, script languages and hot reload.

#![warn(unused_crate_dependencies)]

pub mod adapter;
pub mod compiled;
mod language;
mod loader;
mod report;
#[path = "source/input.rs"]
mod source_input;

pub use adapter::{
    AdaptedProject, AdapterCategory, AdapterDescriptor, FormatAdapter, KeineStore, LoaderRegistry,
    NativeDocument, NativeLanguage, NativeSceneSyntax, NativeToken, NativeTokenKind,
    ProjectAdapter, ProjectDebugCursor, ProjectInitialState, SavedState, SourceLineIndex,
    StoreAdapter, StoreMetadata, StoreStatus, StructuredSceneLoader, WebGalLanguage,
    format_native_source, is_native_dotted_command, is_native_structured_command,
    native_expanded_fields, native_stage_property_names, native_tokens, parse_native_document,
    parse_native_scenes, parse_webgal, parse_webgal_report,
};
pub use compiled::{
    CompiledError, CompiledProgramV1, CompiledSceneV1, DecodedProgram, ENVELOPE_VERSION,
    EncodeInput, FIXED_HEADER_LEN, IR_SCHEMA_VERSION, PROGRAM_MAGIC, ProgramMetadataV1, decode,
    encode,
};
pub use hakutaku_core::OpenPolicy;
pub use language::{ParsedScene, ScriptLanguage, ScriptLanguageRegistry};
#[cfg(feature = "hot-reload")]
pub use loader::ScriptWatcher;
pub use loader::{
    ContentBackend, ContentFile, ContentMount, ContentProject, HakutakuArchive, LoadedScene,
    SourceMount, load_hakutaku_project, load_hakutaku_project_from_archive, load_project,
    load_project_with, load_scenes, load_scenes_with, load_startup_scenes_with,
    validate_native_entry_flow,
};
pub use report::{
    Diagnostic, DiagnosticLevel, ParseReport, ResourceKind, ResourceRef, SceneRef, SourceSpan,
};
pub use source_input::MAX_SOURCE_FILE_BYTES;

// Criterion is a bench-only dev-dependency; the lib-test build sees it as
// available and the crate-level lint would otherwise report it as unused.
#[cfg(test)]
use criterion as _;
