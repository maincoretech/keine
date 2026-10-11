//! Output panel presentation.
use crate::app::*;

impl WorkbenchPanel {
    pub(in crate::app) fn render_output(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let PanelContent::Output { root, file_count } = &self.content else {
            return Empty.into_any_element();
        };
        let root = root.clone();
        let file_count = *file_count;
        let inspection = runtime_inspection(&root, true, cx);
        let diagnostics = cx.global::<EditorDocuments>().runtime_diagnostics(&root);
        let mono = Theme::global(cx).mono_font_family.clone();

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
                    .notice(&root)
                    .map(str::to_owned),
                |this, notice| this.child(output_line("STATUS", PRIMARY, notice)),
            )
            .when_some(inspection, |this, inspection| this.child(inspection))
            .children(
                diagnostics
                    .into_iter()
                    .enumerate()
                    .map(|(row, diagnostic)| {
                        let root = root.clone();
                        let path = diagnostic.path.clone();
                        let line = diagnostic.line;
                        let column = diagnostic.column;
                        super::render::problem_row(
                            ("output-diagnostic", row),
                            0xdb7780,
                            path.display().to_string(),
                            line,
                            column,
                            diagnostic.message,
                        )
                        .on_click(move |_, window, cx| {
                            super::edits::navigate_inspection(
                                &root, &path, line, column, window, cx,
                            )
                        })
                    }),
            );
        vertical_overflow_view("output-scroll", &self.view_scroll, content)
    }
}

/// Same read-only presentation in Inspector and Output; source navigation uses
/// the normal document owner and never issues a Replay/seek command.
pub(super) fn runtime_inspection(
    root: &Path,
    variables: bool,
    cx: &mut Context<WorkbenchPanel>,
) -> Option<AnyElement> {
    let preview = cx
        .global_mut::<EditorDocuments>()
        .preview(root)
        .ok()?
        .snapshot();
    let runtime = preview.runtime?;
    let mut content = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(section_label("PREVIEW"))
        .child(property_row(
            "Action",
            format!("{} · {}", runtime.scene, runtime.cursor.saturating_sub(1)),
        ))
        .child(property_row(
            "Waiting",
            if runtime.waiting.is_empty() {
                "None".into()
            } else {
                runtime.waiting.join(" · ")
            },
        ));
    for (id, label, location) in [
        (
            "preview-current-source",
            "Current action",
            preview.runtime_position,
        ),
        (
            "preview-save-source",
            if runtime.checkpoint_is_live {
                "Save: current action"
            } else {
                "Save: previous reachable action"
            },
            runtime.checkpoint,
        ),
    ] {
        content = if let Some((path, line, column)) = location {
            let root = root.to_owned();
            content.child(
                div()
                    .id(id)
                    .px_1()
                    .py_1()
                    .rounded(px(4.))
                    .text_xs()
                    .text_color(rgb(PRIMARY))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                    .child(format!("{label} · {}:{line}:{column}", path.display()))
                    .on_click(move |_, window, cx| {
                        super::edits::navigate_inspection(&root, &path, line, column, window, cx)
                    }),
            )
        } else if id == "preview-save-source" {
            content.child(property_row("Save", "No reachable checkpoint".into()))
        } else {
            content
        };
    }
    if variables {
        content = content.child(section_label("VARIABLES · READ ONLY"));
        if runtime.variables.is_empty() {
            content = content.child(div().text_xs().text_color(rgb(MUTED)).child("No variables"));
        }
        content = content.children(runtime.variables.into_iter().map(|(name, value)| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_xs()
                .child(div().text_color(rgb(PRIMARY)).child(name))
                .child(div().text_color(rgb(INK)).child(value))
        }));
        if runtime.omitted_variables > 0 {
            content = content.child(div().text_xs().text_color(rgb(MUTED))
                .child(format!("{} more variables; first 128 shown. Strings limited to 512 bytes; arrays show item count.", runtime.omitted_variables)));
        }
    }
    Some(content.into_any_element())
}
