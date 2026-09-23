use super::*;

impl WorkbenchPanel {
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
            let projection = EiyashouProjection::parse(&source);
            let block = projection
                .scenes
                .iter()
                .flat_map(|scene| &scene.blocks)
                .find(|block| block.source_range.start == block_start)?;
            let fields = projection.source_fields(&source, block_start)?;
            if fields.is_empty() {
                return None;
            }
            let command = source
                .get(block.source_range.clone())?
                .split('(')
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned();
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
        self.source_inspector_key = next.clone();
        self.source_inspector_inputs.clear();
        self.source_inspector_subscriptions.clear();
        let Some(key) = next else {
            return;
        };
        self.source_inspector_inputs = key
            .fields
            .iter()
            .map(|field| {
                cx.new(|cx| InputState::new(window, cx).default_value(field.value.clone()))
            })
            .collect();
        let window_handle = window.window_handle();
        for (position, input) in self.source_inspector_inputs.iter().enumerate() {
            let root = root.to_owned();
            let key = key.clone();
            self.source_inspector_subscriptions.push(cx.subscribe(
                input,
                move |_, input, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. }) {
                        return;
                    }
                    let Some(field) = key.fields.get(position) else {
                        return;
                    };
                    let value = input.read(cx).value().to_string();
                    if value == field.value
                        || (value.trim().is_empty() && (!field.quoted || field.insertion.is_some()))
                    {
                        return;
                    }
                    let Some(source) = cx.global::<EditorDocuments>().source(&root, &key.path)
                    else {
                        return;
                    };
                    let current = EiyashouProjection::parse(&source);
                    let still_same =
                        current
                            .scenes
                            .iter()
                            .flat_map(|scene| &scene.blocks)
                            .any(|block| {
                                block.source_range.start == key.block_start
                                    && block.kind == key.kind
                            })
                            && current
                                .source_fields(&source, key.block_start)
                                .is_some_and(|fields| fields.get(position) == Some(field));
                    if !still_same {
                        cx.global_mut::<EditorDocuments>()
                            .set_notice(&root, "Source changed; refresh Inspector");
                        cx.refresh_windows();
                        return;
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
                    let block_intact =
                        checked
                            .scenes
                            .iter()
                            .flat_map(|scene| &scene.blocks)
                            .any(|block| {
                                block.source_range.start == key.block_start
                                    && block.kind == key.kind
                            });
                    if !block_intact || checked.read_only.len() > current.read_only.len() {
                        cx.global_mut::<EditorDocuments>()
                            .set_notice(&root, "Invalid property value");
                        cx.refresh_windows();
                        return;
                    }
                    let result = cx.update_window(window_handle, |_, window, cx| {
                        apply_workspace_edit(&root, &key.path, edited, window, cx);
                    });
                    cx.global_mut::<EditorDocuments>().set_notice(
                        &root,
                        if result.is_ok() {
                            "Property updated".to_owned()
                        } else {
                            "Property update failed".to_owned()
                        },
                    );
                    cx.refresh_windows();
                },
            ));
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
            self.inspector_subscriptions.clear();
            self.source_inspector_key = None;
            self.source_inspector_inputs.clear();
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
                projected_block_at(&source, line, column)
                    .map(|(_, block)| (path, block.source_range.start))
            });
        self.refresh_source_inspector(root, selected.clone(), window, cx);
        let next = selected.and_then(|(path, block_start)| {
            if path.extension().and_then(|extension| extension.to_str()) != Some("shou") {
                return None;
            }
            let source = cx.global::<EditorDocuments>().source(root, &path)?;
            let metadata =
                EiyashouProjection::parse(&source).text_block_metadata(&source, block_start)?;
            Some(InspectorEditKey {
                path,
                block_start,
                metadata,
            })
        });
        if self.inspector_key == next {
            return;
        }
        self.inspector_key = next.clone();
        self.inspector_inputs.clear();
        self.inspector_subscriptions.clear();
        let Some(key) = next else {
            return;
        };
        let values = [
            key.metadata
                .speaker
                .clone()
                .unwrap_or_else(|| "Narrator".to_owned()),
            key.metadata.voice.clone().unwrap_or_default(),
            key.metadata.stable_id.clone().unwrap_or_default(),
        ];
        let placeholders = ["Narrator / character id", "Voice id", "Stable ID"];
        self.inspector_inputs = values
            .into_iter()
            .zip(placeholders)
            .map(|(value, placeholder)| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(value)
                        .placeholder(placeholder)
                })
            })
            .collect();
        let inputs = self.inspector_inputs.clone();
        let window_handle = window.window_handle();
        for input in &inputs {
            let inputs = inputs.clone();
            let root = root.to_owned();
            let key = key.clone();
            self.inspector_subscriptions.push(cx.subscribe(
                input,
                move |_, _, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. }) {
                        return;
                    }
                    let values = inputs
                        .iter()
                        .map(|input| input.read(cx).value().to_string())
                        .collect::<Vec<_>>();
                    let speaker = values[0].trim();
                    let voice = values[1].trim();
                    let stable_id = values[2].trim();
                    let metadata = TextBlockMetadata {
                        speaker: (!speaker.is_empty() && !speaker.eq_ignore_ascii_case("Narrator"))
                            .then(|| speaker.to_owned()),
                        voice: (!voice.is_empty()).then(|| voice.to_owned()),
                        stable_id: (!stable_id.is_empty()).then(|| stable_id.to_owned()),
                    };
                    if metadata == key.metadata {
                        return;
                    }
                    let Some(source) = cx.global::<EditorDocuments>().source(&root, &key.path)
                    else {
                        return;
                    };
                    if metadata.stable_id != key.metadata.stable_id
                        && metadata.stable_id.as_ref().is_some_and(|id| {
                            cx.global::<EditorDocuments>()
                                .explicit_source_ids(&root)
                                .contains(id)
                        })
                    {
                        cx.global_mut::<EditorDocuments>()
                            .set_notice(&root, "Stable ID already exists");
                        cx.refresh_windows();
                        return;
                    }
                    match EiyashouProjection::parse(&source).replace_text_block_metadata(
                        &source,
                        key.block_start,
                        &metadata,
                    ) {
                        Ok(edited) => {
                            let result = cx.update_window(window_handle, |_, window, cx| {
                                apply_workspace_edit(&root, &key.path, edited, window, cx);
                            });
                            if result.is_ok() {
                                cx.global_mut::<EditorDocuments>().set_block_selection(
                                    &root,
                                    key.path.clone(),
                                    vec![key.block_start],
                                );
                            }
                            cx.global_mut::<EditorDocuments>().set_notice(
                                &root,
                                if result.is_ok() {
                                    "Text properties updated".to_owned()
                                } else {
                                    "Text properties update failed".to_owned()
                                },
                            );
                        }
                        Err(error) => cx
                            .global_mut::<EditorDocuments>()
                            .set_notice(&root, format!("Text properties blocked: {error}")),
                    }
                    cx.refresh_windows();
                },
            ));
        }
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
                    let result = prepare_asset_edits(
                        &root,
                        &index,
                        asset,
                        values[0].trim(),
                        kind,
                        &tags,
                        |path| cx.global::<EditorDocuments>().source(&root, path),
                    );
                    match result {
                        Ok(edits) => {
                            let applied = cx.update_window(window_handle, |_, window, cx| {
                                apply_prepared_edits(&root, &edits, window, cx)
                            });
                            match applied {
                                Ok(Ok(())) => {
                                    cx.global_mut::<EditorDocuments>().set_asset_selection(
                                        &root,
                                        vec![AssetKey {
                                            kind,
                                            id: values[0].trim().to_owned(),
                                        }],
                                    );
                                    cx.global_mut::<EditorDocuments>()
                                        .set_notice(&root, "Asset updated");
                                }
                                _ => {
                                    panel.asset_inspector_key = None;
                                    cx.global_mut::<EditorDocuments>()
                                        .set_notice(&root, "Asset update failed");
                                }
                            }
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
