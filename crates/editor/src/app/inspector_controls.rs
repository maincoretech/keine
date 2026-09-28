use super::*;

pub(super) fn render_text_ending(
    root: &Path,
    key: &InspectorEditKey,
    inputs: &[Entity<InputState>],
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    if inputs.len() != 2 {
        return Empty.into_any_element();
    }
    let dialogue_root = root.to_owned();
    let character_root = root.to_owned();
    let target = inputs[0].clone();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_label("Ending"))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child("Keep dialogue"),
                )
                .child(
                    Switch::new("text-keep-dialogue")
                        .small()
                        .color(rgb(PRIMARY))
                        .checked(key.lifetime.text_box.is_none())
                        .accessibility_label("Keep dialogue")
                        .on_change(cx.listener(move |panel, keep: &bool, window, cx| {
                            panel.commit_text_lifetime(
                                &dialogue_root,
                                Some(*keep),
                                false,
                                window,
                                cx,
                            );
                        })),
                ),
        )
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child("Keep characters"),
                )
                .child(
                    Switch::new("text-keep-characters")
                        .small()
                        .color(rgb(PRIMARY))
                        .checked(key.lifetime.hide.is_none())
                        .accessibility_label("Keep characters")
                        .on_change(cx.listener(move |panel, keep: &bool, window, cx| {
                            if !keep && target.read(cx).value().trim().is_empty() {
                                target.update(cx, |target, cx| target.focus(window, cx));
                                cx.global_mut::<EditorDocuments>().set_notice(
                                    &character_root,
                                    "Enter a sprite ID or prefix* to hide on advance",
                                );
                                cx.refresh_windows();
                                return;
                            }
                            panel.commit_text_lifetime(&character_root, None, *keep, window, cx);
                        })),
                ),
        )
        .child(property_input("Hide target", &inputs[0]))
        .child(property_input("Transition", &inputs[1]))
        .into_any_element()
}

pub(super) fn run_source_block(root: &Path, path: &Path, start: usize, cx: &mut App) {
    let Some(source) = cx.global::<EditorDocuments>().source(root, path) else {
        return;
    };
    let projection = EiyashouProjection::parse(&source);
    let Some(block) = projection
        .scenes
        .iter()
        .flat_map(|scene| &scene.blocks)
        .find(|block| block.source_range.start == start && !block.read_only)
    else {
        return;
    };
    let documents = cx.global::<EditorDocuments>().preview_documents(root);
    let Ok(preview) = cx.global_mut::<EditorDocuments>().preview(root) else {
        return;
    };
    for (path, contents) in documents {
        preview.apply_snapshot(path, contents);
    }
    if !matches!(
        preview.snapshot().lifecycle,
        PreviewLifecycle::Running | PreviewLifecycle::Starting
    ) {
        preview.start();
    }
    preview.seek_cursor(path.to_owned(), block.line + 1, block.column + 1);
    preview.show();
    cx.refresh_windows();
}

// Shared by the source Inspector and inline command controls. The presentation
// follows Studio 2.0's NumberSliderField; the values stay in Eiyashou units.
#[derive(Clone, Copy)]
pub(super) struct SourceNumber {
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub default: f32,
    pub unit: &'static str,
}

impl SourceNumber {
    pub fn parse(self, source: &str) -> Option<f32> {
        let source = source.trim();
        let value = if self.unit == "ms" {
            if let Some(value) = source.strip_suffix("ms") {
                value.trim().parse().ok()?
            } else {
                source.strip_suffix('s')?.trim().parse::<f32>().ok()? * 1000.
            }
        } else {
            source.parse::<f32>().ok()? * if self.unit == "%" { 100. } else { 1. }
        };
        value.is_finite().then_some(value)
    }

    pub fn source(self, value: f32) -> String {
        if self.unit == "ms" {
            format!("{}ms", number_text(value))
        } else {
            number_text(value / if self.unit == "%" { 100. } else { 1. })
        }
    }
}

fn number_text(value: f32) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

// Slider ranges from Studio 2.0's effect detail controls. Native model
// defaults remain the single owner of neutral values.
fn effect_number(name: &str) -> Option<SourceNumber> {
    let defaults = keine_core::PostProcessEffect::default();
    let (min, max, step, default, unit) = match name {
        "mirror_shatter_intensity" => (
            0.0,
            100.0,
            1.0,
            defaults.v2.mirror_shatter_intensity * 100.,
            "%",
        ),
        "mirror_shatter_center_x" => (-1.0, 2.0, 0.01, defaults.v2.mirror_shatter_center_x, ""),
        "mirror_shatter_center_y" => (-1.0, 2.0, 0.01, defaults.v2.mirror_shatter_center_y, ""),
        "mirror_shatter_seed" => (0.0, 9999.0, 1.0, defaults.v2.mirror_shatter_seed, ""),
        "mirror_shatter_spread" => (0.0, 3.0, 0.05, defaults.v2.mirror_shatter_spread, ""),
        "eyelid_openness" => (0.0, 1.0, 0.01, defaults.eyelid_openness, ""),
        "eyelid_width" => (0.5, 2.0, 0.05, defaults.eyelid_width, ""),
        "eyelid_curvature" => (0.2, 2.0, 0.05, defaults.eyelid_curvature, ""),
        "eyelid_softness" => (0.0, 0.08, 0.005, defaults.eyelid_softness, ""),
        "eyelid_center_x" => (-1.0, 2.0, 0.01, defaults.eyelid_center_x, ""),
        "eyelid_center_y" => (-1.0, 2.0, 0.01, defaults.eyelid_center_y, ""),
        "godray_intensity" => (0.0, 1.0, 0.05, defaults.godray_intensity, ""),
        "godray_speed" => (-3.0, 3.0, 0.1, defaults.godray_speed, ""),
        "godray_gain" => (0.0, 1.0, 0.05, defaults.godray_gain, ""),
        "godray_lacunarity" => (1.0, 5.0, 0.1, defaults.godray_lacunarity, ""),
        "godray_center_x" => (-1.0, 2.0, 0.01, defaults.godray_center_x, ""),
        "godray_center_y" => (-1.0, 2.0, 0.01, defaults.godray_center_y, ""),
        "vignette_intensity" => (0.0, 1.0, 0.05, defaults.vignette_intensity, ""),
        "vignette_size" => (0.0, 1.0, 0.05, defaults.vignette_size, ""),
        "color_tone_intensity" => (0.0, 1.0, 0.05, defaults.color_tone_intensity, ""),
        "color_exposure" => (-2.0, 2.0, 0.05, defaults.color_exposure, ""),
        "color_brightness" => (-1.0, 1.0, 0.05, defaults.color_brightness, ""),
        "color_contrast" => (-1.0, 1.0, 0.05, defaults.color_contrast, ""),
        "color_saturation" => (0.0, 2.0, 0.05, defaults.color_saturation, ""),
        "color_temperature" => (-1.0, 1.0, 0.05, defaults.color_temperature, ""),
        "lut_intensity" => (0.0, 1.0, 0.05, defaults.lut_intensity, ""),
        "radial_blur_strength" => (-20.0, 20.0, 0.5, defaults.radial_blur_strength, "°"),
        "radial_blur_center_x" => (-1.0, 2.0, 0.01, defaults.radial_blur_center_x, ""),
        "radial_blur_center_y" => (-1.0, 2.0, 0.01, defaults.radial_blur_center_y, ""),
        "motion_blur_strength" => (0.0, 40.0, 0.5, defaults.motion_blur_strength, "px"),
        "motion_blur_angle" => (-180.0, 180.0, 1.0, defaults.motion_blur_angle, "°"),
        "zoom_blur_strength" => (-1.0, 1.0, 0.05, defaults.zoom_blur_strength, ""),
        "zoom_blur_center_x" => (-1.0, 2.0, 0.01, defaults.zoom_blur_center_x, ""),
        "zoom_blur_center_y" => (-1.0, 2.0, 0.01, defaults.zoom_blur_center_y, ""),
        "light_leak_intensity" => (0.0, 1.0, 0.05, defaults.light_leak_intensity, ""),
        "light_leak_angle" => (-180.0, 180.0, 1.0, defaults.light_leak_angle, "°"),
        "lens_flare_intensity" => (0.0, 1.0, 0.05, defaults.lens_flare_intensity, ""),
        "lens_flare_center_x" => (-1.0, 2.0, 0.01, defaults.lens_flare_center_x, ""),
        "lens_flare_center_y" => (-1.0, 2.0, 0.01, defaults.lens_flare_center_y, ""),
        "film_grain_intensity" => (0.0, 1.0, 0.05, defaults.film_grain_intensity, ""),
        "film_grain_size" => (1.0, 4.0, 0.1, defaults.film_grain_size, ""),
        "heat_haze_intensity" => (0.0, 1.0, 0.05, defaults.heat_haze_intensity, ""),
        "heat_haze_speed" => (-3.0, 3.0, 0.1, defaults.heat_haze_speed, ""),
        "heat_haze_scale" => (1.0, 10.0, 0.25, defaults.heat_haze_scale, ""),
        "water_ripple_intensity" => (0.0, 1.0, 0.05, defaults.water_ripple_intensity, ""),
        "water_ripple_frequency" => (1.0, 30.0, 0.5, defaults.water_ripple_frequency, ""),
        "water_ripple_speed" => (-3.0, 3.0, 0.1, defaults.water_ripple_speed, ""),
        "water_ripple_center_x" => (-1.0, 2.0, 0.01, defaults.water_ripple_center_x, ""),
        "water_ripple_center_y" => (-1.0, 2.0, 0.01, defaults.water_ripple_center_y, ""),
        "fog_intensity" => (0.0, 1.0, 0.05, defaults.fog_intensity, ""),
        "fog_speed" => (-1.0, 1.0, 0.05, defaults.fog_speed, ""),
        "fog_scale" => (1.0, 10.0, 0.25, defaults.fog_scale, ""),
        "vhs_intensity" => (0.0, 1.0, 0.05, defaults.vhs_intensity, ""),
        "vhs_jitter" => (0.0, 1.0, 0.05, defaults.vhs_jitter, ""),
        "vhs_noise" => (0.0, 1.0, 0.05, defaults.vhs_noise, ""),
        "halftone_intensity" => (0.0, 1.0, 0.05, defaults.halftone_intensity, ""),
        "halftone_scale" => (2.0, 20.0, 0.5, defaults.halftone_scale, "px"),
        "halftone_angle" => (-180.0, 180.0, 1.0, defaults.halftone_angle, "°"),
        "dither_intensity" => (0.0, 1.0, 0.05, defaults.dither_intensity, ""),
        "dither_levels" => (2.0, 16.0, 1.0, defaults.dither_levels, ""),
        "outline_intensity" => (0.0, 1.0, 0.05, defaults.outline_intensity, ""),
        "outline_thickness" => (1.0, 5.0, 0.5, defaults.outline_thickness, "px"),
        "speed_lines_intensity" => (0.0, 1.0, 0.05, defaults.v2.speed_lines_intensity, ""),
        "speed_lines_density" => (0.0, 1.0, 0.05, defaults.v2.speed_lines_density, ""),
        "speed_lines_speed" => (-5.0, 5.0, 0.1, defaults.v2.speed_lines_speed, ""),
        "speed_lines_region_feather" => {
            (0.0, 0.2, 0.01, defaults.v2.speed_lines_region_feather, "")
        }
        "distortion_strength" => (-1.0, 1.0, 0.05, defaults.distortion_strength, ""),
        "blur_amount" => (0.0, 20.0, 0.5, defaults.blur_amount, "px"),
        "old_film_intensity" => (0.0, 1.0, 0.05, defaults.old_film_intensity, ""),
        "shock_intensity" => (0.0, 1.0, 0.05, defaults.shock_intensity, ""),
        "bloom_intensity" => (0.0, 1.0, 0.05, defaults.bloom_intensity, ""),
        "chromatic_aberration" => (-20.0, 20.0, 0.5, defaults.chromatic_aberration, "px"),
        "pixelate_size" => (1.0, 64.0, 1.0, defaults.pixelate_size, "px"),
        "glitch_intensity" => (0.0, 1.0, 0.05, defaults.glitch_intensity, ""),
        "crt_intensity" => (0.0, 1.0, 0.05, defaults.crt_intensity, ""),
        "sharpen_strength" => (-1.0, 1.0, 0.05, defaults.sharpen_strength, ""),
        "focal_distance" => (0.1, 10., 0.1, 1., ""),
        "blur_strength" => (0., 1., 0.05, defaults.blur_strength, ""),
        "godray_angle" | "speed_lines_angle" => (-180., 180., 1., 0., "°"),
        "speed_lines_region_x" | "speed_lines_region_y" => (-1., 2., 0.01, 0.5, ""),
        "speed_lines_region_width" | "speed_lines_region_height" => (0.1, 3., 0.01, 1., ""),
        _ => return None,
    };
    Some(SourceNumber {
        min,
        max,
        step,
        default,
        unit,
    })
}

pub(super) fn source_number(key: &SourceInspectorKey, field: &SourceField) -> Option<SourceNumber> {
    let command = key.command.as_str();
    if matches!(
        command,
        "camera.effect" | "camera.effect.v2" | "event.camera.patch"
    ) && let Some(control) = effect_number(&field.key)
    {
        return (field.value.is_empty() || control.parse(&field.value).is_some())
            .then_some(control);
    }
    if field.key == "hold" && command == "text.intro" {
        return None;
    }
    let name = field.key.rsplit('.').next().unwrap_or(&field.key);
    let name = name.strip_prefix("transform_").unwrap_or(name);
    let (min, max, step, default, unit) = match name {
        "volume" => (0., 100., 1., 100., "%"),
        "duration" | "fade" | "fade_in" | "fade_out" | "hold" | "time" | "reveal_duration" => {
            (0., 5000., 100., 0., "ms")
        }
        "0" if command == "wait" => (0., 5000., 100., 1000., "ms"),
        "x" | "y" if command == "camera.move" => (-1000., 1000., 1., 0., "px"),
        "x" | "y" | "anchor_offset" | "layout_x" | "layout_y" => (-1920., 1920., 1., 0., "px"),
        "rotation" | "angle" => (-180., 180., 1., 0., "°"),
        "scale" | "scale_x" | "scale_y" => (0.1, 3., 0.01, 1., "×"),
        "alpha" => (0., 100., 1., 100., "%"),
        "blur" | "blur_amount" | "blur_strength" => (0., 30., 0.1, 0., ""),
        "amplitude" => (0., 60., 1., 0., "px"),
        "amplitude_randomness" | "frequency_randomness" => (0., 100., 5., 0., "%"),
        "frequency" => (0., 30., 0.5, 12., "Hz"),
        "fps" => (1., 60., 1., 12., "fps"),
        "font_size" => (8., 128., 1., 32., "px"),
        "count" => (1., 1000., 1., 100., ""),
        "playback_rate" => (0.1, 4., 0.1, 1., "×"),
        "repeat" => (0., 20., 1., 0., ""),
        "brightness" | "contrast" | "saturation" => (0., 2., 0.01, 1., ""),
        _ => return None,
    };
    let control = SourceNumber {
        min,
        max,
        step,
        default,
        unit,
    };
    // Expressions keep their precise source editor instead of being coerced to
    // constants by a slider. Empty optional fields remain unwritten until changed.
    (field.value.is_empty() || control.parse(&field.value).is_some()).then_some(control)
}

// The typed core inventory is shared by the native parser and these controls.
fn camera_tween_field(key: &SourceInspectorKey, field: &SourceField) -> bool {
    let Some(numeric) = keine_core::CameraTweenField::from_name(&field.key) else {
        return false;
    };
    match key.command.as_str() {
        "camera.move" => numeric.is_transform(),
        "camera.effect" => !numeric.is_transform() && !numeric.is_v2(),
        "camera.effect.v2" => numeric.is_v2(),
        _ => false,
    }
}

fn camera_field_tweens(key: &SourceInspectorKey, name: &str) -> bool {
    key.fields
        .iter()
        .find(|field| field.key == "tween" && field.insertion.is_none())
        .is_none_or(|field| {
            field
                .value
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .any(|value| value.trim() == name)
        })
}

fn toggle_camera_tween(key: &SourceInspectorKey, name: &str) -> String {
    let explicit = key
        .fields
        .iter()
        .any(|field| field.key == "tween" && field.insertion.is_none());
    let enabled = camera_field_tweens(key, name);
    let selected = key
        .fields
        .iter()
        .filter(|field| camera_tween_field(key, field))
        .filter(|field| {
            if field.key == name {
                !enabled
            } else if explicit {
                camera_field_tweens(key, &field.key)
            } else {
                field.insertion.is_none()
            }
        })
        .map(|field| field.key.as_str())
        .collect::<Vec<_>>();
    format!("[{}]", selected.join(", "))
}

pub(super) fn source_input_value(key: &SourceInspectorKey, field: &SourceField) -> String {
    source_number(key, field)
        .and_then(|control| control.parse(&field.value))
        .map(number_text)
        .unwrap_or_else(|| field.value.clone())
}

pub(super) fn source_input_commit(
    key: &SourceInspectorKey,
    field: &SourceField,
    draft: String,
) -> String {
    if draft == source_input_value(key, field) {
        return field.value.clone();
    }
    source_number(key, field)
        .and_then(|control| {
            let value = draft.trim().parse::<f32>().ok()?;
            value.is_finite().then(|| control.source(value))
        })
        .unwrap_or(draft)
}

pub(super) fn segmented_source_property(
    root: &Path,
    key: &SourceInspectorKey,
    position: usize,
    cx: &mut Context<WorkbenchPanel>,
) -> Option<AnyElement> {
    let field = &key.fields[position];
    let default = match field.key.as_str() {
        "axis" => "both",
        "falloff" => "linear",
        "position" => "center",
        _ => return None,
    };
    let choices = source_field_choices(key, field);
    if !field.value.is_empty() && !choices.contains(&field.value.as_str()) {
        return None;
    }
    Some(
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(source_field_label(key, field)),
            )
            .child(div().flex().gap_1().children(choices.iter().map(|choice| {
                let root = root.to_owned();
                let key = key.clone();
                let choice = *choice;
                let active = if field.value.is_empty() {
                    choice == default
                } else {
                    choice == field.value
                };
                div()
                    .id(format!(
                        "source-segment-{}-{}-{choice}",
                        key.block_start, field.key
                    ))
                    .flex_1()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(rgb(if active { PRIMARY } else { BORDER }))
                    .bg(rgb(if active { PRIMARY_DIM } else { CANVAS }))
                    .text_size(px(11.))
                    .text_color(rgb(if active { PRIMARY } else { MUTED }))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.commit_source_field(
                            &root,
                            &key,
                            position,
                            choice.to_owned(),
                            window,
                            cx,
                        );
                    }))
                    .child(if choice == "both" {
                        "XY".to_owned()
                    } else {
                        title_case(choice)
                    })
            })))
            .into_any_element(),
    )
}

pub(super) fn asset_source_value(field: &SourceField, value: &str) -> String {
    if !field.quoted && !valid_identifier(value) {
        format!("\"{}\"", escape_eiyashou_string(value))
    } else {
        value.to_owned()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct SourceOption {
    pub value: String,
    pub title: SharedString,
    pub asset: Option<(PathBuf, AssetKind, PathBuf)>,
}

impl SelectItem for SourceOption {
    type Value = String;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &String {
        &self.value
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.title.to_lowercase().contains(&query)
            || self.value.to_lowercase().contains(&query)
            || self
                .asset
                .as_ref()
                .is_some_and(|(_, _, path)| path.to_string_lossy().to_lowercase().contains(&query))
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some((root, kind, path)) = &self.asset else {
            return self.title.clone().into_any_element();
        };
        let image = matches!(kind, AssetKind::Background | AssetKind::Figure)
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    gpui_kit::Img::extensions()
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(ext))
                });
        let preview = if image && let Some(file) = confined_existing_file(root, path) {
            img(file)
                .size_full()
                .with_fallback(|| Empty.into_any_element())
                .into_any_element()
        } else {
            Icon::new(match kind {
                AssetKind::Background | AssetKind::Figure => AssetIconName::Image,
                AssetKind::Video => AssetIconName::Video,
                _ => AssetIconName::Music,
            })
            .small()
            .into_any_element()
        };
        let audio = matches!(kind, AssetKind::Voice | AssetKind::Bgm | AssetKind::Effect);
        div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(40.))
                    .h(px(30.))
                    .flex_shrink_0()
                    .rounded(px(3.))
                    .bg(rgb(CANVAS))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(preview),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(11.))
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(self.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(rgb(MUTED))
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(path.display().to_string()),
                    ),
            )
            .when(audio, |this| {
                this.child(audition_control(root, path, true, cx))
            })
            .into_any_element()
    }
}

pub(super) fn source_asset_kind(
    key: &SourceInspectorKey,
    field: &SourceField,
) -> Option<AssetKind> {
    match (key.command.as_str(), field.key.as_str()) {
        ("background", "0") => Some(AssetKind::Background),
        ("sprite" | "sprite.update", "1")
        | ("avatar.show", "0")
        | ("frame", "0")
        | ("case", "1") => Some(AssetKind::Figure),
        ("track", "image") if character_track(key) => Some(AssetKind::Figure),
        ("sprite.select" | "sprite.select.when", "default") => Some(AssetKind::Figure),
        ("layer", "1") => Some(AssetKind::Background),
        ("event.audio", "2") => match key
            .fields
            .iter()
            .find(|field| field.key == "1")?
            .value
            .as_str()
        {
            "bgm" => Some(AssetKind::Bgm),
            "effect" => Some(AssetKind::Effect),
            "vocal" => Some(AssetKind::Voice),
            _ => None,
        },
        ("resource", "0") => match key
            .fields
            .iter()
            .find(|field| field.key == "kind")?
            .value
            .as_str()
        {
            "background" => Some(AssetKind::Background),
            "figure" => Some(AssetKind::Figure),
            _ => None,
        },
        ("bgm", "0") => Some(AssetKind::Bgm),
        ("se", "0") | ("se.loop", "1") => Some(AssetKind::Effect),
        ("vocal.play", "0") => Some(AssetKind::Voice),
        ("video", "0") | ("video.play", "1") => Some(AssetKind::Video),
        _ => None,
    }
}

fn character_track(key: &SourceInspectorKey) -> bool {
    key.fields
        .iter()
        .find(|field| field.key == "0")
        .is_some_and(|field| {
            field
                .value
                .split_once('(')
                .is_some_and(|(target, _)| target.trim() == "character")
        })
}

pub(super) fn source_options(
    root: &Path,
    key: &SourceInspectorKey,
    field: &SourceField,
    cx: &App,
) -> Vec<SourceOption> {
    if source_number(key, field).is_some() {
        return Vec::new();
    }
    let mut options = if key.command == "track" && field.key == "1" {
        keine_loader::native_stage_property_names()
            .map(|name| SourceOption {
                value: name.into(),
                title: title_case(&name.replace('_', " ")).into(),
                asset: None,
            })
            .collect()
    } else if key.command == "track" && field.key == "0" {
        let mut targets = vec![("camera".to_owned(), "Camera".to_owned())];
        if let Some(source) = cx.global::<EditorDocuments>().source(root, &key.path) {
            let projection = EiyashouProjection::parse(&source);
            for block in projection
                .scenes
                .iter()
                .flat_map(|scene| &scene.blocks)
                .filter(|block| !block.disabled)
            {
                let command = block.summary.split('(').next().unwrap_or("").trim();
                let kind = match command {
                    "sprite" | "sprite.update" => "character",
                    "layer" => "scene_layer",
                    _ => continue,
                };
                if let Some(fields) = projection.source_fields_for_block(&source, block)
                    && let Some(id) = fields.iter().find(|field| field.key == "0")
                {
                    let Ok(identifier) = serde_json::to_string(&id.value) else {
                        continue;
                    };
                    targets.push((format!("{kind}({identifier})"), id.value.clone()));
                }
            }
        }
        targets.sort();
        targets.dedup_by(|left, right| left.0 == right.0);
        targets
            .into_iter()
            .map(|(value, title)| SourceOption {
                value,
                title: title.into(),
                asset: None,
            })
            .collect()
    } else if let Some(kind) = source_asset_kind(key, field) {
        cx.global::<EditorDocuments>()
            .authoring_ref(root)
            .into_iter()
            .flat_map(|index| &index.assets)
            .filter(|asset| asset.kind == kind)
            .map(|asset| SourceOption {
                value: asset.id.clone(),
                title: asset
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&asset.id)
                    .to_owned()
                    .into(),
                asset: Some((root.to_owned(), asset.kind, asset.path.clone())),
            })
            .collect::<Vec<_>>()
    } else {
        source_field_choices(key, field)
            .iter()
            .filter(|value| !matches!(**value, "true" | "false"))
            .map(|value| SourceOption {
                value: (*value).to_owned(),
                title: title_case(&value.replace('_', " ")).into(),
                asset: None,
            })
            .collect::<Vec<_>>()
    };
    let default = match field.key.as_str() {
        "easing" | "falloff" => Some("linear"),
        "axis" => Some("both"),
        "position" => Some("center"),
        "layout" => Some("natural"),
        "layout_fit" | "fit" => Some("contain"),
        "blend" => Some("alpha"),
        "mode" if key.command == "video.play" => Some("fullscreen"),
        _ => None,
    };
    if field.value.is_empty()
        && let Some(default) = default
    {
        options.insert(
            0,
            SourceOption {
                value: String::new(),
                title: title_case(default).into(),
                asset: None,
            },
        );
    }
    if !options.is_empty()
        && !field.value.is_empty()
        && !options.iter().any(|option| option.value == field.value)
    {
        options.insert(
            0,
            SourceOption {
                value: field.value.clone(),
                title: field.value.clone().into(),
                asset: None,
            },
        );
    }
    options
}

fn retraction_property(
    key: &SourceInspectorKey,
    field: &SourceField,
    input: &Entity<TextareaState>,
) -> AnyElement {
    let label = source_field_label(key, field);
    let hint = if field.key == "source" {
        "Empty uses the current dialogue."
    } else {
        "A prefix of the full text. Empty erases the entire line."
    };

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(MUTED))
                .child(label.clone()),
        )
        .child(
            div()
                .w_full()
                .min_h(px(34.))
                .px_2()
                .py_1()
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(CANVAS))
                .child(
                    Textarea::new(input)
                        .aria_label(label)
                        .appearance(false)
                        .bordered(false)
                        .w_full()
                        .px_0()
                        .py_0()
                        .text_size(px(13.))
                        .text_color(rgb(INK)),
                ),
        )
        .child(div().text_size(px(10.)).text_color(rgb(MUTED)).child(hint))
        .into_any_element()
}

pub(super) fn typed_source_property(
    key: &SourceInspectorKey,
    position: usize,
    input: &Entity<InputState>,
    slider: Option<&Entity<SliderState>>,
    select: Option<&Entity<SelectState<Vec<SourceOption>>>>,
) -> Option<AnyElement> {
    let field = &key.fields[position];
    let enabled = source_field_enabled(key, field);
    let label = source_field_label(key, field);
    if let Some(state) = select {
        return Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child(label.clone()),
                )
                .child(
                    Select::new(state)
                        .disabled(!enabled)
                        .small()
                        .w_full()
                        .accessibility_label(label)
                        .placeholder("Select…")
                        .menu_max_h(px(320.)),
                )
                .into_any_element(),
        );
    }
    let control = source_number(key, field)?;
    let state = slider?;
    Some(
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(label))
            .child(
                div()
                    .h(px(28.))
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(64.))
                            .h(px(26.))
                            .flex_shrink_0()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(CANVAS))
                            .px_2()
                            .child(
                                Input::new(input)
                                    .disabled(!enabled)
                                    .appearance(false)
                                    .bordered(false)
                                    .size_full()
                                    .px_0()
                                    .py_0()
                                    .text_align(gpui_kit::TextAlign::Right)
                                    .text_size(px(12.))
                                    .text_color(rgb(INK)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(22.))
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(control.unit),
                    )
                    .child(
                        div().flex_1().min_w_0().px_2().child(
                            Slider::new(state)
                                .disabled(!enabled)
                                .bg(rgb(PRIMARY))
                                .text_color(rgb(INK)),
                        ),
                    ),
            )
            .into_any_element(),
    )
}

// Studio's effect workspace uses an effect list and a single detail pane. These
// groups contain only the effects already exposed by the native action schema.
const EFFECT_GROUPS: &[(&str, &str)] = &[
    ("eyelid", "Eyelid"),
    ("godray", "God rays"),
    ("distortion", "Distortion"),
    ("vignette", "Vignette"),
    ("blur_amount", "Blur"),
    ("color_", "Color tone"),
    ("old_film", "Old film"),
    ("shock", "Shock"),
    ("mirror_shatter", "Mirror shatter"),
    ("lut_", "LUT"),
    ("bloom", "Bloom"),
    ("chromatic", "Chromatic aberration"),
    ("pixelate", "Pixelate"),
    ("glitch", "Glitch"),
    ("crt_", "CRT"),
    ("sharpen", "Sharpen"),
    ("radial_blur", "Radial blur"),
    ("motion_blur", "Motion blur"),
    ("speed_lines", "Speed lines"),
    ("zoom_blur", "Zoom blur"),
    ("light_leak", "Light leak"),
    ("lens_flare", "Lens flare"),
    ("film_grain", "Film grain"),
    ("heat_haze", "Heat haze"),
    ("water_ripple", "Water ripple"),
    ("fog_", "Fog"),
    ("vhs_", "VHS"),
    ("halftone", "Halftone"),
    ("dither", "Dither"),
    ("outline", "Outline"),
    ("focal_distance", "Depth of field"),
];

fn source_effect_group(name: &str) -> Option<(&'static str, &'static str)> {
    let name = if name == "blur_strength" {
        "focal_distance"
    } else {
        name
    };
    EFFECT_GROUPS
        .iter()
        .copied()
        .find(|(prefix, _)| name.starts_with(prefix))
}

pub(super) fn source_field_enabled(key: &SourceInspectorKey, field: &SourceField) -> bool {
    if !matches!(
        key.command.as_str(),
        "camera.effect" | "camera.effect.v2" | "event.camera.patch"
    ) {
        return true;
    }
    let Some((prefix, _)) = source_effect_group(&field.key) else {
        return true;
    };
    key.fields.iter().any(|field| {
        field.insertion.is_none()
            && source_effect_group(&field.key).is_some_and(|(group, _)| group == prefix)
    })
}

fn source_property_group(field: &SourceField) -> &'static str {
    let name = field.key.as_str();
    if name.starts_with("speaking.") {
        return "Speaking";
    }
    if name.starts_with("others.") {
        return "Other characters";
    }
    if name.starts_with("narration.") {
        return "Narration";
    }
    if matches!(
        name,
        "duration" | "easing" | "blocking" | "time" | "fade" | "fade_in" | "fade_out"
    ) {
        "Timing"
    } else if name.starts_with("layout_") || name == "layout" {
        "Layout"
    } else if name.starts_with("transform_")
        || matches!(
            name,
            "x" | "y"
                | "alpha"
                | "scale"
                | "scale_x"
                | "scale_y"
                | "rotation"
                | "width"
                | "height"
                | "anchor_offset"
        )
    {
        "Transform"
    } else if matches!(
        name,
        "volume"
            | "loop"
            | "muted"
            | "skippable"
            | "wait"
            | "fps"
            | "repeat"
            | "infinite"
            | "playback_rate"
    ) {
        "Playback"
    } else {
        "Properties"
    }
}

pub(super) struct SourceInspectorView<'a> {
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

impl WorkbenchPanel {
    fn move_source_position(
        &mut self,
        root: &Path,
        start: usize,
        point: Point<Pixels>,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = *self.source_position_bounds.borrow();
        if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
            return;
        }
        let Some(mut key) = self
            .source_inspector_key
            .clone()
            .filter(|key| key.block_start == start)
        else {
            return;
        };
        // Several pointer events can arrive before Inspector is rendered again.
        // Read this same command's current bounded fields for each atomic X/Y edit.
        let Some(source) = cx.global::<EditorDocuments>().source(root, &key.path) else {
            return;
        };
        let projection = EiyashouProjection::parse(&source);
        let Some(block) = projection
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .find(|block| {
                block.source_range.start == start
                    && block.kind == key.kind
                    && block.summary.split('(').next().unwrap_or_default().trim() == key.command
            })
        else {
            return;
        };
        let Some(fields) = projection.source_fields_for_block(&source, block) else {
            return;
        };
        key.fields = fields;
        let percent = |value: f32| {
            let value = value.clamp(0., 100.);
            if shift {
                (value / 25.).round() * 25.
            } else {
                (value * 10.).round() / 10.
            }
        };
        let x = ((percent(((point.x - bounds.origin.x) / bounds.size.width) * 100.) / 100. - 0.5)
            * keine_core::DESIGN_WIDTH)
            .round();
        let y = ((percent(((point.y - bounds.origin.y) / bounds.size.height) * 100.) / 100. - 0.5)
            * keine_core::DESIGN_HEIGHT)
            .round();
        self.source_position_draft = Some((start, x, y));
        self.commit_source_fields(
            root,
            &key,
            &[
                ("x".into(), Some(number_text(x))),
                ("y".into(), Some(number_text(y))),
            ],
            window,
            cx,
        );
        cx.notify();
    }
}

impl SourceInspectorView<'_> {
    fn position_pad(&self, cx: &mut Context<WorkbenchPanel>) -> Option<AnyElement> {
        if !matches!(
            self.key.command.as_str(),
            "camera.move" | "sprite.offset" | "sprite.transform" | "background.transform"
        ) {
            return None;
        }
        let coordinate = |name: &str| {
            let field = self.key.fields.iter().find(|field| field.key == name)?;
            let number = source_number(self.key, field)?;
            Some(number.parse(&field.value).unwrap_or(number.default))
        };
        let (mut x, mut y) = (coordinate("x")?, coordinate("y")?);
        if let Some((start, draft_x, draft_y)) = self.position_draft
            && start == self.key.block_start
        {
            (x, y) = (draft_x, draft_y);
        }
        let start = self.key.block_start;
        let down_root = self.root.to_owned();
        let move_root = self.root.to_owned();
        let panel = cx.weak_entity();
        let bounds = self.position_bounds.clone();
        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id("source-position-pad")
                        .relative()
                        .w(px(200.))
                        .h(px(112.5))
                        .max_w_full()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(CANVAS))
                        .overflow_hidden()
                        .cursor_crosshair()
                        .child(
                            canvas(
                                move |bounds_now, _, _| *bounds.borrow_mut() = bounds_now,
                                move |_, _, window, _| {
                                    let panel = panel.clone();
                                    let root = move_root.clone();
                                    // GPUI requires window listeners to be registered
                                    // during paint. Keep tracking outside the pad.
                                    let release_panel = panel.clone();
                                    let release_root = root.clone();
                                    window.on_mouse_event(
                                        move |event: &gpui_kit::MouseUpEvent, phase, window, cx| {
                                            if !phase.capture() || event.button != MouseButton::Left
                                            {
                                                return;
                                            }
                                            let Some(panel) = release_panel.upgrade() else {
                                                return;
                                            };
                                            if !panel
                                                .read(cx)
                                                .source_position_draft
                                                .is_some_and(|(active, _, _)| active == start)
                                            {
                                                return;
                                            }
                                            panel.update(cx, |panel, cx| {
                                                panel.move_source_position(
                                                    &release_root,
                                                    start,
                                                    event.position,
                                                    event.modifiers.shift,
                                                    window,
                                                    cx,
                                                );
                                                panel.source_position_draft = None;
                                                cx.notify();
                                            });
                                        },
                                    );
                                    window.on_mouse_event(
                                        move |event: &gpui_kit::MouseMoveEvent,
                                              phase,
                                              window,
                                              cx| {
                                            if !phase.bubble() {
                                                return;
                                            }
                                            let Some(panel) = panel.upgrade() else {
                                                return;
                                            };
                                            if !panel
                                                .read(cx)
                                                .source_position_draft
                                                .is_some_and(|(active, _, _)| active == start)
                                            {
                                                return;
                                            }
                                            panel.update(cx, |panel, cx| {
                                                if event.pressed_button == Some(MouseButton::Left) {
                                                    panel.move_source_position(
                                                        &root,
                                                        start,
                                                        event.position,
                                                        event.modifiers.shift,
                                                        window,
                                                        cx,
                                                    );
                                                } else {
                                                    panel.source_position_draft = None;
                                                    cx.notify();
                                                }
                                            });
                                        },
                                    );
                                },
                            )
                            .absolute()
                            .size_full(),
                        )
                        .children([0.25, 0.5, 0.75].into_iter().map(|at| {
                            div()
                                .absolute()
                                .left(gpui_kit::relative(at))
                                .top_0()
                                .bottom_0()
                                .w(px(1.))
                                .bg(rgb(BORDER))
                        }))
                        .children([0.25, 0.5, 0.75].into_iter().map(|at| {
                            div()
                                .absolute()
                                .top(gpui_kit::relative(at))
                                .left_0()
                                .right_0()
                                .h(px(1.))
                                .bg(rgb(BORDER))
                        }))
                        .child(
                            div()
                                .absolute()
                                .left(gpui_kit::relative(
                                    (x / keine_core::DESIGN_WIDTH + 0.5).clamp(0., 1.),
                                ))
                                .top(gpui_kit::relative(
                                    (y / keine_core::DESIGN_HEIGHT + 0.5).clamp(0., 1.),
                                ))
                                .ml(px(-5.))
                                .mt(px(-5.))
                                .size(px(10.))
                                .rounded_full()
                                .border_2()
                                .border_color(rgb(PRIMARY))
                                .bg(rgb(CANVAS)),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                this.move_source_position(
                                    &down_root,
                                    start,
                                    event.position,
                                    event.modifiers.shift,
                                    window,
                                    cx,
                                );
                            }),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.source_position_draft = None;
                                cx.notify();
                            }),
                        )
                        .on_mouse_up_out(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.source_position_draft = None;
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(MUTED))
                        .child(format!(
                            "X {} · Y {} px · Shift to snap",
                            number_text(x),
                            number_text(y)
                        )),
                )
                .into_any_element(),
        )
    }

    fn timeline(&self, cx: &mut Context<WorkbenchPanel>) -> Option<AnyElement> {
        let source = cx
            .global::<EditorDocuments>()
            .source(self.root, &self.key.path)?;
        let projection = EiyashouProjection::parse(&source);
        let blocks = projection
            .scenes
            .iter()
            .flat_map(|scene| &scene.blocks)
            .collect::<Vec<_>>();
        let stage = blocks.iter().find(|block| {
            block.summary.starts_with("stage.animate(")
                && block.source_range.contains(&self.key.block_start)
        })?;
        let time = SourceNumber {
            min: 0.,
            max: 5000.,
            step: 100.,
            default: 0.,
            unit: "ms",
        };
        let duration = projection
            .source_fields_for_block(&source, stage)?
            .iter()
            .find(|field| field.key == "duration")
            .and_then(|field| time.parse(&field.value))
            .unwrap_or(0.);
        let tracks = blocks
            .iter()
            .filter(|block| {
                block.summary.starts_with("track(")
                    && stage.source_range.contains(&block.source_range.start)
            })
            .collect::<Vec<_>>();
        let last_time = blocks
            .iter()
            .filter(|block| {
                block.summary.starts_with("key(")
                    && stage.source_range.contains(&block.source_range.start)
            })
            .filter_map(|block| projection.source_fields_for_block(&source, block))
            .flatten()
            .filter(|field| field.key == "time")
            .filter_map(|field| time.parse(&field.value))
            .fold(duration, f32::max)
            .max(1.);
        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label("Timeline"))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .justify_between()
                        .text_size(px(10.))
                        .text_color(rgb(MUTED))
                        .child("0ms")
                        .child(format!("{}ms", number_text(last_time))),
                )
                .children(tracks.into_iter().map(|track| {
                    let fields = projection
                        .source_fields_for_block(&source, track)
                        .unwrap_or_default();
                    let title = fields
                        .iter()
                        .filter(|field| matches!(field.key.as_str(), "0" | "1"))
                        .map(|field| field.value.as_str())
                        .collect::<Vec<_>>()
                        .join(" → ");
                    let keys = blocks
                        .iter()
                        .filter(|block| {
                            block.summary.starts_with("key(")
                                && track.source_range.contains(&block.source_range.start)
                        })
                        .filter_map(|block| {
                            let fields = projection.source_fields_for_block(&source, block)?;
                            let at = fields
                                .iter()
                                .find(|field| field.key == "time")
                                .and_then(|field| time.parse(&field.value))?;
                            Some((*block, at))
                        })
                        .collect::<Vec<_>>();
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(title))
                        .child(
                            div()
                                .relative()
                                .w_full()
                                .h(px(30.))
                                .rounded(px(3.))
                                .border_1()
                                .border_color(rgb(BORDER))
                                .bg(rgb(CANVAS))
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .top(px(14.))
                                        .h(px(1.))
                                        .bg(rgb(BORDER)),
                                )
                                .children(keys.into_iter().map(|(block, at)| {
                                    let root = self.root.to_owned();
                                    let path = self.key.path.clone();
                                    let start = block.source_range.start;
                                    let line = block.line;
                                    let column = block.column;
                                    div()
                                        .id(("timeline-key", start))
                                        .absolute()
                                        .left(gpui_kit::relative((at / last_time).clamp(0., 1.)))
                                        .ml(px(-6.))
                                        .top(px(8.))
                                        .size(px(12.))
                                        .rounded(px(2.))
                                        .border_1()
                                        .border_color(rgb(PRIMARY))
                                        .bg(rgb(if start == self.key.block_start {
                                            PRIMARY
                                        } else {
                                            PRIMARY_DIM
                                        }))
                                        .cursor_pointer()
                                        .tooltip(icon_hint(format!("{}ms", number_text(at))))
                                        .on_click(move |_, window, cx| {
                                            follow_preview_position(
                                                &root,
                                                &path,
                                                line + 1,
                                                column + 1,
                                                window,
                                                cx,
                                            );
                                            set_authoring_selection(
                                                &root,
                                                path.clone(),
                                                line,
                                                column,
                                                cx,
                                            );
                                        })
                                })),
                        )
                }))
                .into_any_element(),
        )
    }

    fn property(&self, position: usize, cx: &mut Context<WorkbenchPanel>) -> AnyElement {
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
            "camera.effect" | "camera.effect.v2" | "event.camera.patch"
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

fn replay_button(root: &Path, key: &SourceInspectorKey) -> impl IntoElement {
    let root = root.to_owned();
    let path = key.path.clone();
    let start = key.block_start;
    div()
        .id(("source-replay", start))
        .h(px(24.))
        .px_2()
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CANVAS))
        .flex()
        .items_center()
        .gap_1()
        .text_size(px(11.))
        .text_color(rgb(INK))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(SURFACE_HOVER)))
        .on_click(move |_, _, cx| run_source_block(&root, &path, start, cx))
        .child(Icon::new(AssetIconName::Play).xsmall())
        .child("Replay")
}

fn render_wait_control(
    root: &Path,
    key: &SourceInspectorKey,
    input: &Entity<InputState>,
    compact: bool,
    cx: &mut Context<WorkbenchPanel>,
) -> AnyElement {
    let advance = key.command == "wait.advance";
    let current = key
        .fields
        .first()
        .and_then(|field| source_number(key, field)?.parse(&field.value));
    let mut presets = div()
        .flex()
        .flex_wrap()
        .gap(px(if compact { 4. } else { 6. }));
    for duration in [200, 500, 1000, 2000, 3000]
        .into_iter()
        .map(Some)
        .chain([None])
    {
        let root = root.to_owned();
        let key = key.clone();
        let selected = duration.map_or(advance, |time| !advance && current == Some(time as f32));
        presets = presets.child(
            div()
                .id(format!("wait-{}-{duration:?}", key.block_start))
                .h(px(if compact { 22. } else { 26. }))
                .px(px(if compact { 8. } else { 10. }))
                .flex()
                .items_center()
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(if selected { PRIMARY } else { BORDER }))
                .bg(rgb(if selected { PRIMARY_DIM } else { CANVAS }))
                .text_size(px(if compact { 11. } else { 12. }))
                .text_color(rgb(if selected { PRIMARY } else { MUTED }))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(SURFACE_HOVER)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    let duration = duration.map(|time| time.to_string());
                    this.commit_wait_mode(&root, &key, duration.as_deref(), window, cx);
                }))
                .child(
                    duration.map_or_else(|| "Wait for input".to_owned(), |time| time.to_string()),
                ),
        );
    }
    let numeric = div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(if compact { 64. } else { 110. }))
                .h(px(if compact { 24. } else { 30. }))
                .px_2()
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(CANVAS))
                .child(
                    Input::new(input)
                        .appearance(false)
                        .bordered(false)
                        .size_full()
                        .px_0()
                        .py_0()
                        .text_align(gpui_kit::TextAlign::Right)
                        .text_size(px(if compact { 13. } else { 15. })),
                ),
        )
        .child(div().text_size(px(12.)).text_color(rgb(MUTED)).child("ms"));
    let content = if compact {
        div()
            .relative()
            .flex()
            .items_center()
            .w_full()
            .gap_2()
            .child(numeric)
            .child(
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .p_1()
                    .bg(rgb(CANVAS))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .invisible()
                    .group_hover("block-row", |style| style.visible())
                    .child(presets),
            )
    } else {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_label("Duration"))
            .child(numeric)
            .child(section_label("Presets"))
            .child(presets)
            .when(advance, |this| {
                this.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(MUTED))
                        .child("Waits here until the player advances."),
                )
            })
    };
    let start = key.block_start;
    content
        .id(("wait-controls", start))
        .capture_any_mouse_down(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
            if compact && event.button == MouseButton::Left {
                this.select_current_block(start, cx);
            }
        }))
        .on_click(|_, _, cx| cx.stop_propagation())
        .into_any_element()
}

pub(super) fn render_inline_block(
    root: &Path,
    control: &InlineBlockControl,
    cx: &mut Context<WorkbenchPanel>,
) -> Option<AnyElement> {
    let key = &control.key;
    let position = control.position;
    let start = key.block_start;
    if matches!(key.command.as_str(), "wait" | "wait.advance") {
        return Some(render_wait_control(root, key, &control.input, true, cx));
    }
    let field = &key.fields[position];
    if source_asset_kind(key, field).is_some() {
        return Some(resource_trigger(
            root,
            ResourceTarget::Source(key.clone(), position),
            control.options.clone(),
            field.value.clone(),
            cx,
        ));
    }
    let select = control.select.as_ref()?;
    Some(
        div()
            .id(("inline-asset", key.block_start))
            .flex_1()
            .min_w_0()
            .capture_any_mouse_down(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if event.button == MouseButton::Left {
                    this.select_current_block(start, cx);
                }
            }))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                Select::new(select)
                    .small()
                    .w_full()
                    .menu_max_h(px(320.))
                    .accessibility_label(source_field_label(key, field)),
            )
            .into_any_element(),
    )
}

// These are UI entities over bounded source fields, not another document model.
pub(super) struct InlineBlockControl {
    pub key: SourceInspectorKey,
    pub position: usize,
    pub input: Entity<InputState>,
    pub select: Option<Entity<SelectState<Vec<SourceOption>>>>,
    pub options: Vec<SourceOption>,
    pub _subscriptions: Vec<Subscription>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera_key(source: &str) -> SourceInspectorKey {
        let projection = EiyashouProjection::parse(source);
        let block = &projection.scenes[0].blocks[0];
        SourceInspectorKey {
            path: "main.shou".into(),
            block_start: block.source_range.start,
            kind: block.kind.clone(),
            command: block.summary.split('(').next().unwrap().trim().into(),
            fields: projection.source_fields_for_block(source, block).unwrap(),
        }
    }

    #[test]
    fn tween_switch_preserves_bounded_source_and_explicit_empty_selection() {
        let source =
            "scene a { camera.move(scene, x: 120, scale_x: 1.5, duration: 1s), wait.advance() }";
        let key = camera_key(source);
        assert!(camera_field_tweens(&key, "x"));
        assert_eq!(toggle_camera_tween(&key, "x"), "[scale_x]");
        let edited = EiyashouProjection::parse(source)
            .replace_block_fields(
                source,
                key.block_start,
                &[("tween".into(), Some(toggle_camera_tween(&key, "x")))],
            )
            .unwrap();
        let key = camera_key(&edited);
        assert!(!camera_field_tweens(&key, "x"));
        assert!(camera_field_tweens(&key, "scale_x"));
        assert_eq!(toggle_camera_tween(&key, "scale_x"), "[]");
        assert!(edited.ends_with(", wait.advance() }"));
    }

    #[test]
    fn shake_randomness_percent_controls_roundtrip_without_coercing_expressions() {
        let key = camera_key(
            "scene a { camera.shake(all, amplitude: 8, frequency: 12, amplitude_randomness: 0.3, duration: 1s) }",
        );
        let field = key
            .fields
            .iter()
            .find(|field| field.key == "amplitude_randomness")
            .unwrap();
        let control = source_number(&key, field).unwrap();
        assert!((control.parse(&field.value).unwrap() - 30.).abs() < 0.0001);
        assert_eq!(control.source(45.), "0.45");
        assert_eq!(
            (control.min, control.max, control.step, control.unit),
            (0., 100., 5., "%")
        );
    }
}
