//! Read-only project search. Open documents override disk, just as they do for authoring.
use super::*;
use std::io::Read as _;
use std::sync::atomic::{AtomicBool, Ordering};

const DEBOUNCE: Duration = Duration::from_millis(120);
const RESULT_LIMIT: usize = 2000;
const ROW_HEIGHT: f32 = 28.;

pub(super) struct ProjectSearch {
    input: Entity<InputState>,
    case_sensitive: bool,
    epoch: u64,
    revision: u64,
    task: Option<gpui_kit::Task<()>>,
    cancelled: Arc<AtomicBool>,
    loading: bool,
    results: SearchResults,
    collapsed: HashSet<PathBuf>,
    selected: usize,
}

impl Drop for ProjectSearch {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct SearchResults {
    hits: Vec<SearchHit>,
    truncated: bool,
    skipped: usize,
}

#[derive(Clone)]
struct SearchHit {
    path: PathBuf,
    line: usize,
    range: Range<usize>,
    preview: SharedString,
    highlight: Range<usize>,
    matched: String,
}

enum SearchRow {
    File(PathBuf, usize),
    Hit(usize),
}

impl WorkbenchPanel {
    pub(super) fn install_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search project"));
        self._subscriptions
            .push(cx.subscribe(&input, |panel, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    panel.schedule_search(cx);
                }
            }));
        self.focus = input.read(cx).focus_handle(cx).clone();
        self.project_search = Some(ProjectSearch {
            input,
            case_sensitive: false,
            epoch: 0,
            revision: 0,
            task: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            loading: false,
            results: SearchResults::default(),
            collapsed: HashSet::new(),
            selected: 0,
        });
    }

    fn schedule_search(&mut self, cx: &mut Context<Self>) {
        let PanelContent::Search { root } = &self.content else {
            return;
        };
        let root = root.clone();
        let Some(search) = self.project_search.as_mut() else {
            return;
        };
        search.cancelled.store(true, Ordering::Relaxed);
        search.task.take();
        search.cancelled = Arc::new(AtomicBool::new(false));
        search.epoch = search.epoch.wrapping_add(1);
        let epoch = search.epoch;
        let query = search.input.read(cx).value().to_string();
        search.results = SearchResults::default();
        search.selected = 0;
        self.view_scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
        let revision = cx
            .global::<EditorDocuments>()
            .workspaces
            .get(&root)
            .map_or(0, |workspace| workspace.index_epoch);
        search.revision = revision;
        search.loading = !query.is_empty();
        if query.is_empty() {
            cx.notify();
            return;
        }
        let case_sensitive = search.case_sensitive;
        let cancelled = search.cancelled.clone();
        let background = cx.background_executor().clone();
        search.task = Some(cx.spawn(async move |this, cx| {
            background.timer(DEBOUNCE).await;
            let Some((files, overrides)) = cx.update(|cx| {
                let workspace = cx.global::<EditorDocuments>().workspaces.get(&root)?;
                Some((
                    workspace.files.clone(),
                    workspace.manager.source_overrides(),
                ))
            }) else {
                return;
            };
            let results = background
                .spawn(async move {
                    search_sources(
                        &root,
                        &files,
                        &overrides,
                        &query,
                        case_sensitive,
                        &cancelled,
                    )
                })
                .await;
            let _ = this.update(cx, |panel, cx| {
                if let Some(search) = panel.project_search.as_mut()
                    && search.epoch == epoch
                {
                    search.results = results;
                    search.loading = false;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    fn open_search_hit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let PanelContent::Search { root } = &self.content else {
            return;
        };
        let root = root.clone();
        let Some(search) = self
            .project_search
            .as_mut()
            .filter(|search| !search.loading)
        else {
            return;
        };
        let Some(hit) = search.results.hits.get(index).cloned() else {
            return;
        };
        search.selected = index;
        let source = cx.global::<EditorDocuments>().source(&root, &hit.path);
        if source
            .as_ref()
            .and_then(|source| source.get(hit.range.clone()))
            != Some(hit.matched.as_str())
        {
            self.schedule_search(cx);
            return;
        }
        open_workspace_document(&root, &hit.path, window, cx);
        let panel = cx
            .global::<EditorDocuments>()
            .workspaces
            .get(&root)
            .and_then(|workspace| workspace.panel_entities.get(&hit.path))
            .cloned();
        if let Some(panel) = panel {
            let _ = panel.update(cx, |panel, cx| {
                let PanelContent::Document { editor, .. } = &panel.content else {
                    return;
                };
                let editor = editor.clone();
                editor.update(cx, |editor, cx| {
                    let position = editor.text().offset_to_position(hit.range.start);
                    editor.set_cursor_position(position, window, cx);
                    editor.set_selected_range(hit.range.clone(), cx);
                    if panel.document_mode == DocumentMode::Text {
                        editor.focus(window, cx);
                    }
                });
                if panel.document_mode == DocumentMode::Block
                    && let Some(source) = source.as_ref()
                {
                    let projection = cx
                        .global::<EditorDocuments>()
                        .projection(&root, &hit.path, source);
                    let column = source[..hit.range.start]
                        .rsplit('\n')
                        .next()
                        .unwrap_or("")
                        .chars()
                        .count();
                    if let Some((_, block)) =
                        block_at_position(&projection, source, hit.line, column)
                    {
                        panel.select_current_block(block.source_range.start, cx);
                        panel.block_scroll_pending = true;
                        panel.focus.focus(window, cx);
                    }
                }
                cx.notify();
            });
        }
        cx.notify();
    }

    pub(super) fn render_search(
        &mut self,
        root: &Path,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let revision = cx
            .global::<EditorDocuments>()
            .workspaces
            .get(root)
            .map_or(0, |workspace| workspace.index_epoch);
        if self
            .project_search
            .as_ref()
            .is_some_and(|search| search.revision != revision)
        {
            self.schedule_search(cx);
        }
        let Some(search) = self.project_search.as_ref() else {
            return Empty.into_any_element();
        };
        let input = search.input.clone();
        let case_sensitive = search.case_sensitive;
        let status = if search.loading {
            "Searching…".to_owned()
        } else if input.read(cx).value().is_empty() {
            "Search dialogue, commands and project files".to_owned()
        } else if search.results.hits.is_empty() {
            "No results".to_owned()
        } else {
            format!(
                "{}{} results",
                search.results.hits.len(),
                if search.results.truncated { "+" } else { "" }
            )
        };
        let mut rows = Vec::new();
        let mut start = 0;
        while start < search.results.hits.len() {
            let path = &search.results.hits[start].path;
            let end = start
                + search.results.hits[start..]
                    .iter()
                    .take_while(|hit| hit.path == *path)
                    .count();
            rows.push(SearchRow::File(path.clone(), end - start));
            if !search.collapsed.contains(path) {
                rows.extend((start..end).map(SearchRow::Hit));
            }
            start = end;
        }
        let first = ((-f32::from(self.view_scroll.offset().y) / ROW_HEIGHT).max(0.) as usize)
            .saturating_sub(3)
            .min(rows.len());
        let count = (f32::from(self.view_scroll.bounds().size.height) / ROW_HEIGHT)
            .ceil()
            .max(24.) as usize
            + 6;
        let end = (first + count).min(rows.len());
        let contents = div()
            .flex()
            .flex_col()
            .w_full()
            .child(div().h(px(first as f32 * ROW_HEIGHT)).flex_none())
            .children(
                rows[first..end]
                    .iter()
                    .enumerate()
                    .map(|(row, result)| match result {
                        SearchRow::File(path, count) => {
                            let path = path.clone();
                            let collapsed = search.collapsed.contains(&path);
                            div()
                                .id(("search-file", first + row))
                                .h(px(ROW_HEIGHT))
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap_1()
                                .px_2()
                                .text_size(px(11.))
                                .text_color(rgb(MUTED))
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(SURFACE)))
                                .child(
                                    Icon::new(if collapsed {
                                        IconName::ChevronRight
                                    } else {
                                        IconName::ChevronDown
                                    })
                                    .xsmall(),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .child(path.display().to_string()),
                                )
                                .child(count.to_string())
                                .on_click(cx.listener(move |panel, _, _, cx| {
                                    if let Some(search) = panel.project_search.as_mut()
                                        && !search.collapsed.remove(&path)
                                    {
                                        search.collapsed.insert(path.clone());
                                    }
                                    cx.notify();
                                }))
                                .into_any_element()
                        }
                        SearchRow::Hit(index) => {
                            let index = *index;
                            let hit = &search.results.hits[index];
                            let text = gpui_kit::StyledText::new(hit.preview.clone())
                                .with_highlights([(
                                    hit.highlight.clone(),
                                    gpui_kit::HighlightStyle {
                                        color: Some(rgb(PRIMARY).into()),
                                        background_color: Some(rgb(PRIMARY_DIM).into()),
                                        ..Default::default()
                                    },
                                )]);
                            div()
                                .id(("search-hit", index))
                                .h(px(ROW_HEIGHT))
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap_2()
                                .pl_5()
                                .pr_2()
                                .bg(rgb(if search.selected == index {
                                    SURFACE_HOVER
                                } else {
                                    PANEL
                                }))
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                .text_size(px(12.))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(rgb(MUTED))
                                        .child((hit.line + 1).to_string()),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .child(text),
                                )
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    panel.open_search_hit(index, window, cx)
                                }))
                                .into_any_element()
                        }
                    }),
            )
            .child(
                div()
                    .h(px((rows.len() - end) as f32 * ROW_HEIGHT))
                    .flex_none(),
            );
        div().size_full().flex().flex_col().min_h_0()
            .on_key_down(cx.listener(move |panel, event: &gpui_kit::KeyDownEvent, window, cx| {
                let Some(search) = panel.project_search.as_mut().filter(|search| !search.loading) else { return; };
                match event.keystroke.key.as_str() {
                    "down" | "up" if !search.results.hits.is_empty() => {
                        search.selected = if event.keystroke.key == "down" { (search.selected + 1).min(search.results.hits.len() - 1) } else { search.selected.saturating_sub(1) };
                        // Keep the selected result in the visible window, including file headers.
                        let row = rows.iter().position(|row| matches!(row, SearchRow::Hit(index) if *index == search.selected)).unwrap_or(0);
                        let visible = f32::from(panel.view_scroll.bounds().size.height);
                        let current = -f32::from(panel.view_scroll.offset().y);
                        let top = row as f32 * ROW_HEIGHT;
                        let scroll = if top < current { top } else if top + ROW_HEIGHT > current + visible { top + ROW_HEIGHT - visible } else { current };
                        panel.view_scroll.set_offset(gpui_kit::point(px(0.), px(-scroll.max(0.))));
                        cx.stop_propagation(); cx.notify();
                    }
                    "enter" => { let index = search.selected; panel.open_search_hit(index, window, cx); cx.stop_propagation(); }
                    _ => {}
                }
            }))
            .child(div().flex_none().p_2().flex().items_center().gap_1()
                .child(Input::new(&input).appearance(false).bordered(false).w_full().text_size(px(12.)))
                .child(div().id("search-case").h(px(26.)).px_2().flex().items_center().rounded(px(6.)).cursor_pointer()
                    .bg(rgb(if case_sensitive { PRIMARY_DIM } else { SURFACE })).text_color(rgb(if case_sensitive { PRIMARY } else { MUTED }))
                    .tooltip(icon_hint("Match case")).child("Aa")
                    .on_click(cx.listener(|panel, _, _, cx| { if let Some(search) = panel.project_search.as_mut() { search.case_sensitive = !search.case_sensitive; } panel.schedule_search(cx); }))))
            .child(div().px_3().pb_2().flex_none().text_size(px(10.)).text_color(rgb(MUTED)).child(status))
            .when(search.results.skipped > 0, |this| this.child(div().px_3().text_size(px(10.)).text_color(rgb(MUTED)).child(format!("{} unreadable or oversized files skipped", search.results.skipped))))
            .child(vertical_overflow_view("project-search-results", &self.view_scroll, contents))
            .into_any_element()
    }
}

fn search_sources(
    root: &Path,
    files: &[WorkspaceFile],
    overrides: &BTreeMap<PathBuf, String>,
    query: &str,
    case_sensitive: bool,
    cancelled: &AtomicBool,
) -> SearchResults {
    let mut results = SearchResults::default();
    let query = if case_sensitive {
        query.to_owned()
    } else {
        query.to_lowercase()
    };
    for file in files
        .iter()
        .filter(|file| !file.is_dir() && crate::workspace::is_text_document(&file.relative_path))
    {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        if file.size > crate::workspace::MAX_DOCUMENT_BYTES {
            results.skipped += 1;
            continue;
        }
        let source = overrides.get(&file.relative_path).cloned().or_else(|| {
            let path = confined_existing_file(root, &file.relative_path)?;
            let mut source = String::new();
            fs::File::open(path)
                .ok()?
                .take(crate::workspace::MAX_DOCUMENT_BYTES + 1)
                .read_to_string(&mut source)
                .ok()?;
            (source.len() as u64 <= crate::workspace::MAX_DOCUMENT_BYTES).then_some(source)
        });
        let Some(source) = source else {
            results.skipped += 1;
            continue;
        };
        search_source(
            &file.relative_path,
            &source,
            &query,
            case_sensitive,
            &mut results,
            cancelled,
        );
        if results.truncated {
            break;
        }
    }
    results
}

fn search_source(
    path: &Path,
    source: &str,
    query: &str,
    case_sensitive: bool,
    results: &mut SearchResults,
    cancelled: &AtomicBool,
) {
    if query.is_empty() {
        return;
    }
    let mut offset = 0;
    for (line, raw) in source.split_inclusive('\n').enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        let text = raw.trim_end_matches(['\r', '\n']);
        let (folded, map) = if case_sensitive {
            (text.to_owned(), Vec::new())
        } else {
            fold_with_offsets(text)
        };
        for (start, found) in folded.match_indices(query) {
            if results.hits.len() == RESULT_LIMIT {
                results.truncated = true;
                return;
            }
            let range = if case_sensitive {
                start..start + found.len()
            } else {
                map[start]
                    ..map[start + found.len() - 1]
                        + text[map[start + found.len() - 1]..]
                            .chars()
                            .next()
                            .map_or(0, char::len_utf8)
            };
            let before = text[..range.start]
                .char_indices()
                .rev()
                .nth(60)
                .map_or(0, |(index, _)| index);
            let after = text[range.end..]
                .char_indices()
                .nth(100)
                .map_or(text.len(), |(index, _)| range.end + index);
            let prefix = if before > 0 { "…" } else { "" };
            let preview = format!(
                "{prefix}{}{}",
                &text[before..after],
                if after < text.len() { "…" } else { "" }
            );
            results.hits.push(SearchHit {
                path: path.to_owned(),
                line,
                range: offset + range.start..offset + range.end,
                preview: preview.into(),
                highlight: prefix.len() + range.start - before..prefix.len() + range.end - before,
                matched: text[range].to_owned(),
            });
        }
        offset += raw.len();
    }
}

fn fold_with_offsets(source: &str) -> (String, Vec<usize>) {
    let mut text = String::new();
    let mut offsets = Vec::new();
    for (start, character) in source.char_indices() {
        for folded in character.to_lowercase() {
            offsets.extend(std::iter::repeat_n(start, folded.len_utf8()));
            text.push(folded);
        }
    }
    offsets.push(source.len());
    (text, offsets)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_matches_keep_source_and_preview_offsets() {
        let mut results = SearchResults::default();
        let source = "前 İSTANBUL 后\r\n相同 istanbul";
        search_source(
            Path::new("main.shou"),
            source,
            "i\u{307}stanbul",
            false,
            &mut results,
            &AtomicBool::new(false),
        );
        let hit = &results.hits[0];
        assert_eq!(&source[hit.range.clone()], "İSTANBUL");
        assert_eq!(&hit.preview[hit.highlight.clone()], "İSTANBUL");
        assert_eq!(hit.line, 0);
        let mut partial = SearchResults::default();
        search_source(
            Path::new("main.shou"),
            source,
            "i",
            false,
            &mut partial,
            &AtomicBool::new(false),
        );
        assert_eq!(&source[partial.hits[0].range.clone()], "İ");
        let mut results = SearchResults::default();
        search_source(
            Path::new("main.shou"),
            source,
            "istanbul",
            true,
            &mut results,
            &AtomicBool::new(false),
        );
        assert_eq!(results.hits.len(), 1);
        assert_eq!(results.hits[0].line, 1);
        assert_eq!(&source[results.hits[0].range.clone()], "istanbul");
    }
}
