pub mod blur;
#[path = "render/camera/blur.rs"]
pub(crate) mod camera_blur;

#[cfg(all(test, target_os = "macos", feature = "video-native"))]
pub(crate) mod test_support {
    use bevy::prelude::*;

    pub(crate) fn metal_app() -> App {
        use bevy::render::RenderPlugin;
        use bevy::render::settings::{Backends, RenderCreation, WgpuSettings};
        use bevy::window::{ExitCondition, WindowPlugin};
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .build()
                .disable::<bevy::winit::WinitPlugin>()
                .disable::<bevy::log::LogPlugin>()
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                        backends: Some(Backends::METAL),
                        ..default()
                    })),
                    ..default()
                }),
        );
        app.insert_resource(crate::runtime::resources::GameState(
            keine_core::State::new(),
        ));
        app
    }

    pub(crate) fn start(app: &mut App) {
        app.finish();
        app.cleanup();
        app.insert_resource(bevy::render::error_handler::RenderErrorHandler(
            |error, _, _| panic!("post-process GPU regression: {error:?}"),
        ));
    }
}
