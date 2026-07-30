//! Typed native-shell and open extension dispatch at the engine host boundary.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::runtime::resources::{GameConfigResource, GameState};
use crate::ui::backlog::BacklogUiState;
use crate::ui::control_bar::ToggleStates;
use crate::ui::extra::ExtraUi;
use crate::ui::save_load::{SaveLoadMode, SaveLoadUi};
use crate::ui::settings_panel::SettingsUi;
use crabgal_core::{ShellEvent, SystemUiSlot};

pub(crate) fn dispatch_shell(
    mut state: ResMut<GameState>,
    mut toggles: ResMut<ToggleStates>,
    mut save_load: ResMut<SaveLoadUi>,
    mut settings: ResMut<SettingsUi>,
    mut backlog: ResMut<BacklogUiState>,
    mut extra: ResMut<ExtraUi>,
    config: Res<GameConfigResource>,
) {
    for event in std::mem::take(&mut state.shell_events) {
        match event {
            ShellEvent::SetAutoplay(enabled) => toggles.auto = enabled,
            ShellEvent::SetSystemUi { slot, visible } => set_system_ui(
                slot,
                visible,
                SystemUiContext {
                    state: &mut state,
                    save_load: &mut save_load,
                    settings: &mut settings,
                    backlog: &mut backlog,
                    extra: &mut extra,
                    extra_enabled: config.features.extra,
                },
            ),
        }
    }
}

struct SystemUiContext<'a> {
    state: &'a mut crabgal_core::State,
    save_load: &'a mut SaveLoadUi,
    settings: &'a mut SettingsUi,
    backlog: &'a mut BacklogUiState,
    extra: &'a mut ExtraUi,
    extra_enabled: bool,
}

fn set_system_ui(slot: SystemUiSlot, visible: bool, context: SystemUiContext<'_>) {
    match (slot, visible) {
        (SystemUiSlot::Title, true) => crabgal_core::step::end_game(context.state),
        (SystemUiSlot::Save, true) => {
            context.settings.open = false;
            context.save_load.mode = Some(SaveLoadMode::Save);
        }
        (SystemUiSlot::Load, true) => {
            context.settings.open = false;
            context.save_load.mode = Some(SaveLoadMode::Load);
        }
        (SystemUiSlot::Settings, true) => {
            context.save_load.mode = None;
            context.settings.open = true;
        }
        (SystemUiSlot::History, true) => context.backlog.open = true,
        (SystemUiSlot::Gallery, true) => context.extra.open = context.extra_enabled,
        (SystemUiSlot::Save | SystemUiSlot::Load, false) => context.save_load.mode = None,
        (SystemUiSlot::Settings, false) => context.settings.open = false,
        (SystemUiSlot::History, false) => context.backlog.open = false,
        (SystemUiSlot::Gallery, false) => context.extra.open = false,
        (SystemUiSlot::Input | SystemUiSlot::Title, false) | (SystemUiSlot::Input, true) => {}
    }
}

/// A preserved third-party extension call emitted by a project adapter.
///
/// Built-in engine behavior must use typed core actions. External plugins can
/// read this message without changing an adapter or the script VM.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct HostCommandMessage(pub crabgal_core::HostCommandEvent);

/// Capability names claimed by installed extension plugins.
#[derive(Resource, Default)]
pub struct HostCapabilityRegistry(HashSet<(String, String)>);

impl HostCapabilityRegistry {
    pub fn claim(&mut self, namespace: impl Into<String>, command: impl Into<String>) {
        self.0.insert((namespace.into(), command.into()));
    }

    fn contains(&self, event: &crabgal_core::HostCommandEvent) -> bool {
        self.0
            .contains(&(event.namespace.clone(), event.command.clone()))
    }
}

#[derive(Resource, Default)]
pub(crate) struct HostCommandDiagnostics(HashSet<(String, String)>);

pub(crate) fn dispatch(
    mut state: ResMut<GameState>,
    mut messages: MessageWriter<HostCommandMessage>,
) {
    for event in std::mem::take(&mut state.host_commands) {
        messages.write(HostCommandMessage(event));
    }
}

pub(crate) fn diagnose_unhandled(
    mut messages: MessageReader<HostCommandMessage>,
    capabilities: Res<HostCapabilityRegistry>,
    mut diagnostics: ResMut<HostCommandDiagnostics>,
) {
    for message in messages.read() {
        let event = &message.0;
        if capabilities.contains(event) {
            continue;
        }
        if diagnostics
            .0
            .insert((event.namespace.clone(), event.command.clone()))
        {
            log::warn!(
                "no extension plugin handled capability {}/{}",
                event.namespace,
                event.command
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_system_slots_route_to_the_native_shell() {
        let mut state = crabgal_core::State::new();
        let mut save_load = SaveLoadUi::default();
        let mut settings = SettingsUi::default();
        let mut backlog = BacklogUiState::default();
        let mut extra = ExtraUi::default();

        let context = SystemUiContext {
            state: &mut state,
            save_load: &mut save_load,
            settings: &mut settings,
            backlog: &mut backlog,
            extra: &mut extra,
            extra_enabled: false,
        };
        set_system_ui(SystemUiSlot::Load, true, context);
        assert_eq!(save_load.mode, Some(SaveLoadMode::Load));

        let context = SystemUiContext {
            state: &mut state,
            save_load: &mut save_load,
            settings: &mut settings,
            backlog: &mut backlog,
            extra: &mut extra,
            extra_enabled: false,
        };
        set_system_ui(SystemUiSlot::Settings, true, context);
        assert!(settings.open);
        assert_eq!(save_load.mode, None);
    }

    #[test]
    fn gallery_slot_respects_the_project_feature_gate() {
        let mut state = crabgal_core::State::new();
        let mut save_load = SaveLoadUi::default();
        let mut settings = SettingsUi::default();
        let mut backlog = BacklogUiState::default();
        let mut extra = ExtraUi::default();

        let context = SystemUiContext {
            state: &mut state,
            save_load: &mut save_load,
            settings: &mut settings,
            backlog: &mut backlog,
            extra: &mut extra,
            extra_enabled: false,
        };
        set_system_ui(SystemUiSlot::Gallery, true, context);
        assert!(!extra.open);

        let context = SystemUiContext {
            state: &mut state,
            save_load: &mut save_load,
            settings: &mut settings,
            backlog: &mut backlog,
            extra: &mut extra,
            extra_enabled: true,
        };
        set_system_ui(SystemUiSlot::Gallery, true, context);
        assert!(extra.open);
    }
}
