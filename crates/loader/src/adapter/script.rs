mod native;
mod webgal;

use std::sync::Arc;

use crate::ScriptLanguage;

pub(crate) use native::eiyashou_semantic_diagnostics;
pub use native::{
    NativeDocument, NativeLanguage, NativeSceneSyntax, NativeToken, NativeTokenKind,
    SourceLineIndex, format_native_source, is_native_dotted_command, is_native_structured_command,
    native_child_command_argument_names, native_command_argument_names, native_expanded_fields,
    native_stage_property_names, native_text_argument_names, native_text_voice_allowed,
    native_tokens, parse_native_document, parse_native_scenes,
};
pub use webgal::{WebGalLanguage, parse_webgal, parse_webgal_report};

pub(crate) fn builtin() -> Vec<Arc<dyn ScriptLanguage>> {
    vec![Arc::new(WebGalLanguage), Arc::new(NativeLanguage)]
}
