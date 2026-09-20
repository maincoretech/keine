use std::fmt;
use std::ops::Range;

use keine_loader::{Diagnostic, NativeTokenKind, parse_native_document};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneCard {
    pub name: String,
    pub name_range: Range<usize>,
    pub source_range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadOnlyCard {
    pub source_range: Option<Range<usize>>,
    pub message: String,
}

/// A disposable projection over the authoritative `.shou` source. It owns no
/// source text and every editable field points at an exact source byte range.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EiyashouProjection {
    pub scenes: Vec<SceneCard>,
    pub read_only: Vec<ReadOnlyCard>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectionEditError {
    MissingScene,
    InvalidIdentifier,
    DuplicateScene,
    StaleRange,
}

impl fmt::Display for ProjectionEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingScene => formatter.write_str("scene no longer exists"),
            Self::InvalidIdentifier => formatter.write_str(
                "scene names must start with a letter or underscore and contain only letters, digits, or underscores",
            ),
            Self::DuplicateScene => formatter.write_str("scene name is already used"),
            Self::StaleRange => formatter.write_str("source changed; refresh the Card view"),
        }
    }
}

impl std::error::Error for ProjectionEditError {}

impl EiyashouProjection {
    pub fn parse(source: &str) -> Self {
        let document = parse_native_document(source);
        let scenes = document
            .scenes
            .into_iter()
            .map(|scene| SceneCard {
                name: scene.name,
                name_range: scene.name_range,
                source_range: scene.range,
            })
            .collect();
        let mut read_only = document
            .tokens
            .into_iter()
            .filter(|token| token.kind == NativeTokenKind::Unknown)
            .map(|token| ReadOnlyCard {
                source_range: Some(token.range),
                message: "Unknown syntax is preserved. Edit it in Text view.".into(),
            })
            .collect::<Vec<_>>();
        read_only.extend(document.diagnostics.iter().map(read_only_diagnostic));
        Self { scenes, read_only }
    }

    /// Apply the smallest possible edit to the source. No whitespace, comment,
    /// statement, or unknown token outside the identifier range is rewritten.
    pub fn rename_scene(
        &self,
        source: &str,
        scene_index: usize,
        new_name: &str,
    ) -> Result<String, ProjectionEditError> {
        if !valid_identifier(new_name) {
            return Err(ProjectionEditError::InvalidIdentifier);
        }
        let scene = self
            .scenes
            .get(scene_index)
            .ok_or(ProjectionEditError::MissingScene)?;
        if self
            .scenes
            .iter()
            .enumerate()
            .any(|(index, other)| index != scene_index && other.name == new_name)
        {
            return Err(ProjectionEditError::DuplicateScene);
        }
        if source.get(scene.name_range.clone()) != Some(scene.name.as_str()) {
            return Err(ProjectionEditError::StaleRange);
        }
        let mut edited = source.to_owned();
        edited.replace_range(scene.name_range.clone(), new_name);
        Ok(edited)
    }
}

fn read_only_diagnostic(diagnostic: &Diagnostic) -> ReadOnlyCard {
    ReadOnlyCard {
        source_range: None,
        message: format!(
            "Line {}, column {}: {}",
            diagnostic.span.line, diagnostic.span.column, diagnostic.message
        ),
    }
}

fn valid_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_rename_is_a_bounded_source_edit() {
        let source = "// keep\nscene opening {\n  \"text\"\n}\n";
        let projection = EiyashouProjection::parse(source);
        let edited = projection.rename_scene(source, 0, "intro").unwrap();
        assert_eq!(edited, "// keep\nscene intro {\n  \"text\"\n}\n");
    }

    #[test]
    fn invalid_or_stale_scene_edits_fail_closed() {
        let source = "scene opening { \"text\" }";
        let projection = EiyashouProjection::parse(source);
        assert_eq!(
            projection.rename_scene(source, 0, "not valid"),
            Err(ProjectionEditError::InvalidIdentifier)
        );
        assert_eq!(
            projection.rename_scene("scene changed { \"text\" }", 0, "intro"),
            Err(ProjectionEditError::StaleRange)
        );
    }

    #[test]
    fn unknown_syntax_stays_visible_as_read_only_cards() {
        let projection = EiyashouProjection::parse("scene opening { ? } ");
        assert!(!projection.read_only.is_empty());
    }
}
