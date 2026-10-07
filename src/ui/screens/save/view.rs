//! Save_Load view; registered through the parent facade.
use super::*;

pub(super) fn spawn_save_content(
    root: &mut ChildSpawnerCommands,
    ui: &SaveLoadUi,
    mode: SaveLoadMode,
    context: &mut SaveContentContext,
) {
    root.spawn((
        SaveLoadContent,
        UiTransform::default(),
        Node {
            position_type: PositionType::Relative,
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            ..default()
        },
    ))
    .with_children(|content| {
        content
            .spawn((Node {
                width: Val::Percent(100.0),
                height: Val::Percent(7.0),
                // Match Config's 13.5 px content inset without moving the
                // save-slot grid below it.
                margin: UiRect {
                    top: Val::Px(13.5),
                    bottom: Val::Px(19.5),
                    ..default()
                },
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },))
            .with_children(|pages| {
                for page in 1..=PAGE_COUNT {
                    spawn_page_button(pages, page, ui.page, context.font);
                }
            });
        content
            .spawn((
                SaveLoadGridViewport,
                Node {
                    position_type: PositionType::Relative,
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|viewport| {
                spawn_slot_grid(viewport, ui, mode, SaveLoadGridPhase::Settled, context)
            });
    });
}

pub(super) fn spawn_slot_grid(
    content: &mut ChildSpawnerCommands,
    ui: &SaveLoadUi,
    mode: SaveLoadMode,
    phase: SaveLoadGridPhase,
    context: &mut SaveContentContext,
) {
    let first = (ui.page - 1) * SLOTS_PER_PAGE + 1;
    let last = first + SLOTS_PER_PAGE;
    context
        .preview_cache
        .ready
        .retain(|slot, _| (first..last).contains(slot));
    context
        .preview_cache
        .pending
        .retain(|slot, _| (first..last).contains(slot));
    content
        .spawn((
            SaveLoadSlotGrid { phase },
            UiTransform::default(),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::flex(5, 1.0),
                grid_template_rows: RepeatedGridTrack::flex(2, 1.0),
                column_gap: Val::Percent(1.8),
                row_gap: Val::Percent(4.0),
                ..default()
            },
        ))
        .with_children(|grid| {
            for slot in first..first + SLOTS_PER_PAGE {
                let preview = request_preview(context.project_root, slot, context.preview_cache);
                spawn_slot(grid, slot, mode, context, preview);
            }
        });
}

pub(super) fn spawn_page_button(
    pages: &mut ChildSpawnerCommands,
    page: u32,
    selected_page: u32,
    font: &Handle<Font>,
) {
    let selected = page == selected_page;
    pages
        .spawn((
            Button,
            UiSoundStyle::Switch,
            SaveLoadPage(page),
            SaveLoadPageVisual {
                selected,
                text: if selected {
                    PAGE_HIGHLIGHT_TEXT_ALPHA
                } else {
                    0.2
                },
                press: 0.0,
            },
            HoverAlpha {
                target: page_highlight_alpha(selected, false),
                current: page_highlight_alpha(selected, false),
                active: selected,
                active_alpha: SURFACE_ACTIVE_ALPHA,
                hover_alpha: SURFACE_HOVER_ALPHA,
                ..default()
            },
            UiTransform::default(),
            Node {
                width: Val::Px(60.0),
                height: Val::Px(58.5),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(button_surface(page_highlight_alpha(selected, false))),
        ))
        .with_child((
            SaveLoadPageLabel,
            text_weight(
                page.to_string(),
                font,
                24.0,
                if selected {
                    PAGE_HIGHLIGHT_TEXT_ALPHA
                } else {
                    0.2
                },
                if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                },
            ),
        ));
}

pub(super) fn spawn_slot(
    grid: &mut ChildSpawnerCommands,
    slot: u32,
    mode: SaveLoadMode,
    context: &SaveContentContext,
    preview: Option<Handle<Image>>,
) {
    use crate::storage::save::SlotStatus;

    let status = crate::storage::save::inspect_slot(context.store, slot, context.project_root);
    let empty = matches!(status, SlotStatus::Empty);
    let compatible = matches!(
        &status,
        SlotStatus::Ready(metadata)
            if metadata.program_fingerprint == context.program_fingerprint
    );
    let preview = compatible.then_some(preview).flatten();
    let enabled = mode == SaveLoadMode::Save || compatible;
    let ready = compatible;
    let primary_text_alpha = if ready {
        0.72
    } else if empty {
        0.34
    } else {
        0.28
    };
    let secondary_text_alpha = if ready {
        0.58
    } else if empty {
        0.28
    } else {
        0.24
    };
    let base_alpha = if enabled { 0.11 } else { 0.06 };
    let detail = match &status {
        SlotStatus::Empty => String::new(),
        SlotStatus::Corrupt => "CORRUPT SLOT\nSave here to replace it".to_owned(),
        SlotStatus::Unsupported(version) => {
            format!("NEWER SAVE · v{version}\nCannot load in this engine")
        }
        SlotStatus::Ready(_) if !compatible => {
            "DIFFERENT SCRIPT BUILD\nSave here to replace it".to_owned()
        }
        SlotStatus::Ready(meta) => meta.text.clone(),
    };
    grid.spawn((
        Button,
        SaveLoadSlot(slot),
        SaveLoadSlotMotion { scale: 1.0 },
        Interaction::None,
        UiTransform::default(),
        Node {
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(if empty {
            empty_slot_surface()
        } else {
            Color::srgba(0.0, 0.0, 0.0, base_alpha)
        }),
    ))
    .with_children(|slot_node| {
        slot_node.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(12.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                ..default()
            },
            children![
                (
                    Node {
                        width: Val::Percent(22.0),
                        height: Val::Percent(100.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(if empty {
                        Color::NONE
                    } else {
                        Color::srgba(0.0, 0.0, 0.0, 0.48)
                    }),
                    children![text_weight(
                        slot.to_string(),
                        context.font,
                        21.75,
                        primary_text_alpha,
                        FontWeight::BOLD,
                    )]
                ),
                (
                    Node {
                        width: Val::Percent(78.0),
                        height: Val::Percent(100.0),
                        padding: UiRect::left(Val::Px(10.5)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(if empty {
                        Color::NONE
                    } else {
                        Color::srgba(0.0, 0.0, 0.0, 0.32)
                    }),
                    children![text(
                        slot_time(&status),
                        context.font,
                        17.25,
                        secondary_text_alpha,
                    )]
                )
            ],
        ));
        slot_node.spawn((
            SaveLoadPreviewImage(slot),
            preview.map_or_else(ImageNode::default, ImageNode::new),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(48.0),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        slot_node.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(40.0),
                padding: UiRect::axes(Val::Px(10.5), Val::Px(6.75)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.25),
                ..default()
            },
            BackgroundColor(if empty {
                Color::NONE
            } else {
                Color::srgba(0.0, 0.0, 0.0, 0.52)
            }),
            children![
                text_weight(
                    slot_speaker(&status),
                    context.font,
                    19.5,
                    primary_text_alpha,
                    FontWeight::BOLD,
                ),
                text(detail, context.font, 17.25, secondary_text_alpha)
            ],
        ));
    });
}

pub(super) fn slot_time(status: &crate::storage::save::SlotStatus) -> String {
    match status {
        crate::storage::save::SlotStatus::Ready(metadata) => relative_time(metadata.saved_at_unix),
        _ => String::new(),
    }
}

pub(super) fn slot_speaker(status: &crate::storage::save::SlotStatus) -> String {
    match status {
        crate::storage::save::SlotStatus::Ready(metadata) if !metadata.speaker.is_empty() => {
            metadata.speaker.clone()
        }
        crate::storage::save::SlotStatus::Ready(_) => " ".into(),
        _ => String::new(),
    }
}

pub(super) fn relative_time(saved_at_unix: u64) -> String {
    if saved_at_unix == 0 {
        return "legacy save".into();
    }
    format_utc(saved_at_unix)
}

pub(super) fn format_utc(timestamp: u64) -> String {
    let days = (timestamp / 86_400) as i64;
    let seconds = timestamp % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_piece = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_piece + 2) / 5 + 1;
    let month = month_piece + if month_piece < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!(
        "{year}/{month}/{day} {:02}:{:02}:{:02}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}
