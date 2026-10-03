//! Block cards, inline text and projected document layout.
use super::*;
use crate::authoring::fields::{command_field_label, title_case};

/// One palette for Block type badges and their overview strokes.
pub(in crate::app) fn block_type_color(kind: &BlockKind, source: &str) -> u32 {
    match kind {
        BlockKind::Narration | BlockKind::Dialogue { .. } => 0xa4c9a5,
        BlockKind::Declaration | BlockKind::Assignment => 0xe2c58f,
        BlockKind::Unsupported => 0xdb7780,
        BlockKind::Command => {
            let name = source.split('(').next().unwrap_or_default().trim();
            match name.split('.').next().unwrap_or(name) {
                "camera" | "track" | "key" | "stage" => 0xc4b0e5,
                "background" | "scene" => 0xa4c9a5,
                "sprite" | "hide" | "move" | "avatar" | "frame" | "resource" | "case" => PRIMARY,
                "particle" => 0xe2c58f,
                "screen" => 0xe4abbc,
                _ => match InsertKind::for_command(name).map(InsertKind::category) {
                    Some("Text") => 0xa4c9a5,
                    Some("Media") => 0xe7b18b,
                    Some("Data") => 0xe2c58f,
                    Some("Flow") => 0xacaee4,
                    _ => PRIMARY,
                },
            }
        }
        _ => 0xacaee4,
    }
}

pub(in crate::app) fn block_card_label(kind: &BlockKind, source: &str) -> String {
    match kind {
        BlockKind::Dialogue { speaker } => speaker.clone(),
        BlockKind::Command => {
            let name = source.split('(').next().unwrap_or_default().trim();
            InsertKind::for_command(name)
                .map(|kind| kind.label().to_owned())
                .unwrap_or_else(|| title_case(name.rsplit('.').next().unwrap_or(name)))
        }
        BlockKind::Control => title_case(source.trim()),
        _ => kind.label().to_owned(),
    }
}

pub(in crate::app) fn block_card_icon(kind: &BlockKind, source: &str) -> AssetIconName {
    match kind {
        BlockKind::Narration | BlockKind::Dialogue { .. } => AssetIconName::MessageSquareText,
        BlockKind::Choice
        | BlockKind::ChoiceOption
        | BlockKind::Conditional
        | BlockKind::ElseIf => AssetIconName::GitBranch,
        BlockKind::Else => AssetIconName::Workflow,
        BlockKind::Loop => AssetIconName::Repeat2,
        BlockKind::Declaration | BlockKind::Assignment => AssetIconName::Braces,
        BlockKind::Command => {
            InsertKind::for_command(source.split('(').next().unwrap_or_default().trim())
                .map(insert_kind_icon)
                .unwrap_or(AssetIconName::Braces)
        }
        BlockKind::Control => AssetIconName::Workflow,
        BlockKind::Unsupported => AssetIconName::TriangleAlert,
    }
}

pub(in crate::app) fn block_card_summary(kind: &BlockKind, source: &str, line: usize) -> String {
    match kind {
        BlockKind::Narration | BlockKind::Dialogue { .. } => {
            format!("Dynamic text · L{}", line + 1)
        }
        BlockKind::Choice
        | BlockKind::Conditional
        | BlockKind::ElseIf
        | BlockKind::Else
        | BlockKind::Loop
        | BlockKind::Control => String::new(),
        BlockKind::ChoiceOption => first_quoted_text(source).unwrap_or_default(),
        BlockKind::Declaration => source
            .strip_prefix("let ")
            .and_then(|tail| tail.split_whitespace().next())
            .unwrap_or_default()
            .to_owned(),
        BlockKind::Assignment => source
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned(),
        BlockKind::Command => String::new(),
        BlockKind::Unsupported => format!("Unsupported syntax · L{}", line + 1),
    }
}

fn block_card_headline(
    kind: &BlockKind,
    source: &str,
    fields: &[SourceField],
    fallback: String,
) -> String {
    if !matches!(kind, BlockKind::Command) {
        return fallback;
    }
    let command = source.split('(').next().unwrap_or_default().trim();
    let value = |key: &str| {
        fields
            .iter()
            .find(|field| field.key == key)
            .map(|field| field.value.as_str())
    };
    match command {
        // Summaries are truncated and commas can belong to nested calls or text.
        // Source fields already retain complete, token-bounded argument values.
        "sprite" | "move" => value("1").unwrap_or_default().to_owned(),
        "sprite.focus.configure" => "Sprite focus styles".to_owned(),
        "screen.curtain.show" | "screen.curtain.hide" => {
            value("color").unwrap_or_default().to_owned()
        }
        "text.retract" => format!(
            "{}  →  {}",
            value("source")
                .filter(|text| !text.is_empty())
                .unwrap_or("Current text"),
            value("keep")
                .filter(|text| !text.is_empty())
                .unwrap_or("Empty")
        )
        .replace(['\r', '\n'], " ↵ "),
        "track" => match (value("0"), value("1")) {
            (Some(target), Some(property)) => format!("{target}  →  {property}"),
            _ => fallback,
        },
        "key" => value("time").unwrap_or(fallback.as_str()).to_owned(),
        "stage.animate" => value("0").unwrap_or(fallback.as_str()).to_owned(),
        event if event.starts_with("event.") => value("time")
            .map(|time| format!("at {time}"))
            .unwrap_or(fallback),
        _ => value("0")
            .or_else(|| {
                fields
                    .iter()
                    .find(|field| field.insertion.is_none())
                    .map(|field| field.value.as_str())
            })
            .unwrap_or_default()
            .to_owned(),
    }
}

pub(in crate::app) fn first_quoted_text(source: &str) -> Option<String> {
    let start = source.find('"')? + 1;
    let mut escaped = false;
    for (offset, character) in source[start..].char_indices() {
        if character == '"' && !escaped {
            return Some(source[start..start + offset].to_owned());
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    None
}

pub(in crate::app) struct BlockProjectionView<'a> {
    pub(in crate::app) root: &'a Path,
    pub(in crate::app) relative: &'a Path,
    pub(in crate::app) document: &'a DocumentHandle,
    pub(in crate::app) editors: &'a [BlockTextEditor],
    pub(in crate::app) inline: &'a HashMap<usize, InlineBlockControl>,
    pub(in crate::app) collapsed_scenes: &'a HashSet<String>,
    pub(in crate::app) selected_blocks: &'a HashSet<usize>,
    pub(in crate::app) draft_text: Option<&'a DraftTextBlock>,
    pub(in crate::app) drag: &'a super::BlockDragState,
    pub(in crate::app) row_bounds: &'a Rc<RefCell<HashMap<usize, Bounds<Pixels>>>>,
    pub(in crate::app) row_positions: &'a RefCell<HashMap<usize, f32>>,
    pub(in crate::app) scroll_handle: &'a ScrollHandle,
    pub(in crate::app) scroll_anchor: &'a ScrollAnchor,
    pub(in crate::app) scroll_pending: bool,
    pub(in crate::app) minimap: &'a minimap::Navigation,
    pub(in crate::app) scene_edit: Option<&'a SceneEditMode>,
    pub(in crate::app) scene_name_input: &'a Entity<InputState>,
    pub(in crate::app) visible: &'a HashSet<usize>,
    pub(in crate::app) heights: &'a HashMap<usize, f32>,
    pub(in crate::app) layout: &'a RefCell<super::layout::Cache>,
}

pub(in crate::app) fn block_row_height(
    block: &crate::projection::BlockCard,
    editors: &[BlockTextEditor],
    draft_text: Option<&DraftTextBlock>,
    heights: &HashMap<usize, f32>,
    cx: &App,
) -> f32 {
    if matches!(
        block.kind,
        BlockKind::Narration | BlockKind::Dialogue { .. }
    ) {
        let text_state = block.text_range.as_ref().and_then(|range| {
            editors
                .iter()
                .find(|editor| editor.text_start == range.start)
                .map(|editor| &editor.state)
                .or_else(|| {
                    draft_text
                        .filter(|draft| {
                            draft
                                .text_range
                                .as_ref()
                                .is_some_and(|draft_range| draft_range.start == range.start)
                        })
                        .map(|draft| &draft.state)
                })
        });
        if text_state.is_none_or(|state| state.read(cx).text_bounds().is_none())
            && let Some(height) = heights.get(&block.source_range.start)
        {
            return *height;
        }
        let text_rows = text_state.map_or(block.text_rows, |state| {
            state.read(cx).value().lines().count().clamp(1, 6)
        });
        let measured = text_state
            .and_then(|state| state.read(cx).text_bounds())
            .map(|bounds| f32::from(bounds.size.height))
            .filter(|height| *height > 0.);
        let speaker_height = if matches!(block.kind, BlockKind::Dialogue { .. }) {
            20.
        } else {
            0.
        };
        speaker_height
            + measured.map_or(38. + (text_rows.saturating_sub(1) as f32 * 20.), |height| {
                (height + 8.).max(38.)
            })
    } else if matches!(
        block.kind,
        BlockKind::Choice
            | BlockKind::ChoiceOption
            | BlockKind::Conditional
            | BlockKind::ElseIf
            | BlockKind::Else
            | BlockKind::Loop
    ) {
        44.
    } else {
        38.
    }
}

pub(in crate::app) fn draft_text_row(draft: &DraftTextBlock, indent: f32, id: usize) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .pl(px(indent))
        .child(
            div()
                .id(("draft-text-block", id))
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .min_h(px(38.))
                .px_2()
                .rounded(px(7.))
                .bg(rgb(SURFACE))
                .on_action(|_: &gpui_kit::base::input::Enter, _, cx| cx.stop_propagation())
                .child(
                    Icon::new(AssetIconName::MessageSquarePlus)
                        .xsmall()
                        .text_color(rgb(PRIMARY)),
                )
                .child(
                    div()
                        .w(px(64.))
                        .flex_none()
                        .text_xs()
                        .text_color(rgb(PRIMARY))
                        .child("Text"),
                )
                .child(
                    div().min_h(px(30.)).flex_1().min_w_0().child(
                        Textarea::new(&draft.state)
                            .appearance(false)
                            .bordered(false)
                            .w_full()
                            .text_sm()
                            .text_color(rgb(INK)),
                    ),
                ),
        )
        .into_any_element()
}

struct WaitPreview {
    text: String,
    /// Display and source byte ranges for the shortened wait labels.
    waits: Vec<(Range<usize>, Range<usize>)>,
    editing: Option<Range<usize>>,
}

impl WaitPreview {
    fn source_selection(&self, offset: usize) -> Range<usize> {
        let mut delta = 0isize;
        for (display, source) in &self.waits {
            if offset < display.start {
                break;
            }
            if offset < display.end {
                return source.clone();
            }
            delta += source.len() as isize - display.len() as isize;
        }
        let offset = offset.saturating_add_signed(delta);
        offset..offset
    }
}

fn inline_wait_preview(source: &str, editing: Option<(usize, &str)>) -> WaitPreview {
    let mut text = String::new();
    let mut ranges = Vec::new();
    let mut end = 0;
    let mut editing_range = None;
    for (ordinal, wait) in keine_core::runtime::text::inline_waits(source).enumerate() {
        text.push_str(&source[end..wait.range.start]);
        let start = text.len();
        if let Some((_, value)) = editing.filter(|(index, _)| *index == ordinal) {
            text.push_str("[Wait ");
            let number_start = text.len();
            // Reserve the field's width in the same shaped line. The native input is
            // prepainted over this invisible run, so surrounding prose still wraps normally.
            text.extend(std::iter::repeat_n(
                '\u{2007}',
                value.chars().count().max(6),
            ));
            editing_range = Some(number_start..text.len());
            text.push_str(" ms]");
        } else {
            text.push_str(&match wait.duration {
                Some(seconds) => format!("[Wait {seconds}s]"),
                None => "[Wait for input]".to_owned(),
            });
        }
        ranges.push((start..text.len(), wait.range.clone()));
        end = wait.range.end;
    }
    text.push_str(&source[end..]);
    WaitPreview {
        text,
        waits: ranges,
        editing: editing_range,
    }
}

fn render_block_text(
    state: &Entity<TextareaState>,
    editors: &[BlockTextEditor],
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let active = editors
        .iter()
        .find(|row| row.state == *state)
        .and_then(|row| row.wait.as_ref())
        .map(|wait| (wait.ordinal, wait.input.clone()));
    let value = active.as_ref().map(|(_, input)| input.read(cx).value());
    let preview = inline_wait_preview(
        state.read(cx).value().as_ref(),
        active
            .as_ref()
            .zip(value.as_ref())
            .map(|((ordinal, _), value)| (*ordinal, value.as_ref())),
    );
    if !preview.waits.is_empty() && !state.read(cx).focus_handle(cx).is_focused(window) {
        let color: Hsla = rgb(block_type_color(&BlockKind::Command, "wait(1s)")).into();
        let highlights = preview.waits.iter().map(|(range, _)| {
            (
                range.clone(),
                gpui_kit::HighlightStyle {
                    color: Some(color),
                    background_color: Some(color.opacity(0.16)),
                    font_weight: Some(gpui_kit::FontWeight::SEMIBOLD),
                    ..Default::default()
                },
            )
        });
        let text = gpui_kit::StyledText::new(preview.text.clone()).with_highlights(highlights);
        let layout = text.layout().clone();
        let input = state.clone();
        let overlay = active
            .zip(preview.editing.clone())
            .map(|((_, input), range)| {
                let layout = layout.clone();
                canvas(
                    move |_, window, cx| {
                        let start = layout.position_for_index(range.start)?;
                        let end = layout.position_for_index(range.end)?;
                        let height = layout.line_height();
                        let width = if start.y == end.y {
                            end.x - start.x
                        } else {
                            px(48.)
                        };
                        let mut field = div()
                            .id(("inline-wait-duration", input.entity_id()))
                            .w(width)
                            .h(height)
                            // Single-line native inputs propagate Enter; consume it
                            // here so committing a duration cannot create a Text block.
                            .on_action(|_: &gpui_kit::base::input::Enter, _, cx| {
                                cx.stop_propagation()
                            })
                            .child(
                                Input::new(&input)
                                    .appearance(false)
                                    .bordered(false)
                                    .size_full()
                                    .min_h_0()
                                    .px_0()
                                    .py_0()
                                    .line_height(height)
                                    .text_size(px(13.))
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(color),
                            )
                            .into_any_element();
                        field.layout_as_root(
                            size(width, height).map(gpui_kit::AvailableSpace::Definite),
                            window,
                            cx,
                        );
                        field.prepaint_at(start, window, cx);
                        Some(field)
                    },
                    |_, field, window, cx| {
                        if let Some(mut field) = field {
                            field.paint(window, cx);
                        }
                    },
                )
                .absolute()
                .size_full()
            });
        return div()
            .relative()
            .track_focus(&state.read(cx).focus_handle(cx))
            .w_full()
            .min_w_0()
            .py(px(5.))
            .px_2()
            .text_size(px(13.))
            .text_color(rgb(INK))
            .cursor_text()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    let offset = layout
                        .index_for_position(event.position)
                        .unwrap_or_else(|index| index);
                    if let Some(ordinal) = preview
                        .waits
                        .iter()
                        .position(|(range, _)| range.contains(&offset))
                    {
                        // track_focus's default mouse handler would otherwise steal
                        // focus back from the duration field to the raw textarea.
                        window.prevent_default();
                        this.edit_inline_wait(&input, ordinal, window, cx);
                        return;
                    }
                    input.update(cx, |input, cx| {
                        input.set_selected_range(preview.source_selection(offset), cx);
                        input.focus(window, cx);
                    });
                    cx.notify();
                }),
            )
            .child(text)
            .children(overlay)
            .into_any_element();
    }
    // The row owns the complete-node menu; preserve ordinary IME, undo and source editing.
    div()
        // Submit is handled by PressEnter. Do not let the propagated native
        // action fall through to the workbench's new-Text key binding.
        .on_action(|_: &gpui_kit::base::input::Enter, _, cx| cx.stop_propagation())
        .w_full()
        .min_w_0()
        .child(
            Textarea::new(state)
                .context_menu(|menu, _, _| menu)
                .appearance(false)
                .bordered(false)
                .w_full()
                .text_size(px(13.))
                .text_color(rgb(INK)),
        )
        .into_any_element()
}

pub(in crate::app) fn render_block_projection(
    view: BlockProjectionView<'_>,
    window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let BlockProjectionView {
        root,
        relative,
        document,
        editors,
        inline,
        collapsed_scenes,
        selected_blocks,
        draft_text,
        drag,
        row_bounds,
        row_positions,
        scroll_handle,
        scroll_anchor,
        scroll_pending,
        minimap,
        scene_edit,
        scene_name_input,
        visible,
        heights,
        layout,
    } = view;
    let session = drag.session();
    let reorder_motion = drag.motion();
    let motion_progress = reorder_motion.map_or(1., BlockReorderMotion::progress);
    if motion_progress < 1.
        || session.is_some_and(|session| session.animating()) && !cx.reduce_motion()
    {
        window.request_animation_frame();
    }
    let mut layout = layout.borrow_mut();
    let snapshot = layout.snapshot(document, drag.source());
    let source = &snapshot.source;
    let projection = &snapshot.projection;
    let source_lines = &snapshot.lines;
    let index = cx.global::<EditorDocuments>().authoring(root);
    let errors = index
        .problems
        .iter()
        .filter(|problem| problem.path == relative && problem.severity == ProblemSeverity::Error)
        .map(|problem| {
            (
                source_lines.offset(
                    source,
                    problem.line.saturating_sub(1),
                    problem.column.saturating_sub(1),
                ),
                problem.message.as_str(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let line_number_width = snapshot.number_width;
    let line_number_gutter = line_number_width + 4.;
    let collapse_progresses = projection
        .scenes
        .iter()
        .enumerate()
        .map(|(scene_index, scene)| {
            let closed = collapsed_scenes.contains(&scene.name);
            transition(
                (
                    format!("block-scene-{}-{scene_index}", relative.display()),
                    "collapse",
                ),
                if closed { 0. } else { 1. },
                Transition::new(Duration::from_millis(120)),
                window,
                cx,
            )
        })
        .collect::<Vec<_>>();
    let geometry = (session.is_none()
        && reorder_motion.is_none()
        && draft_text.is_none()
        && scene_edit.is_none()
        && projection
            .scenes
            .iter()
            .zip(&collapse_progresses)
            .all(|(scene, progress)| {
                *progress
                    == if collapsed_scenes.contains(&scene.name) {
                        0.
                    } else {
                        1.
                    }
            }))
    .then(|| layout.geometry(&snapshot, collapsed_scenes, heights));
    let mut row_positions = row_positions.borrow_mut();
    if let Some(geometry) = &geometry {
        if !layout.positioned {
            row_positions.clone_from(&geometry.positions);
            layout.positioned = true;
        }
    } else {
        row_positions.clear();
        layout.positioned = false;
    }
    row_bounds.borrow_mut().clear();
    let block_fields = visible
        .iter()
        .filter_map(|start| snapshot.lookup.get(start))
        .map(|&(scene, block)| &projection.scenes[scene].blocks[block])
        .filter(|block| matches!(block.kind, BlockKind::Command))
        .map(|block| {
            (
                block.source_range.start,
                projection
                    .source_fields_for_block(source, block)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|field| field.insertion.is_none() && !field.value.is_empty())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let textbox_endings = &snapshot.endings;
    let block_order = snapshot.order.clone();
    let selected_position = cx
        .global::<EditorDocuments>()
        .selection(root)
        .filter(|(path, _, _)| path == relative)
        .map(|(_, line, column)| (*line, *column));
    let selected_line = selected_position.map(|(line, _)| line);
    let selected_start = selected_position.and_then(|(line, column)| {
        snapshot
            .block_at_position(line, column)
            .map(|block| block.source_range.start)
    });
    let executing_start = cx
        .global_mut::<EditorDocuments>()
        .preview(root)
        .ok()
        .map(|preview| preview.snapshot())
        .filter(|snapshot| matches!(snapshot.lifecycle, PreviewLifecycle::Running))
        .and_then(|snapshot| snapshot.runtime_position)
        .filter(|(path, _, _)| path == relative)
        .and_then(|(_, line, column)| {
            snapshot
                .block_at_position(line.saturating_sub(1), column.saturating_sub(1))
                .map(|block| {
                    if block.is_textbox_ending() {
                        block.lifetime_owner.unwrap_or(block.source_range.start)
                    } else {
                        block.source_range.start
                    }
                })
        });
    let root = root.to_owned();
    let relative = relative.to_owned();
    let mut rows = Vec::new();
    let mut overview = Vec::new();
    let mut overview_offset = 40.;
    for (scene_index, scene) in projection.scenes.iter().enumerate() {
        let collapsed = collapsed_scenes.contains(&scene.name);
        let scene_name = scene.name.clone();
        let context_name = scene.name.clone();
        let button_name = scene.name.clone();
        let scene_start = scene.source_range.start;
        let editing_scene = matches!(scene_edit, Some(SceneEditMode::Rename { start, .. }) if *start == scene_start);
        let scene_root = root.clone();
        let scene_relative = relative.clone();
        let collapse_progress = collapse_progresses[scene_index];
        let scene_line = source_lines.span(source, scene.name_range.start).line - 1;
        if geometry.is_none() {
            overview.push(minimap::Mark {
                top: overview_offset,
                height: 32.,
                depth: 0,
                width: 60.,
                color: PRIMARY,
                selected: selected_line == Some(scene_line)
                    || collapsed
                        && scene.blocks.iter().any(|block| {
                            selected_blocks.contains(&block.source_range.start)
                                || selected_start == Some(block.source_range.start)
                        }),
                error: collapsed && scene.blocks.iter().any(|block| block.read_only),
            });
        }
        overview_offset += 36.;
        let header = div()
            .id(("scene-section", scene_index))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .h(px(32.))
            .px_2()
            .rounded(px(7.))
            .bg(rgb(if selected_line == Some(scene_line) {
                SURFACE
            } else {
                PANEL
            }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(SURFACE_HOVER)))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if this.scene_edit.is_some() {
                    return;
                }
                if !this.collapsed_scenes.remove(&scene_name) {
                    this.collapsed_scenes.insert(scene_name.clone());
                }
                this.selected_blocks.clear();
                this.block_selection_anchor = None;
                cx.global_mut::<EditorDocuments>()
                    .clear_block_selection(&scene_root);
                set_authoring_selection(&scene_root, scene_relative.clone(), scene_line, 0, cx);
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_scene_context_menu(
                        scene_start,
                        context_name.clone(),
                        event.position,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .child(
                Icon::new(IconName::ChevronRight)
                    .xsmall()
                    .rotate(radians(collapse_progress * std::f32::consts::FRAC_PI_2))
                    .text_color(rgb(MUTED)),
            )
            .child(if editing_scene {
                div()
                    .flex_1()
                    .min_w_0()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        Input::new(scene_name_input)
                            .appearance(false)
                            .bordered(false)
                            .size_full()
                            .text_sm()
                            .text_color(rgb(INK)),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(rgb(INK))
                    .child(scene.name.clone())
                    .into_any_element()
            })
            .child(
                div()
                    .id(("scene-menu", scene_index))
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(5.))
                    .hover(|style| style.bg(rgb(SURFACE)))
                    .tooltip(icon_hint("Scene menu"))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.open_scene_context_menu(
                                scene_start,
                                button_name.clone(),
                                event.position,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .child(
                        Icon::new(AssetIconName::Ellipsis)
                            .xsmall()
                            .text_color(rgb(MUTED)),
                    ),
            )
            .into_any_element();
        let mut scene_rows = Vec::new();
        let mut scene_body_height = 0.;
        let mut hidden_height = 0.;
        let cached_scene = geometry
            .as_ref()
            .map(|geometry| &geometry.scenes[scene_index]);
        let expanded = if cached_scene.is_none() {
            scene
                .blocks
                .iter()
                .filter(|block| !block.is_textbox_ending())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let candidates = if let Some(cached) = cached_scene {
            let mut indices = visible
                .iter()
                .filter_map(|start| snapshot.lookup.get(start))
                .filter(|(index, _)| *index == scene_index)
                .filter_map(|(_, index)| cached.indices.binary_search(index).ok())
                .collect::<Vec<_>>();
            if collapsed {
                indices.clear();
            }
            indices.sort_unstable();
            indices
        } else {
            (0..expanded.len()).collect()
        };
        let mut consumed = 0;
        for block_index in candidates {
            let block = if let Some(cached) = cached_scene {
                let skipped = cached.prefix[block_index] - cached.prefix[consumed];
                scene_body_height += skipped;
                hidden_height += skipped;
                consumed = block_index + 1;
                &scene.blocks[cached.indices[block_index]]
            } else {
                expanded[block_index]
            };
            let row_height = cached_scene.map_or_else(
                || block_row_height(block, editors, draft_text, heights, cx),
                |cached| cached.prefix[block_index + 1] - cached.prefix[block_index] - 4.,
            );
            let row_top = overview_offset + scene_body_height * collapse_progress;
            // Use the same row height, draft/drop gaps and collapse transition as the main layout.
            let overview_row = overview.len();
            if geometry.is_none() && collapse_progress > 0. {
                overview.push(minimap::Mark::block(
                    block,
                    overview_offset + scene_body_height * collapse_progress,
                    row_height * collapse_progress,
                    selected_blocks.contains(&block.source_range.start)
                        || selected_start == Some(block.source_range.start),
                ));
            }
            let frozen_height = session
                .and_then(|session| session.row(block.source_range.start))
                .map_or(row_height, |row| row.height);
            let row_height = frozen_height;
            let position = session
                .and_then(|session| session.position(block.source_range.start, cx.reduce_motion()))
                .or_else(|| {
                    reorder_motion.and_then(|motion| {
                        motion
                            .positions
                            .get(&block.source_range.start)
                            .map(|origin| row_top + (origin - row_top) * (1. - motion_progress))
                    })
                })
                .unwrap_or(row_top);
            if geometry.is_none() {
                row_positions.insert(block.source_range.start, row_top);
            }
            if let Some(mark) = overview.get_mut(overview_row) {
                mark.top = position;
            }
            if !visible.contains(&block.source_range.start) {
                let height = row_height + 4.;
                scene_body_height += height;
                hidden_height += height;
                continue;
            }
            if hidden_height > 0. {
                scene_rows.push(
                    div()
                        .h(px((hidden_height - 4.).max(0.)))
                        .flex_none()
                        .into_any_element(),
                );
                hidden_height = 0.;
            }
            let block = block.clone();
            let row_id = block.source_range.start;
            let resource_errors = errors
                .range(block.source_range.clone())
                .map(|(_, message)| *message)
                .collect::<Vec<_>>()
                .join("\n");
            let line = block.line;
            let column = block.column;
            let root = root.clone();
            let relative = relative.clone();
            let source_root = root.clone();
            let source_relative = relative.clone();
            let visibility_root = root.clone();
            let visibility_path = relative.clone();
            let selected = selected_blocks.contains(&row_id) || selected_start == Some(row_id);
            let executing = executing_start == Some(row_id);
            let icon = block_card_icon(&block.kind, &block.summary);
            let label = block_card_label(&block.kind, &block.summary);
            let type_color = block_type_color(&block.kind, &block.summary);
            let text_state = block.text_range.as_ref().and_then(|range| {
                draft_text
                    .filter(|draft| {
                        draft
                            .text_range
                            .as_ref()
                            .is_some_and(|draft_range| draft_range.start == range.start)
                    })
                    .map(|draft| &draft.state)
                    .or_else(|| {
                        editors
                            .iter()
                            .find(|editor| editor.text_start == range.start)
                            .map(|editor| &editor.state)
                    })
            });
            let is_structure = matches!(
                &block.kind,
                BlockKind::Choice
                    | BlockKind::ChoiceOption
                    | BlockKind::Conditional
                    | BlockKind::ElseIf
                    | BlockKind::Else
                    | BlockKind::Loop
            );
            let source_summary = block_card_summary(&block.kind, &block.summary, block.line);
            let compact_key = block.summary.trim_start().starts_with("key(");
            let fields = block_fields.get(&row_id).cloned().unwrap_or_default();
            let headline =
                block_card_headline(&block.kind, &block.summary, &fields, source_summary.clone());
            let order = block_order.clone();
            let drag_selection = if selected_blocks.contains(&row_id) {
                selected_blocks.clone()
            } else {
                HashSet::from([row_id])
            };
            let drag_panel = cx.entity().downgrade();
            let drag_label = label.clone();
            let drag_summary = headline.clone();
            let drag_count = drag_selection.len();
            let block_indent = block.depth as f32 * 18.;
            let movable = !matches!(&block.kind, BlockKind::ElseIf | BlockKind::Else);
            let text_block = matches!(
                &block.kind,
                BlockKind::Narration | BlockKind::Dialogue { .. }
            );
            let payload = BlockDrag {
                selected: drag_selection.clone(),
                token: Rc::new(()),
                document: document.clone(),
                revision: document.borrow().revision(),
            };
            let grip = if movable {
                div()
                    .id(("block-grip", row_id))
                    .size(px(18.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_move()
                    .on_drag(payload, move |drag, _, window, cx| {
                        let (width, grip_height) = drag_panel
                            .update(cx, |panel, cx| {
                                let fallback_width = if compact_key {
                                    360.
                                } else {
                                    (f32::from(panel.view_scroll.bounds().size.width)
                                        - line_number_gutter
                                        - 16.
                                        - block_indent)
                                        .max(72.)
                                };
                                panel.begin_block_drag(
                                    drag,
                                    row_id,
                                    fallback_width,
                                    row_height,
                                    block_indent,
                                    window,
                                    cx,
                                )
                            })
                            .unwrap_or((300., row_height));
                        cx.new(|_| BlockDragPreview {
                            label: drag_label.clone(),
                            summary: drag_summary.clone(),
                            icon,
                            count: drag_count,
                            width,
                            height: grip_height,
                            color: type_color,
                            grip_top: (grip_height - 18.) * 0.5,
                        })
                    })
                    .child(
                        Icon::new(AssetIconName::GripVertical)
                            .xsmall()
                            .text_color(rgb(0x686e75)),
                    )
                    .into_any_element()
            } else {
                div().size(px(18.)).flex_none().into_any_element()
            };
            scene_body_height += row_height + 4.;
            let inline_control = inline
                .get(&row_id)
                .and_then(|control| render_inline_block(&root, control, cx));
            let mut row = div()
                .id(("block-row", scene_index * 10_000 + block_index))
                .relative()
                .w_full()
                .when(compact_key, |this| this.w(px(360.)).max_w_full())
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .min_h(px(row_height))
                .when(session.is_some(), |this| {
                    this.h(px(row_height)).overflow_hidden()
                })
                .px_2()
                .rounded(px(4.))
                .opacity(
                    if session.is_some_and(|session| session.moved.contains(&row_id)) {
                        0.
                    } else if block.disabled {
                        0.45
                    } else {
                        1.
                    },
                )
                .bg(rgb(if selected {
                    SURFACE
                } else if is_structure || text_block {
                    CANVAS
                } else {
                    PANEL
                }))
                .cursor_pointer()
                .anchor_scroll((selected_start == Some(row_id)).then(|| scroll_anchor.clone()))
                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    let modifiers = event.modifiers();
                    if modifiers.shift {
                        let anchor = this.block_selection_anchor.unwrap_or(row_id);
                        if let (Some(anchor_index), Some(row_index)) = (
                            order.iter().position(|candidate| *candidate == anchor),
                            order.iter().position(|candidate| *candidate == row_id),
                        ) {
                            let start = anchor_index.min(row_index);
                            let end = anchor_index.max(row_index);
                            this.selected_blocks.clear();
                            this.selected_blocks
                                .extend(order[start..=end].iter().copied());
                        }
                    } else if modifiers.platform || modifiers.control {
                        if !this.selected_blocks.remove(&row_id) {
                            this.selected_blocks.insert(row_id);
                        }
                        this.block_selection_anchor = Some(row_id);
                    } else {
                        this.selected_blocks.clear();
                        this.selected_blocks.insert(row_id);
                        this.block_selection_anchor = Some(row_id);
                    }
                    cx.global_mut::<EditorDocuments>().set_block_selection(
                        &root,
                        relative.clone(),
                        this.selected_blocks.iter().copied().collect(),
                    );
                    set_authoring_selection(&root, relative.clone(), line, column, cx);
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.open_block_context_menu(row_id, event.position, cx);
                    }),
                )
                .child(grip)
                .child(
                    div()
                        .absolute()
                        .left(px(-line_number_gutter))
                        .top_0()
                        .h_full()
                        .w(px(line_number_width))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .w_full()
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_end()
                                .pr(px(3.))
                                .whitespace_nowrap()
                                .text_size(px(10.))
                                .when(executing, |this| {
                                    this.font_weight(gpui_kit::FontWeight::BOLD)
                                })
                                .text_color(rgb(if executing {
                                    CHROME
                                } else if selected {
                                    PRIMARY
                                } else {
                                    MUTED
                                }))
                                .child((block_index + 1).to_string()),
                        ),
                );
            row = if let Some(state) = text_state.filter(|_| !block.read_only) {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .when(matches!(&block.kind, BlockKind::Dialogue { .. }), |this| {
                            this.child(
                                div()
                                    .flex_shrink_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_xs()
                                    .text_color(rgb(PRIMARY))
                                    .child(label),
                            )
                        })
                        .child(
                            div()
                                .min_h(px(30.))
                                .flex_1()
                                .min_w_0()
                                .child(render_block_text(state, editors, window, cx)),
                        ),
                )
            } else {
                let command = block.summary.split('(').next().unwrap_or_default().trim();
                let visible_fields = fields
                    .iter()
                    .filter(|field| {
                        field.insertion.is_none()
                            && field.key != "tween"
                            && inline.get(&row_id).is_none_or(|control| {
                                control.key.fields[control.position].key != field.key
                            })
                            && !((command == "track" && matches!(field.key.as_str(), "0" | "1"))
                                || (command == "key" && field.key == "time")
                                || (command == "stage.animate" && field.key == "0")
                                || (command == "text.retract"
                                    && matches!(field.key.as_str(), "source" | "keep"))
                                || (command.starts_with("event.") && field.key == "time")
                                || field.value == headline)
                    })
                    .take(4)
                    .map(|field| {
                        div()
                            .max_w(px(220.))
                            .min_w_0()
                            .flex_shrink_0()
                            .flex()
                            .gap_1()
                            .px_1()
                            .rounded(px(4.))
                            .bg(rgb(CANVAS))
                            .text_xs()
                            .child(div().text_color(rgb(MUTED)).child(command_field_label(
                                &block.kind,
                                command,
                                &field.key,
                            )))
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_color(rgb(INK))
                                    .child(field.value.clone()),
                            )
                    })
                    .collect::<Vec<_>>();
                let has_fields = !visible_fields.is_empty();
                let has_inline = inline_control.is_some();
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .py_1()
                        .child(
                            div()
                                .when(has_inline, |this| {
                                    this.flex_none().max_w(gpui_kit::relative(0.55))
                                })
                                .when(!has_inline, |this| this.flex_1())
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .h(px(24.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .px(px(6.))
                                        .rounded(px(3.))
                                        .bg(gpui_kit::rgba((type_color << 8) | 0x20))
                                        .text_size(px(13.))
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .text_color(rgb(if block.read_only && !block.disabled {
                                            0xd2aa62
                                        } else {
                                            type_color
                                        }))
                                        .child(Icon::new(icon).xsmall())
                                        .child(label),
                                )
                                .when(inline_control.is_none(), |this| {
                                    this.child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .text_sm()
                                            .text_color(rgb(
                                                if block.read_only && !block.disabled {
                                                    0xd2aa62
                                                } else {
                                                    INK
                                                },
                                            ))
                                            .child(headline),
                                    )
                                })
                                .when_some(inline_control, |this, control| this.child(control)),
                        )
                        .when(has_fields, |this| {
                            this.child(
                                div()
                                    .min_w_0()
                                    .max_w(gpui_kit::relative(0.55))
                                    .flex()
                                    .gap_1()
                                    .overflow_hidden()
                                    .children(visible_fields),
                            )
                        }),
                )
            };
            if text_block && !block.read_only {
                let hidden = textbox_endings.contains(&row_id);
                row = row.child(
                    div()
                        .id(("text-ending-toggle", row_id))
                        .size(px(24.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .rounded_full()
                        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                        .tooltip(icon_hint(if hidden {
                            "Keep textbox after this line"
                        } else {
                            "Hide textbox after this line"
                        }))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.focus.focus(window, cx);
                            this.toggle_text_ending(
                                &visibility_root,
                                &visibility_path,
                                row_id,
                                hidden,
                                window,
                                cx,
                            );
                        }))
                        .child(div().size(px(8.)).rounded_full().bg(rgb(if hidden {
                            PRIMARY
                        } else {
                            0x59616a
                        }))),
                );
            }
            if block.read_only && !block.disabled {
                row = row.child(
                    div()
                        .id(("open-source", row_id))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .hover(|style| style.bg(rgb(SURFACE)))
                        .tooltip(icon_hint("Open source"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.document_mode = DocumentMode::Text;
                            navigate_source(
                                &source_root,
                                &source_relative,
                                line + 1,
                                column + 1,
                                window,
                                cx,
                            );
                            cx.notify();
                        }))
                        .child(Icon::new(IconName::FileText).xsmall()),
                );
            }
            if !resource_errors.is_empty() {
                row = row.child(
                    div()
                        .id(("block-errors", row_id))
                        .flex_none()
                        .tooltip(icon_hint(resource_errors))
                        .child(
                            Icon::new(AssetIconName::TriangleAlert)
                                .xsmall()
                                .text_color(rgb(0xdb7780)),
                        ),
                );
            }
            let measured_bounds = row_bounds.clone();
            row = row.child(
                canvas(
                    move |bounds, _, _| {
                        measured_bounds.borrow_mut().insert(row_id, bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
            let displacement = position - row_top;
            let row_wrapper = div()
                .relative()
                .top(px(displacement))
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .flex_none()
                .on_drag_move(cx.listener(
                    move |this, event: &DragMoveEvent<AssetDrag>, window, cx| {
                        if event.bounds.contains(&event.event.position) {
                            this.track_asset_drag(event, row_id, window, cx);
                        }
                    },
                ))
                .on_drop(cx.listener(move |this, drag: &AssetDrag, window, cx| {
                    cx.stop_propagation();
                    this.drop_assets(drag, row_id, false, window, cx);
                }))
                .child(
                    div().pl(px(block_indent)).child(
                        div()
                            .relative()
                            .w_full()
                            .when(compact_key, |this| this.w(px(360.)).max_w_full())
                            .min_w_0()
                            .when(executing, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .left(px(-line_number_gutter))
                                        .w(px(line_number_gutter + 4.))
                                        .top_0()
                                        .h_full()
                                        .rounded_l(px(4.))
                                        .bg(rgb(PRIMARY)),
                                )
                            })
                            .child(row),
                    ),
                )
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(px(6.))
                        .on_drop(cx.listener(move |this, drag: &AssetDrag, window, cx| {
                            cx.stop_propagation();
                            this.drop_assets(drag, row_id, true, window, cx);
                        })),
                );
            if let Some(draft) = draft_text.filter(|draft| {
                matches!(draft.target, DraftInsertionTarget::Before(start) if start == row_id)
                    && draft.text_range.is_none()
            }) {
                scene_body_height += 42.;
                if let Some(mark) = overview.get_mut(overview_row) {
                    mark.top += 42. * collapse_progress;
                }
                scene_rows.push(draft_text_row(draft, block_indent, row_id));
            }
            scene_rows.push(row_wrapper.into_any_element());
            if let Some(draft) = draft_text.filter(|draft| {
                matches!(draft.target, DraftInsertionTarget::After(start) if start == row_id)
                    && draft.text_range.is_none()
            }) {
                scene_body_height += 42.
                    + (draft.state.read(cx).value().lines().count().clamp(1, 6) - 1) as f32 * 20.;
                scene_rows.push(draft_text_row(draft, block_indent, row_id));
            }
        }
        if let Some(cached) = cached_scene {
            let skipped = cached.prefix.last().unwrap() - cached.prefix[consumed];
            scene_body_height += skipped;
            hidden_height += skipped;
        }
        if hidden_height > 0. {
            scene_rows.push(
                div()
                    .h(px((hidden_height - 4.).max(0.)))
                    .flex_none()
                    .into_any_element(),
            );
        }
        if let Some(draft) = draft_text.filter(|draft| {
            matches!(
                draft.target,
                DraftInsertionTarget::SceneEnd(start) if start == scene.source_range.start
            ) && draft.text_range.is_none()
        }) {
            scene_body_height +=
                42. + (draft.state.read(cx).value().lines().count().clamp(1, 6) - 1) as f32 * 20.;
            scene_rows.push(draft_text_row(draft, 0., scene.source_range.start));
        }
        scene_body_height = cached_scene.map_or_else(
            || (scene_body_height - 4.).max(0.),
            |cached| cached.body_height,
        );
        if let Some(session) = session.filter(|session| {
            session.assets.is_some() && session.preview_top(cx.reduce_motion()).is_some()
        }) && session.target.is_some_and(|target| {
            scene
                .blocks
                .iter()
                .any(|block| block.source_range.start == target.row)
        }) {
            let gap = session.height + 4.;
            scene_body_height += gap;
            scene_rows.push(
                div()
                    .h(px((gap - 4.).max(0.)))
                    .flex_none()
                    .into_any_element(),
            );
        }
        overview_offset += scene_body_height * collapse_progress + 4.;
        rows.push(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .child(header)
                .child(
                    div()
                        .w_full()
                        .when(collapsed || collapse_progress < 1., |this| {
                            this.h(px(scene_body_height * collapse_progress))
                                .overflow_hidden()
                        })
                        .opacity(collapse_progress)
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .children(scene_rows),
                        ),
                )
                .into_any_element(),
        );
    }
    if matches!(scene_edit, Some(SceneEditMode::New)) {
        overview_offset += 36.;
        rows.push(
            div()
                .id("new-scene-row")
                .w_full()
                .h(px(32.))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .rounded(px(7.))
                .bg(rgb(SURFACE))
                .child(
                    Icon::new(AssetIconName::Plus)
                        .xsmall()
                        .text_color(rgb(PRIMARY)),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(scene_name_input)
                            .appearance(false)
                            .bordered(false)
                            .size_full()
                            .text_sm()
                            .text_color(rgb(INK)),
                    ),
                )
                .child(
                    file_action_icon("scene-add-confirm", AssetIconName::Check, "Add scene")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.commit_scene_edit(window, cx)),
                        ),
                )
                .child(
                    file_action_icon("scene-add-cancel", AssetIconName::Close, "Cancel").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.scene_edit = None;
                            cx.notify();
                        }),
                    ),
                )
                .into_any_element(),
        );
    }
    rows.extend(
        projection
            .read_only
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, card)| {
                overview.push(minimap::Mark {
                    top: overview_offset,
                    height: 30.,
                    depth: 0,
                    width: 48.,
                    color: 0xdb7780,
                    selected: false,
                    error: true,
                });
                overview_offset += 34.;
                div()
                    .id(("projection-diagnostic", index))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_h(px(30.))
                    .px_2()
                    .rounded(px(7.))
                    .bg(rgb(SURFACE))
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .xsmall()
                            .text_color(rgb(0xd2aa62)),
                    )
                    .child(div().text_xs().text_color(rgb(MUTED)).child(card.message))
                    .into_any_element()
            }),
    );
    if scroll_pending && selected_start.is_some() {
        scroll_anchor.scroll_to(window, cx);
    }
    let mut content = div()
        .id("eiyashou-block-content")
        .relative()
        .flex()
        .flex_col()
        .gap_1()
        .w_full()
        .min_w_0()
        .pl(px(line_number_gutter))
        .pr_2()
        .pt(px(40.))
        .pb(px(120.))
        .children(rows)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.selected_blocks.clear();
            this.block_selection_anchor = None;
            cx.global_mut::<EditorDocuments>()
                .clear_block_selection(&root);
            cx.notify();
        }));
    if let Some(session) = session
        && let Some(top) = session.preview_top(cx.reduce_motion())
    {
        content = content.child(
            div()
                .absolute()
                .top(px(top))
                .left(px(line_number_gutter + session.indent))
                .w(px(session.width))
                .h(px(session.height))
                .rounded(px(4.))
                .bg(rgb(SURFACE_HOVER))
                .opacity(0.5),
        );
    }
    content = content
        .on_drag_move(
            cx.listener(|this, event: &DragMoveEvent<AssetDrag>, _, cx| {
                if this
                    .block_drag
                    .session()
                    .is_some_and(|session| Rc::ptr_eq(&session.token, &event.drag(cx).token))
                {
                    this.update_block_drag_target(event.event.position, cx);
                }
            }),
        )
        .on_drop(cx.listener(|this, drag: &AssetDrag, window, cx| {
            this.finish_asset_drag(drag, window, cx);
            cx.stop_propagation();
        }))
        .on_drag_move(
            cx.listener(|this, event: &DragMoveEvent<BlockDrag>, _, cx| {
                this.track_block_drag(event, cx);
            }),
        )
        .on_drop(cx.listener(|this, drag: &BlockDrag, window, cx| {
            this.finish_block_drag(drag, window, cx);
            cx.stop_propagation();
        }));
    div()
        .size_full()
        .flex()
        .min_w_0()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(vertical_overflow_view(
                    "eiyashou-block-scroll",
                    scroll_handle,
                    content,
                )),
        )
        .child(minimap::render(
            geometry.as_ref().map_or_else(
                || Rc::new(overview),
                |geometry| {
                    layout.marks(
                        &snapshot,
                        geometry,
                        selected_blocks,
                        selected_line.map(|line| (line, selected_start.unwrap_or(usize::MAX))),
                    )
                },
            ),
            scroll_handle,
            minimap,
            cx,
        ))
        .into_any_element()
}

#[cfg(test)]
mod inline_wait_tests {
    use super::*;

    #[test]
    fn shortened_wait_labels_preserve_exact_source_selection() {
        let source = "前[wait=1000]後[wait=500]尾";
        let preview = inline_wait_preview(source, None);
        assert_eq!(preview.text, "前[Wait 1s]後[Wait 0.5s]尾");
        for (display, range) in &preview.waits {
            assert_eq!(preview.source_selection(display.start + 1), *range);
            assert!(source[range.clone()].starts_with("[wait="));
        }
        let suffix = preview.text.find('尾').unwrap();
        assert_eq!(
            preview.source_selection(suffix).start,
            source.find('尾').unwrap()
        );
        assert_eq!(preview.source_selection(0), 0..0);
        let invalid = inline_wait_preview("前[wait=bad]後", None);
        assert!(invalid.waits.is_empty());
        assert_eq!(invalid.text, "前[wait=bad]後");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_headlines_use_complete_nested_and_quoted_source_fields() {
        for (command, expected) in [
            (
                "screen.curtain.hide(color: rgba(0, 0, 0, 1), duration: 2200ms)",
                "rgba(0, 0, 0, 1)",
            ),
            (
                "screen.curtain.show(duration: 300ms, color: rgba(0.1, 0.2, 0.3, 1))",
                "rgba(0.1, 0.2, 0.3, 1)",
            ),
            ("sprite(hero, face, position: right(x: 20, y: 10))", "face"),
            ("move(hero, point(20, 10), duration: 1s)", "point(20, 10)"),
            (
                "text.float(\"你好，世界, hello\", font_size: 48)",
                "你好，世界, hello",
            ),
            ("camera.reset(all)", "all"),
            ("scene.parallax.stop()", ""),
        ] {
            let source = format!("scene a {{ {command} }}");
            let projection = EiyashouProjection::parse(&source);
            let block = &projection.scenes[0].blocks[0];
            let fields = projection.source_fields_for_block(&source, block).unwrap();
            let fallback = block_card_summary(&block.kind, &block.summary, block.line);
            assert_eq!(
                block_card_headline(&block.kind, &block.summary, &fields, fallback),
                expected,
                "{command}"
            );
        }
    }
}
