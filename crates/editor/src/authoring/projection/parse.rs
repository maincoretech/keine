//! Read-only structural projection from Loader tokens.
use super::{
    BlockCard, BlockKind, NativeToken, NativeTokenKind, Range, SourceLineIndex, compact_summary,
    decode_source_string, find_top_level, inner_string_range, is_assignment, is_native_command,
    is_native_dotted_command, is_native_structured_command, matching_delimiter, split_top_level,
    split_top_level_with_voice,
};

pub(super) struct BlockProjectionParser<'a> {
    source: &'a str,
    lines: SourceLineIndex,
    tokens: Vec<&'a NativeToken>,
}

impl<'a> BlockProjectionParser<'a> {
    pub(super) fn new(source: &'a str, tokens: &'a [NativeToken]) -> Self {
        Self {
            source,
            lines: SourceLineIndex::new(source),
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

    pub(super) fn scene_blocks(&self, scene_range: Range<usize>) -> Vec<BlockCard> {
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
        let span = self.lines.span(self.source, source_range.start);
        let line = span.line - 1;
        let column = span.column - 1;
        BlockCard {
            summary: compact_summary(self.source.get(source_range.clone()).unwrap_or("")),
            kind,
            statement_range: source_range.clone(),
            source_range,
            disabled: false,
            lifetime_owner: None,
            text_rows: text_range
                .as_ref()
                .and_then(|range| self.source.get(range.clone()))
                .and_then(decode_source_string)
                .map_or(1, |text| text.lines().count().clamp(1, 6)),
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
