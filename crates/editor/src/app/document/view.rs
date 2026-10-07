//! Document panel presentation.
use crate::app::*;

impl WorkbenchPanel {
    pub(in crate::app) fn render_document(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let PanelContent::Document {
            root,
            relative,
            document,
            editor,
        } = &self.content
        else {
            return Empty.into_any_element();
        };
        let mono = Theme::global(cx).mono_font_family.clone();

        let eiyashou = document.is_some()
            && relative.extension().and_then(|value| value.to_str()) == Some("shou");
        let mode = self.document.document_mode;
        let body = if eiyashou && mode == DocumentMode::Block {
            render_block_projection(
                BlockProjectionView {
                    root,
                    relative,
                    document: document.as_ref().unwrap(),
                    editors: &self.document.block_text_editors,
                    inline: &self.document.inline_block_controls,
                    collapsed_scenes: &self.document.collapsed_scenes,
                    selected_blocks: &self.document.selected_blocks,
                    draft_text: self.document.draft_text.as_ref(),
                    drag: &self.document.block_drag,
                    row_bounds: &self.document.block_row_bounds,
                    row_positions: &self.document.block_row_positions,
                    scroll_handle: &self.view_scroll,
                    scroll_anchor: &self.document.block_scroll_anchor,
                    scroll_pending: self.document.block_scroll_pending,
                    minimap: &self.document.minimap_navigation,
                    scene_edit: self.document.scene_edit.as_ref(),
                    scene_name_input: &self.document.scene_name_input,
                    visible: &self.document.block_visible,
                    heights: &self.document.block_heights,
                    layout: &self.document.block_layout,
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
                    &mut self.document.text_minimap,
                    &self.document.minimap_navigation,
                    window,
                    cx,
                ))
                .into_any_element()
        };
        let picker = (eiyashou && mode == DocumentMode::Block && self.picker.block_picker_open)
            .then(|| {
                let query = self
                    .picker
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
                    self.picker.block_picker_category,
                    self.picker.block_picker_customize,
                );
                render_block_picker(&kinds, &preferences, self, cx)
            });
        let scene_menu = (eiyashou && mode == DocumentMode::Block)
            .then(|| self.document.scene_context_menu.clone())
            .flatten()
            .map(|menu| render_scene_context_menu(menu, cx));
        let block_menu = (eiyashou && mode == DocumentMode::Block)
            .then(|| self.document.block_context_menu.clone())
            .flatten()
            .map(|menu| render_block_context_menu(menu, cx));
        if eiyashou && mode == DocumentMode::Block {
            self.document.block_scroll_pending = false;
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
}

impl WorkbenchPanel {
    pub(in crate::app) fn render_scenes(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let PanelContent::Scenes { root } = &self.content else {
            return Empty.into_any_element();
        };

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
                                .child(div().text_sm().text_color(rgb(INK)).child(scene.name))
                                .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                                    "{}:{}",
                                    scene.path.display(),
                                    scene.line
                                )))
                        }),
                ),
            );
        vertical_overflow_view("scene-scroll", &self.view_scroll, content)
    }
}

impl WorkbenchPanel {
    pub(in crate::app) fn prepare_document_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                .document
                .diagnostic_index
                .upgrade()
                .is_none_or(|previous| !Arc::ptr_eq(&previous, &index))
            {
                self.document.diagnostic_index = Arc::downgrade(&index);
                self.document.syntax_check = Some(completion::schedule_syntax_check(
                    editor.clone(),
                    self.document.syntax_marks.clone(),
                    (index.clone(), relative.clone()),
                    window,
                    cx,
                ));
            }
        }
        if self.document.document_mode == DocumentMode::Text && self.document.text_scroll_pending {
            self.document.text_scroll_pending = false;
            let panel = cx.entity().downgrade();
            // Cursor scrolling needs the Text editor's first layout, including after
            // switching from Blocks. Run once after that frame, without a timer.
            window.on_next_frame(move |window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if panel.document.document_mode == DocumentMode::Text
                        && let PanelContent::Document { editor, .. } = &panel.content
                    {
                        editor.update(cx, |editor, cx| {
                            editor.set_cursor_position(editor.cursor_position(), window, cx);
                        });
                    }
                });
            });
        }
    }
}
