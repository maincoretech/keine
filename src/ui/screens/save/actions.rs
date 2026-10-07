//! Save_Load actions; registered through the parent facade.
use super::*;

pub(crate) fn save_load_open(ui: Res<SaveLoadUi>) -> bool {
    ui.mode.is_some()
}

pub fn toggle_save_load(
    keys: Res<ButtonInput<KeyCode>>,
    input: ControlInput,
    back: Query<&Interaction, (With<MenuBack>, Changed<Interaction>)>,
    mut ui: ResMut<SaveLoadUi>,
    mut settings: ResMut<crate::ui::settings_panel::SettingsUi>,
    mut transition: ResMut<SaveLoadPageTransition>,
    mut route_transition: ResMut<MenuRouteTransition>,
) {
    let requested = [ButtonAction::Save, ButtonAction::Load]
        .into_iter()
        .find(|action| input.pressed(*action));
    if let Some(action) = requested {
        let previous_route = active_route(&ui, &settings);
        ui.mode = match action {
            ButtonAction::Save => {
                if settings.open {
                    settings.open = false;
                }
                Some(SaveLoadMode::Save)
            }
            ButtonAction::Load => {
                if settings.open {
                    settings.open = false;
                }
                Some(SaveLoadMode::Load)
            }
            _ => ui.mode,
        };
        // SAVE and LOAD share one slot surface. Switching mode only changes the
        // operation semantics and selected tab; page motion belongs exclusively
        // to page-number navigation.
        transition.active = false;
        transition.elapsed = 0.0;
        transition.direction = 0.0;
        let next_route = active_route(&ui, &settings);
        begin_route_change(&mut route_transition, previous_route, next_route);
    }
    if ui.mode.is_some()
        && (keys.just_pressed(KeyCode::Escape)
            || input.back_pressed()
            || back
                .iter()
                .any(|interaction| *interaction == Interaction::Pressed))
    {
        ui.mode = None;
    }
}

pub fn handle_save_load_page(
    interactions: Query<(&Interaction, &SaveLoadPage), Changed<Interaction>>,
    mut ui: ResMut<SaveLoadUi>,
    mut transition: ResMut<SaveLoadPageTransition>,
) {
    for (interaction, page) in &interactions {
        if *interaction == Interaction::Pressed && ui.page != page.0 {
            let direction = if page.0 > ui.page { 1.0 } else { -1.0 };
            ui.page = page.0;
            transition.begin(direction);
        }
    }
}

pub fn handle_save_load_slot(
    interactions: Query<(&Interaction, &SaveLoadSlot), Changed<Interaction>>,
    mut ui: ResMut<SaveLoadUi>,
    mut context: SaveSlotContext,
) {
    let Some(mode) = ui.mode else { return };
    let Some(slot) = interactions
        .iter()
        .find_map(|(interaction, slot)| (*interaction == Interaction::Pressed).then_some(slot.0))
    else {
        return;
    };
    let status =
        crate::storage::save::inspect_slot(context.store.0.as_ref(), slot, &context.project_root);
    if mode == SaveLoadMode::Save && context.state.ended {
        return;
    }
    if mode == SaveLoadMode::Load
        && !matches!(
            &status,
            crate::storage::save::SlotStatus::Ready(metadata)
                if metadata.program_fingerprint == context.state.program_fingerprint
        )
    {
        return;
    }
    if mode == SaveLoadMode::Save && !matches!(status, crate::storage::save::SlotStatus::Ready(_)) {
        match crate::storage::save::save_game_replacing_preview(
            context.store.0.as_ref(),
            &context.state,
            slot,
            &context.project_root,
            &context.preview_coordinator,
        ) {
            Ok(generation) => {
                context.save_previews.invalidate(slot);
                if let Ok(window) = context.windows.single() {
                    // Keep the current slot intact until the captured preview is ready, then the
                    // screenshot callback refreshes the complete card in one pass.
                    crate::ui::save_load::capture::capture_save_preview(
                        &mut context.commands,
                        &mut context.images,
                        Vec2::new(window.width(), window.height()),
                        slot,
                        generation,
                    );
                } else {
                    ui.set_changed();
                }
            }
            Err(error) => {
                log::error!("save slot {slot} failed: {error:#}");
                if !context.state.persistence_safety().is_exact() {
                    context
                        .commands
                        .insert_resource(DialogRequest::confirmation(
                            crate::ui::support::i18n::tr(
                                context.settings.locale,
                                crate::ui::support::i18n::UiText::SaveUnavailableDuringPresentation,
                            ),
                            DialogAction::Noop,
                        ));
                }
            }
        }
        return;
    }
    let (title, action) = match mode {
        SaveLoadMode::Save => (
            crate::ui::support::i18n::overwrite_slot(context.settings.locale, slot),
            DialogAction::SaveSlot(slot),
        ),
        SaveLoadMode::Load => (
            crate::ui::support::i18n::load_slot(context.settings.locale, slot),
            DialogAction::LoadSlot(slot),
        ),
    };
    context
        .commands
        .insert_resource(DialogRequest::confirmation(title, action));
}

pub fn handle_save_delete(keys: Res<ButtonInput<KeyCode>>, mut context: SaveDeleteContext) {
    if context.ui.mode.is_none() || context.request.is_some() || !keys.just_pressed(KeyCode::Delete)
    {
        return;
    }
    let Some(slot) = context
        .slots
        .iter()
        .find_map(|(interaction, slot)| (*interaction == Interaction::Hovered).then_some(slot.0))
    else {
        return;
    };
    if !matches!(
        crate::storage::save::inspect_slot(context.store.0.as_ref(), slot, &context.project_root),
        crate::storage::save::SlotStatus::Ready(_)
    ) {
        return;
    }
    context
        .commands
        .insert_resource(DialogRequest::confirmation(
            crate::ui::support::i18n::delete_slot(context.settings.locale, slot),
            DialogAction::DeleteSlot(slot),
        ));
}
