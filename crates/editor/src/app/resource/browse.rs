//! Responsive resource browsing; only visible rows mount image elements.
use super::*;
use crate::authoring::{AssetEntry, UnmappedAsset};

pub(in crate::app) fn render_assets(
    root: &Path,
    index: &Arc<AuthoringIndex>,
    panel: &WorkbenchPanel,
    window: &Window,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let query = AssetQuery {
        search: panel.asset_search.read(cx).value().to_string(),
        kind: panel.asset_kind,
        folder: panel.asset_folder.clone(),
        sort: panel.asset_sort,
        tag: panel.asset_tag.clone(),
        status: panel.asset_status,
        size: panel.asset_size,
        modified: panel.asset_modified,
    };
    let bounds = panel.view_scroll.bounds();
    let offset = panel.view_scroll.offset();
    let width = (if bounds.size.width > px(0.) {
        f32::from(bounds.size.width)
    } else {
        280.
    } - 16.)
        .max(1.);
    let height = if bounds.size.height > px(0.) {
        f32::from(bounds.size.height)
    } else {
        f32::from(window.viewport_size().height)
    };
    let columns = (width / if panel.asset_large { 176. } else { 120. })
        .floor()
        .max(1.) as usize;
    let snapshot = panel.asset_browser.borrow_mut().resolve(
        index,
        &query,
        panel.asset_unmapped,
        columns,
        panel.asset_large,
        panel.asset_grid,
    );
    let results = &snapshot.assets;
    let rows = &snapshot.rows;
    let ordered = snapshot.ordered.clone();
    let unmapped_mode = panel.asset_unmapped;
    let selection = cx.global::<EditorDocuments>().asset_selection(root);
    let filter_active = panel.asset_tag.is_some()
        || panel.asset_status != crate::authoring::AssetStatus::All
        || panel.asset_size != crate::authoring::AssetSize::All
        || panel.asset_modified.is_some()
        || panel.asset_kind.is_some()
        || panel.asset_folder.is_some()
        || panel.asset_sort != AssetSort::Name
        || panel.asset_grid.is_some()
        || panel.asset_large
        || panel.asset_unmapped;
    let filter_menu = panel
        .asset_filter_menu
        .clone()
        .map(|menu| render_asset_filter_menu(menu, &snapshot.folders, index, panel, cx));
    let controls = div()
        .flex()
        .p_2()
        .gap_1()
        .items_center()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(28.))
                .rounded(px(7.))
                .bg(rgb(SURFACE))
                .px_2()
                .child(
                    Input::new(&panel.asset_search)
                        .appearance(false)
                        .bordered(false)
                        .size_full()
                        .text_sm()
                        .text_color(rgb(INK)),
                ),
        )
        .child(
            div()
                .id("asset-filter-button")
                .size(px(28.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .bg(rgb(if filter_active || panel.asset_filter_menu.is_some() {
                    SURFACE
                } else {
                    CHROME
                }))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                .tooltip(icon_hint("Filter assets"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        this.toggle_asset_filter_menu(event.position, window, cx);
                        cx.stop_propagation();
                    }),
                )
                .child(
                    Icon::new(AssetIconName::ListFilter)
                        .xsmall()
                        .text_color(rgb(if filter_active { PRIMARY } else { MUTED })),
                ),
        );
    let root = root.to_owned();
    let (visible, before, after) = visible_rows(rows, -f32::from(offset.y), height);
    let cell_width = (width - (columns - 1) as f32 * 4.) / columns as f32;
    let cards = div()
        .id("asset-cards")
        .w_full()
        .flex()
        .flex_col()
        .p_2()
        .child(div().h(px(before)).flex_none())
        .children(rows[visible].iter().map(|layout| {
            div()
                .w_full()
                .h(px(layout.height))
                .flex_none()
                .flex()
                .gap_1()
                .children(layout.items.clone().map(|row| {
                    let asset = &results[row];
                    let media = index.media.get(&asset.path);
                    let key = asset.key();
                    let row_key = key.clone();
                    let row_root = root.clone();
                    let row_order = ordered.clone();
                    let selected = !unmapped_mode && selection.contains(&key);
                    let unmapped = UnmappedAsset {
                        kind: asset.kind,
                        path: asset.path.clone(),
                    };
                    div()
                        .id(("asset-row", row))
                        .when(layout.grid, |this| {
                            this.w(px(cell_width)).flex_none().flex_col()
                        })
                        .when(!layout.grid, |this| this.w_full().items_center())
                        .h(px(layout.height - 4.))
                        .flex()
                        .min_w_0()
                        .gap_1()
                        .p_1()
                        .when(!layout.grid, |this| this.px_2().py(px(6.)))
                        .rounded(px(7.))
                        .bg(rgb(if selected { SURFACE } else { PANEL }))
                        .tooltip(icon_hint(format!(
                            "{}{} · {}",
                            if !unmapped_mode && asset.exists {
                                "Registered resource · "
                            } else {
                                ""
                            },
                            asset.path.display(),
                            asset.tags.join(", ")
                        )))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.focus.focus(window, cx);
                            if unmapped_mode {
                                cx.global_mut::<EditorDocuments>()
                                    .set_unmapped_asset_preview(&row_root, &unmapped);
                                window.dispatch_action(Box::new(ShowAssetPreview), cx);
                                cx.refresh_windows();
                                return;
                            }
                            let previous =
                                cx.global::<EditorDocuments>().asset_selection(&row_root);
                            let modifiers = event.modifiers();
                            let next = select_asset_keys(
                                &previous,
                                &row_order,
                                this.asset_anchor.as_ref(),
                                &row_key,
                                modifiers.shift,
                                modifiers.platform || modifiers.control,
                            );
                            if !modifiers.shift {
                                this.asset_anchor = Some(row_key.clone());
                            }
                            let show_preview = !next.is_empty();
                            cx.global_mut::<EditorDocuments>()
                                .set_asset_selection(&row_root, next);
                            if show_preview {
                                window.dispatch_action(Box::new(ShowAssetPreview), cx);
                            }
                            cx.refresh_windows();
                        }))
                        .when(!unmapped_mode, |this| {
                            this.on_drag(
                                AssetDrag {
                                    root: root.clone(),
                                    keys: if selected {
                                        selection.clone()
                                    } else {
                                        vec![key]
                                    },
                                },
                                |_, _, _, cx| cx.new(|_| Empty),
                            )
                        })
                        .child(asset_content(
                            &root,
                            asset,
                            media,
                            layout.grid,
                            &panel.asset_thumbnails,
                            cx,
                        ))
                }))
        }))
        .child(div().h(px(after)).flex_none())
        .on_click(cx.listener({
            let root = root.clone();
            move |this, _, _, cx| {
                this.asset_anchor = None;
                cx.global_mut::<EditorDocuments>()
                    .clear_asset_selection(&root);
                cx.refresh_windows();
            }
        }));
    // The scroll handle changes during layout, including scrollbar dragging.
    // Refresh the bounded row window only when its viewport actually changes.
    let handle = panel.view_scroll.clone();
    let entity = cx.weak_entity();
    div()
        .relative()
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .child(controls.flex_none())
        .child(
            div()
                .px_2()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(format!("{} assets", results.len())),
        )
        .child(
            div()
                .id("asset-viewport")
                .relative()
                .flex_1()
                .min_h_0()
                .on_click(cx.listener({
                    let root = root.clone();
                    move |this, _, _, cx| {
                        this.asset_anchor = None;
                        cx.global_mut::<EditorDocuments>()
                            .clear_asset_selection(&root);
                        cx.refresh_windows();
                    }
                }))
                .child(vertical_overflow_view(
                    "asset-scroll",
                    &panel.view_scroll,
                    cards,
                ))
                .child(
                    canvas(
                        move |_, _, cx| {
                            if handle.bounds() != bounds || handle.offset() != offset {
                                let entity = entity.clone();
                                cx.defer(move |cx| {
                                    let _ = entity.update(cx, |_, cx| cx.notify());
                                });
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                ),
        )
        .when_some(filter_menu, |this, menu| this.child(menu))
        .into_any_element()
}

fn image_kind(kind: AssetKind) -> bool {
    matches!(
        kind,
        AssetKind::Background | AssetKind::Figure | AssetKind::Particle
    )
}

fn asset_content(
    root: &Path,
    asset: &AssetEntry,
    media: Option<&file_ops::AssetMediaInfo>,
    grid: bool,
    thumbnails: &Entity<super::thumbnail::Thumbnails>,
    cx: &mut App,
) -> AnyElement {
    let image = image_kind(asset.kind);
    let audio = matches!(
        asset.kind,
        AssetKind::Bgm | AssetKind::Voice | AssetKind::Effect
    );
    let icon = if image {
        AssetIconName::Image
    } else if audio {
        AssetIconName::Music
    } else {
        AssetIconName::Film
    };
    let thumbnail = div()
        .flex_none()
        .rounded(px(5.))
        .overflow_hidden()
        .bg(rgb(CANVAS))
        .when(grid, |this| this.w_full().flex_1().min_h_0())
        .when(!grid, |this| this.size(px(32.)))
        .flex()
        .items_center()
        .justify_center()
        .child(if image {
            media
                .filter(|_| asset.exists)
                .map(|media| {
                    super::thumbnail::Thumbnails::image(thumbnails, root, &asset.path, media)
                        .size_full()
                        .with_fallback(|| {
                            Icon::new(AssetIconName::Image)
                                .small()
                                .text_color(rgb(MUTED))
                                .into_any_element()
                        })
                        .into_any_element()
                })
                .unwrap_or_else(|| {
                    Icon::new(icon)
                        .small()
                        .text_color(rgb(MUTED))
                        .into_any_element()
                })
        } else {
            Icon::new(icon)
                .small()
                .text_color(rgb(PRIMARY))
                .into_any_element()
        });
    let name = asset
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| asset.id.clone());
    let detail = if asset.id.is_empty() {
        "Unmapped".to_owned()
    } else if !asset.exists {
        "Missing".to_owned()
    } else {
        format!("{} · {} refs", asset.id, asset.reference_count)
    };
    let mut metadata = String::new();
    if !asset.id.is_empty() && asset.exists {
        metadata.push_str("Resource");
        if let Some(extension) = asset.path.extension().and_then(|value| value.to_str()) {
            metadata.push_str(&format!(" · {}", extension.to_ascii_uppercase()));
        }
    }
    if let Some(info) = media {
        if !metadata.is_empty() {
            metadata.push_str(" · ");
        }
        metadata.push_str(&asset_bytes(info.bytes));
        if audio {
            if let Some(duration) = info.duration {
                metadata.push_str(&format!(
                    " · {}:{:02}",
                    duration as u64 / 60,
                    duration as u64 % 60
                ));
            } else {
                metadata.push_str(" · —");
            }
        }
    }
    div()
        .size_full()
        .min_w_0()
        .flex()
        .gap_1()
        .when(grid, |this| this.flex_col())
        .when(!grid, |this| this.items_center())
        .child(thumbnail)
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .when(grid, |this| this.flex_none().w_full())
                .when(!grid, |this| this.flex_1())
                .child(
                    div()
                        .w_full()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px(12.))
                        .when(!grid, |this| this.line_height(px(12.)))
                        .text_color(rgb(INK))
                        .child(name),
                )
                .child(
                    div()
                        .w_full()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px(10.))
                        .when(!grid, |this| this.line_height(px(10.)))
                        .text_color(rgb(if asset.exists { MUTED } else { 0xe68e98 }))
                        .child(detail),
                )
                .when(!metadata.is_empty(), |this| {
                    this.child(
                        div()
                            .w_full()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(10.))
                            .when(!grid, |this| this.line_height(px(10.)))
                            .text_color(rgb(MUTED))
                            .child(metadata),
                    )
                }),
        )
        .when(audio, |this| {
            this.child(audition_control(root, &asset.path, true, cx))
        })
        .into_any_element()
}

#[derive(Default)]
pub(in crate::app) struct Cache {
    index: std::sync::Weak<AuthoringIndex>,
    key: Option<(AssetQuery, bool, usize, bool, Option<bool>)>,
    snapshot: Option<Rc<Snapshot>>,
}

struct Snapshot {
    assets: Vec<AssetEntry>,
    ordered: Arc<Vec<AssetKey>>,
    folders: Vec<PathBuf>,
    rows: Vec<BrowseRow>,
}

impl Cache {
    fn resolve(
        &mut self,
        index: &Arc<AuthoringIndex>,
        query: &AssetQuery,
        unmapped: bool,
        columns: usize,
        large: bool,
        grid: Option<bool>,
    ) -> Rc<Snapshot> {
        let key = (query.clone(), unmapped, columns, large, grid);
        // Age-based filters expire with wall time; evaluate those on each render.
        if query.modified.is_none()
            && self
                .index
                .upgrade()
                .is_some_and(|previous| Arc::ptr_eq(&previous, index))
            && self.key.as_ref() == Some(&key)
            && let Some(snapshot) = &self.snapshot
        {
            return snapshot.clone();
        }
        let assets = if unmapped {
            query
                .unmapped_results_with_media(&index.unmapped, &index.media)
                .into_iter()
                .map(|asset| AssetEntry {
                    kind: asset.kind,
                    id: String::new(),
                    path: asset.path.clone(),
                    tags: Vec::new(),
                    exists: true,
                    reference_count: 0,
                })
                .collect::<Vec<_>>()
        } else {
            query
                .results_with_media(&index.assets, &index.media)
                .into_iter()
                .cloned()
                .collect()
        };
        let ordered = Arc::new(assets.iter().map(AssetEntry::key).collect());
        let folders = index
            .assets
            .iter()
            .map(|asset| &asset.path)
            .chain(index.unmapped.iter().map(|asset| &asset.path))
            .filter_map(|path| path.parent().map(Path::to_path_buf))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let rows = browse_rows(
            assets
                .iter()
                .map(|asset| grid.unwrap_or_else(|| image_kind(asset.kind))),
            columns,
            large,
        );
        let snapshot = Rc::new(Snapshot {
            assets,
            ordered,
            folders,
            rows,
        });
        self.index = Arc::downgrade(index);
        self.key = Some(key);
        self.snapshot = Some(snapshot.clone());
        snapshot
    }
}

#[derive(Debug)]
struct BrowseRow {
    items: std::ops::Range<usize>,
    grid: bool,
    top: f32,
    height: f32,
}

// Preserve query order: changing presentation must not change Shift selection or drag order.
fn browse_rows(
    modes: impl IntoIterator<Item = bool>,
    columns: usize,
    large: bool,
) -> Vec<BrowseRow> {
    let mut rows: Vec<BrowseRow> = Vec::new();
    for (index, grid) in modes.into_iter().enumerate() {
        if let Some(row) = rows.last_mut()
            && grid
            && row.grid
            && row.items.len() < columns.max(1)
        {
            row.items.end += 1;
            continue;
        }
        let top = rows.last().map_or(0., |row| row.top + row.height);
        rows.push(BrowseRow {
            items: index..index + 1,
            grid,
            top,
            height: if grid {
                if large { 188. } else { 124. }
            } else {
                48.
            },
        });
    }
    rows
}

fn visible_rows(
    rows: &[BrowseRow],
    scroll: f32,
    height: f32,
) -> (std::ops::Range<usize>, f32, f32) {
    let total = rows.last().map_or(0., |row| row.top + row.height);
    let start = rows.partition_point(|row| row.top + row.height < (scroll - 200.).max(0.));
    let end = rows
        .partition_point(|row| row.top <= scroll + height + 200.)
        .max(start);
    let before = rows.get(start).map_or(total, |row| row.top);
    let bottom = rows.get(end).map_or(total, |row| row.top);
    (start..end, before, total - bottom)
}

fn asset_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_browser_invalidates_on_source_query_and_layout_changes() {
        let mut index = Arc::new(AuthoringIndex::default());
        Arc::get_mut(&mut index).unwrap().assets = vec![AssetEntry {
            kind: AssetKind::Background,
            id: "room".into(),
            path: "assets/room.webp".into(),
            tags: vec![],
            exists: true,
            reference_count: 0,
        }];
        let mut cache = Cache::default();
        let query = AssetQuery::default();
        let first = cache.resolve(&index, &query, false, 2, false, None);
        assert!(Rc::ptr_eq(
            &first,
            &cache.resolve(&index, &query, false, 2, false, None)
        ));
        let list = cache.resolve(&index, &query, false, 2, false, Some(false));
        assert!(!list.rows[0].grid);
        let missing = AssetQuery {
            status: crate::authoring::AssetStatus::Missing,
            ..Default::default()
        };
        assert!(
            cache
                .resolve(&index, &missing, false, 2, false, None)
                .assets
                .is_empty()
        );
        let filtered = AssetQuery {
            search: "ROOM".into(),
            ..Default::default()
        };
        assert_eq!(
            cache
                .resolve(&index, &filtered, false, 2, false, None)
                .ordered[0]
                .id,
            "room"
        );
        let mut replacement = (*index).clone();
        replacement.assets[0].exists = false;
        let replacement = Arc::new(replacement);
        assert_eq!(
            cache
                .resolve(&replacement, &missing, false, 2, false, None)
                .assets
                .len(),
            1
        );
        let large = cache.resolve(&replacement, &query, false, 1, true, None);
        assert_eq!(large.rows[0].height, 188.);
        let timed = AssetQuery {
            modified: Some(std::time::Duration::from_secs(60)),
            ..Default::default()
        };
        let before = cache.resolve(&replacement, &timed, false, 1, true, None);
        let after = cache.resolve(&replacement, &timed, false, 1, true, None);
        assert!(!Rc::ptr_eq(&before, &after));
        let mut unmapped = (*replacement).clone();
        unmapped.unmapped.push(UnmappedAsset {
            kind: AssetKind::Figure,
            path: "assets/hero.webp.unmapped".into(),
        });
        let unmapped = cache.resolve(&Arc::new(unmapped), &query, true, 1, false, None);
        assert_eq!(
            unmapped.assets[0].path,
            Path::new("assets/hero.webp.unmapped")
        );
    }

    #[test]
    fn mixed_rows_preserve_asset_order_and_responsive_grid() {
        let modes = [true, true, true, false, false, true, true];
        for columns in [0, 1, 2, 4] {
            let rows = browse_rows(modes, columns, false);
            assert_eq!(
                rows.iter()
                    .flat_map(|row| row.items.clone())
                    .collect::<Vec<_>>(),
                (0..7).collect::<Vec<_>>()
            );
            assert!(
                rows.iter()
                    .all(|row| row.items.len() <= if row.grid { columns.max(1) } else { 1 })
            );
            assert!(
                rows.windows(2)
                    .all(|pair| pair[1].top == pair[0].top + pair[0].height)
            );
        }
    }

    #[test]
    fn bounded_rows_keep_scroll_extent_at_top_middle_and_end() {
        let rows = browse_rows(std::iter::repeat_n(true, 1000), 3, false);
        let total = rows.last().unwrap().top + rows.last().unwrap().height;
        for scroll in [0., 10000., total - 600.] {
            let (visible, before, after) = visible_rows(&rows, scroll, 600.);
            assert!(visible.len() <= 10);
            let displayed = rows[visible].iter().map(|row| row.height).sum::<f32>();
            assert_eq!(before + displayed + after, total);
        }
        let (visible, before, after) = visible_rows(&[], 0., 600.);
        assert!(visible.is_empty());
        assert_eq!(before + after, 0.);
    }
}

#[cfg(test)]
#[path = "../../../../../tests/bench/editor/filter.rs"]
mod benchmark;
