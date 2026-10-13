//! Typed native-shell and open extension dispatch at the engine host boundary.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::runtime::resources::{GameConfigResource, GameState};
use crate::ui::backlog::BacklogUiState;
use crate::ui::control_bar::ToggleStates;
use crate::ui::extra::ExtraUi;
use crate::ui::save_load::{SaveLoadMode, SaveLoadUi};
use crate::ui::settings_panel::SettingsUi;
use keine_core::{ShellEvent, SystemUiSlot};

pub(crate) fn dispatch_shell(
    mut state: ResMut<GameState>,
    mut toggles: ResMut<ToggleStates>,
    mut save_load: ResMut<SaveLoadUi>,
    mut settings: ResMut<SettingsUi>,
    mut backlog: ResMut<BacklogUiState>,
    mut extra: ResMut<ExtraUi>,
    config: Res<GameConfigResource>,
) {
    if state.shell_events.is_empty() {
        return;
    }
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
    state: &'a mut keine_core::State,
    save_load: &'a mut SaveLoadUi,
    settings: &'a mut SettingsUi,
    backlog: &'a mut BacklogUiState,
    extra: &'a mut ExtraUi,
    extra_enabled: bool,
}

fn set_system_ui(slot: SystemUiSlot, visible: bool, context: SystemUiContext<'_>) {
    match (slot, visible) {
        (SystemUiSlot::Title, true) => keine_core::step::end_game(context.state),
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
pub struct HostCommandMessage(pub keine_core::HostCommandEvent);

/// Capability names claimed by installed extension plugins.
#[derive(Resource, Default)]
pub struct HostCapabilityRegistry(HashSet<(String, String)>);

impl HostCapabilityRegistry {
    pub fn claim(&mut self, namespace: impl Into<String>, command: impl Into<String>) {
        self.0.insert((namespace.into(), command.into()));
    }

    fn contains(&self, event: &keine_core::HostCommandEvent) -> bool {
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
    if state.host_commands.is_empty() {
        return;
    }
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
    fn idle_host_and_audio_sync_preserve_state_change_detection() {
        use crate::scene::audio::{EffectPlayback, VocalPlayback, sync_effects, sync_vocal};
        use crate::storage::settings::RuntimeSettings;

        #[derive(Resource, Default)]
        struct Changes(usize);
        fn observe(state: Res<GameState>, mut changes: ResMut<Changes>) {
            changes.0 += usize::from(state.is_changed());
        }
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .insert_resource(GameState(keine_core::State::new()))
            .insert_resource(GameConfigResource(Default::default()))
            .init_resource::<RuntimeSettings>()
            .init_resource::<VocalPlayback>()
            .init_resource::<EffectPlayback>()
            .init_resource::<ToggleStates>()
            .init_resource::<SaveLoadUi>()
            .init_resource::<SettingsUi>()
            .init_resource::<BacklogUiState>()
            .init_resource::<ExtraUi>()
            .init_resource::<Changes>()
            .add_message::<HostCommandMessage>()
            .add_systems(
                Update,
                (dispatch_shell, dispatch, sync_vocal, sync_effects, observe).chain(),
            );
        app.update();
        app.world_mut().resource_mut::<Changes>().0 = 0;
        for _ in 0..10 {
            app.update();
        }
        let changes = app.world().resource::<Changes>().0;
        println!("idle GameState notifications over 10 frames: {changes}");
        assert_eq!(changes, 0);

        app.world_mut()
            .resource_mut::<GameState>()
            .shell_events
            .push(ShellEvent::SetAutoplay(true));
        app.update();
        assert!(app.world().resource::<ToggleStates>().auto);
        assert!(app.world().resource::<GameState>().shell_events.is_empty());
        assert_eq!(app.world().resource::<Changes>().0, 1);
        app.update();
        assert_eq!(app.world().resource::<Changes>().0, 1);
    }

    #[test]
    fn typed_system_slots_route_to_the_native_shell() {
        let mut state = keine_core::State::new();
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
        let mut state = keine_core::State::new();
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
