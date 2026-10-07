//! Explorer panel presentation.
use crate::app::*;

const EXPLORER_ROW_HEIGHT: f32 = 24.;
const EXPLORER_ROW_GAP: f32 = 1.;

impl WorkbenchPanel {
    pub(in crate::app) fn render_explorer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let PanelContent::Explorer {
            root,
            files,
            expanded,
        } = &self.content
        else {
            return Empty.into_any_element();
        };

        let project_root = root.clone();
        let index = cx.global::<EditorDocuments>().authoring(root);
        let mut errors = std::collections::BTreeMap::<PathBuf, usize>::new();
        for problem in &index.problems {
            if problem.severity == ProblemSeverity::Error {
                for path in problem
                    .path
                    .ancestors()
                    .filter(|path| !path.as_os_str().is_empty())
                {
                    *errors.entry(path.to_owned()).or_default() += 1;
                }
            }
        }
        let used = index
            .assets
            .iter()
            .filter(|asset| asset.reference_count > 0)
            .map(|asset| &asset.path)
            .collect::<HashSet<_>>();
        let unused = index
            .assets
            .iter()
            .filter(|asset| asset.exists && !used.contains(&asset.path))
            .map(|asset| &asset.path)
            .collect::<HashSet<_>>();
        let mut files = cx
            .global::<EditorDocuments>()
            .workspaces
            .get(root)
            .map(|workspace| workspace.files.clone())
            .unwrap_or_else(|| files.clone());
        if index.native {
            index.order_script_files(&mut files);
        }
        let project_name = project_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("PROJECT")
            .to_uppercase();
        let selected = self.explorer.file_selection.clone();
        let selected_directory = selected
            .as_ref()
            .and_then(|path| {
                files
                    .iter()
                    .find(|file| &file.relative_path == path)
                    .map(|file| {
                        if file.is_dir() {
                            path.clone()
                        } else {
                            path.parent().unwrap_or_else(|| Path::new("")).to_owned()
                        }
                    })
            })
            .unwrap_or_default();
        let reveals = files
            .iter()
            .filter(|file| file.is_dir())
            .map(|file| {
                let path = &file.relative_path;
                (
                    path.clone(),
                    disclosure_progress(
                        format!("explorer-folder-{}", path.display()),
                        expanded.contains(path),
                        window,
                        cx,
                    ),
                )
            })
            .collect::<HashMap<_, _>>();
        let visible = files
            .iter()
            .filter_map(|file| {
                let progress = file
                    .relative_path
                    .ancestors()
                    .skip(1)
                    .filter_map(|path| reveals.get(path))
                    .product::<f32>();
                (progress > 0.).then(|| (file.clone(), progress))
            })
            .collect::<Vec<_>>();
        let offsets = row_offsets(
            visible
                .iter()
                .map(|(_, progress)| (EXPLORER_ROW_HEIGHT + EXPLORER_ROW_GAP) * progress),
        );
        let range = visible_row_range(
            &offsets,
            (-f32::from(self.view_scroll.offset().y) - 100.).max(0.),
            f32::from(window.viewport_size().height) + 200.,
        );
        let first_row = range.start;
        let end_row = range.end;
        let bottom_height = offsets.last().unwrap() - offsets[end_row];
        let hovered_folder = self
            .explorer
            .file_drop_target
            .as_ref()
            .map(|(path, _)| path.clone());
        let external_root_target = PathBuf::new();
        let internal_root_target = PathBuf::new();
        let header = div()
            .h(px(34.))
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_3()
            .text_xs()
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .text_color(rgb(MUTED))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(project_name),
            )
            .child(
                file_action_icon("explorer-menu", AssetIconName::Ellipsis, "Explorer menu")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.open_explorer_menu(event.position, cx);
                            cx.stop_propagation();
                        }),
                    ),
            );
        let edit_row = self.explorer.file_edit.as_ref().map(|_| {
            div()
                .h(px(30.))
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .mx_2()
                .px_1()
                .rounded(px(6.))
                .bg(rgb(SURFACE))
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&self.explorer.file_name_input)
                            .appearance(false)
                            .bordered(false)
                            .size_full()
                            .text_xs()
                            .text_color(rgb(INK)),
                    ),
                )
                .child(
                    file_action_icon("file-edit-accept", AssetIconName::Check, "Apply").on_click(
                        cx.listener(|this, _, window, cx| this.commit_file_edit(window, cx)),
                    ),
                )
                .child(
                    file_action_icon("file-edit-cancel", AssetIconName::Close, "Cancel").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.explorer.file_edit = None;
                            cx.notify();
                        }),
                    ),
                )
        });
        let progress = self.explorer.file_progress.as_ref().map(|progress| {
            div()
                .h(px(24.))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(
                    Icon::new(IconName::LoaderCircle)
                        .xsmall()
                        .text_color(rgb(PRIMARY)),
                )
                .child(format!(
                    "Importing {} / {}",
                    progress.completed, progress.total
                ))
        });
        let context_menu = self.explorer.file_context_menu.clone().map(|menu| {
            let mut items = Vec::new();
            let new_parent = menu
                .path
                .as_ref()
                .and_then(|path| {
                    files
                        .iter()
                        .find(|file| &file.relative_path == path)
                        .map(|file| {
                            if file.is_dir() {
                                path.clone()
                            } else {
                                path.parent().unwrap_or_else(|| Path::new("")).to_owned()
                            }
                        })
                })
                .unwrap_or_else(|| selected_directory.clone());
            let new_file_parent = new_parent.clone();
            items.push(
                file_context_menu_item("explorer-new-file", AssetIconName::File, "New file", false)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_file_context_menu(window, cx);
                        this.begin_file_edit(
                            FileEditMode::NewFile {
                                parent: new_file_parent.clone(),
                            },
                            "",
                            window,
                            cx,
                        );
                    }))
                    .into_any_element(),
            );
            items.push(
                file_context_menu_item(
                    "explorer-new-folder",
                    AssetIconName::Folder,
                    "New folder",
                    false,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_file_context_menu(window, cx);
                    this.begin_file_edit(
                        FileEditMode::NewFolder {
                            parent: new_parent.clone(),
                        },
                        "",
                        window,
                        cx,
                    );
                }))
                .into_any_element(),
            );
            if let Some(path) = &menu.path {
                let rename_path = path.clone();
                let rename_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned();
                let copy_path = path.clone();
                let show_path = path.clone();
                let delete_path = path.clone();
                items.push(
                    file_context_menu_item(
                        "file-context-rename",
                        AssetIconName::Replace,
                        "Rename",
                        false,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_file_context_menu(window, cx);
                        this.begin_file_edit(
                            FileEditMode::Rename {
                                path: rename_path.clone(),
                            },
                            &rename_name,
                            window,
                            cx,
                        );
                    }))
                    .into_any_element(),
                );
                items.push(
                    file_context_menu_item("file-context-copy", AssetIconName::Copy, "Copy", false)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.explorer.file_clipboard = Some(copy_path.clone());
                            this.close_file_context_menu(window, cx);
                            cx.notify();
                        }))
                        .into_any_element(),
                );
                items.push(
                    file_context_menu_item(
                        "file-context-show-in-folder",
                        AssetIconName::ExternalLink,
                        "Show in Folder",
                        false,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_file_context_menu(window, cx);
                        this.show_in_folder(&show_path, window, cx);
                    }))
                    .into_any_element(),
                );
                items.push(
                    file_context_menu_item(
                        "file-context-delete",
                        AssetIconName::Delete,
                        "Delete",
                        true,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.explorer.file_selection = Some(delete_path.clone());
                        this.close_file_context_menu(window, cx);
                        this.confirm_delete_file(window, cx);
                    }))
                    .into_any_element(),
                );
            } else {
                let refresh_root = project_root.clone();
                if self.explorer.file_clipboard.is_some() {
                    items.push(
                        file_context_menu_item(
                            "explorer-paste",
                            AssetIconName::Copy,
                            "Paste",
                            false,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_file_context_menu(window, cx);
                            this.paste_file(window, cx);
                        }))
                        .into_any_element(),
                    );
                }
                items.push(
                    file_context_menu_item(
                        "explorer-refresh",
                        AssetIconName::RotateCw,
                        "Refresh",
                        false,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_file_context_menu(window, cx);
                        this.refresh_explorer(&refresh_root, cx);
                    }))
                    .into_any_element(),
                );
            }
            let menu_height = items.len() as f32 * 27. + 12.;
            let closing = menu.closing;
            let motion_duration = if closing { 90 } else { 120 };
            let menu_surface = div()
                .id(("file-context-surface", menu.epoch))
                .w(px(FILE_CONTEXT_MENU_WIDTH_PX))
                .h(px(menu_height))
                .overflow_hidden()
                .p_1()
                .rounded(px(9.))
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(SURFACE))
                .shadow_lg()
                .flex()
                .flex_col()
                .children(items)
                .with_animation(
                    ("file-context-motion", menu.epoch),
                    Animation::new(if cx.reduce_motion() {
                        Duration::ZERO
                    } else {
                        Duration::from_millis(motion_duration)
                    })
                    .with_easing(ease_out_quint()),
                    move |surface, delta| {
                        let progress = if closing { 1. - delta } else { delta };
                        let scale = 0.94 + progress * 0.06;
                        surface
                            .opacity(progress)
                            .w(px(FILE_CONTEXT_MENU_WIDTH_PX * scale))
                            .h(px(menu_height * scale))
                    },
                );
            deferred(
                anchored()
                    .anchor(Anchor::TopLeft)
                    .position(menu.position)
                    .snap_to_window_with_margin(px(6.))
                    .child(
                        div()
                            .w(px(FILE_CONTEXT_MENU_WIDTH_PX))
                            .h(px(menu_height))
                            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                                this.close_file_context_menu(window, cx)
                            }))
                            .child(menu_surface),
                    ),
            )
            .priority(100)
        });
        let content = div()
                    .flex()
                    .flex_col()
                    .child(header)
                    .children(edit_row)
                    .children(progress)
                    .child(
                        div()
                            .id("explorer-drop-root")
                            .w_full()
                            .min_h(px(80.))
                            .flex_none()
                            .on_drag_move(cx.listener(
                                |this, event: &DragMoveEvent<FileDrag>, _, cx| {
                                    if this.explorer.file_drop_target.as_ref().is_some_and(|(_, bounds)| {
                                        !bounds.contains(&event.event.position)
                                    }) {
                                        this.explorer.file_drop_target = None;
                                        cx.notify();
                                    }
                                },
                            ))
                            .on_drag_move(cx.listener(
                                |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                                    if this.explorer.file_drop_target.as_ref().is_some_and(|(_, bounds)| {
                                        !bounds.contains(&event.event.position)
                                    }) {
                                        this.explorer.file_drop_target = None;
                                        cx.notify();
                                    }
                                },
                            ))
                            .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
                                this.explorer.file_drop_target = None;
                                this.start_external_import(
                                    paths.paths().to_vec(),
                                    external_root_target.clone(),
                                    window,
                                    cx,
                                );
                            }))
                            .on_drop(cx.listener(move |this, drag: &FileDrag, window, cx| {
                                this.explorer.file_drop_target = None;
                                this.move_file(&drag.relative, &internal_root_target, window, cx);
                            }))
                            .child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .px_2()
                                    .id("explorer-files")
                                    .when(first_row > 0, |list| list.child(div().h(px(offsets[first_row])).flex_none()))
                                    .children(visible.into_iter().enumerate().skip(first_row).take(end_row - first_row).map(
                                        |(index, (file, progress))| {
                                            let root = project_root.clone();
                                            let relative = file.relative_path.clone();
                                            let click_relative = relative.clone();
                                            let context_relative = relative.clone();
                                            let drag = FileDrag {
                                                relative: relative.clone(),
                                            };
                                            let is_dir = file.kind == WorkspaceEntryKind::Directory;
                                            let openable = !is_dir
                                                && matches!(
                                                    relative
                                                        .extension()
                                                        .and_then(|extension| extension.to_str()),
                                                    Some(
                                                        "shou"
                                                            | "txt"
                                                            | "json"
                                                            | "yaml"
                                                            | "yml"
                                                            | "toml"
                                                            | "md"
                                                            | "webgal"
                                                    )
                                                );
                                            let depth =
                                                relative.components().count().saturating_sub(1);
                                            let name = relative
                                                .file_name()
                                                .and_then(|name| name.to_str())
                                                .unwrap_or("File")
                                                .to_owned();
                                            let error_count = errors.get(&relative).copied().unwrap_or(0);
                                            let unused = unused.contains(&relative);
                                            let row_selected = selected.as_ref() == Some(&relative);
                                            let collapsed = !expanded.contains(&relative);
                                            let drop_target = relative.clone();
                                            let external_target = relative.clone();
                                            let folder_hovered = is_dir
                                                && cx.has_active_drag()
                                                && hovered_folder.as_ref() == Some(&relative);
                                            let drop_gap = if is_dir {
                                                transition(
                                                    (
                                                        format!("explorer-drop-{}", relative.display()),
                                                        "height",
                                                    ),
                                                    if folder_hovered {
                                                        px(EXPLORER_ROW_HEIGHT + EXPLORER_ROW_GAP)
                                                    } else {
                                                        px(0.)
                                                    },
                                                    Transition::new(if cx.has_active_drag() {
                                                        TAB_MOTION_DURATION
                                                    } else {
                                                        Duration::ZERO
                                                    }),
                                                    window,
                                                    cx,
                                                )
                                            } else {
                                                px(0.)
                                            };
                                            let row = div()
                                                .id(("explorer-file", index))
                                                .h(px(EXPLORER_ROW_HEIGHT))
                                                .w_full()
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .pl(px(6. + depth as f32 * 16.))
                                                .pr_2()
                                                .rounded(px(8.))
                                                .whitespace_nowrap()
                                                .text_size(px(11.))
                                                .text_color(rgb(if row_selected {
                                                    INK
                                                } else {
                                                    MUTED
                                                }))
                                                .when(row_selected, |style| {
                                                    style.bg(rgb(SURFACE_HOVER))
                                                })
                                                .when(folder_hovered, |style| {
                                                    style
                                                        .bg(rgb(PRIMARY_DIM))
                                                        .border_1()
                                                        .border_color(rgb(0x405a68))
                                                })
                                                .cursor_pointer()
                                                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.explorer.file_selection =
                                                            Some(click_relative.clone());
                                                        if is_dir {
                                                            if let PanelContent::Explorer {
                                                                expanded, ..
                                                            } = &mut this.content
                                                                && !expanded.remove(&click_relative)
                                                            {
                                                                expanded
                                                                    .insert(click_relative.clone());
                                                            }
                                                            this.persist_explorer_state(cx);
                                                            cx.notify();
                                                        } else if openable {
                                                            open_workspace_document(
                                                                &root,
                                                                &click_relative,
                                                                window,
                                                                cx,
                                                            );
                                                        }
                                                    },
                                                ))
                                                .on_mouse_down(
                                                    MouseButton::Right,
                                                    cx.listener(
                                                        move |this,
                                                              event: &MouseDownEvent,
                                                              _,
                                                              cx| {
                                                            this.open_file_context_menu(
                                                                context_relative.clone(),
                                                                event.position,
                                                                cx,
                                                            );
                                                            cx.stop_propagation();
                                                        },
                                                    ),
                                                )
                                                .on_drag(drag, |drag: &FileDrag, _, _, cx| {
                                                    cx.new(|_| drag.clone())
                                                })
                                                .when(is_dir, |row| {
                                                    let hover_target = drop_target.clone();
                                                    let external_hover_target = external_target.clone();
                                                    row.on_drag_move(cx.listener(
                                                        move |this, event: &DragMoveEvent<FileDrag>, _, cx| {
                                                            if !event.bounds.contains(&event.event.position) {
                                                                return;
                                                            }
                                                            let source = &event.drag(cx).relative;
                                                            let valid = source != &hover_target
                                                                && !hover_target.starts_with(source)
                                                                && source.parent() != Some(hover_target.as_path());
                                                            if valid {
                                                                match &mut this.explorer.file_drop_target {
                                                                    Some((path, bounds)) if path == &hover_target => {
                                                                        *bounds = event.bounds;
                                                                    }
                                                                    target => {
                                                                        *target = Some((hover_target.clone(), event.bounds));
                                                                        cx.notify();
                                                                    }
                                                                }
                                                            } else if this.explorer.file_drop_target.take().is_some() {
                                                                cx.notify();
                                                            }
                                                        },
                                                    ))
                                                .on_drag_move(cx.listener(
                                                    move |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                                                        if event.bounds.contains(&event.event.position) {
                                                            match &mut this.explorer.file_drop_target {
                                                                Some((path, bounds)) if path == &external_hover_target => {
                                                                    *bounds = event.bounds;
                                                                }
                                                                target => {
                                                                    *target = Some((external_hover_target.clone(), event.bounds));
                                                                    cx.notify();
                                                                }
                                                            }
                                                        }
                                                    },
                                                ))
                                                .on_drop(cx.listener(
                                                    move |this, drag: &FileDrag, window, cx| {
                                                        cx.stop_propagation();
                                                        this.explorer.file_drop_target = None;
                                                        this.move_file(
                                                            &drag.relative,
                                                            &drop_target,
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                ))
                                                .on_drop(cx.listener(
                                                    move |this,
                                                          paths: &ExternalPaths,
                                                          window,
                                                          cx| {
                                                        cx.stop_propagation();
                                                        this.explorer.file_drop_target = None;
                                                        this.start_external_import(
                                                            paths.paths().to_vec(),
                                                            external_target.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                ))
                                                })
                                                .child(
                                                    div()
                                                        .w(px(12.))
                                                        .h(px(14.))
                                                        .flex_none()
                                                        .flex()
                                                        .items_center()
                                                        .when(is_dir, |slot| {
                                                            slot.child(disclosure_chevron(reveals.get(&relative).copied().unwrap_or(0.)))
                                                        }),
                                                )
                                                .child(
                                                    Icon::new(if is_dir {
                                                        if collapsed {
                                                            IconName::FolderClosed
                                                        } else {
                                                            IconName::FolderOpen
                                                        }
                                                    } else {
                                                        IconName::File
                                                    })
                                                    .xsmall()
                                                    .text_color(rgb(PRIMARY)),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .whitespace_nowrap()
                                                        .text_ellipsis()
                                                        .text_color(rgb(if error_count > 0 { 0xdb7780 } else if unused { 0xd2aa62 } else if row_selected { INK } else { MUTED }))
                                                        .child(name),
                                                )
                                                .when(error_count > 0, |row| row.child(div().flex_none().text_xs().font_weight(gpui_kit::FontWeight::BOLD).text_color(rgb(0xdb7780)).child(error_count.to_string())))
                                                .when(unused && error_count == 0, |row| row.child(div().id(("unused-resource", index)).flex_none().text_xs().text_color(rgb(0xd2aa62)).tooltip(icon_hint("Unused resource · no script references")).child("U")));
                                            div()
                                                .w_full()
                                                .h(px((EXPLORER_ROW_HEIGHT + EXPLORER_ROW_GAP) * progress) + drop_gap)
                                                .min_h_0()
                                                .flex_none()
                                                .overflow_hidden()
                                                .opacity(progress)
                                                .flex()
                                                .flex_col()
                                                .child(row)
                                                .when(is_dir, |wrapper| {
                                                    wrapper.child(
                                                        div()
                                                            .h(drop_gap)
                                                            .min_h_0()
                                                            .flex_none()
                                                            .overflow_hidden()
                                                            .pl(px(22. + depth as f32 * 16.))
                                                            .pr_2()
                                                            .child(
                                                                div()
                                                                    .h(px(EXPLORER_ROW_HEIGHT))
                                                                    .rounded(px(8.))
                                                                    .bg(rgb(PRIMARY_DIM)),
                                                            ),
                                                    )
                                                })
                                        },
                                    ))
                                    .when(bottom_height > 0., |list| list.child(div().h(px(bottom_height)).flex_none())),
                            ),
                    );
        div()
            .relative()
            .size_full()
            .min_h_0()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.open_explorer_menu(event.position, cx);
                }),
            )
            .child(vertical_overflow_view(
                "explorer-vertical-scroll",
                &self.view_scroll,
                content,
            ))
            .children(context_menu)
            .into_any_element()
    }
}
