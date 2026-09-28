use std::ops::Range;
use std::{collections::HashSet, fmt};

use keine_loader::{
    Diagnostic, NativeToken, NativeTokenKind, is_native_dotted_command,
    is_native_structured_command, native_expanded_fields, parse_native_document,
};

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
    /// Original statement, before any associated Text cleanup commands.
    pub statement_range: Range<usize>,
    pub disabled: bool,
    pub lifetime_owner: Option<usize>,
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
    InvalidDisabledBlock,
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
            Self::InvalidDisabledBlock => {
                formatter.write_str("block comment delimiters prevent lossless disabling")
            }
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
        self.source_fields_for_block(source, block)
    }

    pub fn source_fields_for_block(
        &self,
        source: &str,
        block: &BlockCard,
    ) -> Option<Vec<SourceField>> {
        if block.disabled {
            return None;
        }
        let node = source.get(block.statement_range.clone())?;
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
                    if command == "sprite.focus.configure" {
                        fields = fields
                            .into_iter()
                            .flat_map(|field| {
                                if matches!(field.key.as_str(), "speaking" | "others" | "narration")
                                {
                                    portrait_style_fields(source, &field)
                                        .unwrap_or_else(|| vec![field])
                                } else {
                                    vec![field]
                                }
                            })
                            .collect();
                    }
                    let optional = command_argument_names(command);
                    let insertion_point = argument_start + arguments.trim_end().len();
                    let has_arguments = !arguments.trim().is_empty();
                    for name in optional {
                        if fields.iter().any(|field| {
                            field.key == name || field.key.starts_with(&format!("{name}."))
                        }) {
                            continue;
                        }
                        fields.push(SourceField {
                            key: name.to_owned(),
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
                let mut blocks = parser.scene_blocks(scene.range.clone());
                project_disabled_comments(source, &document.tokens, &scene.range, &mut blocks);
                associate_text_lifetime(source, &mut blocks);
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

    /// Toggle a whole source node. The wrapper is a normal nested block comment;
    /// the runtime grammar and enabled text remain unchanged.
    pub fn toggle_disabled(&self, source: &str, start: usize) -> Result<String, BlockEditError> {
        let block = self
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .find(|block| block.source_range.start == start)
            .ok_or(BlockEditError::MissingSelection)?;
        if matches!(
            block.kind,
            BlockKind::Narration | BlockKind::Dialogue { .. } | BlockKind::Else | BlockKind::ElseIf
        ) {
            return Err(BlockEditError::NotEditableText);
        }
        let node = source
            .get(block.source_range.clone())
            .ok_or(BlockEditError::StaleRange)?;
        let replacement = if block.disabled {
            disabled_body(node)
                .ok_or(BlockEditError::StaleRange)?
                .to_owned()
        } else {
            let wrapped = format!("/* disabled\n{node}\n*/");
            // A comment delimiter inside a string may terminate the wrapper.
            // Require one complete lexer token, rather than escaping authored text.
            let prefix = "scene __disabled {} ";
            let inventory = parse_native_document(&format!("{prefix}{wrapped}"));
            if !inventory.tokens.iter().any(|token| {
                token.kind == NativeTokenKind::Comment
                    && token.range == (prefix.len()..prefix.len() + wrapped.len())
            }) || !inventory.diagnostics.is_empty()
            {
                return Err(BlockEditError::InvalidDisabledBlock);
            }
            wrapped
        };
        let mut edited = source.to_owned();
        edited.replace_range(block.source_range.clone(), &replacement);
        if block.disabled {
            let inventory = parse_native_document(source);
            let before = inventory.tokens.iter().rfind(|token| {
                token.range.end <= block.source_range.start
                    && !matches!(
                        token.kind,
                        NativeTokenKind::Whitespace | NativeTokenKind::Comment
                    )
            });
            let after = inventory.tokens.iter().find(|token| {
                token.range.start >= block.source_range.end
                    && !matches!(
                        token.kind,
                        NativeTokenKind::Whitespace | NativeTokenKind::Comment
                    )
            });
            let left = before
                .and_then(|token| source.get(token.range.clone()))
                .is_some_and(|text| !matches!(text, "{" | ","));
            let right = after
                .and_then(|token| source.get(token.range.clone()))
                .is_some_and(|text| !matches!(text, "}" | ","));
            if right {
                edited.insert(block.source_range.start + replacement.len(), ',');
            }
            if left {
                edited.insert(block.source_range.start, ',');
            }
        }
        let edited = remove_empty_statement_separators(&edited);
        if Self::parse(&edited).read_only.len() > self.read_only.len() {
            return Err(BlockEditError::StaleRange);
        }
        Ok(edited)
    }

    pub fn text_lifetime(&self, source: &str, start: usize) -> Option<TextLifetime> {
        let block = self
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .find(|block| {
                block.source_range.start == start && block.text_range.is_some() && !block.read_only
            })?;
        let mut lifetime = TextLifetime::default();
        for command in self
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .filter(|command| command.lifetime_owner == Some(block.source_range.start))
        {
            match command.summary.split('(').next().map(str::trim) {
                Some("text.box") => lifetime.text_box = Some(command.statement_range.clone()),
                Some("hide") => {
                    lifetime.hide = Some(command.statement_range.clone());
                    let fields = self.source_fields_for_block(source, command)?;
                    lifetime.target = fields.iter().find(|field| field.key == "0")?.value.clone();
                    lifetime.transition = fields
                        .iter()
                        .find(|field| field.key == "transition" && field.insertion.is_none())
                        .map(|field| field.value.clone())
                        .unwrap_or_default();
                }
                _ => {}
            }
        }
        Some(lifetime)
    }

    pub fn replace_text_lifetime(
        &self,
        source: &str,
        start: usize,
        keep_dialogue: bool,
        target: &str,
        transition: &str,
    ) -> Result<String, BlockEditError> {
        let block = self
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .find(|block| {
                block.source_range.start == start && block.text_range.is_some() && !block.read_only
            })
            .ok_or(BlockEditError::NotEditableText)?;
        let lifetime = self
            .text_lifetime(source, start)
            .ok_or(BlockEditError::NotEditableText)?;
        let mut edits = Vec::new();
        if let Some(range) = lifetime.text_box {
            if keep_dialogue {
                edits.push((range, String::new()));
            }
        } else if !keep_dialogue {
            edits.push((
                block.source_range.end..block.source_range.end,
                ", text.box(visible: false, auto: true)".into(),
            ));
        }
        let hide = if target.trim().is_empty() {
            None
        } else {
            Some(format!(
                "hide({}{})",
                target.trim(),
                if transition.trim().is_empty() {
                    String::new()
                } else {
                    format!(", transition: {}", transition.trim())
                }
            ))
        };
        match (lifetime.hide, hide) {
            (Some(range), Some(hide))
                if lifetime.target != target.trim() || lifetime.transition != transition.trim() =>
            {
                edits.push((range, hide))
            }
            (Some(range), None) => edits.push((range, String::new())),
            (None, Some(hide)) => edits.push((
                block.source_range.end..block.source_range.end,
                format!(", {hide}"),
            )),
            _ => {}
        }
        edits.sort_by_key(|(range, _)| (range.start, range.end));
        let mut edited = source.to_owned();
        for (range, replacement) in edits.into_iter().rev() {
            edited.replace_range(range, &replacement);
        }
        let edited = remove_empty_statement_separators(&edited);
        if Self::parse(&edited).read_only.len() > self.read_only.len() {
            return Err(BlockEditError::StaleRange);
        }
        Ok(edited)
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

    /// Applies a group of named argument changes in one bounded source edit.
    /// None removes the argument; absent arguments are only inserted explicitly.
    pub fn replace_block_fields(
        &self,
        source: &str,
        start: usize,
        updates: &[(String, Option<String>)],
    ) -> Result<String, BlockEditError> {
        let block = self
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .find(|block| block.source_range.start == start && !block.read_only)
            .ok_or(BlockEditError::MissingSelection)?;
        let fields = self
            .source_fields_for_block(source, block)
            .ok_or(BlockEditError::StaleRange)?;
        let node = source
            .get(block.source_range.clone())
            .ok_or(BlockEditError::StaleRange)?;
        let open = node.find('(').ok_or(BlockEditError::StaleRange)?;
        let close = matching_parenthesis(node, open).ok_or(BlockEditError::StaleRange)?;
        let body_start = start + open + 1;
        let body = &node[open + 1..close];
        let mut kept = Vec::new();
        for span in split_source_ranges(body, ',') {
            let absolute = body_start + span.start..body_start + span.end;
            let update = updates.iter().find_map(|(name, value)| {
                fields
                    .iter()
                    .find(|field| {
                        &field.key == name
                            && field.insertion.is_none()
                            && field.range.start >= absolute.start
                            && field.range.end <= absolute.end
                    })
                    .map(|field| (field, value))
            });
            let mut argument = body[span.clone()].to_owned();
            if let Some((field, value)) = update {
                let Some(value) = value else {
                    continue;
                };
                // This API takes source expressions. Quoted values are deliberately
                // not reconstructed: callers must use the individual string editor.
                if field.quoted {
                    return Err(BlockEditError::StaleRange);
                }
                argument.replace_range(
                    field.range.start - absolute.start..field.range.end - absolute.start,
                    value,
                );
            }
            if !argument.trim().is_empty() {
                kept.push(argument);
            }
        }
        for (name, value) in updates {
            let field = fields
                .iter()
                .find(|field| &field.key == name)
                .ok_or(BlockEditError::StaleRange)?;
            if name.contains('.') || name.chars().all(|character| character.is_ascii_digit()) {
                return Err(BlockEditError::StaleRange);
            }
            if field.insertion.is_some()
                && let Some(value) = value
            {
                kept.push(format!(" {name}: {value}"));
            }
        }
        let mut edited = source.to_owned();
        edited.replace_range(body_start..start + close, &kept.join(","));
        Ok(edited)
    }

    /// Duplicates keep statement bytes but receive new implicit source IDs.
    /// Explicit IDs belong to the original text/choice; copying them would make
    /// the entire project invalid when the duplicate is saved.
    pub fn duplicate_blocks(
        &self,
        source: &str,
        selected: &HashSet<usize>,
    ) -> Result<(String, Range<usize>), BlockEditError> {
        let after = self
            .selected_ranges(selected)?
            .last()
            .ok_or(BlockEditError::MissingSelection)?
            .start;
        let mut fragment = self.copy_blocks(source, selected)?;
        const PREFIX: &str = "scene __duplicate { ";
        let wrapped = format!("{PREFIX}{fragment} }}");
        let parsed = Self::parse(&wrapped);
        let mut annotations = parsed
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .filter_map(|block| {
                block
                    .stable_id
                    .as_ref()
                    .map(|id| (block.source_range.start, id))
            })
            .map(|(start, id)| {
                let start = start
                    .checked_sub(PREFIX.len())
                    .ok_or(BlockEditError::StaleRange)?;
                let mut end = start + 1 + id.len();
                if fragment.get(start..end) != Some(format!("@{id}").as_str()) {
                    return Err(BlockEditError::StaleRange);
                }
                while fragment
                    .as_bytes()
                    .get(end)
                    .is_some_and(|byte| *byte == b' ' || *byte == b'\t')
                {
                    end += 1;
                }
                Ok(start..end)
            })
            .collect::<Result<Vec<_>, _>>()?;
        annotations.sort_by_key(|range| range.start);
        for range in annotations.into_iter().rev() {
            fragment.replace_range(range, "");
        }
        self.insert_block_after(source, after, &fragment)
    }

    pub fn delete_blocks(
        &self,
        source: &str,
        selected: &HashSet<usize>,
    ) -> Result<String, BlockEditError> {
        let ranges = self.selected_ranges(selected)?;
        let mut edited = source.to_owned();
        for range in ranges.into_iter().rev() {
            let disabled = self
                .scenes
                .iter()
                .flat_map(|scene| &scene.blocks)
                .any(|block| block.disabled && block.source_range == range);
            let range = if disabled {
                range
            } else {
                deletion_range(source, range)
            };
            if edited.get(range.clone()).is_none() {
                return Err(BlockEditError::StaleRange);
            }
            edited.replace_range(range, "");
        }
        Ok(if source.contains("/* disabled\n") {
            remove_empty_statement_separators(&edited)
        } else {
            edited
        })
    }

    /// Resolve the live text range, including nested speech, before deleting the
    /// owning node and its associated Text Ending. Stale/nonempty rows fail closed.
    pub fn delete_empty_text(&self, source: &str, text_start: usize) -> Option<String> {
        let block = self
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .find(|block| {
                !block.read_only
                    && !block.disabled
                    && matches!(
                        block.kind,
                        BlockKind::Narration | BlockKind::Dialogue { .. }
                    )
                    && block
                        .text_range
                        .as_ref()
                        .is_some_and(|range| range.start == text_start && range.is_empty())
            })?;
        self.delete_blocks(source, &HashSet::from([block.source_range.start]))
            .ok()
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
        after: bool,
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
            .ok_or(BlockEditError::NoMoveTarget)?
            + usize::from(after);
        order.splice(insertion..insertion, selected_order);
        replace_block_texts(source, &siblings, &siblings, &order)
    }

    pub fn insert_block_before(
        &self,
        source: &str,
        before_start: usize,
        statement: &str,
    ) -> Result<(String, Range<usize>), BlockEditError> {
        let block = self
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .find(|block| block.source_range.start == before_start)
            .ok_or(BlockEditError::MissingSelection)?;
        if matches!(block.kind, BlockKind::ElseIf | BlockKind::Else) {
            return Err(BlockEditError::NoMoveTarget);
        }
        if source.contains("/* disabled\n") || statement.contains("/* disabled\n") {
            return insert_with_comments(source, block.source_range.start, statement);
        }
        let start = block.source_range.start;
        let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
        let prefix = &source[line_start..start];
        let (insertion, text, offset) = if prefix
            .chars()
            .all(|character| matches!(character, ' ' | '\t'))
        {
            (line_start, format!("{prefix}{statement},\n"), prefix.len())
        } else {
            (start, format!("{statement}, "), 0)
        };
        let mut edited = source.to_owned();
        edited.insert_str(insertion, &text);
        let start = insertion + offset;
        Ok((edited, start..start + statement.len()))
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
        if source.contains("/* disabled\n") || statement.contains("/* disabled\n") {
            return insert_with_comments(source, block.source_range.end, statement);
        }
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
            .get(text_range.end.saturating_add(1)..block.statement_range.end)
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
        edited.replace_range(block.statement_range.clone(), &replacement);
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextLifetime {
    pub text_box: Option<Range<usize>>,
    pub hide: Option<Range<usize>>,
    pub target: String,
    pub transition: String,
}

fn disabled_body(comment: &str) -> Option<&str> {
    let body = comment.strip_prefix("/* disabled")?;
    let body = body
        .strip_prefix("\r\n")
        .or_else(|| body.strip_prefix('\n'))?;
    Some(body.strip_suffix("*/")?.trim())
}

fn project_disabled_comments(
    source: &str,
    tokens: &[NativeToken],
    scene: &Range<usize>,
    blocks: &mut Vec<BlockCard>,
) {
    for token in tokens.iter().filter(|token| {
        token.kind == NativeTokenKind::Comment
            && token.range.start > scene.start
            && token.range.end < scene.end
    }) {
        let Some(body) = source.get(token.range.clone()).and_then(disabled_body) else {
            continue;
        };
        let wrapper = format!("scene __disabled {{ {body} }}");
        let inventory = parse_native_document(&wrapper);
        let parser = BlockProjectionParser::new(&wrapper, &inventory.tokens);
        let Some(inner_scene) = inventory.scenes.first() else {
            continue;
        };
        let Some(mut block) = parser
            .scene_blocks(inner_scene.range.clone())
            .into_iter()
            .next()
        else {
            continue;
        };
        let depth = blocks
            .iter()
            .filter(|parent| {
                parent.source_range.start < token.range.start
                    && parent.source_range.end > token.range.end
            })
            .map(|parent| parent.depth + 1)
            .max()
            .unwrap_or(0);
        block.source_range = token.range.clone();
        block.statement_range = token.range.clone();
        block.text_range = None;
        block.stable_id = None;
        block.disabled = true;
        block.read_only = true;
        block.depth = depth;
        block.line = source[..token.range.start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        block.column = source[..token.range.start]
            .rsplit_once('\n')
            .map_or(token.range.start, |(_, tail)| tail.chars().count());
        blocks.push(block);
    }
    blocks.sort_by_key(|block| block.source_range.start);
}

fn associate_text_lifetime(source: &str, blocks: &mut [BlockCard]) {
    for index in 0..blocks.len() {
        if blocks[index].disabled
            || !matches!(
                blocks[index].kind,
                BlockKind::Narration | BlockKind::Dialogue { .. }
            )
        {
            continue;
        }
        let mut seen_box = false;
        let mut seen_hide = false;
        for next in index + 1..blocks.len() {
            if blocks[next].disabled || blocks[next].depth != blocks[index].depth {
                break;
            }
            let node = source
                .get(blocks[next].statement_range.clone())
                .unwrap_or("");
            let command = node.split('(').next().unwrap_or("").trim();
            let recognized = match command {
                "text.box" if !seen_box => {
                    let projection = EiyashouProjection::default();
                    let fields = projection
                        .source_fields_for_block(source, &blocks[next])
                        .unwrap_or_default();
                    let matches = fields
                        .iter()
                        .any(|field| field.key == "visible" && field.value == "false")
                        && fields
                            .iter()
                            .any(|field| field.key == "auto" && field.value == "true");
                    if matches {
                        seen_box = true;
                    }
                    matches
                }
                "hide" if !seen_hide => {
                    seen_hide = true;
                    true
                }
                _ => false,
            };
            if !recognized {
                break;
            }
            blocks[next].lifetime_owner = Some(blocks[index].source_range.start);
            blocks[next].depth += 1;
            blocks[index].source_range.end = blocks[next].source_range.end;
        }
    }
}

fn remove_empty_statement_separators(source: &str) -> String {
    let inventory = parse_native_document(source);
    let tokens = inventory
        .tokens
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                NativeTokenKind::Whitespace | NativeTokenKind::Comment
            )
        })
        .collect::<Vec<_>>();
    let mut edited = source.to_owned();
    for (index, token) in tokens.iter().enumerate().rev() {
        if source.get(token.range.clone()) != Some(",") {
            continue;
        }
        let previous = index
            .checked_sub(1)
            .and_then(|index| tokens.get(index))
            .and_then(|token| source.get(token.range.clone()));
        let next = tokens
            .get(index + 1)
            .and_then(|token| source.get(token.range.clone()));
        if matches!(previous, Some("{" | ",")) || next == Some("}") {
            edited.replace_range(token.range.clone(), "");
        }
    }
    edited
}

fn insert_with_comments(
    source: &str,
    at: usize,
    statement: &str,
) -> Result<(String, Range<usize>), BlockEditError> {
    let prefix = "scene __fragment { ";
    let wrapped = remove_empty_statement_separators(&format!("{prefix}{statement} }}"));
    let statement = &wrapped[prefix.len()..wrapped.len() - 2];
    let inventory = parse_native_document(source);
    let fragment = parse_native_document(&format!("{prefix}{statement} }}"));
    let active = fragment.tokens.iter().any(|token| {
        token.range.start >= prefix.len()
            && token.range.end <= prefix.len() + statement.len()
            && !matches!(
                token.kind,
                NativeTokenKind::Whitespace | NativeTokenKind::Comment
            )
    });
    let before = inventory.tokens.iter().rfind(|token| {
        token.range.end <= at
            && !matches!(
                token.kind,
                NativeTokenKind::Whitespace | NativeTokenKind::Comment
            )
    });
    let after = inventory.tokens.iter().find(|token| {
        token.range.start >= at
            && !matches!(
                token.kind,
                NativeTokenKind::Whitespace | NativeTokenKind::Comment
            )
    });
    let left = active
        && before
            .and_then(|token| source.get(token.range.clone()))
            .is_some_and(|text| !matches!(text, "{" | ","));
    let right = active
        && after
            .and_then(|token| source.get(token.range.clone()))
            .is_some_and(|text| !matches!(text, "}" | ","));
    let text = format!(
        "{} {statement}{} ",
        if left { "," } else { "" },
        if right { "," } else { "" }
    );
    let start = at + usize::from(left) + 1;
    let mut edited = source.to_owned();
    edited.insert_str(at, &text);
    Ok((edited, start..start + statement.len()))
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
    if slots.iter().any(|slot| slot.disabled) {
        let mut replacement = String::new();
        let mut active = false;
        for (position, index) in order.iter().enumerate() {
            let block = siblings.get(*index).ok_or(BlockEditError::StaleRange)?;
            if position > 0 {
                let gap = &source
                    [slots[position - 1].source_range.end..slots[position].source_range.start];
                if !block.disabled && active {
                    replacement.push(',');
                }
                let inventory = parse_native_document(gap);
                let mut trivia = gap.to_owned();
                for token in inventory.tokens.iter().rev().filter(|token| {
                    token.kind == NativeTokenKind::Punctuation
                        && gap.get(token.range.clone()) == Some(",")
                }) {
                    trivia.replace_range(token.range.clone(), "");
                }
                replacement.push_str(&trivia);
            }
            replacement.push_str(
                source
                    .get(block.source_range.clone())
                    .ok_or(BlockEditError::StaleRange)?,
            );
            active |= !block.disabled;
        }
        let mut edited = source.to_owned();
        edited.replace_range(
            slots
                .first()
                .ok_or(BlockEditError::MissingSelection)?
                .source_range
                .start
                ..slots
                    .last()
                    .ok_or(BlockEditError::MissingSelection)?
                    .source_range
                    .end,
            &replacement,
        );
        return Ok(edited);
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
        let mut command_path = name.to_owned();
        let mut command_end = head + 1;
        while tokens
            .get(command_end)
            .is_some_and(|index| self.text(*index) == ".")
            && tokens
                .get(command_end + 1)
                .is_some_and(|index| self.tokens[*index].kind == NativeTokenKind::Identifier)
        {
            command_path.push('.');
            command_path.push_str(self.text(tokens[command_end + 1]));
            command_end += 2;
        }
        let dotted_command = command_end > head + 1
            && tokens
                .get(command_end)
                .is_some_and(|index| self.text(*index) == "(")
            && is_native_dotted_command(&command_path);
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
            _ if dotted_command => {
                blocks.push(self.block(BlockKind::Command, source_range, None, depth, None, false));
                if is_native_structured_command(&command_path)
                    && let Some(open) = (command_end..tokens.len())
                        .find(|position| self.text(tokens[*position]) == "{")
                    && let Some(close) =
                        matching_delimiter(tokens, open, |index| self.text(index), "{", "}")
                {
                    self.project_structured_rows(&tokens[open + 1..close], depth + 1, blocks);
                }
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

    fn project_structured_rows(&self, tokens: &[usize], depth: usize, blocks: &mut Vec<BlockCard>) {
        for row in split_top_level(tokens, |index| self.text(index), ",") {
            let (Some(first), Some(last)) = (row.first(), row.last()) else {
                continue;
            };
            blocks.push(self.block(
                BlockKind::Command,
                self.tokens[*first].range.start..self.tokens[*last].range.end,
                None,
                depth,
                None,
                false,
            ));
            if let Some(open) = row.iter().position(|index| self.text(*index) == "{")
                && let Some(close) =
                    matching_delimiter(row, open, |index| self.text(index), "{", "}")
            {
                self.project_structured_rows(&row[open + 1..close], depth + 1, blocks);
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
            statement_range: source_range.clone(),
            source_range,
            disabled: false,
            lifetime_owner: None,
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

fn portrait_style_fields(source: &str, parent: &SourceField) -> Option<Vec<SourceField>> {
    const STYLE_FIELDS: [&str; 6] = [
        "scale",
        "brightness",
        "saturation",
        "contrast",
        "blur",
        "alpha",
    ];
    let style = source.get(parent.range.clone())?;
    if !style.starts_with("style(") || matching_parenthesis(style, 5)? != style.len() - 1 {
        return None;
    }
    let body_start = parent.range.start + 6;
    let body = &style[6..style.len() - 1];
    let mut fields = Vec::new();
    for span in split_source_ranges(body, ',') {
        let Some(full) =
            trimmed_source_range(source, body_start + span.start..body_start + span.end)
        else {
            if body.trim().is_empty() {
                break;
            }
            return None;
        };
        let raw = source.get(full.clone())?;
        let colon = top_level_position(raw, ':')?;
        let name = raw[..colon].trim();
        if !STYLE_FIELDS.contains(&name) {
            return None;
        }
        let range = trimmed_source_range(source, full.start + colon + 1..full.end)?;
        fields.push(SourceField {
            key: format!("{}.{}", parent.key, name),
            range: range.clone(),
            value: source.get(range)?.to_owned(),
            quoted: false,
            insertion: None,
            insertion_suffix: None,
        });
    }
    let insertion_point = parent.range.end - 1;
    let has_fields = !body.trim().is_empty();
    for name in STYLE_FIELDS {
        let key = format!("{}.{}", parent.key, name);
        if fields.iter().any(|field| field.key == key) {
            continue;
        }
        fields.push(SourceField {
            key,
            range: insertion_point..insertion_point,
            value: String::new(),
            quoted: false,
            insertion: Some(format!("{}{name}: ", if has_fields { ", " } else { "" })),
            insertion_suffix: None,
        });
    }
    Some(fields)
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

/// Shared named-argument inventory for Inspector and Text completions.
pub fn command_argument_names(command: &str) -> Vec<&str> {
    if command == "event.camera.patch" {
        let mut fields = vec!["time", "targets"];
        fields.extend(
            native_expanded_fields("camera.effect")
                .unwrap_or(&[])
                .iter()
                .copied()
                .filter(|field| !matches!(*field, "duration" | "easing" | "blocking" | "tween")),
        );
        fields
    } else {
        let known: &[&str] = match command {
            "text.box" => &["visible", "auto"],
            "text.retract" => &["source", "keep"],
            "text.float.configure" => &["id", "infinite"],
            "particle.hide" => &["duration"],
            "video.stop" => &["fade"],
            "gallery.unlock" => &["name"],
            "input.simple" => &["title", "button"],
            "camera.bind" | "camera.unbind" => &["distance"],
            "style" => &[
                "scale",
                "brightness",
                "saturation",
                "contrast",
                "blur",
                "alpha",
            ],
            "background" => &[
                "transition",
                "transform_x",
                "transform_y",
                "transform_alpha",
                "transform_scale_x",
                "transform_scale_y",
                "transform_rotation",
                "transform_blur",
                "transform_width",
                "transform_height",
            ],
            "hide" => &["transition"],
            "sprite" => &[
                "position",
                "anchor_offset",
                "y",
                "transition",
                "z",
                "blend",
                "layout",
                "layout_height",
                "layout_fit",
                "layout_x",
                "layout_y",
                "layout_anchor_x",
                "layout_anchor_y",
                "layout_width",
                "layout_canvas_width",
                "layout_canvas_height",
                "layout_rect_x",
                "layout_rect_y",
                "layout_rect_width",
                "layout_rect_height",
                "layout_height_ratio",
                "transform_x",
                "transform_y",
                "transform_alpha",
                "transform_scale_x",
                "transform_scale_y",
                "transform_rotation",
                "transform_blur",
                "transform_width",
                "transform_height",
            ],
            "move" => &["anchor_offset", "y", "duration", "easing", "blocking"],
            "bgm" => &["volume", "fade", "loop"],
            "se" => &["volume"],
            "video" => &["skippable"],
            "pop" => &["into"],
            "camera.move" => &[
                "x", "y", "alpha", "scale_x", "scale_y", "rotation", "blur", "width", "height",
                "duration", "easing", "blocking", "tween",
            ],
            "camera.shake" => &[
                "amplitude",
                "frequency",
                "amplitude_randomness",
                "frequency_randomness",
                "duration",
                "axis",
                "falloff",
                "blocking",
            ],
            "sprite.focus.configure" => &[
                "characters",
                "speaking",
                "others",
                "narration",
                "enabled",
                "duration",
                "easing",
            ],
            "sprite.offset" => &["x", "y", "duration", "easing"],
            "sprite.transform" | "background.transform" => &[
                "x", "y", "alpha", "scale_x", "scale_y", "rotation", "blur", "width", "height",
                "duration", "easing",
            ],
            "sprite.filter" => &["blur", "brightness", "contrast", "saturation"],
            "sprite.animate" => &["duration"],
            "sprite.transition" => &["enter", "exit", "duration"],
            "se.loop" | "vocal.play" => &["volume"],
            "video.play" => &["loop", "muted", "alpha", "skippable", "wait", "mode"],
            "screen.curtain.show" | "screen.curtain.hide" => &["color", "duration"],
            "text.float" => &[
                "x",
                "y",
                "font_size",
                "color",
                "fade_in",
                "hold",
                "fade_out",
                "blocking",
            ],
            "scene.parallax" => &[
                "amplitude_percent",
                "edge_ease_percent",
                "return_to_center_on_leave",
                "scale",
            ],
            "particle.show" => &["texture", "count", "wind", "gravity", "fade_in"],
            "ui.message" => &["title", "message", "confirm_text", "cancel_text", "result"],
            "text.intro" => &["hold"],
            "sprite.sequence" => &["fps", "loop"],
            "sprite.sequence.timed" => &["loop"],
            "sprite.select" => &["default"],
            "sprite.select.when" => &["default"],
            "sprite.keyframes" => &["repeat", "blocking"],
            "sprite.update" => &[
                "position",
                "anchor_offset",
                "y",
                "layout",
                "layout_height",
                "layout_fit",
                "layout_x",
                "layout_y",
                "layout_anchor_x",
                "layout_anchor_y",
                "layout_width",
                "layout_canvas_width",
                "layout_canvas_height",
                "layout_rect_x",
                "layout_rect_y",
                "layout_rect_width",
                "layout_rect_height",
                "layout_height_ratio",
                "scale",
                "duration",
                "easing",
                "blocking",
            ],
            "assets.loading" => &["mode", "lookahead", "blocking"],
            "input.request" => &[
                "type",
                "title",
                "description",
                "placeholder",
                "confirm_text",
                "required_text",
                "required",
                "min_length",
                "max_length",
                "min_value",
                "max_value",
                "step",
                "true_text",
                "false_text",
            ],
            "text.paragraph.style" => &[
                "typewriter_speed",
                "reveal_duration",
                "reveal_effect",
                "reveal_distance",
                "reveal_scale",
                "reveal_rotation",
                "reveal_blur",
            ],
            "camera.effect" | "camera.effect.v2" | "stage.mask.show" => {
                native_expanded_fields(command).unwrap_or(&[])
            }
            "stage.mask.hide" => &["duration", "blocking"],
            "stage.animate" => &[
                "duration",
                "repeat",
                "infinite",
                "playback_rate",
                "blocking",
            ],
            "track" => &["image", "muted"],
            "key" => &["time", "value", "easing"],
            "event.camera.shake" => &[
                "time",
                "amplitude",
                "frequency",
                "amplitude_randomness",
                "frequency_randomness",
                "duration",
                "axis",
                "falloff",
            ],
            "event.particle" => &[
                "time", "texture", "count", "wind", "gravity", "fade_in", "duration", "fade_out",
            ],
            "event.scene" => &[
                "time",
                "transition",
                "reset_camera",
                "fit",
                "x",
                "y",
                "anchor_x",
                "anchor_y",
                "width",
                "height",
            ],
            "event.audio" => &["time", "volume", "loop", "duration", "fade_in", "fade_out"],
            "layer" => &["distance", "x", "y"],
            _ => &[],
        };
        known.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_nodes_stay_valid_through_enable_reorder_copy_and_delete() {
        for command in [
            "camera.move(scene, x: 1)",
            "camera.shake(all, amplitude: 2, frequency: 3, duration: 1s)",
            "wait(1s)",
        ] {
            let source = "scene start { camera.move(scene, x: 1), camera.shake(all, amplitude: 2, frequency: 3, duration: 1s), wait(1s) }";
            let projection = EiyashouProjection::parse(source);
            let start = projection.scenes[0]
                .blocks
                .iter()
                .find(|block| source.get(block.source_range.clone()) == Some(command))
                .unwrap()
                .source_range
                .start;
            let disabled = projection.toggle_disabled(source, start).unwrap();
            let projection = EiyashouProjection::parse(&disabled);
            assert!(
                projection.read_only.is_empty(),
                "{disabled}: {:?}",
                projection.read_only
            );
            let card = projection.scenes[0]
                .blocks
                .iter()
                .find(|block| block.disabled)
                .unwrap();
            assert_eq!(
                disabled_body(&disabled[card.source_range.clone()]),
                Some(command)
            );
            let selected = HashSet::from([card.source_range.start]);
            let enabled = projection
                .toggle_disabled(&disabled, card.source_range.start)
                .unwrap();
            assert!(
                EiyashouProjection::parse(&enabled).read_only.is_empty(),
                "{enabled}"
            );
            assert_eq!(
                EiyashouProjection::parse(&enabled).scenes[0].blocks.len(),
                3
            );
            let deleted = projection.delete_blocks(&disabled, &selected).unwrap();
            assert!(
                EiyashouProjection::parse(&deleted).read_only.is_empty(),
                "{deleted}"
            );
            for direction in [MoveDirection::Up, MoveDirection::Down] {
                if let Ok(moved) = projection.move_blocks(&disabled, &selected, direction) {
                    assert!(
                        EiyashouProjection::parse(&moved).read_only.is_empty(),
                        "{moved}"
                    );
                    let copy = projection.copy_blocks(&disabled, &selected).unwrap();
                    let anchor = projection.scenes[0].blocks[0].source_range.start;
                    let (pasted, _) = projection
                        .insert_block_before(&disabled, anchor, &copy)
                        .unwrap();
                    assert!(
                        EiyashouProjection::parse(&pasted).read_only.is_empty(),
                        "{pasted}"
                    );
                }
            }
        }
        let source = r#"scene start { text.float("*/", x: 1, y: 2) }"#;
        let projection = EiyashouProjection::parse(source);
        assert!(
            projection
                .toggle_disabled(source, projection.scenes[0].blocks[0].source_range.start)
                .is_err()
        );
    }

    #[test]
    fn text_ending_keeps_voice_metadata_and_structural_edits_together() {
        let source = r#"scene start { @line hero: "Hi", voice, text.box(visible: false, auto: true), hide(hero*, transition: fade(200ms)), "Next" }"#;
        let projection = EiyashouProjection::parse(source);
        let text = &projection.scenes[0].blocks[0];
        let metadata = projection
            .text_block_metadata(source, text.source_range.start)
            .unwrap();
        assert_eq!(metadata.voice.as_deref(), Some("voice"));
        assert_eq!(
            projection.scenes[0].blocks[1].lifetime_owner,
            Some(text.source_range.start)
        );
        let changed = projection
            .replace_text_block_metadata(
                source,
                text.source_range.start,
                &TextBlockMetadata {
                    speaker: Some("rin".into()),
                    ..metadata
                },
            )
            .unwrap();
        assert!(changed.contains("hide(hero*, transition: fade(200ms))"));
        let selected = HashSet::from([text.source_range.start]);
        let copied = projection.copy_blocks(source, &selected).unwrap();
        assert!(copied.contains("text.box(") && copied.contains("hide(hero*"));
        let deleted = projection.delete_blocks(source, &selected).unwrap();
        assert!(!deleted.contains("text.box(") && !deleted.contains("hide(hero*"));
        assert!(EiyashouProjection::parse(&deleted).read_only.is_empty());
        let kept = projection
            .replace_text_lifetime(source, text.source_range.start, true, "", "")
            .unwrap();
        assert!(
            EiyashouProjection::parse(&kept).read_only.is_empty(),
            "{kept}"
        );
        assert!(kept.contains(r#"hero: "Hi", voice"#));
        let added = EiyashouProjection::parse(&kept)
            .replace_text_lifetime(
                &kept,
                text.source_range.start,
                false,
                "hero*",
                "fade(200ms)",
            )
            .unwrap();
        assert!(
            EiyashouProjection::parse(&added).read_only.is_empty(),
            "{added}"
        );
        assert_eq!(EiyashouProjection::parse(&added).scenes[0].blocks.len(), 4);

        // Retraction is a separate executable presentation step. Ending edits
        // and whole-Text clipboard operations must not absorb or remove it.
        let source = r#"scene start { @line hero: "我当然来了", voice, text.retract(source: "我当然来了", keep: "我当然"), text.retract(source: "", keep: "我"), "Next" }"#;
        let projection = EiyashouProjection::parse(source);
        let text = &projection.scenes[0].blocks[0];
        let selected = HashSet::from([text.source_range.start]);
        let copied = projection.copy_blocks(source, &selected).unwrap();
        assert!(!copied.contains("text.retract"));
        let ended = projection
            .replace_text_lifetime(source, text.source_range.start, false, "hero*", "")
            .unwrap();
        assert_eq!(ended.matches("text.retract(").count(), 2);
        let ended_projection = EiyashouProjection::parse(&ended);
        assert!(ended_projection.read_only.is_empty());
        assert!(
            ended_projection.scenes[0]
                .blocks
                .iter()
                .filter(|block| block.summary.starts_with("text.retract("))
                .all(|block| block.lifetime_owner.is_none() && block.depth == text.depth)
        );
    }

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
        let (duplicated, _) = projection.duplicate_blocks(source, &selected).unwrap();
        assert_eq!(duplicated.matches("@a").count(), 1);
        assert_eq!(duplicated.matches("\"one\"").count(), 2);
        assert!(EiyashouProjection::parse(&duplicated).read_only.is_empty());
        let source = "scene start { stage.animate(a, duration: 1s) { track(camera, x) { key(time: 0ms, value: 0) } }, \"next\" }";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        let selected = HashSet::from([blocks[0].source_range.start, blocks[2].source_range.start]);
        let (duplicated, _) = projection.duplicate_blocks(source, &selected).unwrap();
        let checked = EiyashouProjection::parse(&duplicated);
        assert_eq!(
            checked.scenes[0]
                .blocks
                .iter()
                .filter(|block| block.depth == 0)
                .count(),
            3
        );
        assert!(checked.read_only.is_empty());
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
                .move_blocks_to(source, &selected, blocks[2].source_range.start, false)
                .unwrap(),
            "scene start {\n  background(day),\n  \"one\",\n  wait(1s),\n  return\n}\n"
        );
        assert_eq!(
            projection
                .move_blocks_to(source, &selected, blocks[3].source_range.start, true)
                .unwrap(),
            "scene start {\n  background(day),\n  wait(1s),\n  return,\n  \"one\"\n}\n"
        );
    }

    #[test]
    fn reordered_text_blocks_keep_their_text_and_new_offsets() {
        let source = "scene opening {\n  background(day),\n  \"First narration.\",\n  aya: \"A different line.\",\n  \"Second narration wraps\\nonto another line.\"\n}\n";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        let selected = HashSet::from([blocks[1].source_range.start]);
        let edited = projection
            .move_blocks_to(source, &selected, blocks[3].source_range.start, true)
            .unwrap();
        let dialogues = crate::authoring::dialogues_for_source(
            std::path::Path::new("scripts/main.shou"),
            &edited,
        );
        let texts = dialogues
            .iter()
            .filter(|dialogue| dialogue.editable)
            .map(|dialogue| (dialogue.text_range.start, dialogue.text.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            texts.iter().map(|(_, text)| *text).collect::<Vec<_>>(),
            [
                "A different line.",
                "Second narration wraps\nonto another line.",
                "First narration.",
            ]
        );
        assert!(texts.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(
            dialogues
                .iter()
                .all(|dialogue| { edited.get(dialogue.text_range.clone()).is_some() })
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
                .move_blocks_to(source, &selected, blocks[4].source_range.start, false)
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
            projection.move_blocks_to(source, &selected, blocks[0].source_range.start, false),
            Err(BlockEditError::NoMoveTarget)
        );
    }

    #[test]
    fn inserts_around_a_block_without_reformatting_neighbors() {
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
        let (edited, range) = projection
            .insert_block_before(source, first, "wait(200ms)")
            .unwrap();
        assert_eq!(
            edited,
            "scene start {\n  wait(200ms),\n  \"one\",\n  wait(1s)\n}\n"
        );
        assert_eq!(edited.get(range), Some("wait(200ms)"));
        assert!(EiyashouProjection::parse(&edited).read_only.is_empty());
        let inline = "scene start { loop { \"one\", break } }";
        let projection = EiyashouProjection::parse(inline);
        let nested = projection.scenes[0]
            .blocks
            .iter()
            .find(|block| block.kind == BlockKind::Narration)
            .unwrap();
        let (edited, range) = projection
            .insert_block_before(inline, nested.source_range.start, "wait(200ms)")
            .unwrap();
        assert_eq!(
            edited,
            "scene start { loop { wait(200ms), \"one\", break } }"
        );
        assert_eq!(edited.get(range), Some("wait(200ms)"));
        assert!(EiyashouProjection::parse(&edited).read_only.is_empty());
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
    fn dotted_camera_and_sprite_focus_are_editable_blocks() {
        let source = "scene a { camera.move(scene, x: 20), camera.shake(all, amplitude: 8, frequency: 12, duration: 300ms), sprite.focus.configure(characters: [hero], speaking: style(), others: style(), narration: style()), sprite.focus(hero), hide(hero_*) }";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        assert_eq!(blocks.len(), 5);
        assert!(
            blocks
                .iter()
                .all(|block| block.kind == BlockKind::Command && !block.read_only)
        );
        let rule = &blocks[2];
        let fields = projection
            .source_fields(source, rule.source_range.start)
            .unwrap();
        assert!(fields.iter().any(|field| field.key == "characters"));
        let speaking = fields
            .iter()
            .find(|field| field.key == "speaking.scale")
            .unwrap();
        assert_eq!(speaking.value, "");
        let mut edited = source.to_owned();
        edited.replace_range(
            speaking.range.clone(),
            &format!("{}1.1", speaking.insertion.as_deref().unwrap()),
        );
        assert!(edited.contains("speaking: style(scale: 1.1)"));
        assert!(fields.iter().any(|field| field.key == "others.alpha"));
    }

    #[test]
    fn projects_existing_portrait_style_values_into_independent_fields() {
        let source = "scene a { sprite.focus.configure(characters: [hero], speaking: style(scale: 1.1, alpha: 0.8), others: style(), narration: style()) }";
        let projection = EiyashouProjection::parse(source);
        let block = &projection.scenes[0].blocks[0];
        let fields = projection
            .source_fields(source, block.source_range.start)
            .unwrap();
        let alpha = fields
            .iter()
            .find(|field| field.key == "speaking.alpha")
            .unwrap();
        assert_eq!(alpha.value, "0.8");
        assert_eq!(&source[alpha.range.clone()], "0.8");
        assert!(alpha.insertion.is_none());
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
        let z = fields.iter().find(|field| field.key == "z").unwrap();
        assert_eq!(z.insertion.as_deref(), Some(", z: "));
        let mut edited = source.to_owned();
        edited.replace_range(z.range.clone(), ", z: 2");
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
        let source = "scene start { camera.move(scene, x: 20, duration: 300ms), \"after\" }";
        let projection = EiyashouProjection::parse(source);
        let start = projection.scenes[0].blocks[0].source_range.start;
        let edited = projection
            .replace_block_fields(
                source,
                start,
                &[
                    ("x".into(), Some("-960".into())),
                    ("y".into(), Some("540".into())),
                    ("duration".into(), None),
                ],
            )
            .unwrap();
        assert_eq!(
            edited,
            "scene start { camera.move(scene, x: -960, y: 540), \"after\" }"
        );
        assert!(EiyashouProjection::parse(&edited).read_only.is_empty());
        let source = "scene start { camera.effect(all, vignette_intensity: 0.4, bloom_intensity: 0.5), \"after\" }";
        let projection = EiyashouProjection::parse(source);
        let start = projection.scenes[0].blocks[0].source_range.start;
        let edited = projection
            .replace_block_fields(source, start, &[("vignette_intensity".into(), None)])
            .unwrap();
        assert_eq!(
            edited,
            "scene start { camera.effect(all, bloom_intensity: 0.5), \"after\" }"
        );
        assert!(EiyashouProjection::parse(&edited).read_only.is_empty());
    }

    #[test]
    fn stage_timeline_projects_track_key_and_scene_layer_rows() {
        let source = "scene a { stage.animate(opening, duration: 2s) { track(camera, x) { key(time: 0ms, value: 0) }, event.scene(next, time: 1s) { layer(front, room, distance: 1) } } }";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        assert_eq!(
            blocks.iter().map(|block| block.depth).collect::<Vec<_>>(),
            [0, 1, 2, 1, 2]
        );
        assert!(
            blocks
                .iter()
                .all(|block| block.kind == BlockKind::Command && !block.read_only)
        );
        let key = &blocks[2];
        let fields = projection
            .source_fields(source, key.source_range.start)
            .unwrap();
        assert!(
            fields
                .iter()
                .any(|field| field.key == "value" && field.value == "0")
        );
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
        let source = r#"scene start { "Before", text.retract(source: "A👩‍👩‍👧‍👧\n\"B\"", keep: "A"), text.retract(source: "", keep: "") }"#;
        let projection = EiyashouProjection::parse(source);
        assert!(projection.read_only.is_empty());
        let fields = projection
            .source_fields(source, projection.scenes[0].blocks[1].source_range.start)
            .unwrap();
        assert_eq!(fields[0].value, "A👩‍👩‍👧‍👧\n\"B\"");
        assert!(fields[0].quoted && fields[1].quoted);
        let empty = projection
            .source_fields(source, projection.scenes[0].blocks[2].source_range.start)
            .unwrap();
        assert!(
            empty
                .iter()
                .all(|field| field.quoted && field.value.is_empty())
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
