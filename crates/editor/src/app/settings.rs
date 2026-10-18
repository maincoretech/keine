//! Settings navigation and the small set of persistent Editor preferences.
use super::*;
use crate::persistence::EditorPreferences;
use gpui_kit::base::Selectable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SettingsTab {
    #[default]
    Project,
    Gallery,
    Editor,
}

pub(super) fn render(
    root: &Path,
    panel: &mut WorkbenchPanel,
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let mut tabs = div()
        .flex()
        .flex_wrap()
        .gap_1()
        .p_2()
        .border_b_1()
        .border_color(rgb(BORDER));
    for (tab, label) in [
        (SettingsTab::Project, "Project"),
        (SettingsTab::Gallery, "Extra"),
        (SettingsTab::Editor, "Editor"),
    ] {
        let active = panel.settings_tab == tab;
        tabs = tabs.child(
            Button::new(label)
                .ghost()
                .small()
                .label(crate::i18n::text(label))
                .when(active, |button| {
                    button.bg(rgb(PRIMARY_DIM)).text_color(rgb(PRIMARY))
                })
                .on_click(cx.listener(move |panel, _, window, cx| {
                    project_settings::cancel_drag(panel, window, cx);
                    panel.close_resource_picker(false, window, cx);
                    panel.settings_tab = tab;
                    panel
                        .view_scroll
                        .set_offset(gpui_kit::point(px(0.), px(0.)));
                    cx.notify();
                })),
        );
    }
    let body = if panel.settings_tab == SettingsTab::Editor {
        editor_preferences(true, cx)
    } else {
        project_settings::render(root, panel, window, cx)
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(tabs)
        .child(div().flex_1().min_h_0().child(body))
        .into_any_element()
}

fn change(update: impl FnOnce(&mut EditorPreferences), window: &mut Window, cx: &mut App) {
    let mut preferences = cx.global::<EditorDocuments>().editor_preferences;
    update(&mut preferences);
    if let Err(error) = cx
        .global_mut::<EditorDocuments>()
        .set_editor_preferences(preferences)
    {
        window.push_notification(
            Notification::error(crate::i18n::format(
                "Could not save Editor settings: {error}",
                &[("error", error.to_string())],
            )),
            cx,
        );
    }
    cx.refresh_windows();
}

pub(super) fn editor_preferences(has_project: bool, cx: &App) -> AnyElement {
    let preferences = cx.global::<EditorDocuments>().editor_preferences;
    div()
        .id("editor-settings-scroll")
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .w_full()
                .max_w(px(640.))
                .p_3()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(crate::i18n::tr(
                            "Applies to all projects. Changes are saved automatically.",
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .items_center()
                        .justify_between()
                        .child(div().text_sm().child(crate::i18n::tr("Language")))
                        .child(div().flex().flex_wrap().gap_1().children(
                            crate::i18n::Language::ALL.into_iter().map(|language| {
                                Button::new(language.code())
                                    .ghost()
                                    .small()
                                    .label(language.name())
                                    .selected(preferences.language == language)
                                    .on_click(move |_, window, cx| {
                                        change(|p| p.language = language, window, cx);
                                    })
                            }),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .items_center()
                        .justify_between()
                        .child(div().text_sm().child(crate::i18n::tr("Source font size")))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new("font-smaller")
                                        .ghost()
                                        .small()
                                        .label("−")
                                        .tooltip(crate::i18n::tr("Decrease source font size"))
                                        .accessibility_label(crate::i18n::tr(
                                            "Decrease source font size",
                                        ))
                                        .disabled(preferences.source_font_size <= 10)
                                        .on_click(|_, window, cx| {
                                            change(
                                                |p| {
                                                    p.source_font_size =
                                                        p.source_font_size.saturating_sub(1)
                                                },
                                                window,
                                                cx,
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .child(format!("{} px", preferences.source_font_size)),
                                )
                                .child(
                                    Button::new("font-larger")
                                        .ghost()
                                        .small()
                                        .label("+")
                                        .tooltip(crate::i18n::tr("Increase source font size"))
                                        .accessibility_label(crate::i18n::tr(
                                            "Increase source font size",
                                        ))
                                        .disabled(preferences.source_font_size >= 24)
                                        .on_click(|_, window, cx| {
                                            change(
                                                |p| {
                                                    p.source_font_size =
                                                        p.source_font_size.saturating_add(1)
                                                },
                                                window,
                                                cx,
                                            )
                                        }),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .child(crate::i18n::tr("Text / Blocks overview")),
                        )
                        .child(
                            Switch::new("editor-minimap")
                                .small()
                                .color(rgb(PRIMARY))
                                .checked(preferences.show_minimap)
                                .accessibility_label(crate::i18n::tr(
                                    "Show Text and Blocks overview",
                                ))
                                .on_change(|checked, window, cx| {
                                    change(|p| p.show_minimap = *checked, window, cx)
                                }),
                        ),
                )
                .when(has_project, |content| {
                    content.child(
                        div().pt_2().border_t_1().border_color(rgb(BORDER)).child(
                            Button::new("restore-workspace-layout")
                                .ghost()
                                .small()
                                .icon(AssetIconName::RotateCcw)
                                .label(crate::i18n::tr("Restore layout"))
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(ResetLayout), cx)
                                }),
                        ),
                    )
                }),
        )
        .into_any_element()
}

pub(super) fn open_without_project(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, |dialog, _, cx| {
        dialog
            .title(crate::i18n::tr("Editor settings"))
            .width(px(440.))
            .child(div().h(px(230.)).child(editor_preferences(false, cx)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_preference_save_keeps_live_settings() {
        let path =
            std::env::temp_dir().join(format!("keine-preferences-blocked-{}", std::process::id()));
        fs::write(&path, "not a directory").unwrap();
        let mut documents = EditorDocuments::new(AppPersistence::new(path.clone()));
        assert!(
            documents
                .set_editor_preferences(EditorPreferences {
                    source_font_size: 20,
                    show_minimap: false,
                    ..Default::default()
                })
                .is_err()
        );
        assert_eq!(documents.editor_preferences, EditorPreferences::default());
        fs::remove_file(path).unwrap();
    }
}
