//! Project configuration form. The document transaction remains the only writer.
use super::*;
use keine_core::config::GameConfig;
use noyalib::{Value, cst::parse_document};

pub(super) struct Settings {
    source: String,
    inputs: Vec<Entity<InputState>>,
    original: Vec<String>,
    extra: bool,
    original_extra: bool,
    orders: [Vec<String>; 2],
    original_orders: [Vec<String>; 2],
    error: Option<String>,
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
                    .child(format!("Could not read project settings: {error}"))
                    .into_any_element();
            }
        };
        let original = vec![config.title, config.title_background, config.project.icon];
        let inputs = original
            .iter()
            .map(|value| {
                cx.new(|cx| {
                    let mut input = InputState::new(window, cx);
                    input.set_value(value.clone(), window, cx);
                    input
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
            orders,
            error: None,
        });
    }
    let state = panel.project_settings.as_ref().unwrap();
    let mut content = div()
        .w_full()
        .max_w(px(640.))
        .flex()
        .flex_col()
        .gap_3()
        .p_3();
    for (label, input) in [
        "Title",
        "Title background (asset ID or path)",
        "Application icon (project PNG / WebP path)",
    ]
    .into_iter()
    .zip(&state.inputs)
    {
        content = content
            .child(div().text_xs().text_color(rgb(MUTED)).child(label))
            .child(tool_input(input));
    }
    let extra = state.extra;
    content = content.child(
        tool_action(if extra {
            "Extra: enabled"
        } else {
            "Extra: disabled"
        })
        .on_click(cx.listener(|panel, _, _, cx| {
            if let Some(state) = &mut panel.project_settings {
                state.extra = !state.extra;
            }
            cx.notify();
        })),
    );
    if extra {
        content = content.child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child("Drag to reorder. Only unlocked entries appear in-game."),
        );
        for (section, title) in ["CG order", "Music order"].into_iter().enumerate() {
            content = content.child(section_label(title));
            for id in &state.orders[section] {
                let target = id.clone();
                let drag = GalleryDrag {
                    owner: cx.entity_id(),
                    section,
                    id: id.clone(),
                };
                content = content.child(
                    div()
                        .id(format!("gallery-order-{section}-{id}"))
                        .flex()
                        .items_center()
                        .gap_2()
                        .p_2()
                        .rounded(px(7.))
                        .bg(rgb(SURFACE))
                        .cursor_move()
                        .drag_over::<GalleryDrag>(|style, _, _, _| style.bg(rgb(PRIMARY_DIM)))
                        .on_drag(drag, |drag, _, _, cx| {
                            cx.new(|_| BlockDragPreview {
                                label: drag.id.clone(),
                                summary: String::new(),
                                icon: AssetIconName::GripVertical,
                                count: 1,
                                width: 240.,
                                height: 34.,
                                color: PRIMARY,
                                grip_top: 8.,
                            })
                        })
                        .on_drop(cx.listener(move |panel, drag: &GalleryDrag, _, cx| {
                            if drag.owner != cx.entity_id() || drag.section != section {
                                return;
                            }
                            if let Some(state) = &mut panel.project_settings {
                                let order = &mut state.orders[section];
                                if let (Some(from), Some(to)) = (
                                    order.iter().position(|s| s == &drag.id),
                                    order.iter().position(|s| s == &target),
                                ) {
                                    let value = order.remove(from);
                                    order.insert(to, value);
                                    cx.notify();
                                }
                            }
                        }))
                        .child(
                            Icon::new(AssetIconName::GripVertical)
                                .xsmall()
                                .text_color(rgb(MUTED)),
                        )
                        .child(div().text_sm().child(id.clone())),
                );
            }
        }
    }
    let root_apply = root.to_owned();
    let root_source = root.to_owned();
    content = content
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    tool_action("Apply").on_click(cx.listener(move |panel, _, window, cx| {
                        let result = apply(&root_apply, panel, window, cx);
                        if let Err(error) = result {
                            panel.project_settings.as_mut().unwrap().error = Some(error);
                        } else {
                            panel.project_settings = None;
                        }
                        cx.refresh_windows();
                    })),
                )
                .child(
                    tool_action("Reset form").on_click(cx.listener(|panel, _, _, cx| {
                        panel.project_settings = None;
                        cx.notify();
                    })),
                )
                .child(tool_action("Edit YAML").on_click(move |_, window, cx| {
                    navigate_source(&root_source, Path::new("config.yaml"), 1, 1, window, cx)
                })),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child("Apply, then save. Restart Preview to use the new settings."),
        );
    if let Some(error) = &state.error {
        content = content.child(
            div()
                .text_sm()
                .text_color(rgb(0xdb7780))
                .child(error.clone()),
        );
    }
    div()
        .id("project-settings-scroll")
        .size_full()
        .overflow_y_scroll()
        .track_scroll(&panel.view_scroll)
        .child(content)
        .into_any_element()
}

fn apply(
    root: &Path,
    panel: &WorkbenchPanel,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), String> {
    let state = panel
        .project_settings
        .as_ref()
        .ok_or("Open project settings again")?;
    let values = state
        .inputs
        .iter()
        .map(|i| i.read(cx).value().to_string())
        .collect::<Vec<_>>();
    if values[0].trim().is_empty() {
        return Err("Enter a title".into());
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
            return Err("Choose a PNG or WebP inside the project".into());
        }
        let full = root
            .join(path)
            .canonicalize()
            .map_err(|_| "Icon file was not found")?;
        if !full.starts_with(root.canonicalize().map_err(|e| e.to_string())?) {
            return Err("Icon must be inside the project".into());
        }
        let (w, h) = image::image_dimensions(full).map_err(|_| "Could not read the icon")?;
        if w == 0 || w != h {
            return Err("Choose a square icon".into());
        }
    }
    let mut changes = Vec::new();
    for (i, path) in ["title", "title_background", "project.icon"]
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
    let updated = edit_config(&state.source, &changes)?;
    let path = PathBuf::from("config.yaml");
    edits::apply_checked_edits(
        root,
        &[(path.clone(), updated)],
        &BTreeMap::from([(path, state.source.clone())]),
        window,
        cx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
