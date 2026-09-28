use super::*;
use keine_core::{AnimationPreset, VisualFilter};

pub(super) const VISUAL_COMMANDS: &[&str] = &[
    "sprite.offset",
    "sprite.transform",
    "background.transform",
    "sprite.filter",
    "sprite.animate",
    "sprite.transition",
    "sprite.update",
];

impl<'a> Parser<'a> {
    pub(super) fn parse_v11_visual_command(
        &self,
        name: &str,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<Action> {
        let (positional, named): (usize, &[&str]) = match name {
            "sprite.offset" => (1, &["x", "y", "duration", "easing"]),
            "sprite.transform" => (
                1,
                &[
                    "x", "y", "alpha", "scale_x", "scale_y", "rotation", "blur", "width", "height",
                    "duration", "easing",
                ],
            ),
            "background.transform" => (
                0,
                &[
                    "x", "y", "alpha", "scale_x", "scale_y", "rotation", "blur", "width", "height",
                    "duration", "easing",
                ],
            ),
            "sprite.filter" => (1, &["blur", "brightness", "contrast", "saturation"]),
            "sprite.animate" => (2, &["duration"]),
            "sprite.transition" => (1, &["enter", "exit", "duration"]),
            "sprite.update" => (
                2,
                &[
                    "position",
                    "anchor_offset",
                    "y",
                    "layout",
                    "layout_height",
                    "layout_fit",
                    "layout_x",
                    "layout_y",
                    "layout_anchor_x",
                    "layout_anchor_y",
                    "layout_width",
                    "layout_canvas_width",
                    "layout_canvas_height",
                    "layout_rect_x",
                    "layout_rect_y",
                    "layout_rect_width",
                    "layout_rect_height",
                    "layout_height_ratio",
                    "scale",
                    "duration",
                    "easing",
                    "blocking",
                ],
            ),
            _ => return None,
        };
        let before = report.diagnostics.len();
        self.validate_signature(name, args, positional, named, report);
        if report.diagnostics.len() != before {
            return None;
        }
        match name {
            "sprite.offset" | "sprite.transform" | "background.transform" => {
                let id = if name == "background.transform" {
                    "background".to_owned()
                } else {
                    self.v11_identifier(args.first(), "sprite target", report)?
                };
                let names: &[&str] = if name == "sprite.offset" {
                    &["x", "y"]
                } else {
                    &[
                        "x", "y", "alpha", "scale_x", "scale_y", "rotation", "blur", "width",
                        "height",
                    ]
                };
                let mut transform = TransformPatch::default();
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
                if transform.is_empty() {
                    report.diagnostics.push(
                        self.error(format!("{name}(...) requires at least one transform field")),
                    );
                    return None;
                }
                Some(Action::SetTransform {
                    id,
                    transform,
                    duration: self.named_duration_checked(args, "duration", report)?,
                    easing: self.named_easing(args, "easing", report)?,
                })
            }
            "sprite.filter" => {
                let target = self.v11_identifier(args.first(), "filter target", report)?;
                let mut filter = VisualFilter::default();
                for field in ["blur", "brightness", "contrast", "saturation"] {
                    let Some(value) = self.checked_number(args, field, report)? else {
                        continue;
                    };
                    match field {
                        "blur" => filter.blur = value,
                        "brightness" => filter.brightness = value,
                        "contrast" => filter.contrast = value,
                        "saturation" => filter.saturation = value,
                        _ => unreachable!(),
                    }
                }
                Some(Action::SetFilter { target, filter })
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
                let scale = self.v11_optional_number(args, "scale", 1.0, report)?;
                if scale <= 0.0 {
                    report
                        .diagnostics
                        .push(self.error("sprite scale must be positive"));
                    return None;
                }
                Some(Action::UpdateSprite {
                    id: self.v11_identifier(args.first(), "sprite ID", report)?,
                    image: self.v11_identifier(args.get(1), "sprite asset", report)?,
                    position: self.v11_position(args, "position", report)?,
                    layout: self.v11_sprite_layout(args, report)?,
                    scale,
                    duration: self.named_duration_checked(args, "duration", report)?,
                    easing: self.named_easing(args, "easing", report)?,
                    blocking: self.v11_optional_bool(args, "blocking", true, report)?,
                })
            }
            _ => None,
        }
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
