use std::collections::{HashMap, HashSet};

use bevy::asset::LoadState;
use bevy::prelude::*;
use keine_loader::ResourceKind;

use crate::runtime::resources::{
    AssetLoadingGate, GameConfigResource, GameState, LocalAssetCache, LocalAssetManifest,
};
use crate::ui::foundation::UiFonts;

const LOOKAHEAD_ACTIONS: usize = 20;

fn prefetch_action_window(cursor: usize) -> std::ops::RangeInclusive<usize> {
    // Script execution advances the cursor as soon as an action starts. Keep
    // that active action in the warm set so long timelines can load embedded
    // event assets before their authored trigger time.
    cursor.saturating_sub(1)..=cursor.saturating_add(LOOKAHEAD_ACTIONS)
}

#[derive(Default)]
struct AssetPlan {
    retained: HashMap<String, ResourceKind>,
    critical: HashSet<String>,
}

impl AssetPlan {
    fn warm(&mut self, path: String, kind: ResourceKind) {
        if kind != ResourceKind::Video {
            self.retained.insert(path, kind);
        }
    }

    fn require(&mut self, path: String, kind: ResourceKind) {
        if kind != ResourceKind::Video {
            self.critical.insert(path.clone());
            self.retained.insert(path, kind);
        }
    }
}

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
    let mut plan = AssetPlan::default();
    // While the title is open, use otherwise idle time to warm the entry scene.
    // This also keeps its handles alive after returning from the game, instead
    // of releasing and recreating them on the next START click.
    let (scene_name, cursor) = if state.ended {
        (crate::scene::entry_scene(&state), 0)
    } else {
        (state.current_scene.clone(), state.cursor)
    };
    if let Some(scene) = manifest.get(&scene_name) {
        let window = prefetch_action_window(cursor);
        for resource in scene
            .resources
            .iter()
            .filter(|resource| window.contains(&resource.action_index))
        {
            let path = resource.resolved_path(&config);
            plan.warm(path, resource.kind);
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
                    plan.warm(path, resource.kind);
                }
            }
        }
    }

    if state.ended {
        plan.require(
            config.bg_path(&config.title_background),
            ResourceKind::Background,
        );
    }

    if let Some(background) = &state.bg {
        plan.require(config.bg_path(background), ResourceKind::Background);
    }
    if let Some(transition) = &state.bg_transition {
        if let Some(background) = &transition.from {
            plan.require(config.bg_path(background), ResourceKind::Background);
        }
        if !transition.to.is_empty() {
            plan.require(config.bg_path(&transition.to), ResourceKind::Background);
        }
    }
    for sprite in state.sprites.values() {
        plan.require(config.figure_path(&sprite.image), ResourceKind::Figure);
    }
    for sequence in state.sprite_sequences.values() {
        for frame in &sequence.frames {
            plan.warm(config.figure_path(frame), ResourceKind::Figure);
        }
    }
    if let Some(avatar) = &state.mini_avatar {
        plan.require(config.figure_path(avatar), ResourceKind::MiniAvatar);
    }
    if let Some(vocal) = state
        .dialogue
        .as_ref()
        .and_then(|dialogue| dialogue.vocal.as_ref())
    {
        plan.require(config.voice_path(vocal), ResourceKind::Voice);
    }
    if let Some(bgm) = &state.bgm.file {
        plan.require(config.bgm_path(bgm), ResourceKind::Bgm);
    }
    for effect in state.looping_effects.values() {
        plan.require(config.effect_path(&effect.file), ResourceKind::Effect);
    }
    for effect in state.particle_effects.values() {
        if let Some(texture) = effect
            .effect
            .texture
            .as_ref()
            .filter(|path| !path.is_empty())
        {
            plan.require(texture.clone(), ResourceKind::Particle);
        }
    }

    if cache.handles.len() == plan.retained.len()
        && plan
            .retained
            .keys()
            .all(|path| cache.handles.contains_key(path))
        && cache.critical == plan.critical
    {
        return;
    }
    cache
        .handles
        .retain(|path, _| plan.retained.contains_key(path));
    for (path, kind) in plan.retained {
        cache
            .handles
            .entry(path.clone())
            .or_insert_with(|| match kind {
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
    cache.critical = plan.critical;
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
    gate.blocked = cache.blocking_handles().any(|handle| pending(handle.id()))
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

    #[test]
    fn prefetch_window_keeps_the_action_that_advanced_the_cursor() {
        let window = prefetch_action_window(12);

        assert!(window.contains(&11));
        assert!(window.contains(&12));
        assert!(window.contains(&(12 + LOOKAHEAD_ACTIONS)));
        assert!(!window.contains(&10));
    }

    #[test]
    fn predicted_assets_are_retained_without_becoming_critical() {
        let mut plan = AssetPlan::default();
        plan.warm("background/later.webp".into(), ResourceKind::Background);
        plan.require("background/current.webp".into(), ResourceKind::Background);

        assert_eq!(plan.retained.len(), 2);
        assert_eq!(
            plan.critical,
            HashSet::from(["background/current.webp".into()])
        );
    }

    #[test]
    fn streamed_video_is_never_added_to_the_generic_asset_plan() {
        let mut plan = AssetPlan::default();
        plan.warm("video/opening.mp4".into(), ResourceKind::Video);
        plan.require("video/current.mp4".into(), ResourceKind::Video);

        assert!(plan.retained.is_empty());
        assert!(plan.critical.is_empty());
    }
}
