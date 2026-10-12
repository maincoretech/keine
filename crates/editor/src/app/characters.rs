//! Character manifest transactions and explicit source insertion. No second scene model.
use super::*;
use crate::authoring::CharacterEntry;
use gpui_kit::component::select::Select;
use keine_core::config::CharacterDisplayPreset;

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
        panel.characters.character_script = Some((path.clone(), *line));
    }
    panel.sync_character_images(root, &index, window, cx);
    let inputs = panel.characters.tool_inputs.clone();
    let selected = panel.characters.character_id.clone();
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
    if inputs.len() == 10 {
        content = content
            .child(tool_action("New character").on_click(
                cx.listener(|panel, _, window, cx| panel.select_character(None, window, cx)),
            ))
            .child(tool_input(&inputs[0]))
            .child(tool_input(&inputs[1]))
            .child(tool_input(&inputs[2]))
            .child(tool_input(&inputs[5]))
            .child(section_label("DISPLAY PRESET"));
        for (label, select) in ["Position", "Entry", "Exit"]
            .into_iter()
            .zip(&panel.characters.display_selects)
        {
            content = content
                .child(div().text_xs().text_color(rgb(MUTED)).child(label))
                .child(Select::new(select).small());
        }
        for (label, input) in [
            ("Horizontal offset (px)", &inputs[6]),
            ("Vertical offset (px)", &inputs[7]),
            ("Height (% of viewport; blank = natural)", &inputs[8]),
            ("Transition duration (ms)", &inputs[9]),
        ] {
            content = content
                .child(div().text_xs().text_color(rgb(MUTED)).child(label))
                .child(tool_input(input));
        }
        content = content
            .child(tool_action("Reset display preset").on_click(cx.listener(
                |panel, _, window, cx| {
                    panel.set_character_display(&CharacterDisplayPreset::default(), window, cx)
                },
            )))
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
            .child(tool_action("Apply saved preset and preview").on_click(
                cx.listener(|panel, _, window, cx| panel.apply_character_preset(window, cx)),
            ))
            .child(
                tool_action("Hide").on_click(cx.listener(|panel, _, window, cx| {
                    panel.insert_character_expression("", "hide", window, cx)
                })),
            )
            .when_some(
                panel.characters.character_script.as_ref(),
                |this, (path, line)| {
                    this.child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                        "Insert after {}:{}",
                        path.display(),
                        line + 1
                    )))
                },
            )
            .child(tool_action("Delete character").on_click(
                cx.listener(|panel, _, window, cx| panel.confirm_delete_character(window, cx)),
            ))
            .child(section_label("EXPRESSIONS"))
            .when_some(
                panel.characters.character_images.as_ref(),
                |this, images| this.child(Select::new(images).small()),
            )
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
    pub(super) fn init_character_display(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for options in [
            vec![("center", "Center"), ("left", "Left"), ("right", "Right")],
            vec![
                ("", "None"),
                ("enter", "Fade in"),
                ("enter_from_bottom", "From bottom"),
                ("enter_from_left", "From left"),
                ("enter_from_right", "From right"),
            ],
            vec![("", "None"), ("exit", "Fade out")],
        ] {
            let options = options
                .into_iter()
                .map(|(value, title)| SourceOption {
                    value: value.into(),
                    title: title.into(),
                    asset: None,
                })
                .collect::<Vec<_>>();
            let first = options[0].value.clone();
            self.characters.display_selects.push(cx.new(|cx| {
                let mut select = SelectState::new(options, None, window, cx);
                select.set_selected_value(&first, window, cx);
                select
            }));
        }
        self.set_character_display(&CharacterDisplayPreset::default(), window, cx);
    }

    fn set_character_display(
        &mut self,
        preset: &CharacterDisplayPreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (select, value) in self.characters.display_selects.iter().zip([
            preset.position.as_str(),
            preset.enter.as_deref().unwrap_or(""),
            preset.exit.as_deref().unwrap_or(""),
        ]) {
            select.update(cx, |select, cx| {
                select.set_selected_value(&value.to_owned(), window, cx)
            });
        }
        for (input, value) in self.characters.tool_inputs[6..].iter().zip([
            preset.x.to_string(),
            preset.y.to_string(),
            preset
                .height_percent
                .map(|height| height.to_string())
                .unwrap_or_default(),
            preset.duration_ms.to_string(),
        ]) {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        cx.notify();
    }

    fn character_display(&self, cx: &App) -> Result<CharacterDisplayPreset, String> {
        let values = self.characters.tool_inputs[6..]
            .iter()
            .map(|input| input.read(cx).value().trim().to_owned())
            .collect::<Vec<_>>();
        let selected = |index: usize| {
            self.characters.display_selects[index]
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default()
        };
        let preset = CharacterDisplayPreset {
            position: selected(0),
            x: values[0]
                .parse()
                .map_err(|_| "Horizontal offset must be a whole number")?,
            y: values[1]
                .parse()
                .map_err(|_| "Vertical offset must be a whole number")?,
            height_percent: if values[2].is_empty() {
                None
            } else {
                Some(
                    values[2]
                        .parse()
                        .map_err(|_| "Height must be a whole percentage")?,
                )
            },
            enter: (!selected(1).is_empty()).then(|| selected(1)),
            exit: (!selected(2).is_empty()).then(|| selected(2)),
            duration_ms: values[3]
                .parse()
                .map_err(|_| "Transition duration must be a non-negative whole number")?,
        };
        preset.validate().map_err(str::to_owned)?;
        Ok(preset)
    }

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
            .find(|character| Some(&character.id) == self.characters.character_id.as_ref())
            .cloned()
    }
    fn select_character(
        &mut self,
        id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.characters.character_id = id;
        self.characters.character_expression = None;
        let character = self.selected_character(cx).unwrap_or_default();
        let display = character.display.clone().unwrap_or_default();
        for (input, value) in self.characters.tool_inputs.iter().zip([
            character.id,
            character.name,
            character.color.unwrap_or_default(),
            String::new(),
            String::new(),
            character.avatar.unwrap_or_default(),
        ]) {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        self.set_character_display(&display, window, cx);
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
        if self.characters.character_images.is_some()
            && options == self.characters.character_image_options
        {
            return;
        }
        self.characters.character_image_options = options.clone();
        self.characters.character_images =
            Some(cx.new(|cx| SelectState::new(options, None, window, cx).searchable(true)));
    }
    fn add_character_frame(&mut self, avatar: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self
            .characters
            .character_images
            .as_ref()
            .and_then(|select| select.read(cx).selected_value())
            .cloned()
        else {
            return;
        };
        let input = self.characters.tool_inputs[if avatar { 5 } else { 4 }].clone();
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
            .characters
            .tool_inputs
            .iter()
            .map(|input| input.read(cx).value().trim().to_owned())
            .collect::<Vec<_>>();
        let Some(root) = self.character_root() else {
            return;
        };
        let mut character = self.selected_character(cx).unwrap_or_default();
        if self.characters.character_id.is_some() && character.id != values[0] {
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
        let display = match self.character_display(cx) {
            Ok(display) => display,
            Err(error) => {
                window.push_notification(Notification::error(error), cx);
                return;
            }
        };
        character.display = (character.display.is_some()
            || display != CharacterDisplayPreset::default())
        .then_some(display);
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
        let new = self.characters.character_id.is_none();
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
            self.characters.character_id = Some(character.id);
        }
        cx.refresh_windows();
    }
    fn select_expression(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(character) = self.selected_character(cx) else {
            return;
        };
        self.characters.character_expression = Some(name.to_owned());
        let frames = character.expressions.get(name).cloned().unwrap_or_default();
        for (input, value) in self.characters.tool_inputs[3..5]
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
        let name = self.characters.tool_inputs[3]
            .read(cx)
            .value()
            .trim()
            .to_owned();
        if !valid_identifier(&name) {
            window.push_notification(
                Notification::error("Expression name must be an identifier"),
                cx,
            );
            return;
        }
        if delete {
            let selected = self
                .characters
                .character_expression
                .as_ref()
                .unwrap_or(&name);
            character.expressions.remove(selected);
        } else {
            let frames = self.characters.tool_inputs[4]
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
                .characters
                .character_expression
                .as_ref()
                .is_some_and(|selected| selected != &name)
                && character.expressions.contains_key(&name)
            {
                window
                    .push_notification(Notification::error("Expression name is already used"), cx);
                return;
            }
            if let Some(selected) = &self.characters.character_expression {
                character.expressions.remove(selected);
            }
            character.expressions.insert(name.clone(), frames);
        }
        if self.write_character(|source| edit_character(source, &character), window, cx) {
            self.characters.character_expression = (!delete).then_some(name);
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
            self.characters.tool_inputs[4].update(cx, |input, cx| {
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
            if prompt_answer(receiver.await.ok(), 2) != Some(0) {
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
        let Some((path, line)) = self.characters.character_script.clone() else {
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

    fn apply_character_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.character_root() else {
            return;
        };
        let Some(mut character) = self.selected_character(cx) else {
            return;
        };
        // The index rebuild is asynchronous. Read the current manifest overlay
        // so Save character followed by Apply never uses the previous preset.
        let manifest = cx
            .global::<EditorDocuments>()
            .authoring(&root)
            .characters_manifest
            .as_ref()
            .and_then(|path| cx.global::<EditorDocuments>().source(&root, path))
            .and_then(|source| {
                keine_core::config::EiyashouCharacterManifest::from_yaml(&source).ok()
            });
        let Some(saved) = manifest
            .as_ref()
            .and_then(|manifest| manifest.characters.get(&character.id))
        else {
            window.push_notification(
                Notification::error("Character manifest changed or is invalid; refresh Characters"),
                cx,
            );
            return;
        };
        character.display = saved.display.clone();
        let selected =
            if let Some((path, starts)) = cx.global::<EditorDocuments>().block_selection(&root) {
                (starts.len() == 1).then(|| (path.clone(), starts[0]))
            } else {
                cx.global::<EditorDocuments>()
                    .selection(&root)
                    .and_then(|(path, line, column)| {
                        let source = cx.global::<EditorDocuments>().source(&root, path)?;
                        projected_block_at(&source, *line, *column)
                            .map(|(_, block)| (path.clone(), block.source_range.start))
                    })
            };
        let Some((path, start)) =
            selected.filter(|(path, _)| path.extension().is_some_and(|ext| ext == "shou"))
        else {
            window.push_notification(
                Notification::error("Select one native sprite block first"),
                cx,
            );
            return;
        };
        let Some(source) = cx.global::<EditorDocuments>().source(&root, &path) else {
            return;
        };
        let result =
            apply_character_display(&source, start, &character).and_then(|(edited, start)| {
                if edited != source {
                    apply_prepared_edits(&root, &[(path.clone(), edited)], window, cx)?;
                }
                Ok(start)
            });
        match result {
            Ok(start) => {
                cx.global_mut::<EditorDocuments>().set_block_selection(
                    &root,
                    path.clone(),
                    vec![start],
                );
                cx.global_mut::<EditorDocuments>()
                    .set_notice(&root, "Saved preset applied; save the script to keep it");
                // Document Change delivery updates the shared source first.
                // Reuse the normal Inspector Preview path and current overlays.
                cx.defer(move |cx| super::inspector::run_source_block(&root, &path, start, cx));
            }
            Err(error) => window.push_notification(Notification::error(error), cx),
        }
        cx.refresh_windows();
    }
}

/// One explicit source transaction, preserving the selected sprite's identity.
fn apply_character_display(
    source: &str,
    start: usize,
    character: &CharacterEntry,
) -> Result<(String, usize), String> {
    let display = character.display.clone().unwrap_or_default();
    display.validate().map_err(str::to_owned)?;
    if !valid_identifier(&character.id) {
        return Err("Character ID is invalid".into());
    }
    let projection = EiyashouProjection::parse(source);
    let scene = projection
        .scenes
        .iter()
        .find(|scene| {
            scene
                .blocks
                .iter()
                .any(|block| block.source_range.start == start)
        })
        .ok_or("Select one native sprite block first")?;
    let block = scene
        .blocks
        .iter()
        .find(|block| block.source_range.start == start)
        .unwrap();
    let command = block.summary.split('(').next().unwrap_or("").trim();
    if block.disabled || block.read_only || !matches!(command, "sprite" | "sprite.update") {
        return Err("Select an enabled sprite or sprite.update block".into());
    }
    let fields = projection
        .source_fields(source, start)
        .ok_or("Sprite fields are unavailable")?;
    if !fields
        .iter()
        .any(|field| field.key == "0" && field.value == character.id)
    {
        return Err("Selected sprite belongs to a different character".into());
    }
    let mut updates = vec![
        (
            "position".into(),
            Some(format!(
                "{}(x: {}, y: {})",
                display.position, display.x, display.y
            )),
        ),
        (
            "layout".into(),
            Some(display.height_percent.map_or_else(
                || "natural".into(),
                |height| format!("viewport(height: {})", f32::from(height) / 100.0),
            )),
        ),
    ];
    // A per-show transition overrides the saved rule, so clear it explicitly.
    if command == "sprite" {
        updates.push(("transition".into(), None));
    }
    let mut edited = projection
        .replace_block_fields(source, start, &updates)
        .map_err(|error| error.to_string())?;
    let container_start = scene
        .blocks
        .iter()
        .filter(|parent| {
            parent.depth < block.depth
                && parent.source_range.start < start
                && parent.source_range.end >= block.source_range.end
        })
        .max_by_key(|parent| (parent.depth, parent.source_range.start))
        .map_or(scene.source_range.start, |parent| parent.source_range.start);
    let previous = scene
        .blocks
        .iter()
        .filter(|candidate| {
            candidate.depth == block.depth
                && candidate.source_range.end <= start
                && candidate.source_range.start > container_start
        })
        .max_by_key(|candidate| candidate.source_range.start);
    let transition = previous.filter(|previous| {
        !previous.disabled
            && !previous.read_only
            && previous
                .summary
                .split('(')
                .next()
                .is_some_and(|name| name.trim() == "sprite.transition")
            && projection
                .source_fields(source, previous.source_range.start)
                .is_some_and(|fields| {
                    fields
                        .iter()
                        .any(|field| field.key == "0" && field.value == character.id)
                })
    });
    let checked = EiyashouProjection::parse(&edited);
    let before = edited.len();
    if let Some(transition) = transition {
        edited = checked
            .replace_block_fields(
                &edited,
                transition.source_range.start,
                &[
                    (
                        "enter".into(),
                        Some(display.enter.unwrap_or_else(|| "none".into())),
                    ),
                    (
                        "exit".into(),
                        Some(display.exit.unwrap_or_else(|| "none".into())),
                    ),
                    (
                        "duration".into(),
                        Some(format!("{}ms", display.duration_ms)),
                    ),
                ],
            )
            .map_err(|error| error.to_string())?;
    } else {
        let statement = format!(
            "sprite.transition({}, enter: {}, exit: {}, duration: {}ms)",
            character.id,
            display.enter.as_deref().unwrap_or("none"),
            display.exit.as_deref().unwrap_or("none"),
            display.duration_ms,
        );
        edited = checked
            .insert_block_before(&edited, start, &statement)
            .map_err(|error| error.to_string())?
            .0;
    }
    let start = start
        .checked_add_signed(edited.len() as isize - before as isize)
        .ok_or("Source range changed unexpectedly")?;
    let checked = EiyashouProjection::parse(&edited);
    if checked.read_only.len() > projection.read_only.len()
        || !checked
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .any(|block| {
                block.source_range.start == start
                    && !block.disabled
                    && !block.read_only
                    && block
                        .summary
                        .split('(')
                        .next()
                        .is_some_and(|name| name.trim() == command)
            })
    {
        return Err("Preset would produce invalid source".into());
    }
    Ok((edited, start))
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
    if let Some(display) = &character.display {
        display.validate().map_err(str::to_owned)?;
    }
    if mode == "hide" {
        let mut result = display_transition(character);
        result.push_str(&format!("hide({})", character.id));
        return Ok(result);
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
    let mut result = if mode == "show" {
        display_transition(character)
    } else {
        String::new()
    };
    result.push_str(&format!("{header}({}, {}", character.id, frames[0]));
    if mode == "show"
        && let Some(display) = &character.display
    {
        result.push_str(&format!(
            ", position: {}(x: {}, y: {})",
            display.position, display.x, display.y
        ));
        if let Some(height) = display.height_percent {
            result.push_str(&format!(
                ", layout: viewport(height: {})",
                f32::from(height) / 100.0
            ));
        } else {
            result.push_str(", layout: natural");
        }
    }
    result.push(')');
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

fn display_transition(character: &CharacterEntry) -> String {
    character
        .display
        .as_ref()
        .map(|display| {
            format!(
                "sprite.transition({}, enter: {}, exit: {}, duration: {}ms),\n",
                character.id,
                display.enter.as_deref().unwrap_or("none"),
                display.exit.as_deref().unwrap_or("none"),
                display.duration_ms,
            )
        })
        .unwrap_or_default()
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
    fn saved_display_applies_to_one_sprite_preserving_source_and_reuses_its_rule() {
        use keine_core::{Action, Anchor, SpriteLayout};
        let (mut character, _) = fixture();
        character.display = Some(CharacterDisplayPreset {
            position: "right".into(),
            x: -24,
            y: -10,
            height_percent: Some(80),
            enter: Some("enter_from_right".into()),
            exit: Some("exit".into()),
            duration_ms: 300,
        });
        for (source, selected) in [
            (
                "// header\nscene start {\n  sprite(hero, rest, position: left(x: 90, y: 12), layout: viewport(height: 0.2), z: 4, alpha: 0.7, transition: fade(2s)), // keep\n  \"中文\", sprite(other, closed)\n}",
                "sprite(hero",
            ),
            (
                "scene start { sprite(\n hero,\n rest\n), \"next\" }",
                "sprite(",
            ),
            (
                "scene start { sprite.transition(hero, enter: enter_from_left, duration: 9s), /* rule note */ sprite(hero, rest), \"next\" }",
                "sprite(hero",
            ),
            (
                "scene start { sprite.transition(other, enter: enter_from_left, duration: 500ms), sprite(hero, rest), \"next\" }",
                "sprite(hero",
            ),
            (
                "scene start { sprite(hero, rest), sprite.update(hero, closed, position: center(x: 8), layout: natural, scale: 1.2, duration: 600ms, blocking: false), \"next\" }",
                "sprite.update(",
            ),
        ] {
            let (edited, start) =
                apply_character_display(source, source.find(selected).unwrap(), &character)
                    .unwrap();
            assert_eq!(
                edited.as_str().matches("sprite.transition(hero").count(),
                1,
                "{edited}"
            );
            let parsed = keine_loader::parse_native_scenes(&edited);
            assert!(
                parsed[0].report.diagnostics.is_empty(),
                "{edited}: {:?}",
                parsed[0].report.diagnostics
            );
            let action = parsed[0].report.actions.iter().rev().find(|action| {
                matches!(action, Action::ShowSprite { id, .. } | Action::PatchSprite { id, .. } if id == "hero")
            }).unwrap();
            let (position, layout) = match action {
                Action::ShowSprite {
                    image,
                    position,
                    layout,
                    ..
                } => {
                    assert_eq!(image, "rest");
                    (position, layout)
                }
                Action::PatchSprite {
                    image,
                    position,
                    layout,
                    duration,
                    scale,
                    blocking,
                    ..
                } => {
                    assert_eq!(image, "closed");
                    assert_eq!((*duration, *scale, *blocking), (0.6, Some(1.2), false));
                    (position.as_ref().unwrap(), layout.as_ref().unwrap())
                }
                _ => unreachable!(),
            };
            assert_eq!(position.x, Anchor::Right(-24.0), "{edited}");
            assert_eq!(position.y, -10.0);
            assert!(
                matches!(layout, SpriteLayout::ViewportHeight(height) if (*height - 0.8).abs() < f32::EPSILON)
            );
            if source.contains("// header") {
                for kept in [
                    "// header",
                    "// keep",
                    "z: 4",
                    "alpha: 0.7",
                    "\"中文\"",
                    "sprite(other, closed)",
                ] {
                    assert!(edited.contains(kept), "{kept}: {edited}");
                }
                assert!(!edited.contains("fade(2s)"));
            }
            if source.contains("/* rule note */") {
                assert!(edited.contains("/* rule note */"));
            }
            if source.contains("sprite.transition(other") {
                assert!(
                    edited.contains(
                        "sprite.transition(other, enter: enter_from_left, duration: 500ms)"
                    )
                );
            }
            assert_eq!(
                apply_character_display(&edited, start, &character).unwrap(),
                (edited, start)
            );
        }
        let nested = "scene start { if (true) { sprite.transition(hero, enter: enter_from_left, duration: 500ms) } else { sprite(hero, rest), \"next\" } }";
        let (edited, _) =
            apply_character_display(nested, nested.find("sprite(hero").unwrap(), &character)
                .unwrap();
        assert!(edited.contains(
            "if (true) { sprite.transition(hero, enter: enter_from_left, duration: 500ms) }"
        ));
        assert_eq!(edited.as_str().matches("sprite.transition(hero").count(), 2);
        assert!(
            keine_loader::parse_native_scenes(&edited)[0]
                .report
                .diagnostics
                .is_empty()
        );
        for (source, selected) in [
            ("scene start { sprite(other, rest) }", "sprite("),
            ("scene start { wait(1s) }", "wait("),
            (
                "scene start { /* disabled\nsprite(hero, rest)\n*/ }",
                "/* disabled",
            ),
        ] {
            assert!(
                apply_character_display(source, source.find(selected).unwrap(), &character)
                    .is_err(),
                "{source}"
            );
        }
        let source = "scene start { sprite.transition(hero, enter: enter_from_left, duration: 500ms), sprite(hero, rest, position: right(x: 88), layout: viewport(height: 0.8)) }";
        character.display = None;
        let (edited, start) =
            apply_character_display(source, source.find("sprite(hero").unwrap(), &character)
                .unwrap();
        assert!(edited.contains("position: center(x: 0, y: 0)"));
        assert!(edited.contains("layout: natural"));
        assert!(edited.contains("enter: none"));
        assert!(edited.contains("exit: none"));
        assert_eq!(
            apply_character_display(&edited, start, &character).unwrap(),
            (edited.clone(), start)
        );
        assert!(apply_character_display(&edited, edited.len() + 1, &character).is_err());
        character.display = Some(CharacterDisplayPreset {
            height_percent: Some(0),
            ..Default::default()
        });
        assert!(apply_character_display(&edited, start, &character).is_err());
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

    #[test]
    fn display_presets_expand_show_and_hide_but_preserve_expression_update_geometry() {
        use keine_core::{Action, Anchor, SpriteLayout};
        let (mut character, index) = fixture();
        for (position, enter, height) in [
            ("left", "enter_from_left", Some(85)),
            ("center", "enter", None),
            ("right", "enter_from_right", Some(100)),
            ("center", "enter_from_bottom", Some(400)),
        ] {
            character.display = Some(CharacterDisplayPreset {
                position: position.into(),
                x: 24,
                y: -10,
                height_percent: height,
                enter: Some(enter.into()),
                exit: Some("exit".into()),
                duration_ms: 300,
            });
            let show = character_statement(&character, "smile", "show", &index).unwrap();
            let hide =
                character_statement(&character, "", "hide", &AuthoringIndex::default()).unwrap();
            let parsed = keine_loader::parse_native_scenes(&format!(
                "scene start {{ {show}, \"visible\", {hide} }}"
            ));
            assert!(
                parsed[0].report.diagnostics.is_empty(),
                "{position}: {:?}",
                parsed[0].report.diagnostics
            );
            assert!(
                matches!(&parsed[0].report.actions[0], Action::SetTransition {target, enter: Some(_), exit: Some(_), duration}
                if target == "hero" && (*duration - 0.3).abs() < f32::EPSILON)
            );
            let expected_anchor = match position {
                "left" => Anchor::Left(24.0),
                "right" => Anchor::Right(24.0),
                _ => Anchor::Center(24.0),
            };
            assert!(
                matches!(&parsed[0].report.actions[1], Action::ShowSprite {position, layout, ..}
                if position.x == expected_anchor && position.y == -10.0 && *layout == height.map(|height| SpriteLayout::ViewportHeight(f32::from(height)/100.0)).unwrap_or(SpriteLayout::Natural))
            );
            for mode in ["change", "blink", "talk"] {
                let change = character_statement(&character, "blink", mode, &index).unwrap();
                let parsed =
                    keine_loader::parse_native_scenes(&format!("scene start {{ {change} }}"));
                assert!(parsed[0].report.diagnostics.is_empty(), "{mode}");
                assert!(
                    matches!(
                        &parsed[0].report.actions[0],
                        Action::PatchSprite {
                            position: None,
                            layout: None,
                            ..
                        }
                    ),
                    "{mode}"
                );
                assert!(!change.contains("sprite.transition"), "{mode}");
            }
        }
        character.display.as_mut().unwrap().height_percent = Some(0);
        assert!(character_statement(&character, "smile", "show", &index).is_err());
        // Resetting a saved preset must actively clear an earlier transition rule.
        character.display = Some(CharacterDisplayPreset::default());
        let show = character_statement(&character, "smile", "show", &index).unwrap();
        let parsed =
            keine_loader::parse_native_scenes(&format!("scene start {{ {show}, \"ready\" }}"));
        assert!(parsed[0].report.diagnostics.is_empty());
        let mut state = keine_core::State::new();
        state.current_scene = "start".into();
        state.insert_scene("start".into(), parsed[0].report.actions.clone());
        state.transition_rules.insert(
            "hero".into(),
            keine_core::model::state::TransitionRule {
                enter: Some(keine_core::AnimationPreset::EnterFromRight),
                exit: Some(keine_core::AnimationPreset::Exit),
                duration: 0.3,
            },
        );
        assert_eq!(
            keine_core::step::step(&mut state),
            keine_core::StepResult::AwaitClick
        );
        assert!(state.transition_rules["hero"].enter.is_none());
        assert!(state.transition_rules["hero"].exit.is_none());
        assert!(state.sprites["hero"].animation.is_none());
        assert_eq!(state.sprites["hero"].layout, SpriteLayout::Natural);
    }
}

mod state;
pub(in crate::app) use state::CharactersState;
