// Central game state — the single source of truth.
//
// No ECS. No VM. Just a struct.
// All persistent fields are Serde-serializable (save/rollback ready).

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use crate::action::{ChoiceTarget, Program, StageAnimation, SystemMessageMode, SystemUiSlot};
use crate::config::TextRevealConfig;
use crate::types::{
    AnimationPreset, BlendMode, CameraShakeSpec, CameraTargets, DialogueStyle, Easing, FilmEffects,
    InputValueType, ParticleEffect, PortraitStyle, Position, PostProcessEffect, SpriteLayout,
    SpriteTransform, Transition, Value, VideoSpec, VisualFilter,
};

/// The complete game state at any point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct State {
    // ── Script execution ──
    /// Immutable compiled script data, shared by snapshots and omitted from saves.
    #[serde(skip, default)]
    pub program: Arc<Program>,
    /// Stable identity of the compiled program this state was created against.
    pub program_fingerprint: u64,
    /// Typed, transient commands consumed by the engine-owned UI shell.
    #[serde(skip, default)]
    pub shell_events: Vec<ShellEvent>,
    #[serde(skip, default)]
    pub host_commands: Vec<HostCommandEvent>,
    /// Current scene being executed.
    pub current_scene: String,
    /// Index into the current scene's action list.
    pub cursor: usize,
    /// Suspended callers, most recent frame last.
    pub scene_stack: Vec<SceneFrame>,
    /// Explicit `end` reached; the presentation layer should show the title.
    pub ended: bool,

    // ── Display state ──
    /// Current background image path.
    pub bg: Option<String>,
    /// Current background transition in progress.
    pub bg_transition: Option<BgTransition>,
    pub bg_transform: SpriteTransform,
    pub bg_transform_animation: Option<TransformAnimation>,
    #[serde(skip, default)]
    pub bg_keyframe_animation: Option<KeyframeAnimation>,
    pub bg_filter: VisualFilter,
    #[serde(default)]
    pub bg_films: FilmEffects,
    pub bg_animation: Option<PresetAnimation>,
    #[serde(default)]
    pub bg_camera_distance: Option<f32>,
    #[serde(default)]
    pub camera_effect: PostProcessEffect,
    #[serde(default)]
    pub camera_transform: SpriteTransform,
    #[serde(default)]
    pub camera_targets: CameraTargets,
    #[serde(skip, default)]
    pub camera_transform_animation: Option<TransformAnimation>,
    #[serde(skip, default)]
    pub camera_shake: Option<CameraShakeState>,
    #[serde(default)]
    pub camera_effect_targets: CameraTargets,
    #[serde(skip, default)]
    pub camera_effect_animation: Option<PostProcessAnimation>,
    /// One adapter-neutral shared-clock stage timeline. It is transient
    /// presentation state and is reconstructed by editor preview replay.
    #[serde(skip, default)]
    pub stage_animation: Option<StageAnimationState>,
    #[serde(skip, default)]
    pub videos: HashMap<String, VideoState>,
    #[serde(skip, default)]
    pub video_revision_counter: u64,
    /// Monotonic invalidation token for sprite/camera render inputs. Hosts use
    /// this instead of comparing the complete sprite map every frame.
    #[serde(skip, default)]
    pub stage_revision: u64,
    /// Active sprites, keyed by id.
    pub sprites: HashMap<String, Sprite>,
    #[serde(skip, default)]
    pub sprite_sequences: HashMap<String, SpriteSequenceState>,
    /// Current dialogue text (if any).
    pub dialogue: Option<Dialogue>,
    /// Last settled dialogue, used by WebGAL `-concat`.
    pub previous_dialogue: Option<Dialogue>,
    /// Active sentence-tail deletion. This is persisted so loading a save
    /// resumes the same visual character and click-wait phase.
    #[serde(default)]
    pub dialogue_retraction: Option<DialogueRetraction>,
    /// Mini avatar image path (displayed beside text box).
    pub mini_avatar: Option<String>,
    /// Mini avatar enter/exit transition progress (0→1).
    pub mini_avatar_progress: f32,
    pub textbox_hidden: bool,
    pub textbox_auto_hidden: bool,
    pub user_input: Option<UserInputState>,
    /// Active engine-owned alert/confirmation emitted by a script adapter.
    #[serde(skip, default)]
    pub system_message: Option<SystemMessageState>,

    // ── Presentation state ──
    pub wait_remaining: f32,
    pub wait_blocking: bool,
    #[serde(skip, default)]
    pub waiting_for_advance: bool,
    pub intro: Option<IntroState>,
    pub film_mode: bool,
    #[serde(skip, default)]
    pub curtain: CurtainState,
    #[serde(skip, default)]
    pub floating_text: Option<FloatingTextState>,
    #[serde(skip, default)]
    pub portrait_rule: Option<PortraitRuleState>,
    /// Active dialogue presentation. It is editor/runtime presentation state,
    /// so legacy save payloads remain stable; direct preview reconstructs it by
    /// replaying source actions up to the selected block.
    #[serde(skip, default)]
    pub dialogue_style: DialogueStyle,
    #[serde(skip, default)]
    pub paragraph_style: DialogueStyle,
    #[serde(skip, default)]
    pub active_text_style: DialogueStyle,
    #[serde(skip, default)]
    pub paragraph_typewriter_speed: Option<f64>,
    #[serde(skip, default)]
    pub paragraph_text_reveal: Option<TextRevealConfig>,
    #[serde(skip, default)]
    pub active_typewriter_speed: Option<f64>,
    #[serde(skip, default)]
    pub active_text_reveal: Option<TextRevealConfig>,
    #[serde(skip, default)]
    pub next_text_is_paragraph: bool,
    pub particle_effects: HashMap<String, ActiveParticleEffect>,
    pub transition_rules: HashMap<String, TransitionRule>,

    // ── Audio state ──
    pub bgm: BgmState,
    /// Persistent looping effects, keyed by WebGAL's `-id`.
    pub looping_effects: HashMap<String, EffectState>,
    /// One-shot effects emitted since the presentation layer last synchronized.
    #[serde(skip)]
    pub effect_queue: Vec<EffectEvent>,
    /// Standalone voice command waiting for the audio backend.
    #[serde(skip, default)]
    pub vocal_event: Option<VocalCue>,

    // ── Choice state ──
    /// Active choice menu (if any).
    pub menu: Option<MenuState>,

    // ── Variables ──
    /// Game variables (set by scripts).
    pub vars: HashMap<String, Value>,
    /// Persistent variables requested with WebGAL's `-global` flag.
    ///
    /// Profile data is stored independently from individual save slots.
    #[serde(skip, default)]
    pub global_vars: HashMap<String, Value>,

    // ── Backlog / rollback ──
    /// Recent dialogue checkpoints. Scripts are intentionally excluded from
    /// each snapshot so history remains bounded and cheap to clone.
    pub backlog: Vec<BacklogEntry>,
    /// Stable script positions already presented to the player.
    #[serde(skip, default)]
    pub read_dialogues: HashSet<DialogueKey>,
    #[serde(skip, default)]
    pub unlocked_cg: HashMap<String, String>,
    #[serde(skip, default)]
    pub unlocked_bgm: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    SetAutoplay(bool),
    SetSystemUi { slot: SystemUiSlot, visible: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCommandEvent {
    pub namespace: String,
    pub command: String,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoState {
    pub spec: VideoSpec,
    pub revision: u64,
    pub elapsed: f32,
    pub opacity: f32,
    pub stopping: bool,
    pub fade_out: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraShakeState {
    pub spec: CameraShakeSpec,
    pub elapsed: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub blocking: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageAnimationState {
    pub animation: StageAnimation,
    /// Absolute authored time across all loops, in seconds.
    pub elapsed: f32,
    pub previous_elapsed: f32,
    /// A scene layer may be created by a cue after the timeline starts, so
    /// track initial values are captured lazily.
    pub initial_values: Vec<Option<f32>>,
    pub track_start_times: Vec<f32>,
    pub initial_camera_transform: SpriteTransform,
    pub initial_camera_effect: PostProcessEffect,
    pub initial_camera_targets: CameraTargets,
    pub initial_camera_effect_targets: CameraTargets,
}

impl StageAnimationState {
    pub fn new(animation: StageAnimation, state: &State) -> Self {
        let track_count = animation.tracks.len();
        Self {
            animation,
            // Start just before authored time zero so zero-time events fire on
            // the first presentation tick instead of being skipped.
            elapsed: -f32::EPSILON,
            previous_elapsed: -f32::EPSILON,
            initial_values: vec![None; track_count],
            track_start_times: vec![0.0; track_count],
            initial_camera_transform: state.camera_transform,
            initial_camera_effect: state.camera_effect.clone(),
            initial_camera_targets: state.camera_targets,
            initial_camera_effect_targets: state.camera_effect_targets,
        }
    }
}

pub const DEFAULT_BACKLOG_CAPACITY: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BgmState {
    pub file: Option<String>,
    pub volume: f32,
    pub fade_seconds: f32,
    pub revision: u64,
}

impl Default for BgmState {
    fn default() -> Self {
        Self {
            file: None,
            volume: 1.0,
            fade_seconds: 0.0,
            revision: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectState {
    pub file: String,
    pub volume: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectCue {
    pub id: Option<String>,
    pub file: String,
    pub volume: f32,
    pub fade_in: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectEvent {
    Play(EffectCue),
    Stop,
    StopOneShot { id: String, fade_out: f32 },
    StartLoop { id: String, fade_in: f32 },
    StopLoop { id: String, fade_out: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct VocalCue {
    pub file: Option<String>,
    pub volume: f32,
    pub fade_in: f32,
    pub fade_out: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DialogueKey {
    pub scene: String,
    pub action_index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacklogEntry {
    pub key: DialogueKey,
    pub speaker: String,
    pub text: String,
    pub vocal: Option<String>,
    pub snapshot: RollbackSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RollbackSnapshot {
    pub program_fingerprint: u64,
    pub current_scene: String,
    pub cursor: usize,
    pub scene_stack: Vec<SceneFrame>,
    pub bg: Option<String>,
    pub bg_transform: SpriteTransform,
    pub bg_filter: VisualFilter,
    #[serde(default)]
    pub bg_films: FilmEffects,
    #[serde(default)]
    pub bg_camera_distance: Option<f32>,
    #[serde(default)]
    pub camera_effect: PostProcessEffect,
    #[serde(default)]
    pub camera_transform: SpriteTransform,
    #[serde(default)]
    pub camera_targets: CameraTargets,
    #[serde(default)]
    pub camera_effect_targets: CameraTargets,
    pub sprites: Arc<HashMap<String, Sprite>>,
    #[serde(skip, default)]
    pub sprite_sequences: HashMap<String, SpriteSequenceState>,
    pub dialogue: Dialogue,
    pub mini_avatar: Option<String>,
    pub textbox_hidden: bool,
    pub textbox_auto_hidden: bool,
    pub film_mode: bool,
    #[serde(skip, default)]
    pub curtain: CurtainState,
    #[serde(skip, default)]
    pub floating_text: Option<FloatingTextState>,
    #[serde(skip, default)]
    pub portrait_rule: Option<PortraitRuleState>,
    #[serde(skip, default)]
    pub dialogue_style: DialogueStyle,
    #[serde(skip, default)]
    pub paragraph_style: DialogueStyle,
    #[serde(skip, default)]
    pub active_text_style: DialogueStyle,
    #[serde(skip, default)]
    pub paragraph_typewriter_speed: Option<f64>,
    #[serde(skip, default)]
    pub paragraph_text_reveal: Option<TextRevealConfig>,
    #[serde(skip, default)]
    pub active_typewriter_speed: Option<f64>,
    #[serde(skip, default)]
    pub active_text_reveal: Option<TextRevealConfig>,
    #[serde(skip, default)]
    pub next_text_is_paragraph: bool,
    pub particle_effects: Arc<HashMap<String, ActiveParticleEffect>>,
    pub transition_rules: Arc<HashMap<String, TransitionRule>>,
    pub bgm: BgmState,
    pub looping_effects: Arc<HashMap<String, EffectState>>,
    pub vars: Arc<HashMap<String, Value>>,
}

/// Return point saved by `callScene`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneFrame {
    pub scene: String,
    pub cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreError {
    ProgramMismatch { saved: u64, current: u64 },
}

impl fmt::Display for RestoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProgramMismatch { saved, current } => write!(
                formatter,
                "save program fingerprint {saved:016x} does not match current program {current:016x}"
            ),
        }
    }
}

impl std::error::Error for RestoreError {}

/// Choice menu currently blocking script execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuState {
    pub prompt: String,
    pub choices: Vec<MenuChoice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MenuChoice {
    pub text: String,
    pub target: ChoiceTarget,
    pub enabled: bool,
}

/// A sprite displayed on screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sprite {
    /// Asset path.
    pub image: String,
    /// Design-space position.
    pub position: Position,
    /// Figure baseline or editor-authored scene-canvas layout.
    #[serde(default)]
    pub layout: SpriteLayout,
    /// Current animation progress (0.0 → 1.0).
    pub transition_progress: f32,
    /// Active enter/exit transition.
    pub transition: Transition,
    /// Whether this sprite is entering (true) or exiting (false).
    pub entering: bool,
    /// Horizontal offset at the start of a slide transition.
    ///
    /// This field keeps its original serialization position for save compatibility.
    pub transition_offset_x: f32,
    pub transition_blocking: bool,
    /// Transform applied on top of base position (offset, alpha, scale, etc).
    #[serde(default)]
    pub transform: SpriteTransform,
    pub transform_animation: Option<TransformAnimation>,
    #[serde(skip, default)]
    pub keyframe_animation: Option<KeyframeAnimation>,
    pub filter: VisualFilter,
    #[serde(default)]
    pub films: FilmEffects,
    pub animation: Option<PresetAnimation>,
    pub z_index: i32,
    pub blend: BlendMode,
    /// Camera-space distance when this sprite participates in camera moves.
    #[serde(default)]
    pub camera_distance: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpriteSequenceState {
    pub frames: Vec<String>,
    pub fps: f32,
    pub looped: bool,
    pub elapsed: f32,
    pub frame: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetAnimation {
    pub preset: AnimationPreset,
    pub base: SpriteTransform,
    pub elapsed: f32,
    pub duration: f32,
    pub blocking: bool,
    pub remove_on_finish: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransitionRule {
    pub enter: Option<AnimationPreset>,
    pub exit: Option<AnimationPreset>,
    pub duration: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntroState {
    pub pages: Vec<String>,
    pub page: usize,
    pub elapsed: f32,
    pub hold: bool,
    pub blocking: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CurtainState {
    pub color: [f32; 4],
    pub current: f32,
    pub from: f32,
    pub target: f32,
    pub elapsed: f32,
    pub duration: f32,
    pub blocking: bool,
}

impl Default for CurtainState {
    fn default() -> Self {
        Self {
            color: [0.0, 0.0, 0.0, 1.0],
            current: 0.0,
            from: 0.0,
            target: 0.0,
            elapsed: 0.0,
            duration: 0.0,
            blocking: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FloatingTextState {
    pub id: Option<String>,
    pub text: String,
    pub position: [f32; 2],
    pub font_size: f32,
    pub color: [f32; 4],
    pub fade_in: f32,
    pub hold: f32,
    pub fade_out: f32,
    pub elapsed: f32,
    pub blocking: bool,
    pub infinite: bool,
}

impl FloatingTextState {
    pub fn duration(&self) -> f32 {
        self.fade_in + self.hold + self.fade_out
    }

    pub fn alpha(&self) -> f32 {
        if self.elapsed < self.fade_in && self.fade_in > f32::EPSILON {
            return (self.elapsed / self.fade_in).clamp(0.0, 1.0);
        }
        if self.infinite {
            return 1.0;
        }
        let fade_out_start = self.fade_in + self.hold;
        if self.elapsed > fade_out_start && self.fade_out > f32::EPSILON {
            return (1.0 - (self.elapsed - fade_out_start) / self.fade_out).clamp(0.0, 1.0);
        }
        1.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemMessageState {
    pub mode: SystemMessageMode,
    pub title: String,
    pub message: String,
    pub confirm_text: String,
    pub cancel_text: String,
    pub result_variable: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PortraitRuleState {
    pub enabled: bool,
    pub character_ids: HashSet<String>,
    pub speaking: PortraitStyle,
    pub others: PortraitStyle,
    pub narration: PortraitStyle,
    pub duration: f32,
    pub easing: Easing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserInputState {
    pub variable: String,
    pub title: String,
    pub button: String,
    pub value: String,
    #[serde(default)]
    pub value_type: InputValueType,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub required_text: String,
    #[serde(default = "default_input_required")]
    pub required: bool,
    #[serde(default)]
    pub min_length: usize,
    #[serde(default)]
    pub max_length: usize,
    #[serde(default)]
    pub min_value: Option<f64>,
    #[serde(default)]
    pub max_value: Option<f64>,
    #[serde(default = "default_input_step")]
    pub step: f64,
    #[serde(default)]
    pub true_text: String,
    #[serde(default)]
    pub false_text: String,
    #[serde(skip, default)]
    pub error: String,
}

const fn default_input_required() -> bool {
    true
}

const fn default_input_step() -> f64 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveParticleEffect {
    pub effect: ParticleEffect,
    pub elapsed: f32,
    pub fading_out: bool,
    pub fade_out: f32,
}

impl ActiveParticleEffect {
    pub fn new(effect: ParticleEffect) -> Self {
        Self {
            effect,
            elapsed: 0.0,
            fading_out: false,
            fade_out: 0.0,
        }
    }

    pub fn opacity(&self) -> f32 {
        if self.fading_out {
            if self.fade_out <= f32::EPSILON {
                return 0.0;
            }
            return (1.0 - self.elapsed / self.fade_out).clamp(0.0, 1.0);
        }
        if self.effect.fade_in <= f32::EPSILON {
            1.0
        } else {
            (self.elapsed / self.effect.fade_in).clamp(0.0, 1.0)
        }
    }

    pub fn begin_fade_out(&mut self, duration: f32) {
        self.elapsed = 0.0;
        self.fading_out = true;
        self.fade_out = duration.max(0.0);
    }

    pub fn finished(&self) -> bool {
        self.fading_out && self.elapsed >= self.fade_out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TransformAnimation {
    pub from: SpriteTransform,
    pub to: SpriteTransform,
    pub elapsed: f32,
    pub duration: f32,
    pub easing: Easing,
    pub blocking: bool,
}

/// Runtime timeline assembled from adapter-neutral keyframe patches.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframeAnimation {
    pub initial: SpriteTransform,
    pub frames: Vec<TransformAnimation>,
    pub index: usize,
    pub repeat_remaining: u32,
    pub blocking: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PostProcessAnimation {
    pub from: PostProcessEffect,
    pub to: PostProcessEffect,
    pub elapsed: f32,
    pub duration: f32,
    pub easing: Easing,
    pub blocking: bool,
}

/// Background transition state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BgTransition {
    /// Previous background path (for crossfade).
    pub from: Option<String>,
    /// Target background path.
    pub to: String,
    /// Transition progress (0.0 → 1.0).
    pub progress: f32,
    /// Transition kind.
    pub kind: Transition,
    pub blocking: bool,
}

/// Current dialogue being displayed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dialogue {
    /// Speaker name.
    pub speaker: String,
    /// Full text content.
    pub text: String,
    /// Interpolated source preserving WebGAL style/ruby markup for the UI backend.
    pub markup: String,
    /// Number of visible characters (typewriter effect).
    pub visible_chars: usize,
    /// Zero-width pauses embedded in the authored dialogue markup.
    #[serde(default)]
    pub pauses: Vec<DialoguePause>,
    pub vocal: Option<String>,
    pub volume: f32,
    pub auto_advance: bool,
}

/// Frame-rate-independent sentence-tail deletion presentation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialogueRetraction {
    /// Final plain-text prefix after the deletion animation.
    pub keep: String,
    /// UI glyph count at which deletion stops.
    pub target_visible_chars: usize,
    /// Fractional deletion progress carried across frames and save/load.
    pub fractional_chars: f64,
    /// Deletion has completed and exactly one new advance input is required.
    pub awaiting_advance: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DialoguePause {
    /// Number of visible characters before this pause becomes active.
    pub at: usize,
    /// Timed pause in seconds; `None` waits for player input.
    pub duration: Option<f32>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark sprite or camera presentation data as changed.
    pub fn invalidate_stage(&mut self) {
        self.stage_revision = self.stage_revision.wrapping_add(1);
    }

    /// Install a freshly compiled program and clamp every persisted cursor to
    /// the new scene boundaries. Replacing the `Arc` keeps hot reload atomic.
    pub fn install_program(&mut self, program: Program) {
        self.program_fingerprint = program.fingerprint();
        self.program = Arc::new(program);
        self.reconcile_program_positions();
    }

    /// Reconcile persisted execution positions against the currently attached
    /// program. Returns whether execution still has a valid current scene.
    fn reconcile_program_positions(&mut self) -> bool {
        let program = &self.program;
        let program_fingerprint = self.program_fingerprint;
        reconcile_scene_stack(program, &mut self.scene_stack);
        self.backlog
            .retain_mut(|entry| reconcile_backlog_entry(program, program_fingerprint, entry));

        if let Some(length) = program.scene_len(&self.current_scene) {
            self.cursor = self.cursor.min(length);
            return true;
        }

        if let Some(frame) = self.scene_stack.pop() {
            self.current_scene = frame.scene;
            self.cursor = frame.cursor;
            return true;
        }

        self.current_scene.clear();
        self.cursor = 0;
        false
    }

    /// Add or replace one scene while constructing tests or incremental tools.
    /// Production hot reload should install a complete program atomically.
    pub fn insert_scene(&mut self, name: String, actions: Vec<crate::Action>) {
        let program = Arc::make_mut(&mut self.program);
        program.insert_scene(name, actions);
        self.program_fingerprint = program.fingerprint();
        self.reconcile_program_positions();
    }

    /// Apply persisted state without discarding the program loaded from the
    /// current project. Programs are deliberately absent from save files.
    pub fn restore_saved(&mut self, mut saved: Self) -> Result<(), RestoreError> {
        if saved.program_fingerprint != self.program_fingerprint {
            return Err(RestoreError::ProgramMismatch {
                saved: saved.program_fingerprint,
                current: self.program_fingerprint,
            });
        }
        saved.program = Arc::clone(&self.program);
        saved.stage_revision = self.stage_revision.wrapping_add(1);
        saved.global_vars = std::mem::take(&mut self.global_vars);
        saved.read_dialogues = std::mem::take(&mut self.read_dialogues);
        saved.unlocked_cg = std::mem::take(&mut self.unlocked_cg);
        saved.unlocked_bgm = std::mem::take(&mut self.unlocked_bgm);
        if !saved.reconcile_program_positions() {
            saved.ended = true;
        }
        *self = saved;
        Ok(())
    }

    pub fn record_dialogue(&mut self, action_index: usize) {
        let Some(dialogue) = self.dialogue.clone() else {
            return;
        };
        let key = DialogueKey {
            scene: self.current_scene.clone(),
            action_index,
        };
        let previous = self.backlog.last().map(|entry| &entry.snapshot);
        let sprites = shared_snapshot(&self.sprites, previous.map(|snapshot| &snapshot.sprites));
        let particle_effects = shared_snapshot(
            &self.particle_effects,
            previous.map(|snapshot| &snapshot.particle_effects),
        );
        let transition_rules = shared_snapshot(
            &self.transition_rules,
            previous.map(|snapshot| &snapshot.transition_rules),
        );
        let looping_effects = shared_snapshot(
            &self.looping_effects,
            previous.map(|snapshot| &snapshot.looping_effects),
        );
        let vars = shared_snapshot(&self.vars, previous.map(|snapshot| &snapshot.vars));
        self.backlog.push(BacklogEntry {
            key,
            speaker: dialogue.speaker.clone(),
            text: dialogue.text.clone(),
            vocal: dialogue.vocal.clone(),
            snapshot: RollbackSnapshot {
                program_fingerprint: self.program_fingerprint,
                current_scene: self.current_scene.clone(),
                cursor: self.cursor,
                scene_stack: self.scene_stack.clone(),
                bg: self.bg.clone(),
                bg_transform: self.bg_transform,
                bg_filter: self.bg_filter,
                bg_films: self.bg_films,
                bg_camera_distance: self.bg_camera_distance,
                camera_effect: self.camera_effect.clone(),
                camera_transform: self.camera_transform,
                camera_targets: self.camera_targets,
                camera_effect_targets: self.camera_effect_targets,
                sprites,
                sprite_sequences: self.sprite_sequences.clone(),
                dialogue,
                mini_avatar: self.mini_avatar.clone(),
                textbox_hidden: self.textbox_hidden,
                textbox_auto_hidden: self.textbox_auto_hidden,
                film_mode: self.film_mode,
                curtain: self.curtain.clone(),
                floating_text: self.floating_text.clone(),
                portrait_rule: self.portrait_rule.clone(),
                dialogue_style: self.dialogue_style.clone(),
                paragraph_style: self.paragraph_style.clone(),
                active_text_style: self.active_text_style.clone(),
                paragraph_typewriter_speed: self.paragraph_typewriter_speed,
                paragraph_text_reveal: self.paragraph_text_reveal,
                active_typewriter_speed: self.active_typewriter_speed,
                active_text_reveal: self.active_text_reveal,
                next_text_is_paragraph: self.next_text_is_paragraph,
                particle_effects,
                transition_rules,
                bgm: self.bgm.clone(),
                looping_effects,
                vars,
            },
        });
        let excess = self.backlog.len().saturating_sub(DEFAULT_BACKLOG_CAPACITY);
        if excess > 0 {
            self.backlog.drain(..excess);
        }
    }

    pub fn current_dialogue_key(&self) -> Option<DialogueKey> {
        self.dialogue.as_ref().map(|_| DialogueKey {
            scene: self.current_scene.clone(),
            action_index: self.cursor.saturating_sub(1),
        })
    }

    pub fn current_dialogue_is_read(&self) -> bool {
        self.current_dialogue_key()
            .is_some_and(|key| self.read_dialogues.contains(&key))
    }

    pub fn mark_current_dialogue_read(&mut self) {
        if let Some(key) = self.current_dialogue_key() {
            self.read_dialogues.insert(key);
        }
    }

    pub fn restore_backlog(&mut self, index: usize) -> bool {
        let Some(mut entry) = self.backlog.get(index).cloned() else {
            return false;
        };
        if !reconcile_backlog_entry(&self.program, self.program_fingerprint, &mut entry) {
            return false;
        }
        let snapshot = entry.snapshot;
        let next_bgm_revision = self.bgm.revision.wrapping_add(1);
        let restored_vocal = VocalCue {
            file: snapshot.dialogue.vocal.clone(),
            volume: snapshot.dialogue.volume,
            fade_in: 0.0,
            fade_out: 0.0,
        };
        self.shell_events.clear();
        self.host_commands.clear();
        self.current_scene = snapshot.current_scene;
        self.cursor = snapshot.cursor;
        self.scene_stack = snapshot.scene_stack;
        self.bg = snapshot.bg;
        self.bg_transition = None;
        self.bg_transform = snapshot.bg_transform;
        self.bg_filter = snapshot.bg_filter;
        self.bg_films = snapshot.bg_films;
        self.bg_camera_distance = snapshot.bg_camera_distance;
        self.camera_effect = snapshot.camera_effect;
        self.camera_transform = snapshot.camera_transform;
        self.camera_targets = snapshot.camera_targets;
        self.camera_transform_animation = None;
        self.camera_shake = None;
        self.camera_effect_targets = snapshot.camera_effect_targets;
        self.camera_effect_animation = None;
        self.stage_animation = None;
        self.videos.clear();
        self.bg_transform_animation = None;
        self.bg_keyframe_animation = None;
        self.bg_animation = None;
        self.sprites = snapshot.sprites.as_ref().clone();
        self.sprite_sequences = snapshot.sprite_sequences;
        self.dialogue = Some(snapshot.dialogue);
        self.previous_dialogue = None;
        self.dialogue_retraction = None;
        self.mini_avatar = snapshot.mini_avatar;
        self.textbox_hidden = snapshot.textbox_hidden;
        self.textbox_auto_hidden = snapshot.textbox_auto_hidden;
        self.user_input = None;
        self.system_message = None;
        self.mini_avatar_progress = if self.mini_avatar.is_some() { 1.0 } else { 0.0 };
        self.bgm = snapshot.bgm;
        self.bgm.revision = next_bgm_revision;
        self.looping_effects = snapshot.looping_effects.as_ref().clone();
        self.effect_queue.clear();
        self.effect_queue.push(EffectEvent::Stop);
        self.vocal_event = Some(restored_vocal);
        self.vars = snapshot.vars.as_ref().clone();
        self.menu = None;
        self.wait_remaining = 0.0;
        self.wait_blocking = false;
        self.waiting_for_advance = false;
        self.intro = None;
        self.film_mode = snapshot.film_mode;
        self.curtain = snapshot.curtain;
        self.floating_text = snapshot.floating_text;
        self.portrait_rule = snapshot.portrait_rule;
        self.dialogue_style = snapshot.dialogue_style;
        self.paragraph_style = snapshot.paragraph_style;
        self.active_text_style = snapshot.active_text_style;
        self.paragraph_typewriter_speed = snapshot.paragraph_typewriter_speed;
        self.paragraph_text_reveal = snapshot.paragraph_text_reveal;
        self.active_typewriter_speed = snapshot.active_typewriter_speed;
        self.active_text_reveal = snapshot.active_text_reveal;
        self.next_text_is_paragraph = snapshot.next_text_is_paragraph;
        self.particle_effects = snapshot.particle_effects.as_ref().clone();
        self.transition_rules = snapshot.transition_rules.as_ref().clone();
        self.ended = false;
        self.backlog.truncate(index + 1);
        self.invalidate_stage();
        true
    }

    pub fn presentation_blocked(&self) -> bool {
        self.dialogue_retraction.is_some()
            || (self.wait_blocking && self.wait_remaining > 0.0)
            || self.waiting_for_advance
            || self.system_message.is_some()
            || self.intro.as_ref().is_some_and(|intro| intro.blocking)
            || self.curtain.blocking
            || self
                .floating_text
                .as_ref()
                .is_some_and(|text| text.blocking)
            || self
                .videos
                .values()
                .any(|video| video.spec.wait_for_finished && !video.spec.looped && !video.stopping)
            || self
                .camera_effect_animation
                .as_ref()
                .is_some_and(|animation| animation.blocking)
            || self
                .camera_transform_animation
                .as_ref()
                .is_some_and(|animation| animation.blocking)
            || self
                .camera_shake
                .as_ref()
                .is_some_and(|animation| animation.blocking)
            || self
                .stage_animation
                .as_ref()
                .is_some_and(|animation| animation.animation.blocking)
            || self
                .bg_animation
                .as_ref()
                .is_some_and(|animation| animation.blocking)
            || self.sprites.values().any(|sprite| {
                sprite
                    .animation
                    .as_ref()
                    .is_some_and(|animation| animation.blocking)
            })
            || self
                .bg_transition
                .as_ref()
                .is_some_and(|transition| transition.blocking)
            || self
                .bg_transform_animation
                .as_ref()
                .is_some_and(|animation| animation.blocking)
            || self
                .bg_keyframe_animation
                .as_ref()
                .is_some_and(|animation| animation.blocking)
            || self.sprites.values().any(|sprite| {
                sprite
                    .transform_animation
                    .as_ref()
                    .is_some_and(|animation| animation.blocking)
                    || sprite
                        .keyframe_animation
                        .as_ref()
                        .is_some_and(|animation| animation.blocking)
                    || (sprite.transition_blocking
                        && ((sprite.entering && sprite.transition_progress < 1.0)
                            || (!sprite.entering && sprite.transition_progress > 0.0)))
            })
    }
}

fn reconcile_scene_stack(program: &Program, scene_stack: &mut Vec<SceneFrame>) {
    scene_stack.retain_mut(|frame| {
        let Some(length) = program.scene_len(&frame.scene) else {
            return false;
        };
        frame.cursor = frame.cursor.min(length);
        true
    });
}

fn shared_snapshot<T: Clone + PartialEq>(current: &T, previous: Option<&Arc<T>>) -> Arc<T> {
    previous
        .filter(|value| value.as_ref() == current)
        .map_or_else(|| Arc::new(current.clone()), Arc::clone)
}

fn reconcile_backlog_entry(
    program: &Program,
    program_fingerprint: u64,
    entry: &mut BacklogEntry,
) -> bool {
    if entry.snapshot.program_fingerprint != program_fingerprint {
        return false;
    }
    let Some(length) = program.scene_len(&entry.snapshot.current_scene) else {
        return false;
    };
    // A dialogue checkpoint cannot originate from an empty scene. Keeping such
    // an entry would display stale text with no valid action to resume from.
    if length == 0 {
        return false;
    }

    entry.snapshot.cursor = entry.snapshot.cursor.min(length);
    reconcile_scene_stack(program, &mut entry.snapshot.scene_stack);
    entry.key.scene.clone_from(&entry.snapshot.current_scene);
    entry.key.action_index = entry.snapshot.cursor.saturating_sub(1).min(length - 1);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Program};

    fn dialogue(text: &str) -> Dialogue {
        Dialogue {
            speaker: "MainCore".into(),
            text: text.into(),
            markup: text.into(),
            visible_chars: 0,
            pauses: Vec::new(),
            vocal: None,
            volume: 1.0,
            auto_advance: false,
        }
    }

    fn saved_for(state: &State) -> State {
        State {
            program_fingerprint: state.program_fingerprint,
            ..State::new()
        }
    }

    #[test]
    fn backlog_is_bounded_independently_from_read_history() {
        let mut state = State::new();
        state.current_scene = "main".into();
        for index in 0..DEFAULT_BACKLOG_CAPACITY + 5 {
            state.cursor = index + 1;
            state.dialogue = Some(dialogue(&format!("line {index}")));
            state.record_dialogue(index);
        }

        assert_eq!(state.backlog.len(), DEFAULT_BACKLOG_CAPACITY);
        assert_eq!(state.backlog.first().unwrap().key.action_index, 5);
        assert!(state.read_dialogues.is_empty());
        state.mark_current_dialogue_read();
        assert_eq!(state.read_dialogues.len(), 1);
    }

    #[test]
    fn snapshots_share_the_immutable_program() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Comment; 2_000],
        )]));

        let snapshot = state.clone();

        assert!(Arc::ptr_eq(&state.program, &snapshot.program));
        assert_eq!(snapshot.program.action_count(), 2_000);
    }

    #[test]
    fn adjacent_checkpoints_share_unchanged_large_collections() {
        let mut state = State::new();
        state.current_scene = "main".into();
        state.vars.insert("route".into(), Value::Int(1));
        state.dialogue = Some(dialogue("first"));
        state.record_dialogue(0);
        state.dialogue = Some(dialogue("second"));
        state.record_dialogue(1);

        let first = &state.backlog[0].snapshot;
        let second = &state.backlog[1].snapshot;
        assert!(Arc::ptr_eq(&first.sprites, &second.sprites));
        assert!(Arc::ptr_eq(&first.vars, &second.vars));
        let second_sprites = Arc::clone(&second.sprites);
        let second_vars = Arc::clone(&second.vars);

        state.vars.insert("route".into(), Value::Int(2));
        state.dialogue = Some(dialogue("third"));
        state.record_dialogue(2);
        let third = &state.backlog[2].snapshot;
        assert!(Arc::ptr_eq(&second_sprites, &third.sprites));
        assert!(!Arc::ptr_eq(&second_vars, &third.vars));
    }

    #[test]
    fn incremental_scene_insertion_keeps_state_fingerprint_in_sync() {
        let mut state = State::new();

        state.insert_scene("main".into(), vec![Action::Comment]);
        let first = state.program_fingerprint;
        assert_ne!(first, 0);
        assert_eq!(first, state.program.fingerprint());

        state.insert_scene("aside".into(), vec![Action::Comment; 2]);
        assert_ne!(state.program_fingerprint, first);
        assert_eq!(state.program_fingerprint, state.program.fingerprint());
    }

    #[test]
    fn restores_lightweight_dialogue_checkpoint() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Comment; 8],
        )]));
        state.current_scene = "main".into();
        state.cursor = 4;
        state.vars.insert("route".into(), Value::Int(1));
        state.dialogue = Some(dialogue("checkpoint"));
        state.record_dialogue(3);

        state.current_scene = "later".into();
        state.cursor = 99;
        state.mini_avatar_progress = 1.0;
        state.vars.insert("route".into(), Value::Int(2));
        state.dialogue = Some(dialogue("later"));

        assert!(state.restore_backlog(0));
        assert_eq!(state.current_scene, "main");
        assert_eq!(state.cursor, 4);
        assert_eq!(state.vars["route"], Value::Int(1));
        assert_eq!(state.dialogue.as_ref().unwrap().text, "checkpoint");
        assert_eq!(state.mini_avatar_progress, 0.0);
    }

    #[test]
    fn rollback_restores_presentation_and_retriggers_audio_deterministically() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Comment; 2],
        )]));
        state.current_scene = "main".into();
        state.cursor = 1;
        state.curtain = CurtainState {
            color: [0.1, 0.2, 0.3, 1.0],
            current: 0.4,
            from: 0.0,
            target: 1.0,
            elapsed: 0.2,
            duration: 0.5,
            blocking: false,
        };
        state.floating_text = Some(FloatingTextState {
            id: Some("checkpoint".into()),
            text: "checkpoint".into(),
            position: [0.25, 0.75],
            font_size: 32.0,
            color: [1.0, 0.5, 0.25, 1.0],
            fade_in: 0.1,
            hold: 1.0,
            fade_out: 0.2,
            elapsed: 0.3,
            blocking: false,
            infinite: false,
        });
        state.portrait_rule = Some(PortraitRuleState {
            enabled: true,
            character_ids: HashSet::from(["hero".into()]),
            speaking: PortraitStyle::default(),
            others: PortraitStyle {
                brightness: 0.5,
                ..PortraitStyle::default()
            },
            narration: PortraitStyle::default(),
            duration: 0.25,
            easing: Easing::EaseInOut,
        });
        state.dialogue_style = DialogueStyle::CinematicCentered;
        state.bgm = BgmState {
            file: Some("checkpoint.opus".into()),
            volume: 0.8,
            fade_seconds: 0.4,
            revision: 7,
        };
        let mut checkpoint = dialogue("checkpoint");
        checkpoint.vocal = Some("checkpoint-vocal.opus".into());
        checkpoint.volume = 0.6;
        state.dialogue = Some(checkpoint);
        state.record_dialogue(0);

        let snapshot = state.backlog[0].snapshot.clone();
        state.curtain = CurtainState::default();
        state.floating_text = None;
        state.portrait_rule = None;
        state.dialogue_style = DialogueStyle::Default;
        state.bgm.revision = 41;
        state.effect_queue.push(EffectEvent::Play(EffectCue {
            id: None,
            file: "stale.wav".into(),
            volume: 1.0,
            fade_in: 0.0,
        }));

        assert!(state.restore_backlog(0));
        assert_eq!(state.curtain, snapshot.curtain);
        assert_eq!(state.floating_text, snapshot.floating_text);
        assert_eq!(state.portrait_rule, snapshot.portrait_rule);
        assert_eq!(state.dialogue_style, snapshot.dialogue_style);
        assert_eq!(state.bgm.revision, 42);
        assert_eq!(state.effect_queue, [EffectEvent::Stop]);
        assert_eq!(
            state.vocal_event,
            Some(VocalCue {
                file: Some("checkpoint-vocal.opus".into()),
                volume: 0.6,
                fade_in: 0.0,
                fade_out: 0.0,
            })
        );

        assert!(state.restore_backlog(0));
        assert_eq!(state.bgm.revision, 43);
    }

    #[test]
    fn install_program_invalidates_backlog_from_the_previous_fingerprint() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([
            ("main".into(), vec![Action::Comment; 5]),
            ("caller".into(), vec![Action::Comment; 4]),
            ("deleted".into(), vec![Action::Comment; 3]),
            ("deleted-caller".into(), vec![Action::Comment; 2]),
        ]));

        state.current_scene = "main".into();
        state.cursor = 5;
        state.scene_stack = vec![
            SceneFrame {
                scene: "caller".into(),
                cursor: 4,
            },
            SceneFrame {
                scene: "deleted-caller".into(),
                cursor: 2,
            },
        ];
        state.dialogue = Some(dialogue("survives"));
        state.record_dialogue(4);

        state.current_scene = "deleted".into();
        state.cursor = 3;
        state.scene_stack.clear();
        state.dialogue = Some(dialogue("removed"));
        state.record_dialogue(2);

        state.install_program(Program::from_scenes([
            ("main".into(), vec![Action::Comment; 2]),
            ("caller".into(), vec![Action::Comment]),
        ]));

        assert!(state.backlog.is_empty());
    }

    #[test]
    fn restore_saved_reconciles_every_backlog_snapshot() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Comment; 2],
        )]));

        let mut saved = saved_for(&state);
        saved.current_scene = "main".into();
        saved.cursor = 9;
        saved.dialogue = Some(dialogue("survives"));
        saved.record_dialogue(8);
        saved.current_scene = "deleted".into();
        saved.cursor = 3;
        saved.dialogue = Some(dialogue("removed"));
        saved.record_dialogue(2);

        state.restore_saved(saved).unwrap();

        assert_eq!(state.backlog.len(), 1);
        assert_eq!(state.backlog[0].snapshot.cursor, 2);
        assert_eq!(state.backlog[0].key.action_index, 1);
    }

    #[test]
    fn restore_saved_rejects_a_different_program_without_mutating_state() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Comment],
        )]));
        state.current_scene = "main".into();
        state.cursor = 1;
        let before = state.clone();

        let mut saved = State::new();
        saved.install_program(Program::from_scenes([(
            "changed".into(),
            vec![Action::Comment; 2],
        )]));
        let error = state.restore_saved(saved).unwrap_err();

        assert_eq!(
            error,
            RestoreError::ProgramMismatch {
                saved: Program::from_scenes([("changed".into(), vec![Action::Comment; 2])])
                    .fingerprint(),
                current: before.program_fingerprint,
            }
        );
        assert_eq!(state, before);
    }

    #[test]
    fn restore_backlog_rejects_a_snapshot_invalidated_after_reconciliation() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Comment; 2],
        )]));
        state.current_scene = "main".into();
        state.cursor = 2;
        state.dialogue = Some(dialogue("stale"));
        state.record_dialogue(1);

        state.program = Arc::new(Program::from_scenes([(
            "replacement".into(),
            vec![Action::Comment],
        )]));

        assert!(!state.restore_backlog(0));
        assert_eq!(state.current_scene, "main");
        assert_eq!(state.cursor, 2);
    }

    #[test]
    fn restore_saved_recovers_from_a_deleted_scene_through_the_latest_valid_caller() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([
            ("main".into(), vec![Action::Comment; 2]),
            ("caller".into(), vec![Action::Comment; 3]),
        ]));

        let mut saved = saved_for(&state);
        saved.current_scene = "deleted-scene".into();
        saved.cursor = 99;
        saved.scene_stack.push(SceneFrame {
            scene: "caller".into(),
            cursor: 99,
        });

        state.restore_saved(saved).unwrap();

        assert_eq!(state.current_scene, "caller");
        assert_eq!(state.cursor, 3);
        assert!(state.scene_stack.is_empty());
        assert!(!state.ended);
    }

    #[test]
    fn restore_saved_safely_ends_when_no_saved_scene_still_exists() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Comment],
        )]));

        let mut saved = saved_for(&state);
        saved.current_scene = "deleted-scene".into();
        saved.cursor = 99;
        saved.scene_stack.push(SceneFrame {
            scene: "deleted-caller".into(),
            cursor: 99,
        });

        state.restore_saved(saved).unwrap();

        assert!(state.current_scene.is_empty());
        assert_eq!(state.cursor, 0);
        assert!(state.scene_stack.is_empty());
        assert!(state.ended);
    }

    #[test]
    fn restore_saved_clamps_the_current_cursor_after_a_script_shrinks() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Comment; 2],
        )]));

        let mut saved = saved_for(&state);
        saved.current_scene = "main".into();
        saved.cursor = 99;

        state.restore_saved(saved).unwrap();

        assert_eq!(state.current_scene, "main");
        assert_eq!(state.cursor, 2);
        assert!(!state.ended);
    }

    #[test]
    fn restore_saved_prunes_and_clamps_nested_call_frames() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([
            ("root".into(), vec![Action::Comment]),
            ("middle".into(), vec![Action::Comment; 3]),
            ("inner".into(), vec![Action::Comment; 2]),
        ]));

        let mut saved = saved_for(&state);
        saved.current_scene = "inner".into();
        saved.cursor = 99;
        saved.scene_stack = vec![
            SceneFrame {
                scene: "root".into(),
                cursor: 99,
            },
            SceneFrame {
                scene: "deleted-middle".into(),
                cursor: 99,
            },
            SceneFrame {
                scene: "middle".into(),
                cursor: 99,
            },
        ];

        state.restore_saved(saved).unwrap();

        assert_eq!(state.current_scene, "inner");
        assert_eq!(state.cursor, 2);
        assert_eq!(
            state.scene_stack,
            vec![
                SceneFrame {
                    scene: "root".into(),
                    cursor: 1,
                },
                SceneFrame {
                    scene: "middle".into(),
                    cursor: 3,
                },
            ]
        );
        assert!(!state.ended);
    }

    #[test]
    fn save_restore_preserves_current_profile_data() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "chapter-2".into(),
            vec![Action::Comment],
        )]));
        state
            .global_vars
            .insert("route_unlocked".into(), Value::Bool(true));
        state.read_dialogues.insert(DialogueKey {
            scene: "main".into(),
            action_index: 7,
        });
        state
            .unlocked_cg
            .insert("memory.webp".into(), "Memory".into());
        state
            .unlocked_bgm
            .insert("theme.opus".into(), "Theme".into());
        let program = Arc::clone(&state.program);

        let mut saved = saved_for(&state);
        saved.current_scene = "chapter-2".into();
        saved
            .global_vars
            .insert("route_unlocked".into(), Value::Bool(false));
        saved.read_dialogues.insert(DialogueKey {
            scene: "old".into(),
            action_index: 1,
        });
        saved.unlocked_cg.insert("old.webp".into(), "Old".into());
        saved.unlocked_bgm.insert("old.opus".into(), "Old".into());
        state.restore_saved(saved).unwrap();

        assert!(Arc::ptr_eq(&state.program, &program));
        assert_eq!(state.current_scene, "chapter-2");
        assert_eq!(state.global_vars["route_unlocked"], Value::Bool(true));
        assert_eq!(state.read_dialogues.len(), 1);
        assert_eq!(state.unlocked_cg["memory.webp"], "Memory");
        assert_eq!(state.unlocked_bgm["theme.opus"], "Theme");
        assert!(!state.unlocked_cg.contains_key("old.webp"));
        assert!(!state.unlocked_bgm.contains_key("old.opus"));
    }

    #[test]
    fn rollback_does_not_revert_global_profile_variables() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Comment],
        )]));
        state.current_scene = "main".into();
        state.global_vars.insert("endings".into(), Value::Int(1));
        state.dialogue = Some(dialogue("checkpoint"));
        state.record_dialogue(0);
        state.global_vars.insert("endings".into(), Value::Int(2));

        assert!(state.restore_backlog(0));
        assert_eq!(state.global_vars["endings"], Value::Int(2));
    }
}
