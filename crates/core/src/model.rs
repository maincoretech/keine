//! Serializable, language-neutral visual-novel model.

pub mod action;
pub mod state;
pub mod types;

pub use action::{
    Action, ChoiceTarget, Program, SayOptions, StageAnimation, StageAudioCue, StageAudioKind,
    StageEvent, StageEventKind, StageKeyframe, StageProperty, StageSceneCue, StageSceneLayer,
    StageTarget, StageTrack, SystemMessageMode, SystemMessageSpec, SystemUiSlot, TransformKeyframe,
};
pub use state::{
    ActiveParticleEffect, BgmState, CameraShakeState, DialoguePause, EffectCue, EffectEvent,
    EffectState, HostCommandEvent, MenuChoice, MenuState, PersistenceHazard, PersistenceSafety,
    PostProcessAnimation, RestoreError, SceneFrame, ShellEvent, SpriteSequenceState,
    StageAnimationState, State, VideoState, VocalCue,
};
pub use types::*;
