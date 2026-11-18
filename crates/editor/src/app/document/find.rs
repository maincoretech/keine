//! Workbench presentation for the source editor's search session.
use crate::app::*;
use gpui_kit::base::input::{Escape, Replace, Search};
use gpui_kit::base::motion::Presence;
use gpui_kit::component::button::{Button, ButtonVariants as _};

pub(in crate::app) struct DocumentFind {
    query: Entity<InputState>,
    replacement: Entity<InputState>,
    open: bool,
    replacing: bool,
}

impl WorkbenchPanel {
    pub(in crate::app) fn open_document_find(
        &mut self,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PanelContent::Document {
            editor, document, ..
        } = &self.content
        else {
            return;
        };
        let editor = editor.clone();
        let find = self.document.find.get_or_insert_with(|| {
            let query = cx
                .new(|cx| InputState::new(window, cx).placeholder(crate::i18n::tr("Find in file")));
            let replacement = cx
                .new(|cx| InputState::new(window, cx).placeholder(crate::i18n::tr("Replace with")));
            self._subscriptions.push(cx.subscribe(
                &query,
                |panel, input, event: &InputEvent, cx| {
                    let PanelContent::Document { editor, .. } = &panel.content else {
                        return;
                    };
                    match event {
                        InputEvent::Change => {
                            let query = input.read(cx).value();
                            editor.update(cx, |editor, cx| {
                                let case_insensitive = editor.search_session().case_insensitive;
                                editor.set_search_query(query, case_insensitive, cx);
                            });
                        }
                        InputEvent::PressEnter { shift, .. } => move_match(editor, *shift, cx),
                        _ => {}
                    }
                    cx.notify();
                },
            ));
            DocumentFind {
                query,
                replacement,
                open: false,
                replacing: false,
            }
        });
        for input in [&find.query, &find.replacement] {
            crate::i18n::refresh_input(input, window, cx);
        }
        let selected = editor.read(cx).selected_text().to_string();
        let query = if selected.is_empty() {
            find.query.read(cx).value()
        } else {
            selected.into()
        };
        find.open = true;
        find.replacing = replace && document.is_some();
        let focus = if replace && find.replacing {
            find.replacement.clone()
        } else {
            find.query.clone()
        };
        find.query
            .update(cx, |input, cx| input.set_value(query.clone(), window, cx));
        editor.update(cx, |editor, cx| {
            let case_insensitive = editor.search_session().case_insensitive;
            editor.set_search_query(query, case_insensitive, cx);
        });
        focus.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    pub(in crate::app) fn close_document_find(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(find) = &mut self.document.find
            && find.open
            && let PanelContent::Document { editor, .. } = &self.content
        {
            find.open = false;
            editor.update(cx, |editor, cx| {
                editor.close_search(cx);
                editor.focus(window, cx);
            });
            cx.notify();
        }
    }

    pub(in crate::app) fn refresh_document_find_language(&self, window: &mut Window, cx: &mut App) {
        if let Some(find) = &self.document.find {
            for input in [&find.query, &find.replacement] {
                crate::i18n::refresh_input(input, window, cx);
            }
        }
    }
}

fn move_match(editor: &Entity<EditorState>, previous: bool, cx: &mut App) {
    editor.update(cx, |editor, cx| {
        if previous {
            editor.previous_search_match(cx);
        } else {
            editor.next_search_match(cx);
        }
    });
}

pub(super) fn render(
    find: &DocumentFind,
    editor: &Entity<EditorState>,
    writable: bool,
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> Option<AnyElement> {
    let id = gpui_kit::ElementId::View(cx.entity_id());
    let presence = Presence::new((id, "document-find"), find.open)
        .transition(Transition::new(Duration::from_millis(160)))
        .sample(window, cx);
    if !presence.should_render() {
        return None;
    }
    let search = editor.read(cx).search_session();
    let case_sensitive = !search.case_insensitive;
    let has_query = !search.query.is_empty();
    let has_matches = !search.matcher.is_empty();
    let count = search.matcher.label();
    let replacing = find.replacing && writable;
    let enabled = find.open;
    let replacement_progress = writable.then(|| {
        transition(
            (gpui_kit::ElementId::View(cx.entity_id()), "find-replace"),
            if replacing { 1. } else { 0. },
            Transition::new(Duration::from_millis(160)),
            window,
            cx,
        )
    });
    let case_hint = crate::i18n::tr("Match case");
    let mut bar = div()
        .id("document-find")
        .absolute()
        .top_2()
        .right_2()
        .w(px(468.))
        .max_w(gpui_kit::relative(0.96))
        .shadow_lg()
        .opacity(presence.progress)
        .when(enabled, |bar| bar.block_mouse_except_scroll())
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(PANEL))
        .p_1()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    find_input(&find.query, AssetIconName::Search, enabled).child(
                        div()
                            .id("find-case")
                            .h(px(22.))
                            .px_1()
                            .flex_none()
                            .flex()
                            .items_center()
                            .rounded(px(4.))
                            .when(enabled, |button| button.focusable())
                            .aria_label(case_hint)
                            .text_xs()
                            .text_color(rgb(if case_sensitive { PRIMARY } else { MUTED }))
                            .bg(rgb(if case_sensitive { PRIMARY_DIM } else { SURFACE }))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                            .focus_visible(|style| style.bg(rgb(PRIMARY_DIM)))
                            .active(|style| style.bg(rgb(PRIMARY_DIM)))
                            .tooltip(icon_hint(case_hint))
                            .child("Aa")
                            .on_click(cx.listener(move |panel, _, _, cx| {
                                if enabled
                                    && let PanelContent::Document { editor, .. } = &panel.content
                                {
                                    editor.update(cx, |editor, cx| {
                                        let search = editor.search_session();
                                        editor.set_search_query(
                                            search.query.clone(),
                                            !search.case_insensitive,
                                            cx,
                                        );
                                    });
                                }
                                cx.notify();
                            })),
                    ),
                )
                .when(has_query, |row| {
                    row.child(
                        div()
                            .px_1()
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(count),
                    )
                })
                .children(
                    [
                        (
                            "find-previous",
                            AssetIconName::ChevronUp,
                            "Previous match",
                            true,
                        ),
                        ("find-next", AssetIconName::ChevronDown, "Next match", false),
                    ]
                    .into_iter()
                    .map(|(id, icon, hint, previous)| {
                        let editor = editor.clone();
                        find_tool(id, icon, hint)
                            .when(!enabled || !has_matches, |button| {
                                button.opacity(0.4).tab_stop(false).cursor_default()
                            })
                            .on_click(cx.listener(move |_, _, _, cx| {
                                if enabled && has_matches {
                                    move_match(&editor, previous, cx);
                                    cx.notify();
                                }
                            }))
                    }),
                )
                .when(writable, |row| {
                    row.child(
                        find_tool("find-replace", AssetIconName::Replace, "Toggle replacement")
                            .tab_stop(enabled)
                            .when(replacing, |button| button.bg(rgb(PRIMARY_DIM)))
                            .on_click(cx.listener(|panel, _, window, cx| {
                                let Some(find) =
                                    panel.document.find.as_mut().filter(|find| find.open)
                                else {
                                    return;
                                };
                                find.replacing = !find.replacing;
                                let input = if find.replacing {
                                    &find.replacement
                                } else {
                                    &find.query
                                };
                                input.update(cx, |input, cx| input.focus(window, cx));
                                cx.notify();
                            })),
                    )
                })
                .child(
                    find_tool("find-close", AssetIconName::X, "Close search")
                        .tab_stop(enabled)
                        .on_click(cx.listener(|panel, _, window, cx| {
                            panel.close_document_find(window, cx);
                        })),
                ),
        );
    if let Some(progress) = replacement_progress.filter(|progress| *progress > 0. || replacing) {
        let replace_enabled = enabled && replacing;
        bar = bar.child(disclosure_content(
            "find-replacement",
            progress,
            div()
                .pt_1()
                .flex()
                .items_center()
                .gap_1()
                .child(find_input(
                    &find.replacement,
                    AssetIconName::Replace,
                    replace_enabled,
                ))
                .children(
                    [
                        ("replace-one", "Replace", false),
                        ("replace-all", "Replace all", true),
                    ]
                    .into_iter()
                    .map(|(id, label, all)| {
                        let editor = editor.clone();
                        let replacement = find.replacement.clone();
                        Button::new(id)
                            .xsmall()
                            .ghost()
                            .label(crate::i18n::tr(label))
                            .disabled(!replace_enabled || !has_matches)
                            .on_click(cx.listener(move |_, _, window, cx| {
                                let value = replacement.read(cx).value();
                                editor.update(cx, |editor, cx| {
                                    if all {
                                        editor.replace_all_search_matches(&value, window, cx);
                                    } else {
                                        editor.replace_current_search_match(&value, window, cx);
                                    }
                                });
                                cx.notify();
                            }))
                    }),
                ),
        ));
    }
    Some(bar.into_any_element())
}

fn find_tool(id: &'static str, icon: AssetIconName, hint: &'static str) -> Stateful<Div> {
    file_action_icon(id, icon, hint).aria_label(crate::i18n::tr(hint))
}

fn find_input(input: &Entity<InputState>, icon: AssetIconName, enabled: bool) -> Div {
    div()
        .h(px(28.))
        .min_w_0()
        .flex_1()
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .rounded(px(6.))
        .bg(rgb(SURFACE))
        .child(Icon::new(icon).xsmall().flex_none().text_color(rgb(MUTED)))
        .child(
            Input::new(input)
                .disabled(!enabled)
                .appearance(false)
                .bordered(false)
                .small()
                .flex_1()
                .min_w_0()
                .text_size(px(12.)),
        )
}

pub(super) fn shortcut_scope(view: Div, cx: &mut Context<WorkbenchPanel>) -> Div {
    view.on_action(cx.listener(|panel, _: &Search, window, cx| {
        panel.open_document_find(false, window, cx);
    }))
    .on_action(cx.listener(|panel, _: &Replace, window, cx| {
        panel.open_document_find(true, window, cx);
    }))
    .on_action(cx.listener(|panel, _: &Escape, window, cx| {
        if panel.document.find.as_ref().is_some_and(|find| find.open) {
            panel.close_document_find(window, cx);
        } else {
            cx.propagate();
        }
    }))
}
