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

pub fn eiyashou_highlighter_factory() -> InputHighlighterFactory {
    Rc::new(|language| {
        language
            .eq_ignore_ascii_case("eiyashou")
            .then(|| Box::new(EiyashouHighlighter::default()) as Box<dyn InputHighlighter>)
    })
}

#[derive(Default)]
struct EiyashouHighlighter {
    spans: Vec<SyntaxSpan>,
}

impl InputHighlighter for EiyashouHighlighter {
    fn language(&self) -> SharedString {
        "eiyashou".into()
    }

    fn update(
        &mut self,
        _edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        self.spans = syntax_spans(&text.to_string());
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
        for span in self
            .spans
            .iter()
            .filter(|span| span.range.end > requested.start && span.range.start < requested.end)
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

fn syntax_spans(source: &str) -> Vec<SyntaxSpan> {
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
        let spans = syntax_spans(source);
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
}
