//! Build view: save, export a native playtest, then reveal or run the result.
use gpui_kit::PathPromptOptions;

use super::*;
use crate::workspace::build::{ExportedGame, export_game, launch_game};

#[derive(Default)]
pub(super) struct BuildState {
    running: bool,
    output: Option<ExportedGame>,
    error: Option<String>,
}

pub(super) fn render(
    root: &Path,
    scroll: &ScrollHandle,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let state = cx
        .global::<EditorDocuments>()
        .workspaces
        .get(root)
        .map(|workspace| &workspace.build);
    let running = state.is_some_and(|state| state.running);
    let output = state.and_then(|state| state.output.clone());
    let error = state.and_then(|state| state.error.clone());
    let native = root.join("config.yaml").is_file();
    let export_root = root.to_owned();
    let mut content = div().flex().flex_col().p_3().gap_3()
        .child(section_label("PLAYTEST"))
        .child(div().text_sm().text_color(rgb(INK)).child("Export a playable copy for this computer’s operating system."))
        .child(div().text_xs().text_color(rgb(MUTED)).child("Saves current edits. Includes readable scripts and registered resources. Formal releases use Hakutaku bundle."))
        .child(button("build-export", AssetIconName::Package, "Export game…", !running && native)
            .on_click(move |_, window, cx| {
                if !running && native { choose_export(export_root.clone(), window, cx); }
            }));
    if running {
        let icon = Icon::new(AssetIconName::RefreshCw)
            .small()
            .text_color(rgb(MUTED));
        let activity = if cx.reduce_motion() {
            icon.into_any_element()
        } else {
            icon.with_animation(
                "build-spinner",
                Animation::new(Duration::from_secs(2)).repeat(),
                |icon, delta| icon.rotate(radians(delta * std::f32::consts::TAU)),
            )
            .into_any_element()
        };
        content = content.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(activity)
                .child(div().text_xs().text_color(rgb(MUTED)).child("Exporting…")),
        );
    }
    if !native {
        content = content.child(
            div()
                .text_sm()
                .text_color(rgb(MUTED))
                .child("Migrate to Eiyashou before exporting."),
        );
    }
    if let Some(error) = error {
        content = content.child(
            div()
                .text_sm()
                .whitespace_normal()
                .text_color(rgb(0xf09090))
                .child(error),
        );
    }
    if let Some(game) = output {
        let reveal = game.directory.clone();
        let play = game.clone();
        let error_root = root.to_owned();
        let reveal_root = root.to_owned();
        content = content
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(SUCCESS))
                    .child("Export ready"),
            )
            .child(
                div()
                    .text_xs()
                    .whitespace_normal()
                    .text_color(rgb(MUTED))
                    .child(game.directory.display().to_string()),
            )
            .child(property_row(
                "Size",
                format!("{:.1} MiB", game.bytes as f64 / 1048576.),
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        button("build-play", AssetIconName::Play, "Play", !running).on_click(
                            move |_, _, cx| {
                                if !running && let Err(error) = launch_game(&play) {
                                    report_error(&error_root, error.to_string(), cx);
                                }
                            },
                        ),
                    )
                    .child(
                        button(
                            "build-reveal",
                            AssetIconName::FolderOpen,
                            "Open folder",
                            true,
                        )
                        .on_click(move |_, _, cx| {
                            if let Err(error) = files::open_in_file_manager(&reveal) {
                                report_error(&reveal_root, error.to_string(), cx);
                            }
                        }),
                    ),
            );
    }
    vertical_overflow_view("build-scroll", scroll, content)
}

fn button(
    id: &'static str,
    icon: AssetIconName,
    label: &'static str,
    enabled: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(px(7.))
        .bg(rgb(SURFACE))
        .text_sm()
        .text_color(rgb(if enabled { PRIMARY } else { MUTED }))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        })
        .child(Icon::new(icon).xsmall())
        .child(label)
}

fn choose_export(root: PathBuf, window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Export game here".into()),
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = receiver.await
            && let Some(parent) = paths.into_iter().next()
        {
            let _ = cx.update_window(handle, |_, window, cx| {
                start_export(&root, parent, window, cx)
            });
        }
    })
    .detach();
}

fn start_export(root: &Path, parent: PathBuf, window: &mut Window, cx: &mut App) {
    if cx
        .global::<EditorDocuments>()
        .workspaces
        .get(root)
        .is_none_or(|workspace| workspace.build.running)
    {
        return;
    }
    if let Err(error) = edits::format_and_save(root, window, cx) {
        report_error(root, error.to_string(), cx);
        return;
    }
    let engine = match crate::engine::EngineLocator::current().and_then(|locator| locator.locate())
    {
        Ok(engine) => engine,
        Err(error) => {
            report_error(root, error.to_string(), cx);
            return;
        }
    };
    let Some(workspace) = cx.global_mut::<EditorDocuments>().workspaces.get_mut(root) else {
        return;
    };
    workspace.build.running = true;
    workspace.build.error = None;
    // This also protects against asset conversion/rename during the snapshot.
    workspace.file_operation_active = true;
    let root = root.to_owned();
    let background = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        let export_root = root.clone();
        let result = background
            .spawn(async move { export_game(&export_root, &parent, &engine) })
            .await;
        cx.update(|cx| {
            if let Some(workspace) = cx.global_mut::<EditorDocuments>().workspaces.get_mut(&root) {
                workspace.file_operation_active = false;
                workspace.build.running = false;
                match result {
                    Ok(game) => {
                        workspace.build.output = Some(game);
                        workspace.build.error = None;
                    }
                    Err(error) => {
                        workspace.build.error = Some(format!("{error:#}"));
                    }
                }
            }
            cx.refresh_windows();
        });
    })
    .detach();
    cx.refresh_windows();
}

fn report_error(root: &Path, error: String, cx: &mut App) {
    if let Some(workspace) = cx.global_mut::<EditorDocuments>().workspaces.get_mut(root) {
        workspace.build.error = Some(error);
    }
    cx.refresh_windows();
}
