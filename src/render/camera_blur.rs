//! One-pass optical effects for camera effects that target the composed stage.

use std::borrow::Cow;

use bevy::asset::{embedded_asset, load_embedded_asset};
use bevy::core_pipeline::{Core2d, Core2dSystems, FullscreenShader};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_resource::{binding_types::*, *};
use bevy::render::renderer::{RenderAdapter, RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::ViewTarget;
use bevy::render::{RenderApp, RenderStartup};
use keine_core::{CameraTargets, PostProcessEffect};

use crate::render::blur::SceneBlurCamera;
use crate::runtime::resources::GameState;

const ACTIVE: f32 = 0.001;

/// Optical camera effects that can be evaluated once after layer composition.
///
/// Three `Vec4`s keep the extracted uniform compact and naturally aligned on
/// all supported graphics backends.
#[derive(Component, Clone, Copy, Debug, Default, ExtractComponent, ShaderType)]
pub(crate) struct CompositedCameraEffects {
    params_a: Vec4,
    params_b: Vec4,
    params_c: Vec4,
}

impl CompositedCameraEffects {
    fn from_effect(effect: &PostProcessEffect, targets: CameraTargets) -> Self {
        if !uses_composited_path(targets) {
            return Self::default();
        }
        Self {
            params_a: Vec4::new(
                effect.radial_blur_strength.clamp(0.0, 1.0),
                effect.radial_blur_center_x.clamp(0.0, 1.0),
                effect.radial_blur_center_y.clamp(0.0, 1.0),
                effect.motion_blur_strength.clamp(0.0, 1.0),
            ),
            params_b: Vec4::new(
                effect.motion_blur_angle.to_radians(),
                effect.zoom_blur_strength.clamp(0.0, 1.0),
                effect.zoom_blur_center_x.clamp(0.0, 1.0),
                effect.zoom_blur_center_y.clamp(0.0, 1.0),
            ),
            params_c: Vec4::new(
                effect.chromatic_aberration.clamp(0.0, 1.0),
                effect.sharpen_strength.clamp(0.0, 2.0),
                effect.bloom_intensity.clamp(0.0, 2.0),
                0.0,
            ),
        }
    }

    fn is_active(self) -> bool {
        self.params_a.x.max(self.params_b.y) > ACTIVE
            || self.params_a.w > ACTIVE
            || self.params_c.x.max(self.params_c.y).max(self.params_c.z) > ACTIVE
    }
}

/// Camera blur can move out of individual materials only when both stage
/// groups are targeted. Selective effects must retain their per-layer path.
pub(crate) fn uses_composited_path(targets: CameraTargets) -> bool {
    targets == CameraTargets::ALL
}

/// Removes only the fields handled by [`CompositedCameraEffects`]. Local image
/// blur, focal-distance blur and every non-optical post effect stay untouched.
pub(crate) fn strip_composited_effects(effect: &mut PostProcessEffect, targets: CameraTargets) {
    if !uses_composited_path(targets) {
        return;
    }
    effect.radial_blur_strength = 0.0;
    effect.motion_blur_strength = 0.0;
    effect.zoom_blur_strength = 0.0;
    effect.chromatic_aberration = 0.0;
    effect.sharpen_strength = 0.0;
    effect.bloom_intensity = 0.0;
}

pub(crate) struct CameraEffectsPlugin;

impl Plugin for CameraEffectsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "src", "../assets/shaders/camera_blur.wgsl");
        app.add_plugins((
            ExtractComponentPlugin::<CompositedCameraEffects>::default(),
            UniformComponentPlugin::<CompositedCameraEffects>::default(),
        ))
        .add_systems(PostUpdate, sync_camera_effects);
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let shader =
            load_embedded_asset!(render_app.world_mut(), "../assets/shaders/camera_blur.wgsl");
        render_app.insert_resource(CameraEffectsShader(shader));
        render_app.add_systems(
            Core2d,
            run_camera_effects
                .in_set(Core2dSystems::EarlyPostProcess)
                .in_set(super::blur::BlurRenderSet::CompositedCamera),
        );
        render_app.add_systems(
            RenderStartup,
            setup_camera_effects_pipeline.ambiguous_with_all(),
        );
    }
}

fn sync_camera_effects(
    state: Res<GameState>,
    mut cameras: Query<&mut CompositedCameraEffects, With<SceneBlurCamera>>,
) {
    let Ok(mut blur) = cameras.single_mut() else {
        return;
    };
    let next =
        CompositedCameraEffects::from_effect(&state.camera_effect, state.camera_effect_targets);
    if blur.params_a != next.params_a
        || blur.params_b != next.params_b
        || blur.params_c != next.params_c
    {
        *blur = next;
    }
}

#[derive(Resource)]
struct CameraEffectsShader(Handle<Shader>);

#[derive(Resource)]
struct CameraEffectsPipeline {
    layout: BindGroupLayout,
    layout_descriptor: BindGroupLayoutDescriptor,
    sampler: Sampler,
    shader: Handle<Shader>,
    vertex: VertexState,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct CameraEffectsPipelineKey(TextureFormat);

impl SpecializedRenderPipeline for CameraEffectsPipeline {
    type Key = CameraEffectsPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some(Cow::Borrowed("composited_camera_effects")),
            layout: vec![self.layout_descriptor.clone()],
            immediate_size: 0,
            vertex: self.vertex.clone(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                entry_point: Some(Cow::Borrowed("fragment")),
                targets: vec![Some(ColorTargetState {
                    format: key.0,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            zero_initialize_workgroup_memory: false,
        }
    }
}

fn setup_camera_effects_pipeline(
    device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    shader: Res<CameraEffectsShader>,
    mut commands: Commands,
) {
    let entries = &BindGroupLayoutEntries::sequential(
        ShaderStages::FRAGMENT,
        (
            texture_2d(TextureSampleType::Float { filterable: true }),
            sampler(SamplerBindingType::Filtering),
            uniform_buffer::<CompositedCameraEffects>(true),
        ),
    );
    let layout_descriptor = BindGroupLayoutDescriptor::new("camera_effects_layout", entries);
    let layout = device.create_bind_group_layout("camera_effects_layout", entries);
    let sampler = device.create_sampler(&SamplerDescriptor {
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(CameraEffectsPipeline {
        layout,
        layout_descriptor,
        sampler,
        shader: shader.0.clone(),
        vertex: fullscreen_shader.to_vertex_state(),
    });
    commands.insert_resource(SpecializedRenderPipelines::<CameraEffectsPipeline>::default());
}

#[derive(Default)]
struct CameraEffectsBindGroups(Vec<(TextureViewId, BindGroup)>);

#[derive(SystemParam)]
struct CameraEffectsRenderResources<'w> {
    pipeline_cache: Res<'w, PipelineCache>,
    pipeline: Res<'w, CameraEffectsPipeline>,
    adapter: Res<'w, RenderAdapter>,
    pipelines: ResMut<'w, SpecializedRenderPipelines<CameraEffectsPipeline>>,
    uniforms: Res<'w, ComponentUniforms<CompositedCameraEffects>>,
}

fn run_camera_effects(
    view: ViewQuery<(
        &ViewTarget,
        &CompositedCameraEffects,
        &DynamicUniformIndex<CompositedCameraEffects>,
        Option<&SceneBlurCamera>,
    )>,
    mut resources: CameraEffectsRenderResources,
    mut bind_groups: Local<CameraEffectsBindGroups>,
    mut context: RenderContext,
) {
    let (view_target, blur, uniform_index, marker) = view.into_inner();
    if marker.is_none() {
        return;
    }
    let format = view_target.main_texture_format();
    let features = resources.adapter.get_texture_format_features(format);
    if !features
        .allowed_usages
        .contains(TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT)
        || !features
            .flags
            .contains(TextureFormatFeatureFlags::FILTERABLE)
    {
        return;
    }
    let pipeline_id = resources.pipelines.specialize(
        &resources.pipeline_cache,
        &resources.pipeline,
        CameraEffectsPipelineKey(format),
    );
    // Queue the format-specific pipeline while the stage is still unblurred.
    // Activating an authored effect later must not reveal one plain frame while
    // the render pipeline compiles.
    if !blur.is_active() {
        return;
    }
    let Some(render_pipeline) = resources.pipeline_cache.get_render_pipeline(pipeline_id) else {
        return;
    };
    let Some(uniform_binding) = resources.uniforms.uniforms().binding() else {
        return;
    };

    // ViewTarget flips between two main textures. Retaining those two bind
    // groups removes a per-frame allocation; a resize introduces new view IDs,
    // at which point the stale pair is discarded so it cannot retain textures.
    let post_process = view_target.post_process_write();
    let source_id = post_process.source.id();
    let existing = bind_groups
        .0
        .iter()
        .position(|(texture_id, _)| *texture_id == source_id);
    let index = existing.unwrap_or_else(|| {
        if bind_groups.0.len() >= 2 {
            bind_groups.0.clear();
        }
        let bind_group = context.render_device().create_bind_group(
            "camera_effects_bind_group",
            &resources.pipeline.layout,
            &BindGroupEntries::sequential((
                post_process.source,
                &resources.pipeline.sampler,
                uniform_binding.clone(),
            )),
        );
        bind_groups.0.push((source_id, bind_group));
        bind_groups.0.len() - 1
    });

    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("composited_camera_effects"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post_process.destination,
            resolve_target: None,
            ops: Operations::default(),
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(render_pipeline);
    pass.set_bind_group(0, &bind_groups.0[index].1, &[uniform_index.index()]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blur_effect() -> PostProcessEffect {
        PostProcessEffect {
            radial_blur_strength: 0.3,
            motion_blur_strength: 0.4,
            zoom_blur_strength: 0.5,
            chromatic_aberration: 0.6,
            sharpen_strength: 0.7,
            bloom_intensity: 0.8,
            ..default()
        }
    }

    #[test]
    fn all_targets_route_optical_effects_to_composited_pass() {
        let mut layer = blur_effect();
        let composed = CompositedCameraEffects::from_effect(&layer, CameraTargets::ALL);
        strip_composited_effects(&mut layer, CameraTargets::ALL);

        assert!(composed.is_active());
        assert_eq!(layer.radial_blur_strength, 0.0);
        assert_eq!(layer.motion_blur_strength, 0.0);
        assert_eq!(layer.zoom_blur_strength, 0.0);
        assert_eq!(layer.chromatic_aberration, 0.0);
        assert_eq!(layer.sharpen_strength, 0.0);
        assert_eq!(layer.bloom_intensity, 0.0);
    }

    #[test]
    fn selective_targets_keep_optical_effects_in_stage_materials() {
        for targets in [CameraTargets::SCENE, CameraTargets::CHARACTERS] {
            let mut layer = blur_effect();
            let composed = CompositedCameraEffects::from_effect(&layer, targets);
            strip_composited_effects(&mut layer, targets);

            assert!(!composed.is_active());
            assert_eq!(layer, blur_effect());
        }
    }
}
