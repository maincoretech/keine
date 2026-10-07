//! Confirmation dialog state.
use super::*;

/// Which action to perform when the user confirms.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DialogAction {
    QuickSave,
    QuickLoad,
    SaveSlot(u32),
    LoadSlot(u32),
    DeleteSlot(u32),
    ClearSaves,
    ResetSettings,
    BackToTitle,
    Noop,
    SystemMessage,
    ExitGame,
}

/// Active dialog request. When set, the overlay + dialog UI is shown.
#[derive(Resource, Clone)]
pub(crate) struct DialogRequest {
    pub title: String,
    pub message: String,
    pub confirm_text: Option<String>,
    pub cancel_text: Option<String>,
    pub action: DialogAction,
}

impl DialogRequest {
    pub fn confirmation(title: impl Into<String>, action: DialogAction) -> Self {
        Self {
            title: title.into(),
            message: String::new(),
            confirm_text: None,
            cancel_text: None,
            action,
        }
    }

    pub(super) fn system_message(message: &keine_core::state::SystemMessageState) -> Self {
        Self {
            title: message.title.clone(),
            message: message.message.clone(),
            confirm_text: Some(message.confirm_text.clone()),
            cancel_text: (message.mode == keine_core::SystemMessageMode::Confirm)
                .then(|| message.cancel_text.clone()),
            action: DialogAction::SystemMessage,
        }
    }
}

#[derive(Component)]
pub(crate) struct DialogRoot;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogButton {
    Confirm,
    Cancel,
}

#[derive(Component)]
pub(crate) struct DialogFade(pub(super) f32);

impl DialogFade {
    pub(crate) fn progress(&self) -> f32 {
        self.0
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.0 < 0.999
    }
}

#[derive(Component)]
pub(crate) struct DialogBackground {
    pub(super) alpha: f32,
}

#[derive(Component)]
pub(crate) struct DialogBorder {
    pub(super) alpha: f32,
}

#[derive(Component)]
pub(crate) struct DialogText {
    pub(super) alpha: f32,
}
