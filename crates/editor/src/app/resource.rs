//! Asset browsing, filtering, preview and character creation.
use super::*;

pub(super) fn asset_action_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded(px(6.))
        .bg(rgb(SURFACE))
        .text_xs()
        .text_color(rgb(INK))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .child(label)
}

impl WorkbenchPanel {
    pub(super) fn asset_tags(
        &mut self,
        root: &Path,
        add: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tags = self
            .asset_batch_tags
            .read(cx)
            .value()
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let selection = cx.global::<EditorDocuments>().asset_selection(root);
        let index = cx.global::<EditorDocuments>().authoring(root);
        let result = (|| -> Result<(), String> {
            let path = index
                .assets_manifest
                .as_ref()
                .ok_or("Asset manifest unavailable")?;
            let mut source = cx
                .global::<EditorDocuments>()
                .source(root, path)
                .ok_or("Manifest unavailable")?;
            for key in &selection {
                let asset = index
                    .assets
                    .iter()
                    .find(|asset| asset.key() == *key)
                    .ok_or("Selection changed")?;
                let mut next = asset.tags.clone();
                if add {
                    for tag in &tags {
                        if !next.contains(tag) {
                            next.push(tag.clone());
                        }
                    }
                } else {
                    next.retain(|tag| !tags.contains(tag));
                }
                source =
                    file_ops::edit_manifest_asset(&source, asset, &asset.id, asset.kind, &next)
                        .map_err(|error| error.to_string())?;
            }
            apply_prepared_edits(root, &[(path.clone(), source)], window, cx)
        })();
        match result {
            Ok(()) => {
                self.asset_batch_tags
                    .update(cx, |input, cx| input.set_value("", window, cx));
            }
            Err(error) => window.push_notification(Notification::error(error), cx),
        }
        cx.refresh_windows();
    }

    pub(super) fn delete_assets(
        &mut self,
        root: &Path,
        unmapped: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = root.to_owned();
        let selection = cx.global::<EditorDocuments>().asset_selection(&root);
        let count = if unmapped.is_some() {
            1
        } else {
            selection.len()
        };
        if count == 0 {
            return;
        }
        let buttons = if unmapped.is_some() {
            vec![
                PromptButton::Other("Trash".into()),
                PromptButton::Cancel("Cancel".into()),
            ]
        } else {
            vec![
                PromptButton::Other("Keep files".into()),
                PromptButton::Other("Trash".into()),
                PromptButton::Cancel("Cancel".into()),
            ]
        };
        let receiver = window.prompt(PromptLevel::Warning, &format!("Delete {count} asset(s)?"),
            Some("Keep files removes the mapping and marks files .unmapped. Trash uses the system Trash. Existing script references are left for diagnostics; Undo restores the change."), &buttons, cx);
        cx.spawn_in(window, async move |this, cx| {
            let answer = receiver.await.ok();
            let delete_file = if unmapped.is_some() { answer == Some(0) } else { answer == Some(1) };
            if answer.is_none() || answer == Some(if unmapped.is_some() { 1 } else { 2 }) { return; }
            let _ = this.update_in(cx, |this, window, cx| {
                let result = (|| -> Result<(), String> {
                    let index = cx.global::<EditorDocuments>().authoring(&root);
                    let mut edits = Vec::new();
                    let mut files = Vec::new();
                    if let Some(path) = &unmapped {
                        files.push(file_ops::AssetFileChange::delete(path.clone()));
                    } else {
                        let path = index.assets_manifest.as_ref().ok_or("Manifest unavailable")?;
                        let mut source = cx.global::<EditorDocuments>().source(&root, path).ok_or("Manifest unavailable")?;
                        for key in &selection {
                            let asset = index.assets.iter().find(|asset| asset.key() == *key).ok_or("Selection changed")?;
                            if asset.exists && index.assets.iter().any(|other| other.path == asset.path && other.key() != asset.key() && !selection.contains(&other.key())) {
                                return Err("File has another mapping; select all mappings before deleting it".into());
                            }
                            source = file_ops::remove_manifest_asset(&source, asset).map_err(|error| error.to_string())?;
                            if asset.exists && !files.iter().any(|change: &file_ops::AssetFileChange| change.from == asset.path) { files.push(if delete_file { file_ops::AssetFileChange::delete(asset.path.clone()) }
                                else { file_ops::AssetFileChange::relocate(asset.path.clone(), file_ops::unmapped_path(&asset.path)) }); }
                        }
                        edits.push((path.clone(), source));
                    }
                    apply_asset_transaction(&root, &edits, files, window, cx)?;
                    cx.global_mut::<EditorDocuments>().clear_asset_selection(&root);
                    Ok(())
                })();
                if let Err(error) = result { window.push_notification(Notification::error(error), cx); }
                else { this.focus.focus(window, cx); }
                cx.refresh_windows();
            });
        }).detach();
    }

    pub(super) fn remap_asset(
        &mut self,
        root: &Path,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = (|| -> Result<(), String> {
            let mapped =
                file_ops::mapped_path(path).ok_or("Only .unmapped files can be remapped")?;
            let kind = file_ops::unmapped_candidate_kind(path).ok_or("Unknown resource type")?;
            // The retained file must remain valid; this does not transcode it.
            file_ops::validate_asset_type(root, path, kind).map_err(|error| error.to_string())?;
            let index = cx.global::<EditorDocuments>().authoring(root);
            let manifest = index
                .assets_manifest
                .as_ref()
                .ok_or("Manifest unavailable")?;
            let source = cx
                .global::<EditorDocuments>()
                .source(root, manifest)
                .ok_or("Manifest unavailable")?;
            let edited = file_ops::remap_manifest(&source, kind, &mapped)
                .map_err(|error| error.to_string())?;
            apply_asset_transaction(
                root,
                &[(manifest.clone(), edited)],
                vec![file_ops::AssetFileChange::relocate(path.to_owned(), mapped)],
                window,
                cx,
            )
        })();
        if let Err(error) = result {
            window.push_notification(Notification::error(error), cx);
        }
        cx.refresh_windows();
    }
}

pub(super) mod browse;
mod picker;
pub(super) mod thumbnail;
pub(super) use browse::render_assets;
pub(super) use picker::*;

pub(super) fn render_asset_preview(
    root: &Path,
    selection: Option<AssetPreviewSelection>,
    selected_count: usize,
    cx: &mut App,
) -> AnyElement {
    let Some(asset) = selection else {
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .text_sm()
            .text_color(rgb(MUTED))
            .child(if selected_count > 1 {
                format!("{selected_count} assets selected")
            } else {
                "Select an asset to preview".to_owned()
            })
            .into_any_element();
    };
    let image = matches!(
        asset.kind,
        AssetKind::Background | AssetKind::Figure | AssetKind::Particle
    );
    let preview_path = file_ops::mapped_path(&asset.path).unwrap_or_else(|| asset.path.clone());
    let supported_image = preview_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            gpui_kit::Img::extensions()
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(ext))
        });
    let display = if image && supported_image {
        if let Some(file) = asset.file {
            img(file)
                .size_full()
                .with_fallback(|| {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child("Could not decode image")
                        .into_any_element()
                })
                .into_any_element()
        } else {
            preview_placeholder("Image is missing or outside the project")
        }
    } else if image {
        preview_placeholder("This image format cannot be previewed")
    } else if matches!(
        asset.kind,
        AssetKind::Voice | AssetKind::Bgm | AssetKind::Effect
    ) {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                Icon::new(AssetIconName::Music)
                    .size(px(32.))
                    .text_color(rgb(MUTED)),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(audition_control(root, &asset.path, false, cx))
                    .child(replay_control(root, &asset.path, cx)),
            )
            .into_any_element()
    } else {
        preview_placeholder(match asset.kind {
            AssetKind::Voice | AssetKind::Bgm | AssetKind::Effect => "Audio asset",
            AssetKind::Video => "Video asset",
            AssetKind::Background | AssetKind::Figure | AssetKind::Particle => unreachable!(),
        })
    };
    div()
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .child(
            div()
                .flex_none()
                .text_sm()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(rgb(INK))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(asset.label),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .rounded(px(8.))
                .bg(rgb(CANVAS))
                .overflow_hidden()
                .child(display),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(rgb(MUTED))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(format!("{} · {}", asset.kind.label(), asset.path.display())),
        )
        .into_any_element()
}

fn preview_placeholder(message: &'static str) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(rgb(MUTED))
        .child(message)
        .into_any_element()
}

pub(super) fn render_asset_filter_menu(
    menu: AssetFilterMenu,
    folders: &[PathBuf],
    index: &AuthoringIndex,
    panel: &WorkbenchPanel,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let type_choices = std::iter::once((
        "All".to_owned(),
        AssetFilterChoice::Type(None),
        panel.asset_kind.is_none(),
    ))
    .chain(
        [
            AssetKind::Background,
            AssetKind::Figure,
            AssetKind::Voice,
            AssetKind::Bgm,
            AssetKind::Effect,
            AssetKind::Video,
            AssetKind::Particle,
        ]
        .into_iter()
        .map(|kind| {
            (
                kind.label().to_owned(),
                AssetFilterChoice::Type(Some(kind)),
                panel.asset_kind == Some(kind),
            )
        }),
    )
    .collect::<Vec<_>>();
    let folder_choices = std::iter::once((
        "All".to_owned(),
        AssetFilterChoice::Folder(None),
        panel.asset_folder.is_none(),
    ))
    .chain(folders.iter().map(|folder| {
        (
            folder.display().to_string(),
            AssetFilterChoice::Folder(Some(folder.clone())),
            panel.asset_folder.as_ref() == Some(folder),
        )
    }))
    .collect::<Vec<_>>();
    use crate::authoring::{AssetSize, AssetStatus};
    let tags = index
        .assets
        .iter()
        .flat_map(|asset| asset.tags.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut tag_choices = vec![(
        "All".into(),
        AssetFilterChoice::Tags(None),
        panel.asset_tag.is_none(),
    )];
    tag_choices.extend(tags.into_iter().map(|tag| {
        (
            tag.clone(),
            AssetFilterChoice::Tags(Some(tag.clone())),
            panel.asset_tag.as_ref() == Some(&tag),
        )
    }));
    let groups = [
        (
            AssetFilterGroup::Tags,
            "Tags",
            panel.asset_tag.clone().unwrap_or_else(|| "All".into()),
            tag_choices,
        ),
        (
            AssetFilterGroup::Status,
            "Status",
            format!("{:?}", panel.asset_status),
            [
                AssetStatus::All,
                AssetStatus::Used,
                AssetStatus::Unused,
                AssetStatus::Missing,
            ]
            .into_iter()
            .map(|value| {
                (
                    format!("{value:?}"),
                    AssetFilterChoice::Status(value),
                    panel.asset_status == value,
                )
            })
            .collect(),
        ),
        (
            AssetFilterGroup::FileSize,
            "File size",
            format!("{:?}", panel.asset_size),
            [
                (AssetSize::All, "All"),
                (AssetSize::Small, "< 1 MB"),
                (AssetSize::Medium, "1–10 MB"),
                (AssetSize::Large, "≥ 10 MB"),
            ]
            .into_iter()
            .map(|(value, label)| {
                (
                    label.into(),
                    AssetFilterChoice::FileSize(value),
                    panel.asset_size == value,
                )
            })
            .collect(),
        ),
        (
            AssetFilterGroup::Modified,
            "Modified",
            panel.asset_modified.map_or("Any".into(), |age| {
                format!("{} days", age.as_secs() / 86400)
            }),
            [
                (None, "Any"),
                (Some(Duration::from_secs(86400)), "Last day"),
                (Some(Duration::from_secs(7 * 86400)), "Last week"),
                (Some(Duration::from_secs(30 * 86400)), "Last month"),
            ]
            .into_iter()
            .map(|(value, label)| {
                (
                    label.into(),
                    AssetFilterChoice::Modified(value),
                    panel.asset_modified == value,
                )
            })
            .collect(),
        ),
        (
            AssetFilterGroup::Type,
            "Type",
            panel
                .asset_kind
                .map_or("All".to_owned(), |kind| kind.label().to_owned()),
            type_choices,
        ),
        (
            AssetFilterGroup::Folder,
            "Folder",
            panel
                .asset_folder
                .as_ref()
                .map_or("All".to_owned(), |folder| folder.display().to_string()),
            folder_choices,
        ),
        (
            AssetFilterGroup::Sort,
            "Sort",
            match panel.asset_sort {
                AssetSort::Name => "Name",
                AssetSort::Path => "Path",
                AssetSort::References => "References",
                AssetSort::Modified => "Modified",
                AssetSort::Size => "Size",
            }
            .to_owned(),
            vec![
                (
                    "Name".to_owned(),
                    AssetFilterChoice::Sort(AssetSort::Name),
                    panel.asset_sort == AssetSort::Name,
                ),
                (
                    "Path".to_owned(),
                    AssetFilterChoice::Sort(AssetSort::Path),
                    panel.asset_sort == AssetSort::Path,
                ),
                (
                    "References".to_owned(),
                    AssetFilterChoice::Sort(AssetSort::References),
                    panel.asset_sort == AssetSort::References,
                ),
                (
                    "Modified".into(),
                    AssetFilterChoice::Sort(AssetSort::Modified),
                    panel.asset_sort == AssetSort::Modified,
                ),
                (
                    "Size".into(),
                    AssetFilterChoice::Sort(AssetSort::Size),
                    panel.asset_sort == AssetSort::Size,
                ),
            ],
        ),
        (
            AssetFilterGroup::View,
            "View",
            match panel.asset_grid {
                None => "Auto",
                Some(true) => "Grid",
                Some(false) => "List",
            }
            .to_owned(),
            vec![
                (
                    "Auto".to_owned(),
                    AssetFilterChoice::View(None),
                    panel.asset_grid.is_none(),
                ),
                (
                    "List".to_owned(),
                    AssetFilterChoice::View(Some(false)),
                    panel.asset_grid == Some(false),
                ),
                (
                    "Grid".to_owned(),
                    AssetFilterChoice::View(Some(true)),
                    panel.asset_grid == Some(true),
                ),
            ],
        ),
        (
            AssetFilterGroup::Size,
            "Thumbnails",
            if panel.asset_large {
                "Large"
            } else {
                "Compact"
            }
            .to_owned(),
            vec![
                (
                    "Compact".to_owned(),
                    AssetFilterChoice::Size(false),
                    !panel.asset_large,
                ),
                (
                    "Large".to_owned(),
                    AssetFilterChoice::Size(true),
                    panel.asset_large,
                ),
            ],
        ),
        (
            AssetFilterGroup::Show,
            "Show",
            if panel.asset_unmapped {
                "Unmapped"
            } else {
                "Mapped"
            }
            .to_owned(),
            vec![
                (
                    "Mapped".to_owned(),
                    AssetFilterChoice::Show(false),
                    !panel.asset_unmapped,
                ),
                (
                    "Unmapped".to_owned(),
                    AssetFilterChoice::Show(true),
                    panel.asset_unmapped,
                ),
            ],
        ),
    ];
    let expanded_count = groups
        .iter()
        .find(|(group, _, _, _)| menu.expanded == Some(*group))
        .map_or(0, |(_, _, _, choices)| choices.len());
    let height = (8 + groups.len() * 32 + expanded_count * 27).min(344) as f32;
    let closing = menu.closing;
    let motion_duration = if closing { 90 } else { 120 };
    let surface = div()
        .id(("asset-filter-surface", menu.epoch))
        .w(px(ASSET_FILTER_MENU_WIDTH_PX))
        .h(px(height))
        .overflow_hidden()
        .p_1()
        .rounded(px(8.))
        .bg(rgb(SURFACE))
        .shadow_lg()
        .child(
            div()
                .id("asset-filter-options")
                .size_full()
                .overflow_y_scrollbar()
                .children(groups.into_iter().enumerate().map(
                    |(group_index, (group, title, value, choices))| {
                        let expanded = menu.expanded == Some(group);
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .id(("asset-filter-group", group_index))
                                    .h(px(32.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded(px(5.))
                                    .text_xs()
                                    .text_color(rgb(INK))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(menu) = this.asset_filter_menu.as_mut() {
                                            menu.expanded = if menu.expanded == Some(group) {
                                                None
                                            } else {
                                                Some(group)
                                            };
                                            cx.notify();
                                        }
                                    }))
                                    .child(div().flex_1().min_w_0().child(title))
                                    .child(
                                        div()
                                            .max_w(px(110.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .text_color(rgb(MUTED))
                                            .child(value),
                                    )
                                    .child(
                                        Icon::new(AssetIconName::ChevronDown)
                                            .xsmall()
                                            .rotate(radians(if expanded {
                                                std::f32::consts::PI
                                            } else {
                                                0.
                                            }))
                                            .text_color(rgb(MUTED)),
                                    ),
                            )
                            .when(expanded, |this| {
                                this.children(choices.into_iter().enumerate().map(
                                    |(option_index, (label, choice, selected))| {
                                        div()
                                            .id(SharedString::from(format!(
                                                "asset-filter-option-{group_index}-{option_index}"
                                            )))
                                            .h(px(27.))
                                            .pl_4()
                                            .pr_2()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .rounded(px(5.))
                                            .text_xs()
                                            .text_color(rgb(if selected { PRIMARY } else { MUTED }))
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                match &choice {
                                                    AssetFilterChoice::Tags(tag) => {
                                                        this.asset_tag = tag.clone()
                                                    }
                                                    AssetFilterChoice::Status(status) => {
                                                        this.asset_status = *status
                                                    }
                                                    AssetFilterChoice::FileSize(size) => {
                                                        this.asset_size = *size
                                                    }
                                                    AssetFilterChoice::Modified(age) => {
                                                        this.asset_modified = *age
                                                    }
                                                    AssetFilterChoice::Type(kind) => {
                                                        this.asset_kind = *kind
                                                    }
                                                    AssetFilterChoice::Folder(folder) => {
                                                        this.asset_folder = folder.clone()
                                                    }
                                                    AssetFilterChoice::Sort(sort) => {
                                                        this.asset_sort = *sort
                                                    }
                                                    AssetFilterChoice::View(grid) => {
                                                        this.asset_grid = *grid
                                                    }
                                                    AssetFilterChoice::Size(large) => {
                                                        this.asset_large = *large
                                                    }
                                                    AssetFilterChoice::Show(unmapped) => {
                                                        this.asset_unmapped = *unmapped
                                                    }
                                                }
                                                this.view_scroll
                                                    .set_offset(gpui_kit::point(px(0.), px(0.)));
                                                if let Some(menu) = this.asset_filter_menu.as_mut()
                                                {
                                                    menu.expanded = None;
                                                }
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .text_ellipsis()
                                                    .child(label),
                                            )
                                            .when(selected, |this| {
                                                this.child(
                                                    Icon::new(AssetIconName::Check)
                                                        .xsmall()
                                                        .text_color(rgb(PRIMARY)),
                                                )
                                            })
                                    },
                                ))
                            })
                            .into_any_element()
                    },
                )),
        )
        .with_animation(
            ("asset-filter-motion", menu.epoch),
            Animation::new(Duration::from_millis(motion_duration)).with_easing(ease_out_quint()),
            move |surface, delta| {
                let progress = if closing { 1. - delta } else { delta };
                let scale = 0.94 + progress * 0.06;
                surface
                    .opacity(progress)
                    .w(px(ASSET_FILTER_MENU_WIDTH_PX * scale))
                    .h(px(height * scale))
            },
        );
    deferred(
        anchored()
            .anchor(Anchor::TopLeft)
            .position(menu.position)
            .snap_to_window_with_margin(px(6.))
            .child(
                div()
                    .w(px(ASSET_FILTER_MENU_WIDTH_PX))
                    .h(px(height))
                    .occlude()
                    .on_mouse_down_out(
                        cx.listener(|this, _, window, cx| this.close_asset_filter_menu(window, cx)),
                    )
                    .child(surface),
            ),
    )
    .priority(100)
    .into_any_element()
}

impl WorkbenchPanel {
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
        let Some(path) = index.characters_manifest.clone() else {
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
