use super::*;
use gpui_kit::{DispatchPhase, ScrollWheelEvent, canvas, point};

pub(super) fn speaker_options(index: &AuthoringIndex) -> Vec<SourceOption> {
    std::iter::once(SourceOption {
        value: "Narrator".into(),
        title: "Narrator".into(),
        asset: None,
    })
    .chain(index.characters.iter().map(|character| SourceOption {
        value: character.id.clone(),
        title: character.name.clone().into(),
        asset: None,
    }))
    .collect()
}

pub(super) fn audition_control(
    root: &Path,
    path: &Path,
    compact: bool,
    cx: &mut App,
) -> AnyElement {
    let controller = cx.global_mut::<EditorDocuments>().preview(root).ok();
    let playing = controller
        .as_ref()
        .is_some_and(|controller| controller.snapshot().audition_path.as_deref() == Some(path));
    let path = path.to_owned();
    div()
        .id(format!("audition-{}", path.display()))
        .h(px(if compact { 24. } else { 32. }))
        .min_w(px(24.))
        .px_2()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .rounded(px(4.))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .when(!compact, |this| {
            this.border_1().border_color(rgb(BORDER)).bg(rgb(SURFACE))
        })
        .tooltip(icon_hint(if playing { "Stop" } else { "Audition" }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            if let Some(controller) = &controller {
                controller.toggle_audition(path.clone());
            }
        })
        .child(
            Icon::new(if playing {
                AssetIconName::Square
            } else {
                AssetIconName::Play
            })
            .xsmall()
            .text_color(rgb(PRIMARY)),
        )
        .when(!compact, |this| {
            this.child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(INK))
                    .child(if playing { "Stop" } else { "Play" }),
            )
        })
        .into_any_element()
}

pub(super) fn voice_resource_options(root: &Path, index: &AuthoringIndex) -> Vec<SourceOption> {
    let mut options = vec![SourceOption {
        value: String::new(),
        title: "No voice".into(),
        asset: None,
    }];
    options.extend(
        index
            .assets
            .iter()
            .filter(|asset| asset.kind == AssetKind::Voice)
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
            }),
    );
    options
}

#[derive(Clone)]
pub(super) enum ResourceTarget {
    Source(SourceInspectorKey, usize),
    Voice(InspectorEditKey),
    Speaker(InspectorEditKey),
}

pub(super) struct ResourcePicker {
    root: PathBuf,
    target: ResourceTarget,
    options: Vec<SourceOption>,
    current: String,
    input: Entity<InputState>,
    index: usize,
    trigger: Bounds<Pixels>,
    width: Pixels,
    pub window_size: gpui_kit::Size<Pixels>,
    bounds: Rc<RefCell<Bounds<Pixels>>>,
    scroll: ScrollHandle,
    pub epoch: u64,
    pub closing: bool,
    _subscription: Subscription,
}

impl ResourcePicker {
    pub fn source_is_current(&self, cx: &App) -> bool {
        let documents = cx.global::<EditorDocuments>();
        let (path, start) = match &self.target {
            ResourceTarget::Source(key, _) => (&key.path, key.block_start),
            ResourceTarget::Voice(key) | ResourceTarget::Speaker(key) => {
                (&key.path, key.block_start)
            }
        };
        if documents
            .block_selection(&self.root)
            .is_some_and(|(selected_path, starts)| {
                selected_path != path || !starts.contains(&start)
            })
        {
            return false;
        }
        let Some(source) = documents.source(&self.root, path) else {
            return false;
        };
        let projection = documents.projection(&self.root, path, &source);
        match &self.target {
            ResourceTarget::Source(key, _) => projection
                .scenes
                .iter()
                .flat_map(|scene| &scene.blocks)
                .find(|block| block.source_range.start == start)
                .is_some_and(|block| {
                    block.kind == key.kind
                        && block.summary.split('(').next().map(str::trim)
                            == Some(key.command.as_str())
                        && projection.source_fields_for_block(&source, block).as_ref()
                            == Some(&key.fields)
                }),
            ResourceTarget::Voice(key) | ResourceTarget::Speaker(key) => {
                projection.text_block_metadata(&source, start).as_ref() == Some(&key.metadata)
            }
        }
    }
    fn matches(&self, cx: &App) -> Vec<SourceOption> {
        let query = self.input.read(cx).value();
        self.options
            .iter()
            .filter(|option| option.matches(&query))
            .cloned()
            .collect()
    }
}

/// One source-backed resource popup serves inline Blocks and Inspector.
pub(super) fn resource_trigger(
    root: &Path,
    target: ResourceTarget,
    options: Vec<SourceOption>,
    current: String,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let root = root.to_owned();
    let label = options
        .iter()
        .find(|option| option.value == current)
        .map(|option| option.title.to_string())
        .unwrap_or_else(|| current.clone());
    let id = match &target {
        ResourceTarget::Source(key, position) => format!("resource-{}-{position}", key.block_start),
        ResourceTarget::Voice(key) => format!("voice-resource-{}", key.block_start),
        ResourceTarget::Speaker(key) => format!("speaker-resource-{}", key.block_start),
    };
    let resource = options.iter().any(|option| option.asset.is_some());
    let enabled = match &target {
        ResourceTarget::Source(key, position) => source_field_enabled(key, &key.fields[*position]),
        _ => true,
    };
    let bounds = Rc::new(RefCell::new(Bounds::default()));
    let measured = bounds.clone();
    let keyboard_root = root.clone();
    let keyboard_target = target.clone();
    let keyboard_options = options.clone();
    let keyboard_current = current.clone();
    let keyboard_bounds = bounds.clone();
    div()
        .id(id)
        .relative()
        .w_full()
        .min_w_0()
        .h(px(28.))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(7.))
        .bg(rgb(SURFACE))
        .cursor_pointer()
        .when(enabled, |this| {
            this.hover(|style| style.bg(rgb(SURFACE_HOVER)))
        })
        .when(!enabled, |this| this.cursor_default())
        .when(enabled, |this| this.focusable())
        .on_key_down(
            cx.listener(move |panel, event: &gpui_kit::KeyDownEvent, window, cx| {
                if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space" | "down") {
                    cx.stop_propagation();
                    if matches!(panel.content, PanelContent::Document { .. })
                        && let ResourceTarget::Source(key, _) = &keyboard_target
                    {
                        panel.select_current_block(key.block_start, cx);
                    }
                    panel.open_resource_picker(
                        &keyboard_root,
                        keyboard_target.clone(),
                        keyboard_options.clone(),
                        keyboard_current.clone(),
                        *keyboard_bounds.borrow(),
                        window,
                        cx,
                    );
                }
            }),
        )
        .child(
            canvas(
                move |bounds, _, _| {
                    *measured.borrow_mut() = bounds;
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .when(resource, |this| {
            this.child(
                Icon::new(AssetIconName::Folder)
                    .xsmall()
                    .text_color(rgb(MUTED)),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(12.))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(if label.is_empty() {
                    "Select…".to_owned()
                } else {
                    label
                }),
        )
        .child(
            Icon::new(AssetIconName::ChevronDown)
                .xsmall()
                .text_color(rgb(MUTED)),
        )
        .on_click(cx.listener(move |panel, _, window, cx| {
            cx.stop_propagation();
            if !enabled {
                return;
            }
            if matches!(panel.content, PanelContent::Document { .. })
                && let ResourceTarget::Source(key, _) = &target
            {
                panel.select_current_block(key.block_start, cx);
            }
            panel.open_resource_picker(
                &root,
                target.clone(),
                options.clone(),
                current.clone(),
                *bounds.borrow(),
                window,
                cx,
            );
        }))
        .into_any_element()
}

impl WorkbenchPanel {
    fn step_resource_picker(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(picker) = self.resource_picker.as_ref() else {
            return;
        };
        let count = picker.matches(cx).len();
        if count == 0 {
            return;
        }
        let Some(picker) = self.resource_picker.as_mut() else {
            return;
        };
        let direction = if forward { 1 } else { count - 1 };
        picker.index = (picker.index + direction) % count;
        let row_height = if picker.options.iter().any(|option| option.asset.is_some()) {
            42.
        } else {
            28.
        };
        picker.scroll.set_offset(point(
            px(0.),
            -px((picker.index as f32 * row_height - row_height * 2.).max(0.)),
        ));
        cx.notify();
    }

    pub(super) fn resource_picker_next(
        &mut self,
        _: &ResourcePickerNext,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_resource_picker(true, cx);
    }

    pub(super) fn resource_picker_previous(
        &mut self,
        _: &ResourcePickerPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_resource_picker(false, cx);
    }

    pub(super) fn accept_resource_picker(
        &mut self,
        _: &AcceptResourcePicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.resource_picker.as_ref() else {
            return;
        };
        let options = picker.matches(cx);
        let index = picker.index.min(options.len().saturating_sub(1));
        if let Some(option) = options.into_iter().nth(index) {
            self.choose_resource(option, window, cx);
        }
    }

    pub(super) fn dismiss_resource_picker(
        &mut self,
        _: &CloseResourcePicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_resource_picker(true, window, cx);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "popup requires source target and measured trigger"
    )]
    fn open_resource_picker(
        &mut self,
        root: &Path,
        target: ResourceTarget,
        mut options: Vec<SourceOption>,
        current: String,
        trigger: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_resource_picker(false, window, cx);
        if let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(root) {
            preview.stop_audition();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subscription = cx.subscribe(&input, |panel, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                if let Some(picker) = panel.resource_picker.as_mut() {
                    picker.index = 0;
                    picker.scroll.set_offset(point(px(0.), px(0.)));
                }
                cx.notify();
            }
        });
        options.sort_by(|a, b| {
            a.asset
                .as_ref()
                .map(|(_, _, path)| path)
                .cmp(&b.asset.as_ref().map(|(_, _, path)| path))
        });
        let epoch = {
            let documents = cx.global_mut::<EditorDocuments>();
            documents.resource_picker_epoch = documents.resource_picker_epoch.wrapping_add(1);
            documents.resource_picker_epoch
        };
        let index = options
            .iter()
            .position(|option| option.value == current)
            .unwrap_or(0);
        let size = window.viewport_size();
        self.resource_picker = Some(ResourcePicker {
            root: root.to_owned(),
            target,
            options,
            current,
            input: input.clone(),
            index,
            trigger,
            width: trigger.size.width.max(px(220.)),
            window_size: size,
            bounds: Rc::new(RefCell::new(Bounds::default())),
            scroll: ScrollHandle::new(),
            epoch,
            closing: false,
            _subscription: subscription,
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn close_resource_picker(
        &mut self,
        restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.resource_picker.as_mut() else {
            return;
        };
        if picker.closing {
            return;
        }
        picker.closing = true;
        let epoch = picker.epoch;
        if epoch == cx.global::<EditorDocuments>().resource_picker_epoch
            && let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(&picker.root)
        {
            preview.stop_audition();
        }
        if restore_focus {
            self.focus.focus(window, cx);
        }
        cx.notify();
        let delay = if cx.reduce_motion() {
            Duration::ZERO
        } else {
            Duration::from_millis(90)
        };
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this
                    .resource_picker
                    .as_ref()
                    .is_some_and(|picker| picker.epoch == epoch && picker.closing)
                {
                    this.resource_picker = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn choose_resource(
        &mut self,
        option: SourceOption,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.resource_picker.as_ref() else {
            return;
        };
        if picker.closing {
            return;
        }
        let root = picker.root.clone();
        let target = picker.target.clone();
        self.close_resource_picker(true, window, cx);
        match target {
            ResourceTarget::Source(key, position) => {
                if !source_field_enabled(&key, &key.fields[position]) {
                    return;
                }
                self.commit_source_field(
                    &root,
                    &key,
                    position,
                    if source_asset_kind(&key, &key.fields[position]).is_some() {
                        asset_source_value(&key.fields[position], &option.value)
                    } else {
                        option.value.clone()
                    },
                    window,
                    cx,
                );
            }
            ResourceTarget::Voice(ref key) | ResourceTarget::Speaker(ref key) => {
                let position = usize::from(matches!(target, ResourceTarget::Voice(_)));
                if self.inspector_key.as_ref() != Some(key) {
                    return;
                }
                if let Some(input) = self.inspector_inputs.get(position) {
                    input.update(cx, |input, cx| {
                        input.set_value(option.value.clone(), window, cx)
                    });
                }
                self.commit_text_inspector(&root, window, cx);
            }
        }
        if let Some((_, kind, path)) = option.asset {
            cx.global_mut::<EditorDocuments>().preview_asset(
                &root,
                kind,
                option.title.to_string(),
                path,
            );
            window.dispatch_action(Box::new(ShowAssetPreview), cx);
            cx.refresh_windows();
        }
    }

    pub(super) fn render_resource_picker(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let picker = self.resource_picker.as_ref()?;
        let options = picker.matches(cx);
        let index = picker.index.min(options.len().saturating_sub(1));
        let root = picker.root.clone();
        let bounds = picker.bounds.clone();
        let measured = bounds.clone();
        let weak = cx.entity().downgrade();
        let epoch = picker.epoch;
        let resource = picker.options.iter().any(|option| option.asset.is_some());
        let row_height = if resource { 42. } else { 28. };
        let height = px((options.len().max(1) as f32 * row_height
            + if resource { 64. } else { 34. })
        .min(320.));
        let closing = picker.closing;
        let y = if picker.trigger.origin.y >= height + px(8.) {
            picker.trigger.origin.y - height
        } else {
            picker
                .trigger
                .bottom()
                .min(picker.window_size.height - height - px(8.))
                .max(px(8.))
        };
        let position = point(picker.trigger.origin.x, y);
        let surface = div()
            .id(("resource-picker", epoch as usize))
            .w(picker.width)
            .h(height)
            .flex()
            .flex_col()
            .rounded(px(8.))
            .bg(rgb(SURFACE))
            .shadow_lg()
            .overflow_hidden()
            .track_focus(&picker.input.focus_handle(cx))
            .child(
                canvas(
                    move |bounds, _, _| *measured.borrow_mut() = bounds,
                    move |_, _, window, _| {
                        let weak = weak.clone();
                        let bounds = bounds.clone();
                        window.on_mouse_event(
                            move |event: &ScrollWheelEvent, phase, window, cx| {
                                if phase == DispatchPhase::Capture
                                    && !bounds.borrow().contains(&event.position)
                                {
                                    let _ = weak.update(cx, |panel, cx| {
                                        panel.close_resource_picker(false, window, cx)
                                    });
                                }
                            },
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .h(px(34.))
                    .px_2()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        Icon::new(AssetIconName::Search)
                            .xsmall()
                            .text_color(rgb(MUTED)),
                    )
                    .child(
                        Input::new(&picker.input)
                            .appearance(false)
                            .bordered(false)
                            .w_full()
                            .px_0()
                            .py_0()
                            .text_size(px(12.)),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child(options.len().to_string()),
                    ),
            )
            .child(
                div()
                    .id("resource-results")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&picker.scroll)
                    .when(options.is_empty(), |this| {
                        this.child(
                            div()
                                .p_3()
                                .text_size(px(12.))
                                .text_color(rgb(MUTED))
                                .child("No matching resources"),
                        )
                    })
                    .children(options.into_iter().enumerate().map(|(row, option)| {
                        let rendered = if resource {
                            option.render(window, cx).into_any_element()
                        } else {
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(option.title.clone())
                                .into_any_element()
                        };
                        let current = picker.current == option.value;
                        div()
                            .id(("resource-option", row))
                            .h(px(row_height))
                            .flex_none()
                            .px_2()
                            .py_1()
                            .flex()
                            .items_center()
                            .gap_2()
                            .bg(rgb(if row == index { SURFACE_HOVER } else { SURFACE }))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                            .child(rendered)
                            .when(current, |this| {
                                this.child(
                                    Icon::new(AssetIconName::Check)
                                        .xsmall()
                                        .text_color(rgb(PRIMARY)),
                                )
                            })
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                cx.stop_propagation();
                                panel.choose_resource(option.clone(), window, cx);
                            }))
                    })),
            )
            .when(resource, |this| {
                this.child(
                    div()
                        .id("open-resource-manager")
                        .h(px(30.))
                        .px_2()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .border_t_1()
                        .border_color(rgb(BORDER))
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.close_resource_picker(true, window, cx);
                            window.dispatch_action(Box::new(ShowAssets), cx);
                            cx.global_mut::<EditorDocuments>()
                                .set_notice(&root, "Resource manager");
                        }))
                        .child(Icon::new(AssetIconName::Folder).xsmall())
                        .child("Manage resources"),
                )
            })
            .with_animation(
                ("dropdown-motion", epoch as usize * 2 + usize::from(closing)),
                Animation::new(if cx.reduce_motion() {
                    Duration::ZERO
                } else if closing {
                    Duration::from_millis(90)
                } else {
                    TAB_MOTION_DURATION
                })
                .with_easing(ease_out_quint()),
                move |surface, progress| {
                    surface.opacity(if closing { 1. - progress } else { progress })
                },
            );
        Some(
            deferred(
                anchored()
                    .anchor(Anchor::TopLeft)
                    .position(position)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        div()
                            .on_mouse_down_out(cx.listener(|panel, _, window, cx| {
                                panel.close_resource_picker(false, window, cx)
                            }))
                            .child(surface),
                    ),
            )
            .priority(200)
            .into_any_element(),
        )
    }
}
