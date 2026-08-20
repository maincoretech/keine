use std::collections::HashMap;

use bevy::audio::{AudioSinkPlayback, PlaybackMode, Volume};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use keine_core::{BgmState, EffectCue, EffectEvent, EffectState};

use crate::runtime::audio::insert_player;
use crate::runtime::resources::{GameConfigResource, GameState};
use crate::storage::settings::RuntimeSettings;
use crate::ui::control_bar::{ButtonAction, ControlInput};

#[derive(Component)]
pub struct VocalPlayer {
    base_volume: f32,
    applied_volume: Option<f32>,
}

#[derive(Component)]
pub struct BgmPlayer {
    base_volume: f32,
    envelope: f32,
    fade_from: f32,
    elapsed: f32,
    duration: f32,
    direction: FadeDirection,
    applied_volume: Option<f32>,
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
    looping: bool,
    base_volume: f32,
    applied_volume: Option<f32>,
}

#[derive(Component)]
pub(crate) struct PlaybackEnvelope {
    gain: f32,
    from: f32,
    target: f32,
    elapsed: f32,
    duration: f32,
    despawn_on_finish: bool,
}

impl PlaybackEnvelope {
    fn fade_in(seconds: f32) -> Self {
        let duration = seconds.max(0.0);
        Self {
            gain: if duration > f32::EPSILON { 0.0 } else { 1.0 },
            from: 0.0,
            target: 1.0,
            elapsed: 0.0,
            duration,
            despawn_on_finish: false,
        }
    }

    fn fade_out(&mut self, seconds: f32) {
        let duration = seconds.max(0.0);
        self.from = self.gain;
        self.target = 0.0;
        self.elapsed = 0.0;
        self.duration = duration;
        self.despawn_on_finish = true;
        if duration <= f32::EPSILON {
            self.gain = 0.0;
        }
    }

    fn advance(&mut self, delta: f32) -> bool {
        if self.elapsed >= self.duration {
            return true;
        }
        self.elapsed = (self.elapsed + delta).min(self.duration);
        let progress = self.elapsed / self.duration;
        self.gain = self.from + (self.target - self.from) * progress;
        self.elapsed >= self.duration
    }

    fn is_animating(&self) -> bool {
        self.elapsed < self.duration
    }
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

#[derive(Default)]
struct EffectEventBatch {
    plays: Vec<EffectCue>,
    stop_all_one_shots: bool,
    one_shot_fade_outs: HashMap<String, f32>,
    loop_fade_ins: HashMap<String, f32>,
    loop_fade_outs: HashMap<String, f32>,
}

impl EffectEventBatch {
    fn drain(queue: &mut Vec<EffectEvent>) -> Self {
        let mut batch = Self::default();
        for event in std::mem::take(queue) {
            match event {
                EffectEvent::Play(cue) => batch.plays.push(cue),
                EffectEvent::Stop => {
                    // Commands spawn players only after this system flushes.
                    // Remove plays queued before the stop, while preserving
                    // later plays in their authored order.
                    batch.plays.clear();
                    batch.stop_all_one_shots = true;
                }
                EffectEvent::StopOneShot { id, fade_out } => {
                    batch
                        .plays
                        .retain(|cue| cue.id.as_deref() != Some(id.as_str()));
                    batch.one_shot_fade_outs.insert(id, fade_out.max(0.0));
                }
                EffectEvent::StartLoop { id, fade_in } => {
                    batch.loop_fade_ins.insert(id, fade_in.max(0.0));
                }
                EffectEvent::StopLoop { id, fade_out } => {
                    batch.loop_fade_outs.insert(id, fade_out.max(0.0));
                }
            }
        }
        batch
    }
}

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
            applied_volume: None,
        },
    ));
    insert_player(
        &mut entity,
        &context.asset_server,
        context.config.bgm_path(file),
        PlaybackSettings {
            mode: PlaybackMode::Loop,
            volume: Volume::Linear(if fading {
                0.0
            } else {
                base_volume * context.settings.master_volume * context.settings.bgm_volume
            }),
            ..default()
        },
    );
    context.activity.0 = fading;
}

pub fn animate_audio(
    time: Res<Time>,
    settings: Res<RuntimeSettings>,
    state: Res<GameState>,
    mut bgm_players: Query<(Entity, &mut BgmPlayer, Option<&mut AudioSink>)>,
    mut envelopes: Query<(Entity, &mut PlaybackEnvelope), Without<BgmPlayer>>,
    mut activity: ResMut<AudioAnimationActivity>,
    mut commands: Commands,
) {
    let mut animating = false;
    for (entity, mut player, sink) in &mut bgm_players {
        let sink_added = sink.as_ref().is_some_and(|sink| sink.is_added());
        if player.direction == FadeDirection::Settled
            && !settings.is_changed()
            && !state.is_changed()
            && !sink_added
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
            let volume = player.base_volume
                * settings.master_volume
                * settings.bgm_volume
                * player.envelope
                * if state.videos.is_empty() { 1.0 } else { 0.0 };
            if sink_added || player.applied_volume != Some(volume) {
                sink.set_volume(Volume::Linear(volume));
                player.applied_volume = Some(volume);
            }
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
    for (entity, mut envelope) in &mut envelopes {
        let finished = envelope.advance(time.delta_secs());
        if finished && envelope.despawn_on_finish {
            commands.entity(entity).despawn();
            continue;
        }
        animating |= envelope.is_animating();
    }
    if activity.0 != animating {
        activity.0 = animating;
    }
}

pub fn sync_effects(
    mut state: ResMut<GameState>,
    config: Res<GameConfigResource>,
    settings: Res<RuntimeSettings>,
    asset_server: Res<AssetServer>,
    mut playback: ResMut<EffectPlayback>,
    mut players: Query<(Entity, &EffectPlayer, &mut PlaybackEnvelope)>,
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
        for (entity, _, _) in &players {
            commands.entity(entity).despawn();
        }
        return;
    }

    let EffectEventBatch {
        plays,
        stop_all_one_shots,
        one_shot_fade_outs,
        loop_fade_ins,
        loop_fade_outs,
    } = EffectEventBatch::drain(&mut state.effect_queue);
    for cue in plays {
        spawn_effect(
            &mut commands,
            &asset_server,
            &config,
            &settings,
            EffectSpawn {
                id: cue.id,
                looping: false,
                file: &cue.file,
                volume: cue.volume,
                fade_in: cue.fade_in,
            },
        );
    }

    for (entity, player, mut envelope) in &mut players {
        if !player.looping {
            let fade_out = player
                .id
                .as_ref()
                .and_then(|id| one_shot_fade_outs.get(id))
                .copied();
            if stop_all_one_shots || fade_out == Some(0.0) {
                commands.entity(entity).despawn();
            } else if let Some(fade_out) = fade_out {
                envelope.fade_out(fade_out);
            }
            continue;
        }

        let Some(id) = &player.id else { continue };
        let current = state.looping_effects.get(id);
        let changed = current.is_some_and(|effect| {
            effect.file != playback.loops.get(id).map_or("", |old| old.file.as_str())
                || (effect.volume - player.base_volume).abs() > f32::EPSILON
        });
        if config_changed || changed {
            commands.entity(entity).despawn();
        } else if current.is_none() {
            match loop_fade_outs.get(id).copied().unwrap_or(0.0) {
                fade_out if fade_out > f32::EPSILON => envelope.fade_out(fade_out),
                _ => commands.entity(entity).despawn(),
            }
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
            EffectSpawn {
                id: Some(id.clone()),
                looping: true,
                file: &effect.file,
                volume: effect.volume,
                fade_in: loop_fade_ins.get(id).copied().unwrap_or(0.0),
            },
        );
    }
    playback.loops.clone_from(&state.looping_effects);
}

struct EffectSpawn<'a> {
    id: Option<String>,
    looping: bool,
    file: &'a str,
    volume: f32,
    fade_in: f32,
}

fn spawn_effect(
    commands: &mut Commands,
    asset_server: &AssetServer,
    config: &GameConfigResource,
    settings: &RuntimeSettings,
    effect: EffectSpawn<'_>,
) {
    let envelope = PlaybackEnvelope::fade_in(effect.fade_in);
    let mut entity = commands.spawn((
        Name::new(match &effect.id {
            Some(id) => format!("effect::{id}::{}", effect.file),
            None => format!("effect::{}", effect.file),
        }),
        EffectPlayer {
            id: effect.id,
            looping: effect.looping,
            base_volume: effect.volume,
            applied_volume: None,
        },
        envelope,
    ));
    insert_player(
        &mut entity,
        asset_server,
        config.effect_path(effect.file),
        PlaybackSettings {
            mode: if effect.looping {
                PlaybackMode::Loop
            } else {
                PlaybackMode::Despawn
            },
            volume: Volume::Linear(
                effect.volume
                    * settings.master_volume
                    * settings.se_volume
                    * if effect.fade_in > f32::EPSILON {
                        0.0
                    } else {
                        1.0
                    },
            ),
            ..default()
        },
    );
}

type EffectSinkQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut EffectPlayer,
        &'static PlaybackEnvelope,
        &'static mut AudioSink,
    ),
    (With<EffectPlayer>, Without<VocalPlayer>),
>;
type VocalSinkQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut VocalPlayer,
        &'static PlaybackEnvelope,
        &'static mut AudioSink,
    ),
    (With<VocalPlayer>, Without<EffectPlayer>),
>;

pub fn apply_bus_volumes(
    settings: Res<RuntimeSettings>,
    state: Res<GameState>,
    mut sinks: ParamSet<(VocalSinkQuery, EffectSinkQuery)>,
) {
    let video_duck = if state.videos.is_empty() { 1.0 } else { 0.0 };
    let vocal_bus = settings.master_volume * settings.vocal_volume * video_duck;
    let effect_bus = settings.master_volume * settings.se_volume;
    for (mut player, envelope, mut sink) in &mut sinks.p0() {
        let volume = player.base_volume * vocal_bus * envelope.gain;
        if sink.is_added() || player.applied_volume != Some(volume) {
            sink.set_volume(Volume::Linear(volume));
            player.applied_volume = Some(volume);
        }
    }
    for (mut player, envelope, mut sink) in &mut sinks.p1() {
        let volume = player.base_volume * effect_bus * envelope.gain;
        if sink.is_added() || player.applied_volume != Some(volume) {
            sink.set_volume(Volume::Linear(volume));
            player.applied_volume = Some(volume);
        }
    }
}

pub fn sync_vocal(
    mut state: ResMut<GameState>,
    config: Res<GameConfigResource>,
    settings: Res<RuntimeSettings>,
    asset_server: Res<AssetServer>,
    mut playback: ResMut<VocalPlayback>,
    mut players: Query<(Entity, &mut PlaybackEnvelope), With<VocalPlayer>>,
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
        if let Some(file) = cue.file {
            for (entity, _) in &mut players {
                commands.entity(entity).despawn();
            }
            spawn_vocal(
                &mut commands,
                &asset_server,
                &config,
                &settings,
                cue.volume,
                &file,
                cue.fade_in,
            );
        } else {
            for (entity, mut envelope) in &mut players {
                if cue.fade_out <= f32::EPSILON {
                    commands.entity(entity).despawn();
                } else {
                    envelope.fade_out(cue.fade_out);
                }
            }
        }
        return;
    }
    if playback.key == key && !config.is_changed() {
        return;
    }
    playback.key.clone_from(&key);
    for (entity, _) in &mut players {
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
            0.0,
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
        0.0,
    );
}

pub(crate) fn spawn_vocal(
    commands: &mut Commands,
    asset_server: &AssetServer,
    config: &GameConfigResource,
    settings: &RuntimeSettings,
    line_volume: f32,
    vocal: &str,
    fade_in: f32,
) {
    let envelope = PlaybackEnvelope::fade_in(fade_in);
    let mut entity = commands.spawn((
        Name::new(format!("vocal::{vocal}")),
        VocalPlayer {
            base_volume: line_volume,
            applied_volume: None,
        },
        envelope,
    ));
    insert_player(
        &mut entity,
        asset_server,
        config.voice_path(vocal),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(
                line_volume
                    * settings.master_volume
                    * settings.vocal_volume
                    * if fade_in > f32::EPSILON { 0.0 } else { 1.0 },
            ),
            ..default()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_envelope_fades_in_and_then_out_from_its_current_gain() {
        let mut envelope = PlaybackEnvelope::fade_in(2.0);
        assert_eq!(envelope.gain, 0.0);
        assert!(!envelope.advance(0.5));
        assert!((envelope.gain - 0.25).abs() < f32::EPSILON);

        envelope.fade_out(1.0);
        assert!(!envelope.advance(0.5));
        assert!((envelope.gain - 0.125).abs() < f32::EPSILON);
        assert!(envelope.advance(0.5));
        assert_eq!(envelope.gain, 0.0);
        assert!(envelope.despawn_on_finish);
    }

    #[test]
    fn zero_duration_envelopes_settle_immediately() {
        let mut envelope = PlaybackEnvelope::fade_in(0.0);
        assert_eq!(envelope.gain, 1.0);
        assert!(envelope.advance(0.0));
        envelope.fade_out(0.0);
        assert_eq!(envelope.gain, 0.0);
        assert!(envelope.advance(0.0));
    }

    #[test]
    fn effect_event_batch_preserves_stop_then_play() {
        let mut queue = vec![
            EffectEvent::Stop,
            EffectEvent::Play(EffectCue {
                id: None,
                file: "click.opus".into(),
                volume: 1.0,
                fade_in: 0.0,
            }),
        ];

        let batch = EffectEventBatch::drain(&mut queue);

        assert!(batch.stop_all_one_shots);
        assert_eq!(batch.plays.len(), 1);
        assert_eq!(batch.plays[0].file, "click.opus");
    }

    #[test]
    fn effect_event_batch_drops_play_before_stop() {
        let mut queue = vec![
            EffectEvent::Play(EffectCue {
                id: None,
                file: "old.opus".into(),
                volume: 1.0,
                fade_in: 0.0,
            }),
            EffectEvent::Stop,
        ];

        let batch = EffectEventBatch::drain(&mut queue);

        assert!(batch.stop_all_one_shots);
        assert!(batch.plays.is_empty());
    }

    #[test]
    fn effect_event_batch_keeps_only_play_after_global_stop() {
        let mut queue = vec![
            EffectEvent::Play(EffectCue {
                id: None,
                file: "first.opus".into(),
                volume: 0.5,
                fade_in: 0.1,
            }),
            EffectEvent::Play(EffectCue {
                id: Some("timeline:second".into()),
                file: "second.opus".into(),
                volume: 0.7,
                fade_in: 0.2,
            }),
            EffectEvent::Stop,
            EffectEvent::Play(EffectCue {
                id: Some("timeline:third".into()),
                file: "third.opus".into(),
                volume: 0.9,
                fade_in: 0.0,
            }),
        ];

        let batch = EffectEventBatch::drain(&mut queue);

        assert!(queue.is_empty());
        assert!(batch.stop_all_one_shots);
        assert_eq!(batch.plays.len(), 1);
        assert_eq!(batch.plays[0].file, "third.opus");
    }

    #[test]
    fn effect_event_batch_keeps_named_play_after_named_stop() {
        let id = "timeline:second";
        let mut queue = vec![
            EffectEvent::StopOneShot {
                id: id.into(),
                fade_out: 0.3,
            },
            EffectEvent::Play(EffectCue {
                id: Some(id.into()),
                file: "replacement.opus".into(),
                volume: 0.7,
                fade_in: 0.2,
            }),
        ];

        let batch = EffectEventBatch::drain(&mut queue);

        assert!(queue.is_empty());
        assert_eq!(batch.plays.len(), 1);
        assert_eq!(batch.plays[0].file, "replacement.opus");
        assert_eq!(batch.one_shot_fade_outs[id], 0.3);
    }
}
