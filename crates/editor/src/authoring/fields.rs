//! Shared Shou field metadata and source-value rules. No GPUI state or widgets.
use super::projection::{BlockKind, EiyashouProjection, SourceField};
use super::{AssetKind, AuthoringIndex, escape_eiyashou_string, valid_identifier};
use keine_loader::{native_child_command_argument_names, native_command_argument_names};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceContext {
    pub(crate) path: PathBuf,
    pub(crate) block_start: usize,
    pub(crate) kind: BlockKind,
    pub(crate) command: String,
    pub(crate) fields: Vec<SourceField>,
}

/// Filter optional fields using the same model in Inspector and Text completion.
pub(crate) fn argument_conflicts(command: &str, name: &str, fields: &[SourceField]) -> bool {
    let supplied = |key: &str| {
        fields
            .iter()
            .any(|field| field.key == key && field.insertion.is_none())
    };
    if command == "sprite.sequence" {
        let mode = fields
            .iter()
            .find(|field| field.key == "mode" && field.insertion.is_none())
            .map(|field| field.value.as_str());
        return match name {
            "loop" => mode.is_some(),
            "mode" => supplied("loop"),
            "interval" => mode != Some("blink"),
            "speaker" => mode != Some("talk"),
            _ => false,
        };
    }
    if !matches!(
        command,
        "sprite" | "background" | "sprite.transform" | "background.transform"
    ) {
        return false;
    }
    fields
        .iter()
        .filter(|field| field.insertion.is_none())
        .any(|field| match name {
            "scale" => matches!(field.key.as_str(), "scale_x" | "scale_y"),
            "scale_x" | "scale_y" => field.key == "scale",
            _ => false,
        })
}

/// Mode changes are one source transaction, including fields invalid in the new mode.
pub(crate) fn sequence_parameter_updates(
    key: &SourceContext,
    name: &str,
    value: &str,
    speaker: Option<&str>,
) -> Option<Vec<(String, Option<String>)>> {
    if key.command != "sprite.sequence" || name != "mode" {
        return None;
    }
    let mut updates = vec![("mode".into(), Some(value.into())), ("loop".into(), None)];
    match value {
        "blink" => updates.push(("speaker".into(), None)),
        "talk" => {
            updates.push(("interval".into(), None));
            if !key
                .fields
                .iter()
                .any(|field| field.key == "speaker" && field.insertion.is_none())
            {
                let speaker = speaker
                    .or_else(|| {
                        key.fields
                            .iter()
                            .find(|field| field.key == "0")
                            .map(|field| field.value.as_str())
                    })
                    .unwrap_or("speaker");
                updates.push((
                    "speaker".into(),
                    Some(format!("\"{}\"", escape_eiyashou_string(speaker))),
                ));
            }
        }
        _ => return None,
    }
    Some(updates)
}

/// Shared named-argument inventory for Inspector and Text completions.
pub fn command_argument_names(command: &str) -> Vec<&str> {
    native_command_argument_names(command).unwrap_or_default()
}

/// Shared editor context filtering; parser signature ownership stays in Loader.
pub(crate) fn child_argument_names(
    parent: &str,
    child: &str,
    fields: &[SourceField],
) -> Option<Vec<&'static str>> {
    let mut names = native_child_command_argument_names(parent, child)?;
    if parent == "sprite.sequence"
        && fields
            .iter()
            .any(|field| field.key == "fps" && field.insertion.is_none())
    {
        names.retain(|name| *name != "duration");
    }
    Some(names)
}

pub(crate) fn source_field_choices(
    key: &SourceContext,
    field: &SourceField,
) -> &'static [&'static str] {
    if (!field.quoted && matches!(field.value.as_str(), "true" | "false"))
        || matches!(
            field.key.as_str(),
            "light"
                | "blocking"
                | "infinite"
                | "looped"
                | "loop"
                | "muted"
                | "visible"
                | "enabled"
                | "skippable"
                | "wait_for_finished"
                | "auto"
                | "wait"
                | "godray_parallel"
                | "speed_lines_radial"
                | "speed_lines_region_ellipse"
                | "required"
                | "reset_camera"
                | "return_to_center_on_leave"
        )
    {
        return &["true", "false"];
    }
    match (key.command.as_str(), field.key.as_str()) {
        ("screen.film" | "playback.auto", "0") => &["true", "false"],
        ("text.presentation", "0") => &["paragraph", "dialogue"],
        ("input.request", "type") => &["string", "number", "bool"],
        ("assets.loading", "mode") => &["auto", "manual"],
        ("gallery.unlock", "0") => &["cg", "bgm"],
        ("ui.message", "0") => &["alert", "confirm"],
        ("key", "time") => &["0ms", "500ms", "1s", "2s"],
        ("wait", "0") => &["200ms", "500ms", "1s", "2s"],
        (_, "easing") => &[
            "linear",
            "ease_in",
            "ease_out",
            "ease_in_out",
            "in_out_quad",
            "out_cubic",
            "in_out_cubic",
            "out_back",
            "out_bounce",
        ],
        ("camera.move" | "camera.shake" | "camera.effect" | "camera.reset", "0") => {
            &["scene", "characters", "all", "none"]
        }
        ("dialogue", "concat" | "auto" | "inherit_speaker") => &["true", "false"],
        ("text.intro" | "frame", "hold") => &["true", "false"],
        (_, "falloff" | "shake.falloff") => &["linear", "exponential"],
        (_, "axis" | "shake.axis") => &["both", "x", "y"],
        ("camera.move", "shake") => &["shake(amplitude: 4, frequency: 2)"],
        ("move", "1") | (_, "position") => &["left", "center", "right"],
        (_, "layout") => &[
            "natural",
            "viewport(height: 0.85)",
            "scene(fit: cover, x: 960, y: 540)",
            "composite(canvas: size(width: 1920, height: 1080))",
        ],
        (_, "layout.fit") | ("scene", "fit") => &[
            "by_height",
            "by_width",
            "cover",
            "contain",
            "stretch",
            "center",
        ],
        (_, "layout.anchor") | ("scene", "anchor") => &["point(x: 0.5, y: 0.5)"],
        (_, "layout.canvas") | ("composite", "canvas") => &["size(width: 1920, height: 1080)"],
        (_, "layout.rect") | ("composite", "rect") => {
            &["rect(x: 0, y: 0, width: 1920, height: 1080)"]
        }
        (_, "fit") => &["contain", "cover", "fill"],
        ("sprite.sequence", "mode") => &["blink", "talk"],
        (_, "blend") => &["alpha", "add", "multiply", "screen"],
        ("event.audio", "1") => &["bgm", "effect", "vocal"],
        ("resource", "kind") => &["background", "figure"],
        ("stage.mask.show", "mode") => &["overlay", "clip"],
        ("stage.mask.show", "plane") => &["behind_scene", "bottom", "top", "topmost"],
        ("stage.mask.show", "scope") => &["scene", "characters", "all", "selected"],
        ("stage.mask.show", "shape") => &["rectangle", "rounded_rectangle", "ellipse", "image"],
        ("stage.mask.show", "image_channel") => &["alpha", "luminance"],
        ("stage.mask.show", "image_fit" | "texture_fit") => &["stretch", "cover", "contain"],
        ("stage.mask.show", "visibility") => &["inside", "outside"],
        ("stage.mask.show", "fill_mode") => &["solid", "gradient", "texture"],
        ("stage.mask.show", "texture_blend") => &["normal", "multiply", "screen", "add"],
        ("video.play", "mode") => &["fullscreen", "mixed"],
        _ => &[],
    }
}

pub(crate) fn source_field_label(key: &SourceContext, field: &SourceField) -> String {
    if let Some((group, name)) = field.key.split_once('.')
        && matches!(group, "position" | "1" | "layout" | "shake")
    {
        return title_case(&name.replace(['.', '_'], " "));
    }
    command_field_label(&key.kind, &key.command, &field.key)
}

pub(crate) fn command_field_label(kind: &BlockKind, command: &str, field_key: &str) -> String {
    if command == "text.retract" {
        match field_key {
            "source" => return "Full text".into(),
            "keep" => return "Keep prefix".into(),
            _ => {}
        }
    }
    if field_key == "repeat" && matches!(command, "stage.animate" | "sprite.keyframes") {
        return "Additional repeats".into();
    }
    if let Some((group, name)) = field_key.split_once('.')
        && matches!(group, "position" | "1" | "layout" | "shake")
    {
        return format!(
            "{} {}",
            if group == "1" {
                "Position".into()
            } else {
                title_case(group)
            },
            title_case(&name.replace(['.', '_'], " "))
        );
    }
    if field_key.parse::<usize>().is_err() {
        return title_case(
            &field_key
                .rsplit('.')
                .next()
                .unwrap_or(field_key)
                .replace('_', " "),
        );
    }
    let position = field_key.parse::<usize>().unwrap_or_default();
    match kind {
        BlockKind::Choice => "Prompt".into(),
        BlockKind::Conditional | BlockKind::ElseIf => "Condition".into(),
        BlockKind::ChoiceOption => "Option".into(),
        BlockKind::Declaration | BlockKind::Assignment => "Value".into(),
        BlockKind::Command => match (command, position) {
            ("background" | "bgm" | "se" | "video", 0) => "Asset".into(),
            ("sprite", 0) | ("hide", 0) | ("move", 0) => "Slot".into(),
            ("sprite", 1) => "Asset".into(),
            ("move", 1) => "Position".into(),
            ("goto" | "call", 0) => "Scene".into(),
            ("camera.move" | "camera.shake", 0) => "Target".into(),
            ("sprite.focus", 0) => "Speaker".into(),
            ("avatar.show" | "vocal.play", 0) => "Asset".into(),
            ("screen.film" | "playback.auto", 0) => "Enabled".into(),
            ("ui.show" | "ui.hide", 0) => "Surface".into(),
            ("text.presentation", 0) => "Mode".into(),
            ("text.style", 0) => "Style".into(),
            ("particle.hide" | "video.stop" | "text.float.hide", 0) => "Target".into(),
            ("gallery.unlock", 0) => "Kind".into(),
            ("gallery.unlock", 1) => "Asset".into(),
            ("input.request", 0) => "Variable".into(),
            ("text.paragraph.style", 0) => "Style".into(),
            ("particle.show", 0) => "ID".into(),
            ("particle.show", 1) => "Preset".into(),
            ("ui.message", 0) => "Mode".into(),
            ("text.float", 0) => "Text".into(),
            (
                "sprite.sequence" | "sprite.select" | "sprite.select.when" | "sprite.keyframes"
                | "sprite.update",
                0,
            ) => "Target".into(),
            ("sprite.select", 1) => "Variable".into(),
            ("page", 0) => "Text".into(),
            ("frame" | "resource", 0) => "Asset or value".into(),
            ("case", 0) => "Value".into(),
            ("case", 1) => "Asset".into(),
            ("camera.bind" | "camera.unbind", 0) => "Target".into(),
            ("camera.effect" | "camera.reset", 0) => "Targets".into(),
            ("stage.animate", 0) => "Animation ID".into(),
            ("track", 0) => "Target".into(),
            ("track", 1) => "Property".into(),
            ("event.scene", 0) => "Scene".into(),
            ("event.particle" | "event.audio", 0) => "ID".into(),
            ("stage.mask.show" | "stage.mask.hide", 0) => "Mask ID".into(),
            ("se.loop", 0) | ("se.stop", 0) | ("video.play", 0) => "ID".into(),
            ("se.loop", 1) | ("video.play", 1) => "Asset".into(),
            ("sprite.transform" | "sprite.animate" | "sprite.transition", 0) => "Target".into(),
            ("sprite.animate", 1) => "Preset".into(),
            ("wait", 0) => "Duration".into(),
            ("pop", 0) => "List".into(),
            ("pop", 1) => "Index".into(),
            (command, 0) if command.ends_with(".append") || command.ends_with(".remove") => {
                "Value".into()
            }
            (command, 0) if command.ends_with(".insert") => "Index".into(),
            (command, 1) if command.ends_with(".insert") => "Value".into(),
            _ => format!("Arg {}", position + 1),
        },
        _ => format!("Arg {}", position + 1),
    }
}

pub(crate) fn title_case(value: &str) -> String {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    first.to_uppercase().chain(characters).collect()
}

// Shared by the source Inspector and inline command controls. The presentation
// follows Studio 2.0's NumberSliderField; the values stay in Eiyashou units.
#[derive(Clone, Copy)]
pub(crate) struct SourceNumber {
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

pub(crate) fn number_text(value: f32) -> String {
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

pub(crate) fn source_number(key: &SourceContext, field: &SourceField) -> Option<SourceNumber> {
    let command = key.command.as_str();
    if matches!(
        command,
        "camera.move" | "camera.effect" | "event.camera.patch"
    ) && let Some(control) = effect_number(&field.key)
    {
        return (field.value.is_empty() || control.parse(&field.value).is_some())
            .then_some(control);
    }
    if field.key == "hold" && command == "text.intro" {
        return None;
    }
    if field.key.starts_with("layout.anchor.") {
        let control = SourceNumber {
            min: 0.,
            max: 1.,
            step: 0.01,
            default: 0.5,
            unit: "",
        };
        return (field.value.is_empty() || control.parse(&field.value).is_some())
            .then_some(control);
    }
    if field.key == "layout.height"
        && key.fields.iter().any(|parent| {
            parent.key == "layout"
                && (parent.value.starts_with("viewport(") || parent.value.starts_with("composite("))
        })
    {
        let control = SourceNumber {
            min: 0.01,
            max: 4.,
            step: 0.01,
            default: 1.,
            unit: "×",
        };
        return (field.value.is_empty() || control.parse(&field.value).is_some())
            .then_some(control);
    }
    let name = field.key.rsplit('.').next().unwrap_or(&field.key);
    let (min, max, step, default, unit) = match name {
        "volume" => (0., 100., 1., 100., "%"),
        "duration" | "fade" | "fade_in" | "fade_out" | "hold" | "time" | "reveal_duration" => {
            (0., 5000., 100., 0., "ms")
        }
        "0" if command == "wait" => (0., 5000., 100., 1000., "ms"),
        "x" | "y" if command == "camera.move" => (-1000., 1000., 1., 0., "px"),
        "x" | "y" => (-1920., 1920., 1., 0., "px"),
        "rotation" | "angle" => (-180., 180., 1., 0., "°"),
        "scale" | "scale_x" | "scale_y" => (0.1, 3., 0.01, 1., "×"),
        "alpha" => (0., 100., 1., 100., "%"),
        "blur" | "blur_amount" | "blur_strength" => (0., 30., 0.1, 0., ""),
        "amplitude" => (0., 60., 1., 0., "px"),
        "amplitude_randomness" | "frequency_randomness" => (0., 100., 5., 0., "%"),
        "frequency" => (0., 30., 0.5, 12., "Hz"),
        "interval" => (100., 10000., 100., 3000., "ms"),
        "fps" => (1., 60., 1., 12., "fps"),
        "size" if command == "particle.show" => (1., 256., 1., 16., "px"),
        "speed" if command == "particle.show" => (0., 1000., 1., 100., "px/s"),
        "spin" if command == "particle.show" => (-360., 360., 1., 0., "°/s"),
        "drift" if command == "particle.show" => (0., 256., 1., 16., "px"),
        "drag" if command == "particle.show" => (0., 10., 0.01, 0., ""),
        "width" | "height" if field.key.starts_with("layout.") => (1., 4096., 1., 1080., "px"),
        "font_size" => (8., 128., 1., 32., "px"),
        "count" if matches!(command, "particle.show" | "event.particle") => {
            (1., 256., 1., 100., "")
        }
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
pub(crate) fn camera_tween_field(key: &SourceContext, field: &SourceField) -> bool {
    let Some(numeric) = keine_core::CameraTweenField::from_name(&field.key) else {
        return false;
    };
    match key.command.as_str() {
        "camera.move" => true,
        "camera.effect" => !numeric.is_transform(),
        _ => false,
    }
}

pub(crate) fn camera_field_tweens(key: &SourceContext, name: &str) -> bool {
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

pub(crate) fn toggle_camera_tween(key: &SourceContext, name: &str) -> String {
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

pub(crate) fn source_input_value(key: &SourceContext, field: &SourceField) -> String {
    source_number(key, field)
        .and_then(|control| control.parse(&field.value))
        .map(number_text)
        .unwrap_or_else(|| field.value.clone())
}

pub(crate) fn source_input_commit(
    key: &SourceContext,
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

pub(crate) fn asset_source_value(field: &SourceField, value: &str) -> String {
    if !field.quoted && !valid_identifier(value) {
        format!("\"{}\"", escape_eiyashou_string(value))
    } else {
        value.to_owned()
    }
}

pub(crate) fn source_asset_kind(key: &SourceContext, field: &SourceField) -> Option<AssetKind> {
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
        ("particle.show" | "event.particle", "texture") => Some(AssetKind::Particle),
        _ => None,
    }
}

pub(crate) fn source_asset_accepts(
    key: &SourceContext,
    field: &SourceField,
    kind: AssetKind,
) -> bool {
    source_asset_kind(key, field).is_some_and(|expected| {
        expected == kind || (expected == AssetKind::Figure && kind == AssetKind::Background)
    })
}

pub(crate) fn character_track(key: &SourceContext) -> bool {
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

pub(crate) const EFFECT_GROUPS: &[(&str, &str)] = &[
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

pub(crate) fn source_effect_group(name: &str) -> Option<(&'static str, &'static str)> {
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

pub(crate) fn source_field_enabled(key: &SourceContext, field: &SourceField) -> bool {
    if !matches!(
        key.command.as_str(),
        "camera.move" | "camera.effect" | "event.camera.patch"
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

pub(crate) fn source_property_group(field: &SourceField) -> &'static str {
    let name = field.key.as_str();
    if name == "position" || name.starts_with("position.") || name.starts_with("1.") {
        return "Position";
    }
    if name.starts_with("layout.") {
        return "Layout";
    }
    if name == "shake" || name.starts_with("shake.") {
        return "Shake";
    }
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
    } else if name == "layout" {
        "Layout"
    } else if matches!(
        name,
        "x" | "y" | "alpha" | "scale" | "scale_x" | "scale_y" | "rotation" | "width" | "height"
    ) {
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

pub(crate) struct FieldOption {
    pub value: String,
    pub title: String,
    pub asset: Option<(PathBuf, AssetKind, PathBuf)>,
}

pub(crate) fn field_options(
    root: &Path,
    key: &SourceContext,
    field: &SourceField,
    index: Option<&AuthoringIndex>,
    source: Option<(&str, &EiyashouProjection)>,
) -> Vec<FieldOption> {
    if source_number(key, field).is_some() {
        return Vec::new();
    }
    let mut options = if key.command == "track" && field.key == "1" {
        keine_loader::native_stage_property_names()
            .map(|name| FieldOption {
                value: name.into(),
                title: title_case(&name.replace('_', " ")),
                asset: None,
            })
            .collect()
    } else if key.command == "track" && field.key == "0" {
        let mut targets = vec![("camera".to_owned(), "Camera".to_owned())];
        if let Some((source, projection)) = source {
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
                if let Some(fields) = projection.source_fields_for_block(source, block)
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
            .map(|(value, title)| FieldOption {
                value,
                title,
                asset: None,
            })
            .collect()
    } else if source_asset_kind(key, field).is_some() {
        index
            .into_iter()
            .flat_map(|index| &index.assets)
            .filter(|asset| source_asset_accepts(key, field, asset.kind))
            .map(|asset| FieldOption {
                value: asset.id.clone(),
                title: asset
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&asset.id)
                    .to_owned(),
                asset: Some((root.to_owned(), asset.kind, asset.path.clone())),
            })
            .collect::<Vec<_>>()
    } else {
        source_field_choices(key, field)
            .iter()
            .filter(|value| !matches!(**value, "true" | "false"))
            .map(|value| FieldOption {
                value: (*value).to_owned(),
                title: title_case(&value.replace('_', " ")),
                asset: None,
            })
            .collect::<Vec<_>>()
    };
    let default = match field.key.as_str() {
        "easing" | "falloff" => Some("linear"),
        "axis" => Some("both"),
        "position" => Some("center"),
        "layout" => Some("natural"),
        "layout.fit" => Some("by_height"),
        "fit" => Some("contain"),
        "blend" => Some("alpha"),
        "mode" if key.command == "video.play" => Some("fullscreen"),
        _ => None,
    };
    if field.value.is_empty()
        && let Some(default) = default
    {
        options.insert(
            0,
            FieldOption {
                value: String::new(),
                title: title_case(default),
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
            FieldOption {
                value: field.value.clone(),
                title: field.value.clone(),
                asset: None,
            },
        );
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_key(source: &str) -> SourceContext {
        let projection = EiyashouProjection::parse(source);
        let block = &projection.scenes[0].blocks[0];
        SourceContext {
            path: "main.shou".into(),
            block_start: block.source_range.start,
            kind: block.kind.clone(),
            command: block.summary.split('(').next().unwrap().trim().into(),
            fields: projection.source_fields_for_block(source, block).unwrap(),
        }
    }

    #[test]
    fn options_use_project_assets_and_current_source_without_ui_state() {
        let source = "scene start { sprite(灵梦, pose), /* disabled\nsprite(hidden, pose)\n*/ wait.advance() }";
        let projection = EiyashouProjection::parse(source);
        assert!(
            projection.read_only.is_empty(),
            "{:?}",
            projection.read_only
        );
        let mut key = source_key("scene start { background(none) }");
        let mut field = key.fields[0].clone();
        field.value.clear();
        let root = Path::new("project");
        let index = AuthoringIndex {
            assets: vec![super::super::AssetEntry {
                kind: AssetKind::Background,
                id: "庭院".into(),
                path: "assets/庭院.webp".into(),
                tags: Vec::new(),
                exists: true,
                reference_count: 0,
            }],
            ..Default::default()
        };
        let options = field_options(root, &key, &field, Some(&index), None);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].value, "庭院");
        assert_eq!(options[0].title, "庭院.webp");
        assert_eq!(
            options[0].asset,
            Some((
                root.into(),
                AssetKind::Background,
                "assets/庭院.webp".into()
            ))
        );

        let sprite = source_key("scene start { sprite(layer, 庭院) }");
        let image = sprite.fields.iter().find(|field| field.key == "1").unwrap();
        assert!(source_asset_accepts(&sprite, image, AssetKind::Background));
        assert!(!source_asset_accepts(&sprite, image, AssetKind::Bgm));
        assert_eq!(
            field_options(root, &sprite, image, Some(&index), None)[0].value,
            "庭院"
        );

        key.command = "track".into();
        let options = field_options(
            root,
            &key,
            &field,
            Some(&index),
            Some((source, &projection)),
        );
        assert_eq!(
            options
                .iter()
                .map(|option| option.value.as_str())
                .collect::<Vec<_>>(),
            ["camera", "character(\"灵梦\")"]
        );
        assert!(options.iter().all(|option| option.asset.is_none()));
    }

    #[test]
    fn tween_switch_preserves_bounded_source_and_explicit_empty_selection() {
        let source =
            "scene a { camera.move(scene, x: 120, scale_x: 1.5, duration: 1s), wait.advance() }";
        let key = source_key(source);
        assert!(camera_field_tweens(&key, "x"));
        assert_eq!(toggle_camera_tween(&key, "x"), "[scale_x]");
        let edited = EiyashouProjection::parse(source)
            .replace_block_fields(
                source,
                key.block_start,
                &[("tween".into(), Some(toggle_camera_tween(&key, "x")))],
            )
            .unwrap();
        let key = source_key(&edited);
        assert!(!camera_field_tweens(&key, "x"));
        assert!(camera_field_tweens(&key, "scale_x"));
        assert_eq!(toggle_camera_tween(&key, "scale_x"), "[]");
        assert!(edited.ends_with(", wait.advance() }"));
    }

    #[test]
    fn shake_randomness_percent_controls_roundtrip_without_coercing_expressions() {
        let key = source_key(
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

#[cfg(test)]
mod sequence_tests {
    use super::*;
    #[test]
    fn inspector_sequence_mode_changes_are_atomic_and_remove_incompatible_parameters() {
        for (source, mode) in [
            (
                "scene start { sprite.sequence(hero, fps: 10, loop: true) { frame(rest), frame(closed) } }",
                "blink",
            ),
            (
                "scene start { sprite.sequence(hero, fps: 10, mode: blink, interval: 2s) { frame(rest), frame(closed) } }",
                "talk",
            ),
            (
                "scene start { sprite.sequence(hero, fps: 10, mode: talk, speaker: \"hero\") { frame(rest), frame(closed) } }",
                "blink",
            ),
        ] {
            let projection = EiyashouProjection::parse(source);
            let start = source.find("sprite.sequence").unwrap();
            let fields = projection.source_fields(source, start).unwrap();
            let key = SourceContext {
                path: PathBuf::new(),
                block_start: start,
                kind: BlockKind::Command,
                command: "sprite.sequence".into(),
                fields,
            };
            let updates = sequence_parameter_updates(&key, "mode", mode, Some("hero")).unwrap();
            let edited = projection
                .replace_block_fields(source, start, &updates)
                .unwrap();
            let parsed = keine_loader::parse_native_document(&edited);
            assert!(
                parsed.diagnostics.is_empty(),
                "{edited}: {:?}",
                parsed.diagnostics
            );
            assert!(!edited.contains("loop:"));
            if mode == "talk" {
                assert!(!edited.contains("interval:"));
            } else {
                assert!(!edited.contains("speaker:"));
            }
        }
        let source = "scene start { sprite.sequence(hero) { frame(rest, duration: 100ms), frame(closed, duration: 100ms) } }";
        let projection = EiyashouProjection::parse(source);
        let fields = projection
            .source_fields(source, source.find("sprite.sequence").unwrap())
            .unwrap();
        assert!(!fields.iter().any(|field| field.key == "fps"));
    }
}
