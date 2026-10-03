use super::*;
use keine_core::{ParticleEffect, SceneMouseParallax, SystemMessageMode, SystemMessageSpec};

pub(super) const SHELL_COMMANDS: &[&str] = &[
    "screen.curtain.show",
    "screen.curtain.hide",
    "text.float",
    "scene.parallax",
    "particle.show",
    "ui.message",
];

/// Syntax fields shared by lowering and source-based editor consumers.
pub(super) fn signature(name: &str) -> Option<(usize, &'static [&'static str])> {
    let signature: (usize, &[&str]) = match name {
        "screen.curtain.show" | "screen.curtain.hide" => (0, &["color", "duration"]),
        "text.float" => (
            1,
            &[
                "x",
                "y",
                "font_size",
                "color",
                "fade_in",
                "hold",
                "fade_out",
                "blocking",
            ],
        ),
        "scene.parallax" => (
            0,
            &[
                "amplitude_percent",
                "edge_ease_percent",
                "return_to_center_on_leave",
                "scale",
            ],
        ),
        "particle.show" => (2, &["texture", "count", "wind", "gravity", "fade_in"]),
        "ui.message" => (
            1,
            &["title", "message", "confirm_text", "cancel_text", "result"],
        ),
        _ => return None,
    };
    Some(signature)
}

impl<'a> Parser<'a> {
    pub(super) fn parse_v11_shell_command(
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
            "screen.curtain.show" | "screen.curtain.hide" => Some(Action::Curtain {
                visible: name.ends_with("show"),
                color: self.v11_optional_color(args, "color", [0.0, 0.0, 0.0, 1.0], report)?,
                duration: self.named_duration_checked(args, "duration", report)?,
            }),
            "text.float" => {
                let font_size = self.v11_optional_number(args, "font_size", 48.0, report)?;
                if font_size <= 0.0 {
                    report
                        .diagnostics
                        .push(self.error("`font_size` must be positive"));
                    return None;
                }
                Some(Action::FloatingText {
                    text: self.v11_string(args.first(), "floating text", report)?,
                    position: [
                        self.v11_optional_number(args, "x", 960.0, report)?,
                        self.v11_optional_number(args, "y", 540.0, report)?,
                    ],
                    font_size,
                    color: self.v11_optional_color(args, "color", [1.0, 1.0, 1.0, 1.0], report)?,
                    fade_in: self.named_duration_checked(args, "fade_in", report)?,
                    hold: self.named_duration_checked(args, "hold", report)?,
                    fade_out: self.named_duration_checked(args, "fade_out", report)?,
                    blocking: self.v11_optional_bool(args, "blocking", true, report)?,
                })
            }
            "scene.parallax" => {
                let defaults = SceneMouseParallax::default();
                let parallax = SceneMouseParallax {
                    amplitude_percent: self.v11_optional_number(
                        args,
                        "amplitude_percent",
                        defaults.amplitude_percent,
                        report,
                    )?,
                    edge_ease_percent: self.v11_optional_number(
                        args,
                        "edge_ease_percent",
                        defaults.edge_ease_percent,
                        report,
                    )?,
                    return_to_center_on_leave: self.v11_optional_bool(
                        args,
                        "return_to_center_on_leave",
                        defaults.return_to_center_on_leave,
                        report,
                    )?,
                    scale: self.v11_optional_number(args, "scale", defaults.scale, report)?,
                };
                if parallax.scale <= 0.0 {
                    report
                        .diagnostics
                        .push(self.error("parallax scale must be positive"));
                    return None;
                }
                Some(Action::ConfigureSceneMouseParallax {
                    parallax: Some(parallax),
                })
            }
            "particle.show" => {
                let count = self.v11_optional_number(args, "count", 0.0, report)?;
                if count.fract() != 0.0 || !(0.0..=u16::MAX as f32).contains(&count) {
                    report
                        .diagnostics
                        .push(self.error("`count` requires an integer between 0 and 65535"));
                    return None;
                }
                Some(Action::ShowParticles {
                    id: self.v11_identifier(args.first(), "particle ID", report)?,
                    effect: ParticleEffect {
                        preset: self.v11_identifier(args.get(1), "particle preset", report)?,
                        texture: match self.named_arg(args, "texture") {
                            Some(arg) => {
                                Some(self.v11_identifier(Some(arg), "particle texture", report)?)
                            }
                            None => None,
                        },
                        count: count as u16,
                        wind: self.checked_number(args, "wind", report)?,
                        gravity: self.checked_number(args, "gravity", report)?,
                        fade_in: self.named_duration_checked(args, "fade_in", report)?,
                    },
                })
            }
            "ui.message" => {
                let mode = match self
                    .v11_identifier(args.first(), "message mode", report)?
                    .as_str()
                {
                    "alert" => SystemMessageMode::Alert,
                    "confirm" => SystemMessageMode::Confirm,
                    _ => {
                        report
                            .diagnostics
                            .push(self.error("message mode must be `alert` or `confirm`"));
                        return None;
                    }
                };
                Some(Action::SystemMessage {
                    spec: SystemMessageSpec {
                        mode,
                        title: self.v11_named_string(args, "title", report)?,
                        message: self.v11_named_string(args, "message", report)?,
                        confirm_text: self.v11_named_string(args, "confirm_text", report)?,
                        cancel_text: self.v11_named_string(args, "cancel_text", report)?,
                        result_variable: match self.named_arg(args, "result") {
                            Some(arg) => {
                                Some(self.v11_identifier(Some(arg), "result variable", report)?)
                            }
                            None => None,
                        },
                    },
                })
            }
            _ => None,
        }
    }

    pub(super) fn v11_optional_color(
        &self,
        args: &[Argument],
        name: &str,
        default: [f32; 4],
        report: &mut ParseReport,
    ) -> Option<[f32; 4]> {
        let Some(arg) = self.named_arg(args, name) else {
            return Some(default);
        };
        let raw = arg
            .token_indices
            .iter()
            .map(|index| self.text(*index))
            .collect::<String>();
        let Some(inner) = raw
            .strip_prefix("rgba(")
            .and_then(|raw| raw.strip_suffix(')'))
        else {
            report
                .diagnostics
                .push(self.error(format!("`{name}` requires rgba(r, g, b, a)")));
            return None;
        };
        let values = inner
            .split(',')
            .map(str::parse::<f32>)
            .collect::<Result<Vec<_>, _>>();
        let Ok(values) = values else {
            report
                .diagnostics
                .push(self.error(format!("`{name}` requires numeric RGBA values")));
            return None;
        };
        if values.len() != 4
            || values
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        {
            report
                .diagnostics
                .push(self.error(format!("`{name}` requires four values between 0 and 1")));
            return None;
        }
        Some([values[0], values[1], values[2], values[3]])
    }
}
