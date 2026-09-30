//! Shou command catalogue and insertion templates shared by the palette and completion.
use std::collections::HashSet;

use keine_loader::{NativeTokenKind, parse_native_document};

use super::{AssetKind, AuthoringEditError, AuthoringIndex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertKind {
    Narration,
    Dialogue,
    Background,
    Figure,
    Choice,
    Conditional,
    Loop,
    Variable,
    Goto,
    Call,
    Wait,
    Hide,
    Move,
    CameraMove,
    CameraShake,
    SpriteFocusRule,
    SpriteFocus,
    Bgm,
    Effect,
    Video,
    Return,
    Native(&'static str),
}

impl InsertKind {
    /// Canonical source names, including commands whose required asset does not
    /// exist yet. Text completion and the insertion palette share this catalogue.
    pub fn source_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Narration | Self::Dialogue => return None,
            Self::Background => "background",
            Self::Figure => "sprite",
            Self::Choice => "choice",
            Self::Conditional => "if",
            Self::Loop => "loop",
            Self::Variable => "let",
            Self::Goto => "goto",
            Self::Call => "call",
            Self::Wait => "wait",
            Self::Hide => "hide",
            Self::Move => "move",
            Self::CameraMove => "camera.move",
            Self::CameraShake => "camera.shake",
            Self::SpriteFocusRule => "sprite.focus.configure",
            Self::SpriteFocus => "sprite.focus",
            Self::Bgm => "bgm",
            Self::Effect => "se",
            Self::Video => "video",
            Self::Return => "return",
            Self::Native(name) => name,
        })
    }

    pub const ALL: [Self; 75] = [
        Self::Narration,
        Self::Dialogue,
        Self::Background,
        Self::Figure,
        Self::Hide,
        Self::Move,
        Self::CameraMove,
        Self::CameraShake,
        Self::SpriteFocusRule,
        Self::SpriteFocus,
        Self::Bgm,
        Self::Effect,
        Self::Video,
        Self::Choice,
        Self::Conditional,
        Self::Loop,
        Self::Goto,
        Self::Call,
        Self::Return,
        Self::Wait,
        Self::Variable,
        Self::Native("story.end"),
        Self::Native("avatar.show"),
        Self::Native("avatar.hide"),
        Self::Native("vocal.play"),
        Self::Native("vocal.stop"),
        Self::Native("screen.film"),
        Self::Native("text.box"),
        Self::Native("wait.advance"),
        Self::Native("playback.auto"),
        Self::Native("ui.show"),
        Self::Native("ui.hide"),
        Self::Native("particle.layers.clear"),
        Self::Native("text.presentation"),
        Self::Native("text.retract"),
        Self::Native("text.float.hide"),
        Self::Native("text.float.configure"),
        Self::Native("text.style"),
        Self::Native("scene.parallax.stop"),
        Self::Native("particle.hide"),
        Self::Native("video.stop"),
        Self::Native("gallery.unlock"),
        Self::Native("input.simple"),
        Self::Native("camera.bind"),
        Self::Native("camera.unbind"),
        Self::Native("sprite.offset"),
        Self::Native("sprite.transform"),
        Self::Native("background.transform"),
        Self::Native("sprite.filter"),
        Self::Native("sprite.animate"),
        Self::Native("sprite.transition"),
        Self::Native("se.loop"),
        Self::Native("se.stop"),
        Self::Native("video.play"),
        Self::Native("screen.curtain.show"),
        Self::Native("screen.curtain.hide"),
        Self::Native("text.float"),
        Self::Native("scene.parallax"),
        Self::Native("particle.show"),
        Self::Native("ui.message"),
        Self::Native("text.intro"),
        Self::Native("sprite.sequence"),
        Self::Native("sprite.sequence.timed"),
        Self::Native("sprite.select"),
        Self::Native("sprite.keyframes"),
        Self::Native("assets.loading"),
        Self::Native("input.request"),
        Self::Native("text.paragraph.style"),
        Self::Native("sprite.update"),
        Self::Native("camera.effect"),
        Self::Native("camera.effect.v2"),
        Self::Native("stage.mask.show"),
        Self::Native("stage.mask.hide"),
        Self::Native("sprite.select.when"),
        Self::Native("stage.animate"),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Narration => "Narration",
            Self::Dialogue => "Dialogue",
            Self::Background => "Background",
            Self::Figure => "Figure",
            Self::Choice => "Choice",
            Self::Conditional => "If",
            Self::Loop => "Loop",
            Self::Variable => "Variable",
            Self::Goto => "Goto",
            Self::Call => "Call",
            Self::Wait => "Wait",
            Self::Hide => "Hide",
            Self::Move => "Move",
            Self::CameraMove => "Camera move",
            Self::CameraShake => "Camera shake",
            Self::SpriteFocusRule => "Focus rule",
            Self::SpriteFocus => "Focus speaker",
            Self::Bgm => "BGM",
            Self::Effect => "Sound",
            Self::Video => "Video",
            Self::Return => "Return",
            Self::Native(name) => match name {
                "story.end" => "End story",
                "avatar.show" => "Avatar",
                "avatar.hide" => "Hide avatar",
                "vocal.play" => "Vocal",
                "vocal.stop" => "Stop vocal",
                "screen.film" => "Film bars",
                "text.box" => "Textbox",
                "wait.advance" => "Wait for input",
                "playback.auto" => "Autoplay",
                "ui.show" => "Open UI",
                "ui.hide" => "Close UI",
                "particle.layers.clear" => "Clear particle layers",
                "text.presentation" => "Text presentation",
                "text.retract" => "Retract text",
                "text.float.hide" => "Hide floating text",
                "text.float.configure" => "Floating text lifetime",
                "text.style" => "Dialogue style",
                "scene.parallax.stop" => "Stop parallax",
                "particle.hide" => "Hide particles",
                "video.stop" => "Stop video",
                "gallery.unlock" => "Unlock gallery",
                "input.simple" => "Input",
                "camera.bind" => "Bind camera",
                "camera.unbind" => "Unbind camera",
                "sprite.offset" => "Sprite offset",
                "sprite.transform" => "Sprite transform",
                "background.transform" => "Background transform",
                "sprite.filter" => "Sprite filter",
                "sprite.animate" => "Animate sprite",
                "sprite.transition" => "Sprite transition",
                "se.loop" => "Loop sound",
                "se.stop" => "Stop sound",
                "video.play" => "Play video",
                "screen.curtain.show" => "Show curtain",
                "screen.curtain.hide" => "Hide curtain",
                "text.float" => "Floating text",
                "scene.parallax" => "Scene parallax",
                "particle.show" => "Show particles",
                "ui.message" => "System message",
                "text.intro" => "Intro pages",
                "sprite.sequence" => "Sprite sequence",
                "sprite.sequence.timed" => "Timed sprite sequence",
                "sprite.select" => "Select sprite image",
                "sprite.keyframes" => "Sprite keyframes",
                "assets.loading" => "Prepare assets",
                "input.request" => "Input request",
                "text.paragraph.style" => "Paragraph style",
                "sprite.update" => "Update sprite",
                "camera.effect" => "Camera effect",
                "camera.effect.v2" => "Camera effect V2",
                "stage.mask.show" => "Show stage mask",
                "stage.mask.hide" => "Hide stage mask",
                "sprite.select.when" => "Select sprite by condition",
                "stage.animate" => "Stage animation",
                _ => "Command",
            },
        }
    }

    pub fn for_command(name: &str) -> Option<Self> {
        Some(match name {
            "background" => Self::Background,
            "sprite" => Self::Figure,
            "hide" => Self::Hide,
            "move" => Self::Move,
            "bgm" => Self::Bgm,
            "se" => Self::Effect,
            "video" => Self::Video,
            "goto" => Self::Goto,
            "call" => Self::Call,
            "wait" => Self::Wait,
            "camera.move" => Self::CameraMove,
            "camera.shake" => Self::CameraShake,
            "sprite.focus.configure" => Self::SpriteFocusRule,
            "sprite.focus" => Self::SpriteFocus,
            _ => {
                return Self::ALL
                    .iter()
                    .copied()
                    .find(|kind| matches!(kind, Self::Native(command) if *command == name));
            }
        })
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::Narration | Self::Dialogue => "Text",
            Self::Background
            | Self::Figure
            | Self::Hide
            | Self::Move
            | Self::CameraMove
            | Self::CameraShake
            | Self::SpriteFocusRule
            | Self::SpriteFocus => "Scene",
            Self::Bgm | Self::Effect | Self::Video => "Media",
            Self::Choice
            | Self::Conditional
            | Self::Loop
            | Self::Goto
            | Self::Call
            | Self::Return
            | Self::Wait => "Flow",
            Self::Variable => "Data",
            Self::Native(name) => match name {
                "avatar.show"
                | "avatar.hide"
                | "screen.film"
                | "particle.layers.clear"
                | "scene.parallax.stop"
                | "particle.hide"
                | "camera.bind"
                | "camera.unbind"
                | "sprite.offset"
                | "sprite.transform"
                | "background.transform"
                | "sprite.filter"
                | "sprite.animate"
                | "sprite.transition"
                | "screen.curtain.show"
                | "screen.curtain.hide"
                | "scene.parallax"
                | "particle.show" => "Scene",
                "sprite.sequence"
                | "sprite.sequence.timed"
                | "sprite.select"
                | "sprite.select.when"
                | "stage.animate"
                | "sprite.keyframes"
                | "assets.loading"
                | "sprite.update" => "Scene",
                "camera.effect" | "camera.effect.v2" | "stage.mask.show" | "stage.mask.hide" => {
                    "Scene"
                }
                "vocal.play" | "vocal.stop" | "video.stop" | "se.loop" | "se.stop"
                | "video.play" => "Media",
                "text.box"
                | "text.presentation"
                | "text.retract"
                | "text.float.hide"
                | "text.float.configure"
                | "text.style"
                | "text.float"
                | "text.intro"
                | "text.paragraph.style" => "Text",
                _ => "Flow",
            },
        }
    }

    pub fn search_terms(self) -> &'static str {
        match self {
            Self::Native("text.retract") => "retract text backspace erase tail prefix 退格 回删",
            Self::Narration => "narration text narrator",
            Self::Dialogue => "dialogue speaker character text",
            Self::Background => "background scene image",
            Self::Figure => "figure sprite character show",
            Self::Choice => "choice branch option",
            Self::Conditional => "if conditional branch",
            Self::Loop => "loop repeat",
            Self::Variable => "variable let data",
            Self::Goto => "goto scene jump",
            Self::Call => "call scene",
            Self::Wait => "wait delay time",
            Self::Hide => "hide sprite figure",
            Self::Move => "move sprite figure position",
            Self::CameraMove => "camera move scene characters transform",
            Self::CameraShake => "camera shake scene characters",
            Self::SpriteFocusRule => "sprite focus configure portrait characters",
            Self::SpriteFocus => "sprite focus speaker portrait",
            Self::Bgm => "bgm music audio",
            Self::Effect => "sound effect se audio",
            Self::Video => "video movie",
            Self::Return => "return flow",
            Self::Native(name) => name,
        }
    }
}

pub fn insertion_statement(
    source: &str,
    kind: InsertKind,
    index: &AuthoringIndex,
    statement_indent: &str,
) -> Result<String, AuthoringEditError> {
    let first_character = index.characters.first().map(|entry| entry.id.as_str());
    let first_background = index
        .assets
        .iter()
        .find(|entry| entry.kind == AssetKind::Background)
        .map(|entry| entry.id.as_str());
    let first_figure = index
        .assets
        .iter()
        .find(|entry| entry.kind == AssetKind::Figure)
        .map(|entry| entry.id.as_str());
    let first_scene = index.scenes.first().map(|entry| entry.name.as_str());
    let statement = match kind {
        InsertKind::Narration => "\"New narration\"".to_owned(),
        InsertKind::Dialogue => format!(
            "{}: \"New dialogue\"",
            first_character.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Background => format!(
            "background({})",
            first_background.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Figure => {
            let figure = first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!("sprite({figure}_slot, {figure}, position: center)")
        }
        InsertKind::Choice => {
            let scene = first_scene.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!(
                "choice {{\n{statement_indent}  \"Continue\": goto({scene})\n{statement_indent}}}"
            )
        }
        InsertKind::Conditional => {
            format!("if (true) {{\n{statement_indent}  \"New narration\"\n{statement_indent}}}")
        }
        InsertKind::Loop => "loop { break }".to_owned(),
        InsertKind::Variable => format!("let {} = 0", unique_variable_name(source)),
        InsertKind::Goto => format!(
            "goto({})",
            first_scene.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Call => format!(
            "call({})",
            first_scene.ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Wait => "wait(500ms)".to_owned(),
        InsertKind::Hide => {
            let figure = first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!("hide({figure}_slot)")
        }
        InsertKind::Move => {
            let figure = first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!("move({figure}_slot, center)")
        }
        InsertKind::CameraMove => "camera.move(scene, x: 0, y: 0, duration: 300ms)".to_owned(),
        InsertKind::CameraShake => {
            "camera.shake(scene, amplitude: 8, frequency: 12, duration: 300ms)".to_owned()
        }
        InsertKind::SpriteFocusRule => {
            let figure = first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?;
            format!(
                "sprite.focus.configure(characters: [{figure}_slot], speaking: style(), others: style(brightness: 0.7), narration: style(), duration: 300ms)"
            )
        }
        InsertKind::SpriteFocus => "sprite.focus(none)".to_owned(),
        InsertKind::Bgm => format!(
            "bgm({})",
            index
                .assets
                .iter()
                .find(|entry| entry.kind == AssetKind::Bgm)
                .map(|entry| entry.id.as_str())
                .ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Effect => format!(
            "se({})",
            index
                .assets
                .iter()
                .find(|entry| entry.kind == AssetKind::Effect)
                .map(|entry| entry.id.as_str())
                .ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Video => format!(
            "video({})",
            index
                .assets
                .iter()
                .find(|entry| entry.kind == AssetKind::Video)
                .map(|entry| entry.id.as_str())
                .ok_or(AuthoringEditError::MissingInsertionPoint)?
        ),
        InsertKind::Return => "return".to_owned(),
        InsertKind::Native(name) => match name {
            "story.end" => "story.end()".to_owned(),
            "avatar.show" => format!(
                "avatar.show({})",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "avatar.hide" => "avatar.hide()".to_owned(),
            "vocal.play" => format!(
                "vocal.play({})",
                index
                    .assets
                    .iter()
                    .find(|entry| entry.kind == AssetKind::Voice)
                    .map(|entry| entry.id.as_str())
                    .ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "vocal.stop" => "vocal.stop()".to_owned(),
            "screen.film" => "screen.film(true)".to_owned(),
            "text.box" => "text.box(visible: true, auto: false)".to_owned(),
            "wait.advance" => "wait.advance()".to_owned(),
            "playback.auto" => "playback.auto(true)".to_owned(),
            "ui.show" => "ui.show(save)".to_owned(),
            "ui.hide" => "ui.hide(save)".to_owned(),
            "particle.layers.clear" => "particle.layers.clear()".to_owned(),
            "text.presentation" => "text.presentation(paragraph)".to_owned(),
            "text.retract" => "text.retract(source: \"\", keep: \"\")".to_owned(),
            "text.float.hide" => "text.float.hide()".to_owned(),
            "text.float.configure" => "text.float.configure(infinite: false)".to_owned(),
            "text.style" => "text.style(default)".to_owned(),
            "scene.parallax.stop" => "scene.parallax.stop()".to_owned(),
            "particle.hide" => "particle.hide(*)".to_owned(),
            "video.stop" => "video.stop(*)".to_owned(),
            "gallery.unlock" => format!(
                "gallery.unlock(cg, {}, name: \"Artwork\")",
                first_background.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "input.simple" => format!(
                "input.simple({}, title: \"Name\", button: \"OK\")",
                unique_variable_name(source)
            ),
            "camera.bind" | "camera.unbind" => format!(
                "{name}({}_slot, distance: 1.5)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.offset" => format!(
                "sprite.offset({}_slot, x: 0, y: 0)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.transform" => format!(
                "sprite.transform({}_slot, alpha: 1)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "background.transform" => "background.transform(alpha: 1)".to_owned(),
            "sprite.filter" => format!(
                "sprite.filter({}_slot, brightness: 1)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.animate" => format!(
                "sprite.animate({}_slot, shake, duration: 300ms)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.transition" => format!(
                "sprite.transition({}_slot, enter: enter, duration: 300ms)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "se.loop" => format!(
                "se.loop(ambient, {})",
                index
                    .assets
                    .iter()
                    .find(|entry| entry.kind == AssetKind::Effect)
                    .map(|entry| entry.id.as_str())
                    .ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "se.stop" => "se.stop(*)".to_owned(),
            "video.play" => format!(
                "video.play(cutscene, {})",
                index
                    .assets
                    .iter()
                    .find(|entry| entry.kind == AssetKind::Video)
                    .map(|entry| entry.id.as_str())
                    .ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "screen.curtain.show" => "screen.curtain.show(color: rgba(0, 0, 0, 1), duration: 300ms)".to_owned(),
            "screen.curtain.hide" => "screen.curtain.hide(color: rgba(0, 0, 0, 1), duration: 300ms)".to_owned(),
            "text.float" => "text.float(\"New text\", x: 960, y: 540)".to_owned(),
            "scene.parallax" => "scene.parallax(amplitude_percent: 4, scale: 1.08)".to_owned(),
            "particle.show" => "particle.show(sparkles, sparkles)".to_owned(),
            "ui.message" => "ui.message(alert, title: \"Notice\", message: \"Message\", confirm_text: \"OK\", cancel_text: \"Cancel\")".to_owned(),
            "text.intro" => "text.intro(hold: true) { page(\"New page\") }".to_owned(),
            "sprite.sequence" => format!(
                "sprite.sequence({}_slot, fps: 12, loop: true) {{ frame({}) }}",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.sequence.timed" => format!(
                "sprite.sequence.timed({}_slot, loop: true) {{ frame({}, duration: 120ms) }}",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.select" => format!(
                "sprite.select({}_slot, mood, default: {}) {{ case(\"happy\", {}) }}",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "sprite.keyframes" => format!(
                "sprite.keyframes({}_slot, repeat: 0, blocking: true) {{ frame(x: 0, duration: 300ms) }}",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "assets.loading" => format!(
                "assets.loading(mode: auto, lookahead: 20, blocking: false) {{ resource({}, kind: background) }}",
                first_background.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "input.request" => format!(
                "input.request({}, type: string, title: \"Name\")",
                unique_variable_name(source)
            ),
            "text.paragraph.style" => "text.paragraph.style(literary, typewriter_speed: 0.03)".to_owned(),
            "sprite.update" => format!(
                "sprite.update({}_slot, {}, position: center, scale: 1)",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "camera.effect" => "camera.effect(scene, bloom_intensity: 0.4, duration: 300ms)".to_owned(),
            "camera.effect.v2" => concat!(
                "camera.effect.v2(scene, mirror_shatter_intensity: 0, mirror_shatter_center_x: 0.5, ",
                "mirror_shatter_center_y: 0.5, mirror_shatter_spread: 1, mirror_shatter_seed: 0, ",
                "speed_lines_intensity: 0, speed_lines_radial: true, speed_lines_density: 0.55, ",
                "speed_lines_angle: 0, speed_lines_speed: 0, speed_lines_center_x: 0.5, ",
                "speed_lines_center_y: 0.5, speed_lines_region_ellipse: false, ",
                "speed_lines_region_x: 0.5, speed_lines_region_y: 0.5, ",
                "speed_lines_region_width: 1, speed_lines_region_height: 1, ",
                "speed_lines_region_feather: 0.05)"
            ).to_owned(),
            "stage.mask.show" => "stage.mask.show(overlay, shape: rectangle, opacity: 0.5)".to_owned(),
            "stage.mask.hide" => "stage.mask.hide(overlay)".to_owned(),
            "sprite.select.when" => format!(
                "sprite.select.when({}_slot, default: {}) {{ case(true, {}) }}",
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?,
                first_figure.ok_or(AuthoringEditError::MissingInsertionPoint)?
            ),
            "stage.animate" => "stage.animate(opening, duration: 1s) { track(camera, x) { key(time: 0ms, value: 0), key(time: 1s, value: 20) } }".to_owned(),
            _ => return Err(AuthoringEditError::MissingInsertionPoint),
        },
    };
    Ok(statement)
}

fn unique_variable_name(source: &str) -> String {
    let document = parse_native_document(source);
    let identifiers = document
        .tokens
        .iter()
        .filter(|token| token.kind == NativeTokenKind::Identifier)
        .filter_map(|token| source.get(token.range.clone()))
        .collect::<HashSet<_>>();
    if !identifiers.contains("value") {
        return "value".to_owned();
    }
    (2..)
        .map(|suffix| format!("value_{suffix}"))
        .find(|candidate| !identifiers.contains(candidate.as_str()))
        .expect("an unbounded numeric suffix always yields a unique identifier")
}

// Contextual candidates supplement the top-level insertion palette.
pub(crate) const CONTEXTUAL_COMPLETIONS: &[&str] = &[
    "break",
    "else {\n}",
    "track(camera, x) {\n}",
    "key(time: 0ms, value: 0)",
    "event.camera.shake(time: 0ms, amplitude: 8, duration: 300ms)",
    "event.camera.patch(time: 0ms, x: 0)",
    "event.scene(",
    "event.audio(",
    "event.particle(",
    "frame(",
    "page(\"\")",
    "case(",
    "resource(",
    "layer(",
];

// Base calls include pop, which has no insertion-palette entry.
pub(super) fn is_native_command(name: &str) -> bool {
    matches!(
        name,
        "goto"
            | "call"
            | "wait"
            | "background"
            | "sprite"
            | "hide"
            | "move"
            | "bgm"
            | "se"
            | "video"
            | "pop"
    )
}
