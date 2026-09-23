use super::*;

pub(super) fn insert_assets_at_block(
    source: &str,
    target_start: usize,
    keys: &[AssetKey],
    index: &AuthoringIndex,
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
    assets.sort_by_key(|asset| {
        index
            .assets
            .iter()
            .position(|candidate| std::ptr::eq(candidate, *asset))
            .unwrap_or(usize::MAX)
    });
    assets.dedup_by(|left, right| left.kind == right.kind && left.id == right.id);
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
            AssetKind::Voice => unreachable!(),
        };
        if position == 0 && assets.len() == 1 && matches!(target.kind, BlockKind::Command) {
            let old = source
                .get(target.source_range.clone())
                .ok_or("Target changed")?;
            let compatible = old.trim_start().starts_with(match asset.kind {
                AssetKind::Background => "background(",
                AssetKind::Figure => "sprite(",
                AssetKind::Bgm => "bgm(",
                AssetKind::Effect => "se(",
                AssetKind::Video => "video(",
                AssetKind::Voice => unreachable!(),
            });
            if compatible {
                edited.replace_range(target.source_range.clone(), &statement);
                return Ok(edited);
            }
        }
        let (next, inserted) = EiyashouProjection::parse(&edited)
            .insert_block_after(&edited, after_start, &statement)
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
    if let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(root) {
        preview.set_cursor(relative, line + 1, column + 1);
    }
    cx.refresh_windows();
}

pub(super) fn apply_workspace_edit(
    root: &Path,
    relative: &Path,
    edited: String,
    window: &mut Window,
    cx: &mut App,
) {
    open_workspace_document(root, relative, window, cx);
    if let Some(editor) = cx.global::<EditorDocuments>().editor_for(root, relative) {
        let _ = editor.update(cx, |editor, cx| editor.replace_all(edited, window, cx));
    }
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
    if retyped && asset.reference_count > 0 {
        return Err(format!("{} refs block type change", asset.reference_count));
    }
    if retyped {
        file_ops::validate_asset_type(root, &asset.path, new_kind)
            .map_err(|error| error.to_string())?;
    }
    let edited_manifest =
        file_ops::edit_manifest_asset(&manifest_source, asset, new_id, new_kind, tags)
            .map_err(|error| error.to_string())?;
    let mut edits = BTreeMap::new();
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
    let line_start = source
        .split_inclusive('\n')
        .take(line)
        .map(str::len)
        .sum::<usize>();
    let offset = (line_start + column).min(source.len());
    let projection = EiyashouProjection::parse(source);
    projection.scenes.into_iter().find_map(|scene| {
        if !scene.source_range.contains(&offset) && offset != scene.source_range.end {
            return None;
        }
        scene
            .blocks
            .iter()
            .filter(|block| block.source_range.start <= offset && block.source_range.end >= offset)
            .max_by_key(|block| block.depth)
            .cloned()
            .map(|block| (scene.name, block))
    })
}

pub(super) fn text_voice(source: &str) -> Option<String> {
    let quote = source.rfind('"')?;
    source[quote + 1..]
        .strip_prefix(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
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
