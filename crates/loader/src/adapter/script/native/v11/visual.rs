use super::*;
use keine_core::{AnimationPreset, VisualFilterPatch};

pub(super) const VISUAL_COMMANDS: &[&str] = &[
    "sprite.transform",
    "background.transform",
    "sprite.animate",
    "sprite.transition",
    "sprite.update",
];

/// Syntax fields shared by lowering and source-based editor consumers.
pub(super) fn signature(name: &str) -> Option<(usize, &'static [&'static str])> {
    let signature: (usize, &[&str]) = match name {
        "sprite.transform" => (
            1,
            &[
                "light",
                "x",
                "y",
                "alpha",
                "scale",
                "scale_x",
                "scale_y",
                "rotation",
                "blur",
                "width",
                "height",
                "brightness",
                "contrast",
                "saturation",
                "duration",
                "easing",
            ],
        ),
        "background.transform" => (
            0,
            &[
                "x",
                "y",
                "alpha",
                "scale",
                "scale_x",
                "scale_y",
                "rotation",
                "blur",
                "width",
                "height",
                "brightness",
                "contrast",
                "saturation",
                "duration",
                "easing",
            ],
        ),
        "sprite.animate" => (2, &["duration"]),
        "sprite.transition" => (1, &["enter", "exit", "duration"]),
        "sprite.update" => (
            2,
            &[
                "position", "layout", "scale", "duration", "easing", "blocking",
            ],
        ),
        _ => return None,
    };
    Some(signature)
}

impl<'a> Parser<'a> {
    pub(super) fn parse_v11_visual_command(
        &self,
        name: &str,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<Action> {
        let (positional, named) = signature(name)?;
        let before = report.diagnostics.len();
        self.validate_signature(name, args, positional, named, report);
        if report.diagnostics.len() != before {
            return None;
        }
        match name {
            "sprite.transform" | "background.transform" => {
                let id = if name == "background.transform" {
                    "background".to_owned()
                } else {
                    self.v11_identifier(args.first(), "sprite target", report)?
                };
                let names = &[
                    "x", "y", "alpha", "scale_x", "scale_y", "rotation", "blur", "width", "height",
                ];
                let mut transform = TransformPatch::default();
                if let Some(scale) = self.v11_uniform_scale(args, report)? {
                    transform.set_scale_x(scale);
                    transform.set_scale_y(scale);
                }
                for field in names {
                    let Some(value) = self.checked_number(args, field, report)? else {
                        continue;
                    };
                    match *field {
                        "x" => transform.set_offset_x(value),
                        "y" => transform.set_offset_y(value),
                        "alpha" => transform.set_alpha(value),
                        "scale_x" => transform.set_scale_x(value),
                        "scale_y" => transform.set_scale_y(value),
                        "rotation" => transform.set_rotation(value),
                        "blur" => transform.set_blur(value),
                        "width" => transform.set_width(value),
                        "height" => transform.set_height(value),
                        _ => unreachable!(),
                    }
                }
                let filter = self.v11_filter_patch(args, report)?;
                if transform.is_empty() && filter.is_empty() {
                    report.diagnostics.push(
                        self.error(format!("{name}(...) requires at least one transform field")),
                    );
                    return None;
                }
                if transform.is_empty()
                    && self.named_duration_checked(args, "duration", report)? > 0.0
                {
                    report.diagnostics.push(self.error(
                        "duration animates transform fields; colour/lighting-only updates are immediate",
                    ));
                    return None;
                }
                let action = Action::SetTransform {
                    id,
                    transform,
                    duration: self.named_duration_checked(args, "duration", report)?,
                    easing: self.named_easing(args, "easing", report)?,
                };
                Some(if filter.is_empty() {
                    action
                } else {
                    Action::SpriteVisual {
                        action: Box::new(action),
                        filter,
                    }
                })
            }
            "sprite.animate" => Some(Action::Animate {
                target: self.v11_identifier(args.first(), "animation target", report)?,
                preset: self.v11_animation_preset(args.get(1), report)?,
                duration: self.v11_required_duration(args, "duration", report)?,
            }),
            "sprite.transition" => Some(Action::SetTransition {
                target: self.v11_identifier(args.first(), "transition target", report)?,
                enter: self.v11_optional_animation_preset(args, "enter", report)?,
                exit: self.v11_optional_animation_preset(args, "exit", report)?,
                duration: self.v11_required_duration(args, "duration", report)?,
            }),
            "sprite.update" => {
                let scale = self.v11_uniform_scale(args, report)?;
                Some(Action::PatchSprite {
                    id: self.v11_identifier(args.first(), "sprite ID", report)?,
                    image: self.v11_identifier(args.get(1), "sprite asset", report)?,
                    position: if let Some(arg) = self.named_arg(args, "position") {
                        Some(self.author_position(Some(arg), report)?)
                    } else {
                        None
                    },
                    layout: if self.named_arg(args, "layout").is_some() {
                        Some(self.v11_sprite_layout(args, report)?)
                    } else {
                        None
                    },
                    scale,
                    duration: self.named_duration_checked(args, "duration", report)?,
                    easing: self.named_easing(args, "easing", report)?,
                    blocking: self.v11_optional_bool(args, "blocking", true, report)?,
                })
            }
            _ => None,
        }
    }

    pub(in crate::adapter::script::native) fn v11_uniform_scale(
        &self,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<Option<f32>> {
        let scale = self.checked_number(args, "scale", report)?;
        if let Some(value) = scale {
            if value <= 0.0 {
                report
                    .diagnostics
                    .push(self.error("sprite scale must be positive"));
                return None;
            }
            if self.named_arg(args, "scale_x").is_some()
                || self.named_arg(args, "scale_y").is_some()
            {
                report
                    .diagnostics
                    .push(self.error("use either scale or scale_x/scale_y, not both"));
                return None;
            }
        }
        Some(scale)
    }

    pub(in crate::adapter::script::native) fn v11_filter_patch(
        &self,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<VisualFilterPatch> {
        let filter = VisualFilterPatch {
            environment_light: if args.iter().any(|arg| arg.name.as_deref() == Some("light")) {
                Some(self.v11_optional_bool(args, "light", true, report)?)
            } else {
                None
            },
            brightness: self.checked_number(args, "brightness", report)?,
            contrast: self.checked_number(args, "contrast", report)?,
            saturation: self.checked_number(args, "saturation", report)?,
        };
        if [filter.brightness, filter.contrast, filter.saturation]
            .into_iter()
            .flatten()
            .any(|value| !(0.0..=4.0).contains(&value))
        {
            report
                .diagnostics
                .push(self.error("sprite colour fields must be from 0 to 4"));
            return None;
        }
        Some(filter)
    }

    fn v11_optional_animation_preset(
        &self,
        args: &[Argument],
        name: &str,
        report: &mut ParseReport,
    ) -> Option<Option<AnimationPreset>> {
        let Some(arg) = self.named_arg(args, name) else {
            return Some(None);
        };
        if self.argument_identifier(arg).as_deref() == Some("none") {
            return Some(None);
        }
        self.v11_animation_preset(Some(arg), report).map(Some)
    }

    fn v11_animation_preset(
        &self,
        arg: Option<&Argument>,
        report: &mut ParseReport,
    ) -> Option<AnimationPreset> {
        let arg = arg?;
        let name = self.argument_identifier(arg);
        let preset = match name.as_deref() {
            Some("enter") => AnimationPreset::Enter,
            Some("exit") => AnimationPreset::Exit,
            Some("shake") => AnimationPreset::Shake,
            Some("enter_from_bottom") => AnimationPreset::EnterFromBottom,
            Some("enter_from_left") => AnimationPreset::EnterFromLeft,
            Some("enter_from_right") => AnimationPreset::EnterFromRight,
            Some("move_front_and_back") => AnimationPreset::MoveFrontAndBack,
            Some("blur") => AnimationPreset::Blur,
            Some("old_film") => AnimationPreset::OldFilm,
            Some("dot_film") => AnimationPreset::DotFilm,
            Some("reflection_film") => AnimationPreset::ReflectionFilm,
            Some("glitch_film") => AnimationPreset::GlitchFilm,
            Some("rgb_film") => AnimationPreset::RgbFilm,
            Some("godray_film") => AnimationPreset::GodrayFilm,
            Some("remove_film") => AnimationPreset::RemoveFilm,
            Some("shockwave_in") => AnimationPreset::ShockwaveIn,
            Some("shockwave_out") => AnimationPreset::ShockwaveOut,
            _ if arg.token_indices.len() == 1
                && self.tokens[arg.token_indices[0]].kind == NativeTokenKind::String =>
            {
                AnimationPreset::Custom(self.v11_string(Some(arg), "custom animation", report)?)
            }
            _ => {
                report
                    .diagnostics
                    .push(self.error("unknown animation preset; use a built-in preset name"));
                return None;
            }
        };
        Some(preset)
    }

    fn v11_required_duration(
        &self,
        args: &[Argument],
        name: &str,
        report: &mut ParseReport,
    ) -> Option<f32> {
        let duration = self
            .named_arg(args, name)
            .and_then(|arg| self.argument_duration(arg));
        match duration {
            Some(value) => Some(value),
            None => {
                report.diagnostics.push(self.error(format!(
                    "`{name}` requires a non-negative `ms` or `s` duration"
                )));
                None
            }
        }
    }
}
