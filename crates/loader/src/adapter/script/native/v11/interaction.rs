use super::*;
use keine_core::config::{TextRevealConfig, TextRevealEffect};
use keine_core::{InputValueType, UserInputSpec};

pub(super) const INTERACTION_COMMANDS: &[&str] = &["input.request", "text.paragraph.style"];

/// Syntax fields shared by lowering and source-based editor consumers.
pub(super) fn signature(name: &str) -> Option<(usize, &'static [&'static str])> {
    let signature: (usize, &[&str]) = match name {
        "input.request" => (
            1,
            &[
                "type",
                "title",
                "description",
                "placeholder",
                "confirm_text",
                "required_text",
                "required",
                "min_length",
                "max_length",
                "min_value",
                "max_value",
                "step",
                "true_text",
                "false_text",
            ],
        ),
        "text.paragraph.style" => (
            1,
            &[
                "typewriter_speed",
                "reveal_duration",
                "reveal_effect",
                "reveal_distance",
                "reveal_scale",
                "reveal_rotation",
                "reveal_blur",
            ],
        ),
        _ => return None,
    };
    Some(signature)
}

impl<'a> Parser<'a> {
    pub(super) fn parse_v11_interaction_command(
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
            "input.request" => {
                let mut spec = UserInputSpec {
                    variable: self.v11_identifier(args.first(), "input variable", report)?,
                    ..UserInputSpec::default()
                };
                if let Some(arg) = self.named_arg(args, "type") {
                    spec.value_type = match self.argument_identifier(arg).as_deref() {
                        Some("string") => InputValueType::String,
                        Some("number") => InputValueType::Number,
                        Some("bool") => InputValueType::Bool,
                        _ => {
                            report.diagnostics.push(
                                self.error("input type must be `string`, `number`, or `bool`"),
                            );
                            return None;
                        }
                    };
                }
                for field in [
                    "title",
                    "description",
                    "placeholder",
                    "confirm_text",
                    "required_text",
                    "true_text",
                    "false_text",
                ] {
                    if self.named_arg(args, field).is_none() {
                        continue;
                    }
                    let value = self.v11_named_string(args, field, report)?;
                    match field {
                        "title" => spec.title = value,
                        "description" => spec.description = value,
                        "placeholder" => spec.placeholder = value,
                        "confirm_text" => spec.confirm_text = value,
                        "required_text" => spec.required_text = value,
                        "true_text" => spec.true_text = value,
                        "false_text" => spec.false_text = value,
                        _ => unreachable!(),
                    }
                }
                spec.required = self.v11_optional_bool(args, "required", spec.required, report)?;
                spec.min_length =
                    self.v11_optional_usize(args, "min_length", spec.min_length, report)?;
                spec.max_length =
                    self.v11_optional_usize(args, "max_length", spec.max_length, report)?;
                spec.min_value = self.v11_optional_f64(args, "min_value", report)?;
                spec.max_value = self.v11_optional_f64(args, "max_value", report)?;
                spec.step = self
                    .v11_optional_f64(args, "step", report)?
                    .unwrap_or(spec.step);
                if spec.step <= 0.0
                    || (spec.max_length != 0 && spec.min_length > spec.max_length)
                    || matches!((spec.min_value, spec.max_value), (Some(min), Some(max)) if min > max)
                {
                    report
                        .diagnostics
                        .push(self.error("invalid input length, range, or step"));
                    return None;
                }
                Some(Action::RequestInput { spec })
            }
            "text.paragraph.style" => {
                let typewriter_speed = self.v11_optional_f64(args, "typewriter_speed", report)?;
                if typewriter_speed.is_some_and(|value| value < 0.0) {
                    report
                        .diagnostics
                        .push(self.error("`typewriter_speed` must be non-negative"));
                    return None;
                }
                let reveal_names = [
                    "reveal_duration",
                    "reveal_effect",
                    "reveal_distance",
                    "reveal_scale",
                    "reveal_rotation",
                    "reveal_blur",
                ];
                let text_reveal = if reveal_names
                    .iter()
                    .any(|name| self.named_arg(args, name).is_some())
                {
                    let mut reveal = TextRevealConfig::default();
                    if self.named_arg(args, "reveal_duration").is_some() {
                        reveal.duration =
                            self.named_duration_checked(args, "reveal_duration", report)?;
                    }
                    if let Some(arg) = self.named_arg(args, "reveal_effect") {
                        reveal.effect = match self.argument_identifier(arg).as_deref() {
                            Some("instant") => TextRevealEffect::Instant,
                            Some("smooth_rise") => TextRevealEffect::SmoothRise,
                            Some("classic") => TextRevealEffect::Classic,
                            Some("smooth_drop") => TextRevealEffect::SmoothDrop,
                            Some("slide_left") => TextRevealEffect::SlideLeft,
                            Some("slide_right") => TextRevealEffect::SlideRight,
                            Some("pop") => TextRevealEffect::Pop,
                            Some("flip") => TextRevealEffect::Flip,
                            Some("swing") => TextRevealEffect::Swing,
                            Some("blur") => TextRevealEffect::Blur,
                            _ => {
                                report
                                    .diagnostics
                                    .push(self.error("unknown text reveal effect"));
                                return None;
                            }
                        };
                    }
                    reveal.distance =
                        self.v11_optional_number(args, "reveal_distance", reveal.distance, report)?;
                    reveal.scale =
                        self.v11_optional_number(args, "reveal_scale", reveal.scale, report)?;
                    reveal.rotation =
                        self.v11_optional_number(args, "reveal_rotation", reveal.rotation, report)?;
                    reveal.blur =
                        self.v11_optional_number(args, "reveal_blur", reveal.blur, report)?;
                    Some(reveal)
                } else {
                    None
                };
                Some(Action::SetParagraphStyle {
                    style: DialogueStyle::from_id(self.v11_id_or_string(
                        args.first(),
                        "paragraph style",
                        report,
                    )?),
                    typewriter_speed,
                    text_reveal,
                })
            }
            _ => None,
        }
    }

    fn v11_optional_f64(
        &self,
        args: &[Argument],
        name: &str,
        report: &mut ParseReport,
    ) -> Option<Option<f64>> {
        let Some(arg) = self.named_arg(args, name) else {
            return Some(None);
        };
        match self.argument_number(arg) {
            Some(value) if value.is_finite() => Some(Some(value)),
            _ => {
                report
                    .diagnostics
                    .push(self.error(format!("`{name}` requires a finite number")));
                None
            }
        }
    }

    fn v11_optional_usize(
        &self,
        args: &[Argument],
        name: &str,
        default: usize,
        report: &mut ParseReport,
    ) -> Option<usize> {
        let Some(value) = self.v11_optional_f64(args, name, report)? else {
            return Some(default);
        };
        if value.fract() != 0.0 || value < 0.0 || value > usize::MAX as f64 {
            report
                .diagnostics
                .push(self.error(format!("`{name}` requires a non-negative integer")));
            return None;
        }
        Some(value as usize)
    }
}
