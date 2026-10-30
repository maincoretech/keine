//! Bounded read-only authoring state, sampled only on existing runtime frames.
use super::*;
use crate::storage::save::ContinuationCheckpoint;
use crate::ui::input_scope::UiInputScope;
use keine_authoring::{Diagnostic, DiagnosticLevel, RuntimeSnapshot};
use keine_core::{State, Value};

const MAX_VARIABLES: usize = 128;
const MAX_NAME_BYTES: usize = 128;
const MAX_VALUE_BYTES: usize = 512;

fn bounded(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit.saturating_sub(3);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

fn value_display(value: &Value) -> String {
    match value {
        Value::Array(values) => format!("Array [{}]", values.len()),
        Value::Str(value) => bounded(value, MAX_VALUE_BYTES),
        scalar => scalar.display(),
    }
}

pub(crate) fn runtime_snapshot(world: &World) -> RuntimeSnapshot {
    let Some(state) = world.get_resource::<GameState>() else {
        return RuntimeSnapshot::default();
    };
    let mut snapshot = RuntimeSnapshot {
        scene: bounded(&state.current_scene, MAX_VALUE_BYTES),
        cursor: state.cursor,
        checkpoint_is_live: state.persistence_safety().is_exact() && !state.ended,
        ..Default::default()
    };
    let scope = world
        .get_resource::<UiInputScope>()
        .copied()
        .unwrap_or_default();
    if scope != UiInputScope::Stage {
        snapshot.waiting.push(
            match scope {
                UiInputScope::Loading => "Asset loading / title transition",
                UiInputScope::UserInput => "Text input",
                UiInputScope::Dialog => "Confirmation dialog",
                UiInputScope::Menu => "Settings / save / load",
                UiInputScope::Backlog => "Backlog",
                UiInputScope::Extra => "Extra",
                UiInputScope::Title => "Title",
                UiInputScope::Stage => unreachable!(),
            }
            .into(),
        );
    }
    if world
        .get_resource::<super::super::resources::AssetLoadingGate>()
        .is_some_and(|gate| gate.blocked)
        && scope != UiInputScope::Loading
    {
        snapshot.waiting.push("Asset loading".into());
    }
    append_wait_reasons(state, &mut snapshot.waiting);
    let mut variables = state
        .vars
        .iter()
        .map(|(name, value)| (false, name, value))
        .chain(
            state
                .global_vars
                .iter()
                .map(|(name, value)| (true, name, value)),
        )
        .collect::<Vec<_>>();
    variables.sort_unstable_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    snapshot.omitted_variables = variables.len().saturating_sub(MAX_VARIABLES);
    snapshot.variables = variables
        .into_iter()
        .take(MAX_VARIABLES)
        .enumerate()
        .map(|(index, (global, name, value))| {
            if name.len() <= MAX_NAME_BYTES
                && match value {
                    Value::Array(_) => false,
                    Value::Str(s) => s.len() <= MAX_VALUE_BYTES,
                    _ => true,
                }
            {
                snapshot.editable_variables.push(index);
            }
            let name = bounded(name, MAX_NAME_BYTES);
            (
                if global {
                    format!("global.{name}")
                } else {
                    name
                },
                value_display(value),
            )
        })
        .collect();
    if let Some(checkpoint) = world.get_resource::<ContinuationCheckpoint>()
        && !state.ended
        && let Some(saved) = checkpoint.state_for_continuation(state)
    {
        // A saved dialogue/choice resumes that interaction; a checkpoint before
        // transient presentation resumes its next action, not the previous one.
        let index =
            if saved.dialogue.is_some() || saved.menu.is_some() || saved.user_input.is_some() {
                saved.cursor.saturating_sub(1)
            } else {
                saved.cursor
            };
        snapshot.checkpoint = state_location_at(world, &saved.current_scene, index);
    }
    if let Some(error) = &state.script_diagnostic
        && let Some((path, line, column)) =
            state_location_at(world, &error.scene, error.cursor.saturating_sub(1))
    {
        snapshot.diagnostic = Some(Diagnostic {
            level: DiagnosticLevel::Error,
            path,
            line,
            column,
            message: error.message.clone(),
        });
    }
    snapshot
}

fn append_wait_reasons(state: &State, reasons: &mut Vec<String>) {
    if state.menu.is_some() {
        reasons.push("Choice".into());
    }
    if state.user_input.is_some() {
        reasons.push("Text input".into());
    }
    let mut blocked = false;
    let _ = state.visit_presentation_blocks(|reason| {
        use keine_core::PresentationBlock as B;
        blocked = true;
        let label = match reason {
            B::TextRetraction => "Text retraction / advance input",
            B::TimedWait => "Timed wait",
            B::AdvanceInput => "Advance input",
            B::ScriptDialog => "Script dialog",
            B::IntroPages => "Intro pages",
            B::Curtain => "Curtain",
            B::StageMask => "Stage mask",
            B::FloatingText => "Floating text",
            B::Video => "Video",
            B::CameraEffect => "Camera effect",
            B::CameraTransform => "Camera transform",
            B::CameraShake => "Camera shake",
            B::StageAnimation => "Stage animation",
            B::BackgroundAnimation => "Background animation",
            B::BackgroundTransition => "Background transition",
            B::BackgroundTransform => "Background transform",
            B::BackgroundKeyframes => "Background keyframes",
            B::SpriteAnimation | B::SpriteTransform => "Sprite animation / transition",
        };
        if !reasons.iter().any(|existing| existing == label) {
            reasons.push(label.into());
        }
        std::ops::ControlFlow::<()>::Continue(())
    });
    if state.dialogue.is_some() && !blocked {
        reasons.push("Dialogue / advance input".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoint_and_failure_positions_survive_transient_presentation_and_title_cleanup() {
        let mut world = World::new();
        world.insert_resource(LocalAssetManifest(std::collections::HashMap::from([(
            "main".into(),
            super::super::super::resources::LocalSceneAssets {
                source_path: PathBuf::from("main.shou"),
                action_spans: vec![
                    keine_loader::SourceSpan { line: 2, column: 3 },
                    keine_loader::SourceSpan { line: 5, column: 3 },
                ],
                ..Default::default()
            },
        )])));
        let mut state = State::new();
        state.current_scene = "main".into();
        state.cursor = 1;
        let mut checkpoint = ContinuationCheckpoint::default();
        checkpoint.capture(&state);
        state.camera_shake = Some(keine_core::state::CameraShakeState::new(
            keine_core::CameraShakeSpec {
                amplitude: 8.,
                frequency: 12.,
                duration: 1.,
                axis: Default::default(),
                falloff: Default::default(),
            },
            Default::default(),
            1,
            false,
        ));
        state.cursor = 2;
        world.insert_resource(checkpoint);
        world.insert_resource(GameState(state));
        let snapshot = runtime_snapshot(&world);
        assert!(
            snapshot.waiting.is_empty(),
            "Non-blocking animation must not be called a wait"
        );
        assert!(!snapshot.checkpoint_is_live);
        assert_eq!(
            snapshot.checkpoint,
            Some(("scripts/main.shou".into(), 5, 3))
        );
        let mut state = world.resource_mut::<GameState>();
        state.install_program(keine_core::Program::from_scenes([(
            "main".into(),
            vec![keine_core::Action::Flow {
                action: Box::new(keine_core::Action::Comment),
                when: Some("missing_flag".into()),
                next: false,
            }],
        )]));
        state.cursor = 0;
        assert_eq!(
            keine_core::step::step_preserving_presentation(&mut state),
            keine_core::StepResult::RuntimeError(
                keine_core::step::ScriptRuntimeError::InvalidFlowCondition
            )
        );
        keine_core::step::end_game(&mut state);
        let snapshot = runtime_snapshot(&world);
        assert!(snapshot.checkpoint.is_none());
        let diagnostic = snapshot.diagnostic.unwrap();
        assert_eq!(
            (diagnostic.path, diagnostic.line, diagnostic.column),
            ("scripts/main.shou".into(), 2, 3)
        );
    }

    #[test]
    fn inspection_is_bounded_deterministic_and_keeps_waits_distinct_from_save_safety() {
        let mut world = World::new();
        let mut state = State::new();
        state.wait_blocking = true;
        state.wait_remaining = 2.;
        state.vars.insert("a".into(), Value::Str("中".repeat(500)));
        state
            .vars
            .insert("b".into(), Value::Array(vec![Value::Int(1); 1000]));
        for index in 0..200 {
            state.vars.insert(format!("z{index:03}"), Value::Int(index));
        }
        world.insert_resource(GameState(state));
        let snapshot = runtime_snapshot(&world);
        assert_eq!(snapshot.waiting, ["Timed wait"]);
        assert!(snapshot.checkpoint_is_live); // Timed waits are serializable.
        assert_eq!(snapshot.variables.len(), MAX_VARIABLES);
        assert_eq!(snapshot.omitted_variables, 74);
        assert!(snapshot.variables[0].1.len() <= MAX_VALUE_BYTES);
        assert_eq!(snapshot.variables[1].1, "Array [1000]");
        assert_eq!(snapshot, runtime_snapshot(&world));
        let message = keine_authoring::ServerMessage {
            generation: 1,
            request_id: 0,
            response: keine_authoring::ServerResponse::ExecutionLocation {
                document_revision: 1,
                location: None,
                snapshot,
            },
        };
        let mut encoded = Vec::new();
        keine_authoring::write_message(&mut encoded, &message).unwrap();
        assert!(encoded.len() < keine_authoring::MAX_MESSAGE_BYTES);
    }
}
