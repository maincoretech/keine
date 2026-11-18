//! Voice matching previews and checked source transactions.
use crate::app::*;
use crate::authoring::voices::{VoiceScope, match_voices};

pub(in crate::app) fn open(root: &Path, window: &mut Window, cx: &mut App) {
    let paths: std::collections::BTreeSet<_> = cx
        .global::<EditorDocuments>()
        .authoring(root)
        .scenes
        .iter()
        .map(|s| s.path.clone())
        .collect();
    open_matches(root, paths, VoiceScope::File, window, cx);
}

pub(in crate::app) fn open_scene(
    root: &Path,
    path: &Path,
    name: &str,
    window: &mut Window,
    cx: &mut App,
) {
    open_matches(root, [path.to_owned()], VoiceScope::Scene(name), window, cx);
}

pub(in crate::app) fn open_line(
    root: &Path,
    path: &Path,
    start: usize,
    window: &mut Window,
    cx: &mut App,
) {
    open_matches(root, [path.to_owned()], VoiceScope::Line(start), window, cx);
}

fn open_matches(
    root: &Path,
    paths: impl IntoIterator<Item = PathBuf>,
    scope: VoiceScope<'_>,
    window: &mut Window,
    cx: &mut App,
) {
    let index = cx.global::<EditorDocuments>().authoring(root);
    let mut expected = BTreeMap::new();
    let mut edits = Vec::new();
    let mut rows = Vec::new();
    let mut matched = 0;
    for path in paths {
        let result = cx
            .global::<EditorDocuments>()
            .source(root, &path)
            .and_then(|source| {
                match_voices(&source, &index.assets, scope)
                    .map(|(edited, rows)| (source, edited, rows))
            });
        let Some((source, edited, matches)) = result else {
            if matches!(scope, VoiceScope::Scene(_)) {
                return;
            }
            continue;
        };
        matched += matches.iter().filter(|r| r.voice.is_some()).count();
        rows.extend(matches.into_iter().map(|row| (path.clone(), row)));
        if edited != source {
            expected.insert(path.clone(), source);
            edits.push((path, edited));
        }
    }
    let title = match scope {
        VoiceScope::File => "Match dialogue voices",
        VoiceScope::Scene(_) => "Match scene voices",
        VoiceScope::Line(_) => "Match voice for this line",
    };
    let root = root.to_owned();
    let rows = Rc::new(rows);
    let edits = Rc::new(edits);
    let expected = Rc::new(expected);
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let content = div()
            .id("voice-matches")
            .max_h(px(380.))
            .overflow_y_scroll()
            .when(rows.is_empty(), |this| {
                this.child(crate::i18n::tr("No unassigned dialogue in this scope"))
            })
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
            .title(crate::i18n::tr(title))
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
                    .ok_text(crate::i18n::tr(if matched == 0 {
                        "Close"
                    } else {
                        "Apply"
                    }))
                    .cancel_text(crate::i18n::tr("Cancel"))
                    .show_cancel(matched != 0),
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
