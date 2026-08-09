pub mod assets;
pub mod audio;
pub mod background;
pub(crate) mod effects;
pub(crate) mod images;
pub mod sprites;
pub(crate) mod video;

#[cfg(all(feature = "video-native", target_os = "macos"))]
pub(crate) use video::validate_native_video;

use bevy::prelude::*;

use crate::runtime::GameSystemSet;

pub(crate) struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(effects::StageEffectsPlugin)
            .add_plugins(video::VideoPlugin)
            .insert_resource(audio::VocalPlayback::default())
            .init_resource::<audio::BgmPlayback>()
            .init_resource::<audio::EffectPlayback>()
            .init_resource::<audio::AudioAnimationActivity>()
            .init_resource::<images::ImageDimensions>()
            .init_resource::<images::PreparedImages>()
            .init_resource::<crate::runtime::resources::AssetLoadingGate>();
        app.add_systems(
            Update,
            (
                (
                    assets::prefetch_local_assets,
                    images::prepare,
                    assets::update_loading_gate,
                )
                    .chain(),
                background::sync_bg,
                sprites::sync_sprites,
                (
                    audio::sync_bgm,
                    audio::sync_effects,
                    audio::sync_vocal,
                    audio::replay_vocal,
                    audio::animate_bgm,
                    audio::apply_bus_volumes,
                )
                    .chain(),
            )
                .in_set(GameSystemSet::Sync),
        );
    }
}

use keine_core::State;

/// Prefer WebGAL's conventional `start`, with `main` as a language-neutral fallback.
pub fn entry_scene(state: &State) -> String {
    ["start", "main"]
        .into_iter()
        .find(|name| state.program.contains_scene(name))
        .map(str::to_owned)
        .or_else(|| state.program.scene_names().min().map(str::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_scene_prefers_start_then_main() {
        let mut state = State::new();
        state.insert_scene("chapter".into(), Vec::new());
        state.insert_scene("main".into(), Vec::new());
        assert_eq!(entry_scene(&state), "main");
        state.insert_scene("start".into(), Vec::new());
        assert_eq!(entry_scene(&state), "start");
    }
}
