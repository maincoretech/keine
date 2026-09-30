use super::*;

use crate::authoring::fields::{source_field_choices, source_field_label, title_case};

pub(in crate::app) fn render_text_ending(
    root: &Path,
    key: &InspectorEditKey,
    inputs: &[Entity<InputState>],
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    if inputs.len() != 2 {
        return Empty.into_any_element();
    }
    let dialogue_root = root.to_owned();
    let character_root = root.to_owned();
    let target = inputs[0].clone();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_label("Ending"))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child("Keep dialogue"),
                )
                .child(
                    Switch::new("text-keep-dialogue")
                        .small()
                        .color(rgb(PRIMARY))
                        .checked(key.lifetime.text_box.is_none())
                        .accessibility_label("Keep dialogue")
                        .on_change(cx.listener(move |panel, keep: &bool, window, cx| {
                            panel.commit_text_lifetime(
                                &dialogue_root,
                                Some(*keep),
                                false,
                                window,
                                cx,
                            );
                        })),
                ),
        )
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child("Keep characters"),
                )
                .child(
                    Switch::new("text-keep-characters")
                        .small()
                        .color(rgb(PRIMARY))
                        .checked(key.lifetime.hide.is_none())
                        .accessibility_label("Keep characters")
                        .on_change(cx.listener(move |panel, keep: &bool, window, cx| {
                            if !keep && target.read(cx).value().trim().is_empty() {
                                target.update(cx, |target, cx| target.focus(window, cx));
                                cx.global_mut::<EditorDocuments>().set_notice(
                                    &character_root,
                                    "Enter a sprite ID or prefix* to hide on advance",
                                );
                                cx.refresh_windows();
                                return;
                            }
                            panel.commit_text_lifetime(&character_root, None, *keep, window, cx);
                        })),
                ),
        )
        .child(property_input("Hide target", &inputs[0]))
        .child(property_input("Transition", &inputs[1]))
        .into_any_element()
}

pub(in crate::app) fn run_source_block(root: &Path, path: &Path, start: usize, cx: &mut App) {
    let Some(source) = cx.global::<EditorDocuments>().source(root, path) else {
        return;
    };
    let projection = EiyashouProjection::parse(&source);
    let Some(block) = projection
        .scenes
        .iter()
        .flat_map(|scene| &scene.blocks)
        .find(|block| block.source_range.start == start && !block.read_only)
    else {
        return;
    };
    let documents = cx.global::<EditorDocuments>().preview_documents(root);
    let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(root) else {
        return;
    };
    if !preview.apply_sources(documents) {
        return;
    }
    if !matches!(
        preview.snapshot().lifecycle,
        PreviewLifecycle::Running | PreviewLifecycle::Starting
    ) {
        preview.start();
    }
    preview.seek_cursor(path.to_owned(), block.line + 1, block.column + 1);
    preview.show();
    cx.refresh_windows();
}

pub(in crate::app) fn segmented_source_property(
    root: &Path,
    key: &SourceInspectorKey,
    position: usize,
    cx: &mut Context<WorkbenchPanel>,
) -> Option<AnyElement> {
    let field = &key.fields[position];
    let default = match field.key.as_str() {
        "axis" => "both",
        "falloff" => "linear",
        "position" => "center",
        _ => return None,
    };
    let choices = source_field_choices(key, field);
    if !field.value.is_empty() && !choices.contains(&field.value.as_str()) {
        return None;
    }
    Some(
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(source_field_label(key, field)),
            )
            .child(div().flex().gap_1().children(choices.iter().map(|choice| {
                let root = root.to_owned();
                let key = key.clone();
                let choice = *choice;
                let active = if field.value.is_empty() {
                    choice == default
                } else {
                    choice == field.value
                };
                div()
                    .id(format!(
                        "source-segment-{}-{}-{choice}",
                        key.block_start, field.key
                    ))
                    .flex_1()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(rgb(if active { PRIMARY } else { BORDER }))
                    .bg(rgb(if active { PRIMARY_DIM } else { CANVAS }))
                    .text_size(px(11.))
                    .text_color(rgb(if active { PRIMARY } else { MUTED }))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.commit_source_field(
                            &root,
                            &key,
                            position,
                            choice.to_owned(),
                            window,
                            cx,
                        );
                    }))
                    .child(if choice == "both" {
                        "XY".to_owned()
                    } else {
                        title_case(choice)
                    })
            })))
            .into_any_element(),
    )
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::app) struct SourceOption {
    pub value: String,
    pub title: SharedString,
    pub asset: Option<(PathBuf, AssetKind, PathBuf)>,
}

impl SelectItem for SourceOption {
    type Value = String;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &String {
        &self.value
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.title.to_lowercase().contains(&query)
            || self.value.to_lowercase().contains(&query)
            || self
                .asset
                .as_ref()
                .is_some_and(|(_, _, path)| path.to_string_lossy().to_lowercase().contains(&query))
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some((root, kind, path)) = &self.asset else {
            return self.title.clone().into_any_element();
        };
        let image = matches!(
            kind,
            AssetKind::Background | AssetKind::Figure | AssetKind::Particle
        ) && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                gpui_kit::Img::extensions()
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(ext))
            });
        let preview = if image && let Some(file) = confined_existing_file(root, path) {
            img(file)
                .size_full()
                .with_fallback(|| Empty.into_any_element())
                .into_any_element()
        } else {
            Icon::new(match kind {
                AssetKind::Background | AssetKind::Figure | AssetKind::Particle => {
                    AssetIconName::Image
                }
                AssetKind::Video => AssetIconName::Video,
                _ => AssetIconName::Music,
            })
            .small()
            .into_any_element()
        };
        let audio = matches!(kind, AssetKind::Voice | AssetKind::Bgm | AssetKind::Effect);
        div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(40.))
                    .h(px(30.))
                    .flex_shrink_0()
                    .rounded(px(3.))
                    .bg(rgb(CANVAS))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(preview),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(11.))
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(self.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(rgb(MUTED))
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(path.display().to_string()),
                    ),
            )
            .when(audio, |this| {
                this.child(audition_control(root, path, true, cx))
            })
            .into_any_element()
    }
}

pub(in crate::app) fn source_options(
    root: &Path,
    key: &SourceInspectorKey,
    field: &SourceField,
    cx: &App,
) -> Vec<SourceOption> {
    let documents = cx.global::<EditorDocuments>();
    let source = (key.command == "track" && field.key == "0")
        .then(|| documents.source(root, &key.path))
        .flatten();
    let projection = source
        .as_ref()
        .map(|source| documents.projection(root, &key.path, source));
    crate::authoring::fields::field_options(
        root,
        key,
        field,
        documents.authoring_ref(root),
        source.as_deref().zip(projection.as_deref()),
    )
    .into_iter()
    .map(|option| SourceOption {
        value: option.value,
        title: option.title.into(),
        asset: option.asset,
    })
    .collect()
}

pub(super) fn retraction_property(
    key: &SourceInspectorKey,
    field: &SourceField,
    input: &Entity<TextareaState>,
) -> AnyElement {
    let label = source_field_label(key, field);
    let hint = if field.key == "source" {
        "Empty uses the current dialogue."
    } else {
        "A prefix of the full text. Empty erases the entire line."
    };

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(MUTED))
                .child(label.clone()),
        )
        .child(
            div()
                .w_full()
                .min_h(px(34.))
                .px_2()
                .py_1()
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(CANVAS))
                .child(
                    Textarea::new(input)
                        .aria_label(label)
                        .appearance(false)
                        .bordered(false)
                        .w_full()
                        .px_0()
                        .py_0()
                        .text_size(px(13.))
                        .text_color(rgb(INK)),
                ),
        )
        .child(div().text_size(px(10.)).text_color(rgb(MUTED)).child(hint))
        .into_any_element()
}

pub(in crate::app) fn typed_source_property(
    root: &Path,
    key: &SourceInspectorKey,
    position: usize,
    input: &Entity<InputState>,
    slider: Option<&Entity<SliderState>>,
    select: Option<&Entity<SelectState<Vec<SourceOption>>>>,
    cx: &mut Context<WorkbenchPanel>,
) -> Option<AnyElement> {
    let field = &key.fields[position];
    let enabled = source_field_enabled(key, field);
    let label = source_field_label(key, field);
    if select.is_some() {
        return Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child(label.clone()),
                )
                .child(
                    div()
                        .opacity(if enabled { 1. } else { 0.4 })
                        .child(resource_trigger(
                            root,
                            ResourceTarget::Source(key.clone(), position),
                            source_options(root, key, field, cx),
                            field.value.clone(),
                            false,
                            cx,
                        )),
                )
                .into_any_element(),
        );
    }
    let control = source_number(key, field)?;
    let state = slider?;
    Some(
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(label))
            .child(
                div()
                    .h(px(28.))
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(64.))
                            .h(px(26.))
                            .flex_shrink_0()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(CANVAS))
                            .px_2()
                            .child(
                                Input::new(input)
                                    .disabled(!enabled)
                                    .appearance(false)
                                    .bordered(false)
                                    .size_full()
                                    .px_0()
                                    .py_0()
                                    .text_align(gpui_kit::TextAlign::Right)
                                    .text_size(px(12.))
                                    .text_color(rgb(INK)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(22.))
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(control.unit),
                    )
                    .child(
                        div().flex_1().min_w_0().px_2().child(
                            Slider::new(state)
                                .disabled(!enabled)
                                .bg(rgb(PRIMARY))
                                .text_color(rgb(INK)),
                        ),
                    ),
            )
            .into_any_element(),
    )
}

pub(super) fn replay_button(root: &Path, key: &SourceInspectorKey) -> impl IntoElement {
    let root = root.to_owned();
    let path = key.path.clone();
    let start = key.block_start;
    div()
        .id(("source-replay", start))
        .h(px(24.))
        .px_2()
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CANVAS))
        .flex()
        .items_center()
        .gap_1()
        .text_size(px(11.))
        .text_color(rgb(INK))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .on_click(move |_, _, cx| run_source_block(&root, &path, start, cx))
        .child(Icon::new(AssetIconName::Play).xsmall())
        .child("Replay")
}

pub(super) fn render_wait_control(
    root: &Path,
    key: &SourceInspectorKey,
    input: &Entity<InputState>,
    compact: bool,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let advance = key.command == "wait.advance";
    let current = key
        .fields
        .first()
        .and_then(|field| source_number(key, field)?.parse(&field.value));
    let mut presets = div()
        .flex()
        .flex_wrap()
        .gap(px(if compact { 4. } else { 6. }));
    for duration in [200, 500, 1000, 2000, 3000]
        .into_iter()
        .map(Some)
        .chain([None])
    {
        let root = root.to_owned();
        let key = key.clone();
        let selected = duration.map_or(advance, |time| !advance && current == Some(time as f32));
        presets = presets.child(
            div()
                .id(format!("wait-{}-{duration:?}", key.block_start))
                .h(px(if compact { 22. } else { 26. }))
                .px(px(if compact { 8. } else { 10. }))
                .flex()
                .items_center()
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(if selected { PRIMARY } else { BORDER }))
                .bg(rgb(if selected { PRIMARY_DIM } else { CANVAS }))
                .text_size(px(if compact { 11. } else { 12. }))
                .text_color(rgb(if selected { PRIMARY } else { MUTED }))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    let duration = duration.map(|time| time.to_string());
                    this.commit_wait_mode(&root, &key, duration.as_deref(), window, cx);
                }))
                .child(
                    duration.map_or_else(|| "Wait for input".to_owned(), |time| time.to_string()),
                ),
        );
    }
    let numeric = div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(if compact { 64. } else { 110. }))
                .h(px(if compact { 24. } else { 30. }))
                .px_2()
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(CANVAS))
                .child(
                    Input::new(input)
                        .appearance(false)
                        .bordered(false)
                        .size_full()
                        .px_0()
                        .py_0()
                        .text_align(gpui_kit::TextAlign::Right)
                        .text_size(px(if compact { 13. } else { 15. })),
                ),
        )
        .child(div().text_size(px(12.)).text_color(rgb(MUTED)).child("ms"));
    let content = if compact {
        div()
            .relative()
            .flex()
            .items_center()
            .w_full()
            .gap_2()
            .child(numeric)
            .child(
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .p_1()
                    .bg(rgb(CANVAS))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .invisible()
                    .group_hover("block-row", |style| style.visible())
                    .child(presets),
            )
    } else {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_label("Duration"))
            .child(numeric)
            .child(section_label("Presets"))
            .child(presets)
            .when(advance, |this| {
                this.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(MUTED))
                        .child("Waits here until the player advances."),
                )
            })
    };
    let start = key.block_start;
    content
        .id(("wait-controls", start))
        .capture_any_mouse_down(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
            if compact && event.button == MouseButton::Left {
                this.select_current_block(start, cx);
            }
        }))
        .on_click(|_, _, cx| cx.stop_propagation())
        .into_any_element()
}

pub(in crate::app) fn render_inline_block(
    root: &Path,
    control: &InlineBlockControl,
    cx: &mut Context<WorkbenchPanel>,
) -> Option<AnyElement> {
    let key = &control.key;
    let position = control.position;
    let start = key.block_start;
    if matches!(key.command.as_str(), "wait" | "wait.advance") {
        return Some(render_wait_control(root, key, &control.input, true, cx));
    }
    let field = &key.fields[position];
    if source_asset_kind(key, field).is_some() {
        return Some(resource_trigger(
            root,
            ResourceTarget::Source(key.clone(), position),
            control.options.clone(),
            field.value.clone(),
            true,
            cx,
        ));
    }
    control.select.as_ref()?;
    Some(
        div()
            .id(("inline-asset", key.block_start))
            .flex_none()
            .max_w(px(320.))
            .min_w_0()
            .capture_any_mouse_down(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if event.button == MouseButton::Left {
                    this.select_current_block(start, cx);
                }
            }))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(resource_trigger(
                root,
                ResourceTarget::Source(key.clone(), position),
                control.options.clone(),
                field.value.clone(),
                true,
                cx,
            ))
            .into_any_element(),
    )
}

// These are UI entities over bounded source fields, not another document model.
pub(in crate::app) struct InlineBlockControl {
    pub key: SourceInspectorKey,
    pub position: usize,
    pub input: Entity<InputState>,
    pub select: Option<Entity<SelectState<Vec<SourceOption>>>>,
    pub options: Vec<SourceOption>,
    pub _subscriptions: Vec<Subscription>,
}

pub(in crate::app) fn source_property_editor(
    root: &Path,
    key: &SourceInspectorKey,
    position: usize,
    input: &Entity<InputState>,
    slider: Option<&Entity<SliderState>>,
    select: Option<&Entity<SelectState<Vec<SourceOption>>>>,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    if source_asset_kind(key, &key.fields[position]).is_some() {
        return div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(source_field_label(key, &key.fields[position])),
            )
            .child(resource_trigger(
                root,
                ResourceTarget::Source(key.clone(), position),
                source_options(root, key, &key.fields[position], cx),
                key.fields[position].value.clone(),
                false,
                cx,
            ))
            .into_any_element();
    }
    if let Some(control) = segmented_source_property(root, key, position, cx) {
        return control;
    }
    if let Some(control) = typed_source_property(root, key, position, input, slider, select, cx) {
        return control;
    }
    let field = &key.fields[position];
    let enabled = source_field_enabled(key, field);
    let label = source_field_label(key, field);
    let choices = source_field_choices(key, field);
    let root = root.to_owned();
    let key = key.clone();
    if choices == ["true", "false"] && matches!(field.value.as_str(), "" | "true" | "false") {
        let checked = field.value == "true"
            || (field.value.is_empty()
                && ((field.key == "blocking" && key.command != "assets.loading")
                    || field.key == "environment_light"
                    || field.key == "skippable"
                    || (key.command == "bgm" && field.key == "loop")
                    || (key.command == "sprite.focus.configure" && field.key == "enabled")
                    || (key.command == "video.play" && field.key == "wait")
                    || matches!(field.key.as_str(), "godray_parallel" | "speed_lines_radial")));
        return div()
            .w_full()
            .min_h(px(34.))
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(div().text_sm().text_color(rgb(INK)).child(label.clone()))
            .child(
                Switch::new(format!("source-switch-{}-{}", key.block_start, field.key))
                    .disabled(!enabled)
                    .checked(checked)
                    .accessibility_label(label)
                    .small()
                    .color(rgb(PRIMARY))
                    .on_change(cx.listener(move |this, checked: &bool, window, cx| {
                        this.commit_source_field(
                            &root,
                            &key,
                            position,
                            checked.to_string(),
                            window,
                            cx,
                        );
                    })),
            )
            .into_any_element();
    }
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .pb_3()
        .child(
            div()
                .text_xs()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(rgb(MUTED))
                .child(label),
        )
        .child(
            div()
                .h(px(34.))
                .w_full()
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(PANEL))
                .px_2()
                .flex()
                .items_center()
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(input)
                            .disabled(!enabled)
                            .appearance(false)
                            .bordered(false)
                            .size_full()
                            .text_sm()
                            .text_color(rgb(INK)),
                    ),
                ),
        )
        .when(!choices.is_empty(), |this| {
            this.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .children(choices.iter().map(|choice| {
                        let selected = field.value == *choice;
                        let value = (*choice).to_owned();
                        let root = root.clone();
                        let key = key.clone();
                        div()
                            .id(format!(
                                "source-choice-{}-{position}-{choice}",
                                key.block_start
                            ))
                            .px_2()
                            .py_1()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(if selected { PRIMARY } else { BORDER }))
                            .bg(rgb(if selected { PRIMARY_DIM } else { PANEL }))
                            .text_xs()
                            .text_color(rgb(if selected { PRIMARY } else { MUTED }))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.commit_source_field(
                                    &root,
                                    &key,
                                    position,
                                    value.clone(),
                                    window,
                                    cx,
                                );
                            }))
                            .child(*choice)
                    })),
            )
        })
        .into_any_element()
}
