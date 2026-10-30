//! Project-wide voice matching preview and one checked source transaction.
use crate::app::*;

pub(in crate::app) fn open(root: &Path, window: &mut Window, cx: &mut App) {
    let index = cx.global::<EditorDocuments>().authoring(root);
    let paths: std::collections::BTreeSet<_> =
        index.scenes.iter().map(|s| s.path.clone()).collect();
    let mut expected = BTreeMap::new();
    let mut edits = Vec::new();
    let mut rows = Vec::new();
    let mut matched = 0;
    for path in paths {
        let Some(source) = cx.global::<EditorDocuments>().source(root, &path) else {
            continue;
        };
        let (edited, matches) = crate::authoring::voices::match_voices(&source, &index.assets);
        matched += matches.iter().filter(|r| r.voice.is_some()).count();
        rows.extend(matches.into_iter().map(|row| (path.clone(), row)));
        // Check all inspected documents, including those without a match.
        expected.insert(path.clone(), source.clone());
        if edited != source {
            edits.push((path, edited));
        }
    }
    let root = root.to_owned();
    let rows = Rc::new(rows);
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let content = div()
            .id("voice-matches")
            .max_h(px(380.))
            .overflow_y_scroll()
            .children(rows.iter().enumerate().map(|(i, (path, row))| {
                let root = root.clone();
                let path = path.clone();
                let line = row.line + 1;
                let column = row.column + 1;
                div()
                    .id(("voice-match", i))
                    .py_1()
                    .px_2()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(SURFACE_HOVER)))
                    .child(format!(
                        "{}:{} · {} · {}",
                        path.display(),
                        line,
                        row.id,
                        row.voice.as_deref().unwrap_or(crate::i18n::tr(row.status))
                    ))
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        super::super::edits::navigate_inspection(
                            &root, &path, line, column, window, cx,
                        );
                    })
            }));
        let root = root.clone();
        let edits = edits.clone();
        let expected = expected.clone();
        dialog
            .title(crate::i18n::tr("Match dialogue voices"))
            .width(px(660.))
            .child(div().text_sm().child(crate::i18n::tr(
                "Match voice ID or filename to dialogue ID. Existing voices are kept.",
            )))
            .child(div().text_sm().child(crate::i18n::format(
                "Matched voices: {count}",
                &[("count", matched.to_string())],
            )))
            .child(content)
            .button_props(
                gpui_kit::component::dialog::DialogButtonProps::default()
                    .ok_text(crate::i18n::tr("Apply"))
                    .cancel_text(crate::i18n::tr("Cancel"))
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| {
                if edits.is_empty() {
                    return true;
                }
                match super::super::edits::apply_checked_edits(&root, &edits, &expected, window, cx)
                {
                    Ok(()) => true,
                    Err(error) => {
                        window.push_notification(Notification::error(error), cx);
                        false
                    }
                }
            })
    });
}
