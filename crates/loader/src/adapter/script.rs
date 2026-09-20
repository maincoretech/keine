mod native;
mod webgal;

use std::sync::Arc;

use crate::ScriptLanguage;

pub(crate) use native::eiyashou_semantic_diagnostics;
pub use native::{
    NativeDocument, NativeLanguage, NativeSceneSyntax, NativeToken, NativeTokenKind,
    parse_native_document, parse_native_scenes,
};
pub use webgal::{WebGalLanguage, parse_webgal, parse_webgal_report};

pub(crate) fn builtin() -> Vec<Arc<dyn ScriptLanguage>> {
    vec![Arc::new(WebGalLanguage), Arc::new(NativeLanguage)]
}
