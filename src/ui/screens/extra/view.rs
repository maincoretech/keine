//! Gallery header, panels, thumbnails and full-CG layout.
use super::*;
use crate::ui::foundation::{Button, Interaction};

pub(super) fn extra_button() -> impl Bundle {
    (ExtraButtonVisual, SettingChoiceVisual::new(false))
}

pub(crate) fn attach_button_visuals(
    mut commands: Commands,
    buttons: Query<(Entity, &Children, &SettingChoiceVisual), Added<ExtraButtonVisual>>,
    mut labels: Query<&mut TextColor>,
) {
    for (entity, children, visual) in &buttons {
        let fill = commands.spawn(choice_fill(false)).id();
        commands.entity(entity).insert_children(0, &[fill]);
        for child in children.iter() {
            if let Ok(mut color) = labels.get_mut(child) {
                color.0.set_alpha(visual.text_alpha);
            }
        }
    }
}

pub(super) fn spawn_header(
    root: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    _selected: ExtraSection,
    locale: crate::storage::settings::UiLocale,
) {
    root.spawn((Node {
        width: Val::Percent(100.0),
        height: Val::Percent(7.0),
        padding: UiRect::horizontal(Val::Px(9.0)),
        flex_shrink: 0.0,
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        ..default()
    },))
        .with_children(|header| {
            header
                .spawn(Node {
                    height: Val::Percent(100.0),
                    ..default()
                })
                .with_children(|tabs| {
                    for (section, label, icon) in [
                        (ExtraSection::Cg, UiText::GalleryCg, "\u{f42a}"),
                        (ExtraSection::Music, UiText::GalleryMusic, "\u{f49e}"),
                    ] {
                        tabs.spawn((
                            Button,
                            ExtraTab { section },
                            extra_button(),
                            Node {
                                min_width: Val::Px(123.75),
                                height: Val::Percent(100.0),
                                padding: UiRect::horizontal(Val::Px(21.0)),
                                margin: UiRect::right(Val::Px(9.0)),
                                column_gap: Val::Px(7.5),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|button| {
                            button.spawn(text(icon, &fonts.icons, 21.0, 0.82));
                            button.spawn((
                                LocalizedText(label),
                                text(
                                    crate::ui::support::i18n::tr(locale, label),
                                    &fonts.text,
                                    21.0,
                                    0.82,
                                ),
                            ));
                        });
                    }
                });
            header
                .spawn((
                    Button,
                    ExtraClose,
                    extra_button(),
                    Node {
                        min_width: Val::Px(112.5),
                        height: Val::Percent(100.0),
                        padding: UiRect::horizontal(Val::Px(21.0)),
                        margin: UiRect::horizontal(Val::Px(EXTRA_CONTROL_MARGIN)),
                        column_gap: Val::Px(7.5),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|button| {
                    button.spawn(text("\u{f1c3}", &fonts.icons, 21.0, 0.82));
                    button.spawn((
                        LocalizedText(UiText::Back),
                        text("BACK", &fonts.text, 21.0, 0.82),
                    ));
                });
        });
}

pub(super) fn spawn_bgm_panel(
    body: &mut ChildSpawnerCommands,
    tracks: &[(String, String)],
    ui: &ExtraUi,
    fonts: &UiFonts,
    settings: &RuntimeSettings,
) {
    body.spawn((
        ExtraPanel(ExtraSection::Music),
        UiTransform::default(),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            display: if ui.section == ExtraSection::Music {
                Display::Flex
            } else {
                Display::None
            },
            min_width: Val::Px(0.0),
            height: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(EXTRA_PANEL_PADDING)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(EXTRA_SECTION_GAP),
            ..default()
        },
    ))
    .with_children(|panel| {
        panel
            .spawn((
                ExtraBgmList::default(),
                ScrollPosition::default(),
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::ZERO,
                    flex_grow: 1.0,
                    flex_basis: Val::ZERO,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|list| {
                if tracks.is_empty() {
                    list.spawn((
                        LocalizedText(UiText::NoGalleryMusic),
                        text("NO MUSIC UNLOCKED", &fonts.text, 22.5, 0.58),
                    ));
                }
                for (file, name) in tracks {
                    list.spawn((
                        Button,
                        ExtraBgm(file.clone()),
                        extra_button(),
                        Node {
                            width: Val::Percent(100.0),
                            flex_shrink: 0.0,
                            min_height: Val::Px(63.0),
                            align_items: AlignItems::Center,
                            padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
                            margin: UiRect::all(Val::Px(EXTRA_CONTROL_MARGIN)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                        children![(
                            Node {
                                width: Val::Percent(100.0),
                                ..default()
                            },
                            text(name.clone(), &fonts.text, 24.0, 0.8)
                        )],
                    ));
                }
            });
        panel
            .spawn((
                ExtraBgmName,
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(36.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ))
            .with_child(text(
                ui.selected_bgm
                    .as_ref()
                    .and_then(|file| tracks.iter().find(|track| &track.0 == file))
                    .map_or("", |track| track.1.as_str()),
                &fonts.text,
                27.0,
                0.8,
            ));
        panel.spawn((
            ExtraBgmStatus,
            LocalizedText(UiText::PlaybackStopped),
            Node {
                display: Display::None,
                ..default()
            },
            text("", &fonts.text, 18.75, 0.58),
        ));
        panel
            .spawn(Node {
                width: Val::Percent(100.0),
                height: Val::Px(90.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(9.0),
                padding: UiRect::horizontal(Val::Px(EXTRA_CONTROL_MARGIN)),
                ..default()
            })
            .with_children(|progress| {
                progress
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        ..default()
                    })
                    .with_children(|slider| {
                        let parts =
                            spawn_slider(slider, &fonts.text, 0.0, "00:00", Val::Percent(100.0));
                        let mut commands = slider.commands();
                        commands
                            .entity(parts.control)
                            .insert(ExtraBgmSeekBar::default());
                        commands.entity(parts.thumb).insert((
                            ExtraBgmProgressThumb(10.0),
                            Interaction::None,
                            FocusPolicy::Pass,
                        ));
                        commands.entity(parts.bubble).insert(ExtraBgmBubble);
                        commands.entity(parts.value).insert(ExtraBgmBubbleText);
                    });
                progress.spawn((
                    ExtraBgmTime,
                    Node {
                        width: Val::Percent(100.0),
                        flex_shrink: 0.0,
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    },
                    children![text("00:00 / --:--", &fonts.text, 22.5, 0.58)],
                ));
            });
        panel
            .spawn((Node {
                width: Val::Percent(100.0),
                height: Val::Px(63.0),
                flex_shrink: 0.0,
                margin: UiRect::top(Val::Px(EXTRA_CONTROL_MARGIN)),
                align_items: AlignItems::Center,
                ..default()
            },))
            .with_children(|controls| {
                for (icon, action, label) in [
                    ('\u{f564}', ExtraBgmControl::Previous, UiText::Previous),
                    ('\u{f4f4}', ExtraBgmControl::Play, UiText::Play),
                    ('\u{f558}', ExtraBgmControl::Next, UiText::Next),
                    ('\u{f592}', ExtraBgmControl::Stop, UiText::Stop),
                ] {
                    let mut button = controls.spawn((
                        Button,
                        action,
                        extra_button(),
                        Node {
                            min_width: Val::Px(144.0),
                            height: Val::Px(63.0),
                            padding: UiRect::horizontal(Val::Px(12.0)),
                            column_gap: Val::Px(9.0),
                            margin: UiRect::horizontal(Val::Px(EXTRA_CONTROL_MARGIN)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ));
                    if matches!(action, ExtraBgmControl::Play) {
                        button.insert(ExtraBgmPlayIcon);
                    }
                    button.with_children(|button| {
                        button.spawn(text(icon.to_string(), &fonts.icons, 24.0, 0.8));
                        button.spawn((LocalizedText(label), text("", &fonts.text, 22.5, 0.8)));
                    });
                }
                crate::ui::settings_panel::spawn_gallery_volume_slider(
                    controls,
                    settings,
                    &fonts.text,
                );
            });
    });
}

pub(super) fn spawn_cg_panel(
    body: &mut ChildSpawnerCommands,
    images: &[(String, String)],
    ui: &ExtraUi,
    config: &GameConfigResource,
    fonts: &UiFonts,
    assets: &AssetServer,
    image_roles: &ImageRoleRegistry,
) {
    let page_count = images.len().div_ceil(CG_PER_PAGE).max(1);
    let page = ui.page.clamp(1, page_count);
    body.spawn((
        ExtraPanel(ExtraSection::Cg),
        UiTransform::default(),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            display: if ui.section == ExtraSection::Cg {
                Display::Flex
            } else {
                Display::None
            },
            min_width: Val::Px(0.0),
            height: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(EXTRA_PANEL_PADDING)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(EXTRA_SECTION_GAP),
            ..default()
        },
    ))
    .with_children(|panel| {
        if images.is_empty() {
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|empty| {
                    empty.spawn((
                        LocalizedText(UiText::NoGalleryCg),
                        text("", &fonts.text, 22.5, 0.58),
                    ));
                });
            return;
        }
        if page_count > 1 {
            panel
                .spawn((Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(63.0),
                    flex_shrink: 0.0,
                    padding: UiRect::horizontal(Val::Px(12.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },))
                .with_children(|pages| {
                    pages
                        .spawn((Node {
                            align_items: AlignItems::Center,
                            ..default()
                        },))
                        .with_children(|buttons| {
                            for (step, icon) in [(-1, "\u{f284}"), (1, "\u{f285}")] {
                                buttons.spawn((
                                    Button,
                                    ExtraPage(step),
                                    extra_button(),
                                    Node {
                                        min_width: Val::Px(126.0),
                                        height: Val::Px(63.0),
                                        padding: UiRect::horizontal(Val::Px(12.0)),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                    children![text(icon, &fonts.icons, 24.0, 0.8)],
                                ));
                                if step == -1 {
                                    buttons.spawn((
                                        ExtraPageLabel,
                                        text(
                                            format!("{page} / {page_count}"),
                                            &fonts.text,
                                            22.5,
                                            0.58,
                                        ),
                                    ));
                                }
                            }
                        });
                });
        }
        panel
            .spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::ZERO,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|viewport| {
                spawn_cg_grid(viewport, images, page, config, fonts, assets, image_roles);
            });
    });
}

pub(super) fn spawn_cg_grid(
    viewport: &mut ChildSpawnerCommands,
    images: &[(String, String)],
    page: usize,
    config: &GameConfigResource,
    fonts: &UiFonts,
    assets: &AssetServer,
    image_roles: &ImageRoleRegistry,
) {
    viewport
        .spawn((
            ExtraCgGrid { outgoing: false },
            UiTransform::default(),
            Node {
                position_type: PositionType::Absolute,
                left: Val::ZERO,
                top: Val::ZERO,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                padding: UiRect::top(Val::Px(30.0)),
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                align_content: AlignContent::FlexStart,
                ..default()
            },
        ))
        .with_children(|grid| {
            spawn_cg_cards(grid, images, page, config, fonts, assets, image_roles);
        });
}

pub(super) fn spawn_cg_cards(
    grid: &mut ChildSpawnerCommands,
    images: &[(String, String)],
    page: usize,
    config: &GameConfigResource,
    fonts: &UiFonts,
    assets: &AssetServer,
    image_roles: &ImageRoleRegistry,
) {
    let first = page.saturating_sub(1) * CG_PER_PAGE;
    let visible = images.iter().skip(first).take(CG_PER_PAGE);
    if visible.clone().next().is_none() {
        grid.spawn((
            LocalizedText(UiText::NoGalleryCg),
            text("NO CG UNLOCKED", &fonts.text, 22.5, 0.58),
        ));
        return;
    }
    for (file, name) in visible {
        grid.spawn((
            Button,
            ExtraCg(file.clone()),
            Node {
                width: Val::Percent(22.5),
                height: Val::Percent(46.0),
                padding: UiRect::all(Val::Px(12.0)),
                margin: UiRect::all(Val::Percent(1.25)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::NONE),
            extra_button(),
        ))
        .with_children(|card| {
            let path = config.bg_path(file);
            let thumbnail = crate::scene::images::is_webp(&path)
                .then(|| assets.load::<crate::scene::images::GalleryThumbnail>(path.clone()));
            let image = if thumbnail.is_some() {
                Handle::default()
            } else {
                crate::scene::images::load(assets, image_roles, path, ImageRole::BACKGROUND)
            };
            let status = thumbnail.as_ref().map_or_else(
                || image.clone().untyped(),
                |handle| handle.clone().untyped(),
            );
            card.spawn(Node {
                width: Val::Percent(100.0),
                min_height: Val::ZERO,
                flex_grow: 1.0,
                flex_basis: Val::ZERO,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|preview| {
                let mut node = preview.spawn((
                    ImageNode::new(image.clone()),
                    Node {
                        max_width: Val::Percent(100.0),
                        max_height: Val::Percent(100.0),
                        ..default()
                    },
                    FocusPolicy::Pass,
                ));
                if let Some(thumbnail) = thumbnail {
                    node.insert(ExtraThumbnail(thumbnail));
                }
                preview.spawn((
                    ExtraImageStatus(status),
                    LocalizedText(UiText::GalleryLoading),
                    text("", &fonts.text, 18.75, 0.58),
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                    FocusPolicy::Pass,
                ));
            });
            card.spawn((
                Node {
                    height: Val::Px(63.0),
                    width: Val::Percent(100.0),
                    overflow: Overflow::clip(),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![text(name.clone(), &fonts.text, 18.75, 0.8)],
            ));
        });
    }
}

pub(super) fn spawn_full_cg(
    commands: &mut Commands,
    camera: Entity,
    file: &str,
    images: &[(String, String)],
    fonts: &UiFonts,
    image: Handle<Image>,
) -> Entity {
    let name = images
        .iter()
        .find(|item| item.0 == file)
        .map_or("CG", |item| item.1.as_str());
    commands
        .spawn((
            ExtraFullCg(file.to_owned()),
            ExtraMotion {
                current: 0.0,
                target: 1.0,
            },
            UiTransform::default(),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(DESIGN_WIDTH),
                height: Val::Px(DESIGN_HEIGHT),
                padding: UiRect::all(Val::Px(24.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.72)),
            FocusPolicy::Block,
            GlobalZIndex(190),
            UiTargetCamera(camera),
            RenderLayers::layer(2),
        ))
        .with_children(|overlay| {
            overlay.spawn((
                ImageNode::new(image.clone()),
                Node {
                    max_width: Val::Percent(94.0),
                    max_height: Val::Percent(80.0),
                    ..default()
                },
                FocusPolicy::Pass,
            ));
            overlay.spawn((
                ExtraImageStatus(image.untyped()),
                LocalizedText(UiText::GalleryLoading),
                text("", &fonts.text, 24.0, 0.78),
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                FocusPolicy::Pass,
            ));
            overlay
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(42.0),
                        right: Val::Px(42.0),
                        top: Val::Px(30.0),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    children![(
                        Node {
                            max_width: Val::Percent(60.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        text_weight(name, &fonts.text, 24.0, 0.78, bevy::text::FontWeight::BOLD,)
                    )],
                ))
                .with_children(|chrome| {
                    chrome
                        .spawn((Node {
                            column_gap: Val::Px(9.0),
                            ..default()
                        },))
                        .with_children(|buttons| {
                            for (label, action, key) in [
                                ("‹", ExtraCgControl::Previous, UiText::Previous),
                                ("›", ExtraCgControl::Next, UiText::Next),
                                ("×", ExtraCgControl::Close, UiText::Back),
                            ] {
                                buttons
                                    .spawn((
                                        Button,
                                        action,
                                        extra_button(),
                                        Node {
                                            min_width: Val::Px(144.0),
                                            height: Val::Px(63.0),
                                            column_gap: Val::Px(9.0),
                                            padding: UiRect::horizontal(Val::Px(12.0)),
                                            margin: UiRect::horizontal(Val::Px(
                                                EXTRA_CONTROL_MARGIN,
                                            )),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(Color::NONE),
                                    ))
                                    .with_children(|button| {
                                        button.spawn(text(label, &fonts.text, 27.0, 0.82));
                                        button.spawn((
                                            LocalizedText(key),
                                            text("", &fonts.text, 22.5, 0.82),
                                        ));
                                    });
                            }
                        });
                });
        })
        .id()
}
