use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourceChange {
    path: PathBuf,
    before: String,
    after: String,
}

#[derive(Clone, Default)]
struct SourceTransaction {
    changes: Vec<SourceChange>,
    files: Vec<file_ops::AssetFileChange>,
}

#[derive(Default)]
pub(super) struct SourceHistory {
    undo: VecDeque<SourceTransaction>,
    redo: Vec<SourceTransaction>,
}

impl SourceHistory {
    pub(super) fn forget(&mut self, path: &Path) {
        self.undo
            .retain(|edit| !edit.changes.iter().any(|change| change.path == path));
        self.redo
            .retain(|edit| !edit.changes.iter().any(|change| change.path == path));
    }
    const BYTE_BUDGET: usize = 16 * 1024 * 1024;

    fn bytes(&self) -> usize {
        self.undo
            .iter()
            .chain(&self.redo)
            .flat_map(|edit| &edit.changes)
            .map(|change| change.before.len().saturating_add(change.after.len()))
            .sum()
    }
    fn record(&mut self, changes: Vec<SourceChange>) {
        let changes = changes
            .into_iter()
            .filter(|change| change.before != change.after)
            .collect::<Vec<_>>();
        if changes.is_empty() {
            return;
        }
        self.record_transaction(SourceTransaction {
            changes,
            files: Vec::new(),
        });
    }

    fn record_transaction(&mut self, transaction: SourceTransaction) {
        self.undo.push_back(transaction);
        self.redo.clear();
        while self.undo.len() > 32 || self.bytes() > Self::BYTE_BUDGET {
            self.undo.pop_front();
        }
    }

    #[cfg(test)]
    fn next(&self, undo: bool) -> Option<&[SourceChange]> {
        self.transaction(undo).map(|edit| edit.changes.as_slice())
    }

    fn transaction(&self, undo: bool) -> Option<&SourceTransaction> {
        if undo {
            self.undo.back()
        } else {
            self.redo.last()
        }
    }

    fn finish(&mut self, undo: bool) {
        if undo {
            if let Some(change) = self.undo.pop_back() {
                self.redo.push(change);
            }
        } else if let Some(change) = self.redo.pop() {
            self.undo.push_back(change);
        }
    }
}

impl EditorDocuments {
    pub(super) fn record_source_edit(
        &mut self,
        root: &Path,
        path: &Path,
        before: &str,
        after: &str,
    ) {
        self.record_source_transaction(
            root,
            vec![SourceChange {
                path: path.to_owned(),
                before: before.to_owned(),
                after: after.to_owned(),
            }],
        );
    }

    fn record_source_transaction(&mut self, root: &Path, changes: Vec<SourceChange>) {
        if let Ok(workspace) = self.workspace_mut(root) {
            workspace.source_history.record(changes);
        }
    }
}

pub(super) fn replay_source_history(
    root: &Path,
    undo: bool,
    window: &mut Window,
    cx: &mut App,
) -> Result<bool, String> {
    let key = ProjectKey::from_path(root).map_err(|error| error.to_string())?;
    let Some(mut transaction) = cx
        .global::<EditorDocuments>()
        .workspaces
        .get(key.path())
        .and_then(|workspace| workspace.source_history.transaction(undo))
        .cloned()
    else {
        return Ok(false);
    };
    let mut replacements = Vec::with_capacity(transaction.changes.len());
    for change in &transaction.changes {
        let expected = if undo { &change.after } else { &change.before };
        let replacement = if undo { &change.before } else { &change.after };
        if cx
            .global::<EditorDocuments>()
            .source(root, &change.path)
            .as_deref()
            != Some(expected.as_str())
        {
            return Err(format!(
                "{} changed; refresh before undoing",
                change.path.display()
            ));
        }
        replacements.push((change.path.clone(), replacement.clone()));
    }
    apply_file_changes(root, &mut transaction.files, undo)?;
    if let Err(error) = apply_prepared_edits_impl(root, &replacements, false, window, cx) {
        let rollback = apply_file_changes(root, &mut transaction.files, !undo);
        return Err(format!("{error}; file rollback: {rollback:?}"));
    }
    if let Some(workspace) = cx
        .global_mut::<EditorDocuments>()
        .workspaces
        .get_mut(key.path())
    {
        if let Some(original) = if undo {
            workspace.source_history.undo.back_mut()
        } else {
            workspace.source_history.redo.last_mut()
        } {
            original.files = transaction.files;
        }
        workspace.source_history.finish(undo);
    }
    refresh_resource_files(root, cx);
    Ok(true)
}

fn apply_file_changes(
    root: &Path,
    files: &mut [file_ops::AssetFileChange],
    undo: bool,
) -> Result<(), String> {
    let order = if undo {
        (0..files.len()).rev().collect::<Vec<_>>()
    } else {
        (0..files.len()).collect()
    };
    for (count, &position) in order.iter().enumerate() {
        if let Err(error) = files[position].apply(root, undo) {
            let mut failures = Vec::new();
            for &completed in order[..count].iter().rev() {
                if let Err(error) = files[completed].apply(root, !undo) {
                    failures.push(error.to_string());
                }
            }
            return Err(format!(
                "{error}{}",
                if failures.is_empty() {
                    String::new()
                } else {
                    format!("; rollback failed: {}", failures.join("; "))
                }
            ));
        }
    }
    Ok(())
}

pub(super) fn apply_asset_transaction(
    root: &Path,
    edits: &[(PathBuf, String)],
    mut files: Vec<file_ops::AssetFileChange>,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), String> {
    let changes = edits
        .iter()
        .map(|(path, after)| {
            let before = cx
                .global::<EditorDocuments>()
                .source(root, path)
                .ok_or_else(|| format!("{} unavailable", path.display()))?;
            Ok(SourceChange {
                path: path.clone(),
                before,
                after: after.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    // Prepare every editor before touching files, so a failed open leaves the project intact.
    for (path, _) in edits {
        open_workspace_document(root, path, window, cx);
        if cx
            .global::<EditorDocuments>()
            .editor_for(root, path)
            .and_then(|editor| editor.upgrade())
            .is_none()
        {
            return Err(format!("Could not open {}", path.display()));
        }
    }
    apply_file_changes(root, &mut files, false)?;
    if let Err(error) = apply_prepared_edits_impl(root, edits, false, window, cx) {
        let rollback = apply_file_changes(root, &mut files, true);
        return Err(format!("{error}; file rollback: {rollback:?}"));
    }
    cx.global_mut::<EditorDocuments>()
        .workspace_mut(root)
        .map_err(|error| error.to_string())?
        .source_history
        .record_transaction(SourceTransaction { changes, files });
    refresh_resource_files(root, cx);
    Ok(())
}

pub(super) fn refresh_resource_files(root: &Path, cx: &mut App) {
    let root = root.to_owned();
    let scan_root = root.clone();
    let background = cx.background_executor().spawn(async move {
        WorkspaceSession::open(scan_root).map(|session| session.files().to_vec())
    });
    cx.spawn(async move |cx| {
        let result = background.await;
        cx.update(|cx| {
            if let Ok(files) = result
                && let Some(workspace) =
                    cx.global_mut::<EditorDocuments>().workspaces.get_mut(&root)
            {
                workspace.files = files;
                schedule_authoring_refresh(&root, None, cx);
                cx.refresh_windows();
            }
        });
    })
    .detach();
}

#[cfg(test)]
pub(super) fn insert_assets_at_block(
    source: &str,
    target_start: usize,
    keys: &[AssetKey],
    index: &AuthoringIndex,
) -> Result<String, String> {
    asset_drop_edit(source, target_start, keys, index, false)
}

pub(super) fn asset_drop_edit(
    source: &str,
    target_start: usize,
    keys: &[AssetKey],
    index: &AuthoringIndex,
    insertion: bool,
) -> Result<String, String> {
    asset_drop_edit_at(source, target_start, keys, index, insertion.then_some(true))
}

pub(super) fn asset_drop_edit_at(
    source: &str,
    target_start: usize,
    keys: &[AssetKey],
    index: &AuthoringIndex,
    insertion: Option<bool>,
) -> Result<String, String> {
    if keys.is_empty() {
        return Err("No asset selected".into());
    }
    let projection = EiyashouProjection::parse(source);
    let target = projection
        .scenes
        .iter()
        .flat_map(|scene| &scene.blocks)
        .find(|block| block.source_range.start == target_start)
        .ok_or("Target changed")?;
    if target.read_only {
        return Err("Target is read-only".into());
    }
    let mut assets = keys
        .iter()
        .map(|key| {
            index
                .assets
                .iter()
                .find(|entry| entry.kind == key.kind && entry.id == key.id && entry.exists)
                .ok_or_else(|| format!("{} is unavailable", key.id))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = HashSet::new();
    assets.retain(|asset| seen.insert(asset.key()));
    if assets.iter().any(|asset| !valid_identifier(&asset.id)) {
        return Err("Asset ID is not a script identifier".into());
    }
    if assets.iter().any(|asset| asset.kind == AssetKind::Voice) {
        if assets.len() != 1 {
            return Err("Voice must be dropped alone".into());
        }
        let mut metadata = projection
            .text_block_metadata(source, target_start)
            .ok_or("Voice requires a Text block")?;
        metadata.voice = Some(assets[0].id.clone());
        return projection
            .replace_text_block_metadata(source, target_start, &metadata)
            .map_err(|error| error.to_string());
    }
    let mut edited = source.to_owned();
    let mut after_start = target_start;
    for (position, asset) in assets.iter().enumerate() {
        let statement = match asset.kind {
            AssetKind::Background => format!("background({})", asset.id),
            AssetKind::Figure => {
                format!("sprite({}_slot, {}, position: center)", asset.id, asset.id)
            }
            AssetKind::Bgm => format!("bgm({})", asset.id),
            AssetKind::Effect => format!("se({})", asset.id),
            AssetKind::Video => format!("video({})", asset.id),
            AssetKind::Particle => format!(
                "particle.show({}_emitter, LIGHT_SNOW, texture: {})",
                asset.id, asset.id
            ),
            AssetKind::Voice => unreachable!(),
        };
        if insertion.is_none()
            && position == 0
            && assets.len() == 1
            && matches!(target.kind, BlockKind::Command)
        {
            let fields = projection
                .source_fields(source, target_start)
                .ok_or("Target has no asset field")?;
            let command = source
                .get(target.statement_range.clone())
                .ok_or("Target changed")?
                .split_once('(')
                .map(|(command, _)| command.trim())
                .unwrap_or_default();
            let context = crate::authoring::fields::SourceContext {
                path: PathBuf::new(),
                block_start: target_start,
                kind: target.kind.clone(),
                command: command.to_owned(),
                fields: fields.clone(),
            };
            if let Some(field) = fields.iter().find(|field| {
                crate::authoring::fields::source_asset_accepts(&context, field, asset.kind)
            }) {
                if field.insertion.is_some() {
                    return Err("Asset field has no exact source range".into());
                }
                let value = if field.quoted {
                    escape_eiyashou_string(&asset.id)
                } else {
                    asset_source_value(field, &asset.id)
                };
                edited.replace_range(field.range.clone(), &value);
                return Ok(edited);
            }
            return Err("Resource type is incompatible; drop between Blocks to insert".into());
        }
        let projection = EiyashouProjection::parse(&edited);
        let (next, inserted) = if position == 0 && insertion == Some(false) {
            projection.insert_block_before(&edited, after_start, &statement)
        } else {
            projection.insert_block_after(&edited, after_start, &statement)
        }
        .map_err(|error| error.to_string())?;
        edited = next;
        after_start = inserted.start;
    }
    Ok(edited)
}

pub(super) fn select_asset_keys(
    previous: &[AssetKey],
    ordered: &[AssetKey],
    anchor: Option<&AssetKey>,
    target: &AssetKey,
    shift: bool,
    toggle: bool,
) -> Vec<AssetKey> {
    if shift {
        let anchor = anchor.unwrap_or(target);
        return match (
            ordered.iter().position(|key| key == anchor),
            ordered.iter().position(|key| key == target),
        ) {
            (Some(start), Some(end)) => ordered[start.min(end)..=start.max(end)].to_vec(),
            _ => vec![target.clone()],
        };
    }
    if toggle {
        let mut next = previous.to_vec();
        if let Some(position) = next.iter().position(|key| key == target) {
            next.remove(position);
        } else {
            next.push(target.clone());
        }
        return next;
    }
    vec![target.clone()]
}

pub(super) fn set_authoring_selection(
    root: &Path,
    relative: PathBuf,
    line: usize,
    column: usize,
    cx: &mut App,
) {
    cx.global_mut::<EditorDocuments>()
        .set_selection(root, relative.clone(), line, column);
    let disabled = cx
        .global::<EditorDocuments>()
        .source(root, &relative)
        .and_then(|source| projected_block_at(&source, line, column))
        .is_some_and(|(_, block)| block.disabled);
    if !disabled && let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(root) {
        preview.seek_cursor(relative, line + 1, column + 1);
    }
    cx.refresh_windows();
}

pub(super) fn follow_preview_position(
    root: &Path,
    relative: &Path,
    line: usize,
    column: usize,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    if !relative.starts_with("scripts")
        || relative.extension().and_then(|value| value.to_str()) != Some("shou")
    {
        return false;
    }
    let Some(source) = cx.global::<EditorDocuments>().source(root, relative) else {
        return false;
    };
    let line = line.saturating_sub(1);
    let column = column.saturating_sub(1);
    let block = projected_block_at(&source, line, column);
    let current_path = cx
        .global::<EditorDocuments>()
        .selection(root)
        .map(|(path, _, _)| path.clone());
    if current_path.as_deref() != Some(relative) {
        open_workspace_document(root, relative, window, cx);
    }
    let documents = cx.global_mut::<EditorDocuments>();
    documents.set_selection(root, relative.to_owned(), line, column);
    if let Some((_, block)) = &block {
        documents.set_block_selection(root, relative.to_owned(), vec![block.source_range.start]);
    } else {
        documents.clear_block_selection(root);
    }
    if let Some(panel) = documents.panel_entity_for(root, relative) {
        let _ = panel.update(cx, |panel, cx| {
            let selected = block.as_ref().map(|(_, block)| block.source_range.start);
            if panel.selected_blocks.len() != usize::from(selected.is_some())
                || selected.is_some_and(|start| !panel.selected_blocks.contains(&start))
            {
                panel.selected_blocks.clear();
                panel.block_selection_anchor = selected;
                if let Some((scene, block)) = &block {
                    panel.selected_blocks.insert(block.source_range.start);
                    panel.collapsed_scenes.remove(scene);
                    panel.block_scroll_pending = true;
                }
                cx.notify();
            }
        });
    }
    cx.refresh_windows();
    true
}

pub(super) fn apply_workspace_edit(
    root: &Path,
    relative: &Path,
    edited: String,
    window: &mut Window,
    cx: &mut App,
) {
    let _ = apply_prepared_edits(root, &[(relative.to_owned(), edited)], window, cx);
}

pub(super) fn prepare_asset_edits(
    root: &Path,
    index: &AuthoringIndex,
    asset: &crate::authoring::AssetEntry,
    new_id: &str,
    new_kind: AssetKind,
    tags: &[String],
    source_for: impl Fn(&Path) -> Option<String>,
) -> Result<Vec<(PathBuf, String)>, String> {
    if new_id == asset.id && new_kind == asset.kind && tags == asset.tags {
        return Ok(Vec::new());
    }
    let manifest = index.assets_manifest.as_ref().ok_or("No asset manifest")?;
    let manifest_source = source_for(manifest).ok_or("Asset manifest unavailable")?;
    let renamed = new_id != asset.id;
    let retyped = new_kind != asset.kind;
    if (renamed || retyped) && !index.unindexed_sources.is_empty() {
        return Err(format!(
            "{} script sources could not be indexed",
            index.unindexed_sources.len()
        ));
    }
    if retyped {
        file_ops::validate_asset_type(root, &asset.path, new_kind)
            .map_err(|error| error.to_string())?;
    }
    let edited_manifest =
        file_ops::edit_manifest_asset(&manifest_source, asset, new_id, new_kind, tags)
            .map_err(|error| error.to_string())?;
    let mut edits = BTreeMap::new();
    if let Some(path) = &index.characters_manifest {
        let references = index.characters.iter().any(|character| {
            character.avatar.as_deref() == Some(asset.id.as_str())
                || character
                    .expressions
                    .values()
                    .flatten()
                    .any(|id| id == &asset.id)
        });
        if references && retyped && new_kind != AssetKind::Figure {
            return Err(
                "Remove this image from character expressions/avatars before changing its type"
                    .into(),
            );
        }
        if references && renamed {
            let source = source_for(path).ok_or("Character manifest unavailable")?;
            let updated = crate::authoring::rename_character_asset(&source, &asset.id, new_id)
                .map_err(|error| error.to_string())?;
            edits.insert(path.clone(), updated);
        }
    }
    if renamed {
        let references = index
            .asset_references
            .iter()
            .filter(|reference| reference.key == asset.key())
            .collect::<Vec<_>>();
        if references.len() != asset.reference_count
            || references.iter().any(|reference| reference.range.is_none())
        {
            return Err("A script reference cannot be located exactly".into());
        }
        let mut ranges_by_path = BTreeMap::<PathBuf, Vec<Range<usize>>>::new();
        for reference in references {
            ranges_by_path
                .entry(reference.path.clone())
                .or_default()
                .push(reference.range.clone().expect("checked above"));
        }
        for (path, mut ranges) in ranges_by_path {
            let mut source =
                source_for(&path).ok_or_else(|| format!("{} unavailable", path.display()))?;
            ranges.sort_by_key(|range| range.start);
            if ranges
                .windows(2)
                .any(|pair| pair[0].end > pair[1].start || pair[0] == pair[1])
            {
                return Err(format!("Ambiguous reference in {}", path.display()));
            }
            for range in ranges.into_iter().rev() {
                let raw = source
                    .get(range.clone())
                    .ok_or("Script changed; refresh Inspector")?;
                let replacement = if raw == asset.id {
                    new_id.to_owned()
                } else if raw == format!("\"{}\"", asset.id) {
                    format!("\"{new_id}\"")
                } else {
                    return Err("Script changed; refresh Inspector".into());
                };
                source.replace_range(range, &replacement);
            }
            edits.insert(path, source);
        }
    }
    if edited_manifest != manifest_source {
        edits.insert(manifest.clone(), edited_manifest);
    }
    Ok(edits.into_iter().collect())
}

pub(super) fn apply_prepared_edits(
    root: &Path,
    edits: &[(PathBuf, String)],
    window: &mut Window,
    cx: &mut App,
) -> Result<(), String> {
    apply_prepared_edits_impl(root, edits, true, window, cx)
}

/// Save is the only automatic formatting boundary. Update the shared document
/// immediately: GPUI's Change subscriber runs after this action returns.
pub(super) fn format_and_save(
    root: &Path,
    window: &mut Window,
    cx: &mut App,
) -> Result<usize, crate::document::SaveError> {
    use gpui_kit::EntityInputHandler;
    let documents = {
        let workspace = cx.global_mut::<EditorDocuments>().workspace_mut(root)?;
        if workspace.file_operation_active {
            return Err(io::Error::other(
                "A file operation is still running; save when it finishes",
            )
            .into());
        }
        workspace.manager.documents().cloned().collect::<Vec<_>>()
    };
    let mut changes = Vec::new();
    for document in documents {
        let (path, before) = {
            let document = document.borrow();
            (
                document.relative_path().to_owned(),
                document.contents().to_owned(),
            )
        };
        if path.extension().is_none_or(|extension| extension != "shou") {
            continue;
        }
        let editor = cx
            .global::<EditorDocuments>()
            .editor_for(root, &path)
            .and_then(|editor| editor.upgrade());
        if editor.as_ref().is_some_and(|editor| {
            editor.update(cx, |editor, cx| {
                editor.marked_text_range(window, cx).is_some()
            })
        }) {
            return Err(io::Error::other("Finish text composition before saving").into());
        }
        let Some(after) =
            keine_loader::format_native_source(&before).filter(|after| after != &before)
        else {
            continue;
        };
        if after.len() > 1024 * 1024 {
            return Err(io::Error::other("Formatted document exceeds the 1 MiB size limit").into());
        }
        changes.push((
            document,
            editor,
            SourceChange {
                path,
                before,
                after,
            },
        ));
    }
    let mut history = Vec::new();
    for (document, editor, change) in changes {
        let original_tokens = keine_loader::native_tokens(&change.before);
        let formatted_tokens = keine_loader::native_tokens(&change.after);
        let map = |offset| {
            formatted_offset(
                &original_tokens,
                &formatted_tokens,
                change.after.len(),
                offset,
            )
        };
        document
            .borrow_mut()
            .replace_contents(change.after.clone())?;
        if let Some(panel) = cx
            .global::<EditorDocuments>()
            .panel_entity_for(root, &change.path)
        {
            let _ = panel.update(cx, |panel, _| {
                panel.selected_blocks = panel.selected_blocks.iter().copied().map(map).collect();
                panel.block_selection_anchor = panel.block_selection_anchor.map(map);
                // Formatting changes byte offsets, not Block geometry. Keep the
                // measured heights under their new identities to avoid a relayout jump.
                panel.block_heights = std::mem::take(&mut panel.block_heights)
                    .into_iter()
                    .map(|(start, height)| (map(start), height))
                    .collect();
                *panel.block_row_positions.borrow_mut() = panel
                    .block_row_positions
                    .take()
                    .into_iter()
                    .map(|(start, y)| (map(start), y))
                    .collect();
                *panel.block_row_bounds.borrow_mut() = panel
                    .block_row_bounds
                    .take()
                    .into_iter()
                    .map(|(start, bounds)| (map(start), bounds))
                    .collect();
                for row in &mut panel.block_text_editors {
                    row.text_start = map(row.text_start);
                }
                if let Some(draft) = &mut panel.draft_text
                    && let Some(range) = &mut draft.text_range
                {
                    *range = map(range.start)..map(range.end);
                }
            });
        }
        if let Some(editor) = editor {
            let anchor = editor.update(cx, |editor, cx| {
                let selection = editor.selected_range();
                let mut scroll = editor.scroll_offset();
                let anchor = std::iter::once(editor.cursor())
                    .chain(original_tokens.iter().map(|token| token.range.start))
                    .find_map(|offset| {
                        let bounds = editor.range_to_bounds(&(offset..offset))?;
                        let viewport = editor.input_bounds();
                        (bounds.top() >= viewport.top() && bounds.top() < viewport.bottom())
                            .then_some((offset, bounds.top()))
                    });
                if let Some((offset, _)) = anchor
                    && let Some(height) = editor.line_height()
                {
                    let old_line = change.before[..offset]
                        .bytes()
                        .filter(|b| *b == b'\n')
                        .count();
                    let new_line = change.after[..map(offset)]
                        .bytes()
                        .filter(|b| *b == b'\n')
                        .count();
                    scroll.y -= height * (new_line as f32 - old_line as f32);
                }
                editor.replace_all(change.after.clone(), window, cx);
                editor.set_selected_range(map(selection.start)..map(selection.end), cx);
                // This overrides replace_all's reset and selection's reveal request.
                editor.set_scroll_offset(scroll, cx);
                anchor.map(|(offset, y)| (map(offset), y))
            });
            if let Some((offset, y)) = anchor {
                let expected = editor.read(cx).text().clone();
                // Resolve soft wrapping against the new layout, without a timer
                // or another cursor/focus change. Plain lines already stay put.
                window.on_next_frame(move |_, cx| {
                    editor.update(cx, |editor, cx| {
                        if editor.text() == &expected
                            && let Some(bounds) = editor.range_to_bounds(&(offset..offset))
                        {
                            let mut scroll = editor.scroll_offset();
                            let delta = y - bounds.top();
                            if delta.abs() > px(0.5) {
                                scroll.y += delta;
                                editor.set_scroll_offset(scroll, cx);
                            }
                        }
                    });
                });
            }
        }
        schedule_authoring_refresh(root, Some(&change.path), cx);
        history.push(change);
    }
    cx.global_mut::<EditorDocuments>()
        .record_source_transaction(root, history);
    cx.global_mut::<EditorDocuments>().save_all(root)
}

fn formatted_offset(
    original: &[keine_loader::NativeToken],
    formatted: &[keine_loader::NativeToken],
    after_len: usize,
    offset: usize,
) -> usize {
    use keine_loader::NativeTokenKind;
    let mut previous = 0;
    for (old, new) in original
        .iter()
        .filter(|token| token.kind != NativeTokenKind::Whitespace)
        .zip(
            formatted
                .iter()
                .filter(|token| token.kind != NativeTokenKind::Whitespace),
        )
    {
        if offset < old.range.start {
            return previous;
        }
        if offset <= old.range.end {
            return new.range.start + offset - old.range.start;
        }
        previous = new.range.end;
    }
    after_len
}

#[cfg(test)]
mod formatting_tests {
    use super::*;

    struct SaveView(Entity<EditorState>);

    impl Render for SaveView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(Editor::new(&self.0).size_full())
        }
    }

    #[gpui_kit::test]
    fn save_preserves_selection_and_its_screen_position(cx: &mut gpui_kit::TestAppContext) {
        let temporary = std::env::temp_dir().join(format!(
            "keine-save-view-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = temporary.join("project");
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("config.yaml"),
            include_str!("../../../../tests/fixtures/native-smoke/config.yaml"),
        )
        .unwrap();
        let source = format!(
            "scene start {{wait(1s),\n{}\n}}",
            (0..80)
                .map(|i| format!("\"对白{i}中文\","))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let path = Path::new("scripts/main.shou");
        fs::write(root.join(path), &source).unwrap();
        let root = root.canonicalize().unwrap();
        cx.update(gpui_kit::init);
        let mut editor = None;
        let window = cx.open_window(size(px(800.), px(300.)), |window, cx| {
            let session = crate::workspace::WorkspaceSession::open(&root).unwrap();
            let mut documents = EditorDocuments::new(crate::persistence::AppPersistence::new(
                temporary.join("app-data"),
            ));
            documents
                .ensure_workspace_with_files(session.root(), session.files())
                .unwrap();
            documents.open(session.root(), path).unwrap();
            let state = cx.new(|cx| EditorState::new(window, cx).default_value(source.clone()));
            documents.register_editor(session.root(), path.to_owned(), state.downgrade());
            cx.set_global(documents);
            editor = Some(state.clone());
            SaveView(state)
        });
        cx.run_until_parked();
        let editor = editor.unwrap();
        window
            .update(cx, |_, window, cx| {
                editor.update(cx, |editor, cx| {
                    let start = source.find("对白30中文").unwrap();
                    editor.set_cursor_position(Position::new(31, 1), window, cx);
                    editor.set_selected_range(start..start + "对白30中文".len(), cx);
                    let height = editor.line_height().unwrap();
                    editor.set_scroll_offset(gpui_kit::point(px(0.), -height * 25.), cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        let before = editor.read_with(cx, |editor, _| {
            assert!(editor.scroll_offset().y < px(-100.));
            editor
                .range_to_bounds(&editor.selected_range())
                .unwrap()
                .top()
        });
        window
            .update(cx, |_, window, cx| {
                assert_eq!(format_and_save(&root, window, cx).unwrap(), 1);
            })
            .unwrap();
        cx.run_until_parked();
        let after = editor.read_with(cx, |editor, _| {
            assert_eq!(editor.selected_value().as_ref(), "对白30中文");
            let after = editor
                .range_to_bounds(&editor.selected_range())
                .unwrap()
                .top();
            assert!((after - before).abs() < px(0.5), "{before:?} -> {after:?}");
            editor.scroll_offset()
        });
        window
            .update(cx, |_, window, cx| {
                assert!(editor.read(cx).focus_handle(cx).is_focused(window));
                assert_eq!(format_and_save(&root, window, cx).unwrap(), 0);
            })
            .unwrap();
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| assert_eq!(editor.scroll_offset(), after));
        fs::remove_dir_all(temporary).unwrap();
    }

    #[gpui_kit::test]
    fn save_formats_shared_source_and_editor_before_writing(cx: &mut gpui_kit::TestAppContext) {
        let temporary = std::env::temp_dir().join(format!(
            "keine-format-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = temporary.join("project");
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("config.yaml"),
            include_str!("../../../../tests/fixtures/native-smoke/config.yaml"),
        )
        .unwrap();
        let source = "scene start {wait(1s),\"中文\"}";
        let path = Path::new("scripts/main.shou");
        fs::write(root.join(path), source).unwrap();
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            gpui_kit::init(cx);
            let session = crate::workspace::WorkspaceSession::open(&root).unwrap();
            let root = session.root();
            let mut documents = EditorDocuments::new(crate::persistence::AppPersistence::new(
                temporary.join("app-data"),
            ));
            documents
                .ensure_workspace_with_files(root, session.files())
                .unwrap();
            let document = documents.open(root, path).unwrap();
            let editor = cx.new(|cx| EditorState::new(window, cx).default_value(source));
            documents.register_editor(root, path.to_owned(), editor.downgrade());
            cx.set_global(documents);
            let position = Position::new(0, source.find('中').unwrap() as u32);
            editor.update(cx, |editor, cx| {
                editor.set_cursor_position(position, window, cx)
            });
            assert_eq!(format_and_save(root, window, cx).unwrap(), 1);
            let formatted = keine_loader::format_native_source(source).unwrap();
            assert_eq!(fs::read_to_string(root.join(path)).unwrap(), formatted);
            assert_eq!(editor.read(cx).value().as_ref(), formatted);
            assert_eq!(editor.read(cx).cursor_position(), Position::new(2, 3));
            assert_eq!(document.borrow().contents(), formatted);
            assert!(!document.borrow().is_dirty());
            assert_eq!(format_and_save(root, window, cx).unwrap(), 0);
            let workspace = cx.global::<EditorDocuments>().workspaces.get(root).unwrap();
            let transaction = workspace.source_history.next(true).unwrap();
            assert_eq!(transaction[0].before, source);
            assert_eq!(transaction[0].after, formatted);
            fs::write(root.join(path), "external").unwrap();
            document
                .borrow_mut()
                .replace_contents("scene start {wait(2s)}".into())
                .unwrap();
            assert!(format_and_save(root, window, cx).is_err());
            assert_eq!(fs::read_to_string(root.join(path)).unwrap(), "external");
        });
        fs::remove_dir_all(temporary).unwrap();
    }
}

fn apply_prepared_edits_impl(
    root: &Path,
    edits: &[(PathBuf, String)],
    record: bool,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), String> {
    let changes = if record {
        edits
            .iter()
            .map(|(path, after)| {
                let before = cx
                    .global::<EditorDocuments>()
                    .source(root, path)
                    .ok_or_else(|| format!("{} unavailable", path.display()))?;
                Ok(SourceChange {
                    path: path.clone(),
                    before,
                    after: after.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?
    } else {
        Vec::new()
    };
    let mut editors = Vec::with_capacity(edits.len());
    for (path, _) in edits {
        open_workspace_document(root, path, window, cx);
        let editor = cx
            .global::<EditorDocuments>()
            .editor_for(root, path)
            .and_then(|editor| editor.upgrade())
            .ok_or_else(|| format!("Could not open {}", path.display()))?;
        editors.push(editor);
    }
    for ((_, source), editor) in edits.iter().zip(editors) {
        editor.update(cx, |editor, cx| {
            editor.replace_all(source.clone(), window, cx)
        });
    }
    if record {
        cx.global_mut::<EditorDocuments>()
            .record_source_transaction(root, changes);
    }
    Ok(())
}

pub(super) fn navigate_source(
    root: &Path,
    relative: &Path,
    line: usize,
    column: usize,
    window: &mut Window,
    cx: &mut App,
) {
    open_workspace_document(root, relative, window, cx);
    if let Some(editor) = cx.global::<EditorDocuments>().editor_for(root, relative) {
        let _ = editor.update(cx, |editor, cx| {
            editor.set_cursor_position(
                Position::new(
                    line.saturating_sub(1) as u32,
                    column.saturating_sub(1) as u32,
                ),
                window,
                cx,
            );
        });
    }
}

pub(super) fn projected_block_at(
    source: &str,
    line: usize,
    column: usize,
) -> Option<(String, crate::projection::BlockCard)> {
    block_at_position(&EiyashouProjection::parse(source), source, line, column)
}

pub(super) fn block_at_position(
    projection: &EiyashouProjection,
    source: &str,
    line: usize,
    column: usize,
) -> Option<(String, crate::projection::BlockCard)> {
    let offset = keine_loader::SourceLineIndex::new(source).offset(source, line, column);
    block_at_offset(projection, offset).map(|(scene, block)| (scene.name.clone(), block.clone()))
}

pub(super) fn block_at_offset(
    projection: &EiyashouProjection,
    offset: usize,
) -> Option<(
    &crate::projection::SceneSection,
    &crate::projection::BlockCard,
)> {
    projection.scenes.iter().find_map(|scene| {
        if !scene.source_range.contains(&offset) && offset != scene.source_range.end {
            return None;
        }
        scene
            .blocks
            .iter()
            .filter(|block| !block.is_textbox_ending())
            .filter(|block| block.source_range.start <= offset && block.source_range.end >= offset)
            .max_by_key(|block| block.depth)
            .map(|block| (scene, block))
    })
}

#[cfg(test)]
mod preview_follow_tests {
    use super::*;

    #[test]
    fn source_cursor_after_chinese_selects_the_following_command() {
        let source = "scene start {\n  \"中文中文中文中文中文\", wait(500ms)\n}\n";
        let wait = source.find("wait").unwrap();
        let start = source[..wait].rfind('\n').unwrap() + 1;
        let column = source[start..wait].chars().count();
        assert_eq!(
            projected_block_at(source, 1, column).unwrap().1.kind,
            BlockKind::Command
        );
    }

    #[test]
    fn native_preview_action_lines_resolve_to_visible_blocks() {
        let source = "scene start {\n  \"One\",\n  \"Two\"\n}\n";
        for line in [2, 3] {
            assert!(
                projected_block_at(source, line - 1, 2).is_some(),
                "Preview source line {line} should select a Block"
            );
        }
    }
}

pub(super) fn common_value(values: impl IntoIterator<Item = String>) -> String {
    let mut values = values.into_iter();
    let Some(first) = values.next() else {
        return "—".to_owned();
    };
    if values.all(|value| value == first) {
        first
    } else {
        "Mixed".to_owned()
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;

    #[test]
    fn resource_replacement_preserves_properties_and_multi_drag_preserves_selection_order() {
        let source =
            "scene start { sprite(hero, old, position: left, z: 3, transition: fade(2s)) }";
        let asset = |id: &str| crate::authoring::AssetEntry {
            kind: AssetKind::Figure,
            id: id.into(),
            path: format!("assets/figures/{id}.webp").into(),
            tags: Vec::new(),
            exists: true,
            reference_count: 0,
        };
        let index = AuthoringIndex {
            assets: vec![asset("first"), asset("second")],
            ..Default::default()
        };
        let start = source.find("sprite").unwrap();
        assert_eq!(
            insert_assets_at_block(source, start, &[index.assets[1].key()], &index).unwrap(),
            source.replace("old", "second")
        );
        let edited = insert_assets_at_block(
            source,
            start,
            &[index.assets[1].key(), index.assets[0].key()],
            &index,
        )
        .unwrap();
        assert!(edited.find("second_slot").unwrap() < edited.find("first_slot").unwrap());
    }

    #[test]
    fn failed_file_transaction_rolls_back_completed_moves() {
        let root =
            std::env::temp_dir().join(format!("keine-asset-rollback-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("one.webp"), b"one").unwrap();
        fs::write(root.join("two.webp"), b"two").unwrap();
        fs::write(root.join("collision.webp"), b"untouched").unwrap();
        let mut files = vec![
            file_ops::AssetFileChange::relocate("one.webp".into(), "new.webp".into()),
            file_ops::AssetFileChange::relocate("two.webp".into(), "collision.webp".into()),
        ];
        assert!(apply_file_changes(&root, &mut files, false).is_err());
        assert_eq!(fs::read(root.join("one.webp")).unwrap(), b"one");
        assert!(!root.join("new.webp").exists());
        assert_eq!(fs::read(root.join("collision.webp")).unwrap(), b"untouched");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_history_enforces_a_byte_budget_across_undo_and_redo() {
        let mut history = SourceHistory::default();
        for i in 0..20 {
            history.record(vec![SourceChange {
                path: "scripts/main.shou".into(),
                before: "a".repeat(1024 * 1024),
                after: format!("{i}{}", "b".repeat(1024 * 1024 - 2)),
            }]);
            assert!(history.bytes() <= SourceHistory::BYTE_BUDGET);
        }
        assert!(history.undo.len() < 20);
        history.finish(true);
        assert!(history.next(false).is_some());
        history.forget(Path::new("scripts/main.shou"));
        assert_eq!(history.bytes(), 0);
    }

    #[test]
    fn multi_file_source_edit_is_one_undo_step() {
        let mut history = SourceHistory::default();
        history.record(vec![
            SourceChange {
                path: "assets.yaml".into(),
                before: "old asset".into(),
                after: "new asset".into(),
            },
            SourceChange {
                path: "scripts/main.shou".into(),
                before: "old reference".into(),
                after: "new reference".into(),
            },
        ]);
        assert_eq!(history.next(true).unwrap().len(), 2);
        history.finish(true);
        assert!(history.next(true).is_none());
        assert_eq!(history.next(false).unwrap().len(), 2);
        history.finish(false);
        assert_eq!(history.next(true).unwrap().len(), 2);
    }

    #[test]
    fn a_new_source_edit_discards_redo() {
        let mut history = SourceHistory::default();
        let change = |before: &str, after: &str| SourceChange {
            path: "assets.yaml".into(),
            before: before.into(),
            after: after.into(),
        };
        history.record(vec![change("a", "b")]);
        history.finish(true);
        history.record(vec![change("a", "c")]);
        assert!(history.next(false).is_none());
    }
}
