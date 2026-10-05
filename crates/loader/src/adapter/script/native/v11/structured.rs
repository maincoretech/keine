use super::*;
use keine_core::{
    AssetHint, AssetHintKind, LoadingStrategy, LoadingStrategyMode, TransformKeyframe,
};

pub(super) const STRUCTURED_COMMANDS: &[&str] = &[
    "text.intro",
    "sprite.sequence",
    "sprite.select",
    "sprite.select.when",
    "sprite.keyframes",
    "assets.loading",
];

/// Syntax fields shared by lowering and source-based editor consumers.
pub(super) fn signature(name: &str) -> Option<(usize, &'static [&'static str])> {
    let signature: (usize, &[&str]) = match name {
        "text.intro" => (0, &["hold"]),
        "sprite.sequence" => (1, &["fps", "loop", "mode", "interval", "speaker"]),
        "sprite.select" => (2, &["default"]),
        "sprite.select.when" => (1, &["default"]),
        "sprite.keyframes" => (1, &["repeat", "blocking"]),
        "assets.loading" => (0, &["mode", "lookahead", "blocking"]),
        _ => return None,
    };
    Some(signature)
}

/// A child row's signature belongs to its structured parent command.
pub(in crate::adapter::script::native) fn child_signature(
    parent: &str,
    child: &str,
) -> Option<(usize, &'static [&'static str])> {
    Some(match (parent, child) {
        ("text.intro", "page") => (1, &[]),
        ("sprite.sequence", "frame") => (1, &["duration"]),
        ("sprite.select" | "sprite.select.when", "case") => (2, &[]),
        ("sprite.keyframes", "frame") => (
            0,
            &[
                "duration", "easing", "x", "y", "alpha", "scale_x", "scale_y", "rotation", "blur",
                "width", "height",
            ],
        ),
        ("assets.loading", "resource") => (1, &["kind"]),
        _ => return None,
    })
}

impl<'a> Parser<'a> {
    pub(super) fn parse_v11_structured_command(
        &mut self,
        name: &str,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<Action> {
        let (positional, named) = signature(name)?;
        let before = report.diagnostics.len();
        self.validate_signature(name, args, positional, named, report);
        let rows = self.take_v11_rows(report)?;
        if report.diagnostics.len() != before {
            return None;
        }
        if rows.is_empty() {
            report
                .diagnostics
                .push(self.error(format!("`{name}` requires at least one child row")));
            return None;
        }
        let action =
            match name {
                "text.intro" => {
                    let mut pages = Vec::new();
                    for (row, fields) in &rows {
                        let (positional, named) =
                            child_signature(name, "page").expect("static structured child");
                        self.validate_signature(row, fields, positional, named, report);
                        if row != "page" {
                            report
                                .diagnostics
                                .push(self.error("text.intro children must be page(...)"));
                            return None;
                        }
                        pages.push(self.v11_string(fields.first(), "intro page", report)?);
                    }
                    Some(Action::Intro {
                        pages,
                        hold: self.v11_optional_bool(args, "hold", false, report)?,
                    })
                }
                "sprite.sequence" => {
                    let timed = rows
                        .iter()
                        .any(|(_, fields)| self.named_arg(fields, "duration").is_some());
                    if timed && self.named_arg(args, "fps").is_some() {
                        report.diagnostics.push(
                            self.error("sprite.sequence uses fps or per-frame duration, not both"),
                        );
                        return None;
                    }
                    let mut frames = Vec::new();
                    let mut frame_durations = Vec::new();
                    for (row, fields) in &rows {
                        let (positional, named) =
                            child_signature(name, "frame").expect("static structured child");
                        self.validate_signature(row, fields, positional, named, report);
                        if row != "frame" {
                            report.diagnostics.push(self.error(
                                "sprite.sequence children must be frame(asset, duration: ...)",
                            ));
                            return None;
                        }
                        frames.push(self.v11_identifier(fields.first(), "frame asset", report)?);
                        if timed {
                            frame_durations
                                .push(self.v11_structured_duration(fields, "duration", report)?);
                        }
                    }
                    let id = self.v11_identifier(args.first(), "sprite ID", report)?;
                    let looped = self.v11_optional_bool(args, "loop", false, report)?;
                    let mode = match self.named_arg(args, "mode") {
                        Some(arg) => {
                            Some(self.v11_identifier(Some(arg), "sequence mode", report)?)
                        }
                        None => None,
                    };
                    if let Some(mode) = mode {
                        if self.named_arg(args, "loop").is_some() || frames.len() < 2 {
                            report.diagnostics.push(self.error(
                            "dynamic sequences need at least two frames and control their own loop",
                        ));
                            return None;
                        }
                        let playback = match mode.as_str() {
                            "blink" => {
                                if self.named_arg(args, "speaker").is_some() {
                                    report
                                        .diagnostics
                                        .push(self.error("blink sequences do not use speaker"));
                                    return None;
                                }
                                let interval = if self.named_arg(args, "interval").is_some() {
                                    self.named_duration_checked(args, "interval", report)?
                                } else {
                                    3.0
                                };
                                if interval <= 0.0 {
                                    report
                                        .diagnostics
                                        .push(self.error("blink interval must be positive"));
                                    return None;
                                }
                                keine_core::SequencePlayback::Blink { interval }
                            }
                            "talk" => {
                                if self.named_arg(args, "interval").is_some() {
                                    report
                                        .diagnostics
                                        .push(self.error("talk sequences do not use interval"));
                                    return None;
                                }
                                keine_core::SequencePlayback::Talk {
                                    speaker: self.v11_string(
                                        self.named_arg(args, "speaker"),
                                        "speaker",
                                        report,
                                    )?,
                                }
                            }
                            _ => {
                                report
                                    .diagnostics
                                    .push(self.error("sequence mode must be blink or talk"));
                                return None;
                            }
                        };
                        let fps = self.v11_optional_number(args, "fps", 12.0, report)?;
                        if fps <= 0.0 {
                            report.diagnostics.push(self.error("fps must be positive"));
                            return None;
                        }
                        return (report.diagnostics.len() == before).then_some(
                            Action::ConfigureDynamicSpriteSequence {
                                id,
                                frames,
                                fps,
                                frame_durations,
                                playback,
                            },
                        );
                    }
                    if self.named_arg(args, "interval").is_some()
                        || self.named_arg(args, "speaker").is_some()
                    {
                        report
                            .diagnostics
                            .push(self.error("interval/speaker require a sequence mode"));
                        return None;
                    }
                    if timed {
                        Some(Action::ConfigureTimedSpriteSequence {
                            id,
                            frames,
                            frame_durations,
                            looped,
                        })
                    } else {
                        let fps = self.v11_optional_number(args, "fps", 12.0, report)?;
                        if fps <= 0.0 {
                            report.diagnostics.push(self.error("fps must be positive"));
                            return None;
                        }
                        Some(Action::ConfigureSpriteSequence {
                            id,
                            frames,
                            fps,
                            looped,
                        })
                    }
                }
                "sprite.select" => {
                    let mut variants = Vec::new();
                    for (row, fields) in &rows {
                        let (positional, named) =
                            child_signature(name, "case").expect("static structured child");
                        self.validate_signature(row, fields, positional, named, report);
                        if row != "case" {
                            report.diagnostics.push(
                                self.error("sprite.select children must be case(\"value\", asset)"),
                            );
                            return None;
                        }
                        variants.push((
                            self.v11_string(fields.first(), "variant value", report)?,
                            self.v11_identifier(fields.get(1), "variant asset", report)?,
                        ));
                    }
                    Some(Action::SelectSpriteImage {
                        id: self.v11_identifier(args.first(), "sprite ID", report)?,
                        variable: self.v11_identifier(args.get(1), "selection variable", report)?,
                        default_image: self.v11_identifier(
                            self.named_arg(args, "default"),
                            "default asset",
                            report,
                        )?,
                        variants,
                    })
                }
                "sprite.select.when" => {
                    let mut variants = Vec::new();
                    for (row, fields) in &rows {
                        let (positional, named) =
                            child_signature(name, "case").expect("static structured child");
                        self.validate_signature(row, fields, positional, named, report);
                        if row != "case" {
                            report.diagnostics.push(self.error(
                                "sprite.select.when children must be case(condition, asset)",
                            ));
                            return None;
                        }
                        let Some(condition_arg) = fields.first() else {
                            report
                                .diagnostics
                                .push(self.error("case requires a condition"));
                            return None;
                        };
                        let condition =
                            self.expression_from_indices(&condition_arg.token_indices, report)?;
                        variants.push((
                            condition,
                            self.v11_identifier(fields.get(1), "variant asset", report)?,
                        ));
                    }
                    Some(Action::EiyashouSelectSpriteImageByCondition {
                        id: self.v11_identifier(args.first(), "sprite ID", report)?,
                        default_image: self.v11_identifier(
                            self.named_arg(args, "default"),
                            "default asset",
                            report,
                        )?,
                        variants,
                    })
                }
                "sprite.keyframes" => {
                    let repeat = self.v11_optional_number(args, "repeat", 0.0, report)?;
                    if repeat.fract() != 0.0 || !(0.0..=u32::MAX as f32).contains(&repeat) {
                        report
                            .diagnostics
                            .push(self.error("`repeat` requires a non-negative integer"));
                        return None;
                    }
                    let mut frames = Vec::new();
                    for (row, fields) in &rows {
                        let (positional, named) =
                            child_signature(name, "frame").expect("static structured child");
                        self.validate_signature(row, fields, positional, named, report);
                        if row != "frame" {
                            report
                                .diagnostics
                                .push(self.error("sprite.keyframes children must be frame(...)"));
                            return None;
                        }
                        let mut transform = TransformPatch::default();
                        for field in [
                            "x", "y", "alpha", "scale_x", "scale_y", "rotation", "blur", "width",
                            "height",
                        ] {
                            let Some(value) = self.checked_number(fields, field, report)? else {
                                continue;
                            };
                            match field {
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
                        // An empty sparse patch holds the previous transform for this
                        // segment's duration, matching the typed keyframe engine.
                        frames.push(TransformKeyframe {
                            transform,
                            duration: self.v11_structured_duration(fields, "duration", report)?,
                            easing: self.named_easing(fields, "easing", report)?,
                        });
                    }
                    Some(Action::AnimateKeyframes {
                        target: self.v11_identifier(args.first(), "animation target", report)?,
                        frames,
                        repeat: repeat as u32,
                        blocking: self.v11_optional_bool(args, "blocking", true, report)?,
                    })
                }
                "assets.loading" => {
                    let mode = match self.named_identifier(args, "mode").as_deref() {
                        None | Some("auto") => LoadingStrategyMode::Auto,
                        Some("manual") => LoadingStrategyMode::Manual,
                        _ => {
                            report
                                .diagnostics
                                .push(self.error("loading mode must be `auto` or `manual`"));
                            return None;
                        }
                    };
                    let lookahead = self.v11_optional_number(args, "lookahead", 20.0, report)?;
                    if lookahead.fract() != 0.0 || !(0.0..=u16::MAX as f32).contains(&lookahead) {
                        report.diagnostics.push(
                            self.error("`lookahead` requires an integer between 0 and 65535"),
                        );
                        return None;
                    }
                    let mut resources = Vec::new();
                    for (row, fields) in &rows {
                        let (positional, named) =
                            child_signature(name, "resource").expect("static structured child");
                        self.validate_signature(row, fields, positional, named, report);
                        if row != "resource" {
                            report.diagnostics.push(self.error(
                                "assets.loading children must be resource(asset, kind: ...)",
                            ));
                            return None;
                        }
                        let kind = match self.named_identifier(fields, "kind").as_deref() {
                            Some("background") => AssetHintKind::Background,
                            Some("figure") => AssetHintKind::Figure,
                            _ => {
                                report.diagnostics.push(
                                    self.error("resource kind must be `background` or `figure`"),
                                );
                                return None;
                            }
                        };
                        resources.push(AssetHint {
                            path: self.v11_identifier(fields.first(), "resource asset", report)?,
                            kind,
                        });
                    }
                    Some(Action::ConfigureLoading {
                        strategy: LoadingStrategy {
                            mode,
                            lookahead: lookahead as u16,
                            blocking: self.v11_optional_bool(args, "blocking", false, report)?,
                            resources,
                        },
                    })
                }
                _ => None,
            };
        (report.diagnostics.len() == before)
            .then_some(action)
            .flatten()
    }

    fn v11_structured_duration(
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

    fn take_v11_rows(&mut self, report: &mut ParseReport) -> Option<Vec<(String, Vec<Argument>)>> {
        if !self.eat("{") {
            report
                .diagnostics
                .push(self.error("expected `{` after structured command"));
            return None;
        }
        let mut rows = Vec::new();
        while !self.eof() && self.peek_text() != Some("}") {
            let Some(name) = self.take_identifier() else {
                report.diagnostics.push(self.error("expected a child row"));
                return None;
            };
            let fields = self.take_call_args(report);
            rows.push((name, fields));
            if self.peek_text() == Some("}") {
                break;
            }
            if !self.eat(",") {
                report
                    .diagnostics
                    .push(self.error("expected `,` between child rows"));
                return None;
            }
        }
        if !self.eat("}") {
            report
                .diagnostics
                .push(self.error("unterminated structured command"));
            return None;
        }
        Some(rows)
    }
}
