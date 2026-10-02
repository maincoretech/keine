//! Native manifests lower author names once; runtime grouping keeps its original IDs.
use super::*;
use keine_core::config::EiyashouObjectManifest;
use keine_core::{Action, StageEventKind, StageTarget};

#[derive(Clone, Debug, Default)]
pub(super) struct ObjectAliases {
    objects: HashMap<String, String>,
    prefixes: HashMap<String, String>,
}

pub(super) fn load(root: &Path, configured: &str) -> Result<ObjectAliases> {
    if configured.is_empty() {
        return Ok(ObjectAliases::default());
    }
    let path = confined_manifest_path(root, configured)?;
    let source =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let manifest = EiyashouObjectManifest::from_yaml(&source)
        .with_context(|| format!("invalid object manifest {}", path.display()))?;
    let aliases = ObjectAliases {
        objects: manifest.objects,
        prefixes: manifest.prefixes,
    };
    for names in [&aliases.objects, &aliases.prefixes] {
        let mut targets = HashSet::new();
        for (name, target) in names {
            let mut chars = name.chars();
            if !chars
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
                || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                || target.is_empty()
                || !targets.insert(target)
            {
                bail!(
                    "object aliases require bare identifiers and unique non-empty runtime IDs: {name}"
                );
            }
        }
    }
    Ok(aliases)
}

impl ObjectAliases {
    fn id(&self, id: &mut String) {
        if let Some(runtime_id) = self.objects.get(id) {
            id.clone_from(runtime_id);
        }
    }

    fn optional_id(&self, id: &mut Option<String>) {
        if let Some(id) = id {
            self.id(id);
        }
    }

    /// Resolve each typed object slot exactly once. Assets, variables, scene names
    /// and dialogue text have different namespaces and are never rewritten here.
    pub(super) fn resolve(&self, action: &mut Action) {
        match action {
            Action::ShowSprite { id, .. }
            | Action::HideSprite { id, .. }
            | Action::SetTransform { id, .. }
            | Action::ShowParticles { id, .. }
            | Action::SelectSpriteImage { id, .. }
            | Action::SelectSpriteImageByCondition { id, .. }
            | Action::EiyashouSelectSpriteImageByCondition { id, .. }
            | Action::ConfigureSpriteSequence { id, .. }
            | Action::ConfigureTimedSpriteSequence { id, .. }
            | Action::UpdateSprite { id, .. }
            | Action::PatchSprite { id, .. }
            | Action::MoveSprite { id, .. } => self.id(id),
            Action::Animate { target, .. }
            | Action::SetTransition { target, .. }
            | Action::SetFilter { target, .. }
            | Action::AnimateKeyframes { target, .. }
            | Action::SetCameraBinding { target, .. } => self.id(target),
            Action::HideParticles { id, .. }
            | Action::Effect { id, .. }
            | Action::SoundEffect { id, .. }
            | Action::StopVideo { id, .. }
            | Action::HideFloatingText { id, .. }
            | Action::ConfigureFloatingText { id, .. } => self.optional_id(id),
            Action::FocusPortrait { speaker_id } => self.optional_id(speaker_id),
            Action::ConfigurePortraits { character_ids, .. } => {
                for id in character_ids {
                    self.id(id);
                }
            }
            Action::PlayVideo { video } => self.id(&mut video.id),
            Action::HideSprites { prefix, .. } => {
                if let Some(runtime_prefix) = self.prefixes.get(prefix) {
                    prefix.clone_from(runtime_prefix);
                }
            }
            Action::StageMask { id, mask, .. } => {
                self.id(id);
                if let Some(mask) = mask {
                    for target in &mut mask.targets {
                        self.id(target);
                    }
                }
            }
            Action::StageAnimation { animation } => {
                self.id(&mut animation.id);
                for track in &mut animation.tracks {
                    match &mut track.target {
                        StageTarget::Character { id, .. } | StageTarget::SceneLayer { id } => {
                            self.id(id)
                        }
                        StageTarget::Camera => {}
                    }
                }
                for event in &mut animation.events {
                    match &mut event.kind {
                        StageEventKind::Particle { id, .. } => self.id(id),
                        StageEventKind::Scene(scene) => {
                            for layer in &mut scene.layers {
                                self.id(&mut layer.id);
                            }
                        }
                        StageEventKind::Audio(audio) => self.id(&mut audio.id),
                        _ => {}
                    }
                }
            }
            Action::Flow { action, .. } | Action::SpriteVisual { action, .. } => {
                self.resolve(action)
            }
            _ => {}
        }
    }
}
