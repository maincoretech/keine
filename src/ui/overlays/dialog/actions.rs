//! Confirmation dialog actions.
use super::*;

#[derive(SystemParam)]
pub(crate) struct QuickSaveContext<'w, 's> {
    state: ResMut<'w, crate::runtime::resources::GameState>,
    checkpoint: ResMut<'w, crate::storage::save::ContinuationCheckpoint>,
    project_root: Res<'w, crate::runtime::resources::PersistenceRoot>,
    store: Res<'w, crate::runtime::resources::StoreCodec>,
    preview: ResMut<'w, QuickSavePreview>,
    save_previews: ResMut<'w, crate::ui::save_load::SavePreviewCache>,
    images: ResMut<'w, Assets<Image>>,
    windows: Query<'w, 's, &'static Window>,
    primary_window: Query<'w, 's, Entity, With<PrimaryWindow>>,
    save_load: ResMut<'w, crate::ui::save_load::SaveLoadUi>,
    settings_ui: ResMut<'w, crate::ui::settings_panel::SettingsUi>,
    backlog_ui: ResMut<'w, crate::ui::backlog::BacklogUiState>,
    settings: ResMut<'w, crate::storage::settings::RuntimeSettings>,
    toggles: ResMut<'w, crate::ui::control_bar::ToggleStates>,
    pending_window: ResMut<'w, crate::ui::settings_panel::PendingWindowMode>,
    preview_coordinator: Res<'w, crate::storage::save::SavePreviewCoordinator>,
    editor_sync: Option<Res<'w, crate::runtime::resources::EditorSyncSession>>,
    authoring_preview: Option<Res<'w, crate::runtime::preview::AuthoringPreviewSession>>,
    persistence_disabled: Option<Res<'w, crate::runtime::resources::PersistenceDisabled>>,
}

/// Handle dialog button clicks: execute the action and remove the request.
pub fn handle_dialog_click(
    mut commands: Commands,
    buttons: Query<(&Interaction, &DialogButton), Changed<Interaction>>,
    request: Option<Res<DialogRequest>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    actions: Res<crate::runtime::platform::InputActions>,
    mut context: QuickSaveContext,
) {
    let left_clicked = buttons.iter().any(|(interaction, button)| {
        matches!(interaction, Interaction::Pressed) && *button == DialogButton::Confirm
    }) || keys.just_pressed(KeyCode::Enter);
    let right_clicked = buttons.iter().any(|(interaction, button)| {
        matches!(interaction, Interaction::Pressed) && *button == DialogButton::Cancel
    }) || keys.just_pressed(KeyCode::Escape)
        || mouse.just_pressed(MouseButton::Right)
        || actions.back;

    if !left_clicked && !right_clicked {
        return;
    }
    let Some(req) = request else { return };
    commands.remove_resource::<DialogRequest>();

    if left_clicked && !right_clicked {
        if !crate::runtime::resources::writable_runtime_session(
            context.editor_sync.is_some(),
            context.authoring_preview.is_some(),
            context.persistence_disabled.is_some(),
        ) && !matches!(
            req.action,
            DialogAction::Noop | DialogAction::SystemMessage | DialogAction::ExitGame
        ) {
            log::debug!("ignored persistent UI action in a read-only runtime session");
            return;
        }
        match &req.action {
            DialogAction::QuickSave => {
                match crate::storage::save::save_game_replacing_preview(
                    context.store.0.as_ref(),
                    &context.state,
                    QUICK_SAVE_SLOT,
                    &context.project_root,
                    &context.preview_coordinator,
                ) {
                    Ok(generation) => {
                        context.preview.state = Some(
                            crate::ui::control_bar::QuickSaveSnapshot::from(&**context.state),
                        );
                        context.preview.image = None;
                        if let Ok(window) = context.windows.single() {
                            let size = Vec2::new(window.width(), window.height());
                            capture_save_preview(
                                &mut commands,
                                &mut context.images,
                                size,
                                QUICK_SAVE_SLOT,
                                generation,
                            );
                        }
                    }
                    Err(error) => {
                        log::error!("quick save failed: {error:#}");
                        if !context.state.persistence_safety().is_exact() {
                            commands.insert_resource(DialogRequest::confirmation(
                                tr(
                                    context.settings.locale,
                                    UiText::SaveUnavailableDuringPresentation,
                                ),
                                DialogAction::Noop,
                            ));
                        }
                    }
                }
            }
            DialogAction::QuickLoad => {
                match crate::storage::save::load_game(
                    context.store.0.as_ref(),
                    QUICK_SAVE_SLOT,
                    &context.project_root,
                ) {
                    Ok(loaded) => match loaded.restore_into(&mut context.state) {
                        Ok(()) => context.checkpoint.reset(&context.state),
                        Err(error) => {
                            log::error!("quick load rejected: {error}");
                            commands.insert_resource(DialogRequest::confirmation(
                                tr(context.settings.locale, UiText::ForeignSave),
                                DialogAction::Noop,
                            ));
                        }
                    },
                    Err(error) => log::error!("quick load failed: {error:#}"),
                }
            }
            DialogAction::BackToTitle => {
                let continuation = context
                    .checkpoint
                    .state_for_continuation(&context.state)
                    .cloned();
                match crate::storage::save::save_continuation_replacing_preview(
                    context.store.0.as_ref(),
                    &context.state,
                    &context.checkpoint,
                    &context.project_root,
                    &context.preview_coordinator,
                ) {
                    Ok(crate::storage::save::ContinuationSave::Skipped) => {
                        log::warn!(
                            "kept the previous continuation because no exact checkpoint exists"
                        )
                    }
                    Ok(_) => {
                        context.preview.state = continuation
                            .as_ref()
                            .map(crate::ui::control_bar::QuickSaveSnapshot::from);
                        context.preview.image = None;
                    }
                    Err(error) => {
                        log::error!(
                            "failed to save continuation before returning to title: {error:#}"
                        )
                    }
                }
                commands.insert_resource(crate::ui::title::ReturnToTitleTransition::default());
                context.save_load.mode = None;
                context.settings_ui.open = false;
                context.backlog_ui.open = false;
            }
            DialogAction::SaveSlot(slot) => {
                match crate::storage::save::save_game_replacing_preview(
                    context.store.0.as_ref(),
                    &context.state,
                    *slot,
                    &context.project_root,
                    &context.preview_coordinator,
                ) {
                    Ok(generation) => {
                        context.save_previews.invalidate(*slot);
                        if let Ok(window) = context.windows.single() {
                            let size = Vec2::new(window.width(), window.height());
                            // Keep the old card intact until its replacement preview is ready;
                            // the screenshot callback refreshes metadata and image together.
                            capture_save_preview(
                                &mut commands,
                                &mut context.images,
                                size,
                                *slot,
                                generation,
                            );
                        } else {
                            context.save_load.set_changed();
                        }
                    }
                    Err(error) => {
                        log::error!("save slot {slot} failed: {error:#}");
                        if !context.state.persistence_safety().is_exact() {
                            commands.insert_resource(DialogRequest::confirmation(
                                tr(
                                    context.settings.locale,
                                    UiText::SaveUnavailableDuringPresentation,
                                ),
                                DialogAction::Noop,
                            ));
                        }
                    }
                }
            }
            DialogAction::LoadSlot(slot) => {
                match crate::storage::save::load_game(
                    context.store.0.as_ref(),
                    *slot,
                    &context.project_root,
                ) {
                    Ok(loaded) => match loaded.restore_into(&mut context.state) {
                        Ok(()) => {
                            context.checkpoint.reset(&context.state);
                            context.save_load.mode = None;
                        }
                        Err(error) => {
                            log::error!("load slot {slot} rejected: {error}");
                            commands.insert_resource(DialogRequest::confirmation(
                                tr(context.settings.locale, UiText::ForeignSave),
                                DialogAction::Noop,
                            ));
                        }
                    },
                    Err(error) => log::error!("load slot {slot} failed: {error:#}"),
                }
            }
            DialogAction::DeleteSlot(slot) => {
                context.preview_coordinator.invalidate_slot(*slot);
                context.save_previews.invalidate(*slot);
                match crate::storage::save::delete_game(
                    context.store.0.as_ref(),
                    *slot,
                    &context.project_root,
                ) {
                    Ok(()) => context.save_load.set_changed(),
                    Err(error) => log::error!("delete slot {slot} failed: {error:#}"),
                }
            }
            DialogAction::ClearSaves => {
                context.preview_coordinator.invalidate_all();
                if let Err(error) = crate::storage::save::clear_games(
                    context.store.0.as_ref(),
                    &context.project_root,
                ) {
                    log::error!("failed to clear save slots: {error:#}");
                } else {
                    context.preview.state = None;
                    context.preview.image = None;
                    context.save_previews.clear();
                    context.save_load.set_changed();
                }
            }
            DialogAction::ResetSettings => {
                crate::ui::settings_panel::reset_runtime_settings(
                    &mut context.settings,
                    &mut context.toggles,
                    &mut context.pending_window,
                    &context.project_root,
                );
            }
            DialogAction::Noop => {}
            DialogAction::SystemMessage => {
                if keine_core::step::resolve_system_message(&mut context.state, true) {
                    let outcome = crate::runtime::script_driver::resume(
                        &mut context.state,
                        &mut context.checkpoint,
                    );
                    crate::ui::title::handle_script_outcome(&mut commands, outcome);
                }
            }
            DialogAction::ExitGame => {
                if let Ok(window) = context.primary_window.single() {
                    commands.write_message(WindowCloseRequested { window });
                } else {
                    log::warn!("primary window unavailable; exiting directly");
                    commands.write_message(bevy::app::AppExit::Success);
                }
            }
        }
    }
    if right_clicked
        && matches!(req.action, DialogAction::SystemMessage)
        && keine_core::step::resolve_system_message(&mut context.state, false)
    {
        let outcome =
            crate::runtime::script_driver::resume(&mut context.state, &mut context.checkpoint);
        crate::ui::title::handle_script_outcome(&mut commands, outcome);
    }
}
