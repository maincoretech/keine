use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
    InputHighlighterFactory, Rope,
};
use gpui_kit::{Context, FontWeight, HighlightStyle, Hsla, SharedString, Window, rgb};
use keine_loader::{NativeTokenKind, parse_native_document};

const KEYWORDS: &[&str] = &[
    "scene", "choice", "if", "else", "loop", "let", "break", "return", "true", "false",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyntaxKind {
    Keyword,
    Function,
    Label,
    String,
    Number,
    Comment,
    Operator,
    Attribute,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SyntaxSpan {
    range: Range<usize>,
    kind: SyntaxKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyntaxLanguage {
    Eiyashou,
    Json,
    Yaml,
    Markdown,
    Toml,
}

impl SyntaxLanguage {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "eiyashou" => Some(Self::Eiyashou),
            "json" => Some(Self::Json),
            "yaml" => Some(Self::Yaml),
            "markdown" => Some(Self::Markdown),
            "toml" => Some(Self::Toml),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Eiyashou => "eiyashou",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Markdown => "markdown",
            Self::Toml => "toml",
        }
    }
}

pub fn editor_highlighter_factory() -> InputHighlighterFactory {
    Rc::new(|name| {
        SyntaxLanguage::from_name(name).map(|language| {
            Box::new(EditorHighlighter {
                language,
                spans: Vec::new(),
            }) as Box<dyn InputHighlighter>
        })
    })
}

struct EditorHighlighter {
    language: SyntaxLanguage,
    spans: Vec<SyntaxSpan>,
}

impl InputHighlighter for EditorHighlighter {
    fn language(&self) -> SharedString {
        self.language.name().into()
    }

    fn update(
        &mut self,
        _edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        let source = text.to_string();
        self.spans = syntax_spans(&source, self.language);
    }

    fn styles(
        &self,
        requested: &Range<usize>,
        _resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        if requested.is_empty() {
            return Vec::new();
        }
        let mut cursor = requested.start;
        let mut styles = Vec::new();
        let first = self
            .spans
            .partition_point(|span| span.range.end <= requested.start);
        for span in self.spans[first..]
            .iter()
            .take_while(|span| span.range.start < requested.end)
        {
            let start = span.range.start.max(requested.start);
            let end = span.range.end.min(requested.end);
            if cursor < start {
                styles.push((cursor..start, HighlightStyle::default()));
            }
            styles.push((start..end, style(span.kind)));
            cursor = end;
        }
        if cursor < requested.end {
            styles.push((cursor..requested.end, HighlightStyle::default()));
        }
        styles
    }

    fn fold_ranges(&self, _text: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

fn syntax_spans(source: &str, language: SyntaxLanguage) -> Vec<SyntaxSpan> {
    match language {
        SyntaxLanguage::Eiyashou => eiyashou_spans(source),
        SyntaxLanguage::Json => json_spans(source),
        SyntaxLanguage::Yaml => mapping_spans(source, b':'),
        SyntaxLanguage::Markdown => markdown_spans(source),
        SyntaxLanguage::Toml => mapping_spans(source, b'='),
    }
}

fn eiyashou_spans(source: &str) -> Vec<SyntaxSpan> {
    let tokens = parse_native_document(source).tokens;
    let significant = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| {
            !matches!(
                token.kind,
                NativeTokenKind::Whitespace | NativeTokenKind::Comment
            )
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut significant_position = vec![None; tokens.len()];
    for (position, token_index) in significant.iter().copied().enumerate() {
        significant_position[token_index] = Some(position);
    }

    tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            let raw = source.get(token.range.clone()).unwrap_or_default();
            let adjacent = |offset: isize| {
                let position = significant_position[index]? as isize + offset;
                let token_index = *significant.get(usize::try_from(position).ok()?)?;
                source.get(tokens[token_index].range.clone())
            };
            let kind = match token.kind {
                NativeTokenKind::String => Some(SyntaxKind::String),
                NativeTokenKind::Number => Some(SyntaxKind::Number),
                NativeTokenKind::Comment => Some(SyntaxKind::Comment),
                NativeTokenKind::Operator => Some(SyntaxKind::Operator),
                NativeTokenKind::Punctuation if raw == "@" => Some(SyntaxKind::Attribute),
                NativeTokenKind::Identifier if KEYWORDS.contains(&raw) => Some(SyntaxKind::Keyword),
                NativeTokenKind::Identifier if adjacent(-1) == Some("@") => {
                    Some(SyntaxKind::Attribute)
                }
                NativeTokenKind::Identifier if adjacent(1) == Some("(") => {
                    Some(SyntaxKind::Function)
                }
                NativeTokenKind::Identifier if adjacent(1) == Some(":") => Some(SyntaxKind::Label),
                _ => None,
            }?;
            Some(SyntaxSpan {
                range: token.range.clone(),
                kind,
            })
        })
        .collect()
}

fn json_spans(source: &str) -> Vec<SyntaxSpan> {
    let bytes = source.as_bytes();
    let mut spans = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        match bytes[index] {
            b'"' => {
                index = quoted_end(bytes, index, b'"');
                let mut next = index;
                while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
                    next += 1;
                }
                let kind = if bytes.get(next) == Some(&b':') {
                    SyntaxKind::Label
                } else {
                    SyntaxKind::String
                };
                spans.push(SyntaxSpan {
                    range: start..index,
                    kind,
                });
            }
            b'-' | b'0'..=b'9' => {
                index += 1;
                while bytes.get(index).is_some_and(|byte| {
                    byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-')
                }) {
                    index += 1;
                }
                if source[start..index].parse::<f64>().is_ok() {
                    spans.push(SyntaxSpan {
                        range: start..index,
                        kind: SyntaxKind::Number,
                    });
                }
            }
            b'a'..=b'z' | b'A'..=b'Z' => {
                index += 1;
                while bytes.get(index).is_some_and(u8::is_ascii_alphabetic) {
                    index += 1;
                }
                if matches!(&source[start..index], "true" | "false" | "null") {
                    spans.push(SyntaxSpan {
                        range: start..index,
                        kind: SyntaxKind::Keyword,
                    });
                }
            }
            _ => index += 1,
        }
    }
    spans
}

// Text-only colorization: project validation and Markdown rendering stay with their owners.
fn mapping_spans(source: &str, separator: u8) -> Vec<SyntaxSpan> {
    let mut spans = Vec::new();
    let mut offset = 0;
    let mut block_indent = None;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.trim_end_matches(['\r', '\n']);
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        let trimmed = &line[indent..];
        if let Some(base) = block_indent {
            if trimmed.is_empty() {
                offset += raw_line.len();
                continue;
            }
            if indent > base {
                spans.push(SyntaxSpan {
                    range: offset + indent..offset + line.len(),
                    kind: SyntaxKind::String,
                });
                offset += raw_line.len();
                continue;
            }
            block_indent = None;
        }
        if trimmed.starts_with('#') {
            spans.push(SyntaxSpan {
                range: offset + indent..offset + line.len(),
                kind: SyntaxKind::Comment,
            });
            offset += raw_line.len();
            continue;
        }
        if separator == b':' && (trimmed.starts_with("---") || trimmed.starts_with("...")) {
            let marker = &trimmed[..3];
            if trimmed[3..].is_empty() || trimmed[3..].starts_with([' ', '\t', '#']) {
                spans.push(SyntaxSpan {
                    range: offset + indent..offset + indent + marker.len(),
                    kind: SyntaxKind::Keyword,
                });
                scan_value(line, offset, indent + 3, &mut spans);
                offset += raw_line.len();
                continue;
            }
        }
        if separator == b'='
            && trimmed.starts_with('[')
            && let Some(end) = trimmed.find(']')
        {
            let end = if trimmed.as_bytes().get(end + 1) == Some(&b']') {
                end + 2
            } else {
                end + 1
            };
            spans.push(SyntaxSpan {
                range: offset + indent..offset + indent + end,
                kind: SyntaxKind::Keyword,
            });
            scan_value(line, offset, indent + end, &mut spans);
            offset += raw_line.len();
            continue;
        }
        let mut start = indent;
        if separator == b':'
            && line.as_bytes().get(start) == Some(&b'-')
            && line
                .as_bytes()
                .get(start + 1)
                .is_none_or(u8::is_ascii_whitespace)
        {
            spans.push(SyntaxSpan {
                range: offset + start..offset + start + 1,
                kind: SyntaxKind::Operator,
            });
            start += 1;
            while line
                .as_bytes()
                .get(start)
                .is_some_and(u8::is_ascii_whitespace)
            {
                start += 1;
            }
        }
        if let Some(relative) = mapping_separator(&line[start..], separator) {
            let key = &line[start..start + relative];
            let key_start = start + key.len() - key.trim_start().len();
            let key_end = start + key.trim_end().len();
            if key_start < key_end {
                spans.push(SyntaxSpan {
                    range: offset + key_start..offset + key_end,
                    kind: SyntaxKind::Label,
                });
            }
            let split = start + relative;
            spans.push(SyntaxSpan {
                range: offset + split..offset + split + 1,
                kind: SyntaxKind::Operator,
            });
            start = split + 1;
        }
        let value = line[start..].trim_start();
        if separator == b':' && value.starts_with(['|', '>']) {
            block_indent = Some(indent);
        }
        scan_value(line, offset, start, &mut spans);
        offset += raw_line.len();
    }
    spans
}

fn mapping_separator(line: &str, separator: u8) -> Option<usize> {
    let bytes = line.as_bytes();
    let (mut quote, mut depth, mut index) = (None, 0_u32, 0);
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(current) = quote {
            if current == b'"' && byte == b'\\' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            if byte == current {
                quote = None;
            }
        } else {
            match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'[' | b'{' => depth += 1,
                b']' | b'}' => depth = depth.saturating_sub(1),
                b'#' if index == 0 || bytes[index - 1].is_ascii_whitespace() => return None,
                _ if byte == separator
                    && depth == 0
                    && (separator != b':'
                        || bytes.get(index + 1).is_none_or(u8::is_ascii_whitespace)) =>
                {
                    return Some(index);
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn scan_value(line: &str, offset: usize, mut index: usize, spans: &mut Vec<SyntaxSpan>) {
    let bytes = line.as_bytes();
    while index < bytes.len() {
        let start = index;
        match bytes[index] {
            b' ' | b'\t' => index += 1,
            b'#' => {
                spans.push(SyntaxSpan {
                    range: offset + start..offset + line.len(),
                    kind: SyntaxKind::Comment,
                });
                break;
            }
            b'"' | b'\'' => {
                index = quoted_end(bytes, index, bytes[index]);
                spans.push(SyntaxSpan {
                    range: offset + start..offset + index,
                    kind: SyntaxKind::String,
                });
            }
            b'[' | b']' | b'{' | b'}' | b',' | b'|' | b'>' => {
                index += 1;
                spans.push(SyntaxSpan {
                    range: offset + start..offset + index,
                    kind: SyntaxKind::Operator,
                });
            }
            _ => {
                index += 1;
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && !matches!(bytes[index], b',' | b'[' | b']' | b'{' | b'}' | b'|' | b'>')
                {
                    index += 1;
                }
                let atom = &line[start..index];
                let kind = if matches!(atom, "true" | "false" | "null" | "~") {
                    SyntaxKind::Keyword
                } else if atom.parse::<f64>().is_ok() {
                    SyntaxKind::Number
                } else {
                    SyntaxKind::String
                };
                spans.push(SyntaxSpan {
                    range: offset + start..offset + index,
                    kind,
                });
            }
        }
    }
}

fn quoted_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut index = start + 1;
    while index < bytes.len() {
        if quote == b'"' && bytes[index] == b'\\' {
            index = (index + 2).min(bytes.len());
            continue;
        }
        index += 1;
        if bytes[index - 1] == quote {
            break;
        }
    }
    index
}

fn markdown_spans(source: &str) -> Vec<SyntaxSpan> {
    let mut spans = Vec::new();
    let mut offset = 0;
    let mut fence: Option<(u8, usize)> = None;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.trim_end_matches(['\r', '\n']);
        let indent = line.len() - line.trim_start_matches(' ').len();
        let trimmed = &line[indent..];
        let marker = trimmed.as_bytes().first().copied();
        let run = marker.map_or(0, |byte| {
            trimmed
                .as_bytes()
                .iter()
                .take_while(|next| **next == byte)
                .count()
        });
        if let Some((byte, minimum)) = fence {
            if marker == Some(byte) && run >= minimum && trimmed[run..].trim().is_empty() {
                spans.push(SyntaxSpan {
                    range: offset + indent..offset + line.len(),
                    kind: SyntaxKind::Attribute,
                });
                fence = None;
            } else if !trimmed.is_empty() {
                spans.push(SyntaxSpan {
                    range: offset + indent..offset + line.len(),
                    kind: SyntaxKind::Function,
                });
            }
        } else if indent <= 3 && matches!(marker, Some(b'`' | b'~')) && run >= 3 {
            spans.push(SyntaxSpan {
                range: offset + indent..offset + line.len(),
                kind: SyntaxKind::Attribute,
            });
            fence = marker.map(|byte| (byte, run));
        } else if trimmed.starts_with("<!--") {
            spans.push(SyntaxSpan {
                range: offset + indent..offset + line.len(),
                kind: SyntaxKind::Comment,
            });
        } else if marker == Some(b'#')
            && (1..=6).contains(&run)
            && trimmed.as_bytes().get(run) == Some(&b' ')
        {
            spans.push(SyntaxSpan {
                range: offset + indent..offset + line.len(),
                kind: SyntaxKind::Keyword,
            });
        } else if matches!(trimmed, "---" | "***" | "___") {
            spans.push(SyntaxSpan {
                range: offset + indent..offset + line.len(),
                kind: SyntaxKind::Operator,
            });
        } else {
            let mut start = indent;
            if let Some(marker_len) = markdown_list_marker_len(trimmed) {
                spans.push(SyntaxSpan {
                    range: offset + start..offset + start + marker_len,
                    kind: SyntaxKind::Operator,
                });
                start += marker_len;
            }
            scan_markdown_inline(line, offset, start, &mut spans);
        }
        offset += raw_line.len();
    }
    spans
}

fn markdown_list_marker_len(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    if matches!(bytes.first(), Some(b'-' | b'*' | b'+' | b'>')) && bytes.get(1) == Some(&b' ') {
        return Some(1);
    }
    let digits = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    (digits > 0
        && matches!(bytes.get(digits), Some(b'.' | b')'))
        && bytes.get(digits + 1) == Some(&b' '))
    .then_some(digits + 1)
}

fn scan_markdown_inline(line: &str, offset: usize, mut index: usize, spans: &mut Vec<SyntaxSpan>) {
    let bytes = line.as_bytes();
    while index < bytes.len() {
        let start = index;
        match bytes[index] {
            b'\\' => index = (index + 2).min(bytes.len()),
            b'`' => {
                let run = bytes[index..]
                    .iter()
                    .take_while(|byte| **byte == b'`')
                    .count();
                index += run;
                if let Some(close) = line[index..].find(&"`".repeat(run)) {
                    index += close + run;
                    spans.push(SyntaxSpan {
                        range: offset + start..offset + index,
                        kind: SyntaxKind::String,
                    });
                }
            }
            b'[' => {
                if let Some(label_end) = line[index..].find("](") {
                    let url_start = index + label_end + 2;
                    if let Some(url_end) = line[url_start..].find(')') {
                        index = url_start + url_end + 1;
                        spans.push(SyntaxSpan {
                            range: offset + start..offset + index,
                            kind: SyntaxKind::Attribute,
                        });
                        continue;
                    }
                }
                index += 1;
            }
            b'*' | b'_' => {
                let marker = bytes[index];
                let run = bytes[index..]
                    .iter()
                    .take_while(|byte| **byte == marker)
                    .count()
                    .min(2);
                index += run;
                if let Some(close) = line[index..].find(&line[start..start + run])
                    && close > 0
                {
                    index += close + run;
                    spans.push(SyntaxSpan {
                        range: offset + start..offset + index,
                        kind: SyntaxKind::Attribute,
                    });
                }
            }
            _ => index += 1,
        }
    }
}

fn style(kind: SyntaxKind) -> HighlightStyle {
    let (color, font_weight) = match kind {
        SyntaxKind::Keyword => (0xbaebff, Some(FontWeight::SEMIBOLD)),
        SyntaxKind::Function => (0xc6d4ec, None),
        SyntaxKind::Label => (0xf0b69f, None),
        SyntaxKind::String => (0xaed6ae, None),
        SyntaxKind::Number => (0xddc58e, None),
        SyntaxKind::Comment => (0x71808e, None),
        SyntaxKind::Operator => (0x9aa8b5, None),
        SyntaxKind::Attribute => (0xc6b6e7, None),
    };
    HighlightStyle {
        color: Some(color_value(color)),
        font_weight,
        ..HighlightStyle::default()
    }
}

fn color_value(value: u32) -> Hsla {
    rgb(value).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoStyles;

    impl HighlightStyleResolver for NoStyles {
        fn style(&self, _: &str) -> Option<HighlightStyle> {
            None
        }
    }

    #[test]
    fn highlights_eiyashou_structure_without_rewriting_source() {
        let source = concat!(
            "// opening\n",
            "@id(start) scene opening {\n",
            "  background(room),\n",
            "  rin: \"Hello\",\n",
            "  let score = 2\n",
            "}\n",
        );
        assert_spans(source, SyntaxLanguage::Eiyashou);
        let spans = syntax_spans(source, SyntaxLanguage::Eiyashou);
        let highlighted = |kind| {
            spans
                .iter()
                .filter(|span| span.kind == kind)
                .map(|span| &source[span.range.clone()])
                .collect::<Vec<_>>()
        };
        assert_eq!(highlighted(SyntaxKind::Comment), ["// opening"]);
        assert_eq!(highlighted(SyntaxKind::Attribute), ["@", "id"]);
        assert_eq!(highlighted(SyntaxKind::Keyword), ["scene", "let"]);
        assert_eq!(highlighted(SyntaxKind::Function), ["background"]);
        assert_eq!(highlighted(SyntaxKind::Label), ["rin"]);
        assert_eq!(highlighted(SyntaxKind::String), ["\"Hello\""]);
        assert_eq!(highlighted(SyntaxKind::Number), ["2"]);
    }

    #[test]
    fn one_factory_covers_supported_text_formats() {
        let factory = editor_highlighter_factory();
        for language in ["eiyashou", "json", "yaml", "markdown", "toml"] {
            assert_eq!(factory(language).unwrap().language(), language);
        }
        assert!(factory("plaintext").is_none());
    }

    #[test]
    fn yaml_and_toml_highlight_keys_values_and_comments() {
        let yaml = "title: \"Kēne\" # name\nitems:\n  - true\ntext: |\n  # this is scalar text\n";
        assert_spans(yaml, SyntaxLanguage::Yaml);
        assert_eq!(
            tokens(yaml, SyntaxLanguage::Yaml, SyntaxKind::Label),
            ["title", "items", "text"]
        );
        assert_eq!(
            tokens(yaml, SyntaxLanguage::Yaml, SyntaxKind::Keyword),
            ["true"]
        );
        assert_eq!(
            tokens(yaml, SyntaxLanguage::Yaml, SyntaxKind::Comment),
            ["# name"]
        );
        assert!(
            tokens(yaml, SyntaxLanguage::Yaml, SyntaxKind::String)
                .contains(&"# this is scalar text")
        );

        let toml = "[package]\nname = 'keine'\ncount = 2 # note\n";
        assert_spans(toml, SyntaxLanguage::Toml);
        assert_eq!(
            tokens(toml, SyntaxLanguage::Toml, SyntaxKind::Label),
            ["name", "count"]
        );
        assert_eq!(
            tokens(toml, SyntaxLanguage::Toml, SyntaxKind::Number),
            ["2"]
        );
    }

    #[test]
    fn markdown_highlights_structure_without_coloring_code_as_prose() {
        let source = "# 标题\n- [link](https://example.test) and `code`\n1. step\n---\n```yaml\nkey: value\n```\n";
        assert_spans(source, SyntaxLanguage::Markdown);
        assert_eq!(
            tokens(source, SyntaxLanguage::Markdown, SyntaxKind::Keyword),
            ["# 标题"]
        );
        assert_eq!(
            tokens(source, SyntaxLanguage::Markdown, SyntaxKind::Attribute),
            ["[link](https://example.test)", "```yaml", "```"]
        );
        assert!(tokens(source, SyntaxLanguage::Markdown, SyntaxKind::String).contains(&"`code`"));
        assert!(
            tokens(source, SyntaxLanguage::Markdown, SyntaxKind::Function).contains(&"key: value")
        );
        assert_eq!(
            tokens(source, SyntaxLanguage::Markdown, SyntaxKind::Operator),
            ["-", "1.", "---"]
        );
    }

    #[test]
    fn json_highlights_keys_literals_and_escaped_strings() {
        let source = "{\"title\": \"a \\\"quote\\\"\", \"count\": 12, \"ready\": true}";
        assert_spans(source, SyntaxLanguage::Json);
        assert_eq!(
            tokens(source, SyntaxLanguage::Json, SyntaxKind::Label),
            ["\"title\"", "\"count\"", "\"ready\""]
        );
        assert_eq!(
            tokens(source, SyntaxLanguage::Json, SyntaxKind::Number),
            ["12"]
        );
        assert_eq!(
            tokens(source, SyntaxLanguage::Json, SyntaxKind::Keyword),
            ["true"]
        );
    }

    #[test]
    fn requested_style_runs_cover_only_the_visible_range() {
        let source = "prefix: plain\nvalue: true\n";
        let highlighter = EditorHighlighter {
            language: SyntaxLanguage::Yaml,
            spans: syntax_spans(source, SyntaxLanguage::Yaml),
        };
        let requested = 10..21;
        let runs = highlighter.styles(&requested, &NoStyles);
        assert_eq!(runs.first().unwrap().0.start, requested.start);
        assert_eq!(runs.last().unwrap().0.end, requested.end);
        for pair in runs.windows(2) {
            assert_eq!(pair[0].0.end, pair[1].0.start);
        }
    }

    #[test]
    fn incomplete_multilingual_input_keeps_valid_nonoverlapping_ranges() {
        for (language, source) in [
            (SyntaxLanguage::Eiyashou, "scene 开始 { \"未完成"),
            (SyntaxLanguage::Json, "{\"标题\": \"未完成\\"),
            (
                SyntaxLanguage::Yaml,
                "标题: \"未完成\ntext: |\n  中文 # text",
            ),
            (SyntaxLanguage::Markdown, "# 标题\n[链接](未完成\n`代码"),
            (SyntaxLanguage::Toml, "[标题]\n值 = '未完成"),
        ] {
            assert_spans(source, language);
        }
    }

    fn tokens(source: &str, language: SyntaxLanguage, kind: SyntaxKind) -> Vec<&str> {
        syntax_spans(source, language)
            .into_iter()
            .filter(|span| span.kind == kind)
            .map(|span| &source[span.range])
            .collect()
    }

    fn assert_spans(source: &str, language: SyntaxLanguage) {
        let mut end = 0;
        for span in syntax_spans(source, language) {
            assert!(span.range.start >= end);
            assert!(span.range.start < span.range.end);
            assert!(source.is_char_boundary(span.range.start));
            assert!(source.is_char_boundary(span.range.end));
            end = span.range.end;
        }
    }
}
