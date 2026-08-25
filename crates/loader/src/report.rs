use keine_core::config::GameConfig;
use keine_core::{Action, ChoiceTarget};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceSpan {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub span: SourceSpan,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    Background,
    Figure,
    Voice,
    Bgm,
    Effect,
    Particle,
    Video,
    MiniAvatar,
    Lut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRef {
    pub path: String,
    pub kind: ResourceKind,
    pub action_index: usize,
    pub span: SourceSpan,
}

impl ResourceRef {
    /// Whether runtime interpolation still has to resolve this reference.
    /// Static validation and speculative loading must not treat the template
    /// itself as an asset path; the resolved active state is loaded normally.
    pub fn is_dynamic(&self) -> bool {
        self.path.contains('{')
    }

    /// Resolves an adapter-neutral resource reference through the active
    /// project's aliases and conventional fallback directories.
    pub fn resolved_path(&self, config: &GameConfig) -> String {
        match self.kind {
            ResourceKind::Background => config.bg_path(&self.path),
            ResourceKind::Figure | ResourceKind::MiniAvatar => config.figure_path(&self.path),
            ResourceKind::Voice => config.voice_path(&self.path),
            ResourceKind::Bgm => config.bgm_path(&self.path),
            ResourceKind::Effect => config.effect_path(&self.path),
            ResourceKind::Particle => self.path.clone(),
            ResourceKind::Video => config.video_path(&self.path),
            ResourceKind::Lut => config.lut_path(&self.path),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneRef {
    pub scene: String,
    pub action_index: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Default)]
pub struct ParseReport {
    pub actions: Vec<Action>,
    pub spans: Vec<SourceSpan>,
    pub diagnostics: Vec<Diagnostic>,
    pub resources: Vec<ResourceRef>,
    pub sub_scenes: Vec<SceneRef>,
}

impl ParseReport {
    pub(crate) fn push(&mut self, action: Action, span: SourceSpan) {
        let action_index = self.actions.len();
        collect_references(&action, action_index, span, self);
        self.actions.push(action);
        self.spans.push(span);
    }
}

fn collect_references(
    action: &Action,
    action_index: usize,
    span: SourceSpan,
    report: &mut ParseReport,
) {
    let mut resource = |path: &str, kind| {
        if !path.is_empty() && path != "none" {
            report.resources.push(ResourceRef {
                path: path.to_owned(),
                kind,
                action_index,
                span,
            });
        }
    };
    match action {
        Action::ShowBg { image, .. } => resource(image, ResourceKind::Background),
        Action::ShowSprite { image, .. } => resource(image, ResourceKind::Figure),
        Action::UpdateSprite { image, .. } => resource(image, ResourceKind::Figure),
        Action::ConfigureSpriteSequence { frames, .. } => {
            for frame in frames {
                resource(frame, ResourceKind::Figure);
            }
        }
        Action::Say { options, .. } => {
            if let Some(vocal) = &options.vocal {
                resource(vocal, ResourceKind::Voice);
            }
        }
        Action::Bgm { file, .. } => resource(file, ResourceKind::Bgm),
        Action::Effect {
            file: Some(file), ..
        } => resource(file, ResourceKind::Effect),
        Action::Vocal {
            file: Some(file), ..
        } => resource(file, ResourceKind::Voice),
        Action::ShowParticles { effect, .. } => {
            if let Some(texture) = &effect.texture {
                resource(texture, ResourceKind::Particle);
            }
        }
        Action::StageAnimation { animation } => {
            for track in &animation.tracks {
                if let keine_core::StageTarget::Character {
                    image: Some(image), ..
                } = &track.target
                {
                    resource(image, ResourceKind::Figure);
                }
            }
            for event in &animation.events {
                match &event.kind {
                    keine_core::StageEventKind::Particle { effect, .. } => {
                        if let Some(texture) = &effect.texture {
                            resource(texture, ResourceKind::Particle);
                        }
                    }
                    keine_core::StageEventKind::Scene(cue) => {
                        for layer in &cue.layers {
                            resource(&layer.image, ResourceKind::Figure);
                        }
                    }
                    keine_core::StageEventKind::Audio(cue) => resource(
                        &cue.file,
                        match cue.kind {
                            keine_core::StageAudioKind::Bgm => ResourceKind::Bgm,
                            keine_core::StageAudioKind::Effect => ResourceKind::Effect,
                            keine_core::StageAudioKind::Vocal => ResourceKind::Voice,
                        },
                    ),
                    keine_core::StageEventKind::CameraPatch { effect, .. } => {
                        collect_visible_lut(effect, &mut resource);
                    }
                    keine_core::StageEventKind::CameraShake(_) => {}
                }
            }
        }
        Action::SetPostProcess { effect, .. } => collect_visible_lut(effect, &mut resource),
        Action::PlayVideo { video } => resource(&video.file, ResourceKind::Video),
        Action::MiniAvatar { image } => resource(image, ResourceKind::MiniAvatar),
        Action::Unlock { kind, file, .. } => resource(
            file,
            match kind {
                keine_core::UnlockKind::Cg => ResourceKind::Background,
                keine_core::UnlockKind::Bgm => ResourceKind::Bgm,
            },
        ),
        Action::ChangeScene(scene) | Action::CallScene(scene) => {
            report.sub_scenes.push(SceneRef {
                scene: scene.clone(),
                action_index,
                span,
            });
        }
        Action::Menu { choices, .. } => {
            for choice in choices {
                if let ChoiceTarget::ChangeScene(scene) | ChoiceTarget::CallScene(scene) =
                    &choice.target
                {
                    report.sub_scenes.push(SceneRef {
                        scene: scene.clone(),
                        action_index,
                        span,
                    });
                }
            }
        }
        Action::Flow { action, .. } => {
            collect_references(action, action_index, span, report);
        }
        _ => {}
    }
}

fn collect_visible_lut(
    effect: &keine_core::PostProcessPatch,
    resource: &mut impl FnMut(&str, ResourceKind),
) {
    if effect.lut_intensity.unwrap_or_default() > 0.001
        && let Some(Some(lut)) = &effect.lut_preset
    {
        resource(lut, ResourceKind::Lut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keine_core::{
        CameraTargets, Easing, PostProcessPatch, StageAnimation, StageEvent, StageEventKind,
    };

    fn lut_patch(name: &str) -> Box<PostProcessPatch> {
        Box::new(PostProcessPatch {
            lut_preset: Some(Some(name.into())),
            lut_intensity: Some(0.5),
            ..Default::default()
        })
    }

    #[test]
    fn collects_luts_from_direct_and_timeline_camera_patches() {
        let span = SourceSpan { line: 3, column: 7 };
        let mut report = ParseReport::default();
        report.push(
            Action::SetPostProcess {
                targets: CameraTargets::ALL,
                effect: lut_patch("warm"),
                duration: 0.0,
                easing: Easing::Linear,
                blocking: false,
            },
            span,
        );
        report.push(
            Action::StageAnimation {
                animation: StageAnimation {
                    id: "camera".into(),
                    duration: 1.0,
                    tracks: Vec::new(),
                    events: vec![StageEvent {
                        time: 0.0,
                        kind: StageEventKind::CameraPatch {
                            targets: Some(CameraTargets::SCENE),
                            effect: lut_patch("night"),
                        },
                    }],
                    repeat: 0,
                    infinite: false,
                    playback_rate: 1.0,
                    blocking: false,
                },
            },
            span,
        );

        assert_eq!(
            report
                .resources
                .iter()
                .map(|resource| (resource.path.as_str(), resource.kind))
                .collect::<Vec<_>>(),
            [("warm", ResourceKind::Lut), ("night", ResourceKind::Lut),]
        );
    }

    #[test]
    fn ignores_lut_presets_that_cannot_affect_pixels() {
        let mut report = ParseReport::default();
        report.push(
            Action::SetPostProcess {
                targets: CameraTargets::ALL,
                effect: Box::new(PostProcessPatch {
                    lut_preset: Some(Some("warm".into())),
                    ..Default::default()
                }),
                duration: 0.0,
                easing: Easing::Linear,
                blocking: false,
            },
            SourceSpan { line: 1, column: 1 },
        );

        assert!(report.resources.is_empty());
    }

    #[test]
    fn marks_interpolated_resources_without_resolving_them_as_files() {
        let resource = ResourceRef {
            path: "characters/{route}.webp".into(),
            kind: ResourceKind::Figure,
            action_index: 0,
            span: SourceSpan { line: 1, column: 1 },
        };

        assert!(resource.is_dynamic());
    }
}
