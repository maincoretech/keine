// Action enum: the script DSL compiles into a Vec<Action>.
//
// Inspired by:
//   WebGAL's command DSL (simplicity)
//   Siglus's command dispatch (structure)

use std::collections::HashMap;
use std::io::{self, Write};

use serde::{Deserialize, Serialize};

use crate::types::{
    AnimationPreset, BlendMode, CameraShakeSpec, CameraTargets, DialogueStyle, Easing,
    ParticleEffect, PortraitStyle, Position, PostProcessPatch, SceneLayerLayout, SpriteLayout,
    SpriteTransform, TransformPatch, Transition, VideoSpec, VisualFilter,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SayOptions {
    pub vocal: Option<String>,
    pub volume: f32,
    pub concat: bool,
    pub auto_advance: bool,
    pub inherit_speaker: bool,
}

impl Default for SayOptions {
    fn default() -> Self {
        Self {
            vocal: None,
            volume: 1.0,
            concat: false,
            auto_advance: false,
            inherit_speaker: false,
        }
    }
}

/// One absolute-target segment in an adapter-authored sprite timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformKeyframe {
    pub transform: TransformPatch,
    pub duration: f32,
    pub easing: Easing,
}

/// One normalized keyframe in an adapter-neutral shared stage timeline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StageKeyframe {
    pub time: f32,
    pub value: f32,
    pub easing: Easing,
}

/// Render object addressed by a shared stage timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StageTarget {
    Camera,
    Character {
        id: String,
        /// Lets an editor timeline prepare an expression that is not already
        /// present on stage without leaking the editor's character model.
        image: Option<String>,
    },
    SceneLayer {
        id: String,
    },
}

/// Numeric properties supported by the engine's shared stage clock.
///
/// The camera list mirrors the native camera state rather than retaining an
/// editor-owned string. That keeps sampling allocation-free and ensures an
/// adapter cannot claim a property which the runtime silently discards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StageProperty {
    X,
    Y,
    Zoom,
    ScaleX,
    ScaleY,
    Alpha,
    Rotation,
    Width,
    Height,
    FocalDistance,
    BlurStrength,
    DistortionStrength,
    VignetteIntensity,
    VignetteSize,
    BlurAmount,
    ColorToneIntensity,
    ColorExposure,
    ColorBrightness,
    ColorContrast,
    ColorSaturation,
    ColorTemperature,
    OldFilmIntensity,
    ShockIntensity,
    GodrayIntensity,
    GodrayAngle,
    GodrayGain,
    GodrayLacunarity,
    GodraySpeed,
    GodrayCenterX,
    GodrayCenterY,
    LutIntensity,
    BloomIntensity,
    ChromaticAberration,
    PixelateSize,
    GlitchIntensity,
    CrtIntensity,
    SharpenStrength,
    RadialBlurStrength,
    RadialBlurCenterX,
    RadialBlurCenterY,
    MotionBlurStrength,
    MotionBlurAngle,
    ZoomBlurStrength,
    ZoomBlurCenterX,
    ZoomBlurCenterY,
    LightLeakIntensity,
    LightLeakAngle,
    LensFlareIntensity,
    LensFlareCenterX,
    LensFlareCenterY,
    FilmGrainIntensity,
    FilmGrainSize,
    HeatHazeIntensity,
    HeatHazeSpeed,
    HeatHazeScale,
    WaterRippleIntensity,
    WaterRippleFrequency,
    WaterRippleSpeed,
    WaterRippleCenterX,
    WaterRippleCenterY,
    FogIntensity,
    FogSpeed,
    FogScale,
    VhsIntensity,
    VhsJitter,
    VhsNoise,
    HalftoneIntensity,
    HalftoneScale,
    HalftoneAngle,
    DitherIntensity,
    DitherLevels,
    OutlineIntensity,
    OutlineThickness,
    EyelidOpenness,
    EyelidWidth,
    EyelidCurvature,
    EyelidSoftness,
    EyelidCenterX,
    EyelidCenterY,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageTrack {
    pub target: StageTarget,
    pub property: StageProperty,
    pub keyframes: Vec<StageKeyframe>,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageSceneLayer {
    pub id: String,
    pub image: String,
    pub distance: f32,
    pub offset: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageSceneCue {
    pub scene_id: String,
    pub transition: Transition,
    pub reset_camera: bool,
    pub layout: SceneLayerLayout,
    pub layers: Vec<StageSceneLayer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StageAudioKind {
    Bgm,
    Effect,
    Vocal,
}

/// Adapter-neutral audio event driven by the shared stage clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageAudioCue {
    pub id: String,
    pub kind: StageAudioKind,
    pub file: String,
    pub volume: f32,
    pub looped: bool,
    pub duration: f32,
    pub fade_in: f32,
    pub fade_out: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StageEventKind {
    CameraShake(CameraShakeSpec),
    CameraPatch {
        targets: Option<CameraTargets>,
        effect: Box<PostProcessPatch>,
    },
    Particle {
        id: String,
        effect: ParticleEffect,
        duration: f32,
        fade_out: f32,
    },
    Scene(StageSceneCue),
    Audio(StageAudioCue),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageEvent {
    pub time: f32,
    pub kind: StageEventKind,
}

/// A shared-clock stage animation. `repeat` counts additional plays, matching
/// the common editor convention where zero means play once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageAnimation {
    pub id: String,
    pub duration: f32,
    pub tracks: Vec<StageTrack>,
    pub events: Vec<StageEvent>,
    pub repeat: u32,
    pub infinite: bool,
    pub playback_rate: f32,
    pub blocking: bool,
}

/// A single script action — the entire script language compiles to this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    // ── Display ──
    /// Set the background image.
    ShowBg {
        image: String,
        transition: Transition,
        transform: SpriteTransform,
    },
    /// Remove the current background.
    HideBg {
        transition: Transition,
    },
    /// Show a sprite (character / effect).
    ShowSprite {
        id: String,
        image: String,
        position: Position,
        #[serde(default)]
        layout: SpriteLayout,
        transition: Transition,
        transform: SpriteTransform,
        z_index: i32,
        blend: BlendMode,
    },
    /// Remove a sprite.
    HideSprite {
        id: String,
        transition: Transition,
    },
    // ── Dialogue ──
    /// Display dialogue text (triggers click-to-continue).
    Say {
        speaker: String,
        text: String,
        options: SayOptions,
    },

    // ── Choice ──
    /// Show a choice menu (triggers click-to-choose).
    Menu {
        prompt: String,
        choices: Vec<Choice>,
    },

    // ── Control flow ──
    /// Jump to a named label.
    Jump(String),
    /// Define a label (no-op in execution, used for jump resolution).
    Label(String),
    /// Replace the current scene without adding a return point.
    ChangeScene(String),
    /// Enter a scene and return to the following action when it finishes.
    CallScene(String),
    /// End the current game flow without returning through the scene stack.
    End,

    // ── Audio ──
    /// Play background music.
    Bgm {
        file: String,
        volume: f32,
        fade_seconds: f32,
    },
    /// Play, replace, or stop a sound effect. An id makes the effect loop.
    Effect {
        file: Option<String>,
        volume: f32,
        id: Option<String>,
    },

    // ── UI ──
    /// Show mini avatar beside the text box.
    MiniAvatar {
        image: String,
    },
    /// Hide the mini avatar.
    HideMiniAvatar,
    /// Evaluate an expression and set a local or persistent variable.
    Set {
        name: String,
        expression: String,
        global: bool,
    },

    /// Common WebGAL flow parameters applied to any command.
    Flow {
        action: Box<Action>,
        when: Option<String>,
        next: bool,
    },

    // ── Transform ──
    /// Modify an existing sprite's transform (position offset, alpha, scale, etc).
    SetTransform {
        /// Target sprite id (e.g. "fig-center", "fig-left", or custom id).
        id: String,
        /// Sparse transform fields to apply; absent fields inherit current state.
        transform: TransformPatch,
        duration: f32,
        easing: Easing,
    },

    // ── Presentation ──
    /// Play a built-in or adapter-defined animation on a stage target.
    Animate {
        target: String,
        preset: AnimationPreset,
        duration: f32,
    },
    /// Configure the enter/exit animation used by later image changes.
    SetTransition {
        target: String,
        enter: Option<AnimationPreset>,
        exit: Option<AnimationPreset>,
        duration: f32,
    },
    /// Apply image-local filtering without changing its logical transform.
    SetFilter {
        target: String,
        filter: VisualFilter,
    },
    /// Block script execution for a real-time duration.
    Wait {
        seconds: f32,
    },
    /// Fullscreen black narration pages.
    Intro {
        pages: Vec<String>,
        hold: bool,
    },
    /// Toggle cinematic letterbox bars.
    FilmMode {
        enabled: bool,
    },
    /// Create or replace a named particle emitter.
    ShowParticles {
        id: String,
        effect: crate::types::ParticleEffect,
    },
    /// Fade and remove one emitter, or all emitters when `id` is absent.
    HideParticles {
        id: Option<String>,
        duration: f32,
    },

    // ── Text and interaction ──
    SetTextbox {
        visible: bool,
        auto: bool,
    },
    UserInput {
        variable: String,
        title: String,
        button: String,
    },
    Comment,
    Unlock {
        kind: crate::types::UnlockKind,
        file: String,
        name: String,
    },

    /// Fade a solid full-screen curtain without coupling core to a UI backend.
    /// New variants stay at the tail so existing serialized action tags and
    /// program fingerprints remain stable.
    Curtain {
        visible: bool,
        color: [f32; 4],
        duration: f32,
    },
    /// Timed overlay text authored by an editor adapter.
    FloatingText {
        text: String,
        position: [f32; 2],
        font_size: f32,
        color: [f32; 4],
        fade_in: f32,
        hold: f32,
        fade_out: f32,
        blocking: bool,
    },
    ConfigurePortraits {
        enabled: bool,
        character_ids: Vec<String>,
        speaking: PortraitStyle,
        others: PortraitStyle,
        narration: PortraitStyle,
        duration: f32,
        easing: Easing,
    },
    FocusPortrait {
        speaker_id: Option<String>,
    },
    /// Select an adapter-authored dialogue presentation without coupling core
    /// to a concrete UI toolkit.
    SetDialogueStyle {
        style: DialogueStyle,
    },
    /// Timeline animation used by structured editor adapters.
    AnimateKeyframes {
        target: String,
        frames: Vec<TransformKeyframe>,
        repeat: u32,
        blocking: bool,
    },
    /// Hide every sprite whose stable id starts with `prefix`.
    /// Structured adapters use this to replace composed scene layers without
    /// expanding one source command into dozens of cleanup actions. Keep new
    /// variants appended so existing action fingerprints remain stable.
    HideSprites {
        prefix: String,
        transition: Transition,
    },
    /// Enable or disable the engine's native autoplay state.
    SetAutoplay {
        enabled: bool,
    },
    /// Open or close one engine-owned system surface.
    SetSystemUi {
        slot: SystemUiSlot,
        visible: bool,
    },
    PlayVideo {
        video: VideoSpec,
    },
    StopVideo {
        id: Option<String>,
        fade_out: f32,
    },
    SetPostProcess {
        targets: CameraTargets,
        effect: Box<PostProcessPatch>,
        duration: f32,
        easing: Easing,
        blocking: bool,
    },
    /// Bind a render object to the adapter-neutral camera depth model.
    SetCameraBinding {
        target: String,
        bound: bool,
        distance: f32,
    },
    /// Move the logical VN camera without baking camera motion into objects.
    SetCameraTransform {
        targets: CameraTargets,
        transform: TransformPatch,
        duration: f32,
        easing: Easing,
        blocking: bool,
    },
    ShakeCamera {
        targets: CameraTargets,
        shake: CameraShakeSpec,
        blocking: bool,
    },
    /// Opaque call into a third-party extension plugin.
    ///
    /// Built-in adapter semantics must never use this escape hatch: they need
    /// a typed action and a native runtime consumer so the engine remains a
    /// strict superset of every built-in adapter's emitted IR.
    HostCommand {
        namespace: String,
        command: String,
        payload: String,
    },
    /// Play or stop a standalone voice clip outside a dialogue action.
    Vocal {
        file: Option<String>,
        volume: f32,
    },
    /// Rich input request emitted by structured editor adapters.
    RequestInput {
        spec: crate::types::UserInputSpec,
    },
    /// Drive camera, characters, scene layers and timed effects from one
    /// frame-rate-independent clock.
    StageAnimation {
        animation: StageAnimation,
    },
    /// Retract the currently displayed dialogue from `source` to its `keep`
    /// prefix, then wait for one explicit advance input. An empty `source`
    /// resolves to the current dialogue for legacy authored content.
    ///
    /// This is adapter-neutral runtime IR. Editor adapters may lower their
    /// own sentence-tail deletion features to it without requiring an editor
    /// extension at runtime.
    RetractDialogue {
        source: String,
        keep: String,
    },
    /// Wait until the player explicitly requests the next presentation step.
    WaitForAdvance,
    /// Resolve a sprite image from a runtime variable.
    SelectSpriteImage {
        id: String,
        variable: String,
        default_image: String,
        variants: Vec<(String, String)>,
    },
}

/// Fixed system surfaces owned by the engine shell, never by a script adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SystemUiSlot {
    Title,
    Save,
    Load,
    Settings,
    History,
    Gallery,
    Input,
}

/// A single choice in a menu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub text: String,
    pub target: ChoiceTarget,
    pub show_when: Option<String>,
    pub enable_when: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChoiceTarget {
    Label(String),
    ChangeScene(String),
    CallScene(String),
}

/// Immutable, adapter-neutral script program shared by every runtime snapshot.
///
/// Actions are packed once and labels are indexed during construction. Runtime
/// state keeps this behind an `Arc`, so cloning a snapshot is O(1) with respect
/// to script size and persisted saves never duplicate project scripts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    scenes: HashMap<String, Scene>,
    fingerprint: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct Scene {
    actions: Box<[Action]>,
    labels: HashMap<String, usize>,
}

impl Program {
    pub fn from_scenes(scenes: impl IntoIterator<Item = (String, Vec<Action>)>) -> Self {
        let scenes = scenes
            .into_iter()
            .map(|(name, actions)| (name, Scene::new(actions)))
            .collect();
        let fingerprint = program_fingerprint(&scenes);
        Self {
            scenes,
            fingerprint,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.scenes.is_empty()
    }

    pub fn insert_scene(&mut self, name: String, actions: Vec<Action>) {
        self.scenes.insert(name, Scene::new(actions));
        self.fingerprint = program_fingerprint(&self.scenes);
    }

    pub fn scene_count(&self) -> usize {
        self.scenes.len()
    }

    pub fn action_count(&self) -> usize {
        self.scenes.values().map(|scene| scene.actions.len()).sum()
    }

    /// Stable content identity used to reject saves and rollback checkpoints
    /// compiled against a different script layout.
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    pub fn contains_scene(&self, name: &str) -> bool {
        self.scenes.contains_key(name)
    }

    pub fn scene(&self, name: &str) -> Option<&[Action]> {
        self.scenes.get(name).map(|scene| scene.actions.as_ref())
    }

    pub fn scene_len(&self, name: &str) -> Option<usize> {
        self.scenes.get(name).map(|scene| scene.actions.len())
    }

    pub fn label(&self, scene: &str, label: &str) -> Option<usize> {
        self.scenes
            .get(scene)
            .and_then(|scene| scene.labels.get(label))
            .copied()
    }

    pub fn scene_names(&self) -> impl Iterator<Item = &str> {
        self.scenes.keys().map(String::as_str)
    }
}

impl Scene {
    fn new(actions: Vec<Action>) -> Self {
        let labels = actions
            .iter()
            .enumerate()
            .filter_map(|(index, action)| match action {
                Action::Label(name) => Some((name.clone(), index)),
                _ => None,
            })
            .collect();
        Self {
            actions: actions.into_boxed_slice(),
            labels,
        }
    }
}

/// FNV-1a is intentionally small and deterministic. This is a compatibility
/// identity, not a cryptographic signature; the save payload has its own CRC.
fn program_fingerprint(scenes: &HashMap<String, Scene>) -> u64 {
    if scenes.is_empty() {
        return 0;
    }

    let mut names = scenes.keys().collect::<Vec<_>>();
    names.sort_unstable();
    let mut writer = Fnv64::default();
    for name in names {
        let scene = &scenes[name];
        postcard::to_io(&(name.as_str(), scene.actions.as_ref()), &mut writer)
            .expect("serializing typed actions into a fingerprint cannot fail");
    }
    writer.finish()
}

struct Fnv64(u64);

impl Default for Fnv64 {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv64 {
    fn finish(self) -> u64 {
        self.0
    }
}

impl Write for Fnv64 {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod program_tests {
    use super::*;

    #[test]
    fn packs_scenes_and_indexes_labels_once() {
        let program = Program::from_scenes([(
            "main".into(),
            vec![Action::Label("start".into()), Action::Comment],
        )]);

        assert_eq!(program.scene_count(), 1);
        assert_eq!(program.action_count(), 2);
        assert_eq!(program.label("main", "start"), Some(0));
        assert!(program.contains_scene("main"));
        assert_ne!(program.fingerprint(), 0);
    }

    #[test]
    fn fingerprint_is_order_independent_and_changes_with_action_layout() {
        let first = Program::from_scenes([
            ("b".into(), vec![Action::Comment]),
            ("a".into(), vec![Action::Label("start".into())]),
        ]);
        let reordered = Program::from_scenes([
            ("a".into(), vec![Action::Label("start".into())]),
            ("b".into(), vec![Action::Comment]),
        ]);
        let changed = Program::from_scenes([
            ("a".into(), vec![Action::Comment]),
            ("b".into(), vec![Action::Label("start".into())]),
        ]);

        assert_eq!(first.fingerprint(), reordered.fingerprint());
        assert_ne!(first.fingerprint(), changed.fingerprint());
    }
}
