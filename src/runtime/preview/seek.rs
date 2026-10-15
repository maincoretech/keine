//! Deterministic state reconstruction for Editor source positions and reload.
use super::update_transitions;
use crate::runtime::resources::LocalAssetManifest;
use keine_core::{Program, State, step};

pub(crate) fn sync_editor_cursor(
    content: &keine_loader::ContentProject,
    state: &mut State,
    asset_manifest: &LocalAssetManifest,
) -> bool {
    match read_and_sync_editor_cursor(content, state, asset_manifest) {
        Ok(Some(changed)) => changed,
        Ok(None) => false,
        Err(error) => {
            log::warn!("failed to synchronize Studio debug position: {error:#}");
            false
        }
    }
}

#[cfg(feature = "hot-reload")]
pub(super) fn try_sync_editor_cursor(
    content: &keine_loader::ContentProject,
    state: &mut State,
    asset_manifest: &LocalAssetManifest,
    last: &mut Option<keine_loader::ProjectDebugCursor>,
    force: bool,
) -> anyhow::Result<Option<bool>> {
    let Some(cursor) = content.debug_cursor()? else {
        *last = None;
        return Ok(None);
    };
    if !force && last.as_ref() == Some(&cursor) {
        return Ok(Some(false));
    }
    let changed = sync_editor_cursor_at(content, state, asset_manifest, &cursor)?;
    *last = Some(cursor);
    Ok(Some(changed))
}

fn read_and_sync_editor_cursor(
    content: &keine_loader::ContentProject,
    state: &mut State,
    asset_manifest: &LocalAssetManifest,
) -> anyhow::Result<Option<bool>> {
    let Some(cursor) = content.debug_cursor()? else {
        return Ok(None);
    };
    Ok(Some(sync_editor_cursor_at(
        content,
        state,
        asset_manifest,
        &cursor,
    )?))
}

fn sync_editor_cursor_at(
    content: &keine_loader::ContentProject,
    state: &mut State,
    asset_manifest: &LocalAssetManifest,
    cursor: &keine_loader::ProjectDebugCursor,
) -> anyhow::Result<bool> {
    let initial = content.initial_state()?;
    Ok(sync_editor_position(
        state,
        asset_manifest,
        &cursor.scene,
        cursor.source_step,
        initial,
    ))
}

fn sync_editor_position(
    state: &mut State,
    asset_manifest: &LocalAssetManifest,
    scene_name: &str,
    source_step: usize,
    initial: keine_loader::ProjectInitialState,
) -> bool {
    let Some(scene) = asset_manifest.get(scene_name) else {
        log::warn!("editor selected unknown fragment {scene_name:?}");
        return false;
    };
    // A scene header names a fragment but is not itself an action. Replaying
    // zero actions would clear the visible stage when that header is selected.
    let source_step = scene
        .action_spans
        .first()
        .map_or(source_step, |first| source_step.max(first.line));
    let selected_start = scene
        .action_spans
        .iter()
        .position(|span| span.line >= source_step)
        .unwrap_or(scene.action_spans.len());
    let target = scene
        .action_spans
        .iter()
        .position(|span| span.line > source_step)
        .unwrap_or(scene.action_spans.len());
    let new_preview = || State {
        program: state.program.clone(),
        program_fingerprint: state.program_fingerprint,
        script_entry: state.script_entry.clone(),
        vars: initial
            .variables
            .iter()
            .chain(&initial.session_variables)
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
        session_variable_names: initial.session_variables.keys().cloned().collect(),
        global_vars: initial.shared_variables.clone(),
        ..State::new()
    };
    let mut preview = new_preview();
    preview.current_scene = crate::scene::entry_scene(&preview);
    preview.ended = false;

    // Editor previews reconstruct state from the project entry through the
    // selected block. Replaying only the selected fragment loses the scene,
    // characters and audio inherited from earlier chapters and is the reason
    // later chapters intermittently appeared to have missing resources.
    if !seek_editor_state(&mut preview, scene_name, selected_start, target) {
        // Some editor-only/title fragments are deliberately unreachable from
        // keine's normal entry. They still need direct block inspection.
        preview = new_preview();
        preview.current_scene = scene_name.to_owned();
        preview.ended = false;
        if !seek_editor_state(&mut preview, scene_name, selected_start, target) {
            return false;
        }
    }
    log::info!(
        "editor seek · fragment {} · block {}",
        scene_name,
        source_step
    );
    // Replay counters are local to reconstruction. Keep an unchanged track
    // playing across seeks, but allow explicitly selecting BGM to replay it.
    let selected_bgm = preview.program.scene(scene_name).is_some_and(|actions| {
        actions.get(selected_start..target).is_some_and(|actions| {
            actions.iter().any(|action| {
                matches!(
                    action,
                    keine_core::Action::Bgm { .. } | keine_core::Action::EiyashouBgm { .. }
                )
            })
        })
    });
    preview.bgm.revision = state.bgm.revision;
    if selected_bgm || preview.bgm != state.bgm {
        preview.bgm.revision = state.bgm.revision.wrapping_add(1);
    }
    preview.stage_revision = state.stage_revision.wrapping_add(1);
    *state = preview;
    true
}

pub(crate) fn sync_editor_source_position(
    content: &keine_loader::ContentProject,
    state: &mut State,
    asset_manifest: &LocalAssetManifest,
    source_path: &std::path::Path,
    source_line: usize,
) -> bool {
    let logical = source_path.strip_prefix("scripts").unwrap_or(source_path);
    let candidate = asset_manifest
        .iter()
        .filter(|(_, scene)| {
            scene.source_path == source_path
                || scene.source_path == logical
                || source_path.ends_with(&scene.source_path)
                || scene.source_path.ends_with(logical)
        })
        .filter_map(|(name, scene)| {
            let distance = scene
                .action_spans
                .iter()
                .map(|span| span.line.abs_diff(source_line))
                .min()?;
            Some((distance, name.as_str()))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, name)| name);
    let Some(scene_name) = candidate else {
        log::warn!(
            "editor selected source {} with no runtime scene",
            source_path.display()
        );
        return false;
    };
    let Ok(initial) = content.initial_state() else {
        return false;
    };
    sync_editor_position(state, asset_manifest, scene_name, source_line, initial)
}

const MAX_EDITOR_REPLAY_STEPS: usize = 65_536;
const MAX_EDITOR_PRESENTATION_STEPS: usize = 1_024;

pub(crate) fn seek_editor_state(
    preview: &mut State,
    target_scene: &str,
    selected_start: usize,
    target: usize,
) -> bool {
    let mut selected = false;
    for _ in 0..MAX_EDITOR_REPLAY_STEPS {
        if !selected && preview.current_scene == target_scene && preview.cursor >= selected_start {
            // Reconstruction restores persistent audio, not historical playback
            // events. Stop old one-shots/voice before previewing this block.
            preview.effect_queue.clear();
            preview.effect_queue.push(keine_core::EffectEvent::Stop);
            preview.vocal_event = Some(keine_core::VocalCue {
                file: None,
                volume: 1.0,
                fade_in: 0.0,
                fade_out: 0.0,
            });
            selected = true;
        }
        if selected && preview.current_scene == target_scene && preview.cursor >= target {
            return true;
        }
        let stop = if selected { target } else { selected_start };
        let result = step::step_until_cursor(preview, target_scene, stop);
        // A dialogue block may contain post-confirmation cleanup after `Say`
        // (for example LetsGal's `keepDialogue: false` textbox hide). Studio
        // selecting that block previews the line before confirmation, so do
        // not synthesize the click merely to reach the block's final action.
        if matches!(result, keine_core::StepResult::AwaitClick)
            && preview.current_scene == target_scene
            && preview.cursor > selected_start
            && preview.cursor <= target
        {
            if preview
                .vocal_event
                .as_ref()
                .is_some_and(|cue| cue.file.is_none())
                && let Some(dialogue) = &preview.dialogue
            {
                preview.vocal_event = Some(keine_core::VocalCue {
                    file: dialogue.vocal.clone(),
                    volume: dialogue.volume,
                    fade_in: 0.0,
                    fade_out: 0.0,
                });
            }
            return true;
        }
        // Replay prior blocks to completion, but preserve the selected block's
        // own yield. This lets its dialogue/typewriter, transition, timeline,
        // particle or video presentation run exactly once in the preview.
        if selected && preview.current_scene == target_scene && preview.cursor >= target {
            return true;
        }
        match result {
            keine_core::StepResult::AwaitClick => step::advance(preview),
            keine_core::StepResult::AwaitPresentation => {
                if !finish_editor_presentation(preview) {
                    log::warn!("editor seek could not finish a prior presentation");
                    return false;
                }
            }
            keine_core::StepResult::AwaitInput => {
                let _ = step::submit_user_input(preview);
            }
            keine_core::StepResult::AwaitChoice => {
                let direct = preview.menu.as_ref().and_then(|menu| {
                    menu.choices.iter().position(|choice| {
                        choice.enabled
                            && match &choice.target {
                                keine_core::ChoiceTarget::ChangeScene(scene)
                                | keine_core::ChoiceTarget::CallScene(scene) => {
                                    scene == target_scene
                                }
                                keine_core::ChoiceTarget::Label(_)
                                | keine_core::ChoiceTarget::Continue
                                | keine_core::ChoiceTarget::Assign(_) => false,
                            }
                    })
                });
                let fallback = preview
                    .menu
                    .as_ref()
                    .and_then(|menu| menu.choices.iter().position(|choice| choice.enabled));
                let Some(index) = direct.or(fallback) else {
                    return false;
                };
                step::select_choice(preview, index);
            }
            keine_core::StepResult::EndOfScene => {
                if !selected
                    && preview.current_scene == target_scene
                    && preview.cursor >= selected_start
                {
                    continue;
                }
                return preview.current_scene == target_scene && preview.cursor >= target;
            }
            keine_core::StepResult::ExecutionLimit => return false,
            keine_core::StepResult::RuntimeError(_) => return false,
        }
    }
    log::warn!("editor seek exceeded the deterministic replay limit");
    false
}

pub(super) fn finish_editor_presentation(preview: &mut State) -> bool {
    for _ in 0..MAX_EDITOR_PRESENTATION_STEPS {
        if !preview.presentation_blocked() {
            return true;
        }
        // A headless editor replay has no decoder callback that can finish a
        // blocking video. The selected block returns before this helper, so
        // only playback belonging to earlier source blocks is retired here.
        preview.videos.retain(|_, video| {
            !video.spec.wait_for_finished || video.spec.looped || video.stopping
        });
        // Prior sentence-tail deletions are deterministic editor history, so
        // finish them without manufacturing their separate player click.
        step::update_dialogue_retraction(preview, 0.0, 0.0, false, true);
        update_transitions(preview, 86_400.0, true);
    }
    !preview.presentation_blocked()
}

/// Re-enter one scene against the new Program without carrying presentation
/// or interaction state produced by the previous script fingerprint.
///
/// Native authoring rebuilds from project defaults at entry; changed programs
/// never reuse live variables as replay input. Legacy development reloads keep
/// their scene-local policy. Neither path migrates shipping save checkpoints.
pub(crate) fn restart_after_program_reload(
    state: &mut State,
    program: Program,
    fresh_preview: Option<(String, keine_loader::ProjectInitialState)>,
) {
    let next_stage_revision = state.stage_revision.wrapping_add(1);
    let previous_scene = state.current_scene.clone();
    let was_ended = state.ended;
    let script_entry = state.script_entry.clone();
    let vars = std::mem::take(&mut state.vars);
    let global_vars = std::mem::take(&mut state.global_vars);
    let unlocked_cg = std::mem::take(&mut state.unlocked_cg);
    let unlocked_bgm = std::mem::take(&mut state.unlocked_bgm);

    let mut restarted = State {
        script_entry,
        vars,
        global_vars,
        unlocked_cg,
        unlocked_bgm,
        ..State::new()
    };
    restarted.install_program(program);
    if let Some((entry, initial)) = fresh_preview {
        // Explicitly restart from the project entry, not a modified scene with
        // already-mutated variables. No cross-Program snapshot migration.
        restarted.script_entry = Some(entry);
        restarted.vars = initial.variables;
        restarted.session_variable_names = initial.session_variables.keys().cloned().collect();
        restarted.vars.extend(initial.session_variables);
        restarted.global_vars = initial.shared_variables;
        restarted.current_scene = crate::scene::entry_scene(&restarted);
        log::info!("preview source changed; restarted from project initial state");
    } else {
        restarted.current_scene = if restarted.program.contains_scene(&previous_scene) {
            previous_scene
        } else {
            crate::scene::entry_scene(&restarted)
        };
    }
    restarted.ended = was_ended || restarted.current_scene.is_empty();
    restarted.effect_queue.push(keine_core::EffectEvent::Stop);
    restarted.stage_revision = next_stage_revision;
    if !restarted.ended {
        crate::runtime::script_driver::resume_for_tooling(&mut restarted);
    }
    *state = restarted;
}

#[cfg(test)]
mod tests {
    use super::super::advance_camera_transitions;
    use super::*;
    use crate::runtime::resources::LocalSceneAssets;
    use bevy::prelude::*;
    use keine_core::{Action, SpriteTransform, Transition, Value, VideoMode, VideoSpec};

    #[test]
    fn editor_seek_discards_historical_sound_events_but_keeps_selected_audio() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([
            (
                "start".into(),
                vec![
                    Action::Bgm {
                        file: "music.WAV".into(),
                        volume: 0.5,
                        fade_seconds: 1.0,
                    },
                    Action::SoundEffect {
                        file: Some("old.wav".into()),
                        id: None,
                        volume: 1.0,
                        looped: false,
                        fade: 0.0,
                        fade_out: 0.0,
                    },
                    Action::SoundEffect {
                        file: Some("rain.wav".into()),
                        id: Some("rain".into()),
                        volume: 0.2,
                        looped: true,
                        fade: 1.0,
                        fade_out: 0.0,
                    },
                    Action::Say {
                        speaker: String::new(),
                        text: "old".into(),
                        options: keine_core::SayOptions {
                            vocal: Some("old-voice.wav".into()),
                            ..Default::default()
                        },
                    },
                    Action::ChangeScene("main".into()),
                ],
            ),
            (
                "main".into(),
                vec![
                    Action::SoundEffect {
                        file: Some("selected.wav".into()),
                        id: None,
                        volume: 1.0,
                        looped: false,
                        fade: 0.0,
                        fade_out: 0.0,
                    },
                    Action::Say {
                        speaker: String::new(),
                        text: "selected".into(),
                        options: Default::default(),
                    },
                    Action::Say {
                        speaker: String::new(),
                        text: "voiced".into(),
                        options: keine_core::SayOptions {
                            vocal: Some("selected-voice.wav".into()),
                            ..Default::default()
                        },
                    },
                ],
            ),
        ]));
        state.current_scene = "start".into();
        state.ended = false;
        let initial = state.clone();

        assert!(seek_editor_state(&mut state, "main", 1, 2));
        assert_eq!(state.effect_queue, vec![keine_core::EffectEvent::Stop]);
        assert_eq!(state.vocal_event.as_ref().unwrap().file, None);
        assert_eq!(state.bgm.file.as_deref(), Some("music.WAV"));
        assert_eq!(state.looping_effects["rain"].file, "rain.wav");

        state = initial.clone();
        assert!(seek_editor_state(&mut state, "main", 0, 1));
        assert!(
            matches!(state.effect_queue.as_slice(), [keine_core::EffectEvent::Stop, keine_core::EffectEvent::Play(cue)] if cue.file == "selected.wav")
        );

        state = initial;
        assert!(seek_editor_state(&mut state, "main", 2, 3));
        assert_eq!(state.effect_queue, vec![keine_core::EffectEvent::Stop]);
        assert_eq!(
            state.vocal_event.as_ref().unwrap().file.as_deref(),
            Some("selected-voice.wav")
        );
    }

    #[test]
    fn editor_seek_keeps_resources_inherited_from_an_earlier_fragment() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([
            (
                "start".into(),
                vec![
                    Action::ShowBg {
                        image: "backgrounds/inherited.webp".into(),
                        transition: Transition::Instant,
                        transform: SpriteTransform::default(),
                    },
                    Action::ChangeScene("chapter-two".into()),
                ],
            ),
            (
                "chapter-two".into(),
                vec![Action::Say {
                    speaker: String::new(),
                    text: "continued".into(),
                    options: Default::default(),
                }],
            ),
        ]));
        state.current_scene = "start".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "chapter-two", 0, 0));
        assert_eq!(state.current_scene, "chapter-two");
        assert_eq!(state.bg.as_deref(), Some("backgrounds/inherited.webp"));
    }

    #[test]
    fn editor_seek_keeps_the_selected_dialogue_visible_after_replay() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Say {
                speaker: "小夜".into(),
                text: "被选中的对白".into(),
                options: Default::default(),
            }],
        )]));
        state.current_scene = "main".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "main", 0, 1));
        let retained = state
            .dialogue
            .as_ref()
            .expect("selected dialogue must remain at its native yield");
        assert_eq!(retained.speaker, "小夜");
        assert_eq!(retained.text, "被选中的对白");
        assert!(!state.textbox_hidden);
    }

    #[test]
    fn editor_seek_retires_a_prior_blocking_video_without_spinning() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::PlayVideo {
                    video: VideoSpec {
                        id: "opening".into(),
                        file: "opening.mp4".into(),
                        looped: false,
                        muted: false,
                        alpha: 1.0,
                        skippable: true,
                        wait_for_finished: true,
                        mode: VideoMode::Fullscreen,
                    },
                },
                Action::Say {
                    speaker: "小夜".into(),
                    text: "视频之后".into(),
                    options: Default::default(),
                },
            ],
        )]));
        state.current_scene = "main".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "main", 1, 2));
        assert!(state.videos.is_empty());
        assert_eq!(
            state
                .dialogue
                .as_ref()
                .map(|dialogue| dialogue.text.as_str()),
            Some("视频之后")
        );
    }

    #[test]
    fn editor_seek_keeps_the_bus_dialogue_shaking_at_its_authored_strength() {
        let scenes = keine_loader::parse_native_scenes(
            "scene main { camera.move(all, shake: shake(amplitude: 1.6, frequency: 1.15, duration: 600000ms, amplitude_randomness: 0.8, frequency_randomness: 0.7), tween: [shake_amplitude, shake_frequency], duration: 600000ms, blocking: false), \"公交车的路程并不远\" }",
        );
        assert!(scenes[0].report.diagnostics.is_empty());
        let mut state = State::new();
        state.install_program(Program::from_scenes(
            scenes
                .into_iter()
                .map(|scene| (scene.name.unwrap(), scene.report.actions)),
        ));
        state.current_scene = "main".into();
        state.ended = false;
        assert!(seek_editor_state(&mut state, "main", 1, 2));
        assert_eq!(state.dialogue.as_ref().unwrap().text, "公交车的路程并不远");
        assert!(advance_camera_transitions(&mut state, 0.25));
        let shake = state.camera_shake.as_ref().unwrap();
        assert_eq!((shake.spec.amplitude, shake.spec.frequency), (1.6, 1.15));
        assert!(shake.offset_x.abs().max(shake.offset_y.abs()) > 0.1);
        assert!(!state.presentation_blocked());
    }

    #[test]
    fn editor_seek_preserves_the_selected_block_animation() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::ShowBg {
                image: "backgrounds/selected.webp".into(),
                transition: Transition::Fade(1.0),
                transform: SpriteTransform::default(),
            }],
        )]));
        state.current_scene = "main".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "main", 0, 1));
        let transition = state
            .bg_transition
            .as_ref()
            .expect("selected transition must not be fast-forwarded");
        assert_eq!(transition.progress, 0.0);
        assert!(transition.blocking);
    }

    #[test]
    fn editor_seek_preserves_a_selected_dialogue_retraction() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::Say {
                    speaker: "小夜".into(),
                    text: "我当然来了".into(),
                    options: Default::default(),
                },
                Action::RetractDialogue {
                    source: "我当然来了".into(),
                    keep: "我当然".into(),
                },
            ],
        )]));
        state.current_scene = "main".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "main", 1, 2));
        assert_eq!(
            state.dialogue.as_ref().map(|line| line.text.as_str()),
            Some("我当然来了")
        );
        assert!(state.dialogue_retraction.is_some());
    }

    #[test]
    fn editor_seek_completes_prior_retractions_before_the_selected_block() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::Say {
                    speaker: "小夜".into(),
                    text: "我当然来了".into(),
                    options: Default::default(),
                },
                Action::RetractDialogue {
                    source: "我当然来了".into(),
                    keep: "我当然".into(),
                },
                Action::Say {
                    speaker: "小夜".into(),
                    text: "下一句".into(),
                    options: Default::default(),
                },
            ],
        )]));
        state.current_scene = "main".into();
        state.ended = false;

        assert!(seek_editor_state(&mut state, "main", 2, 3));
        assert_eq!(
            state.dialogue.as_ref().map(|line| line.text.as_str()),
            Some("下一句")
        );
        assert!(state.dialogue_retraction.is_none());
    }

    #[test]
    fn editor_seek_rebuilds_variables_from_adapter_defaults() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "start".into(),
            vec![Action::Say {
                speaker: String::new(),
                text: "{route}/{ending}".into(),
                options: Default::default(),
            }],
        )]));
        state
            .vars
            .insert("route".into(), Value::Str("stale".into()));
        state.global_vars.insert("ending".into(), Value::Int(99));

        let manifest = LocalAssetManifest(std::collections::HashMap::from([(
            "start".into(),
            LocalSceneAssets {
                action_spans: vec![keine_loader::SourceSpan { line: 1, column: 1 }],
                ..default()
            },
        )]));
        let initial = keine_loader::ProjectInitialState {
            variables: std::collections::HashMap::from([(
                "route".into(),
                Value::Str("fresh".into()),
            )]),
            session_variables: std::collections::HashMap::new(),
            shared_variables: std::collections::HashMap::from([("ending".into(), Value::Int(2))]),
        };

        assert!(sync_editor_position(
            &mut state, &manifest, "start", 1, initial,
        ));
        assert_eq!(state.dialogue.as_ref().unwrap().text, "fresh/2");
        assert_eq!(state.vars["route"], Value::Str("fresh".into()));
        assert_eq!(state.global_vars["ending"], Value::Int(2));
    }

    #[test]
    fn editor_seek_keeps_bgm_playing_except_when_selecting_its_block() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::Bgm {
                    file: "music.WAV".into(),
                    volume: 0.5,
                    fade_seconds: 1.0,
                },
                Action::Say {
                    speaker: String::new(),
                    text: "first".into(),
                    options: Default::default(),
                },
                Action::Say {
                    speaker: String::new(),
                    text: "second".into(),
                    options: Default::default(),
                },
            ],
        )]));
        state.script_entry = Some("main".into());
        let manifest = LocalAssetManifest(std::collections::HashMap::from([(
            "main".into(),
            LocalSceneAssets {
                action_spans: (1..=3)
                    .map(|line| keine_loader::SourceSpan { line, column: 1 })
                    .collect(),
                ..default()
            },
        )]));
        assert!(sync_editor_position(
            &mut state,
            &manifest,
            "main",
            2,
            Default::default()
        ));
        let revision = state.bgm.revision;
        assert!(sync_editor_position(
            &mut state,
            &manifest,
            "main",
            3,
            Default::default()
        ));
        assert_eq!(state.bgm.revision, revision);
        assert!(sync_editor_position(
            &mut state,
            &manifest,
            "main",
            1,
            Default::default()
        ));
        assert_eq!(state.bgm.revision, revision.wrapping_add(1));
    }

    #[test]
    fn editor_scene_header_previews_its_first_action() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "opening".into(),
            vec![
                Action::ShowBg {
                    image: "day.webp".into(),
                    transition: Transition::Instant,
                    transform: SpriteTransform::default(),
                },
                Action::Say {
                    speaker: String::new(),
                    text: "Welcome".into(),
                    options: Default::default(),
                },
            ],
        )]));
        let manifest = LocalAssetManifest(std::collections::HashMap::from([(
            "opening".into(),
            LocalSceneAssets {
                action_spans: vec![
                    keine_loader::SourceSpan { line: 2, column: 1 },
                    keine_loader::SourceSpan { line: 3, column: 1 },
                ],
                ..default()
            },
        )]));

        assert!(sync_editor_position(
            &mut state,
            &manifest,
            "opening",
            1,
            keine_loader::ProjectInitialState::default(),
        ));
        assert_eq!(state.bg.as_deref(), Some("day.webp"));
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn editor_seek_rejects_a_block_that_cannot_be_replayed() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([
            (
                "main".into(),
                vec![
                    Action::ChangeScene("end".into()),
                    Action::Say {
                        speaker: String::new(),
                        text: "unreachable".into(),
                        options: Default::default(),
                    },
                ],
            ),
            ("end".into(), vec![Action::End]),
        ]));
        state.script_entry = Some("main".into());
        state.current_scene = "main".into();
        state.ended = false;
        let manifest = LocalAssetManifest(std::collections::HashMap::from([(
            "main".into(),
            LocalSceneAssets {
                action_spans: vec![
                    keine_loader::SourceSpan { line: 1, column: 1 },
                    keine_loader::SourceSpan { line: 2, column: 1 },
                ],
                ..default()
            },
        )]));

        assert!(!sync_editor_position(
            &mut state,
            &manifest,
            "main",
            2,
            keine_loader::ProjectInitialState::default(),
        ));
        assert_eq!(state.current_scene, "main");
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn editor_seek_does_not_confirm_the_selected_dialogue_cleanup() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![
                Action::Say {
                    speaker: String::new(),
                    text: "应当显示的旁白".into(),
                    options: Default::default(),
                },
                Action::SetTextbox {
                    visible: false,
                    auto: true,
                },
            ],
        )]));
        let manifest = LocalAssetManifest(std::collections::HashMap::from([(
            "main".into(),
            LocalSceneAssets {
                action_spans: vec![
                    keine_loader::SourceSpan { line: 1, column: 1 },
                    keine_loader::SourceSpan { line: 1, column: 1 },
                ],
                ..default()
            },
        )]));

        assert!(sync_editor_position(
            &mut state,
            &manifest,
            "main",
            1,
            keine_loader::ProjectInitialState::default(),
        ));
        assert_eq!(
            state
                .dialogue
                .as_ref()
                .map(|dialogue| dialogue.text.as_str()),
            Some("应当显示的旁白")
        );
        assert!(!state.textbox_hidden);
    }

    #[test]
    fn fresh_preview_reload_does_not_apply_side_effects_twice() {
        let program = Program::from_scenes([(
            "main".into(),
            vec![
                Action::Set {
                    name: "counter".into(),
                    expression: "counter + 1".into(),
                    global: false,
                },
                Action::Set {
                    name: "total".into(),
                    expression: "total + 1".into(),
                    global: true,
                },
                Action::Say {
                    speaker: String::new(),
                    text: "line".into(),
                    options: Default::default(),
                },
            ],
        )]);
        let initial = keine_loader::ProjectInitialState {
            variables: std::collections::HashMap::from([("counter".into(), Value::Int(0))]),
            shared_variables: std::collections::HashMap::from([("total".into(), Value::Int(10))]),
            ..Default::default()
        };
        let mut state = State::new();
        for _ in 0..2 {
            restart_after_program_reload(
                &mut state,
                program.clone(),
                Some(("main".into(), initial.clone())),
            );
            assert_eq!(state.vars.get("counter"), Some(&Value::Int(1)));
            assert_eq!(state.global_vars.get("total"), Some(&Value::Int(11)));
            assert_eq!(state.backlog.len(), 1);
            assert_eq!(state.effect_queue, [keine_core::EffectEvent::Stop]);
        }
    }

    #[test]
    #[cfg(feature = "hot-reload")]
    fn program_reload_rebuilds_interaction_state_from_the_new_scene() {
        let mut state = State::new();
        state.install_program(Program::from_scenes([(
            "main".into(),
            vec![Action::Say {
                speaker: "old".into(),
                text: "old line".into(),
                options: Default::default(),
            }],
        )]));
        state.current_scene = "main".into();
        state.vars.insert("route".into(), Value::Str("kept".into()));
        state.global_vars.insert("chapter".into(), Value::Int(2));
        state.unlocked_cg.insert("old.webp".into(), "Old".into());
        assert_eq!(step::step(&mut state), keine_core::StepResult::AwaitClick);
        state.record_dialogue(0);
        state.mark_current_dialogue_read();

        restart_after_program_reload(
            &mut state,
            Program::from_scenes([(
                "main".into(),
                vec![
                    Action::ShowBg {
                        image: "new.webp".into(),
                        transition: Transition::Instant,
                        transform: SpriteTransform::default(),
                    },
                    Action::Say {
                        speaker: "new".into(),
                        text: "new line".into(),
                        options: Default::default(),
                    },
                ],
            )]),
            None,
        );

        assert_eq!(state.current_scene, "main");
        assert_eq!(state.bg.as_deref(), Some("new.webp"));
        assert_eq!(
            state.dialogue.as_ref().map(|line| line.text.as_str()),
            Some("new line")
        );
        assert_eq!(state.vars.get("route"), Some(&Value::Str("kept".into())));
        assert!(state.global_vars.contains_key("chapter"));
        assert!(state.unlocked_cg.contains_key("old.webp"));
        assert!(state.scene_stack.is_empty());
        assert_eq!(state.backlog.len(), 1);
        assert_eq!(state.backlog[0].text, "new line");
        assert_eq!(
            state.backlog[0].snapshot.program_fingerprint,
            state.program_fingerprint
        );
        assert!(state.read_dialogues.is_empty());
        assert_eq!(state.effect_queue, [keine_core::EffectEvent::Stop]);
    }
}
