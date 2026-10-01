use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName as AssetIconName;
use gpui_kit::base::input::RopeExt as _;
use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::base::{InteractiveElementExt as _, Placement, ResizeHandleContext, ScrollbarMode};
use gpui_kit::component::dock::{
    AnyDrag, BasePanelView, DockArea, DockAreaRenderer, DockContext, DockEvent, DockLayout,
    DockPlacement, DockSkin, DragPanel, DropIndicator, DropPlaceholderBounds, InsertTarget, NodeId,
    PanelHandle, PanelId, PanelState, PanelStyle, TabGroupContext, TabGroupRenderer, panel_handle,
};
use gpui_kit::component::input::{
    Editor, EditorState, Input, InputEvent, InputState, Position, Textarea, TextareaState,
};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::select::{SelectEvent, SelectItem, SelectState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    Disableable as _, Icon, IconName, Sizable as _, Theme, ThemeMode, WindowExt as _,
};
use gpui_kit::{
    Anchor, Animation, AnimationExt as _, AnyElement, AnyView, App, AppContext as _, Axis, Bounds,
    ClickEvent, ClipboardItem, Context, Div, DragMoveEvent, Element, Empty, Entity, ExternalPaths,
    Focusable, Hsla, InteractiveElement, IntoElement, KeyBinding, MouseButton, MouseDownEvent,
    ParentElement, Pixels, Point, PromptButton, PromptLevel, Render, ScrollAnchor, ScrollHandle,
    SharedString, Stateful, Styled, StyledImage as _, Subscription, WeakEntity, Window, actions,
    anchored, canvas, deferred, div, ease_out_quint, fill, hsla, img, linear_color_stop,
    linear_gradient, prelude::*, px, radians, rgb, size,
};

use crate::authoring::fields::{
    EFFECT_GROUPS, SourceNumber, asset_source_value, camera_field_tweens, camera_tween_field,
    source_asset_kind, source_effect_group, source_field_enabled, source_input_commit,
    source_input_value, source_number, source_property_group, toggle_camera_tween,
};
use crate::authoring::{
    AssetKey, AssetKind, AssetQuery, AssetSort, AuthoringIndex, AuthoringSelection, InsertKind,
    ProblemSeverity, append_character, append_scene, confined_existing_file, delete_scene,
    dialogues_for_source, escape_eiyashou_string, insert_statement, insertion_statement,
    move_scene, rename_scene, rename_scene_references, replace_dialogue_text, scene_references,
    valid_identifier,
};
use crate::document::DocumentHandle;
use crate::file_ops::{self, ImportResult};
use crate::instance::{Startup, acquire_or_forward};
use crate::persistence::{AppPersistence, BlockPickerPreferences};
use crate::preview::{PreviewController, PreviewLifecycle};
use crate::project_key::ProjectKey;
use crate::projection::{
    BlockKind, EiyashouProjection, MoveDirection, SourceField, TextBlockMetadata,
};
use crate::workspace::{WorkspaceEntryKind, WorkspaceFile, WorkspaceSession};

use documents::{AssetPreviewSelection, EditorDocuments, schedule_authoring_refresh};
use panel::{
    AssetDrag, AssetFilterChoice, AssetFilterGroup, AssetFilterMenu, BlockDrag, BlockDragPreview,
    BlockDropTarget, BlockMenuAction, BlockTextEditor, DocumentMode, DraftInsertionTarget,
    DraftTextBlock, FileContextMenu, FileDrag, FileEditMode, FileProgress, InlineWaitEdit,
    InspectorEditKey, PanelContent, PanelPayload, SceneContextMenu, SceneEditMode,
    SourceInspectorKey, WorkbenchPanel, language_for_path, register_workbench_panels,
};
use window::{EditorApp, EditorAppOwner, WorkbenchWindow, listen_for_secondary_launches};

mod blocks;
mod completion;
mod controls;
mod dock;
mod documents;
mod edits;
mod files;
mod inspector;
mod minimap;
mod panel;
mod performance;
mod render;
mod resource;
mod search;
#[path = "app/text/minimap.rs"]
mod text_minimap;
mod window;
use blocks::*;
use controls::*;
#[cfg(test)]
use dock::editor_drop_placement;
use edits::*;
use inspector::*;
use resource::*;

const CANVAS: u32 = 0x070809;
const CHROME: u32 = 0x0b0d10;
const PANEL: u32 = 0x101317;
const SURFACE: u32 = 0x191e23;
const SURFACE_HOVER: u32 = 0x222a31;
const BORDER: u32 = 0x2d343b;
const INK: u32 = 0xd8dadd;
const MUTED: u32 = 0x92979e;
const PRIMARY: u32 = 0xbaebff;
const PRIMARY_DIM: u32 = 0x243138;
const SUCCESS: u32 = 0x69c38d;
const LAYOUT_SCHEMA: usize = 2;

gpui_kit::assets::icon_assets!(
    EditorExtraIcons,
    [
        Braces,
        Clipboard,
        Clock,
        Film,
        GitBranch,
        GripVertical,
        Image,
        Images,
        ListFilter,
        MessageSquarePlus,
        MessageSquareText,
        Move,
        Music,
        PersonStanding,
        Repeat2,
        Scissors,
        SlidersHorizontal,
        Square,
        Volume2,
        Workflow,
    ]
);

struct EditorAssets;

impl gpui_kit::AssetSource for EditorAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        gpui_kit::AssetSource::load(&EditorExtraIcons, path).and_then(|extra| {
            extra.map_or_else(
                || gpui_kit::AssetSource::load(&gpui_kit::assets::Assets, path),
                |bytes| Ok(Some(bytes)),
            )
        })
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut paths = gpui_kit::AssetSource::list(&gpui_kit::assets::Assets, path)?;
        paths.extend(gpui_kit::AssetSource::list(&EditorExtraIcons, path)?);
        Ok(paths)
    }
}
// Each Dock group owns one half-gap. Adjacent groups therefore have a 4 px
// gutter, while the window and activity rail contribute the matching other
// half at the workspace edge. Keep this as the single spacing owner instead
// of adding per-panel margins.
const VIEW_INSET_PX: f32 = 2.;
const VIEW_RADIUS_PX: f32 = 14.;
const ACTIVITY_RAIL_WIDTH_PX: f32 = 44.;
const ACTIVITY_ITEM_SIZE_PX: f32 = 34.;
const ACTIVITY_BRAND_SIZE_PX: f32 = 32.;
const ACTIVITY_ICON_SIZE_PX: f32 = 18.;
// GPUI reserves one leading digit plus input padding before the widest visible
// line number. Crop that reserve while keeping the built-in right margin as a
// distinct dark gap before source text.
const EDITOR_GUTTER_TRIM_PX: f32 = 18.;
const TAB_MOTION_DURATION: Duration = Duration::from_millis(140);
const PREVIEW_SOURCE_DEBOUNCE: Duration = Duration::from_millis(100);
const FILE_CONTEXT_MENU_WIDTH_PX: f32 = 144.;
const SCENE_CONTEXT_MENU_WIDTH_PX: f32 = 154.;
const ASSET_FILTER_MENU_WIDTH_PX: f32 = 240.;
const SCENE_CONTEXT_MENU_HEIGHT_PX: f32 = 143.;

const EXPLORER_PANEL: &str = "keine.editor.explorer";
const SEARCH_PANEL: &str = "keine.editor.search";
const DOCUMENT_PANEL: &str = "keine.editor.document";
const INSPECTOR_PANEL: &str = "keine.editor.inspector";
const OUTPUT_PANEL: &str = "keine.editor.output";
const ASSETS_PANEL: &str = "keine.editor.assets";
const ASSET_PREVIEW_PANEL: &str = "keine.editor.asset_preview";
const CHARACTERS_PANEL: &str = "keine.editor.characters";
const SCENES_PANEL: &str = "keine.editor.scenes";
const PROBLEMS_PANEL: &str = "keine.editor.problems";
const PERFORMANCE_PANEL: &str = "keine.editor.performance";

actions!(
    keine_editor,
    [
        OpenFolder,
        ResetLayout,
        Save,
        SaveAll,
        ToggleEngine,
        ShowAssets,
        ShowSearch,
        ShowAssetPreview,
        MigrateEiyashou,
        CopyBlocks,
        PasteBlocks,
        DeleteBlocks,
        BeginTextBlock,
        ToggleBlockPicker,
        BlockPickerNext,
        BlockPickerPrevious,
        BlockPickerLeft,
        BlockPickerRight,
        AcceptBlockPicker,
        CloseBlockPicker,
        ResourcePickerNext,
        ResourcePickerPrevious,
        AcceptResourcePicker,
        CloseResourcePicker,
        MoveBlocksUp,
        MoveBlocksDown,
        UndoBlocks,
        RedoBlocks,
        UndoFiles,
        RedoFiles,
        UndoSources,
        RedoSources,
        ReloadDocument
    ]
);

fn theme_color(value: u32) -> Hsla {
    rgb(value).into()
}

fn configure_dark_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    let theme = Theme::global_mut(cx);
    theme.background = theme_color(CANVAS);
    theme.foreground = theme_color(INK);
    theme.border = theme_color(BORDER);
    theme.muted = theme_color(SURFACE);
    theme.muted_foreground = theme_color(MUTED);
    theme.accent = theme_color(PRIMARY_DIM);
    theme.accent_foreground = theme_color(PRIMARY);
    theme.primary = theme_color(PRIMARY);
    theme.primary_hover = theme_color(0xd6f3ff);
    theme.primary_active = theme_color(0x9dd7ee);
    theme.primary_foreground = theme_color(0x121a1e);
    theme.secondary = theme_color(SURFACE);
    theme.secondary_hover = theme_color(SURFACE_HOVER);
    theme.secondary_active = theme_color(0x202428);
    theme.secondary_foreground = theme_color(0xbfc3c7);
    theme.success = theme_color(SUCCESS);
    theme.success_foreground = theme_color(0x0b1b11);
    theme.warning = theme_color(0xd2aa62);
    theme.warning_foreground = theme_color(0x211707);
    theme.danger = theme_color(0xdb7780);
    theme.danger_foreground = theme_color(0x230d11);
    theme.info = theme_color(PRIMARY);
    theme.info_foreground = theme_color(0x121a1e);
    theme.ring = theme_color(PRIMARY);
    theme.drag_border = theme_color(PRIMARY);
    theme.drop_target = hsla(0.55, 0.38, 0.72, 0.22);
    theme.selection = theme_color(0x29353b);
    theme.sidebar = theme_color(CHROME);
    theme.sidebar_border = theme_color(BORDER);
    theme.sidebar_foreground = theme_color(0x9da2a8);
    theme.sidebar_accent = theme_color(PRIMARY_DIM);
    theme.sidebar_accent_foreground = theme_color(PRIMARY);
    theme.tab = theme_color(PANEL);
    theme.tab_bar = theme_color(CHROME);
    theme.tab_bar_segmented = theme_color(SURFACE);
    theme.tab_foreground = theme_color(0x8e9399);
    theme.tab_active = theme_color(SURFACE);
    theme.tab_active_foreground = theme_color(INK);
    theme.title_bar = theme_color(CHROME);
    theme.title_bar_border = theme_color(BORDER);
    theme.status_bar = theme_color(CHROME);
    theme.status_bar_border = theme_color(BORDER);
    theme.scrollbar_mode = ScrollbarMode::Scrolling;
    theme.scrollbar = hsla(0.58, 0.04, 0.10, 0.08);
    theme.scrollbar_thumb = hsla(0.56, 0.06, 0.56, 0.32);
    theme.scrollbar_thumb_hover = hsla(0.55, 0.14, 0.70, 0.48);
    theme.input = theme_color(BORDER);
    Arc::make_mut(&mut theme.highlight_theme)
        .style
        .editor_gutter_background = Some(theme_color(0x050607));
    theme.popover = theme_color(SURFACE);
    theme.popover_foreground = theme_color(INK);
    theme.button = theme_color(SURFACE);
    theme.button_hover = theme_color(SURFACE_HOVER);
    theme.button_active = theme_color(0x202428);
    theme.button_foreground = theme_color(0xc1c4c8);
    theme.radius = px(9.);
    theme.radius_lg = px(VIEW_RADIUS_PX);
    theme.shadow = false;
    Theme::sync_base(cx);
    // Dock edges retain their resize hit targets without painting a divider.
    let resize_theme = &mut gpui_kit::base::Theme::global_mut(cx).resizable;
    resize_theme.handle = Some(hsla(0., 0., 0., 0.));
    resize_theme.active_handle = Some(hsla(0., 0., 0., 0.));
}

struct IconHint(SharedString);

impl Render for IconHint {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(px(6.))
            .bg(rgb(SURFACE))
            .text_xs()
            .text_color(rgb(INK))
            .child(self.0.clone())
    }
}

fn icon_hint(
    label: impl Into<SharedString> + 'static,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label = label.into();
    move |_, cx| cx.new(|_| IconHint(label.clone())).into()
}

fn activity_tool(
    id: &'static str,
    icon: impl gpui_kit::assets::IconNamed,
    active: bool,
    hint: &'static str,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(ACTIVITY_ITEM_SIZE_PX))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .when(active, |style| style.bg(rgb(SURFACE)))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .tooltip(icon_hint(hint))
        .child(
            Icon::new(icon)
                .with_size(px(ACTIVITY_ICON_SIZE_PX))
                .text_color(rgb(if active { PRIMARY } else { MUTED })),
        )
}

fn activity_divider() -> Div {
    div().w(px(22.)).h(px(1.)).my(px(1.)).bg(rgb(BORDER))
}

fn file_action_icon(id: &'static str, icon: AssetIconName, hint: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(22.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .tooltip(icon_hint(hint))
        .child(Icon::new(icon).xsmall().text_color(rgb(MUTED)))
}

fn file_context_menu_item(
    id: &'static str,
    icon: AssetIconName,
    label: &'static str,
    danger: bool,
) -> Stateful<Div> {
    let color = if danger { 0xdb7780 } else { INK };
    div()
        .id(id)
        .h(px(27.))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .rounded(px(5.))
        .text_xs()
        .text_color(rgb(color))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .child(Icon::new(icon).xsmall().text_color(rgb(color)))
        .child(label)
}

fn short_error(error: &io::Error) -> String {
    error
        .to_string()
        .lines()
        .next()
        .unwrap_or("File operation failed")
        .to_owned()
}

fn open_workspace_document(root: &Path, relative: &Path, window: &mut Window, cx: &mut App) {
    let existing = cx.global::<EditorDocuments>().panel_for(root, relative);
    let Some((dock, document_node)) = cx.global::<EditorDocuments>().document_dock(root) else {
        return;
    };
    if let Some(panel) = existing {
        let _ = dock.update(cx, |dock, cx| dock.select_panel(panel, window, cx));
        return;
    }
    let panel = match WorkbenchPanel::from_payload(
        PanelPayload::Document {
            root: root.to_owned(),
            relative: relative.to_owned(),
        },
        window,
        cx,
    ) {
        Ok(panel) => panel,
        Err(error) => {
            cx.global_mut::<EditorDocuments>().set_notice(
                root,
                format!("Could not open {}: {error}", relative.display()),
            );
            cx.refresh_windows();
            return;
        }
    };
    let panel_id = PanelId::from(panel.entity_id());
    let documents = cx.global::<EditorDocuments>();
    let document_panels = documents.document_panels(root);
    let output = documents.tool_panel(root, OUTPUT_PANEL);
    let _ = dock.update(cx, |dock, cx| {
        let target = dock::document_insert_target(dock, document_node, &document_panels, output);
        dock.add_panel_view(panel_handle(panel), DockPlacement::Center, None, window, cx);
        dock.move_panel(panel_id, target, window, cx);
        dock.select_panel(panel_id, window, cx);
    });
}

#[derive(Debug, PartialEq, Eq)]
enum StartupArgs {
    Launch(Vec<PathBuf>),
    Help,
    Version,
}

fn parse_startup_args(args: &[OsString]) -> Result<StartupArgs, String> {
    if args.len() == 1 {
        if args[0] == "-h" || args[0] == "--help" {
            return Ok(StartupArgs::Help);
        }
        if args[0] == "-V" || args[0] == "--version" {
            return Ok(StartupArgs::Version);
        }
    }
    let mut paths = Vec::new();
    let mut positional = false;
    for argument in args {
        if !positional && argument == "--" {
            positional = true;
            continue;
        }
        if !positional && argument.to_string_lossy().starts_with('-') {
            return Err(format!(
                "unknown option {argument:?}; usage: editor [project ...]"
            ));
        }
        paths.push(PathBuf::from(argument));
    }
    Ok(StartupArgs::Launch(paths))
}

pub fn run() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let paths = match parse_startup_args(&args) {
        Ok(StartupArgs::Launch(paths)) => paths,
        Ok(StartupArgs::Help) => {
            println!(
                "Kēne Editor {}\n\nUsage: editor [project ...]",
                env!("CARGO_PKG_VERSION")
            );
            println!("Open the workbench or one window per project directory.");
            return ExitCode::SUCCESS;
        }
        Ok(StartupArgs::Version) => {
            println!("Kēne Editor {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let app_data = match crate::app_data::root() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Kēne Editor could not locate app data: {error}");
            return ExitCode::FAILURE;
        }
    };
    let instance = match acquire_or_forward(&app_data, paths.clone()) {
        Ok(Startup::Forwarded) => return ExitCode::SUCCESS,
        Ok(Startup::Primary(instance)) => instance,
        Err(error) => {
            eprintln!("Kēne Editor could not initialize its app instance: {error}");
            return ExitCode::FAILURE;
        }
    };
    let receiver = instance.receiver();
    gpui_kit::application()
        .with_assets(EditorAssets)
        .with_quit_mode(gpui_kit::QuitMode::LastWindowClosed)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            crate::syntax::install_editing_languages(cx);
            configure_dark_theme(cx);
            let persistence = AppPersistence::new(app_data.clone());
            cx.set_global(EditorDocuments::new(persistence.clone()));
            register_workbench_panels(cx);
            cx.bind_keys([
                KeyBinding::new("cmd-o", OpenFolder, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-o", OpenFolder, Some("KeineWorkbench")),
                KeyBinding::new("cmd-s", Save, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-s", Save, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-f", ShowSearch, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-shift-f", ShowSearch, Some("KeineWorkbench")),
                // Native Input binds cmd-shift-f to Replace. Override at its context depth.
                KeyBinding::new("cmd-shift-f", ShowSearch, Some("KeineWorkbench > Input")),
                KeyBinding::new("ctrl-shift-f", ShowSearch, Some("KeineWorkbench > Input")),
                KeyBinding::new("cmd-shift-s", SaveAll, Some("KeineWorkbench")),
                KeyBinding::new("cmd-alt-r", ReloadDocument, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-alt-r", ReloadDocument, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-shift-s", SaveAll, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-r", ToggleEngine, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-shift-r", ToggleEngine, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-m", MigrateEiyashou, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-shift-m", MigrateEiyashou, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-0", ResetLayout, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-shift-0", ResetLayout, Some("KeineWorkbench")),
                KeyBinding::new("cmd-c", CopyBlocks, Some("KeineBlockView")),
                KeyBinding::new("ctrl-c", CopyBlocks, Some("KeineBlockView")),
                KeyBinding::new("cmd-v", PasteBlocks, Some("KeineBlockView")),
                KeyBinding::new("ctrl-v", PasteBlocks, Some("KeineBlockView")),
                KeyBinding::new("enter", BeginTextBlock, Some("KeineBlockView")),
                KeyBinding::new("tab", ToggleBlockPicker, Some("KeineBlockView")),
                KeyBinding::new("escape", CloseBlockPicker, Some("KeineBlockView")),
                KeyBinding::new("down", BlockPickerNext, Some("KeineBlockPicker")),
                KeyBinding::new("up", BlockPickerPrevious, Some("KeineBlockPicker")),
                KeyBinding::new("left", BlockPickerLeft, Some("KeineBlockPicker")),
                KeyBinding::new("right", BlockPickerRight, Some("KeineBlockPicker")),
                KeyBinding::new("tab", CloseBlockPicker, Some("KeineBlockPicker")),
                KeyBinding::new("enter", AcceptBlockPicker, Some("KeineBlockPicker")),
                KeyBinding::new("escape", CloseBlockPicker, Some("KeineBlockPicker")),
                KeyBinding::new("down", ResourcePickerNext, Some("KeineResourcePicker")),
                KeyBinding::new("up", ResourcePickerPrevious, Some("KeineResourcePicker")),
                KeyBinding::new("enter", AcceptResourcePicker, Some("KeineResourcePicker")),
                KeyBinding::new("escape", CloseResourcePicker, Some("KeineResourcePicker")),
                KeyBinding::new("tab", CloseResourcePicker, Some("KeineResourcePicker")),
                KeyBinding::new("backspace", DeleteBlocks, Some("KeineBlockView")),
                KeyBinding::new("delete", DeleteBlocks, Some("KeineBlockView")),
                KeyBinding::new("alt-up", MoveBlocksUp, Some("KeineBlockView")),
                KeyBinding::new("alt-down", MoveBlocksDown, Some("KeineBlockView")),
                KeyBinding::new("cmd-z", UndoBlocks, Some("KeineBlockView")),
                KeyBinding::new("cmd-shift-z", RedoBlocks, Some("KeineBlockView")),
                KeyBinding::new("ctrl-z", UndoBlocks, Some("KeineBlockView")),
                KeyBinding::new("ctrl-y", RedoBlocks, Some("KeineBlockView")),
                KeyBinding::new("cmd-z", UndoFiles, Some("KeineExplorer")),
                KeyBinding::new("cmd-shift-z", RedoFiles, Some("KeineExplorer")),
                KeyBinding::new("ctrl-z", UndoFiles, Some("KeineExplorer")),
                KeyBinding::new("ctrl-y", RedoFiles, Some("KeineExplorer")),
                KeyBinding::new("cmd-z", UndoSources, Some("KeineWorkbench")),
                KeyBinding::new("cmd-shift-z", RedoSources, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-z", UndoSources, Some("KeineWorkbench")),
                KeyBinding::new("ctrl-y", RedoSources, Some("KeineWorkbench")),
            ]);
            let editor = cx.new(|cx| EditorApp::new(cx.weak_entity(), persistence, instance));
            cx.set_global(EditorAppOwner {
                _editor: editor.clone(),
            });
            let weak_editor = editor.downgrade();
            editor.update(cx, |editor, cx| {
                if paths.is_empty() {
                    editor.open_empty(cx);
                } else {
                    editor.open_paths(paths, cx);
                }
            });
            listen_for_secondary_launches(receiver, weak_editor, cx);
            cx.activate(true);
        });
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::point;

    #[test]
    fn editor_cli_handles_help_and_rejects_unknown_options_before_opening_a_window() {
        assert_eq!(
            parse_startup_args(&["--help".into()]).unwrap(),
            StartupArgs::Help
        );
        assert_eq!(
            parse_startup_args(&["-V".into()]).unwrap(),
            StartupArgs::Version
        );
        assert!(parse_startup_args(&["--unknown".into()]).is_err());
        assert_eq!(
            parse_startup_args(&["project-one".into(), "project-two".into()]).unwrap(),
            StartupArgs::Launch(vec!["project-one".into(), "project-two".into()])
        );
        assert_eq!(
            parse_startup_args(&["--".into(), "-project".into()]).unwrap(),
            StartupArgs::Launch(vec!["-project".into()])
        );
    }

    #[test]
    fn asset_range_and_toggle_selection_follow_visible_order() {
        let ordered = ["a", "b", "c"]
            .into_iter()
            .map(|id| AssetKey {
                kind: AssetKind::Background,
                id: id.into(),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            select_asset_keys(&[], &ordered, Some(&ordered[0]), &ordered[2], true, false),
            ordered
        );
        assert_eq!(
            select_asset_keys(&ordered, &ordered, None, &ordered[1], false, true),
            vec![ordered[0].clone(), ordered[2].clone()]
        );
        assert_eq!(
            select_asset_keys(&ordered, &ordered, None, &ordered[1], false, false),
            vec![ordered[1].clone()]
        );
    }

    #[test]
    fn asset_drop_is_bounded_and_voice_requires_text() {
        let source = "scene start {\n  \"Hello\",\n  wait(500ms)\n}\n";
        let projection = EiyashouProjection::parse(source);
        let blocks = &projection.scenes[0].blocks;
        let text_start = blocks[0].source_range.start;
        let command_start = blocks[1].source_range.start;
        let index = AuthoringIndex {
            assets: vec![
                crate::authoring::AssetEntry {
                    kind: AssetKind::Background,
                    id: "room".into(),
                    path: "assets/room.webp".into(),
                    tags: Vec::new(),
                    exists: true,
                    reference_count: 0,
                },
                crate::authoring::AssetEntry {
                    kind: AssetKind::Voice,
                    id: "hello".into(),
                    path: "assets/hello.ogg".into(),
                    tags: Vec::new(),
                    exists: true,
                    reference_count: 0,
                },
            ],
            ..Default::default()
        };
        let voice = index.assets[1].key();
        let background = index.assets[0].key();
        assert!(
            insert_assets_at_block(source, command_start, std::slice::from_ref(&voice), &index)
                .is_err()
        );
        assert!(
            insert_assets_at_block(
                source,
                text_start,
                &[voice.clone(), background.clone()],
                &index
            )
            .is_err()
        );
        assert!(
            insert_assets_at_block(source, text_start, &[voice], &index)
                .unwrap()
                .contains("\"Hello\", hello")
        );
        assert!(
            insert_assets_at_block(
                source,
                command_start,
                std::slice::from_ref(&background),
                &index
            )
            .is_err()
        );
        let edited = asset_drop_edit(source, command_start, &[background], &index, true).unwrap();
        assert!(edited.contains("background(room)"));
        assert!(edited.contains("wait(500ms)"));
        let changed = asset_drop_edit(
            source,
            command_start,
            &[index.assets[0].key(), index.assets[0].key()],
            &index,
            true,
        )
        .unwrap();
        assert_eq!(changed.as_str().matches("background(room)").count(), 1);
    }

    #[test]
    fn asset_rename_preflights_every_reference_before_emitting_edits() {
        let asset = crate::authoring::AssetEntry {
            kind: AssetKind::Background,
            id: "room".into(),
            path: "assets/room.webp".into(),
            tags: Vec::new(),
            exists: true,
            reference_count: 2,
        };
        let script_a = "scene a { background(room) }";
        let script_b = "scene b { background(room) }";
        let index = AuthoringIndex {
            assets_manifest: Some("assets.yaml".into()),
            assets: vec![asset.clone()],
            asset_references: ["scripts/a.shou", "scripts/b.shou"]
                .into_iter()
                .map(|path| crate::authoring::AssetReference {
                    key: asset.key(),
                    path: path.into(),
                    line: 1,
                    column: 22,
                    range: Some(21..25),
                })
                .collect(),
            ..Default::default()
        };
        let source_for = |path: &Path| match path.to_str()? {
            "assets.yaml" => Some("backgrounds:\n  room: assets/room.webp\n".into()),
            "scripts/a.shou" => Some(script_a.into()),
            "scripts/b.shou" => Some(script_b.into()),
            _ => None,
        };
        let edits = prepare_asset_edits(
            Path::new("."),
            &index,
            &asset,
            "hall",
            AssetKind::Background,
            &[],
            source_for,
        )
        .unwrap();
        assert_eq!(edits.len(), 3);
        assert!(
            edits
                .iter()
                .any(|(path, text)| path == Path::new("scripts/a.shou")
                    && text.contains("background(hall)"))
        );
        let mut incomplete = index.clone();
        incomplete.asset_references[1].range = None;
        assert!(
            prepare_asset_edits(
                Path::new("."),
                &incomplete,
                &asset,
                "hall",
                AssetKind::Background,
                &[],
                source_for
            )
            .is_err()
        );
        assert!(
            prepare_asset_edits(
                Path::new("."),
                &index,
                &asset,
                "room",
                AssetKind::Figure,
                &[],
                source_for
            )
            .is_err()
        );
    }

    #[test]
    fn empty_text_blocks_can_be_inserted_consecutively() {
        let source = "scene start {\n  \"Hello.\"\n}\n";
        let first = EiyashouProjection::parse(source).scenes[0].blocks[0]
            .source_range
            .start;
        let (source, inserted) = EiyashouProjection::parse(source)
            .insert_block_after(source, first, "\"\"")
            .unwrap();
        let (source, _) = EiyashouProjection::parse(&source)
            .insert_block_after(&source, inserted.start, "\"\"")
            .unwrap();
        assert_eq!(source.as_str().matches("\"\"").count(), 2);
        assert_eq!(EiyashouProjection::parse(&source).scenes[0].blocks.len(), 3);
    }

    #[test]
    fn editor_drop_zones_keep_a_large_merge_center() {
        let bounds = Bounds::new(point(px(100.), px(50.)), size(px(1000.), px(500.)));

        assert_eq!(
            editor_drop_placement(bounds, point(px(600.), px(300.))),
            None
        );
        assert_eq!(
            editor_drop_placement(bounds, point(px(200.), px(300.))),
            Some(Placement::Left)
        );
        assert_eq!(
            editor_drop_placement(bounds, point(px(1000.), px(300.))),
            Some(Placement::Right)
        );
        assert_eq!(
            editor_drop_placement(bounds, point(px(600.), px(100.))),
            Some(Placement::Top)
        );
        assert_eq!(
            editor_drop_placement(bounds, point(px(600.), px(500.))),
            Some(Placement::Bottom)
        );
    }

    #[test]
    fn editor_drop_corner_uses_the_nearest_edge() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(1000.), px(500.)));

        assert_eq!(
            editor_drop_placement(bounds, point(px(25.), px(80.))),
            Some(Placement::Left)
        );
        assert_eq!(
            editor_drop_placement(bounds, point(px(90.), px(15.))),
            Some(Placement::Top)
        );
    }

    #[test]
    fn picker_preferences_hide_browse_items_without_removing_search_access() {
        let preferences = BlockPickerPreferences {
            hidden: vec!["Video".into()],
            favorites: vec!["Wait".into()],
            ..BlockPickerPreferences::default()
        };
        assert!(!picker_kinds(&preferences, "", None, false).contains(&InsertKind::Video));
        assert!(picker_kinds(&preferences, "video", None, false).contains(&InsertKind::Video));
        assert_eq!(
            picker_kinds(&preferences, "", Some("Favorites"), false),
            vec![InsertKind::Wait]
        );
    }

    #[test]
    fn picker_icons_are_present_in_the_editor_asset_source() {
        for kind in InsertKind::ALL {
            let icon = insert_kind_icon(kind);
            assert!(
                gpui_kit::AssetSource::load(&EditorAssets, icon.path().as_ref())
                    .unwrap()
                    .is_some(),
                "missing icon for {}",
                kind.label()
            );
        }
    }

    #[test]
    fn preview_transport_icons_are_present_in_the_editor_asset_source() {
        for running in [false, true] {
            let icon = preview_transport_icon(running);
            assert!(
                gpui_kit::AssetSource::load(&EditorAssets, icon.path().as_ref())
                    .unwrap()
                    .is_some(),
                "missing Preview transport icon for running={running}"
            );
        }
    }

    #[test]
    fn picker_item_reorder_is_limited_to_its_category() {
        let universe = InsertKind::ALL
            .into_iter()
            .map(InsertKind::label)
            .collect::<Vec<_>>();
        let group = InsertKind::ALL
            .into_iter()
            .filter(|kind| kind.category() == "Text")
            .map(InsertKind::label)
            .collect::<Vec<_>>();
        let mut order = Vec::new();
        move_group_preference(&mut order, "Dialogue", &group, &universe, -1);
        assert!(
            preference_rank(&order, "Dialogue", usize::MAX / 2)
                < preference_rank(&order, "Narration", usize::MAX / 2)
        );
        assert!(order.contains(&"Background".to_owned()));
    }
}
