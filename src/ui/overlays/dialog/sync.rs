//! Confirmation dialog sync.
use super::*;

pub fn sync_system_message(
    mut commands: Commands,
    state: Res<crate::runtime::resources::GameState>,
    request: Option<Res<DialogRequest>>,
) {
    if request.is_none()
        && let Some(message) = &state.system_message
    {
        commands.insert_resource(DialogRequest::system_message(message));
    }
}

type ModalBackdropQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut UiTargetCamera, &'static mut RenderLayers),
    Or<(With<BacklogRoot>, With<SaveLoadRoot>, With<SettingsRoot>)>,
>;

/// Full-screen menus normally render after their own backdrop blur. When a
/// confirmation dialog opens, temporarily render those menus on the UI camera
/// so the dialog's full-screen post-process also blurs the menu beneath it.
pub fn sync_modal_backdrop_layer(
    request: Option<Res<DialogRequest>>,
    ui_camera: Query<Entity, With<UiBlurCamera>>,
    dialog_camera: Query<Entity, (With<DialogCamera>, Without<UiBlurCamera>)>,
    mut roots: ModalBackdropQuery,
) {
    let target = if request.is_some() {
        ui_camera.single().ok().map(|entity| (entity, 1))
    } else {
        dialog_camera.single().ok().map(|entity| (entity, 2))
    };
    let Some((target, layer)) = target else {
        return;
    };
    for (mut current, mut layers) in &mut roots {
        if current.0 != target {
            *current = UiTargetCamera(target);
            *layers = RenderLayers::layer(layer);
        }
    }
}
