//! Whitespace-only formatting shared by migration and the native Editor.
use super::{NativeTokenKind as Kind, lex};

/// Two-space indentation and 100-column call wrapping. Invalid/incomplete
/// lexical input is left to the author; strings and comments remain byte exact.
pub fn format_native_source(source: &str) -> Option<String> {
    let (tokens, errors) = lex(source);
    if !errors.is_empty() {
        return None;
    }
    let tokens: Vec<_> = tokens
        .iter()
        .filter(|token| token.kind != Kind::Whitespace)
        .collect();
    let text = |i: usize| &source[tokens[i].range.clone()];
    let mut pairs = vec![0; tokens.len()];
    let mut stack = Vec::new();
    let mut widths = vec![0usize];
    let mut complex = vec![0usize];
    for (i, token) in tokens.iter().enumerate() {
        widths.push(widths[i] + text(i).chars().count() + 1);
        complex.push(complex[i] + usize::from(token.kind == Kind::Comment || text(i) == "{"));
        match text(i) {
            "(" | "[" | "{" => {
                if stack.len() >= 128 {
                    return None;
                }
                stack.push(i);
            }
            ")" | "]" | "}" => {
                let open = stack.pop()?;
                if !matches!((text(open), text(i)), ("(", ")") | ("[", "]") | ("{", "}")) {
                    return None;
                }
                pairs[open] = i;
            }
            _ => {}
        }
    }
    if !stack.is_empty() {
        return None;
    }
    let mut output = String::with_capacity(source.len());
    let mut frames: Vec<Frame> = Vec::new();
    let mut indent = 0;
    let mut column = 0;
    for (i, token) in tokens.iter().enumerate() {
        let raw = text(i);
        let previous = i.checked_sub(1).map(text).unwrap_or("");
        let gap = i
            .checked_sub(1)
            .map(|p| &source[tokens[p].range.end..token.range.start])
            .unwrap_or("");
        if token.kind == Kind::Comment {
            if gap.contains('\n') {
                newline(&mut output, &mut column);
            } else if column > 0 {
                space(&mut output, &mut column);
            }
            write(&mut output, &mut column, indent, raw);
            if raw.starts_with("//") || raw.contains('\n') || gap.contains('\n') {
                newline(&mut output, &mut column);
            }
            continue;
        }
        if matches!(raw, ")" | "]" | "}") {
            let frame = frames.pop()?;
            if frame.multiline {
                indent -= 1;
                newline(&mut output, &mut column);
            }
        }
        if column > 0 {
            let separated = !gap.is_empty() || matches!(previous, "," | ":") || raw == "{";
            if separated && !matches!(raw, "," | ":" | "." | ")" | "]" | "}") && previous != "." {
                space(&mut output, &mut column);
            }
        }
        write(&mut output, &mut column, indent, raw);
        match raw {
            "(" | "[" | "{" => {
                let end = pairs[i];
                let multiline = raw == "{"
                    || column + widths[end + 1] - widths[i + 1] > 100
                    || complex[end] > complex[i + 1];
                frames.push(Frame {
                    multiline,
                    block: raw == "{",
                    dialogue: false,
                });
                if multiline {
                    indent += 1;
                    newline(&mut output, &mut column);
                }
            }
            "," => {
                let next = tokens.get(i + 1);
                let after = tokens.get(i + 2).map(|_| text(i + 2));
                let dialogue_tail = next.is_some_and(|next| next.kind == Kind::Identifier)
                    && ((matches!(after, Some("," | "}"))
                        && !matches!(text(i + 1), "return" | "break"))
                        || (matches!(
                            text(i + 1),
                            "volume" | "concat" | "auto" | "inherit_speaker"
                        ) && after == Some(":")));
                if let Some(frame) = frames.last_mut()
                    && frame.multiline
                    && !(frame.block && frame.dialogue && dialogue_tail)
                {
                    frame.dialogue = false;
                    let inline_comment = next.is_some_and(|next| {
                        next.kind == Kind::Comment
                            && !source[token.range.end..next.range.start].contains('\n')
                    });
                    if !inline_comment {
                        newline(&mut output, &mut column);
                    }
                }
            }
            "}" if tokens.get(i + 1).is_some()
                && !matches!(text(i + 1), "," | ")" | "]" | "else") =>
            {
                newline(&mut output, &mut column);
                if frames.is_empty() {
                    output.push('\n');
                }
            }
            _ if token.kind == Kind::String => {
                if let Some(frame) = frames.last_mut()
                    && frame.block
                {
                    frame.dialogue = true;
                }
            }
            _ => {}
        }
    }
    while output.ends_with([' ', '\n', '\r']) {
        output.pop();
    }
    if !output.is_empty() {
        output.push('\n');
    }
    // Retokenization protects adjacent duration units, operators and comments.
    let (formatted, errors) = lex(&output);
    let mut significant = formatted
        .iter()
        .filter(|token| token.kind != Kind::Whitespace);
    if !errors.is_empty()
        || !tokens.iter().all(|original| {
            significant.next().is_some_and(|new| {
                original.kind == new.kind
                    && source[original.range.clone()] == output[new.range.clone()]
            })
        })
        || significant.next().is_some()
    {
        return None;
    }
    Some(output)
}

struct Frame {
    multiline: bool,
    block: bool,
    dialogue: bool,
}

fn newline(output: &mut String, column: &mut usize) {
    while output.ends_with(' ') {
        output.pop();
    }
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    *column = 0;
}
fn space(output: &mut String, column: &mut usize) {
    if !output.ends_with(' ') {
        output.push(' ');
        *column += 1;
    }
}
fn write(output: &mut String, column: &mut usize, indent: usize, raw: &str) {
    if *column == 0 {
        output.push_str(&"  ".repeat(indent));
        *column = indent * 2;
    }
    output.push_str(raw);
    *column = if let Some((_, last)) = raw.rsplit_once('\n') {
        last.chars().count()
    } else {
        *column + raw.chars().count()
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_native_scenes;

    #[test]
    fn wraps_nested_calls_preserves_tokens_comments_and_semantics() {
        let source = "// 中文\nscene start { sprite(hero, face, position: right(x: 500, y: 20), layout: viewport(height: 0.85), light: false, transition: crossfade(300ms)), // keep\n\"文字[wait=1000]与 \\\"引号\\\"\", voice, volume: 0.8, auto: true, if (true) { wait(1s) } else { wait(2s) } }";
        let formatted = format_native_source(source).unwrap();
        assert!(formatted.contains("sprite(\n    hero,\n"));
        assert!(
            formatted
                .contains("\"文字[wait=1000]与 \\\"引号\\\"\", voice, volume: 0.8, auto: true,\n")
        );
        assert!(formatted.contains("// keep\n"));
        assert!(formatted.contains("} else {\n    wait(2s)\n  }"));
        assert_eq!(format_native_source(&formatted).unwrap(), formatted);
        let before = parse_native_scenes(source);
        let after = parse_native_scenes(&formatted);
        assert!(
            before[0].report.diagnostics.is_empty(),
            "{:?}",
            before[0].report.diagnostics
        );
        assert!(
            after[0].report.diagnostics.is_empty(),
            "{:?}",
            after[0].report.diagnostics
        );
        assert_eq!(before[0].report.actions, after[0].report.actions);
    }

    #[test]
    fn incomplete_input_is_not_formatted() {
        for source in [
            "scene a {",
            "scene a { \"unterminated }",
            "scene a { /* unfinished",
            "scene a { wait(1s] }",
            "scene a { ? }",
        ] {
            assert!(format_native_source(source).is_none());
        }
    }

    #[test]
    fn short_calls_and_multiline_comments_stay_readable() {
        let source = "/* first\nsecond */\r\nscene a {wait(1s),\"a\"}\r\nscene b {wait(2s)}";
        let formatted = format_native_source(source).unwrap();
        assert_eq!(
            formatted,
            "/* first\nsecond */\nscene a {\n  wait(1s),\n  \"a\"\n}\n\nscene b {\n  wait(2s)\n}\n"
        );
        assert_eq!(format_native_source(&formatted), Some(formatted));
    }
}
