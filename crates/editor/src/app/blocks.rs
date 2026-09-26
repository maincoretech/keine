use super::*;

impl WorkbenchPanel {
    pub(super) fn schedule_visual_editors_rebuild(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.weak_entity();
        let window_handle = window.window_handle();
        // EditorState emits Change after replace_all returns. Rebuild only after
        // its subscriber has copied the new source into SourceDocument.
        cx.defer(move |cx| {
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    panel.rebuild_visual_editors(window, cx);
                    cx.notify();
                });
            });
        });
    }

    pub(super) fn hover_block_drop(&mut self, candidate: BlockDropTarget, cx: &mut Context<Self>) {
        let target = self
            .block_dragging
            .as_ref()
            .filter(|selected| !selected.contains(&candidate.row))
            .map(|_| candidate);
        if self.block_drop_target != target {
            self.block_drop_target = target;
            cx.notify();
        }
    }

    pub(super) fn rebuild_visual_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.block_text_editors.clear();
        self.visual_subscriptions.clear();
        self.draft_text = None;
        let PanelContent::Document {
            root,
            relative,
            document: Some(_),
            editor,
        } = &self.content
        else {
            return;
        };
        if relative.extension().and_then(|value| value.to_str()) != Some("shou") {
            return;
        }

        let window_handle = window.window_handle();
        // The source editor already contains replace_all's new value here, while
        // SourceDocument may not receive its Change event until later. Build
        // row states from the same revision that the Blocks projection will
        // render after the event, so moved text cannot bind to old offsets.
        let source = editor.read(cx).value().to_string();
        let dialogues = dialogues_for_source(relative, &source);
        for dialogue in dialogues.into_iter().filter(|dialogue| dialogue.editable) {
            let state = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .default_value(dialogue.text)
                    .placeholder("Text")
                    .auto_grow(1, 6)
                    .submit_on_enter(true)
            });
            let source_editor = editor.clone();
            let root = root.clone();
            let relative = relative.clone();
            let state_for_change = state.clone();
            let subscription = cx.subscribe(&state, move |panel, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    let panel = cx.weak_entity();
                    let state = state_for_change.clone();
                    cx.defer(move |cx| {
                        let _ = cx.update_window(window_handle, |_, window, cx| {
                            let _ = panel.update(cx, |panel, cx| {
                                panel.continue_text_block(&state, window, cx);
                            });
                        });
                    });
                    return;
                }
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                // A drag or structural edit replaces every row state. Ignore
                // events queued by a retired state, and resolve live rows by
                // their updated source offset rather than their original line.
                let Some(text_start) = panel
                    .block_text_editors
                    .iter()
                    .find(|editor| editor.state.entity_id() == state_for_change.entity_id())
                    .map(|editor| editor.text_start)
                else {
                    return;
                };
                let value = state_for_change.read(cx).value().to_string();
                let source = source_editor.read(cx).value().to_string();
                let current = dialogues_for_source(&relative, &source)
                    .into_iter()
                    .find(|dialogue| dialogue.text_range.start == text_start);
                if current
                    .as_ref()
                    .is_some_and(|dialogue| dialogue.text == value)
                {
                    return;
                }
                let result = current
                    .as_ref()
                    .ok_or_else(|| "dialogue no longer exists".to_owned())
                    .and_then(|dialogue| {
                        replace_dialogue_text(&source, dialogue, &value)
                            .map_err(|error| error.to_string())
                    });
                match result {
                    Ok(edited) => {
                        let delta = edited.len() as isize - source.len() as isize;
                        let changed_end = current
                            .as_ref()
                            .map_or(usize::MAX, |dialogue| dialogue.text_range.end);
                        let result = cx.update_window(window_handle, |_, window, cx| {
                            source_editor.update(cx, |editor, cx| {
                                editor.replace_all(edited, window, cx);
                            });
                        });
                        if result.is_ok() && delta != 0 {
                            for editor in &mut panel.block_text_editors {
                                if editor.text_start >= changed_end {
                                    editor.text_start =
                                        editor.text_start.saturating_add_signed(delta);
                                }
                            }
                        }
                        let notice = match result {
                            Ok(()) => "Text updated from Blocks".to_owned(),
                            Err(error) => format!("Block text edit failed: {error}"),
                        };
                        cx.global_mut::<EditorDocuments>().set_notice(&root, notice);
                    }
                    Err(error) => cx
                        .global_mut::<EditorDocuments>()
                        .set_notice(&root, format!("Block text edit blocked: {error}")),
                }
                cx.refresh_windows();
            });
            self.visual_subscriptions.push(subscription);
            self.block_text_editors.push(BlockTextEditor {
                text_start: dialogue.text_range.start,
                state,
            });
        }
    }

    pub(super) fn toggle_block_picker(
        &mut self,
        _: &ToggleBlockPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        self.block_picker_open = !self.block_picker_open;
        self.block_picker_index = 0;
        if self.block_picker_open {
            self.block_picker_input
                .update(cx, |state, cx| state.focus(window, cx));
        } else {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn block_picker_next(
        &mut self,
        _: &BlockPickerNext,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.filtered_picker_kinds(cx).len();
        if count > 0 {
            self.block_picker_index = (self.block_picker_index + 1).min(count - 1);
            cx.notify();
        }
    }

    pub(super) fn block_picker_previous(
        &mut self,
        _: &BlockPickerPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.block_picker_index = self.block_picker_index.saturating_sub(1);
        cx.notify();
    }

    pub(super) fn accept_block_picker(
        &mut self,
        _: &AcceptBlockPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kinds = self.filtered_picker_kinds(cx);
        let Some(kind) = kinds.get(self.block_picker_index).copied() else {
            return;
        };
        self.block_picker_open = false;
        self.insert_from_palette(kind, window, cx);
        self.schedule_visual_editors_rebuild(window, cx);
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_block_picker(
        &mut self,
        _: &CloseBlockPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.block_picker_open = false;
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn filtered_picker_kinds(&self, cx: &App) -> Vec<InsertKind> {
        let query = self
            .block_picker_input
            .read(cx)
            .value()
            .to_string()
            .to_lowercase();
        picker_kinds(
            cx.global::<EditorDocuments>().block_picker_preferences(),
            &query,
            self.block_picker_category,
            self.block_picker_customize,
        )
    }

    pub(super) fn change_picker_preferences(
        &self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut BlockPickerPreferences),
    ) {
        let result = cx
            .global_mut::<EditorDocuments>()
            .update_block_picker_preferences(update);
        if let Err(error) = result
            && let PanelContent::Document { root, .. } = &self.content
        {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, format!("Picker preferences not saved: {error}"));
        }
        cx.refresh_windows();
    }

    pub(super) fn toggle_picker_favorite(&self, kind: InsertKind, cx: &mut Context<Self>) {
        self.change_picker_preferences(cx, move |preferences| {
            toggle_preference(&mut preferences.favorites, kind.label());
        });
    }

    pub(super) fn toggle_picker_hidden(&self, kind: InsertKind, cx: &mut Context<Self>) {
        self.change_picker_preferences(cx, move |preferences| {
            toggle_preference(&mut preferences.hidden, kind.label());
        });
    }

    pub(super) fn move_picker_item(&self, kind: InsertKind, delta: isize, cx: &mut Context<Self>) {
        self.change_picker_preferences(cx, move |preferences| {
            let universe = InsertKind::ALL
                .into_iter()
                .map(InsertKind::label)
                .collect::<Vec<_>>();
            let group = InsertKind::ALL
                .into_iter()
                .filter(|candidate| candidate.category() == kind.category())
                .map(InsertKind::label)
                .collect::<Vec<_>>();
            move_group_preference(
                &mut preferences.item_order,
                kind.label(),
                &group,
                &universe,
                delta,
            );
        });
    }

    pub(super) fn move_picker_category(
        &self,
        category: &'static str,
        delta: isize,
        cx: &mut Context<Self>,
    ) {
        self.change_picker_preferences(cx, move |preferences| {
            move_preference(
                &mut preferences.category_order,
                category,
                &PICKER_CATEGORIES,
                delta,
            );
        });
    }

    pub(super) fn begin_text_block(
        &mut self,
        _: &BeginTextBlock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        if let Some(draft) = &self.draft_text {
            let state = draft.state.clone();
            self.continue_text_block(&state, window, cx);
            return;
        }
        let PanelContent::Document {
            root,
            relative,
            document: Some(document),
            editor,
        } = &self.content
        else {
            return;
        };
        let source = document.borrow().contents().to_owned();
        let projection = EiyashouProjection::parse(&source);
        let selected_line = cx
            .global::<EditorDocuments>()
            .selection(root)
            .filter(|(path, _, _)| path == relative)
            .map_or(0, |(_, line, _)| *line);
        let target = self
            .selected_blocks
            .iter()
            .copied()
            .max()
            .map(DraftInsertionTarget::After)
            .or_else(|| {
                projection
                    .scenes
                    .iter()
                    .filter(|scene| {
                        source
                            .get(..scene.name_range.start)
                            .map(|prefix| prefix.bytes().filter(|byte| *byte == b'\n').count())
                            .is_some_and(|line| line <= selected_line)
                    })
                    .max_by_key(|scene| scene.source_range.start)
                    .map(|scene| {
                        scene.blocks.last().map_or(
                            DraftInsertionTarget::SceneEnd(scene.source_range.start),
                            |block| DraftInsertionTarget::After(block.source_range.start),
                        )
                    })
            })
            .or_else(|| {
                projection.scenes.first().map(|scene| {
                    scene.blocks.last().map_or(
                        DraftInsertionTarget::SceneEnd(scene.source_range.start),
                        |block| DraftInsertionTarget::After(block.source_range.start),
                    )
                })
            });
        let Some(target) = target else {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "No Scene available");
            cx.refresh_windows();
            return;
        };
        if let Some(scene) = projection.scenes.iter().find(|scene| match target {
            DraftInsertionTarget::After(start) => scene
                .blocks
                .iter()
                .any(|block| block.source_range.start == start),
            DraftInsertionTarget::SceneEnd(start) => scene.source_range.start == start,
        }) {
            self.collapsed_scenes.remove(&scene.name);
        }
        let state = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Text")
                .auto_grow(1, 6)
                .submit_on_enter(true)
        });
        let state_for_change = state.clone();
        let document = document.clone();
        let source_editor = editor.clone();
        let root = root.clone();
        let window_handle = window.window_handle();
        let subscription = cx.subscribe(&state, move |panel, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                let panel = cx.weak_entity();
                let state = state_for_change.clone();
                cx.defer(move |cx| {
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        let _ = panel.update(cx, |panel, cx| {
                            panel.continue_text_block(&state, window, cx);
                        });
                    });
                });
                return;
            }
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let value = state_for_change.read(cx).value().to_string();
            let escaped = escape_eiyashou_string(&value);
            let source = document.borrow().contents().to_owned();
            let Some(draft) = panel
                .draft_text
                .as_mut()
                .filter(|draft| draft.state.entity_id() == state_for_change.entity_id())
            else {
                return;
            };
            let edit = if let Some(range) = draft.text_range.clone() {
                if source.get(range.clone()) != Some(draft.last_escaped.as_str()) {
                    Err("source changed; refresh the Block view".to_owned())
                } else {
                    let start = range.start;
                    let mut edited = source;
                    edited.replace_range(range, &escaped);
                    draft.text_range = Some(start..start + escaped.len());
                    draft.last_escaped.clone_from(&escaped);
                    Ok(edited)
                }
            } else if value.is_empty() {
                return;
            } else {
                let statement = format!("\"{escaped}\"");
                let projection = EiyashouProjection::parse(&source);
                match draft.target {
                    DraftInsertionTarget::After(start) => {
                        projection.insert_block_after(&source, start, &statement)
                    }
                    DraftInsertionTarget::SceneEnd(start) => {
                        projection.insert_block_in_scene(&source, start, &statement)
                    }
                }
                .map(|(edited, range)| {
                    draft.text_range = Some(range.start + 1..range.end - 1);
                    draft.last_escaped.clone_from(&escaped);
                    panel.selected_blocks.clear();
                    panel.selected_blocks.insert(range.start);
                    panel.block_selection_anchor = Some(range.start);
                    edited
                })
                .map_err(|error| error.to_string())
            };
            match edit {
                Ok(edited) => {
                    let result = cx.update_window(window_handle, |_, window, cx| {
                        source_editor.update(cx, |editor, cx| {
                            editor.replace_all(edited, window, cx);
                        });
                    });
                    let notice = match result {
                        Ok(()) => "Text updated".to_owned(),
                        Err(error) => format!("Text edit failed: {error}"),
                    };
                    cx.global_mut::<EditorDocuments>().set_notice(&root, notice);
                }
                Err(error) => cx
                    .global_mut::<EditorDocuments>()
                    .set_notice(&root, format!("Text edit blocked: {error}")),
            }
            cx.notify();
            cx.refresh_windows();
        });
        self.visual_subscriptions.push(subscription);
        self.draft_text = Some(DraftTextBlock {
            target,
            text_range: None,
            last_escaped: String::new(),
            state: state.clone(),
        });
        state.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub(super) fn continue_text_block(
        &mut self,
        state: &Entity<TextareaState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        let PanelContent::Document {
            root,
            relative,
            document: Some(document),
            editor,
        } = &self.content
        else {
            return;
        };
        let root = root.clone();
        let relative = relative.clone();
        let document = document.clone();
        let editor = editor.clone();
        let source = document.borrow().contents().to_owned();
        let projection = EiyashouProjection::parse(&source);
        let draft = self
            .draft_text
            .as_ref()
            .filter(|draft| draft.state.entity_id() == state.entity_id());
        let statement = format!("\"{}\"", escape_eiyashou_string(&state.read(cx).value()));
        let inserted = draft
            .filter(|draft| draft.text_range.is_none())
            .map(|draft| match draft.target {
                DraftInsertionTarget::After(start) => {
                    projection.insert_block_after(&source, start, &statement)
                }
                DraftInsertionTarget::SceneEnd(start) => {
                    projection.insert_block_in_scene(&source, start, &statement)
                }
            });
        let start = match inserted {
            Some(Ok((edited, range))) => {
                cx.global_mut::<EditorDocuments>()
                    .record_source_edit(&root, &relative, &source, &edited);
                editor.update(cx, |editor, cx| editor.replace_all(edited, window, cx));
                range.start
            }
            Some(Err(error)) => {
                self.set_block_notice(format!("Text edit blocked: {error}"), cx);
                return;
            }
            None => {
                let text_start = draft
                    .and_then(|draft| draft.text_range.as_ref().map(|range| range.start))
                    .or_else(|| {
                        self.block_text_editors
                            .iter()
                            .find(|editor| editor.state.entity_id() == state.entity_id())
                            .map(|editor| editor.text_start)
                    });
                let Some(start) = text_start.and_then(|text_start| {
                    projection
                        .scenes
                        .iter()
                        .flat_map(|scene| &scene.blocks)
                        .find(|block| {
                            block
                                .text_range
                                .as_ref()
                                .is_some_and(|range| range.start == text_start)
                        })
                        .map(|block| block.source_range.start)
                }) else {
                    return;
                };
                start
            }
        };
        self.selected_blocks.clear();
        self.selected_blocks.insert(start);
        self.block_selection_anchor = Some(start);
        cx.global_mut::<EditorDocuments>()
            .set_block_selection(&root, relative, vec![start]);
        self.draft_text = None;
        self.rebuild_visual_editors(window, cx);
        self.begin_text_block(&BeginTextBlock, window, cx);
    }

    pub(super) fn copy_selected_blocks(
        &mut self,
        _: &CopyBlocks,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        let PanelContent::Document {
            root,
            document: Some(document),
            ..
        } = &self.content
        else {
            return;
        };
        let source = document.borrow().contents().to_owned();
        match EiyashouProjection::parse(&source).copy_blocks(&source, &self.selected_blocks) {
            Ok(value) => {
                cx.write_to_clipboard(ClipboardItem::new_string(value));
                cx.global_mut::<EditorDocuments>()
                    .set_notice(root, "Blocks copied");
            }
            Err(error) => cx
                .global_mut::<EditorDocuments>()
                .set_notice(root, format!("Copy blocked: {error}")),
        }
        cx.refresh_windows();
    }

    pub(super) fn paste_blocks(
        &mut self,
        _: &PasteBlocks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        let Some(fragment) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.set_block_notice("Paste blocked: clipboard has no text".into(), cx);
            return;
        };
        let fragment = fragment.trim();
        if fragment.is_empty() {
            self.set_block_notice("Paste blocked: clipboard is empty".into(), cx);
            return;
        }
        let wrapper = format!("scene __paste {{ {fragment} }}");
        let fragment_projection = EiyashouProjection::parse(&wrapper);
        let fragment_blocks = fragment_projection
            .scenes
            .first()
            .map(|scene| scene.blocks.as_slice())
            .unwrap_or_default();
        if fragment_blocks.is_empty()
            || !fragment_projection.read_only.is_empty()
            || fragment_blocks.iter().any(|block| block.read_only)
        {
            self.set_block_notice("Paste blocked: not valid block source".into(), cx);
            return;
        }
        let mut pasted_ids = HashSet::new();
        if fragment_blocks
            .iter()
            .filter_map(|block| block.stable_id.as_deref())
            .any(|id| !pasted_ids.insert(id.to_owned()))
        {
            self.set_block_notice("Paste blocked: duplicate stable ID".into(), cx);
            return;
        }
        let PanelContent::Document {
            root,
            document: Some(document),
            ..
        } = &self.content
        else {
            return;
        };
        let project_ids = cx.global::<EditorDocuments>().explicit_source_ids(root);
        if pasted_ids.iter().any(|id| project_ids.contains(id)) {
            self.set_block_notice("Paste blocked: stable ID already exists".into(), cx);
            return;
        }
        let Some(after_start) = self.selected_blocks.iter().copied().max() else {
            self.set_block_notice("Paste blocked: select an insertion block".into(), cx);
            return;
        };
        let source = document.borrow().contents().to_owned();
        match EiyashouProjection::parse(&source).insert_block_after(&source, after_start, fragment)
        {
            Ok((edited, _)) => self.apply_block_source(edited, "Blocks pasted", window, cx),
            Err(error) => self.set_block_notice(format!("Paste blocked: {error}"), cx),
        }
    }

    pub(super) fn delete_selected_blocks(
        &mut self,
        _: &DeleteBlocks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        let source = match &self.content {
            PanelContent::Document {
                document: Some(document),
                ..
            } => document.borrow().contents().to_owned(),
            _ => return,
        };
        match EiyashouProjection::parse(&source).delete_blocks(&source, &self.selected_blocks) {
            Ok(edited) => self.apply_block_source(edited, "Blocks deleted", window, cx),
            Err(error) => self.set_block_notice(format!("Delete blocked: {error}"), cx),
        }
    }

    pub(super) fn move_selected_blocks_up(
        &mut self,
        _: &MoveBlocksUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selected_blocks(MoveDirection::Up, window, cx);
    }

    pub(super) fn move_selected_blocks_down(
        &mut self,
        _: &MoveBlocksDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selected_blocks(MoveDirection::Down, window, cx);
    }

    pub(super) fn move_selected_blocks(
        &mut self,
        direction: MoveDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        let source = match &self.content {
            PanelContent::Document {
                document: Some(document),
                ..
            } => document.borrow().contents().to_owned(),
            _ => return,
        };
        match EiyashouProjection::parse(&source).move_blocks(
            &source,
            &self.selected_blocks,
            direction,
        ) {
            Ok(edited) => self.apply_block_source(edited, "Blocks moved", window, cx),
            Err(error) => self.set_block_notice(format!("Move blocked: {error}"), cx),
        }
    }

    pub(super) fn drop_blocks(
        &mut self,
        drag: &BlockDrag,
        target_start: usize,
        after: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block {
            return;
        }
        let source = match &self.content {
            PanelContent::Document {
                document: Some(document),
                ..
            } => document.borrow().contents().to_owned(),
            _ => return,
        };
        match EiyashouProjection::parse(&source).move_blocks_to(
            &source,
            &drag.selected,
            target_start,
            after,
        ) {
            Ok(edited) if edited != source => {
                self.apply_block_source(edited, "Blocks moved", window, cx)
            }
            Ok(_) => {}
            Err(error) => self.set_block_notice(format!("Move blocked: {error}"), cx),
        }
    }

    pub(super) fn drop_assets(
        &mut self,
        drag: &AssetDrag,
        target_start: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PanelContent::Document {
            root,
            document: Some(document),
            ..
        } = &self.content
        else {
            return;
        };
        if self.document_mode != DocumentMode::Block || drag.root != *root {
            return;
        }
        let source = document.borrow().contents().to_owned();
        let index = cx.global::<EditorDocuments>().authoring(root);
        match insert_assets_at_block(&source, target_start, &drag.keys, &index) {
            Ok(edited) => self.apply_block_source(edited, "Assets inserted", window, cx),
            Err(error) => self.set_block_notice(format!("Asset drop blocked: {error}"), cx),
        }
    }

    pub(super) fn apply_block_source(
        &mut self,
        edited: String,
        notice: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PanelContent::Document {
            root,
            relative,
            document: Some(document),
            editor,
            ..
        } = &self.content
        else {
            return;
        };
        cx.global_mut::<EditorDocuments>().record_source_edit(
            root,
            relative,
            document.borrow().contents(),
            &edited,
        );
        let root = root.clone();
        let editor = editor.clone();
        editor.update(cx, |editor, cx| editor.replace_all(edited, window, cx));
        self.selected_blocks.clear();
        self.block_selection_anchor = None;
        self.schedule_visual_editors_rebuild(window, cx);
        self.focus.focus(window, cx);
        cx.global_mut::<EditorDocuments>()
            .clear_block_selection(&root);
        cx.global_mut::<EditorDocuments>().set_notice(&root, notice);
        cx.notify();
        cx.refresh_windows();
    }

    pub(super) fn undo_blocks(
        &mut self,
        _: &UndoBlocks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block || !self.focus.is_focused(window) {
            return;
        }
        self.replay_block_source(true, window, cx);
    }

    pub(super) fn redo_blocks(
        &mut self,
        _: &RedoBlocks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_mode != DocumentMode::Block || !self.focus.is_focused(window) {
            return;
        }
        self.replay_block_source(false, window, cx);
    }

    fn replay_block_source(&mut self, undo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let PanelContent::Document { root, .. } = &self.content else {
            return;
        };
        let root = root.clone();
        match replay_source_history(&root, undo, window, cx) {
            Ok(true) => {
                self.selected_blocks.clear();
                self.block_selection_anchor = None;
                self.schedule_visual_editors_rebuild(window, cx);
                cx.global_mut::<EditorDocuments>()
                    .clear_block_selection(&root);
                cx.refresh_windows();
            }
            Ok(false) => {}
            Err(error) => self.set_block_notice(error, cx),
        }
    }

    pub(super) fn begin_scene_edit(
        &mut self,
        mode: SceneEditMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = match &mode {
            SceneEditMode::New => "",
            SceneEditMode::Rename { old_name, .. } => old_name.as_str(),
        };
        self.scene_name_input
            .update(cx, |input, cx| input.set_value(initial, window, cx));
        self.scene_edit = Some(mode);
        self.scene_name_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn commit_scene_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = self.scene_edit.clone() else {
            return;
        };
        let PanelContent::Document {
            root,
            relative,
            document: Some(document),
            ..
        } = &self.content
        else {
            return;
        };
        let root = root.clone();
        let relative = relative.clone();
        let source = document.borrow().contents().to_owned();
        let name = self.scene_name_input.read(cx).value().trim().to_owned();
        if !valid_identifier(&name) {
            self.set_block_notice("Scene name must be an identifier".into(), cx);
            return;
        }
        if matches!(&mode, SceneEditMode::Rename { old_name, .. } if old_name == &name) {
            self.scene_edit = None;
            cx.notify();
            return;
        }
        let index = cx.global::<EditorDocuments>().authoring(&root);
        let same_scene = |scene: &crate::authoring::SceneEntry| {
            matches!(&mode, SceneEditMode::Rename { start, .. }
                if scene.path == relative && scene.source_range.start == *start)
        };
        if index
            .scenes
            .iter()
            .any(|scene| scene.name == name && !same_scene(scene))
        {
            self.set_block_notice("Scene name already exists".into(), cx);
            return;
        }
        let result = match &mode {
            SceneEditMode::New => append_scene(&source, &name)
                .map(|edited| vec![(relative.clone(), edited)])
                .map_err(|error| error.to_string()),
            SceneEditMode::Rename { start, old_name } => {
                if !index.unindexed_sources.is_empty() {
                    Err("Some scripts could not be indexed".to_owned())
                } else if index.scenes.iter().any(|scene| {
                    scene.path == relative
                        && scene.source_range.start == *start
                        && scene.name == *old_name
                }) {
                    let mut edits = BTreeMap::new();
                    match rename_scene(&source, *start, &name) {
                        Ok(edited) => {
                            edits.insert(relative.clone(), edited);
                            for path in index
                                .scenes
                                .iter()
                                .map(|scene| scene.path.clone())
                                .collect::<HashSet<_>>()
                            {
                                if path == relative {
                                    continue;
                                }
                                let Some(other_source) =
                                    cx.global::<EditorDocuments>().source(&root, &path)
                                else {
                                    self.set_block_notice(
                                        format!("Could not read {}", path.display()),
                                        cx,
                                    );
                                    return;
                                };
                                let rewritten =
                                    rename_scene_references(&other_source, old_name, &name);
                                if rewritten != other_source {
                                    edits.insert(path, rewritten);
                                }
                            }
                            Ok(edits.into_iter().collect())
                        }
                        Err(error) => Err(error.to_string()),
                    }
                } else {
                    Err("Scene changed; refresh Blocks".to_owned())
                }
            }
        };
        match result.and_then(|edits| apply_prepared_edits(&root, &edits, window, cx)) {
            Ok(()) => {
                if let SceneEditMode::Rename { old_name, .. } = &mode
                    && self.collapsed_scenes.remove(old_name)
                {
                    self.collapsed_scenes.insert(name.clone());
                }
                self.scene_edit = None;
                self.schedule_visual_editors_rebuild(window, cx);
                self.set_block_notice(format!("Scene {name} updated"), cx);
            }
            Err(error) => self.set_block_notice(format!("Scene edit blocked: {error}"), cx),
        }
    }

    pub(super) fn move_scene_from_menu(
        &mut self,
        start: usize,
        direction: MoveDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PanelContent::Document {
            document: Some(document),
            ..
        } = &self.content
        else {
            return;
        };
        let source = document.borrow().contents().to_owned();
        match move_scene(&source, start, direction) {
            Ok(edited) => self.apply_block_source(edited, "Scene moved", window, cx),
            Err(error) => self.set_block_notice(format!("Move blocked: {error}"), cx),
        }
    }

    pub(super) fn confirm_delete_scene(
        &mut self,
        start: usize,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PanelContent::Document { root, .. } = &self.content else {
            return;
        };
        let index = cx.global::<EditorDocuments>().authoring(root);
        let references = index
            .scenes
            .iter()
            .map(|scene| scene.path.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .filter_map(|path| cx.global::<EditorDocuments>().source(root, &path))
            .map(|source| scene_references(&source, &name).len())
            .sum::<usize>();
        let detail = if references == 0 {
            "This removes the scene and its blocks. Undo remains available.".to_owned()
        } else {
            format!(
                "This removes the scene and its blocks. {references} references will be unresolved. Undo remains available."
            )
        };
        let receiver = window.prompt(
            PromptLevel::Warning,
            "Delete scene?",
            Some(&detail),
            &[
                PromptButton::Other("Delete".into()),
                PromptButton::Cancel("Cancel".into()),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if receiver.await.ok() != Some(0) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                let PanelContent::Document {
                    document: Some(document),
                    ..
                } = &this.content
                else {
                    return;
                };
                let source = document.borrow().contents().to_owned();
                let still_same = EiyashouProjection::parse(&source)
                    .scenes
                    .iter()
                    .any(|scene| scene.source_range.start == start && scene.name == name);
                if !still_same {
                    this.set_block_notice("Scene changed; refresh Blocks".into(), cx);
                    return;
                }
                match delete_scene(&source, start) {
                    Ok(edited) => {
                        this.collapsed_scenes.remove(&name);
                        this.apply_block_source(edited, "Scene deleted", window, cx)
                    }
                    Err(error) => this.set_block_notice(format!("Delete blocked: {error}"), cx),
                }
            });
        })
        .detach();
    }

    pub(super) fn open_scene_context_menu(
        &mut self,
        start: usize,
        name: String,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.scene_context_epoch = self.scene_context_epoch.wrapping_add(1);
        self.scene_context_menu = Some(SceneContextMenu {
            start,
            name,
            position,
            epoch: self.scene_context_epoch,
            closing: false,
        });
        cx.notify();
    }

    pub(super) fn close_scene_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.scene_context_menu.as_mut() else {
            return;
        };
        if menu.closing {
            return;
        }
        self.scene_context_epoch = self.scene_context_epoch.wrapping_add(1);
        menu.epoch = self.scene_context_epoch;
        menu.closing = true;
        let epoch = menu.epoch;
        let delay = if cx.reduce_motion() {
            Duration::ZERO
        } else {
            Duration::from_millis(90)
        };
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this
                    .scene_context_menu
                    .as_ref()
                    .is_some_and(|menu| menu.epoch == epoch && menu.closing)
                {
                    this.scene_context_menu = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn toggle_asset_filter_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .asset_filter_menu
            .as_ref()
            .is_some_and(|menu| !menu.closing)
        {
            self.close_asset_filter_menu(window, cx);
            return;
        }
        self.asset_filter_epoch = self.asset_filter_epoch.wrapping_add(1);
        self.asset_filter_menu = Some(AssetFilterMenu {
            position: Point {
                x: position.x - px(ASSET_FILTER_MENU_WIDTH_PX / 2.),
                y: position.y + px(14.),
            },
            epoch: self.asset_filter_epoch,
            closing: false,
            expanded: None,
        });
        cx.notify();
    }

    pub(super) fn close_asset_filter_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.asset_filter_menu.as_mut() else {
            return;
        };
        if menu.closing {
            return;
        }
        self.asset_filter_epoch = self.asset_filter_epoch.wrapping_add(1);
        menu.epoch = self.asset_filter_epoch;
        menu.closing = true;
        let epoch = menu.epoch;
        let delay = if cx.reduce_motion() {
            Duration::ZERO
        } else {
            Duration::from_millis(90)
        };
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this
                    .asset_filter_menu
                    .as_ref()
                    .is_some_and(|menu| menu.epoch == epoch && menu.closing)
                {
                    this.asset_filter_menu = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn set_block_notice(&self, notice: String, cx: &mut Context<Self>) {
        if let PanelContent::Document { root, .. } = &self.content {
            cx.global_mut::<EditorDocuments>().set_notice(root, notice);
            cx.refresh_windows();
        }
    }

    pub(super) fn insert_from_palette(
        &mut self,
        kind: InsertKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PanelContent::Document {
            root,
            relative,
            document: Some(document),
            editor,
            ..
        } = &self.content
        else {
            return;
        };
        let source = document.borrow().contents().to_owned();
        let line = document.borrow().selection().line;
        let index = cx.global::<EditorDocuments>().authoring(root);
        match insert_statement(&source, line, kind, &index) {
            Ok(edited) => {
                cx.global_mut::<EditorDocuments>()
                    .record_source_edit(root, relative, &source, &edited);
                editor.update(cx, |editor, cx| editor.replace_all(edited, window, cx));
                cx.global_mut::<EditorDocuments>().set_notice(
                    root,
                    format!("Inserted {} at a source line boundary", kind.label()),
                );
            }
            Err(error) => cx
                .global_mut::<EditorDocuments>()
                .set_notice(root, format!("Insert blocked: {error}")),
        }
        cx.refresh_windows();
    }

    pub(super) fn add_character(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let PanelContent::Characters { root } = &self.content else {
            return;
        };
        if self.tool_inputs.len() != 3 {
            return;
        }
        let id = self.tool_inputs[0].read(cx).value().to_string();
        let name = self.tool_inputs[1].read(cx).value().to_string();
        let color = self.tool_inputs[2].read(cx).value().to_string();
        let index = cx.global::<EditorDocuments>().authoring(root);
        let Some(path) = index.characters_manifest else {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "Character manifest is unavailable");
            return;
        };
        let result = cx
            .global_mut::<EditorDocuments>()
            .open(root, &path)
            .map_err(|error| error.to_string())
            .and_then(|document| {
                append_character(
                    document.borrow().contents(),
                    id.trim(),
                    name.trim(),
                    Some(color.trim()),
                )
                .map_err(|error| error.to_string())
            });
        match result {
            Ok(edited) => {
                apply_workspace_edit(root, &path, edited, window, cx);
                self.focus.focus(window, cx);
                for input in &self.tool_inputs {
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                }
                cx.global_mut::<EditorDocuments>()
                    .set_notice(root, format!("Added character `{}`", id.trim()));
            }
            Err(error) => cx
                .global_mut::<EditorDocuments>()
                .set_notice(root, format!("Character edit blocked: {error}")),
        }
        cx.refresh_windows();
    }
}
