//! Settings actions; registered through the parent facade.
use super::*;

pub fn toggle_settings(
    keys: Res<ButtonInput<KeyCode>>,
    input: ControlInput,
    back: Query<&Interaction, (With<MenuBack>, Changed<Interaction>)>,
    mut ui: ResMut<SettingsUi>,
    mut save_load: ResMut<SaveLoadUi>,
    mut route_transition: ResMut<MenuRouteTransition>,
    mut page_transition: ResMut<SettingsPageTransition>,
) {
    let previous_route = active_route(&save_load, &ui);
    let toggled = input.pressed(ButtonAction::System);
    if toggled {
        ui.open = !ui.open;
        if ui.open {
            save_load.mode = None;
        }
    }
    let next_route = active_route(&save_load, &ui);
    begin_route_change(&mut route_transition, previous_route, next_route);
    if ui.open
        && (((keys.just_pressed(KeyCode::Escape) || input.back_pressed()) && !toggled)
            || back
                .iter()
                .any(|interaction| *interaction == Interaction::Pressed))
    {
        ui.open = false;
    }
    if !ui.open {
        page_transition.reset();
    }
}

pub fn settings_open(ui: Res<SettingsUi>) -> bool {
    ui.open
}

pub(crate) fn settings_or_extra_open(
    ui: Res<SettingsUi>,
    extra: Res<crate::ui::extra::ExtraUi>,
) -> bool {
    ui.open || extra.open
}

pub fn handle_settings_page(
    buttons: Query<(&Interaction, &SettingsPageButton), Changed<Interaction>>,
    mut ui: ResMut<SettingsUi>,
    mut transition: ResMut<SettingsPageTransition>,
) {
    for (interaction, page) in &buttons {
        if *interaction == Interaction::Pressed && ui.page != page.0 {
            transition.begin(ui.page, page.0);
            ui.page = page.0;
        }
    }
}

pub fn handle_about_repository_link(links: AboutRepositoryLinkQuery) {
    for interaction in &links {
        if *interaction == Interaction::Pressed {
            bevy::tasks::IoTaskPool::get()
                .spawn(async {
                    if let Err(error) = webbrowser::open("https://github.com/maincoretech/keine") {
                        log::error!("failed to open Kēne repository: {error}");
                    }
                })
                .detach();
        }
    }
}

#[cfg(not(target_os = "android"))]
pub(super) fn choose_backup_path(export: bool) -> Option<std::path::PathBuf> {
    let dialog = rfd::FileDialog::new().add_filter("keine backup", &["keine-backup"]);
    if export {
        dialog.set_file_name("keine.keine-backup").save_file()
    } else {
        dialog.pick_file()
    }
}

pub fn handle_setting_action(context: SettingActionContext) {
    let SettingActionContext {
        mut commands,
        actions,
        mut settings,
        mut toggles,
        mut pending_window,
        project_root,
        store,
        mut state,
        mut quick_preview,
        mut save_previews,
        preview_coordinator,
        title_entities,
        mut settings_roots,
        mut locale_transition,
    } = context;
    let action = actions.iter().find_map(|(interaction, action)| {
        (*interaction == Interaction::Pressed).then_some(*action)
    });
    #[cfg(target_os = "android")]
    let (action, selection) = match crate::runtime::platform::take_backup_result() {
        Some(Ok(Some(selection))) => (
            Some(if selection.export {
                SettingAction::ExportData
            } else {
                SettingAction::ImportData
            }),
            Some(selection),
        ),
        Some(Ok(None)) => return,
        Some(Err(error)) => {
            log::error!("failed to choose backup: {error:#}");
            return;
        }
        None => (action, None),
    };
    let Some(action) = action else {
        return;
    };
    match action {
        SettingAction::SetSkip(value) => {
            settings.skip_all = value;
            toggles.skip_mode = if settings.skip_all {
                SkipMode::All
            } else {
                SkipMode::Read
            };
            toggles.skip = false;
        }
        SettingAction::SetLanguage(locale) => {
            if settings.locale == locale || locale_transition.is_animating() {
                return;
            }
            locale_transition.begin(locale);
            for (_, mut fade, mut surface, mut visibility) in &mut settings_roots {
                *surface = MenuSurface::fade_only();
                fade.target = 0.0;
                *visibility = Visibility::Inherited;
            }
            return;
        }
        SettingAction::SetFullscreen(value) => {
            settings.fullscreen = value;
            pending_window.target = Some(value);
            pending_window.delay_frames = 1;
        }
        SettingAction::SetTextSize(value) => settings.text_size = value.min(2),
        SettingAction::ClearSaves => {
            commands.insert_resource(crate::ui::dialog::DialogRequest::confirmation(
                tr(settings.locale, UiText::ConfirmClearSaves),
                crate::ui::dialog::DialogAction::ClearSaves,
            ));
            return;
        }
        SettingAction::ResetSettings => {
            commands.insert_resource(crate::ui::dialog::DialogRequest::confirmation(
                tr(settings.locale, UiText::ConfirmResetSettings),
                crate::ui::dialog::DialogAction::ResetSettings,
            ));
            return;
        }
        SettingAction::ExportData => {
            #[cfg(not(target_os = "android"))]
            let result = {
                let Some(path) = choose_backup_path(true) else {
                    return;
                };
                crate::storage::backup::export(&project_root, &path)
            };
            #[cfg(target_os = "android")]
            let result = match selection {
                Some(selection) => {
                    crate::storage::backup::export_to_stream(&project_root, selection.file)
                }
                None => {
                    if let Err(error) = crate::runtime::platform::request_backup(true) {
                        log::error!("failed to open backup export: {error:#}");
                    }
                    return;
                }
            };
            if let Err(error) = result {
                log::error!("failed to export save data: {error:#}");
            } else {
                log::info!("backup export completed");
            }
            return;
        }
        SettingAction::ImportData => {
            #[cfg(not(target_os = "android"))]
            let result = {
                let Some(path) = choose_backup_path(false) else {
                    return;
                };
                crate::storage::backup::import(&project_root, &path)
            };
            #[cfg(target_os = "android")]
            let result = match selection {
                Some(selection) => {
                    crate::storage::backup::import_from_stream(&project_root, selection.file)
                }
                None => {
                    if let Err(error) = crate::runtime::platform::request_backup(false) {
                        log::error!("failed to open backup import: {error:#}");
                    }
                    return;
                }
            };
            if let Err(error) = result {
                log::error!("failed to import save data: {error:#}");
                return;
            }
            preview_coordinator.invalidate_all();
            log::info!("backup import completed");
            if let Some(mut imported) = crate::storage::settings::load(&project_root) {
                crate::storage::settings::sanitize(&mut imported);
                *settings = imported;
                toggles.skip = false;
                toggles.skip_mode = if settings.skip_all {
                    SkipMode::All
                } else {
                    SkipMode::Read
                };
                pending_window.target =
                    (!cfg!(target_os = "android")).then_some(settings.fullscreen);
                pending_window.delay_frames = 1;
            }
            state.global_vars = crate::storage::profile::load(&project_root);
            state.read_dialogues = crate::storage::read_history::load(&project_root);
            state.unlocked_cg.clear();
            state.unlocked_bgm.clear();
            crate::storage::gallery::load(&mut state, &project_root);
            quick_preview.state = crate::storage::save::load_game(
                store.0.as_ref(),
                crate::storage::save::QUICK_SAVE_SLOT,
                &project_root,
            )
            .ok()
            .filter(|saved| {
                !saved.snapshot().ended
                    && saved.snapshot().program_fingerprint == state.program_fingerprint
            })
            .map(|saved| crate::ui::control_bar::QuickSaveSnapshot::from(saved.snapshot()));
            quick_preview.image = None;
            save_previews.clear();
            for entity in &title_entities {
                commands.entity(entity).despawn();
            }
            return;
        }
    }
    if let Err(error) = crate::storage::settings::persist(&settings, &project_root) {
        log::error!("failed to persist settings: {error:#}");
    }
}

pub(crate) fn reset_runtime_settings(
    settings: &mut RuntimeSettings,
    toggles: &mut ToggleStates,
    pending_window: &mut PendingWindowMode,
    project_root: &PersistenceRoot,
) {
    *settings = RuntimeSettings::default();
    toggles.skip = false;
    toggles.skip_mode = SkipMode::Read;
    pending_window.target = Some(settings.fullscreen);
    pending_window.delay_frames = 1;
    if let Err(error) = crate::storage::settings::persist(settings, project_root) {
        log::error!("failed to restore default settings: {error:#}");
    }
}

pub fn apply_pending_window_mode(
    mut pending: ResMut<PendingWindowMode>,
    mut windows: Query<&mut Window>,
    actions: Res<crate::runtime::platform::InputActions>,
    mut settings: ResMut<RuntimeSettings>,
) {
    if actions.toggle_fullscreen
        && let Ok(window) = windows.single()
    {
        let fullscreen = pending
            .target
            .unwrap_or(window.mode != WindowMode::Windowed);
        settings.fullscreen = !fullscreen;
        pending.target = Some(!fullscreen);
        pending.delay_frames = 0;
    }
    let Some(value) = pending.target else {
        return;
    };
    if pending.delay_frames > 0 {
        pending.delay_frames -= 1;
        return;
    }
    if let Ok(mut window) = windows.single_mut() {
        window.mode = if value {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };
    }
    pending.target = None;
}

#[derive(SystemParam)]
pub(crate) struct SettingSliderInput<'w, 's> {
    windows: Query<'w, 's, &'static Window>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    touch: Option<Res<'w, crate::ui::touch::TouchInputState>>,
    ui: Res<'w, SettingsUi>,
    scope: Res<'w, crate::ui::input_scope::UiInputScope>,
    route: Res<'w, MenuRouteTransition>,
    extra: Res<'w, crate::ui::extra::ExtraUi>,
    extra_transition: Res<'w, crate::ui::extra::ExtraPageTransition>,
    full_cg: Query<'w, 's, (), With<crate::ui::extra::ExtraFullCg>>,
}

pub fn handle_setting_sliders(
    sliders: Query<(
        Entity,
        &Interaction,
        &SettingSlider,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    input: SettingSliderInput,
    mut settings: ResMut<RuntimeSettings>,
    project_root: Res<PersistenceRoot>,
    mut drag: ResMut<ActiveSettingSlider>,
) {
    let SettingSliderInput {
        windows,
        mouse,
        touch,
        ui,
        scope,
        route,
        extra,
        extra_transition,
        full_cg,
    } = input;
    let Ok(window) = windows.single() else { return };
    let enabled = window.focused
        && (ui.open
            && *scope == crate::ui::input_scope::UiInputScope::Menu
            && !route.is_animating()
            || extra.open
                && *scope == crate::ui::input_scope::UiInputScope::Extra
                && !extra_transition.is_animating()
                && full_cg.is_empty());
    // UI layout and UiGlobalTransform are expressed in physical pixels.
    // Using the logical cursor position on HiDPI displays offsets the hit
    // calculation and commonly clamps the slider to zero.
    if enabled && mouse.just_pressed(MouseButton::Left) {
        drag.touch = None;
        drag.kind = sliders
            .iter()
            .find(|(_, interaction, _, _, _)| {
                matches!(interaction, Interaction::Hovered | Interaction::Pressed)
            })
            .map(|(_, _, slider, _, _)| slider.0);
    }
    if enabled
        && let Some(touch) = touch.as_ref()
        && let Some((entity, _, slider, _, _)) = sliders
            .iter()
            .find(|(entity, _, _, _, _)| touch.control_position(*entity).is_some())
    {
        drag.kind = Some(slider.0);
        drag.touch = Some(entity);
    }
    let cursor = if !enabled {
        None
    } else if let Some(entity) = drag.touch {
        touch
            .as_ref()
            .and_then(|touch| touch.control_position(entity))
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
    if let (Some(kind), Some(cursor)) = (drag.kind, cursor) {
        for (_, _, slider, node, transform) in &sliders {
            if slider.0 != kind {
                continue;
            }
            let size = node.size();
            if size.x <= 0.0 {
                continue;
            }
            let Some(point) = node.normalize_point(*transform, cursor) else {
                continue;
            };
            let ratio = (point.x + 0.5).clamp(0.0, 1.0);
            if (slider.0.ratio(&settings) - ratio).abs() > 0.0005 {
                slider.0.set_ratio(&mut settings, ratio);
                drag.dirty = true;
            }
        }
    }
    let finished = drag.touch.is_some_and(|entity| {
        touch
            .as_ref()
            .is_some_and(|touch| touch.control_finished(entity))
    }) || (drag.touch.is_none() && mouse.just_released(MouseButton::Left))
        || !enabled;
    if finished {
        drag.kind = None;
        drag.touch = None;
        if drag.dirty {
            if let Err(error) = crate::storage::settings::persist(&settings, &project_root) {
                log::error!("failed to persist settings: {error:#}");
            }
            drag.dirty = false;
        }
    }
}
