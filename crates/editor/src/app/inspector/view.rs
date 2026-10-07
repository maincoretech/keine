//! Source Inspector assembly and selection summaries.
use super::controls::{render_wait_control, replay_button, retraction_property};
use super::*;
use crate::authoring::fields::character_track;

// Studio's effect workspace uses an effect list and a single detail pane. These
// groups contain only the effects already exposed by the native action schema.
pub(in crate::app) struct SourceInspectorView<'a> {
    pub root: &'a Path,
    pub key: &'a SourceInspectorKey,
    pub inputs: &'a [Entity<InputState>],
    pub texts: &'a [Entity<TextareaState>],
    pub sliders: &'a HashMap<String, Entity<SliderState>>,
    pub selects: &'a HashMap<String, Entity<SelectState<Vec<SourceOption>>>>,
    pub effect: Option<&'static str>,
    pub position_bounds: &'a Rc<RefCell<Bounds<Pixels>>>,
    pub position_draft: Option<(usize, f32, f32)>,
}

impl SourceInspectorView<'_> {
    pub(in crate::app) fn property(
        &self,
        position: usize,
        cx: &mut Context<WorkbenchPanel>,
    ) -> AnyElement {
        let field = &self.key.fields[position];
        if let Some(input) = self.texts.get(position) {
            return retraction_property(self.key, field, input);
        }
        let editor = source_property_editor(
            self.root,
            self.key,
            position,
            &self.inputs[position],
            self.sliders.get(&field.key),
            self.selects.get(&field.key),
            cx,
        );
        if !camera_tween_field(self.key, field) {
            return editor;
        }
        let enabled = camera_field_tweens(self.key, &field.key);
        let root = self.root.to_owned();
        let key = self.key.clone();
        let updated = toggle_camera_tween(self.key, &field.key);
        div()
            .relative()
            .w_full()
            .child(editor)
            .child(
                div()
                    .id(format!("camera-tween-{}-{}", key.block_start, field.key))
                    .absolute()
                    .top(px(-3.))
                    .right_0()
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(rgb(if enabled { PRIMARY } else { MUTED }))
                    .hover(|this| this.bg(rgb(SURFACE)))
                    .tooltip(icon_hint(if enabled {
                        "Tween over duration"
                    } else {
                        "Apply immediately"
                    }))
                    .child(if enabled { "◆" } else { "◇" })
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.commit_source_fields(
                            &root,
                            &key,
                            &[("tween".into(), Some(updated.clone()))],
                            window,
                            cx,
                        );
                    })),
            )
            .into_any_element()
    }

    pub fn render(&self, cx: &mut Context<WorkbenchPanel>) -> AnyElement {
        if matches!(self.key.command.as_str(), "wait" | "wait.advance")
            && let Some(input) = self.inputs.first()
        {
            return div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .child(replay_button(self.root, self.key))
                .child(render_wait_control(self.root, self.key, input, false, cx))
                .into_any_element();
        }
        let effects = matches!(
            self.key.command.as_str(),
            "camera.move" | "camera.effect" | "event.camera.patch"
        );
        let mut content = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .when_some(self.timeline(cx), |this, timeline| this.child(timeline));
        if self.key.command == "text.retract" {
            return content
                .child(replay_button(self.root, self.key))
                .child(section_label("Text"))
                .children((0..self.key.fields.len()).map(|position| self.property(position, cx)))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child("Deletes the tail, then waits for a new advance input."),
                )
                .into_any_element();
        }
        if self.key.command == "stage.animate" {
            let order = [
                "duration",
                "0",
                "playback_rate",
                "repeat",
                "infinite",
                "blocking",
                "easing",
            ];
            content = content.child(replay_button(self.root, self.key));
            for name in order {
                if let Some(position) = self.key.fields.iter().position(|field| field.key == name) {
                    content = content.child(self.property(position, cx));
                }
            }
            return content.into_any_element();
        }
        if matches!(self.key.command.as_str(), "key" | "track") {
            return content
                .children(
                    self.key
                        .fields
                        .iter()
                        .enumerate()
                        .filter(|(_, field)| {
                            self.key.command != "track"
                                || field.key != "image"
                                || field.insertion.is_none()
                                || character_track(self.key)
                        })
                        .map(|(position, _)| self.property(position, cx)),
                )
                .into_any_element();
        }
        let camera = self.key.command.starts_with("camera.");
        let groups = if camera {
            [
                "Properties",
                "Position",
                "Timing",
                "Transform",
                "Speaking",
                "Other characters",
                "Narration",
                "Layout",
                "Playback",
            ]
        } else {
            [
                "Properties",
                "Position",
                "Speaking",
                "Other characters",
                "Narration",
                "Transform",
                "Layout",
                "Playback",
                "Timing",
            ]
        };
        for group in groups {
            let positions = self
                .key
                .fields
                .iter()
                .enumerate()
                .filter(|(_, field)| {
                    field.key != "tween"
                        && source_property_group(field) == group
                        && (!effects || source_effect_group(&field.key).is_none())
                })
                .map(|(position, _)| position)
                .collect::<Vec<_>>();
            if positions.is_empty() {
                continue;
            }
            content = content.child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(section_label(group))
                    .when(group == "Transform", |this| {
                        this.when_some(self.position_pad(cx), |this, pad| this.child(pad))
                    })
                    .children(
                        positions
                            .into_iter()
                            .map(|position| self.property(position, cx)),
                    ),
            );
        }
        if effects {
            let groups = EFFECT_GROUPS
                .iter()
                .copied()
                .filter(|(prefix, _)| {
                    self.key.fields.iter().any(|field| {
                        source_effect_group(&field.key).is_some_and(|(group, _)| group == *prefix)
                    })
                })
                .collect::<Vec<_>>();
            let active = self
                .effect
                .or_else(|| {
                    self.key
                        .fields
                        .iter()
                        .filter(|field| field.insertion.is_none())
                        .find_map(|field| source_effect_group(&field.key).map(|(prefix, _)| prefix))
                })
                .or_else(|| groups.first().map(|(prefix, _)| *prefix));
            content = content.child(section_label("Effects")).child(
                div()
                    .w_full()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .w(px(116.))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(groups.into_iter().map(|(prefix, title)| {
                                let selected = active == Some(prefix);
                                let written = self.key.fields.iter().any(|field| {
                                    field.insertion.is_none()
                                        && source_effect_group(&field.key)
                                            .is_some_and(|(group, _)| group == prefix)
                                });
                                let root = self.root.to_owned();
                                let key = self.key.clone();
                                let updates = key
                                    .fields
                                    .iter()
                                    .filter(|field| {
                                        source_effect_group(&field.key)
                                            .is_some_and(|(group, _)| group == prefix)
                                    })
                                    .filter_map(|field| {
                                        let value = if written {
                                            None
                                        } else if let Some(number) = source_number(&key, field) {
                                            Some(number.source(number.default))
                                        } else if matches!(
                                            field.key.as_str(),
                                            "godray_parallel" | "speed_lines_radial"
                                        ) {
                                            Some("true".into())
                                        } else if field.key == "speed_lines_region_ellipse" {
                                            Some("false".into())
                                        } else {
                                            return None;
                                        };
                                        Some((field.key.clone(), value))
                                    })
                                    .collect::<Vec<_>>();
                                div()
                                    .id(format!("inspector-effect-{prefix}"))
                                    .w_full()
                                    .min_h(px(28.))
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.))
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_size(px(11.))
                                    .text_color(rgb(if selected { PRIMARY } else { INK }))
                                    .when(selected, |this| this.bg(rgb(PRIMARY_DIM)))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.inspector.source_inspector_effect = Some(prefix);
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .id(format!("effect-include-{prefix}"))
                                            .size(px(14.))
                                            .flex_shrink_0()
                                            .rounded(px(2.))
                                            .border_1()
                                            .border_color(rgb(if written {
                                                PRIMARY
                                            } else {
                                                MUTED
                                            }))
                                            .bg(rgb(if written { PRIMARY_DIM } else { CANVAS }))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(written, |this| {
                                                this.child(
                                                    Icon::new(AssetIconName::Check)
                                                        .xsmall()
                                                        .text_color(rgb(PRIMARY)),
                                                )
                                            })
                                            .tooltip(icon_hint(if written {
                                                "Remove from command"
                                            } else {
                                                "Include in command"
                                            }))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                cx.stop_propagation();
                                                this.inspector.source_inspector_effect =
                                                    Some(prefix);
                                                this.commit_source_fields(
                                                    &root, &key, &updates, window, cx,
                                                );
                                            })),
                                    )
                                    .child(title)
                            })),
                    )
                    .child(
                        div().flex_1().min_w_0().flex().flex_col().gap_2().children(
                            self.key
                                .fields
                                .iter()
                                .enumerate()
                                .filter(|(_, field)| {
                                    source_effect_group(&field.key)
                                        .is_some_and(|(prefix, _)| Some(prefix) == active)
                                })
                                .map(|(position, _)| self.property(position, cx)),
                        ),
                    ),
            );
        }
        content.into_any_element()
    }
}

pub(in crate::app) fn multi_block_summary(source: &str, starts: &[usize]) -> Option<AnyElement> {
    let projection = EiyashouProjection::parse(source);
    let selected = projection
        .scenes
        .iter()
        .flat_map(|scene| {
            let name = &scene.name;
            scene
                .blocks
                .iter()
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
        let metadata = selected
            .iter()
            .map(|(_, block)| projection.text_block_metadata_for_block(source, block))
            .collect::<Option<Vec<_>>>()?;
        properties.push((
            "Speaker",
            common_value(metadata.iter().map(|metadata| {
                metadata
                    .speaker
                    .clone()
                    .unwrap_or_else(|| "Narrator".into())
            })),
        ));
        properties.push((
            "Voice",
            common_value(
                metadata
                    .iter()
                    .map(|metadata| metadata.voice.clone().unwrap_or_else(|| "None".into())),
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

pub(in crate::app) fn selection_summary(
    root: &Path,
    index: &AuthoringIndex,
    cx: &App,
) -> AnyElement {
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

/// Common editable fields only; identities, references and readonly nodes are not bulk overwritten.
pub(in crate::app) fn batch_block_fields(source: &str, starts: &[usize]) -> Vec<(String, String)> {
    let projection = EiyashouProjection::parse(source);
    let blocks = projection
        .scenes
        .iter()
        .flat_map(|scene| &scene.blocks)
        .filter(|block| starts.contains(&block.source_range.start))
        .collect::<Vec<_>>();
    if blocks.len() != starts.len()
        || blocks.len() < 2
        || blocks.iter().any(|block| block.read_only || block.disabled)
    {
        return Vec::new();
    }
    let all_text = blocks.iter().all(|block| {
        matches!(
            block.kind,
            BlockKind::Narration | BlockKind::Dialogue { .. }
        )
    });
    let mut result = Vec::new();
    if all_text {
        for name in ["speaker", "voice"] {
            result.push((
                name.into(),
                common_value(
                    blocks
                        .iter()
                        .filter_map(|block| {
                            projection.text_block_metadata(source, block.source_range.start)
                        })
                        .map(|metadata| {
                            if name == "speaker" {
                                metadata.speaker.unwrap_or_else(|| "Narrator".into())
                            } else {
                                metadata.voice.unwrap_or_default()
                            }
                        }),
                ),
            ));
        }
    }
    let fields = blocks
        .iter()
        .map(|block| {
            projection
                .source_fields(source, block.source_range.start)
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let command = if all_text {
        "dialogue"
    } else {
        blocks[0]
            .summary
            .split('(')
            .next()
            .unwrap_or_default()
            .trim()
    };
    if !all_text
        && blocks.iter().any(|block| {
            block.kind != BlockKind::Command
                || block.summary.split('(').next().map(str::trim) != Some(command)
        })
    {
        return Vec::new();
    }
    for field in &fields[0] {
        let context = SourceInspectorKey {
            path: PathBuf::new(),
            block_start: 0,
            kind: blocks[0].kind.clone(),
            command: command.into(),
            fields: fields[0].clone(),
        };
        if field.key.parse::<usize>().is_ok()
            || field.quoted
            || source_asset_kind(&context, field).is_some()
        {
            continue;
        }
        if fields.iter().all(|fields| {
            fields
                .iter()
                .any(|candidate| candidate.key == field.key && !candidate.quoted)
        }) {
            result.push((
                field.key.clone(),
                common_value(fields.iter().map(|fields| {
                    fields
                        .iter()
                        .find(|candidate| candidate.key == field.key)
                        .unwrap()
                        .value
                        .clone()
                })),
            ));
        }
    }
    result
}

pub(in crate::app) fn batch_block_edit(
    source: &str,
    starts: &[usize],
    name: &str,
    value: &str,
) -> Result<(String, Vec<usize>), String> {
    if !batch_block_fields(source, starts)
        .iter()
        .any(|(field, _)| field == name)
    {
        return Err("Field is not editable for this selection".into());
    }
    let original = EiyashouProjection::parse(source);
    let selected_indexes = original
        .scenes
        .iter()
        .flat_map(|scene| &scene.blocks)
        .enumerate()
        .filter_map(|(i, block)| starts.contains(&block.source_range.start).then_some(i))
        .collect::<Vec<_>>();
    let mut edited = source.to_owned();
    let mut ordered = starts.to_vec();
    ordered.sort_unstable_by(|a, b| b.cmp(a));
    for start in ordered {
        let projection = EiyashouProjection::parse(&edited);
        if matches!(name, "speaker" | "voice") {
            let mut metadata = projection
                .text_block_metadata(&edited, start)
                .ok_or("Text changed")?;
            let value = (!value.trim().is_empty() && !matches!(value.trim(), "none" | "Narrator"))
                .then(|| value.trim().to_owned());
            if name == "speaker" {
                metadata.speaker = value;
            } else {
                metadata.voice = value;
            }
            edited = projection
                .replace_text_block_metadata(&edited, start, &metadata)
                .map_err(|error| error.to_string())?;
        } else {
            let fields = projection
                .source_fields(&edited, start)
                .ok_or("Source changed")?;
            let field = fields
                .iter()
                .find(|field| field.key == name)
                .ok_or("Field changed")?;
            let replacement = field.insertion.as_ref().map_or_else(
                || value.to_owned(),
                |prefix| {
                    format!(
                        "{prefix}{value}{}",
                        field.insertion_suffix.as_deref().unwrap_or_default()
                    )
                },
            );
            edited.replace_range(field.range.clone(), &replacement);
        }
    }
    let parsed = EiyashouProjection::parse(&edited);
    if parsed.read_only.len() > original.read_only.len()
        || parsed.scenes.iter().flat_map(|scene| &scene.blocks).count()
            != original
                .scenes
                .iter()
                .flat_map(|scene| &scene.blocks)
                .count()
    {
        return Err("Invalid batch value; nothing changed".into());
    }
    let starts = parsed
        .scenes
        .iter()
        .flat_map(|scene| &scene.blocks)
        .enumerate()
        .filter_map(|(i, block)| {
            selected_indexes
                .contains(&i)
                .then_some(block.source_range.start)
        })
        .collect();
    Ok((edited, starts))
}

impl WorkbenchPanel {
    pub(in crate::app) fn render_inspector(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let PanelContent::Inspector { root, file_count } = &self.content else {
            return Empty.into_any_element();
        };

        let document_count = cx.global::<EditorDocuments>().open_document_count(root);
        let index = cx.global::<EditorDocuments>().authoring(root);
        let has_selection = cx.global::<EditorDocuments>().selection(root).is_some();
        let asset_selection = cx.global::<EditorDocuments>().asset_selection(root);
        let selected_assets = asset_selection
            .iter()
            .filter_map(|key| index.assets.iter().find(|asset| asset.key() == *key))
            .collect::<Vec<_>>();
        let inputs = self.inspector.inspector_inputs.clone();
        let text_selects = self.inspector.inspector_selects.clone();
        let source_key = self.inspector.source_inspector_key.clone();
        let source_inputs = self.inspector.source_inspector_inputs.clone();
        let source_texts = self.inspector.source_inspector_texts.clone();
        let source_sliders = self.inspector.source_inspector_sliders.clone();
        let source_selects = self.inspector.source_inspector_selects.clone();
        let source_effect = self.inspector.source_inspector_effect;
        let asset_inputs = self.inspector.asset_inspector_inputs.clone();
        let unmapped_preview =
            cx.global::<EditorDocuments>()
                .asset_preview(root)
                .filter(|preview| {
                    file_ops::mapped_path(&preview.path).is_some() && asset_selection.is_empty()
                });
        let content = div()
            .flex()
            .flex_col()
            .px(px(16.))
            .py(px(10.))
            .gap_2()
            .when(!selected_assets.is_empty(), |this| {
                this.child(section_label("ASSET"))
                    .when(selected_assets.len() == 1, |this| {
                        let asset = selected_assets[0];
                        let mut groups =
                            BTreeMap::<PathBuf, Vec<&crate::authoring::AssetReference>>::new();
                        for reference in index
                            .asset_references
                            .iter()
                            .filter(|reference| reference.key == asset.key())
                        {
                            let group = groups.entry(reference.path.clone()).or_default();
                            if !group.iter().any(|entry| entry.line == reference.line) {
                                group.push(reference);
                            }
                        }
                        let limit = (f32::from(window.viewport_size().height) / 96.)
                            .floor()
                            .clamp(3., 12.) as usize;
                        let more = groups.len().saturating_sub(limit);
                        let references = groups
                            .iter()
                            .take(limit)
                            .enumerate()
                            .map(|(row, (path, refs))| {
                                let root = root.clone();
                                let path = path.clone();
                                let line = refs[0].line;
                                let column = refs[0].column;
                                let lines = refs
                                    .iter()
                                    .take(4)
                                    .map(|entry| format!("L{}", entry.line))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let extra = refs.len().saturating_sub(4);
                                let label = format!(
                                    "{}  {lines}{}",
                                    path.display(),
                                    if extra > 0 {
                                        format!(" +{extra}")
                                    } else {
                                        String::new()
                                    }
                                );
                                div()
                                    .id(("asset-reference", row))
                                    .p_1()
                                    .rounded(px(6.))
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                                    .tooltip(icon_hint(
                                        refs.iter()
                                            .map(|entry| {
                                                format!("{}:L{}", path.display(), entry.line)
                                            })
                                            .collect::<Vec<_>>()
                                            .join(
                                                "
",
                                            ),
                                    ))
                                    .on_click(move |_, window, cx| {
                                        navigate_source(&root, &path, line, column, window, cx)
                                    })
                                    .child(label)
                            })
                            .collect::<Vec<_>>();
                        this.when(asset_inputs.len() == 3, |this| {
                            this.child(property_input("ID", &asset_inputs[0]))
                                .child(property_input("Type", &asset_inputs[1]))
                                .child(property_row("Path", asset.path.display().to_string()))
                                .child(property_input("Tags", &asset_inputs[2]))
                                .child(
                                    div()
                                        .flex()
                                        .gap_1()
                                        .items_center()
                                        .child(
                                            Switch::new("rename-asset-file")
                                                .checked(self.inspector.asset_rename_file)
                                                .on_click(cx.listener(
                                                    |this, checked: &bool, _, cx| {
                                                        this.inspector.asset_rename_file = *checked;
                                                        cx.notify();
                                                    },
                                                )),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(MUTED))
                                                .child("Rename file to match"),
                                        ),
                                )
                        })
                        .child(section_label("REFERENCES"))
                        .children(references)
                        .child(resource_trigger(
                            root,
                            ResourceTarget::Reference,
                            index
                                .asset_references
                                .iter()
                                .filter(|reference| reference.key == asset.key())
                                .map(|reference| SourceOption {
                                    value: serde_json::to_string(&(
                                        reference.path.clone(),
                                        reference.line,
                                        reference.column,
                                    ))
                                    .expect("reference tuple serializes"),
                                    title: format!(
                                        "{}:L{}",
                                        reference.path.display(),
                                        reference.line
                                    )
                                    .into(),
                                    asset: None,
                                })
                                .collect(),
                            "Browse references".into(),
                            false,
                            cx,
                        ))
                        .when(more > 0, |this| {
                            this.child(
                                div()
                                    .id("asset-more-references")
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .tooltip(icon_hint(
                                        groups
                                            .keys()
                                            .skip(limit)
                                            .map(|path| path.display().to_string())
                                            .collect::<Vec<_>>()
                                            .join(
                                                "
",
                                            ),
                                    ))
                                    .child(format!("+{more} files")),
                            )
                        })
                    })
                    .when(selected_assets.len() > 1, |this| {
                        this.child(property_row("Selected", selected_assets.len().to_string()))
                            .child(property_row(
                                "Type",
                                common_value(
                                    selected_assets
                                        .iter()
                                        .map(|asset| asset.kind.label().to_owned()),
                                ),
                            ))
                            .child(property_row(
                                "Folder",
                                common_value(selected_assets.iter().map(|asset| {
                                    asset
                                        .path
                                        .parent()
                                        .unwrap_or(Path::new(""))
                                        .display()
                                        .to_string()
                                })),
                            ))
                            .child(property_row(
                                "Tags",
                                common_value(
                                    selected_assets.iter().map(|asset| asset.tags.join(", ")),
                                ),
                            ))
                            .child(property_input(
                                "Batch tags",
                                &self.inspector.asset_batch_tags,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .gap_1()
                                    .child(asset_action_button("add-tags", "Add").on_click(
                                        cx.listener({
                                            let root = root.clone();
                                            move |this, _, window, cx| {
                                                this.asset_tags(&root, true, window, cx)
                                            }
                                        }),
                                    ))
                                    .child(asset_action_button("remove-tags", "Remove").on_click(
                                        cx.listener({
                                            let root = root.clone();
                                            move |this, _, window, cx| {
                                                this.asset_tags(&root, false, window, cx)
                                            }
                                        }),
                                    )),
                            )
                    })
                    .child(
                        asset_action_button("delete-assets", "Delete").on_click(cx.listener({
                            let root = root.clone();
                            move |this, _, window, cx| this.delete_assets(&root, None, window, cx)
                        })),
                    )
            })
            .when_some(unmapped_preview.clone(), |this, preview| {
                let path = preview.path;
                this.child(section_label("UNMAPPED"))
                    .child(property_row("Path", path.display().to_string()))
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(asset_action_button("remap-asset", "Remap").on_click(
                                cx.listener({
                                    let root = root.clone();
                                    let path = path.clone();
                                    move |this, _, window, cx| {
                                        this.remap_asset(&root, &path, window, cx)
                                    }
                                }),
                            ))
                            .child(asset_action_button("trash-unmapped", "Trash").on_click(
                                cx.listener({
                                    let root = root.clone();
                                    move |this, _, window, cx| {
                                        this.delete_assets(&root, Some(path.clone()), window, cx)
                                    }
                                }),
                            )),
                    )
            })
            .when(
                asset_selection.is_empty() && unmapped_preview.is_none() && has_selection,
                |this| {
                    this.when(source_key.is_none() && inputs.is_empty(), |this| {
                        let batch = cx
                            .global::<EditorDocuments>()
                            .block_selection(root)
                            .and_then(|(path, starts)| {
                                let source = cx.global::<EditorDocuments>().source(root, path)?;
                                Some(batch_block_fields(&source, starts))
                            })
                            .unwrap_or_default();
                        this.child(selection_summary(root, &index, cx)).when(
                            !batch.is_empty(),
                            |this| {
                                this.child(section_label("BATCH EDIT"))
                                    .child(div().flex().flex_wrap().gap_1().children(
                                        batch.into_iter().enumerate().map(|(i, (name, value))| {
                                            let active = self.inspector.batch_block_field.as_ref()
                                                == Some(&name);
                                            div()
                                                .id(("batch-field", i))
                                                .px_2()
                                                .py_1()
                                                .rounded(px(5.))
                                                .text_xs()
                                                .bg(rgb(if active { PRIMARY_DIM } else { SURFACE }))
                                                .text_color(rgb(if active {
                                                    PRIMARY
                                                } else {
                                                    MUTED
                                                }))
                                                .cursor_pointer()
                                                .child(format!(
                                                    "{name} · {}",
                                                    if value.is_empty() {
                                                        "Default"
                                                    } else {
                                                        &value
                                                    }
                                                ))
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.inspector.batch_block_field =
                                                            Some(name.clone());
                                                        this.inspector.batch_block_input.update(
                                                            cx,
                                                            |input, cx| {
                                                                input.set_value(
                                                                    if value == "Mixed" {
                                                                        ""
                                                                    } else {
                                                                        &value
                                                                    },
                                                                    window,
                                                                    cx,
                                                                )
                                                            },
                                                        );
                                                        cx.notify();
                                                    },
                                                ))
                                        }),
                                    ))
                                    .child(property_input(
                                        "Value",
                                        &self.inspector.batch_block_input,
                                    ))
                                    .child(
                                        asset_action_button("apply-batch-block", "Apply").on_click(
                                            cx.listener({
                                                let root = root.clone();
                                                move |this, _, window, cx| {
                                                    this.apply_batch_block_field(&root, window, cx)
                                                }
                                            }),
                                        ),
                                    )
                            },
                        )
                    })
                    .when(inputs.len() == 3 && text_selects.len() == 2, |this| {
                        this.child(section_label("TEXT"))
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(section_label("Speaker"))
                                    .child(resource_trigger(
                                        root,
                                        ResourceTarget::Speaker(
                                            self.inspector
                                                .inspector_key
                                                .clone()
                                                .expect("selected text Inspector"),
                                        ),
                                        resource::speaker_options(&index),
                                        inputs[0].read(cx).value().to_string(),
                                        false,
                                        cx,
                                    )),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(section_label("Voice"))
                                    .child(resource_trigger(
                                        root,
                                        ResourceTarget::Voice(
                                            self.inspector
                                                .inspector_key
                                                .clone()
                                                .expect("Text Inspector key"),
                                        ),
                                        voice_resource_options(root, &index),
                                        self.inspector
                                            .inspector_key
                                            .as_ref()
                                            .and_then(|key| key.metadata.voice.clone())
                                            .unwrap_or_default(),
                                        false,
                                        cx,
                                    )),
                            )
                            .child(property_input("Stable ID", &inputs[2]))
                            .child(render_text_ending(
                                root,
                                self.inspector
                                    .inspector_key
                                    .as_ref()
                                    .expect("Text Inspector key"),
                                &self.inspector.text_lifetime_inputs,
                                cx,
                            ))
                    })
                    .when_some(source_key, |this, key| {
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Icon::new(block_card_icon(&key.kind, &key.command))
                                        .small()
                                        .text_color(rgb(PRIMARY)),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(INK))
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .child(block_card_label(&key.kind, &key.command)),
                                ),
                        )
                        .child(
                            SourceInspectorView {
                                root,
                                key: &key,
                                inputs: &source_inputs,
                                texts: &source_texts,
                                sliders: &source_sliders,
                                selects: &source_selects,
                                effect: source_effect,
                                position_bounds: &self.inspector.source_position_bounds,
                                position_draft: self.inspector.source_position_draft,
                            }
                            .render(cx),
                        )
                    })
                },
            )
            .when(asset_selection.is_empty() && !has_selection, |this| {
                this.child(section_label("WORKSPACE"))
                    .child(property_row("Path", root.display().to_string()))
                    .child(property_row("Text files", file_count.to_string()))
                    .child(property_row("Open documents", document_count.to_string()))
            });
        div()
            .size_full()
            .relative()
            .child(vertical_overflow_view(
                "inspector-scroll",
                &self.view_scroll,
                content,
            ))
            .when_some(self.render_resource_picker(window, cx), |this, picker| {
                this.child(picker)
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    #[test]
    fn batch_edits_are_atomic_and_preserve_unselected_source() {
        let source = "scene start { bgm(first, volume: 0.5), wait(1s), bgm(second, volume: 0.2) }";
        let projection = EiyashouProjection::parse(source);
        let starts = [
            projection.scenes[0].blocks[0].source_range.start,
            projection.scenes[0].blocks[2].source_range.start,
        ];
        assert!(batch_block_edit(source, &starts, "volume", "5").is_err());
        let (edited, starts) = batch_block_edit(source, &starts, "volume", "0.8").unwrap();
        assert_eq!(starts.len(), 2);
        assert!(edited.contains("wait(1s)"));
        assert_eq!(edited.as_str().matches("volume: 0.8").count(), 2);
        assert!(edited.contains("bgm(first"));
        assert!(edited.contains("bgm(second"));
    }
}
