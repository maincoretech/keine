//! Asset query controls, selection actions and native import entry point.
use super::*;
use crate::authoring::{AssetSize, AssetStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Filter {
    Kind,
    Folder,
    Tag,
    Status,
    Size,
    Modified,
    Unmapped,
}

impl state::AssetsState {
    pub(in crate::app) fn query(&self, cx: &App) -> AssetQuery {
        AssetQuery {
            search: self.asset_search.read(cx).value().to_string(),
            kind: self.asset_kind,
            folder: self.asset_folder.clone(),
            sort: self.asset_sort,
            tag: self.asset_tag.clone(),
            status: self.asset_status,
            size: self.asset_size,
            modified: self.asset_modified,
        }
    }

    fn filters(&self) -> Vec<(Filter, String)> {
        let mut filters = Vec::new();
        if let Some(kind) = self.asset_kind {
            filters.push((Filter::Kind, kind.label().into()));
        }
        if let Some(folder) = &self.asset_folder {
            filters.push((Filter::Folder, folder.display().to_string()));
        }
        if let Some(tag) = &self.asset_tag {
            filters.push((Filter::Tag, format!("#{tag}")));
        }
        if self.asset_status != AssetStatus::All {
            filters.push((Filter::Status, status_label(self.asset_status).into()));
        }
        if self.asset_size != AssetSize::All {
            filters.push((
                Filter::Size,
                match self.asset_size {
                    AssetSize::Small => "< 1 MB",
                    AssetSize::Medium => "1–10 MB",
                    AssetSize::Large => "≥ 10 MB",
                    AssetSize::All => unreachable!(),
                }
                .into(),
            ));
        }
        if let Some(age) = self.asset_modified {
            filters.push((
                Filter::Modified,
                format!("Last {} days", age.as_secs() / 86400),
            ));
        }
        if self.asset_unmapped {
            filters.push((Filter::Unmapped, "Unmapped".into()));
        }
        filters
    }

    pub(in crate::app) fn has_filters(&self, cx: &App) -> bool {
        !self.asset_search.read(cx).value().trim().is_empty() || !self.filters().is_empty()
    }

    fn clear(&mut self, filter: Filter) {
        match filter {
            Filter::Kind => self.asset_kind = None,
            Filter::Folder => self.asset_folder = None,
            Filter::Tag => self.asset_tag = None,
            Filter::Status => self.asset_status = AssetStatus::All,
            Filter::Size => self.asset_size = AssetSize::All,
            Filter::Modified => self.asset_modified = None,
            Filter::Unmapped => self.asset_unmapped = false,
        }
        self.asset_anchor = None;
    }
}

pub(super) fn status_label(status: AssetStatus) -> &'static str {
    match status {
        AssetStatus::All => "All",
        AssetStatus::Used => "Used",
        AssetStatus::Unused => "Unused",
        AssetStatus::Missing => "Missing",
        AssetStatus::Canonical => "Ready",
        AssetStatus::NeedsConversion => "Pending",
    }
}

impl WorkbenchPanel {
    pub(in crate::app) fn clear_asset_filters(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for filter in [
            Filter::Kind,
            Filter::Folder,
            Filter::Tag,
            Filter::Status,
            Filter::Size,
            Filter::Modified,
            Filter::Unmapped,
        ] {
            self.assets.clear(filter);
        }
        self.assets
            .asset_search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.view_scroll.set_offset(Point::default());
        cx.notify();
    }

    fn choose_asset_import(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.explorer.file_progress.is_some() {
            return;
        }
        let selected_folder = self.assets.asset_folder.clone();
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import assets".into()),
        });
        cx.spawn_in(window, async move |panel, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            if paths.is_empty() {
                return;
            }
            // An explicit folder filter is already a project-relative destination.
            // Otherwise ask for a destination; the existing importer validates category and confinement.
            let target = if let Some(folder) = selected_folder {
                folder
            } else {
                let prompt = cx.update(|_, cx| {
                    cx.prompt_for_paths(gpui_kit::PathPromptOptions {
                        files: false,
                        directories: true,
                        multiple: false,
                        prompt: Some(
                            "Choose a project resource folder (backgrounds, figures, bgm, se…)"
                                .into(),
                        ),
                    })
                });
                let Ok(prompt) = prompt else {
                    return;
                };
                let Ok(Ok(Some(folders))) = prompt.await else {
                    return;
                };
                let Some(folder) = folders.into_iter().next() else {
                    return;
                };
                let result = root.canonicalize().and_then(|root| {
                    folder.canonicalize().and_then(|folder| {
                        folder
                            .strip_prefix(root)
                            .map(Path::to_path_buf)
                            .map_err(|_| {
                                std::io::Error::other(
                                    "Choose a resource folder inside this project",
                                )
                            })
                    })
                });
                match result {
                    Ok(folder) => folder,
                    Err(error) => {
                        let _ = panel.update_in(cx, |_, window, cx| {
                            window.push_notification(Notification::error(error.to_string()), cx)
                        });
                        return;
                    }
                }
            };
            let _ = panel.update_in(cx, |panel, window, cx| {
                panel.import_asset_files(root, paths, target, window, cx)
            });
        })
        .detach();
    }
}

fn action(
    id: &'static str,
    icon: AssetIconName,
    hint: impl Into<SharedString> + 'static,
    enabled: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .text_color(rgb(if enabled { PRIMARY } else { MUTED }))
        .tooltip(icon_hint(hint))
        .when(enabled, |this| {
            this.cursor_pointer()
                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        })
        .child(Icon::new(icon).xsmall())
}

pub(super) struct Results<'a> {
    pub count: usize,
    pub total: usize,
    pub ordered: Arc<Vec<AssetKey>>,
    pub selection: &'a [AssetKey],
}

pub(super) fn render(
    root: &Path,
    results: Results<'_>,
    panel: &WorkbenchPanel,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let Results {
        count,
        total,
        ordered,
        selection,
    } = results;
    let selected = selection.len();
    let busy = panel.explorer.file_progress.is_some();
    let filtered = panel.assets.has_filters(cx);
    let import_root = root.to_owned();
    let select_root = root.to_owned();
    let clear_root = root.to_owned();
    let delete_root = root.to_owned();
    let can_select = count > 0 && !panel.assets.asset_unmapped;
    let can_delete = selected > 0 && !busy && !panel.assets.asset_unmapped;
    let actions = div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .child(
            action("asset-import", AssetIconName::Plus, "Import assets…", !busy).on_click(
                cx.listener(move |panel, _, window, cx| {
                    if !busy {
                        panel.choose_asset_import(import_root.clone(), window, cx);
                    }
                }),
            ),
        )
        .child(
            action(
                "asset-select-visible",
                AssetIconName::Check,
                "Select all results",
                can_select,
            )
            .on_click(move |_, _, cx| {
                if can_select {
                    cx.global_mut::<EditorDocuments>()
                        .set_asset_selection(&select_root, ordered.as_ref().clone());
                    cx.refresh_windows();
                }
            }),
        )
        .when(selected > 0, |row| {
            row.child(
                action(
                    "asset-clear-selection",
                    AssetIconName::X,
                    "Clear selection",
                    true,
                )
                .on_click(move |_, _, cx| {
                    cx.global_mut::<EditorDocuments>()
                        .clear_asset_selection(&clear_root);
                    cx.refresh_windows();
                }),
            )
        })
        .when(selected > 0, |row| {
            row.child(
                action(
                    "asset-delete-selected",
                    AssetIconName::Trash,
                    "Remove selected assets…",
                    can_delete,
                )
                .on_click(cx.listener(move |panel, _, window, cx| {
                    if can_delete {
                        panel.delete_assets(&delete_root, None, window, cx);
                    }
                })),
            )
        })
        .when(filtered, |row| {
            row.child(
                action(
                    "asset-clear-filters",
                    AssetIconName::ListFilter,
                    "Clear filters",
                    true,
                )
                .on_click(
                    cx.listener(|panel, _, window, cx| panel.clear_asset_filters(window, cx)),
                ),
            )
        });
    let mut content = div()
        .px_2()
        .pb_1()
        .flex_none()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(84.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(if count == total {
                            format!("{count} assets")
                        } else {
                            format!("{count} / {total} assets")
                        }),
                )
                .when(selected > 0, |row| {
                    row.child(
                        div()
                            .text_xs()
                            .text_color(rgb(PRIMARY))
                            .child(format!("{selected} selected")),
                    )
                })
                .child(actions),
        );
    let filters = panel.assets.filters();
    if !filters.is_empty() {
        content = content.child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .children(
                    filters
                        .into_iter()
                        .enumerate()
                        .map(|(id, (filter, label))| {
                            div()
                                .id(("asset-filter-chip", id))
                                .max_w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_1()
                                .px_2()
                                .py_1()
                                .rounded(px(5.))
                                .bg(rgb(SURFACE))
                                .text_xs()
                                .text_color(rgb(PRIMARY))
                                .tooltip(icon_hint(format!("Remove filter: {label}")))
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                .on_click(cx.listener(move |panel, _, _, cx| {
                                    panel.assets.clear(filter);
                                    panel.view_scroll.set_offset(Point::default());
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(label),
                                )
                                .child(Icon::new(AssetIconName::X).size(px(10.)))
                        }),
                ),
        );
    }
    content.into_any_element()
}

pub(super) fn empty_state(
    panel: &WorkbenchPanel,
    index: &AuthoringIndex,
    _window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let filtered = panel.assets.has_filters(cx);
    let empty_project = index.assets.is_empty() && index.unmapped.is_empty();
    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .p_3()
        .gap_2()
        .text_sm()
        .text_color(rgb(MUTED))
        .child(Icon::new(AssetIconName::FolderOpen).small())
        .child(if empty_project {
            "No assets yet"
        } else {
            "No matching assets"
        })
        .when(filtered, |this| {
            this.child(tool_action("Clear filters").on_click(
                cx.listener(|panel, _, window, cx| panel.clear_asset_filters(window, cx)),
            ))
        })
        .into_any_element()
}
