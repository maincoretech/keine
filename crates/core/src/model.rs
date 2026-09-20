//! Serializable, language-neutral visual-novel model.

pub mod action;
pub mod eiyashou;
pub mod state;
pub mod types;

pub use action::{
    Action, AssetHint, AssetHintKind, ChoiceTarget, LoadingStrategy, LoadingStrategyMode, Program,
    SayOptions, StageAnimation, StageAudioCue, StageAudioKind, StageEvent, StageEventKind,
    StageKeyframe, StageProperty, StageSceneCue, StageSceneLayer, StageTarget, StageTrack,
    SystemMessageMode, SystemMessageSpec, SystemUiSlot, TransformKeyframe,
};
pub use eiyashou::{
    EiyashouAssignOp, EiyashouBinaryOp, EiyashouChoice, EiyashouDialogue, EiyashouExpr,
    EiyashouListOperation, EiyashouPlace, EiyashouScalarType, EiyashouText, EiyashouTextPart,
    EiyashouType, EiyashouUnaryOp,
};
pub use state::{
    ActiveParticleEffect, BgmState, CameraShakeState, DialoguePause, EffectCue, EffectEvent,
    EffectState, HostCommandEvent, MenuChoice, MenuState, PersistenceHazard, PersistenceSafety,
    PostProcessAnimation, RestoreError, SceneFrame, ShellEvent, SpriteSequenceState,
    StageAnimationState, StageMaskState, State, VideoState, VocalCue,
};
pub use types::*;
