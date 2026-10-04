use super::*;

const EXPLORER_ROW_HEIGHT: f32 = 24.;
const EXPLORER_ROW_GAP: f32 = 1.;

impl Render for WorkbenchPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let PanelContent::Document {
            root,
            relative,
            editor,
            ..
        } = &self.content
            && relative
                .extension()
                .is_some_and(|extension| extension == "shou")
        {
            let index = cx.global::<EditorDocuments>().authoring(root);
            if self
                .diagnostic_index
                .upgrade()
                .is_none_or(|previous| !Arc::ptr_eq(&previous, &index))
            {
                self.diagnostic_index = Arc::downgrade(&index);
                self.syntax_check = Some(completion::schedule_syntax_check(
                    editor.clone(),
                    self.syntax_marks.clone(),
                    (index.clone(), relative.clone()),
                    window,
                    cx,
                ));
            }
        }
        if self.document_mode == DocumentMode::Text && self.text_scroll_pending {
            self.text_scroll_pending = false;
            let panel = cx.entity().downgrade();
            // Cursor scrolling needs the Text editor's first layout, including after
            // switching from Blocks. Run once after that frame, without a timer.
            window.on_next_frame(move |window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if panel.document_mode == DocumentMode::Text
                        && let PanelContent::Document { editor, .. } = &panel.content
                    {
                        editor.update(cx, |editor, cx| {
                            editor.set_cursor_position(editor.cursor_position(), window, cx);
                        });
                    }
                });
            });
        }
        self.refresh_block_drag(window, cx);
        if !cx.has_active_drag() {
            self.file_drop_target = None;
        }
        if self.file_commit_requested {
            self.file_commit_requested = false;
            self.commit_file_edit(window, cx);
        }
        if self.scene_commit_requested {
            self.scene_commit_requested = false;
            self.commit_scene_edit(window, cx);
        }
        if let PanelContent::Inspector { root, .. } = &self.content {
            let root = root.clone();
            self.refresh_inspector_editors(&root, window, cx);
        }
        if self.document_mode == DocumentMode::Block
            && let PanelContent::Document { root, relative, .. } = &self.content
        {
            let root = root.clone();
            let relative = relative.clone();
            self.update_block_viewport(window, cx);
            self.sync_visual_editors(window, cx);
            self.refresh_inline_block_controls(&root, &relative, window, cx);
        }
        if self.resource_picker.as_ref().is_some_and(|picker| {
            picker.epoch != cx.global::<EditorDocuments>().resource_picker_epoch
                || picker.window_size != window.viewport_size()
                || !picker.source_is_current(cx)
        }) {
            self.close_resource_picker(false, window, cx);
        }
        let mono = Theme::global(cx).mono_font_family.clone();
        let body = match &self.content {
            PanelContent::Explorer {
                root,
                files,
                expanded,
            } => {
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
                let selected = self.file_selection.clone();
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
                let visible = files
                    .iter()
                    .filter(|file| {
                        let mut ancestor = file.relative_path.parent();
                        while let Some(path) = ancestor {
                            if !path.as_os_str().is_empty() && !expanded.contains(path) {
                                return false;
                            }
                            ancestor = path.parent();
                        }
                        true
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                let stride = EXPLORER_ROW_HEIGHT + EXPLORER_ROW_GAP;
                let first_row =
                    (((-f32::from(self.view_scroll.offset().y) - 100.).max(0.) / stride) as usize)
                        .min(visible.len());
                let row_count =
                    ((f32::from(window.viewport_size().height) + 200.) / stride).ceil() as usize;
                let end_row = (first_row + row_count).min(visible.len());
                let bottom_rows = visible.len() - end_row;
                let hovered_folder = self.file_drop_target.as_ref().map(|(path, _)| path.clone());
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
                let edit_row = self.file_edit.as_ref().map(|_| {
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
                                Input::new(&self.file_name_input)
                                    .appearance(false)
                                    .bordered(false)
                                    .size_full()
                                    .text_xs()
                                    .text_color(rgb(INK)),
                            ),
                        )
                        .child(
                            file_action_icon("file-edit-accept", AssetIconName::Check, "Apply")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.commit_file_edit(window, cx)
                                })),
                        )
                        .child(
                            file_action_icon("file-edit-cancel", AssetIconName::Close, "Cancel")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.file_edit = None;
                                    cx.notify();
                                })),
                        )
                });
                let progress = self.file_progress.as_ref().map(|progress| {
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
                let context_menu = self.file_context_menu.clone().map(|menu| {
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
                        file_context_menu_item(
                            "explorer-new-file",
                            AssetIconName::File,
                            "New file",
                            false,
                        )
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
                            file_context_menu_item(
                                "file-context-copy",
                                AssetIconName::Copy,
                                "Copy",
                                false,
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.file_clipboard = Some(copy_path.clone());
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
                                this.file_selection = Some(delete_path.clone());
                                this.close_file_context_menu(window, cx);
                                this.confirm_delete_file(window, cx);
                            }))
                            .into_any_element(),
                        );
                    } else {
                        let refresh_root = project_root.clone();
                        if self.file_clipboard.is_some() {
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
                            Animation::new(Duration::from_millis(motion_duration))
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
                                    if this.file_drop_target.as_ref().is_some_and(|(_, bounds)| {
                                        !bounds.contains(&event.event.position)
                                    }) {
                                        this.file_drop_target = None;
                                        cx.notify();
                                    }
                                },
                            ))
                            .on_drag_move(cx.listener(
                                |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                                    if this.file_drop_target.as_ref().is_some_and(|(_, bounds)| {
                                        !bounds.contains(&event.event.position)
                                    }) {
                                        this.file_drop_target = None;
                                        cx.notify();
                                    }
                                },
                            ))
                            .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
                                this.file_drop_target = None;
                                this.start_external_import(
                                    paths.paths().to_vec(),
                                    external_root_target.clone(),
                                    window,
                                    cx,
                                );
                            }))
                            .on_drop(cx.listener(move |this, drag: &FileDrag, window, cx| {
                                this.file_drop_target = None;
                                this.move_file(&drag.relative, &internal_root_target, window, cx);
                            }))
                            .child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(EXPLORER_ROW_GAP))
                                    .px_2()
                                    .id("explorer-files")
                                    .when(first_row > 0, |list| list.child(div().h(px(first_row as f32 * stride - EXPLORER_ROW_GAP)).flex_none()))
                                    .children(visible.into_iter().enumerate().skip(first_row).take(row_count).map(
                                        |(index, file)| {
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
                                                        this.file_selection =
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
                                                                match &mut this.file_drop_target {
                                                                    Some((path, bounds)) if path == &hover_target => {
                                                                        *bounds = event.bounds;
                                                                    }
                                                                    target => {
                                                                        *target = Some((hover_target.clone(), event.bounds));
                                                                        cx.notify();
                                                                    }
                                                                }
                                                            } else if this.file_drop_target.take().is_some() {
                                                                cx.notify();
                                                            }
                                                        },
                                                    ))
                                                .on_drag_move(cx.listener(
                                                    move |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                                                        if event.bounds.contains(&event.event.position) {
                                                            match &mut this.file_drop_target {
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
                                                        this.file_drop_target = None;
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
                                                        this.file_drop_target = None;
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
                                                            slot.child(
                                                                Icon::new(if collapsed {
                                                                    IconName::ChevronRight
                                                                } else {
                                                                    IconName::ChevronDown
                                                                })
                                                                .xsmall()
                                                                .text_color(rgb(MUTED)),
                                                            )
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
                                    .when(bottom_rows > 0, |list| list.child(div().h(px(bottom_rows as f32 * stride - EXPLORER_ROW_GAP)).flex_none())),
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
            PanelContent::Document {
                root,
                relative,
                document,
                editor,
            } => {
                let eiyashou = document.is_some()
                    && relative.extension().and_then(|value| value.to_str()) == Some("shou");
                let mode = self.document_mode;
                let body = if eiyashou && mode == DocumentMode::Block {
                    render_block_projection(
                        BlockProjectionView {
                            root,
                            relative,
                            document: document.as_ref().unwrap(),
                            editors: &self.block_text_editors,
                            inline: &self.inline_block_controls,
                            collapsed_scenes: &self.collapsed_scenes,
                            selected_blocks: &self.selected_blocks,
                            draft_text: self.draft_text.as_ref(),
                            drag: &self.block_drag,
                            row_bounds: &self.block_row_bounds,
                            row_positions: &self.block_row_positions,
                            scroll_handle: &self.view_scroll,
                            scroll_anchor: &self.block_scroll_anchor,
                            scroll_pending: self.block_scroll_pending,
                            minimap: &self.minimap_navigation,
                            scene_edit: self.scene_edit.as_ref(),
                            scene_name_input: &self.scene_name_input,
                            visible: &self.block_visible,
                            heights: &self.block_heights,
                            layout: &self.block_layout,
                        },
                        window,
                        cx,
                    )
                } else {
                    let editor_for_fade = editor.clone();
                    let selection_root = root.clone();
                    let code = div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .bottom_0()
                        .left(px(-EDITOR_GUTTER_TRIM_PX))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |_, _, _, cx| {
                                cx.global_mut::<EditorDocuments>()
                                    .clear_asset_selection(&selection_root);
                                cx.refresh_windows();
                            }),
                        )
                        .child(
                            Editor::new(editor)
                                .appearance(false)
                                .bordered(false)
                                .readonly(document.is_none())
                                .h(gpui_kit::relative(1.))
                                .w_full()
                                .p_1()
                                .font_family(mono)
                                .text_size(px(13.))
                                .text_color(rgb(0xc8cbd0)),
                        )
                        .child(bottom_overflow_fade(move |cx| {
                            let editor = editor_for_fade.read(cx);
                            // Keep the overflow hint independent of document length on scroll.
                            let row_count = editor.text().lines_len();
                            editor
                                .visible_row_range()
                                .is_some_and(|visible| visible.end < row_count)
                        }))
                        .into_any_element();
                    div()
                        .size_full()
                        .flex()
                        .min_w_0()
                        .min_h_0()
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .h_full()
                                .min_w_0()
                                .overflow_hidden()
                                .child(code),
                        )
                        .child(text_minimap::render(
                            editor,
                            relative,
                            &mut self.text_minimap,
                            &self.minimap_navigation,
                            window,
                            cx,
                        ))
                        .into_any_element()
                };
                let picker = (eiyashou && mode == DocumentMode::Block && self.block_picker_open)
                    .then(|| {
                        let query = self
                            .block_picker_input
                            .read(cx)
                            .value()
                            .to_string()
                            .to_lowercase();
                        let preferences = cx
                            .global::<EditorDocuments>()
                            .block_picker_preferences()
                            .clone();
                        let kinds = picker_kinds(
                            &preferences,
                            &query,
                            self.block_picker_category,
                            self.block_picker_customize,
                        );
                        render_block_picker(
                            &kinds,
                            &self.block_picker_input,
                            self.block_picker_index,
                            self.block_picker_category,
                            self.block_picker_customize,
                            &preferences,
                            cx,
                        )
                    });
                let scene_menu = (eiyashou && mode == DocumentMode::Block)
                    .then(|| self.scene_context_menu.clone())
                    .flatten()
                    .map(|menu| render_scene_context_menu(menu, cx));
                let block_menu = (eiyashou && mode == DocumentMode::Block)
                    .then(|| self.block_context_menu.clone())
                    .flatten()
                    .map(|(row, position, source)| {
                        render_block_context_menu(row, position, source, cx)
                    });
                if eiyashou && mode == DocumentMode::Block {
                    self.block_scroll_pending = false;
                }
                div()
                    .id("document-content")
                    .size_full()
                    .flex()
                    .flex_col()
                    .rounded_b(px(VIEW_RADIUS_PX))
                    .bg(rgb(CANVAS))
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(body)
                            .when_some(picker, |this, picker| this.child(picker))
                            .when_some(scene_menu, |this, menu| this.child(menu))
                            .when_some(block_menu, |this, menu| this.child(menu)),
                    )
                    .into_any_element()
            }
            PanelContent::Inspector { root, file_count } => {
                let document_count = cx.global::<EditorDocuments>().open_document_count(root);
                let index = cx.global::<EditorDocuments>().authoring(root);
                let has_selection = cx.global::<EditorDocuments>().selection(root).is_some();
                let asset_selection = cx.global::<EditorDocuments>().asset_selection(root);
                let selected_assets = asset_selection
                    .iter()
                    .filter_map(|key| index.assets.iter().find(|asset| asset.key() == *key))
                    .collect::<Vec<_>>();
                let inputs = self.inspector_inputs.clone();
                let text_selects = self.inspector_selects.clone();
                let source_key = self.source_inspector_key.clone();
                let source_inputs = self.source_inspector_inputs.clone();
                let source_texts = self.source_inspector_texts.clone();
                let source_sliders = self.source_inspector_sliders.clone();
                let source_selects = self.source_inspector_selects.clone();
                let source_effect = self.source_inspector_effect;
                let asset_inputs = self.asset_inspector_inputs.clone();
                let unmapped_preview =
                    cx.global::<EditorDocuments>()
                        .asset_preview(root)
                        .filter(|preview| {
                            file_ops::mapped_path(&preview.path).is_some()
                                && asset_selection.is_empty()
                        });
                let content = div()
                    .flex()
                    .flex_col()
                    .px(px(16.))
                    .py(px(10.))
                    .gap_2()
                    .when(!selected_assets.is_empty(), |this| {
                        this.child(section_label("ASSET"))
                            .when(selected_assets.len() == 1, |this| {
                                let asset = selected_assets[0];
                                let mut groups = BTreeMap::<
                                    PathBuf,
                                    Vec<&crate::authoring::AssetReference>,
                                >::new();
                                for reference in index
                                    .asset_references
                                    .iter()
                                    .filter(|reference| reference.key == asset.key())
                                {
                                    let group = groups.entry(reference.path.clone()).or_default();
                                    if !group.iter().any(|entry| entry.line == reference.line) {
                                        group.push(reference);
                                    }
                                }
                                let limit = (f32::from(window.viewport_size().height) / 96.)
                                    .floor()
                                    .clamp(3., 12.)
                                    as usize;
                                let more = groups.len().saturating_sub(limit);
                                let references = groups
                                    .iter()
                                    .take(limit)
                                    .enumerate()
                                    .map(|(row, (path, refs))| {
                                        let root = root.clone();
                                        let path = path.clone();
                                        let line = refs[0].line;
                                        let column = refs[0].column;
                                        let lines = refs
                                            .iter()
                                            .take(4)
                                            .map(|entry| format!("L{}", entry.line))
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        let extra = refs.len().saturating_sub(4);
                                        let label = format!(
                                            "{}  {lines}{}",
                                            path.display(),
                                            if extra > 0 {
                                                format!(" +{extra}")
                                            } else {
                                                String::new()
                                            }
                                        );
                                        div()
                                            .id(("asset-reference", row))
                                            .p_1()
                                            .rounded(px(6.))
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                            .tooltip(icon_hint(
                                                refs.iter()
                                                    .map(|entry| {
                                                        format!(
                                                            "{}:L{}",
                                                            path.display(),
                                                            entry.line
                                                        )
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join(
                                                        "
",
                                                    ),
                                            ))
                                            .on_click(move |_, window, cx| {
                                                navigate_source(
                                                    &root, &path, line, column, window, cx,
                                                )
                                            })
                                            .child(label)
                                    })
                                    .collect::<Vec<_>>();
                                this.when(asset_inputs.len() == 3, |this| {
                                    this.child(property_input("ID", &asset_inputs[0]))
                                        .child(property_input("Type", &asset_inputs[1]))
                                        .child(property_row(
                                            "Path",
                                            asset.path.display().to_string(),
                                        ))
                                        .child(property_input("Tags", &asset_inputs[2]))
                                        .child(
                                            div()
                                                .flex()
                                                .gap_1()
                                                .items_center()
                                                .child(
                                                    Switch::new("rename-asset-file")
                                                        .checked(self.asset_rename_file)
                                                        .on_click(cx.listener(
                                                            |this, checked: &bool, _, cx| {
                                                                this.asset_rename_file = *checked;
                                                                cx.notify();
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(MUTED))
                                                        .child("Rename file to match"),
                                                ),
                                        )
                                })
                                .child(section_label("REFERENCES"))
                                .children(references)
                                .child(resource_trigger(
                                    root,
                                    ResourceTarget::Reference,
                                    index
                                        .asset_references
                                        .iter()
                                        .filter(|reference| reference.key == asset.key())
                                        .map(|reference| SourceOption {
                                            value: serde_json::to_string(&(
                                                reference.path.clone(),
                                                reference.line,
                                                reference.column,
                                            ))
                                            .expect("reference tuple serializes"),
                                            title: format!(
                                                "{}:L{}",
                                                reference.path.display(),
                                                reference.line
                                            )
                                            .into(),
                                            asset: None,
                                        })
                                        .collect(),
                                    "Browse references".into(),
                                    false,
                                    cx,
                                ))
                                .when(more > 0, |this| {
                                    this.child(
                                        div()
                                            .id("asset-more-references")
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .tooltip(icon_hint(
                                                groups
                                                    .keys()
                                                    .skip(limit)
                                                    .map(|path| path.display().to_string())
                                                    .collect::<Vec<_>>()
                                                    .join(
                                                        "
",
                                                    ),
                                            ))
                                            .child(format!("+{more} files")),
                                    )
                                })
                            })
                            .when(selected_assets.len() > 1, |this| {
                                this.child(property_row(
                                    "Selected",
                                    selected_assets.len().to_string(),
                                ))
                                .child(property_row(
                                    "Type",
                                    common_value(
                                        selected_assets
                                            .iter()
                                            .map(|asset| asset.kind.label().to_owned()),
                                    ),
                                ))
                                .child(property_row(
                                    "Folder",
                                    common_value(selected_assets.iter().map(|asset| {
                                        asset
                                            .path
                                            .parent()
                                            .unwrap_or(Path::new(""))
                                            .display()
                                            .to_string()
                                    })),
                                ))
                                .child(property_row(
                                    "Tags",
                                    common_value(
                                        selected_assets.iter().map(|asset| asset.tags.join(", ")),
                                    ),
                                ))
                                .child(property_input("Batch tags", &self.asset_batch_tags))
                                .child(
                                    div()
                                        .flex()
                                        .gap_1()
                                        .child(asset_action_button("add-tags", "Add").on_click(
                                            cx.listener({
                                                let root = root.clone();
                                                move |this, _, window, cx| {
                                                    this.asset_tags(&root, true, window, cx)
                                                }
                                            }),
                                        ))
                                        .child(
                                            asset_action_button("remove-tags", "Remove").on_click(
                                                cx.listener({
                                                    let root = root.clone();
                                                    move |this, _, window, cx| {
                                                        this.asset_tags(&root, false, window, cx)
                                                    }
                                                }),
                                            ),
                                        ),
                                )
                            })
                            .child(asset_action_button("delete-assets", "Delete").on_click(
                                cx.listener({
                                    let root = root.clone();
                                    move |this, _, window, cx| {
                                        this.delete_assets(&root, None, window, cx)
                                    }
                                }),
                            ))
                    })
                    .when_some(unmapped_preview.clone(), |this, preview| {
                        let path = preview.path;
                        this.child(section_label("UNMAPPED"))
                            .child(property_row("Path", path.display().to_string()))
                            .child(
                                div()
                                    .flex()
                                    .gap_1()
                                    .child(asset_action_button("remap-asset", "Remap").on_click(
                                        cx.listener({
                                            let root = root.clone();
                                            let path = path.clone();
                                            move |this, _, window, cx| {
                                                this.remap_asset(&root, &path, window, cx)
                                            }
                                        }),
                                    ))
                                    .child(
                                        asset_action_button("trash-unmapped", "Trash").on_click(
                                            cx.listener({
                                                let root = root.clone();
                                                move |this, _, window, cx| {
                                                    this.delete_assets(
                                                        &root,
                                                        Some(path.clone()),
                                                        window,
                                                        cx,
                                                    )
                                                }
                                            }),
                                        ),
                                    ),
                            )
                    })
                    .when(
                        asset_selection.is_empty() && unmapped_preview.is_none() && has_selection,
                        |this| {
                            this.when(source_key.is_none() && inputs.is_empty(), |this| {
                                let batch = cx
                                    .global::<EditorDocuments>()
                                    .block_selection(root)
                                    .and_then(|(path, starts)| {
                                        let source =
                                            cx.global::<EditorDocuments>().source(root, path)?;
                                        Some(batch_block_fields(&source, starts))
                                    })
                                    .unwrap_or_default();
                                this.child(selection_summary(root, &index, cx)).when(
                                    !batch.is_empty(),
                                    |this| {
                                        this.child(section_label("BATCH EDIT"))
                                            .child(div().flex().flex_wrap().gap_1().children(
                                                batch.into_iter().enumerate().map(
                                                    |(i, (name, value))| {
                                                        let active =
                                                            self.batch_block_field.as_ref()
                                                                == Some(&name);
                                                        div()
                                                            .id(("batch-field", i))
                                                            .px_2()
                                                            .py_1()
                                                            .rounded(px(5.))
                                                            .text_xs()
                                                            .bg(rgb(if active {
                                                                PRIMARY_DIM
                                                            } else {
                                                                SURFACE
                                                            }))
                                                            .text_color(rgb(if active {
                                                                PRIMARY
                                                            } else {
                                                                MUTED
                                                            }))
                                                            .cursor_pointer()
                                                            .child(format!(
                                                                "{name} · {}",
                                                                if value.is_empty() {
                                                                    "Default"
                                                                } else {
                                                                    &value
                                                                }
                                                            ))
                                                            .on_click(cx.listener(
                                                                move |this, _, window, cx| {
                                                                    this.batch_block_field =
                                                                        Some(name.clone());
                                                                    this.batch_block_input.update(
                                                                        cx,
                                                                        |input, cx| {
                                                                            input.set_value(
                                                                                if value == "Mixed"
                                                                                {
                                                                                    ""
                                                                                } else {
                                                                                    &value
                                                                                },
                                                                                window,
                                                                                cx,
                                                                            )
                                                                        },
                                                                    );
                                                                    cx.notify();
                                                                },
                                                            ))
                                                    },
                                                ),
                                            ))
                                            .child(property_input("Value", &self.batch_block_input))
                                            .child(
                                                asset_action_button("apply-batch-block", "Apply")
                                                    .on_click(cx.listener({
                                                        let root = root.clone();
                                                        move |this, _, window, cx| {
                                                            this.apply_batch_block_field(
                                                                &root, window, cx,
                                                            )
                                                        }
                                                    })),
                                            )
                                    },
                                )
                            })
                            .when(inputs.len() == 3 && text_selects.len() == 2, |this| {
                                this.child(section_label("TEXT"))
                                    .child(
                                        div()
                                            .w_full()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(section_label("Speaker"))
                                            .child(resource_trigger(
                                                root,
                                                ResourceTarget::Speaker(
                                                    self.inspector_key
                                                        .clone()
                                                        .expect("selected text Inspector"),
                                                ),
                                                resource::speaker_options(&index),
                                                inputs[0].read(cx).value().to_string(),
                                                false,
                                                cx,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(section_label("Voice"))
                                            .child(resource_trigger(
                                                root,
                                                ResourceTarget::Voice(
                                                    self.inspector_key
                                                        .clone()
                                                        .expect("Text Inspector key"),
                                                ),
                                                voice_resource_options(root, &index),
                                                self.inspector_key
                                                    .as_ref()
                                                    .and_then(|key| key.metadata.voice.clone())
                                                    .unwrap_or_default(),
                                                false,
                                                cx,
                                            )),
                                    )
                                    .child(property_input("Stable ID", &inputs[2]))
                                    .child(render_text_ending(
                                        root,
                                        self.inspector_key.as_ref().expect("Text Inspector key"),
                                        &self.text_lifetime_inputs,
                                        cx,
                                    ))
                            })
                            .when_some(source_key, |this, key| {
                                this.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            Icon::new(block_card_icon(&key.kind, &key.command))
                                                .small()
                                                .text_color(rgb(PRIMARY)),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(INK))
                                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                                .child(block_card_label(&key.kind, &key.command)),
                                        ),
                                )
                                .child(
                                    SourceInspectorView {
                                        root,
                                        key: &key,
                                        inputs: &source_inputs,
                                        texts: &source_texts,
                                        sliders: &source_sliders,
                                        selects: &source_selects,
                                        effect: source_effect,
                                        position_bounds: &self.source_position_bounds,
                                        position_draft: self.source_position_draft,
                                    }
                                    .render(cx),
                                )
                            })
                        },
                    )
                    .when(asset_selection.is_empty() && !has_selection, |this| {
                        this.child(section_label("WORKSPACE"))
                            .child(property_row("Path", root.display().to_string()))
                            .child(property_row("Text files", file_count.to_string()))
                            .child(property_row("Open documents", document_count.to_string()))
                    });
                div()
                    .size_full()
                    .relative()
                    .child(vertical_overflow_view(
                        "inspector-scroll",
                        &self.view_scroll,
                        content,
                    ))
                    .when_some(self.render_resource_picker(window, cx), |this, picker| {
                        this.child(picker)
                    })
                    .into_any_element()
            }
            PanelContent::Search { root } => {
                let root = root.clone();
                self.render_search(&root, window, cx)
            }
            PanelContent::Assets { root } => {
                let root = root.clone();
                let index = cx.global::<EditorDocuments>().authoring(&root);
                render_assets(&root, &index, self, window, cx)
            }
            PanelContent::AssetPreview { root } => {
                let documents = cx.global::<EditorDocuments>();
                let selection = documents.asset_preview(root);
                let count = documents.asset_selection(root).len();
                render_asset_preview(root, selection, count, cx)
            }
            PanelContent::Characters { root } => {
                let index = cx.global::<EditorDocuments>().authoring(root);
                let inputs = self.tool_inputs.clone();
                let content =
                    div()
                        .flex()
                        .flex_col()
                        .p_2()
                        .gap_2()
                        .child(section_label("CHARACTER MANIFEST"))
                        .when(inputs.len() == 3, |this| {
                            this.child(tool_input(&inputs[0]))
                                .child(tool_input(&inputs[1]))
                                .child(tool_input(&inputs[2]))
                                .child(tool_action("Add character").on_click(cx.listener(
                                    |this, _, window, cx| this.add_character(window, cx),
                                )))
                        })
                        .child(
                            div().flex().flex_col().gap_1().children(
                                index.characters.iter().cloned().enumerate().map(
                                    |(row, character)| {
                                        div()
                                            .id(("character-row", row))
                                            .p_2()
                                            .rounded(px(7.))
                                            .bg(rgb(PANEL))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(rgb(INK))
                                                    .child(character.name),
                                            )
                                            .child(div().text_xs().text_color(rgb(MUTED)).child(
                                                format!(
                                                    "{}{}",
                                                    character.id,
                                                    character
                                                        .color
                                                        .map(|color| format!(" · {color}"))
                                                        .unwrap_or_default()
                                                ),
                                            ))
                                    },
                                ),
                            ),
                        );
                vertical_overflow_view("character-scroll", &self.view_scroll, content)
            }
            PanelContent::Scenes { root } => {
                let index = cx.global::<EditorDocuments>().authoring(root);
                let root_for_rows = root.clone();
                let content = div()
                    .flex()
                    .flex_col()
                    .p_2()
                    .gap_2()
                    .child(section_label("SCENE DECLARATIONS"))
                    .child(
                        div().flex().flex_col().gap_1().children(
                            index
                                .scenes
                                .iter()
                                .cloned()
                                .enumerate()
                                .map(|(row, scene)| {
                                    let root = root_for_rows.clone();
                                    let path = scene.path.clone();
                                    let line = scene.line;
                                    div()
                                        .id(("scene-row", row))
                                        .p_2()
                                        .rounded(px(7.))
                                        .bg(rgb(PANEL))
                                        .cursor_pointer()
                                        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                        .on_click(move |_, window, cx| {
                                            navigate_source(&root, &path, line, 1, window, cx)
                                        })
                                        .child(
                                            div().text_sm().text_color(rgb(INK)).child(scene.name),
                                        )
                                        .child(div().text_xs().text_color(rgb(MUTED)).child(
                                            format!("{}:{}", scene.path.display(), scene.line),
                                        ))
                                }),
                        ),
                    );
                vertical_overflow_view("scene-scroll", &self.view_scroll, content)
            }
            PanelContent::Problems { root } => render_problems(root, &self.view_scroll, cx),
            PanelContent::Performance { controller, .. } => {
                performance::render(controller, &self.view_scroll)
            }
            PanelContent::Build { root } => build::render(root, &self.view_scroll, cx),
            PanelContent::Output { root, file_count } => {
                let content = div()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_2()
                    .font_family(mono)
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(output_line("READY", SUCCESS, root.display().to_string()))
                    .child(output_line(
                        "INDEX",
                        PRIMARY,
                        format!("{file_count} text files discovered"),
                    ))
                    .when_some(
                        cx.global::<EditorDocuments>()
                            .notice(root)
                            .map(str::to_owned),
                        |this, notice| this.child(output_line("STATUS", PRIMARY, notice)),
                    );
                vertical_overflow_view("output-scroll", &self.view_scroll, content)
            }
        };
        let resource_popup = self.render_resource_picker(window, cx);
        div()
            .track_focus(&self.focus)
            .capture_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape"
                        && matches!(this.block_drag, blocks::BlockDragState::Dragging(_))
                    {
                        this.cancel_block_drag(window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .capture_action(cx.listener(Self::accept_source_suggestion))
            .capture_action(cx.listener(Self::backspace_empty_text))
            .capture_action(cx.listener(Self::delete_empty_text))
            .when(self.resource_picker.is_some(), |this| {
                this.key_context("KeineResourcePicker")
            })
            .when(
                self.document_mode == DocumentMode::Block && self.resource_picker.is_none(),
                |this| this.key_context("KeineBlockView"),
            )
            .when(
                matches!(self.content, PanelContent::Explorer { .. }),
                |this| this.key_context("KeineExplorer"),
            )
            .on_action(cx.listener(Self::toggle_block_picker))
            .on_action(cx.listener(Self::block_picker_next))
            .on_action(cx.listener(Self::block_picker_previous))
            .on_action(cx.listener(Self::block_picker_left))
            .on_action(cx.listener(Self::block_picker_right))
            .on_action(cx.listener(Self::accept_block_picker))
            .on_action(cx.listener(Self::close_block_picker))
            .on_action(cx.listener(Self::resource_picker_next))
            .on_action(cx.listener(Self::resource_picker_previous))
            .on_action(cx.listener(Self::accept_resource_picker))
            .on_action(cx.listener(Self::dismiss_resource_picker))
            .on_action(cx.listener(Self::begin_text_block))
            .on_action(cx.listener(Self::copy_selected_blocks))
            .on_action(cx.listener(Self::paste_blocks))
            .on_action(cx.listener(Self::delete_selected_blocks))
            .on_action(cx.listener(Self::move_selected_blocks_up))
            .on_action(cx.listener(Self::move_selected_blocks_down))
            .on_action(cx.listener(Self::undo_blocks))
            .on_action(cx.listener(Self::redo_blocks))
            .on_action(cx.listener(Self::undo_files))
            .on_action(cx.listener(Self::redo_files))
            .on_action(cx.listener(Self::reload_document))
            .size_full()
            .text_color(rgb(INK))
            .child(body)
            .children(resource_popup)
    }
}

pub(super) fn render_problems(
    root: &Path,
    scroll_handle: &ScrollHandle,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let index = cx.global::<EditorDocuments>().authoring(root);
    let runtime = cx.global::<EditorDocuments>().runtime_diagnostics(root);
    let root = root.to_owned();
    let authoring_rows = index.problems.iter().cloned().enumerate().map({
        let root = root.clone();
        move |(row, problem)| {
            let root = root.clone();
            let path = problem.path.clone();
            let line = problem.line;
            let column = problem.column;
            problem_row(
                ("authoring-problem", row),
                match problem.severity {
                    ProblemSeverity::Warning => 0xd2aa62,
                    ProblemSeverity::Error => 0xdb7780,
                },
                problem.path.display().to_string(),
                problem.line,
                problem.column,
                problem.message,
            )
            .on_click(move |_, window, cx| navigate_source(&root, &path, line, column, window, cx))
        }
    });
    let runtime_rows = runtime.into_iter().enumerate().map({
        let root = root.clone();
        move |(row, diagnostic)| {
            let root = root.clone();
            let path = diagnostic.path.clone();
            let line = diagnostic.line;
            let column = diagnostic.column;
            problem_row(
                ("runtime-problem", row),
                match diagnostic.level {
                    keine_authoring::DiagnosticLevel::Warning => 0xd2aa62,
                    keine_authoring::DiagnosticLevel::Error => 0xdb7780,
                },
                diagnostic.path.display().to_string(),
                diagnostic.line,
                diagnostic.column,
                diagnostic.message,
            )
            .on_click(move |_, window, cx| navigate_source(&root, &path, line, column, window, cx))
        }
    });
    let content = div()
        .flex()
        .flex_col()
        .p_2()
        .gap_1()
        .child(section_label("PARSE · VALIDATION · RUNTIME"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(authoring_rows)
                .children(runtime_rows),
        );
    vertical_overflow_view("problem-scroll", scroll_handle, content)
}

pub(super) fn problem_row(
    id: (&'static str, usize),
    color: u32,
    path: String,
    line: usize,
    column: usize,
    message: String,
) -> Stateful<Div> {
    div()
        .id(id)
        .p_2()
        .rounded(px(7.))
        .bg(rgb(PANEL))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .child(div().text_xs().text_color(rgb(color)).child(message))
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(format!("{path}:{line}:{column}")),
        )
}
