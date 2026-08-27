use std::collections::HashMap;

use bevy::asset::{AssetPath, embedded_asset, embedded_path};
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};
use keine_core::{
    StageMaskFillMode, StageMaskFit, StageMaskImageChannel, StageMaskMode, StageMaskPlane,
    StageMaskScope, StageMaskShape, StageMaskState, StageMaskTextureBlend, StageMaskVisibility,
};

use crate::runtime::platform::DesignViewport;
use crate::runtime::resources::GameState;
use crate::scene::effects::material::{StageMaterial, StageQuad};
use crate::scene::images::{ImageRole, ImageRoleRegistry};

pub(crate) struct StageMaskPlugin;

impl Plugin for StageMaskPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "../assets/shaders/stage_mask.wgsl");
        app.add_plugins(Material2dPlugin::<StageMaskMaterial>::default())
            .add_systems(Update, sync.in_set(crate::runtime::GameSystemSet::Sync));
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, PartialEq)]
struct StageMaskMaterial {
    #[uniform(0)]
    shape_a: Vec4,
    #[uniform(0)]
    shape_b: Vec4,
    #[uniform(0)]
    shape_c: Vec4,
    #[uniform(0)]
    fill_a: Vec4,
    #[uniform(0)]
    fill_b: Vec4,
    #[uniform(0)]
    color: Vec4,
    #[uniform(0)]
    gradient_start: Vec4,
    #[uniform(0)]
    gradient_end: Vec4,
    #[uniform(0)]
    effect_a: Vec4,
    #[uniform(0)]
    effect_b: Vec4,
    #[texture(1, visibility(fragment))]
    #[sampler(2, visibility(fragment))]
    mask_image: Option<Handle<Image>>,
    #[texture(3, visibility(fragment))]
    #[sampler(4, visibility(fragment))]
    fill_texture: Option<Handle<Image>>,
}

impl Material2d for StageMaskMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path(
            AssetPath::from_path_buf(embedded_path!("../assets/shaders/stage_mask.wgsl"))
                .with_source("embedded"),
        )
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        _descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        Ok(())
    }
}

#[derive(Component)]
struct StageMaskNode(String);

#[derive(Default)]
struct StageMaskIndex(HashMap<String, Entity>);

#[derive(Default)]
struct StageMaskCache {
    initialized: bool,
    revision: u64,
}

type MaskQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static StageMaskNode,
        &'static mut Transform,
        &'static mut RenderLayers,
        &'static MeshMaterial2d<StageMaskMaterial>,
    ),
>;

#[allow(clippy::too_many_arguments)]
fn sync(
    state: Res<GameState>,
    asset_server: Res<AssetServer>,
    image_roles: Res<ImageRoleRegistry>,
    quad: Res<StageQuad>,
    mut materials: ResMut<Assets<StageMaskMaterial>>,
    windows: Query<Ref<Window>>,
    mut commands: Commands,
    mut masks: MaskQuery,
    mut index: Local<StageMaskIndex>,
    mut cache: Local<StageMaskCache>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    if cache.initialized && cache.revision == state.stage_revision && !window.is_changed() {
        return;
    }
    cache.initialized = true;
    cache.revision = state.stage_revision;
    let viewport = DesignViewport::from_window(&window);

    index.0.retain(|id, entity| {
        let retained = state.stage_masks.get(id).is_some_and(|mask| {
            mask.mask.mode == StageMaskMode::Overlay && mask.current > f32::EPSILON
        });
        if !retained {
            commands.entity(*entity).despawn();
        }
        retained
    });

    let mut ordered = state.stage_masks.iter().collect::<Vec<_>>();
    ordered.sort_unstable_by_key(|(_, mask)| mask.order);
    for (id, active) in ordered {
        if active.mask.mode != StageMaskMode::Overlay || active.current <= f32::EPSILON {
            continue;
        }
        let material = overlay_material(active, viewport, &asset_server, &image_roles);
        let (layer, z) = mask_plane(active.mask.plane);
        if let Some(entity) = index.0.get(id).copied()
            && let Ok((_, node, mut transform, mut render_layers, handle)) = masks.get_mut(entity)
        {
            debug_assert_eq!(&node.0, id);
            *transform = overlay_transform(viewport, z);
            *render_layers = RenderLayers::layer(layer);
            if materials
                .get(&handle.0)
                .is_some_and(|current| current != &material)
                && let Some(mut current) = materials.get_mut(&handle.0)
            {
                *current = material;
            }
            continue;
        }

        let material = materials.add(material);
        let entity = commands
            .spawn((
                Name::new(format!("stage-mask::{id}")),
                StageMaskNode(id.clone()),
                Mesh2d(quad.0.clone()),
                MeshMaterial2d(material),
                overlay_transform(viewport, z),
                RenderLayers::layer(layer),
            ))
            .id();
        index.0.insert(id.clone(), entity);
    }
}

fn overlay_transform(viewport: DesignViewport, z: f32) -> Transform {
    Transform::from_translation(viewport.content_center().extend(z)).with_scale(Vec3::new(
        keine_core::DESIGN_WIDTH * viewport.scale,
        keine_core::DESIGN_HEIGHT * viewport.scale,
        1.0,
    ))
}

fn mask_plane(plane: StageMaskPlane) -> (usize, f32) {
    match plane {
        StageMaskPlane::BehindScene => (0, -1_000.0),
        StageMaskPlane::Bottom => (0, 0.05),
        StageMaskPlane::Top => (1, 1_000.0),
        StageMaskPlane::Topmost => (2, 1_000.0),
    }
}

fn overlay_material(
    active: &StageMaskState,
    viewport: DesignViewport,
    asset_server: &AssetServer,
    image_roles: &ImageRoleRegistry,
) -> StageMaskMaterial {
    let mask = &active.mask;
    let center = mask_center(mask.center, viewport);
    let half_size = mask_half_size(mask.size, viewport);
    StageMaskMaterial {
        shape_a: Vec4::new(
            shape_code(mask.shape),
            f32::from(mask.visibility == StageMaskVisibility::Outside),
            mask.feather * viewport.scale,
            mask.opacity * active.current,
        ),
        shape_b: Vec4::new(center.x, center.y, half_size.x, half_size.y),
        shape_c: Vec4::new(
            mask.rotation,
            mask.radius * viewport.scale,
            f32::from(mask.image_channel == StageMaskImageChannel::Luminance),
            fit_code(mask.image_fit),
        ),
        fill_a: Vec4::new(
            fill_code(mask.fill_mode),
            mask.gradient_direction,
            fit_code(mask.texture_fit),
            blend_code(mask.texture_blend),
        ),
        fill_b: Vec4::new(
            mask.texture_scale,
            mask.texture_opacity,
            mask.blur * viewport.scale,
            0.0,
        ),
        color: Vec4::from_array(mask.color),
        gradient_start: Vec4::from_array(mask.gradient_start),
        gradient_end: Vec4::from_array(mask.gradient_end),
        effect_a: Vec4::new(
            mask.vignette_amount,
            mask.vignette_size,
            mask.noise_amount,
            mask.noise_size * viewport.scale,
        ),
        effect_b: Vec4::new(mask.hue, mask.saturation, mask.brightness, 0.0),
        mask_image: load_optional(mask.image.as_deref(), asset_server, image_roles),
        fill_texture: load_optional(mask.texture.as_deref(), asset_server, image_roles),
    }
}

fn load_optional(
    path: Option<&str>,
    asset_server: &AssetServer,
    image_roles: &ImageRoleRegistry,
) -> Option<Handle<Image>> {
    path.filter(|path| !path.is_empty()).map(|path| {
        crate::scene::images::load(asset_server, image_roles, path.to_owned(), ImageRole::RAW)
    })
}

#[derive(Clone)]
pub(crate) struct StageClipInput {
    a: Vec4,
    b: Vec4,
    c: Vec4,
    image: Option<Handle<Image>>,
}

impl StageClipInput {
    pub(crate) fn apply(self, material: &mut StageMaterial) {
        material.clip_a = self.a;
        material.clip_b = self.b;
        material.clip_c = self.c;
        material.clip_image = self.image;
    }
}

pub(crate) fn clip_input(
    state: &GameState,
    group: &str,
    id: &str,
    viewport: DesignViewport,
    asset_server: &AssetServer,
    image_roles: &ImageRoleRegistry,
) -> Option<StageClipInput> {
    let active = state
        .stage_masks
        .values()
        .filter(|active| {
            active.mask.mode == StageMaskMode::Clip
                && active.current > f32::EPSILON
                && mask_targets(&active.mask, group, id)
        })
        .max_by_key(|active| active.order)?;
    let mask = &active.mask;
    let center = mask_center(mask.center, viewport);
    let half_size = mask_half_size(mask.size, viewport);
    Some(StageClipInput {
        a: Vec4::new(
            active.current,
            shape_code(mask.shape),
            f32::from(mask.visibility == StageMaskVisibility::Outside),
            mask.feather * viewport.scale,
        ),
        b: Vec4::new(center.x, center.y, half_size.x, half_size.y),
        c: Vec4::new(
            mask.rotation,
            mask.radius * viewport.scale,
            f32::from(mask.image_channel == StageMaskImageChannel::Luminance),
            fit_code(mask.image_fit),
        ),
        image: load_optional(mask.image.as_deref(), asset_server, image_roles),
    })
}

fn mask_targets(mask: &keine_core::StageMask, group: &str, id: &str) -> bool {
    match mask.scope {
        StageMaskScope::Scene => group == "scene",
        StageMaskScope::Characters => group == "characters",
        StageMaskScope::All => true,
        StageMaskScope::Selected => mask.targets.iter().any(|target| {
            target == id
                || id
                    .strip_prefix("scene-layer:")
                    .is_some_and(|layer| target == layer)
                || (id == "background" && matches!(target.as_str(), "scene" | "background"))
        }),
    }
}

fn mask_center(center: [f32; 2], viewport: DesignViewport) -> Vec2 {
    viewport.world_from_design(Vec2::new(
        keine_core::DESIGN_WIDTH * center[0] / 100.0,
        keine_core::DESIGN_HEIGHT * (1.0 - center[1] / 100.0),
    ))
}

fn mask_half_size(size: [f32; 2], viewport: DesignViewport) -> Vec2 {
    Vec2::new(
        keine_core::DESIGN_WIDTH * size[0] / 200.0,
        keine_core::DESIGN_HEIGHT * size[1] / 200.0,
    ) * viewport.scale
}

const fn shape_code(shape: StageMaskShape) -> f32 {
    match shape {
        StageMaskShape::Rectangle => 0.0,
        StageMaskShape::RoundedRectangle => 1.0,
        StageMaskShape::Ellipse => 2.0,
        StageMaskShape::Image => 3.0,
    }
}

const fn fit_code(fit: StageMaskFit) -> f32 {
    match fit {
        StageMaskFit::Stretch => 0.0,
        StageMaskFit::Cover => 1.0,
        StageMaskFit::Contain => 2.0,
    }
}

const fn fill_code(fill: StageMaskFillMode) -> f32 {
    match fill {
        StageMaskFillMode::Solid => 0.0,
        StageMaskFillMode::Gradient => 1.0,
        StageMaskFillMode::Texture => 2.0,
    }
}

const fn blend_code(blend: StageMaskTextureBlend) -> f32 {
    match blend {
        StageMaskTextureBlend::Normal => 0.0,
        StageMaskTextureBlend::Multiply => 1.0,
        StageMaskTextureBlend::Screen => 2.0,
        StageMaskTextureBlend::Add => 3.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_scope_matches_stable_scene_layer_ids() {
        let mut mask = test_mask();
        mask.scope = StageMaskScope::Selected;
        mask.targets = vec!["clouds".into()];
        assert!(mask_targets(&mask, "scene", "scene-layer:clouds"));
        assert!(!mask_targets(&mask, "characters", "hero"));
    }

    fn test_mask() -> keine_core::StageMask {
        keine_core::StageMask {
            mode: StageMaskMode::Clip,
            plane: StageMaskPlane::Bottom,
            scope: StageMaskScope::All,
            targets: Vec::new(),
            shape: StageMaskShape::Rectangle,
            image: None,
            image_channel: StageMaskImageChannel::Alpha,
            image_fit: StageMaskFit::Stretch,
            center: [50.0, 50.0],
            size: [50.0, 50.0],
            rotation: 0.0,
            radius: 0.0,
            visibility: StageMaskVisibility::Inside,
            feather: 0.0,
            opacity: 1.0,
            fill_mode: StageMaskFillMode::Solid,
            color: [0.0, 0.0, 0.0, 1.0],
            gradient_start: [0.0, 0.0, 0.0, 1.0],
            gradient_end: [1.0, 1.0, 1.0, 1.0],
            gradient_direction: 0.0,
            texture: None,
            texture_fit: StageMaskFit::Cover,
            texture_blend: StageMaskTextureBlend::Normal,
            texture_scale: 1.0,
            texture_opacity: 1.0,
            blur: 0.0,
            vignette_amount: 0.0,
            vignette_size: 0.5,
            noise_amount: 0.0,
            noise_size: 1.0,
            hue: 0.0,
            saturation: 1.0,
            brightness: 1.0,
        }
    }
}
