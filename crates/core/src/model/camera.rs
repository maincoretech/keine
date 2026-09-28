//! Typed camera authoring additions. Existing serialized patches keep their layouts.
use super::{
    CameraTargets, Easing, PostProcessEffect, PostProcessPatch, PostProcessV2, SpriteTransform,
    TransformPatch,
};
use serde::{Deserialize, Serialize};

// One inventory owns native source spelling, validation, Inspector controls and runtime sampling.
macro_rules! camera_fields {
    (transform { $( $tv:ident => $tn:literal : $tf:ident ),* $(,)? }
     effect { $( $ev:ident => $ef:ident ),* $(,)? }
     v2 { $( $vv:ident => $vf:ident ),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum CameraTweenField { $( $tv, )* $( $ev, )* FocalDistance, $( $vv, )* }
        impl CameraTweenField {
            pub const ALL: &'static [Self] = &[ $(Self::$tv,)* $(Self::$ev,)* Self::FocalDistance, $(Self::$vv,)* ];
            pub const fn name(self) -> &'static str { match self {
                $(Self::$tv => $tn,)* $(Self::$ev => stringify!($ef),)* Self::FocalDistance => "focal_distance", $(Self::$vv => stringify!($vf),)*
            }}
            pub fn from_name(name: &str) -> Option<Self> { Self::ALL.iter().copied().find(|field| field.name() == name) }
            pub const fn is_transform(self) -> bool { matches!(self, $(Self::$tv)|*) }
            pub const fn is_v2(self) -> bool { matches!(self, $(Self::$vv)|*) }
            pub fn restore_transform(self, target: &mut SpriteTransform, base: &SpriteTransform) { match self {
                $(Self::$tv => target.$tf = base.$tf,)* _ => {}
            }}
            pub fn interpolate_effect(self, output: &mut PostProcessEffect, from: &PostProcessEffect, to: &PostProcessEffect, progress: f32) {
                let lerp = |a: f32, b: f32| a + (b - a) * progress.clamp(0.0, 1.0);
                match self {
                    $(Self::$ev => output.$ef = lerp(from.$ef, to.$ef),)*
                    Self::FocalDistance => output.focal_distance = match (from.focal_distance, to.focal_distance) {
                        (Some(a), Some(b)) => Some(lerp(a,b)), (_, target) => target,
                    },
                    $(Self::$vv => output.v2.$vf = lerp(from.v2.$vf, to.v2.$vf),)* _ => {}
                }
            }
            pub fn restore_effect(self, target: &mut PostProcessEffect, base: &PostProcessEffect) { match self {
                $(Self::$ev => target.$ef = base.$ef,)* Self::FocalDistance => {
                    if base.focal_distance.is_some() && target.focal_distance.is_some() {
                        target.focal_distance = base.focal_distance;
                    }
                },
                $(Self::$vv => target.v2.$vf = base.v2.$vf,)* _ => {}
            }}
        }
    };
}
camera_fields! {
    transform {
        X => "x": offset_x,
        Y => "y": offset_y,
        Alpha => "alpha": alpha,
        ScaleX => "scale_x": scale_x,
        ScaleY => "scale_y": scale_y,
        Rotation => "rotation": rotation,
        Blur => "blur": blur,
        Width => "width": width,
        Height => "height": height,
    }
    effect {
        BlurStrength => blur_strength,
        DistortionStrength => distortion_strength,
        VignetteIntensity => vignette_intensity,
        VignetteSize => vignette_size,
        BlurAmount => blur_amount,
        ColorToneIntensity => color_tone_intensity,
        ColorExposure => color_exposure,
        ColorBrightness => color_brightness,
        ColorContrast => color_contrast,
        ColorSaturation => color_saturation,
        ColorTemperature => color_temperature,
        OldFilmIntensity => old_film_intensity,
        ShockIntensity => shock_intensity,
        GodrayIntensity => godray_intensity,
        GodrayAngle => godray_angle,
        GodrayGain => godray_gain,
        GodrayLacunarity => godray_lacunarity,
        GodraySpeed => godray_speed,
        GodrayCenterX => godray_center_x,
        GodrayCenterY => godray_center_y,
        LutIntensity => lut_intensity,
        BloomIntensity => bloom_intensity,
        ChromaticAberration => chromatic_aberration,
        PixelateSize => pixelate_size,
        GlitchIntensity => glitch_intensity,
        CrtIntensity => crt_intensity,
        SharpenStrength => sharpen_strength,
        RadialBlurStrength => radial_blur_strength,
        RadialBlurCenterX => radial_blur_center_x,
        RadialBlurCenterY => radial_blur_center_y,
        MotionBlurStrength => motion_blur_strength,
        MotionBlurAngle => motion_blur_angle,
        ZoomBlurStrength => zoom_blur_strength,
        ZoomBlurCenterX => zoom_blur_center_x,
        ZoomBlurCenterY => zoom_blur_center_y,
        LightLeakIntensity => light_leak_intensity,
        LightLeakAngle => light_leak_angle,
        LensFlareIntensity => lens_flare_intensity,
        LensFlareCenterX => lens_flare_center_x,
        LensFlareCenterY => lens_flare_center_y,
        FilmGrainIntensity => film_grain_intensity,
        FilmGrainSize => film_grain_size,
        HeatHazeIntensity => heat_haze_intensity,
        HeatHazeSpeed => heat_haze_speed,
        HeatHazeScale => heat_haze_scale,
        WaterRippleIntensity => water_ripple_intensity,
        WaterRippleFrequency => water_ripple_frequency,
        WaterRippleSpeed => water_ripple_speed,
        WaterRippleCenterX => water_ripple_center_x,
        WaterRippleCenterY => water_ripple_center_y,
        FogIntensity => fog_intensity,
        FogSpeed => fog_speed,
        FogScale => fog_scale,
        VhsIntensity => vhs_intensity,
        VhsJitter => vhs_jitter,
        VhsNoise => vhs_noise,
        HalftoneIntensity => halftone_intensity,
        HalftoneScale => halftone_scale,
        HalftoneAngle => halftone_angle,
        DitherIntensity => dither_intensity,
        DitherLevels => dither_levels,
        OutlineIntensity => outline_intensity,
        OutlineThickness => outline_thickness,
        EyelidOpenness => eyelid_openness,
        EyelidWidth => eyelid_width,
        EyelidCurvature => eyelid_curvature,
        EyelidSoftness => eyelid_softness,
        EyelidCenterX => eyelid_center_x,
        EyelidCenterY => eyelid_center_y,
    }
    v2 {
        MirrorShatterIntensity => mirror_shatter_intensity,
        MirrorShatterCenterX => mirror_shatter_center_x,
        MirrorShatterCenterY => mirror_shatter_center_y,
        MirrorShatterSpread => mirror_shatter_spread,
        MirrorShatterSeed => mirror_shatter_seed,
        SpeedLinesIntensity => speed_lines_intensity,
        SpeedLinesDensity => speed_lines_density,
        SpeedLinesAngle => speed_lines_angle,
        SpeedLinesSpeed => speed_lines_speed,
        SpeedLinesCenterX => speed_lines_center_x,
        SpeedLinesCenterY => speed_lines_center_y,
        SpeedLinesRegionX => speed_lines_region_x,
        SpeedLinesRegionY => speed_lines_region_y,
        SpeedLinesRegionWidth => speed_lines_region_width,
        SpeedLinesRegionHeight => speed_lines_region_height,
        SpeedLinesRegionFeather => speed_lines_region_feather,
    }
}

/// One source command applies instant values atomically, then animates only selected numeric fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraTweenSpec {
    pub targets: CameraTargets,
    pub transform: Option<TransformPatch>,
    pub effect: Option<Box<PostProcessPatch>>,
    pub v2: Option<Box<PostProcessV2>>,
    pub fields: Vec<CameraTweenField>,
    pub duration: f32,
    pub easing: Easing,
    pub blocking: bool,
}

/// Normalized authored jitter. Zero retains the original sinusoidal shake exactly.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct CameraShakeRandomness {
    pub amplitude: f32,
    pub frequency: f32,
}

impl CameraShakeRandomness {
    pub fn is_zero(self) -> bool {
        self.amplitude == 0.0 && self.frequency == 0.0
    }
}

impl CameraTweenSpec {
    /// Returns whether this command started an animation that blocks presentation.
    pub fn start(&self, state: &mut crate::State, blocking: bool) -> bool {
        let timed = self.duration > f32::EPSILON;
        let mut animated = false;
        if let Some(patch) = self.transform {
            state.camera_targets = self.targets;
            let to = patch.apply_to(state.camera_transform);
            let mut from = to;
            for field in &self.fields {
                field.restore_transform(&mut from, &state.camera_transform);
            }
            state.camera_transform = from;
            state.camera_transform_animation = if timed && from != to {
                animated = true;
                Some(crate::state::TransformAnimation {
                    from,
                    to,
                    elapsed: 0.0,
                    duration: self.duration,
                    easing: self.easing,
                    blocking,
                })
            } else {
                state.camera_transform = to;
                None
            };
        }
        if self.effect.is_some() || self.v2.is_some() {
            state.camera_effect_targets = self.targets;
            let mut to = self.effect.as_ref().map_or_else(
                || state.camera_effect.clone(),
                |patch| patch.apply_to(state.camera_effect.clone()),
            );
            if let Some(v2) = &self.v2 {
                to.v2 = (**v2).clone();
            }
            let mut from = to.clone();
            for field in &self.fields {
                field.restore_effect(&mut from, &state.camera_effect);
            }
            state.camera_effect = from.clone();
            state.camera_effect_animation = if timed && from != to {
                animated = true;
                Some(crate::PostProcessAnimation {
                    from,
                    to,
                    elapsed: 0.0,
                    duration: self.duration,
                    easing: self.easing,
                    fields: Some(self.fields.clone()),
                    blocking,
                })
            } else {
                state.camera_effect = to;
                None
            };
        }
        blocking && animated
    }
}
