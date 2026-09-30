use super::*;
use gpui_kit::Task;
use gpui_kit::base::input::{CompletionProvider, Rope};
use keine_loader::{NativeTokenKind, parse_native_document};
use lsp_types::{
    CompletionContext, CompletionResponse, InlineCompletionContext, InlineCompletionItem,
    InlineCompletionResponse,
};

pub(super) struct ShouCompletion {
    pub root: PathBuf,
    pub relative: PathBuf,
}

pub(super) fn schedule_syntax_check(
    editor: Entity<EditorState>,
    marks: Option<gpui_kit::base::input::TextDecorationCollection>,
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> gpui_kit::Task<()> {
    // One owned task per document panel: dropping it cancels debounce/work and
    // prevents an older parse from painting diagnostics onto newer source.
    if let Some(marks) = &marks {
        marks.clear(cx);
    }
    editor.update(cx, |editor, cx| {
        editor.clear_diagnostic_popover(cx);
        if let Some(diagnostics) = editor.diagnostics_mut() {
            diagnostics.clear();
        }
        cx.notify();
    });
    cx.spawn_in(window, async move |_, cx| {
        cx.background_executor()
            .timer(Duration::from_millis(180))
            .await;
        let source = editor.read_with(cx, |editor, _| editor.value().to_string());
        let checked = source.clone();
        let diagnostics = cx
            .background_executor()
            .spawn(async move { syntax_diagnostics(&checked) })
            .await;
        let decorations = editor.update(cx, |editor, cx| {
            if editor.value().as_ref() != source {
                return None;
            }
            let text = editor.text().clone();
            let decorations = diagnostics
                .iter()
                .map(|diagnostic| {
                    let color: Hsla = rgb(match diagnostic.severity {
                        gpui_kit::base::input::DiagnosticSeverity::Warning => 0xd2aa62,
                        _ => 0xdb7780,
                    })
                    .into();
                    gpui_kit::base::input::TextDecoration::new(
                        text.position_to_offset(&diagnostic.range.start)
                            ..text.position_to_offset(&diagnostic.range.end),
                        gpui_kit::HighlightStyle {
                            background_color: Some(color.opacity(0.18)),
                            ..Default::default()
                        },
                    )
                })
                .collect();
            if let Some(set) = editor.diagnostics_mut() {
                set.reset(&text);
                set.extend(diagnostics);
            }
            cx.notify();
            Some(decorations)
        });
        if let (Some(marks), Some(decorations)) = (marks, decorations) {
            let _ = cx.update(|_, cx| marks.set(decorations, cx));
        }
    })
}

fn syntax_diagnostics(source: &str) -> Vec<gpui_kit::base::input::Diagnostic> {
    use gpui_kit::base::input::{Diagnostic, DiagnosticSeverity};
    let lines = source.lines().collect::<Vec<_>>();
    let mut diagnostics = parse_native_document(source)
        .diagnostics
        .into_iter()
        .map(|diagnostic| {
            let mut line = diagnostic
                .span
                .line
                .saturating_sub(1)
                .min(lines.len().saturating_sub(1));
            // EOF and incomplete statements can point beyond the last glyph.
            // Keep their diagnostic visible on the nearest preceding glyph.
            while line > 0 && lines.get(line).is_none_or(|text| text.is_empty()) {
                line -= 1;
            }
            let text = lines.get(line).copied().unwrap_or("");
            let column = diagnostic
                .span
                .column
                .saturating_sub(1)
                .min(text.chars().count().saturating_sub(1));
            // The pinned GPUI RopeExt uses Unicode scalar columns, even though
            // its API carries lsp_types::Position. Do not convert to UTF-16.
            let start = column;
            let width = 1;
            Diagnostic::new(
                Position::new(line as u32, start as u32)
                    ..Position::new(line as u32, (start + width) as u32),
                diagnostic.message,
            )
            .with_source("shou")
            .with_severity(match diagnostic.level {
                keine_loader::DiagnosticLevel::Error => DiagnosticSeverity::Error,
                keine_loader::DiagnosticLevel::Warning => DiagnosticSeverity::Warning,
            })
        })
        .collect::<Vec<_>>();
    // DiagnosticSet's range cursor expects source order, while the parser may
    // append lexical diagnostics after statement diagnostics.
    diagnostics.sort_by_key(|diagnostic| diagnostic.range.start);
    diagnostics
}

impl CompletionProvider for ShouCompletion {
    fn completions(
        &self,
        _: &Rope,
        _: usize,
        _: CompletionContext,
        _: &mut Window,
        _: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        Task::ready(Ok(CompletionResponse::Array(Vec::new())))
    }

    fn is_completion_trigger(&self, _: usize, _: &str, _: &mut App) -> bool {
        false
    }

    fn inline_completion_debounce(&self) -> Duration {
        Duration::from_millis(120)
    }

    fn inline_completion(
        &self,
        rope: &Rope,
        offset: usize,
        _: InlineCompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<InlineCompletionResponse>> {
        let source = rope.to_string();
        let index = cx.global::<EditorDocuments>().authoring_ref(&self.root);
        let fallback = AuthoringIndex::default();
        let suffix = suggestion(
            &source,
            offset,
            index.unwrap_or(&fallback),
            |command, field, fields| {
                let key = SourceInspectorKey {
                    path: self.relative.clone(),
                    block_start: 0,
                    kind: BlockKind::Command,
                    command: command.into(),
                    fields: fields.to_vec(),
                };
                let documents = cx.global::<EditorDocuments>();
                let projection = (command == "track" && field.key == "0")
                    .then(|| documents.projection(&self.root, &self.relative, &source));
                let mut values = crate::authoring::fields::field_options(
                    &self.root,
                    &key,
                    field,
                    index,
                    projection
                        .as_deref()
                        .map(|projection| (source.as_str(), projection)),
                )
                .into_iter()
                .map(|option| option.value)
                .collect::<Vec<_>>();
                if matches!(command, "goto" | "call") && field.key == "0" {
                    values.extend(
                        index
                            .into_iter()
                            .flat_map(|index| &index.scenes)
                            .map(|scene| scene.name.clone()),
                    );
                }
                values
            },
        );
        Task::ready(Ok(InlineCompletionResponse::Array(
            suffix
                .into_iter()
                .map(|insert_text| InlineCompletionItem {
                    insert_text,
                    insert_text_format: None,
                    filter_text: None,
                    range: None,
                    command: None,
                })
                .collect(),
        )))
    }
}

// Only insert a suffix at a token boundary. Existing text, narration and comments
// are never replaced. GPUI's 120 ms cancellable request owns the visible ghost;
// acceptance uses its native edit transaction (including undo and Change events).
fn suggestion(
    source: &str,
    offset: usize,
    index: &AuthoringIndex,
    mut values: impl FnMut(&str, &SourceField, &[SourceField]) -> Vec<String>,
) -> Option<String> {
    let prefix = source.get(..offset)?;
    if source.get(offset..)?.chars().next().is_some_and(word_char) {
        return None;
    }
    let lexical = keine_loader::native_tokens(prefix);
    if lexical.last().is_some_and(|token| {
        token.range.end == offset
            && match token.kind {
                NativeTokenKind::Comment => true,
                NativeTokenKind::String => {
                    !prefix[token.range.clone()].ends_with('"') || token.range.len() == 1
                }
                _ => false,
            }
    }) {
        return None;
    }
    // Dialogue tails use the same parameter vocabulary without parentheses.
    let line = prefix.rsplit('\n').next().unwrap_or(prefix);
    let line_tokens = parse_native_document(line).tokens;
    if let Some(body) = line_tokens
        .iter()
        .find(|token| token.kind == NativeTokenKind::String)
        && (line[..body.range.start].trim().is_empty()
            || line[..body.range.start].trim_end().ends_with(':'))
    {
        let tail = line.get(body.range.end..)?;
        if tail.starts_with(',') {
            let parts = tail.split(',').skip(1).collect::<Vec<_>>();
            let raw = parts.last()?.trim();
            if let Some((name, typed)) = raw.split_once(':') {
                if matches!(name.trim(), "concat" | "auto" | "inherit_speaker") {
                    return suffix_for(typed.trim(), ["true", "false"]);
                }
                return None;
            }
            let mut candidates = ["volume", "concat", "auto", "inherit_speaker"]
                .into_iter()
                .filter(|name| {
                    !parts[..parts.len().saturating_sub(1)]
                        .iter()
                        .any(|part| part.trim_start().starts_with(&format!("{name}:")))
                })
                .map(|name| format!("{name}: "))
                .collect::<Vec<_>>();
            if parts.len() == 1 {
                candidates.extend(
                    index
                        .assets
                        .iter()
                        .filter(|asset| asset.kind == AssetKind::Voice)
                        .map(|asset| asset.id.clone()),
                );
            }
            return suffix_for(raw, candidates.iter().map(String::as_str));
        }
    }
    let tokens = lexical
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                NativeTokenKind::Whitespace | NativeTokenKind::Comment
            )
        })
        .collect::<Vec<_>>();
    // Each nested delimiter owns its argument cursor; a comma inside style(...)
    // or a list cannot become a parameter of the enclosing command.
    let mut stack: Vec<(char, String, usize, usize, Vec<SourceField>)> = Vec::new();
    for (position, token) in tokens.iter().enumerate() {
        let raw = &prefix[token.range.clone()];
        match raw {
            "(" | "[" | "{" => {
                let mut start = token.range.start;
                for preceding in tokens[..position].iter().rev() {
                    let value = &prefix[preceding.range.clone()];
                    if preceding.range.end != start
                        || !(preceding.kind == NativeTokenKind::Identifier || value == ".")
                    {
                        break;
                    }
                    start = preceding.range.start;
                }
                stack.push((
                    raw.chars().next()?,
                    prefix[start..token.range.start].into(),
                    token.range.end,
                    0,
                    Vec::new(),
                ));
            }
            ")" | "]" | "}" => {
                stack.pop();
            }
            "," => {
                if let Some((_, _, start, position, fields)) =
                    stack.last_mut().filter(|frame| frame.0 == '(')
                {
                    fields.push(argument_field(
                        &prefix[*start..token.range.start],
                        *position,
                    ));
                    *start = token.range.end;
                    *position += 1;
                }
            }
            _ => {}
        }
    }
    let inside_body = stack.iter().any(|frame| frame.0 == '{');
    if let Some(('(', command, start, position, mut fields)) = stack.pop() {
        let raw = prefix[start..].trim();
        let field = argument_field(raw, position);
        let typed = field.value.as_str();
        if typed.chars().any(|character| !word_char(character)) {
            return None;
        }
        fields.push(field.clone());
        let options = values(&command, &field, &fields);
        if let Some(suffix) = suffix_for(typed, options.iter().map(String::as_str)) {
            return Some(suffix);
        }
        if !raw.is_empty() && !raw.contains(':') {
            return suffix_for(
                raw,
                crate::authoring::fields::command_argument_names(&command)
                    .into_iter()
                    .filter(|name| !fields.iter().any(|field| field.key == *name))
                    .map(|name| format!("{name}: "))
                    .collect::<Vec<_>>()
                    .iter()
                    .map(String::as_str),
            );
        }
        return None;
    }
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, character)| !word_char(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    let typed = &prefix[start..];
    if typed.is_empty() {
        return None;
    }
    let before = prefix[..start].trim_end();
    if !before.is_empty()
        && !before.ends_with(['{', '}', ',', ';', '\n'])
        && !prefix[..start].ends_with('\n')
        && !prefix[..start]
            .rsplit_once('\n')
            .is_some_and(|(_, line)| line.trim().is_empty())
    {
        return None;
    }
    if !inside_body {
        let name = if source.contains("scene start") {
            "new_scene"
        } else {
            "start"
        };
        let mut candidate = name.to_owned();
        let mut number = 2;
        while index.scenes.iter().any(|scene| scene.name == candidate) {
            candidate = format!("{name}_{number}");
            number += 1;
        }
        return suffix_for(
            typed,
            [format!("scene {candidate} {{\n  \n}}")]
                .iter()
                .map(String::as_str),
        );
    }
    let indent = prefix[..start].rsplit('\n').next().unwrap_or("");
    let templates = InsertKind::ALL
        .into_iter()
        .filter_map(|kind| insertion_statement(source, kind, index, indent).ok())
        .collect::<Vec<_>>();
    // When the user already typed the opening parenthesis, complete just the
    // identifier; otherwise the existing Block insertion templates provide defaults.
    let existing_open = source[offset..].starts_with('(');
    let mut candidates = templates
        .iter()
        .map(|template| {
            if existing_open {
                template.split('(').next().unwrap_or(template)
            } else {
                template.as_str()
            }
        })
        .collect::<Vec<_>>();
    candidates.extend(
        crate::authoring::commands::CONTEXTUAL_COMPLETIONS
            .iter()
            .copied(),
    );
    // Still offer command names when a required project asset is not yet defined.
    for kind in InsertKind::ALL {
        if let Some(name) = kind.source_name() {
            candidates.push(name);
        }
    }
    let suffix = suffix_for(typed, candidates)?;
    Some(suffix)
}

fn word_char(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '.' | '-' | '/')
}

fn argument_field(raw: &str, position: usize) -> SourceField {
    let (key, value) = raw
        .split_once(':')
        .map_or((position.to_string(), raw.trim()), |(key, value)| {
            (key.trim().into(), value.trim())
        });
    SourceField {
        key,
        value: value.into(),
        range: 0..0,
        quoted: false,
        insertion: None,
        insertion_suffix: None,
    }
}

fn suffix_for<'a>(typed: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    candidates
        .into_iter()
        .filter_map(|candidate| candidate.strip_prefix(typed))
        .find(|suffix| !suffix.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn complete(source: &str) -> Option<String> {
        suggestion(
            source,
            source.len(),
            &AuthoringIndex::default(),
            |_, _, _| Vec::new(),
        )
    }
    #[test]
    fn dotted_commands_and_current_arguments() {
        assert!(complete("sce").unwrap().starts_with("ne start {"));
        assert_eq!(complete("camera.m"), None);
        assert!(
            complete("scene start {\n  camera.m")
                .unwrap()
                .starts_with("ove(scene,")
        );
        assert_eq!(
            complete("scene start {\n  camera.shake(scene, amplitude_r"),
            Some("andomness: ".into())
        );
        assert_eq!(
            complete("scene start {\n  camera.move(scene, x: 1, x"),
            None
        );
        assert_eq!(
            complete("scene start {\n  camera.move(scene, tween: [x,"),
            None
        );
    }
    #[test]
    fn literals_comments_unicode_and_mid_token_are_preserved() {
        assert_eq!(complete("scene start {\n  \"对白 camera.m"), None);
        assert_eq!(complete("// camera.m"), None);
        assert_eq!(
            complete("scene start {\n  camera.move(scene, x: \"camera.m"),
            None
        );
        let source = "scene start {\n  camera.move";
        assert_eq!(
            suggestion(
                source,
                source.len() - 2,
                &AuthoringIndex::default(),
                |_, _, _| Vec::new()
            ),
            None
        );
        assert_eq!(
            suggestion("中文", 1, &AuthoringIndex::default(), |_, _, _| Vec::new(
            )),
            None
        );
    }
    #[test]
    fn dialogue_tail_completion_preserves_body_and_existing_options() {
        assert_eq!(
            complete("scene start {\n  hero: \"你好\", con"),
            Some("cat: ".into())
        );
        assert_eq!(
            complete("scene start {\n  \"你好\", concat: f"),
            Some("alse".into())
        );
        assert_eq!(complete("scene start {\n  \"你好\", auto: true, au"), None);
    }

    #[test]
    fn contextual_values_and_nested_calls() {
        let source = "scene start { camera.move(sc";
        assert_eq!(
            suggestion(
                source,
                source.len(),
                &AuthoringIndex::default(),
                |command, field, _| {
                    assert_eq!((command, field.key.as_str()), ("camera.move", "0"));
                    vec!["scene".into()]
                }
            ),
            Some("ene".into())
        );
        assert_eq!(
            complete("scene start { sprite.focus.configure(speaking: style(alpha: 1), dur"),
            Some("ation: ".into())
        );
    }

    #[test]
    fn syntax_errors_use_the_native_parser_and_clear_after_repair() {
        assert!(!syntax_diagnostics("scene start { camera.move(scene, x: ) }").is_empty());
        let source = "scene start {\n  camera.move(scene, x: )\n}\n";
        assert!(
            syntax_diagnostics(source)
                .iter()
                .any(|diagnostic| diagnostic.range.start.line == 1
                    && diagnostic.range.start.character == 24)
        );
        assert!(syntax_diagnostics("scene start { camera.move(scene, x: 1) }").is_empty());
        let diagnostics = syntax_diagnostics("scene start { \"中文\", camera.move(scene, x: ) }");
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.range.start.line == 0)
        );
    }

    #[test]
    fn empty_text_deletion_is_bounded_and_owns_its_ending() {
        let source = "scene start {\n  \"kept\",\n  hero: \"\",\n  text.box(visible: false, auto: true),\n  wait.advance()\n}\n";
        let text_start = source.find("hero: \"\"").unwrap() + "hero: \"".len();
        let projection = EiyashouProjection::parse(source);
        let edited = projection.delete_empty_text(source, text_start).unwrap();
        assert!(edited.contains("\"kept\"") && edited.contains("wait.advance()"));
        assert!(
            !edited.contains("hero:") && !edited.contains("text.box(visible: false, auto: true)")
        );
        assert!(
            projection
                .delete_empty_text(source, source.find("kept").unwrap())
                .is_none()
        );
        assert!(
            projection
                .delete_empty_text(source, text_start + 1)
                .is_none()
        );
        assert_eq!(complete("scene start {\n  bg"), Some("m".into()));
    }
}
