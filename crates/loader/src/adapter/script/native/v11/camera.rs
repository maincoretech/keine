use super::*;
use keine_core::{CameraShakeRandomness, CameraTweenField, CameraTweenSpec};

pub(super) fn move_fields() -> &'static [&'static str] {
    static FIELDS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    FIELDS
        .get_or_init(|| {
            let mut fields = vec![
                "x", "y", "alpha", "scale_x", "scale_y", "rotation", "blur", "width", "height",
            ];
            for field in effects::PATCH_FIELDS {
                if !fields.contains(field) {
                    fields.push(field);
                }
            }
            fields
        })
        .as_slice()
}

impl<'a> Parser<'a> {
    pub(in crate::adapter::script::native) fn combined_camera_move(
        &self,
        args: &[Argument],
        targets: CameraTargets,
        transform: Option<TransformPatch>,
        report: &mut ParseReport,
    ) -> Option<Action> {
        let present = |fields: &[&str]| {
            args.iter().any(|arg| {
                arg.name.as_deref().is_some_and(|name| {
                    !matches!(name, "duration" | "easing" | "blocking" | "tween")
                        && fields.contains(&name)
                })
            })
        };
        let effect = if present(effects::PATCH_FIELDS) {
            Some(Box::new(self.v11_post_process_patch(args, report)?))
        } else {
            None
        };
        if transform.is_none() && effect.is_none() {
            report.diagnostics.push(
                self.error("camera.move(...) requires at least one transform or effect field"),
            );
            return None;
        }
        let action = self.camera_tween(
            args,
            Action::SetCameraTransform {
                targets,
                transform: transform.unwrap_or_default(),
                duration: self.named_duration_checked(args, "duration", report)?,
                easing: self.named_easing(args, "easing", report)?,
                blocking: self.checked_bool(args, "blocking", true, report)?,
            },
            report,
        )?;
        if effect.is_none() {
            return Some(action);
        }
        let mut spec = match action {
            Action::SetCameraTween { spec } => spec,
            Action::SetCameraTransform {
                targets,
                duration,
                easing,
                blocking,
                ..
            } => Box::new(CameraTweenSpec {
                targets,
                transform: None,
                effect: None,
                v2: None,
                fields: CameraTweenField::ALL.to_vec(),
                duration,
                easing,
                blocking,
            }),
            _ => unreachable!("camera_tween only wraps the supplied camera transform"),
        };
        spec.transform = transform;
        spec.effect = effect;
        spec.v2 = None;
        Some(Action::SetCameraTween { spec })
    }

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
            // Compatibility authoring stores one shared numeric selection across
            // camera channels. Preserve it exactly; core's typed sampler ignores
            // fields outside the supplied patch (CameraTweenSpec::start).
            let Some(field) = CameraTweenField::from_name(text) else {
                report
                    .diagnostics
                    .push(self.error(format!("unknown numeric tween field `{text}`")));
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
