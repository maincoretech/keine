use super::*;

pub(super) const MEDIA_COMMANDS: &[&str] = &["se.loop", "se.stop", "video.play"];

impl<'a> Parser<'a> {
    pub(super) fn parse_v11_media_command(
        &self,
        name: &str,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<Action> {
        let (positional, named): (usize, &[&str]) = match name {
            "se.loop" => (2, &["volume"]),
            "se.stop" => (1, &[]),
            "video.play" => (2, &["loop", "muted", "alpha", "skippable", "wait", "mode"]),
            _ => return None,
        };
        let before = report.diagnostics.len();
        self.validate_signature(name, args, positional, named, report);
        if report.diagnostics.len() != before {
            return None;
        }
        match name {
            "se.loop" => Some(Action::Effect {
                id: Some(self.v11_identifier(args.first(), "effect ID", report)?),
                file: Some(self.v11_identifier(args.get(1), "effect asset", report)?),
                volume: self.v11_volume(args, report)?,
            }),
            "se.stop" => Some(Action::Effect {
                file: None,
                volume: 1.0,
                id: self.v11_optional_star_id(args.first(), "effect ID", report)?,
            }),
            "video.play" => {
                let mode = match self.named_arg(args, "mode") {
                    None => VideoMode::Fullscreen,
                    Some(arg) => match self.argument_identifier(arg).as_deref() {
                        Some("fullscreen") => VideoMode::Fullscreen,
                        Some("mixed") => VideoMode::Mixed,
                        _ => {
                            report
                                .diagnostics
                                .push(self.error("`mode` requires `fullscreen` or `mixed`"));
                            return None;
                        }
                    },
                };
                let alpha = self.v11_optional_number(args, "alpha", 1.0, report)?;
                if !(0.0..=1.0).contains(&alpha) {
                    report
                        .diagnostics
                        .push(self.error("`alpha` must be between 0 and 1"));
                    return None;
                }
                Some(Action::PlayVideo {
                    video: VideoSpec {
                        id: self.v11_identifier(args.first(), "video ID", report)?,
                        file: self.v11_identifier(args.get(1), "video asset", report)?,
                        looped: self.v11_optional_bool(args, "loop", false, report)?,
                        muted: self.v11_optional_bool(args, "muted", false, report)?,
                        alpha,
                        skippable: self.v11_optional_bool(args, "skippable", true, report)?,
                        wait_for_finished: self.v11_optional_bool(args, "wait", true, report)?,
                        mode,
                    },
                })
            }
            _ => None,
        }
    }
}
