use keine_core::{State, StepResult, step};

use crate::storage::save::ContinuationCheckpoint;

/// Native policy for one script run after any user or presentation resume.
///
/// Core remains host-independent and reports [`StepResult`]. The native
/// frontend centralizes the two terminal cases here so individual UI entry
/// points cannot silently reinterpret or discard them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptOutcome {
    Yielded,
    ReturnToTitle(ReturnReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReturnReason {
    EndOfScene,
    ExecutionLimit,
    RuntimeError,
}

impl ScriptOutcome {
    pub(crate) const fn returns_to_title(self) -> bool {
        matches!(self, Self::ReturnToTitle(_))
    }
}

pub(crate) fn resume(state: &mut State, checkpoint: &mut ContinuationCheckpoint) -> ScriptOutcome {
    // Normal resumes already have the exact state captured after the previous
    // yield. Only a fresh/hot-reloaded program needs a pre-step seed; avoiding
    // an unconditional clone here keeps skip/auto traversal cheap.
    checkpoint.ensure_current_program(state);
    let result = step::step_preserving_presentation_observing(state, |before, action| {
        if introduces_transient_presentation(action) {
            checkpoint.capture(before);
        }
    });
    let outcome = classify_outcome(state, result);
    checkpoint.capture(state);
    outcome
}

// Capture only actions that can create fields omitted by Save v11. Ordinary
// dialogue/flow does not clone State per action. Replaying this boundary restores
// the presentation action, rather than skipping it or rerunning an earlier chapter.
fn introduces_transient_presentation(action: &keine_core::Action) -> bool {
    use keine_core::Action;
    match action {
        Action::Flow { action, .. } | Action::SpriteVisual { action, .. } => {
            introduces_transient_presentation(action)
        }
        Action::AnimateKeyframes { .. }
        | Action::PlayVideo { .. }
        | Action::SetPostProcess { .. }
        | Action::SetPostProcessV2 { .. }
        | Action::SetCameraTransform { .. }
        | Action::ShakeCamera { .. }
        | Action::ShakeCameraRandomized { .. }
        | Action::SetCameraTween { .. }
        | Action::ShowParticlesWithOptions { .. }
        | Action::ConfigureDynamicSpriteSequence { .. }
        | Action::PatchSprite { .. }
        | Action::StageAnimation { .. }
        | Action::StageMask { .. }
        | Action::ConfigureSceneMouseParallax { .. }
        | Action::ConfigureTimedSpriteSequence { .. }
        | Action::MoveSprite { .. }
        | Action::UpdateSprite { .. } => true,
        _ => false,
    }
}

fn resume_inner(state: &mut State) -> ScriptOutcome {
    let result = step::step_preserving_presentation(state);
    classify_outcome(state, result)
}

fn classify_outcome(state: &State, result: StepResult) -> ScriptOutcome {
    match result {
        StepResult::EndOfScene => ScriptOutcome::ReturnToTitle(ReturnReason::EndOfScene),
        StepResult::ExecutionLimit => {
            log::error!(
                target: "keine::runtime",
                "script execution stopped at {}:{} after reaching the forward-action safety limit; returning to title",
                state.current_scene,
                state.cursor,
            );
            ScriptOutcome::ReturnToTitle(ReturnReason::ExecutionLimit)
        }
        StepResult::RuntimeError(error) => {
            log::error!(
                target: "keine::runtime",
                "script execution stopped at {}:{} after runtime error {error:?}; returning to title",
                state.current_scene,
                state.cursor,
            );
            ScriptOutcome::ReturnToTitle(ReturnReason::RuntimeError)
        }
        StepResult::AwaitClick
        | StepResult::AwaitChoice
        | StepResult::AwaitPresentation
        | StepResult::AwaitInput => ScriptOutcome::Yielded,
    }
}

/// Editor, benchmark, and hot-reload entry points have no player-owned title
/// transition. They still use the same terminal policy, then settle cleanup
/// immediately instead of retaining a final presentation frame.
pub(crate) fn resume_for_tooling(state: &mut State) -> ScriptOutcome {
    let outcome = resume_inner(state);
    if outcome.returns_to_title() {
        step::end_game(state);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use keine_core::action::Choice;
    use keine_core::{
        Action, ChoiceTarget, Program, SayOptions, State, SystemMessageMode, SystemMessageSpec,
        VideoMode, VideoSpec, step,
    };

    use super::{ReturnReason, ScriptOutcome, resume, resume_for_tooling};
    use crate::storage::save::ContinuationCheckpoint;

    fn resume_game(state: &mut State) -> ScriptOutcome {
        resume(state, &mut ContinuationCheckpoint::default())
    }

    fn state_with(actions: Vec<Action>) -> State {
        let mut state = State::new();
        state.install_program(Program::from_scenes([("start".into(), actions)]));
        state.current_scene = "start".into();
        state
    }

    fn runaway_tail() -> impl Iterator<Item = Action> {
        std::iter::repeat_n(Action::Comment, 1_100)
    }

    #[test]
    fn every_resume_source_fails_closed_at_the_execution_limit() {
        let mut start = state_with(runaway_tail().collect());
        assert_eq!(
            resume_game(&mut start),
            ScriptOutcome::ReturnToTitle(ReturnReason::ExecutionLimit)
        );

        let mut choice = state_with(
            [
                Action::Menu {
                    prompt: String::new(),
                    choices: vec![Choice {
                        text: "Continue".into(),
                        target: ChoiceTarget::Label("tail".into()),
                        show_when: None,
                        enable_when: None,
                    }],
                },
                Action::Label("tail".into()),
            ]
            .into_iter()
            .chain(runaway_tail())
            .collect(),
        );
        assert_eq!(resume_game(&mut choice), ScriptOutcome::Yielded);
        step::select_choice(&mut choice, 0);
        assert_eq!(
            resume_game(&mut choice),
            ScriptOutcome::ReturnToTitle(ReturnReason::ExecutionLimit)
        );

        let mut input = state_with(
            [Action::UserInput {
                variable: "name".into(),
                title: "Name".into(),
                button: "OK".into(),
            }]
            .into_iter()
            .chain(runaway_tail())
            .collect(),
        );
        assert_eq!(resume_game(&mut input), ScriptOutcome::Yielded);
        input.user_input.as_mut().unwrap().value = "Kēne".into();
        assert!(step::submit_user_input(&mut input));
        assert_eq!(
            resume_game(&mut input),
            ScriptOutcome::ReturnToTitle(ReturnReason::ExecutionLimit)
        );
    }

    #[test]
    fn every_resume_source_returns_to_title_at_end_of_scene() {
        let expected = ScriptOutcome::ReturnToTitle(ReturnReason::EndOfScene);
        let mut start = state_with(vec![Action::End]);
        assert_eq!(resume_game(&mut start), expected);

        let mut choice = state_with(vec![
            Action::Menu {
                prompt: String::new(),
                choices: vec![Choice {
                    text: "End".into(),
                    target: ChoiceTarget::Label("end".into()),
                    show_when: None,
                    enable_when: None,
                }],
            },
            Action::Label("end".into()),
            Action::End,
        ]);
        assert_eq!(resume_game(&mut choice), ScriptOutcome::Yielded);
        step::select_choice(&mut choice, 0);
        assert_eq!(resume_game(&mut choice), expected);

        let mut input = state_with(vec![
            Action::UserInput {
                variable: "name".into(),
                title: "Name".into(),
                button: "OK".into(),
            },
            Action::End,
        ]);
        assert_eq!(resume_game(&mut input), ScriptOutcome::Yielded);
        input.user_input.as_mut().unwrap().value = "Kēne".into();
        assert!(step::submit_user_input(&mut input));
        assert_eq!(resume_game(&mut input), expected);
    }

    #[test]
    fn normal_yield_and_end_share_one_host_policy() {
        let mut dialogue = state_with(vec![Action::Say {
            speaker: "A".into(),
            text: "Hello".into(),
            options: SayOptions::default(),
        }]);
        assert_eq!(resume_game(&mut dialogue), ScriptOutcome::Yielded);

        let mut ended = state_with(vec![Action::End]);
        assert_eq!(
            resume_game(&mut ended),
            ScriptOutcome::ReturnToTitle(ReturnReason::EndOfScene)
        );
        assert!(!ended.ended, "the title transition owns final cleanup");

        let mut tooling = state_with(vec![Action::End]);
        assert_eq!(resume_for_tooling(&mut tooling), expected_end());
        assert!(tooling.ended, "tooling has no title transition owner");
    }

    fn expected_end() -> ScriptOutcome {
        ScriptOutcome::ReturnToTitle(ReturnReason::EndOfScene)
    }

    #[test]
    fn resolving_a_system_message_consumes_the_interaction_and_resumes() {
        let mut state = state_with(vec![
            Action::SystemMessage {
                spec: SystemMessageSpec {
                    mode: SystemMessageMode::Confirm,
                    title: "Confirm".into(),
                    message: "Continue?".into(),
                    confirm_text: "Yes".into(),
                    cancel_text: "No".into(),
                    result_variable: Some("accepted".into()),
                },
            },
            Action::Say {
                speaker: "A".into(),
                text: "Continued".into(),
                options: SayOptions::default(),
            },
        ]);

        assert_eq!(resume_game(&mut state), ScriptOutcome::Yielded);
        assert!(step::resolve_system_message(&mut state, true));
        assert_eq!(resume_game(&mut state), ScriptOutcome::Yielded);
        assert_eq!(
            state
                .dialogue
                .as_ref()
                .map(|dialogue| dialogue.text.as_str()),
            Some("Continued")
        );
    }

    #[test]
    fn save_boundary_is_the_action_before_presentation_inside_a_forward_batch() {
        let mut state = state_with(vec![
            Action::Set {
                name: "progress".into(),
                expression: "7".into(),
                global: false,
            },
            Action::Comment,
            Action::PlayVideo {
                video: VideoSpec {
                    id: "opening".into(),
                    file: "video/opening.mp4".into(),
                    looped: false,
                    muted: false,
                    alpha: 1.0,
                    skippable: true,
                    wait_for_finished: false,
                    mode: VideoMode::Fullscreen,
                },
            },
            Action::Say {
                speaker: "A".into(),
                text: "During presentation".into(),
                options: SayOptions::default(),
            },
        ]);
        let mut checkpoint = ContinuationCheckpoint::default();
        assert_eq!(resume(&mut state, &mut checkpoint), ScriptOutcome::Yielded);
        let saved = checkpoint.state_for_continuation(&state).unwrap().clone();
        assert_eq!(saved.cursor, 2);
        assert_eq!(saved.vars["progress"], keine_core::Value::Int(7));
        assert!(saved.persistence_safety().is_exact());
        state.restore_saved(saved).unwrap();
        assert_eq!(resume(&mut state, &mut checkpoint), ScriptOutcome::Yielded);
        assert!(state.videos.contains_key("opening"));
        assert_eq!(state.cursor, 4);
    }

    #[test]
    fn resume_captures_the_exact_state_before_nonblocking_video_advances_the_cursor() {
        let mut state = state_with(vec![
            Action::PlayVideo {
                video: VideoSpec {
                    id: "opening".into(),
                    file: "video/opening.mp4".into(),
                    looped: false,
                    muted: false,
                    alpha: 1.0,
                    skippable: true,
                    wait_for_finished: false,
                    mode: VideoMode::Fullscreen,
                },
            },
            Action::Say {
                speaker: "A".into(),
                text: "After video".into(),
                options: SayOptions::default(),
            },
        ]);
        let mut checkpoint = ContinuationCheckpoint::default();

        assert_eq!(resume(&mut state, &mut checkpoint), ScriptOutcome::Yielded);
        assert_eq!(state.cursor, 2);
        assert!(!state.persistence_safety().is_exact());
        assert_eq!(
            checkpoint
                .state_for_continuation(&state)
                .map(|state| state.cursor),
            Some(0)
        );
    }
}
