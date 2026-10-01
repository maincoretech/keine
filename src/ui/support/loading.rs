// Loading screen — shown while assets are being loaded.
use crate::render::blur::DialogCamera;
use crate::runtime::resources::AssetLoadingGate;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

#[derive(Component)]
pub(crate) struct LoadingSpinner;

#[derive(Component)]
pub(crate) struct LoadingOverlay;

pub fn setup_loading(mut commands: Commands, camera: Query<Entity, With<DialogCamera>>) {
    let Ok(camera) = camera.single() else {
        return;
    };
    commands
        .spawn((
            Name::new("loading_overlay"),
            LoadingOverlay,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
            FocusPolicy::Block,
            GlobalZIndex(2000),
            UiTargetCamera(camera),
            RenderLayers::layer(2),
        ))
        .with_child((
            LoadingSpinner,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(32.0),
                bottom: Val::Px(32.0),
                width: Val::Px(24.0),
                height: Val::Px(24.0),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Percent(50.0)),
                ..default()
            },
            BorderColor {
                top: Color::NONE,
                right: Color::srgba(1.0, 1.0, 1.0, 0.65),
                bottom: Color::srgba(1.0, 1.0, 1.0, 0.65),
                left: Color::srgba(1.0, 1.0, 1.0, 0.65),
            },
            UiTransform::default(),
        ));
}

pub fn update_loading(
    gate: Res<AssetLoadingGate>,
    time: Res<Time<Real>>,
    mut spinners: Query<&mut UiTransform, With<LoadingSpinner>>,
    mut query: Query<&mut Visibility, With<LoadingOverlay>>,
) {
    // One slow revolution every three seconds, independent of game speed.
    for mut transform in &mut spinners {
        transform.rotation = if gate.blocked {
            transform.rotation * Rot2::radians(time.delta_secs() * std::f32::consts::TAU / 3.0)
        } else {
            Rot2::IDENTITY
        };
    }
    for mut visibility in &mut query {
        *visibility = if gate.blocked {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

pub fn assets_ready(gate: Res<AssetLoadingGate>) -> bool {
    !gate.blocked
}
