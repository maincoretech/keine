mod adapter_tui;
pub(crate) mod asset_reader;
pub(crate) mod audio;
mod bootstrap;
pub(crate) mod host;
pub(crate) mod platform;
pub(crate) mod resources;
pub(crate) mod tick;

use bevy::prelude::*;

use crate::scene::ScenePlugin;
use crate::storage::StoragePlugin;
use crate::ui::GameUiPlugin;

pub use bootstrap::{build_app_with_loader, run, run_cli, run_with_loader};

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub(crate) enum GameSystemSet {
    Input,
    Sync,
    Layout,
    Ui,
}

pub(crate) struct RuntimePlugin;

impl Plugin for RuntimePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<platform::RuntimeActivity>()
            .init_resource::<host::HostCommandDiagnostics>()
            .init_resource::<host::HostCapabilityRegistry>()
            .init_resource::<platform::InputActions>()
            .init_resource::<platform::PointerClickHistory>()
            .init_resource::<platform::GracefulExit>();
        app.add_systems(PreUpdate, platform::collect_input);
        app.add_systems(Update, tick::tick.in_set(GameSystemSet::Input));
        app.add_systems(Update, host::dispatch_shell.in_set(GameSystemSet::Sync));
        app.add_message::<host::HostCommandMessage>();
        app.add_systems(
            Update,
            (host::dispatch, host::diagnose_unhandled)
                .chain()
                .in_set(GameSystemSet::Sync),
        );
        app.add_systems(
            Update,
            platform::resize_viewport.in_set(GameSystemSet::Layout),
        );
        app.add_systems(PostUpdate, platform::request_graceful_exit);
        app.add_systems(
            Last,
            (
                platform::sync_dialog_camera_activity,
                platform::update_lifecycle,
                platform::sync_background_audio,
            )
                .chain(),
        );
    }
}

pub(crate) struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            Update,
            (
                GameSystemSet::Input,
                GameSystemSet::Sync,
                GameSystemSet::Layout,
                GameSystemSet::Ui,
            )
                .chain(),
        );
        #[cfg(feature = "audio-opus")]
        app.add_plugins(audio::OpusAudioPlugin);
        app.add_plugins((RuntimePlugin, ScenePlugin, StoragePlugin, GameUiPlugin));
    }
}
