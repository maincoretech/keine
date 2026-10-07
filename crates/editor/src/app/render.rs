use super::*;

impl Render for WorkbenchPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.prepare_document_view(window, cx);
        self.refresh_block_drag(window, cx);
        self.refresh_picker_drag(window, cx);
        if !cx.has_active_drag() {
            self.explorer.file_drop_target = None;
        }
        if let PanelContent::Inspector { root, .. } = &self.content {
            let root = root.clone();
            self.refresh_inspector_editors(&root, window, cx);
        }
        if self.document.document_mode == DocumentMode::Block
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
        let body = match &self.content {
            PanelContent::Explorer { .. } => self.render_explorer(window, cx),
            PanelContent::Document { .. } => self.render_document(window, cx),
            PanelContent::Inspector { .. } => self.render_inspector(window, cx),
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
                let root = root.clone();
                characters::render(&root, self, window, cx)
            }
            PanelContent::Scenes { .. } => self.render_scenes(window, cx),
            PanelContent::Problems { root } => render_problems(root, &self.view_scroll, cx),
            PanelContent::Performance { controller, .. } => {
                performance::render(controller, &self.view_scroll)
            }
            PanelContent::Build { root } => build::render(root, &self.view_scroll, cx),
            PanelContent::Output { .. } => self.render_output(window, cx),
        };
        let resource_popup = self.render_resource_picker(window, cx);
        div()
            .track_focus(&self.focus)
            .capture_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape"
                        && matches!(
                            this.document.block_drag,
                            blocks::BlockDragState::Dragging(_)
                        )
                    {
                        this.cancel_block_drag(window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .capture_action(cx.listener(Self::accept_source_suggestion))
            .capture_action(cx.listener(Self::show_source_completions))
            .capture_action(cx.listener(Self::backspace_empty_text))
            .capture_action(cx.listener(Self::delete_empty_text))
            .when(self.resource_picker.is_some(), |this| {
                this.key_context("KeineResourcePicker")
            })
            .when(
                self.document.document_mode == DocumentMode::Block
                    && self.resource_picker.is_none(),
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
