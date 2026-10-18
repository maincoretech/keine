//! Bounded project search and explicit, undoable replacement. Open documents override disk, just as they do for authoring.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

const DEBOUNCE: Duration = Duration::from_millis(120);
const RESULT_LIMIT: usize = 2000;
const ROW_HEIGHT: f32 = 28.;
const SOURCE_BUDGET: usize = 8 * 1024 * 1024;

pub(super) struct ProjectSearch {
    input: Entity<InputState>,
    case_sensitive: bool,
    replacement: Entity<InputState>,
    replacing: bool,
    scope: Option<PathBuf>,
    excluded: HashSet<usize>,
    message: Option<String>,
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
    sources: BTreeMap<PathBuf, String>,
    truncated: bool,
    skipped: usize,
    readonly: usize,
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
    pub(super) fn refresh_search_language(&self, window: &mut Window, cx: &mut App) {
        if let Some(search) = &self.project_search {
            crate::i18n::refresh_input(&search.input, window, cx);
            crate::i18n::refresh_input(&search.replacement, window, cx);
        }
    }

    pub(super) fn install_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder(crate::i18n::tr("Search project")));
        self._subscriptions
            .push(cx.subscribe(&input, |panel, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    panel.schedule_search(cx);
                }
            }));
        let replacement =
            cx.new(|cx| InputState::new(window, cx).placeholder(crate::i18n::tr("Replace with")));
        self._subscriptions
            .push(cx.subscribe(&replacement, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }));
        self.focus = input.read(cx).focus_handle(cx).clone();
        self.project_search = Some(ProjectSearch {
            input,
            case_sensitive: false,
            replacement,
            replacing: false,
            scope: None,
            excluded: HashSet::new(),
            message: None,
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
        search.excluded.clear();
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
        let replacing = search.replacing;
        let scope = search.scope.clone();
        let cancelled = search.cancelled.clone();
        let background = cx.background_executor().clone();
        search.task = Some(cx.spawn(async move |this, cx| {
            background.timer(DEBOUNCE).await;
            let Some((files, overrides)) = cx.update(|cx| {
                let workspace = cx.global::<EditorDocuments>().workspaces.get(&root)?;
                Some((
                    workspace
                        .files
                        .iter()
                        .filter(|file| {
                            scope
                                .as_ref()
                                .is_none_or(|path| file.relative_path == *path)
                        })
                        .cloned()
                        .collect::<Vec<_>>(),
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
                        replacing,
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
        super::navigation::record_departure(&root, cx);
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
                    panel.document.last_text_cursor =
                        Some((position.line as usize, position.character as usize));
                    editor.unfold_at(position, cx);
                    editor.set_cursor_position(position, window, cx);
                    editor.set_selected_range(hit.range.clone(), cx);
                    if panel.document.document_mode == DocumentMode::Text {
                        editor.focus(window, cx);
                    }
                });
                if panel.document.document_mode == DocumentMode::Block
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
                        panel.document.block_scroll_pending = true;
                        panel.focus.focus(window, cx);
                    }
                }
                cx.notify();
            });
        }
        cx.notify();
    }

    fn replace_search_hits(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let PanelContent::Search { root } = &self.content else {
            return;
        };
        let root = root.clone();
        let Some(search) = self
            .project_search
            .as_ref()
            .filter(|search| search.replacing && !search.loading)
        else {
            return;
        };
        if cx
            .global::<EditorDocuments>()
            .workspaces
            .get(&root)
            .is_none_or(|w| w.file_operation_active || w.index_epoch != search.revision)
        {
            self.search_message(
                "Project changed or a file operation is running; refresh the search",
                cx,
            );
            return;
        }
        let replacement = search.replacement.read(cx).value().to_string();
        let edits = prepare_replacements(&search.results, &search.excluded, &replacement, |path| {
            cx.global::<EditorDocuments>().source(&root, path)
        });
        let result = edits.and_then(|edits| {
            let policy = crate::document::AuthoringPolicy::load(&root);
            if let Some((path, _)) = edits.iter().find(|(path, _)| !policy.is_writable(path)) {
                return Err(crate::i18n::format(
                    "{0} is read-only",
                    &[("0", (path.display()).to_string())],
                ));
            }
            let count = edits.len();
            let occurrences = search
                .results
                .hits
                .iter()
                .enumerate()
                .filter(|(index, hit)| {
                    !search.excluded.contains(index) && hit.matched != replacement
                })
                .count();
            super::edits::apply_checked_edits(&root, &edits, &search.results.sources, window, cx)?;
            Ok((count, occurrences))
        });
        match result {
            Ok((count, occurrences)) => {
                self.schedule_search(cx);
                self.search_message(
                    if count == 0 {
                        crate::i18n::tr("No changes to replace").to_owned()
                    } else {
                        crate::i18n::format("Replaced {occurrences} matches in {count} files · unsaved · Undo available", &[("occurrences", (occurrences).to_string()), ("count", (count).to_string())])
                    },
                    cx,
                );
            }
            Err(error) => self.search_message(error, cx),
        }
    }

    fn search_message(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        if let Some(search) = self.project_search.as_mut() {
            search.message = Some(message.into());
        }
        cx.notify();
    }

    fn set_search_scope(&mut self, current: bool, cx: &mut Context<Self>) {
        let PanelContent::Search { root } = &self.content else {
            return;
        };
        let path = current
            .then(|| super::navigation::active_path(root, cx))
            .flatten();
        if current && path.is_none() {
            self.search_message("Open a document first", cx);
            return;
        }
        if let Some(search) = self.project_search.as_mut() {
            search.scope = path;
            search.message = None;
        }
        self.schedule_search(cx);
    }

    fn undo_search(&mut self, undo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let PanelContent::Search { root } = &self.content else {
            return;
        };
        let root = root.clone();
        if cx
            .global::<EditorDocuments>()
            .workspaces
            .get(&root)
            .is_none_or(|w| w.file_operation_active)
        {
            return;
        }
        match super::edits::replay_source_history(&root, undo, window, cx) {
            Ok(_) => {
                self.schedule_search(cx);
                self.search_message(if undo { "Undone" } else { "Redone" }, cx);
            }
            Err(error) => self.search_message(error, cx),
        }
    }

    pub(super) fn render_search(
        &mut self,
        root: &Path,
        window: &mut Window,
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
        let replacing = search.replacing;
        let replacement_input = search.replacement.clone();
        let replacement = replacement_input.read(cx).value().to_string();
        let row_height = if replacing { 48. } else { ROW_HEIGHT };
        let included = search
            .results
            .hits
            .len()
            .saturating_sub(search.excluded.len());
        let current_scope = search.scope.is_some();
        let scope_hint = search
            .scope
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| crate::i18n::tr("All project text files").into());
        let status = if search.loading {
            crate::i18n::tr("Searching…").to_owned()
        } else if input.read(cx).value().is_empty() {
            crate::i18n::tr("Search dialogue, commands and project files").to_owned()
        } else if search.results.hits.is_empty() {
            crate::i18n::tr("No results").to_owned()
        } else {
            crate::i18n::format(
                "{0} results{1}",
                &[
                    ("0", (search.results.hits.len()).to_string()),
                    (
                        "1",
                        (if search.results.truncated {
                            " · partial; narrow search for more"
                        } else {
                            ""
                        })
                        .to_string(),
                    ),
                ],
            )
        };
        let mut rows = Vec::new();
        let mut reveals = HashMap::new();
        let mut progresses = Vec::new();
        let mut start = 0;
        while start < search.results.hits.len() {
            let path = &search.results.hits[start].path;
            let end = start
                + search.results.hits[start..]
                    .iter()
                    .take_while(|hit| hit.path == *path)
                    .count();
            let progress = disclosure_progress(
                format!("search-file-{}", path.display()),
                !search.collapsed.contains(path),
                window,
                cx,
            );
            reveals.insert(path.clone(), progress);
            rows.push(SearchRow::File(path.clone(), end - start));
            progresses.push(1.);
            if progress > 0. {
                rows.extend((start..end).map(SearchRow::Hit));
                progresses.extend(std::iter::repeat_n(progress, end - start));
            }
            start = end;
        }
        let offsets = row_offsets(rows.iter().zip(&progresses).map(|(row, progress)| {
            (if matches!(row, SearchRow::Hit(_)) {
                row_height
            } else {
                ROW_HEIGHT
            }) * progress
        }));
        let range = visible_row_range(
            &offsets,
            (-f32::from(self.view_scroll.offset().y)).max(0.),
            f32::from(self.view_scroll.bounds().size.height).max(672.) + 168.,
        );
        let first = range.start;
        let end = range.end;
        let contents = div()
            .flex()
            .flex_col()
            .w_full()
            .child(div().h(px(offsets[first])).flex_none())
            .children(
                rows[first..end]
                    .iter()
                    .enumerate()
                    .map(|(row, result)| match result {
                        SearchRow::File(path, count) => {
                            let path = path.clone();
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
                                .child(disclosure_chevron(reveals[&path]))
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
                            let mut after = hit.preview.to_string();
                            if replacing {
                                after.replace_range(hit.highlight.clone(), &replacement);
                            }
                            let checked = !search.excluded.contains(&index);
                            let hit_row = div()
                                .id(("search-hit", index))
                                .h(px(row_height))
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
                                .when(replacing, |this| {
                                    this.child(
                                        div()
                                            .id(("search-include", index))
                                            .size(px(20.))
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .tooltip(icon_hint("Include this replacement"))
                                            .child(
                                                Icon::new(if checked {
                                                    AssetIconName::Check
                                                } else {
                                                    AssetIconName::Square
                                                })
                                                .xsmall()
                                                .text_color(rgb(if checked {
                                                    PRIMARY
                                                } else {
                                                    MUTED
                                                })),
                                            )
                                            .on_click(cx.listener(move |panel, _, _, cx| {
                                                cx.stop_propagation();
                                                if let Some(search) = panel.project_search.as_mut()
                                                    && !search.excluded.remove(&index)
                                                {
                                                    search.excluded.insert(index);
                                                }
                                                cx.notify();
                                            })),
                                    )
                                })
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(rgb(MUTED))
                                        .child((hit.line + 1).to_string()),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .overflow_hidden()
                                        .child(
                                            div().whitespace_nowrap().overflow_hidden().child(text),
                                        )
                                        .when(replacing, |this| {
                                            this.child(
                                                div()
                                                    .whitespace_nowrap()
                                                    .overflow_hidden()
                                                    .text_color(rgb(if checked {
                                                        SUCCESS
                                                    } else {
                                                        MUTED
                                                    }))
                                                    .child(after),
                                            )
                                        }),
                                )
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    panel.open_search_hit(index, window, cx)
                                }));
                            div()
                                .h(px(row_height * progresses[first + row]))
                                .flex_none()
                                .overflow_hidden()
                                .opacity(progresses[first + row])
                                .child(hit_row)
                                .into_any_element()
                        }
                    }),
            )
            .child(
                div()
                    .h(px(offsets.last().unwrap() - offsets[end]))
                    .flex_none(),
            );
        div().size_full().flex().flex_col().min_h_0()
            .on_key_down(cx.listener(move |panel, event: &gpui_kit::KeyDownEvent, window, cx| {
                let Some(search) = panel.project_search.as_mut().filter(|search| !search.loading) else { return; };
                if search.replacement.read(cx).focus_handle(cx).is_focused(window) { return; }
                match event.keystroke.key.as_str() {
                    "down" | "up" if !search.results.hits.is_empty() => {
                        let eligible = search.results.hits.iter().enumerate().filter(|(_, hit)| !search.collapsed.contains(&hit.path)).map(|(index, _)| index).collect::<Vec<_>>();
                        let next = if event.keystroke.key == "down" { eligible.iter().find(|index| **index > search.selected) } else { eligible.iter().rev().find(|index| **index < search.selected) };
                        let Some(next) = next else { return; };
                        search.selected = *next;
                        // Keep the selected result in the visible window, including file headers.
                        let row = rows.iter().position(|row| matches!(row, SearchRow::Hit(index) if *index == search.selected)).unwrap_or(0);
                        let visible = f32::from(panel.view_scroll.bounds().size.height);
                        let current = -f32::from(panel.view_scroll.offset().y);
                        let top = offsets[row];
                        let scroll = if top < current { top } else if top + row_height > current + visible { top + row_height - visible } else { current };
                        panel.view_scroll.set_offset(gpui_kit::point(px(0.), px(-scroll.max(0.))));
                        cx.stop_propagation(); cx.notify();
                    }
                    "enter" => { let index = search.selected; if search.results.hits.get(index).is_some_and(|hit| !search.collapsed.contains(&hit.path)) { panel.open_search_hit(index, window, cx); } cx.stop_propagation(); }
                    _ => {}
                }
            }))
            .child(div().flex_none().p_2().flex().items_center().gap_1()
                .child(file_action_icon("search-replace-toggle", AssetIconName::Replace, "Show replacement preview")
                    .on_click(cx.listener(|panel, _, _, cx| {
                        if let Some(search) = panel.project_search.as_mut() { search.replacing = !search.replacing; search.message = None; }
                        panel.schedule_search(cx);
                    })))
                .child(Input::new(&input).appearance(false).bordered(false).w_full().text_size(px(12.)))
                .child(div().id("search-case").h(px(26.)).px_2().flex().items_center().rounded(px(6.)).cursor_pointer()
                    .bg(rgb(if case_sensitive { PRIMARY_DIM } else { SURFACE })).text_color(rgb(if case_sensitive { PRIMARY } else { MUTED }))
                    .tooltip(icon_hint("Match case")).child("Aa")
                    .on_click(cx.listener(|panel, _, _, cx| { if let Some(search) = panel.project_search.as_mut() { search.case_sensitive = !search.case_sensitive; } panel.schedule_search(cx); }))))
            .child(div().px_2().pb_2().flex_none().flex().gap_1()
                .child(div().id("search-project-scope").px_2().py_1().rounded(px(4.)).text_xs().cursor_pointer()
                    .bg(rgb(if !current_scope { PRIMARY_DIM } else { SURFACE })).child(crate::i18n::tr("Project"))
                    .on_click(cx.listener(|panel, _, _, cx| panel.set_search_scope(false, cx))))
                .child(div().id("search-file-scope").px_2().py_1().rounded(px(4.)).text_xs().cursor_pointer()
                    .bg(rgb(if current_scope { PRIMARY_DIM } else { SURFACE })).tooltip(icon_hint(scope_hint)).child(crate::i18n::tr("Current file"))
                    .on_click(cx.listener(|panel, _, _, cx| panel.set_search_scope(true, cx)))))
            .when(replacing, |this| this
                .child(div().px_2().pb_2().flex_none()
                    .child(Input::new(&replacement_input).appearance(false).bordered(false).w_full().text_size(px(12.))))
                .child(div().px_2().pb_2().flex_none().flex().flex_wrap().items_center().gap_1()
                    .child(div().id("search-replace-apply").px_2().py_1().rounded(px(4.)).text_xs()
                        .bg(rgb(SURFACE)).text_color(rgb(if included > 0 && !search.loading { PRIMARY } else { MUTED })).cursor_pointer()
                        .child(crate::i18n::format("Replace selected ({included})", &[("included", (included).to_string())]))
                        .on_click(cx.listener(|panel, _, window, cx| panel.replace_search_hits(window, cx))))
                    .child(div().id("search-select-all").px_2().py_1().text_xs().cursor_pointer().child(crate::i18n::tr(if included == 0 { "Select all" } else { "Deselect all" }))
                        .on_click(cx.listener(|panel, _, _, cx| {
                            if let Some(search) = panel.project_search.as_mut() {
                                if search.excluded.len() == search.results.hits.len() { search.excluded.clear(); }
                                else { search.excluded = (0..search.results.hits.len()).collect(); }
                            }
                            cx.notify();
                        })))
                    .child(file_action_icon("search-undo", AssetIconName::Undo, "Undo last project edit")
                        .on_click(cx.listener(|panel, _, window, cx| panel.undo_search(true, window, cx))))
                    .child(file_action_icon("search-redo", AssetIconName::Redo, "Redo project edit")
                        .on_click(cx.listener(|panel, _, window, cx| panel.undo_search(false, window, cx))))))
            .child(div().px_3().pb_2().flex_none().text_size(px(10.)).text_color(rgb(MUTED)).child(status))
            .when_some(search.message.clone(), |this, message| this.child(div().px_3().pb_2().text_xs().text_color(rgb(PRIMARY)).child(message)))
            .when(search.results.readonly > 0, |this| this.child(div().px_3().text_size(px(10.)).text_color(rgb(MUTED)).child(crate::i18n::format("{0} read-only files excluded from replacement", &[("0", (search.results.readonly).to_string())]))))
            .when(search.results.skipped > 0, |this| this.child(div().px_3().text_size(px(10.)).text_color(rgb(MUTED)).child(crate::i18n::format("{0} unreadable or oversized files skipped", &[("0", (search.results.skipped).to_string())]))))
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
    capture_sources: bool,
    cancelled: &AtomicBool,
) -> SearchResults {
    let mut results = SearchResults::default();
    let mut source_bytes = 0;
    let policy = capture_sources.then(|| crate::document::AuthoringPolicy::load(root));
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
        if policy
            .as_ref()
            .is_some_and(|policy| !policy.is_writable(&file.relative_path))
        {
            results.readonly += 1;
            continue;
        }
        if file.size > crate::workspace::MAX_DOCUMENT_BYTES {
            results.skipped += 1;
            continue;
        }
        let source = overrides
            .get(&file.relative_path)
            .cloned()
            .or_else(|| crate::workspace::read_project_text(root, &file.relative_path).ok());
        let Some(source) = source else {
            results.skipped += 1;
            continue;
        };
        let first_hit = results.hits.len();
        search_source(
            &file.relative_path,
            &source,
            &query,
            case_sensitive,
            &mut results,
            cancelled,
        );
        if capture_sources && results.hits.len() > first_hit {
            if source_bytes + source.len() > SOURCE_BUDGET {
                results.hits.truncate(first_hit);
                results.truncated = true;
            } else {
                source_bytes += source.len();
                results.sources.insert(file.relative_path.clone(), source);
            }
        }
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
            if results
                .hits
                .last()
                .is_some_and(|hit| hit.path == path && hit.range.end > offset + range.start)
            {
                continue;
            }
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

/// Revalidate whole documents, then replace backwards. No filesystem writes;
/// the caller applies one existing source transaction after every file passes.
fn prepare_replacements(
    results: &SearchResults,
    excluded: &HashSet<usize>,
    replacement: &str,
    mut current: impl FnMut(&Path) -> Option<String>,
) -> Result<Vec<(PathBuf, String)>, String> {
    let mut files: BTreeMap<&Path, Vec<&SearchHit>> = BTreeMap::new();
    for (_, hit) in results
        .hits
        .iter()
        .enumerate()
        .filter(|(i, _)| !excluded.contains(i))
    {
        files.entry(&hit.path).or_default().push(hit);
    }
    if files.len() > 128 {
        return Err(crate::i18n::tr("Select at most 128 files per replacement").into());
    }
    let mut edits = Vec::with_capacity(files.len());
    let mut bytes = 0usize;
    for (path, hits) in files {
        let before = results
            .sources
            .get(path)
            .ok_or("Refresh the replacement preview")?;
        if current(path).as_deref() != Some(before.as_str()) {
            return Err(crate::i18n::format(
                "{0} changed; refresh the search before replacing",
                &[("0", (path.display()).to_string())],
            ));
        }
        let mut size = before.len();
        let mut end = 0;
        for hit in &hits {
            if hit.range.start < end || before.get(hit.range.clone()) != Some(hit.matched.as_str())
            {
                return Err(
                    crate::i18n::tr("Search ranges changed; refresh before replacing").into(),
                );
            }
            end = hit.range.end;
            size = size
                .checked_sub(hit.range.len())
                .and_then(|s| s.checked_add(replacement.len()))
                .ok_or("Replacement is too large")?;
        }
        if size > crate::workspace::MAX_DOCUMENT_BYTES as usize {
            return Err(crate::i18n::format(
                "{0} would exceed the document size limit",
                &[("0", (path.display()).to_string())],
            ));
        }
        bytes = bytes.saturating_add(before.len()).saturating_add(size);
        if bytes > super::edits::SourceHistory::BYTE_BUDGET {
            return Err(
                crate::i18n::tr("Replacement exceeds the undo budget; select fewer files").into(),
            );
        }
        let mut after = before.clone();
        for hit in hits.into_iter().rev() {
            after.replace_range(hit.range.clone(), replacement);
        }
        if after != *before {
            edits.push((path.to_owned(), after));
        }
    }
    Ok(edits)
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
    #[test]
    fn replacement_is_selected_unicode_safe_and_rejects_stale_documents() {
        let originals = BTreeMap::from([
            (
                PathBuf::from("scripts/a.shou"),
                "scene a { \"前 İSTANBUL 后\", \"İSTANBUL\" }\r\n".to_owned(),
            ),
            (
                PathBuf::from("scripts/b.shou"),
                "scene b { \"İSTANBUL\" }\n".to_owned(),
            ),
        ]);
        let mut results = SearchResults::default();
        for (path, text) in &originals {
            search_source(
                path,
                text,
                "i\u{307}stanbul",
                false,
                &mut results,
                &AtomicBool::new(false),
            );
        }
        results.sources = originals.clone();
        assert_eq!(results.hits.len(), 3);
        for replacement in ["东京", "", "quoted \"text\""] {
            let edits = prepare_replacements(&results, &HashSet::from([1]), replacement, |path| {
                originals.get(path).cloned()
            })
            .unwrap();
            assert_eq!(edits.len(), 2);
            assert_eq!(
                edits[0].1,
                originals[&edits[0].0].replacen("İSTANBUL", replacement, 1)
            );
            assert_eq!(
                edits[1].1,
                originals[&edits[1].0].replace("İSTANBUL", replacement)
            );
        }
        // Even if the same word remains at the same offset, another edit makes
        // the old replacement plan invalid. No partial edits are returned.
        assert!(
            prepare_replacements(&results, &HashSet::new(), "new", |path| originals
                .get(path)
                .map(|text| format!("{text}// external edit")))
            .is_err()
        );
        assert!(
            prepare_replacements(
                &results,
                &HashSet::new(),
                &"x".repeat(crate::workspace::MAX_DOCUMENT_BYTES as usize),
                |path| originals.get(path).cloned()
            )
            .is_err()
        );
        assert!(
            prepare_replacements(&results, &(0..3).collect(), "new", |path| originals
                .get(path)
                .cloned())
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn replacement_search_honors_unsaved_sources_cancellation_and_limits() {
        let root = std::env::temp_dir().join(format!("keine-search-{}", std::process::id()));
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(root.join("scripts/a.shou"), "disk").unwrap();
        std::fs::write(
            root.join("config.yaml"),
            include_str!("../../../../tests/fixtures/native-smoke/config.yaml"),
        )
        .unwrap();
        std::fs::write(root.join("legacy.json"), "{\"old\": 1}").unwrap();
        let session = crate::workspace::WorkspaceSession::open(&root).unwrap();
        let overrides =
            BTreeMap::from([(PathBuf::from("scripts/a.shou"), "unsaved old old".into())]);
        let results = search_sources(
            &root,
            session.files(),
            &overrides,
            "old",
            true,
            true,
            &AtomicBool::new(false),
        );
        assert_eq!(results.hits.len(), 2);
        assert!(results.readonly >= 1);
        assert_eq!(
            results.sources[Path::new("scripts/a.shou")],
            "unsaved old old"
        );
        assert!(
            search_sources(
                &root,
                session.files(),
                &overrides,
                "old",
                true,
                true,
                &AtomicBool::new(true)
            )
            .hits
            .is_empty()
        );
        let mut results = SearchResults::default();
        search_source(
            Path::new("scripts/a.shou"),
            &"old ".repeat(RESULT_LIMIT + 1),
            "old",
            true,
            &mut results,
            &AtomicBool::new(false),
        );
        assert_eq!(results.hits.len(), RESULT_LIMIT);
        assert!(results.truncated);
        std::fs::remove_dir_all(root).unwrap();
    }

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
