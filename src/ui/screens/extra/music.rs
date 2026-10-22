//! Gallery music transport, seek controls and list scrolling.
use super::*;
use crate::ui::foundation::{Button, Interaction};

#[derive(SystemParam)]
pub(crate) struct ExtraBgmContext<'w, 's> {
    full: Query<'w, 's, (), With<ExtraFullCg>>,
    transition: Res<'w, ExtraPageTransition>,
    tracks: Query<'w, 's, (&'static Interaction, &'static ExtraBgm), Changed<Interaction>>,
    controls: Query<'w, 's, (&'static Interaction, &'static ExtraBgmControl), Changed<Interaction>>,
    ui: ResMut<'w, ExtraUi>,
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    settings: Res<'w, RuntimeSettings>,
    assets: Res<'w, AssetServer>,
    players: Query<
        'w,
        's,
        (
            Entity,
            &'static mut ExtraBgmPlayer,
            Option<&'static mut AudioSink>,
        ),
    >,
    stage_bgm: StageBgmQuery<'w, 's>,
    seek_bars: Query<'w, 's, &'static mut ExtraBgmSeekBar>,
    commands: Commands<'w, 's>,
}

#[derive(SystemParam)]
pub(crate) struct ExtraAudioAssets<'w> {
    #[cfg(feature = "audio-opus")]
    opus: Res<'w, Assets<crate::runtime::audio::OpusAudio>>,
    #[cfg(feature = "audio-seekable")]
    seekable: Res<'w, Assets<crate::runtime::audio::SeekableAudio>>,
    #[cfg(not(any(feature = "audio-opus", feature = "audio-seekable")))]
    _fallback: Res<'w, AssetServer>,
}

impl ExtraAudioAssets<'_> {
    fn duration(&self, audio: Option<&GalleryAudio>) -> Option<Duration> {
        match audio {
            #[cfg(feature = "audio-opus")]
            Some(GalleryAudio::Opus(handle)) => self
                .opus
                .get(handle)
                .and_then(crate::runtime::audio::OpusAudio::duration),
            #[cfg(feature = "audio-seekable")]
            Some(GalleryAudio::Seekable(handle)) => self
                .seekable
                .get(handle)
                .and_then(crate::runtime::audio::SeekableAudio::duration),
            _ => None,
        }
    }
}

pub(crate) fn handle_bgm(mut context: ExtraBgmContext) {
    if !context.ui.open {
        return;
    }
    let input_allowed = context.full.is_empty()
        && !context.transition.is_animating()
        && context.ui.section == ExtraSection::Music;
    let clicked = context.tracks.iter().find_map(|(interaction, track)| {
        (*interaction == Interaction::Pressed).then_some(track.0.clone())
    });
    let control = context.controls.iter().find_map(|(interaction, control)| {
        (*interaction == Interaction::Pressed).then_some(*control)
    });
    let clicked = input_allowed.then_some(clicked).flatten();
    let control = input_allowed.then_some(control).flatten();
    let volume = context.settings.master_volume * context.settings.bgm_volume;
    let (ended, ready_player) = match context.players.single_mut() {
        Ok((_, mut player, Some(mut sink))) => {
            sink.set_volume(Volume::Linear(volume * player.gain));
            if !sink.empty() {
                player.observed_audio = true;
            }
            (player.observed_audio && sink.empty(), true)
        }
        Ok((_, player, None)) => (false, !player.failed),
        Err(_) => (false, false),
    };
    let selected = match control {
        Some(ExtraBgmControl::Stop) => {
            for (entity, _, _) in &mut context.players {
                context.commands.entity(entity).despawn();
            }
            resume_stage_bgm(&mut context.ui, &context.stage_bgm);
            for mut seek in &mut context.seek_bars {
                seek.reset();
            }
            return;
        }
        Some(ExtraBgmControl::Play) if clicked.is_none() => {
            if let Ok((_, _, Some(sink))) = context.players.single_mut() {
                if sink.is_paused() {
                    sink.play();
                } else {
                    sink.pause();
                }
                return;
            }
            if ready_player {
                return;
            }
            context.ui.selected_bgm.clone().or_else(|| {
                ordered_bgm(&context.state, &context.config)
                    .first()
                    .map(|(file, _)| (*file).to_owned())
            })
        }
        Some(ExtraBgmControl::Previous) | Some(ExtraBgmControl::Next) => {
            let ordered = ordered_bgm(&context.state, &context.config);
            if ordered.is_empty() {
                return;
            }
            let current = context
                .ui
                .selected_bgm
                .as_ref()
                .and_then(|file| ordered.iter().position(|candidate| candidate.0 == file))
                .unwrap_or(0);
            let next = if matches!(control, Some(ExtraBgmControl::Previous)) {
                (current + ordered.len() - 1) % ordered.len()
            } else {
                (current + 1) % ordered.len()
            };
            Some(ordered[next].0.to_owned())
        }
        _ if ended => {
            let ordered = ordered_bgm(&context.state, &context.config);
            if ordered.is_empty() {
                return;
            }
            let current = context
                .ui
                .selected_bgm
                .as_ref()
                .and_then(|file| ordered.iter().position(|candidate| candidate.0 == file))
                .unwrap_or(0);
            Some(ordered[(current + 1) % ordered.len()].0.to_owned())
        }
        _ => clicked,
    };
    let Some(file) = selected else { return };
    for (entity, _, _) in &mut context.players {
        context.commands.entity(entity).despawn();
    }
    for (entity, sink) in &context.stage_bgm {
        if !sink.is_paused() {
            context.ui.paused_stage_bgm.push(entity);
            sink.pause();
        }
    }
    for mut seek in &mut context.seek_bars {
        seek.reset();
    }
    let path = context.config.bgm_path(&file);
    let gain = keine_core::config::audio_gain_linear(
        *context.config.audio.bgm_gain_db.get(&path).unwrap_or(&0.0),
    );
    let mut entity = context.commands.spawn(ExtraBgmPlayer {
        gain,
        duration: None,
        observed_audio: false,
        failed: false,
    });
    crate::runtime::audio::insert_gallery_player(
        &mut entity,
        &context.assets,
        path,
        PlaybackSettings {
            // A one-shot source stays seekable; end detection above advances
            // the gallery playlist and wraps the last track to the first.
            mode: PlaybackMode::Once,
            volume: Volume::Linear(volume * gain),
            ..default()
        },
    );
    context.ui.selected_bgm = Some(file);
}

pub(super) fn ordered_bgm<'a>(
    state: &'a GameState,
    config: &GameConfigResource,
) -> Vec<(&'a str, &'a str)> {
    ordered_gallery(&state.unlocked_bgm, &config.extra.bgm_order)
}

pub(crate) fn sync_bgm_selection(
    ui: Res<ExtraUi>,
    state: Res<GameState>,
    names: Query<&Children, With<ExtraBgmName>>,
    mut labels: Query<&mut Text>,
) {
    if !ui.is_changed() && !state.is_changed() {
        return;
    }

    let selected = ui.selected_bgm.as_deref();
    let name = selected
        .and_then(|file| state.unlocked_bgm.get(file))
        .map_or("", String::as_str);
    for children in &names {
        for child in children.iter() {
            if let Ok(mut label) = labels.get_mut(child) {
                label.0.clear();
                label.0.push_str(name);
            }
        }
    }
}

pub(crate) fn sync_bgm_play_icon(
    players: Query<Option<&AudioSink>, With<ExtraBgmPlayer>>,
    buttons: Query<&Children, With<ExtraBgmPlayIcon>>,
    mut labels: Query<(&mut Text, Option<&mut LocalizedText>)>,
) {
    let playing = players
        .single()
        .ok()
        .flatten()
        .is_some_and(|sink| !sink.is_paused() && !sink.empty());
    let icon = if playing { "\u{f4c3}" } else { "\u{f4f4}" };
    for children in &buttons {
        for child in children.iter() {
            if let Ok((mut label, localized)) = labels.get_mut(child) {
                if let Some(mut key) = localized {
                    key.0 = if playing { UiText::Pause } else { UiText::Play };
                } else if label.0 != icon {
                    label.0 = icon.into();
                }
            }
        }
    }
}

pub(crate) fn handle_bgm_seek(
    mut bars: Query<
        (
            Entity,
            &Interaction,
            &ComputedNode,
            &UiGlobalTransform,
            &mut ExtraBgmSeekBar,
        ),
        With<Button>,
    >,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    touch: Option<Res<crate::ui::touch::TouchInputState>>,
    players: Query<(&ExtraBgmPlayer, &AudioSink)>,
) {
    let Ok((entity, interaction, node, transform, mut seek)) = bars.single_mut() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    if !window.focused {
        seek.reset();
        return;
    }
    let touch_point = touch
        .as_ref()
        .and_then(|touch| touch.control_position(entity));
    if touch_point.is_some() {
        seek.dragging = true;
        seek.touch = true;
    } else if mouse.just_pressed(MouseButton::Left) && *interaction != Interaction::None {
        seek.dragging = true;
        seek.touch = false;
    }
    let point = if seek.touch {
        touch_point
    } else if mouse.pressed(MouseButton::Left) {
        window.physical_cursor_position().map(|point| {
            point
                - crate::runtime::platform::DesignViewport::from_window(window)
                    .camera_viewport(window)
                    .physical_position
                    .as_vec2()
        })
    } else {
        None
    };
    if seek.dragging
        && let Ok((player, _sink)) = players.single()
        && let (Some(duration), Some(cursor)) = (player.duration, point)
        && let Some(point) = node.normalize_point(*transform, cursor)
    {
        let ratio = (point.x + 0.5).clamp(0.0, 1.0);
        #[cfg(target_os = "android")]
        if !duration.is_zero() {
            let before = seek
                .preview
                .unwrap_or_else(|| _sink.position())
                .as_secs_f32()
                / duration.as_secs_f32();
            crate::ui::haptics::drag_feedback(before, ratio, 1.0, 0.05);
        }
        seek.preview = Some(duration.mul_f64(f64::from(ratio)));
    }
    let finished = if seek.touch {
        touch
            .as_ref()
            .is_some_and(|touch| touch.control_finished(entity))
    } else {
        mouse.just_released(MouseButton::Left)
    };
    if finished {
        if seek.dragging
            && let Some(position) = seek.preview
            && let Ok((_, sink)) = players.single()
            && let Err(error) = sink.try_seek(position)
        {
            log::warn!("BGM seek failed: {error}");
        }
        // Always discard the drag preview, including a failed seek. The chained
        // progress system reads the sink-reported position again in this frame.
        seek.reset();
    }
}

#[derive(SystemParam)]
pub(crate) struct ExtraBgmProgressUi<'w, 's> {
    time: Res<'w, Time>,
    bars: Query<
        'w,
        's,
        (
            &'static Interaction,
            &'static ExtraBgmSeekBar,
            &'static ComputedNode,
        ),
    >,
    thumbs: Query<
        'w,
        's,
        (
            &'static mut ExtraBgmProgressThumb,
            &'static Interaction,
            &'static mut Node,
            &'static mut BackgroundColor,
        ),
    >,
    bubbles:
        Query<'w, 's, &'static mut Node, (With<ExtraBgmBubble>, Without<ExtraBgmProgressThumb>)>,
    values: Query<'w, 's, Entity, With<ExtraBgmBubbleText>>,
    times: Query<'w, 's, (Entity, &'static Children), With<ExtraBgmTime>>,
    labels: Query<'w, 's, &'static mut Text>,
}

type BgmTimeDisplayKey = (Option<Entity>, Option<Entity>, u64, Option<u64>);

pub(crate) fn update_bgm_progress(
    mut players: Query<(
        &mut ExtraBgmPlayer,
        Option<&AudioSink>,
        Option<&GalleryAudio>,
    )>,
    audio: ExtraAudioAssets,
    seek: Query<&ExtraBgmSeekBar>,
    mut ui: ExtraBgmProgressUi,
    mut displayed_time: Local<Option<BgmTimeDisplayKey>>,
) {
    let Ok((mut player, sink, source)) = players.single_mut() else {
        set_bgm_progress(&mut ui, Duration::ZERO, None, &mut displayed_time);
        return;
    };
    if player.duration.is_none() {
        player.duration = audio.duration(source);
    }
    let position = seek
        .single()
        .ok()
        .and_then(|seek| seek.preview)
        .unwrap_or_else(|| sink.map_or(Duration::ZERO, AudioSinkPlayback::position));
    set_bgm_progress(&mut ui, position, player.duration, &mut displayed_time);
}

fn set_bgm_progress(
    ui: &mut ExtraBgmProgressUi,
    position: Duration,
    duration: Option<Duration>,
    displayed_time: &mut Option<BgmTimeDisplayKey>,
) {
    let elapsed = duration.map_or(position, |duration| position.min(duration));
    let percent = duration
        .filter(|duration| !duration.is_zero())
        .map_or(0.0, |duration| {
            (elapsed.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
        });
    let ratio = percent / 100.0;
    let (_, dragging, width) =
        ui.bars
            .single()
            .map_or((false, false, 375.0), |(interaction, seek, node)| {
                (
                    *interaction != Interaction::None,
                    seek.dragging,
                    logical_node_width(node).max(45.0),
                )
            });
    for (mut visual, interaction, mut thumb, mut background) in &mut ui.thumbs {
        let hovered = *interaction != Interaction::None;
        let target = if hovered || dragging { 12.0 } else { 10.0 };
        let next = visual.0 + (target - visual.0) * exp_lerp(ui.time.delta_secs(), 18.0);
        if visual.0 != next {
            visual.0 = next;
        }
        let thumb_width = (visual.0 * 3.75).min(width);
        let next_width = Val::Px(thumb_width);
        let left = Val::Px(ratio * (width - thumb_width));
        if thumb.width != next_width {
            thumb.width = next_width;
        }
        if thumb.left != left {
            thumb.left = left;
        }
        let color = Color::srgba(1.0, 1.0, 1.0, if hovered { 0.67 } else { 0.5 });
        if background.0 != color {
            background.0 = color;
        }
    }
    let thumb_hovered = ui
        .thumbs
        .iter()
        .any(|(_, interaction, _, _)| *interaction != Interaction::None);
    for mut bubble in &mut ui.bubbles {
        let display = if thumb_hovered || dragging {
            Display::Flex
        } else {
            Display::None
        };
        let bubble_width = 67.5_f32.min(width);
        let next_width = Val::Px(bubble_width);
        let left = Val::Px(ratio * (width - bubble_width));
        if bubble.display != display {
            bubble.display = display;
        }
        if bubble.width != next_width {
            bubble.width = next_width;
        }
        if bubble.left != left {
            bubble.left = left;
        }
    }
    let time_key = (
        ui.times.iter().next().map(|(entity, _)| entity),
        ui.values.iter().next(),
        elapsed.as_secs(),
        duration.map(|duration| duration.as_secs()),
    );
    if *displayed_time == Some(time_key) {
        return;
    }
    *displayed_time = Some(time_key);
    for entity in &ui.values {
        if let Ok(mut value) = ui.labels.get_mut(entity) {
            value.0 = format_bgm_time(elapsed);
        }
    }
    let value = format!(
        "{} / {}",
        format_bgm_time(elapsed),
        duration.map_or_else(|| "--:--".to_owned(), format_bgm_time)
    );
    for (_, children) in &ui.times {
        for child in children.iter() {
            if let Ok(mut label) = ui.labels.get_mut(child) {
                label.0.clone_from(&value);
            }
        }
    }
}

fn format_bgm_time(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

pub(super) fn resume_stage_bgm(ui: &mut ExtraUi, stage: &StageBgmQuery<'_, '_>) {
    for entity in ui.paused_stage_bgm.drain(..) {
        if let Ok((_, sink)) = stage.get(entity) {
            sink.play();
        }
    }
}

#[derive(SystemParam)]
pub(crate) struct ExtraScrollContext<'w, 's> {
    ui: Res<'w, ExtraUi>,
    full: Query<'w, 's, (), With<ExtraFullCg>>,
    transition: Res<'w, ExtraPageTransition>,
}

pub(crate) fn scroll_music(
    mut wheel: MessageReader<MouseWheel>,
    touch: Option<Res<crate::ui::touch::TouchInputState>>,
    time: Res<Time>,
    windows: Query<&Window>,
    mut lists: Query<(
        Entity,
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
        &mut ExtraBgmList,
    )>,
    context: ExtraScrollContext,
) {
    let wheel_delta: f32 = wheel
        .read()
        .map(|event| {
            event.y
                * if event.unit == MouseScrollUnit::Line {
                    36.0
                } else {
                    1.0
                }
        })
        .sum();
    if !context.ui.open
        || context.ui.section != ExtraSection::Music
        || !context.full.is_empty()
        || context.transition.is_animating()
    {
        return;
    }
    let Ok((entity, node, transform, mut position, mut list)) = lists.single_mut() else {
        return;
    };
    let hovered = windows
        .single()
        .ok()
        .and_then(|window| {
            window.physical_cursor_position().map(|point| {
                point
                    - crate::runtime::platform::DesignViewport::from_window(window)
                        .camera_viewport(window)
                        .physical_position
                        .as_vec2()
            })
        })
        .is_some_and(|point| node.contains_point(*transform, point));
    let touch_delta = touch
        .as_ref()
        .and_then(|touch| touch.scroll)
        .filter(|(owner, _)| *owner == entity)
        .map_or(0.0, |(_, amount)| amount * node.inverse_scale_factor());
    let max = (node.content_size().y - node.size().y).max(0.0) * node.inverse_scale_factor();
    #[cfg(target_os = "android")]
    let before = list.target;
    list.target =
        (list.target - if hovered { wheel_delta } else { 0.0 } - touch_delta).clamp(0.0, max);
    #[cfg(target_os = "android")]
    if (hovered && wheel_delta != 0.0) || touch_delta != 0.0 {
        crate::ui::haptics::drag_feedback(before, list.target, max, 64.0);
    }
    position.y += (list.target - position.y) * exp_lerp(time.delta_secs(), UI_MOTION_RATE);
    if (list.target - position.y).abs() < 0.01 {
        position.y = list.target;
    }
}

pub(crate) fn update_bgm_status(
    _server: Res<AssetServer>,
    mut players: Query<(
        &mut ExtraBgmPlayer,
        Option<&AudioSink>,
        Option<&GalleryAudio>,
    )>,
    mut labels: Query<(&mut LocalizedText, &mut Node), With<ExtraBgmStatus>>,
    mut ui: ResMut<ExtraUi>,
    stage: StageBgmQuery<'_, '_>,
) {
    let status = if let Ok((mut player, sink, source)) = players.single_mut() {
        let failed = match source {
            #[cfg(feature = "audio-opus")]
            Some(GalleryAudio::Opus(handle)) => {
                matches!(_server.load_state(handle), LoadState::Failed(_))
            }
            #[cfg(feature = "audio-seekable")]
            Some(GalleryAudio::Seekable(handle)) => {
                matches!(_server.load_state(handle), LoadState::Failed(_))
            }
            _ => true,
        };
        if player.failed != failed {
            player.failed = failed;
        }
        if failed {
            if !ui.paused_stage_bgm.is_empty() {
                resume_stage_bgm(&mut ui, &stage);
            }
            UiText::GalleryUnavailable
        } else if let Some(sink) = sink {
            if sink.is_paused() {
                UiText::PlaybackPaused
            } else {
                UiText::PlaybackPlaying
            }
        } else {
            UiText::GalleryLoading
        }
    } else {
        UiText::PlaybackStopped
    };
    for (mut label, mut node) in &mut labels {
        if label.0 != status {
            label.0 = status;
        }
        let display = if matches!(status, UiText::GalleryLoading | UiText::GalleryUnavailable) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct StatusWrites(usize);

    type StatusChanges = Or<(
        Changed<LocalizedText>,
        Changed<Node>,
        Changed<ExtraBgmPlayer>,
    )>;

    fn count_status_writes(
        changed: Query<Entity, StatusChanges>,
        mut writes: ResMut<StatusWrites>,
    ) {
        writes.0 = changed.iter().count();
    }

    #[test]
    fn music_status_updates_on_failure_recovery_and_new_labels() {
        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::asset::AssetPlugin::default(),
        ))
        .init_resource::<ExtraUi>()
        .init_resource::<StatusWrites>()
        .add_systems(Update, (update_bgm_status, count_status_writes).chain());
        let label = app
            .world_mut()
            .spawn((
                ExtraBgmStatus,
                LocalizedText(UiText::PlaybackStopped),
                Node::default(),
            ))
            .id();
        let player = app
            .world_mut()
            .spawn(ExtraBgmPlayer {
                gain: 1.0,
                duration: None,
                observed_audio: false,
                failed: false,
            })
            .id();
        app.update();
        assert_eq!(
            app.world().get::<LocalizedText>(label).unwrap().0,
            UiText::GalleryUnavailable
        );
        assert_eq!(
            app.world().get::<Node>(label).unwrap().display,
            Display::Flex
        );
        app.update();
        assert_eq!(app.world().resource::<StatusWrites>().0, 0);
        #[cfg(feature = "audio-opus")]
        {
            app.world_mut()
                .entity_mut(player)
                .insert(GalleryAudio::Opus(Handle::default()));
            app.update();
            assert_eq!(
                app.world().get::<LocalizedText>(label).unwrap().0,
                UiText::GalleryLoading
            );
            assert!(!app.world().get::<ExtraBgmPlayer>(player).unwrap().failed);
        }
        app.world_mut().despawn(player);
        app.update();
        assert_eq!(
            app.world().get::<LocalizedText>(label).unwrap().0,
            UiText::PlaybackStopped
        );
        assert_eq!(
            app.world().get::<Node>(label).unwrap().display,
            Display::None
        );
        let replacement = app
            .world_mut()
            .spawn((
                ExtraBgmStatus,
                LocalizedText(UiText::GalleryLoading),
                Node::default(),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<LocalizedText>(replacement).unwrap().0,
            UiText::PlaybackStopped
        );
        assert_eq!(
            app.world().get::<Node>(replacement).unwrap().display,
            Display::None
        );
        app.update();
        assert_eq!(app.world().resource::<StatusWrites>().0, 0);
    }

    #[derive(Resource)]
    struct ProgressSample(Duration, Option<Duration>);

    #[derive(Resource, Default)]
    struct ProgressWrites(usize, usize, usize);

    fn sample_progress(
        sample: Res<ProgressSample>,
        mut ui: ExtraBgmProgressUi,
        mut displayed: Local<Option<BgmTimeDisplayKey>>,
    ) {
        set_bgm_progress(&mut ui, sample.0, sample.1, &mut displayed);
    }

    fn record_progress_writes(
        texts: Query<(), Changed<Text>>,
        nodes: Query<(), Changed<Node>>,
        colors: Query<(), Changed<BackgroundColor>>,
        mut writes: ResMut<ProgressWrites>,
    ) {
        *writes = ProgressWrites(
            texts.iter().count(),
            nodes.iter().count(),
            colors.iter().count(),
        );
    }

    #[test]
    fn progress_keeps_subsecond_motion_and_refreshes_rebuilt_labels() {
        assert_eq!(format_bgm_time(Duration::from_millis(62_999)), "01:02");
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs_f64(1.0 / 60.0));
        app.insert_resource(time)
            .insert_resource(ProgressSample(
                Duration::from_millis(62345),
                Some(Duration::from_secs(180)),
            ))
            .init_resource::<ProgressWrites>()
            .add_systems(Update, (sample_progress, record_progress_writes).chain());
        let bar = app
            .world_mut()
            .spawn((
                ExtraBgmSeekBar::default(),
                Interaction::None,
                ComputedNode::default(),
            ))
            .id();
        let thumb = app
            .world_mut()
            .spawn((
                ExtraBgmProgressThumb(10.0),
                Interaction::None,
                Node::default(),
                BackgroundColor::default(),
            ))
            .id();
        let bubble = app
            .world_mut()
            .spawn((ExtraBgmBubble, Node::default()))
            .id();
        let label = app
            .world_mut()
            .spawn((ExtraBgmBubbleText, Text::default()))
            .id();
        let total = app.world_mut().spawn(Text::default()).id();
        app.world_mut().spawn(ExtraBgmTime).add_child(total);
        app.update();
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "01:02");
        assert_eq!(app.world().get::<Text>(total).unwrap().0, "01:02 / 03:00");
        app.update();
        let writes = app.world().resource::<ProgressWrites>();
        assert_eq!((writes.0, writes.1, writes.2), (0, 0, 0));
        let previous = app.world().get::<Node>(thumb).unwrap().left;
        app.world_mut().resource_mut::<ProgressSample>().0 = Duration::from_millis(62999);
        app.update();
        assert_ne!(app.world().get::<Node>(thumb).unwrap().left, previous);
        assert_eq!(app.world().resource::<ProgressWrites>().0, 0);

        app.world_mut()
            .get_mut::<Interaction>(thumb)
            .unwrap()
            .clone_from(&Interaction::Hovered);
        app.update();
        assert_eq!(
            app.world().get::<Node>(bubble).unwrap().display,
            Display::Flex
        );
        assert!(app.world().get::<ExtraBgmProgressThumb>(thumb).unwrap().0 > 10.0);
        *app.world_mut().get_mut::<Interaction>(thumb).unwrap() = Interaction::None;
        app.world_mut()
            .get_mut::<ExtraBgmSeekBar>(bar)
            .unwrap()
            .dragging = true;
        app.world_mut().resource_mut::<ProgressSample>().0 = Duration::from_secs(120);
        app.update();
        assert_eq!(
            app.world().get::<Node>(bubble).unwrap().display,
            Display::Flex
        );
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "02:00");
        app.world_mut()
            .get_mut::<ExtraBgmSeekBar>(bar)
            .unwrap()
            .dragging = false;
        app.update();
        assert_eq!(
            app.world().get::<Node>(bubble).unwrap().display,
            Display::None
        );

        // Recreating the bubble in the same second must not reuse a stale key.
        app.world_mut().despawn(label);
        let rebuilt = app
            .world_mut()
            .spawn((ExtraBgmBubbleText, Text::default()))
            .id();
        app.update();
        assert_eq!(app.world().get::<Text>(rebuilt).unwrap().0, "02:00");
        app.world_mut().resource_mut::<ProgressSample>().1 = None;
        app.update();
        assert_eq!(app.world().get::<Text>(total).unwrap().0, "02:00 / --:--");
        app.world_mut().resource_mut::<ProgressSample>().1 = Some(Duration::from_secs(60));
        app.update();
        assert_eq!(app.world().get::<Text>(rebuilt).unwrap().0, "01:00");
        assert_eq!(app.world().get::<Text>(total).unwrap().0, "01:00 / 01:00");
    }

    #[test]
    fn gallery_order_preserves_defaults_and_only_reorders_unlocked_entries() {
        let mut state = GameState(keine_core::State::new());
        state.unlocked_bgm.insert("z.opus".into(), "Same".into());
        state.unlocked_bgm.insert("a.opus".into(), "Same".into());
        state
            .unlocked_bgm
            .insert("middle.opus".into(), "Before".into());

        assert_eq!(
            ordered_bgm(&state, &GameConfigResource(default())),
            vec![
                ("middle.opus", "Before"),
                ("a.opus", "Same"),
                ("z.opus", "Same"),
            ]
        );

        let mut config = GameConfigResource(default());
        config.0.extra.bgm_order = ["locked", "z.opus", "a.opus", "z.opus"]
            .map(str::to_owned)
            .to_vec();
        let expected = vec![
            ("z.opus", "Same"),
            ("a.opus", "Same"),
            ("middle.opus", "Before"),
        ];
        assert_eq!(ordered_bgm(&state, &config), expected);

        state.unlocked_cg = state.unlocked_bgm.clone();
        // CG and music orders remain independent.
        assert_eq!(ordered_cg(&state, &config)[0].0, "middle.opus");
        config.0.extra.cg_order = config.extra.bgm_order.clone();
        assert_eq!(
            ordered_cg(&state, &config),
            expected
                .into_iter()
                .map(|(file, name)| (file.to_owned(), name.to_owned()))
                .collect::<Vec<_>>()
        );
    }
}
