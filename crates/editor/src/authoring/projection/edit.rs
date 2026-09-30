//! Bounded source rewrites. Does not mutate documents, UI state or undo history.
use super::{
    BlockCard, BlockEditError, BlockKind, EiyashouProjection, HashSet, MoveDirection,
    NativeTokenKind, Range, SceneSection, TextBlockMetadata, TextLifetime, disabled_body,
    matching_parenthesis, parse_native_document, split_source_ranges, valid_identifier,
};

impl EiyashouProjection {
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
        if updates.iter().any(|(name, _)| name.contains('.')) {
            return replace_grouped_fields(source, block, &fields, updates);
        }
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
        let (siblings, selected_indices, target_index) =
            self.block_drop_siblings(selected, target_start)?;
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

    /// Same scope/selection validation used by the source edit, without copying source.
    pub fn accepts_block_drop(&self, selected: &HashSet<usize>, target_start: usize) -> bool {
        !selected.contains(&target_start)
            && self.block_drop_siblings(selected, target_start).is_ok()
    }

    fn block_drop_siblings(
        &self,
        selected: &HashSet<usize>,
        target_start: usize,
    ) -> Result<(Vec<&BlockCard>, HashSet<usize>, usize), BlockEditError> {
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
        Ok((siblings, selected_indices, target_index))
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
        // Metadata edits preserve accepted dialogue options byte-for-byte.
        if let Some(suffix) = source.get(text_range.end + 1..block.statement_range.end) {
            let suffix = suffix
                .trim_start()
                .strip_prefix(',')
                .unwrap_or("")
                .trim_start();
            let options = if suffix
                .split(',')
                .next()
                .is_some_and(|first| valid_identifier(first.trim()))
            {
                suffix.split_once(',').map(|(_, options)| options)
            } else {
                Some(suffix)
            };
            if let Some(options) = options.filter(|options| !options.trim().is_empty()) {
                replacement.push_str(", ");
                replacement.push_str(options.trim());
            }
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

/// Nested updates retain the original argument text, including unrelated comments.
fn replace_grouped_fields(
    source: &str,
    block: &BlockCard,
    fields: &[super::SourceField],
    updates: &[(String, Option<String>)],
) -> Result<String, BlockEditError> {
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut seen = HashSet::new();
    for (key, value) in updates {
        if !seen.insert(key) {
            return Err(BlockEditError::StaleRange);
        }
        let field = fields
            .iter()
            .find(|field| &field.key == key)
            .ok_or(BlockEditError::StaleRange)?;
        if field.quoted {
            return Err(BlockEditError::StaleRange);
        }
        if let Some(prefix) = &field.insertion {
            let Some(value) = value else {
                continue;
            };
            if let Some((_, insertion)) = edits.iter_mut().find(|(range, _)| range == &field.range)
            {
                let suffix = field.insertion_suffix.as_deref().unwrap_or("");
                insertion.truncate(insertion.len() - suffix.len());
                insertion.push_str(&format!(
                    ", {}: {value}{suffix}",
                    key.rsplit('.').next().unwrap()
                ));
            } else {
                edits.push((
                    field.range.clone(),
                    format!(
                        "{prefix}{value}{}",
                        field.insertion_suffix.as_deref().unwrap_or("")
                    ),
                ));
            }
        } else if let Some(value) = value {
            edits.push((field.range.clone(), value.clone()));
        } else {
            // Select the smallest call containing this argument, then its comma span.
            let node = &source[block.source_range.clone()];
            let (open, close) = node
                .char_indices()
                .filter(|(_, c)| *c == '(')
                .filter_map(|(open, _)| matching_parenthesis(node, open).map(|close| (open, close)))
                .filter(|(open, close)| {
                    block.source_range.start + open < field.range.start
                        && block.source_range.start + close >= field.range.end
                })
                .max_by_key(|(open, _)| *open)
                .ok_or(BlockEditError::StaleRange)?;
            let base = block.source_range.start + open + 1;
            let spans = split_source_ranges(&node[open + 1..close], ',');
            let index = spans
                .iter()
                .position(|span| {
                    base + span.start <= field.range.start && base + span.end >= field.range.end
                })
                .ok_or(BlockEditError::StaleRange)?;
            let range = if index + 1 < spans.len() {
                base + spans[index].start..base + spans[index + 1].start
            } else if index > 0 {
                base + spans[index - 1].end..base + spans[index].end
            } else {
                base + spans[index].start..base + spans[index].end
            };
            edits.push((range, String::new()));
        }
    }
    edits.sort_by_key(|(range, _)| (range.start, range.end));
    if edits.windows(2).any(|pair| pair[0].0.end > pair[1].0.start) {
        return Err(BlockEditError::StaleRange);
    }
    let mut edited = source.to_owned();
    for (range, value) in edits.into_iter().rev() {
        edited.replace_range(range, &value);
    }
    Ok(edited)
}
