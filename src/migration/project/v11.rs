//! Emit the approved dotted author commands, never opaque serialized actions.
use super::*;
use keine_core::{CameraTargets, ColorToneMode, DialogueStyle, TransformPatch, Value};

pub(super) fn value(value: &Value) -> Result<String> {
    Ok(match value {
        Value::Int(value) => value.to_string(),
        Value::Float(value) if value.is_finite() => {
            let text = value.to_string();
            if text.contains(['.', 'e', 'E']) {
                text
            } else {
                format!("{text}.0")
            }
        }
        Value::Str(value) => string_literal(value),
        Value::Bool(value) => value.to_string(),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(self::value)
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        ),
        _ => bail!("non-finite initial variable cannot be migrated"),
    })
}

pub(super) fn render(action: &Action, model: &MigrationModel) -> Result<Option<String>> {
    if let Action::Flow {
        action,
        when: None,
        next,
    } = action
    {
        if !next {
            return super::render_action(action, model).map(Some);
        }
        let mut inner = action.as_ref().clone();
        match &mut inner {
            Action::SetCameraTransform { blocking, .. }
            | Action::SetPostProcess { blocking, .. }
            | Action::SetPostProcessV2 { blocking, .. }
            | Action::ShakeCamera { blocking, .. }
            | Action::ShakeCameraRandomized { blocking, .. } => *blocking = false,
            Action::SetCameraTween { spec } => spec.blocking = false,
            Action::ShowBg { .. }
            | Action::HideBg { .. }
            | Action::ShowSprite { .. }
            | Action::HideSprite { .. }
            | Action::HideSprites { .. } => {
                let mut source = super::render_action(action, model)?;
                source.pop();
                source.push_str(", blocking: false)");
                return Ok(Some(source));
            }
            _ => bail!("parallel compatibility action cannot be represented: {inner:?}"),
        }
        return super::render_action(&inner, model).map(Some);
    }
    let normalized = normalize_camera(action);
    let action = normalized.as_ref();
    let source = match action {
        Action::Flow {
            action,
            when: Some(condition),
            next: false,
        } => format!(
            "if ({}) {{ {} }}",
            expression_source(condition, model)?,
            super::render_action(action, model)?
        ),
        Action::SelectSpriteImage {
            id,
            variable,
            default_image,
            variants,
        } => format!(
            "sprite.select({}, {}, default: {}) {{ {} }}",
            object_id(model, id)?,
            model
                .variable_ids
                .get(variable)
                .context("missing selector variable mapping")?,
            asset_id(model, ResourceKind::Figure, default_image)?,
            variants
                .iter()
                .map(|(value, image)| Ok(format!(
                    "case({}, {})",
                    string_literal(value),
                    asset_id(model, ResourceKind::Figure, image)?
                )))
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        ),
        Action::SelectSpriteImageByCondition {
            id,
            default_image,
            variants,
        } => format!(
            "sprite.select.when({}, default: {}) {{ {} }}",
            object_id(model, id)?,
            asset_id(model, ResourceKind::Figure, default_image)?,
            variants
                .iter()
                .map(|(condition, image)| Ok(format!(
                    "case({}, {})",
                    expression_source(condition, model)?,
                    asset_id(model, ResourceKind::Figure, image)?
                )))
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        ),
        Action::EiyashouBgm {
            file,
            volume,
            fade_seconds,
            looped,
        } => format!(
            "bgm({}, volume: {}, fade: {}, loop: {})",
            file.as_ref()
                .map(|file| asset_id(model, ResourceKind::Bgm, file))
                .transpose()?
                .unwrap_or("none".into()),
            number(*volume),
            duration(*fade_seconds),
            looped
        ),
        Action::WaitForAdvance => "wait.advance()".into(),
        Action::RetractDialogue { source, keep } => format!(
            "text.retract(source: {}, keep: {})",
            string_literal(source),
            string_literal(keep)
        ),
        Action::SoundEffect {
            file,
            id,
            volume,
            looped,
            fade,
        } => match file {
            Some(file) if *looped => format!(
                "se.loop({}, {}, volume: {}, fade: {})",
                object_id(
                    model,
                    id.as_deref().context("looping effect requires an ID")?
                )?,
                asset_id(model, ResourceKind::Effect, file)?,
                number(*volume),
                duration(*fade)
            ),
            Some(file) => format!(
                "se({}{}, volume: {}, fade: {})",
                asset_id(model, ResourceKind::Effect, file)?,
                id.as_ref()
                    .map(|id| object_id(model, id).map(|id| format!(", id: {id}")))
                    .transpose()?
                    .unwrap_or_default(),
                number(*volume),
                duration(*fade)
            ),
            None => format!(
                "se.stop({}, fade: {})",
                id.as_ref()
                    .map(|id| object_id(model, id))
                    .transpose()?
                    .unwrap_or("*"),
                duration(*fade)
            ),
        },
        Action::Effect {
            file,
            volume,
            id: Some(id),
        } => match file {
            Some(file) => format!(
                "se.loop({}, {}, volume: {})",
                object_id(model, id)?,
                asset_id(model, ResourceKind::Effect, file)?,
                number(*volume)
            ),
            None => format!("se.stop({})", object_id(model, id)?),
        },
        Action::SetTransform {
            id,
            transform,
            duration: seconds,
            easing,
        } => format!(
            "sprite.transform({}{}, duration: {}, easing: {})",
            object_id(model, id)?,
            patch_fields(*transform),
            duration(*seconds),
            easing_name(*easing)
        ),
        Action::MoveSprite {
            id,
            position,
            duration: seconds,
            easing,
            blocking,
        } => format!(
            "move({}, {}{})",
            object_id(model, id)?,
            position_fields(*position).trim_start_matches("position: "),
            timing(*seconds, *easing, *blocking)
        ),
        Action::UpdateSprite {
            id,
            image,
            position,
            layout,
            scale,
            duration: seconds,
            easing,
            blocking,
        } => format!(
            "sprite.update({}, {}, {}{}, scale: {}{})",
            object_id(model, id)?,
            asset_id(model, ResourceKind::Figure, image)?,
            position_fields(*position),
            explicit_layout_fields(*layout),
            number(*scale),
            timing(*seconds, *easing, *blocking)
        ),
        Action::PatchSprite {
            id,
            image,
            position,
            layout,
            scale,
            duration: seconds,
            easing,
            blocking,
        } => {
            let mut fields = String::new();
            if let Some(position) = position {
                fields.push_str(&format!(", {}", position_fields(*position)));
            }
            if let Some(layout) = layout {
                fields.push_str(&explicit_layout_fields(*layout));
            }
            if let Some(scale) = scale {
                fields.push_str(&format!(", scale: {}", number(*scale)));
            }
            format!(
                "sprite.update({}, {}{}{})",
                object_id(model, id)?,
                asset_id(model, ResourceKind::Figure, image)?,
                fields,
                timing(*seconds, *easing, *blocking)
            )
        }
        Action::SetFilter { target, filter } => format!(
            "sprite.transform({}, blur: {}, brightness: {}, contrast: {}, saturation: {})",
            object_id(model, target)?,
            number(filter.blur),
            number(filter.brightness),
            number(filter.contrast),
            number(filter.saturation)
        ),
        Action::UserInput {
            variable,
            title,
            button,
        } => format!(
            "input.request({}, title: {}, confirm_text: {})",
            model
                .variable_ids
                .get(variable)
                .context("missing input variable mapping")?,
            string_literal(title),
            string_literal(button)
        ),
        Action::ConfigureSpriteSequence {
            id,
            frames,
            fps,
            looped,
        } => format!(
            "sprite.sequence({}, fps: {}, loop: {}) {{ {} }}",
            object_id(model, id)?,
            number(*fps),
            looped,
            frames
                .iter()
                .map(|frame| Ok(format!(
                    "frame({})",
                    asset_id(model, ResourceKind::Figure, frame)?
                )))
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        ),
        Action::ConfigureTimedSpriteSequence {
            id,
            frames,
            frame_durations,
            looped,
        } => {
            if frames.len() != frame_durations.len() {
                bail!("sequence duration count differs from frame count");
            }
            format!(
                "sprite.sequence({}, loop: {}) {{ {} }}",
                object_id(model, id)?,
                looped,
                frames
                    .iter()
                    .zip(frame_durations)
                    .map(|(frame, seconds)| Ok(format!(
                        "frame({}, duration: {})",
                        asset_id(model, ResourceKind::Figure, frame)?,
                        duration(*seconds)
                    )))
                    .collect::<Result<Vec<_>>>()?
                    .join(", ")
            )
        }
        Action::ShowBg {
            image,
            transition,
            transform,
        } => format!(
            "background({}{}{})",
            asset_id(model, ResourceKind::Background, image)?,
            transition_arg(*transition),
            full_transform(transform)
        ),
        Action::ShowSprite {
            id,
            image,
            position,
            layout,
            transition,
            transform,
            z_index,
            blend,
        } => format!(
            "sprite({}, {}, {}{}{}, z: {}, blend: {}{})",
            object_id(model, id)?,
            asset_id(model, ResourceKind::Figure, image)?,
            position_fields(*position),
            layout_fields(*layout),
            transition_arg(*transition),
            z_index,
            enum_name(blend)?,
            full_transform(transform)
        ),
        Action::HideSprites { prefix, transition } => format!(
            "hide({}{})",
            if prefix.is_empty() {
                "*".into()
            } else {
                format!(
                    "{}*",
                    model
                        .prefix_ids
                        .get(prefix)
                        .context("missing sprite prefix mapping")?
                )
            },
            transition_arg(*transition)
        ),
        Action::Set {
            name,
            expression,
            global: false,
        } => format!(
            "{} = {}",
            model
                .variable_ids
                .get(name)
                .context("missing variable mapping")?,
            expression_source(expression, model)?
        ),
        Action::SetTextbox { visible, auto } => {
            format!("text.box(visible: {visible}, auto: {auto})")
        }
        Action::FocusPortrait { speaker_id } => format!(
            "sprite.focus({})",
            speaker_id
                .as_deref()
                .map(|id| object_id(model, id))
                .transpose()?
                .unwrap_or("none")
        ),
        Action::ConfigurePortraits {
            enabled,
            character_ids,
            speaking,
            others,
            narration,
            duration: seconds,
            easing,
        } => format!(
            "sprite.focus.configure(enabled: {enabled}, characters: [{}], speaking: style({}), others: style({}), narration: style({}), duration: {}, easing: {})",
            character_ids
                .iter()
                .map(|id| object_id(model, id))
                .collect::<Result<Vec<_>>>()?
                .join(", "),
            fields(speaking)?,
            fields(others)?,
            fields(narration)?,
            duration(*seconds),
            easing_name(*easing)
        ),
        Action::SetDialogueStyle { style } => format!(
            "text.style({})",
            string_literal(match style {
                DialogueStyle::Default => "default",
                DialogueStyle::Cinematic => "cinematic",
                DialogueStyle::CinematicCentered => "cinematic-centered",
                DialogueStyle::Literary => "literary",
                DialogueStyle::Sharp => "sharp",
                DialogueStyle::Handwritten => "handwritten",
                DialogueStyle::Custom(id) => id,
            })
        ),
        Action::Curtain {
            visible,
            color,
            duration: seconds,
        } => format!(
            "screen.curtain.{}(color: {}, duration: {})",
            if *visible { "show" } else { "hide" },
            rgba(color),
            duration(*seconds)
        ),
        Action::ConfigureSceneMouseParallax { parallax } => match parallax {
            Some(parallax) => format!("scene.parallax({})", fields(parallax)?),
            None => "scene.parallax.stop()".into(),
        },
        Action::HideParticleLayers => "particle.layers.clear()".into(),
        Action::MiniAvatar { image } => format!(
            "avatar.show({})",
            asset_id(model, ResourceKind::MiniAvatar, image)?
        ),
        Action::HideMiniAvatar => "avatar.hide()".into(),
        Action::ConditionalCall {
            condition,
            then_scene,
            else_scene,
        } => {
            let otherwise = else_scene
                .as_deref()
                .map(|scene| -> Result<String> {
                    Ok(format!(" else {{ call({}) }}", scene_id(model, scene)?))
                })
                .transpose()?
                .unwrap_or_default();
            format!(
                "if ({}) {{ call({}) }}{otherwise}",
                expression_source(condition, model)?,
                scene_id(model, then_scene)?
            )
        }
        Action::ConfigureDynamicSpriteSequence {
            id,
            frames,
            fps,
            frame_durations,
            playback,
        } => {
            let mode = match playback {
                keine_core::SequencePlayback::Blink { interval } => {
                    format!("mode: blink, interval: {}", duration(*interval))
                }
                keine_core::SequencePlayback::Talk { speaker } => {
                    format!("mode: talk, speaker: {}", string_literal(speaker))
                }
            };
            let timing = if frame_durations.is_empty() {
                format!(", fps: {}", number(*fps))
            } else {
                String::new()
            };
            let rows = frames
                .iter()
                .enumerate()
                .map(|(index, frame)| {
                    let time = frame_durations
                        .get(index)
                        .map(|seconds| format!(", duration: {}", duration(*seconds)))
                        .unwrap_or_default();
                    Ok(format!(
                        "frame({}{time})",
                        asset_id(model, ResourceKind::Figure, frame)?
                    ))
                })
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            format!(
                "sprite.sequence({}, {mode}{timing}) {{ {rows} }}",
                object_id(model, id)?
            )
        }
        Action::ShowParticlesWithOptions {
            id,
            effect,
            options,
        } => {
            let mut source = render(
                &Action::ShowParticles {
                    id: id.clone(),
                    effect: effect.clone(),
                },
                model,
            )?
            .expect("particle command");
            source.pop();
            for (name, value) in [
                ("size", options.size),
                ("speed", options.speed),
                ("alpha", options.alpha),
                ("spin", options.spin),
                ("drift", options.drift),
                ("drag", options.drag),
            ] {
                if let Some(value) = value {
                    source.push_str(&format!(", {name}: {}", number(value)));
                }
            }
            if let Some(value) = options.color {
                source.push_str(&format!(", color: {}", rgba(&value)));
            }
            source.push(')');
            source
        }
        Action::ShowParticles { id, effect } => {
            let mut args = format!(
                "{}, {}, count: {}, fade_in: {}",
                object_id(model, id)?,
                super::native_identifier(&effect.preset)?,
                effect.count,
                duration(effect.fade_in)
            );
            if let Some(texture) = &effect.texture {
                args.push_str(&format!(
                    ", texture: {}",
                    asset_id(model, ResourceKind::Particle, texture)?
                ));
            }
            if let Some(wind) = effect.wind {
                args.push_str(&format!(", wind: {}", number(wind)));
            }
            if let Some(gravity) = effect.gravity {
                args.push_str(&format!(", gravity: {}", number(gravity)));
            }
            format!("particle.show({args})")
        }
        Action::HideParticles {
            id,
            duration: seconds,
        } => format!(
            "particle.hide({}, duration: {})",
            id.as_deref()
                .map(|id| object_id(model, id))
                .transpose()?
                .unwrap_or("*"),
            duration(*seconds)
        ),
        Action::SetCameraBinding {
            target,
            bound,
            distance,
        } => format!(
            "camera.{}({}, distance: {})",
            if *bound { "bind" } else { "unbind" },
            object_id(model, target)?,
            number(*distance)
        ),
        Action::SetCameraTransform {
            targets,
            transform,
            duration: seconds,
            easing,
            blocking,
        } => format!(
            "camera.move({}{}{})",
            camera_target(*targets),
            patch_fields(*transform),
            timing(*seconds, *easing, *blocking)
        ),
        Action::ShakeCamera {
            targets,
            shake,
            blocking,
        } => format!(
            "camera.shake({}, amplitude: {}, frequency: {}, duration: {}, axis: {}, falloff: {}, blocking: {})",
            camera_target(*targets),
            number(shake.amplitude),
            number(shake.frequency),
            duration(shake.duration),
            enum_name(&shake.axis)?,
            enum_name(&shake.falloff)?,
            blocking
        ),
        Action::SetPostProcess {
            targets,
            effect,
            duration: seconds,
            easing,
            blocking,
        } => format!(
            "camera.effect({}{}{})",
            camera_target(*targets),
            effect_fields(effect, model)?,
            timing(*seconds, *easing, *blocking)
        ),
        Action::SetPostProcessV2 {
            targets,
            effect,
            duration: seconds,
            easing,
            blocking,
        } => format!(
            "camera.effect({}, {}{})",
            camera_target(*targets),
            fields(effect)?,
            timing(*seconds, *easing, *blocking)
        ),
        Action::SetCameraTween { spec } => {
            let mut source = match (&spec.transform, &spec.effect, &spec.v2) {
                (Some(transform), None, None) => format!(
                    "camera.move({}{}",
                    camera_target(spec.targets),
                    patch_fields(*transform)
                ),
                (None, Some(effect), None) => format!(
                    "camera.effect({}{}",
                    camera_target(spec.targets),
                    effect_fields(effect, model)?
                ),
                (None, None, Some(effect)) => format!(
                    "camera.effect({}, {}",
                    camera_target(spec.targets),
                    fields(effect)?
                ),
                _ => {
                    let mut source = format!("camera.move({}", camera_target(spec.targets));
                    if let Some(transform) = spec.transform {
                        source.push_str(&patch_fields(transform));
                    }
                    if let Some(effect) = &spec.effect {
                        source.push_str(&effect_fields(effect, model)?);
                    }
                    if let Some(effect) = &spec.v2 {
                        source.push_str(&format!(", {}", fields(effect)?));
                    }
                    if spec.transform.is_none()
                        && spec.effect.is_none()
                        && spec.v2.is_none()
                        && spec.shake.is_none()
                    {
                        bail!("empty camera tween requires manual migration");
                    }
                    source
                }
            };
            if let Some(value) = spec.shake {
                // A single native command retains simultaneous camera/shake changes.
                // camera.effect only accepts post-process fields.
                if source.starts_with("camera.effect(") {
                    source = source.replacen("camera.effect(", "camera.move(", 1);
                }
                source.push_str(&format!(
                    ", shake: shake(amplitude: {}, frequency: {}, duration: {}, axis: {}, falloff: {}, amplitude_randomness: {}, frequency_randomness: {})",
                    number(value.shake.amplitude), number(value.shake.frequency), duration(value.shake.duration),
                    enum_name(&value.shake.axis)?, enum_name(&value.shake.falloff)?,
                    number(value.randomness.amplitude), number(value.randomness.frequency),
                ));
            }
            format!(
                "{source}, tween: [{}]{})",
                spec.fields
                    .iter()
                    .map(|field| field.name())
                    .collect::<Vec<_>>()
                    .join(", "),
                timing(spec.duration, spec.easing, spec.blocking)
            )
        }
        Action::ShakeCameraRandomized {
            targets,
            shake,
            randomness,
            blocking,
        } => format!(
            "camera.shake({}, amplitude: {}, frequency: {}, amplitude_randomness: {}, frequency_randomness: {}, duration: {}, axis: {}, falloff: {}, blocking: {})",
            camera_target(*targets),
            number(shake.amplitude),
            number(shake.frequency),
            number(randomness.amplitude),
            number(randomness.frequency),
            duration(shake.duration),
            enum_name(&shake.axis)?,
            enum_name(&shake.falloff)?,
            blocking
        ),
        Action::FilmMode { enabled } => format!("screen.film({enabled})"),
        Action::SetSystemUi { slot, visible } => format!(
            "ui.{}({})",
            if *visible { "show" } else { "hide" },
            enum_name(slot)?
        ),
        Action::Unlock { kind, file, name } => {
            let (kind, resource) = match kind {
                keine_core::UnlockKind::Cg => ("cg", ResourceKind::Background),
                keine_core::UnlockKind::Bgm => ("bgm", ResourceKind::Bgm),
            };
            format!(
                "gallery.unlock({kind}, {}, name: {})",
                asset_id(model, resource, file)?,
                string_literal(name)
            )
        }
        Action::Vocal { file, volume } => match file {
            Some(file) => format!(
                "vocal.play({}, volume: {})",
                asset_id(model, ResourceKind::Voice, file)?,
                number(*volume)
            ),
            None if *volume == 1.0 => "vocal.stop()".into(),
            None => bail!("non-default volume on vocal stop cannot be represented"),
        },
        Action::PlayVideo { video } => format!(
            "video.play({}, {}, loop: {}, muted: {}, alpha: {}, skippable: {}, wait: {}, mode: {})",
            object_id(model, &video.id)?,
            asset_id(model, ResourceKind::Video, &video.file)?,
            video.looped,
            video.muted,
            number(video.alpha),
            video.skippable,
            video.wait_for_finished,
            enum_name(&video.mode)?
        ),
        Action::StopVideo { id, fade_out } => format!(
            "video.stop({}, fade: {})",
            id.as_deref()
                .map(|id| object_id(model, id))
                .transpose()?
                .unwrap_or("*"),
            duration(*fade_out)
        ),
        Action::AnimateKeyframes {
            target,
            frames,
            repeat,
            blocking,
        } => {
            let rows = frames
                .iter()
                .map(|frame| {
                    format!(
                        "    frame(duration: {}, easing: {}{})",
                        duration(frame.duration),
                        easing_name(frame.easing),
                        patch_fields(frame.transform)
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n");
            format!(
                "sprite.keyframes({}, repeat: {repeat}, blocking: {blocking}) {{\n{rows}\n  }}",
                object_id(model, target)?
            )
        }
        Action::FloatingText {
            text,
            position,
            font_size,
            color,
            fade_in,
            hold,
            fade_out,
            blocking,
        } => format!(
            "text.float({}, x: {}, y: {}, font_size: {}, color: {}, fade_in: {}, hold: {}, fade_out: {}, blocking: {})",
            string_literal(text),
            number(position[0]),
            number(position[1]),
            number(*font_size),
            rgba(color),
            duration(*fade_in),
            duration(*hold),
            duration(*fade_out),
            blocking
        ),
        Action::Wait { seconds } => format!("wait({})", duration(*seconds)),
        Action::End => "story.end()".into(),
        _ => return Ok(None),
    };
    Ok(Some(source))
}

/// Collapse only a complete reset group whose lowered actions are unchanged.
pub(super) fn render_camera_reset(
    actions: &[Action],
    model: &MigrationModel,
) -> Result<Option<String>> {
    let Some(group) = actions.get(..4) else {
        return Ok(None);
    };
    let first = match &group[0] {
        Action::Flow {
            action, when: None, ..
        } => action.as_ref(),
        action => action,
    };
    if !matches!(first, Action::ShakeCamera { shake, .. } if shake.amplitude == 0.0 && shake.frequency == 0.0 && shake.duration == 0.0)
    {
        return Ok(None);
    }
    let last = expected_action(&group[3], model)?;
    let (targets, seconds, easing, blocking) = match last {
        Action::SetPostProcess {
            targets,
            duration,
            easing,
            blocking,
            ..
        } => (targets, duration, easing, blocking),
        _ => return Ok(None),
    };
    let timing = if seconds == 0.0 && easing == Easing::Linear {
        if blocking {
            String::new()
        } else {
            ", blocking: false".into()
        }
    } else {
        timing(seconds, easing, blocking)
    };
    let source = format!("camera.reset({}{timing})", camera_target(targets));
    let parsed = keine_loader::adapter::parse_native_scenes(&format!("scene reset {{ {source} }}"));
    let report = &parsed[0].report;
    let expected = group
        .iter()
        .map(|action| expected_action(action, model))
        .collect::<Result<Vec<_>>>()?;
    let actual = report
        .actions
        .iter()
        .map(|action| expected_action(action, model))
        .collect::<Result<Vec<_>>>()?;
    Ok((report.diagnostics.is_empty() && expected == actual).then_some(source))
}

pub(super) fn verify(action: &Action, source: &str, model: &MigrationModel) -> Result<()> {
    if matches!(action, Action::Set { .. }) {
        return Ok(());
    }
    let expected = expected_action(action, model)?;
    let control_flow = matches!(
        action,
        Action::ConditionalCall { .. }
            | Action::Flow { when: Some(_), .. }
            | Action::SelectSpriteImageByCondition { .. }
    );
    let declarations = if control_flow {
        model
            .initial_variables
            .iter()
            .map(|(name, initial)| {
                Ok(format!(
                    "let {} = {}, ",
                    model.variable_ids[name],
                    value(initial)?
                ))
            })
            .collect::<Result<Vec<_>>>()?
            .concat()
    } else {
        String::new()
    };
    let parsed = keine_loader::adapter::parse_native_scenes(&format!(
        "scene migration_verify {{ {declarations}{source} }}"
    ));
    let report = &parsed[0].report;
    if control_flow {
        if report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.level == DiagnosticLevel::Error)
        {
            bail!(
                "generated native control flow is invalid: {source}: {:?}",
                report.diagnostics
            );
        }
        // These compatibility expressions lower into typed native control flow;
        // runtime round-trip tests and whole-project validation cover their scope.
        return Ok(());
    }

    if report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.level == DiagnosticLevel::Error)
        || report.actions != std::slice::from_ref(&expected)
    {
        bail!(
            "generated command does not preserve its typed action: {source}\nexpected: {expected:?}\nactual: {:?}\ndiagnostics: {:?}",
            report.actions,
            report.diagnostics
        );
    }
    Ok(())
}

fn normalize_camera(action: &Action) -> std::borrow::Cow<'_, Action> {
    match action {
        Action::SetPostProcessV2 {
            targets,
            effect,
            duration,
            easing,
            blocking,
        } => std::borrow::Cow::Owned(Action::SetPostProcess {
            targets: *targets,
            effect: Box::new(keine_core::PostProcessPatch::from_v2(effect)),
            duration: *duration,
            easing: *easing,
            blocking: *blocking,
        }),
        Action::SetCameraTween { spec } if spec.v2.is_some() => {
            let mut spec = spec.clone();
            let v2 = spec.v2.take().unwrap();
            let mut patch = spec.effect.take().map(|patch| *patch).unwrap_or_default();
            patch.mirror_shatter_intensity = Some(v2.mirror_shatter_intensity);
            patch.mirror_shatter_center_x = Some(v2.mirror_shatter_center_x);
            patch.mirror_shatter_center_y = Some(v2.mirror_shatter_center_y);
            patch.mirror_shatter_spread = Some(v2.mirror_shatter_spread);
            patch.mirror_shatter_seed = Some(v2.mirror_shatter_seed);
            patch.speed_lines_intensity = Some(v2.speed_lines_intensity);
            patch.speed_lines_radial = Some(v2.speed_lines_radial);
            patch.speed_lines_density = Some(v2.speed_lines_density);
            patch.speed_lines_angle = Some(v2.speed_lines_angle);
            patch.speed_lines_speed = Some(v2.speed_lines_speed);
            patch.speed_lines_center_x = Some(v2.speed_lines_center_x);
            patch.speed_lines_center_y = Some(v2.speed_lines_center_y);
            patch.speed_lines_region_ellipse = Some(v2.speed_lines_region_ellipse);
            patch.speed_lines_region_x = Some(v2.speed_lines_region_x);
            patch.speed_lines_region_y = Some(v2.speed_lines_region_y);
            patch.speed_lines_region_width = Some(v2.speed_lines_region_width);
            patch.speed_lines_region_height = Some(v2.speed_lines_region_height);
            patch.speed_lines_region_feather = Some(v2.speed_lines_region_feather);
            spec.effect = Some(Box::new(patch));
            std::borrow::Cow::Owned(Action::SetCameraTween { spec })
        }
        _ => std::borrow::Cow::Borrowed(action),
    }
}

fn expected_action(action: &Action, model: &MigrationModel) -> Result<Action> {
    if let Action::Flow {
        action,
        when: None,
        next,
    } = action
    {
        let mut inner = expected_action(action, model)?;
        if !next {
            return Ok(inner);
        }
        match &mut inner {
            Action::SetCameraTransform { blocking, .. }
            | Action::SetPostProcess { blocking, .. }
            | Action::SetPostProcessV2 { blocking, .. }
            | Action::ShakeCamera { blocking, .. }
            | Action::ShakeCameraRandomized { blocking, .. } => {
                *blocking = false;
                return Ok(inner);
            }
            Action::SetCameraTween { spec } => {
                spec.blocking = false;
                return Ok(inner);
            }
            _ => {
                return Ok(Action::Flow {
                    action: Box::new(inner),
                    when: None,
                    next: true,
                });
            }
        }
    }
    let normalized = normalize_camera(action);
    let mut expected = match normalized.as_ref() {
        Action::Effect {
            file: None,
            id: Some(id),
            ..
        } => Action::SoundEffect {
            file: None,
            id: Some(id.clone()),
            volume: 1.0,
            looped: false,
            fade: 0.0,
        },
        Action::UpdateSprite {
            id,
            image,
            position,
            layout,
            scale,
            duration,
            easing,
            blocking,
        } => Action::PatchSprite {
            id: id.clone(),
            image: image.clone(),
            position: Some(*position),
            layout: Some(*layout),
            scale: Some(*scale),
            duration: *duration,
            easing: *easing,
            blocking: *blocking,
        },
        Action::SetFilter { target, filter } => {
            let mut transform = TransformPatch::default();
            transform.set_blur(filter.blur);
            Action::SpriteVisual {
                action: Box::new(Action::SetTransform {
                    id: object_id(model, target)?.into(),
                    transform,
                    duration: 0.0,
                    easing: Easing::Linear,
                }),
                filter: keine_core::VisualFilterPatch {
                    environment_light: None,
                    brightness: Some(filter.brightness),
                    contrast: Some(filter.contrast),
                    saturation: Some(filter.saturation),
                },
            }
        }
        Action::UserInput {
            variable,
            title,
            button,
        } => Action::RequestInput {
            spec: keine_core::UserInputSpec {
                variable: model
                    .variable_ids
                    .get(variable)
                    .context("missing input variable mapping")?
                    .clone(),
                title: title.clone(),
                confirm_text: button.clone(),
                ..Default::default()
            },
        },
        _ => normalized.as_ref().clone(),
    };

    match &mut expected {
        Action::SetTransform { id, .. }
        | Action::ConfigureSpriteSequence { id, .. }
        | Action::ConfigureTimedSpriteSequence { id, .. }
        | Action::SelectSpriteImage { id, .. }
        | Action::ShowSprite { id, .. }
        | Action::MoveSprite { id, .. }
        | Action::UpdateSprite { id, .. }
        | Action::PatchSprite { id, .. }
        | Action::HideSprite { id, .. }
        | Action::ShowParticlesWithOptions { id, .. }
        | Action::ConfigureDynamicSpriteSequence { id, .. }
        | Action::ShowParticles { id, .. } => *id = object_id(model, id)?.into(),
        Action::SetCameraBinding { target, .. } | Action::AnimateKeyframes { target, .. } => {
            *target = object_id(model, target)?.into()
        }
        Action::FocusPortrait { speaker_id }
        | Action::Effect { id: speaker_id, .. }
        | Action::SoundEffect { id: speaker_id, .. }
        | Action::HideParticles { id: speaker_id, .. }
        | Action::StopVideo { id: speaker_id, .. } => {
            if let Some(id) = speaker_id {
                *id = object_id(model, id)?.into();
            }
        }
        Action::PlayVideo { video } => video.id = object_id(model, &video.id)?.into(),
        Action::ConfigurePortraits { character_ids, .. } => {
            for id in character_ids {
                *id = object_id(model, id)?.into();
            }
        }
        Action::HideSprites { prefix, .. } if !prefix.is_empty() => {
            *prefix = model
                .prefix_ids
                .get(prefix)
                .context("missing prefix mapping")?
                .clone()
        }
        _ => {}
    }
    match &mut expected {
        Action::SelectSpriteImage {
            variable,
            default_image,
            variants,
            ..
        } => {
            *variable = model
                .variable_ids
                .get(variable)
                .context("missing selector variable mapping")?
                .clone();
            *default_image = asset_id(model, ResourceKind::Figure, default_image)?;
            for (_, image) in variants {
                *image = asset_id(model, ResourceKind::Figure, image)?;
            }
        }
        Action::ConfigureSpriteSequence { frames, .. }
        | Action::ConfigureTimedSpriteSequence { frames, .. }
        | Action::ConfigureDynamicSpriteSequence { frames, .. } => {
            for frame in frames {
                *frame = asset_id(model, ResourceKind::Figure, frame)?;
            }
        }
        Action::ShowBg { image, .. } => *image = asset_id(model, ResourceKind::Background, image)?,
        Action::ShowSprite { image, .. }
        | Action::UpdateSprite { image, .. }
        | Action::PatchSprite { image, .. } => {
            *image = asset_id(model, ResourceKind::Figure, image)?
        }
        Action::PlayVideo { video } => {
            video.file = asset_id(model, ResourceKind::Video, &video.file)?
        }
        Action::EiyashouBgm {
            file: Some(file), ..
        } => *file = asset_id(model, ResourceKind::Bgm, file)?,
        Action::Vocal {
            file: Some(file), ..
        } => *file = asset_id(model, ResourceKind::Voice, file)?,
        Action::Effect {
            file: Some(file), ..
        }
        | Action::SoundEffect {
            file: Some(file), ..
        } => *file = asset_id(model, ResourceKind::Effect, file)?,
        Action::MiniAvatar { image } => *image = asset_id(model, ResourceKind::MiniAvatar, image)?,
        Action::ShowParticles { effect, .. } | Action::ShowParticlesWithOptions { effect, .. } => {
            if let Some(texture) = &mut effect.texture {
                *texture = asset_id(model, ResourceKind::Particle, texture)?;
            }
        }
        Action::Unlock { kind, file, .. } => {
            *file = asset_id(
                model,
                match kind {
                    keine_core::UnlockKind::Cg => ResourceKind::Background,
                    keine_core::UnlockKind::Bgm => ResourceKind::Bgm,
                },
                file,
            )?
        }
        _ => {}
    }
    Ok(expected)
}

pub(super) fn expression_source(expression: &str, model: &MigrationModel) -> Result<String> {
    // Match complete legacy names outside quoted strings. Replacement never expands
    // an expression or rewrites string contents; native validation checks its types.
    let mut names = model.variable_ids.iter().collect::<Vec<_>>();
    names.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    let mut output = String::new();
    let mut rest = expression;
    while !rest.is_empty() {
        let character = rest.chars().next().context("empty expression")?;
        if character == '"' || character == '\'' {
            let mut end = character.len_utf8();
            let mut escaped = false;
            for next in rest[end..].chars() {
                end += next.len_utf8();
                if next == character && !escaped {
                    break;
                }
                escaped = next == '\\' && !escaped;
            }
            output.push_str(&rest[..end]);
            rest = &rest[end..];
        } else if let Some((name, id)) = names.iter().find(|(name, _)| {
            output.chars().last().is_none_or(|previous| {
                !previous.is_alphanumeric() && previous != '_' && previous != '.'
            }) && rest.starts_with(name.as_str())
                && rest[name.len()..]
                    .chars()
                    .next()
                    .is_none_or(|next| !next.is_alphanumeric() && next != '_' && next != '.')
        }) {
            output.push_str(id);
            rest = &rest[name.len()..];
        } else {
            output.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }
    Ok(output)
}

fn timing(seconds: f32, easing: Easing, blocking: bool) -> String {
    format!(
        ", duration: {}, easing: {}, blocking: {blocking}",
        duration(seconds),
        easing_name(easing)
    )
}
fn camera_target(targets: CameraTargets) -> &'static str {
    match (targets.scene(), targets.characters()) {
        (false, false) => "none",
        (true, false) => "scene",
        (false, true) => "characters",
        (true, true) => "all",
    }
}
fn rgba(color: &[f32; 4]) -> String {
    format!(
        "rgba({})",
        color
            .iter()
            .map(|v| number(*v))
            .collect::<Vec<_>>()
            .join(", ")
    )
}
fn enum_name(value: &impl Serialize) -> Result<String> {
    let serde_json::Value::String(text) = serde_json::to_value(value)? else {
        bail!("expected a unit enum")
    };
    let mut name = String::new();
    for (index, character) in text.chars().enumerate() {
        if character.is_uppercase() && index > 0 {
            name.push('_');
        }
        name.extend(character.to_lowercase());
    }
    Ok(name)
}
fn fields(value: &impl Serialize) -> Result<String> {
    let serde_json::Value::Object(fields) = serde_json::to_value(value)? else {
        bail!("expected typed fields")
    };
    fields
        .into_iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(name, value)| {
            let value = match value {
                serde_json::Value::Bool(_) => value.to_string(),
                serde_json::Value::Number(ref value) => {
                    number(value.as_f64().context("invalid scalar field")? as f32)
                }
                _ => bail!("unsupported field {name}"),
            };
            Ok(format!("{name}: {value}"))
        })
        .collect::<Result<Vec<_>>>()
        .map(|fields| fields.join(", "))
}
fn transform_values(transform: &SpriteTransform) -> [(&'static str, f32); 9] {
    [
        ("x", transform.offset_x),
        ("y", transform.offset_y),
        ("alpha", transform.alpha),
        ("scale_x", transform.scale_x),
        ("scale_y", transform.scale_y),
        ("rotation", transform.rotation),
        ("blur", transform.blur),
        ("width", transform.width),
        ("height", transform.height),
    ]
}
fn full_transform(transform: &SpriteTransform) -> String {
    let uniform = transform.scale_x == transform.scale_y && transform.scale_x > 0.0;
    let mut result: String = transform_values(transform)
        .into_iter()
        .zip(transform_values(&SpriteTransform::default()))
        .filter(|((name, value), (_, default))| {
            value != default && !(uniform && matches!(*name, "scale_x" | "scale_y"))
        })
        .map(|((name, value), _)| format!(", {name}: {}", number(value)))
        .collect();
    if uniform && transform.scale_x != 1.0 {
        result.push_str(&format!(", scale: {}", number(transform.scale_x)));
    }
    result
}
fn patch_fields(patch: TransformPatch) -> String {
    // Apply to an absent sentinel through the public patch API, preserving presence
    // even for explicit neutral values without depending on its serialized bit mask.
    let unset = SpriteTransform {
        offset_x: f32::NAN,
        offset_y: f32::NAN,
        alpha: f32::NAN,
        scale_x: f32::NAN,
        scale_y: f32::NAN,
        rotation: f32::NAN,
        blur: f32::NAN,
        width: f32::NAN,
        height: f32::NAN,
    };
    let values = patch.apply_to(unset);
    transform_values(&values)
        .into_iter()
        .filter(|(_, value)| !value.is_nan())
        .map(|(name, value)| format!(", {name}: {}", number(value)))
        .collect()
}
fn position_fields(position: Position) -> String {
    let (name, offset) = match position.x {
        Anchor::Left(offset) => ("left", offset),
        Anchor::Center(offset) => ("center", offset),
        Anchor::Right(offset) => ("right", offset),
    };
    let mut fields = Vec::new();
    if offset != 0.0 {
        fields.push(format!("x: {}", number(offset)));
    }
    if position.y != 0.0 {
        fields.push(format!("y: {}", number(position.y)));
    }
    if fields.is_empty() {
        format!("position: {name}")
    } else {
        format!("position: {name}({})", fields.join(", "))
    }
}
fn explicit_layout_fields(layout: SpriteLayout) -> String {
    if layout == SpriteLayout::Natural {
        ", layout: natural".into()
    } else {
        layout_fields(layout)
    }
}
fn layout_fields(layout: SpriteLayout) -> String {
    match layout {
        SpriteLayout::Natural => String::new(),
        SpriteLayout::ViewportHeight(height) => {
            format!(", layout: viewport(height: {})", number(height))
        }
        SpriteLayout::Scene(layout) => {
            let mut fields = vec![format!(
                "fit: {}",
                match layout.fit {
                    keine_core::SceneFit::Cover => "cover",
                    keine_core::SceneFit::Contain => "contain",
                    keine_core::SceneFit::ByWidth => "by_width",
                    keine_core::SceneFit::ByHeight => "by_height",
                    keine_core::SceneFit::Stretch => "stretch",
                    keine_core::SceneFit::Center => "center",
                }
            )];
            for (name, value) in [("x", layout.position[0]), ("y", layout.position[1])] {
                if value != 0.0 {
                    fields.push(format!("{name}: {}", number(value)));
                }
            }
            if layout.anchor != [0.5; 2] {
                fields.push(format!(
                    "anchor: point(x: {}, y: {})",
                    number(layout.anchor[0]),
                    number(layout.anchor[1])
                ));
            }
            if let Some(size) = layout.size {
                fields.push(format!(
                    "width: {}, height: {}",
                    number(size[0]),
                    number(size[1])
                ));
            }
            format!(", layout: scene({})", fields.join(", "))
        }
        SpriteLayout::Composite {
            canvas,
            rect,
            height_ratio,
        } => {
            let mut fields = vec![format!(
                "canvas: size(width: {}, height: {})",
                number(canvas[0]),
                number(canvas[1])
            )];
            if let Some(rect) = rect {
                fields.push(format!(
                    "rect: rect(x: {}, y: {}, width: {}, height: {})",
                    number(rect[0]),
                    number(rect[1]),
                    number(rect[2]),
                    number(rect[3])
                ));
            }
            if let Some(height) = height_ratio {
                fields.push(format!("height: {}", number(height)));
            }
            format!(", layout: composite({})", fields.join(", "))
        }
    }
}
fn effect_fields(effect: &keine_core::PostProcessPatch, model: &MigrationModel) -> Result<String> {
    // serde's Option<Option<T>> JSON representation conflates absent and clear.
    // Handle those fields from the typed model; all remaining fields are scalar.
    let serde_json::Value::Object(fields) = serde_json::to_value(effect)? else {
        bail!("expected effect patch")
    };
    let mut output = String::new();
    for (name, value) in fields {
        if value.is_null()
            || matches!(
                name.as_str(),
                "focal_distance" | "lut_preset" | "color_tone"
            )
        {
            continue;
        }
        let value = match value {
            serde_json::Value::Bool(_) => value.to_string(),
            serde_json::Value::Number(value) => {
                number(value.as_f64().context("invalid effect scalar")? as f32)
            }
            _ => bail!("unsupported effect field {name}"),
        };
        output.push_str(&format!(", {name}: {value}"));
    }
    if let Some(distance) = effect.focal_distance {
        output.push_str(&format!(
            ", focal_distance: {}",
            distance.map(number).unwrap_or_else(|| "none".into())
        ));
    }
    if let Some(lut) = &effect.lut_preset {
        output.push_str(&format!(
            ", lut_preset: {}",
            lut.as_ref()
                .map(|lut| {
                    // Inactive presets are metadata and have no ResourceRef. Keep
                    // their identifier; referenced LUT assets still fail closed in
                    // build_model until their dedicated migration is supported.
                    Ok::<_, anyhow::Error>(
                        model
                            .asset_ids
                            .get(&AssetKey {
                                kind: ResourceKind::Lut,
                                source_name: lut.clone(),
                            })
                            .cloned()
                            .unwrap_or(native_identifier(lut)?.to_owned()),
                    )
                })
                .transpose()?
                .unwrap_or_else(|| "none".into())
        ));
    }
    if let Some(tone) = effect.color_tone {
        output.push_str(&format!(
            ", color_tone: {}",
            match tone {
                ColorToneMode::None => "none",
                ColorToneMode::Grayscale => "grayscale",
                ColorToneMode::Sepia => "sepia",
            }
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selectors_particles_and_dynamic_sequences_survive_migration_round_trip() {
        let model = MigrationModel {
            scene_ids: HashMap::from([("yes".into(), "yes".into()), ("no".into(), "no".into())]),
            speaker_ids: HashMap::new(),
            asset_ids: ["open.webp", "closed.webp"]
                .into_iter()
                .enumerate()
                .map(|(index, name)| {
                    (
                        AssetKey {
                            kind: ResourceKind::Figure,
                            source_name: name.into(),
                        },
                        format!("frame_{index}"),
                    )
                })
                .collect(),
            object_ids: BTreeMap::from([
                ("eyes".into(), "eyes".into()),
                ("snow".into(), "snow".into()),
            ]),
            prefix_ids: BTreeMap::new(),
            objects: objects::ObjectManifest::default(),
            variable_ids: BTreeMap::from([("score".into(), "score".into())]),
            initial_variables: BTreeMap::from([("score".into(), Value::Int(0))]),
            assets: AssetManifest::default(),
            characters: CharacterManifest {
                characters: BTreeMap::new(),
            },
        };
        for action in [
            Action::SelectSpriteImage {
                id: "eyes".into(),
                variable: "score".into(),
                default_image: "open.webp".into(),
                variants: vec![("1".into(), "closed.webp".into())],
            },
            Action::ShowParticlesWithOptions {
                id: "snow".into(),
                effect: keine_core::ParticleEffect::preset("LIGHT_SNOW"),
                options: keine_core::ParticleOptions {
                    size: Some(24.0),
                    speed: Some(100.0),
                    alpha: Some(0.7),
                    spin: Some(30.0),
                    drift: Some(12.0),
                    drag: Some(0.2),
                    color: Some([1.0, 0.9, 0.8, 1.0]),
                },
            },
            Action::ConfigureDynamicSpriteSequence {
                id: "eyes".into(),
                frames: vec!["open.webp".into(), "closed.webp".into()],
                fps: 10.0,
                frame_durations: Vec::new(),
                playback: keine_core::SequencePlayback::Blink { interval: 3.0 },
            },
            Action::ConfigureDynamicSpriteSequence {
                id: "eyes".into(),
                frames: vec!["open.webp".into(), "closed.webp".into()],
                fps: 12.0,
                frame_durations: vec![0.1, 0.2],
                playback: keine_core::SequencePlayback::Talk {
                    speaker: "Hero".into(),
                },
            },
        ] {
            let source = render(&action, &model).unwrap().unwrap();
            verify(&action, &source, &model).unwrap();
        }
        // A called fragment mutates the condition: only the selected arm may run.
        let action = Action::ConditionalCall {
            condition: "score == 0".into(),
            then_scene: "yes".into(),
            else_scene: Some("no".into()),
        };
        let command = render(&action, &model).unwrap().unwrap();
        verify(&action, &command, &model).unwrap();
        let source = format!(
            "scene main {{ let score = 0, let wrong = false, {command}, \"After\" }}\nscene yes {{ score = 1 }}\nscene no {{ wrong = true }}"
        );
        let scenes = keine_loader::parse_native_scenes(&source);
        assert!(
            scenes.iter().all(|s| s.report.diagnostics.is_empty()),
            "{scenes:?}"
        );
        let mut state = keine_core::State::new();
        state.install_program(keine_core::Program::from_scenes(
            scenes
                .into_iter()
                .filter_map(|s| s.name.map(|name| (name, s.report.actions))),
        ));
        state.current_scene = "main".into();
        assert_eq!(
            keine_core::step::step(&mut state),
            keine_core::StepResult::AwaitClick
        );
        assert_eq!(state.dialogue.as_ref().unwrap().text, "After");
        assert_eq!(state.vars["wrong"], Value::Bool(false));
    }

    #[test]
    fn dotted_output_preserves_sparse_clear_parallel_and_hold_semantics() {
        let mut model = MigrationModel {
            scene_ids: HashMap::new(),
            speaker_ids: HashMap::new(),
            asset_ids: HashMap::from([(
                AssetKey {
                    kind: ResourceKind::Figure,
                    source_name: "face.png".into(),
                },
                "face".into(),
            )]),
            object_ids: BTreeMap::from([("animation_target".into(), "animation_target".into())]),
            prefix_ids: BTreeMap::from([("scene-layer:".into(), "scene_layer_".into())]),
            objects: objects::ObjectManifest::default(),
            variable_ids: BTreeMap::new(),
            initial_variables: BTreeMap::new(),
            assets: AssetManifest::default(),
            characters: CharacterManifest {
                characters: BTreeMap::new(),
            },
        };
        // A logical reset is one authoring command, including animated waits.
        for command in [
            "camera.reset(all)",
            "camera.reset(all, blocking: false)",
            "camera.reset(scene, duration: 500ms, easing: ease_out)",
            "camera.reset(characters, duration: 750ms, blocking: false)",
        ] {
            let parsed = keine_loader::parse_native_scenes(&format!("scene a {{ {command} }}"));
            let actions = &parsed[0].report.actions;
            let source = render_camera_reset(actions, &model).unwrap().unwrap();
            let compact = keine_loader::parse_native_scenes(&format!("scene a {{ {source} }}"));
            assert_eq!(actions, &compact[0].report.actions);
            let scene = LoadedScene {
                name: "a".into(),
                path: "a.json".into(),
                actions: actions.clone(),
                action_spans: Vec::new(),
                diagnostics: Vec::new(),
                resources: Vec::new(),
                sub_scenes: Vec::new(),
            };
            model.scene_ids.insert("a".into(), "a".into());
            let generated = super::super::render_scenes(&[&scene], &model, false).unwrap();
            assert_eq!(generated.matches("camera.reset(").count(), 1);
            assert!(!generated.contains("camera.effect("));
            let flat = actions
                .iter()
                .map(|action| expected_action(action, &model).unwrap())
                .collect::<Vec<_>>();
            assert!(render_camera_reset(&flat, &model).unwrap().is_some());
            let mut changed = flat.clone();
            if let Action::SetPostProcess { effect, .. } = &mut changed[2] {
                effect.bloom_intensity = Some(0.5);
            }
            assert!(render_camera_reset(&changed, &model).unwrap().is_none());
            let mut changed = flat.clone();
            if let Action::SetCameraTransform { blocking, .. } = &mut changed[1] {
                *blocking = true;
            }
            assert!(render_camera_reset(&changed, &model).unwrap().is_none());
            assert!(render_camera_reset(&flat[..3], &model).unwrap().is_none());
        }
        model.asset_ids.insert(
            AssetKey {
                kind: ResourceKind::Effect,
                source_name: "sound.wav".into(),
            },
            "sound".into(),
        );
        model
            .object_ids
            .insert("electric-buzz".into(), "effect_0001".into());
        let mut patch = TransformPatch::default();
        patch.set_offset_x(0.0);
        patch.set_scale_x(1.02);
        let actions = [
            Action::WaitForAdvance,
            Action::RetractDialogue {
                source: "我还蛮喜欢她的".into(),
                keep: "我".into(),
            },
            Action::SoundEffect {
                file: Some("sound.wav".into()),
                id: Some("electric-buzz".into()),
                volume: 0.3,
                looped: false,
                fade: 0.2,
            },
            Action::SoundEffect {
                file: Some("sound.wav".into()),
                id: Some("electric-buzz".into()),
                volume: 0.3,
                looped: true,
                fade: 0.2,
            },
            Action::SoundEffect {
                file: None,
                id: Some("electric-buzz".into()),
                volume: 1.0,
                looped: false,
                fade: 0.4,
            },
            Action::Effect {
                file: Some("sound.wav".into()),
                id: Some("electric-buzz".into()),
                volume: 0.3,
            },
            Action::Effect {
                file: None,
                id: Some("electric-buzz".into()),
                volume: 0.0,
            },
            Action::SetCameraTransform {
                targets: CameraTargets::ALL,
                transform: patch,
                duration: 0.52,
                easing: Easing::InOutQuad,
                blocking: false,
            },
            Action::SetPostProcess {
                targets: CameraTargets::SCENE,
                effect: Box::new(keine_core::PostProcessPatch {
                    focal_distance: Some(None),
                    color_tone: Some(ColorToneMode::None),
                    blur_amount: Some(0.0),
                    ..Default::default()
                }),
                duration: 0.17,
                easing: Easing::OutCubic,
                blocking: true,
            },
            Action::Flow {
                action: Box::new(Action::HideSprites {
                    prefix: "scene-layer:".into(),
                    transition: Transition::Crossfade(0.5),
                }),
                when: None,
                next: true,
            },
            Action::AnimateKeyframes {
                target: "animation_target".into(),
                frames: vec![
                    keine_core::action::TransformKeyframe {
                        transform: patch,
                        duration: 16.7,
                        easing: Easing::Linear,
                    },
                    keine_core::action::TransformKeyframe {
                        transform: TransformPatch::default(),
                        duration: 0.4,
                        easing: Easing::Linear,
                    },
                ],
                repeat: 0,
                blocking: false,
            },
            Action::SetCameraTween {
                spec: Box::new(keine_core::CameraTweenSpec {
                    targets: CameraTargets::ALL,
                    transform: None,
                    effect: Some(Box::new(keine_core::PostProcessPatch {
                        distortion_strength: Some(-0.15),
                        ..Default::default()
                    })),
                    v2: None,
                    shake: None,
                    fields: vec![
                        keine_core::CameraTweenField::X,
                        keine_core::CameraTweenField::DistortionStrength,
                    ],
                    duration: 0.0,
                    easing: Easing::EaseInOut,
                    blocking: true,
                }),
            },
            Action::SetCameraTween {
                spec: Box::new(keine_core::CameraTweenSpec {
                    targets: CameraTargets::ALL,
                    transform: Some(patch),
                    effect: Some(Box::new(keine_core::PostProcessPatch {
                        blur_amount: Some(2.0),
                        ..Default::default()
                    })),
                    v2: Some(Box::default()),
                    shake: Some(keine_core::CameraShakeTweenSpec {
                        shake: keine_core::CameraShakeSpec {
                            amplitude: 4.0,
                            frequency: 2.0,
                            duration: 2.0,
                            axis: keine_core::CameraShakeAxis::Both,
                            falloff: keine_core::CameraShakeFalloff::Linear,
                        },
                        randomness: keine_core::CameraShakeRandomness {
                            amplitude: 0.3,
                            frequency: 0.2,
                        },
                    }),
                    fields: vec![
                        keine_core::CameraTweenField::ShakeAmplitude,
                        keine_core::CameraTweenField::ShakeFrequency,
                        keine_core::CameraTweenField::X,
                        keine_core::CameraTweenField::BlurAmount,
                    ],
                    duration: 1.0,
                    easing: Easing::EaseInOut,
                    blocking: false,
                }),
            },
            Action::ShowSprite {
                id: "animation_target".into(),
                image: "face.png".into(),
                position: Position {
                    x: Anchor::Right(500.0),
                    y: -20.0,
                },
                layout: SpriteLayout::Composite {
                    canvas: [1920.0, 1080.0],
                    rect: Some([10.0, 20.0, 700.0, 900.0]),
                    height_ratio: Some(0.85),
                },
                transform: SpriteTransform::default(),
                transition: Transition::Crossfade(0.2),
                z_index: 2,
                blend: keine_core::BlendMode::Alpha,
            },
            Action::MoveSprite {
                id: "animation_target".into(),
                position: Position {
                    x: Anchor::Left(-40.0),
                    y: 12.0,
                },
                duration: 0.3,
                easing: Easing::Linear,
                blocking: false,
            },
            Action::UpdateSprite {
                id: "animation_target".into(),
                image: "face.png".into(),
                position: Position {
                    x: Anchor::Center(20.0),
                    y: 30.0,
                },
                layout: SpriteLayout::ViewportHeight(0.9),
                scale: 1.2,
                duration: 0.3,
                easing: Easing::Linear,
                blocking: false,
            },
            Action::UpdateSprite {
                id: "animation_target".into(),
                image: "face.png".into(),
                position: Position::center(0.0),
                layout: SpriteLayout::Natural,
                scale: 1.0,
                duration: 0.0,
                easing: Easing::Linear,
                blocking: true,
            },
            Action::PatchSprite {
                id: "animation_target".into(),
                image: "face.png".into(),
                position: None,
                layout: None,
                scale: None,
                duration: 0.0,
                easing: Easing::Linear,
                blocking: true,
            },
            Action::PatchSprite {
                id: "animation_target".into(),
                image: "face.png".into(),
                position: None,
                layout: Some(SpriteLayout::Natural),
                scale: Some(1.0),
                duration: 0.0,
                easing: Easing::Linear,
                blocking: true,
            },
            Action::SetTransform {
                id: "animation_target".into(),
                transform: patch,
                duration: 0.3,
                easing: Easing::Linear,
            },
            Action::End,
        ];
        for action in actions {
            let source = render(&action, &model).unwrap().unwrap();
            let parsed = keine_loader::adapter::parse_native_scenes(&format!(
                "scene example {{ {source} }}"
            ));
            assert!(
                parsed[0].report.diagnostics.is_empty(),
                "{source}: {:?}",
                parsed[0].report.diagnostics
            );
            assert_eq!(
                parsed[0].report.actions,
                vec![expected_action(&action, &model).unwrap()],
                "{source}"
            );
        }
    }
}
