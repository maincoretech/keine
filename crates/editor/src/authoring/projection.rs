mod edit;
mod parse;
use super::commands::is_native_command;
pub use super::fields::command_argument_names;
use super::valid_identifier;
use parse::BlockProjectionParser;
use std::ops::Range;
use std::{collections::HashSet, fmt};

use keine_loader::{
    Diagnostic, NativeToken, NativeTokenKind, SourceLineIndex, is_native_dotted_command,
    is_native_structured_command, parse_native_document,
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
    /// Explicit text lines used before an offscreen input has been measured.
    pub text_rows: usize,
    pub line: usize,
    pub column: usize,
    pub depth: usize,
    pub summary: String,
    pub stable_id: Option<String>,
    pub read_only: bool,
}

impl BlockCard {
    /// This command remains in the source model for copy/move/undo, but its
    /// control is presented on the owning dialogue row.
    pub fn is_textbox_ending(&self) -> bool {
        self.lifetime_owner.is_some()
            && self.kind == BlockKind::Command
            && self.summary.split('(').next().map(str::trim) == Some("text.box")
    }
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
            .and_then(|suffix| suffix.split(',').next())
            .map(str::trim)
            .filter(|value| valid_identifier(value))
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
            BlockKind::Narration | BlockKind::Dialogue { .. } => {
                let text = block.text_range.as_ref()?;
                let start = text.end + 1;
                let suffix = source.get(start..block.statement_range.end)?;
                let mut fields = Vec::new();
                for span in split_source_ranges(suffix, ',') {
                    let Some(full) =
                        trimmed_source_range(source, start + span.start..start + span.end)
                    else {
                        continue;
                    };
                    let raw = source.get(full.clone())?;
                    if let Some(colon) = top_level_position(raw, ':') {
                        let key = raw[..colon].trim();
                        if matches!(key, "volume" | "concat" | "auto" | "inherit_speaker") {
                            let range =
                                trimmed_source_range(source, full.start + colon + 1..full.end)?;
                            fields.push(SourceField {
                                key: key.into(),
                                value: source[range.clone()].into(),
                                range,
                                quoted: false,
                                insertion: None,
                                insertion_suffix: None,
                            });
                        }
                    }
                }
                for key in ["volume", "concat", "auto", "inherit_speaker"] {
                    if !fields.iter().any(|field| field.key == key) {
                        let end = block.statement_range.end;
                        fields.push(SourceField {
                            key: key.into(),
                            value: String::new(),
                            range: end..end,
                            quoted: false,
                            insertion: Some(format!(", {key}: ")),
                            insertion_suffix: None,
                        });
                    }
                }
                Some(fields)
            }
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
                    if matches!(command, "sprite" | "sprite.update" | "move" | "camera.move") {
                        fields = fields
                            .into_iter()
                            .flat_map(|field| {
                                if field.key == "layout"
                                    || field.key == "position"
                                    || (command == "move" && field.key == "1")
                                    || (command == "camera.move" && field.key == "shake")
                                {
                                    grouped_visual_fields(source, &field)
                                        .unwrap_or_else(|| vec![field])
                                } else {
                                    vec![field]
                                }
                            })
                            .collect();
                    }
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
                        if super::fields::scale_argument_conflicts(command, name, &fields)
                            || fields.iter().any(|field| {
                                field.key == name || field.key.starts_with(&format!("{name}."))
                            })
                        {
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

fn grouped_visual_fields(source: &str, parent: &SourceField) -> Option<Vec<SourceField>> {
    let value = source.get(parent.range.clone())?;
    let open = value.find('(');
    let constructor = value[..open.unwrap_or(value.len())].trim();
    if !matches!(
        constructor,
        "left"
            | "center"
            | "right"
            | "natural"
            | "viewport"
            | "scene"
            | "composite"
            | "point"
            | "size"
            | "rect"
            | "shake"
    ) {
        return None;
    }
    let position = matches!(constructor, "left" | "center" | "right");
    let mut root = parent.clone();
    if position {
        root.value = constructor.into();
        root.range.end = root.range.start + constructor.len();
    }
    let mut fields = vec![root];
    let names = command_argument_names(constructor);
    let body = if let Some(open) = open {
        let close = matching_parenthesis(value, open)?;
        if close != value.len() - 1 {
            return None;
        }
        &value[open + 1..close]
    } else {
        ""
    };
    let body_start = parent.range.start + open.map_or(value.len(), |index| index + 1);
    for span in split_source_ranges(body, ',') {
        let Some(full) =
            trimmed_source_range(source, body_start + span.start..body_start + span.end)
        else {
            continue;
        };
        let raw = source.get(full.clone())?;
        let colon = top_level_position(raw, ':')?;
        let name = raw[..colon].trim();
        if !names.contains(&name) {
            return None;
        }
        let range = trimmed_source_range(source, full.start + colon + 1..full.end)?;
        let field = SourceField {
            key: format!("{}.{}", parent.key, name),
            value: source[range.clone()].to_owned(),
            range,
            quoted: false,
            insertion: None,
            insertion_suffix: None,
        };
        if matches!(name, "anchor" | "canvas" | "rect") {
            fields.extend(grouped_visual_fields(source, &field).unwrap_or_else(|| vec![field]));
        } else {
            fields.push(field);
        }
    }
    for name in names {
        let key = format!("{}.{}", parent.key, name);
        if fields.iter().any(|field| field.key == key) {
            continue;
        }
        let at = parent.range.end - usize::from(open.is_some());
        fields.push(SourceField {
            key,
            value: String::new(),
            range: at..at,
            quoted: false,
            insertion: Some(format!(
                "{}{name}: ",
                if open.is_none() {
                    "("
                } else if body.trim().is_empty() {
                    ""
                } else {
                    ", "
                }
            )),
            insertion_suffix: open.is_none().then(|| ")".into()),
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
                        && !matches!(text(next), "return" | "break")
                        && after_next.is_none_or(|after| text(after) == ",")
                });
                let next_is_option = next.is_some_and(|next| {
                    matches!(text(next), "volume" | "concat" | "auto" | "inherit_speaker")
                }) && after_next.is_some_and(|after| text(after) == ":");
                if (next_is_voice || next_is_option)
                    && starts_with_text_statement(&tokens[start..position], text, kind)
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
    let mut start = if tokens.first().is_some_and(|index| text(*index) == "@") {
        2
    } else {
        0
    };
    if tokens
        .get(start + 1)
        .is_some_and(|index| text(*index) == ":")
    {
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
    fn dialogue_options_stay_on_one_block_and_survive_metadata_edits() {
        let source = "scene start { hero: \"你好\", voice, volume: 0.4, concat: true, auto: true, inherit_speaker: true, wait(1s) }";
        let projection = EiyashouProjection::parse(source);
        assert_eq!(projection.scenes[0].blocks.len(), 2);
        assert!(projection.read_only.is_empty());
        let start = projection.scenes[0].blocks[0].source_range.start;
        let mut metadata = projection.text_block_metadata(source, start).unwrap();
        assert_eq!(metadata.voice.as_deref(), Some("voice"));
        metadata.voice = Some("new_voice".into());
        let edited = projection
            .replace_text_block_metadata(source, start, &metadata)
            .unwrap();
        assert_eq!(edited, source.replace(", voice,", ", new_voice,"));
    }

    #[test]
    fn unicode_metadata_roundtrips_through_loader_and_bounded_edits() {
        let source = "// 保留注释\nscene 开始 { \"你好🌙\", wait(1s) }\n";
        let projection = EiyashouProjection::parse(source);
        let start = projection.scenes[0].blocks[0].source_range.start;
        let metadata = TextBlockMetadata {
            speaker: Some("灵梦".into()),
            voice: Some("问候音声".into()),
            stable_id: Some("开场对白".into()),
        };
        let edited = projection
            .replace_text_block_metadata(source, start, &metadata)
            .unwrap();
        assert_eq!(
            edited,
            "// 保留注释\nscene 开始 { @开场对白 灵梦: \"你好🌙\", 问候音声, wait(1s) }\n"
        );
        let document = parse_native_document(&edited);
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        let reparsed = EiyashouProjection::parse(&edited);
        let start = reparsed.scenes[0].blocks[0].source_range.start;
        assert_eq!(
            reparsed.text_block_metadata(&edited, start),
            Some(metadata.clone())
        );
        for invalid in ["", "2speaker", "not valid", "a-b", "a.b"] {
            let invalid = TextBlockMetadata {
                speaker: Some(invalid.into()),
                ..metadata.clone()
            };
            assert_eq!(
                reparsed.replace_text_block_metadata(&edited, start, &invalid),
                Err(BlockEditError::InvalidIdentifier)
            );
        }
    }

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
        assert!(projection.scenes[0].blocks[1].is_textbox_ending());
        assert!(!projection.scenes[0].blocks[2].is_textbox_ending());
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

        // Each dialogue owns its switch; standalone textbox commands and the
        // next line's ending must survive toggling the first line.
        let source = r#"scene start { text.box(visible: false), hero: "Hi", voice, volume: 0.8, concat: true, hide(hero*, transition: fade(200ms)), "Next", text.box(visible: false, auto: true) }"#;
        let projection = EiyashouProjection::parse(source);
        assert!(!projection.scenes[0].blocks[0].is_textbox_ending());
        let first = &projection.scenes[0].blocks[1];
        let ending = projection
            .text_lifetime(source, first.source_range.start)
            .unwrap();
        let hidden = projection
            .replace_text_lifetime(
                source,
                first.source_range.start,
                false,
                &ending.target,
                &ending.transition,
            )
            .unwrap();
        assert!(hidden.contains(r#"hero: "Hi", voice, volume: 0.8, concat: true"#));
        let projection = EiyashouProjection::parse(&hidden);
        let restored = projection
            .replace_text_lifetime(
                &hidden,
                first.source_range.start,
                true,
                &ending.target,
                &ending.transition,
            )
            .unwrap();
        assert_eq!(
            restored.split_whitespace().collect::<Vec<_>>(),
            source.split_whitespace().collect::<Vec<_>>()
        );
    }

    #[test]
    fn dialogue_tail_preserves_standalone_control_blocks() {
        let source = "scene start { \"Ready\", return, loop { hero: \"Hi\", break } }";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        assert_eq!(blocks.len(), 5);
        assert_eq!(blocks[1].kind, BlockKind::Control);
        assert_eq!(&source[blocks[1].source_range.clone()], "return");
        assert_eq!(blocks[4].kind, BlockKind::Control);
        assert_eq!(&source[blocks[4].source_range.clone()], "break");
        assert_eq!(blocks[4].depth, 1);
        assert!(blocks.iter().all(|block| !block.read_only));
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
    fn drag_targets_share_nested_source_edit_boundaries() {
        let source = r#"scene start {
  wait(1s),
  choice {
    "First": { wait(2s), wait(3s) },
    "Second": { wait(4s), wait(5s) }
  },
  wait(6s)
}
scene other { wait(7s) }"#;
        let projection = EiyashouProjection::parse(source);
        assert!(projection.read_only.is_empty());
        let start = |text: &str| source.find(text).unwrap();
        for (selected, target, accepted) in [
            ("wait(1s)", "wait(6s)", true),
            ("wait(2s)", "wait(3s)", true),
            ("\"First\"", "\"Second\"", true),
            ("wait(1s)", "wait(2s)", false),
            ("wait(2s)", "wait(1s)", false),
            ("wait(2s)", "wait(4s)", false),
            ("choice", "wait(2s)", false),
            ("wait(1s)", "wait(7s)", false),
        ] {
            let selected = HashSet::from([start(selected)]);
            let target = start(target);
            assert_eq!(projection.accepts_block_drop(&selected, target), accepted);
            for after in [false, true] {
                let edited = projection.move_blocks_to(source, &selected, target, after);
                assert_eq!(edited.is_ok(), accepted);
                if let Ok(edited) = edited {
                    assert!(EiyashouProjection::parse(&edited).read_only.is_empty());
                    assert_eq!(edited.matches("wait(").count(), 7);
                }
            }
        }
        let selected = HashSet::from([start("wait(1s)"), start("wait(2s)")]);
        assert!(!projection.accepts_block_drop(&selected, start("wait(6s)")));
        assert!(
            projection
                .move_blocks_to(source, &selected, start("wait(6s)"), true)
                .is_err()
        );
        let selected = HashSet::from([start("wait(2s)")]);
        assert!(!projection.accepts_block_drop(&selected, start("wait(2s)")));
        assert_eq!(
            projection
                .move_blocks_to(source, &selected, start("wait(2s)"), true)
                .unwrap(),
            source
        );
        // Moving a complete option preserves both children inside their original branch.
        let selected = HashSet::from([start("\"Second\"")]);
        let edited = projection
            .move_blocks_to(source, &selected, start("\"First\""), false)
            .unwrap();
        assert!(edited.contains(
            "\"Second\": { wait(4s), wait(5s) },\n    \"First\": { wait(2s), wait(3s) }"
        ));
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
    fn grouped_batch_edits_preserve_siblings_and_combine_insertions() {
        let source = "scene start { sprite(hero, face, position: right, layout: viewport(height: 0.85), y: 24) /* kept */ }";
        let projection = EiyashouProjection::parse(source);
        let start = source.find("sprite(").unwrap();
        let edited = projection
            .replace_block_fields(
                source,
                start,
                &[
                    ("position.x".into(), Some("500".into())),
                    ("position.y".into(), Some("20".into())),
                    ("layout.height".into(), Some("0.9".into())),
                ],
            )
            .unwrap();
        assert_eq!(
            edited,
            "scene start { sprite(hero, face, position: right(x: 500, y: 20), layout: viewport(height: 0.9), y: 24) /* kept */ }"
        );
        let removed = EiyashouProjection::parse(&edited)
            .replace_block_fields(&edited, start, &[("position.x".into(), None)])
            .unwrap();
        assert!(removed.contains("right( y: 20)") && removed.contains("y: 24"));
        assert!(EiyashouProjection::parse(&removed).read_only.is_empty());
    }

    #[test]
    fn grouped_camera_shake_edits_preserve_atomic_tween_and_siblings() {
        let source = "scene a { camera.move(all, x: 10, shake: shake(amplitude: 4, frequency: 2), duration: 1s, tween: [x, shake_amplitude]) }";
        let projection = EiyashouProjection::parse(source);
        assert!(projection.read_only.is_empty());
        let start = source.find("camera.move").unwrap();
        let edited = projection
            .replace_block_fields(
                source,
                start,
                &[
                    ("shake.amplitude".into(), Some("8".into())),
                    ("shake.frequency_randomness".into(), Some("0.3".into())),
                ],
            )
            .unwrap();
        assert!(edited.contains("shake(amplitude: 8, frequency: 2, frequency_randomness: 0.3)"));
        assert!(edited.contains("x: 10") && edited.contains("tween: [x, shake_amplitude]"));
        assert!(EiyashouProjection::parse(&edited).read_only.is_empty());
    }

    #[test]
    fn grouped_visual_fields_keep_precise_ranges_and_nested_insertions() {
        let source = "scene a { sprite(hero, face, position: right(x: 500, y: 20), layout: viewport(height: 0.85), y: 12), move(hero, left) }";
        let projection = EiyashouProjection::parse(source);
        assert!(projection.read_only.is_empty());
        let fields = projection
            .source_fields(source, projection.scenes[0].blocks[0].source_range.start)
            .unwrap();
        for (name, expected) in [
            ("position", "right"),
            ("position.x", "500"),
            ("position.y", "20"),
            ("layout.height", "0.85"),
            ("y", "12"),
        ] {
            let field = fields.iter().find(|field| field.key == name).unwrap();
            assert_eq!(&source[field.range.clone()], expected);
        }
        let fields = projection
            .source_fields(source, projection.scenes[0].blocks[1].source_range.start)
            .unwrap();
        let x = fields.iter().find(|field| field.key == "1.x").unwrap();
        let mut edited = source.to_owned();
        edited.replace_range(
            x.range.clone(),
            &format!(
                "{}30{}",
                x.insertion.as_deref().unwrap(),
                x.insertion_suffix.as_deref().unwrap()
            ),
        );
        assert!(edited.ends_with("move(hero, left(x: 30)) }"));
        assert!(EiyashouProjection::parse(&edited).read_only.is_empty());
        let source = "scene a { sprite(layer, face, layout: composite(canvas: size(width: 1920, height: 1080), rect: rect(x: 0, y: 0, width: 700, height: 900))) }";
        let projection = EiyashouProjection::parse(source);
        let fields = projection
            .source_fields(source, projection.scenes[0].blocks[0].source_range.start)
            .unwrap();
        let width = fields
            .iter()
            .find(|field| field.key == "layout.rect.width")
            .unwrap();
        assert_eq!(&source[width.range.clone()], "700");
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
            fields
                .iter()
                .filter(|field| field.insertion.is_none())
                .map(|field| field.key.as_str())
                .collect::<Vec<_>>(),
            ["0", "1", "position", "transition"]
        );
        let transition = fields
            .iter()
            .find(|field| field.key == "transition")
            .unwrap();
        assert_eq!(source.get(transition.range.clone()), Some("fade(300ms)"));
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
