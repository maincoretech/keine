use std::path::Path;
use std::sync::Arc;

use crate::ParseReport;
use crate::adapter::WebGalLanguage;

/// Converts one authoring-language syntax into keine's language-neutral IR.
///
/// Language integrations stop at `ParseReport`; the runtime only consumes the
/// resulting `Action` list and never depends on source syntax.
pub trait ScriptLanguage: Send + Sync {
    fn name(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str];
    fn parse(&self, source: &str) -> ParseReport;

    fn supports(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                self.extensions()
                    .iter()
                    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
            })
    }
}

#[derive(Clone)]
pub struct ScriptLanguageRegistry {
    languages: Vec<Arc<dyn ScriptLanguage>>,
}

impl Default for ScriptLanguageRegistry {
    fn default() -> Self {
        Self::new().with(WebGalLanguage)
    }
}

impl ScriptLanguageRegistry {
    pub fn new() -> Self {
        Self {
            languages: Vec::new(),
        }
    }

    pub fn with(mut self, language: impl ScriptLanguage + 'static) -> Self {
        self.languages.push(Arc::new(language));
        self
    }

    pub fn register(&mut self, language: impl ScriptLanguage + 'static) {
        self.languages.push(Arc::new(language));
    }

    pub(crate) fn register_shared(&mut self, language: Arc<dyn ScriptLanguage>) {
        self.languages.push(language);
    }

    pub(crate) fn select(&self, name: &str) -> Option<Self> {
        self.languages
            .iter()
            .find(|language| language.name().eq_ignore_ascii_case(name))
            .map(|language| Self {
                languages: vec![language.clone()],
            })
    }

    pub fn language_for(&self, path: &Path) -> Option<&dyn ScriptLanguage> {
        self.languages
            .iter()
            .find(|language| language.supports(path))
            .map(AsRef::as_ref)
    }

    pub fn supports(&self, path: &Path) -> bool {
        self.language_for(path).is_some()
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.languages.iter().map(|language| language.name())
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.languages.retain(|language| keep(language.name()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StoryLanguage;

    impl ScriptLanguage for StoryLanguage {
        fn name(&self) -> &'static str {
            "Story"
        }

        fn extensions(&self) -> &'static [&'static str] {
            &["story"]
        }

        fn parse(&self, _source: &str) -> ParseReport {
            ParseReport::default()
        }
    }

    #[test]
    fn registry_can_add_languages_without_runtime_changes() {
        let registry = ScriptLanguageRegistry::default().with(StoryLanguage);

        assert_eq!(
            registry
                .language_for(Path::new("start.txt"))
                .unwrap()
                .name(),
            "WebGAL"
        );
        assert_eq!(
            registry
                .language_for(Path::new("chapter.story"))
                .unwrap()
                .name(),
            "Story"
        );
        assert!(registry.language_for(Path::new("notes.md")).is_none());
    }
}
