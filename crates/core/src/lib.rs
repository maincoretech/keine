//! Language-neutral visual-novel model and deterministic state machine.

#![warn(unused_crate_dependencies)]

pub mod config;
pub mod model;
pub mod runtime;

// Compatibility facades keep the stable public API while implementation files
// live under their actual architectural owners.
pub use model::{action, state, types};
pub use runtime::{dissolve, expression, step};

pub use model::ShellEvent;
pub use model::types::*;
pub use model::{
    Action, ActiveParticleEffect, BgmState, CameraShakeState, ChoiceTarget, DialoguePause,
    EffectCue, EffectEvent, EffectState, HostCommandEvent, MenuChoice, MenuState,
    PostProcessAnimation, Program, RestoreError, SayOptions, SceneFrame, StageAnimation,
    StageAnimationState, StageAudioCue, StageAudioKind, StageEvent, StageEventKind, StageKeyframe,
    StageProperty, StageSceneCue, StageSceneLayer, StageTarget, StageTrack, State,
    SystemMessageMode, SystemMessageSpec, SystemUiSlot, TransformKeyframe, VideoState, VocalCue,
};
pub use runtime::StepResult;

// Criterion is a bench-only dev-dependency; the lib-test build sees it as
// available and the crate-level lint would otherwise report it as unused.
#[cfg(test)]
use criterion as _;
