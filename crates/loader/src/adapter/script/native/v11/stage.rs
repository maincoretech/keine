use super::*;
use keine_core::{
    CameraShakeSpec, ParticleEffect, SceneFit, SceneLayerLayout, StageAnimation, StageAudioCue,
    StageAudioKind, StageEvent, StageEventKind, StageKeyframe, StageProperty, StageSceneCue,
    StageSceneLayer, StageTarget, StageTrack,
};

pub(super) const STAGE_COMMANDS: &[&str] = &["stage.animate"];

// One inventory owns accepted native names and the Editor property selector.
const STAGE_PROPERTIES: &[(&str, StageProperty)] = &[
    ("x", StageProperty::X),
    ("y", StageProperty::Y),
    ("zoom", StageProperty::Zoom),
    ("scale_x", StageProperty::ScaleX),
    ("scale_y", StageProperty::ScaleY),
    ("alpha", StageProperty::Alpha),
    ("rotation", StageProperty::Rotation),
    ("width", StageProperty::Width),
    ("height", StageProperty::Height),
    ("focal_distance", StageProperty::FocalDistance),
    ("blur_strength", StageProperty::BlurStrength),
    ("distortion_strength", StageProperty::DistortionStrength),
    ("vignette_intensity", StageProperty::VignetteIntensity),
    ("vignette_size", StageProperty::VignetteSize),
    ("blur_amount", StageProperty::BlurAmount),
    ("color_tone_intensity", StageProperty::ColorToneIntensity),
    ("color_exposure", StageProperty::ColorExposure),
    ("color_brightness", StageProperty::ColorBrightness),
    ("color_contrast", StageProperty::ColorContrast),
    ("color_saturation", StageProperty::ColorSaturation),
    ("color_temperature", StageProperty::ColorTemperature),
    ("old_film_intensity", StageProperty::OldFilmIntensity),
    ("shock_intensity", StageProperty::ShockIntensity),
    ("godray_intensity", StageProperty::GodrayIntensity),
    ("godray_angle", StageProperty::GodrayAngle),
    ("godray_gain", StageProperty::GodrayGain),
    ("godray_lacunarity", StageProperty::GodrayLacunarity),
    ("godray_speed", StageProperty::GodraySpeed),
    ("godray_center_x", StageProperty::GodrayCenterX),
    ("godray_center_y", StageProperty::GodrayCenterY),
    ("lut_intensity", StageProperty::LutIntensity),
    ("bloom_intensity", StageProperty::BloomIntensity),
    ("chromatic_aberration", StageProperty::ChromaticAberration),
    ("pixelate_size", StageProperty::PixelateSize),
    ("glitch_intensity", StageProperty::GlitchIntensity),
    ("crt_intensity", StageProperty::CrtIntensity),
    ("sharpen_strength", StageProperty::SharpenStrength),
    ("radial_blur_strength", StageProperty::RadialBlurStrength),
    ("radial_blur_center_x", StageProperty::RadialBlurCenterX),
    ("radial_blur_center_y", StageProperty::RadialBlurCenterY),
    ("motion_blur_strength", StageProperty::MotionBlurStrength),
    ("motion_blur_angle", StageProperty::MotionBlurAngle),
    ("zoom_blur_strength", StageProperty::ZoomBlurStrength),
    ("zoom_blur_center_x", StageProperty::ZoomBlurCenterX),
    ("zoom_blur_center_y", StageProperty::ZoomBlurCenterY),
    ("light_leak_intensity", StageProperty::LightLeakIntensity),
    ("light_leak_angle", StageProperty::LightLeakAngle),
    ("lens_flare_intensity", StageProperty::LensFlareIntensity),
    ("lens_flare_center_x", StageProperty::LensFlareCenterX),
    ("lens_flare_center_y", StageProperty::LensFlareCenterY),
    ("film_grain_intensity", StageProperty::FilmGrainIntensity),
    ("film_grain_size", StageProperty::FilmGrainSize),
    ("heat_haze_intensity", StageProperty::HeatHazeIntensity),
    ("heat_haze_speed", StageProperty::HeatHazeSpeed),
    ("heat_haze_scale", StageProperty::HeatHazeScale),
    (
        "water_ripple_intensity",
        StageProperty::WaterRippleIntensity,
    ),
    (
        "water_ripple_frequency",
        StageProperty::WaterRippleFrequency,
    ),
    ("water_ripple_speed", StageProperty::WaterRippleSpeed),
    ("water_ripple_center_x", StageProperty::WaterRippleCenterX),
    ("water_ripple_center_y", StageProperty::WaterRippleCenterY),
    ("fog_intensity", StageProperty::FogIntensity),
    ("fog_speed", StageProperty::FogSpeed),
    ("fog_scale", StageProperty::FogScale),
    ("vhs_intensity", StageProperty::VhsIntensity),
    ("vhs_jitter", StageProperty::VhsJitter),
    ("vhs_noise", StageProperty::VhsNoise),
    ("halftone_intensity", StageProperty::HalftoneIntensity),
    ("halftone_scale", StageProperty::HalftoneScale),
    ("halftone_angle", StageProperty::HalftoneAngle),
    ("dither_intensity", StageProperty::DitherIntensity),
    ("dither_levels", StageProperty::DitherLevels),
    ("outline_intensity", StageProperty::OutlineIntensity),
    ("outline_thickness", StageProperty::OutlineThickness),
    ("eyelid_openness", StageProperty::EyelidOpenness),
    ("eyelid_width", StageProperty::EyelidWidth),
    ("eyelid_curvature", StageProperty::EyelidCurvature),
    ("eyelid_softness", StageProperty::EyelidSoftness),
    ("eyelid_center_x", StageProperty::EyelidCenterX),
    ("eyelid_center_y", StageProperty::EyelidCenterY),
];

pub fn native_stage_property_names() -> impl Iterator<Item = &'static str> {
    STAGE_PROPERTIES.iter().map(|(name, _)| *name)
}

struct StageRow {
    name: String,
    args: Vec<Argument>,
    children: Vec<StageRow>,
}

impl<'a> Parser<'a> {
    pub(super) fn parse_v11_stage_command(
        &mut self,
        name: &str,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<Action> {
        let before = report.diagnostics.len();
        self.validate_signature(
            name,
            args,
            1,
            &[
                "duration",
                "repeat",
                "infinite",
                "playback_rate",
                "blocking",
            ],
            report,
        );
        let rows = self.take_stage_rows(0, report)?;
        if report.diagnostics.len() != before {
            return None;
        }
        let duration = self.v11_stage_duration(args, "duration", report)?;
        let repeat = self.v11_stage_count(args, "repeat", 0, report)?;
        let playback_rate = self.v11_optional_number(args, "playback_rate", 1.0, report)?;
        if duration <= 0.0 || playback_rate <= 0.0 {
            report
                .diagnostics
                .push(self.error("stage duration and playback_rate must be positive"));
            return None;
        }
        let mut tracks = Vec::new();
        let mut events = Vec::new();
        for row in &rows {
            if row.name == "track" {
                tracks.push(self.v11_stage_track(row, report)?);
            } else if row.name.starts_with("event.") {
                events.push(self.v11_stage_event(row, report)?);
            } else {
                report
                    .diagnostics
                    .push(self.error("stage children must be track(...) or event.*(...)"));
                return None;
            }
        }
        if tracks.is_empty() && events.is_empty() {
            report
                .diagnostics
                .push(self.error("stage.animate(...) requires a track or event"));
            return None;
        }
        if report.diagnostics.len() != before {
            return None;
        }
        Some(Action::StageAnimation {
            animation: StageAnimation {
                id: self.v11_identifier(args.first(), "stage animation ID", report)?,
                duration,
                tracks,
                events,
                repeat,
                infinite: self.v11_optional_bool(args, "infinite", false, report)?,
                playback_rate,
                blocking: self.v11_optional_bool(args, "blocking", true, report)?,
            },
        })
    }

    fn take_stage_rows(&mut self, depth: usize, report: &mut ParseReport) -> Option<Vec<StageRow>> {
        if depth > 3 {
            report
                .diagnostics
                .push(self.error("stage rows are nested too deeply"));
            return None;
        }
        if !self.eat("{") {
            report
                .diagnostics
                .push(self.error("expected `{` after stage row"));
            return None;
        }
        let mut rows = Vec::new();
        while !self.eof() && self.peek_text() != Some("}") {
            let Some(mut name) = self.take_identifier() else {
                report.diagnostics.push(self.error("expected stage row"));
                return None;
            };
            while self.eat(".") {
                let Some(part) = self.take_identifier() else {
                    report
                        .diagnostics
                        .push(self.error("expected name after `.`"));
                    return None;
                };
                name.push('.');
                name.push_str(&part);
            }
            let args = self.take_call_args(report);
            let children = if self.peek_text() == Some("{") {
                self.take_stage_rows(depth + 1, report)?
            } else {
                Vec::new()
            };
            rows.push(StageRow {
                name,
                args,
                children,
            });
            if self.peek_text() == Some("}") {
                break;
            }
            if !self.eat(",") {
                report
                    .diagnostics
                    .push(self.error("expected `,` between stage rows"));
                return None;
            }
        }
        if !self.eat("}") {
            report
                .diagnostics
                .push(self.error("unterminated stage rows"));
            return None;
        }
        Some(rows)
    }

    fn v11_stage_duration(
        &self,
        args: &[Argument],
        name: &str,
        report: &mut ParseReport,
    ) -> Option<f32> {
        if self.named_arg(args, name).is_none() {
            report
                .diagnostics
                .push(self.error(format!("`{name}` requires an `ms` or `s` duration")));
            return None;
        }
        self.named_duration_checked(args, name, report)
    }

    fn v11_stage_count(
        &self,
        args: &[Argument],
        name: &str,
        default: u32,
        report: &mut ParseReport,
    ) -> Option<u32> {
        let value = self.v11_optional_number(args, name, default as f32, report)?;
        if value.fract() != 0.0 || value < 0.0 || value as f64 > u32::MAX as f64 {
            report
                .diagnostics
                .push(self.error(format!("`{name}` requires a non-negative 32-bit integer")));
            return None;
        }
        Some(value as u32)
    }

    fn v11_stage_track(&self, row: &StageRow, report: &mut ParseReport) -> Option<StageTrack> {
        let before = report.diagnostics.len();
        self.validate_signature("track", &row.args, 2, &["image", "muted"], report);
        if report.diagnostics.len() != before {
            return None;
        }
        if row.children.is_empty() {
            report
                .diagnostics
                .push(self.error("track(...) requires key(...) rows"));
            return None;
        }
        let target =
            self.v11_stage_target(row.args.first(), self.named_arg(&row.args, "image"), report)?;
        let property = self.v11_stage_property(row.args.get(1), report)?;
        let mut keyframes = Vec::new();
        for child in &row.children {
            if child.name != "key" || !child.children.is_empty() {
                report
                    .diagnostics
                    .push(self.error("track children must be key(...)"));
                return None;
            }
            self.validate_signature("key", &child.args, 0, &["time", "value", "easing"], report);
            keyframes.push(StageKeyframe {
                time: self.v11_stage_duration(&child.args, "time", report)?,
                value: self.v11_named_number(&child.args, "value", report)?,
                easing: self.named_easing(&child.args, "easing", report)?,
            });
        }
        if report.diagnostics.len() != before {
            return None;
        }
        Some(StageTrack {
            target,
            property,
            keyframes,
            muted: self.v11_optional_bool(&row.args, "muted", false, report)?,
        })
    }

    fn v11_stage_target(
        &self,
        arg: Option<&Argument>,
        image: Option<&Argument>,
        report: &mut ParseReport,
    ) -> Option<StageTarget> {
        let Some(arg) = arg else {
            report
                .diagnostics
                .push(self.error("track requires a target"));
            return None;
        };
        let tokens = &arg.token_indices;
        if tokens.len() == 1 && self.text(tokens[0]) == "camera" {
            if image.is_some() {
                report
                    .diagnostics
                    .push(self.error("camera track cannot have an image"));
                return None;
            }
            return Some(StageTarget::Camera);
        }
        if let [head, open, id, close] = tokens.as_slice()
            && self.text(*head) == "character"
            && self.text(*open) == "("
            && self.tokens[*id].kind == NativeTokenKind::Identifier
            && self.text(*close) == ")"
        {
            return Some(StageTarget::Character {
                id: self.text(*id).to_owned(),
                image: match image {
                    Some(arg) => Some(self.v11_identifier(Some(arg), "character image", report)?),
                    None => None,
                },
            });
        }
        if let [head, open, id, close] = tokens.as_slice()
            && self.text(*head) == "scene_layer"
            && self.text(*open) == "("
            && self.tokens[*id].kind == NativeTokenKind::Identifier
            && self.text(*close) == ")"
        {
            if image.is_some() {
                report
                    .diagnostics
                    .push(self.error("invalid scene-layer track target"));
                return None;
            }
            return Some(StageTarget::SceneLayer {
                id: self.text(*id).to_owned(),
            });
        }
        report
            .diagnostics
            .push(self.error("track target must be camera, character(id), or scene_layer(id)"));
        None
    }

    fn v11_stage_event(&self, row: &StageRow, report: &mut ParseReport) -> Option<StageEvent> {
        let before = report.diagnostics.len();
        let (positional, fields): (usize, Vec<&str>) = match row.name.as_str() {
            "event.camera.shake" => (
                0,
                vec![
                    "time",
                    "amplitude",
                    "frequency",
                    "amplitude_randomness",
                    "frequency_randomness",
                    "duration",
                    "axis",
                    "falloff",
                ],
            ),
            "event.camera.patch" => (
                0,
                std::iter::once("time")
                    .chain(std::iter::once("targets"))
                    .chain(
                        super::effects::PATCH_FIELDS
                            .iter()
                            .copied()
                            .filter(|field| {
                                !matches!(*field, "duration" | "easing" | "blocking" | "tween")
                            }),
                    )
                    .collect(),
            ),
            "event.particle" => (
                2,
                vec![
                    "time", "texture", "count", "wind", "gravity", "fade_in", "duration",
                    "fade_out",
                ],
            ),
            "event.scene" => (
                1,
                vec![
                    "time",
                    "transition",
                    "reset_camera",
                    "fit",
                    "x",
                    "y",
                    "anchor_x",
                    "anchor_y",
                    "width",
                    "height",
                ],
            ),
            "event.audio" => (
                3,
                vec!["time", "volume", "loop", "duration", "fade_in", "fade_out"],
            ),
            _ => {
                report
                    .diagnostics
                    .push(self.error("unknown stage event type"));
                return None;
            }
        };
        self.validate_signature(&row.name, &row.args, positional, &fields, report);
        if report.diagnostics.len() != before {
            return None;
        }
        let time = self.v11_stage_duration(&row.args, "time", report)?;
        let kind = match row.name.as_str() {
            "event.camera.shake" => self.v11_stage_shake(&row.args, report)?,
            "event.camera.patch" => {
                let targets = match self.named_arg(&row.args, "targets") {
                    Some(arg) => Some(self.camera_targets(std::slice::from_ref(arg), report)?),
                    None => None,
                };
                StageEventKind::CameraPatch {
                    targets,
                    effect: Box::new(self.v11_post_process_patch(&row.args, report)?),
                }
            }
            "event.particle" => self.v11_stage_particle(&row.args, report)?,
            "event.scene" => self.v11_stage_scene(row, report)?,
            "event.audio" => self.v11_stage_audio(&row.args, report)?,
            _ => unreachable!(),
        };
        if row.name != "event.scene" && !row.children.is_empty() {
            report
                .diagnostics
                .push(self.error("this stage event cannot contain child rows"));
            return None;
        }
        Some(StageEvent { time, kind })
    }

    fn v11_stage_shake(
        &self,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<StageEventKind> {
        let amplitude = self.v11_named_number(args, "amplitude", report)?;
        let frequency = self.v11_named_number(args, "frequency", report)?;
        let duration = self.v11_stage_duration(args, "duration", report)?;
        let axis = match self.named_identifier(args, "axis").as_deref() {
            None | Some("both") => CameraShakeAxis::Both,
            Some("x") => CameraShakeAxis::X,
            Some("y") => CameraShakeAxis::Y,
            _ => {
                report
                    .diagnostics
                    .push(self.error("unknown stage shake axis"));
                return None;
            }
        };
        let falloff = match self.named_identifier(args, "falloff").as_deref() {
            None | Some("linear") => CameraShakeFalloff::Linear,
            Some("exponential") => CameraShakeFalloff::Exponential,
            _ => {
                report
                    .diagnostics
                    .push(self.error("unknown stage shake falloff"));
                return None;
            }
        };
        if amplitude < 0.0 || frequency <= 0.0 {
            report
                .diagnostics
                .push(self.error("invalid stage shake amplitude or frequency"));
            return None;
        }
        let randomness = self.camera_randomness(args, report)?;
        let shake = CameraShakeSpec {
            amplitude,
            frequency,
            duration,
            axis,
            falloff,
        };
        Some(if randomness.is_zero() {
            StageEventKind::CameraShake(shake)
        } else {
            StageEventKind::CameraShakeRandomized { shake, randomness }
        })
    }

    fn v11_stage_particle(
        &self,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<StageEventKind> {
        let count = self.v11_stage_count(args, "count", 0, report)?;
        if count > u16::MAX as u32 {
            report
                .diagnostics
                .push(self.error("particle count exceeds 65535"));
            return None;
        }
        let texture = match self.named_arg(args, "texture") {
            Some(arg) => Some(self.v11_identifier(Some(arg), "particle texture", report)?),
            None => None,
        };
        Some(StageEventKind::Particle {
            id: self.v11_identifier(args.first(), "particle ID", report)?,
            effect: ParticleEffect {
                texture,
                preset: self.v11_identifier(args.get(1), "particle preset", report)?,
                count: count as u16,
                wind: self.checked_number(args, "wind", report)?,
                gravity: self.checked_number(args, "gravity", report)?,
                fade_in: self.named_duration_checked(args, "fade_in", report)?,
            },
            duration: self.v11_stage_duration(args, "duration", report)?,
            fade_out: self.named_duration_checked(args, "fade_out", report)?,
        })
    }

    fn v11_stage_scene(&self, row: &StageRow, report: &mut ParseReport) -> Option<StageEventKind> {
        let args = &row.args;
        let fit = match self.named_identifier(args, "fit").as_deref() {
            None | Some("by_height") => SceneFit::ByHeight,
            Some("by_width") => SceneFit::ByWidth,
            Some("cover") => SceneFit::Cover,
            Some("contain") => SceneFit::Contain,
            Some("stretch") => SceneFit::Stretch,
            Some("center") => SceneFit::Center,
            _ => {
                report
                    .diagnostics
                    .push(self.error("unknown stage scene fit"));
                return None;
            }
        };
        let width = self.checked_number(args, "width", report)?;
        let height = self.checked_number(args, "height", report)?;
        if width.is_some() != height.is_some() {
            report
                .diagnostics
                .push(self.error("scene layout size requires width and height"));
            return None;
        }
        let mut layers = Vec::new();
        for child in &row.children {
            let before = report.diagnostics.len();
            if child.name != "layer" || !child.children.is_empty() {
                report
                    .diagnostics
                    .push(self.error("scene event children must be layer(...)"));
                return None;
            }
            self.validate_signature("layer", &child.args, 2, &["distance", "x", "y"], report);
            if report.diagnostics.len() != before {
                return None;
            }
            layers.push(StageSceneLayer {
                id: self.v11_identifier(child.args.first(), "scene layer ID", report)?,
                image: self.v11_identifier(child.args.get(1), "scene layer asset", report)?,
                distance: self.v11_optional_number(&child.args, "distance", 0.0, report)?,
                offset: [
                    self.v11_optional_number(&child.args, "x", 0.0, report)?,
                    self.v11_optional_number(&child.args, "y", 0.0, report)?,
                ],
            });
        }
        Some(StageEventKind::Scene(StageSceneCue {
            scene_id: self.v11_identifier(args.first(), "scene ID", report)?,
            transition: self.named_transition(args, report),
            reset_camera: self.v11_optional_bool(args, "reset_camera", false, report)?,
            layout: SceneLayerLayout {
                fit,
                position: [
                    self.v11_optional_number(args, "x", 0.0, report)?,
                    self.v11_optional_number(args, "y", 0.0, report)?,
                ],
                anchor: [
                    self.v11_optional_number(args, "anchor_x", 0.5, report)?,
                    self.v11_optional_number(args, "anchor_y", 0.5, report)?,
                ],
                size: width.zip(height).map(|(w, h)| [w, h]),
            },
            layers,
        }))
    }

    fn v11_stage_audio(
        &self,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<StageEventKind> {
        let kind = match self
            .v11_identifier(args.get(1), "audio kind", report)?
            .as_str()
        {
            "bgm" => StageAudioKind::Bgm,
            "effect" => StageAudioKind::Effect,
            "vocal" => StageAudioKind::Vocal,
            _ => {
                report
                    .diagnostics
                    .push(self.error("audio kind must be bgm, effect, or vocal"));
                return None;
            }
        };
        let volume = self.v11_optional_number(args, "volume", 1.0, report)?;
        if !(0.0..=1.0).contains(&volume) {
            report
                .diagnostics
                .push(self.error("audio volume must be between 0 and 1"));
            return None;
        }
        Some(StageEventKind::Audio(StageAudioCue {
            id: self.v11_identifier(args.first(), "audio ID", report)?,
            kind,
            file: self.v11_identifier(args.get(2), "audio asset", report)?,
            volume,
            looped: self.v11_optional_bool(args, "loop", false, report)?,
            duration: self.named_duration_checked(args, "duration", report)?,
            fade_in: self.named_duration_checked(args, "fade_in", report)?,
            fade_out: self.named_duration_checked(args, "fade_out", report)?,
        }))
    }
}

impl<'a> Parser<'a> {
    fn v11_stage_property(
        &self,
        arg: Option<&Argument>,
        report: &mut ParseReport,
    ) -> Option<StageProperty> {
        let name = arg.and_then(|arg| self.argument_identifier(arg));
        match STAGE_PROPERTIES
            .iter()
            .find(|(candidate, _)| Some(*candidate) == name.as_deref())
        {
            Some((_, property)) => Some(*property),
            _ => {
                report
                    .diagnostics
                    .push(self.error("unknown stage property"));
                None
            }
        }
    }
}
