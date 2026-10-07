//! Panel construction and event subscriptions.
use super::*;

impl WorkbenchPanel {
    pub(in crate::app) fn from_payload(
        payload: PanelPayload,
        window: &mut Window,
        cx: &mut App,
    ) -> io::Result<Entity<Self>> {
        let saved_view = match &payload {
            PanelPayload::Document { view, .. } => Some(*view),
            _ => None,
        };
        let content = PanelContent::from_payload(payload, window, cx)?;
        let registration = match &content {
            PanelContent::Document { root, relative, .. } => Some((root.clone(), relative.clone())),
            _ => None,
        };
        let tool_registration = match &content {
            PanelContent::Explorer { root, .. } => Some((root.clone(), EXPLORER_PANEL)),
            PanelContent::Inspector { root, .. } => Some((root.clone(), INSPECTOR_PANEL)),
            PanelContent::Search { root } => Some((root.clone(), SEARCH_PANEL)),
            PanelContent::Assets { root } => Some((root.clone(), ASSETS_PANEL)),
            PanelContent::AssetPreview { root } => Some((root.clone(), ASSET_PREVIEW_PANEL)),
            PanelContent::Characters { root } => Some((root.clone(), CHARACTERS_PANEL)),
            PanelContent::Scenes { root } => Some((root.clone(), SCENES_PANEL)),
            PanelContent::Problems { root } => Some((root.clone(), PROBLEMS_PANEL)),
            PanelContent::Performance { root, .. } => Some((root.clone(), PERFORMANCE_PANEL)),
            PanelContent::Build { root } => Some((root.clone(), BUILD_PANEL)),
            PanelContent::Output { root, .. } => Some((root.clone(), OUTPUT_PANEL)),
            _ => None,
        };
        let panel = cx.new(|cx| {
            let block_picker_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Search blocks"));
            let file_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
            let scene_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Scene"));
            let asset_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
            let view_scroll = ScrollHandle::new();
            let block_scroll_anchor = ScrollAnchor::for_handle(view_scroll.clone());
            let syntax_marks = match &content {
                PanelContent::Document {
                    relative,
                    document: Some(_),
                    editor,
                    ..
                } if relative
                    .extension()
                    .is_some_and(|extension| extension == "shou") =>
                {
                    Some(editor.update(cx, |editor, cx| {
                        editor.create_decorations_collection(Vec::new(), cx)
                    }))
                }
                _ => None,
            };
            let mut panel = Self {
                content,
                focus: cx.focus_handle(),
                view_scroll,
                project_search: None,
                _subscriptions: Vec::new(),
                resource_picker: None,
                picker: crate::app::blocks::PickerState {
                    picker_drag: None,
                    picker_row_bounds: Rc::new(RefCell::new(HashMap::new())),
                    picker_scroll: ScrollHandle::new(),
                    block_picker_open: false,
                    block_picker_closing: false,
                    block_picker_epoch: 0,
                    block_picker_index: 0,
                    block_picker_category: Some("Favorites"),
                    block_picker_customize: false,
                    block_picker_input: block_picker_input.clone(),
                },
                inspector: crate::app::inspector::InspectorState {
                    inspector_key: None,
                    inspector_inputs: Vec::new(),
                    text_lifetime_inputs: Vec::new(),
                    inspector_selects: Vec::new(),
                    inspector_subscriptions: Vec::new(),
                    source_inspector_key: None,
                    source_inspector_inputs: Vec::new(),
                    source_inspector_texts: Vec::new(),
                    source_inspector_sliders: HashMap::new(),
                    source_inspector_selects: HashMap::new(),
                    source_inspector_subscriptions: Vec::new(),
                    source_inspector_effect: None,
                    source_position_bounds: Rc::new(RefCell::new(Bounds::default())),
                    source_position_draft: None,
                    asset_inspector_key: None,
                    asset_inspector_inputs: Vec::new(),
                    asset_rename_file: true,
                    batch_block_field: None,
                    batch_block_input: cx
                        .new(|cx| InputState::new(window, cx).placeholder("Value")),
                    asset_batch_tags: cx.new(|cx| {
                        InputState::new(window, cx).placeholder("Tags, separated by commas")
                    }),
                    asset_inspector_subscriptions: Vec::new(),
                },
                assets: crate::app::resource::AssetsState {
                    asset_search: asset_search.clone(),
                    asset_kind: None,
                    asset_folder: None,
                    asset_sort: AssetSort::Name,
                    asset_tag: None,
                    asset_status: crate::authoring::AssetStatus::All,
                    asset_size: crate::authoring::AssetSize::All,
                    asset_modified: None,
                    asset_grid: None,
                    asset_large: false,
                    asset_statistics_expanded: false,
                    asset_browser: RefCell::new(crate::app::resource::browse::Cache::default()),
                    asset_thumbnails: crate::app::resource::thumbnail::Thumbnails::new(cx),
                    asset_unmapped: false,
                    asset_anchor: None,
                    asset_filter_menu: None,
                    asset_filter_epoch: 0,
                },
                explorer: crate::app::files::ExplorerState {
                    file_selection: None,
                    file_drop_target: None,
                    file_clipboard: None,
                    file_history: FileHistory::default(),
                    file_edit: None,
                    file_name_input: file_name_input.clone(),

                    file_progress: None,
                    file_context_menu: None,
                    file_context_epoch: 0,
                },
                characters: crate::app::characters::CharactersState {
                    tool_inputs: Vec::new(),
                    character_id: None,
                    character_expression: None,
                    character_script: None,
                    character_images: None,
                    character_image_options: Vec::new(),
                },
                document: crate::app::document::DocumentState {
                    document_mode: saved_view.map_or(DocumentMode::Text, |view| view.mode),
                    last_text_cursor: None,
                    text_scroll_pending: saved_view
                        .is_some_and(|view| view.mode == DocumentMode::Text),
                    block_text_editors: Vec::new(),
                    collapsed_scenes: HashSet::new(),
                    selected_blocks: HashSet::new(),
                    block_selection_anchor: None,
                    draft_text: None,
                    block_drag: Default::default(),
                    block_row_bounds: Rc::new(RefCell::new(HashMap::new())),
                    block_row_positions: RefCell::new(HashMap::new()),
                    block_insertion_target: None,
                    block_context_menu: None,
                    block_context_epoch: 0,
                    block_scroll_anchor,
                    block_scroll_pending: false,
                    minimap_navigation: minimap::Navigation::default(),
                    text_minimap: text_minimap::TextMinimap::default(),
                    recovery_epoch: 0,
                    syntax_check: None,
                    diagnostic_index: Default::default(),
                    syntax_marks,
                    visual_subscriptions: Vec::new(),
                    inline_block_controls: HashMap::new(),
                    block_visible: HashSet::new(),
                    block_heights: HashMap::new(),
                    block_layout: RefCell::new(crate::app::blocks::layout::Cache::default()),
                    block_height_revision: 0,
                    block_text_refresh_pending: false,
                    scene_edit: None,
                    scene_name_input: scene_name_input.clone(),

                    scene_context_menu: None,
                    scene_context_epoch: 0,
                },
            };
            if matches!(panel.content, PanelContent::Search { .. }) {
                panel.install_search(window, cx);
            }
            if let PanelContent::Performance { controller, .. } = &panel.content {
                let controller = controller.clone();
                cx.spawn_in(window, async move |this, cx| {
                    let mut revision = controller.performance().revision;
                    loop {
                        cx.background_executor()
                            .timer(crate::preview::performance::SAMPLE_INTERVAL)
                            .await;
                        let latest = controller.performance().revision;
                        if this
                            .update_in(cx, |_, _, cx| {
                                if latest != revision {
                                    cx.notify();
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                        revision = latest;
                    }
                })
                .detach();
            }
            panel._subscriptions.push(cx.subscribe(
                &asset_search,
                |panel: &mut WorkbenchPanel, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        panel
                            .view_scroll
                            .set_offset(gpui_kit::point(px(0.), px(0.)));
                        cx.notify();
                    }
                },
            ));
            panel._subscriptions.push(cx.subscribe(
                &block_picker_input,
                |panel: &mut WorkbenchPanel, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        panel.picker.block_picker_index = 0;
                        cx.notify();
                    }
                },
            ));
            panel._subscriptions.push(cx.subscribe_in(
                &file_name_input,
                window,
                |_, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        cx.defer_in(window, |panel, window, cx| {
                            panel.commit_file_edit(window, cx)
                        });
                    }
                },
            ));
            panel._subscriptions.push(cx.subscribe_in(
                &scene_name_input,
                window,
                |_, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        cx.defer_in(window, |panel, window, cx| {
                            panel.commit_scene_edit(window, cx)
                        });
                    }
                },
            ));
            if let PanelContent::Document { editor, .. } = &panel.content {
                panel._subscriptions.push(cx.subscribe(
                    editor,
                    |panel, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            panel.document.text_minimap.invalidate();
                            cx.notify();
                        }
                    },
                ));
            }
            if let PanelContent::Document {
                root,
                relative,
                document,
                editor,
            } = &panel.content
            {
                let root = root.clone();
                let relative = relative.clone();
                let editor_for_selection = editor.clone();
                let root_for_selection = root.clone();
                let relative_for_selection = relative.clone();
                let selection_window = window.window_handle();
                panel
                    ._subscriptions
                    .push(cx.observe(editor, move |panel, _, cx| {
                        // Blocks mode owns the source selection; the hidden
                        // text editor's stale caret must not seek Preview.
                        if panel.document.document_mode == DocumentMode::Block {
                            return;
                        }
                        use gpui_kit::EntityInputHandler;
                        let composing = cx
                            .update_window(selection_window, |_, window, cx| {
                                editor_for_selection.update(cx, |editor, cx| {
                                    editor.marked_text_range(window, cx).is_some()
                                })
                            })
                            .unwrap_or(false);
                        if composing {
                            return;
                        }
                        let position = editor_for_selection.read(cx).cursor_position();
                        let cursor = (position.line as usize, position.character as usize);
                        if panel.document.last_text_cursor == Some(cursor) {
                            return;
                        }
                        panel.document.last_text_cursor = Some(cursor);
                        cx.global_mut::<EditorDocuments>().set_selection(
                            &root_for_selection,
                            relative_for_selection.clone(),
                            cursor.0,
                            cursor.1,
                        );
                        cx.global_mut::<EditorDocuments>()
                            .clear_block_selection(&root_for_selection);
                        let disabled = cx
                            .global::<EditorDocuments>()
                            .source(&root_for_selection, &relative_for_selection)
                            .and_then(|source| {
                                let projection = cx.global::<EditorDocuments>().projection(
                                    &root_for_selection,
                                    &relative_for_selection,
                                    &source,
                                );
                                block_at_position(&projection, &source, cursor.0, cursor.1)
                            })
                            .is_some_and(|(_, block)| block.disabled);
                        if !disabled
                            && let Ok(preview) = cx
                                .global_mut::<EditorDocuments>()
                                .preview(&root_for_selection)
                        {
                            preview.set_cursor(
                                relative_for_selection.clone(),
                                cursor.0 + 1,
                                cursor.1 + 1,
                            );
                        }
                        cx.refresh_windows();
                    }));
                if let Some(document) = document {
                    if relative
                        .extension()
                        .is_some_and(|extension| extension == "shou")
                    {
                        panel.document.syntax_check = Some(completion::schedule_syntax_check(
                            editor.clone(),
                            panel.document.syntax_marks.clone(),
                            (
                                cx.global::<EditorDocuments>().authoring(&root),
                                relative.clone(),
                            ),
                            window,
                            cx,
                        ));
                    }
                    let document_for_change = document.clone();
                    let syntax_window = window.window_handle();
                    let change_subscription = cx.subscribe(
                        editor,
                        move |panel: &mut WorkbenchPanel, editor, event: &InputEvent, cx| {
                            if !matches!(event, InputEvent::Change) {
                                return;
                            }
                            let editor_entity = editor.clone();
                            let editor = editor.read(cx);
                            let contents = editor.value().to_string();
                            let position = editor.cursor_position();
                            let result =
                                document_for_change.borrow_mut().replace_contents(contents);
                            let changed = match result {
                                Ok(changed) => changed,
                                Err(error) => {
                                    let previous =
                                        document_for_change.borrow().contents().to_owned();
                                    let message = error.to_string();
                                    cx.defer(move |cx| {
                                        let _ = cx.update_window(syntax_window, |_, window, cx| {
                                            editor_entity.update(cx, |editor, cx| {
                                                editor.replace_all(previous, window, cx)
                                            });
                                            window.push_notification(
                                                Notification::warning(message),
                                                cx,
                                            );
                                        });
                                    });
                                    return;
                                }
                            };
                            if !panel.document.block_text_refresh_pending {
                                document_for_change.borrow_mut().set_selection(
                                    position.line as usize,
                                    position.character as usize,
                                );
                                cx.global_mut::<EditorDocuments>().set_selection(
                                    &root,
                                    relative.clone(),
                                    position.line as usize,
                                    position.character as usize,
                                );
                            }
                            if changed {
                                if !panel.document.block_text_refresh_pending
                                    && relative
                                        .extension()
                                        .is_some_and(|extension| extension == "shou")
                                {
                                    let window_handle = syntax_window;
                                    let panel_entity = cx.weak_entity();
                                    let project = (
                                        cx.global::<EditorDocuments>().authoring(&root),
                                        relative.clone(),
                                    );
                                    cx.defer(move |cx| {
                                        let _ = cx.update_window(window_handle, |_, window, cx| {
                                            let _ = panel_entity.update(cx, |panel, cx| {
                                                panel.document.syntax_check =
                                                    Some(completion::schedule_syntax_check(
                                                        editor_entity,
                                                        panel.document.syntax_marks.clone(),
                                                        project,
                                                        window,
                                                        cx,
                                                    ));
                                            });
                                        });
                                    });
                                }
                                if !panel.document.block_text_refresh_pending {
                                    schedule_authoring_refresh(&root, Some(&relative), cx);
                                }
                                panel.document.recovery_epoch =
                                    panel.document.recovery_epoch.wrapping_add(1);
                                let epoch = panel.document.recovery_epoch;
                                let document = document_for_change.clone();
                                let root = root.clone();
                                if !document.borrow().is_dirty() {
                                    let cleanup = document.borrow().recovery_write();
                                    cx.background_executor()
                                        .spawn(async move {
                                            if let Ok(write) = cleanup {
                                                let _ = write.execute();
                                            }
                                        })
                                        .detach();
                                }
                                let any_dirty =
                                    cx.global::<EditorDocuments>().has_dirty_documents(&root);
                                let notice = if !any_dirty {
                                    "Ready"
                                } else {
                                    "Unsaved changes"
                                }
                                .to_owned();
                                cx.global_mut::<EditorDocuments>().set_notice(&root, notice);
                                if !panel.document.block_text_refresh_pending
                                    && relative
                                        .extension()
                                        .and_then(|extension| extension.to_str())
                                        == Some("shou")
                                {
                                    let preview_root = root.clone();
                                    let preview_relative = relative.clone();
                                    let preview_document = document.clone();
                                    cx.spawn(async move |panel, cx| {
                                        cx.background_executor()
                                            .timer(PREVIEW_SOURCE_DEBOUNCE)
                                            .await;
                                        let _ = panel.update(cx, |panel, cx| {
                                            if panel.document.recovery_epoch == epoch
                                                && let Ok(preview) = cx
                                                    .global_mut::<EditorDocuments>()
                                                    .preview(&preview_root)
                                            {
                                                preview.apply_snapshot(
                                                    preview_relative,
                                                    preview_document
                                                        .borrow()
                                                        .contents()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                            }
                                        });
                                    })
                                    .detach();
                                }
                                cx.spawn(async move |panel, cx| {
                                    cx.background_executor()
                                        .timer(Duration::from_millis(350))
                                        .await;
                                    let _ = panel.update(cx, |panel, cx| {
                                        if panel.document.recovery_epoch == epoch {
                                            let clean = !document.borrow().is_dirty();
                                            let recovery = document.borrow().recovery_write();
                                            let revision = document.borrow().revision();
                                            let write_root = root.clone();
                                            let background = cx.background_executor().clone();
                                            cx.spawn(async move |panel, cx| {
                                                let recovery = match recovery {
                                                    Ok(write) => {
                                                        background
                                                            .spawn(async move { write.execute() })
                                                            .await
                                                    }
                                                    Err(error) => Err(error),
                                                };
                                                let _ = panel.update(cx, |panel, cx| {
                                                    if panel.document.recovery_epoch != epoch
                                                        || document.borrow().revision() != revision
                                                    {
                                                        return;
                                                    }
                                                    let any_dirty = cx
                                                        .global::<EditorDocuments>()
                                                        .has_dirty_documents(&write_root);
                                                    let notice = match recovery {
                                                        Ok(()) if !any_dirty => "Ready".to_owned(),
                                                        Ok(()) if clean => {
                                                            "Unsaved changes".to_owned()
                                                        }
                                                        Ok(()) => "Recovery draft saved".to_owned(),
                                                        Err(error) => format!(
                                                            "Recovery draft failed: {error}"
                                                        ),
                                                    };
                                                    cx.global_mut::<EditorDocuments>()
                                                        .set_notice(&write_root, notice);
                                                    if !panel.document.block_text_refresh_pending {
                                                        cx.refresh_windows();
                                                    }
                                                });
                                            })
                                            .detach();
                                        }
                                    });
                                })
                                .detach();
                            }
                            cx.notify();
                            if !panel.document.block_text_refresh_pending {
                                cx.refresh_windows();
                            }
                        },
                    );
                    panel._subscriptions.push(change_subscription);
                }
            }
            if let PanelContent::Characters { .. } = &panel.content {
                for placeholder in [
                    "Character id",
                    "Display name",
                    "Color (optional)",
                    "Expression name",
                    "Frame asset IDs, comma separated",
                    "Avatar asset ID (optional)",
                ] {
                    panel
                        .characters
                        .tool_inputs
                        .push(cx.new(|cx| InputState::new(window, cx).placeholder(placeholder)));
                }
            }
            if let Some(view) = saved_view
                && let PanelContent::Document {
                    root,
                    relative,
                    editor,
                    ..
                } = &panel.content
            {
                let position = editor.update(cx, |editor, cx| {
                    let position = gpui_kit::base::input::Position::new(
                        view.line.min(u32::MAX as usize) as u32,
                        view.column.min(u32::MAX as usize) as u32,
                    );
                    editor.set_cursor_position(position, window, cx);
                    editor.cursor_position()
                });
                cx.global_mut::<EditorDocuments>().set_selection(
                    root,
                    relative.clone(),
                    position.line as usize,
                    position.character as usize,
                );
                if view.mode == DocumentMode::Block {
                    let source = editor.read(cx).value().to_string();
                    let projection = EiyashouProjection::parse(&source);
                    if let Some((_, block)) = block_at_position(
                        &projection,
                        &source,
                        position.line as usize,
                        position.character as usize,
                    ) {
                        panel
                            .document
                            .selected_blocks
                            .insert(block.source_range.start);
                        panel.document.block_selection_anchor = Some(block.source_range.start);
                        panel.document.block_scroll_pending = true;
                        cx.global_mut::<EditorDocuments>().set_block_selection(
                            root,
                            relative.clone(),
                            vec![block.source_range.start],
                        );
                    }
                }
            }
            panel.rebuild_visual_editors(window, cx);
            panel
        });
        if let Some((root, relative)) = registration {
            let editor = match &panel.read(cx).content {
                PanelContent::Document { editor, .. } => Some(editor.downgrade()),
                _ => None,
            };
            let documents = cx.global_mut::<EditorDocuments>();
            documents.register_panel(
                &root,
                relative.clone(),
                PanelId::from(panel.entity_id()),
                panel.downgrade(),
            );
            if let Some(editor) = editor {
                documents.register_editor(&root, relative.clone(), editor);
            }
            let reopened = match &panel.read(cx).content {
                PanelContent::Document {
                    document: Some(document),
                    ..
                } => Some(document.borrow().contents().as_bytes().to_vec()),
                _ => None,
            };
            if let Some(source) = reopened {
                if relative.extension().is_some_and(|ext| ext == "shou")
                    && let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(&root)
                {
                    preview.apply_snapshot(relative.clone(), source);
                }
                // Opening a clean file may adopt a newer disk revision without
                // an InputEvent::Change. Update every derived consumer as well.
                schedule_authoring_refresh(&root, Some(&relative), cx);
            }
        }
        if let Some((root, name)) = tool_registration {
            cx.global_mut::<EditorDocuments>().set_tool_panel(
                &root,
                name,
                Some(PanelId::from(panel.entity_id())),
            );
        }
        Ok(panel)
    }
}
