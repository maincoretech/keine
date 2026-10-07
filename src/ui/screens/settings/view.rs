//! Settings view; registered through the parent facade.
use super::*;

pub(crate) fn menu_watermark(label: &str, font: &Handle<Font>) -> impl Bundle {
    (
        Name::new("menu_watermark"),
        SettingsWatermark::entering(),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(39.0),
            bottom: Val::Px(18.0),
            ..default()
        },
        FocusPolicy::Pass,
        crate::ui::text_style::NoTextShadow,
        Text::new(label),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::from(240.0),
            weight: FontWeight::BOLD,
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.0)),
    )
}

pub(crate) fn spawn_menu_watermark(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    font: &Handle<Font>,
) {
    parent.spawn(menu_watermark(label, font));
}

pub(super) fn spawn_options_content(
    root: &mut ChildSpawnerCommands,
    ui: &SettingsUi,
    settings: &RuntimeSettings,
    config: &GameConfigResource,
    project: &ContentProjectResource,
    development: bool,
    font: &Handle<Font>,
) {
    root.spawn((
        SettingsContent,
        UiTransform::default(),
        Node {
            position_type: PositionType::Relative,
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            ..default()
        },
    ))
    .with_children(|options| {
        options
            .spawn((Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                padding: UiRect::axes(Val::Px(27.0), Val::Px(13.5)),
                ..default()
            },))
            .with_children(|body| {
                body.spawn((Node {
                    width: Val::Percent(12.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    ..default()
                },))
                    .with_children(|pages| {
                        let locale = settings.locale;
                        spawn_page_button(
                            pages,
                            tr(locale, UiText::System),
                            SettingsPage::System,
                            ui,
                            font,
                        );
                        spawn_page_button(
                            pages,
                            tr(locale, UiText::Display),
                            SettingsPage::Display,
                            ui,
                            font,
                        );
                        spawn_page_button(
                            pages,
                            tr(locale, UiText::Audio),
                            SettingsPage::Audio,
                            ui,
                            font,
                        );
                        spawn_page_button(
                            pages,
                            tr(locale, UiText::About),
                            SettingsPage::About,
                            ui,
                            font,
                        );
                    });
                body.spawn((Node {
                    position_type: PositionType::Relative,
                    flex_grow: 1.0,
                    height: Val::Percent(100.0),
                    padding: UiRect::left(Val::Px(24.0)),
                    overflow: Overflow::visible(),
                    ..default()
                },))
                    .with_children(|content| {
                        for page in [
                            SettingsPage::System,
                            SettingsPage::Display,
                            SettingsPage::Audio,
                            SettingsPage::About,
                        ] {
                            spawn_settings_page(
                                content,
                                page,
                                ui,
                                settings,
                                SettingsProjectContext {
                                    config,
                                    content: project,
                                },
                                development,
                                font,
                            );
                        }
                    });
            });
    });
}

pub(super) fn spawn_page_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    page: SettingsPage,
    ui: &SettingsUi,
    font: &Handle<Font>,
) {
    let active = ui.page == page;
    parent.spawn((
        Button,
        UiSoundStyle::Switch,
        SettingsPageButton(page),
        SettingsPageButtonVisual(if active {
            PAGE_TEXT_ACTIVE
        } else {
            PAGE_TEXT_IDLE
        }),
        Node {
            height: Val::Px(63.0),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(4.5)),
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![(
            SettingsPageLabel,
            Text::new(label),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::from(36.0),
                weight: FontWeight::BOLD,
                ..default()
            },
            TextColor(Color::srgba(
                1.0,
                1.0,
                1.0,
                if active {
                    PAGE_TEXT_ACTIVE
                } else {
                    PAGE_TEXT_IDLE
                },
            )),
        )],
    ));
}

pub(super) fn spawn_settings_page(
    parent: &mut ChildSpawnerCommands,
    page: SettingsPage,
    ui: &SettingsUi,
    settings: &RuntimeSettings,
    project: SettingsProjectContext<'_>,
    development: bool,
    font: &Handle<Font>,
) {
    let active = ui.page == page;
    parent
        .spawn((
            SettingsPagePanel { page },
            UiTransform::default(),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(78.0),
                height: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(21.0), Val::Px(9.0)),
                grid_template_columns: RepeatedGridTrack::flex(SETTINGS_COLUMNS, 1.0),
                column_gap: Val::Px(SETTINGS_COLUMN_GAP),
                row_gap: Val::Px(SETTINGS_ROW_GAP),
                align_items: AlignItems::FlexStart,
                align_content: AlignContent::FlexStart,
                // All pages use the same available width. Reserving a scroll
                // gutter only on DISPLAY/AUDIO shifted their column tracks.
                overflow: Overflow::visible(),
                display: if active {
                    settings_page_display(page)
                } else {
                    Display::None
                },
                ..default()
            },
        ))
        .with_children(|content| match page {
            SettingsPage::System => spawn_system_page(content, settings, font),
            SettingsPage::Display => {
                let first_slider = if cfg!(target_os = "android") { 1 } else { 2 };
                if !cfg!(target_os = "android") {
                    spawn_fullscreen_row(content, settings, font, SettingsGridCell::at(1, 1));
                }
                spawn_text_size_row(
                    content,
                    settings,
                    font,
                    SettingsGridCell::from_index(first_slider - 1),
                );
                spawn_sliders(content, DISPLAY_SLIDERS, settings, font, first_slider);
                let preview_row = if cfg!(target_os = "android") { 2 } else { 3 };
                spawn_text_preview(
                    content,
                    settings,
                    font,
                    SettingsGridCell::spanning(1, preview_row, 3),
                );
            }
            SettingsPage::Audio => spawn_sliders(content, AUDIO_SLIDERS, settings, font, 0),
            SettingsPage::About => {
                spawn_about_page(content, project, development, settings.locale, font)
            }
        });
}

pub(super) fn spawn_system_page(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
) {
    content
        .spawn((Node {
            width: Val::Percent(100.0),
            grid_column: SettingsGridCell::spanning(1, 1, SETTINGS_COLUMNS).column(),
            grid_row: SettingsGridCell::at(1, 1).row(),
            display: Display::Grid,
            grid_template_columns: RepeatedGridTrack::flex(2, 1.0),
            column_gap: Val::Px(SETTINGS_COLUMN_GAP),
            row_gap: Val::Px(SETTINGS_ROW_GAP),
            align_items: AlignItems::FlexStart,
            align_content: AlignContent::FlexStart,
            ..default()
        },))
        .with_children(|grid| {
            spawn_system_playback_controls(grid, settings, font);
            spawn_language_row(grid, settings, font, SYSTEM_LANGUAGE_CELL);
            spawn_system_data_controls(grid, settings, font);
        });
}

pub(super) fn settings_page_display(page: SettingsPage) -> Display {
    if page == SettingsPage::About {
        Display::Flex
    } else {
        Display::Grid
    }
}

pub(super) fn spawn_sliders(
    content: &mut ChildSpawnerCommands,
    kinds: &[SettingKind],
    settings: &RuntimeSettings,
    font: &Handle<Font>,
    start_index: usize,
) {
    for (offset, kind) in kinds.iter().copied().enumerate() {
        spawn_row(
            content,
            font,
            tr(settings.locale, kind.label()),
            kind,
            kind.ratio(settings),
            SettingsGridCell::from_index(start_index + offset),
        );
    }
}

pub(super) fn spawn_about_page(
    content: &mut ChildSpawnerCommands,
    project: SettingsProjectContext<'_>,
    development: bool,
    locale: UiLocale,
    font: &Handle<Font>,
) {
    let config = project.config;
    let project_description = if config.project.description.trim().is_empty() {
        tr(locale, UiText::NoProjectDescription)
    } else {
        config.project.description.trim()
    };
    let loader_tree = loader_tree(project);
    let runtime = format!(
        "Kēne {}  ·  {} / {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );

    content
        .spawn((Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            padding: UiRect::new(Val::Px(9.0), Val::Px(36.0), Val::Px(9.0), Val::Px(18.0)),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(60.0),
            ..default()
        },))
        .with_children(|page| {
            page.spawn((Node {
                width: Val::Percent(48.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                ..default()
            },))
                .with_children(|engine| {
                    engine.spawn(about_title(tr(locale, UiText::AboutKeine), font, 48.0));
                    engine.spawn(about_copy(
                        tr(locale, UiText::EngineDescription),
                        font,
                        22.5,
                        0.58,
                    ));
                    spawn_about_section(
                        engine,
                        tr(locale, UiText::VersionSystem),
                        &runtime,
                        font,
                        Val::Px(42.0),
                    );
                    spawn_repository_link(engine, locale, font);
                });

            page.spawn((Node {
                width: Val::Percent(46.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                ..default()
            },))
                .with_children(|project| {
                    project.spawn(about_title(tr(locale, UiText::CurrentProject), font, 39.0));
                    project.spawn(about_title(config.title.clone(), font, 31.5));
                    project.spawn(about_copy(project_description, font, 22.5, 0.62));
                    if development {
                        spawn_about_section(
                            project,
                            tr(locale, UiText::BuildTime),
                            env!("KEINE_BUILD_TIME"),
                            font,
                            Val::Px(30.0),
                        );
                        spawn_about_section(
                            project,
                            tr(locale, UiText::Loader),
                            &loader_tree,
                            font,
                            Val::Px(27.0),
                        );
                    }
                });
        });
}

pub(super) fn loader_tree(project: SettingsProjectContext<'_>) -> String {
    let config = project.config;
    let mut lines = vec!["ASSET".to_owned()];
    for source in &config.adapter.asset {
        lines.push(format!(
            "  {}",
            format_loader_source(&source.format, &source.path)
        ));
    }
    let script = project
        .content
        .project_adapter()
        .unwrap_or(config.adapter.script.as_str());
    lines.push(format!("SCRIPT\n  [{script}]"));
    lines.push(format!("STORE\n  [{}]", config.adapter.store));
    lines.join("\n")
}

pub(super) fn format_loader_source(adapter: &str, path: &str) -> String {
    let path = path.trim().trim_end_matches('/');
    if path.is_empty() || path == "." {
        return format!("[{adapter}]");
    }
    let path = path.strip_prefix("./").unwrap_or(path);
    format!("[{adapter}]/{path}")
}

pub(super) fn spawn_repository_link(
    parent: &mut ChildSpawnerCommands,
    locale: UiLocale,
    font: &Handle<Font>,
) {
    parent
        .spawn((Node {
            width: Val::Percent(100.0),
            margin: UiRect::top(Val::Px(27.0)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(7.5),
            ..default()
        },))
        .with_children(|section| {
            section.spawn(about_title(tr(locale, UiText::Repository), font, 25.5));
            section
                .spawn((
                    Button,
                    AboutRepositoryLink,
                    AboutRepositoryVisual {
                        underline_width: 0.0,
                        text_alpha: 0.58,
                    },
                    Node {
                        position_type: PositionType::Relative,
                        align_self: AlignSelf::FlexStart,
                        padding: UiRect::vertical(Val::Px(6.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|link| {
                    link.spawn((
                        AboutRepositoryUnderline,
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::ZERO,
                            bottom: Val::ZERO,
                            width: Val::ZERO,
                            height: Val::Px(1.5),
                            ..default()
                        },
                        FocusPolicy::Pass,
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
                    ));
                    link.spawn((
                        AboutRepositoryLabel,
                        ZIndex(1),
                        text_weight(
                            "github.com/maincoretech/keine",
                            font,
                            20.25,
                            0.58,
                            FontWeight::NORMAL,
                        ),
                    ));
                });
        });
}

pub(super) fn spawn_about_section(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    value: &str,
    font: &Handle<Font>,
    top: Val,
) {
    parent
        .spawn((Node {
            width: Val::Percent(100.0),
            margin: UiRect::top(top),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(7.5),
            ..default()
        },))
        .with_children(|section| {
            section.spawn(about_title(label, font, 25.5));
            section.spawn(about_copy(value, font, 20.25, 0.52));
        });
}

pub(super) fn about_title(text: impl Into<String>, font: &Handle<Font>, size: f32) -> impl Bundle {
    text_weight(text, font, size, 0.76, FontWeight::BOLD)
}

pub(super) fn about_copy(
    text: impl Into<String>,
    font: &Handle<Font>,
    size: f32,
    alpha: f32,
) -> impl Bundle {
    (
        Node {
            max_width: Val::Percent(100.0),
            ..default()
        },
        text_weight(text, font, size, alpha, FontWeight::NORMAL),
    )
}

pub(super) fn spawn_skip_row(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
) {
    spawn_choice_group(
        content,
        tr(settings.locale, UiText::SkipMode),
        &[
            (
                tr(settings.locale, UiText::Read),
                SettingAction::SetSkip(false),
            ),
            (
                tr(settings.locale, UiText::All),
                SettingAction::SetSkip(true),
            ),
        ],
        usize::from(settings.skip_all),
        false,
        font,
        None,
    );
}

pub(super) fn spawn_system_playback_controls(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
) {
    let auto_play = SettingKind::AutoDelay;
    content
        .spawn((Node {
            width: Val::Percent(100.0),
            grid_column: SYSTEM_PLAYBACK_CELL.column(),
            grid_row: SYSTEM_PLAYBACK_CELL.row(),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(SETTINGS_ROW_GAP),
            align_items: AlignItems::FlexStart,
            ..default()
        },))
        .with_children(|column| {
            spawn_slider_group(
                column,
                font,
                tr(settings.locale, auto_play.label()),
                auto_play,
                auto_play.ratio(settings),
                None,
            );
            spawn_skip_row(column, settings, font);
        });
}

pub(super) fn spawn_language_row(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
    cell: SettingsGridCell,
) {
    let choices =
        UiLocale::ALL.map(|locale| (locale.native_name(), SettingAction::SetLanguage(locale)));
    let selected = UiLocale::ALL
        .iter()
        .position(|locale| *locale == settings.locale)
        .unwrap_or_default();
    spawn_choice_row(
        content,
        tr(settings.locale, UiText::Language),
        &choices,
        selected,
        true,
        font,
        cell,
    );
}

pub(super) fn spawn_fullscreen_row(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
    cell: SettingsGridCell,
) {
    spawn_choice_row(
        content,
        tr(settings.locale, UiText::Fullscreen),
        &[
            (
                tr(settings.locale, UiText::On),
                SettingAction::SetFullscreen(true),
            ),
            (
                tr(settings.locale, UiText::Off),
                SettingAction::SetFullscreen(false),
            ),
        ],
        if settings.fullscreen { 0 } else { 1 },
        false,
        font,
        cell,
    );
}

pub(super) fn spawn_text_size_row(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
    cell: SettingsGridCell,
) {
    spawn_choice_row(
        content,
        tr(settings.locale, UiText::TextSize),
        &[
            (
                tr(settings.locale, UiText::Small),
                SettingAction::SetTextSize(0),
            ),
            (
                tr(settings.locale, UiText::Medium),
                SettingAction::SetTextSize(1),
            ),
            (
                tr(settings.locale, UiText::Large),
                SettingAction::SetTextSize(2),
            ),
        ],
        usize::from(settings.text_size),
        false,
        font,
        cell,
    );
}

pub(super) fn spawn_choice_row(
    content: &mut ChildSpawnerCommands,
    label: &str,
    choices: &[(&str, SettingAction)],
    selected: usize,
    vertical: bool,
    font: &Handle<Font>,
    cell: SettingsGridCell,
) {
    spawn_choice_group(
        content,
        label,
        choices,
        selected,
        vertical,
        font,
        Some(cell),
    );
}

pub(super) fn spawn_choice_group(
    content: &mut ChildSpawnerCommands,
    label: &str,
    choices: &[(&str, SettingAction)],
    selected: usize,
    vertical: bool,
    font: &Handle<Font>,
    cell: Option<SettingsGridCell>,
) {
    content
        .spawn((setting_group_node(cell, 84.0),))
        .with_children(|row| {
            spawn_choice_content(row, label, choices, selected, vertical, font);
        });
}

pub(super) fn spawn_system_data_controls(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
) {
    spawn_choice_row(
        content,
        tr(settings.locale, UiText::DataManagement),
        &[
            (
                tr(settings.locale, UiText::ResetSettings),
                SettingAction::ResetSettings,
            ),
            (
                tr(settings.locale, UiText::ClearSaves),
                SettingAction::ClearSaves,
            ),
        ],
        usize::MAX,
        false,
        font,
        SYSTEM_DATA_CELL,
    );
    spawn_choice_row(
        content,
        tr(settings.locale, UiText::ImportExport),
        &[
            (
                tr(settings.locale, UiText::Export),
                SettingAction::ExportData,
            ),
            (
                tr(settings.locale, UiText::Import),
                SettingAction::ImportData,
            ),
        ],
        usize::MAX,
        false,
        font,
        SYSTEM_TRANSFER_CELL,
    );
}

pub(super) fn spawn_choice_content(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    choices: &[(&str, SettingAction)],
    selected: usize,
    vertical: bool,
    font: &Handle<Font>,
) {
    parent.spawn(setting_text(label, font, SETTING_LABEL_SIZE, true));
    parent
        .spawn((Node {
            margin: UiRect::top(Val::Px(7.5)),
            flex_direction: if vertical {
                FlexDirection::Column
            } else {
                FlexDirection::Row
            },
            column_gap: Val::Px(if vertical { 0.0 } else { 9.0 }),
            row_gap: Val::Px(if vertical { 9.0 } else { 0.0 }),
            align_items: AlignItems::FlexStart,
            ..default()
        },))
        .with_children(|buttons| {
            for (index, (text, action)) in choices.iter().copied().enumerate() {
                spawn_choice(buttons, text, action, index == selected, font);
            }
        });
}

pub(super) fn spawn_text_preview(
    content: &mut ChildSpawnerCommands,
    settings: &RuntimeSettings,
    font: &Handle<Font>,
    cell: SettingsGridCell,
) {
    let size = match settings.text_size {
        0 => 23.25,
        2 => 31.5,
        _ => 27.0,
    };
    content
        .spawn((Node {
            width: Val::Percent(100.0),
            grid_column: cell.column(),
            grid_row: cell.row(),
            min_height: Val::Px(247.5),
            margin: UiRect::vertical(Val::Px(4.5)),
            padding: UiRect::all(Val::Px(4.5)),
            flex_direction: FlexDirection::Column,
            ..default()
        },))
        .with_children(|preview| {
            preview.spawn(setting_text(
                tr(settings.locale, UiText::TextPreview),
                font,
                SETTING_LABEL_SIZE,
                true,
            ));
            preview
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        margin: UiRect::top(Val::Px(9.0)),
                        padding: UiRect::axes(Val::Px(27.0), Val::Px(21.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(7.5),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(
                        0.0,
                        0.0,
                        0.03,
                        settings.textbox_opacity * 0.72,
                    )),
                    SettingPreviewSurface,
                ))
                .with_children(|box_| {
                    box_.spawn((
                        Node {
                            width: Val::Px(225.0),
                            height: Val::Px(43.5),
                            margin: UiRect::new(
                                Val::Px(-13.5),
                                Val::ZERO,
                                Val::Px(-9.0),
                                Val::ZERO,
                            ),
                            padding: UiRect::horizontal(Val::Px(16.5)),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.02, 0.7)),
                    ))
                    .with_child(setting_text(
                        tr(settings.locale, UiText::TextPreview),
                        font,
                        SETTING_LABEL_SIZE,
                        true,
                    ));
                    box_.spawn((
                        SettingPreviewText,
                        setting_text(
                            tr(settings.locale, UiText::PreviewDialogue),
                            font,
                            size,
                            false,
                        ),
                    ));
                });
        });
}

pub(super) fn spawn_row(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    kind: SettingKind,
    ratio: f32,
    cell: SettingsGridCell,
) {
    spawn_slider_group(root, font, label, kind, ratio, Some(cell));
}

pub(super) fn spawn_slider_group(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    kind: SettingKind,
    ratio: f32,
    cell: Option<SettingsGridCell>,
) {
    root.spawn((setting_group_node(cell, 96.0),))
        .with_children(|row| {
            row.spawn(setting_text(label, font, SETTING_LABEL_SIZE, true));
            row.spawn((
                Button,
                UiSoundStyle::HoverOnly,
                SettingSlider(kind),
                Node {
                    position_type: PositionType::Relative,
                    width: Val::Percent(100.0),
                    max_width: Val::Px(375.0),
                    height: Val::Px(37.5),
                    margin: UiRect::top(Val::Px(7.5)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|slider| {
                slider.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(15.0),
                        width: Val::Percent(100.0),
                        height: Val::Px(7.5),
                        ..default()
                    },
                    BackgroundColor(Color::BLACK),
                    Outline::new(Val::Px(3.75), Val::ZERO, Color::srgba(1.0, 1.0, 1.0, 0.19)),
                ));
                slider.spawn((
                    SettingSliderThumb(kind),
                    SettingSliderThumbVisual(10.0),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(ratio.clamp(0.0, 1.0) * 90.0),
                        top: Val::Px(3.75),
                        width: Val::Percent(10.0),
                        height: Val::Px(30.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.5)),
                ));
                slider
                    .spawn((
                        SettingValueBubble(kind),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(ratio.clamp(0.0, 1.0) * 90.0),
                            top: Val::Px(-31.5),
                            width: Val::Percent(10.0),
                            height: Val::Px(27.0),
                            display: Display::None,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border_radius: BorderRadius::all(Val::Px(4.5)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
                    ))
                    .with_child((
                        SettingValueText(kind),
                        Text::new(kind.value_text(ratio)),
                        TextFont {
                            font: font.clone().into(),
                            font_size: FontSize::from(16.5),
                            weight: FontWeight::BOLD,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
            });
        });
}

pub(super) fn setting_group_node(cell: Option<SettingsGridCell>, min_height: f32) -> Node {
    let mut node = Node {
        width: Val::Percent(100.0),
        min_height: Val::Px(min_height),
        margin: UiRect::vertical(Val::Px(4.5)),
        padding: UiRect::all(Val::Px(4.5)),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::FlexStart,
        ..default()
    };
    if let Some(cell) = cell {
        node.grid_column = cell.column();
        node.grid_row = cell.row();
    }
    node
}

pub(super) fn spawn_choice(
    parent: &mut ChildSpawnerCommands,
    text: &str,
    action: SettingAction,
    selected: bool,
    font: &Handle<Font>,
) {
    parent.spawn((
        Button,
        UiSoundStyle::Switch,
        action,
        SettingChoice(action),
        SettingChoiceVisual {
            selected,
            hovered: false,
            fill: if selected { 100.0 } else { 0.0 },
            text_alpha: if selected {
                OPTION_TEXT_ACTIVE
            } else {
                OPTION_TEXT_IDLE
            },
        },
        Node {
            min_width: Val::Px(96.0),
            height: Val::Px(43.5),
            padding: UiRect::horizontal(Val::Px(15.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![
            (
                SettingChoiceFill,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::ZERO,
                    top: Val::ZERO,
                    width: Val::Percent(if selected { 100.0 } else { 0.0 }),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(button_surface(OPTION_FILL_ALPHA)),
                FocusPolicy::Pass,
            ),
            text_weight(
                text,
                font,
                SETTING_OPTION_SIZE,
                if selected {
                    OPTION_TEXT_ACTIVE
                } else {
                    OPTION_TEXT_IDLE
                },
                if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                },
            )
        ],
    ));
}

pub(super) fn setting_text(
    text: impl Into<String>,
    font: &Handle<Font>,
    size: f32,
    bold: bool,
) -> impl Bundle {
    text_weight(
        text,
        font,
        size,
        0.78,
        if bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        },
    )
}
