use std::collections::HashMap;

use bevy::asset::LoadState;
use bevy::prelude::*;
use keine_loader::ResourceKind;

use crate::runtime::resources::{
    AssetLoadingGate, GameConfigResource, GameState, LocalAssetCache, LocalAssetManifest,
};
use crate::ui::foundation::UiFonts;

const LOOKAHEAD_ACTIONS: usize = 20;

#[derive(Default)]
pub(crate) struct PrefetchState {
    initialized: bool,
    scene: String,
    cursor: usize,
    ended: bool,
    background: Option<String>,
    sprites: HashMap<String, String>,
    vocal: Option<String>,
    bgm: Option<String>,
    effects: HashMap<String, String>,
    particles: HashMap<String, Option<String>>,
}

impl PrefetchState {
    fn matches(&self, state: &GameState) -> bool {
        self.initialized
            && self.scene == state.current_scene
            && self.cursor == state.cursor
            && self.ended == state.ended
            && self.background == state.bg
            && self.vocal.as_ref()
                == state
                    .dialogue
                    .as_ref()
                    .and_then(|dialogue| dialogue.vocal.as_ref())
            && self.bgm == state.bgm.file
            && self.sprites.len() == state.sprites.len()
            && state
                .sprites
                .iter()
                .all(|(id, sprite)| self.sprites.get(id) == Some(&sprite.image))
            && self.effects.len() == state.looping_effects.len()
            && state
                .looping_effects
                .iter()
                .all(|(id, effect)| self.effects.get(id) == Some(&effect.file))
            && self.particles.len() == state.particle_effects.len()
            && state
                .particle_effects
                .iter()
                .all(|(id, effect)| self.particles.get(id) == Some(&effect.effect.texture))
    }

    fn capture(&mut self, state: &GameState) {
        self.initialized = true;
        self.scene.clone_from(&state.current_scene);
        self.cursor = state.cursor;
        self.ended = state.ended;
        self.background.clone_from(&state.bg);
        self.vocal = state
            .dialogue
            .as_ref()
            .and_then(|dialogue| dialogue.vocal.clone());
        self.bgm.clone_from(&state.bgm.file);
        self.sprites.clear();
        self.sprites.extend(
            state
                .sprites
                .iter()
                .map(|(id, sprite)| (id.clone(), sprite.image.clone())),
        );
        self.effects.clear();
        self.effects.extend(
            state
                .looping_effects
                .iter()
                .map(|(id, effect)| (id.clone(), effect.file.clone())),
        );
        self.particles.clear();
        self.particles.extend(
            state
                .particle_effects
                .iter()
                .map(|(id, effect)| (id.clone(), effect.effect.texture.clone())),
        );
    }
}

pub fn prefetch_local_assets(
    state: Res<GameState>,
    config: Res<GameConfigResource>,
    manifest: Res<LocalAssetManifest>,
    asset_server: Res<AssetServer>,
    mut cache: ResMut<LocalAssetCache>,
    mut previous: Local<PrefetchState>,
) {
    if previous.matches(&state) && !manifest.is_changed() && !config.is_changed() {
        return;
    }
    previous.capture(&state);
    let mut desired = HashMap::new();
    // While the title is open, use otherwise idle time to warm the entry scene.
    // This also keeps its handles alive after returning from the game, instead
    // of releasing and recreating them on the next START click.
    let (scene_name, cursor) = if state.ended {
        (crate::scene::entry_scene(&state), 0)
    } else {
        (state.current_scene.clone(), state.cursor)
    };
    if let Some(scene) = manifest.get(&scene_name) {
        for resource in scene.resources.iter().filter(|resource| {
            resource.action_index >= cursor && resource.action_index <= cursor + LOOKAHEAD_ACTIONS
        }) {
            let path = resource.resolved_path(&config);
            desired.insert(path, resource.kind);
        }
        for reference in scene.sub_scenes.iter().filter(|reference| {
            reference.action_index >= cursor && reference.action_index <= cursor + LOOKAHEAD_ACTIONS
        }) {
            if let Some(called_scene) = manifest.get(&reference.scene) {
                // A callScene may be large. Warm only its opening window; the
                // normal cursor lookahead takes over after entering it.
                for resource in called_scene
                    .resources
                    .iter()
                    .filter(|resource| resource.action_index <= LOOKAHEAD_ACTIONS)
                {
                    let path = resource.resolved_path(&config);
                    desired.insert(path, resource.kind);
                }
            }
        }
    }

    if state.ended {
        desired.insert(
            config.bg_path(&config.title_background),
            ResourceKind::Background,
        );
    }

    if let Some(background) = &state.bg {
        desired.insert(config.bg_path(background), ResourceKind::Background);
    }
    for sprite in state.sprites.values() {
        desired.insert(config.figure_path(&sprite.image), ResourceKind::Figure);
    }
    if let Some(vocal) = state
        .dialogue
        .as_ref()
        .and_then(|dialogue| dialogue.vocal.as_ref())
    {
        desired.insert(config.voice_path(vocal), ResourceKind::Voice);
    }
    if let Some(bgm) = &state.bgm.file {
        desired.insert(config.bgm_path(bgm), ResourceKind::Bgm);
    }
    for effect in state.looping_effects.values() {
        desired.insert(config.effect_path(&effect.file), ResourceKind::Effect);
    }
    for effect in state.particle_effects.values() {
        if let Some(texture) = effect
            .effect
            .texture
            .as_ref()
            .filter(|path| !path.is_empty())
        {
            desired.insert(texture.clone(), ResourceKind::Particle);
        }
    }

    if cache.0.len() == desired.len() && desired.keys().all(|path| cache.0.contains_key(path)) {
        return;
    }
    cache.0.retain(|path, _| desired.contains_key(path));
    for (path, kind) in desired {
        // Video is streamed by the dedicated playback backend. Keeping the
        // whole container alive in Bevy's generic asset cache would defeat
        // bounded-memory decoding and duplicate the compressed payload.
        if kind == ResourceKind::Video {
            continue;
        }
        cache.0.entry(path.clone()).or_insert_with(|| match kind {
            ResourceKind::Background
            | ResourceKind::Figure
            | ResourceKind::Particle
            | ResourceKind::MiniAvatar => asset_server.load::<Image>(path).untyped(),
            ResourceKind::Voice | ResourceKind::Bgm | ResourceKind::Effect => {
                crate::runtime::audio::load_untyped(&asset_server, path)
            }
            ResourceKind::Video => unreachable!("video prefetch is handled above"),
        });
    }
}

pub fn update_loading_gate(
    asset_server: Res<AssetServer>,
    cache: Res<LocalAssetCache>,
    fonts: Res<UiFonts>,
    mut gate: ResMut<AssetLoadingGate>,
) {
    if !gate.blocked && !cache.is_changed() && !fonts.is_changed() {
        return;
    }
    let pending = |id| {
        matches!(
            asset_server.load_state(id),
            LoadState::NotLoaded | LoadState::Loading
        )
    };
    gate.blocked = cache.0.values().any(|handle| pending(handle.id()))
        || pending(fonts.text.id().untyped())
        || pending(fonts.icons.id().untyped());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typewriter_progress_does_not_rebuild_the_prefetch_set() {
        let mut state = keine_core::State::new();
        state.current_scene = "main".into();
        state.dialogue = Some(keine_core::state::Dialogue {
            speaker: "A".into(),
            text: "hello".into(),
            markup: "hello".into(),
            visible_chars: 1,
            pauses: Vec::new(),
            vocal: Some("line.ogg".into()),
            volume: 1.0,
            auto_advance: false,
        });
        let mut previous = PrefetchState::default();
        previous.capture(&GameState(state.clone()));

        state.dialogue.as_mut().unwrap().visible_chars = 2;
        assert!(previous.matches(&GameState(state.clone())));
        state.cursor += 1;
        assert!(!previous.matches(&GameState(state)));
    }
}
