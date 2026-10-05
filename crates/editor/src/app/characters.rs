//! Character manifest transactions and explicit source insertion. No second scene model.
use super::*;
use crate::authoring::CharacterEntry;
use gpui_kit::component::select::Select;

pub(super) fn render(
    root: &Path,
    panel: &mut WorkbenchPanel,
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let index = cx.global::<EditorDocuments>().authoring(root);
    if let Some((path, line, _)) = cx
        .global::<EditorDocuments>()
        .selection(root)
        .filter(|(path, _, _)| path.extension().is_some_and(|ext| ext == "shou"))
    {
        panel.character_script = Some((path.clone(), *line));
    }
    panel.sync_character_images(root, &index, window, cx);
    let inputs = panel.tool_inputs.clone();
    let selected = panel.character_id.clone();
    let mut content = div()
        .flex()
        .flex_col()
        .p_2()
        .gap_2()
        .child(section_label("CHARACTERS"))
        .when_some(index.characters_manifest.clone(), |this, path| {
            let root = root.to_owned();
            this.child(
                tool_action("Edit YAML")
                    .on_click(move |_, window, cx| navigate_source(&root, &path, 1, 1, window, cx)),
            )
        });
    for character in &index.characters {
        let id = character.id.clone();
        let references = index
            .dialogues
            .iter()
            .filter(|dialogue| dialogue.speaker == id)
            .count();
        content = content.child(
            div()
                .id(format!("character-{id}"))
                .p_2()
                .rounded(px(7.))
                .bg(rgb(if selected.as_ref() == Some(&id) {
                    PRIMARY_DIM
                } else {
                    PANEL
                }))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                .on_click(cx.listener(move |panel, _, window, cx| {
                    panel.select_character(Some(id.clone()), window, cx)
                }))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(INK))
                        .child(character.name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(format!("{} · {references} refs", character.id)),
                ),
        );
    }
    if inputs.len() == 6 {
        content = content
            .child(tool_action("New character").on_click(
                cx.listener(|panel, _, window, cx| panel.select_character(None, window, cx)),
            ))
            .child(tool_input(&inputs[0]))
            .child(tool_input(&inputs[1]))
            .child(tool_input(&inputs[2]))
            .child(tool_input(&inputs[5]))
            .child(
                tool_action(if selected.is_some() {
                    "Save character"
                } else {
                    "Add character"
                })
                .on_click(cx.listener(|panel, _, window, cx| panel.save_character(window, cx))),
            );
    }
    if let Some(character) = selected.as_ref().and_then(|id| {
        index
            .characters
            .iter()
            .find(|character| &character.id == id)
    }) {
        content = content
            .when_some(panel.character_script.as_ref(), |this, (path, line)| {
                this.child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                    "Insert after {}:{}",
                    path.display(),
                    line + 1
                )))
            })
            .child(tool_action("Delete character").on_click(
                cx.listener(|panel, _, window, cx| panel.confirm_delete_character(window, cx)),
            ))
            .child(section_label("EXPRESSIONS"))
            .when_some(panel.character_images.as_ref(), |this, images| {
                this.child(Select::new(images).small())
            })
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(tool_action("Add frame").on_click(cx.listener(
                        |panel, _, window, cx| panel.add_character_frame(false, window, cx),
                    )))
                    .child(tool_action("Use as avatar").on_click(cx.listener(
                        |panel, _, window, cx| panel.add_character_frame(true, window, cx),
                    ))),
            )
            .child(tool_input(&inputs[3]))
            .child(tool_input(&inputs[4]))
            .child(tool_action("Use selected images").on_click(
                cx.listener(|panel, _, window, cx| panel.use_character_assets(false, window, cx)),
            ))
            .child(tool_action("Save expression").on_click(
                cx.listener(|panel, _, window, cx| panel.save_expression(false, window, cx)),
            ))
            .child(tool_action("Add images as expressions").on_click(
                cx.listener(|panel, _, window, cx| panel.use_character_assets(true, window, cx)),
            ));
        for (name, frames) in &character.expressions {
            let expression = name.clone();
            let display = frames
                .iter()
                .map(|id| {
                    index
                        .assets
                        .iter()
                        .find(|asset| &asset.id == id)
                        .map(|asset| {
                            asset
                                .path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned()
                        })
                        .unwrap_or_else(|| format!("Missing: {id}"))
                })
                .collect::<Vec<_>>()
                .join(" → ");
            let mut row = div()
                .id(format!("expression-{name}"))
                .p_2()
                .rounded(px(7.))
                .bg(rgb(PANEL))
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().text_color(rgb(INK)).child(name.clone()))
                .child(div().text_xs().text_color(rgb(MUTED)).child(display))
                .child(tool_action("Edit expression").on_click(cx.listener(
                    move |panel, _, window, cx| panel.select_expression(&expression, window, cx),
                )));
            let mut actions = div().flex().flex_wrap().gap_1();
            for (label, mode) in [
                ("Show", "show"),
                ("Change", "change"),
                ("Blink", "blink"),
                ("Talk", "talk"),
            ] {
                if frames.len() < 2 && matches!(mode, "blink" | "talk") {
                    continue;
                }
                let name = name.clone();
                actions = actions.child(tool_action(label).on_click(cx.listener(
                    move |panel, _, window, cx| {
                        panel.insert_character_expression(&name, mode, window, cx)
                    },
                )));
            }
            row = row.child(actions);
            content = content.child(row);
        }
        content =
            content.child(tool_action("Delete selected expression").on_click(
                cx.listener(|panel, _, window, cx| panel.save_expression(true, window, cx)),
            ));
        if character.avatar.is_some() {
            content = content.child(tool_action("Insert avatar").on_click(cx.listener(
                |panel, _, window, cx| panel.insert_character_expression("", "avatar", window, cx),
            )));
        }
        content = content.child(section_label("REFERENCES"));
        for dialogue in index
            .dialogues
            .iter()
            .filter(|dialogue| dialogue.speaker == character.id)
        {
            let root = root.to_owned();
            let path = dialogue.path.clone();
            let (line, column) = (dialogue.line, dialogue.column);
            content = content.child(
                div()
                    .id(format!("character-ref-{}-{line}", path.display()))
                    .p_1()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .cursor_pointer()
                    .hover(|style| style.text_color(rgb(PRIMARY)))
                    .on_click(move |_, window, cx| {
                        navigate_source(&root, &path, line, column, window, cx)
                    })
                    .child(format!(
                        "{}:{line} · {}",
                        dialogue.path.display(),
                        dialogue.text
                    )),
            );
        }
    }
    vertical_overflow_view("character-scroll", &panel.view_scroll, content)
}

impl WorkbenchPanel {
    fn character_root(&self) -> Option<PathBuf> {
        match &self.content {
            PanelContent::Characters { root } => Some(root.clone()),
            _ => None,
        }
    }
    fn selected_character(&self, cx: &App) -> Option<CharacterEntry> {
        let root = self.character_root()?;
        cx.global::<EditorDocuments>()
            .authoring_ref(&root)?
            .characters
            .iter()
            .find(|character| Some(&character.id) == self.character_id.as_ref())
            .cloned()
    }
    fn select_character(
        &mut self,
        id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.character_id = id;
        self.character_expression = None;
        let character = self.selected_character(cx).unwrap_or_default();
        for (input, value) in self.tool_inputs.iter().zip([
            character.id,
            character.name,
            character.color.unwrap_or_default(),
            String::new(),
            String::new(),
            character.avatar.unwrap_or_default(),
        ]) {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        cx.notify();
    }
    fn sync_character_images(
        &mut self,
        root: &Path,
        index: &AuthoringIndex,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let options = index
            .assets
            .iter()
            .filter(|asset| asset.kind == AssetKind::Figure && asset.exists)
            .map(|asset| SourceOption {
                value: asset.id.clone(),
                title: asset
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
                    .into(),
                asset: Some((root.to_owned(), asset.kind, asset.path.clone())),
            })
            .collect::<Vec<_>>();
        if self.character_images.is_some() && options == self.character_image_options {
            return;
        }
        self.character_image_options = options.clone();
        self.character_images =
            Some(cx.new(|cx| SelectState::new(options, None, window, cx).searchable(true)));
    }
    fn add_character_frame(&mut self, avatar: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self
            .character_images
            .as_ref()
            .and_then(|select| select.read(cx).selected_value())
            .cloned()
        else {
            return;
        };
        let input = self.tool_inputs[if avatar { 5 } else { 4 }].clone();
        let previous = input.read(cx).value().to_string();
        input.update(cx, |input, cx| {
            input.set_value(
                if avatar || previous.trim().is_empty() {
                    id
                } else {
                    format!("{previous}, {id}")
                },
                window,
                cx,
            )
        });
    }
    fn write_character(
        &mut self,
        edit: impl FnOnce(&str) -> Result<String, crate::authoring::AuthoringEditError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(root) = self.character_root() else {
            return false;
        };
        let Some(path) = cx
            .global::<EditorDocuments>()
            .authoring(&root)
            .characters_manifest
            .clone()
        else {
            return false;
        };
        let result = cx
            .global_mut::<EditorDocuments>()
            .open(&root, &path)
            .map_err(|error| error.to_string())
            .and_then(|document| {
                edit(document.borrow().contents()).map_err(|error| error.to_string())
            });
        match result {
            Ok(source) => apply_prepared_edits(&root, &[(path, source)], window, cx).is_ok(),
            Err(error) => {
                window.push_notification(Notification::error(error), cx);
                false
            }
        }
    }
    fn save_character(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let values = self
            .tool_inputs
            .iter()
            .map(|input| input.read(cx).value().trim().to_owned())
            .collect::<Vec<_>>();
        let Some(root) = self.character_root() else {
            return;
        };
        let mut character = self.selected_character(cx).unwrap_or_default();
        if self.character_id.is_some() && character.id != values[0] {
            window.push_notification(
                Notification::error(
                    "Character ID is stable; edit references in Text view before renaming",
                ),
                cx,
            );
            return;
        }
        character.id = values[0].clone();
        character.name = values[1].clone();
        character.color = (!values[2].is_empty()).then(|| values[2].clone());
        character.avatar = (!values[5].is_empty()).then(|| values[5].clone());
        if character.avatar.as_ref().is_some_and(|id| {
            !cx.global::<EditorDocuments>()
                .authoring(&root)
                .assets
                .iter()
                .any(|asset| &asset.id == id && asset.kind == AssetKind::Figure && asset.exists)
        }) {
            window.push_notification(
                Notification::error("Avatar must reference a registered figure image"),
                cx,
            );
            return;
        }
        let new = self.character_id.is_none();
        if self.write_character(
            |source| {
                let source = if new {
                    append_character(
                        source,
                        &character.id,
                        &character.name,
                        character.color.as_deref(),
                    )?
                } else {
                    source.to_owned()
                };
                edit_character(&source, &character)
            },
            window,
            cx,
        ) {
            self.character_id = Some(character.id);
        }
        cx.refresh_windows();
    }
    fn select_expression(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(character) = self.selected_character(cx) else {
            return;
        };
        self.character_expression = Some(name.to_owned());
        let frames = character.expressions.get(name).cloned().unwrap_or_default();
        for (input, value) in self.tool_inputs[3..5]
            .iter()
            .zip([name.to_owned(), frames.join(", ")])
        {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
    }
    fn save_expression(&mut self, delete: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut character) = self.selected_character(cx) else {
            return;
        };
        let name = self.tool_inputs[3].read(cx).value().trim().to_owned();
        if !valid_identifier(&name) {
            window.push_notification(
                Notification::error("Expression name must be an identifier"),
                cx,
            );
            return;
        }
        if delete {
            let selected = self.character_expression.as_ref().unwrap_or(&name);
            character.expressions.remove(selected);
        } else {
            let frames = self.tool_inputs[4]
                .read(cx)
                .value()
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let index = cx
                .global::<EditorDocuments>()
                .authoring(&self.character_root().unwrap());
            if frames.is_empty()
                || frames.iter().any(|id| {
                    !index.assets.iter().any(|asset| {
                        &asset.id == id && asset.kind == AssetKind::Figure && asset.exists
                    })
                })
            {
                window.push_notification(
                    Notification::error("Frames must reference registered figure images"),
                    cx,
                );
                return;
            }
            if self
                .character_expression
                .as_ref()
                .is_some_and(|selected| selected != &name)
                && character.expressions.contains_key(&name)
            {
                window
                    .push_notification(Notification::error("Expression name is already used"), cx);
                return;
            }
            if let Some(selected) = &self.character_expression {
                character.expressions.remove(selected);
            }
            character.expressions.insert(name.clone(), frames);
        }
        if self.write_character(|source| edit_character(source, &character), window, cx) {
            self.character_expression = (!delete).then_some(name);
        }
        cx.refresh_windows();
    }
    fn use_character_assets(&mut self, batch: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.character_root() else {
            return;
        };
        let selected = cx.global::<EditorDocuments>().asset_selection(&root);
        let index = cx.global::<EditorDocuments>().authoring(&root);
        let images = index
            .assets
            .iter()
            .filter(|asset| {
                asset.kind == AssetKind::Figure && asset.exists && selected.contains(&asset.key())
            })
            .collect::<Vec<_>>();
        if images.is_empty() {
            window.push_notification(
                Notification::error("Select figure images in Assets first"),
                cx,
            );
            return;
        }
        if !batch {
            self.tool_inputs[4].update(cx, |input, cx| {
                input.set_value(
                    images
                        .iter()
                        .map(|asset| asset.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    window,
                    cx,
                )
            });
            return;
        }
        let Some(mut character) = self.selected_character(cx) else {
            return;
        };
        for image in images {
            let stem = image.path.file_stem().unwrap_or_default().to_string_lossy();
            let base = if valid_identifier(&stem) {
                stem.as_ref()
            } else {
                &image.id
            };
            let mut name = base.to_owned();
            let mut number = 2;
            while character.expressions.contains_key(&name) {
                name = format!("{base}_{number}");
                number += 1;
            }
            character.expressions.insert(name, vec![image.id.clone()]);
        }
        self.write_character(|source| edit_character(source, &character), window, cx);
        cx.refresh_windows();
    }
    fn confirm_delete_character(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(character) = self.selected_character(cx) else {
            return;
        };
        let Some(root) = self.character_root() else {
            return;
        };
        let count = cx
            .global::<EditorDocuments>()
            .authoring(&root)
            .dialogues
            .iter()
            .filter(|dialogue| dialogue.speaker == character.id)
            .count();
        let expected_source = cx
            .global::<EditorDocuments>()
            .authoring(&root)
            .characters_manifest
            .as_ref()
            .and_then(|path| cx.global::<EditorDocuments>().source(&root, path));
        let receiver = window.prompt(
            PromptLevel::Warning,
            "Delete character?",
            Some(&format!(
                "{count} dialogue references will become unresolved. Undo remains available."
            )),
            &[
                PromptButton::Other("Delete".into()),
                PromptButton::Cancel("Cancel".into()),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if receiver.await.ok() != Some(0) {
                return;
            }
            let _ = this.update_in(cx, |panel, window, cx| {
                let current_source = cx
                    .global::<EditorDocuments>()
                    .authoring(&root)
                    .characters_manifest
                    .as_ref()
                    .and_then(|path| cx.global::<EditorDocuments>().source(&root, path));
                if panel.character_root().as_ref() != Some(&root)
                    || current_source != expected_source
                {
                    window.push_notification(
                        Notification::error("Character manifest changed; retry deletion"),
                        cx,
                    );
                    return;
                }
                if panel.write_character(
                    |source| delete_character(source, &character.id),
                    window,
                    cx,
                ) {
                    panel.select_character(None, window, cx);
                }
                cx.refresh_windows();
            });
        })
        .detach();
    }
    fn insert_character_expression(
        &mut self,
        name: &str,
        mode: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(character) = self.selected_character(cx) else {
            return;
        };
        let Some(root) = self.character_root() else {
            return;
        };
        let Some((path, line)) = self.character_script.clone() else {
            window.push_notification(
                Notification::error("Select a script block or Text position first"),
                cx,
            );
            return;
        };
        if path.extension().is_none_or(|ext| ext != "shou") {
            window.push_notification(
                Notification::error("Select a script block or Text position first"),
                cx,
            );
            return;
        }
        let Some(source) = cx.global::<EditorDocuments>().source(&root, &path) else {
            return;
        };
        let index = cx.global::<EditorDocuments>().authoring(&root);
        let result = character_statement(&character, name, mode, &index).and_then(|statement| {
            // A position inside a multiline command belongs to the entire block.
            let projection = EiyashouProjection::parse(&source);
            let lines = keine_loader::SourceLineIndex::new(&source);
            let end = projection
                .scenes
                .iter()
                .flat_map(|scene| &scene.blocks)
                .filter(|block| {
                    block.line <= line + 1
                        && lines
                            .span(&source, block.source_range.end.saturating_sub(1))
                            .line
                            > line
                })
                .min_by_key(|block| block.source_range.len())
                .map(|block| {
                    lines
                        .span(&source, block.source_range.end.saturating_sub(1))
                        .line
                        - 1
                })
                .unwrap_or(line);
            insert_source_statement(&source, end, &statement).map_err(|error| error.to_string())
        });
        match result {
            Ok(edited) => {
                apply_workspace_edit(&root, &path, edited, window, cx);
                cx.global_mut::<EditorDocuments>().set_notice(
                    &root,
                    "Inserted character command; save the script to keep it",
                );
            }
            Err(error) => window.push_notification(Notification::error(error), cx),
        }
        cx.refresh_windows();
    }
}

fn character_statement(
    character: &CharacterEntry,
    name: &str,
    mode: &str,
    index: &AuthoringIndex,
) -> Result<String, String> {
    if !valid_identifier(&character.id) {
        return Err("Character ID is invalid".into());
    }
    let frames = if mode == "avatar" {
        character.avatar.iter().cloned().collect()
    } else {
        character
            .expressions
            .get(name)
            .cloned()
            .ok_or("Select an expression")?
    };
    if frames.is_empty()
        || frames.iter().any(|id| {
            !index
                .assets
                .iter()
                .any(|asset| &asset.id == id && asset.kind == AssetKind::Figure && asset.exists)
        })
    {
        return Err("Expression references a missing figure image".into());
    }
    if mode == "avatar" {
        return Ok(format!("avatar.show({})", frames[0]));
    }
    let header = if mode == "show" {
        "sprite"
    } else {
        "sprite.update"
    };
    let mut result = format!("{header}({}, {})", character.id, frames[0]);
    if frames.len() > 1 {
        let options = match mode {
            "blink" => "mode: blink, interval: 3s".to_owned(),
            "talk" => format!(
                "mode: talk, speaker: \"{}\"",
                escape_eiyashou_string(&character.id)
            ),
            _ => "loop: true".to_owned(),
        };
        result.push_str(&format!(
            ",\nsprite.sequence({}, fps: 10, {options}) {{\n  {}\n}}",
            character.id,
            frames
                .iter()
                .map(|id| format!("frame({id})"))
                .collect::<Vec<_>>()
                .join(",\n  ")
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::AssetEntry;
    fn fixture() -> (CharacterEntry, AuthoringIndex) {
        let character = CharacterEntry {
            id: "hero".into(),
            name: "少女".into(),
            avatar: Some("rest".into()),
            expressions: BTreeMap::from([
                ("smile".into(), vec!["rest".into()]),
                ("blink".into(), vec!["rest".into(), "closed".into()]),
            ]),
            ..Default::default()
        };
        let index = AuthoringIndex {
            assets: ["rest", "closed"]
                .into_iter()
                .map(|id| AssetEntry {
                    kind: AssetKind::Figure,
                    id: id.into(),
                    path: format!("assets/{id}.webp").into(),
                    tags: Vec::new(),
                    reference_count: 0,
                    exists: true,
                })
                .collect(),
            ..Default::default()
        };
        (character, index)
    }
    #[test]
    fn character_presets_emit_explicit_native_commands_and_reject_missing_assets() {
        let (character, index) = fixture();
        for (name, mode) in [
            ("smile", "show"),
            ("smile", "change"),
            ("blink", "show"),
            ("blink", "blink"),
            ("blink", "talk"),
            ("", "avatar"),
        ] {
            let statement = character_statement(&character, name, mode, &index).unwrap();
            let source = insert_source_statement(
                "scene start {\n  \"before\",\n  \"after\"\n}\n",
                1,
                &statement,
            )
            .unwrap();
            let parsed = keine_loader::parse_native_document(&source);
            assert!(
                parsed.diagnostics.is_empty(),
                "{mode}: {:?}\n{source}",
                parsed.diagnostics
            );
            assert!(source.contains("\"before\",") && source.contains("\"after\""));
        }
        assert!(
            character_statement(&character, "smile", "show", &AuthoringIndex::default()).is_err()
        );
        assert_eq!(
            character_statement(&character, "smile", "change", &index).unwrap(),
            "sprite.update(hero, rest)"
        );
    }
}
