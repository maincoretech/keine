use super::*;

impl Render for WorkbenchPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !cx.has_active_drag() {
            self.file_drop_target = None;
            self.block_drop_target = None;
            self.block_dragging = None;
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
        let mono = Theme::global(cx).mono_font_family.clone();
        let body = match &self.content {
            PanelContent::Explorer { root, files } => {
                let project_root = root.clone();
                let files = files.clone();
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
                            if self.file_collapsed.contains(path) {
                                return false;
                            }
                            ancestor = path.parent();
                        }
                        true
                    })
                    .take(800)
                    .cloned()
                    .collect::<Vec<_>>();
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
                        let reveal_root = project_root.clone();
                        let reveal_path = path.clone();
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
                                "file-context-reveal",
                                AssetIconName::ExternalLink,
                                "Reveal",
                                false,
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                reveal_workspace_path(&reveal_root, &reveal_path);
                                this.close_file_context_menu(window, cx);
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
                                    .gap_1()
                                    .px_2()
                                    .id("explorer-files")
                                    .children(visible.into_iter().enumerate().map(
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
                                            let row_selected = selected.as_ref() == Some(&relative);
                                            let collapsed = self.file_collapsed.contains(&relative);
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
                                                        px(30.)
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
                                                .h(px(26.))
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
                                                            if !this
                                                                .file_collapsed
                                                                .insert(click_relative.clone())
                                                            {
                                                                this.file_collapsed
                                                                    .remove(&click_relative);
                                                            }
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
                                                        .child(name),
                                                );
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
                                                                    .h(px(26.))
                                                                    .rounded(px(8.))
                                                                    .bg(rgb(PRIMARY_DIM)),
                                                            ),
                                                    )
                                                })
                                        },
                                    )),
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
                let text_root = root.clone();
                let text_relative = relative.clone();
                let text_editor = editor.clone();
                let header = eiyashou.then(|| {
                    let text_selected = mode == DocumentMode::Text;
                    let block_selected = mode == DocumentMode::Block;
                    div()
                        .h(px(34.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_1()
                        .px_2()
                        .bg(rgb(CHROME))
                        .when(block_selected, |header| {
                            header.child(
                                file_action_icon("scene-new", AssetIconName::Plus, "Add scene")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.begin_scene_edit(SceneEditMode::New, window, cx)
                                    })),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .child(document_mode_button("Text", text_selected).on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.document_mode = DocumentMode::Text;
                                        cx.global_mut::<EditorDocuments>()
                                            .clear_asset_selection(&text_root);
                                        if let Some((_, line, column)) = cx
                                            .global::<EditorDocuments>()
                                            .selection(&text_root)
                                            .filter(|(path, _, _)| path == &text_relative)
                                        {
                                            let line = *line;
                                            let column = *column;
                                            text_editor.update(cx, |editor, cx| {
                                                editor.set_cursor_position(
                                                    Position::new(line as u32, column as u32),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }
                                        cx.notify();
                                    }),
                                ))
                                .child(document_mode_button("Blocks", block_selected).on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.rebuild_visual_editors(window, cx);
                                        this.document_mode = DocumentMode::Block;
                                        this.block_scroll_pending = true;
                                        if let PanelContent::Document { root, relative, .. } =
                                            &this.content
                                        {
                                            cx.global_mut::<EditorDocuments>().set_block_selection(
                                                root,
                                                relative.clone(),
                                                this.selected_blocks.iter().copied().collect(),
                                            );
                                        }
                                        cx.notify();
                                    }),
                                )),
                        )
                });
                let body = if eiyashou && mode == DocumentMode::Block {
                    render_block_projection(
                        BlockProjectionView {
                            root,
                            relative,
                            document: document.as_ref().unwrap(),
                            editors: &self.block_text_editors,
                            collapsed_scenes: &self.collapsed_scenes,
                            selected_blocks: &self.selected_blocks,
                            draft_text: self.draft_text.as_ref(),
                            drop_target: self.block_drop_target,
                            dragging: self.block_dragging.as_ref(),
                            scroll_handle: &self.view_scroll,
                            scroll_anchor: &self.block_scroll_anchor,
                            scroll_pending: self.block_scroll_pending,
                            scene_edit: self.scene_edit.as_ref(),
                            scene_name_input: &self.scene_name_input,
                        },
                        window,
                        cx,
                    )
                } else {
                    let editor_for_fade = editor.clone();
                    let selection_root = root.clone();
                    div()
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
                    .when_some(header, |this, header| this.child(header))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(body)
                            .when_some(picker, |this, picker| this.child(picker))
                            .when_some(scene_menu, |this, menu| this.child(menu)),
                    )
                    .into_any_element()
            }
            PanelContent::Preview { root, controller } => {
                let snapshot = controller.snapshot();
                let running = matches!(
                    snapshot.lifecycle,
                    PreviewLifecycle::Running | PreviewLifecycle::Paused
                );
                let status = match &snapshot.lifecycle {
                    PreviewLifecycle::Off => String::new(),
                    PreviewLifecycle::Starting => "Starting…".to_owned(),
                    PreviewLifecycle::Running => format!(
                        "Live · frame {} · dropped {}",
                        snapshot.frame_stats.published, snapshot.frame_stats.overwritten
                    ),
                    PreviewLifecycle::Paused => "Paused while hidden".to_owned(),
                    PreviewLifecycle::Failed(error) => format!("Failed · {error}"),
                };
                let start = controller.clone();
                let start_root = root.clone();
                let stop = controller.clone();
                let input = controller.clone();
                let keyboard_input = controller.clone();
                let bounds = self.preview_bounds.clone();
                let surface_bounds = self.preview_bounds.clone();
                let preview_focus = self.focus.clone();
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .bg(rgb(CANVAS))
                    .child(
                        div()
                            .h(px(38.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .bg(rgb(CHROME))
                            .child(div().flex_1())
                            .when(!status.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(match snapshot.lifecycle {
                                            PreviewLifecycle::Failed(_) => 0xdb7780,
                                            PreviewLifecycle::Running => SUCCESS,
                                            _ => MUTED,
                                        }))
                                        .child(status),
                                )
                            })
                            .child(if running {
                                preview_transport_button(true).on_click(move |_, _, cx| {
                                    stop.stop();
                                    cx.refresh_windows();
                                })
                            } else {
                                preview_transport_button(false).on_click(move |_, _, cx| {
                                    for (path, contents) in cx
                                        .global::<EditorDocuments>()
                                        .preview_documents(&start_root)
                                    {
                                        start.apply_snapshot(path, contents);
                                    }
                                    start.start();
                                    cx.refresh_windows();
                                })
                            }),
                    )
                    .child(
                        div()
                            .id("preview-surface")
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgb(0x050607))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                                preview_focus.focus(window, cx);
                                let bounds =
                                    *bounds.lock().expect("preview surface bounds lock poisoned");
                                let Some(bounds) = bounds else {
                                    return;
                                };
                                let local_x = f32::from(event.position.x - bounds.origin.x);
                                let local_y = f32::from(event.position.y - bounds.origin.y);
                                if let Some((x, y)) = map_preview_point(
                                    f32::from(bounds.size.width),
                                    f32::from(bounds.size.height),
                                    local_x,
                                    local_y,
                                ) {
                                    input.input(keine_authoring::PreviewInput::PointerPressed {
                                        x,
                                        y,
                                    });
                                }
                            })
                            .on_key_down(move |event, _, cx| {
                                if !event.keystroke.modifiers.control
                                    && !event.keystroke.modifiers.alt
                                    && !event.keystroke.modifiers.platform
                                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                                {
                                    keyboard_input.input(keine_authoring::PreviewInput::Advance);
                                    cx.stop_propagation();
                                }
                            })
                            .when_some(self.preview_image.clone(), |this, image| {
                                this.child(img(image).size_full().object_fit(ObjectFit::Contain))
                            })
                            .when(self.preview_image.is_none(), |this| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(MUTED))
                                        .child("Start Preview to render the current project"),
                                )
                            })
                            .child(
                                canvas(
                                    move |surface, _, _| {
                                        *surface_bounds
                                            .lock()
                                            .expect("preview surface bounds lock poisoned") =
                                            Some(surface);
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .inset_0(),
                            ),
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
                let source_key = self.source_inspector_key.clone();
                let source_inputs = self.source_inspector_inputs.clone();
                let asset_inputs = self.asset_inspector_inputs.clone();
                let content = div()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_3()
                    .when(!selected_assets.is_empty(), |this| {
                        this.child(section_label("ASSET"))
                            .when(selected_assets.len() == 1, |this| {
                                let asset = selected_assets[0];
                                let references = index
                                    .asset_references
                                    .iter()
                                    .filter(|reference| reference.key == asset.key())
                                    .collect::<Vec<_>>();
                                this.when(asset_inputs.len() == 3, |this| {
                                    this.child(property_input("ID", &asset_inputs[0]))
                                        .child(property_input("Type", &asset_inputs[1]))
                                        .child(property_row(
                                            "Path",
                                            asset.path.display().to_string(),
                                        ))
                                        .child(property_input("Tags", &asset_inputs[2]))
                                })
                                .child(section_label("REFERENCES"))
                                .children(
                                    references.into_iter().enumerate().map(|(row, reference)| {
                                        let root = root.clone();
                                        let path = reference.path.clone();
                                        let line = reference.line;
                                        let column = reference.column;
                                        div()
                                            .id(("asset-reference", row))
                                            .p_1()
                                            .rounded(px(6.))
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                            .on_click(move |_, window, cx| {
                                                navigate_source(
                                                    &root, &path, line, column, window, cx,
                                                )
                                            })
                                            .child(format!(
                                                "{}:{}",
                                                reference.path.display(),
                                                reference.line
                                            ))
                                    }),
                                )
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
                            })
                    })
                    .when(asset_selection.is_empty() && has_selection, |this| {
                        this.child(section_label("SELECTION"))
                            .child(selection_summary(root, &index, cx))
                            .when(inputs.len() == 3, |this| {
                                this.child(section_label("TEXT"))
                                    .child(property_input("Speaker", &inputs[0]))
                                    .child(property_input("Voice", &inputs[1]))
                                    .child(property_input("Stable ID", &inputs[2]))
                            })
                            .when_some(source_key, |this, key| {
                                this.child(section_label("PROPERTIES")).children(
                                    key.fields.iter().zip(source_inputs.iter()).map(
                                        |(field, input)| {
                                            property_input(source_field_label(&key, field), input)
                                        },
                                    ),
                                )
                            })
                    })
                    .when(asset_selection.is_empty() && !has_selection, |this| {
                        this.child(section_label("WORKSPACE"))
                            .child(property_row("Path", root.display().to_string()))
                            .child(property_row("Text files", file_count.to_string()))
                            .child(property_row("Open documents", document_count.to_string()))
                    });
                vertical_overflow_view("inspector-scroll", &self.view_scroll, content)
            }
            PanelContent::Assets { root } => {
                let root = root.clone();
                let index = cx.global::<EditorDocuments>().authoring(&root);
                render_assets(&root, &index, self, cx)
            }
            PanelContent::Characters { root } => {
                let index = cx.global::<EditorDocuments>().authoring(root);
                let inputs = self.tool_inputs.clone();
                let content = div()
                    .flex()
                    .flex_col()
                    .p_2()
                    .gap_2()
                    .child(section_label("CHARACTER MANIFEST"))
                    .when(inputs.len() == 3, |this| {
                        this.child(tool_input(&inputs[0]))
                            .child(tool_input(&inputs[1]))
                            .child(tool_input(&inputs[2]))
                            .child(tool_action("Add character").on_click(
                                cx.listener(|this, _, window, cx| this.add_character(window, cx)),
                            ))
                    })
                    .child(
                        div().flex().flex_col().gap_1().children(
                            index
                                .characters
                                .into_iter()
                                .enumerate()
                                .map(|(row, character)| {
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
                                }),
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
                    .child(div().flex().flex_col().gap_1().children(
                        index.scenes.into_iter().enumerate().map(|(row, scene)| {
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
                                .child(div().text_sm().text_color(rgb(INK)).child(scene.name))
                                .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                                    "{}:{}",
                                    scene.path.display(),
                                    scene.line
                                )))
                        }),
                    ));
                vertical_overflow_view("scene-scroll", &self.view_scroll, content)
            }
            PanelContent::Problems { root } => render_problems(root, &self.view_scroll, cx),
            PanelContent::Performance { controller, .. } => {
                render_performance(controller, &self.timeline, &self.view_scroll)
            }
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
                        |this, notice| this.child(output_line("EDIT", PRIMARY, notice)),
                    );
                vertical_overflow_view("output-scroll", &self.view_scroll, content)
            }
        };
        div()
            .track_focus(&self.focus)
            .when(self.document_mode == DocumentMode::Block, |this| {
                this.key_context("KeineBlockView")
            })
            .when(
                matches!(self.content, PanelContent::Explorer { .. }),
                |this| this.key_context("KeineExplorer"),
            )
            .on_action(cx.listener(Self::toggle_block_picker))
            .on_action(cx.listener(Self::block_picker_next))
            .on_action(cx.listener(Self::block_picker_previous))
            .on_action(cx.listener(Self::accept_block_picker))
            .on_action(cx.listener(Self::close_block_picker))
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
            .size_full()
            .text_color(rgb(INK))
            .child(body)
    }
}
