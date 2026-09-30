//! Pure project-source edits; application and undo ownership stay in app/edits.rs.
use std::fmt;
use std::ops::Range;

use keine_core::config::EiyashouCharacterManifest;
use keine_loader::{NativeTokenKind, parse_native_document};

use crate::projection::EiyashouProjection;

use super::{AuthoringIndex, DialogueEntry, InsertKind, insertion_statement, valid_identifier};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthoringEditError {
    InvalidIdentifier,
    DuplicateIdentifier,
    EmptyName,
    InvalidColor,
    MissingInsertionPoint,
    InvalidManifest,
    UnsupportedDynamicText,
    StaleRange,
}

impl fmt::Display for AuthoringEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier => formatter.write_str("identifier is invalid"),
            Self::DuplicateIdentifier => formatter.write_str("identifier is already used"),
            Self::EmptyName => formatter.write_str("display name cannot be empty"),
            Self::InvalidColor => formatter.write_str("color must use #RRGGBB"),
            Self::MissingInsertionPoint => formatter.write_str("no stable insertion point exists"),
            Self::InvalidManifest => formatter.write_str("manifest is not valid Eiyashou YAML"),
            Self::UnsupportedDynamicText => {
                formatter.write_str("dynamic dialogue must be edited in Text view")
            }
            Self::StaleRange => formatter.write_str("source changed; refresh this view"),
        }
    }
}

impl std::error::Error for AuthoringEditError {}

pub fn replace_dialogue_text(
    source: &str,
    dialogue: &DialogueEntry,
    text: &str,
) -> Result<String, AuthoringEditError> {
    if !dialogue.editable {
        return Err(AuthoringEditError::UnsupportedDynamicText);
    }
    if source.get(dialogue.source_range.clone()).is_none()
        || source.get(dialogue.text_range.clone()).is_none()
    {
        return Err(AuthoringEditError::StaleRange);
    }
    let mut edited = source.to_owned();
    edited.replace_range(dialogue.text_range.clone(), &escape_eiyashou_string(text));
    Ok(edited)
}

pub fn insert_statement(
    source: &str,
    line: usize,
    kind: InsertKind,
    index: &AuthoringIndex,
) -> Result<String, AuthoringEditError> {
    let line_start =
        line_start_offset(source, line).ok_or(AuthoringEditError::MissingInsertionPoint)?;
    let line_end = source[line_start..]
        .find('\n')
        .map(|offset| line_start + offset + 1)
        .unwrap_or(source.len());
    let content_end = line_end.saturating_sub(usize::from(
        line_end > line_start && source.as_bytes().get(line_end - 1) == Some(&b'\n'),
    ));
    let current = source[line_start..content_end].trim();
    if current.starts_with('}') {
        return Err(AuthoringEditError::MissingInsertionPoint);
    }
    let next = source[line_end..].trim_start();
    let statement_has_follower = !next.is_empty() && !next.starts_with('}');
    let current_needs_separator = !current.is_empty()
        && !current.ends_with('{')
        && !current.ends_with(',')
        && !current.ends_with('}');
    let indent = source[line_start..]
        .chars()
        .take_while(|character| character.is_whitespace() && *character != '\n')
        .collect::<String>();
    let statement_indent = if indent.is_empty() {
        "  ".to_owned()
    } else {
        indent
    };
    let statement = insertion_statement(source, kind, index, &statement_indent)?;
    let mut edited = source.to_owned();
    let mut insertion = line_end;
    if current_needs_separator {
        edited.insert(content_end, ',');
        insertion += 1;
    }
    let trailing = if statement_has_follower { "," } else { "" };
    edited.insert_str(
        insertion,
        &format!("{statement_indent}{statement}{trailing}\n"),
    );
    Ok(edited)
}

pub fn append_scene(source: &str, id: &str) -> Result<String, AuthoringEditError> {
    if !valid_identifier(id) {
        return Err(AuthoringEditError::InvalidIdentifier);
    }
    if EiyashouProjection::parse(source)
        .scenes
        .iter()
        .any(|scene| scene.name == id)
    {
        return Err(AuthoringEditError::DuplicateIdentifier);
    }
    let separator = if source.is_empty() || source.ends_with("\n\n") {
        ""
    } else if source.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    Ok(format!("{source}{separator}scene {id} {{\n}}\n"))
}

pub fn rename_scene(source: &str, start: usize, id: &str) -> Result<String, AuthoringEditError> {
    if !valid_identifier(id) {
        return Err(AuthoringEditError::InvalidIdentifier);
    }
    let document = parse_native_document(source);
    let scene = document
        .scenes
        .iter()
        .find(|scene| scene.range.start == start)
        .ok_or(AuthoringEditError::StaleRange)?;
    if document
        .scenes
        .iter()
        .any(|other| other.range.start != start && other.name == id)
    {
        return Err(AuthoringEditError::DuplicateIdentifier);
    }
    let mut ranges = scene_references(source, &scene.name);
    ranges.push(scene.name_range.clone());
    ranges.sort_by_key(|range| range.start);
    let mut edited = source.to_owned();
    for range in ranges.into_iter().rev() {
        edited.replace_range(range, id);
    }
    Ok(edited)
}

pub fn rename_scene_references(source: &str, old: &str, new: &str) -> String {
    let mut edited = source.to_owned();
    for range in scene_references(source, old).into_iter().rev() {
        edited.replace_range(range, new);
    }
    edited
}

pub fn delete_scene(source: &str, start: usize) -> Result<String, AuthoringEditError> {
    let scene = parse_native_document(source)
        .scenes
        .into_iter()
        .find(|scene| scene.range.start == start)
        .ok_or(AuthoringEditError::StaleRange)?;
    let mut edited = source.to_owned();
    edited.replace_range(scene.range, "");
    Ok(edited)
}

pub fn move_scene(
    source: &str,
    start: usize,
    direction: crate::projection::MoveDirection,
) -> Result<String, AuthoringEditError> {
    let document = parse_native_document(source);
    let index = document
        .scenes
        .iter()
        .position(|scene| scene.range.start == start)
        .ok_or(AuthoringEditError::StaleRange)?;
    let neighbor = match direction {
        crate::projection::MoveDirection::Up => index.checked_sub(1),
        crate::projection::MoveDirection::Down => {
            (index + 1 < document.scenes.len()).then_some(index + 1)
        }
    }
    .ok_or(AuthoringEditError::MissingInsertionPoint)?;
    let (first, second) = if index < neighbor {
        (&document.scenes[index], &document.scenes[neighbor])
    } else {
        (&document.scenes[neighbor], &document.scenes[index])
    };
    let before = source
        .get(first.range.clone())
        .ok_or(AuthoringEditError::StaleRange)?;
    let between = source
        .get(first.range.end..second.range.start)
        .ok_or(AuthoringEditError::StaleRange)?;
    let after = source
        .get(second.range.clone())
        .ok_or(AuthoringEditError::StaleRange)?;
    let mut edited = source.to_owned();
    edited.replace_range(
        first.range.start..second.range.end,
        &format!("{after}{between}{before}"),
    );
    Ok(edited)
}

pub fn scene_references(source: &str, name: &str) -> Vec<Range<usize>> {
    let document = parse_native_document(source);
    let tokens = document
        .tokens
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                NativeTokenKind::Whitespace | NativeTokenKind::Comment
            )
        })
        .collect::<Vec<_>>();
    tokens
        .windows(4)
        .filter_map(|window| {
            let [command, open, target, close] = window else {
                return None;
            };
            let text = |token: &keine_loader::NativeToken| source.get(token.range.clone());
            ((text(command) == Some("goto") || text(command) == Some("call"))
                && text(open) == Some("(")
                && target.kind == NativeTokenKind::Identifier
                && text(target) == Some(name)
                && text(close) == Some(")"))
            .then(|| target.range.clone())
        })
        .collect()
}

pub fn append_character(
    source: &str,
    id: &str,
    name: &str,
    color: Option<&str>,
) -> Result<String, AuthoringEditError> {
    if !valid_identifier(id) {
        return Err(AuthoringEditError::InvalidIdentifier);
    }
    if name.trim().is_empty() {
        return Err(AuthoringEditError::EmptyName);
    }
    if let Some(color) = color.filter(|value| !value.trim().is_empty())
        && !valid_color(color)
    {
        return Err(AuthoringEditError::InvalidColor);
    }
    let manifest = EiyashouCharacterManifest::from_yaml(source)
        .map_err(|_| AuthoringEditError::InvalidManifest)?;
    if manifest.characters.contains_key(id) {
        return Err(AuthoringEditError::DuplicateIdentifier);
    }
    let mut entry = format!("  {id}:\n    name: \"{}\"\n", escape_yaml_string(name));
    if let Some(color) = color.filter(|value| !value.trim().is_empty()) {
        entry.push_str(&format!("    color: \"{}\"\n", escape_yaml_string(color)));
    }
    let mut edited = source.to_owned();
    if let Some(empty_mapping) = source.find("characters: {}") {
        let range = empty_mapping..empty_mapping + "characters: {}".len();
        edited.replace_range(range, &format!("characters:\n{entry}"));
    } else {
        let insertion =
            character_insertion_offset(source).ok_or(AuthoringEditError::MissingInsertionPoint)?;
        edited.insert_str(insertion, &entry);
    }
    EiyashouCharacterManifest::from_yaml(&edited)
        .map_err(|_| AuthoringEditError::InvalidManifest)?;
    Ok(edited)
}

pub fn escape_eiyashou_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '$' if characters.peek() == Some(&'{') => escaped.push_str("/$"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn character_insertion_offset(source: &str) -> Option<usize> {
    let mut offset = 0;
    let mut in_characters = false;
    for segment in source.split_inclusive('\n') {
        let trimmed = segment.trim_end_matches(['\r', '\n']);
        if !in_characters {
            if trimmed.trim() == "characters:" && !trimmed.starts_with(char::is_whitespace) {
                in_characters = true;
            }
        } else if !trimmed.is_empty() && !trimmed.starts_with(char::is_whitespace) {
            return Some(offset);
        }
        offset += segment.len();
    }
    in_characters.then_some(source.len())
}

fn escape_yaml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn valid_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn line_start_offset(source: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return Some(0);
    }
    source
        .match_indices('\n')
        .nth(line.saturating_sub(1))
        .map(|(offset, _)| offset + 1)
}
