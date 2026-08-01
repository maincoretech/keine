use std::collections::HashMap;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use keine_core::{Anchor, BlendMode, SceneFit, SceneLayerLayout, SpriteLayout};

use crate::runtime::platform::DesignViewport;
use crate::runtime::resources::{GameConfigResource, GameState};
use crate::scene::effects::material::{
    StageMaterial, StageQuad, active_lut_preset, animation_uniform, effective_post_process,
};
use crate::scene::images::ImageDimensions;

/// Marker for sprite entities with stable IDs used by diff-based synchronization.
#[derive(Component)]
pub(crate) struct SpriteNode(pub(crate) String);

#[derive(Default)]
pub(crate) struct SpriteRenderCache {
    initialized: bool,
    sprites: HashMap<String, keine_core::state::Sprite>,
    camera_transform: keine_core::SpriteTransform,
    camera_targets: keine_core::CameraTargets,
    camera_shake: Option<keine_core::state::CameraShakeState>,
    camera_effect: keine_core::PostProcessEffect,
    camera_effect_targets: keine_core::CameraTargets,
}

impl SpriteRenderCache {
    fn matches(&self, state: &GameState) -> bool {
        self.initialized
            && self.sprites == state.sprites
            && self.camera_transform == state.camera_transform
            && self.camera_targets == state.camera_targets
            && self.camera_shake == state.camera_shake
            && self.camera_effect == state.camera_effect
            && self.camera_effect_targets == state.camera_effect_targets
    }

    fn capture(&mut self, state: &GameState) {
        self.initialized = true;
        self.sprites.clone_from(&state.sprites);
        self.camera_transform = state.camera_transform;
        self.camera_targets = state.camera_targets;
        self.camera_shake.clone_from(&state.camera_shake);
        self.camera_effect.clone_from(&state.camera_effect);
        self.camera_effect_targets = state.camera_effect_targets;
    }
}

fn sprite_center_y(
    position_y: f32,
    base_height: f32,
    project_offset_y: f32,
    transform_offset_y: f32,
) -> f32 {
    position_y + base_height * 0.5 + project_offset_y + transform_offset_y
}

fn sprite_geometry(
    layout: SpriteLayout,
    texture_aspect: f32,
    texture_size: Vec2,
    figure_height: f32,
    position: keine_core::Position,
    anchor_offset: f32,
    figure_offset_y: f32,
) -> (Vec2, Vec2) {
    match layout {
        SpriteLayout::Natural => {
            let size = Vec2::new(figure_height * texture_aspect, figure_height);
            let center_x = match position.x {
                Anchor::Left(offset) => offset.max(anchor_offset) + size.x * 0.5,
                Anchor::Center(offset) => keine_core::DESIGN_WIDTH * 0.5 + offset,
                Anchor::Right(offset) => {
                    keine_core::DESIGN_WIDTH - offset.max(anchor_offset) - size.x * 0.5
                }
            };
            (
                size,
                Vec2::new(
                    center_x,
                    sprite_center_y(position.y, figure_height, figure_offset_y, 0.0),
                ),
            )
        }
        SpriteLayout::ViewportHeight(ratio) => {
            let height = keine_core::DESIGN_HEIGHT * ratio.max(f32::EPSILON);
            let size = Vec2::new(height * texture_aspect, height);
            let center_x = match position.x {
                Anchor::Left(offset) => offset.max(anchor_offset) + size.x * 0.5,
                Anchor::Center(offset) => keine_core::DESIGN_WIDTH * 0.5 + offset,
                Anchor::Right(offset) => {
                    keine_core::DESIGN_WIDTH - offset.max(anchor_offset) - size.x * 0.5
                }
            };
            (
                size,
                Vec2::new(
                    center_x,
                    sprite_center_y(position.y, height, figure_offset_y, 0.0),
                ),
            )
        }
        SpriteLayout::Scene(layout) => {
            let size = scene_layer_size(layout, texture_aspect, texture_size);
            // Studio positions are top-left-origin. keine's scene world is
            // bottom-left-origin, so the vertical anchor is mirrored here.
            let center = Vec2::new(
                layout.position[0] + (0.5 - layout.anchor[0]) * size.x,
                keine_core::DESIGN_HEIGHT - layout.position[1] + (layout.anchor[1] - 0.5) * size.y,
            );
            (size, center)
        }
    }
}

fn scene_layer_size(layout: SceneLayerLayout, aspect: f32, texture_size: Vec2) -> Vec2 {
    if let Some([width, height]) = layout.size {
        return Vec2::new(width, height).max(Vec2::ONE);
    }
    let aspect = aspect.max(f32::EPSILON);
    let design = Vec2::new(keine_core::DESIGN_WIDTH, keine_core::DESIGN_HEIGHT);
    match layout.fit {
        SceneFit::Cover => {
            if aspect >= design.x / design.y {
                Vec2::new(design.y * aspect, design.y)
            } else {
                Vec2::new(design.x, design.x / aspect)
            }
        }
        SceneFit::Contain => {
            if aspect >= design.x / design.y {
                Vec2::new(design.x, design.x / aspect)
            } else {
                Vec2::new(design.y * aspect, design.y)
            }
        }
        SceneFit::ByWidth => Vec2::new(design.x, design.x / aspect),
        SceneFit::ByHeight => Vec2::new(design.y * aspect, design.y),
        SceneFit::Stretch => design,
        SceneFit::Center => texture_size.max(Vec2::ONE),
    }
}

#[derive(SystemParam)]
pub(crate) struct SpriteRenderResources<'w> {
    asset_server: Res<'w, AssetServer>,
    dimensions: Res<'w, ImageDimensions>,
    quad: Res<'w, StageQuad>,
    materials: ResMut<'w, Assets<StageMaterial>>,
}

type RenderedSpriteQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Transform,
        Option<&'static mut Sprite>,
        Option<&'static MeshMaterial2d<StageMaterial>>,
    ),
>;

#[derive(SystemParam)]
pub(crate) struct SpriteEntityContext<'w, 's> {
    commands: Commands<'w, 's>,
    nodes: Query<'w, 's, (Entity, &'static SpriteNode)>,
    rendered: RenderedSpriteQuery<'w, 's>,
    windows: Query<'w, 's, Ref<'static, Window>>,
    cache: Local<'s, SpriteRenderCache>,
}

/// Synchronizes character sprites with the engine state via stable sprite IDs.
pub(crate) fn sync_sprites(
    state: Res<GameState>,
    config: Res<GameConfigResource>,
    mut render: SpriteRenderResources,
    mut entities: SpriteEntityContext,
) {
    let Ok(window) = entities.windows.single() else {
        return;
    };
    let sprites_changed = !entities.cache.matches(&state);
    if !sprites_changed
        && !config.is_changed()
        && !render.dimensions.is_changed()
        && !window.is_changed()
    {
        return;
    }
    if sprites_changed {
        entities.cache.capture(&state);
    }
    let viewport = DesignViewport::from_window(&window);
    for (entity, node) in &entities.nodes {
        if !state.sprites.contains_key(&node.0) {
            entities.commands.entity(entity).despawn();
        }
    }

    for (id, data) in &state.sprites {
        let handle: Handle<Image> = render.asset_server.load(config.figure_path(&data.image));
        let texture_size = render
            .dimensions
            .size(&handle)
            .unwrap_or(UVec2::new(756, 1080));
        let texture_aspect = render.dimensions.aspect(&handle).unwrap_or(0.7);
        let (base_size, base_center) = sprite_geometry(
            data.layout,
            texture_aspect,
            texture_size.as_vec2(),
            config.layout.sprite_height,
            data.position,
            config.layout.anchor_offset,
            config.layout.sprite_y_offset,
        );
        let group = if id.starts_with("scene-layer:") {
            "scene"
        } else {
            "characters"
        };
        let mut transform = data.transform;
        let camera_targeted = if group == "scene" {
            state.camera_targets.scene()
        } else {
            state.camera_targets.characters()
        };
        let mut camera_zoom = Vec2::ONE;
        if data.camera_distance.is_some() && camera_targeted {
            let distance = data.camera_distance.unwrap_or(1.0).max(f32::EPSILON);
            let shake_x = state
                .camera_shake
                .as_ref()
                .map_or(0.0, |shake| shake.offset_x);
            let shake_y = state
                .camera_shake
                .as_ref()
                .map_or(0.0, |shake| shake.offset_y);
            transform.offset_x -= (state.camera_transform.offset_x + shake_x) / distance;
            transform.offset_y += (state.camera_transform.offset_y + shake_y) / distance;
            if group == "scene" {
                camera_zoom = Vec2::new(
                    state.camera_transform.scale_x,
                    state.camera_transform.scale_y,
                );
                transform.scale_x *= state.camera_transform.scale_x;
                transform.scale_y *= state.camera_transform.scale_y;
            }
        }
        let post = effective_post_process(
            &state.camera_effect,
            state.camera_effect_targets,
            group,
            data.camera_distance,
        );
        let lut = active_lut_preset(&post)
            .map(|preset| render.asset_server.load(config.lut_path(preset)));
        let progress = data.transition_progress.clamp(0.0, 1.0);
        let transition_x = (1.0 - progress) * data.transition_offset_x;
        let alpha = (progress * transform.alpha).clamp(0.0, 1.0);
        let width = if transform.width > f32::EPSILON {
            transform.width
        } else {
            base_size.x * transform.scale_x
        };
        let height = if transform.height > f32::EPSILON {
            transform.height
        } else {
            base_size.y * transform.scale_y
        };
        let scene_center_adjustment = if matches!(data.layout, SpriteLayout::Scene(_)) {
            Vec2::new(
                (base_center.x - keine_core::DESIGN_WIDTH * 0.5) * (camera_zoom.x - 1.0),
                (base_center.y - keine_core::DESIGN_HEIGHT * 0.5) * (camera_zoom.y - 1.0),
            )
        } else {
            Vec2::ZERO
        };
        let center_x = base_center.x + scene_center_adjustment.x;
        let center_y = base_center.y + scene_center_adjustment.y + transform.offset_y;
        let world_position = viewport.world_from_design(Vec2::new(
            center_x + transform.offset_x + transition_x,
            center_y,
        ));
        let z =
            0.1 + data.z_index as f32 * 0.001 + data.position.y.clamp(-999.0, 999.0) * 0.000_000_01;

        let sprite = Sprite {
            image: handle.clone(),
            custom_size: Some(Vec2::new(width, height) * viewport.scale),
            color: Color::srgba(1.0, 1.0, 1.0, alpha),
            ..default()
        };
        let entity_transform = Transform::from_translation(world_position.extend(z))
            .with_rotation(Quat::from_rotation_z(transform.rotation));
        let mut filter = data.filter;
        filter.blur += transform.blur;
        let animation = animation_uniform(data.films, data.animation.as_ref());
        let uses_material = data.blend != BlendMode::Alpha
            || !filter.is_identity()
            || !post.is_identity()
            || animation.z > 0.0;

        if let Some(entity) = entities
            .nodes
            .iter()
            .find_map(|(entity, node)| (node.0 == *id).then_some(entity))
        {
            let Ok((mut current_transform, current_sprite, existing_material)) =
                entities.rendered.get_mut(entity)
            else {
                continue;
            };
            if uses_material {
                let material =
                    StageMaterial::new(handle, alpha, filter, data.blend, animation, &post, lut);
                let material_handle = if let Some(existing_material) = existing_material {
                    if let Some(mut current) = render.materials.get_mut(&existing_material.0) {
                        *current = material;
                    }
                    existing_material.0.clone()
                } else {
                    render.materials.add(material)
                };
                let mesh_transform = entity_transform.with_scale(Vec3::new(
                    width * viewport.scale,
                    height * viewport.scale,
                    1.0,
                ));
                *current_transform = mesh_transform;
                if existing_material.is_none() {
                    entities.commands.entity(entity).remove::<Sprite>().insert((
                        Mesh2d(render.quad.0.clone()),
                        MeshMaterial2d(material_handle),
                    ));
                }
            } else {
                *current_transform = entity_transform;
                if let Some(mut current_sprite) = current_sprite {
                    *current_sprite = sprite;
                } else {
                    entities
                        .commands
                        .entity(entity)
                        .remove::<Mesh2d>()
                        .remove::<MeshMaterial2d<StageMaterial>>()
                        .insert(sprite);
                }
            }
        } else {
            let mut entity = entities.commands.spawn((
                Name::new(format!("sprite::{id}")),
                SpriteNode(id.clone()),
                RenderLayers::layer(0),
            ));
            if uses_material {
                let material = render.materials.add(StageMaterial::new(
                    handle, alpha, filter, data.blend, animation, &post, lut,
                ));
                entity.insert((
                    Mesh2d(render.quad.0.clone()),
                    MeshMaterial2d(material),
                    entity_transform.with_scale(Vec3::new(
                        width * viewport.scale,
                        height * viewport.scale,
                        1.0,
                    )),
                ));
            } else {
                entity.insert((sprite, entity_transform));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{scene_layer_size, sprite_center_y, sprite_geometry};
    use bevy::prelude::*;
    use keine_core::{Position, SceneFit, SceneLayerLayout, SpriteLayout};

    #[test]
    fn project_offset_moves_the_shared_sprite_baseline() {
        let center = sprite_center_y(0.0, 1080.0, -90.0, 0.0);

        assert_eq!(center, 450.0);
        assert_eq!(center - 540.0, -90.0);
        assert_eq!(center + 540.0, 990.0);
    }

    #[test]
    fn script_transform_remains_relative_to_the_project_offset() {
        assert_eq!(sprite_center_y(12.0, 1080.0, -90.0, 24.0), 486.0);
    }

    #[test]
    fn studio_by_height_layer_keeps_its_wide_canvas() {
        let layout = SceneLayerLayout {
            fit: SceneFit::ByHeight,
            position: [0.0, 0.0],
            anchor: [0.0, 0.0],
            size: None,
        };
        let (size, center) = sprite_geometry(
            SpriteLayout::Scene(layout),
            5359.0 / 1080.0,
            Vec2::new(1920.0, 387.0),
            1080.0,
            Position::left(0.0),
            0.0,
            0.0,
        );

        assert!((size.x - 5359.0).abs() < 0.01);
        assert_eq!(size.y, 1080.0);
        assert!((center.x - 2679.5).abs() < 0.01);
        assert_eq!(center.y, 540.0);
    }

    #[test]
    fn studio_cover_and_by_width_follow_the_design_canvas() {
        let base = SceneLayerLayout {
            position: [960.0, 540.0],
            anchor: [0.5, 0.5],
            ..default()
        };
        let cover = scene_layer_size(
            SceneLayerLayout {
                fit: SceneFit::Cover,
                ..base
            },
            4.0 / 3.0,
            Vec2::new(1440.0, 1080.0),
        );
        let by_width = scene_layer_size(
            SceneLayerLayout {
                fit: SceneFit::ByWidth,
                ..base
            },
            4.0 / 3.0,
            Vec2::new(1440.0, 1080.0),
        );

        assert_eq!(cover, Vec2::new(1920.0, 1440.0));
        assert_eq!(by_width, Vec2::new(1920.0, 1440.0));
    }
}
