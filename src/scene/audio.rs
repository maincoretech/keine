use std::collections::HashMap;

use bevy::audio::{AudioSinkPlayback, PlaybackMode, Volume};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use keine_core::{BgmState, EffectEvent, EffectState};

use crate::runtime::audio::insert_player;
use crate::runtime::resources::{GameConfigResource, GameState};
use crate::storage::settings::RuntimeSettings;
use crate::ui::control_bar::{ButtonAction, ControlInput};

#[derive(Component)]
pub struct VocalPlayer;

#[derive(Component)]
pub struct BgmPlayer {
    base_volume: f32,
    envelope: f32,
    fade_from: f32,
    elapsed: f32,
    duration: f32,
    direction: FadeDirection,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FadeDirection {
    In,
    Out,
    Settled,
}

#[derive(Component)]
pub struct EffectPlayer {
    id: Option<String>,
    base_volume: f32,
}

#[derive(Resource, Default)]
pub struct VocalPlayback {
    key: Option<(String, usize, String)>,
}

#[derive(Resource, Default)]
pub struct BgmPlayback {
    applied: Option<BgmState>,
}

#[derive(Resource, Default)]
pub struct EffectPlayback {
    loops: HashMap<String, EffectState>,
}

#[derive(Resource, Default, Deref)]
pub struct AudioAnimationActivity(pub bool);

#[derive(SystemParam)]
pub struct BgmSyncContext<'w> {
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    settings: Res<'w, RuntimeSettings>,
    asset_server: Res<'w, AssetServer>,
    playback: ResMut<'w, BgmPlayback>,
    activity: ResMut<'w, AudioAnimationActivity>,
}

pub fn sync_bgm(
    mut context: BgmSyncContext,
    mut players: Query<(Entity, &mut BgmPlayer)>,
    mut commands: Commands,
) {
    if context.playback.applied.as_ref() == Some(&context.state.bgm) && !context.config.is_changed()
    {
        return;
    }
    context.playback.applied = Some(context.state.bgm.clone());
    let duration = context.state.bgm.fade_seconds.max(0.0);

    let Some(file) = &context.state.bgm.file else {
        for (entity, mut player) in &mut players {
            if duration <= f32::EPSILON {
                commands.entity(entity).despawn();
            } else {
                player.elapsed = 0.0;
                player.duration = duration;
                player.fade_from = player.envelope;
                player.direction = FadeDirection::Out;
                context.activity.0 = true;
            }
        }
        return;
    };

    for (entity, _) in &players {
        commands.entity(entity).despawn();
    }
    let fading = duration > f32::EPSILON;
    let base_volume = context.state.bgm.volume.clamp(0.0, 1.0);
    let mut entity = commands.spawn((
        Name::new(format!("bgm::{file}")),
        BgmPlayer {
            base_volume,
            envelope: if fading { 0.0 } else { 1.0 },
            fade_from: 0.0,
            elapsed: if fading { 0.0 } else { duration },
            duration,
            direction: if fading {
                FadeDirection::In
            } else {
                FadeDirection::Settled
            },
        },
        PlaybackSettings {
            mode: PlaybackMode::Loop,
            volume: Volume::Linear(if fading {
                0.0
            } else {
                base_volume * context.settings.master_volume * context.settings.bgm_volume
            }),
            ..default()
        },
    ));
    insert_player(
        &mut entity,
        &context.asset_server,
        context.config.bgm_path(file),
    );
    context.activity.0 = fading;
}

pub fn animate_bgm(
    time: Res<Time>,
    settings: Res<RuntimeSettings>,
    state: Res<GameState>,
    mut players: Query<(Entity, &mut BgmPlayer, Option<&mut AudioSink>)>,
    mut activity: ResMut<AudioAnimationActivity>,
    mut commands: Commands,
) {
    let mut animating = false;
    for (entity, mut player, sink) in &mut players {
        if player.direction == FadeDirection::Settled
            && !settings.is_changed()
            && !state.is_changed()
        {
            continue;
        }
        if player.direction != FadeDirection::Settled {
            player.elapsed = (player.elapsed + time.delta_secs()).min(player.duration);
        }
        let progress = if player.duration <= f32::EPSILON {
            1.0
        } else {
            (player.elapsed / player.duration).clamp(0.0, 1.0)
        };
        player.envelope = match player.direction {
            FadeDirection::In => progress,
            FadeDirection::Out => player.fade_from * (1.0 - progress),
            FadeDirection::Settled => 1.0,
        };
        if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(
                player.base_volume
                    * settings.master_volume
                    * settings.bgm_volume
                    * player.envelope
                    * if state.videos.is_empty() { 1.0 } else { 0.0 },
            ));
        }
        if progress >= 1.0 {
            match player.direction {
                FadeDirection::Out => {
                    commands.entity(entity).despawn();
                    continue;
                }
                FadeDirection::In => player.direction = FadeDirection::Settled,
                FadeDirection::Settled => {}
            }
        }
        animating |= player.direction != FadeDirection::Settled;
    }
    activity.0 = animating;
}

pub fn sync_effects(
    mut state: ResMut<GameState>,
    config: Res<GameConfigResource>,
    settings: Res<RuntimeSettings>,
    asset_server: Res<AssetServer>,
    mut playback: ResMut<EffectPlayback>,
    players: Query<(Entity, &EffectPlayer)>,
    mut commands: Commands,
) {
    let has_event = !state.effect_queue.is_empty();
    let config_changed = config.is_changed();
    let loops_changed = config_changed || playback.loops != state.looping_effects;
    let needs_title_cleanup = state.ended && (has_event || loops_changed || !players.is_empty());
    if !has_event && !loops_changed && !needs_title_cleanup {
        return;
    }
    if needs_title_cleanup {
        state.effect_queue.clear();
        playback.loops.clear();
        for (entity, _) in &players {
            commands.entity(entity).despawn();
        }
        return;
    }
    if has_event && let Some(event) = state.effect_queue.pop() {
        state.effect_queue.clear();
        for (entity, player) in &players {
            if player.id.is_none() {
                commands.entity(entity).despawn();
            }
        }
        if let EffectEvent::Play(cue) = event {
            spawn_effect(
                &mut commands,
                &asset_server,
                &config,
                &settings,
                None,
                &cue.file,
                cue.volume,
            );
        }
    }

    if playback.loops == state.looping_effects {
        return;
    }
    for (entity, player) in &players {
        let Some(id) = &player.id else { continue };
        if config_changed
            || state.looping_effects.get(id).is_none_or(|effect| {
                effect.file != playback.loops.get(id).map_or("", |old| old.file.as_str())
                    || (effect.volume - player.base_volume).abs() > f32::EPSILON
            })
        {
            commands.entity(entity).despawn();
        }
    }
    for (id, effect) in &state.looping_effects {
        if !config_changed && playback.loops.get(id) == Some(effect) {
            continue;
        }
        spawn_effect(
            &mut commands,
            &asset_server,
            &config,
            &settings,
            Some(id.clone()),
            &effect.file,
            effect.volume,
        );
    }
    playback.loops.clone_from(&state.looping_effects);
}

fn spawn_effect(
    commands: &mut Commands,
    asset_server: &AssetServer,
    config: &GameConfigResource,
    settings: &RuntimeSettings,
    id: Option<String>,
    file: &str,
    volume: f32,
) {
    let looping = id.is_some();
    let mut entity = commands.spawn((
        Name::new(match &id {
            Some(id) => format!("effect::{id}::{file}"),
            None => format!("effect::{file}"),
        }),
        EffectPlayer {
            id,
            base_volume: volume,
        },
        PlaybackSettings {
            mode: if looping {
                PlaybackMode::Loop
            } else {
                PlaybackMode::Despawn
            },
            volume: Volume::Linear(volume * settings.master_volume * settings.se_volume),
            ..default()
        },
    ));
    insert_player(&mut entity, asset_server, config.effect_path(file));
}

type EffectSinkQuery<'w, 's> = Query<
    'w,
    's,
    (&'static EffectPlayer, &'static mut AudioSink),
    (With<EffectPlayer>, Without<VocalPlayer>),
>;

pub fn apply_bus_volumes(
    settings: Res<RuntimeSettings>,
    state: Res<GameState>,
    mut vocals: Query<&mut AudioSink, (With<VocalPlayer>, Without<BgmPlayer>)>,
    mut effects: EffectSinkQuery,
) {
    if !settings.is_changed() && !state.is_changed() {
        return;
    }
    let line_volume = state
        .dialogue
        .as_ref()
        .map_or(1.0, |dialogue| dialogue.volume);
    let video_duck = if state.videos.is_empty() { 1.0 } else { 0.0 };
    for mut sink in &mut vocals {
        sink.set_volume(Volume::Linear(
            line_volume * settings.master_volume * settings.vocal_volume * video_duck,
        ));
    }
    for (player, mut sink) in &mut effects {
        sink.set_volume(Volume::Linear(
            player.base_volume * settings.master_volume * settings.se_volume,
        ));
    }
}

pub fn sync_vocal(
    mut state: ResMut<GameState>,
    config: Res<GameConfigResource>,
    settings: Res<RuntimeSettings>,
    asset_server: Res<AssetServer>,
    mut playback: ResMut<VocalPlayback>,
    players: Query<Entity, With<VocalPlayer>>,
    mut commands: Commands,
) {
    let key = state.dialogue.as_ref().and_then(|dialogue| {
        dialogue
            .vocal
            .as_ref()
            .map(|vocal| (state.current_scene.clone(), state.cursor, vocal.clone()))
    });
    if let Some(cue) = state.vocal_event.take() {
        playback.key.clone_from(&key);
        for entity in &players {
            commands.entity(entity).despawn();
        }
        if let Some(file) = cue.file {
            spawn_vocal(
                &mut commands,
                &asset_server,
                &config,
                &settings,
                cue.volume,
                &file,
            );
        }
        return;
    }
    if playback.key == key && !config.is_changed() {
        return;
    }
    playback.key.clone_from(&key);
    for entity in &players {
        commands.entity(entity).despawn();
    }
    if let Some((_, _, vocal)) = key {
        spawn_vocal(
            &mut commands,
            &asset_server,
            &config,
            &settings,
            state.dialogue.as_ref().map_or(1.0, |line| line.volume),
            &vocal,
        );
    }
}

pub fn replay_vocal(
    input: ControlInput,
    state: Res<GameState>,
    config: Res<GameConfigResource>,
    settings: Res<RuntimeSettings>,
    asset_server: Res<AssetServer>,
    players: Query<Entity, With<VocalPlayer>>,
    mut commands: Commands,
) {
    if !input.pressed_on_stage(ButtonAction::Replay) {
        return;
    }
    let Some(dialogue) = state.dialogue.as_ref() else {
        return;
    };
    let Some(vocal) = dialogue.vocal.as_deref() else {
        return;
    };
    for entity in &players {
        commands.entity(entity).despawn();
    }
    spawn_vocal(
        &mut commands,
        &asset_server,
        &config,
        &settings,
        dialogue.volume,
        vocal,
    );
}

fn spawn_vocal(
    commands: &mut Commands,
    asset_server: &AssetServer,
    config: &GameConfigResource,
    settings: &RuntimeSettings,
    line_volume: f32,
    vocal: &str,
) {
    let mut entity = commands.spawn((
        Name::new(format!("vocal::{vocal}")),
        VocalPlayer,
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(line_volume * settings.master_volume * settings.vocal_volume),
            ..default()
        },
    ));
    insert_player(&mut entity, asset_server, config.voice_path(vocal));
}
