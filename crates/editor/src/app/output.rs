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
                    .notice(root)
                    .map(str::to_owned),
                |this, notice| this.child(output_line("STATUS", PRIMARY, notice)),
            );
        vertical_overflow_view("output-scroll", &self.view_scroll, content)
    }
}
