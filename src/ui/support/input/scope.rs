use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::runtime::preview::AuthoringPreviewSession;
use crate::runtime::resources::{
    AssetLoadingGate, EditorSyncSession, GameState, PersistenceDisabled, writable_runtime_session,
};
use crate::ui::backlog::BacklogUiState;
use crate::ui::dialog::DialogRequest;
use crate::ui::extra::ExtraUi;
use crate::ui::save_load::SaveLoadUi;
use crate::ui::settings_panel::SettingsUi;
use crate::ui::title::ReturnToTitleTransition;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum UiInputScope {
    Loading,
    UserInput,
    Dialog,
    Menu,
    Backlog,
    Extra,
    Title,
    #[default]
    Stage,
}

impl UiInputScope {
    pub(crate) const fn allows_backlog(self) -> bool {
        matches!(self, Self::Stage | Self::Backlog)
    }

    pub(crate) const fn allows_menu(self) -> bool {
        matches!(self, Self::Stage | Self::Menu)
    }
}

#[derive(SystemParam)]
pub(crate) struct InputScopeContext<'w> {
    loading: Res<'w, AssetLoadingGate>,
    dialog: Option<Res<'w, DialogRequest>>,
    settings: Res<'w, SettingsUi>,
    save_load: Res<'w, SaveLoadUi>,
    backlog: Res<'w, BacklogUiState>,
    extra: Res<'w, ExtraUi>,
    return_to_title: Option<Res<'w, ReturnToTitleTransition>>,
    state: Res<'w, GameState>,
    scope: ResMut<'w, UiInputScope>,
}

pub(crate) fn sync(mut context: InputScopeContext) {
    *context.scope = if context.loading.blocked || context.return_to_title.is_some() {
        UiInputScope::Loading
    } else if context.dialog.is_some() {
        UiInputScope::Dialog
    } else if context.state.user_input.is_some() {
        UiInputScope::UserInput
    } else if context.settings.open || context.save_load.mode.is_some() {
        UiInputScope::Menu
    } else if context.backlog.open {
        UiInputScope::Backlog
    } else if context.extra.open {
        UiInputScope::Extra
    } else if context.state.ended {
        UiInputScope::Title
    } else {
        UiInputScope::Stage
    };
}

pub(crate) fn backlog_allowed(scope: Res<UiInputScope>) -> bool {
    scope.allows_backlog()
}

pub(crate) fn menu_allowed(scope: Res<UiInputScope>) -> bool {
    scope.allows_menu()
}

pub(crate) fn user_input_allowed(scope: Res<UiInputScope>) -> bool {
    *scope == UiInputScope::UserInput
}

pub(crate) fn extra_allowed(scope: Res<UiInputScope>) -> bool {
    *scope == UiInputScope::Extra
}

pub(crate) fn stage_allowed(scope: Res<UiInputScope>) -> bool {
    *scope == UiInputScope::Stage
}

pub(crate) fn title_allowed(scope: Res<UiInputScope>) -> bool {
    *scope == UiInputScope::Title
}

pub(crate) fn dialog_allowed(scope: Res<UiInputScope>) -> bool {
    *scope == UiInputScope::Dialog
}

/// Studio sync stays read-only. Native authoring Preview writes only to the
/// Engine child's isolated preview-data root.
pub(crate) fn writable_session(
    editor_sync: Option<Res<EditorSyncSession>>,
    authoring_preview: Option<Res<AuthoringPreviewSession>>,
    persistence_disabled: Option<Res<PersistenceDisabled>>,
) -> bool {
    writable_runtime_session(
        editor_sync.is_some(),
        authoring_preview.is_some(),
        persistence_disabled.is_some(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_and_backlog_scopes_do_not_accept_modal_input() {
        assert!(UiInputScope::Stage.allows_menu());
        assert!(UiInputScope::Menu.allows_menu());
        assert!(!UiInputScope::Dialog.allows_menu());
        assert!(!UiInputScope::UserInput.allows_menu());

        assert!(UiInputScope::Stage.allows_backlog());
        assert!(UiInputScope::Backlog.allows_backlog());
        assert!(!UiInputScope::Menu.allows_backlog());
        assert!(!UiInputScope::Dialog.allows_backlog());
    }

    #[test]
    fn only_isolated_authoring_preview_can_write_during_editor_sync() {
        assert!(writable_runtime_session(false, false, false));
        assert!(!writable_runtime_session(true, false, false));
        assert!(writable_runtime_session(true, true, false));
        assert!(!writable_runtime_session(true, true, true));
    }
}
