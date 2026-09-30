use super::*;

mod controls;
mod spatial;
mod view;
pub(super) use controls::*;
pub(super) use view::*;

impl WorkbenchPanel {
    pub(super) fn apply_batch_block_field(
        &mut self,
        root: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((path, starts)) = cx
            .global::<EditorDocuments>()
            .block_selection(root)
            .cloned()
            .filter(|(_, starts)| starts.len() > 1)
        else {
            return;
        };
        let Some(source) = cx.global::<EditorDocuments>().source(root, &path) else {
            return;
        };
        let Some(field) = self.batch_block_field.clone() else {
            return;
        };
        let value = self.batch_block_input.read(cx).value().to_string();
        match batch_block_edit(&source, &starts, &field, &value) {
            Ok((edited, starts)) => {
                apply_workspace_edit(root, &path, edited, window, cx);
                cx.global_mut::<EditorDocuments>()
                    .set_block_selection(root, path, starts);
            }
            Err(error) => window.push_notification(Notification::error(error), cx),
        }
        cx.refresh_windows();
    }

    /// Switch between the two existing native Wait commands without a second
    /// block model or hidden source metadata.
    pub(super) fn commit_wait_mode(
        &mut self,
        root: &Path,
        key: &SourceInspectorKey,
        duration: Option<&str>,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        if !matches!(key.command.as_str(), "wait" | "wait.advance") {
            return false;
        }
        let replacement = if let Some(duration) = duration {
            let Ok(duration) = duration.parse::<f32>() else {
                return false;
            };
            if !duration.is_finite() || duration < 0. {
                return false;
            }
            format!("wait({duration}ms)")
        } else {
            "wait.advance()".to_owned()
        };
        let Some(source) = cx.global::<EditorDocuments>().source(root, &key.path) else {
            return false;
        };
        let projection = EiyashouProjection::parse(&source);
        let Some(block) = projection
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .find(|block| block.source_range.start == key.block_start && !block.read_only)
        else {
            return false;
        };
        if projection.source_fields_for_block(&source, block).as_ref() != Some(&key.fields)
            || block.summary.split('(').next().map(str::trim) != Some(&key.command)
        {
            return false;
        }
        let mut edited = source;
        edited.replace_range(block.source_range.clone(), &replacement);
        let checked = EiyashouProjection::parse(&edited);
        if checked.read_only.len() > projection.read_only.len() {
            return false;
        }
        apply_workspace_edit(root, &key.path, edited, window, cx);
        cx.refresh_windows();
        true
    }

    pub(super) fn commit_source_fields(
        &mut self,
        root: &Path,
        key: &SourceInspectorKey,
        updates: &[(String, Option<String>)],
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(source) = cx.global::<EditorDocuments>().source(root, &key.path) else {
            return;
        };
        let projection = EiyashouProjection::parse(&source);
        if projection.source_fields(&source, key.block_start).as_ref() != Some(&key.fields) {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "Source changed; refresh Inspector");
            cx.refresh_windows();
            return;
        }
        let Ok(edited) = projection.replace_block_fields(&source, key.block_start, updates) else {
            return;
        };
        let checked = EiyashouProjection::parse(&edited);
        if checked.read_only.len() > projection.read_only.len()
            || !checked
                .scenes
                .iter()
                .flat_map(|scene| &scene.blocks)
                .any(|block| block.source_range.start == key.block_start && block.kind == key.kind)
        {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "Invalid property value");
        } else {
            apply_workspace_edit(root, &key.path, edited, window, cx);
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "Properties updated");
        }
        cx.refresh_windows();
    }

    pub(super) fn refresh_inline_block_controls(
        &mut self,
        root: &Path,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = cx.global::<EditorDocuments>().source(root, path) else {
            self.inline_block_controls.clear();
            return;
        };
        let projection = cx
            .global::<EditorDocuments>()
            .projection(root, path, &source);
        let mut previous = std::mem::take(&mut self.inline_block_controls);
        let window_handle = window.window_handle();
        for block in projection.scenes.iter().flat_map(|scene| &scene.blocks) {
            if !self.block_visible.contains(&block.source_range.start) {
                continue;
            }
            if block.read_only || block.kind != BlockKind::Command {
                continue;
            }
            let Some(fields) = projection.source_fields_for_block(&source, block) else {
                continue;
            };
            let key = SourceInspectorKey {
                path: path.to_owned(),
                block_start: block.source_range.start,
                kind: block.kind.clone(),
                command: block
                    .summary
                    .split('(')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                fields,
            };
            if key.command == "wait.advance" {
                let input = previous
                    .remove(&key.block_start)
                    .filter(|control| {
                        control.key.path == key.path
                            && matches!(control.key.command.as_str(), "wait" | "wait.advance")
                    })
                    .map(|control| control.input)
                    .unwrap_or_else(|| {
                        cx.new(|cx| InputState::new(window, cx).default_value("1000"))
                    });
                let start = key.block_start;
                let root = root.to_owned();
                let initial_draft = input.read(cx).value().to_string();
                let subscription =
                    cx.subscribe(&input, move |panel, input, event: &InputEvent, cx| {
                        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                            return;
                        }
                        let Some(control) = panel.inline_block_controls.get(&start) else {
                            return;
                        };
                        if control.input.entity_id() != input.entity_id() {
                            return;
                        }
                        let key = control.key.clone();
                        let draft = input.read(cx).value().to_string();
                        let _ = cx.update_window(window_handle, |_, window, cx| {
                            if (draft != initial_draft
                                || matches!(event, InputEvent::PressEnter { .. }))
                                && !panel.commit_wait_mode(&root, &key, Some(&draft), window, cx)
                            {
                                input.update(cx, |input, cx| {
                                    input.set_value(initial_draft.clone(), window, cx)
                                });
                            }
                        });
                    });
                self.inline_block_controls.insert(
                    start,
                    InlineBlockControl {
                        key,
                        position: 0,
                        input,
                        select: None,
                        options: Vec::new(),
                        _subscriptions: vec![subscription],
                    },
                );
                continue;
            }
            let Some(position) = key.fields.iter().position(|field| {
                (key.command == "wait" && field.key == "0")
                    || (source_asset_kind(&key, field).is_some() && field.insertion.is_none())
            }) else {
                continue;
            };
            let field = &key.fields[position];
            let options = source_options(root, &key, field, cx);
            if let Some(mut control) = previous.remove(&key.block_start)
                && control.key.path == key.path
                && control.key.command == key.command
                && control.key.fields[control.position].key == field.key
            {
                let old_value =
                    source_input_value(&control.key, &control.key.fields[control.position]);
                let value = source_input_value(&key, field);
                if old_value != value && control.input.read(cx).value() == old_value {
                    control
                        .input
                        .update(cx, |input, cx| input.set_value(value, window, cx));
                }
                if let Some(select) = &control.select {
                    if control.options != options {
                        select.update(cx, |select, cx| {
                            select.set_items(options.clone(), window, cx)
                        });
                    }
                    if control.key.fields[control.position].value != field.value {
                        select.update(cx, |select, cx| {
                            select.set_selected_value(&field.value, window, cx)
                        });
                    }
                }
                control.options = options;
                control.position = position;
                control.key = key;
                self.inline_block_controls
                    .insert(control.key.block_start, control);
                continue;
            }
            let input = cx.new(|cx| {
                InputState::new(window, cx).default_value(source_input_value(&key, field))
            });
            let start = key.block_start;
            let input_root = root.to_owned();
            let mut subscriptions =
                vec![
                    cx.subscribe(&input, move |panel, input, event: &InputEvent, cx| {
                        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                            return;
                        }
                        let Some(control) = panel.inline_block_controls.get(&start) else {
                            return;
                        };
                        if control.input.entity_id() != input.entity_id() {
                            return;
                        }
                        let key = control.key.clone();
                        let position = control.position;
                        let value = source_input_commit(
                            &key,
                            &key.fields[position],
                            input.read(cx).value().to_string(),
                        );
                        let _ = cx.update_window(window_handle, |_, window, cx| {
                            if !panel.commit_source_field(
                                &input_root,
                                &key,
                                position,
                                value,
                                window,
                                cx,
                            ) {
                                input.update(cx, |input, cx| {
                                    input.set_value(
                                        source_input_value(&key, &key.fields[position]),
                                        window,
                                        cx,
                                    )
                                });
                            }
                        });
                    }),
                ];
            let select =
                (!options.is_empty() && source_asset_kind(&key, field).is_none()).then(|| {
                    let select = cx.new(|cx| {
                        let mut select =
                            SelectState::new(options.clone(), None, window, cx).searchable(true);
                        select.set_selected_value(&field.value, window, cx);
                        select
                    });
                    let root = root.to_owned();
                    subscriptions.push(cx.subscribe(
                        &select,
                        move |panel, select, event: &SelectEvent<Vec<SourceOption>>, cx| {
                            let SelectEvent::Confirm(Some(value)) = event else {
                                return;
                            };
                            let Some(control) = panel.inline_block_controls.get(&start) else {
                                return;
                            };
                            if control
                                .select
                                .as_ref()
                                .is_none_or(|active| active.entity_id() != select.entity_id())
                            {
                                return;
                            }
                            let key = control.key.clone();
                            let position = control.position;
                            let value = asset_source_value(&key.fields[position], value);
                            let _ = cx.update_window(window_handle, |_, window, cx| {
                                if !panel
                                    .commit_source_field(&root, &key, position, value, window, cx)
                                {
                                    select.update(cx, |select, cx| {
                                        select.set_selected_value(
                                            &key.fields[position].value,
                                            window,
                                            cx,
                                        )
                                    });
                                }
                            });
                        },
                    ));
                    select
                });
            self.inline_block_controls.insert(
                start,
                InlineBlockControl {
                    key,
                    position,
                    input,
                    select,
                    options,
                    _subscriptions: subscriptions,
                },
            );
        }
    }

    pub(super) fn commit_source_field(
        &mut self,
        root: &Path,
        key: &SourceInspectorKey,
        position: usize,
        value: String,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Some(field) = key.fields.get(position) else {
            return false;
        };
        if value == field.value {
            return true;
        }
        if value.trim().is_empty() && (!field.quoted || field.insertion.is_some()) {
            return false;
        }
        let Some(source) = cx.global::<EditorDocuments>().source(root, &key.path) else {
            return false;
        };
        let current = EiyashouProjection::parse(&source);
        let still_same = current
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .any(|block| {
                block.source_range.start == key.block_start
                    && block.kind == key.kind
                    && (block.kind != BlockKind::Command
                        || block.summary.split('(').next().map(str::trim)
                            == Some(key.command.as_str()))
            })
            && current
                .source_fields(&source, key.block_start)
                .is_some_and(|fields| fields.get(position) == Some(field));
        if !still_same {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "Source changed; refresh Inspector");
            cx.refresh_windows();
            return false;
        }
        let value = if field.quoted {
            escape_eiyashou_string(&value)
        } else {
            value
        };
        let replacement = field.insertion.as_ref().map_or_else(
            || value.clone(),
            |prefix| {
                format!(
                    "{prefix}{value}{}",
                    field.insertion_suffix.as_deref().unwrap_or_default()
                )
            },
        );
        let mut edited = source;
        edited.replace_range(field.range.clone(), &replacement);
        let checked = EiyashouProjection::parse(&edited);
        let block_intact = checked
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .any(|block| block.source_range.start == key.block_start && block.kind == key.kind);
        if !block_intact || checked.read_only.len() > current.read_only.len() {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "Invalid property value");
            cx.refresh_windows();
            return false;
        }
        apply_workspace_edit(root, &key.path, edited, window, cx);
        cx.global_mut::<EditorDocuments>()
            .set_notice(root, "Property updated");
        cx.refresh_windows();
        true
    }

    pub(super) fn refresh_source_inspector(
        &mut self,
        root: &Path,
        selected: Option<(PathBuf, usize)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = selected.and_then(|(path, block_start)| {
            if path.extension().and_then(|extension| extension.to_str()) != Some("shou") {
                return None;
            }
            let source = cx.global::<EditorDocuments>().source(root, &path)?;
            let projection = cx
                .global::<EditorDocuments>()
                .projection(root, &path, &source);
            let block = projection
                .scenes
                .iter()
                .flat_map(|scene| &scene.blocks)
                .find(|block| block.source_range.start == block_start)?;
            if block.disabled {
                return None;
            }
            let fields = projection.source_fields(&source, block_start)?;
            let command = if matches!(
                block.kind,
                BlockKind::Narration | BlockKind::Dialogue { .. }
            ) {
                "dialogue".to_owned()
            } else {
                source
                    .get(block.source_range.clone())?
                    .split('(')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_owned()
            };
            Some(SourceInspectorKey {
                path,
                block_start,
                kind: block.kind.clone(),
                command,
                fields,
            })
        });
        if self.source_inspector_key == next {
            return;
        }
        let previous_key = self.source_inspector_key.take();
        let previous_inputs = std::mem::take(&mut self.source_inspector_inputs);
        let previous_texts = std::mem::take(&mut self.source_inspector_texts);
        let previous_sliders = std::mem::take(&mut self.source_inspector_sliders);
        let previous_selects = std::mem::take(&mut self.source_inspector_selects);
        let same_block = previous_key
            .as_ref()
            .zip(next.as_ref())
            .is_some_and(|(old, new)| {
                old.path == new.path
                    && old.block_start == new.block_start
                    && old.kind == new.kind
                    && old.command == new.command
            });
        self.source_inspector_key = next.clone();
        self.source_inspector_subscriptions.clear();
        if !same_block {
            self.source_inspector_effect = None;
            self.source_position_draft = None;
        }
        let Some(key) = next else {
            return;
        };
        if key.command == "text.retract" {
            let window_handle = window.window_handle();
            for (position, field) in key.fields.iter().enumerate() {
                let input = if same_block {
                    previous_key.as_ref().and_then(|previous_key| {
                        let old_position = previous_key
                            .fields
                            .iter()
                            .position(|old| old.key == field.key)?;
                        let input = previous_texts.get(old_position)?.clone();
                        let old_value = &previous_key.fields[old_position].value;
                        if old_value != &field.value && input.read(cx).value() == *old_value {
                            input.update(cx, |input, cx| {
                                input.set_value(field.value.clone(), window, cx)
                            });
                        }
                        Some(input)
                    })
                } else {
                    None
                }
                .unwrap_or_else(|| {
                    cx.new(|cx| {
                        let mut input = TextareaState::new(window, cx)
                            .auto_grow(1, 4)
                            .submit_on_enter(true);
                        // set_value initializes wrapped rows; default_value leaves them pending.
                        input.set_value(field.value.clone(), window, cx);
                        input
                    })
                });
                let input_root = root.to_owned();
                self.source_inspector_subscriptions.push(cx.subscribe(
                    &input,
                    move |panel, input, event: &InputEvent, cx| {
                        if !matches!(
                            event,
                            InputEvent::PressEnter { shift: false, .. } | InputEvent::Blur
                        ) {
                            return;
                        }
                        let Some(key) = panel
                            .source_inspector_key
                            .clone()
                            .filter(|key| key.command == "text.retract")
                        else {
                            return;
                        };
                        if panel
                            .source_inspector_texts
                            .get(position)
                            .is_none_or(|active| active.entity_id() != input.entity_id())
                        {
                            return;
                        }
                        let draft = input.read(cx).value().to_string();
                        let _ = cx.update_window(window_handle, |_, window, cx| {
                            if !panel.commit_source_field(
                                &input_root,
                                &key,
                                position,
                                draft,
                                window,
                                cx,
                            ) {
                                input.update(cx, |input, cx| {
                                    input.set_value(key.fields[position].value.clone(), window, cx)
                                });
                            }
                        });
                    },
                ));
                self.source_inspector_texts.push(input);
            }
            return;
        }
        self.source_inspector_inputs = key
            .fields
            .iter()
            .map(|field| {
                if same_block
                    && let Some(previous_key) = previous_key.as_ref()
                    && let Some((old_position, old_field)) = previous_key
                        .fields
                        .iter()
                        .enumerate()
                        .find(|(_, old)| old.key == field.key)
                    && let Some(input) = previous_inputs.get(old_position)
                {
                    let old_value = source_input_value(previous_key, old_field);
                    let value = source_input_value(&key, field);
                    // Keep the input entity (and its focus/caret/draft) when a sibling
                    // property's bounded source edit moves the ranges.
                    if old_value != value && input.read(cx).value() == old_value {
                        input.update(cx, |input, cx| input.set_value(value, window, cx));
                    }
                    return input.clone();
                }
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(source_input_value(&key, field))
                        .placeholder(
                            source_number(&key, field)
                                .map(|control| control.default.to_string())
                                .unwrap_or_default(),
                        )
                })
            })
            .collect();
        let window_handle = window.window_handle();
        if key.command == "wait.advance" {
            let input = previous_inputs
                .first()
                .filter(|_| {
                    previous_key.as_ref().is_some_and(|previous| {
                        previous.path == key.path
                            && previous.block_start == key.block_start
                            && matches!(previous.command.as_str(), "wait" | "wait.advance")
                    })
                })
                .cloned()
                .unwrap_or_else(|| cx.new(|cx| InputState::new(window, cx).default_value("1000")));
            let root = root.to_owned();
            let initial_draft = input.read(cx).value().to_string();
            self.source_inspector_subscriptions.push(cx.subscribe(
                &input,
                move |panel, input, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        return;
                    }
                    let Some(key) = panel
                        .source_inspector_key
                        .clone()
                        .filter(|key| key.command == "wait.advance")
                    else {
                        return;
                    };
                    if panel
                        .source_inspector_inputs
                        .first()
                        .is_none_or(|active| active.entity_id() != input.entity_id())
                    {
                        return;
                    }
                    let draft = input.read(cx).value().to_string();
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        if (draft != initial_draft
                            || matches!(event, InputEvent::PressEnter { .. }))
                            && !panel.commit_wait_mode(&root, &key, Some(&draft), window, cx)
                        {
                            input.update(cx, |input, cx| {
                                input.set_value(initial_draft.clone(), window, cx)
                            });
                        }
                    });
                },
            ));
            self.source_inspector_inputs.push(input);
            return;
        }
        for (position, input) in self.source_inspector_inputs.iter().enumerate() {
            let root = root.to_owned();
            let field_name = key.fields[position].key.clone();
            self.source_inspector_subscriptions.push(cx.subscribe(
                input,
                move |panel, input, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        return;
                    }
                    let Some(key) = panel.source_inspector_key.clone() else {
                        return;
                    };
                    let Some(position) =
                        key.fields.iter().position(|field| field.key == field_name)
                    else {
                        return;
                    };
                    if panel.source_inspector_inputs[position].entity_id() != input.entity_id() {
                        return;
                    }
                    let value = source_input_commit(
                        &key,
                        &key.fields[position],
                        input.read(cx).value().to_string(),
                    );
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        if !panel.commit_source_field(&root, &key, position, value, window, cx) {
                            input.update(cx, |input, cx| {
                                input.set_value(
                                    source_input_value(&key, &key.fields[position]),
                                    window,
                                    cx,
                                )
                            });
                        }
                    });
                },
            ));
        }
        for field in &key.fields {
            if let Some(control) = source_number(&key, field) {
                let value = control
                    .parse(&field.value)
                    .unwrap_or(control.default)
                    .clamp(control.min, control.max);
                let state = if same_block && let Some(state) = previous_sliders.get(&field.key) {
                    state.update(cx, |state, cx| state.set_value(value, window, cx));
                    state.clone()
                } else {
                    cx.new(|_| {
                        SliderState::new()
                            .min(control.min)
                            .max(control.max)
                            .step(control.step)
                            .default_value(value)
                    })
                };
                let field_name = field.key.clone();
                let root = root.to_owned();
                self.source_inspector_subscriptions.push(cx.subscribe(
                    &state,
                    move |panel, slider, event: &SliderEvent, cx| {
                        let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
                        if panel
                            .source_inspector_sliders
                            .get(&field_name)
                            .is_none_or(|active| active.entity_id() != slider.entity_id())
                        {
                            return;
                        }
                        let Some(mut key) = panel.source_inspector_key.clone() else {
                            return;
                        };
                        // Pointer events may arrive before the next Inspector render.
                        // Read this command's current bounded fields for each update.
                        let Some(source) = cx.global::<EditorDocuments>().source(&root, &key.path)
                        else {
                            return;
                        };
                        let projection = EiyashouProjection::parse(&source);
                        let Some(block) = projection
                            .scenes
                            .iter()
                            .flat_map(|scene| &scene.blocks)
                            .find(|block| {
                                block.source_range.start == key.block_start
                                    && block.kind == key.kind
                                    && block.summary.split('(').next().map(str::trim)
                                        == Some(key.command.as_str())
                            })
                        else {
                            return;
                        };
                        let Some(fields) = projection.source_fields_for_block(&source, block)
                        else {
                            return;
                        };
                        key.fields = fields;
                        let Some(position) =
                            key.fields.iter().position(|field| field.key == field_name)
                        else {
                            return;
                        };
                        let value = control.source(value.start());
                        let _ = cx.update_window(window_handle, |_, window, cx| {
                            if !panel.commit_source_field(&root, &key, position, value, window, cx)
                            {
                                slider.update(cx, |slider, cx| {
                                    slider.set_value(
                                        control
                                            .parse(&key.fields[position].value)
                                            .unwrap_or(control.default)
                                            .clamp(control.min, control.max),
                                        window,
                                        cx,
                                    )
                                });
                            }
                        });
                    },
                ));
                self.source_inspector_sliders
                    .insert(field.key.clone(), state);
            }
            if source_asset_kind(&key, field).is_some() {
                continue;
            }
            let options = source_options(root, &key, field, cx);
            if options.is_empty() {
                continue;
            }
            let state = if same_block && let Some(state) = previous_selects.get(&field.key) {
                state.update(cx, |state, cx| {
                    state.set_items(options, window, cx);
                    state.set_selected_value(&field.value, window, cx)
                });
                state.clone()
            } else {
                cx.new(|cx| {
                    let mut state = SelectState::new(options, None, window, cx).searchable(true);
                    state.set_selected_value(&field.value, window, cx);
                    state
                })
            };
            let field_name = field.key.clone();
            let root = root.to_owned();
            self.source_inspector_subscriptions.push(cx.subscribe(
                &state,
                move |panel, select, event: &SelectEvent<Vec<SourceOption>>, cx| {
                    let SelectEvent::Confirm(Some(value)) = event else {
                        return;
                    };
                    if panel
                        .source_inspector_selects
                        .get(&field_name)
                        .is_none_or(|active| active.entity_id() != select.entity_id())
                    {
                        return;
                    }
                    let Some(key) = panel.source_inspector_key.clone() else {
                        return;
                    };
                    let Some(position) =
                        key.fields.iter().position(|field| field.key == field_name)
                    else {
                        return;
                    };
                    let field = &key.fields[position];
                    let value = if source_asset_kind(&key, field).is_some() {
                        asset_source_value(field, value)
                    } else {
                        value.clone()
                    };
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        if !panel.commit_source_field(&root, &key, position, value, window, cx) {
                            select.update(cx, |select, cx| {
                                select.set_selected_value(&field.value, window, cx)
                            });
                        }
                    });
                },
            ));
            self.source_inspector_selects
                .insert(field.key.clone(), state);
        }
    }

    pub(super) fn refresh_inspector_editors(
        &mut self,
        root: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asset_selection = cx.global::<EditorDocuments>().asset_selection(root);
        if !asset_selection.is_empty() {
            self.inspector_key = None;
            self.inspector_inputs.clear();
            self.text_lifetime_inputs.clear();
            self.inspector_selects.clear();
            self.inspector_subscriptions.clear();
            self.source_inspector_key = None;
            self.source_inspector_inputs.clear();
            self.source_inspector_sliders.clear();
            self.source_inspector_selects.clear();
            self.source_inspector_subscriptions.clear();
            self.refresh_asset_inspector(root, &asset_selection, window, cx);
            return;
        }
        self.asset_inspector_key = None;
        self.asset_inspector_inputs.clear();
        self.asset_inspector_subscriptions.clear();
        let selected = cx
            .global::<EditorDocuments>()
            .block_selection(root)
            .filter(|(_, starts)| starts.len() == 1)
            .map(|(path, starts)| (path.clone(), starts[0]))
            .or_else(|| {
                let (path, line, column) = cx.global::<EditorDocuments>().selection(root)?.clone();
                let source = cx.global::<EditorDocuments>().source(root, &path)?;
                let projection = cx
                    .global::<EditorDocuments>()
                    .projection(root, &path, &source);
                block_at_position(&projection, &source, line, column)
                    .map(|(_, block)| (path, block.source_range.start))
            });
        self.refresh_source_inspector(root, selected.clone(), window, cx);
        let next = selected.and_then(|(path, block_start)| {
            if path.extension().and_then(|extension| extension.to_str()) != Some("shou") {
                return None;
            }
            let source = cx.global::<EditorDocuments>().source(root, &path)?;
            let projection = cx
                .global::<EditorDocuments>()
                .projection(root, &path, &source);
            let metadata = projection.text_block_metadata(&source, block_start)?;
            let lifetime = projection.text_lifetime(&source, block_start)?;
            Some(InspectorEditKey {
                path,
                block_start,
                metadata,
                lifetime,
            })
        });
        if self.inspector_key == next {
            return;
        }
        let previous_key = self.inspector_key.take();
        let previous_inputs = std::mem::take(&mut self.inspector_inputs);
        let previous_selects = std::mem::take(&mut self.inspector_selects);
        let same_block = previous_key
            .as_ref()
            .zip(next.as_ref())
            .is_some_and(|(old, new)| old.path == new.path && old.block_start == new.block_start);
        self.inspector_key = next.clone();
        self.inspector_subscriptions.clear();
        let previous_lifetime_inputs = std::mem::take(&mut self.text_lifetime_inputs);
        let Some(key) = next else {
            return;
        };
        let lifetime_values = [&key.lifetime.target, &key.lifetime.transition];
        let old_lifetime_values = previous_key
            .as_ref()
            .map(|key| [&key.lifetime.target, &key.lifetime.transition]);
        self.text_lifetime_inputs = lifetime_values
            .iter()
            .enumerate()
            .map(|(position, value)| {
                if same_block && let Some(input) = previous_lifetime_inputs.get(position) {
                    if let Some(old) = &old_lifetime_values
                        && old[position] != *value
                        && input.read(cx).value().as_ref() == old[position].as_str()
                    {
                        input.update(cx, |input, cx| {
                            input.set_value((*value).clone(), window, cx)
                        });
                    }
                    return input.clone();
                }
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value((*value).clone())
                        .placeholder(if position == 0 {
                            "Sprite ID / prefix*"
                        } else {
                            "fade(200ms)"
                        })
                })
            })
            .collect();
        let lifetime_window = window.window_handle();
        for input in &self.text_lifetime_inputs {
            let root = root.to_owned();
            self.inspector_subscriptions.push(cx.subscribe(
                input,
                move |panel, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur)
                        && panel
                            .text_lifetime_inputs
                            .iter()
                            .any(|active| active.entity_id() == input.entity_id())
                    {
                        let _ = cx.update_window(lifetime_window, |_, window, cx| {
                            panel.commit_text_lifetime(&root, None, false, window, cx)
                        });
                    }
                },
            ));
        }
        let values = text_inspector_values(&key.metadata);
        let old_values = previous_key
            .as_ref()
            .map(|key| text_inspector_values(&key.metadata));
        let placeholders = ["Narrator / character id", "Voice id", "Stable ID"];
        self.inspector_inputs = values
            .iter()
            .zip(placeholders)
            .enumerate()
            .map(|(position, (value, placeholder))| {
                if same_block && let Some(input) = previous_inputs.get(position) {
                    if let Some(old_values) = &old_values
                        && old_values[position] != *value
                        && input.read(cx).value() == old_values[position]
                    {
                        input.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
                    }
                    return input.clone();
                }
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(value.clone())
                        .placeholder(placeholder)
                })
            })
            .collect();
        let window_handle = window.window_handle();
        for input in &self.inspector_inputs {
            let root = root.to_owned();
            self.inspector_subscriptions.push(cx.subscribe(
                input,
                move |panel, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur)
                        && panel
                            .inspector_inputs
                            .iter()
                            .any(|active| active.entity_id() == input.entity_id())
                    {
                        let _ = cx.update_window(window_handle, |_, window, cx| {
                            panel.commit_text_inspector(&root, window, cx)
                        });
                    }
                },
            ));
        }
        let index = cx.global::<EditorDocuments>().authoring(root);
        let speakers = speaker_options(&index);
        let mut voices = vec![SourceOption {
            value: String::new(),
            title: "No voice".into(),
            asset: None,
        }];
        voices.extend(
            index
                .assets
                .iter()
                .filter(|asset| asset.kind == AssetKind::Voice)
                .map(|asset| SourceOption {
                    value: asset.id.clone(),
                    title: asset
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&asset.id)
                        .to_owned()
                        .into(),
                    asset: Some((root.to_owned(), asset.kind, asset.path.clone())),
                }),
        );
        for (position, mut options) in [speakers, voices].into_iter().enumerate() {
            let value = &values[position];
            if !options.iter().any(|option| option.value == *value) {
                options.push(SourceOption {
                    value: value.clone(),
                    title: value.clone().into(),
                    asset: None,
                });
            }
            let state = if same_block && let Some(state) = previous_selects.get(position) {
                state.update(cx, |state, cx| {
                    state.set_items(options, window, cx);
                    state.set_selected_value(value, window, cx);
                });
                state.clone()
            } else {
                cx.new(|cx| {
                    let mut state = SelectState::new(options, None, window, cx).searchable(true);
                    state.set_selected_value(value, window, cx);
                    state
                })
            };
            let root = root.to_owned();
            self.inspector_subscriptions.push(cx.subscribe(
                &state,
                move |panel, state, event: &SelectEvent<Vec<SourceOption>>, cx| {
                    let SelectEvent::Confirm(Some(value)) = event else {
                        return;
                    };
                    if panel
                        .inspector_selects
                        .get(position)
                        .is_none_or(|active| active.entity_id() != state.entity_id())
                    {
                        return;
                    }
                    let value = value.clone();
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        panel.inspector_inputs[position]
                            .update(cx, |input, cx| input.set_value(value, window, cx));
                        panel.commit_text_inspector(&root, window, cx);
                    });
                },
            ));
            self.inspector_selects.push(state);
        }
    }

    pub(super) fn commit_text_lifetime(
        &mut self,
        root: &Path,
        keep_dialogue: Option<bool>,
        keep_characters: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(key) = self.inspector_key.clone() else {
            return;
        };
        if self.text_lifetime_inputs.len() != 2 {
            return;
        }
        let target = if keep_characters {
            String::new()
        } else {
            self.text_lifetime_inputs[0].read(cx).value().to_string()
        };
        let transition = self.text_lifetime_inputs[1].read(cx).value().to_string();
        let keep_dialogue = keep_dialogue.unwrap_or(key.lifetime.text_box.is_none());
        if keep_dialogue == key.lifetime.text_box.is_none()
            && target.trim() == key.lifetime.target
            && transition.trim() == key.lifetime.transition
        {
            return;
        }
        let Some(source) = cx.global::<EditorDocuments>().source(root, &key.path) else {
            return;
        };
        let projection = EiyashouProjection::parse(&source);
        let edited =
            if projection.text_lifetime(&source, key.block_start).as_ref() == Some(&key.lifetime) {
                projection.replace_text_lifetime(
                    &source,
                    key.block_start,
                    keep_dialogue,
                    &target,
                    &transition,
                )
            } else {
                Err(crate::authoring::projection::BlockEditError::StaleRange)
            };
        match edited {
            Ok(edited) => {
                if edited == source {
                    return;
                }
                apply_workspace_edit(root, &key.path, edited, window, cx);
                cx.global_mut::<EditorDocuments>().set_block_selection(
                    root,
                    key.path.clone(),
                    vec![key.block_start],
                );
            }
            Err(error) => {
                for (input, original) in self
                    .text_lifetime_inputs
                    .iter()
                    .zip([&key.lifetime.target, &key.lifetime.transition])
                {
                    input.update(cx, |input, cx| {
                        input.set_value(original.clone(), window, cx)
                    });
                }
                cx.global_mut::<EditorDocuments>()
                    .set_notice(root, format!("Text ending edit blocked: {error}"));
            }
        }
        cx.refresh_windows();
    }

    pub(super) fn commit_text_inspector(&mut self, root: &Path, window: &mut Window, cx: &mut App) {
        let Some(key) = self.inspector_key.clone() else {
            return;
        };
        if self.inspector_inputs.len() != 3 {
            return;
        }
        let values = self
            .inspector_inputs
            .iter()
            .map(|input| input.read(cx).value().to_string())
            .collect::<Vec<_>>();
        let metadata = TextBlockMetadata {
            speaker: (!values[0].trim().is_empty() && !values[0].eq_ignore_ascii_case("Narrator"))
                .then(|| values[0].trim().to_owned()),
            voice: (!values[1].trim().is_empty()).then(|| values[1].trim().to_owned()),
            stable_id: (!values[2].trim().is_empty()).then(|| values[2].trim().to_owned()),
        };
        if metadata == key.metadata {
            return;
        }
        let Some(source) = cx.global::<EditorDocuments>().source(root, &key.path) else {
            return;
        };
        let projection = EiyashouProjection::parse(&source);
        if projection
            .text_block_metadata(&source, key.block_start)
            .as_ref()
            != Some(&key.metadata)
        {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "Source changed; refresh Inspector");
        } else if metadata.stable_id != key.metadata.stable_id
            && metadata.stable_id.as_ref().is_some_and(|id| {
                cx.global::<EditorDocuments>()
                    .explicit_source_ids(root)
                    .contains(id)
            })
        {
            cx.global_mut::<EditorDocuments>()
                .set_notice(root, "Stable ID already exists");
        } else {
            match projection.replace_text_block_metadata(&source, key.block_start, &metadata) {
                Ok(edited) => {
                    apply_workspace_edit(root, &key.path, edited, window, cx);
                    cx.global_mut::<EditorDocuments>().set_block_selection(
                        root,
                        key.path.clone(),
                        vec![key.block_start],
                    );
                    cx.global_mut::<EditorDocuments>()
                        .set_notice(root, "Text properties updated");
                }
                Err(error) => cx
                    .global_mut::<EditorDocuments>()
                    .set_notice(root, format!("Text properties blocked: {error}")),
            }
        }
        cx.refresh_windows();
    }

    pub(super) fn refresh_asset_inspector(
        &mut self,
        root: &Path,
        selection: &[AssetKey],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = cx.global::<EditorDocuments>().authoring(root);
        let next = selection
            .first()
            .filter(|_| selection.len() == 1)
            .and_then(|key| index.assets.iter().find(|asset| asset.key() == *key))
            .map(|asset| (asset.key(), asset.path.clone(), asset.tags.clone()));
        if self.asset_inspector_key == next {
            return;
        }
        self.asset_inspector_key = next.clone();
        self.asset_inspector_inputs.clear();
        self.asset_inspector_subscriptions.clear();
        let Some((key, _, _)) = next else {
            return;
        };
        let Some(asset) = index.assets.iter().find(|asset| asset.key() == key) else {
            return;
        };
        for (value, placeholder) in [
            (asset.id.clone(), "ID"),
            (asset.kind.label().to_owned(), "Type"),
            (asset.tags.join(", "), "Tags"),
        ] {
            self.asset_inspector_inputs.push(cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(placeholder)
            }));
        }
        let inputs = self.asset_inspector_inputs.clone();
        let root = root.to_owned();
        let window_handle = window.window_handle();
        for input in &inputs {
            let inputs = inputs.clone();
            let root = root.clone();
            let key = key.clone();
            self.asset_inspector_subscriptions.push(cx.subscribe(
                input,
                move |panel: &mut WorkbenchPanel, _, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. }) {
                        return;
                    }
                    let values = inputs
                        .iter()
                        .map(|input| input.read(cx).value().to_string())
                        .collect::<Vec<_>>();
                    let kind = match values[1].trim().to_ascii_lowercase().as_str() {
                        "background" => AssetKind::Background,
                        "figure" => AssetKind::Figure,
                        "voice" => AssetKind::Voice,
                        "bgm" => AssetKind::Bgm,
                        "effect" | "se" => AssetKind::Effect,
                        "video" => AssetKind::Video,
                        "particle" => AssetKind::Particle,
                        _ => {
                            panel.asset_inspector_key = None;
                            cx.global_mut::<EditorDocuments>()
                                .set_notice(&root, "Unknown asset type");
                            cx.refresh_windows();
                            return;
                        }
                    };
                    let tags = values[2]
                        .split(',')
                        .map(str::trim)
                        .filter(|tag| !tag.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    let index = cx.global::<EditorDocuments>().authoring(&root);
                    let Some(asset) = index.assets.iter().find(|asset| asset.key() == key) else {
                        return;
                    };
                    let result = if !cx.global::<EditorDocuments>().authoring_is_current(&root) {
                        Err("Script index is updating; try again shortly".to_owned())
                    } else {
                        prepare_asset_edits(
                            &root,
                            &index,
                            asset,
                            values[0].trim(),
                            kind,
                            &tags,
                            |path| cx.global::<EditorDocuments>().source(&root, path),
                        )
                    };
                    match result {
                        Ok(mut edits) => {
                            let id = values[0].trim().to_owned();
                            let path = file_ops::asset_edit_path(asset, &id, kind, panel.asset_rename_file);
                            if path != asset.path {
                                let update = edits.iter_mut().find(|(path, _)| Some(path) == index.assets_manifest.as_ref());
                                if let Some((_, source)) = update {
                                    match file_ops::edit_manifest_asset_path(source, &asset.path, &path) {
                                        Ok(updated) => *source = updated,
                                        Err(error) => { cx.global_mut::<EditorDocuments>().set_notice(&root, error.to_string()); return; }
                                    }
                                }
                            }
                            if path != asset.path && index.assets.iter().any(|other| other.path == asset.path && other.key() != asset.key()) {
                                panel.asset_inspector_key = None;
                                cx.global_mut::<EditorDocuments>().set_notice(&root, "File is mapped more than once; resolve duplicate mappings before moving it");
                                return;
                            }
                            let files = if path == asset.path || !asset.exists { Vec::new() } else { vec![file_ops::AssetFileChange::relocate(asset.path.clone(), path.clone())] };
                            let message = format!("{} → {}\n{} → {}\n{} reference(s) in {} source file(s).{}", asset.id, id, asset.path.display(), path.display(), asset.reference_count, edits.len().saturating_sub(1), if asset.kind != kind { "\nReferences incompatible with the new type will be reported in Problems." } else { "" });
                            let receiver = if id != asset.id || kind != asset.kind {
                                match cx.update_window(window_handle, |_, window, cx| window.prompt(PromptLevel::Warning, "Update asset?", Some(&message), &[PromptButton::Other("Apply".into()), PromptButton::Cancel("Cancel".into())], cx)) {
                                    Ok(receiver) => Some(receiver),
                                    Err(error) => { panel.asset_inspector_key = None; cx.global_mut::<EditorDocuments>().set_notice(&root, error.to_string()); return; }
                                }
                            } else { None };
                            let baselines = edits.iter().filter_map(|(path, _)| cx.global::<EditorDocuments>().source(&root, path).map(|source| (path.clone(), source))).collect::<Vec<_>>();
                            let root = root.clone();
                            cx.spawn(async move |this, cx| {
                                if let Some(receiver) = receiver && receiver.await.ok() != Some(0) {
                                    let _ = this.update(cx, |panel, cx| { panel.asset_inspector_key = None; cx.refresh_windows(); });
                                    return;
                                }
                                let _ = this.update(cx, |panel, cx| {
                                    let applied = cx.update_window(window_handle, |_, window, cx| {
                                        if baselines.len() != edits.len() || baselines.iter().any(|(path, before)| cx.global::<EditorDocuments>().source(&root, path).as_ref() != Some(before)) {
                                            return Err("Source changed while confirming; refresh Inspector".into());
                                        }
                                        let result = apply_asset_transaction(&root, &edits, files, window, cx);
                                        if result.is_ok() { panel.focus.focus(window, cx); }
                                        result
                                    });
                                    match applied {
                                        Ok(Ok(())) => {
                                            cx.global_mut::<EditorDocuments>().set_asset_selection(&root, vec![AssetKey { kind, id }]);
                                            cx.global_mut::<EditorDocuments>().set_notice(&root, "Asset updated");
                                        }
                                        Ok(Err(error)) => { panel.asset_inspector_key = None; cx.global_mut::<EditorDocuments>().set_notice(&root, format!("Asset: {error}")); }
                                        Err(error) => { cx.global_mut::<EditorDocuments>().set_notice(&root, error.to_string()); }
                                    }
                                    cx.refresh_windows();
                                });
                            }).detach();
                        }
                        Err(error) => {
                            panel.asset_inspector_key = None;
                            cx.global_mut::<EditorDocuments>()
                                .set_notice(&root, format!("Asset: {error}"));
                        }
                    }
                    cx.refresh_windows();
                },
            ));
        }
    }
}

fn text_inspector_values(metadata: &TextBlockMetadata) -> [String; 3] {
    [
        metadata
            .speaker
            .clone()
            .unwrap_or_else(|| "Narrator".into()),
        metadata.voice.clone().unwrap_or_default(),
        metadata.stable_id.clone().unwrap_or_default(),
    ]
}
