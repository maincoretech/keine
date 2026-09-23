use std::ops::Range;
use std::{collections::HashSet, fmt};

use keine_loader::{Diagnostic, NativeToken, NativeTokenKind, parse_native_document};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneSection {
    pub name: String,
    pub name_range: Range<usize>,
    pub source_range: Range<usize>,
    pub blocks: Vec<BlockCard>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockCard {
    pub kind: BlockKind,
    pub source_range: Range<usize>,
    pub text_range: Option<Range<usize>>,
    pub line: usize,
    pub column: usize,
    pub depth: usize,
    pub summary: String,
    pub stable_id: Option<String>,
    pub read_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Narration,
    Dialogue { speaker: String },
    Choice,
    ChoiceOption,
    Conditional,
    ElseIf,
    Else,
    Loop,
    Declaration,
    Assignment,
    Command,
    Control,
    Unsupported,
}

impl BlockKind {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Narration => "Text",
            Self::Dialogue { .. } => "Dialogue",
            Self::Choice => "Choice",
            Self::ChoiceOption => "Option",
            Self::Conditional => "If",
            Self::ElseIf => "Else if",
            Self::Else => "Else",
            Self::Loop => "Loop",
            Self::Declaration => "Let",
            Self::Assignment => "Set",
            Self::Command => "Command",
            Self::Control => "Flow",
            Self::Unsupported => "Source",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadOnlyCard {
    pub source_range: Option<Range<usize>>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextBlockMetadata {
    pub speaker: Option<String>,
    pub voice: Option<String>,
    pub stable_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceField {
    pub key: String,
    pub range: Range<usize>,
    pub value: String,
    pub quoted: bool,
    /// Syntax prepended when this optional argument does not yet exist.
    pub insertion: Option<String>,
    pub insertion_suffix: Option<String>,
}

/// A disposable projection over the authoritative `.shou` source. It owns no
/// source text and every editable field points at an exact source byte range.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EiyashouProjection {
    pub scenes: Vec<SceneSection>,
    pub read_only: Vec<ReadOnlyCard>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveDirection {
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockEditError {
    MissingSelection,
    StaleRange,
    NonContiguousSelection,
    NoMoveTarget,
    InvalidIdentifier,
    NotEditableText,
}

impl fmt::Display for BlockEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSelection => formatter.write_str("no blocks are selected"),
            Self::StaleRange => formatter.write_str("source changed; refresh the Block view"),
            Self::NonContiguousSelection => {
                formatter.write_str("selected blocks must share one structural scope")
            }
            Self::NoMoveTarget => formatter.write_str("selected blocks cannot move further"),
            Self::InvalidIdentifier => formatter.write_str("identifier is invalid"),
            Self::NotEditableText => formatter.write_str("block is not editable text"),
        }
    }
}

impl std::error::Error for BlockEditError {}

impl EiyashouProjection {
    pub fn source_fields(&self, source: &str, start: usize) -> Option<Vec<SourceField>> {
        let block = self
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .find(|block| block.source_range.start == start)?;
        let node = source.get(block.source_range.clone())?;
        match block.kind {
            BlockKind::Command | BlockKind::Choice | BlockKind::Conditional | BlockKind::ElseIf => {
                let header = if block.kind == BlockKind::Command {
                    node
                } else {
                    &node[..top_level_position(node, '{').unwrap_or(node.len())]
                };
                let Some(open) = header.find('(') else {
                    if block.kind == BlockKind::Choice {
                        let insert = block.source_range.start + "choice".len();
                        return Some(vec![SourceField {
                            key: "prompt".into(),
                            range: insert..insert,
                            value: String::new(),
                            quoted: true,
                            insertion: Some("(\"".into()),
                            insertion_suffix: Some("\")".into()),
                        }]);
                    }
                    return None;
                };
                let close = matching_parenthesis(header, open)?;
                let argument_start = block.source_range.start + open + 1;
                let arguments = &node[open + 1..close];
                let mut fields = Vec::new();
                for (position, span) in split_source_ranges(arguments, ',').into_iter().enumerate()
                {
                    let absolute = argument_start + span.start..argument_start + span.end;
                    let Some(full) = trimmed_source_range(source, absolute) else {
                        continue;
                    };
                    let raw = source.get(full.clone())?;
                    let (key, range) = if let Some(colon) = top_level_position(raw, ':') {
                        let name = raw[..colon].trim();
                        if valid_identifier(name) {
                            (
                                name.to_owned(),
                                trimmed_source_range(source, full.start + colon + 1..full.end)?,
                            )
                        } else {
                            (position.to_string(), full)
                        }
                    } else {
                        (position.to_string(), full)
                    };
                    let value = source.get(range.clone())?;
                    let quoted = value.len() >= 2 && value.starts_with('"') && value.ends_with('"');
                    let range = if quoted {
                        range.start + 1..range.end - 1
                    } else {
                        range
                    };
                    let raw = source.get(range.clone())?;
                    fields.push(SourceField {
                        key,
                        value: if quoted {
                            decode_source_string(raw).unwrap_or_else(|| raw.to_owned())
                        } else {
                            raw.to_owned()
                        },
                        range,
                        quoted,
                        insertion: None,
                        insertion_suffix: None,
                    });
                }
                if block.kind == BlockKind::Command {
                    let command = node[..open].trim();
                    let optional: &[&str] = match command {
                        "background" | "hide" => &["transition"],
                        "sprite" => &["position", "transition", "z"],
                        "move" => &["duration", "easing"],
                        "bgm" => &["volume", "fade", "loop"],
                        "se" => &["volume"],
                        "video" => &["skippable"],
                        "pop" => &["into"],
                        _ => &[],
                    };
                    let insertion_point = argument_start + arguments.trim_end().len();
                    let has_arguments = !arguments.trim().is_empty();
                    for name in optional {
                        if fields.iter().any(|field| field.key == *name) {
                            continue;
                        }
                        fields.push(SourceField {
                            key: (*name).to_owned(),
                            range: insertion_point..insertion_point,
                            value: String::new(),
                            quoted: false,
                            insertion: Some(format!(
                                "{}{name}: ",
                                if has_arguments { ", " } else { "" }
                            )),
                            insertion_suffix: None,
                        });
                    }
                }
                Some(fields)
            }
            BlockKind::ChoiceOption => {
                let range = block.text_range.clone()?;
                let raw = source.get(range.clone())?;
                Some(vec![SourceField {
                    key: "option".into(),
                    value: decode_source_string(raw).unwrap_or_else(|| raw.to_owned()),
                    range,
                    quoted: true,
                    insertion: None,
                    insertion_suffix: None,
                }])
            }
            BlockKind::Declaration | BlockKind::Assignment => {
                let equal = node.find('=')?;
                let range = trimmed_source_range(
                    source,
                    block.source_range.start + equal + 1..block.source_range.end,
                )?;
                Some(vec![SourceField {
                    key: "value".into(),
                    value: source.get(range.clone())?.to_owned(),
                    range,
                    quoted: false,
                    insertion: None,
                    insertion_suffix: None,
                }])
            }
            _ => Some(Vec::new()),
        }
    }
    pub fn parse(source: &str) -> Self {
        let document = parse_native_document(source);
        let parser = BlockProjectionParser::new(source, &document.tokens);
        let scenes = document
            .scenes
            .into_iter()
            .map(|scene| {
                let blocks = parser.scene_blocks(scene.range.clone());
                SceneSection {
                    name: scene.name,
                    name_range: scene.name_range,
                    source_range: scene.range,
                    blocks,
                }
            })
            .collect();
        let read_only = document
            .diagnostics
            .iter()
            .map(read_only_diagnostic)
            .collect();
        Self { scenes, read_only }
    }

    pub fn copy_blocks(
        &self,
        source: &str,
        selected: &HashSet<usize>,
    ) -> Result<String, BlockEditError> {
        let ranges = self.selected_ranges(selected)?;
        ranges
            .iter()
            .map(|range| {
                source
                    .get(range.clone())
                    .map(str::to_owned)
                    .ok_or(BlockEditError::StaleRange)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|blocks| blocks.join(",\n"))
    }

    pub fn delete_blocks(
        &self,
        source: &str,
        selected: &HashSet<usize>,
    ) -> Result<String, BlockEditError> {
        let ranges = self.selected_ranges(selected)?;
        let mut edited = source.to_owned();
        for range in ranges.into_iter().rev() {
            let range = deletion_range(source, range);
            if edited.get(range.clone()).is_none() {
                return Err(BlockEditError::StaleRange);
            }
            edited.replace_range(range, "");
        }
        Ok(edited)
    }

    pub fn move_blocks(
        &self,
        source: &str,
        selected: &HashSet<usize>,
        direction: MoveDirection,
    ) -> Result<String, BlockEditError> {
        let ranges = self.selected_ranges(selected)?;
        let starts = ranges
            .iter()
            .map(|range| range.start)
            .collect::<HashSet<_>>();
        if self
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .any(|block| {
                starts.contains(&block.source_range.start)
                    && matches!(&block.kind, BlockKind::ElseIf | BlockKind::Else)
            })
        {
            return Err(BlockEditError::NoMoveTarget);
        }
        let Some((scene, depth)) = self.scenes.iter().find_map(|scene| {
            scene
                .blocks
                .iter()
                .find(|block| starts.contains(&block.source_range.start))
                .map(|block| (scene, block.depth))
        }) else {
            return Err(BlockEditError::MissingSelection);
        };
        let selected_block = scene
            .blocks
            .iter()
            .find(|block| starts.contains(&block.source_range.start))
            .ok_or(BlockEditError::MissingSelection)?;
        let scope = block_scope(scene, selected_block);
        let siblings = scene
            .blocks
            .iter()
            .filter(|block| block.depth == depth && block_scope(scene, block) == scope)
            .collect::<Vec<_>>();
        let selected_indices = siblings
            .iter()
            .enumerate()
            .filter(|(_, block)| starts.contains(&block.source_range.start))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let Some(first_index) = selected_indices.first().copied() else {
            return Err(BlockEditError::MissingSelection);
        };
        if selected_indices.len() != ranges.len() {
            return Err(BlockEditError::NonContiguousSelection);
        }
        let last_index = *selected_indices.last().unwrap_or(&first_index);
        let selected_indices = selected_indices.into_iter().collect::<HashSet<_>>();
        let selected_order = (0..siblings.len())
            .filter(|index| selected_indices.contains(index))
            .collect::<Vec<_>>();
        let mut order = (0..siblings.len())
            .filter(|index| !selected_indices.contains(index))
            .collect::<Vec<_>>();
        let insertion = match direction {
            MoveDirection::Up => {
                let previous = (0..first_index)
                    .rev()
                    .find(|index| !selected_indices.contains(index))
                    .ok_or(BlockEditError::NoMoveTarget)?;
                order
                    .iter()
                    .position(|index| *index == previous)
                    .ok_or(BlockEditError::NoMoveTarget)?
            }
            MoveDirection::Down => {
                let next = (last_index + 1..siblings.len())
                    .find(|index| !selected_indices.contains(index))
                    .ok_or(BlockEditError::NoMoveTarget)?;
                order
                    .iter()
                    .position(|index| *index == next)
                    .map(|index| index + 1)
                    .ok_or(BlockEditError::NoMoveTarget)?
            }
        };
        order.splice(insertion..insertion, selected_order);
        replace_block_texts(source, &siblings, &siblings, &order)
    }

    pub fn move_blocks_to(
        &self,
        source: &str,
        selected: &HashSet<usize>,
        target_start: usize,
    ) -> Result<String, BlockEditError> {
        if selected.contains(&target_start) {
            return Ok(source.to_owned());
        }
        let ranges = self.selected_ranges(selected)?;
        let starts = ranges
            .iter()
            .map(|range| range.start)
            .collect::<HashSet<_>>();
        if self
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .any(|block| {
                starts.contains(&block.source_range.start)
                    && matches!(&block.kind, BlockKind::ElseIf | BlockKind::Else)
            })
        {
            return Err(BlockEditError::NoMoveTarget);
        }
        let Some((scene, target)) = self.scenes.iter().find_map(|scene| {
            scene
                .blocks
                .iter()
                .find(|block| block.source_range.start == target_start)
                .map(|block| (scene, block))
        }) else {
            return Err(BlockEditError::NoMoveTarget);
        };
        let Some(selected_block) = scene
            .blocks
            .iter()
            .find(|block| starts.contains(&block.source_range.start))
        else {
            return Err(BlockEditError::NonContiguousSelection);
        };
        let depth = selected_block.depth;
        let scope = block_scope(scene, selected_block);
        if target.depth != depth || block_scope(scene, target) != scope {
            return Err(BlockEditError::NoMoveTarget);
        }
        let siblings = scene
            .blocks
            .iter()
            .filter(|block| block.depth == depth && block_scope(scene, block) == scope)
            .collect::<Vec<_>>();
        let selected_indices = siblings
            .iter()
            .enumerate()
            .filter(|(_, block)| starts.contains(&block.source_range.start))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if selected_indices.is_empty() {
            return Err(BlockEditError::MissingSelection);
        }
        if selected_indices.len() != ranges.len() {
            return Err(BlockEditError::NonContiguousSelection);
        }
        let target_index = siblings
            .iter()
            .position(|block| block.source_range.start == target_start)
            .ok_or(BlockEditError::NoMoveTarget)?;
        let selected_indices = selected_indices.into_iter().collect::<HashSet<_>>();
        if selected_indices.contains(&target_index) {
            return Ok(source.to_owned());
        }
        let selected_order = (0..siblings.len())
            .filter(|index| selected_indices.contains(index))
            .collect::<Vec<_>>();
        let mut order = (0..siblings.len())
            .filter(|index| !selected_indices.contains(index))
            .collect::<Vec<_>>();
        let insertion = order
            .iter()
            .position(|index| *index == target_index)
            .ok_or(BlockEditError::NoMoveTarget)?;
        order.splice(insertion..insertion, selected_order);
        replace_block_texts(source, &siblings, &siblings, &order)
    }

    pub fn insert_block_after(
        &self,
        source: &str,
        after_start: usize,
        statement: &str,
    ) -> Result<(String, Range<usize>), BlockEditError> {
        let block = self
            .scenes
            .iter()
            .flat_map(|scene| scene.blocks.iter())
            .find(|block| block.source_range.start == after_start)
            .ok_or(BlockEditError::MissingSelection)?;
        let bytes = source.as_bytes();
        let line_start = source[..block.source_range.start]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let indent = source[line_start..block.source_range.start]
            .chars()
            .take_while(|character| matches!(character, ' ' | '\t'))
            .collect::<String>();
        let mut separator = block.source_range.end;
        while separator < bytes.len() && matches!(bytes[separator], b' ' | b'\t' | b'\r') {
            separator += 1;
        }
        let mut edited = source.to_owned();
        let (insertion, text, statement_offset) = if bytes.get(separator) == Some(&b',') {
            let after_comma = separator + 1;
            if let Some(newline) = source[after_comma..].find('\n') {
                let insertion = after_comma + newline + 1;
                let text = format!("{indent}{statement},\n");
                (insertion, text, indent.len())
            } else {
                (after_comma, format!(" {statement},"), 1)
            }
        } else if source[block.source_range.end..].contains('\n') {
            (
                block.source_range.end,
                format!(",\n{indent}{statement}"),
                2 + indent.len(),
            )
        } else {
            (block.source_range.end, format!(", {statement}"), 2)
        };
        let statement_start = insertion + statement_offset;
        edited.insert_str(insertion, &text);
        Ok((edited, statement_start..statement_start + statement.len()))
    }

    pub fn insert_block_in_scene(
        &self,
        source: &str,
        scene_start: usize,
        statement: &str,
    ) -> Result<(String, Range<usize>), BlockEditError> {
        let scene = self
            .scenes
            .iter()
            .find(|scene| scene.source_range.start == scene_start)
            .ok_or(BlockEditError::MissingSelection)?;
        if let Some(last) = scene.blocks.last() {
            return self.insert_block_after(source, last.source_range.start, statement);
        }
        let scene_source = source
            .get(scene.source_range.clone())
            .ok_or(BlockEditError::StaleRange)?;
        let close = scene_source
            .rfind('}')
            .map(|offset| scene.source_range.start + offset)
            .ok_or(BlockEditError::StaleRange)?;
        let close_line = source[..close].rfind('\n').map_or(close, |index| index + 1);
        let close_indent = source
            .get(close_line..close)
            .filter(|prefix| prefix.trim().is_empty())
            .unwrap_or("");
        let indent = format!("{close_indent}  ");
        let (insertion, text, statement_offset) = if close_line < close {
            (close, format!(" {statement} "), 1)
        } else {
            (close_line, format!("{indent}{statement}\n"), indent.len())
        };
        let mut edited = source.to_owned();
        edited.insert_str(insertion, &text);
        let statement_start = insertion + statement_offset;
        Ok((edited, statement_start..statement_start + statement.len()))
    }

    pub fn text_block_metadata(
        &self,
        source: &str,
        block_start: usize,
    ) -> Option<TextBlockMetadata> {
        let block = self
            .scenes
            .iter()
            .flat_map(|scene| scene.blocks.iter())
            .find(|block| block.source_range.start == block_start)?;
        let text_range = block.text_range.as_ref()?;
        let suffix = source
            .get(text_range.end.saturating_add(1)..block.source_range.end)
            .unwrap_or("")
            .trim();
        let voice = suffix
            .strip_prefix(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        Some(TextBlockMetadata {
            speaker: match &block.kind {
                BlockKind::Narration => None,
                BlockKind::Dialogue { speaker } => Some(speaker.clone()),
                _ => return None,
            },
            voice,
            stable_id: block.stable_id.clone(),
        })
    }

    pub fn replace_text_block_metadata(
        &self,
        source: &str,
        block_start: usize,
        metadata: &TextBlockMetadata,
    ) -> Result<String, BlockEditError> {
        let block = self
            .scenes
            .iter()
            .flat_map(|scene| scene.blocks.iter())
            .find(|block| block.source_range.start == block_start)
            .ok_or(BlockEditError::MissingSelection)?;
        if block.read_only {
            return Err(BlockEditError::NotEditableText);
        }
        let text_range = block
            .text_range
            .as_ref()
            .ok_or(BlockEditError::NotEditableText)?;
        for value in [
            metadata.speaker.as_deref(),
            metadata.voice.as_deref(),
            metadata.stable_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !valid_identifier(value) {
                return Err(BlockEditError::InvalidIdentifier);
            }
        }
        let literal = source
            .get(text_range.start.saturating_sub(1)..text_range.end.saturating_add(1))
            .ok_or(BlockEditError::StaleRange)?;
        let mut replacement = String::new();
        if let Some(stable_id) = metadata.stable_id.as_deref() {
            replacement.push('@');
            replacement.push_str(stable_id);
            replacement.push(' ');
        }
        if let Some(speaker) = metadata.speaker.as_deref() {
            replacement.push_str(speaker);
            replacement.push_str(": ");
        }
        replacement.push_str(literal);
        if let Some(voice) = metadata.voice.as_deref() {
            replacement.push_str(", ");
            replacement.push_str(voice);
        }
        let mut edited = source.to_owned();
        if edited.get(block.source_range.clone()).is_none() {
            return Err(BlockEditError::StaleRange);
        }
        edited.replace_range(block.source_range.clone(), &replacement);
        Ok(edited)
    }

    fn selected_ranges(
        &self,
        selected: &HashSet<usize>,
    ) -> Result<Vec<Range<usize>>, BlockEditError> {
        let mut ranges = self
            .scenes
            .iter()
            .flat_map(|scene| scene.blocks.iter())
            .filter(|block| selected.contains(&block.source_range.start))
            .map(|block| block.source_range.clone())
            .collect::<Vec<_>>();
        if ranges.is_empty() {
            return Err(BlockEditError::MissingSelection);
        }
        ranges.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
        let mut normalized: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
        for range in ranges {
            if normalized
                .last()
                .is_some_and(|parent| parent.end >= range.end)
            {
                continue;
            }
            normalized.push(range);
        }
        Ok(normalized)
    }
}

fn replace_block_texts(
    source: &str,
    slots: &[&BlockCard],
    siblings: &[&BlockCard],
    order: &[usize],
) -> Result<String, BlockEditError> {
    if slots.len() != order.len() {
        return Err(BlockEditError::NonContiguousSelection);
    }
    let replacements = order
        .iter()
        .map(|index| {
            source
                .get(siblings[*index].source_range.clone())
                .map(str::to_owned)
                .ok_or(BlockEditError::StaleRange)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut edited = source.to_owned();
    for (slot, replacement) in slots.iter().zip(replacements).rev() {
        if edited.get(slot.source_range.clone()).is_none() {
            return Err(BlockEditError::StaleRange);
        }
        edited.replace_range(slot.source_range.clone(), &replacement);
    }
    Ok(edited)
}

fn block_scope(scene: &SceneSection, block: &BlockCard) -> usize {
    scene
        .blocks
        .iter()
        .filter(|candidate| {
            candidate.depth < block.depth
                && candidate.source_range.start <= block.source_range.start
                && candidate.source_range.end >= block.source_range.end
        })
        .max_by_key(|candidate| {
            (
                candidate.depth,
                std::cmp::Reverse(candidate.source_range.len()),
            )
        })
        .map_or(scene.source_range.start, |candidate| {
            candidate.source_range.start
        })
}

fn deletion_range(source: &str, range: Range<usize>) -> Range<usize> {
    let bytes = source.as_bytes();
    let line_start = source[..range.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let starts_on_indented_line = source[line_start..range.start].trim().is_empty();
    let mut end = range.end;
    while end < bytes.len() && matches!(bytes[end], b' ' | b'\t' | b'\r') {
        end += 1;
    }
    if bytes.get(end) == Some(&b',') {
        end += 1;
        while end < bytes.len() && matches!(bytes[end], b' ' | b'\t' | b'\r') {
            end += 1;
        }
        if bytes.get(end) == Some(&b'\n') {
            end += 1;
        }
        return if starts_on_indented_line {
            line_start..end
        } else {
            range.start..end
        };
    }
    let mut start = if starts_on_indented_line {
        line_start
    } else {
        range.start
    };
    while start > 0 && bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }
    if start > 0 && bytes[start - 1] == b',' {
        start -= 1;
    }
    start..range.end
}

struct BlockProjectionParser<'a> {
    source: &'a str,
    tokens: Vec<&'a NativeToken>,
}

impl<'a> BlockProjectionParser<'a> {
    fn new(source: &'a str, tokens: &'a [NativeToken]) -> Self {
        Self {
            source,
            tokens: tokens
                .iter()
                .filter(|token| {
                    !matches!(
                        token.kind,
                        NativeTokenKind::Whitespace | NativeTokenKind::Comment
                    )
                })
                .collect(),
        }
    }

    fn scene_blocks(&self, scene_range: Range<usize>) -> Vec<BlockCard> {
        let scene_tokens = self
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| {
                token.range.start >= scene_range.start && token.range.end <= scene_range.end
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let Some(open_position) = scene_tokens
            .iter()
            .position(|index| self.text(*index) == "{")
        else {
            return Vec::new();
        };
        let Some(close_position) = matching_delimiter(
            &scene_tokens,
            open_position,
            |index| self.text(index),
            "{",
            "}",
        ) else {
            return Vec::new();
        };
        let mut blocks = Vec::new();
        self.project_statement_list(
            &scene_tokens[open_position + 1..close_position],
            0,
            &mut blocks,
        );
        blocks
    }

    fn project_statement_list(&self, tokens: &[usize], depth: usize, blocks: &mut Vec<BlockCard>) {
        for statement in split_top_level_with_voice(
            tokens,
            |index| self.text(index),
            |index| self.tokens[index].kind,
        ) {
            self.project_statement(statement, depth, blocks);
        }
    }

    fn project_statement(&self, tokens: &[usize], depth: usize, blocks: &mut Vec<BlockCard>) {
        let Some(first) = tokens.first().copied() else {
            return;
        };
        let last = *tokens.last().unwrap_or(&first);
        let source_range = self.tokens[first].range.start..self.tokens[last].range.end;
        if tokens
            .iter()
            .any(|index| self.tokens[*index].kind == NativeTokenKind::Unknown)
        {
            blocks.push(self.block(
                BlockKind::Unsupported,
                source_range,
                None,
                depth,
                None,
                true,
            ));
            return;
        }

        let (stable_id, head) = self.annotation(tokens);
        let Some(head_index) = tokens.get(head).copied() else {
            return;
        };
        if self.tokens[head_index].kind == NativeTokenKind::String {
            blocks.push(self.text_block(
                BlockKind::Narration,
                source_range,
                head_index,
                depth,
                stable_id,
            ));
            return;
        }

        let name = self.text(head_index);
        match name {
            "choice" => self.project_choice(tokens, head, source_range, depth, blocks),
            "if" => self.project_if(tokens, head, source_range, depth, blocks),
            "loop" => self.project_loop(tokens, head, source_range, depth, blocks),
            "let" => blocks.push(self.block(
                BlockKind::Declaration,
                source_range,
                None,
                depth,
                None,
                false,
            )),
            "break" | "return" => {
                blocks.push(self.block(BlockKind::Control, source_range, None, depth, None, false))
            }
            _ if tokens
                .get(head + 1)
                .is_some_and(|index| self.text(*index) == ":") =>
            {
                self.project_dialogue(tokens, head, source_range, depth, stable_id, blocks);
            }
            _ if tokens
                .get(head + 1)
                .is_some_and(|index| self.text(*index) == ".")
                && tokens.get(head + 2).is_some_and(|index| {
                    matches!(self.text(*index), "append" | "remove" | "clear" | "insert")
                })
                && tokens
                    .get(head + 3)
                    .is_some_and(|index| self.text(*index) == "(") =>
            {
                blocks.push(self.block(BlockKind::Command, source_range, None, depth, None, false));
            }
            _ => {
                let kind = if tokens
                    .get(head + 1..)
                    .is_some_and(|tail| tail.iter().any(|index| is_assignment(self.text(*index))))
                {
                    BlockKind::Assignment
                } else if tokens
                    .get(head + 1)
                    .is_some_and(|index| self.text(*index) == "(")
                    && is_native_command(name)
                {
                    BlockKind::Command
                } else {
                    BlockKind::Unsupported
                };
                let read_only = kind == BlockKind::Unsupported;
                blocks.push(self.block(kind, source_range, None, depth, None, read_only));
            }
        }
    }

    fn project_dialogue(
        &self,
        tokens: &[usize],
        head: usize,
        source_range: Range<usize>,
        depth: usize,
        stable_id: Option<String>,
        blocks: &mut Vec<BlockCard>,
    ) {
        let speaker = self.text(tokens[head]).to_owned();
        let value = head + 2;
        let Some(value_index) = tokens.get(value).copied() else {
            blocks.push(self.block(
                BlockKind::Unsupported,
                source_range,
                None,
                depth,
                stable_id,
                true,
            ));
            return;
        };
        if self.text(value_index) == "{" {
            if let Some(close) =
                matching_delimiter(tokens, value, |index| self.text(index), "{", "}")
            {
                for entry in split_top_level_with_voice(
                    &tokens[value + 1..close],
                    |index| self.text(index),
                    |index| self.tokens[index].kind,
                ) {
                    let (entry_id, entry_head) = self.annotation(entry);
                    if let Some(string_index) = entry.get(entry_head).copied()
                        && self.tokens[string_index].kind == NativeTokenKind::String
                    {
                        let entry_range = self.tokens[*entry.first().unwrap()].range.start
                            ..self.tokens[*entry.last().unwrap()].range.end;
                        blocks.push(self.text_block(
                            BlockKind::Dialogue {
                                speaker: speaker.clone(),
                            },
                            entry_range,
                            string_index,
                            depth,
                            entry_id,
                        ));
                    }
                }
            }
            return;
        }
        if self.tokens[value_index].kind == NativeTokenKind::String {
            blocks.push(self.text_block(
                BlockKind::Dialogue { speaker },
                source_range,
                value_index,
                depth,
                stable_id,
            ));
        } else {
            blocks.push(self.block(
                BlockKind::Unsupported,
                source_range,
                None,
                depth,
                stable_id,
                true,
            ));
        }
    }

    fn project_choice(
        &self,
        tokens: &[usize],
        head: usize,
        source_range: Range<usize>,
        depth: usize,
        blocks: &mut Vec<BlockCard>,
    ) {
        blocks.push(self.block(BlockKind::Choice, source_range, None, depth, None, false));
        let Some(open) =
            (head + 1..tokens.len()).find(|position| self.text(tokens[*position]) == "{")
        else {
            return;
        };
        let Some(close) = matching_delimiter(tokens, open, |index| self.text(index), "{", "}")
        else {
            return;
        };
        for option in split_top_level(&tokens[open + 1..close], |index| self.text(index), ",") {
            let (stable_id, option_head) = self.annotation(option);
            let Some(colon) = find_top_level(option, |index| self.text(index), ":") else {
                self.project_statement(option, depth + 1, blocks);
                continue;
            };
            let option_range = self.tokens[*option.first().unwrap()].range.start
                ..self.tokens[*option.last().unwrap()].range.end;
            let text_range = option
                .get(option_head)
                .copied()
                .filter(|index| self.tokens[*index].kind == NativeTokenKind::String)
                .map(|index| inner_string_range(&self.tokens[index].range));
            blocks.push(self.block(
                BlockKind::ChoiceOption,
                option_range,
                text_range,
                depth + 1,
                stable_id,
                false,
            ));
            let branch = &option[colon + 1..];
            if branch.first().is_some_and(|index| self.text(*index) == "{")
                && let Some(branch_close) =
                    matching_delimiter(branch, 0, |index| self.text(index), "{", "}")
            {
                self.project_statement_list(&branch[1..branch_close], depth + 2, blocks);
            } else {
                self.project_statement(branch, depth + 2, blocks);
            }
        }
    }

    fn project_if(
        &self,
        tokens: &[usize],
        head: usize,
        source_range: Range<usize>,
        depth: usize,
        blocks: &mut Vec<BlockCard>,
    ) {
        blocks.push(self.block(
            BlockKind::Conditional,
            source_range,
            None,
            depth,
            None,
            false,
        ));
        let Some(open) =
            (head + 1..tokens.len()).find(|position| self.text(tokens[*position]) == "{")
        else {
            return;
        };
        let Some(close) = matching_delimiter(tokens, open, |index| self.text(index), "{", "}")
        else {
            return;
        };
        self.project_statement_list(&tokens[open + 1..close], depth + 1, blocks);
        let mut cursor = close + 1;
        while tokens
            .get(cursor)
            .is_some_and(|index| self.text(*index) == "else")
        {
            let else_start = tokens[cursor];
            if tokens
                .get(cursor + 1)
                .is_some_and(|index| self.text(*index) == "if")
            {
                let Some(open) =
                    (cursor + 2..tokens.len()).find(|position| self.text(tokens[*position]) == "{")
                else {
                    break;
                };
                let Some(branch_close) =
                    matching_delimiter(tokens, open, |index| self.text(index), "{", "}")
                else {
                    break;
                };
                blocks.push(self.block(
                    BlockKind::ElseIf,
                    self.tokens[else_start].range.start
                        ..self.tokens[tokens[branch_close]].range.end,
                    None,
                    depth,
                    None,
                    false,
                ));
                self.project_statement_list(&tokens[open + 1..branch_close], depth + 1, blocks);
                cursor = branch_close + 1;
                continue;
            }
            if tokens
                .get(cursor + 1)
                .is_some_and(|index| self.text(*index) == "{")
                && let Some(else_close) =
                    matching_delimiter(tokens, cursor + 1, |index| self.text(index), "{", "}")
            {
                blocks.push(self.block(
                    BlockKind::Else,
                    self.tokens[else_start].range.start..self.tokens[tokens[else_close]].range.end,
                    None,
                    depth,
                    None,
                    false,
                ));
                self.project_statement_list(&tokens[cursor + 2..else_close], depth + 1, blocks);
            }
            break;
        }
    }

    fn project_loop(
        &self,
        tokens: &[usize],
        head: usize,
        source_range: Range<usize>,
        depth: usize,
        blocks: &mut Vec<BlockCard>,
    ) {
        blocks.push(self.block(BlockKind::Loop, source_range, None, depth, None, false));
        let Some(open) =
            (head + 1..tokens.len()).find(|position| self.text(tokens[*position]) == "{")
        else {
            return;
        };
        if let Some(close) = matching_delimiter(tokens, open, |index| self.text(index), "{", "}") {
            self.project_statement_list(&tokens[open + 1..close], depth + 1, blocks);
        }
    }

    fn annotation(&self, tokens: &[usize]) -> (Option<String>, usize) {
        if tokens.first().is_some_and(|index| self.text(*index) == "@") {
            let value = tokens.get(1).map(|index| self.text(*index).to_owned());
            (value, 2)
        } else {
            (None, 0)
        }
    }

    fn text_block(
        &self,
        kind: BlockKind,
        source_range: Range<usize>,
        string_index: usize,
        depth: usize,
        stable_id: Option<String>,
    ) -> BlockCard {
        let text_range = inner_string_range(&self.tokens[string_index].range);
        let dynamic = self
            .source
            .get(text_range.clone())
            .is_some_and(|text| text.contains("${"));
        self.block(
            kind,
            source_range,
            Some(text_range),
            depth,
            stable_id,
            dynamic,
        )
    }

    fn block(
        &self,
        kind: BlockKind,
        source_range: Range<usize>,
        text_range: Option<Range<usize>>,
        depth: usize,
        stable_id: Option<String>,
        read_only: bool,
    ) -> BlockCard {
        let line = self.source[..source_range.start.min(self.source.len())]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        let column = self.source[..source_range.start.min(self.source.len())]
            .rsplit_once('\n')
            .map_or(source_range.start, |(_, tail)| tail.chars().count());
        BlockCard {
            summary: compact_summary(self.source.get(source_range.clone()).unwrap_or("")),
            kind,
            source_range,
            text_range,
            line,
            column,
            depth,
            stable_id,
            read_only,
        }
    }

    fn text(&self, index: usize) -> &'a str {
        self.source
            .get(self.tokens[index].range.clone())
            .unwrap_or("")
    }
}

fn inner_string_range(range: &Range<usize>) -> Range<usize> {
    range.start.saturating_add(1)..range.end.saturating_sub(1)
}

fn decode_source_string(raw: &str) -> Option<String> {
    let mut output = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '/' && chars.peek() == Some(&'$') {
            let mut lookahead = chars.clone();
            lookahead.next();
            if lookahead.next() == Some('{') {
                output.push_str("${");
                chars.next();
                chars.next();
                continue;
            }
        }
        if character != '\\' {
            output.push(character);
            continue;
        }
        output.push(match chars.next()? {
            '"' => '"',
            '\\' => '\\',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            _ => return None,
        });
    }
    Some(output)
}

fn matching_parenthesis(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, character) in source
        .char_indices()
        .skip_while(|(offset, _)| *offset < open)
    {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        match character {
            '"' => quoted = true,
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn top_level_position(source: &str, needle: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, character) in source.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        if character == needle && depth == 0 {
            return Some(offset);
        }
        match character {
            '"' => quoted = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

fn split_source_ranges(source: &str, separator: char) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < source.len() {
        let Some(offset) = top_level_position(&source[start..], separator) else {
            ranges.push(start..source.len());
            break;
        };
        ranges.push(start..start + offset);
        start += offset + separator.len_utf8();
    }
    ranges
}

fn trimmed_source_range(source: &str, range: Range<usize>) -> Option<Range<usize>> {
    let value = source.get(range.clone())?;
    let prefix = value.len() - value.trim_start().len();
    let suffix = value.len() - value.trim_end().len();
    let start = range.start + prefix;
    let end = range.end.saturating_sub(suffix);
    (start < end).then_some(start..end)
}

fn compact_summary(source: &str) -> String {
    const LIMIT: usize = 96;
    let compact = source.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= LIMIT {
        return compact;
    }
    let mut summary = compact.chars().take(LIMIT - 1).collect::<String>();
    summary.push('…');
    summary
}

fn split_top_level<'a>(
    tokens: &'a [usize],
    text: impl Fn(usize) -> &'a str + Copy,
    separator: &str,
) -> Vec<&'a [usize]> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (position, index) in tokens.iter().copied().enumerate() {
        match text(index) {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            value if value == separator && depth == 0 => {
                if start < position {
                    result.push(&tokens[start..position]);
                }
                start = position + 1;
            }
            _ => {}
        }
    }
    if start < tokens.len() {
        result.push(&tokens[start..]);
    }
    result
}

fn split_top_level_with_voice<'a>(
    tokens: &'a [usize],
    text: impl Fn(usize) -> &'a str + Copy,
    kind: impl Fn(usize) -> NativeTokenKind + Copy,
) -> Vec<&'a [usize]> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (position, index) in tokens.iter().copied().enumerate() {
        match text(index) {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            "," if depth == 0 => {
                let next = tokens.get(position + 1).copied();
                let after_next = tokens.get(position + 2).copied();
                let next_is_voice = next.is_some_and(|next| {
                    kind(next) == NativeTokenKind::Identifier
                        && after_next.is_none_or(|after| text(after) == ",")
                });
                if next_is_voice && starts_with_text_statement(&tokens[start..position], text, kind)
                {
                    continue;
                }
                if start < position {
                    result.push(&tokens[start..position]);
                }
                start = position + 1;
            }
            _ => {}
        }
    }
    if start < tokens.len() {
        result.push(&tokens[start..]);
    }
    result
}

fn starts_with_text_statement<'a>(
    tokens: &[usize],
    text: impl Fn(usize) -> &'a str,
    kind: impl Fn(usize) -> NativeTokenKind,
) -> bool {
    let mut start = 0;
    let mut depth = 0usize;
    for (position, index) in tokens.iter().copied().enumerate() {
        match text(index) {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            ":" if depth == 0 => start = position + 1,
            _ => {}
        }
    }
    if tokens.get(start).is_some_and(|index| text(*index) == "@") {
        start += 2;
    }
    tokens
        .get(start)
        .is_some_and(|index| kind(*index) == NativeTokenKind::String)
}

fn find_top_level<'a>(
    tokens: &'a [usize],
    text: impl Fn(usize) -> &'a str,
    needle: &str,
) -> Option<usize> {
    let mut depth = 0usize;
    for (position, index) in tokens.iter().copied().enumerate() {
        match text(index) {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            value if value == needle && depth == 0 => return Some(position),
            _ => {}
        }
    }
    None
}

fn matching_delimiter<'a>(
    tokens: &'a [usize],
    open: usize,
    text: impl Fn(usize) -> &'a str,
    opening: &str,
    closing: &str,
) -> Option<usize> {
    let mut depth = 0usize;
    for (position, index) in tokens.iter().copied().enumerate().skip(open) {
        match text(index) {
            value if value == opening => depth += 1,
            value if value == closing => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(position);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_assignment(value: &str) -> bool {
    matches!(value, "=" | "+=" | "-=" | "*=" | "/=" | "%=")
}

fn is_native_command(name: &str) -> bool {
    matches!(
        name,
        "goto"
            | "call"
            | "wait"
            | "background"
            | "sprite"
            | "hide"
            | "move"
            | "bgm"
            | "se"
            | "video"
            | "pop"
    )
}

fn valid_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_scene_blocks_in_source_order() {
        let source = r#"scene start {
  @intro "Hello",
  rin: "Hi",
  background(day),
  let score = 0,
  score += 1,
  return
}"#;
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        assert_eq!(
            blocks.iter().map(|block| &block.kind).collect::<Vec<_>>(),
            [
                &BlockKind::Narration,
                &BlockKind::Dialogue {
                    speaker: "rin".into()
                },
                &BlockKind::Command,
                &BlockKind::Declaration,
                &BlockKind::Assignment,
                &BlockKind::Control,
            ]
        );
        assert_eq!(blocks[0].stable_id.as_deref(), Some("intro"));
        assert_eq!(
            source.get(blocks[0].text_range.clone().unwrap()),
            Some("Hello")
        );
        assert!(
            blocks
                .windows(2)
                .all(|pair| pair[0].source_range.start < pair[1].source_range.start)
        );
    }

    #[test]
    fn projects_nested_flow_with_lightweight_depth() {
        let source = r#"scene start {
  if (ready) { "Go" } else if (later) { rin: "Soon" } else { "Wait" },
  loop { break },
  choice("Next?") {
    @take "Take": { rin: "Mine", return },
    "Leave": return
  }
}"#;
        let blocks = &EiyashouProjection::parse(source).scenes[0].blocks;
        assert!(
            blocks
                .iter()
                .any(|block| block.kind == BlockKind::Conditional)
        );
        assert!(blocks.iter().any(|block| block.kind == BlockKind::Else));
        assert!(blocks.iter().any(|block| block.kind == BlockKind::ElseIf));
        assert!(blocks.iter().any(|block| block.kind == BlockKind::Loop));
        assert!(blocks.iter().any(|block| block.kind == BlockKind::Choice));
        assert_eq!(
            blocks
                .iter()
                .filter(|block| block.kind == BlockKind::ChoiceOption)
                .count(),
            2
        );
        assert!(blocks.iter().any(|block| block.depth == 2));
    }

    #[test]
    fn unknown_and_dynamic_text_fail_closed_in_place() {
        let source = r#"scene start { "${name}", ?, "after" }"#;
        let blocks = &EiyashouProjection::parse(source).scenes[0].blocks;
        assert!(blocks[0].read_only);
        assert_eq!(blocks[1].kind, BlockKind::Unsupported);
        assert!(blocks[1].read_only);
        assert_eq!(blocks[2].kind, BlockKind::Narration);
        assert!(blocks[1].source_range.start < blocks[2].source_range.start);
    }

    #[test]
    fn copies_and_deletes_complete_source_nodes() {
        let source = "scene start {\n  @a \"one\",\n  background(day),\n  \"three\"\n}\n";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        let selected = HashSet::from([blocks[0].source_range.start, blocks[1].source_range.start]);
        assert_eq!(
            projection.copy_blocks(source, &selected).unwrap(),
            "@a \"one\",\nbackground(day)"
        );
        assert_eq!(
            projection.delete_blocks(source, &selected).unwrap(),
            "scene start {\n  \"three\"\n}\n"
        );
    }

    #[test]
    fn moves_adjacent_siblings_as_one_source_edit() {
        let source = "scene start {\n  \"one\",\n  background(day),\n  wait(1s)\n}\n";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        let selected = HashSet::from([blocks[1].source_range.start]);
        assert_eq!(
            projection
                .move_blocks(source, &selected, MoveDirection::Up)
                .unwrap(),
            "scene start {\n  background(day),\n  \"one\",\n  wait(1s)\n}\n"
        );
        assert_eq!(
            projection
                .move_blocks(source, &selected, MoveDirection::Down)
                .unwrap(),
            "scene start {\n  \"one\",\n  wait(1s),\n  background(day)\n}\n"
        );
    }

    #[test]
    fn drops_blocks_without_intermediate_source_rewrites() {
        let source = "scene start {\n  \"one\",\n  background(day),\n  wait(1s),\n  return\n}\n";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        let selected = HashSet::from([blocks[0].source_range.start]);
        assert_eq!(
            projection
                .move_blocks_to(source, &selected, blocks[2].source_range.start)
                .unwrap(),
            "scene start {\n  background(day),\n  \"one\",\n  wait(1s),\n  return\n}\n"
        );
    }

    #[test]
    fn moves_discrete_siblings_as_one_group_in_source_order() {
        let source = "scene start {\n  \"a\",\n  \"b\",\n  \"c\",\n  \"d\",\n  \"e\"\n}\n";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        let selected = HashSet::from([blocks[1].source_range.start, blocks[3].source_range.start]);
        assert_eq!(
            projection
                .move_blocks(source, &selected, MoveDirection::Up)
                .unwrap(),
            "scene start {\n  \"b\",\n  \"d\",\n  \"a\",\n  \"c\",\n  \"e\"\n}\n"
        );
        assert_eq!(
            projection
                .move_blocks_to(source, &selected, blocks[4].source_range.start)
                .unwrap(),
            "scene start {\n  \"a\",\n  \"c\",\n  \"b\",\n  \"d\",\n  \"e\"\n}\n"
        );
    }

    #[test]
    fn branch_headers_cannot_move_independently() {
        let source = r#"scene start {
  if (ready) { "Go" } else if (later) { "Soon" } else { "Wait" },
  "after"
}"#;
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        let else_if = blocks
            .iter()
            .find(|block| block.kind == BlockKind::ElseIf)
            .unwrap();
        let selected = HashSet::from([else_if.source_range.start]);
        assert_eq!(
            projection.move_blocks(source, &selected, MoveDirection::Up),
            Err(BlockEditError::NoMoveTarget)
        );
        assert_eq!(
            projection.move_blocks_to(source, &selected, blocks[0].source_range.start),
            Err(BlockEditError::NoMoveTarget)
        );
    }

    #[test]
    fn inserts_after_a_block_without_reformatting_neighbors() {
        let source = "scene start {\n  \"one\",\n  wait(1s)\n}\n";
        let projection = EiyashouProjection::parse(source);
        let first = projection.scenes[0].blocks[0].source_range.start;
        let (edited, range) = projection
            .insert_block_after(source, first, "\"two\"")
            .unwrap();
        assert_eq!(
            edited,
            "scene start {\n  \"one\",\n  \"two\",\n  wait(1s)\n}\n"
        );
        assert_eq!(edited.get(range), Some("\"two\""));
    }

    #[test]
    fn keeps_inline_voice_with_its_text_block() {
        let source = r#"scene start {
  @opening "Good morning", opening_hello,
  rin: "Let's go", rin_01,
  wait(500ms)
}"#;
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].kind, BlockKind::Narration);
        assert!(blocks[0].summary.ends_with("opening_hello"));
        assert_eq!(
            blocks[1].kind,
            BlockKind::Dialogue {
                speaker: "rin".into()
            }
        );
        assert!(blocks[1].summary.ends_with("rin_01"));
        assert_eq!(blocks[2].kind, BlockKind::Command);
    }

    #[test]
    fn inserts_the_first_block_in_an_empty_scene() {
        let source = "scene empty {\n}\n";
        let projection = EiyashouProjection::parse(source);
        let scene_start = projection.scenes[0].source_range.start;
        let (edited, range) = projection
            .insert_block_in_scene(source, scene_start, "\"first\"")
            .unwrap();
        assert_eq!(edited, "scene empty {\n  \"first\"\n}\n");
        assert_eq!(edited.get(range), Some("\"first\""));
    }

    #[test]
    fn supported_list_mutation_is_a_block_not_unknown_source() {
        let source = "scene start { let items = [1], items.append(2), items.clear() }";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        assert_eq!(blocks[1].kind, BlockKind::Command);
        assert_eq!(blocks[2].kind, BlockKind::Command);
        assert!(!blocks[1].read_only);
    }

    #[test]
    fn unknown_call_remains_read_only_at_its_source_position() {
        let source = "scene start { wait(100ms), mystery(1), \"next\" }";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        assert_eq!(blocks[0].kind, BlockKind::Command);
        assert_eq!(blocks[1].kind, BlockKind::Unsupported);
        assert!(blocks[1].read_only);
        assert!(blocks[0].source_range.start < blocks[1].source_range.start);
        assert!(blocks[1].source_range.start < blocks[2].source_range.start);
    }

    #[test]
    fn inspector_fields_keep_exact_source_ranges() {
        let source = "scene start { sprite(rin_slot, rin, position: center, transition: fade(300ms)), if (ready) { \"ok\" } }";
        let projection = EiyashouProjection::parse(source);
        let sprite = &projection.scenes[0].blocks[0];
        let fields = projection
            .source_fields(source, sprite.source_range.start)
            .unwrap();
        assert_eq!(
            fields[..4]
                .iter()
                .map(|field| field.key.as_str())
                .collect::<Vec<_>>(),
            ["0", "1", "position", "transition"]
        );
        assert_eq!(source.get(fields[3].range.clone()), Some("fade(300ms)"));
        assert_eq!(fields[4].key, "z");
        assert_eq!(fields[4].insertion.as_deref(), Some(", z: "));
        let mut edited = source.to_owned();
        edited.replace_range(fields[4].range.clone(), ", z: 2");
        let edited_projection = EiyashouProjection::parse(&edited);
        assert!(edited_projection.read_only.is_empty());
        assert_eq!(
            edited_projection.scenes[0].blocks[0].kind,
            BlockKind::Command
        );
        let conditional = projection.scenes[0]
            .blocks
            .iter()
            .find(|block| block.kind == BlockKind::Conditional)
            .unwrap();
        let fields = projection
            .source_fields(source, conditional.source_range.start)
            .unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].value, "ready");
    }

    #[test]
    fn choice_prompt_can_be_added_without_rebuilding_child_blocks() {
        let source = "scene start { choice { \"Go\": goto(start) } }";
        let projection = EiyashouProjection::parse(source);
        let choice = &projection.scenes[0].blocks[0];
        let field = &projection
            .source_fields(source, choice.source_range.start)
            .unwrap()[0];
        assert_eq!(field.key, "prompt");
        let mut edited = source.to_owned();
        edited.replace_range(field.range.clone(), "(\"Next?\")");
        assert_eq!(
            edited,
            "scene start { choice(\"Next?\") { \"Go\": goto(start) } }"
        );
        assert!(
            EiyashouProjection::parse(&edited).scenes[0]
                .blocks
                .iter()
                .any(|block| block.kind == BlockKind::ChoiceOption)
        );
    }

    #[test]
    fn inspector_choice_prompt_keeps_braces_inside_quoted_text() {
        let source = r#"scene start { choice("What {now}?") { "Go": goto(start) } }"#;
        let projection = EiyashouProjection::parse(source);
        let choice = &projection.scenes[0].blocks[0];
        let fields = projection
            .source_fields(source, choice.source_range.start)
            .unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].value, "What {now}?");
    }

    #[test]
    fn inspector_text_fields_decode_source_escapes_for_editing() {
        assert_eq!(decode_source_string(r#"A\n\"B\""#), Some("A\n\"B\"".into()));
        assert_eq!(
            decode_source_string("literal /${value}"),
            Some("literal ${value}".into())
        );
    }

    #[test]
    fn text_metadata_changes_speaker_voice_and_stable_id_as_one_node() {
        let source = "scene start {\n  @old \"hello\", old_voice\n}\n";
        let projection = EiyashouProjection::parse(source);
        let block = &projection.scenes[0].blocks[0];
        assert_eq!(
            projection.text_block_metadata(source, block.source_range.start),
            Some(TextBlockMetadata {
                speaker: None,
                voice: Some("old_voice".into()),
                stable_id: Some("old".into()),
            })
        );
        assert_eq!(
            projection
                .replace_text_block_metadata(
                    source,
                    block.source_range.start,
                    &TextBlockMetadata {
                        speaker: Some("rin".into()),
                        voice: Some("rin_01".into()),
                        stable_id: Some("greeting".into()),
                    },
                )
                .unwrap(),
            "scene start {\n  @greeting rin: \"hello\", rin_01\n}\n"
        );
    }
}
