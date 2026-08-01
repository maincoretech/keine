use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use keine_core::SpriteTransform;
use keine_core::dissolve;
use keine_core::{DESIGN_HEIGHT, DESIGN_WIDTH};

use crate::runtime::platform::DesignViewport;
use crate::runtime::resources::GameConfigResource;
use crate::runtime::resources::GameState;
use crate::scene::effects::material::{
    StageMaterial, StageQuad, active_lut_preset, animation_uniform, effective_post_process,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BackgroundLayer {
    Previous,
    Current,
}

#[derive(Component)]
pub(crate) struct BackgroundNode {
    pub(crate) layer: BackgroundLayer,
    pub(crate) image: String,
}

#[derive(Default)]
pub(crate) struct BackgroundRenderCache {
    initialized: bool,
    image: Option<String>,
    transition: Option<keine_core::state::BgTransition>,
    transform: SpriteTransform,
    filter: keine_core::VisualFilter,
    films: keine_core::FilmEffects,
    animation: Option<keine_core::state::PresetAnimation>,
    camera_distance: Option<f32>,
    camera_transform: SpriteTransform,
    camera_targets: keine_core::CameraTargets,
    camera_shake: Option<keine_core::state::CameraShakeState>,
    camera_effect: keine_core::PostProcessEffect,
    camera_effect_targets: keine_core::CameraTargets,
}

impl BackgroundRenderCache {
    fn matches(&self, state: &GameState) -> bool {
        self.initialized
            && self.image == state.bg
            && self.transition == state.bg_transition
            && self.transform == state.bg_transform
            && self.filter == state.bg_filter
            && self.films == state.bg_films
            && self.animation == state.bg_animation
            && self.camera_distance == state.bg_camera_distance
            && self.camera_transform == state.camera_transform
            && self.camera_targets == state.camera_targets
            && self.camera_shake == state.camera_shake
            && self.camera_effect == state.camera_effect
            && self.camera_effect_targets == state.camera_effect_targets
    }

    fn capture(&mut self, state: &GameState) {
        self.initialized = true;
        self.image.clone_from(&state.bg);
        self.transition.clone_from(&state.bg_transition);
        self.transform = state.bg_transform;
        self.filter = state.bg_filter;
        self.films = state.bg_films;
        self.animation.clone_from(&state.bg_animation);
        self.camera_distance = state.bg_camera_distance;
        self.camera_transform = state.camera_transform;
        self.camera_targets = state.camera_targets;
        self.camera_shake.clone_from(&state.camera_shake);
        self.camera_effect.clone_from(&state.camera_effect);
        self.camera_effect_targets = state.camera_effect_targets;
    }
}

struct DesiredBackground<'a> {
    layer: BackgroundLayer,
    image: &'a str,
    alpha: f32,
    z: f32,
    transform: SpriteTransform,
    transition: Vec4,
}

type BackgroundQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut BackgroundNode,
        &'static mut Transform,
        Option<&'static MeshMaterial2d<StageMaterial>>,
        Option<&'static mut Sprite>,
    ),
>;

#[derive(SystemParam)]
pub(crate) struct BackgroundSyncContext<'w, 's> {
    state: Res<'w, GameState>,
    config: Res<'w, GameConfigResource>,
    asset_server: Res<'w, AssetServer>,
    commands: Commands<'w, 's>,
    backgrounds: BackgroundQuery<'w, 's>,
    quad: Res<'w, StageQuad>,
    materials: ResMut<'w, Assets<StageMaterial>>,
    windows: Query<'w, 's, Ref<'static, Window>>,
    cache: Local<'s, BackgroundRenderCache>,
}

/// Synchronizes background entities without recreating them every frame.
pub fn sync_bg(context: BackgroundSyncContext) {
    let BackgroundSyncContext {
        state,
        config,
        asset_server,
        mut commands,
        mut backgrounds,
        quad,
        mut materials,
        windows,
        mut cache,
    } = context;
    let Ok(window) = windows.single() else {
        return;
    };
    if cache.matches(&state) && !config.is_changed() && !window.is_changed() {
        return;
    }
    cache.capture(&state);
    let viewport = DesignViewport::from_window(&window);
    let desired = desired_backgrounds(&state);
    let mut previous_exists = false;
    let mut current_exists = false;

    for (entity, mut node, mut transform, existing_material, existing_sprite) in &mut backgrounds {
        let Some(background) = desired.iter().find(|item| item.layer == node.layer) else {
            commands.entity(entity).despawn();
            continue;
        };

        match node.layer {
            BackgroundLayer::Previous => previous_exists = true,
            BackgroundLayer::Current => current_exists = true,
        }
        node.image = background.image.to_owned();
        let image = asset_server.load(config.bg_path(background.image));
        let post = effective_post_process(
            &state.camera_effect,
            state.camera_effect_targets,
            "scene",
            state.bg_camera_distance,
        );
        let lut = active_lut_preset(&post).map(|preset| asset_server.load(config.lut_path(preset)));
        apply_background_entity(
            &mut commands,
            entity,
            &mut transform,
            existing_material,
            &mut materials,
            &quad,
            image,
            background,
            viewport,
            state.bg_filter,
            camera_transform(&state),
            post,
            lut,
            existing_sprite,
            true,
        );
    }

    for background in desired {
        let exists = match background.layer {
            BackgroundLayer::Previous => previous_exists,
            BackgroundLayer::Current => current_exists,
        };
        if exists {
            continue;
        }
        let mut transform = Transform::default();
        let image = asset_server.load(config.bg_path(background.image));
        let post = effective_post_process(
            &state.camera_effect,
            state.camera_effect_targets,
            "scene",
            state.bg_camera_distance,
        );
        let lut = active_lut_preset(&post).map(|preset| asset_server.load(config.lut_path(preset)));
        let entity = commands
            .spawn((
                Name::new(format!("background::{:?}", background.layer)),
                BackgroundNode {
                    layer: background.layer,
                    image: background.image.to_owned(),
                },
                transform,
                RenderLayers::layer(0),
            ))
            .id();
        apply_background_entity(
            &mut commands,
            entity,
            &mut transform,
            None,
            &mut materials,
            &quad,
            image,
            &background,
            viewport,
            state.bg_filter,
            camera_transform(&state),
            post,
            lut,
            None,
            false,
        );
    }
}

fn camera_transform(state: &GameState) -> Option<(SpriteTransform, f32)> {
    let targeted = state.camera_targets.scene();
    state
        .bg_camera_distance
        .filter(|_| targeted)
        .map(|distance| {
            let mut camera = state.camera_transform;
            if let Some(shake) = &state.camera_shake {
                camera.offset_x += shake.offset_x;
                camera.offset_y += shake.offset_y;
            }
            (camera, distance.max(f32::EPSILON))
        })
}

fn desired_backgrounds(state: &GameState) -> Vec<DesiredBackground<'_>> {
    let animation = animation_uniform(state.bg_films, state.bg_animation.as_ref());
    if let Some(transition) = &state.bg_transition {
        let mut backgrounds = Vec::with_capacity(2);
        if let Some(previous) = &transition.from {
            backgrounds.push(DesiredBackground {
                layer: BackgroundLayer::Previous,
                image: previous,
                alpha: 1.0,
                z: -1.0,
                transform: state.bg_transform,
                transition: animation,
            });
        }
        if !transition.to.is_empty() {
            let mut transform = state.bg_transform;
            let slide_remaining = 1.0 - dissolve::smooth_fade(transition.progress);
            match transition.kind {
                keine_core::Transition::SlideFromLeft(_) => {
                    transform.offset_x -= DESIGN_WIDTH * slide_remaining;
                }
                keine_core::Transition::SlideFromRight(_) => {
                    transform.offset_x += DESIGN_WIDTH * slide_remaining;
                }
                _ => {}
            }
            backgrounds.push(DesiredBackground {
                layer: BackgroundLayer::Current,
                image: &transition.to,
                alpha: match transition.kind {
                    keine_core::Transition::Wipe(_)
                    | keine_core::Transition::Dissolve(_)
                    | keine_core::Transition::SlideFromLeft(_)
                    | keine_core::Transition::SlideFromRight(_) => 1.0,
                    _ => dissolve::smooth_fade(transition.progress),
                },
                z: 0.0,
                transform,
                transition: match transition.kind {
                    keine_core::Transition::Wipe(_) => {
                        Vec4::new(1.0, transition.progress, animation.z, animation.w)
                    }
                    keine_core::Transition::Dissolve(_) => {
                        Vec4::new(2.0, transition.progress, animation.z, animation.w)
                    }
                    _ => animation,
                },
            });
        }
        backgrounds
    } else {
        state
            .bg
            .as_deref()
            .map(|image| {
                vec![DesiredBackground {
                    layer: BackgroundLayer::Current,
                    image,
                    alpha: 1.0,
                    z: 0.0,
                    transform: state.bg_transform,
                    transition: animation,
                }]
            })
            .unwrap_or_default()
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_background_entity(
    commands: &mut Commands,
    entity: Entity,
    transform: &mut Transform,
    existing_material: Option<&MeshMaterial2d<StageMaterial>>,
    materials: &mut Assets<StageMaterial>,
    quad: &StageQuad,
    image: Handle<Image>,
    background: &DesiredBackground<'_>,
    viewport: DesignViewport,
    mut filter: keine_core::VisualFilter,
    camera: Option<(SpriteTransform, f32)>,
    post: keine_core::PostProcessEffect,
    lut: Option<Handle<Image>>,
    existing_sprite: Option<Mut<'_, Sprite>>,
    existing_entity: bool,
) {
    let mut effect = background.transform;
    if let Some((camera, distance)) = camera {
        effect.offset_x -= camera.offset_x / distance;
        effect.offset_y += camera.offset_y / distance;
        effect.scale_x *= camera.scale_x;
        effect.scale_y *= camera.scale_y;
    }
    filter.blur += effect.blur;
    let size = Vec2::new(
        DESIGN_WIDTH * effect.scale_x,
        DESIGN_HEIGHT * effect.scale_y,
    ) * viewport.scale;
    let alpha = background.alpha * effect.alpha;
    transform.translation = (viewport.content_center()
        + Vec2::new(effect.offset_x, effect.offset_y) * viewport.scale)
        .extend(background.z);
    transform.rotation = Quat::from_rotation_z(effect.rotation);
    if !filter.is_identity()
        || !post.is_identity()
        || background.transition.x > 0.0
        || background.transition.z > 0.0
    {
        transform.scale = size.extend(1.0);
        let material = StageMaterial::new(
            image,
            alpha,
            filter,
            keine_core::BlendMode::Alpha,
            background.transition,
            &post,
            lut,
        );
        let material_handle = if let Some(existing) = existing_material {
            if let Some(mut current) = materials.get_mut(&existing.0) {
                *current = material;
            }
            existing.0.clone()
        } else {
            materials.add(material)
        };
        if existing_material.is_none() {
            commands
                .entity(entity)
                .remove::<Sprite>()
                .insert((Mesh2d(quad.0.clone()), MeshMaterial2d(material_handle)));
        }
    } else {
        transform.scale = Vec3::ONE;
        let desired = Sprite {
            image,
            custom_size: Some(size),
            color: Color::srgba(1.0, 1.0, 1.0, alpha),
            ..default()
        };
        if let Some(mut sprite) = existing_sprite {
            *sprite = desired;
        } else {
            commands
                .entity(entity)
                .remove::<Mesh2d>()
                .remove::<MeshMaterial2d<StageMaterial>>()
                .insert(desired);
        }
    }
    if !existing_entity {
        commands.entity(entity).insert(*transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogue_changes_do_not_invalidate_the_background() {
        let mut state = keine_core::State::new();
        state.bg = Some("scene.webp".into());
        let mut cache = BackgroundRenderCache::default();
        cache.capture(&GameState(state.clone()));

        state.dialogue = Some(keine_core::state::Dialogue {
            speaker: "A".into(),
            text: "hello".into(),
            markup: "hello".into(),
            visible_chars: 1,
            pauses: Vec::new(),
            vocal: None,
            volume: 1.0,
            auto_advance: false,
        });
        assert!(cache.matches(&GameState(state.clone())));
        state.bg_transform.offset_x = 10.0;
        assert!(!cache.matches(&GameState(state)));
    }
}
