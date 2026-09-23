use super::*;

pub(super) fn preview_control(label: &'static str, selected: bool) -> Stateful<Div> {
    div()
        .id(match label {
            "View" => "preview-edit",
            "Interact" => "preview-play",
            "Start" => "preview-start",
            _ => "preview-stop",
        })
        .h(px(26.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(7.))
        .bg(rgb(if selected { SURFACE } else { CHROME }))
        .text_xs()
        .text_color(rgb(if selected { PRIMARY } else { MUTED }))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)).text_color(rgb(INK)))
        .child(label)
}

pub(super) fn section_label(label: &'static str) -> impl IntoElement {
    div()
        .pt_1()
        .text_xs()
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(rgb(PRIMARY))
        .child(label)
}

pub(super) fn property_row(label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .pb_2()
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(div().text_xs().text_color(rgb(MUTED)).child(label))
        .child(div().text_xs().text_color(rgb(INK)).child(value))
}

pub(super) fn property_input(
    label: impl Into<SharedString>,
    state: &Entity<InputState>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .pb_2()
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(div().text_xs().text_color(rgb(MUTED)).child(label.into()))
        .child(
            div().h(px(28.)).child(
                Input::new(state)
                    .appearance(false)
                    .bordered(false)
                    .size_full()
                    .text_xs()
                    .text_color(rgb(INK)),
            ),
        )
}

pub(super) fn output_line(label: &'static str, color: u32, value: String) -> impl IntoElement {
    div()
        .flex()
        .gap_3()
        .child(
            div()
                .w(px(44.))
                .flex_none()
                .text_color(rgb(color))
                .child(label),
        )
        .child(div().min_w_0().flex_1().whitespace_normal().child(value))
}

pub(super) fn document_mode_button(label: &'static str, selected: bool) -> Stateful<Div> {
    div()
        .id(if label == "Text" {
            "document-mode-text"
        } else {
            "document-mode-block"
        })
        .h(px(25.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(rgb(if selected { SURFACE } else { CHROME }))
        .text_xs()
        .text_color(rgb(if selected { INK } else { MUTED }))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)).text_color(rgb(INK)))
        .child(label)
}

pub(super) fn tool_input(state: &Entity<InputState>) -> impl IntoElement {
    div()
        .h(px(30.))
        .rounded(px(7.))
        .bg(rgb(SURFACE))
        .px_2()
        .child(
            Input::new(state)
                .appearance(false)
                .bordered(false)
                .size_full()
                .text_sm()
                .text_color(rgb(INK)),
        )
}

pub(super) fn tool_action(label: &'static str) -> Stateful<Div> {
    div()
        .id(if label == "Add character" {
            "add-character"
        } else {
            "add-scene"
        })
        .h(px(28.))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .bg(rgb(PRIMARY_DIM))
        .text_xs()
        .text_color(rgb(PRIMARY))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)).text_color(rgb(INK)))
        .child(label)
}

pub(super) const PICKER_CATEGORIES: [&str; 5] = ["Text", "Scene", "Media", "Flow", "Data"];

pub(super) fn toggle_preference(values: &mut Vec<String>, value: &str) {
    if let Some(index) = values.iter().position(|candidate| candidate == value) {
        values.remove(index);
    } else {
        values.push(value.to_owned());
    }
}

pub(super) fn move_preference(
    values: &mut Vec<String>,
    value: &str,
    universe: &[&str],
    delta: isize,
) {
    let mut ordered = values
        .iter()
        .map(String::as_str)
        .filter(|candidate| universe.contains(candidate))
        .collect::<Vec<_>>();
    for candidate in universe {
        if !ordered.contains(candidate) {
            ordered.push(candidate);
        }
    }
    let Some(index) = ordered.iter().position(|candidate| *candidate == value) else {
        return;
    };
    let target = index.saturating_add_signed(delta).min(ordered.len() - 1);
    ordered.swap(index, target);
    *values = ordered.into_iter().map(str::to_owned).collect();
}

pub(super) fn move_group_preference(
    values: &mut Vec<String>,
    value: &str,
    group: &[&str],
    universe: &[&str],
    delta: isize,
) {
    let mut ordered = values
        .iter()
        .map(String::as_str)
        .filter(|candidate| universe.contains(candidate))
        .collect::<Vec<_>>();
    for candidate in universe {
        if !ordered.contains(candidate) {
            ordered.push(candidate);
        }
    }
    let group_positions = ordered
        .iter()
        .enumerate()
        .filter(|(_, candidate)| group.contains(candidate))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let Some(group_index) = group_positions
        .iter()
        .position(|index| ordered[*index] == value)
    else {
        return;
    };
    let target = group_index
        .saturating_add_signed(delta)
        .min(group_positions.len() - 1);
    ordered.swap(group_positions[group_index], group_positions[target]);
    *values = ordered.into_iter().map(str::to_owned).collect();
}

pub(super) fn preference_rank(values: &[String], value: &str, fallback: usize) -> usize {
    values
        .iter()
        .position(|candidate| candidate == value)
        .unwrap_or(values.len() + fallback)
}

pub(super) fn ordered_picker_categories(preferences: &BlockPickerPreferences) -> Vec<&'static str> {
    let mut categories = PICKER_CATEGORIES.to_vec();
    categories.sort_by_key(|category| {
        preference_rank(
            &preferences.category_order,
            category,
            PICKER_CATEGORIES
                .iter()
                .position(|candidate| candidate == category)
                .unwrap_or(usize::MAX / 2),
        )
    });
    categories
}

pub(super) fn picker_kinds(
    preferences: &BlockPickerPreferences,
    query: &str,
    selected_category: Option<&str>,
    customize: bool,
) -> Vec<InsertKind> {
    let categories = ordered_picker_categories(preferences);
    let mut kinds = InsertKind::ALL
        .into_iter()
        .filter(|kind| {
            let matches_query = query.is_empty()
                || kind.search_terms().contains(query)
                || kind.label().to_lowercase().contains(query);
            if !matches_query {
                return false;
            }
            if !query.is_empty() {
                return true;
            }
            let visible = customize
                || !preferences
                    .hidden
                    .iter()
                    .any(|candidate| candidate == kind.label());
            visible
                && match selected_category {
                    Some("Favorites") => preferences
                        .favorites
                        .iter()
                        .any(|candidate| candidate == kind.label()),
                    Some(category) => kind.category() == category,
                    None => true,
                }
        })
        .collect::<Vec<_>>();
    kinds.sort_by_key(|kind| {
        let category_rank = categories
            .iter()
            .position(|category| *category == kind.category())
            .unwrap_or(categories.len());
        let item_fallback = InsertKind::ALL
            .iter()
            .position(|candidate| candidate == kind)
            .unwrap_or(usize::MAX / 2);
        (
            category_rank,
            preference_rank(&preferences.item_order, kind.label(), item_fallback),
        )
    });
    kinds
}

pub(super) fn insert_kind_icon(kind: InsertKind) -> AssetIconName {
    match kind {
        InsertKind::Narration => AssetIconName::MessageSquareText,
        InsertKind::Dialogue => AssetIconName::User,
        InsertKind::Background => AssetIconName::Image,
        InsertKind::Figure => AssetIconName::PersonStanding,
        InsertKind::Choice | InsertKind::Conditional => AssetIconName::GitBranch,
        InsertKind::Loop => AssetIconName::Repeat2,
        InsertKind::Variable => AssetIconName::Braces,
        InsertKind::Goto | InsertKind::Call | InsertKind::Return => AssetIconName::Workflow,
        InsertKind::Wait => AssetIconName::Clock,
        InsertKind::Hide => AssetIconName::EyeOff,
        InsertKind::Move => AssetIconName::Move,
        InsertKind::Bgm => AssetIconName::Music,
        InsertKind::Effect => AssetIconName::Volume2,
        InsertKind::Video => AssetIconName::Film,
    }
}

pub(super) fn render_block_picker(
    kinds: &[InsertKind],
    input: &Entity<InputState>,
    selected_index: usize,
    selected_category: Option<&'static str>,
    customize: bool,
    preferences: &BlockPickerPreferences,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let mut rows = Vec::new();
    let mut previous_category = None;
    for (index, kind) in kinds.iter().copied().enumerate() {
        let category = kind.category();
        if previous_category != Some(category) {
            rows.push(
                div()
                    .w_full()
                    .flex_none()
                    .pt_2()
                    .px_2()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(rgb(MUTED))
                    .child(category.to_uppercase())
                    .into_any_element(),
            );
            previous_category = Some(category);
        }
        let icon = insert_kind_icon(kind);
        let favorite = preferences
            .favorites
            .iter()
            .any(|candidate| candidate == kind.label());
        let hidden = preferences
            .hidden
            .iter()
            .any(|candidate| candidate == kind.label());
        let mut row =
            div()
                .id(("block-picker-item", index))
                .when(customize, |this| this.w_full())
                .when(!customize, |this| {
                    this.w(gpui_kit::relative(0.48)).min_w(px(150.))
                })
                .flex_none()
                .h(px(34.))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .rounded(px(6.))
                .bg(rgb(if index == selected_index {
                    SURFACE
                } else {
                    PANEL
                }))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.block_picker_open = false;
                    this.insert_from_palette(kind, window, cx);
                    this.rebuild_visual_editors(window, cx);
                    this.focus.focus(window, cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(16.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon).xsmall().text_color(rgb(
                            if index == selected_index {
                                PRIMARY
                            } else {
                                MUTED
                            },
                        ))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_sm()
                        .text_color(rgb(if hidden { MUTED } else { INK }))
                        .child(kind.label()),
                );
        if customize {
            row = row
                .child(
                    div()
                        .id(("picker-favorite", index))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .hover(|style| style.bg(rgb(SURFACE)))
                        .tooltip(icon_hint("Favorite"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_picker_favorite(kind, cx);
                            cx.notify();
                        }))
                        .child(
                            Icon::new(if favorite {
                                AssetIconName::StarFill
                            } else {
                                AssetIconName::Star
                            })
                            .xsmall()
                            .text_color(rgb(if favorite {
                                PRIMARY
                            } else {
                                MUTED
                            })),
                        ),
                )
                .child(
                    div()
                        .id(("picker-up", index))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .hover(|style| style.bg(rgb(SURFACE)))
                        .tooltip(icon_hint("Move up"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.move_picker_item(kind, -1, cx);
                            cx.notify();
                        }))
                        .child(
                            Icon::new(AssetIconName::ArrowUp)
                                .xsmall()
                                .text_color(rgb(MUTED)),
                        ),
                )
                .child(
                    div()
                        .id(("picker-down", index))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .hover(|style| style.bg(rgb(SURFACE)))
                        .tooltip(icon_hint("Move down"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.move_picker_item(kind, 1, cx);
                            cx.notify();
                        }))
                        .child(
                            Icon::new(AssetIconName::ArrowDown)
                                .xsmall()
                                .text_color(rgb(MUTED)),
                        ),
                )
                .child(
                    div()
                        .id(("picker-visible", index))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .hover(|style| style.bg(rgb(SURFACE)))
                        .tooltip(icon_hint("Show or hide"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_picker_hidden(kind, cx);
                            cx.notify();
                        }))
                        .child(
                            Icon::new(if hidden {
                                AssetIconName::EyeOff
                            } else {
                                AssetIconName::Eye
                            })
                            .xsmall()
                            .text_color(rgb(if hidden {
                                MUTED
                            } else {
                                PRIMARY
                            })),
                        ),
                );
        } else if favorite {
            row = row.child(
                Icon::new(AssetIconName::StarFill)
                    .xsmall()
                    .text_color(rgb(PRIMARY)),
            );
        }
        rows.push(row.into_any_element());
    }
    let categories = std::iter::once(None)
        .chain(std::iter::once(Some("Favorites")))
        .chain(ordered_picker_categories(preferences).into_iter().map(Some))
        .collect::<Vec<_>>();
    div()
        .id("block-picker-overlay")
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .p_3()
        .flex()
        .bg(hsla(0., 0., 0.01, 0.84))
        .child(
            div()
                .id("block-picker")
                .key_context("KeineBlockPicker")
                .size_full()
                .min_w_0()
                .min_h_0()
                .flex()
                .flex_col()
                .rounded(px(10.))
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(PANEL))
                .overflow_hidden()
                .child(
                    div()
                        .h(px(44.))
                        .flex_none()
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .bg(rgb(CHROME))
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(input)
                                    .prefix(
                                        Icon::new(AssetIconName::Search)
                                            .xsmall()
                                            .text_color(rgb(MUTED)),
                                    )
                                    .appearance(false)
                                    .bordered(false)
                                    .size_full()
                                    .text_sm()
                                    .text_color(rgb(INK)),
                            ),
                        )
                        .child(
                            div()
                                .id("block-picker-customize")
                                .size(px(28.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(6.))
                                .bg(rgb(if customize { SURFACE } else { CHROME }))
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                .tooltip(icon_hint("Customize blocks"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.block_picker_customize = !this.block_picker_customize;
                                    this.block_picker_index = 0;
                                    cx.notify();
                                }))
                                .child(
                                    Icon::new(AssetIconName::SlidersHorizontal)
                                        .xsmall()
                                        .text_color(rgb(if customize { PRIMARY } else { MUTED })),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(
                            div()
                                .w(px(112.))
                                .flex_none()
                                .p_2()
                                .flex()
                                .flex_wrap()
                                .content_start()
                                .gap_1()
                                .bg(rgb(CHROME))
                                .children(categories.into_iter().enumerate().map(
                                    |(index, category)| {
                                        let selected = selected_category == category;
                                        let label = category.unwrap_or("All");
                                        let mut row = div()
                                            .id(("block-picker-category", index))
                                            .h(px(30.))
                                            .px_2()
                                            .flex()
                                            .items_center()
                                            .rounded(px(6.))
                                            .bg(rgb(if selected { SURFACE } else { CHROME }))
                                            .text_xs()
                                            .text_color(rgb(if selected { PRIMARY } else { MUTED }))
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.block_picker_category = category;
                                                this.block_picker_index = 0;
                                                cx.notify();
                                            }))
                                            .child(div().flex_1().child(label));
                                        if let Some(category) = category.filter(|category| {
                                            customize && *category != "Favorites"
                                        }) {
                                            row = row
                                                .child(
                                                    div()
                                                        .id(("picker-category-up", index))
                                                        .size(px(20.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .tooltip(icon_hint("Move category up"))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                cx.stop_propagation();
                                                                this.move_picker_category(
                                                                    category, -1, cx,
                                                                );
                                                                cx.notify();
                                                            },
                                                        ))
                                                        .child(
                                                            Icon::new(AssetIconName::ChevronUp)
                                                                .xsmall(),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .id(("picker-category-down", index))
                                                        .size(px(20.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .tooltip(icon_hint("Move category down"))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                cx.stop_propagation();
                                                                this.move_picker_category(
                                                                    category, 1, cx,
                                                                );
                                                                cx.notify();
                                                            },
                                                        ))
                                                        .child(
                                                            Icon::new(AssetIconName::ChevronDown)
                                                                .xsmall(),
                                                        ),
                                                );
                                        }
                                        row
                                    },
                                )),
                        )
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .p_2()
                                .flex()
                                .flex_wrap()
                                .content_start()
                                .gap_1()
                                .children(rows)
                                .overflow_y_scrollbar()
                                .id("block-picker-results"),
                        ),
                ),
        )
        .into_any_element()
}

pub(super) fn block_card_label(kind: &BlockKind, source: &str) -> String {
    match kind {
        BlockKind::Dialogue { speaker } => speaker.clone(),
        BlockKind::Command => {
            let name = source.split('(').next().unwrap_or_default().trim();
            InsertKind::for_command(name)
                .map(|kind| kind.label().to_owned())
                .unwrap_or_else(|| title_case(name.rsplit('.').next().unwrap_or(name)))
        }
        BlockKind::Control => title_case(source.trim()),
        _ => kind.label().to_owned(),
    }
}

pub(super) fn source_field_label(key: &SourceInspectorKey, field: &SourceField) -> String {
    if field.key.parse::<usize>().is_err() {
        return title_case(&field.key);
    }
    let position = field.key.parse::<usize>().unwrap_or_default();
    match &key.kind {
        BlockKind::Choice => "Prompt".into(),
        BlockKind::Conditional | BlockKind::ElseIf => "Condition".into(),
        BlockKind::ChoiceOption => "Option".into(),
        BlockKind::Declaration | BlockKind::Assignment => "Value".into(),
        BlockKind::Command => match (key.command.as_str(), position) {
            ("background" | "bgm" | "se" | "video", 0) => "Asset".into(),
            ("sprite", 0) | ("hide", 0) | ("move", 0) => "Slot".into(),
            ("sprite", 1) => "Asset".into(),
            ("move", 1) => "Position".into(),
            ("goto" | "call", 0) => "Scene".into(),
            ("wait", 0) => "Duration".into(),
            ("pop", 0) => "List".into(),
            ("pop", 1) => "Index".into(),
            (command, 0) if command.ends_with(".append") || command.ends_with(".remove") => {
                "Value".into()
            }
            (command, 0) if command.ends_with(".insert") => "Index".into(),
            (command, 1) if command.ends_with(".insert") => "Value".into(),
            _ => format!("Arg {}", position + 1),
        },
        _ => format!("Arg {}", position + 1),
    }
}

pub(super) fn block_card_icon(kind: &BlockKind, source: &str) -> AssetIconName {
    match kind {
        BlockKind::Narration | BlockKind::Dialogue { .. } => AssetIconName::MessageSquareText,
        BlockKind::Choice
        | BlockKind::ChoiceOption
        | BlockKind::Conditional
        | BlockKind::ElseIf => AssetIconName::GitBranch,
        BlockKind::Else => AssetIconName::Workflow,
        BlockKind::Loop => AssetIconName::Repeat2,
        BlockKind::Declaration | BlockKind::Assignment => AssetIconName::Braces,
        BlockKind::Command => {
            InsertKind::for_command(source.split('(').next().unwrap_or_default().trim())
                .map(insert_kind_icon)
                .unwrap_or(AssetIconName::Braces)
        }
        BlockKind::Control => AssetIconName::Workflow,
        BlockKind::Unsupported => AssetIconName::TriangleAlert,
    }
}

pub(super) fn block_card_summary(kind: &BlockKind, source: &str, line: usize) -> String {
    match kind {
        BlockKind::Narration | BlockKind::Dialogue { .. } => {
            format!("Dynamic text · L{}", line + 1)
        }
        BlockKind::Choice
        | BlockKind::Conditional
        | BlockKind::ElseIf
        | BlockKind::Else
        | BlockKind::Loop
        | BlockKind::Control => String::new(),
        BlockKind::ChoiceOption => first_quoted_text(source).unwrap_or_default(),
        BlockKind::Declaration => source
            .strip_prefix("let ")
            .and_then(|tail| tail.split_whitespace().next())
            .unwrap_or_default()
            .to_owned(),
        BlockKind::Assignment => source
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned(),
        BlockKind::Command => {
            let name = source.split('(').next().unwrap_or_default().trim();
            let args = source
                .split_once('(')
                .and_then(|(_, tail)| tail.rsplit_once(')'))
                .map(|(args, _)| args)
                .unwrap_or_default();
            let values = args.split(',').map(str::trim).collect::<Vec<_>>();
            match name {
                "sprite" => values.get(1).copied().unwrap_or_default(),
                "move" => values.get(1).copied().unwrap_or_default(),
                "pop" => values.first().copied().unwrap_or_default(),
                method if method.contains('.') => method.split('.').next().unwrap_or_default(),
                _ => values.first().copied().unwrap_or_default(),
            }
            .to_owned()
        }
        BlockKind::Unsupported => format!("Unsupported syntax · L{}", line + 1),
    }
}

pub(super) fn first_quoted_text(source: &str) -> Option<String> {
    let start = source.find('"')? + 1;
    let mut escaped = false;
    for (offset, character) in source[start..].char_indices() {
        if character == '"' && !escaped {
            return Some(source[start..start + offset].to_owned());
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    None
}

pub(super) fn title_case(value: &str) -> String {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    first.to_uppercase().chain(characters).collect()
}

pub(super) fn bottom_overflow_fade(visible: impl Fn(&App) -> bool + 'static) -> impl IntoElement {
    canvas(
        move |_, _, cx| visible(cx),
        |bounds, visible, window, _| {
            if visible {
                window.paint_quad(fill(
                    bounds,
                    linear_gradient(
                        180.,
                        linear_color_stop(hsla(0., 0., 0.01, 0.), 0.),
                        linear_color_stop(hsla(0., 0., 0.01, 0.22), 1.),
                    ),
                ));
            }
        },
    )
    .absolute()
    .left_0()
    .right_0()
    .bottom_0()
    .h(px(28.))
}

pub(super) fn vertical_overflow_view<E>(
    id: &'static str,
    handle: &ScrollHandle,
    content: E,
) -> AnyElement
where
    E: InteractiveElement + Styled + ParentElement + Element + 'static,
{
    let fade_handle = handle.clone();
    let area = div()
        .id(format!("{id}-area"))
        .size_full()
        .min_h_0()
        .track_scroll(handle)
        .overflow_y_scroll()
        .lock_scroll_axis()
        .child(content.w_full().h_auto().min_h_full().flex_none());
    div()
        .id(id)
        .relative()
        .size_full()
        .min_h_0()
        .overflow_hidden()
        .child(area)
        .child(bottom_overflow_fade(move |_| {
            let max = fade_handle.max_offset().y;
            max > px(1.) && max + fade_handle.offset().y > px(1.)
        }))
        .vertical_scrollbar(handle)
        .into_any_element()
}

pub(super) struct BlockProjectionView<'a> {
    pub(super) root: &'a Path,
    pub(super) relative: &'a Path,
    pub(super) document: &'a DocumentHandle,
    pub(super) editors: &'a [BlockTextEditor],
    pub(super) collapsed_scenes: &'a HashSet<String>,
    pub(super) selected_blocks: &'a HashSet<usize>,
    pub(super) draft_text: Option<&'a DraftTextBlock>,
    pub(super) drop_target: Option<usize>,
    pub(super) scroll_handle: &'a ScrollHandle,
    pub(super) scroll_anchor: &'a ScrollAnchor,
    pub(super) scroll_pending: bool,
    pub(super) scene_edit: Option<&'a SceneEditMode>,
    pub(super) scene_name_input: &'a Entity<InputState>,
}

pub(super) fn draft_text_row(draft: &DraftTextBlock, indent: f32, id: usize) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .pl(px(indent))
        .child(
            div()
                .id(("draft-text-block", id))
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .min_h(px(38.))
                .px_2()
                .rounded(px(7.))
                .bg(rgb(SURFACE))
                .child(
                    Icon::new(AssetIconName::MessageSquarePlus)
                        .xsmall()
                        .text_color(rgb(PRIMARY)),
                )
                .child(
                    div()
                        .w(px(64.))
                        .flex_none()
                        .text_xs()
                        .text_color(rgb(PRIMARY))
                        .child("Text"),
                )
                .child(
                    div().min_h(px(30.)).flex_1().min_w_0().child(
                        Textarea::new(&draft.state)
                            .appearance(false)
                            .bordered(false)
                            .size_full()
                            .text_sm()
                            .text_color(rgb(INK)),
                    ),
                ),
        )
        .into_any_element()
}

pub(super) fn render_block_projection(
    view: BlockProjectionView<'_>,
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let BlockProjectionView {
        root,
        relative,
        document,
        editors,
        collapsed_scenes,
        selected_blocks,
        draft_text,
        drop_target,
        scroll_handle,
        scroll_anchor,
        scroll_pending,
        scene_edit,
        scene_name_input,
    } = view;
    let source = document.borrow().contents().to_owned();
    let projection = EiyashouProjection::parse(&source);
    let block_order = Arc::new(
        projection
            .scenes
            .iter()
            .flat_map(|scene| scene.blocks.iter().map(|block| block.source_range.start))
            .collect::<Vec<_>>(),
    );
    let selected_position = cx
        .global::<EditorDocuments>()
        .selection(root)
        .filter(|(path, _, _)| path == relative)
        .map(|(_, line, column)| (*line, *column));
    let selected_line = selected_position.map(|(line, _)| line);
    let selected_start = selected_position.and_then(|(line, column)| {
        projected_block_at(&source, line, column).map(|(_, block)| block.source_range.start)
    });
    let root = root.to_owned();
    let relative = relative.to_owned();
    let mut rows = Vec::new();
    for (scene_index, scene) in projection.scenes.into_iter().enumerate() {
        let collapsed = collapsed_scenes.contains(&scene.name);
        let scene_name = scene.name.clone();
        let context_name = scene.name.clone();
        let button_name = scene.name.clone();
        let scene_start = scene.source_range.start;
        let editing_scene = matches!(scene_edit, Some(SceneEditMode::Rename { start, .. }) if *start == scene_start);
        let scene_root = root.clone();
        let scene_relative = relative.clone();
        let collapse_progress = transition(
            (
                format!("block-scene-{}-{scene_index}", relative.display()),
                "collapse",
            ),
            if collapsed { 0. } else { 1. },
            Transition::new(Duration::from_millis(120)),
            window,
            cx,
        );
        let scene_line = document
            .borrow()
            .contents()
            .get(..scene.name_range.start)
            .map(|prefix| prefix.bytes().filter(|byte| *byte == b'\n').count())
            .unwrap_or_default();
        let header = div()
            .id(("scene-section", scene_index))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .h(px(32.))
            .px_2()
            .rounded(px(7.))
            .bg(rgb(if selected_line == Some(scene_line) {
                SURFACE
            } else {
                PANEL
            }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if this.scene_edit.is_some() {
                    return;
                }
                if !this.collapsed_scenes.remove(&scene_name) {
                    this.collapsed_scenes.insert(scene_name.clone());
                }
                this.selected_blocks.clear();
                this.block_selection_anchor = None;
                cx.global_mut::<EditorDocuments>()
                    .clear_block_selection(&scene_root);
                set_authoring_selection(&scene_root, scene_relative.clone(), scene_line, 0, cx);
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_scene_context_menu(
                        scene_start,
                        context_name.clone(),
                        event.position,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .child(
                Icon::new(IconName::ChevronRight)
                    .xsmall()
                    .rotate(radians(collapse_progress * std::f32::consts::FRAC_PI_2))
                    .text_color(rgb(MUTED)),
            )
            .child(if editing_scene {
                div()
                    .flex_1()
                    .min_w_0()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        Input::new(scene_name_input)
                            .appearance(false)
                            .bordered(false)
                            .size_full()
                            .text_sm()
                            .text_color(rgb(INK)),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(rgb(INK))
                    .child(scene.name)
                    .into_any_element()
            })
            .child(
                div()
                    .id(("scene-menu", scene_index))
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(5.))
                    .hover(|style| style.bg(rgb(SURFACE)))
                    .tooltip(icon_hint("Scene menu"))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.open_scene_context_menu(
                                scene_start,
                                button_name.clone(),
                                event.position,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .child(
                        Icon::new(AssetIconName::Ellipsis)
                            .xsmall()
                            .text_color(rgb(MUTED)),
                    ),
            )
            .into_any_element();
        let mut scene_rows = Vec::new();
        let mut scene_body_height = 0.;
        for (block_index, block) in scene.blocks.into_iter().enumerate() {
            let row_id = block.source_range.start;
            let line = block.line;
            let column = block.column;
            let root = root.clone();
            let relative = relative.clone();
            let source_root = root.clone();
            let source_relative = relative.clone();
            let selected = selected_blocks.contains(&row_id) || selected_start == Some(row_id);
            let icon = block_card_icon(&block.kind, &block.summary);
            let label = block_card_label(&block.kind, &block.summary);
            let text_state = block.text_range.as_ref().and_then(|range| {
                editors
                    .iter()
                    .find(|editor| editor.text_start == range.start)
                    .map(|editor| &editor.state)
                    .or_else(|| {
                        draft_text
                            .filter(|draft| {
                                draft
                                    .text_range
                                    .as_ref()
                                    .is_some_and(|draft_range| draft_range.start == range.start)
                            })
                            .map(|draft| &draft.state)
                    })
            });
            let is_text = matches!(
                &block.kind,
                BlockKind::Narration | BlockKind::Dialogue { .. }
            );
            let is_structure = matches!(
                &block.kind,
                BlockKind::Choice
                    | BlockKind::ChoiceOption
                    | BlockKind::Conditional
                    | BlockKind::ElseIf
                    | BlockKind::Else
                    | BlockKind::Loop
            );
            let source_summary = block_card_summary(&block.kind, &block.summary, block.line);
            let order = block_order.clone();
            let drag_selection = if selected_blocks.contains(&row_id) {
                selected_blocks.clone()
            } else {
                HashSet::from([row_id])
            };
            let movable = !matches!(&block.kind, BlockKind::ElseIf | BlockKind::Else);
            let grip = if movable {
                div()
                    .id(("block-grip", row_id))
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_move()
                    .on_drag(
                        BlockDrag {
                            selected: drag_selection,
                        },
                        |_, _, _, cx| cx.new(|_| Empty),
                    )
                    .child(
                        Icon::new(AssetIconName::GripVertical)
                            .xsmall()
                            .text_color(rgb(0x686e75)),
                    )
                    .into_any_element()
            } else {
                div().size(px(18.)).into_any_element()
            };
            let text_rows = text_state.map_or(1, |state| {
                state.read(cx).value().lines().count().clamp(1, 6)
            });
            let row_height = if is_text {
                38. + (text_rows.saturating_sub(1) as f32 * 20.)
            } else if is_structure {
                28.
            } else {
                32.
            };
            scene_body_height += row_height + 4.;
            let drop_line_opacity = transition(
                (format!("block-drop-line-{row_id}"), "opacity"),
                f32::from(drop_target == Some(row_id) && cx.has_active_drag()),
                Transition::new(Duration::from_millis(90)),
                window,
                cx,
            );
            let block_indent = 8. + block.depth as f32 * 18.;
            let mut row = div()
                .id(("block-row", scene_index * 10_000 + block_index))
                .relative()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .min_h(px(row_height))
                .px_2()
                .rounded(px(7.))
                .bg(rgb(if selected {
                    SURFACE
                } else if is_structure {
                    CANVAS
                } else {
                    PANEL
                }))
                .cursor_pointer()
                .anchor_scroll((selected_start == Some(row_id)).then(|| scroll_anchor.clone()))
                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    let modifiers = event.modifiers();
                    if modifiers.shift {
                        let anchor = this.block_selection_anchor.unwrap_or(row_id);
                        if let (Some(anchor_index), Some(row_index)) = (
                            order.iter().position(|candidate| *candidate == anchor),
                            order.iter().position(|candidate| *candidate == row_id),
                        ) {
                            let start = anchor_index.min(row_index);
                            let end = anchor_index.max(row_index);
                            this.selected_blocks.clear();
                            this.selected_blocks
                                .extend(order[start..=end].iter().copied());
                        }
                    } else if modifiers.platform || modifiers.control {
                        if !this.selected_blocks.remove(&row_id) {
                            this.selected_blocks.insert(row_id);
                        }
                        this.block_selection_anchor = Some(row_id);
                    } else {
                        this.selected_blocks.clear();
                        this.selected_blocks.insert(row_id);
                        this.block_selection_anchor = Some(row_id);
                    }
                    cx.global_mut::<EditorDocuments>().set_block_selection(
                        &root,
                        relative.clone(),
                        this.selected_blocks.iter().copied().collect(),
                    );
                    set_authoring_selection(&root, relative.clone(), line, column, cx);
                    cx.notify();
                }))
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if cx.has_active_drag() && this.block_drop_target != Some(row_id) {
                        this.block_drop_target = Some(row_id);
                        cx.notify();
                    }
                }))
                .on_drop(cx.listener(move |this, drag: &BlockDrag, window, cx| {
                    cx.stop_propagation();
                    this.block_drop_target = None;
                    this.drop_blocks(drag, row_id, window, cx);
                }))
                .on_drop(cx.listener(move |this, drag: &AssetDrag, window, cx| {
                    cx.stop_propagation();
                    this.block_drop_target = None;
                    this.drop_assets(drag, row_id, window, cx);
                }))
                .child(
                    div()
                        .absolute()
                        .top(px(-1.))
                        .left(px(8.))
                        .right(px(8.))
                        .h(px(2.))
                        .rounded_full()
                        .bg(rgb(PRIMARY))
                        .opacity(drop_line_opacity),
                )
                .child(grip)
                .child(Icon::new(icon).xsmall().text_color(rgb(if block.read_only {
                    0xd2aa62
                } else {
                    MUTED
                })))
                .child(
                    div()
                        .w(px(64.))
                        .flex_shrink_1()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_xs()
                        .text_color(rgb(if is_text { PRIMARY } else { MUTED }))
                        .child(label),
                );
            row = if let Some(state) = text_state.filter(|_| !block.read_only) {
                row.child(
                    div().min_h(px(30.)).flex_1().min_w_0().child(
                        Textarea::new(state)
                            .appearance(false)
                            .bordered(false)
                            .size_full()
                            .text_sm()
                            .text_color(rgb(INK)),
                    ),
                )
            } else {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_sm()
                        .text_color(rgb(if block.read_only { 0xd2aa62 } else { INK }))
                        .child(source_summary),
                )
            };
            if block.read_only {
                row = row.child(
                    div()
                        .id(("open-source", row_id))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .hover(|style| style.bg(rgb(SURFACE)))
                        .tooltip(icon_hint("Open source"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.document_mode = DocumentMode::Text;
                            navigate_source(
                                &source_root,
                                &source_relative,
                                line + 1,
                                column + 1,
                                window,
                                cx,
                            );
                            cx.notify();
                        }))
                        .child(Icon::new(IconName::FileText).xsmall()),
                );
            }
            scene_rows.push(
                div()
                    .w_full()
                    .min_w_0()
                    .pl(px(block_indent))
                    .child(row)
                    .into_any_element(),
            );
            if let Some(draft) = draft_text.filter(|draft| {
                matches!(draft.target, DraftInsertionTarget::After(start) if start == row_id)
                    && draft.text_range.is_none()
            }) {
                scene_body_height += 42.
                    + (draft.state.read(cx).value().lines().count().clamp(1, 6) - 1) as f32 * 20.;
                scene_rows.push(draft_text_row(draft, block_indent, row_id));
            }
        }
        if let Some(draft) = draft_text.filter(|draft| {
            matches!(
                draft.target,
                DraftInsertionTarget::SceneEnd(start) if start == scene.source_range.start
            ) && draft.text_range.is_none()
        }) {
            scene_body_height +=
                42. + (draft.state.read(cx).value().lines().count().clamp(1, 6) - 1) as f32 * 20.;
            scene_rows.push(draft_text_row(draft, 8., scene.source_range.start));
        }
        scene_body_height = (scene_body_height - 4.).max(0.);
        rows.push(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .child(header)
                .child(
                    div()
                        .w_full()
                        .h(px(scene_body_height * collapse_progress))
                        .opacity(collapse_progress)
                        .overflow_hidden()
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .children(scene_rows),
                        ),
                )
                .into_any_element(),
        );
    }
    if matches!(scene_edit, Some(SceneEditMode::New)) {
        rows.push(
            div()
                .id("new-scene-row")
                .w_full()
                .h(px(32.))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .rounded(px(7.))
                .bg(rgb(SURFACE))
                .child(
                    Icon::new(AssetIconName::Plus)
                        .xsmall()
                        .text_color(rgb(PRIMARY)),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(scene_name_input)
                            .appearance(false)
                            .bordered(false)
                            .size_full()
                            .text_sm()
                            .text_color(rgb(INK)),
                    ),
                )
                .child(
                    file_action_icon("scene-add-confirm", AssetIconName::Check, "Add scene")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.commit_scene_edit(window, cx)),
                        ),
                )
                .child(
                    file_action_icon("scene-add-cancel", AssetIconName::Close, "Cancel").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.scene_edit = None;
                            cx.notify();
                        }),
                    ),
                )
                .into_any_element(),
        );
    }
    rows.extend(
        projection
            .read_only
            .into_iter()
            .enumerate()
            .map(|(index, card)| {
                div()
                    .id(("projection-diagnostic", index))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_h(px(30.))
                    .px_2()
                    .rounded(px(7.))
                    .bg(rgb(SURFACE))
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .xsmall()
                            .text_color(rgb(0xd2aa62)),
                    )
                    .child(div().text_xs().text_color(rgb(MUTED)).child(card.message))
                    .into_any_element()
            }),
    );
    if scroll_pending && selected_start.is_some() {
        scroll_anchor.scroll_to(window, cx);
    }
    let content = div()
        .id("eiyashou-block-content")
        .relative()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .pr_4()
        .children(rows)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.selected_blocks.clear();
            this.block_selection_anchor = None;
            cx.global_mut::<EditorDocuments>()
                .clear_block_selection(&root);
            cx.notify();
        }))
        .key_context("KeineBlockView");
    vertical_overflow_view("eiyashou-block-scroll", scroll_handle, content)
}

pub(super) fn render_assets(
    root: &Path,
    index: &AuthoringIndex,
    panel: &WorkbenchPanel,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let query = AssetQuery {
        search: panel.asset_search.read(cx).value().to_string(),
        kind: panel.asset_kind,
        folder: panel.asset_folder.clone(),
        sort: panel.asset_sort,
    };
    let results = if panel.asset_unmapped {
        Vec::new()
    } else {
        query.results(&index.assets)
    };
    let ordered = Arc::new(results.iter().map(|asset| asset.key()).collect::<Vec<_>>());
    let selection = cx.global::<EditorDocuments>().asset_selection(root);
    let folders = index
        .assets
        .iter()
        .filter_map(|asset| asset.path.parent().map(Path::to_path_buf))
        .chain(
            index
                .unmapped
                .iter()
                .filter_map(|asset| asset.path.parent().map(Path::to_path_buf)),
        )
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let filter_active = panel.asset_kind.is_some()
        || panel.asset_folder.is_some()
        || panel.asset_sort != AssetSort::Name
        || panel.asset_grid
        || panel.asset_unmapped;
    let filter_menu = panel
        .asset_filter_menu
        .clone()
        .map(|menu| render_asset_filter_menu(menu, &folders, panel, cx));
    let controls = div()
        .flex()
        .p_2()
        .gap_1()
        .items_center()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(28.))
                .rounded(px(7.))
                .bg(rgb(SURFACE))
                .px_2()
                .child(
                    Input::new(&panel.asset_search)
                        .appearance(false)
                        .bordered(false)
                        .size_full()
                        .text_sm()
                        .text_color(rgb(INK)),
                ),
        )
        .child(
            div()
                .id("asset-filter-button")
                .size(px(28.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .bg(rgb(if filter_active || panel.asset_filter_menu.is_some() {
                    SURFACE
                } else {
                    CHROME
                }))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                .tooltip(icon_hint("Filter assets"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        this.toggle_asset_filter_menu(event.position, window, cx);
                        cx.stop_propagation();
                    }),
                )
                .child(
                    Icon::new(AssetIconName::ListFilter)
                        .xsmall()
                        .text_color(rgb(if filter_active { PRIMARY } else { MUTED })),
                ),
        );
    let root = root.to_owned();
    let cards =
        div()
            .flex()
            .flex_wrap()
            .gap_1()
            .p_2()
            .children(results.into_iter().enumerate().map(|(row, asset)| {
                let key = asset.key();
                let row_key = key.clone();
                let row_root = root.clone();
                let row_order = ordered.clone();
                let selected = selection.contains(&key);
                let icon = match asset.kind {
                    AssetKind::Background | AssetKind::Figure => AssetIconName::Image,
                    AssetKind::Voice | AssetKind::Effect => AssetIconName::Volume2,
                    AssetKind::Bgm => AssetIconName::Music,
                    AssetKind::Video => AssetIconName::Film,
                };
                div()
                    .id(("asset-row", row))
                    .when(panel.asset_grid, |this| {
                        this.w(px(140.)).flex_col().items_start()
                    })
                    .when(!panel.asset_grid, |this| this.w_full().items_center())
                    .flex()
                    .min_w_0()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(7.))
                    .bg(rgb(if selected { SURFACE } else { PANEL }))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        let previous = cx.global::<EditorDocuments>().asset_selection(&row_root);
                        let modifiers = event.modifiers();
                        let next = select_asset_keys(
                            &previous,
                            &row_order,
                            this.asset_anchor.as_ref(),
                            &row_key,
                            modifiers.shift,
                            modifiers.platform || modifiers.control,
                        );
                        if !modifiers.shift {
                            this.asset_anchor = Some(row_key.clone());
                        }
                        cx.global_mut::<EditorDocuments>()
                            .set_asset_selection(&row_root, next);
                        cx.refresh_windows();
                    }))
                    .on_drag(
                        AssetDrag {
                            root: root.clone(),
                            keys: if selected {
                                ordered
                                    .iter()
                                    .filter(|key| selection.contains(key))
                                    .cloned()
                                    .collect()
                            } else {
                                vec![key]
                            },
                        },
                        |_, _, _, cx| cx.new(|_| Empty),
                    )
                    .child(Icon::new(icon).xsmall().text_color(rgb(PRIMARY)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_sm()
                            .text_color(rgb(INK))
                            .child(asset.id.clone()),
                    )
            }));
    let unmapped = query
        .unmapped_results(&index.unmapped)
        .into_iter()
        .enumerate()
        .map(|(row, asset)| {
            div()
                .id(("unmapped-row", row))
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .p_2()
                .rounded(px(7.))
                .bg(rgb(PANEL))
                .child(
                    Icon::new(AssetIconName::File)
                        .xsmall()
                        .text_color(rgb(MUTED)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_sm()
                        .text_color(rgb(INK))
                        .child(
                            asset
                                .path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| asset.path.display().to_string()),
                        ),
                )
        });
    div()
        .relative()
        .size_full()
        .min_h_0()
        .child(vertical_overflow_view(
            "asset-scroll",
            &panel.view_scroll,
            div()
                .w_full()
                .flex()
                .flex_col()
                .child(controls)
                .child(if panel.asset_unmapped {
                    div()
                        .flex()
                        .flex_col()
                        .p_2()
                        .gap_1()
                        .children(unmapped)
                        .into_any_element()
                } else {
                    cards.into_any_element()
                }),
        ))
        .when_some(filter_menu, |this, menu| this.child(menu))
        .into_any_element()
}

pub(super) fn render_asset_filter_menu(
    menu: AssetFilterMenu,
    folders: &[PathBuf],
    panel: &WorkbenchPanel,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let type_choices = std::iter::once((
        "All".to_owned(),
        AssetFilterChoice::Type(None),
        panel.asset_kind.is_none(),
    ))
    .chain(
        [
            AssetKind::Background,
            AssetKind::Figure,
            AssetKind::Voice,
            AssetKind::Bgm,
            AssetKind::Effect,
            AssetKind::Video,
        ]
        .into_iter()
        .map(|kind| {
            (
                kind.label().to_owned(),
                AssetFilterChoice::Type(Some(kind)),
                panel.asset_kind == Some(kind),
            )
        }),
    )
    .collect::<Vec<_>>();
    let folder_choices = std::iter::once((
        "All".to_owned(),
        AssetFilterChoice::Folder(None),
        panel.asset_folder.is_none(),
    ))
    .chain(folders.iter().map(|folder| {
        (
            folder.display().to_string(),
            AssetFilterChoice::Folder(Some(folder.clone())),
            panel.asset_folder.as_ref() == Some(folder),
        )
    }))
    .collect::<Vec<_>>();
    let groups = [
        (
            AssetFilterGroup::Type,
            "Type",
            panel
                .asset_kind
                .map_or("All".to_owned(), |kind| kind.label().to_owned()),
            type_choices,
        ),
        (
            AssetFilterGroup::Folder,
            "Folder",
            panel
                .asset_folder
                .as_ref()
                .map_or("All".to_owned(), |folder| folder.display().to_string()),
            folder_choices,
        ),
        (
            AssetFilterGroup::Sort,
            "Sort",
            match panel.asset_sort {
                AssetSort::Name => "Name",
                AssetSort::Path => "Path",
                AssetSort::References => "References",
            }
            .to_owned(),
            vec![
                (
                    "Name".to_owned(),
                    AssetFilterChoice::Sort(AssetSort::Name),
                    panel.asset_sort == AssetSort::Name,
                ),
                (
                    "Path".to_owned(),
                    AssetFilterChoice::Sort(AssetSort::Path),
                    panel.asset_sort == AssetSort::Path,
                ),
                (
                    "References".to_owned(),
                    AssetFilterChoice::Sort(AssetSort::References),
                    panel.asset_sort == AssetSort::References,
                ),
            ],
        ),
        (
            AssetFilterGroup::View,
            "View",
            if panel.asset_grid { "Grid" } else { "List" }.to_owned(),
            vec![
                (
                    "List".to_owned(),
                    AssetFilterChoice::View(false),
                    !panel.asset_grid,
                ),
                (
                    "Grid".to_owned(),
                    AssetFilterChoice::View(true),
                    panel.asset_grid,
                ),
            ],
        ),
        (
            AssetFilterGroup::Show,
            "Show",
            if panel.asset_unmapped {
                "Unmapped"
            } else {
                "Mapped"
            }
            .to_owned(),
            vec![
                (
                    "Mapped".to_owned(),
                    AssetFilterChoice::Show(false),
                    !panel.asset_unmapped,
                ),
                (
                    "Unmapped".to_owned(),
                    AssetFilterChoice::Show(true),
                    panel.asset_unmapped,
                ),
            ],
        ),
    ];
    let expanded_count = groups
        .iter()
        .find(|(group, _, _, _)| menu.expanded == Some(*group))
        .map_or(0, |(_, _, _, choices)| choices.len());
    let height = (8 + groups.len() * 32 + expanded_count * 27).min(344) as f32;
    let closing = menu.closing;
    let motion_duration = if closing { 90 } else { 120 };
    let surface = div()
        .id(("asset-filter-surface", menu.epoch))
        .w(px(ASSET_FILTER_MENU_WIDTH_PX))
        .h(px(height))
        .overflow_hidden()
        .p_1()
        .rounded(px(7.))
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(SURFACE))
        .shadow_lg()
        .child(
            div()
                .id("asset-filter-options")
                .size_full()
                .overflow_y_scrollbar()
                .children(groups.into_iter().enumerate().map(
                    |(group_index, (group, title, value, choices))| {
                        let expanded = menu.expanded == Some(group);
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .id(("asset-filter-group", group_index))
                                    .h(px(32.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded(px(5.))
                                    .text_xs()
                                    .text_color(rgb(INK))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(menu) = this.asset_filter_menu.as_mut() {
                                            menu.expanded = if menu.expanded == Some(group) {
                                                None
                                            } else {
                                                Some(group)
                                            };
                                            cx.notify();
                                        }
                                    }))
                                    .child(div().flex_1().min_w_0().child(title))
                                    .child(
                                        div()
                                            .max_w(px(110.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .text_color(rgb(MUTED))
                                            .child(value),
                                    )
                                    .child(
                                        Icon::new(AssetIconName::ChevronDown)
                                            .xsmall()
                                            .rotate(radians(if expanded {
                                                std::f32::consts::PI
                                            } else {
                                                0.
                                            }))
                                            .text_color(rgb(MUTED)),
                                    ),
                            )
                            .when(expanded, |this| {
                                this.children(choices.into_iter().enumerate().map(
                                    |(option_index, (label, choice, selected))| {
                                        div()
                                            .id((
                                                "asset-filter-option",
                                                group_index * 16 + option_index,
                                            ))
                                            .h(px(27.))
                                            .pl_4()
                                            .pr_2()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .rounded(px(5.))
                                            .text_xs()
                                            .text_color(rgb(if selected { PRIMARY } else { MUTED }))
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                match &choice {
                                                    AssetFilterChoice::Type(kind) => {
                                                        this.asset_kind = *kind
                                                    }
                                                    AssetFilterChoice::Folder(folder) => {
                                                        this.asset_folder = folder.clone()
                                                    }
                                                    AssetFilterChoice::Sort(sort) => {
                                                        this.asset_sort = *sort
                                                    }
                                                    AssetFilterChoice::View(grid) => {
                                                        this.asset_grid = *grid
                                                    }
                                                    AssetFilterChoice::Show(unmapped) => {
                                                        this.asset_unmapped = *unmapped
                                                    }
                                                }
                                                if let Some(menu) = this.asset_filter_menu.as_mut()
                                                {
                                                    menu.expanded = None;
                                                }
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .text_ellipsis()
                                                    .child(label),
                                            )
                                            .when(selected, |this| {
                                                this.child(
                                                    Icon::new(AssetIconName::Check)
                                                        .xsmall()
                                                        .text_color(rgb(PRIMARY)),
                                                )
                                            })
                                    },
                                ))
                            })
                            .into_any_element()
                    },
                )),
        )
        .with_animation(
            ("asset-filter-motion", menu.epoch),
            Animation::new(Duration::from_millis(motion_duration)).with_easing(ease_out_quint()),
            move |surface, delta| {
                let progress = if closing { 1. - delta } else { delta };
                let scale = 0.94 + progress * 0.06;
                surface
                    .opacity(progress)
                    .w(px(ASSET_FILTER_MENU_WIDTH_PX * scale))
                    .h(px(height * scale))
            },
        );
    deferred(
        anchored()
            .anchor(Anchor::TopLeft)
            .position(menu.position)
            .snap_to_window_with_margin(px(6.))
            .child(
                div()
                    .w(px(ASSET_FILTER_MENU_WIDTH_PX))
                    .h(px(height))
                    .on_mouse_down_out(
                        cx.listener(|this, _, window, cx| this.close_asset_filter_menu(window, cx)),
                    )
                    .child(surface),
            ),
    )
    .priority(100)
    .into_any_element()
}

pub(super) fn render_problems(
    root: &Path,
    scroll_handle: &ScrollHandle,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let index = cx.global::<EditorDocuments>().authoring(root);
    let runtime = cx.global::<EditorDocuments>().runtime_diagnostics(root);
    let root = root.to_owned();
    let authoring_rows = index.problems.into_iter().enumerate().map({
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

pub(super) fn render_performance(
    controller: &PreviewController,
    timeline: &VecDeque<TimelineSample>,
    scroll_handle: &ScrollHandle,
) -> AnyElement {
    let snapshot = controller.snapshot();
    let latest = timeline.back().copied().unwrap_or(TimelineSample {
        published: snapshot.frame_stats.published,
        overwritten: snapshot.frame_stats.overwritten,
    });
    let deltas = timeline
        .iter()
        .zip(timeline.iter().skip(1))
        .map(|(before, after)| after.published.saturating_sub(before.published))
        .collect::<Vec<_>>();
    let peak = deltas.iter().copied().max().unwrap_or(1).max(1);
    let content = div()
        .flex()
        .flex_col()
        .p_3()
        .gap_3()
        .child(section_label("PREVIEW TRANSPORT"))
        .child(property_row(
            "Published frames",
            latest.published.to_string(),
        ))
        .child(property_row(
            "Overwritten frames",
            latest.overwritten.to_string(),
        ))
        .child(property_row("Samples", timeline.len().to_string()))
        .child(
            div()
                .h(px(84.))
                .flex()
                .items_end()
                .gap(px(2.))
                .px_1()
                .rounded(px(8.))
                .bg(rgb(CANVAS))
                .children(deltas.into_iter().map(|delta| {
                    let height = 4. + (delta as f32 / peak as f32) * 68.;
                    div()
                        .w(px(5.))
                        .h(px(height))
                        .rounded(px(2.))
                        .bg(rgb(PRIMARY))
                })),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child("500 ms transport samples · not CPU/GPU frame time"),
        );
    vertical_overflow_view("performance-scroll", scroll_handle, content)
}

pub(super) fn multi_block_summary(source: &str, starts: &[usize]) -> Option<AnyElement> {
    let selected = EiyashouProjection::parse(source)
        .scenes
        .into_iter()
        .flat_map(|scene| {
            let name = scene.name;
            scene
                .blocks
                .into_iter()
                .filter(|block| starts.contains(&block.source_range.start))
                .map(move |block| (name.clone(), block))
        })
        .collect::<Vec<_>>();
    if selected.len() < 2 {
        return None;
    }
    let mut properties = vec![
        ("Blocks", selected.len().to_string()),
        (
            "Type",
            common_value(
                selected
                    .iter()
                    .map(|(_, block)| block.kind.label().to_owned()),
            ),
        ),
        (
            "Scene",
            common_value(selected.iter().map(|(scene, _)| scene.clone())),
        ),
    ];
    let all_text = selected.iter().all(|(_, block)| {
        matches!(
            block.kind,
            BlockKind::Narration | BlockKind::Dialogue { .. }
        )
    });
    if all_text {
        properties.push((
            "Speaker",
            common_value(selected.iter().map(|(_, block)| match &block.kind {
                BlockKind::Narration => "Narrator".to_owned(),
                BlockKind::Dialogue { speaker } => speaker.clone(),
                _ => unreachable!("guarded above"),
            })),
        ));
        properties.push((
            "Voice",
            common_value(
                selected.iter().map(|(_, block)| {
                    text_voice(&block.summary).unwrap_or_else(|| "None".to_owned())
                }),
            ),
        ));
    }
    properties.push((
        "Stable ID",
        common_value(
            selected
                .iter()
                .map(|(_, block)| block.stable_id.clone().unwrap_or_else(|| "None".to_owned())),
        ),
    ));
    Some(
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(
                properties
                    .into_iter()
                    .map(|(label, value)| property_row(label, value)),
            )
            .into_any_element(),
    )
}

pub(super) fn selection_summary(root: &Path, index: &AuthoringIndex, cx: &App) -> AnyElement {
    match cx.global::<EditorDocuments>().selection(root) {
        Some((path, line, column)) => {
            let diagnostics = cx
                .global::<EditorDocuments>()
                .diagnostics_for(root, path)
                .take(3)
                .map(|diagnostic| {
                    let color = match diagnostic.level {
                        keine_authoring::DiagnosticLevel::Warning => 0xd2aa62,
                        keine_authoring::DiagnosticLevel::Error => 0xdb7780,
                    };
                    div().text_color(rgb(color)).child(format!(
                        "{}:{}  {}",
                        diagnostic.line, diagnostic.column, diagnostic.message
                    ))
                })
                .collect::<Vec<_>>();
            if let Some((selected_path, starts)) =
                cx.global::<EditorDocuments>().block_selection(root)
                && selected_path == path
                && let Some(source) = cx.global::<EditorDocuments>().source(root, path)
                && let Some(summary) = multi_block_summary(&source, starts)
            {
                return div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(path.display().to_string()),
                    )
                    .child(summary)
                    .children(diagnostics)
                    .into_any_element();
            }
            if path.extension().is_some_and(|value| value == "shou")
                && let Some(source) = cx.global::<EditorDocuments>().source(root, path)
                && let Some((scene, block)) = projected_block_at(&source, *line, *column)
            {
                let mut properties = vec![
                    ("Type", block_card_label(&block.kind, &block.summary)),
                    ("Scene", scene),
                ];
                if !matches!(
                    &block.kind,
                    BlockKind::Narration | BlockKind::Dialogue { .. }
                ) && let Some(stable_id) = &block.stable_id
                {
                    properties.push(("Stable ID", stable_id.clone()));
                }
                return div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(path.display().to_string()),
                    )
                    .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                        "Line {}, column {}",
                        line + 1,
                        column + 1
                    )))
                    .children(
                        properties
                            .into_iter()
                            .map(|(label, value)| property_row(label, value)),
                    )
                    .children(diagnostics)
                    .into_any_element();
            }
            let authoring = match index.selection(path, *line) {
                AuthoringSelection::Source => "Source selection".to_owned(),
                AuthoringSelection::Scene(scene) => format!("Scene · {}", scene.name),
                AuthoringSelection::Dialogue(dialogue) => format!(
                    "{} · {}",
                    if dialogue.speaker.is_empty() {
                        "Narration"
                    } else {
                        dialogue.speaker.as_str()
                    },
                    dialogue.text
                ),
            };
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_xs()
                .text_color(rgb(0xb8c4cf))
                .child(path.display().to_string())
                .child(format!("Line {}, column {}", line + 1, column + 1))
                .child(div().text_color(rgb(PRIMARY)).child(authoring))
                .children(diagnostics)
                .into_any_element()
        }
        None => div()
            .text_xs()
            .text_color(rgb(MUTED))
            .child("No source selection")
            .into_any_element(),
    }
}
