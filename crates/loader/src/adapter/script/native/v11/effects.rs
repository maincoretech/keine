use super::*;
use keine_core::{ColorToneMode, PostProcessPatch, PostProcessV2};

pub(super) const EFFECT_COMMANDS: &[&str] = &["camera.effect", "camera.effect.v2"];

pub(super) const PATCH_FIELDS: &[&str] = &[
    "duration",
    "easing",
    "blocking",
    "tween",
    "focal_distance",
    "blur_strength",
    "distortion_strength",
    "vignette_intensity",
    "vignette_size",
    "blur_amount",
    "color_tone",
    "color_tone_intensity",
    "color_exposure",
    "color_brightness",
    "color_contrast",
    "color_saturation",
    "color_temperature",
    "old_film_intensity",
    "shock_intensity",
    "godray_intensity",
    "godray_angle",
    "godray_gain",
    "godray_lacunarity",
    "godray_speed",
    "godray_parallel",
    "godray_center_x",
    "godray_center_y",
    "lut_preset",
    "lut_intensity",
    "bloom_intensity",
    "chromatic_aberration",
    "pixelate_size",
    "glitch_intensity",
    "crt_intensity",
    "sharpen_strength",
    "radial_blur_strength",
    "radial_blur_center_x",
    "radial_blur_center_y",
    "motion_blur_strength",
    "motion_blur_angle",
    "zoom_blur_strength",
    "zoom_blur_center_x",
    "zoom_blur_center_y",
    "light_leak_intensity",
    "light_leak_angle",
    "lens_flare_intensity",
    "lens_flare_center_x",
    "lens_flare_center_y",
    "film_grain_intensity",
    "film_grain_size",
    "heat_haze_intensity",
    "heat_haze_speed",
    "heat_haze_scale",
    "water_ripple_intensity",
    "water_ripple_frequency",
    "water_ripple_speed",
    "water_ripple_center_x",
    "water_ripple_center_y",
    "fog_intensity",
    "fog_speed",
    "fog_scale",
    "vhs_intensity",
    "vhs_jitter",
    "vhs_noise",
    "halftone_intensity",
    "halftone_scale",
    "halftone_angle",
    "dither_intensity",
    "dither_levels",
    "outline_intensity",
    "outline_thickness",
    "eyelid_openness",
    "eyelid_width",
    "eyelid_curvature",
    "eyelid_softness",
    "eyelid_center_x",
    "eyelid_center_y",
];
pub(super) const V2_FIELDS: &[&str] = &[
    "duration",
    "easing",
    "blocking",
    "tween",
    "mirror_shatter_intensity",
    "mirror_shatter_center_x",
    "mirror_shatter_center_y",
    "mirror_shatter_spread",
    "mirror_shatter_seed",
    "speed_lines_intensity",
    "speed_lines_radial",
    "speed_lines_density",
    "speed_lines_angle",
    "speed_lines_speed",
    "speed_lines_center_x",
    "speed_lines_center_y",
    "speed_lines_region_ellipse",
    "speed_lines_region_x",
    "speed_lines_region_y",
    "speed_lines_region_width",
    "speed_lines_region_height",
    "speed_lines_region_feather",
];

impl<'a> Parser<'a> {
    pub(super) fn parse_v11_effect_command(
        &self,
        name: &str,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<Action> {
        let fields = if name == "camera.effect" {
            PATCH_FIELDS
        } else {
            V2_FIELDS
        };
        let before = report.diagnostics.len();
        self.validate_signature(name, args, 1, fields, report);
        if report.diagnostics.len() != before {
            return None;
        }
        let targets = self.camera_targets(args, report)?;
        let duration = self.named_duration_checked(args, "duration", report)?;
        let easing = self.named_easing(args, "easing", report)?;
        let blocking = self.v11_optional_bool(args, "blocking", true, report)?;
        match name {
            "camera.effect" => self.camera_tween(
                args,
                Action::SetPostProcess {
                    targets,
                    effect: Box::new(self.v11_post_process_patch(args, report)?),
                    duration,
                    easing,
                    blocking,
                },
                report,
            ),
            "camera.effect.v2" => {
                // V2 is a full state action, so every field is required. Partial calls would reset omitted effects.
                let effect = PostProcessV2 {
                    mirror_shatter_intensity: self.v11_named_number(
                        args,
                        "mirror_shatter_intensity",
                        report,
                    )?,
                    mirror_shatter_center_x: self.v11_named_number(
                        args,
                        "mirror_shatter_center_x",
                        report,
                    )?,
                    mirror_shatter_center_y: self.v11_named_number(
                        args,
                        "mirror_shatter_center_y",
                        report,
                    )?,
                    mirror_shatter_spread: self.v11_named_number(
                        args,
                        "mirror_shatter_spread",
                        report,
                    )?,
                    mirror_shatter_seed: self.v11_named_number(
                        args,
                        "mirror_shatter_seed",
                        report,
                    )?,
                    speed_lines_intensity: self.v11_named_number(
                        args,
                        "speed_lines_intensity",
                        report,
                    )?,
                    speed_lines_radial: self.v11_named_bool(args, "speed_lines_radial", report)?,
                    speed_lines_density: self.v11_named_number(
                        args,
                        "speed_lines_density",
                        report,
                    )?,
                    speed_lines_angle: self.v11_named_number(args, "speed_lines_angle", report)?,
                    speed_lines_speed: self.v11_named_number(args, "speed_lines_speed", report)?,
                    speed_lines_center_x: self.v11_named_number(
                        args,
                        "speed_lines_center_x",
                        report,
                    )?,
                    speed_lines_center_y: self.v11_named_number(
                        args,
                        "speed_lines_center_y",
                        report,
                    )?,
                    speed_lines_region_ellipse: self.v11_named_bool(
                        args,
                        "speed_lines_region_ellipse",
                        report,
                    )?,
                    speed_lines_region_x: self.v11_named_number(
                        args,
                        "speed_lines_region_x",
                        report,
                    )?,
                    speed_lines_region_y: self.v11_named_number(
                        args,
                        "speed_lines_region_y",
                        report,
                    )?,
                    speed_lines_region_width: self.v11_named_number(
                        args,
                        "speed_lines_region_width",
                        report,
                    )?,
                    speed_lines_region_height: self.v11_named_number(
                        args,
                        "speed_lines_region_height",
                        report,
                    )?,
                    speed_lines_region_feather: self.v11_named_number(
                        args,
                        "speed_lines_region_feather",
                        report,
                    )?,
                };
                self.camera_tween(
                    args,
                    Action::SetPostProcessV2 {
                        targets,
                        effect: Box::new(effect),
                        duration,
                        easing,
                        blocking,
                    },
                    report,
                )
            }
            _ => None,
        }
    }
    pub(super) fn v11_post_process_patch(
        &self,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<PostProcessPatch> {
        let mut effect = PostProcessPatch {
            blur_strength: self.checked_number(args, "blur_strength", report)?,
            distortion_strength: self.checked_number(args, "distortion_strength", report)?,
            vignette_intensity: self.checked_number(args, "vignette_intensity", report)?,
            vignette_size: self.checked_number(args, "vignette_size", report)?,
            blur_amount: self.checked_number(args, "blur_amount", report)?,
            color_tone_intensity: self.checked_number(args, "color_tone_intensity", report)?,
            color_exposure: self.checked_number(args, "color_exposure", report)?,
            color_brightness: self.checked_number(args, "color_brightness", report)?,
            color_contrast: self.checked_number(args, "color_contrast", report)?,
            color_saturation: self.checked_number(args, "color_saturation", report)?,
            color_temperature: self.checked_number(args, "color_temperature", report)?,
            old_film_intensity: self.checked_number(args, "old_film_intensity", report)?,
            shock_intensity: self.checked_number(args, "shock_intensity", report)?,
            godray_intensity: self.checked_number(args, "godray_intensity", report)?,
            godray_angle: self.checked_number(args, "godray_angle", report)?,
            godray_gain: self.checked_number(args, "godray_gain", report)?,
            godray_lacunarity: self.checked_number(args, "godray_lacunarity", report)?,
            godray_speed: self.checked_number(args, "godray_speed", report)?,
            godray_center_x: self.checked_number(args, "godray_center_x", report)?,
            godray_center_y: self.checked_number(args, "godray_center_y", report)?,
            lut_intensity: self.checked_number(args, "lut_intensity", report)?,
            bloom_intensity: self.checked_number(args, "bloom_intensity", report)?,
            chromatic_aberration: self.checked_number(args, "chromatic_aberration", report)?,
            pixelate_size: self.checked_number(args, "pixelate_size", report)?,
            glitch_intensity: self.checked_number(args, "glitch_intensity", report)?,
            crt_intensity: self.checked_number(args, "crt_intensity", report)?,
            sharpen_strength: self.checked_number(args, "sharpen_strength", report)?,
            radial_blur_strength: self.checked_number(args, "radial_blur_strength", report)?,
            radial_blur_center_x: self.checked_number(args, "radial_blur_center_x", report)?,
            radial_blur_center_y: self.checked_number(args, "radial_blur_center_y", report)?,
            motion_blur_strength: self.checked_number(args, "motion_blur_strength", report)?,
            motion_blur_angle: self.checked_number(args, "motion_blur_angle", report)?,
            zoom_blur_strength: self.checked_number(args, "zoom_blur_strength", report)?,
            zoom_blur_center_x: self.checked_number(args, "zoom_blur_center_x", report)?,
            zoom_blur_center_y: self.checked_number(args, "zoom_blur_center_y", report)?,
            light_leak_intensity: self.checked_number(args, "light_leak_intensity", report)?,
            light_leak_angle: self.checked_number(args, "light_leak_angle", report)?,
            lens_flare_intensity: self.checked_number(args, "lens_flare_intensity", report)?,
            lens_flare_center_x: self.checked_number(args, "lens_flare_center_x", report)?,
            lens_flare_center_y: self.checked_number(args, "lens_flare_center_y", report)?,
            film_grain_intensity: self.checked_number(args, "film_grain_intensity", report)?,
            film_grain_size: self.checked_number(args, "film_grain_size", report)?,
            heat_haze_intensity: self.checked_number(args, "heat_haze_intensity", report)?,
            heat_haze_speed: self.checked_number(args, "heat_haze_speed", report)?,
            heat_haze_scale: self.checked_number(args, "heat_haze_scale", report)?,
            water_ripple_intensity: self.checked_number(args, "water_ripple_intensity", report)?,
            water_ripple_frequency: self.checked_number(args, "water_ripple_frequency", report)?,
            water_ripple_speed: self.checked_number(args, "water_ripple_speed", report)?,
            water_ripple_center_x: self.checked_number(args, "water_ripple_center_x", report)?,
            water_ripple_center_y: self.checked_number(args, "water_ripple_center_y", report)?,
            fog_intensity: self.checked_number(args, "fog_intensity", report)?,
            fog_speed: self.checked_number(args, "fog_speed", report)?,
            fog_scale: self.checked_number(args, "fog_scale", report)?,
            vhs_intensity: self.checked_number(args, "vhs_intensity", report)?,
            vhs_jitter: self.checked_number(args, "vhs_jitter", report)?,
            vhs_noise: self.checked_number(args, "vhs_noise", report)?,
            halftone_intensity: self.checked_number(args, "halftone_intensity", report)?,
            halftone_scale: self.checked_number(args, "halftone_scale", report)?,
            halftone_angle: self.checked_number(args, "halftone_angle", report)?,
            dither_intensity: self.checked_number(args, "dither_intensity", report)?,
            dither_levels: self.checked_number(args, "dither_levels", report)?,
            outline_intensity: self.checked_number(args, "outline_intensity", report)?,
            outline_thickness: self.checked_number(args, "outline_thickness", report)?,
            eyelid_openness: self.checked_number(args, "eyelid_openness", report)?,
            eyelid_width: self.checked_number(args, "eyelid_width", report)?,
            eyelid_curvature: self.checked_number(args, "eyelid_curvature", report)?,
            eyelid_softness: self.checked_number(args, "eyelid_softness", report)?,
            eyelid_center_x: self.checked_number(args, "eyelid_center_x", report)?,
            eyelid_center_y: self.checked_number(args, "eyelid_center_y", report)?,
            ..PostProcessPatch::default()
        };
        if let Some(arg) = self.named_arg(args, "focal_distance") {
            effect.focal_distance = Some(
                if self.argument_identifier(arg).as_deref() == Some("none") {
                    None
                } else {
                    Some(self.v11_named_number(args, "focal_distance", report)?)
                },
            );
        }
        if let Some(arg) = self.named_arg(args, "lut_preset") {
            effect.lut_preset = Some(
                if self.argument_identifier(arg).as_deref() == Some("none") {
                    None
                } else {
                    Some(self.v11_id_or_string(Some(arg), "LUT preset", report)?)
                },
            );
        }
        if let Some(arg) = self.named_arg(args, "color_tone") {
            effect.color_tone = Some(match self.argument_identifier(arg).as_deref() {
                Some("none") => ColorToneMode::None,
                Some("grayscale") => ColorToneMode::Grayscale,
                Some("sepia") => ColorToneMode::Sepia,
                _ => {
                    report.diagnostics.push(self.error("unknown color tone"));
                    return None;
                }
            });
        }
        if self.named_arg(args, "godray_parallel").is_some() {
            effect.godray_parallel = Some(self.v11_named_bool(args, "godray_parallel", report)?);
        }
        if effect.is_empty() {
            report
                .diagnostics
                .push(self.error("camera.effect(...) requires at least one effect field"));
            return None;
        }
        Some(effect)
    }
}
