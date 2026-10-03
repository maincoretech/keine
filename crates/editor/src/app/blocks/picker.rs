//! Block insertion palette, context menus and persisted ordering preferences.
use super::*;

pub(in crate::app) const PICKER_CATEGORIES: [&str; 5] = ["Text", "Scene", "Media", "Flow", "Data"];

pub(in crate::app) fn toggle_preference(values: &mut Vec<String>, value: &str) {
    if let Some(index) = values.iter().position(|candidate| candidate == value) {
        values.remove(index);
    } else {
        values.push(value.to_owned());
    }
}

pub(in crate::app) fn move_preference(
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

pub(in crate::app) fn move_group_preference(
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

pub(in crate::app) fn preference_rank(values: &[String], value: &str, fallback: usize) -> usize {
    values
        .iter()
        .position(|candidate| candidate == value)
        .unwrap_or(values.len() + fallback)
}

pub(in crate::app) fn ordered_picker_categories(
    preferences: &BlockPickerPreferences,
) -> Vec<&'static str> {
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

pub(in crate::app) fn picker_kinds(
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

pub(in crate::app) fn insert_kind_icon(kind: InsertKind) -> AssetIconName {
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
        InsertKind::Move | InsertKind::CameraMove | InsertKind::CameraShake => AssetIconName::Move,
        InsertKind::SpriteFocusRule | InsertKind::SpriteFocus => AssetIconName::PersonStanding,
        InsertKind::Bgm => AssetIconName::Music,
        InsertKind::Effect => AssetIconName::Volume2,
        InsertKind::Video => AssetIconName::Film,
        InsertKind::Native(name) => match name {
            "avatar.show" | "avatar.hide" => AssetIconName::PersonStanding,
            "vocal.play" | "vocal.stop" => AssetIconName::Volume2,
            "screen.film"
            | "video.stop"
            | "video.play"
            | "screen.curtain.show"
            | "screen.curtain.hide" => AssetIconName::Film,
            "se.loop" | "se.stop" => AssetIconName::Volume2,
            "text.box"
            | "text.presentation"
            | "text.retract"
            | "text.float.hide"
            | "text.float.configure"
            | "text.style"
            | "text.float"
            | "text.intro"
            | "text.paragraph.style" => AssetIconName::MessageSquareText,
            "wait.advance" => AssetIconName::Clock,
            "particle.hide" | "particle.layers.clear" | "particle.show" => AssetIconName::EyeOff,
            "camera.bind"
            | "camera.unbind"
            | "scene.parallax.stop"
            | "scene.parallax"
            | "camera.effect"
            | "camera.reset" => AssetIconName::Move,
            "stage.animate" => AssetIconName::Move,
            "sprite.transform" | "background.transform" => AssetIconName::Move,
            "sprite.animate" | "sprite.transition" | "stage.mask.show" | "stage.mask.hide" => {
                AssetIconName::Image
            }
            "sprite.sequence" | "sprite.select" | "sprite.select.when" | "sprite.keyframes"
            | "sprite.update" => AssetIconName::Image,
            "assets.loading" => AssetIconName::Workflow,
            "gallery.unlock" => AssetIconName::Image,
            "input.request" => AssetIconName::Braces,
            _ => AssetIconName::Workflow,
        },
    }
}

pub(in crate::app) fn render_block_picker(
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

pub(in crate::app) fn render_block_context_menu(
    row: usize,
    position: Point<Pixels>,
    source: String,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let block = EiyashouProjection::parse(&source)
        .scenes
        .into_iter()
        .flat_map(|scene| scene.blocks)
        .find(|block| block.source_range.start == row);
    let mut items = vec![
        (
            BlockMenuAction::Run,
            AssetIconName::Play,
            "Run to here",
            false,
        ),
        (BlockMenuAction::Copy, AssetIconName::Copy, "Copy", false),
        (
            BlockMenuAction::Duplicate,
            AssetIconName::Copy,
            "Duplicate",
            false,
        ),
        (BlockMenuAction::Cut, AssetIconName::Scissors, "Cut", false),
        (
            BlockMenuAction::Paste,
            AssetIconName::Clipboard,
            "Paste",
            false,
        ),
        (
            BlockMenuAction::SelectAll,
            AssetIconName::Square,
            "Select all",
            false,
        ),
        (
            BlockMenuAction::InsertAbove,
            AssetIconName::ArrowUp,
            "Insert above",
            false,
        ),
        (
            BlockMenuAction::InsertBelow,
            AssetIconName::ArrowDown,
            "Insert below",
            false,
        ),
        (
            BlockMenuAction::MoveUp,
            AssetIconName::ArrowUp,
            "Move up",
            false,
        ),
        (
            BlockMenuAction::MoveDown,
            AssetIconName::ArrowDown,
            "Move down",
            false,
        ),
        (
            BlockMenuAction::Delete,
            AssetIconName::Delete,
            "Delete",
            true,
        ),
    ];
    if let Some(block) = &block
        && !matches!(
            block.kind,
            BlockKind::Narration | BlockKind::Dialogue { .. } | BlockKind::Else | BlockKind::ElseIf
        )
    {
        items.insert(
            6,
            (
                BlockMenuAction::ToggleDisabled,
                if block.disabled {
                    AssetIconName::Eye
                } else {
                    AssetIconName::EyeOff
                },
                if block.disabled { "Enable" } else { "Disable" },
                false,
            ),
        );
    }
    deferred(
        anchored()
            .anchor(Anchor::TopLeft)
            .position(position)
            .snap_to_window_with_margin(px(6.))
            .child(
                div()
                    .id("block-context-menu")
                    .w(px(188.))
                    .p_1()
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.block_context_menu = None;
                        cx.notify();
                    }))
                    .children(items.into_iter().enumerate().map(
                        |(index, (action, icon, label, danger))| {
                            let source = source.clone();
                            div()
                                .id(("block-context-item", index))
                                .w_full()
                                .h(px(27.))
                                .when(
                                    matches!(
                                        action,
                                        BlockMenuAction::Copy
                                            | BlockMenuAction::ToggleDisabled
                                            | BlockMenuAction::InsertAbove
                                            | BlockMenuAction::MoveUp
                                            | BlockMenuAction::Delete
                                    ),
                                    |this| this.mt_1().border_t_1().border_color(rgb(BORDER)),
                                )
                                .px_2()
                                .rounded(px(3.))
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_size(px(12.))
                                .text_color(rgb(if danger { 0xdb7780 } else { INK }))
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.block_menu_action(row, &source, action, window, cx);
                                }))
                                .child(Icon::new(icon).xsmall())
                                .child(label)
                        },
                    )),
            ),
    )
    .priority(100)
    .into_any_element()
}

pub(in crate::app) fn render_scene_context_menu(
    menu: SceneContextMenu,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let start = menu.start;
    let name = menu.name.clone();
    let closing = menu.closing;
    let motion_duration = if closing { 90 } else { 120 };
    let surface = div()
        .id(("scene-context-surface", menu.epoch))
        .w(px(SCENE_CONTEXT_MENU_WIDTH_PX))
        .h(px(SCENE_CONTEXT_MENU_HEIGHT_PX))
        .overflow_hidden()
        .p_1()
        .rounded(px(7.))
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(SURFACE))
        .shadow_lg()
        .flex()
        .flex_col()
        .child(
            file_context_menu_item("scene-context-new", AssetIconName::Plus, "New", false)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_scene_context_menu(window, cx);
                    this.begin_scene_edit(SceneEditMode::New, window, cx);
                })),
        )
        .child(
            file_context_menu_item(
                "scene-context-rename",
                AssetIconName::Replace,
                "Rename",
                false,
            )
            .on_click(cx.listener({
                let name = name.clone();
                move |this, _, window, cx| {
                    this.close_scene_context_menu(window, cx);
                    this.begin_scene_edit(
                        SceneEditMode::Rename {
                            start,
                            old_name: name.clone(),
                        },
                        window,
                        cx,
                    );
                }
            })),
        )
        .child(
            file_context_menu_item("scene-context-up", AssetIconName::ArrowUp, "Move up", false)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_scene_context_menu(window, cx);
                    this.move_scene_from_menu(start, MoveDirection::Up, window, cx);
                })),
        )
        .child(
            file_context_menu_item(
                "scene-context-down",
                AssetIconName::ArrowDown,
                "Move down",
                false,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.close_scene_context_menu(window, cx);
                this.move_scene_from_menu(start, MoveDirection::Down, window, cx);
            })),
        )
        .child(
            file_context_menu_item(
                "scene-context-delete",
                AssetIconName::Delete,
                "Delete",
                true,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.close_scene_context_menu(window, cx);
                this.confirm_delete_scene(start, name.clone(), window, cx);
            })),
        )
        .with_animation(
            ("scene-context-motion", menu.epoch),
            Animation::new(Duration::from_millis(motion_duration)).with_easing(ease_out_quint()),
            move |surface, delta| {
                let progress = if closing { 1. - delta } else { delta };
                let scale = 0.94 + progress * 0.06;
                surface
                    .opacity(progress)
                    .w(px(SCENE_CONTEXT_MENU_WIDTH_PX * scale))
                    .h(px(SCENE_CONTEXT_MENU_HEIGHT_PX * scale))
            },
        );
    deferred(
        anchored()
            .anchor(Anchor::TopLeft)
            .position(menu.position)
            .snap_to_window_with_margin(px(6.))
            .child(
                div()
                    .w(px(SCENE_CONTEXT_MENU_WIDTH_PX))
                    .h(px(SCENE_CONTEXT_MENU_HEIGHT_PX))
                    .on_mouse_down_out(
                        cx.listener(|this, _, window, cx| {
                            this.close_scene_context_menu(window, cx)
                        }),
                    )
                    .child(surface),
            ),
    )
    .priority(100)
    .into_any_element()
}
