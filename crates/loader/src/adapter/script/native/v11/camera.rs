use super::*;
use keine_core::{CameraShakeRandomness, CameraTweenField, CameraTweenSpec};

impl<'a> Parser<'a> {
    pub(in crate::adapter::script::native) fn camera_randomness(
        &self,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<CameraShakeRandomness> {
        let amplitude = self
            .checked_number(args, "amplitude_randomness", report)?
            .unwrap_or(0.0);
        let frequency = self
            .checked_number(args, "frequency_randomness", report)?
            .unwrap_or(0.0);
        if !(0.0..=1.0).contains(&amplitude) || !(0.0..=1.0).contains(&frequency) {
            report
                .diagnostics
                .push(self.error("shake randomness must be between 0 and 1"));
            return None;
        }
        Some(CameraShakeRandomness {
            amplitude,
            frequency,
        })
    }

    pub(in crate::adapter::script::native) fn camera_tween(
        &self,
        args: &[Argument],
        action: Action,
        report: &mut ParseReport,
    ) -> Option<Action> {
        let Some(argument) = self.named_arg(args, "tween") else {
            return Some(action);
        };
        let tokens = &argument.token_indices;
        if tokens.len() < 2 || self.text(tokens[0]) != "[" || self.text(*tokens.last()?) != "]" {
            report.diagnostics.push(
                self.error("tween requires a list of numeric field names, e.g. [x, scale_x]"),
            );
            return None;
        }
        let (targets, transform, effect, v2, duration, easing, blocking) = match action {
            Action::SetCameraTransform {
                targets,
                transform,
                duration,
                easing,
                blocking,
            } => (
                targets,
                Some(transform),
                None,
                None,
                duration,
                easing,
                blocking,
            ),
            Action::SetPostProcess {
                targets,
                effect,
                duration,
                easing,
                blocking,
            } => (
                targets,
                None,
                Some(effect),
                None,
                duration,
                easing,
                blocking,
            ),
            Action::SetPostProcessV2 {
                targets,
                effect,
                duration,
                easing,
                blocking,
            } => (
                targets,
                None,
                None,
                Some(effect),
                duration,
                easing,
                blocking,
            ),
            _ => return Some(action),
        };
        let mut fields = Vec::new();
        for (position, &token) in tokens[1..tokens.len() - 1].iter().enumerate() {
            let text = self.text(token);
            if position % 2 == 1 {
                if text != "," {
                    report
                        .diagnostics
                        .push(self.error("tween fields require commas"));
                    return None;
                }
                continue;
            }
            let Some(field) = CameraTweenField::from_name(text).filter(|field| {
                if transform.is_some() {
                    field.is_transform()
                } else if v2.is_some() {
                    field.is_v2()
                } else {
                    !field.is_transform() && !field.is_v2()
                }
            }) else {
                report.diagnostics.push(self.error(format!(
                    "unknown numeric tween field `{text}` for this camera command"
                )));
                return None;
            };
            if fields.contains(&field) {
                report
                    .diagnostics
                    .push(self.error(format!("duplicate tween field `{text}`")));
                return None;
            }
            fields.push(field);
        }
        if tokens.len() > 2 && self.text(tokens[tokens.len() - 2]) == "," {
            report
                .diagnostics
                .push(self.error("tween list cannot end with a comma"));
            return None;
        }
        Some(Action::SetCameraTween {
            spec: Box::new(CameraTweenSpec {
                targets,
                transform,
                effect,
                v2,
                fields,
                duration,
                easing,
                blocking,
            }),
        })
    }
}
