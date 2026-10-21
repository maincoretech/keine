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
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
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
        embedded_asset!(app, "src", "../../assets/shaders/camera/blur.wgsl");
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
        let shader = load_embedded_asset!(
            render_app.world_mut(),
            "../../assets/shaders/camera/blur.wgsl"
        );
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
        render_app.add_systems(
            Render,
            retire_camera_effects_bind_groups.in_set(RenderSystems::PrepareBindGroups),
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
    commands.insert_resource(CameraEffectsBindGroups::default());
}

#[derive(Resource, Default)]
struct CameraEffectsBindGroups(Vec<(TextureViewId, BufferId, BindGroup)>);

#[expect(
    clippy::type_complexity,
    reason = "only active stage views may retain bindings"
)]
fn retire_camera_effects_bind_groups(
    mut cache: ResMut<CameraEffectsBindGroups>,
    uniforms: Res<ComponentUniforms<CompositedCameraEffects>>,
    views: Query<
        (&ViewTarget, &CompositedCameraEffects),
        (
            With<SceneBlurCamera>,
            With<bevy::render::camera::ExtractedCamera>,
        ),
    >,
) {
    let buffer = uniforms.uniforms().buffer().map(Buffer::id);
    cache.0.retain(|(source, uniform, _)| {
        Some(*uniform) == buffer
            && views.iter().any(|(target, effects)| {
                effects.is_active()
                    && (*source == target.main_texture_view().id()
                        || *source == target.main_texture_other_view().id())
            })
    });
}

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
    mut bind_groups: ResMut<CameraEffectsBindGroups>,
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
    let uniform_id = resources
        .uniforms
        .uniforms()
        .buffer()
        .expect("binding exists")
        .id();

    // ViewTarget flips between two main textures. Retaining those two bind
    // groups removes a per-frame allocation; a resize introduces new view IDs,
    // at which point the stale pair is discarded so it cannot retain textures.
    let post_process = view_target.post_process_write();
    let source_id = post_process.source.id();
    let existing = bind_groups.0.iter().position(|(texture_id, buffer_id, _)| {
        *texture_id == source_id && *buffer_id == uniform_id
    });
    let index = existing.unwrap_or_else(|| {
        let bind_group = context.render_device().create_bind_group(
            "camera_effects_bind_group",
            &resources.pipeline.layout,
            &BindGroupEntries::sequential((
                post_process.source,
                &resources.pipeline.sampler,
                uniform_binding.clone(),
            )),
        );
        bind_groups.0.push((source_id, uniform_id, bind_group));
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
    pass.set_bind_group(0, &bind_groups.0[index].2, &[uniform_index.index()]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(target_os = "macos", feature = "video-native"))]
    #[test]
    #[ignore = "requires a real Metal device"]
    fn camera_effects_release_bind_groups_without_running_camera_on_gpu() {
        use crate::render::test_support;
        use bevy::camera::RenderTarget;
        let mut app = test_support::metal_app();
        app.add_plugins((crate::render::blur::BlurPlugin, CameraEffectsPlugin));
        let effect = PostProcessEffect {
            sharpen_strength: 0.5,
            ..default()
        };
        app.world_mut().resource_mut::<GameState>().camera_effect = effect.clone();
        app.world_mut()
            .resource_mut::<GameState>()
            .camera_effect_targets = CameraTargets::ALL;
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::new_target_texture(
                128,
                128,
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
        let camera = app
            .world_mut()
            .spawn((
                Camera2d,
                Msaa::Off,
                RenderTarget::Image(image.clone().into()),
                SceneBlurCamera,
                CompositedCameraEffects::from_effect(&effect, CameraTargets::ALL),
            ))
            .id();
        test_support::start(&mut app);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            app.update();
            if !app
                .sub_app(RenderApp)
                .world()
                .resource::<CameraEffectsBindGroups>()
                .0
                .is_empty()
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "camera effects pipeline did not render"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let old_uniform = app
            .sub_app(RenderApp)
            .world()
            .resource::<ComponentUniforms<CompositedCameraEffects>>()
            .uniforms()
            .buffer()
            .unwrap()
            .id();
        for _ in 0..64 {
            app.world_mut().spawn(CompositedCameraEffects::default());
        }
        for _ in 0..3 {
            app.update();
        }
        let render = app.sub_app(RenderApp).world();
        let uniform = render
            .resource::<ComponentUniforms<CompositedCameraEffects>>()
            .uniforms()
            .buffer()
            .unwrap()
            .id();
        assert_ne!(old_uniform, uniform);
        let cache = render.resource::<CameraEffectsBindGroups>();
        assert!(!cache.0.is_empty());
        assert!(cache.0.iter().all(|(_, id, _)| *id == uniform));

        let old_views: Vec<_> = cache.0.iter().map(|(id, _, _)| *id).collect();
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .get_mut(&image)
            .unwrap()
            .resize(Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            });
        for _ in 0..4 {
            app.update();
        }
        let cache = app
            .sub_app(RenderApp)
            .world()
            .resource::<CameraEffectsBindGroups>();
        assert!(!cache.0.is_empty());
        assert!(cache.0.iter().all(|(id, _, _)| !old_views.contains(id)));

        app.world_mut().get_mut::<Camera>(camera).unwrap().is_active = false;
        for _ in 0..4 {
            app.update();
        }
        assert!(
            app.sub_app(RenderApp)
                .world()
                .resource::<CameraEffectsBindGroups>()
                .0
                .is_empty()
        );
        app.world_mut().get_mut::<Camera>(camera).unwrap().is_active = true;
        for _ in 0..4 {
            app.update();
        }
        assert!(
            !app.sub_app(RenderApp)
                .world()
                .resource::<CameraEffectsBindGroups>()
                .0
                .is_empty()
        );
        app.world_mut().despawn(camera);
        for _ in 0..4 {
            app.update();
        }
        assert!(
            app.sub_app(RenderApp)
                .world()
                .resource::<CameraEffectsBindGroups>()
                .0
                .is_empty()
        );
    }

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
