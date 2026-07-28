use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crabgal_core::{
    Action, Anchor, AnimationPreset, BlendMode, Easing, StageEventKind, StageProperty, StageTarget,
    Transition,
};

#[derive(Default)]
struct Coverage {
    actions: BTreeSet<&'static str>,
    animations: BTreeSet<&'static str>,
    transitions: BTreeSet<&'static str>,
    particles: BTreeSet<String>,
    blends: BTreeSet<&'static str>,
    anchors: BTreeSet<&'static str>,
    easings: BTreeSet<&'static str>,
    commands: BTreeSet<&'static str>,
    videos: usize,
    has_intro: bool,
    has_film_mode: bool,
    has_filter: bool,
    has_transition_rule: bool,
    has_mini_avatar: bool,
    has_input: bool,
    has_unlock: bool,
}

#[test]
fn checked_in_showcase_exercises_every_native_effect_family() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/webgal-showcase");
    let mut coverage = Coverage::default();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|value| value.to_str()) != Some("txt") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        record_command_spellings(&source, &mut coverage.commands);
        let report = crabgal_loader::parse_webgal_report(&source);
        assert!(
            report.diagnostics.is_empty(),
            "{}: {:?}",
            path.display(),
            report.diagnostics
        );
        for action in &report.actions {
            record(action, &mut coverage);
        }
    }

    assert_eq!(
        coverage.actions,
        BTreeSet::from([
            "animate",
            "bgm",
            "call-scene",
            "change-scene",
            "comment",
            "effect",
            "end",
            "film-mode",
            "flow",
            "hide-bg",
            "hide-mini-avatar",
            "hide-particles",
            "hide-sprite",
            "intro",
            "jump",
            "label",
            "menu",
            "mini-avatar",
            "play-video",
            "say",
            "set",
            "set-filter",
            "set-textbox",
            "set-transform",
            "set-transition",
            "show-bg",
            "show-particles",
            "show-sprite",
            "stop-video",
            "unlock",
            "user-input",
            "wait",
        ])
    );
    assert_eq!(
        coverage.commands,
        BTreeSet::from([
            "bgm",
            "callScene",
            "changeBg",
            "changeFigure",
            "changeScene",
            "choose",
            "comment",
            "end",
            "filmMode",
            "getUserInput",
            "intro",
            "jumpLabel",
            "label",
            "miniAvatar",
            "pixiInit",
            "pixiPerform",
            "playEffect",
            "playVideo",
            "say",
            "setAnimation",
            "setComplexAnimation",
            "setFilter",
            "setTempAnimation",
            "setTextbox",
            "setTransform",
            "setTransition",
            "setVar",
            "stopBgm",
            "unlockBgm",
            "unlockCg",
            "wait",
        ])
    );
    assert_eq!(
        coverage.animations,
        BTreeSet::from([
            "blur",
            "dot-film",
            "enter",
            "enter-bottom",
            "enter-left",
            "enter-right",
            "exit",
            "glitch-film",
            "godray-film",
            "move-front-back",
            "old-film",
            "reflection-film",
            "remove-film",
            "rgb-film",
            "shake",
            "shockwave-in",
            "shockwave-out",
        ])
    );
    assert_eq!(
        coverage.transitions,
        BTreeSet::from([
            "crossfade",
            "dissolve",
            "fade",
            "instant",
            "slide-left",
            "slide-right",
            "wipe",
        ])
    );
    assert_eq!(
        coverage.particles,
        BTreeSet::from([
            "FALLEN_LEAVES".into(),
            "FIREFLY".into(),
            "HEAVY_RAIN".into(),
            "HEAVY_SNOW".into(),
            "LIGHT_RAIN".into(),
            "LIGHT_SNOW".into(),
            "MODERATE_RAIN".into(),
            "MODERATE_SNOW".into(),
        ])
    );
    assert_eq!(
        coverage.blends,
        BTreeSet::from(["add", "alpha", "multiply", "screen"])
    );
    assert_eq!(
        coverage.anchors,
        BTreeSet::from(["center", "left", "right"])
    );
    assert_eq!(
        coverage.easings,
        BTreeSet::from(["ease-in", "ease-in-out", "ease-out", "linear"])
    );
    assert!(coverage.videos >= 3);
    assert!(coverage.has_intro && coverage.has_film_mode);
    assert!(coverage.has_filter && coverage.has_transition_rule);
    assert!(coverage.has_mini_avatar && coverage.has_input && coverage.has_unlock);
}

#[test]
fn letsgal_1_9_showcase_exercises_every_timeline_property_and_event() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("projects/test-project");
    let project = crabgal_loader::LoaderRegistry::default()
        .open_project(&root)
        .expect("the LetsGal showcase should be a valid project")
        .expect("the LetsGal adapter should detect its showcase");
    assert_eq!(project.format, "letsgal");

    let scenes = crabgal_loader::load_scenes(&project.content)
        .expect("the LetsGal showcase should compile through the public loader");
    let mut properties = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut events = BTreeSet::new();
    let mut animations = 0;
    let mut retractions = 0;
    let mut has_muted_track = false;
    let mut has_repeated_fast_blocking_clip = false;
    for scene in scenes {
        assert!(
            scene.diagnostics.is_empty(),
            "{}: {:?}",
            scene.path.display(),
            scene.diagnostics
        );
        for action in scene.actions {
            match action {
                Action::RetractDialogue { .. } => retractions += 1,
                Action::StageAnimation { animation } => {
                    animations += 1;
                    has_repeated_fast_blocking_clip |= animation.repeat == 1
                        && (animation.playback_rate - 1.5).abs() <= f32::EPSILON
                        && animation.blocking;
                    for track in animation.tracks {
                        properties.insert(stage_property_name(track.property));
                        targets.insert(match track.target {
                            StageTarget::Camera => "camera",
                            StageTarget::Character { .. } => "character",
                            StageTarget::SceneLayer { .. } => "scene-layer",
                        });
                        has_muted_track |= track.muted;
                    }
                    for event in animation.events {
                        events.insert(match event.kind {
                            StageEventKind::CameraShake(_) => "camera-shake",
                            StageEventKind::CameraPatch { .. } => "camera-patch",
                            StageEventKind::Particle { .. } => "particle",
                            StageEventKind::Scene(_) => "scene",
                            StageEventKind::Audio(_) => "audio",
                        });
                    }
                }
                _ => {}
            }
        }
    }

    assert_eq!(animations, 8);
    assert_eq!(retractions, 2);
    assert_eq!(
        targets,
        BTreeSet::from(["camera", "character", "scene-layer"])
    );
    assert_eq!(
        events,
        BTreeSet::from(["audio", "camera-patch", "camera-shake", "particle", "scene",])
    );
    assert_eq!(
        properties,
        BTreeSet::from([
            "alpha",
            "bloom-intensity",
            "blur-amount",
            "blur-strength",
            "chromatic-aberration",
            "color-brightness",
            "color-contrast",
            "color-exposure",
            "color-saturation",
            "color-temperature",
            "color-tone-intensity",
            "crt-intensity",
            "dither-intensity",
            "dither-levels",
            "distortion-strength",
            "eyelid-center-x",
            "eyelid-center-y",
            "eyelid-curvature",
            "eyelid-openness",
            "eyelid-softness",
            "eyelid-width",
            "film-grain-intensity",
            "film-grain-size",
            "focal-distance",
            "fog-intensity",
            "fog-scale",
            "fog-speed",
            "glitch-intensity",
            "godray-angle",
            "godray-center-x",
            "godray-center-y",
            "godray-gain",
            "godray-intensity",
            "godray-lacunarity",
            "godray-speed",
            "halftone-angle",
            "halftone-intensity",
            "halftone-scale",
            "heat-haze-intensity",
            "heat-haze-scale",
            "heat-haze-speed",
            "height",
            "lens-flare-center-x",
            "lens-flare-center-y",
            "lens-flare-intensity",
            "light-leak-angle",
            "light-leak-intensity",
            "lut-intensity",
            "motion-blur-angle",
            "motion-blur-strength",
            "old-film-intensity",
            "outline-intensity",
            "outline-thickness",
            "pixelate-size",
            "radial-blur-center-x",
            "radial-blur-center-y",
            "radial-blur-strength",
            "rotation",
            "scale-x",
            "scale-y",
            "sharpen-strength",
            "shock-intensity",
            "vhs-intensity",
            "vhs-jitter",
            "vhs-noise",
            "vignette-intensity",
            "vignette-size",
            "water-ripple-center-x",
            "water-ripple-center-y",
            "water-ripple-frequency",
            "water-ripple-intensity",
            "water-ripple-speed",
            "width",
            "x",
            "y",
            "zoom",
            "zoom-blur-center-x",
            "zoom-blur-center-y",
            "zoom-blur-strength",
        ])
    );
    assert!(has_muted_track);
    assert!(has_repeated_fast_blocking_clip);
}

fn record(action: &Action, coverage: &mut Coverage) {
    coverage.actions.insert(action_name(action));
    if let Action::Flow { action, .. } = action {
        record(action, coverage);
        return;
    }
    match action {
        Action::Animate { preset, .. } => {
            coverage.animations.insert(animation_name(preset));
        }
        Action::SetTransition { enter, exit, .. } => {
            coverage.has_transition_rule = true;
            for preset in enter.iter().chain(exit) {
                coverage.animations.insert(animation_name(preset));
            }
        }
        Action::ShowBg { transition, .. }
        | Action::HideBg { transition }
        | Action::HideSprite { transition, .. } => {
            coverage.transitions.insert(transition_name(*transition));
        }
        Action::ShowSprite {
            position,
            transition,
            blend,
            ..
        } => {
            coverage.transitions.insert(transition_name(*transition));
            coverage.blends.insert(blend_name(*blend));
            coverage.anchors.insert(match position.x {
                Anchor::Left(_) => "left",
                Anchor::Right(_) => "right",
                Anchor::Center(_) => "center",
            });
        }
        Action::SetTransform { easing, .. } => {
            coverage.easings.insert(easing_name(*easing));
        }
        Action::ShowParticles { effect, .. } => {
            coverage.particles.insert(effect.preset.clone());
        }
        Action::PlayVideo { .. } => coverage.videos += 1,
        Action::Intro { .. } => coverage.has_intro = true,
        Action::FilmMode { .. } => coverage.has_film_mode = true,
        Action::SetFilter { .. } => coverage.has_filter = true,
        Action::MiniAvatar { .. } => coverage.has_mini_avatar = true,
        Action::UserInput { .. } | Action::RequestInput { .. } => coverage.has_input = true,
        Action::Unlock { .. } => coverage.has_unlock = true,
        _ => {}
    }
}

fn record_command_spellings(source: &str, commands: &mut BTreeSet<&'static str>) {
    const COMMANDS: &[&str] = &[
        "bgm",
        "callScene",
        "changeBg",
        "changeFigure",
        "changeScene",
        "choose",
        "comment",
        "end",
        "filmMode",
        "getUserInput",
        "intro",
        "jumpLabel",
        "label",
        "miniAvatar",
        "pixiInit",
        "pixiPerform",
        "playEffect",
        "playVideo",
        "say",
        "setAnimation",
        "setComplexAnimation",
        "setFilter",
        "setTempAnimation",
        "setTextbox",
        "setTransform",
        "setTransition",
        "setVar",
        "stopBgm",
        "unlockBgm",
        "unlockCg",
        "wait",
    ];
    for line in source.lines().map(str::trim) {
        for command in COMMANDS {
            if line == *command
                || line.starts_with(&format!("{command}:"))
                || line.starts_with(&format!("{command} "))
            {
                commands.insert(command);
            }
        }
    }
}

/// This exhaustive match intentionally turns every new native Action variant
/// into a compile error here until its showcase ownership is decided.
fn action_name(action: &Action) -> &'static str {
    match action {
        Action::ShowBg { .. } => "show-bg",
        Action::HideBg { .. } => "hide-bg",
        Action::ShowSprite { .. } => "show-sprite",
        Action::HideSprite { .. } => "hide-sprite",
        Action::Say { .. } => "say",
        Action::Menu { .. } => "menu",
        Action::Jump(_) => "jump",
        Action::Label(_) => "label",
        Action::ChangeScene(_) => "change-scene",
        Action::CallScene(_) => "call-scene",
        Action::End => "end",
        Action::Bgm { .. } => "bgm",
        Action::Effect { .. } => "effect",
        Action::MiniAvatar { .. } => "mini-avatar",
        Action::HideMiniAvatar => "hide-mini-avatar",
        Action::Set { .. } => "set",
        Action::Flow { .. } => "flow",
        Action::SetTransform { .. } => "set-transform",
        Action::Animate { .. } => "animate",
        Action::SetTransition { .. } => "set-transition",
        Action::SetFilter { .. } => "set-filter",
        Action::Wait { .. } => "wait",
        Action::Intro { .. } => "intro",
        Action::FilmMode { .. } => "film-mode",
        Action::ShowParticles { .. } => "show-particles",
        Action::HideParticles { .. } => "hide-particles",
        Action::SetTextbox { .. } => "set-textbox",
        Action::UserInput { .. } => "user-input",
        Action::Comment => "comment",
        Action::Unlock { .. } => "unlock",
        Action::Curtain { .. } => "curtain",
        Action::FloatingText { .. } => "floating-text",
        Action::ConfigurePortraits { .. } => "configure-portraits",
        Action::FocusPortrait { .. } => "focus-portrait",
        Action::SetDialogueStyle { .. } => "set-dialogue-style",
        Action::AnimateKeyframes { .. } => "animate-keyframes",
        Action::HideSprites { .. } => "hide-sprites",
        Action::SetAutoplay { .. } => "set-autoplay",
        Action::SetSystemUi { .. } => "set-system-ui",
        Action::PlayVideo { .. } => "play-video",
        Action::StopVideo { .. } => "stop-video",
        Action::SetPostProcess { .. } => "set-post-process",
        Action::SetCameraBinding { .. } => "set-camera-binding",
        Action::SetCameraTransform { .. } => "set-camera-transform",
        Action::ShakeCamera { .. } => "shake-camera",
        Action::HostCommand { .. } => "host-command",
        Action::Vocal { .. } => "vocal",
        Action::RequestInput { .. } => "request-input",
        // LetsGal owns this adapter-native fixture; the WebGAL showcase
        // intentionally does not need to manufacture Studio timeline JSON.
        Action::StageAnimation { .. } => "stage-animation",
        Action::RetractDialogue { .. } => "retract-dialogue",
        Action::WaitForAdvance => "wait-for-advance",
        Action::SelectSpriteImage { .. } => "select-sprite-image",
    }
}

fn easing_name(easing: Easing) -> &'static str {
    match easing {
        Easing::Linear => "linear",
        Easing::EaseIn => "ease-in",
        Easing::EaseOut => "ease-out",
        Easing::EaseInOut => "ease-in-out",
    }
}

fn animation_name(preset: &AnimationPreset) -> &'static str {
    match preset {
        AnimationPreset::Enter => "enter",
        AnimationPreset::Exit => "exit",
        AnimationPreset::Shake => "shake",
        AnimationPreset::EnterFromBottom => "enter-bottom",
        AnimationPreset::EnterFromLeft => "enter-left",
        AnimationPreset::EnterFromRight => "enter-right",
        AnimationPreset::MoveFrontAndBack => "move-front-back",
        AnimationPreset::Blur => "blur",
        AnimationPreset::OldFilm => "old-film",
        AnimationPreset::DotFilm => "dot-film",
        AnimationPreset::ReflectionFilm => "reflection-film",
        AnimationPreset::GlitchFilm => "glitch-film",
        AnimationPreset::RgbFilm => "rgb-film",
        AnimationPreset::GodrayFilm => "godray-film",
        AnimationPreset::RemoveFilm => "remove-film",
        AnimationPreset::ShockwaveIn => "shockwave-in",
        AnimationPreset::ShockwaveOut => "shockwave-out",
        AnimationPreset::Custom(_) => "custom",
    }
}

fn transition_name(transition: Transition) -> &'static str {
    match transition {
        Transition::Instant => "instant",
        Transition::Fade(_) => "fade",
        Transition::SlideFromLeft(_) => "slide-left",
        Transition::SlideFromRight(_) => "slide-right",
        Transition::Crossfade(_) => "crossfade",
        Transition::Wipe(_) => "wipe",
        Transition::Dissolve(_) => "dissolve",
    }
}

fn blend_name(blend: BlendMode) -> &'static str {
    match blend {
        BlendMode::Alpha => "alpha",
        BlendMode::Add => "add",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
    }
}

/// Exhaustive on purpose: adding a native timeline property requires adding
/// a visible fixture track and an acceptance expectation in the same change.
fn stage_property_name(property: StageProperty) -> &'static str {
    match property {
        StageProperty::X => "x",
        StageProperty::Y => "y",
        StageProperty::Zoom => "zoom",
        StageProperty::ScaleX => "scale-x",
        StageProperty::ScaleY => "scale-y",
        StageProperty::Alpha => "alpha",
        StageProperty::Rotation => "rotation",
        StageProperty::Width => "width",
        StageProperty::Height => "height",
        StageProperty::FocalDistance => "focal-distance",
        StageProperty::BlurStrength => "blur-strength",
        StageProperty::DistortionStrength => "distortion-strength",
        StageProperty::VignetteIntensity => "vignette-intensity",
        StageProperty::VignetteSize => "vignette-size",
        StageProperty::BlurAmount => "blur-amount",
        StageProperty::ColorToneIntensity => "color-tone-intensity",
        StageProperty::ColorExposure => "color-exposure",
        StageProperty::ColorBrightness => "color-brightness",
        StageProperty::ColorContrast => "color-contrast",
        StageProperty::ColorSaturation => "color-saturation",
        StageProperty::ColorTemperature => "color-temperature",
        StageProperty::OldFilmIntensity => "old-film-intensity",
        StageProperty::ShockIntensity => "shock-intensity",
        StageProperty::GodrayIntensity => "godray-intensity",
        StageProperty::GodrayAngle => "godray-angle",
        StageProperty::GodrayGain => "godray-gain",
        StageProperty::GodrayLacunarity => "godray-lacunarity",
        StageProperty::GodraySpeed => "godray-speed",
        StageProperty::GodrayCenterX => "godray-center-x",
        StageProperty::GodrayCenterY => "godray-center-y",
        StageProperty::LutIntensity => "lut-intensity",
        StageProperty::BloomIntensity => "bloom-intensity",
        StageProperty::ChromaticAberration => "chromatic-aberration",
        StageProperty::PixelateSize => "pixelate-size",
        StageProperty::GlitchIntensity => "glitch-intensity",
        StageProperty::CrtIntensity => "crt-intensity",
        StageProperty::SharpenStrength => "sharpen-strength",
        StageProperty::RadialBlurStrength => "radial-blur-strength",
        StageProperty::RadialBlurCenterX => "radial-blur-center-x",
        StageProperty::RadialBlurCenterY => "radial-blur-center-y",
        StageProperty::MotionBlurStrength => "motion-blur-strength",
        StageProperty::MotionBlurAngle => "motion-blur-angle",
        StageProperty::ZoomBlurStrength => "zoom-blur-strength",
        StageProperty::ZoomBlurCenterX => "zoom-blur-center-x",
        StageProperty::ZoomBlurCenterY => "zoom-blur-center-y",
        StageProperty::LightLeakIntensity => "light-leak-intensity",
        StageProperty::LightLeakAngle => "light-leak-angle",
        StageProperty::LensFlareIntensity => "lens-flare-intensity",
        StageProperty::LensFlareCenterX => "lens-flare-center-x",
        StageProperty::LensFlareCenterY => "lens-flare-center-y",
        StageProperty::FilmGrainIntensity => "film-grain-intensity",
        StageProperty::FilmGrainSize => "film-grain-size",
        StageProperty::HeatHazeIntensity => "heat-haze-intensity",
        StageProperty::HeatHazeSpeed => "heat-haze-speed",
        StageProperty::HeatHazeScale => "heat-haze-scale",
        StageProperty::WaterRippleIntensity => "water-ripple-intensity",
        StageProperty::WaterRippleFrequency => "water-ripple-frequency",
        StageProperty::WaterRippleSpeed => "water-ripple-speed",
        StageProperty::WaterRippleCenterX => "water-ripple-center-x",
        StageProperty::WaterRippleCenterY => "water-ripple-center-y",
        StageProperty::FogIntensity => "fog-intensity",
        StageProperty::FogSpeed => "fog-speed",
        StageProperty::FogScale => "fog-scale",
        StageProperty::VhsIntensity => "vhs-intensity",
        StageProperty::VhsJitter => "vhs-jitter",
        StageProperty::VhsNoise => "vhs-noise",
        StageProperty::HalftoneIntensity => "halftone-intensity",
        StageProperty::HalftoneScale => "halftone-scale",
        StageProperty::HalftoneAngle => "halftone-angle",
        StageProperty::DitherIntensity => "dither-intensity",
        StageProperty::DitherLevels => "dither-levels",
        StageProperty::OutlineIntensity => "outline-intensity",
        StageProperty::OutlineThickness => "outline-thickness",
        StageProperty::EyelidOpenness => "eyelid-openness",
        StageProperty::EyelidWidth => "eyelid-width",
        StageProperty::EyelidCurvature => "eyelid-curvature",
        StageProperty::EyelidSoftness => "eyelid-softness",
        StageProperty::EyelidCenterX => "eyelid-center-x",
        StageProperty::EyelidCenterY => "eyelid-center-y",
    }
}
