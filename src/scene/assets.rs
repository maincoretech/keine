use std::collections::{HashMap, HashSet};

use bevy::asset::LoadState;
use bevy::prelude::*;
use keine_loader::ResourceKind;

use crate::runtime::resources::{
    AssetLoadingGate, GameConfigResource, GameState, LocalAssetCache, LocalAssetManifest,
};
use crate::scene::effects::material::active_lut_preset;
use crate::scene::images::{ImageRole, ImageRoleRegistry};
use crate::ui::foundation::UiFonts;

const LOOKAHEAD_ACTIONS: usize = 20;
const MAX_PREDICTED_ASSETS: usize = 8;
const MAX_SPECULATIVE_LOADS: usize = 1;

fn prefetch_action_window(cursor: usize) -> std::ops::RangeInclusive<usize> {
    // Script execution advances the cursor as soon as an action starts. Keep
    // that active action in the warm set so long timelines can load embedded
    // event assets before their authored trigger time.
    cursor.saturating_sub(1)..=cursor.saturating_add(LOOKAHEAD_ACTIONS)
}

#[derive(Default)]
struct AssetPlan {
    critical: HashMap<String, ResourceKind>,
    urgent: Vec<(String, ResourceKind)>,
    predicted: Vec<(String, ResourceKind)>,
    speculative: HashSet<String>,
}

impl AssetPlan {
    fn warm_urgent(&mut self, path: String, kind: ResourceKind) {
        if kind == ResourceKind::Video || self.critical.contains_key(&path) {
            return;
        }
        if let Some(index) = self
            .predicted
            .iter()
            .position(|(candidate, _)| candidate == &path)
        {
            self.predicted.remove(index);
            self.urgent.push((path, kind));
        } else if self.speculative.insert(path.clone()) {
            self.urgent.push((path, kind));
        }
    }

    fn warm_predicted(&mut self, path: String, kind: ResourceKind) {
        if kind != ResourceKind::Video
            && !self.critical.contains_key(&path)
            && self.speculative.insert(path.clone())
        {
            self.predicted.push((path, kind));
        }
    }

    fn require(&mut self, path: String, kind: ResourceKind) {
        if kind == ResourceKind::Video {
            return;
        }
        self.urgent.retain(|(candidate, _)| candidate != &path);
        self.predicted.retain(|(candidate, _)| candidate != &path);
        self.speculative.remove(&path);
        self.critical.insert(path, kind);
    }

    fn speculative_assets(&self) -> impl Iterator<Item = &(String, ResourceKind)> {
        self.urgent
            .iter()
            .chain(self.predicted.iter().take(MAX_PREDICTED_ASSETS))
    }

    fn retains(&self, path: &str) -> bool {
        self.critical.contains_key(path)
            || self
                .speculative_assets()
                .any(|(candidate, _)| candidate == path)
    }

    fn fully_admitted(&self, cache: &LocalAssetCache) -> bool {
        self.critical
            .keys()
            .chain(self.speculative_assets().map(|(path, _)| path))
            .all(|path| cache.handles.contains_key(path))
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
    lut: Option<String>,
    plan: AssetPlan,
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
            && self.lut.as_deref() == active_lut_preset(&state.camera_effect)
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
        self.lut = active_lut_preset(&state.camera_effect).map(str::to_owned);
    }
}

pub fn prefetch_local_assets(
    state: Res<GameState>,
    config: Res<GameConfigResource>,
    manifest: Res<LocalAssetManifest>,
    image_roles: Res<ImageRoleRegistry>,
    asset_server: Res<AssetServer>,
    mut cache: ResMut<LocalAssetCache>,
    mut previous: Local<PrefetchState>,
) {
    let rebuild = !previous.matches(&state) || manifest.is_changed() || config.is_changed();
    if rebuild {
        previous.capture(&state);
        previous.plan = build_asset_plan(&state, &config, &manifest);
        cache.handles.retain(|path, _| previous.plan.retains(path));
    } else if previous.plan.fully_admitted(&cache) {
        return;
    }

    for (path, kind) in &previous.plan.critical {
        cache
            .handles
            .entry(path.clone())
            .or_insert_with(|| load_handle(&asset_server, &image_roles, path.clone(), *kind));
    }
    let critical = previous.plan.critical.keys().cloned().collect();
    if cache.critical != critical {
        cache.critical = critical;
    }

    let pending = |handle: &UntypedHandle| {
        matches!(
            asset_server.load_state(handle.id()),
            LoadState::NotLoaded | LoadState::Loading
        )
    };
    if cache.blocking_handles().any(pending) {
        return;
    }
    let speculative_in_flight = cache
        .handles
        .iter()
        .filter(|(path, handle)| !cache.critical.contains(*path) && pending(handle))
        .count();
    let available = MAX_SPECULATIVE_LOADS.saturating_sub(speculative_in_flight);
    let next = (available != 0)
        .then(|| {
            previous
                .plan
                .speculative_assets()
                .find(|(path, _)| !cache.handles.contains_key(path))
                .cloned()
        })
        .flatten();
    if let Some((path, kind)) = next {
        cache.handles.insert(
            path.clone(),
            load_handle(&asset_server, &image_roles, path, kind),
        );
    }
}

fn build_asset_plan(
    state: &GameState,
    config: &GameConfigResource,
    manifest: &LocalAssetManifest,
) -> AssetPlan {
    let mut plan = AssetPlan::default();
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
            plan.warm_urgent(config.figure_path(frame), ResourceKind::Figure);
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
    if let Some(lut) = active_lut_preset(&state.camera_effect) {
        plan.require(config.lut_path(lut), ResourceKind::Lut);
    }

    // Predictions are admitted only after the currently visible state is
    // ready. This prevents title/gameplay latency from competing with a burst
    // of speculative decode work on slower CPUs and storage.
    let (scene_name, cursor) = if state.ended {
        (crate::scene::entry_scene(state), 0)
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
            plan.warm_predicted(resource.resolved_path(config), resource.kind);
        }
        let call_end = cursor.saturating_add(LOOKAHEAD_ACTIONS);
        for reference in scene.sub_scenes.iter().filter(|reference| {
            reference.action_index >= cursor && reference.action_index <= call_end
        }) {
            if let Some(called_scene) = manifest.get(&reference.scene) {
                for resource in called_scene
                    .resources
                    .iter()
                    .filter(|resource| resource.action_index <= LOOKAHEAD_ACTIONS)
                {
                    plan.warm_predicted(resource.resolved_path(config), resource.kind);
                }
            }
        }
    }
    plan
}

fn load_handle(
    asset_server: &AssetServer,
    image_roles: &ImageRoleRegistry,
    path: String,
    kind: ResourceKind,
) -> UntypedHandle {
    match kind {
        ResourceKind::Background => {
            crate::scene::images::load(asset_server, image_roles, path, ImageRole::BACKGROUND)
                .untyped()
        }
        ResourceKind::Figure | ResourceKind::MiniAvatar => {
            crate::scene::images::load(asset_server, image_roles, path, ImageRole::FIGURE).untyped()
        }
        ResourceKind::Particle | ResourceKind::Lut => {
            crate::scene::images::load(asset_server, image_roles, path, ImageRole::RAW).untyped()
        }
        ResourceKind::Voice | ResourceKind::Bgm | ResourceKind::Effect => {
            crate::runtime::audio::load_untyped(asset_server, path)
        }
        ResourceKind::Video => unreachable!("video prefetch is handled separately"),
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
    gate.blocked = cache.blocking_handles().any(|handle| pending(handle.id()))
        || pending(fonts.text.id().untyped())
        || pending(fonts.icons.id().untyped());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_lut_does_not_enter_the_prefetch_identity() {
        let mut state = keine_core::State::new();
        state.camera_effect.lut_preset = Some("warm".into());
        let mut previous = PrefetchState::default();
        previous.capture(&GameState(state.clone()));

        assert_eq!(previous.lut, None);
        state.camera_effect.lut_intensity = 0.5;
        assert!(!previous.matches(&GameState(state.clone())));
        previous.capture(&GameState(state));
        assert_eq!(previous.lut.as_deref(), Some("warm"));
    }

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
        plan.warm_predicted("background/later.webp".into(), ResourceKind::Background);
        plan.require("background/current.webp".into(), ResourceKind::Background);

        assert!(plan.retains("background/later.webp"));
        assert!(plan.retains("background/current.webp"));
        assert_eq!(plan.critical.len(), 1);
        assert!(plan.critical.contains_key("background/current.webp"));
    }

    #[test]
    fn speculative_plan_prioritizes_active_assets_and_bounds_predictions() {
        let mut plan = AssetPlan::default();
        for index in 0..MAX_PREDICTED_ASSETS + 2 {
            plan.warm_predicted(format!("voice/{index}.opus"), ResourceKind::Voice);
        }
        plan.warm_urgent("figure/animation.webp".into(), ResourceKind::Figure);

        let paths = plan
            .speculative_assets()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), MAX_PREDICTED_ASSETS + 1);
        assert_eq!(paths[0], "figure/animation.webp");
        assert!(!paths.contains(&format!("voice/{}.opus", MAX_PREDICTED_ASSETS + 1).as_str()));
    }

    #[test]
    fn streamed_video_is_never_added_to_the_generic_asset_plan() {
        let mut plan = AssetPlan::default();
        plan.warm_predicted("video/opening.mp4".into(), ResourceKind::Video);
        plan.require("video/current.mp4".into(), ResourceKind::Video);

        assert!(plan.speculative_assets().next().is_none());
        assert!(plan.critical.is_empty());
    }
}
