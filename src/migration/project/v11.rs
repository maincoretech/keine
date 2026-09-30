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
    let source = match action {
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
            "camera.effect.v2({}, {}{})",
            camera_target(*targets),
            fields(effect)?,
            timing(*seconds, *easing, *blocking)
        ),
        Action::SetCameraTween { spec } => {
            let source = match (&spec.transform, &spec.effect, &spec.v2) {
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
                    "camera.effect.v2({}, {}",
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
                    if spec.transform.is_none() && spec.effect.is_none() && spec.v2.is_none() {
                        bail!("empty camera tween requires manual migration");
                    }
                    source
                }
            };
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

pub(super) fn verify(action: &Action, source: &str, model: &MigrationModel) -> Result<()> {
    if matches!(action, Action::Set { .. }) {
        // Assignment is intentionally lowered to the native typed evaluator;
        // whole-project validation checks the generated declaration/type scope.
        return Ok(());
    }
    let expected = expected_action(action, model)?;
    let parsed = keine_loader::adapter::parse_native_scenes(&format!(
        "scene migration_verify {{ {source} }}"
    ));
    let report = &parsed[0].report;
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
    let mut expected = action.clone();
    match &mut expected {
        Action::ShowSprite { id, .. }
        | Action::HideSprite { id, .. }
        | Action::ShowParticles { id, .. } => *id = object_id(model, id)?.into(),
        Action::SetCameraBinding { target, .. } | Action::AnimateKeyframes { target, .. } => {
            *target = object_id(model, target)?.into()
        }
        Action::FocusPortrait { speaker_id }
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
        Action::ShowBg { image, .. } => *image = asset_id(model, ResourceKind::Background, image)?,
        Action::ShowSprite { image, .. } => *image = asset_id(model, ResourceKind::Figure, image)?,
        Action::PlayVideo { video } => {
            video.file = asset_id(model, ResourceKind::Video, &video.file)?
        }
        Action::Vocal {
            file: Some(file), ..
        } => *file = asset_id(model, ResourceKind::Voice, file)?,
        Action::ShowParticles { effect, .. } => {
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
    transform_values(transform)
        .into_iter()
        .map(|(name, value)| format!(", transform_{name}: {}", number(value)))
        .collect()
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
    transform_values(&patch.apply_to(unset))
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
    format!(
        "position: {name}, anchor_offset: {}, y: {}",
        number(offset),
        number(position.y)
    )
}
fn layout_fields(layout: SpriteLayout) -> String {
    match layout {
        SpriteLayout::Natural => ", layout: natural".into(),
        SpriteLayout::ViewportHeight(height) => format!(
            ", layout: viewport_height, layout_height: {}",
            number(height)
        ),
        SpriteLayout::Scene(layout) => {
            let mut args = format!(
                ", layout: scene, layout_fit: {}, layout_x: {}, layout_y: {}, layout_anchor_x: {}, layout_anchor_y: {}",
                match layout.fit {
                    keine_core::SceneFit::Cover => "cover",
                    keine_core::SceneFit::Contain => "contain",
                    keine_core::SceneFit::ByWidth => "by_width",
                    keine_core::SceneFit::ByHeight => "by_height",
                    keine_core::SceneFit::Stretch => "stretch",
                    keine_core::SceneFit::Center => "center",
                },
                number(layout.position[0]),
                number(layout.position[1]),
                number(layout.anchor[0]),
                number(layout.anchor[1])
            );
            if let Some(size) = layout.size {
                args.push_str(&format!(
                    ", layout_width: {}, layout_height: {}",
                    number(size[0]),
                    number(size[1])
                ));
            }
            args
        }
        SpriteLayout::Composite {
            canvas,
            rect,
            height_ratio,
        } => {
            let mut args = format!(
                ", layout: composite, layout_canvas_width: {}, layout_canvas_height: {}",
                number(canvas[0]),
                number(canvas[1])
            );
            if let Some(rect) = rect {
                args.push_str(&format!(", layout_rect_x: {}, layout_rect_y: {}, layout_rect_width: {}, layout_rect_height: {}",number(rect[0]),number(rect[1]),number(rect[2]),number(rect[3])));
            }
            if let Some(height) = height_ratio {
                args.push_str(&format!(", layout_height_ratio: {}", number(height)));
            }
            args
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
    fn dotted_output_preserves_sparse_clear_parallel_and_hold_semantics() {
        let model = MigrationModel {
            scene_ids: HashMap::new(),
            speaker_ids: HashMap::new(),
            asset_ids: HashMap::new(),
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
        let mut patch = TransformPatch::default();
        patch.set_offset_x(0.0);
        patch.set_scale_x(1.02);
        let actions = [
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
                    fields: vec![
                        keine_core::CameraTweenField::X,
                        keine_core::CameraTweenField::BlurAmount,
                    ],
                    duration: 1.0,
                    easing: Easing::EaseInOut,
                    blocking: false,
                }),
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
