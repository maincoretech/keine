use super::*;
use gpui_kit::Task;
use gpui_kit::base::input::{CompletionProvider, HoverProvider, Rope};
use keine_loader::{NativeTokenKind, native_text_argument_names, parse_native_document};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    Documentation, InlineCompletionContext, InlineCompletionItem, InlineCompletionResponse,
    MarkupContent, MarkupKind, TextEdit,
};

pub(super) struct ShouCompletion {
    pub root: PathBuf,
    pub relative: PathBuf,
}

pub(super) fn schedule_syntax_check(
    editor: Entity<EditorState>,
    marks: Option<gpui_kit::base::input::TextDecorationCollection>,
    project: (Arc<AuthoringIndex>, PathBuf),
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> gpui_kit::Task<()> {
    use gpui_kit::EntityInputHandler;
    // Snapshot confirmed text before spawning work. Composition updates are
    // local to the input control; they must not become diagnostic input.
    let source = editor.update(cx, |editor, cx| {
        if editor.marked_text_range(window, cx).is_some() {
            return None;
        }
        editor.clear_diagnostic_popover(cx);
        if let Some(diagnostics) = editor.diagnostics_mut() {
            diagnostics.clear();
        }
        cx.notify();
        Some(editor.value().to_string())
    });
    let Some(source) = source else {
        return Task::ready(());
    };
    if let Some(marks) = &marks {
        marks.clear(cx);
    }
    // One owned task per panel: newer confirmed edits cancel obsolete work.
    cx.spawn_in(window, async move |_, cx| {
        let checked = source.clone();
        let diagnostics = cx
            .background_executor()
            .spawn(async move {
                let mut diagnostics = syntax_diagnostics(&checked);
                diagnostics.extend(project_diagnostics(&checked, &project.0, &project.1));
                diagnostics.sort_by(|a, b| {
                    a.range
                        .start
                        .cmp(&b.range.start)
                        .then(a.range.end.cmp(&b.range.end))
                        .then(a.message.cmp(&b.message))
                });
                diagnostics.dedup_by(|a, b| {
                    a.range == b.range && a.message == b.message && a.severity == b.severity
                });
                diagnostics
            })
            .await;
        let decorations = editor
            .update_in(cx, |editor, window, cx| {
                if editor.marked_text_range(window, cx).is_some()
                    || editor.value().as_ref() != source
                {
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
                                underline: Some(gpui_kit::UnderlineStyle {
                                    thickness: px(1.),
                                    color: Some(color),
                                    wavy: true,
                                }),
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
            })
            .ok()
            .flatten();
        if let (Some(marks), Some(decorations)) = (marks, decorations) {
            let _ = cx.update(|_, cx| marks.set(decorations, cx));
        }
    })
}

fn project_diagnostics(
    source: &str,
    index: &AuthoringIndex,
    path: &Path,
) -> Vec<gpui_kit::base::input::Diagnostic> {
    use gpui_kit::base::input::{Diagnostic, DiagnosticSeverity};
    let lines = keine_loader::SourceLineIndex::new(source);
    index
        .problems
        .iter()
        .filter(|problem| problem.path == path)
        .map(|problem| {
            let start = lines.offset(
                source,
                problem.line.saturating_sub(1),
                problem.column.saturating_sub(1),
            );
            let end = index
                .asset_references
                .iter()
                .find(|reference| {
                    reference.path == path
                        && reference.line == problem.line
                        && reference.column == problem.column
                })
                .and_then(|reference| reference.range.as_ref())
                .filter(|range| {
                    range.start == start
                        && range.end <= source.len()
                        && source.is_char_boundary(range.end)
                })
                .map_or_else(
                    || start + source[start..].chars().next().map_or(0, char::len_utf8),
                    |range| range.end,
                );
            let end = lines.span(source, end);
            let start = lines.span(source, start);
            Diagnostic::new(
                Position::new(
                    start.line.saturating_sub(1) as u32,
                    start.column.saturating_sub(1) as u32,
                )
                    ..Position::new(
                        end.line.saturating_sub(1) as u32,
                        end.column.saturating_sub(1) as u32,
                    ),
                problem.message.clone(),
            )
            .with_source("project")
            .with_severity(match problem.severity {
                ProblemSeverity::Error => DiagnosticSeverity::Error,
                ProblemSeverity::Warning => DiagnosticSeverity::Warning,
            })
        })
        .collect()
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
        text: &Rope,
        offset: usize,
        _: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        Task::ready(Ok(CompletionResponse::Array(self.items(
            &text.to_string(),
            offset,
            cx,
        ))))
    }

    fn is_completion_trigger(&self, _: usize, text: &str, _: &mut App) -> bool {
        !text.trim().is_empty()
            && text
                .chars()
                .all(|ch| word_char(ch) || matches!(ch, '(' | ':' | ',' | ' '))
    }

    fn inline_completion_debounce(&self) -> Duration {
        Duration::ZERO
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
        let suffix = self.candidates(&source, offset, cx).and_then(|set| {
            if set.range.end != offset || set.range.start == offset {
                return None;
            }
            suffix_for(
                &source[set.range.start..offset],
                set.values.iter().map(String::as_str),
            )
        });
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

impl ShouCompletion {
    fn candidates(&self, source: &str, offset: usize, cx: &App) -> Option<CompletionCandidates> {
        let index = cx.global::<EditorDocuments>().authoring_ref(&self.root);
        let fallback = AuthoringIndex::default();
        completion_candidates(
            source,
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
                    .then(|| documents.projection(&self.root, &self.relative, source));
                let mut values = crate::authoring::fields::field_options(
                    &self.root,
                    &key,
                    field,
                    index,
                    projection.as_deref().map(|projection| (source, projection)),
                )
                .into_iter()
                .filter(|option| {
                    crate::authoring::fields::source_asset_kind(&key, field).is_none()
                        || option.value == "none"
                        || index
                            .into_iter()
                            .flat_map(|index| &index.assets)
                            .any(|asset| asset.id == option.value)
                })
                .map(|option| option.value)
                .collect::<Vec<_>>();
                values.extend(
                    crate::authoring::fields::source_field_choices(&key, field)
                        .iter()
                        .map(|value| (*value).to_owned()),
                );
                if command == "sprite.sequence" && field.key == "speaker" {
                    values.extend(
                        index
                            .into_iter()
                            .flat_map(|index| &index.characters)
                            .map(|character| {
                                format!("\"{}\"", escape_eiyashou_string(&character.id))
                            }),
                    );
                }
                if matches!(command, "set" | "input.request") && field.key == "0" {
                    values.extend(variable_names(source));
                    values.extend(
                        index
                            .into_iter()
                            .flat_map(|index| &index.variables)
                            .map(|(_, name)| name.clone()),
                    );
                }
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
        )
    }

    fn items(&self, source: &str, offset: usize, cx: &App) -> Vec<CompletionItem> {
        let Some(set) = self.candidates(source, offset, cx) else {
            return Vec::new();
        };
        let index = cx.global::<EditorDocuments>().authoring_ref(&self.root);
        completion_items(source, set, index)
    }
}

struct CompletionCandidates {
    range: Range<usize>,
    values: Vec<String>,
    command: String,
}

fn candidate_set(
    source: &str,
    offset: usize,
    index: &AuthoringIndex,
    typed: &str,
    candidates: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<CompletionCandidates> {
    let start = offset.checked_sub(typed.len())?;
    let end = source[offset..]
        .char_indices()
        .take_while(|(_, ch)| word_char(*ch))
        .last()
        .map_or(offset, |(index, ch)| offset + index + ch.len_utf8());
    let mut values = candidates
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .filter(|value| {
            value.starts_with(typed)
                || !typed.is_empty()
                    && index.assets.iter().any(|asset| {
                        &asset.id == value
                            && asset.path.file_name().is_some_and(|name| {
                                name.to_string_lossy()
                                    .to_lowercase()
                                    .contains(&typed.to_lowercase())
                            })
                    })
        })
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
    Some(CompletionCandidates {
        range: start..end,
        values,
        command: String::new(),
    })
}

fn completion_items(
    source: &str,
    set: CompletionCandidates,
    index: Option<&AuthoringIndex>,
) -> Vec<CompletionItem> {
    let lines = keine_loader::SourceLineIndex::new(source);
    let position = |offset| {
        let span = lines.span(source, offset);
        lsp_types::Position::new(span.line as u32 - 1, span.column as u32 - 1)
    };
    set.values
        .into_iter()
        .map(|value| {
            let label = value
                .lines()
                .next()
                .unwrap_or(&value)
                .split('(')
                .next()
                .unwrap_or(&value)
                .trim_end()
                .to_owned();
            let asset = index
                .into_iter()
                .flat_map(|index| &index.assets)
                .find(|asset| asset.id == value);
            let character = index
                .into_iter()
                .flat_map(|index| &index.characters)
                .find(|character| value.starts_with(&format!("{}:", character.id)));
            let detail = asset
                .map(|asset| asset.path.display().to_string())
                .or_else(|| character.map(|character| character.name.clone()))
                .unwrap_or_else(|| {
                    if value.ends_with(": ") {
                        "Parameter".into()
                    } else {
                        "EYS".into()
                    }
                });
            let description = if value.ends_with(": ") {
                parameter_help(&set.command, value.trim_end_matches(": "))
            } else {
                format!("{}\n\n```eiyashou\n{value}\n```", detail)
            };
            CompletionItem {
                label,
                detail: Some(detail),
                kind: Some(if asset.is_some() {
                    CompletionItemKind::FILE
                } else if value.ends_with(": ") {
                    CompletionItemKind::FIELD
                } else {
                    CompletionItemKind::FUNCTION
                }),
                documentation: Some(Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: description,
                })),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: lsp_types::Range::new(
                        position(set.range.start),
                        position(set.range.end),
                    ),
                    new_text: value,
                })),
                ..Default::default()
            }
        })
        .collect()
}

fn parameter_help(command: &str, name: &str) -> String {
    let field = argument_field(&format!("{name}: "), 0);
    let context = SourceInspectorKey {
        path: PathBuf::new(),
        block_start: 0,
        kind: BlockKind::Command,
        command: command.into(),
        fields: vec![field.clone()],
    };
    let label = crate::authoring::fields::source_field_label(&context, &field);
    let choices = crate::authoring::fields::source_field_choices(&context, &field);
    let description = match (command, name) {
        ("sprite.sequence", "mode") => {
            "blink: rest on the first frame between cycles. talk: animate while the named speaker's text is typing. Both require at least two frames; omit loop."
        }
        ("sprite.sequence", "speaker") => {
            "Character ID or speaker name in quotes; required by mode: talk."
        }
        ("sprite.sequence", "interval") => {
            "Rest between blink cycles, e.g. 3s. Only used by mode: blink."
        }
        ("sprite.sequence", "fps") => {
            "Frames per second. Omit when every frame specifies duration."
        }
        ("se", "loop") => "Repeat playback. true requires an explicit id; default false.",
        ("se", "id") => "Playback ID for se.stop(id). An ID alone does not enable looping.",
        ("se" | "bgm", "fade") => {
            "Fade in when playing; fade out when stopping. Starts with actual playback; default 0ms."
        }
        ("se", "fade_out") => {
            "Natural one-shot tail fade before the audio ends. Not valid for loops; default 0ms."
        }
        ("se.stop", "fade") => {
            "Fade out this playback ID before stopping. * stops every effect, including loops; default 0ms."
        }
        ("camera.effect" | "camera.move" | "camera.reset", "duration") => {
            "Transition time, not effect lifetime. 0ms is immediate; nonblocking transitions can be replaced by a later camera command."
        }
        (_, "light") => "Enable background-derived lighting for this sprite. false disables it.",
        (_, "scale") => "Uniform scale; omit scale_x and scale_y when using scale.",
        (_, "blocking") => "Wait for completion before running the next command.",
        _ => {
            "Named parameter. Use the shown source value; units and defaults follow the command model."
        }
    };
    let example = if let Some(control) = crate::authoring::fields::source_number(&context, &field) {
        crate::authoring::fields::source_input_commit(&context, &field, control.default.to_string())
    } else {
        choices.first().copied().unwrap_or("value").to_owned()
    };
    format!(
        "**{label}** — `{command}`\n\n{description}\n\n`{name}: {example}`{}",
        if choices.is_empty() {
            String::new()
        } else {
            format!("\n\nValues: {}", choices.join(", "))
        }
    )
}

impl HoverProvider for ShouCompletion {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _: &mut Window,
        _: &mut App,
    ) -> Task<anyhow::Result<Option<lsp_types::Hover>>> {
        Task::ready(Ok(source_hover(&text.to_string(), offset)))
    }
}

fn source_hover(source: &str, offset: usize) -> Option<lsp_types::Hover> {
    if !source.is_char_boundary(offset) {
        return None;
    }
    let tokens = keine_loader::native_tokens(source);
    let token = tokens.iter().find(|token| token.range.contains(&offset))?;
    if token.kind != NativeTokenKind::Identifier {
        return None;
    }
    let projection = EiyashouProjection::parse(source);
    let block = projection
        .scenes
        .iter()
        .flat_map(|scene| &scene.blocks)
        .filter(|block| block.statement_range.contains(&offset))
        .min_by_key(|block| block.statement_range.len())?;
    let statement = source.get(block.statement_range.clone())?;
    let command = statement.split('(').next()?.trim();
    let name = source.get(token.range.clone())?;
    let fields = projection.source_fields_for_block(source, block)?;
    let value = if fields.iter().any(|field| field.key == name)
        && source[token.range.end..].trim_start().starts_with(':')
    {
        parameter_help(command, name)
    } else {
        let signature = keine_loader::native_command_argument_names(command)?;
        if offset >= block.statement_range.start + command.len() {
            return None;
        }
        let visible = &signature[..signature.len().min(6)];
        format!(
            "`{command}({}{})`\n\nType inside the command for parameter suggestions.",
            visible.join(", "),
            if signature.len() > visible.len() {
                ", …"
            } else {
                ""
            }
        )
    };
    let lines = keine_loader::SourceLineIndex::new(source);
    let start = lines.span(source, token.range.start);
    let end = lines.span(source, token.range.end);
    Some(lsp_types::Hover {
        contents: lsp_types::HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(lsp_types::Range::new(
            Position::new(start.line as u32 - 1, start.column as u32 - 1),
            Position::new(end.line as u32 - 1, end.column as u32 - 1),
        )),
    })
}

#[cfg(test)]
fn suggestion(
    source: &str,
    offset: usize,
    index: &AuthoringIndex,
    values: impl FnMut(&str, &SourceField, &[SourceField]) -> Vec<String>,
) -> Option<String> {
    let set = completion_candidates(source, offset, index, values)?;
    if set.range.end != offset || set.range.start == offset {
        return None;
    }
    suffix_for(
        &source[set.range.start..offset],
        set.values.iter().map(String::as_str),
    )
}

// Only insert a suffix at a token boundary. Existing text, narration and comments
// are never replaced. GPUI's cancellable committed-input request owns the ghost;
// acceptance uses its native edit transaction (including undo and Change events).
fn completion_candidates(
    source: &str,
    offset: usize,
    index: &AuthoringIndex,
    mut values: impl FnMut(&str, &SourceField, &[SourceField]) -> Vec<String>,
) -> Option<CompletionCandidates> {
    let prefix = source.get(..offset)?;
    let lexical = keine_loader::native_tokens(prefix);
    if let Some(token) = lexical
        .last()
        .filter(|token| token.kind == NativeTokenKind::String)
        && prefix[..token.range.start].trim_end().ends_with("speaker:")
        && prefix[..token.range.start]
            .rfind("sprite.sequence(")
            .is_some()
        && (!prefix[token.range.clone()].ends_with('"') || token.range.len() == 1)
    {
        let values = index
            .characters
            .iter()
            .map(|character| format!("\"{}\"", escape_eiyashou_string(&character.id)));
        let mut set = candidate_set(source, offset, index, &prefix[token.range.start..], values)?;
        if source[set.range.end..].starts_with('"') {
            set.range.end += 1;
        }
        return Some(set);
    }
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
                    return candidate_set(source, offset, index, typed.trim(), ["true", "false"]);
                }
                return None;
            }
            let mut candidates = native_text_argument_names()
                .iter()
                .copied()
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
            return candidate_set(
                source,
                offset,
                index,
                raw,
                candidates.iter().map(String::as_str),
            );
        }
    }
    let word_start = prefix
        .char_indices()
        .rev()
        .find(|(_, ch)| !word_char(*ch))
        .map_or(0, |(start, ch)| start + ch.len_utf8());
    let before_word = prefix[..word_start].trim_end();
    if before_word.ends_with(['=', '+', '-', '*', '/', '>', '<', '!'])
        || before_word.ends_with("if") && prefix[..word_start].ends_with(char::is_whitespace)
    {
        return candidate_set(
            source,
            offset,
            index,
            &prefix[word_start..],
            variable_names(source)
                .into_iter()
                .chain(index.variables.iter().map(|(_, name)| name.clone())),
        );
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
    let mut closed_call = None;
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
                let (name, body_fields) = if raw == "{"
                    && position
                        .checked_sub(1)
                        .is_some_and(|previous| &prefix[tokens[previous].range.clone()] == ")")
                {
                    closed_call.take().unwrap_or_default()
                } else {
                    (prefix[start..token.range.start].to_owned(), Vec::new())
                };
                stack.push((raw.chars().next()?, name, token.range.end, 0, body_fields));
            }
            ")" | "]" | "}" => {
                closed_call = stack.pop().filter(|frame| frame.0 == '(').map(|mut frame| {
                    let last = &prefix[frame.2..token.range.start];
                    if !last.trim().is_empty() {
                        frame.4.push(argument_field(last, frame.3));
                    }
                    (frame.1, frame.4)
                });
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
    if let Some(('(', command, start, position, mut fields)) = stack.last().cloned() {
        stack.pop();
        let raw = prefix[start..].trim();
        let field = argument_field(raw, position);
        let typed = field.value.as_str();
        if typed.chars().any(|character| !word_char(character)) {
            return None;
        }
        fields.push(field.clone());
        let options = values(&command, &field, &fields);
        let mut options = options;
        if !raw.contains(':') {
            options.extend(
                stack
                    .iter()
                    .rev()
                    .find(|frame| frame.0 == '{')
                    .and_then(|frame| {
                        crate::authoring::fields::child_argument_names(&frame.1, &command, &frame.4)
                    })
                    .unwrap_or_else(|| crate::authoring::fields::command_argument_names(&command))
                    .into_iter()
                    .filter(|name| {
                        !fields.iter().any(|field| field.key == *name)
                            && !crate::authoring::fields::argument_conflicts(
                                &command, name, &fields,
                            )
                    })
                    .map(|name| format!("{name}: ")),
            );
        }
        let mut set = candidate_set(source, offset, index, typed, options)?;
        set.command = command;
        return Some(set);
    }
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, character)| !word_char(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    let typed = &prefix[start..];
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
        return candidate_set(
            source,
            offset,
            index,
            typed,
            [format!("scene {candidate} {{\n  \n}}")]
                .iter()
                .map(String::as_str),
        );
    }
    let indent = prefix[..start].rsplit('\n').next().unwrap_or("");
    let templates = InsertKind::ALL
        .into_iter()
        .filter(|kind| {
            kind.source_name()
                .is_none_or(|name| name.starts_with(typed))
        })
        .filter_map(|kind| insertion_statement(source, kind, index, indent).ok())
        .collect::<Vec<_>>();
    // When the user already typed the opening parenthesis, complete just the
    // identifier; otherwise the existing Block insertion templates provide defaults.
    let end = source[offset..]
        .char_indices()
        .take_while(|(_, ch)| word_char(*ch))
        .last()
        .map_or(offset, |(start, ch)| offset + start + ch.len_utf8());
    let existing_open = source[end..].starts_with('(');
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
            .copied()
            .filter(|candidate| {
                let child = candidate.split('(').next().unwrap_or(candidate);
                if matches!(child, "break" | "else {\n}") {
                    return true;
                }
                stack
                    .iter()
                    .rev()
                    .find(|frame| frame.0 == '{')
                    .is_some_and(|frame| {
                        keine_loader::native_child_command_argument_names(&frame.1, child).is_some()
                    })
            }),
    );
    // Still offer command names when a required project asset is not yet defined.
    for kind in InsertKind::ALL {
        if let Some(name) = kind.source_name()
            && !candidates
                .iter()
                .any(|candidate| candidate.split('(').next() == Some(name))
        {
            candidates.push(name);
        }
    }
    let mut candidates = candidates
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    candidates.extend(
        index
            .characters
            .iter()
            .map(|character| format!("{}: \"\"", character.id)),
    );
    candidate_set(source, offset, index, typed, candidates)
}

fn variable_names(source: &str) -> Vec<String> {
    let tokens = keine_loader::native_tokens(source)
        .into_iter()
        .filter(|token| {
            !matches!(
                token.kind,
                NativeTokenKind::Whitespace | NativeTokenKind::Comment
            )
        })
        .collect::<Vec<_>>();
    tokens
        .windows(2)
        .filter(|pair| {
            source.get(pair[0].range.clone()) == Some("let")
                && pair[1].kind == NativeTokenKind::Identifier
        })
        .map(|pair| source[pair[1].range.clone()].to_owned())
        .collect()
}

impl WorkbenchPanel {
    pub(super) fn show_source_completions(
        &mut self,
        _: &ShowSourceCompletions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui_kit::EntityInputHandler;
        let PanelContent::Document {
            root,
            relative,
            editor,
            ..
        } = &self.content
        else {
            cx.propagate();
            return;
        };
        if self.document.document_mode != DocumentMode::Text
            || relative
                .extension()
                .is_none_or(|extension| extension != "shou")
            || !editor.focus_handle(cx).is_focused(window)
        {
            cx.propagate();
            return;
        }
        let provider = ShouCompletion {
            root: root.clone(),
            relative: relative.clone(),
        };
        let shown = editor.update(cx, |editor, cx| {
            if editor.marked_text_range(window, cx).is_some() {
                return false;
            }
            let source = editor.value().to_string();
            let offset = editor.text().position_to_offset(&editor.cursor_position());
            let items = provider.items(&source, offset, cx);
            let shown = !items.is_empty();
            editor.present_completion_items(offset, "", items, cx);
            shown
        });
        if shown {
            cx.stop_propagation();
        } else {
            cx.propagate();
        }
    }
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
    fn menu(source: &str, offset: usize, index: &AuthoringIndex) -> Option<CompletionCandidates> {
        completion_candidates(source, offset, index, |command, field, fields| {
            let key = SourceInspectorKey {
                path: PathBuf::new(),
                block_start: 0,
                kind: BlockKind::Command,
                command: command.into(),
                fields: fields.to_vec(),
            };
            let mut values = crate::authoring::fields::field_options(
                Path::new(""),
                &key,
                field,
                Some(index),
                None,
            )
            .into_iter()
            .filter(|option| {
                crate::authoring::fields::source_asset_kind(&key, field).is_none()
                    || option.value == "none"
                    || index.assets.iter().any(|asset| asset.id == option.value)
            })
            .map(|option| option.value)
            .collect::<Vec<_>>();
            values.extend(
                crate::authoring::fields::source_field_choices(&key, field)
                    .iter()
                    .map(|value| (*value).into()),
            );
            values
        })
    }

    #[test]
    fn unified_se_completes_loop_and_hides_legacy_command() {
        assert_eq!(complete("scene start { se(click, lo"), Some("op: ".into()));
        let source = "scene start { se(click, loop: tr";
        let set = menu(source, source.len(), &AuthoringIndex::default()).unwrap();
        assert_eq!(set.values, ["true"]);
        let source = "scene start { se.";
        let set = menu(source, source.len(), &AuthoringIndex::default()).unwrap();
        assert!(set.values.iter().any(|value| value.starts_with("se.stop")));
        assert!(!set.values.iter().any(|value| value.starts_with("se.loop")));
    }

    #[test]
    fn menu_replaces_whole_token_and_preserves_existing_call_and_unicode_columns() {
        let source = "scene start { \"中文😀\", camera.move(all) }";
        let offset = source.find("camera.mo").unwrap() + "camera.mo".len();
        let set = menu(source, offset, &AuthoringIndex::default()).unwrap();
        assert_eq!(&source[set.range.clone()], "camera.move");
        assert_eq!(set.values, ["camera.move"]);
        let items = completion_items(source, set, None);
        let CompletionTextEdit::Edit(edit) = items[0].text_edit.as_ref().unwrap() else {
            panic!()
        };
        let line = keine_loader::SourceLineIndex::new(source);
        let start = line.offset(
            source,
            edit.range.start.line as usize,
            edit.range.start.character as usize,
        );
        let end = line.offset(
            source,
            edit.range.end.line as usize,
            edit.range.end.character as usize,
        );
        let mut edited = source.to_owned();
        edited.replace_range(start..end, &edit.new_text);
        assert_eq!(edited, source);
        assert!(items[0].documentation.is_some());
    }

    #[test]
    fn menu_values_use_resource_kind_and_filename_and_offer_all_boolean_choices() {
        use crate::authoring::AssetEntry;
        let index = AuthoringIndex {
            assets: [
                (AssetKind::Bgm, "music", "rain.opus"),
                (AssetKind::Figure, "face", "rain.webp"),
            ]
            .into_iter()
            .map(|(kind, id, path)| AssetEntry {
                kind,
                id: id.into(),
                path: path.into(),
                exists: true,
                tags: Vec::new(),
                reference_count: 0,
            })
            .collect(),
            ..Default::default()
        };
        let source = "scene start { bgm(rain";
        let set = menu(source, source.len(), &index).unwrap();
        assert_eq!(set.values, ["music"]);
        let source = "scene start { bgm(music, looped: ";
        assert_eq!(
            menu(source, source.len(), &index).unwrap().values,
            ["true", "false"]
        );
        let source = "scene start { sprite.sequence(hero, mode: blink, ";
        let values = menu(source, source.len(), &index).unwrap().values;
        assert!(values.contains(&"interval: ".into()));
        assert!(!values.contains(&"loop: ".into()) && !values.contains(&"speaker: ".into()));
    }

    #[test]
    fn menu_manual_empty_context_and_parent_children_are_bounded() {
        let index = AuthoringIndex::default();
        let source = "scene start {\n  ";
        let values = menu(source, source.len(), &index).unwrap().values;
        assert!(values.iter().any(|value| value.starts_with("camera.move(")));
        assert!(!values.iter().any(|value| value.starts_with("frame(")));
        let source = "scene start { sprite.sequence(hero) {\n  fr";
        assert_eq!(
            menu(source, source.len(), &index).unwrap().values,
            ["frame("]
        );
        for source in [
            "// camera.mo",
            "scene start { \"中文 bgm",
            "scene start { hero: \"输入中",
            "scene start { bgm( /* comment",
        ] {
            assert!(menu(source, source.len(), &index).is_none());
        }
    }

    #[test]
    fn quoted_talk_speaker_completion_and_project_variables_keep_source_boundaries() {
        let index = AuthoringIndex {
            characters: vec![crate::authoring::CharacterEntry {
                id: "hero".into(),
                name: "少女".into(),
                ..Default::default()
            }],
            variables: vec![("other.shou".into(), "score".into())],
            ..Default::default()
        };
        let source = "scene start { sprite.sequence(hero, mode: talk, speaker: \"he\") { frame(a), frame(b) } }";
        let offset = source.find("he\"").unwrap() + 2;
        let set = menu(source, offset, &index).unwrap();
        assert_eq!(&source[set.range], "\"he\"");
        assert_eq!(set.values, ["\"hero\""]);
        let source = "scene start { let local = 0, let value = sc";
        assert_eq!(
            menu(source, source.len(), &index).unwrap().values,
            ["score"]
        );
        let source = "scene start { camera.move(all, x: 10) }";
        let hover = source_hover(source, source.find("x:").unwrap()).unwrap();
        let lsp_types::HoverContents::Markup(markup) = hover.contents else {
            panic!()
        };
        assert!(markup.value.contains("x: 0"));
    }

    #[test]
    fn project_diagnostics_underline_whole_resource_token_with_unicode_columns() {
        let source = "scene start { \"中文\", background(missing) }";
        let path = PathBuf::from("scripts/chapter.shou");
        let start = source.find("missing").unwrap();
        let span = keine_loader::SourceLineIndex::new(source).span(source, start);
        let mut index = AuthoringIndex::default();
        index
            .asset_references
            .push(crate::authoring::AssetReference {
                key: AssetKey {
                    kind: AssetKind::Background,
                    id: "missing".into(),
                },
                path: path.clone(),
                line: span.line,
                column: span.column,
                range: Some(start..start + 7),
            });
        index.problems.push(crate::authoring::AuthoringProblem {
            severity: ProblemSeverity::Error,
            path: path.clone(),
            line: span.line,
            column: span.column,
            message: "Missing asset".into(),
        });
        let diagnostics = project_diagnostics(source, &index, &path);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].range,
            Position::new(0, span.column as u32 - 1)..Position::new(0, span.column as u32 + 6)
        );
        assert!(project_diagnostics(source, &index, Path::new("other.shou")).is_empty());
    }

    #[test]
    fn child_completion_uses_parent_signature_and_sequence_mode() {
        assert_eq!(
            complete("scene start { sprite.keyframes(hero) { frame(dur"),
            Some("ation: ".into())
        );
        assert_eq!(
            complete("scene start { sprite.keyframes(hero) { frame(x"),
            Some(": ".into())
        );
        assert_eq!(
            complete("scene start { sprite.sequence(hero) { frame(face, dur"),
            Some("ation: ".into())
        );
        assert_eq!(
            complete("scene start { sprite.sequence(hero, fps: 12) { frame(face, dur"),
            None
        );
        assert_eq!(
            complete("scene start { sprite.sequence(hero) { frame(face, x"),
            None
        );
        assert_eq!(
            complete(
                "scene start { sprite.sequence(hero) { frame(face) }, sprite.keyframes(hero) { frame(x"
            ),
            Some(": ".into())
        );
        assert_eq!(
            complete("scene start { assets.loading() { resource(room, ki"),
            Some("nd: ".into())
        );
        assert_eq!(
            complete("scene start { sprite.focus.configure(speaking: style(bri"),
            Some("ghtness: ".into())
        );
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
    fn grouped_visual_completion_uses_the_current_constructor() {
        assert_eq!(
            complete("scene start { sprite(hero, face, position: right(x: 20, y"),
            Some(": ".into())
        );
        assert_eq!(
            complete("scene start { sprite(hero, face, layout: viewport(he"),
            Some("ight: ".into())
        );
        assert_eq!(
            complete("scene start { sprite(hero, face, layout: composite(canvas: size(wi"),
            Some("dth: ".into())
        );
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
