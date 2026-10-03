//! Camera reset lowering shared by native authoring and compatibility input.
use keine_core::{
    Action, CameraShakeAxis, CameraShakeFalloff, CameraShakeSpec, CameraTargets, Easing,
    PostProcessEffect, PostProcessPatch, TransformPatch,
};

/// Stop shake immediately; reset transform/effects together, waiting only once.
pub(crate) fn reset(
    targets: CameraTargets,
    duration: f32,
    easing: Easing,
    wait: bool,
) -> Vec<Action> {
    let mut transform = TransformPatch::default();
    transform.set_offset_x(0.0);
    transform.set_offset_y(0.0);
    transform.set_scale_x(1.0);
    transform.set_scale_y(1.0);
    let mut timed = Vec::with_capacity(4);
    timed.push(Action::ShakeCamera {
        targets,
        shake: CameraShakeSpec {
            amplitude: 0.0,
            frequency: 0.0,
            duration: 0.0,
            axis: CameraShakeAxis::Both,
            falloff: CameraShakeFalloff::Linear,
        },
        blocking: false,
    });
    timed.push(Action::SetCameraTransform {
        targets,
        transform,
        duration,
        easing,
        blocking: wait,
    });
    let defaults = PostProcessEffect::default();
    timed.push(Action::SetPostProcess {
        targets,
        effect: Box::new(PostProcessPatch {
            focal_distance: Some(None),
            blur_strength: Some(defaults.blur_strength),
            distortion_strength: Some(defaults.distortion_strength),
            vignette_intensity: Some(defaults.vignette_intensity),
            vignette_size: Some(defaults.vignette_size),
            blur_amount: Some(defaults.blur_amount),
            color_tone: Some(defaults.color_tone),
            color_tone_intensity: Some(defaults.color_tone_intensity),
            color_exposure: Some(defaults.color_exposure),
            color_brightness: Some(defaults.color_brightness),
            color_contrast: Some(defaults.color_contrast),
            color_saturation: Some(defaults.color_saturation),
            color_temperature: Some(defaults.color_temperature),
            old_film_intensity: Some(defaults.old_film_intensity),
            shock_intensity: Some(defaults.shock_intensity),
            godray_intensity: Some(defaults.godray_intensity),
            godray_angle: Some(defaults.godray_angle),
            godray_gain: Some(defaults.godray_gain),
            godray_lacunarity: Some(defaults.godray_lacunarity),
            godray_speed: Some(defaults.godray_speed),
            godray_parallel: Some(defaults.godray_parallel),
            godray_center_x: Some(defaults.godray_center_x),
            godray_center_y: Some(defaults.godray_center_y),
            lut_preset: Some(None),
            lut_intensity: Some(defaults.lut_intensity),
            bloom_intensity: Some(defaults.bloom_intensity),
            chromatic_aberration: Some(defaults.chromatic_aberration),
            pixelate_size: Some(defaults.pixelate_size),
            glitch_intensity: Some(defaults.glitch_intensity),
            crt_intensity: Some(defaults.crt_intensity),
            sharpen_strength: Some(defaults.sharpen_strength),
            radial_blur_strength: Some(defaults.radial_blur_strength),
            radial_blur_center_x: Some(defaults.radial_blur_center_x),
            radial_blur_center_y: Some(defaults.radial_blur_center_y),
            motion_blur_strength: Some(defaults.motion_blur_strength),
            motion_blur_angle: Some(defaults.motion_blur_angle),
            zoom_blur_strength: Some(defaults.zoom_blur_strength),
            zoom_blur_center_x: Some(defaults.zoom_blur_center_x),
            zoom_blur_center_y: Some(defaults.zoom_blur_center_y),
            light_leak_intensity: Some(defaults.light_leak_intensity),
            light_leak_angle: Some(defaults.light_leak_angle),
            lens_flare_intensity: Some(defaults.lens_flare_intensity),
            lens_flare_center_x: Some(defaults.lens_flare_center_x),
            lens_flare_center_y: Some(defaults.lens_flare_center_y),
            film_grain_intensity: Some(defaults.film_grain_intensity),
            film_grain_size: Some(defaults.film_grain_size),
            heat_haze_intensity: Some(defaults.heat_haze_intensity),
            heat_haze_speed: Some(defaults.heat_haze_speed),
            heat_haze_scale: Some(defaults.heat_haze_scale),
            water_ripple_intensity: Some(defaults.water_ripple_intensity),
            water_ripple_frequency: Some(defaults.water_ripple_frequency),
            water_ripple_speed: Some(defaults.water_ripple_speed),
            water_ripple_center_x: Some(defaults.water_ripple_center_x),
            water_ripple_center_y: Some(defaults.water_ripple_center_y),
            fog_intensity: Some(defaults.fog_intensity),
            fog_speed: Some(defaults.fog_speed),
            fog_scale: Some(defaults.fog_scale),
            vhs_intensity: Some(defaults.vhs_intensity),
            vhs_jitter: Some(defaults.vhs_jitter),
            vhs_noise: Some(defaults.vhs_noise),
            halftone_intensity: Some(defaults.halftone_intensity),
            halftone_scale: Some(defaults.halftone_scale),
            halftone_angle: Some(defaults.halftone_angle),
            dither_intensity: Some(defaults.dither_intensity),
            dither_levels: Some(defaults.dither_levels),
            outline_intensity: Some(defaults.outline_intensity),
            outline_thickness: Some(defaults.outline_thickness),
            eyelid_openness: Some(defaults.eyelid_openness),
            eyelid_width: Some(defaults.eyelid_width),
            eyelid_curvature: Some(defaults.eyelid_curvature),
            eyelid_softness: Some(defaults.eyelid_softness),
            eyelid_center_x: Some(defaults.eyelid_center_x),
            eyelid_center_y: Some(defaults.eyelid_center_y),
            ..Default::default()
        }),
        duration,
        easing,
        blocking: wait,
    });
    timed.push(Action::SetPostProcessV2 {
        targets,
        effect: Box::default(),
        duration,
        easing,
        blocking: wait,
    });
    let timed_len = timed.len();
    timed
        .into_iter()
        .enumerate()
        .map(|(index, action)| Action::Flow {
            action: Box::new(action),
            when: None,
            next: !wait || index + 1 < timed_len,
        })
        .collect()
}
