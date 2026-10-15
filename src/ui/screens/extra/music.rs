//! Gallery music transport, seek controls and list scrolling.
use super::*;

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
            sink.set_volume(Volume::Linear(volume));
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
                ordered_bgm(&context.state)
                    .first()
                    .map(|(file, _)| (*file).to_owned())
            })
        }
        Some(ExtraBgmControl::Previous) | Some(ExtraBgmControl::Next) => {
            let ordered = ordered_bgm(&context.state);
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
            let ordered = ordered_bgm(&context.state);
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
    let mut entity = context.commands.spawn(ExtraBgmPlayer {
        duration: None,
        observed_audio: false,
        failed: false,
    });
    crate::runtime::audio::insert_gallery_player(
        &mut entity,
        &context.assets,
        context.config.bgm_path(&file),
        PlaybackSettings {
            // A one-shot source stays seekable; end detection above advances
            // the gallery playlist and wraps the last track to the first.
            mode: PlaybackMode::Once,
            volume: Volume::Linear(volume),
            ..default()
        },
    );
    context.ui.selected_bgm = Some(file);
}

pub(super) fn ordered_bgm(state: &GameState) -> Vec<(&str, &str)> {
    let mut ordered = state
        .unlocked_bgm
        .iter()
        .map(|(file, name)| (file.as_str(), name.as_str()))
        .collect::<Vec<_>>();
    ordered.sort_unstable_by(|left, right| left.1.cmp(right.1).then_with(|| left.0.cmp(right.0)));
    ordered
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

type BgmTimeDisplayKey = (Option<Entity>, u64, Option<u64>);

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
        visual.0 += (target - visual.0) * exp_lerp(ui.time.delta_secs(), 18.0);
        let thumb_width = (visual.0 * 3.75).min(width);
        thumb.width = Val::Px(thumb_width);
        thumb.left = Val::Px(ratio * (width - thumb_width));
        background.0 = Color::srgba(1.0, 1.0, 1.0, if hovered { 0.67 } else { 0.5 });
    }
    let thumb_hovered = ui
        .thumbs
        .iter()
        .any(|(_, interaction, _, _)| *interaction != Interaction::None);
    for mut bubble in &mut ui.bubbles {
        bubble.display = if thumb_hovered || dragging {
            Display::Flex
        } else {
            Display::None
        };
        let bubble_width = 67.5_f32.min(width);
        bubble.width = Val::Px(bubble_width);
        bubble.left = Val::Px(ratio * (width - bubble_width));
    }
    for entity in &ui.values {
        if let Ok(mut value) = ui.labels.get_mut(entity) {
            value.0 = format_bgm_time(elapsed);
        }
    }
    let time_key = (
        ui.times.iter().next().map(|(entity, _)| entity),
        elapsed.as_secs(),
        duration.map(|duration| duration.as_secs()),
    );
    if *displayed_time == Some(time_key) {
        return;
    }
    *displayed_time = Some(time_key);
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
        player.failed = failed;
        if failed {
            resume_stage_bgm(&mut ui, &stage);
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
        label.0 = status;
        node.display = if matches!(status, UiText::GalleryLoading | UiText::GalleryUnavailable) {
            Display::Flex
        } else {
            Display::None
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bgm_time_without_fractional_jitter() {
        assert_eq!(format_bgm_time(Duration::from_millis(62_999)), "01:02");
    }

    #[test]
    fn orders_bgm_by_display_name_then_file() {
        let mut state = GameState(keine_core::State::new());
        state.unlocked_bgm.insert("z.opus".into(), "Same".into());
        state.unlocked_bgm.insert("a.opus".into(), "Same".into());
        state
            .unlocked_bgm
            .insert("middle.opus".into(), "Before".into());

        assert_eq!(
            ordered_bgm(&state),
            vec![
                ("middle.opus", "Before"),
                ("a.opus", "Same"),
                ("z.opus", "Same"),
            ]
        );
    }
}
