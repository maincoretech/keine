// WebGAL .txt script parser
// Convert WebGAL script format to keine Actions.

use std::borrow::Cow;
use std::collections::HashMap;

use keine_core::action::{Action, Choice, ChoiceTarget, SayOptions};
use keine_core::types::{
    AnimationPreset, BlendMode, Easing, Position, SpriteLayout, SpriteTransform, TransformPatch,
    Transition, VideoMode, VideoSpec, VisualFilter,
};

use crate::ScriptLanguage;
use crate::report::{Diagnostic, DiagnosticLevel, ParseReport, SourceSpan};

#[derive(Clone, Copy, Debug, Default)]
pub struct WebGalLanguage;

impl ScriptLanguage for WebGalLanguage {
    fn name(&self) -> &'static str {
        "WebGAL"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["txt"]
    }

    fn parse(&self, source: &str) -> ParseReport {
        parse_webgal_report(source)
    }
}

pub fn parse_webgal(input: &str) -> Vec<Action> {
    parse_webgal_report(input).actions
}

pub fn parse_webgal_report(input: &str) -> ParseReport {
    let mut report = ParseReport::default();

    for line in logical_lines(input) {
        // WebGAL treats the first unescaped semicolon as the start of a
        // comment. Keep the historical `//` extension, but only recognize it
        // at a token boundary so URLs such as https://example.com survive.
        let cmd = strip_legacy_slash_comment(before_unescaped(&line.source, ';')).trim();
        if cmd.is_empty() {
            continue;
        }

        if let Some(command) = unsupported_command(cmd) {
            report.diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Warning,
                span: line.span,
                message: format!("unsupported WebGAL command `{command}`"),
            });
            continue;
        }

        if let Some(action) = parse_webgal_line(cmd) {
            report.push(action, line.span);
        } else {
            report.diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Warning,
                span: line.span,
                message: format!("unsupported or malformed WebGAL command: {cmd}"),
            });
        }
    }

    report
}

#[derive(Debug)]
struct LogicalLine<'a> {
    source: Cow<'a, str>,
    span: SourceSpan,
}

/// Joins WebGAL's indented continuation syntax before parsing commands.
///
/// A leading `|` extends dialogue without adding whitespace, while a leading
/// `-` appends command arguments with one separating space. `-concat` is an
/// intentional exception in WebGAL and remains a separate statement.
fn logical_lines(input: &str) -> Vec<LogicalLine<'_>> {
    let mut logical = Vec::<LogicalLine<'_>>::new();

    for (line_index, source) in input.lines().enumerate() {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            continue;
        }

        let is_indented = source.chars().next().is_some_and(char::is_whitespace);
        let is_continuation = is_indented
            && (trimmed.starts_with('|') || trimmed.starts_with('-'))
            && !trimmed.contains("-concat");

        if is_continuation && let Some(previous) = logical.last_mut() {
            let previous = previous.source.to_mut();
            if trimmed.starts_with('-') {
                previous.push(' ');
            }
            previous.push_str(trimmed);
            continue;
        }

        logical.push(LogicalLine {
            source: Cow::Borrowed(trimmed),
            span: SourceSpan {
                line: line_index + 1,
                column: source.find(trimmed).unwrap_or(0) + 1,
            },
        });
    }

    logical
}

fn parse_webgal_line(raw: &str) -> Option<Action> {
    let (cmd, args) = split_command_args(raw);
    let action = parse_webgal_command(&cmd, &args)?;
    let when = args.get("when").cloned();
    let next = args.boolean("next");
    if when.is_some() || next {
        Some(Action::Flow {
            action: Box::new(action),
            when,
            next,
        })
    } else {
        Some(action)
    }
}

fn parse_webgal_command(cmd: &str, args: &ScriptArgs) -> Option<Action> {
    if let Some((rest, kind)) = cmd
        .strip_prefix("unlockCg:")
        .map(|value| (value, keine_core::UnlockKind::Cg))
        .or_else(|| {
            cmd.strip_prefix("unlockBgm:")
                .map(|value| (value, keine_core::UnlockKind::Bgm))
        })
    {
        let file = statement_value(rest).unwrap_or_default();
        if !file.is_empty() {
            return Some(Action::Unlock {
                kind,
                name: args.get("name").cloned().unwrap_or_else(|| file.clone()),
                file,
            });
        }
    }

    if cmd == ":" {
        return Some(Action::SetTextbox {
            visible: false,
            auto: true,
        });
    }
    if let Some(rest) = cmd.strip_prefix("setTextbox:") {
        let mode = statement_value(rest).unwrap_or_else(|| "show".into());
        return Some(Action::SetTextbox {
            visible: !matches!(mode.as_str(), "hide" | "none" | "off" | "false"),
            auto: false,
        });
    }
    if let Some(rest) = cmd.strip_prefix("getUserInput:") {
        let variable = statement_value(rest).unwrap_or_default();
        if !variable.is_empty() {
            return Some(Action::UserInput {
                variable,
                title: args.get("title").cloned().unwrap_or_else(|| "INPUT".into()),
                button: args
                    .get("buttonText")
                    .cloned()
                    .unwrap_or_else(|| "OK".into()),
            });
        }
    }
    if cmd == "comment" || cmd.starts_with("comment:") {
        return Some(Action::Comment);
    }

    if let Some(rest) = cmd
        .strip_prefix("setAnimation:")
        .or_else(|| cmd.strip_prefix("setComplexAnimation:"))
        .or_else(|| cmd.strip_prefix("setTempAnimation:"))
    {
        let name = statement_value(rest).unwrap_or_else(|| "enter".into());
        return Some(Action::Animate {
            target: args
                .get("target")
                .cloned()
                .unwrap_or_else(|| "fig-center".into()),
            preset: parse_animation_preset(&name),
            duration: positive_duration(args, 0.45),
        });
    }

    if let Some(rest) = cmd.strip_prefix("setTransition:") {
        let fallback = statement_value(rest).filter(|value| !value.is_empty());
        return Some(Action::SetTransition {
            target: args
                .get("target")
                .cloned()
                .unwrap_or_else(|| "fig-center".into()),
            enter: args
                .get("enter")
                .map(|value| parse_animation_preset(value))
                .or_else(|| fallback.as_deref().map(parse_animation_preset)),
            exit: args.get("exit").map(|value| parse_animation_preset(value)),
            duration: positive_duration(args, 0.45),
        });
    }

    if let Some(rest) = cmd.strip_prefix("setFilter:") {
        return Some(Action::SetFilter {
            target: args
                .get("target")
                .cloned()
                .unwrap_or_else(|| "bg-main".into()),
            filter: parse_filter(rest),
        });
    }

    if let Some(rest) = cmd.strip_prefix("wait:") {
        let milliseconds = statement_value(rest)
            .as_deref()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(0.0)
            .max(0.0);
        return Some(Action::Wait {
            seconds: milliseconds / 1000.0,
        });
    }

    if let Some(rest) = cmd.strip_prefix("intro:") {
        let pages = split_unescaped(rest, '|')
            .into_iter()
            .map(|page| unescape_webgal(page.trim()))
            .filter(|page| !page.is_empty())
            .collect::<Vec<_>>();
        if !pages.is_empty() {
            return Some(Action::Intro {
                pages,
                hold: args.boolean("hold"),
            });
        }
    }

    if let Some(rest) = cmd.strip_prefix("filmMode:") {
        let mode = statement_value(rest).unwrap_or_else(|| "none".into());
        return Some(Action::FilmMode {
            enabled: !matches!(mode.as_str(), "none" | "disable" | "off" | "false"),
        });
    }

    if cmd == "pixiInit" || cmd.starts_with("pixiInit:") {
        return Some(Action::HideParticles {
            id: None,
            duration: 0.0,
        });
    }
    if let Some(rest) = cmd.strip_prefix("pixiPerform:") {
        let effect = statement_value(rest)
            .filter(|value| !value.is_empty() && !matches!(value.as_str(), "none" | "stop"));
        return Some(match effect {
            Some(effect) => Action::ShowParticles {
                id: "webgal".into(),
                effect: keine_core::ParticleEffect::preset(effect),
            },
            None => Action::HideParticles {
                id: Some("webgal".into()),
                duration: 0.0,
            },
        });
    }

    // setTransform:id x=100 y=0 alpha=0.5 ...
    if let Some(rest) = cmd.strip_prefix("setTransform:") {
        let id = args
            .get("target")
            .cloned()
            .or_else(|| {
                rest.split_whitespace()
                    .find(|part| !part.contains('=') && !part.starts_with('{'))
                    .map(unescape_webgal)
            })
            .unwrap_or_else(|| "0".into());
        let id = if id == "0" { figure_id("center") } else { id };
        let transform = parse_transform_patch(rest);
        if !id.is_empty() {
            return Some(Action::SetTransform {
                id,
                transform,
                duration: duration_from_args_or(args, 0.5),
                easing: easing_from_args(args),
            });
        }
    }

    // label:name
    if let Some(label) = cmd.strip_prefix("label:") {
        return Some(Action::Label(unescape_webgal(label.trim())));
    }

    // jumpLabel:target
    if let Some(target) = cmd.strip_prefix("jumpLabel:") {
        let t = unescape_webgal(target.trim());
        if !t.is_empty() {
            return Some(Action::Jump(t));
        }
    }

    // changeScene/callScene use script paths; loaded scenes are keyed by file stem.
    if let Some(rest) = cmd.strip_prefix("changeScene:") {
        return parse_scene_target(rest).map(Action::ChangeScene);
    }
    if let Some(rest) = cmd.strip_prefix("callScene:") {
        return parse_scene_target(rest).map(Action::CallScene);
    }
    if cmd == "end" {
        return Some(Action::End);
    }

    // changeBg:file [flags]
    if let Some(rest) = cmd.strip_prefix("changeBg:") {
        let image = statement_value(rest)
            .filter(|value| !value.starts_with('-'))
            .unwrap_or_default();
        if image == "none" || image.is_empty() {
            return Some(Action::HideBg {
                transition: transition_from_args(args),
            });
        }
        if !image.is_empty() {
            return Some(Action::ShowBg {
                image,
                transition: transition_from_args(args),
                transform: args
                    .get("transform")
                    .or_else(|| args.get("filter"))
                    .map_or_else(SpriteTransform::default, |value| parse_transform(value)),
            });
        }
    }

    // miniAvatar:file — show; miniAvatar:none — hide
    if let Some(rest) = cmd.strip_prefix("miniAvatar:") {
        let file = statement_value(rest).unwrap_or_default();
        if file == "none" || file.is_empty() {
            return Some(Action::HideMiniAvatar);
        }
        return Some(Action::MiniAvatar { image: file });
    }

    // changeFigure:file -left|-right [flags]
    if let Some(rest) = cmd.strip_prefix("changeFigure:") {
        let image = statement_value(rest).unwrap_or_default();
        let side = if args.boolean("left") {
            "-left"
        } else if args.boolean("right") {
            "-right"
        } else {
            "center"
        };
        let id = args.get("id").cloned().unwrap_or_else(|| figure_id(side));
        let transition = transition_from_args(args);
        if image.is_empty() || image == "none" || args.boolean("clear") || args.boolean("none") {
            return Some(Action::HideSprite { id, transition });
        }
        return Some(Action::ShowSprite {
            id,
            image,
            position: parse_position(side),
            layout: SpriteLayout::Natural,
            transition,
            transform: args
                .get("transform")
                .map_or_else(SpriteTransform::default, |value| parse_transform(value)),
            z_index: args
                .get("zIndex")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            blend: parse_blend(
                args.get("blendMode")
                    .or_else(|| args.get("blend"))
                    .map(String::as_str),
            ),
        });
    }

    // choose:text1:target1|text2:target2
    if let Some(rest) = cmd.strip_prefix("choose:") {
        let choices = parse_choices(rest);
        if !choices.is_empty() {
            return Some(Action::Menu {
                prompt: String::new(),
                choices,
            });
        }
    }

    // bgm:file
    if let Some(rest) = cmd.strip_prefix("bgm:") {
        let file = statement_value(rest).unwrap_or_default();
        return Some(Action::Bgm {
            file: if file.is_empty() { "none".into() } else { file },
            volume: percent_arg(args, "volume", 100.0),
            fade_seconds: milliseconds_arg(args, "enter"),
        });
    }

    // stopBgm
    if cmd == "stopBgm" {
        return Some(Action::Bgm {
            file: "none".into(),
            volume: 1.0,
            fade_seconds: milliseconds_arg(args, "enter"),
        });
    }

    if let Some(rest) = cmd.strip_prefix("playEffect:") {
        let file = statement_value(rest).filter(|file| !file.is_empty() && file != "none");
        return Some(Action::Effect {
            file,
            volume: percent_arg(args, "volume", 100.0),
            id: args.get("id").cloned().filter(|id| !id.is_empty()),
        });
    }

    if let Some(rest) = cmd.strip_prefix("playVideo:") {
        let file = statement_value(rest).unwrap_or_default();
        if file == "none" {
            return Some(Action::StopVideo {
                id: Some("webgal".into()),
                fade_out: 0.0,
            });
        }
        if !file.is_empty() {
            return Some(Action::PlayVideo {
                video: VideoSpec {
                    id: "webgal".into(),
                    file,
                    looped: false,
                    muted: false,
                    alpha: 1.0,
                    skippable: !args.boolean("skipOff"),
                    wait_for_finished: true,
                    mode: VideoMode::Fullscreen,
                },
            });
        }
    }

    // Say: speaker:text [flags]
    //   or  {speaker}:text [flags]
    //   or  plain text
    if let Some(rest) = cmd.strip_prefix("setVar:") {
        let (name, expression) = rest.split_once('=')?;
        if !name.trim().is_empty() {
            return Some(Action::Set {
                name: unescape_webgal(name.trim()),
                expression: unescape_webgal(expression.trim()),
                global: args.boolean("global"),
            });
        }
    }

    if let Some(rest) = cmd.strip_prefix("say:") {
        return parse_explicit_say(rest, args);
    }

    if let Some(say) = parse_say(cmd, args) {
        return Some(say);
    }

    None
}

fn parse_say(cmd: &str, args: &ScriptArgs) -> Option<Action> {
    // Check for speaker:text pattern — speaker is BEFORE colon, not after
    if let Some(colon_idx) = find_unescaped(cmd, ':') {
        let prefix = &cmd[..colon_idx].trim();
        // Only treat as speaker if prefix looks like a name (no spaces, no leading dash)
        if !prefix.starts_with('-')
            && !prefix.contains(char::is_whitespace)
            && !cmd[colon_idx + 1..].starts_with("//")
        {
            let speaker = unescape_webgal(
                prefix.trim_matches(|character: char| character == '{' || character == '}'),
            );
            let rest = cmd[colon_idx + 1..].trim();
            let text = decode_dialogue_text(rest);
            if !text.is_empty() {
                return Some(Action::Say {
                    speaker: if args.boolean("clear") {
                        String::new()
                    } else {
                        speaker
                    },
                    text,
                    options: say_options(args, false),
                });
            }
        }
    }

    // Plain narration line
    let text = decode_dialogue_text(cmd);
    if !text.is_empty() {
        return Some(Action::Say {
            speaker: String::new(),
            text,
            options: say_options(args, !args.boolean("clear")),
        });
    }
    None
}

fn parse_explicit_say(input: &str, args: &ScriptArgs) -> Option<Action> {
    let text = decode_dialogue_text(input.trim());
    if text.is_empty() {
        return None;
    }

    let clear = args.boolean("clear");
    let speaker = if clear {
        String::new()
    } else {
        args.get("speaker").cloned().unwrap_or_default()
    };
    let inherit_speaker = !clear && args.get("speaker").is_none();
    Some(Action::Say {
        speaker,
        text,
        options: say_options(args, inherit_speaker),
    })
}

fn say_options(args: &ScriptArgs, inherit_speaker: bool) -> SayOptions {
    SayOptions {
        vocal: args.get("vocal").or_else(|| args.get("V")).cloned(),
        volume: args
            .get("volume")
            .and_then(|value| value.parse::<f32>().ok())
            .map_or(1.0, |value| value / 100.0),
        concat: args.boolean("concat"),
        auto_advance: args.boolean("notend"),
        inherit_speaker,
    }
}

#[derive(Default)]
struct ScriptArgs(HashMap<String, String>);

impl ScriptArgs {
    fn get(&self, key: &str) -> Option<&String> {
        self.0.get(key)
    }

    fn boolean(&self, key: &str) -> bool {
        self.get(key)
            .is_some_and(|value| !matches!(value.as_str(), "false" | "0"))
    }
}

fn split_command_args(input: &str) -> (String, ScriptArgs) {
    let mut depth = 0_u32;
    let mut quote = None;
    let chars = input.char_indices().collect::<Vec<_>>();
    let mut split = None;
    for (index, (offset, value)) in chars.iter().copied().enumerate() {
        if quote.is_some() {
            if quote == Some(value)
                && chars.get(index.wrapping_sub(1)).map(|(_, c)| *c) != Some('\\')
            {
                quote = None;
            }
            continue;
        }
        match value {
            '"' | '\'' => quote = Some(value),
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth = depth.saturating_sub(1),
            value
                if value.is_whitespace()
                    && depth == 0
                    && input[offset..].trim_start().starts_with('-') =>
            {
                split = Some(offset);
                break;
            }
            _ => {}
        }
    }

    let (content, raw_args) = split.map_or((input, ""), |offset| input.split_at(offset));
    let mut args = ScriptArgs::default();
    for token in split_flag_tokens(raw_args) {
        let raw = token.as_str();
        let Some(raw) = raw.strip_prefix('-') else {
            continue;
        };
        if let Some((key, value)) = raw.split_once('=') {
            args.0.insert(
                key.to_owned(),
                unescape_webgal(strip_wrapping_quotes(value)),
            );
        } else if raw.contains('.') && !matches!(raw, "left" | "right" | "center") {
            args.0.insert("vocal".into(), unescape_webgal(raw));
        } else {
            args.0.insert(raw.to_owned(), "true".into());
        }
    }
    (content.trim().to_owned(), args)
}

fn split_flag_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0_u32;
    for value in input.chars() {
        if escaped {
            token.push(value);
            escaped = false;
            continue;
        }
        if value == '\\' {
            token.push(value);
            escaped = true;
            continue;
        }
        if let Some(active) = quote {
            token.push(value);
            if value == active {
                quote = None;
            }
            continue;
        }
        match value {
            '"' | '\'' => {
                quote = Some(value);
                token.push(value);
            }
            '{' | '[' | '(' => {
                depth += 1;
                token.push(value);
            }
            '}' | ']' | ')' => {
                depth = depth.saturating_sub(1);
                token.push(value);
            }
            value if value.is_whitespace() && depth == 0 => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            _ => token.push(value),
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

fn strip_wrapping_quotes(input: &str) -> &str {
    let Some(quote) = input.chars().next() else {
        return input;
    };
    if !matches!(quote, '"' | '\'') {
        return input;
    }

    let rest = &input[quote.len_utf8()..];
    let Some(closing) = find_unescaped(rest, quote) else {
        return input;
    };
    if closing + quote.len_utf8() == rest.len() {
        &rest[..closing]
    } else {
        input
    }
}

fn transition_from_args(args: &ScriptArgs) -> Transition {
    let named = args.get("enter").or_else(|| args.get("exit"));
    let parsed_duration = duration_from_args(args);
    let duration = if parsed_duration > 0.0 {
        parsed_duration
    } else if named.is_some() {
        0.45
    } else {
        0.0
    };
    if let Some(name) = named.map(String::as_str) {
        return match name {
            "enter-from-left" | "exit-to-left" => Transition::SlideFromLeft(duration),
            "enter-from-right" | "exit-to-right" => Transition::SlideFromRight(duration),
            "wipe" | "wipeIn" | "wipeOut" => Transition::Wipe(duration),
            "dissolve" => Transition::Dissolve(duration),
            "crossfade" => Transition::Crossfade(duration),
            _ => Transition::Fade(duration),
        };
    }
    if args.boolean("wipe") || args.get("transition").is_some_and(|value| value == "wipe") {
        Transition::Wipe(if duration > 0.0 { duration } else { 0.5 })
    } else if args.boolean("dissolve")
        || args
            .get("transition")
            .is_some_and(|value| value == "dissolve")
    {
        Transition::Dissolve(if duration > 0.0 { duration } else { 0.5 })
    } else if args.boolean("fade") {
        Transition::Fade(if duration > 0.0 { duration } else { 0.5 })
    } else if duration > 0.0 {
        Transition::Fade(duration)
    } else {
        Transition::Instant
    }
}

fn positive_duration(args: &ScriptArgs, default: f32) -> f32 {
    let duration = duration_from_args(args);
    if duration > 0.0 { duration } else { default }
}

fn duration_from_args(args: &ScriptArgs) -> f32 {
    args.get("duration")
        .and_then(|value| value.parse::<f32>().ok())
        .map(|milliseconds| milliseconds / 1000.0)
        .unwrap_or(0.0)
}

fn duration_from_args_or(args: &ScriptArgs, default: f32) -> f32 {
    args.get("duration")
        .and_then(|value| value.parse::<f32>().ok())
        .map_or(default, |milliseconds| milliseconds.max(0.0) / 1000.0)
}

fn milliseconds_arg(args: &ScriptArgs, key: &str) -> f32 {
    args.get(key)
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.0)
        .max(0.0)
        / 1000.0
}

fn percent_arg(args: &ScriptArgs, key: &str, default: f32) -> f32 {
    args.get(key)
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(default)
        .clamp(0.0, 100.0)
        / 100.0
}

fn easing_from_args(args: &ScriptArgs) -> Easing {
    match args
        .get("ease")
        .or_else(|| args.get("easing"))
        .map(String::as_str)
    {
        Some("linear") => Easing::Linear,
        Some("easeIn") | Some("ease-in") => Easing::EaseIn,
        Some("easeOut") | Some("ease-out") => Easing::EaseOut,
        Some("easeInOut") | Some("ease-in-out") => Easing::EaseInOut,
        _ => Easing::EaseInOut,
    }
}

fn parse_blend(value: Option<&str>) -> BlendMode {
    match value {
        Some("add") | Some("additive") => BlendMode::Add,
        Some("multiply") => BlendMode::Multiply,
        Some("screen") => BlendMode::Screen,
        _ => BlendMode::Alpha,
    }
}

fn parse_animation_preset(value: &str) -> AnimationPreset {
    match value.trim() {
        "enter" => AnimationPreset::Enter,
        "exit" => AnimationPreset::Exit,
        "shake" => AnimationPreset::Shake,
        "enter-from-bottom" => AnimationPreset::EnterFromBottom,
        "enter-from-left" => AnimationPreset::EnterFromLeft,
        "enter-from-right" => AnimationPreset::EnterFromRight,
        "move-front-and-back" => AnimationPreset::MoveFrontAndBack,
        "blur" => AnimationPreset::Blur,
        "oldFilm" => AnimationPreset::OldFilm,
        "dotFilm" => AnimationPreset::DotFilm,
        "reflectionFilm" => AnimationPreset::ReflectionFilm,
        "glitchFilm" => AnimationPreset::GlitchFilm,
        "rgbFilm" => AnimationPreset::RgbFilm,
        "godrayFilm" => AnimationPreset::GodrayFilm,
        "removeFilm" => AnimationPreset::RemoveFilm,
        "shockwaveIn" => AnimationPreset::ShockwaveIn,
        "shockwaveOut" => AnimationPreset::ShockwaveOut,
        value => AnimationPreset::Custom(value.to_owned()),
    }
}

fn parse_filter(input: &str) -> VisualFilter {
    if matches!(input.trim(), "" | "none" | "clear") {
        return VisualFilter::default();
    }
    let mut filter = VisualFilter::default();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(input) else {
        return filter;
    };
    let number = |name: &str, default: f32| {
        value
            .get(name)
            .and_then(serde_json::Value::as_f64)
            .map_or(default, |value| value as f32)
    };
    filter.blur = number("blur", 0.0).max(0.0);
    filter.brightness = normalize_filter_ratio(number("brightness", 1.0));
    filter.contrast = normalize_filter_ratio(number("contrast", 1.0));
    filter.saturation = normalize_filter_ratio(number("saturation", 1.0));
    filter
}

fn normalize_filter_ratio(value: f32) -> f32 {
    if value > 4.0 { value / 100.0 } else { value }.clamp(0.0, 4.0)
}

fn parse_transform(input: &str) -> SpriteTransform {
    parse_transform_patch(input).apply_to(SpriteTransform::default())
}

fn parse_transform_patch(input: &str) -> TransformPatch {
    let mut transform = TransformPatch::default();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(input) {
        let number = |path: &[&str]| {
            path.iter()
                .try_fold(&value, |value, key| value.get(*key))
                .and_then(serde_json::Value::as_f64)
                .map(|value| value as f32)
        };
        if let Some(value) = number(&["position", "x"]).or_else(|| number(&["x"])) {
            transform.set_offset_x(value);
        }
        if let Some(value) = number(&["position", "y"]).or_else(|| number(&["y"])) {
            transform.set_offset_y(value);
        }
        if let Some(value) = number(&["alpha"]) {
            transform.set_alpha(value);
        }
        if let Some(value) = number(&["scale", "x"]).or_else(|| number(&["scale_x"])) {
            transform.set_scale_x(value);
        }
        if let Some(value) = number(&["scale", "y"]).or_else(|| number(&["scale_y"])) {
            transform.set_scale_y(value);
        }
        if let Some(value) = number(&["rotation"]) {
            transform.set_rotation(value);
        }
        if let Some(value) = number(&["blur"]) {
            transform.set_blur(value);
        }
        return transform;
    }
    for part in input.split_whitespace() {
        if let Some((key, value)) = part.split_once('=') {
            match key {
                "x" => transform.set_offset_x(parse_number(value, "x")),
                "y" => transform.set_offset_y(parse_number(value, "y")),
                "alpha" => transform.set_alpha(parse_number(value, "alpha")),
                "scale_x" => transform.set_scale_x(parse_number(value, "scale_x")),
                "scale_y" => transform.set_scale_y(parse_number(value, "scale_y")),
                "rotation" => transform.set_rotation(parse_number(value, "rotation")),
                "blur" => transform.set_blur(parse_number(value, "blur")),
                _ => {}
            }
        }
    }
    transform
}

fn parse_choices(input: &str) -> Vec<Choice> {
    let mut choices = Vec::new();
    for part in split_unescaped(input, '|') {
        let part = part.trim();
        let (conditions, main) = part
            .split_once("->")
            .map_or(("", part), |(conditions, main)| (conditions, main));
        if let Some(colon_idx) = find_unescaped(main, ':') {
            let text = unescape_webgal(main[..colon_idx].trim());
            let target = main[colon_idx + 1..].trim();
            if !text.is_empty() && !target.is_empty() {
                choices.push(Choice {
                    text,
                    target: parse_webgal_choice_target(target),
                    show_when: condition_between(conditions, '(', ')'),
                    enable_when: condition_between(conditions, '[', ']'),
                });
            }
        }
    }
    choices
}

fn condition_between(input: &str, start: char, end: char) -> Option<String> {
    let open = input.find(start)?;
    let content_start = open + start.len_utf8();
    let mut depth = 0_u32;
    let mut quote = None;
    let mut escaped = false;
    for (offset, value) in input[content_start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if value == '\\' {
            escaped = true;
            continue;
        }
        if let Some(active) = quote {
            if value == active {
                quote = None;
            }
            continue;
        }
        match value {
            '"' | '\'' => quote = Some(value),
            value if value == start => depth += 1,
            value if value == end && depth > 0 => depth -= 1,
            value if value == end => {
                let close = content_start + offset;
                return Some(unescape_webgal(input[content_start..close].trim()))
                    .filter(|condition| !condition.is_empty());
            }
            _ => {}
        }
    }
    None
}

fn parse_webgal_choice_target(target: &str) -> ChoiceTarget {
    if let Some(scene) = target
        .strip_prefix("callScene(")
        .and_then(|target| target.strip_suffix(')'))
    {
        ChoiceTarget::CallScene(parse_scene_target(scene).unwrap_or_else(|| scene.to_owned()))
    } else if target.contains(['.', '/', '\\']) {
        ChoiceTarget::ChangeScene(parse_scene_target(target).unwrap_or_else(|| target.to_owned()))
    } else {
        ChoiceTarget::Label(unescape_webgal(target))
    }
}

fn parse_position(side: &str) -> Position {
    match side {
        "-left" => Position::left(0.0),
        "-right" => Position::right(0.0),
        _ => Position::center(0.0),
    }
}

fn parse_scene_target(input: &str) -> Option<String> {
    let mut path = statement_value(input)?.replace('\\', "/");
    while let Some(stripped) = path.strip_prefix("./") {
        path = stripped.to_owned();
    }
    while path.ends_with('/') {
        path.pop();
    }
    if path
        .get(path.len().saturating_sub(4)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".txt"))
    {
        path.truncate(path.len() - 4);
    }
    (!path.is_empty()).then_some(path)
}

fn before_unescaped(input: &str, delimiter: char) -> &str {
    find_unescaped(input, delimiter).map_or(input, |index| &input[..index])
}

fn find_unescaped(input: &str, delimiter: char) -> Option<usize> {
    let mut escaped = false;
    for (index, character) in input.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == delimiter {
            return Some(index);
        }
    }
    None
}

fn split_unescaped(input: &str, delimiter: char) -> Vec<&str> {
    let mut values = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (index, character) in input.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == delimiter {
            values.push(&input[start..index]);
            start = index + character.len_utf8();
        }
    }
    values.push(&input[start..]);
    values
}

fn unescape_webgal(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut characters = input.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\'
            && characters
                .peek()
                .is_some_and(|next| matches!(next, ':' | ',' | '.' | ';' | '|'))
        {
            output.push(characters.next().expect("peeked character exists"));
        } else {
            output.push(character);
        }
    }
    output
}

fn decode_dialogue_text(input: &str) -> String {
    split_unescaped(input.trim(), '|')
        .into_iter()
        .map(unescape_webgal)
        .collect::<Vec<_>>()
        .join("\n")
}

fn statement_value(input: &str) -> Option<String> {
    let input = input.trim_start();
    let first = input.chars().next()?;
    let value = if matches!(first, '"' | '\'') {
        let rest = &input[first.len_utf8()..];
        let end = find_unescaped(rest, first)?;
        &rest[..end]
    } else {
        input.split_whitespace().next()?
    };
    Some(unescape_webgal(value))
}

fn strip_legacy_slash_comment(input: &str) -> &str {
    let bytes = input.as_bytes();
    let mut escaped = false;
    let mut quote = None;
    let mut previous = None;
    for (index, character) in input.char_indices() {
        if escaped {
            escaped = false;
            previous = Some(character);
            continue;
        }
        if character == '\\' {
            escaped = true;
            previous = Some(character);
            continue;
        }
        if let Some(active) = quote {
            if character == active {
                quote = None;
            }
            previous = Some(character);
            continue;
        }
        if matches!(character, '"' | '\'') {
            quote = Some(character);
            previous = Some(character);
            continue;
        }
        if character == '/'
            && bytes.get(index + 1) == Some(&b'/')
            && previous.is_none_or(char::is_whitespace)
        {
            return &input[..index];
        }
        previous = Some(character);
    }
    input
}

fn unsupported_command(input: &str) -> Option<&'static str> {
    let end = input
        .char_indices()
        .find_map(|(index, character)| {
            (character == ':' || character.is_whitespace()).then_some(index)
        })
        .unwrap_or(input.len());
    match &input[..end] {
        "showVars" => Some("showVars"),
        "applyStyle" => Some("applyStyle"),
        "callSteam" => Some("callSteam"),
        _ => None,
    }
}

fn figure_id(side: &str) -> String {
    match side {
        "-left" => "fig-left".into(),
        "-right" => "fig-right".into(),
        _ => "fig-center".into(),
    }
}

fn parse_number(value: &str, field: &str) -> f32 {
    value.parse().unwrap_or_else(|_| {
        log::warn!("Invalid setTransform {field} value: {value}");
        0.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_change_figure_removal() {
        let actions = parse_webgal(
            "changeFigure:none -left;\n\
             changeFigure: -right;\n\
             changeFigure:ignored.webp -id=extra -clear;\n\
             changeFigure:ignored.webp -id=flagged -none;",
        );
        assert_eq!(
            actions,
            vec![
                Action::HideSprite {
                    id: "fig-left".into(),
                    transition: Transition::Instant,
                },
                Action::HideSprite {
                    id: "fig-right".into(),
                    transition: Transition::Instant,
                },
                Action::HideSprite {
                    id: "extra".into(),
                    transition: Transition::Instant,
                },
                Action::HideSprite {
                    id: "flagged".into(),
                    transition: Transition::Instant,
                },
            ]
        );
    }

    #[test]
    fn parses_transform_target_without_numeric_coercion() {
        let actions = parse_webgal("setTransform:x=12 alpha=0.5 -target=hero;");
        let Action::SetTransform {
            id,
            transform,
            duration,
            ..
        } = &actions[0]
        else {
            panic!("expected transform action");
        };
        let applied = transform.apply_to(SpriteTransform::default());

        assert_eq!(id, "hero");
        assert_eq!(applied.offset_x, 12.0);
        assert_eq!(applied.alpha, 0.5);
        assert_eq!(*duration, 0.5);
    }

    #[test]
    fn set_transform_distinguishes_default_and_explicit_zero_duration() {
        let actions = parse_webgal(
            "setTransform:{\"alpha\":0.5} -target=hero;\n\
             setTransform:{\"alpha\":1} -target=hero -duration=0;",
        );

        assert!(matches!(
            actions[0],
            Action::SetTransform { duration: 0.5, .. }
        ));
        assert!(matches!(
            actions[1],
            Action::SetTransform { duration: 0.0, .. }
        ));
    }

    #[test]
    fn set_transform_maps_webgal_default_target_to_center_figure() {
        let actions = parse_webgal(
            "setTransform:{\"alpha\":0.4};\n\
             setTransform:{\"alpha\":0.5} -target=0;\n\
             setTransform:{\"alpha\":0.6} -target=hero;",
        );

        assert!(matches!(
            &actions[0],
            Action::SetTransform { id, .. } if id == "fig-center"
        ));
        assert!(matches!(
            &actions[1],
            Action::SetTransform { id, .. } if id == "fig-center"
        ));
        assert!(matches!(
            &actions[2],
            Action::SetTransform { id, .. } if id == "hero"
        ));
    }

    #[test]
    fn json_transform_patch_preserves_every_absent_field() {
        let actions = parse_webgal(
            "setTransform:{\"position\":{\"x\":100},\"blur\":0} -target=hero -duration=250;",
        );
        let Action::SetTransform { transform, .. } = actions[0] else {
            panic!("expected transform action");
        };
        let base = SpriteTransform {
            offset_x: 8.0,
            offset_y: -30.0,
            alpha: 0.4,
            scale_x: 1.2,
            scale_y: 0.8,
            rotation: 0.25,
            blur: 9.0,
            ..SpriteTransform::default()
        };

        assert_eq!(
            transform.apply_to(base),
            SpriteTransform {
                offset_x: 100.0,
                blur: 0.0,
                ..base
            }
        );
    }

    #[test]
    fn parses_scene_control_commands() {
        assert_eq!(
            parse_webgal(
                "changeScene:chapter/part-2.txt;\n\
                 callScene:aside\\talk.txt;\n\
                 changeScene:\"bonus route/final scene.txt\";\n\
                 end;",
            ),
            vec![
                Action::ChangeScene("chapter/part-2".into()),
                Action::CallScene("aside/talk".into()),
                Action::ChangeScene("bonus route/final scene".into()),
                Action::End,
            ]
        );
    }

    #[test]
    fn parses_webgal_semicolon_comments_escapes_and_dialogue_lines() {
        let report = parse_webgal_report(
            r#"say:https://example.com/a\:b\,c\.d\;e\|f|next -speaker=Alice; ignored
Alice:legacy // ignored
say:final; comment"#,
        );

        assert!(report.diagnostics.is_empty());
        assert_eq!(report.actions.len(), 3);
        assert!(matches!(
            &report.actions[0],
            Action::Say { speaker, text, .. }
                if speaker == "Alice" && text == "https://example.com/a:b,c.d;e|f\nnext"
        ));
        assert!(matches!(
            &report.actions[1],
            Action::Say { speaker, text, .. } if speaker == "Alice" && text == "legacy"
        ));
        assert!(matches!(
            &report.actions[2],
            Action::Say { text, .. } if text == "final"
        ));
    }

    #[test]
    fn legacy_slash_comments_respect_quotes_escapes_and_json() {
        let report = parse_webgal_report(
            r#"Alice:"A \"quoted\" // value";
setFilter:{"blur":6,"note":"C // D"} -target=hero;
Bob:hello // trailing comment"#,
        );

        assert!(report.diagnostics.is_empty());
        assert!(matches!(
            &report.actions[0],
            Action::Say { text, .. } if text.contains("// value")
        ));
        assert!(matches!(
            &report.actions[1],
            Action::SetFilter { target, filter } if target == "hero" && filter.blur == 6.0
        ));
        assert!(matches!(
            &report.actions[2],
            Action::Say { speaker, text, .. } if speaker == "Bob" && text == "hello"
        ));
    }

    #[test]
    fn preprocesses_indented_continuations_and_keeps_origin_span() {
        let report = parse_webgal_report(
            "  Alice:first line\n    |second line\n    -vocal=voice.ogg -volume=40;\nBob:next;",
        );

        assert!(report.diagnostics.is_empty());
        assert_eq!(report.actions.len(), 2);
        assert!(matches!(
            &report.actions[0],
            Action::Say { speaker, text, options }
                if speaker == "Alice"
                    && text == "first line\nsecond line"
                    && options.vocal.as_deref() == Some("voice.ogg")
                    && options.volume == 0.4
        ));
        assert_eq!(report.spans[0], SourceSpan { line: 1, column: 3 });
        assert_eq!(report.spans[1], SourceSpan { line: 4, column: 1 });
        assert_eq!(report.resources[0].span, report.spans[0]);
    }

    #[test]
    fn concat_flag_starts_a_new_logical_line_like_webgal() {
        let report = parse_webgal_report("Alice:first;\n  |second -concat;");

        assert!(report.diagnostics.is_empty());
        assert_eq!(report.actions.len(), 2);
        assert!(matches!(
            &report.actions[1],
            Action::Say { text, options, .. } if text == "\nsecond" && options.concat
        ));
        assert_eq!(report.spans[1], SourceSpan { line: 2, column: 3 });
    }

    #[test]
    fn parses_official_and_simplified_say_speaker_rules() {
        let actions = parse_webgal(
            "say:Hello -speaker=Alice;\n\
             say:Again;\n\
             say:Narration -clear;\n\
             Bob:Simplified;\n\
             plain continuation;\n\
             :Explicit narration;",
        );

        assert!(matches!(
            &actions[0],
            Action::Say { speaker, options, .. }
                if speaker == "Alice" && !options.inherit_speaker
        ));
        assert!(matches!(
            &actions[1],
            Action::Say { speaker, options, .. }
                if speaker.is_empty() && options.inherit_speaker
        ));
        assert!(matches!(
            &actions[2],
            Action::Say { speaker, options, .. }
                if speaker.is_empty() && !options.inherit_speaker
        ));
        assert!(matches!(
            &actions[3],
            Action::Say { speaker, options, .. }
                if speaker == "Bob" && !options.inherit_speaker
        ));
        assert!(matches!(
            &actions[4],
            Action::Say { speaker, options, .. }
                if speaker.is_empty() && options.inherit_speaker
        ));
        assert!(matches!(
            &actions[5],
            Action::Say { speaker, options, .. }
                if speaker.is_empty() && !options.inherit_speaker
        ));
    }

    #[test]
    fn parses_official_blend_mode_and_ease_with_legacy_aliases() {
        let actions = parse_webgal(
            "changeFigure:hero.webp -blendMode=screen;\n\
             changeFigure:glow.webp -blend=add;\n\
             setTransform:x=1 -target=hero -ease=easeOut;\n\
             setTransform:x=2 -target=hero -easing=easeIn;\n\
             setTransform:x=3 -target=hero;",
        );

        assert!(matches!(
            actions[0],
            Action::ShowSprite {
                blend: BlendMode::Screen,
                ..
            }
        ));
        assert!(matches!(
            actions[1],
            Action::ShowSprite {
                blend: BlendMode::Add,
                ..
            }
        ));
        assert!(matches!(
            actions[2],
            Action::SetTransform {
                easing: Easing::EaseOut,
                ..
            }
        ));
        assert!(matches!(
            actions[3],
            Action::SetTransform {
                easing: Easing::EaseIn,
                ..
            }
        ));
        assert!(matches!(
            actions[4],
            Action::SetTransform {
                easing: Easing::EaseInOut,
                ..
            }
        ));
    }

    #[test]
    fn parses_escaped_choice_delimiters_and_nested_targets() {
        let actions =
            parse_webgal(r#"choose:Look \: closer:end|Pipe \| literal:chapter_01/part_02.txt;"#);
        let Action::Menu { choices, .. } = &actions[0] else {
            panic!("expected choice menu");
        };

        assert_eq!(choices[0].text, "Look : closer");
        assert_eq!(choices[0].target, ChoiceTarget::Label("end".into()));
        assert_eq!(choices[1].text, "Pipe | literal");
        assert_eq!(
            choices[1].target,
            ChoiceTarget::ChangeScene("chapter_01/part_02".into())
        );
    }

    #[test]
    fn reports_reserved_unsupported_commands_without_dialogue_fallback() {
        let report = parse_webgal_report(
            "playVideo:opening.mp4;\nshowVars;\napplyStyle:#app;\ncallSteam:achievement;\nfutureCommand:value;",
        );

        assert_eq!(report.diagnostics.len(), 3);
        for (index, command) in ["showVars", "applyStyle", "callSteam"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(report.diagnostics[index].span.line, index + 2);
            assert_eq!(
                report.diagnostics[index].message,
                format!("unsupported WebGAL command `{command}`")
            );
        }
        assert!(matches!(
            &report.actions[..],
            [Action::PlayVideo { .. }, Action::Say { speaker, text, .. }]
                if speaker == "futureCommand" && text == "value"
        ));
    }

    #[test]
    fn video_is_typed_blocking_and_resource_tracked() {
        let report = parse_webgal_report("playVideo:opening.mp4 -skipOff;\nplayVideo:none;");

        assert!(matches!(
            &report.actions[0],
            Action::PlayVideo { video }
                if video.file == "opening.mp4"
                    && !video.skippable
                    && video.wait_for_finished
        ));
        assert!(matches!(
            &report.actions[1],
            Action::StopVideo { id: Some(id), .. } if id == "webgal"
        ));
        assert!(matches!(
            report.resources.as_slice(),
            [crate::ResourceRef { kind: crate::ResourceKind::Video, path, .. }]
                if path == "opening.mp4"
        ));
    }

    #[test]
    fn parses_say_flow_and_audio_arguments() {
        let actions = parse_webgal(
            "Sayo:Hello {name} -voice.wav -volume=30 -concat -notend -when=ready -next;",
        );
        assert!(matches!(
            &actions[0],
            Action::Flow { when: Some(condition), next: true, action }
                if condition == "ready"
                    && matches!(action.as_ref(), Action::Say { options, .. }
                        if options.vocal.as_deref() == Some("voice.wav")
                            && options.volume == 0.3
                            && options.concat
                            && options.auto_advance)
        ));
    }

    #[test]
    fn preserves_quoted_string_literals_in_when_expressions() {
        let actions = parse_webgal(
            "未来的澪:车票在相机包里。 -when=route==\"海玻璃车票\";\n\
             未来的澪:站长收取炸虾。 -when=route==\"海鸥站长\";\n\
             旁白:两个字符串字面量也不是参数外壳。 -when=\"海玻璃车票\"==\"海鸥站长\";",
        );

        assert!(matches!(
            &actions[0],
            Action::Flow { when: Some(condition), .. }
                if condition == "route==\"海玻璃车票\""
        ));
        assert!(matches!(
            &actions[1],
            Action::Flow { when: Some(condition), .. }
                if condition == "route==\"海鸥站长\""
        ));
        assert!(matches!(
            &actions[2],
            Action::Flow { when: Some(condition), .. }
                if condition == "\"海玻璃车票\"==\"海鸥站长\""
        ));
    }

    #[test]
    fn flag_tokenizer_keeps_escaped_quotes_and_inner_whitespace() {
        let tokens = split_flag_tokens(r#" -when=route=="海玻璃 \"纪念 车票\"" -next"#);

        assert_eq!(tokens, [r#"-when=route=="海玻璃 \"纪念 车票\"""#, "-next"]);
    }

    #[test]
    fn parses_bgm_and_effect_parameters() {
        let actions = parse_webgal(
            "bgm:theme.ogg -volume=35 -enter=1200;\n\
             bgm: -enter=900;\n\
             playEffect:rain.ogg -volume=60 -id=weather;\n\
             playEffect:none -id=weather;",
        );
        assert!(matches!(
            &actions[0],
            Action::Bgm { file, volume, fade_seconds }
                if file == "theme.ogg" && *volume == 0.35 && *fade_seconds == 1.2
        ));
        assert!(matches!(
            &actions[1],
            Action::Bgm { file, fade_seconds, .. }
                if file == "none" && *fade_seconds == 0.9
        ));
        assert!(matches!(
            &actions[2],
            Action::Effect { file: Some(file), volume, id: Some(id) }
                if file == "rain.ogg" && *volume == 0.6 && id == "weather"
        ));
        assert!(matches!(
            &actions[3],
            Action::Effect { file: None, id: Some(id), .. } if id == "weather"
        ));
    }

    #[test]
    fn parses_choice_conditions_scene_and_call_targets() {
        let actions = parse_webgal(
            "choose:(score>0)[enabled]->Scene:chapter.txt|Call:callScene(aside.txt)|Label:end;",
        );
        let Action::Menu { choices, .. } = &actions[0] else {
            panic!("expected choice menu");
        };
        assert_eq!(choices[0].show_when.as_deref(), Some("score>0"));
        assert_eq!(choices[0].enable_when.as_deref(), Some("enabled"));
        assert_eq!(
            choices[0].target,
            ChoiceTarget::ChangeScene("chapter".into())
        );
        assert_eq!(choices[1].target, ChoiceTarget::CallScene("aside".into()));
        assert_eq!(choices[2].target, ChoiceTarget::Label("end".into()));
    }

    #[test]
    fn parses_nested_array_access_in_choice_conditions() {
        let actions =
            parse_webgal("choose:[clues[2]]->Known:end|[(flags[1] && scores[0] > 3)]->Ready:go;");
        let Action::Menu { choices, .. } = &actions[0] else {
            panic!("expected choice menu");
        };

        assert_eq!(choices[0].enable_when.as_deref(), Some("clues[2]"));
        assert_eq!(
            choices[1].enable_when.as_deref(),
            Some("(flags[1] && scores[0] > 3)")
        );
    }

    #[test]
    fn parses_global_expression_and_json_transform() {
        let actions = parse_webgal(
            "setVar:items=[1, 2, 3] -global;\nsetTransform:{\"position\":{\"x\":100},\"blur\":8} -target=bg-main -duration=500 -easing=easeInOut;",
        );
        assert!(matches!(
            &actions[0],
            Action::Set { expression, global: true, .. } if expression == "[1, 2, 3]"
        ));
        let Action::SetTransform {
            id,
            transform,
            duration,
            easing: Easing::EaseInOut,
        } = &actions[1]
        else {
            panic!("expected transform action");
        };
        let applied = transform.apply_to(SpriteTransform::default());
        assert_eq!(id, "bg-main");
        assert_eq!(applied.offset_x, 100.0);
        assert_eq!(applied.blur, 8.0);
        assert_eq!(*duration, 0.5);
    }

    #[test]
    fn report_tracks_spans_resources_subscenes_and_diagnostics() {
        let report = parse_webgal_report(
            "changeBg:bg.webp;\nSayo:Hi -voice.wav;\ncallScene:aside.txt;\nsetVar:broken;",
        );
        assert_eq!(report.spans[1].line, 2);
        assert_eq!(report.resources.len(), 2);
        assert_eq!(report.sub_scenes[0].scene, "aside");
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].span.line, 4);
    }

    #[test]
    fn parses_phase_five_presentation_commands() {
        let report = parse_webgal_report(
            "setAnimation:shake -target=hero -duration=350;\n\
             setTransition: -target=hero -enter=enter-from-left -exit=exit;\n\
             setFilter:{\"blur\":6,\"brightness\":90} -target=hero;\n\
             intro:first|second -hold;\n\
             filmMode:enable;\n\
             wait:250;\n\
             pixiInit;\n\
             pixiPerform:rain;\n\
             setTempAnimation:glitchFilm -target=bg-main -next;",
        );
        assert!(report.diagnostics.is_empty());
        assert_eq!(report.actions.len(), 9);
        assert!(matches!(
            report.actions[0],
            Action::Animate {
                preset: AnimationPreset::Shake,
                duration,
                ..
            } if duration == 0.35
        ));
        assert!(matches!(
            report.actions[2],
            Action::SetFilter { filter, .. } if filter.blur == 6.0 && filter.brightness == 0.9
        ));
        assert!(matches!(report.actions[8], Action::Flow { next: true, .. }));
    }

    #[test]
    fn parses_text_input_comments_and_gallery_unlocks() {
        let report = parse_webgal_report(
            ":;\nsetTextbox:show;\ncomment:note;\n\
             getUserInput:name -title=\"Your name\" -buttonText=\"Confirm\";\n\
             unlockCg:cg.webp -name=\"Spring scene\";",
        );
        assert!(report.diagnostics.is_empty());
        assert!(matches!(
            report.actions[0],
            Action::SetTextbox {
                visible: false,
                auto: true
            }
        ));
        assert!(matches!(report.actions[2], Action::Comment));
        assert!(matches!(
            &report.actions[3],
            Action::UserInput { title, button, .. }
                if title == "Your name" && button == "Confirm"
        ));
        assert!(matches!(
            &report.actions[4],
            Action::Unlock { name, .. } if name == "Spring scene"
        ));
    }
}
