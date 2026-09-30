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
            "camera.move" | "camera.effect" | "camera.effect.v2" | "event.camera.patch"
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
                                        this.source_inspector_effect = Some(prefix);
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
                                                this.source_inspector_effect = Some(prefix);
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
    let selected = EiyashouProjection::parse(source)
        .scenes
        .into_iter()
        .flat_map(|scene| {
            let name = scene.name;
            scene
                .blocks
                .into_iter()
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
        properties.push((
            "Speaker",
            common_value(selected.iter().map(|(_, block)| match &block.kind {
                BlockKind::Narration => "Narrator".to_owned(),
                BlockKind::Dialogue { speaker } => speaker.clone(),
                _ => unreachable!("guarded above"),
            })),
        ));
        properties.push((
            "Voice",
            common_value(
                selected.iter().map(|(_, block)| {
                    text_voice(&block.summary).unwrap_or_else(|| "None".to_owned())
                }),
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
