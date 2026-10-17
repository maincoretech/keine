//! Small, coalesced queue for host errors; never replace a pending confirmation.
use super::*;
use std::collections::{HashSet, VecDeque};

#[derive(Resource, Default)]
pub(crate) struct PendingNotices {
    pending: VecDeque<UiText>,
    reported: HashSet<UiText>,
}

impl PendingNotices {
    fn push(&mut self, key: UiText, once: bool) {
        if once && !self.reported.insert(key) {
            return;
        }
        if !self.pending.contains(&key) && self.pending.len() < 8 {
            self.pending.push_back(key);
        }
    }
}

pub(crate) fn report_notice(commands: &mut Commands, key: UiText, once: bool) {
    commands.queue(move |world: &mut World| {
        world.init_resource::<PendingNotices>();
        world.resource_mut::<PendingNotices>().push(key, once);
    });
}

pub(crate) fn report_background_save_failure(commands: &mut Commands) {
    report_notice(commands, UiText::BackgroundSaveFailed, true);
}

pub(crate) fn report_data_read_failure(commands: &mut Commands) {
    report_notice(commands, UiText::DataReadFailed, true);
}

pub(crate) fn show_notice(
    mut commands: Commands,
    notices: Option<ResMut<PendingNotices>>,
    request: Option<Res<DialogRequest>>,
    state: Res<crate::runtime::resources::GameState>,
    settings: Res<RuntimeSettings>,
) {
    if request.is_some() || state.system_message.is_some() {
        return;
    }
    if let Some(mut notices) = notices
        && let Some(key) = notices.pending.pop_front()
    {
        commands.insert_resource(DialogRequest {
            title: tr(settings.locale, key).into(),
            message: String::new(),
            confirm_text: Some(tr(settings.locale, UiText::Confirm).into()),
            cancel_text: None,
            action: DialogAction::Noop,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_notice_does_not_replace_confirmation() {
        let mut app = App::new();
        let mut pending = PendingNotices::default();
        pending.push(UiText::SaveFailed, false);
        app.insert_resource(pending)
            .insert_resource(crate::runtime::resources::GameState(
                keine_core::State::new(),
            ))
            .init_resource::<RuntimeSettings>()
            .insert_resource(DialogRequest::confirmation(
                "keep",
                DialogAction::SaveSlot(1),
            ))
            .add_systems(Update, show_notice);
        app.update();
        assert_eq!(app.world().resource::<DialogRequest>().title, "keep");
        assert_eq!(app.world().resource::<PendingNotices>().pending.len(), 1);
        app.world_mut().remove_resource::<DialogRequest>();
        app.update();
        assert_eq!(
            app.world().resource::<DialogRequest>().title,
            tr(crate::storage::settings::UiLocale::ZhCn, UiText::SaveFailed)
        );
        assert!(app.world().resource::<PendingNotices>().pending.is_empty());
    }

    #[test]
    fn background_failures_coalesce_but_explicit_retries_can_report_again() {
        let mut queue = PendingNotices::default();
        for _ in 0..100 {
            queue.push(UiText::BackgroundSaveFailed, true);
        }
        assert_eq!(queue.pending.len(), 1);
        queue.pending.clear();
        queue.push(UiText::BackgroundSaveFailed, true);
        assert!(queue.pending.is_empty());
        queue.push(UiText::SaveFailed, false);
        assert_eq!(queue.pending.len(), 1);
        queue.pending.clear();
        queue.push(UiText::SaveFailed, false);
        assert_eq!(queue.pending.len(), 1);
    }
}
