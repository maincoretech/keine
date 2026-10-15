//! Explicit source jumps and a bounded, index-backed quick picker.
use super::*;
use gpui_kit::component::dock::PaneRef;

const HISTORY_LIMIT: usize = 64;
const RESULT_LIMIT: usize = 80;

#[derive(Clone, Debug)]
struct DocumentStamp {
    document: std::rc::Weak<RefCell<crate::document::SourceDocument>>,
    revision: u64,
}

impl DocumentStamp {
    fn capture(document: &DocumentHandle) -> Self {
        Self {
            document: Rc::downgrade(document),
            revision: document.borrow().revision(),
        }
    }
}

impl PartialEq for DocumentStamp {
    fn eq(&self, other: &Self) -> bool {
        self.document.ptr_eq(&other.document) && self.revision == other.revision
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Location {
    path: PathBuf,
    selection: Range<usize>,
    cursor: Position,
    text_scroll: Point<Pixels>,
    block_scroll: Point<Pixels>,
    blocks: Vec<usize>,
    mode: DocumentMode,
    revision: Option<DocumentStamp>,
}

#[derive(Default)]
pub(super) struct NavigationHistory {
    back: Vec<Location>,
    forward: Vec<Location>,
}

impl NavigationHistory {
    fn depart(&mut self, location: Location) {
        if self.back.last() != Some(&location) {
            Self::push(&mut self.back, location);
        }
        self.forward.clear();
    }

    fn push(stack: &mut Vec<Location>, location: Location) {
        if stack.len() == HISTORY_LIMIT {
            stack.remove(0);
        }
        stack.push(location);
    }

    fn travel(&mut self, back: bool, current: Option<Location>) -> Option<Location> {
        let (from, to) = if back {
            (&mut self.back, &mut self.forward)
        } else {
            (&mut self.forward, &mut self.back)
        };
        let target = from.pop()?;
        if let Some(current) = current {
            Self::push(to, current);
        }
        Some(target)
    }
}

/// Read dock identities without borrowing an already leased panel entity.
pub(super) fn active_path(root: &Path, cx: &App) -> Option<PathBuf> {
    let documents = cx.global::<EditorDocuments>();
    let (dock, node) = documents.document_dock(root)?;
    let dock = dock.upgrade()?;
    let dock = dock.read(cx);
    let active = [
        DockPlacement::Center,
        DockPlacement::Left,
        DockPlacement::Right,
        DockPlacement::Bottom,
    ]
    .into_iter()
    .find_map(|placement| {
        let PaneRef::Tabs { panels, active_ix } = dock.layout(placement)?.find_node(node?)?.kind()
        else {
            return None;
        };
        panels.get(active_ix).copied()
    })?;
    documents.path_for_panel(root, active)
}

fn capture(root: &Path, cx: &App) -> Option<Location> {
    let path = active_path(root, cx)?;
    let panel = cx
        .global::<EditorDocuments>()
        .panel_entity_for(root, &path)?
        .upgrade()?;
    let panel = panel.read(cx);
    let PanelContent::Document {
        document, editor, ..
    } = &panel.content
    else {
        return None;
    };
    let editor = editor.read(cx);
    let mut blocks: Vec<_> = panel.document.selected_blocks.iter().copied().collect();
    blocks.sort_unstable();
    let cursor = if panel.document.document_mode == DocumentMode::Block {
        cx.global::<EditorDocuments>()
            .selection(root)
            .filter(|(selected, _, _)| *selected == path)
            .map(|(_, line, column)| Position::new(*line as u32, *column as u32))
            .unwrap_or_else(|| editor.cursor_position())
    } else {
        editor.cursor_position()
    };
    Some(Location {
        path,
        selection: editor.selected_range(),
        cursor,
        text_scroll: editor.scroll_offset(),
        block_scroll: panel.view_scroll.offset(),
        blocks,
        mode: panel.document.document_mode,
        revision: document.as_ref().map(DocumentStamp::capture),
    })
}

pub(super) fn record_departure(root: &Path, cx: &mut App) {
    if let Some(location) = capture(root, cx)
        && let Ok(workspace) = cx.global_mut::<EditorDocuments>().workspace_mut(root)
    {
        workspace.navigation.depart(location);
    }
}

pub(super) fn jump(
    root: &Path,
    path: &Path,
    line: usize,
    column: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let (root, path) = (root.to_owned(), path.to_owned());
    // A jump can originate in the current document's Inspector/Block callback.
    // Capture after releasing that entity; never borrow it twice.
    window.defer(cx, move |window, cx| {
        record_departure(&root, cx);
        super::edits::navigate_inspection_untracked(&root, &path, line, column, window, cx);
        focus_document(&root, &path, window, cx);
    });
}

fn focus_document(root: &Path, path: &Path, window: &mut Window, cx: &mut App) {
    if let Some(panel) = cx.global::<EditorDocuments>().panel_entity_for(root, path) {
        let _ = panel.update(cx, |panel, cx| {
            if panel.document.document_mode == DocumentMode::Text {
                if let PanelContent::Document { editor, .. } = &panel.content {
                    editor.update(cx, |editor, cx| editor.focus(window, cx));
                }
            } else {
                panel.focus.focus(window, cx);
            }
        });
    }
}

pub(super) fn can_travel(root: &Path, cx: &App) -> (bool, bool) {
    cx.global::<EditorDocuments>()
        .workspaces
        .get(root)
        .map(|workspace| {
            (
                !workspace.navigation.back.is_empty(),
                !workspace.navigation.forward.is_empty(),
            )
        })
        .unwrap_or_default()
}

pub(super) fn travel(root: &Path, back: bool, window: &mut Window, cx: &mut App) {
    let root = root.to_owned();
    window.defer(cx, move |window, cx| {
        let current = capture(&root, cx);
        let target = cx
            .global::<EditorDocuments>()
            .workspaces
            .get(&root)
            .and_then(|workspace| {
                let history = &workspace.navigation;
                if back {
                    history.back.last()
                } else {
                    history.forward.last()
                }
                .cloned()
            });
        let Some(target) = target else {
            return;
        };
        // Validate before changing either history stack, including when tabs were closed.
        if let Err(error) = cx.global_mut::<EditorDocuments>().open(&root, &target.path) {
            window.push_notification(
                Notification::warning(format!("Could not return to location: {error}")),
                cx,
            );
            return;
        }
        if let Ok(workspace) = cx.global_mut::<EditorDocuments>().workspace_mut(&root) {
            workspace.navigation.travel(back, current);
        }
        super::edits::navigate_inspection_untracked(
            &root,
            &target.path,
            target.cursor.line as usize + 1,
            target.cursor.character as usize + 1,
            window,
            cx,
        );
        if let Some(panel) = cx
            .global::<EditorDocuments>()
            .panel_entity_for(&root, &target.path)
        {
            let _ = panel.update(cx, |panel, cx| {
                let PanelContent::Document {
                    document, editor, ..
                } = &panel.content
                else {
                    return;
                };
                let unchanged = document.as_ref().map(DocumentStamp::capture) == target.revision;
                panel.document.document_mode = target.mode;
                panel.document.text_scroll_pending = false;
                if unchanged {
                    panel.document.block_scroll_pending = false;
                    panel.document.selected_blocks = target.blocks.iter().copied().collect();
                    panel.document.block_selection_anchor = target.blocks.first().copied();
                }
                editor.update(cx, |editor, cx| {
                    if unchanged {
                        editor.set_selected_range(target.selection.clone(), cx);
                    }
                    let position = editor.cursor_position();
                    panel.document.last_text_cursor =
                        Some((position.line as usize, position.character as usize));
                    if unchanged {
                        editor.set_scroll_offset(target.text_scroll, cx);
                    }
                });
                if target.mode == DocumentMode::Block && unchanged {
                    cx.global_mut::<EditorDocuments>().set_block_selection(
                        &root,
                        target.path.clone(),
                        target.blocks.clone(),
                    );
                } else if target.mode == DocumentMode::Text {
                    cx.global_mut::<EditorDocuments>()
                        .clear_block_selection(&root);
                }
                if unchanged {
                    panel.view_scroll.set_offset(target.block_scroll);
                }
                cx.notify();
            });
        }
        focus_document(&root, &target.path, window, cx);
        cx.refresh_windows();
    });
}

#[derive(Clone)]
struct Candidate {
    path: PathBuf,
    title: SharedString,
    scene: Option<String>,
    searchable: String,
}

fn candidates(files: &[WorkspaceFile], index: &AuthoringIndex) -> Vec<Candidate> {
    files
        .iter()
        .filter(|file| {
            !file.is_dir()
                && file.size <= crate::workspace::MAX_DOCUMENT_BYTES
                && crate::workspace::is_text_document(&file.relative_path)
        })
        .map(|file| Candidate {
            path: file.relative_path.clone(),
            title: file
                .relative_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
                .into(),
            scene: None,
            searchable: file.relative_path.to_string_lossy().to_lowercase(),
        })
        .chain(index.scenes.iter().map(|scene| Candidate {
            path: scene.path.clone(),
            title: scene.name.clone().into(),
            scene: Some(scene.name.clone()),
            searchable: format!("{} {}", scene.name, scene.path.display()).to_lowercase(),
        }))
        .collect()
}

fn matching(candidates: &[Candidate], query: &str) -> (Vec<usize>, usize) {
    let query = query.trim().to_lowercase();
    let scenes_only = query.starts_with('@');
    let query = query.trim_start_matches('@').trim();
    let words: Vec<_> = query.split_whitespace().collect();
    let mut matches = std::collections::BinaryHeap::new();
    let mut count = 0;
    for (index, candidate) in candidates.iter().enumerate() {
        if scenes_only && candidate.scene.is_none() {
            continue;
        }
        if !words.iter().all(|word| candidate.searchable.contains(word)) {
            continue;
        }
        count += 1;
        let title = candidate.title.to_lowercase();
        let rank = if title == query {
            0
        } else if title.starts_with(query) {
            1
        } else {
            2
        };
        matches.push((rank, index));
        if matches.len() > RESULT_LIMIT {
            matches.pop();
        }
    }
    let mut matches = matches.into_vec();
    matches.sort_unstable();
    (matches.into_iter().map(|(_, index)| index).collect(), count)
}

struct QuickPicker {
    root: PathBuf,
    input: Entity<InputState>,
    candidates: Arc<Vec<Candidate>>,
    matches: Vec<usize>,
    count: usize,
    selected: usize,
    loading: bool,
    epoch: u64,
    task: Option<gpui_kit::Task<()>>,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

pub(super) fn open_quick(root: &Path, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    let root = root.to_owned();
    let picker = cx.new(|cx| {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Files and scenes · @ for scenes"));
        let subscription = cx.subscribe(&input, |this: &mut QuickPicker, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.search(cx);
            }
        });
        let documents = cx.global::<EditorDocuments>();
        let entries = documents
            .workspaces
            .get(&root)
            .map(|workspace| candidates(&workspace.files, &workspace.authoring))
            .unwrap_or_default();
        let (matches, count) = matching(&entries, "");
        QuickPicker {
            root,
            input,
            candidates: Arc::new(entries),
            matches,
            count,
            selected: 0,
            loading: false,
            epoch: 0,
            task: None,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        }
    });
    let input = picker.read(cx).input.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title("Go to file or scene")
            .width(px(560.))
            .child(picker.clone())
    });
    input.read(cx).focus_handle(cx).focus(window, cx);
}

impl QuickPicker {
    fn search(&mut self, cx: &mut Context<Self>) {
        self.epoch = self.epoch.wrapping_add(1);
        let epoch = self.epoch;
        self.task.take();
        self.loading = true;
        self.selected = 0;
        self.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
        let candidates = self.candidates.clone();
        let query = self.input.read(cx).value().to_string();
        let background = cx.background_executor().clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let (matches, count) = background
                .spawn(async move { matching(&candidates, &query) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.epoch == epoch {
                    this.matches = matches;
                    this.count = count;
                    this.loading = false;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    fn select(&mut self, next: bool, cx: &mut Context<Self>) {
        if self.matches.is_empty() || self.loading {
            return;
        }
        self.selected = if next {
            (self.selected + 1).min(self.matches.len() - 1)
        } else {
            self.selected.saturating_sub(1)
        };
        let top = self.selected as f32 * 42.;
        let offset = -f32::from(self.scroll.offset().y);
        let height = f32::from(self.scroll.bounds().size.height).max(42.);
        if top < offset {
            self.scroll.set_offset(gpui_kit::point(px(0.), px(-top)));
        } else if top + 42. > offset + height {
            self.scroll
                .set_offset(gpui_kit::point(px(0.), px(height - top - 42.)));
        }
        cx.notify();
    }

    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::EntityInputHandler;
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        if self.loading {
            return;
        }
        let Some(candidate) = self
            .matches
            .get(self.selected)
            .map(|&index| self.candidates[index].clone())
        else {
            return;
        };
        let source = cx
            .global::<EditorDocuments>()
            .source(&self.root, &candidate.path);
        let Some(source) = source else {
            window.push_notification(
                Notification::warning("File unavailable; reopen the picker to refresh"),
                cx,
            );
            return;
        };
        let line = if let Some(name) = &candidate.scene {
            let projection =
                cx.global::<EditorDocuments>()
                    .projection(&self.root, &candidate.path, &source);
            let Some(scene) = projection.scenes.iter().find(|scene| scene.name == *name) else {
                window.push_notification(
                    Notification::warning("Scene changed; reopen the picker to refresh"),
                    cx,
                );
                return;
            };
            source[..scene.source_range.start]
                .bytes()
                .filter(|&byte| byte == b'\n')
                .count()
                + 1
        } else {
            1
        };
        window.close_dialog(cx);
        jump(&self.root, &candidate.path, line, 1, window, cx);
    }
}

impl Render for QuickPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("KeineQuickOpen")
            .flex()
            .flex_col()
            .gap_2()
            .on_action(cx.listener(|this, _: &QuickOpenNext, _, cx| this.select(true, cx)))
            .on_action(cx.listener(|this, _: &QuickOpenPrevious, _, cx| this.select(false, cx)))
            .on_action(cx.listener(|this, _: &QuickOpenAccept, window, cx| this.accept(window, cx)))
            .on_action(cx.listener(|_, _: &QuickOpenClose, window, cx| window.close_dialog(cx)))
            .child(Input::new(&self.input))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(if self.loading {
                        "Searching…".to_owned()
                    } else if self.count == 0 {
                        "No matching files or scenes".to_owned()
                    } else if self.count > RESULT_LIMIT {
                        format!(
                            "First {RESULT_LIMIT} of {} results · type to narrow",
                            self.count
                        )
                    } else {
                        format!(
                            "{} results · ↑↓ select · Enter open · Esc close",
                            self.count
                        )
                    }),
            )
            .child(
                div()
                    .id("quick-results")
                    .max_h(px(336.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(self.matches.iter().enumerate().map(|(row, &index)| {
                        let candidate = &self.candidates[index];
                        div()
                            .id(("quick-result", row))
                            .h(px(42.))
                            .flex_none()
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(5.))
                            .bg(rgb(if row == self.selected {
                                SURFACE_HOVER
                            } else {
                                SURFACE
                            }))
                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                            .cursor_pointer()
                            .child(
                                Icon::new(if candidate.scene.is_some() {
                                    AssetIconName::Film
                                } else {
                                    AssetIconName::FileText
                                })
                                .small(),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_ellipsis()
                                            .child(candidate.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .text_ellipsis()
                                            .child(candidate.path.display().to_string()),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.selected = row;
                                this.accept(window, cx);
                            }))
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(line: u32) -> Location {
        Location {
            path: "scripts/a.shou".into(),
            selection: 2..8,
            cursor: Position::new(line, 0),
            text_scroll: gpui_kit::point(px(0.), px(-40.)),
            block_scroll: gpui_kit::point(px(0.), px(-80.)),
            blocks: vec![4],
            mode: DocumentMode::Text,
            revision: None,
        }
    }

    #[test]
    fn navigation_keeps_selection_and_scroll_and_bounds_branches() {
        let mut history = NavigationHistory::default();
        history.depart(location(0));
        assert_eq!(history.travel(true, Some(location(10))), Some(location(0)));
        assert_eq!(history.travel(false, Some(location(0))), Some(location(10)));
        history.travel(true, Some(location(10)));
        history.depart(location(3));
        assert!(history.travel(false, Some(location(4))).is_none());
        for line in 0..100 {
            history.depart(location(line));
        }
        assert_eq!(history.back.len(), HISTORY_LIMIT);
        history.depart(location(99));
        assert_eq!(history.back.len(), HISTORY_LIMIT);
    }

    #[test]
    fn quick_results_match_unicode_scene_filter_and_keep_a_bound() {
        let candidates = (0..120)
            .map(|i| Candidate {
                path: format!("scripts/{i}.shou").into(),
                title: "海边".into(),
                scene: (i % 2 == 0).then(|| "海边".into()),
                searchable: "海边 scripts".into(),
            })
            .collect::<Vec<_>>();
        assert_eq!(matching(&candidates, "海边").0.len(), RESULT_LIMIT);
        assert_eq!(matching(&candidates, "@海边").1, 60);
        assert_eq!(matching(&candidates, "scripts 海边").1, 120);
        assert_eq!(matching(&candidates, "missing").1, 0);
    }
}
