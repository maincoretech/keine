//! Dialogue markup shared by execution and source-backed authoring views.

use std::ops::Range;

use crate::state::DialoguePause;

/// A zero-width wait in authored text. Ranges are UTF-8 byte offsets.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineWait {
    pub range: Range<usize>,
    /// Seconds, or `None` for a player-input wait.
    pub duration: Option<f32>,
}

/// Uses the execution tokenizer, so style labels and malformed waits are not highlighted as waits.
pub fn inline_waits(source: &str) -> impl Iterator<Item = InlineWait> + '_ {
    tokens(source).filter_map(|token| match token {
        Token::Wait(wait) => Some(wait),
        _ => None,
    })
}

pub(super) fn compile_rich_text(source: &str) -> (String, String, Vec<DialoguePause>) {
    let mut text = String::new();
    let mut markup = String::new();
    let mut pauses = Vec::new();
    let mut characters = 0;
    for token in tokens(source) {
        match token {
            Token::Text(value) => {
                text.push_str(value);
                markup.push_str(value);
                characters += value.chars().count();
            }
            Token::Styled { label, source } => {
                text.push_str(label);
                markup.push_str(source);
                characters += label.chars().count();
            }
            Token::Wait(wait) => pauses.push(DialoguePause {
                at: characters,
                duration: wait.duration,
            }),
        }
    }
    (text, markup, pauses)
}

enum Token<'a> {
    Text(&'a str),
    Styled { label: &'a str, source: &'a str },
    Wait(InlineWait),
}

fn tokens(source: &str) -> impl Iterator<Item = Token<'_>> {
    let mut cursor = 0;
    std::iter::from_fn(move || {
        let rest = source.get(cursor..).filter(|rest| !rest.is_empty())?;
        let start = cursor;
        if !rest.starts_with('[') {
            cursor += rest.find('[').unwrap_or(rest.len());
            return Some(Token::Text(&source[start..cursor]));
        }
        if let Some(end) = rest.find(']') {
            let label = &rest[1..end];
            if let Some(duration) = parse_wait(label) {
                cursor += end + 1;
                return Some(Token::Wait(InlineWait {
                    range: start..cursor,
                    duration,
                }));
            }
            if let Some(arguments) = rest[end + 1..].strip_prefix('(')
                && let Some(argument_end) = arguments.find(')')
            {
                cursor += end + 2 + argument_end + 1;
                return Some(Token::Styled {
                    label,
                    source: &source[start..cursor],
                });
            }
        }
        cursor += 1;
        Some(Token::Text(&source[start..cursor]))
    })
}

fn parse_wait(label: &str) -> Option<Option<f32>> {
    let label = label.trim();
    if label.eq_ignore_ascii_case("wait") {
        return Some(None);
    }
    let milliseconds = label
        .strip_prefix("wait=")
        .or_else(|| label.strip_prefix("wait time=\""))?
        .trim_end_matches('"')
        .parse::<f32>()
        .ok()?;
    // Invalid durations must remain literal text, never stall the typewriter forever.
    milliseconds
        .is_finite()
        .then_some(Some(milliseconds.max(0.) / 1000.))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_ranges_and_execution_share_unicode_and_style_boundaries() {
        let source = "[前](color=#fff)[wait=1000]後[wait]";
        let (text, markup, pauses) = compile_rich_text(source);
        assert_eq!(text, "前後");
        assert_eq!(markup, "[前](color=#fff)後");
        assert_eq!(
            pauses,
            [
                DialoguePause {
                    at: 1,
                    duration: Some(1.)
                },
                DialoguePause {
                    at: 2,
                    duration: None
                }
            ]
        );
        let waits = inline_waits(source).collect::<Vec<_>>();
        assert_eq!(&source[waits[0].range.clone()], "[wait=1000]");
        assert_eq!(waits[0].duration, pauses[0].duration);
        assert_eq!(&source[waits[1].range.clone()], "[wait]");
        assert!(inline_waits("[text](note=[wait=1000])").next().is_none());
    }

    #[test]
    fn malformed_waits_remain_literal_and_legacy_wait_spelling_is_retained() {
        let source = "[wait=NaN][wait=inf][wait=no][wait=1000";
        assert_eq!(
            compile_rich_text(source),
            (source.into(), source.into(), Vec::new())
        );
        assert_eq!(
            compile_rich_text("[wait time=\"250\"]後").2,
            [DialoguePause {
                at: 0,
                duration: Some(0.25)
            }]
        );
    }
}
