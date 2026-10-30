//! Project configuration form. The document transaction remains the only writer.
use super::*;
use keine_core::config::GameConfig;
use noyalib::{Value, cst::parse_document};
use std::time::Instant;

pub(super) struct Settings {
    source: String,
    inputs: Vec<Entity<InputState>>,
    original: Vec<String>,
    extra: bool,
    original_extra: bool,
    orders: [Vec<String>; 2],
    original_orders: [Vec<String>; 2],
    drag_tick: Option<Instant>,
    error: Option<String>,
    field_error: Option<(usize, String)>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone)]
struct GalleryDrag {
    owner: gpui_kit::EntityId,
    section: usize,
    id: String,
}

pub(super) fn edit_config(source: &str, changes: &[(&str, Value)]) -> Result<String, String> {
    GameConfig::from_yaml(source).map_err(|e| e.to_string())?;
    let mut document = parse_document(source).map_err(|e| e.to_string())?;
    for (path, value) in changes {
        document.set_path(path, value).map_err(|e| e.to_string())?;
    }
    let result = document.to_string();
    GameConfig::from_yaml(&result).map_err(|e| e.to_string())?;
    Ok(result)
}

fn gallery_order(preferred: &[String], assets: impl Iterator<Item = String>) -> Vec<String> {
    let mut tail = assets.collect::<Vec<_>>();
    tail.sort();
    let mut result = Vec::new();
    // Keep IDs not currently in the manifest, including future unlocks.
    for id in preferred.iter().cloned().chain(tail) {
        if !result.contains(&id) {
            result.push(id);
        }
    }
    result
}

impl Settings {
    fn dirty(&self, cx: &App) -> bool {
        self.extra != self.original_extra
            || self.orders != self.original_orders
            || self
                .inputs
                .iter()
                .zip(&self.original)
                .any(|(input, value)| input.read(cx).value().as_str() != value)
    }
}

pub(super) fn render(
    root: &Path,
    panel: &mut WorkbenchPanel,
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let source = cx
        .global::<EditorDocuments>()
        .source(root, Path::new("config.yaml"))
        .unwrap_or_default();
    let reload = panel
        .project_settings
        .as_ref()
        .is_none_or(|state| state.source != source && !state.dirty(cx));
    if reload {
        let config = match GameConfig::from_yaml(&source) {
            Ok(config) => config,
            Err(error) => {
                return div()
                    .p_3()
                    .text_sm()
                    .child(crate::i18n::format(
                        "Could not read project settings: {error}",
                        &[("error", crate::i18n::tr(&(error).to_string()).to_owned())],
                    ))
                    .into_any_element();
            }
        };
        let original = vec![
            config.title,
            config.title_background,
            config.project.icon,
            config.script.entry,
        ];
        let inputs = original
            .iter()
            .map(|value| {
                cx.new(|cx| {
                    let mut input = InputState::new(window, cx);
                    input.set_value(value.clone(), window, cx);
                    input
                })
            })
            .collect::<Vec<_>>();
        let subscriptions = inputs
            .iter()
            .enumerate()
            .map(|(field, input)| {
                cx.subscribe(input, move |panel, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        if let Some(state) = &mut panel.project_settings {
                            if state.field_error.as_ref().is_some_and(|(i, _)| *i == field) {
                                state.field_error = None;
                            }
                            state.error = None;
                        }
                        cx.notify();
                    }
                })
            })
            .collect();
        let index = cx.global::<EditorDocuments>().authoring(root);
        let orders = [
            gallery_order(
                &config.extra.cg_order,
                index
                    .assets
                    .iter()
                    .filter(|a| a.kind == AssetKind::Background)
                    .map(|a| a.id.clone()),
            ),
            gallery_order(
                &config.extra.bgm_order,
                index
                    .assets
                    .iter()
                    .filter(|a| a.kind == AssetKind::Bgm)
                    .map(|a| a.id.clone()),
            ),
        ];
        panel.project_settings = Some(Settings {
            source,
            inputs,
            original,
            extra: config.features.extra,
            original_extra: config.features.extra,
            original_orders: orders.clone(),
            drag_tick: None,
            orders,
            error: None,
            field_error: None,
            _subscriptions: subscriptions,
        });
    }
    let state = panel.project_settings.as_ref().unwrap();
    let dirty = state.dirty(cx);
    let owner = cx.entity_id();
    let unsaved = cx
        .global::<EditorDocuments>()
        .workspaces
        .get(root)
        .and_then(|workspace| workspace.manager.document(Path::new("config.yaml")))
        .is_some_and(|doc| doc.borrow().is_dirty());
    let index = cx.global::<EditorDocuments>().authoring(root);
    let mut content = div()
        .w_full()
        .max_w(px(640.))
        .flex()
        .flex_col()
        .gap_3()
        .p_3();
    if panel.settings_tab == settings::SettingsTab::Project {
        for (field, label) in [
            "Title",
            "Title background",
            "Application icon",
            "Entry scene",
        ]
        .into_iter()
        .enumerate()
        {
            let value = state.inputs[field].read(cx).value().to_string();
            let mut control = div().w_full().min_w_0().flex().flex_col().gap_2().child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(crate::i18n::text(label)),
            );
            let mut input_row = div().flex().items_center().gap_2().child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(tool_input(&state.inputs[field])),
            );
            if field == 1 {
                let options = index
                    .assets
                    .iter()
                    .filter(|a| a.kind == AssetKind::Background)
                    .map(|a| SourceOption {
                        localize: false,
                        value: a.id.clone(),
                        title: a.id.clone().into(),
                        asset: Some((root.to_owned(), a.kind, a.path.clone())),
                    })
                    .collect();
                input_row = input_row.child(resource_trigger(
                    root,
                    ResourceTarget::ProjectBackground,
                    options,
                    value.clone(),
                    false,
                    cx,
                ));
            } else if field == 3 {
                let options = index
                    .scenes
                    .iter()
                    .map(|scene| SourceOption {
                        localize: false,
                        value: scene.name.clone(),
                        title: scene.name.clone().into(),
                        asset: None,
                    })
                    .collect();
                input_row = div().child(resource_trigger(
                    root,
                    ResourceTarget::ProjectEntry,
                    options,
                    value.clone(),
                    false,
                    cx,
                ));
            } else if field == 2 {
                let root = root.to_owned();
                input_row = input_row.child(
                    tool_icon_action("Choose icon…", AssetIconName::FolderOpen, true).on_click(
                        cx.listener(move |_, _, window, cx| {
                            let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
                                files: true,
                                directories: false,
                                multiple: false,
                                prompt: Some(
                                    crate::i18n::tr("Choose a project PNG or WebP").into(),
                                ),
                            });
                            let root = root.clone();
                            cx.spawn_in(window, async move |this, cx| {
                                let Ok(Ok(Some(paths))) = paths.await else {
                                    return;
                                };
                                let Some(path) = paths.first() else {
                                    return;
                                };
                                let selected = root.canonicalize().and_then(|root| {
                                    path.canonicalize()?
                                        .strip_prefix(root)
                                        .map(Path::to_owned)
                                        .map_err(|_| {
                                            io::Error::other("Choose an icon inside the project")
                                        })
                                });
                                let _ = this.update_in(cx, |panel, window, cx| {
                                    if let Some(state) = &mut panel.project_settings {
                                        match selected {
                                            Ok(path) => {
                                                state.inputs[2].update(cx, |input, cx| {
                                                    input.set_value(
                                                        path.to_string_lossy().replace('\\', "/"),
                                                        window,
                                                        cx,
                                                    )
                                                });
                                                state.field_error = None;
                                            }
                                            Err(error) => {
                                                state.field_error = Some((2, error.to_string()))
                                            }
                                        }
                                    }
                                    cx.notify();
                                });
                            })
                            .detach();
                        }),
                    ),
                );
            }
            control = control.child(input_row);
            if let Some((_, error)) = state.field_error.as_ref().filter(|(i, _)| *i == field) {
                control = control.child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xdb7780))
                        .child(crate::i18n::text(error)),
                );
            }
            let mut row = div()
                .w_full()
                .min_w_0()
                .flex()
                .items_start()
                .gap_3()
                .child(control.flex_1());
            if field > 0 && !value.is_empty() {
                let path = if field == 1 {
                    index
                        .assets
                        .iter()
                        .find(|a| a.kind == AssetKind::Background && a.id == value)
                        .map(|a| a.path.clone())
                        .unwrap_or_else(|| PathBuf::from(&value))
                } else {
                    PathBuf::from(&value)
                };
                if let Some(media) = file_ops::asset_media_info(root, &path) {
                    row = row.child(
                        resource::thumbnail::Thumbnails::image(
                            &panel.assets.asset_thumbnails,
                            root,
                            &path,
                            &media,
                        )
                        .w(px(72.))
                        .h(px(64.))
                        .flex_shrink_0()
                        .object_fit(gpui_kit::ObjectFit::Contain),
                    );
                }
            }
            content = content.child(row);
        }
    }
    if panel.settings_tab == settings::SettingsTab::Gallery {
        content = content.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .child(section_label("EXTRA"))
                .child(
                    Switch::new("project-extra")
                        .small()
                        .color(rgb(PRIMARY))
                        .checked(state.extra)
                        .accessibility_label(crate::i18n::tr("Enable Extra"))
                        .on_change(cx.listener(|panel, value: &bool, _, cx| {
                            if let Some(state) = &mut panel.project_settings {
                                state.extra = *value;
                            }
                            cx.notify();
                        })),
                ),
        );
        if state.extra {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(crate::i18n::tr(
                        "Drag to reorder. Only unlocked entries appear in-game.",
                    )),
            );
            for (section, title) in ["CG order", "Music order"].into_iter().enumerate() {
                content = content.child(section_label(title));
                let mut rows = div().flex().flex_col();
                for id in &state.orders[section] {
                    let target = id.clone();
                    let owner_panel = cx.entity().downgrade();
                    let drag = GalleryDrag {
                        owner: cx.entity_id(),
                        section,
                        id: id.clone(),
                    };
                    rows = rows.child(
                        div()
                            .id(format!("gallery-order-{section}-{id}"))
                            .h(px(32.))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .border_t_2()
                            .border_color(gpui_kit::transparent_black())
                            .rounded(px(7.))
                            .cursor_move()
                            .hover(|style| style.bg(rgb(SURFACE)))
                            .drag_over::<GalleryDrag>(move |style, drag, _, _| {
                                if drag.owner == owner && drag.section == section {
                                    style.border_color(rgb(PRIMARY)).bg(rgb(PRIMARY_DIM))
                                } else {
                                    style
                                }
                            })
                            .on_drag(drag, move |drag, _, window, cx| {
                                let _ = owner_panel.update(cx, |panel, cx| {
                                    if let Some(state) = &mut panel.project_settings {
                                        state.drag_tick = Some(Instant::now());
                                    }
                                    panel.focus.focus(window, cx);
                                    cx.notify();
                                });
                                cx.new(|_| BlockDragPreview {
                                    label: drag.id.clone(),
                                    summary: String::new(),
                                    icon: AssetIconName::GripVertical,
                                    count: 1,
                                    width: 240.,
                                    height: 32.,
                                    color: PRIMARY,
                                    grip_top: 8.,
                                })
                            })
                            .on_drop(cx.listener(move |panel, drag: &GalleryDrag, _, cx| {
                                if drag.owner == cx.entity_id() && drag.section == section {
                                    if let Some(state) = &mut panel.project_settings {
                                        state.drag_tick = None;
                                        move_before(
                                            &mut state.orders[section],
                                            &drag.id,
                                            Some(&target),
                                        );
                                    }
                                    cx.notify();
                                }
                            }))
                            .child(
                                Icon::new(AssetIconName::GripVertical)
                                    .xsmall()
                                    .text_color(rgb(MUTED)),
                            )
                            .child(
                                Icon::new(if section == 0 {
                                    AssetIconName::Image
                                } else {
                                    AssetIconName::Music
                                })
                                .xsmall()
                                .text_color(rgb(MUTED)),
                            )
                            .child(div().min_w_0().text_sm().truncate().child(id.clone())),
                    );
                }
                rows = rows.child(
                    div()
                        .id(format!("gallery-end-{section}"))
                        .h(px(20.))
                        .w_full()
                        .border_t_2()
                        .border_color(gpui_kit::transparent_black())
                        .drag_over::<GalleryDrag>(move |style, drag, _, _| {
                            if drag.owner == owner && drag.section == section {
                                style.border_color(rgb(PRIMARY))
                            } else {
                                style
                            }
                        })
                        .on_drop(cx.listener(move |panel, drag: &GalleryDrag, _, cx| {
                            if drag.owner == cx.entity_id() && drag.section == section {
                                if let Some(state) = &mut panel.project_settings {
                                    state.drag_tick = None;
                                    move_before(&mut state.orders[section], &drag.id, None);
                                }
                                cx.notify();
                            }
                        })),
                );
                content = content.child(rows);
            }
        }
    }
    let root_apply = root.to_owned();
    let root_source = root.to_owned();
    let mut footer = div()
        .w_full()
        .flex_shrink_0()
        .p_3()
        .bg(rgb(PANEL))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(tool_icon_action("Apply", AssetIconName::Check, dirty).when(
                    dirty,
                    |button| {
                        button.on_click(cx.listener(move |panel, _, window, cx| {
                            match apply(&root_apply, panel, window, cx) {
                                Err((field, error)) => {
                                    let state = panel.project_settings.as_mut().unwrap();
                                    if let Some(field) = field {
                                        panel.settings_tab = settings::SettingsTab::Project;
                                        state.field_error = Some((field, error));
                                    } else {
                                        state.error = Some(error);
                                    }
                                }
                                Ok(()) => {
                                    panel.project_settings = None;
                                    panel.focus.focus(window, cx);
                                }
                            }
                            cx.refresh_windows();
                        }))
                    },
                ))
                .child(
                    tool_icon_action("Reset form", AssetIconName::RotateCcw, dirty).when(
                        dirty,
                        |button| {
                            button.on_click(cx.listener(|panel, _, _, cx| {
                                panel.project_settings = None;
                                cx.notify();
                            }))
                        },
                    ),
                )
                .child(
                    tool_icon_action("Edit YAML", AssetIconName::FileText, true).on_click(
                        move |_, window, cx| {
                            navigate_source(
                                &root_source,
                                Path::new("config.yaml"),
                                1,
                                1,
                                window,
                                cx,
                            )
                        },
                    ),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(crate::i18n::tr(if dirty {
                    "Unapplied changes"
                } else if unsaved {
                    "Applied · not saved"
                } else {
                    "Saved"
                })),
        );
    if let Some(error) = &state.error {
        footer = footer.child(
            div()
                .text_xs()
                .text_color(rgb(0xdb7780))
                .child(crate::i18n::text(error)),
        );
    }
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .id("project-settings-scroll")
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&panel.view_scroll)
                .on_drag_move(cx.listener(
                    |_, event: &gpui_kit::DragMoveEvent<GalleryDrag>, _, cx| {
                        if event.drag(cx).owner == cx.entity_id() {
                            // Animation frames are requested by refresh_drag during render.
                            cx.notify();
                        }
                    },
                ))
                .child(content),
        )
        .child(footer)
        .into_any_element()
}

pub(super) fn cancel_drag(panel: &mut WorkbenchPanel, window: &mut Window, cx: &mut App) -> bool {
    if panel
        .project_settings
        .as_mut()
        .and_then(|state| state.drag_tick.take())
        .is_some()
    {
        cx.stop_active_drag(window);
        true
    } else {
        false
    }
}

pub(super) fn refresh_drag(panel: &mut WorkbenchPanel, window: &mut Window, cx: &mut App) {
    let Some(tick) = panel
        .project_settings
        .as_mut()
        .and_then(|state| state.drag_tick.as_mut())
    else {
        return;
    };
    if !cx.has_active_drag() || !window.is_window_active() {
        cancel_drag(panel, window, cx);
        return;
    }
    let now = Instant::now();
    let dt = now.duration_since(*tick).as_secs_f32().min(0.05);
    *tick = now;
    let bounds = panel.view_scroll.bounds();
    let pointer = window.mouse_position();
    if bounds.contains(&pointer) {
        let speed = reorder::edge_scroll_velocity(
            f32::from(pointer.y - bounds.top()),
            f32::from(bounds.size.height),
        );
        let mut offset = panel.view_scroll.offset();
        let next = (offset.y + px(speed * dt)).clamp(-panel.view_scroll.max_offset().y, px(0.));
        if next != offset.y {
            offset.y = next;
            panel.view_scroll.set_offset(offset);
            window.request_animation_frame();
        }
    }
}

pub(super) fn select_field(
    field: usize,
    panel: &mut WorkbenchPanel,
    value: String,
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) {
    if let Some(state) = &mut panel.project_settings {
        state.inputs[field].update(cx, |input, cx| input.set_value(value, window, cx));
    }
    cx.notify();
}

fn move_before(order: &mut Vec<String>, id: &str, before: Option<&str>) {
    if before == Some(id) || before.is_some_and(|target| !order.iter().any(|s| s == target)) {
        return;
    }
    let Some(from) = order.iter().position(|s| s == id) else {
        return;
    };
    let value = order.remove(from);
    let to = before
        .and_then(|target| order.iter().position(|s| s == target))
        .unwrap_or(order.len());
    order.insert(to, value);
}

fn apply(
    root: &Path,
    panel: &WorkbenchPanel,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), (Option<usize>, String)> {
    let state = panel
        .project_settings
        .as_ref()
        .ok_or_else(|| (None, crate::i18n::tr("Open project settings again").into()))?;
    let values = state
        .inputs
        .iter()
        .map(|i| i.read(cx).value().to_string())
        .collect::<Vec<_>>();
    if values[0].trim().is_empty() {
        return Err((Some(0), crate::i18n::tr("Enter a title").into()));
    }
    if !values[2].is_empty() && values[2] != state.original[2] {
        let path = Path::new(&values[2]);
        if !path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
            || !path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("png") || s.eq_ignore_ascii_case("webp"))
        {
            return Err((
                Some(2),
                crate::i18n::tr("Choose a PNG or WebP inside the project").into(),
            ));
        }
        let full = root
            .join(path)
            .canonicalize()
            .map_err(|_| (Some(2), crate::i18n::tr("Icon file was not found").into()))?;
        if !full.starts_with(root.canonicalize().map_err(|e| (Some(2), e.to_string()))?) {
            return Err((
                Some(2),
                crate::i18n::tr("Icon must be inside the project").into(),
            ));
        }
        let (w, h) = image::image_dimensions(full)
            .map_err(|_| (Some(2), crate::i18n::tr("Could not read the icon").into()))?;
        if w == 0 || w != h {
            return Err((Some(2), crate::i18n::tr("Choose a square icon").into()));
        }
    }
    if values[3] != state.original[3]
        && !cx
            .global::<EditorDocuments>()
            .authoring(root)
            .scenes
            .iter()
            .any(|scene| scene.name == values[3])
    {
        return Err((Some(3), crate::i18n::tr("Choose an existing scene").into()));
    }
    let mut changes = Vec::new();
    for (i, path) in ["title", "title_background", "project.icon", "script.entry"]
        .iter()
        .enumerate()
    {
        if values[i] != state.original[i] {
            changes.push((*path, Value::String(values[i].clone())));
        }
    }
    if state.extra != state.original_extra {
        changes.push(("features.extra", Value::Bool(state.extra)));
    }
    for (i, path) in ["extra.cg_order", "extra.bgm_order"].iter().enumerate() {
        if state.orders[i] != state.original_orders[i] {
            changes.push((
                *path,
                Value::Sequence(state.orders[i].iter().cloned().map(Value::String).collect()),
            ));
        }
    }
    if changes.is_empty() {
        return Ok(());
    }
    let updated = edit_config(&state.source, &changes).map_err(|e| (None, e))?;
    let path = PathBuf::from("config.yaml");
    edits::apply_checked_edits(
        root,
        &[(path.clone(), updated)],
        &BTreeMap::from([(path, state.source.clone())]),
        window,
        cx,
    )
    .map_err(|e| (None, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gallery_drop_inserts_at_the_visible_boundary() {
        let mut order = vec!["a".into(), "b".into(), "c".into()];
        move_before(&mut order, "a", Some("c"));
        assert_eq!(order, ["b", "a", "c"]);
        move_before(&mut order, "c", Some("b"));
        assert_eq!(order, ["c", "b", "a"]);
        move_before(&mut order, "c", None);
        assert_eq!(order, ["b", "a", "c"]);
        move_before(&mut order, "a", Some("a"));
        move_before(&mut order, "a", Some("missing"));
        assert_eq!(order, ["b", "a", "c"]);
    }
    #[test]
    fn project_form_preserves_unrelated_yaml_and_comments() {
        let source =
            "# my project\ntitle: Old # title note\nproject:\n  id: demo\ncustom: {keep: true}\n";
        let edited = edit_config(
            source,
            &[
                ("title", Value::String("A: new title".into())),
                ("features.extra", Value::Bool(true)),
                (
                    "extra.cg_order",
                    Value::Sequence(vec![Value::String("flower".into())]),
                ),
            ],
        )
        .unwrap();
        assert!(edited.contains("# my project"));
        assert!(edited.contains("# title note"));
        assert!(edited.contains("custom: {keep: true}"));
        let config = GameConfig::from_yaml(&edited).unwrap();
        assert_eq!(config.title, "A: new title");
        assert!(config.features.extra);
        assert_eq!(config.extra.cg_order, ["flower"]);
        assert_eq!(
            gallery_order(
                &["later".into(), "later".into()],
                ["b".into(), "a".into()].into_iter()
            ),
            ["later", "a", "b"]
        );
    }
}
