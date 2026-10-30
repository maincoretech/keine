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
                crate::i18n::format(
                    "{file_count} text files discovered",
                    &[("file_count", (file_count).to_string())],
                ),
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

/// Shared inspection presentation in Inspector and Output; source navigation uses
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
    let mut content = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(section_label("PREVIEW"));
    if let Some(status) = preview.source_status() {
        content = content.child(
            div()
                .text_xs()
                .text_color(rgb(PRIMARY))
                .child(crate::i18n::tr(status)),
        );
    }
    let Some(runtime) = preview.runtime else {
        return preview.source_status().map(|_| content.into_any_element());
    };
    content = content
        .child(property_row(
            "Action",
            format!("{} · {}", runtime.scene, runtime.cursor.saturating_sub(1)),
        ))
        .child(property_row(
            "Waiting",
            if runtime.waiting.is_empty() {
                crate::i18n::tr("None").into()
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
            content.child(property_row(
                "Save",
                crate::i18n::tr("No reachable checkpoint").into(),
            ))
        } else {
            content
        };
    }
    if variables {
        content = content.child(section_label("Preview variables"));
        if runtime.variables.is_empty() {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(crate::i18n::tr("No variables")),
            );
        }
        if let Some(error) = preview.variable_error {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(rgb(0xdb7780))
                    .child(crate::i18n::text(&error)),
            );
        }
        content = content.child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(crate::i18n::tr("Changes affect this Preview session only.")),
        );
        let editable = runtime.editable_variables;
        content = content.children(runtime.variables.into_iter().enumerate().map(
            |(row, (name, value))| {
                let root = root.to_owned();
                let revision = preview.revision;
                let edit_name = name.clone();
                let edit_value = value.clone();
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_color(rgb(PRIMARY)).child(name))
                            .child(div().text_color(rgb(INK)).child(value)),
                    )
                    .when(editable.contains(&row), |row_view| {
                        row_view.child(
                            file_action_icon(
                                ("preview-variable", row),
                                AssetIconName::Pencil,
                                "Edit variable",
                            )
                            .on_click(move |_, window, cx| {
                                edit_variable(&root, revision, &edit_name, &edit_value, window, cx)
                            }),
                        )
                    })
            },
        ));
        if runtime.omitted_variables > 0 {
            content = content.child(div().text_xs().text_color(rgb(MUTED))
                .child(crate::i18n::format("{0} more variables; first 128 shown. Strings limited to 512 bytes; arrays show item count.", &[("0", (runtime.omitted_variables).to_string())])));
        }
    }
    Some(content.into_any_element())
}

fn edit_variable(
    root: &Path,
    revision: u64,
    name: &str,
    expected: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(expected));
    let focus = input.clone();
    let root = root.to_owned();
    let title = name.to_owned();
    let (global, name) = name
        .strip_prefix("global.")
        .map_or((false, name), |name| (true, name));
    let name = name.to_owned();
    let expected = expected.to_owned();
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let root = root.clone();
        let name = name.clone();
        let expected = expected.clone();
        let value = input.clone();
        dialog
            .title(title.clone())
            .width(px(420.))
            .child(tool_input(&input))
            .button_props(
                gpui_kit::component::dialog::DialogButtonProps::default()
                    .ok_text(crate::i18n::tr("Apply"))
                    .cancel_text(crate::i18n::tr("Cancel"))
                    .show_cancel(true),
            )
            .on_ok(move |_, _, cx| {
                let value = value.read(cx).value().to_string();
                if let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(&root) {
                    preview.set_variable(revision, name.clone(), global, expected.clone(), value);
                }
                true
            })
    });
    focus.read(cx).focus_handle(cx).focus(window, cx);
}
