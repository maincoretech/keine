//! Give the migrated source simple names while retaining typed runtime grouping.
use super::*;
use std::collections::BTreeSet;

#[derive(Default, Serialize)]
pub(super) struct ObjectManifest {
    pub objects: BTreeMap<String, String>,
    pub prefixes: BTreeMap<String, String>,
}

#[derive(Default)]
struct Names {
    objects: BTreeSet<String>,
    characters: BTreeSet<String>,
    prefixes: BTreeSet<String>,
}

pub(super) fn build(
    scenes: &[LoadedScene],
) -> (
    BTreeMap<String, String>,
    BTreeMap<String, String>,
    ObjectManifest,
) {
    let mut names = Names::default();
    for action in scenes.iter().flat_map(|scene| &scene.actions) {
        collect(action, &mut names);
    }
    let mut ids = BTreeMap::new();
    for (index, name) in names.characters.iter().enumerate() {
        ids.insert(name.clone(), format!("character_{:04}", index + 1));
    }
    let mut prefixes = BTreeMap::new();
    for (index, prefix) in names
        .prefixes
        .iter()
        .filter(|prefix| !prefix.is_empty())
        .enumerate()
    {
        let alias = if prefix == "scene-layer:" {
            "scene_layer_".into()
        } else if let Some(character) = prefix
            .strip_prefix("character-layer:")
            .and_then(|id| id.strip_suffix(':'))
            .and_then(|id| ids.get(id))
        {
            format!("{character}_layer_")
        } else {
            format!("object_group_{:04}_", index + 1)
        };
        prefixes.insert(prefix.clone(), alias);
    }
    let mut counts = BTreeMap::<String, usize>::new();
    for name in names.objects {
        if ids.contains_key(&name) {
            continue;
        }
        let stem = prefixes
            .iter()
            .filter(|(prefix, _)| name.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, id)| id.as_str())
            .unwrap_or("object_");
        let count = counts.entry(stem.to_owned()).or_default();
        *count += 1;
        ids.insert(name, format!("{stem}{count:04}"));
    }
    let manifest = ObjectManifest {
        objects: ids
            .iter()
            .map(|(source, id)| (id.clone(), source.clone()))
            .collect(),
        prefixes: prefixes
            .iter()
            .map(|(source, id)| (id.clone(), source.clone()))
            .collect(),
    };
    (ids, prefixes, manifest)
}

fn collect(action: &Action, names: &mut Names) {
    let id = match action {
        Action::ShowSprite { id, .. }
        | Action::HideSprite { id, .. }
        | Action::SelectSpriteImage { id, .. }
        | Action::SelectSpriteImageByCondition { id, .. }
        | Action::ShowParticlesWithOptions { id, .. }
        | Action::ConfigureDynamicSpriteSequence { id, .. }
        | Action::ShowParticles { id, .. } => Some(id),
        Action::SetCameraBinding { target, .. } | Action::AnimateKeyframes { target, .. } => {
            Some(target)
        }
        Action::FocusPortrait { speaker_id }
        | Action::Effect { id: speaker_id, .. }
        | Action::SoundEffect { id: speaker_id, .. }
        | Action::HideParticles { id: speaker_id, .. }
        | Action::StopVideo { id: speaker_id, .. } => speaker_id.as_ref(),
        Action::PlayVideo { video } => Some(&video.id),
        Action::Flow { action, .. } => {
            collect(action, names);
            None
        }
        Action::HideSprites { prefix, .. } => {
            names.prefixes.insert(prefix.clone());
            None
        }
        Action::ConfigurePortraits { character_ids, .. } => {
            names.characters.extend(character_ids.iter().cloned());
            names.objects.extend(character_ids.iter().cloned());
            None
        }
        _ => None,
    };
    if let Some(id) = id {
        names.objects.insert(id.clone());
    }
}
