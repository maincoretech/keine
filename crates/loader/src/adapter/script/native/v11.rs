//! Eiyashou v2.0 author commands that lower directly to existing typed IR.
//! The names are shared with the Block projection; compatibility adapter IR
//! remains outside this authoring surface.

use super::*;
use keine_core::{DialogueStyle, SystemUiSlot, UnlockKind};

mod camera;
mod effects;
mod interaction;
mod layout;
mod mask;
mod media;
mod shell;
mod stage;
mod structured;
mod visual;

pub(super) const SIMPLE_COMMANDS: &[&str] = &[
    "story.end",
    "avatar.show",
    "avatar.hide",
    "vocal.play",
    "vocal.stop",
    "screen.film",
    "text.box",
    "wait.advance",
    "playback.auto",
    "ui.show",
    "ui.hide",
    "particle.layers.clear",
    "text.presentation",
    "text.retract",
    "text.float.hide",
    "text.float.configure",
    "text.style",
    "scene.parallax.stop",
    "particle.hide",
    "video.stop",
    "gallery.unlock",
    "camera.reset",
    "camera.bind",
    "camera.unbind",
];

pub(super) fn is_dotted_command(name: &str) -> bool {
    matches!(
        name,
        "camera.move" | "camera.shake" | "sprite.focus" | "sprite.focus.configure"
    ) || is_extension_command(name)
}

pub(super) fn is_extension_command(name: &str) -> bool {
    SIMPLE_COMMANDS.contains(&name)
        || visual::VISUAL_COMMANDS.contains(&name)
        || media::MEDIA_COMMANDS.contains(&name)
        || shell::SHELL_COMMANDS.contains(&name)
        || structured::STRUCTURED_COMMANDS.contains(&name)
        || interaction::INTERACTION_COMMANDS.contains(&name)
        || effects::EFFECT_COMMANDS.contains(&name)
        || mask::MASK_COMMANDS.contains(&name)
        || stage::STAGE_COMMANDS.contains(&name)
}

pub(super) fn is_structured_command(name: &str) -> bool {
    structured::STRUCTURED_COMMANDS.contains(&name) || stage::STAGE_COMMANDS.contains(&name)
}

pub(super) fn expanded_fields(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "camera.move" => Some(camera::move_fields()),
        "camera.effect" => Some(effects::PATCH_FIELDS),
        "stage.mask.show" => Some(mask::MASK_FIELDS),
        _ => None,
    }
}

impl<'a> Parser<'a> {
    pub(super) fn parse_v11_command(
        &mut self,
        name: &str,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<Action> {
        if SIMPLE_COMMANDS.contains(&name) {
            self.parse_v11_simple_command(name, args, report)
        } else if visual::VISUAL_COMMANDS.contains(&name) {
            self.parse_v11_visual_command(name, args, report)
        } else if media::MEDIA_COMMANDS.contains(&name) {
            self.parse_v11_media_command(name, args, report)
        } else if shell::SHELL_COMMANDS.contains(&name) {
            self.parse_v11_shell_command(name, args, report)
        } else if structured::STRUCTURED_COMMANDS.contains(&name) {
            self.parse_v11_structured_command(name, args, report)
        } else if interaction::INTERACTION_COMMANDS.contains(&name) {
            self.parse_v11_interaction_command(name, args, report)
        } else if effects::EFFECT_COMMANDS.contains(&name) {
            self.parse_v11_effect_command(name, args, report)
        } else if mask::MASK_COMMANDS.contains(&name) {
            self.parse_v11_mask_command(name, args, report)
        } else if stage::STAGE_COMMANDS.contains(&name) {
            self.parse_v11_stage_command(name, args, report)
        } else {
            None
        }
    }

    pub(super) fn parse_v11_simple_command(
        &self,
        name: &str,
        args: &[Argument],
        report: &mut ParseReport,
    ) -> Option<Action> {
        let (positional, named): (usize, &[&str]) = match name {
            "story.end"
            | "avatar.hide"
            | "vocal.stop"
            | "wait.advance"
            | "particle.layers.clear"
            | "scene.parallax.stop" => (0, &[]),
            "avatar.show" | "screen.film" | "playback.auto" | "ui.show" | "ui.hide"
            | "text.presentation" | "text.style" => (1, &[]),
            "vocal.play" => (1, &["volume"]),
            "text.box" => (0, &["visible", "auto"]),
            "text.retract" => (0, &["source", "keep"]),
            "text.float.configure" => (0, &["id", "infinite"]),
            "particle.hide" => (1, &["duration"]),
            "video.stop" => (1, &["fade"]),
            "gallery.unlock" => (2, &["name"]),
            "camera.bind" | "camera.unbind" => (1, &["distance"]),
            // The active floating text may be hidden without naming an ID.
            "text.float.hide" => {
                let count = args.iter().filter(|arg| arg.name.is_none()).count();
                if count > 1 {
                    report
                        .diagnostics
                        .push(self.error("text.float.hide(...) accepts at most one ID"));
                    return None;
                }
                (count, &[])
            }
            _ => return None,
        };
        let before = report.diagnostics.len();
        self.validate_signature(name, args, positional, named, report);
        if report.diagnostics.len() != before {
            return None;
        }
        let first = args.first();
        match name {
            "story.end" => Some(Action::End),
            "avatar.show" => Some(Action::MiniAvatar {
                image: self.v11_identifier(first, "avatar image", report)?,
            }),
            "avatar.hide" => Some(Action::HideMiniAvatar),
            "vocal.play" => Some(Action::Vocal {
                file: Some(self.v11_identifier(first, "vocal asset", report)?),
                volume: self.v11_volume(args, report)?,
            }),
            "vocal.stop" => Some(Action::Vocal {
                file: None,
                volume: 1.0,
            }),
            "screen.film" => Some(Action::FilmMode {
                enabled: self.v11_bool(first, "screen.film value", report)?,
            }),
            "text.box" => Some(Action::SetTextbox {
                visible: self.v11_named_bool(args, "visible", report)?,
                auto: self.v11_named_bool(args, "auto", report)?,
            }),
            "wait.advance" => Some(Action::WaitForAdvance),
            "playback.auto" => Some(Action::SetAutoplay {
                enabled: self.v11_bool(first, "playback.auto value", report)?,
            }),
            "ui.show" | "ui.hide" => Some(Action::SetSystemUi {
                slot: self.v11_system_ui_slot(first, report)?,
                visible: name == "ui.show",
            }),
            "particle.layers.clear" => Some(Action::HideParticleLayers),
            "text.presentation" => {
                let mode = self.v11_identifier(first, "text presentation", report)?;
                match mode.as_str() {
                    "paragraph" => Some(Action::SelectTextPresentation { paragraph: true }),
                    "dialogue" => Some(Action::SelectTextPresentation { paragraph: false }),
                    _ => {
                        report.diagnostics.push(
                            self.error("text.presentation(...) requires `paragraph` or `dialogue`"),
                        );
                        None
                    }
                }
            }
            "text.retract" => Some(Action::RetractDialogue {
                source: self.v11_named_string(args, "source", report)?,
                keep: self.v11_named_string(args, "keep", report)?,
            }),
            "text.float.hide" => Some(Action::HideFloatingText {
                id: match first {
                    Some(arg) => {
                        Some(self.v11_identifier(Some(arg), "floating text ID", report)?)
                    }
                    None => None,
                },
            }),
            "text.float.configure" => Some(Action::ConfigureFloatingText {
                id: match self.named_arg(args, "id") {
                    Some(arg) => {
                        Some(self.v11_identifier(Some(arg), "floating text ID", report)?)
                    }
                    None => None,
                },
                infinite: self.v11_named_bool(args, "infinite", report)?,
            }),
            "text.style" => Some(Action::SetDialogueStyle {
                style: DialogueStyle::from_id(self.v11_id_or_string(
                    first,
                    "text style",
                    report,
                )?),
            }),
            "scene.parallax.stop" => Some(Action::ConfigureSceneMouseParallax { parallax: None }),
            "particle.hide" => Some(Action::HideParticles {
                id: self.v11_optional_star_id(first, "particle ID", report)?,
                duration: self.named_duration_checked(args, "duration", report)?,
            }),
            "video.stop" => Some(Action::StopVideo {
                id: self.v11_optional_star_id(first, "video ID", report)?,
                fade_out: self.named_duration_checked(args, "fade", report)?,
            }),
            "gallery.unlock" => {
                let kind = self.v11_identifier(first, "gallery kind", report)?;
                let kind = match kind.as_str() {
                    "cg" => UnlockKind::Cg,
                    "bgm" => UnlockKind::Bgm,
                    _ => {
                        report
                            .diagnostics
                            .push(self.error("gallery.unlock(...) kind must be `cg` or `bgm`"));
                        return None;
                    }
                };
                Some(Action::Unlock {
                    kind,
                    file: self.v11_identifier(args.get(1), "gallery asset", report)?,
                    name: self.v11_named_string(args, "name", report)?,
                })
            }
            "camera.bind" | "camera.unbind" => Some(Action::SetCameraBinding {
                target: self.v11_identifier(first, "camera binding target", report)?,
                bound: name == "camera.bind",
                distance: self.v11_named_number(args, "distance", report)?,
            }),
            _ => None,
        }
    }

    pub(super) fn v11_identifier(
        &self,
        arg: Option<&Argument>,
        description: &str,
        report: &mut ParseReport,
    ) -> Option<String> {
        match arg.and_then(|arg| self.argument_identifier(arg)) {
            Some(value) => Some(value),
            None => {
                report
                    .diagnostics
                    .push(self.error(format!("{description} requires an identifier")));
                None
            }
        }
    }

    fn v11_id_or_string(
        &self,
        arg: Option<&Argument>,
        description: &str,
        report: &mut ParseReport,
    ) -> Option<String> {
        if arg.is_some_and(|arg| {
            arg.token_indices.len() == 1
                && self.tokens[arg.token_indices[0]].kind == NativeTokenKind::String
        }) {
            self.v11_string(arg, description, report)
        } else {
            self.v11_identifier(arg, description, report)
        }
    }

    fn v11_bool(
        &self,
        arg: Option<&Argument>,
        description: &str,
        report: &mut ParseReport,
    ) -> Option<bool> {
        match arg.and_then(|arg| self.argument_bool(arg)) {
            Some(value) => Some(value),
            None => {
                report
                    .diagnostics
                    .push(self.error(format!("{description} requires `true` or `false`")));
                None
            }
        }
    }

    fn v11_named_bool(
        &self,
        args: &[Argument],
        name: &str,
        report: &mut ParseReport,
    ) -> Option<bool> {
        self.v11_bool(self.named_arg(args, name), name, report)
    }

    fn v11_optional_bool(
        &self,
        args: &[Argument],
        name: &str,
        default: bool,
        report: &mut ParseReport,
    ) -> Option<bool> {
        match self.named_arg(args, name) {
            Some(arg) => self.v11_bool(Some(arg), name, report),
            None => Some(default),
        }
    }

    fn v11_optional_number(
        &self,
        args: &[Argument],
        name: &str,
        default: f32,
        report: &mut ParseReport,
    ) -> Option<f32> {
        self.checked_number(args, name, report)
            .map(|value| value.unwrap_or(default))
    }

    fn v11_named_number(
        &self,
        args: &[Argument],
        name: &str,
        report: &mut ParseReport,
    ) -> Option<f32> {
        match self.checked_number(args, name, report)? {
            Some(value) => Some(value),
            None => {
                report
                    .diagnostics
                    .push(self.error(format!("`{name}` requires a number")));
                None
            }
        }
    }

    fn v11_named_string(
        &self,
        args: &[Argument],
        name: &str,
        report: &mut ParseReport,
    ) -> Option<String> {
        self.v11_string(self.named_arg(args, name), name, report)
    }

    fn v11_string(
        &self,
        arg: Option<&Argument>,
        name: &str,
        report: &mut ParseReport,
    ) -> Option<String> {
        let Some(arg) = arg else {
            report
                .diagnostics
                .push(self.error(format!("`{name}` requires a string")));
            return None;
        };
        let Some(&token) = arg.token_indices.first() else {
            report
                .diagnostics
                .push(self.error(format!("`{name}` requires a string")));
            return None;
        };
        if arg.token_indices.len() != 1 || self.tokens[token].kind != NativeTokenKind::String {
            report
                .diagnostics
                .push(self.error(format!("`{name}` requires a quoted string")));
            return None;
        }
        match decode_expression_string(self.text(token)) {
            Ok(value) => Some(value),
            Err(message) => {
                report.diagnostics.push(self.error(message));
                None
            }
        }
    }

    fn v11_optional_star_id(
        &self,
        arg: Option<&Argument>,
        description: &str,
        report: &mut ParseReport,
    ) -> Option<Option<String>> {
        if arg.is_some_and(|arg| {
            arg.token_indices.len() == 1 && self.text(arg.token_indices[0]) == "*"
        }) {
            return Some(None);
        }
        self.v11_identifier(arg, description, report).map(Some)
    }

    pub(super) fn v11_volume(&self, args: &[Argument], report: &mut ParseReport) -> Option<f32> {
        let volume = self.checked_number(args, "volume", report)?.unwrap_or(1.0);
        if !(0.0..=1.0).contains(&volume) {
            report
                .diagnostics
                .push(self.error("`volume` must be between 0 and 1"));
            return None;
        }
        Some(volume)
    }

    fn v11_system_ui_slot(
        &self,
        arg: Option<&Argument>,
        report: &mut ParseReport,
    ) -> Option<SystemUiSlot> {
        let value = self.v11_identifier(arg, "system UI slot", report)?;
        match value.as_str() {
            "title" => Some(SystemUiSlot::Title),
            "save" => Some(SystemUiSlot::Save),
            "load" => Some(SystemUiSlot::Load),
            "settings" => Some(SystemUiSlot::Settings),
            "history" => Some(SystemUiSlot::History),
            "gallery" => Some(SystemUiSlot::Gallery),
            "input" => Some(SystemUiSlot::Input),
            _ => {
                report
                    .diagnostics
                    .push(self.error("unknown system UI slot"));
                None
            }
        }
    }
}

pub use stage::native_stage_property_names;
