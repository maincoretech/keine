//! Audio asset calibration reuses source transactions and Engine audition.
use super::*;
use crate::workspace::audio_calibration::{self as audio, Measurement};
use keine_core::config::EiyashouAssetManifest;
use std::time::SystemTime;

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: SystemTime,
}
fn stamp(root: &Path, path: &Path) -> Result<Stamp, String> {
    let file = file_ops::confined_existing(root, path).map_err(|e| e.to_string())?;
    let meta = std::fs::metadata(file).map_err(|e| e.to_string())?;
    Ok(Stamp {
        size: meta.len(),
        modified: meta.modified().map_err(|e| e.to_string())?,
    })
}
#[derive(Default)]
pub(super) struct Calibration {
    running: Option<PathBuf>,
    result: Option<(PathBuf, Stamp, Measurement)>,
    error: Option<(PathBuf, String)>,
}
fn manifest(
    root: &Path,
    path: &Path,
    cx: &App,
) -> Result<(PathBuf, String, crate::authoring::AssetEntry), String> {
    let index = cx.global::<EditorDocuments>().authoring(root);
    let asset = index
        .assets
        .iter()
        .find(|a| {
            a.path == path
                && matches!(
                    a.kind,
                    AssetKind::Bgm | AssetKind::Voice | AssetKind::Effect
                )
        })
        .cloned()
        .ok_or("Add this file to the asset manifest before calibrating")?;
    let manifest = index
        .assets_manifest
        .clone()
        .ok_or("Asset manifest unavailable")?;
    let source = cx
        .global::<EditorDocuments>()
        .source(root, &manifest)
        .ok_or("Could not read the asset manifest")?;
    Ok((manifest, source, asset))
}
fn namespace(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Bgm => "bgm",
        AssetKind::Voice => "voices",
        _ => "se",
    }
}
pub(super) fn saved_gain(root: &Path, path: &Path, cx: &App) -> f32 {
    let Ok((_, source, asset)) = manifest(root, path, cx) else {
        return 0.0;
    };
    let Ok(manifest) = EiyashouAssetManifest::from_yaml(&source) else {
        return 0.0;
    };
    let entries = match asset.kind {
        AssetKind::Bgm => manifest.bgm,
        AssetKind::Voice => manifest.voices,
        _ => manifest.effects,
    };
    entries.get(&asset.id).map_or(0.0, |e| e.gain_db())
}

pub(super) fn render(
    root: &Path,
    asset: &AssetPreviewSelection,
    panel: &mut WorkbenchPanel,
    _window: &mut Window,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let path = asset.path.clone();
    let gain = saved_gain(root, &path, cx);
    let mut content = div()
        .w_full()
        .flex_shrink_0()
        .max_w(px(440.))
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .child(section_label("LOUDNESS"))
        .child(
            div()
                .text_sm()
                .child(format!("Applied gain: {gain:+.1} dB")),
        );
    if manifest(root, &path, cx).is_err() {
        return content
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("Add this file to the asset manifest to calibrate it."),
            )
            .into_any_element();
    }
    if let Some(running) = &panel.calibration.running {
        content = content.child(div().text_sm().text_color(rgb(MUTED)).child(format!(
            "Analyzing {}…",
            running.file_name().unwrap_or_default().to_string_lossy()
        )));
    } else {
        let root = root.to_owned();
        let path = path.clone();
        content = content.child(tool_action("Analyze loudness").on_click(cx.listener(
            move |panel, _, window, cx| {
                if panel.calibration.running.is_some() {
                    return;
                }
                panel.calibration.running = Some(path.clone());
                panel.calibration.error = None;
                let root = root.clone();
                let path = path.clone();
                let worker = cx.background_executor().spawn({
                    let root = root.clone();
                    let path = path.clone();
                    async move {
                        let before = stamp(&root, &path)?;
                        let result = audio::measure(&root, &path)?;
                        if stamp(&root, &path)? != before {
                            return Err("Audio changed; analyze it again".to_owned());
                        }
                        Ok((path, before, result))
                    }
                });
                cx.spawn_in(window, async move |this, cx| {
                    let result = worker.await;
                    let _ = this.update_in(cx, |panel, _, cx| {
                        panel.calibration.running = None;
                        match result {
                            Ok(result) => {
                                panel.calibration.result = Some(result);
                                panel.calibration.error = None;
                            }
                            Err(error) => panel.calibration.error = Some((path, error)),
                        }
                        cx.notify();
                    });
                })
                .detach();
                cx.notify();
            },
        )));
    }
    if let Some((_, measured_stamp, measurement)) = panel
        .calibration
        .result
        .as_ref()
        .filter(|(p, _, _)| p == &path)
    {
        let suggested = measurement.suggested();
        let loudness = measurement.integrated.map_or_else(
            || "No integrated reading".to_owned(),
            |i| format!("{i:.1} LUFS"),
        );
        content = content
            .child(
                div()
                    .text_sm()
                    .child(format!("{loudness} · {:.1} dBTP", measurement.true_peak)),
            )
            .child(
                div()
                    .text_sm()
                    .child(format!("Suggested gain: {suggested:+.1} dB")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("Target −18 LUFS; peak ceiling −1 dBTP. Adjust the final mix by ear."),
            );
        let mut buttons = div().flex().flex_wrap().gap_2();
        for (label, db) in [("Original", 0.0), ("Adjusted", suggested)] {
            let root = root.to_owned();
            let path = path.clone();
            let baseline = measured_stamp.clone();
            buttons = buttons.child(tool_action(label).on_click(cx.listener(
                move |panel, _, _, cx| {
                    if stamp(&root, &path).ok().as_ref() != Some(&baseline) {
                        panel.calibration.error =
                            Some((path.clone(), "Audio changed; analyze it again".into()));
                        cx.notify();
                        return;
                    }
                    if let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(&root) {
                        preview.restart_audition(path.clone(), db);
                    }
                },
            )));
        }
        let root = root.to_owned();
        let path = path.clone();
        let baseline = measured_stamp.clone();
        buttons = buttons.child(tool_action("Apply gain").on_click(cx.listener(
            move |panel, _, window, cx| {
                let result = if stamp(&root, &path).ok().as_ref() == Some(&baseline) {
                    apply(&root, &path, suggested, window, cx)
                } else {
                    Err("Audio changed; analyze it again".into())
                };
                panel.calibration.error = result.err().map(|e| (path.clone(), e));
                cx.refresh_windows();
            },
        )));
        content = content.child(buttons);
    }
    if gain != 0.0 {
        let root = root.to_owned();
        let path = path.clone();
        content = content.child(tool_action("Reset gain").on_click(cx.listener(
            move |panel, _, window, cx| {
                panel.calibration.error = apply(&root, &path, 0.0, window, cx)
                    .err()
                    .map(|e| (path.clone(), e));
                cx.refresh_windows();
            },
        )));
    }
    if let Some((_, message)) = panel.calibration.error.as_ref().filter(|(p, _)| p == &path) {
        content = content.child(
            div()
                .text_sm()
                .text_color(rgb(0xdb7780))
                .child(message.clone()),
        );
    }
    content
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child("Apply, then save. Original audio is unchanged."),
        )
        .into_any_element()
}
fn apply(
    root: &Path,
    path: &Path,
    gain: f32,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), String> {
    let (manifest_path, source, asset) = manifest(root, path, cx)?;
    let edited = audio::edit_gain(
        &source,
        namespace(asset.kind),
        &asset.id,
        &asset.path.to_string_lossy().replace('\\', "/"),
        gain,
    )?;
    edits::apply_checked_edits(
        root,
        &[(manifest_path.clone(), edited)],
        &BTreeMap::from([(manifest_path, source)]),
        window,
        cx,
    )?;
    if let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(root) {
        preview.stop_audition();
    }
    Ok(())
}
