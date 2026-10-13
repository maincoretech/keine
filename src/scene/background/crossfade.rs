use super::StageQuad;
use crate::runtime::resources::GameState;
use bevy::asset::{AssetPath, embedded_asset, embedded_path};
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::core_2d::Transparent2d;
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{
    AddRenderCommand, DrawFunctionId, DrawFunctions, RenderCommand, RenderCommandResult,
    SortedRenderPhase, TrackedRenderPass, ViewSortedRenderPhases,
};
use bevy::render::render_resource::{
    AsBindGroup, BlendState, OwnedBindingResource, PipelineCache, RenderPipelineDescriptor,
    SpecializedMeshPipelineError,
};
use bevy::render::sync_world::MainEntity;
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{
    AlphaMode2d, Material2d, Material2dKey, Material2dPlugin, PreparedMaterial2d,
};

pub(crate) struct BackgroundCrossfadePlugin;
impl Plugin for BackgroundCrossfadePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "../../assets/shaders/stage/crossfade.wgsl");
        app.add_plugins((
            Material2dPlugin::<BackgroundCrossfade>::default(),
            ExtractComponentPlugin::<CrossfadeSources>::default(),
        ));
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .add_render_command::<Transparent2d, SkipCrossfade>()
                .add_systems(
                    Render,
                    select_crossfade
                        .after(RenderSystems::Queue)
                        .before(RenderSystems::PhaseSort),
                );
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, PartialEq)]
pub(crate) struct BackgroundCrossfade {
    #[uniform(0)]
    weights: Vec4,
    #[texture(1, visibility(fragment))]
    #[sampler(2, visibility(fragment))]
    previous: Handle<Image>,
    #[texture(3, visibility(fragment))]
    #[sampler(4, visibility(fragment))]
    current: Handle<Image>,
}
impl BackgroundCrossfade {
    pub(super) fn new(previous: Handle<Image>, current: Handle<Image>, progress: f32) -> Self {
        Self {
            weights: Vec4::new(1.0, progress, 0.0, 0.0),
            previous,
            current,
        }
    }
}
impl Material2d for BackgroundCrossfade {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path(
            AssetPath::from_path_buf(embedded_path!("../../assets/shaders/stage/crossfade.wgsl"))
                .with_source("embedded"),
        )
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(target) = descriptor
            .fragment
            .as_mut()
            .and_then(|fragment| fragment.targets.first_mut())
            .and_then(Option::as_mut)
        {
            target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        }
        Ok(())
    }
}

// Only plain, coincident backgrounds can replace two source-over draws.
// Other stage layers must remain above both backgrounds, and each layer's
// filters/masks/animation continue through the existing renderer.
pub(super) fn eligible_progress(state: &GameState) -> Option<f32> {
    let transition = state.bg_transition.as_ref()?;
    (matches!(
        transition.kind,
        keine_core::Transition::Fade(_) | keine_core::Transition::Crossfade(_)
    ) && transition.from.is_some()
        && !transition.to.is_empty()
        && state.bg_transform == keine_core::SpriteTransform::default()
        && state.bg_filter.is_identity()
        && state.bg_films == keine_core::FilmEffects::default()
        && state.bg_animation.is_none()
        && state.bg_camera_distance.is_none()
        && state.camera_effect.is_identity()
        && state.stage_masks.is_empty()
        && state.videos.is_empty()
        && state.particle_effects.is_empty()
        && state.sprites.values().all(|sprite| sprite.z_index > 0))
    .then(|| keine_core::dissolve::smooth_fade(transition.progress))
}

#[derive(Component, Clone, PartialEq, ExtractComponent)]
struct CrossfadeSources {
    material: AssetId<BackgroundCrossfade>,
    images: [AssetId<Image>; 2],
    fallbacks: [Entity; 2],
}
#[derive(Default)]
pub(super) struct CrossfadeVisual {
    entity: Option<Entity>,
    material: Option<Handle<BackgroundCrossfade>>,
    sources: Option<CrossfadeSources>,
    transform: Option<Transform>,
}
impl CrossfadeVisual {
    pub(super) fn sync(
        &mut self,
        commands: &mut Commands,
        materials: &mut Assets<BackgroundCrossfade>,
        quad: &StageQuad,
        desired: Option<(BackgroundCrossfade, [Entity; 2])>,
        transform: Transform,
    ) {
        let Some((material, fallbacks)) = desired else {
            if let Some(entity) = self.entity.take() {
                commands.entity(entity).despawn();
            }
            self.material = None;
            self.sources = None;
            self.transform = None;
            return;
        };
        let images = [material.previous.id(), material.current.id()];
        let handle = self
            .material
            .get_or_insert_with(|| materials.add(material.clone()));
        if materials
            .get(handle.id())
            .is_some_and(|current| *current != material)
            && let Some(mut current) = materials.get_mut(handle.id())
        {
            *current = material;
        }
        let sources = CrossfadeSources {
            material: handle.id(),
            images,
            fallbacks,
        };
        if let Some(entity) = self.entity {
            if self.sources.as_ref() != Some(&sources) {
                commands.entity(entity).insert(sources.clone());
            }
            if self.transform != Some(transform) {
                commands.entity(entity).insert(transform);
            }
        } else {
            self.entity = Some(
                commands
                    .spawn((
                        Name::new("background::crossfade"),
                        sources.clone(),
                        Mesh2d(quad.0.clone()),
                        MeshMaterial2d(handle.clone()),
                        transform,
                        RenderLayers::layer(0),
                    ))
                    .id(),
            );
        }
        self.sources = Some(sources);
        self.transform = Some(transform);
    }
}

struct SkipCrossfade;
impl RenderCommand<Transparent2d> for SkipCrossfade {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = ();
    fn render<'w>(
        _: &Transparent2d,
        _: (),
        _: Option<()>,
        _: (),
        _: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        RenderCommandResult::Skip
    }
}

fn sources_match(
    bindings: &bevy::render::render_resource::BindingResources,
    source_images: [AssetId<Image>; 2],
    images: &RenderAssets<GpuImage>,
) -> bool {
    // Bindings can still hold an older image pair while asset preparation is
    // waiting. Check actual views and samplers, including resize/hot reload/
    // device recovery, before removing this frame's fallback sprite draws.
    [1, 3].into_iter().zip(source_images).all(|(binding, image)| {
        let Some(image) = images.get(image) else { return false; };
        let texture_matches = bindings.iter().any(|(index, resource)| {
            *index == binding && matches!(resource, OwnedBindingResource::TextureView(_, view) if view.id() == image.texture_view.id())
        });
        let sampler_matches = bindings.iter().any(|(index, resource)| {
            *index == binding + 1 && matches!(resource, OwnedBindingResource::Sampler(_, sampler) if sampler.id() == image.sampler.id())
        });
        texture_matches && sampler_matches
    })
}

fn select_crossfade(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent2d>>,
    sources: Query<(&MainEntity, &CrossfadeSources)>,
    materials: Res<RenderAssets<PreparedMaterial2d<BackgroundCrossfade>>>,
    images: Res<RenderAssets<GpuImage>>,
    pipelines: Res<PipelineCache>,
    functions: Res<DrawFunctions<Transparent2d>>,
) {
    let Ok((entity, sources)) = sources.single() else {
        return;
    };
    let functions = functions.read();
    let skip = functions.id::<SkipCrossfade>();
    for phase in phases.values_mut() {
        let Some(item) = phase
            .items
            .values_mut()
            .find(|item| item.entity.1 == *entity)
        else {
            continue;
        };
        let material = materials.get(sources.material);
        let ready = pipelines.get_render_pipeline(item.pipeline).is_some()
            && material
                .is_some_and(|material| sources_match(&material.bindings, sources.images, &images));
        let draw = material.map_or(skip, |material| material.properties.draw_function_id);
        choose_phase(phase, *entity, sources.fallbacks, draw, skip, ready);
    }
}

fn choose_phase(
    phase: &mut SortedRenderPhase<Transparent2d>,
    entity: MainEntity,
    fallbacks: [Entity; 2],
    draw: DrawFunctionId,
    skip: DrawFunctionId,
    ready: bool,
) {
    let Some(item) = phase
        .items
        .values_mut()
        .find(|item| item.entity.1 == entity)
    else {
        return;
    };
    item.draw_function = if ready { draw } else { skip };
    if ready {
        // Sprite phase items are transient and requeued every frame. The
        // combined mesh item is retained: keep it queued even while its
        // pipeline/material is pending, so retry can activate next frame.
        phase
            .items
            .retain(|_, item| !fallbacks.contains(&item.entity.1.id()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::FloatOrd;
    use bevy::render::render_phase::PhaseItemExtraIndex;
    use bevy::render::render_resource::CachedRenderPipelineId;
    use bevy::render::view::RetainedViewEntity;

    #[test]
    fn fallback_survives_pending_pipeline_and_recovers_after_readiness_loss() {
        let mut app = App::new();
        app.init_resource::<DrawFunctions<Transparent2d>>()
            .add_render_command::<Transparent2d, SkipCrossfade>()
            .add_render_command::<Transparent2d, (SkipCrossfade,)>();
        let functions = app
            .world()
            .resource::<DrawFunctions<Transparent2d>>()
            .read();
        let skip = functions.id::<SkipCrossfade>();
        let draw = functions.id::<(SkipCrossfade,)>();
        let mut world = World::new();
        let ids = std::array::from_fn::<_, 4, _>(|_| world.spawn_empty().id());
        let view = RetainedViewEntity::new(ids[3].into(), None, 0);
        let mut phases = ViewSortedRenderPhases::<Transparent2d>::default();
        phases.prepare_for_new_frame(view);
        let item = |id: Entity, render_entity: Entity| Transparent2d {
            entity: (render_entity, id.into()),
            sort_key: FloatOrd(0.0),
            pipeline: CachedRenderPipelineId::INVALID,
            draw_function: draw,
            batch_range: 0..1,
            extracted_index: 0,
            extra_index: PhaseItemExtraIndex::None,
            indexed: true,
        };
        // Material2d phases use a placeholder render entity; selection must
        // use MainEntity. A retained item must remain queued while unavailable.
        phases
            .get_mut(&view)
            .unwrap()
            .add_retained(item(ids[2], Entity::PLACEHOLDER));
        for ready in [false, true, false, true] {
            phases.prepare_for_new_frame(view);
            let phase = phases.get_mut(&view).unwrap();
            for id in [ids[0], ids[1], ids[3]] {
                phase.add_transient(item(id, id));
            }
            choose_phase(phase, ids[2].into(), [ids[0], ids[1]], draw, skip, ready);
            let combined = phase
                .items
                .get(&(Entity::PLACEHOLDER, ids[2].into()))
                .unwrap();
            assert_eq!(combined.draw_function, if ready { draw } else { skip });
            for id in [ids[0], ids[1]] {
                assert_eq!(phase.items.contains_key(&(id, id.into())), !ready);
            }
            assert!(phase.items.contains_key(&(ids[3], ids[3].into())));
            assert_eq!(phase.items.len(), if ready { 2 } else { 4 });
        }
    }

    #[test]
    fn only_plain_coincident_fades_with_other_layers_above_are_eligible() {
        use keine_core::{BlendMode, SpriteTransform, Transition};
        let mut state = GameState(keine_core::State::new());
        state.bg_transition = Some(keine_core::state::BgTransition {
            from: Some("old".into()),
            to: "new".into(),
            progress: 0.5,
            kind: Transition::Fade(0.3),
            blocking: false,
        });
        for (kind, expected) in [
            (Transition::Fade(0.3), true),
            (Transition::Crossfade(0.3), true),
            (Transition::Wipe(0.3), false),
            (Transition::Dissolve(0.3), false),
            (Transition::SlideFromLeft(0.3), false),
            (Transition::Instant, false),
        ] {
            state.bg_transition.as_mut().unwrap().kind = kind;
            assert_eq!(eligible_progress(&state).is_some(), expected);
        }
        state.bg_transition.as_mut().unwrap().kind = Transition::Fade(0.3);
        for case in 0..6 {
            let mut changed = GameState(state.0.clone());
            match case {
                0 => changed.bg_transform.offset_x = 1.0,
                1 => changed.bg_filter.blur = 1.0,
                2 => changed.bg_camera_distance = Some(1.0),
                3 => changed.camera_effect.shock_intensity = 0.5,
                4 => changed.bg_transition.as_mut().unwrap().from = None,
                _ => changed.bg_transition.as_mut().unwrap().to.clear(),
            }
            assert!(eligible_progress(&changed).is_none(), "case {case}");
        }
        for z_index in [-1, 0, 1, 10] {
            state.sprites.insert(
                "sprite".into(),
                keine_core::state::Sprite {
                    image: "sprite".into(),
                    position: keine_core::Position::center(0.0),
                    layout: default(),
                    transition_progress: 1.0,
                    transition: Transition::Instant,
                    entering: true,
                    transition_offset_x: 0.0,
                    transition_blocking: false,
                    transform: SpriteTransform::default(),
                    transform_animation: None,
                    position_animation: None,
                    keyframe_animation: None,
                    filter: default(),
                    films: default(),
                    animation: None,
                    z_index,
                    blend: BlendMode::Alpha,
                    camera_distance: None,
                },
            );
            assert_eq!(eligible_progress(&state).is_some(), z_index > 0);
        }
    }
}

#[cfg(all(test, target_os = "macos", feature = "video-native"))]
mod gpu_tests {
    use super::*;
    use bevy::render::render_resource::{
        BindingResources, SamplerBindingType, TextureViewDimension,
    };
    use wgpu::util::DeviceExt;

    #[test]
    #[ignore = "requires a real Metal device"]
    fn transparent_crossfade_and_replaced_bindings_on_gpu() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = futures_lite::future::block_on(instance.request_adapter(&default())).unwrap();
        let (device, queue) =
            futures_lite::future::block_on(adapter.request_device(&default())).unwrap();
        let descriptor = wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let pixels = |seed: u32| {
            (0..512)
                .flat_map(|pixel| {
                    [
                        ((pixel * 37 + seed) % 256) as u8,
                        ((pixel * 53 + seed * 2) % 256) as u8,
                        ((pixel * 17 + seed * 3) % 256) as u8,
                        ((pixel * 61 + seed) % 256) as u8,
                    ]
                })
                .collect::<Vec<_>>()
        };
        let textures = [3, 127].map(|seed| {
            device.create_texture_with_data(
                &queue,
                &descriptor,
                wgpu::util::TextureDataOrder::LayerMajor,
                &pixels(seed),
            )
        });
        let views = textures
            .each_ref()
            .map(|texture| texture.create_view(&default()));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..default()
        });

        // Real resource IDs: a replaced view or sampler must keep the two
        // fallback sprites, even when the asset ID and pipeline are unchanged.
        let mut assets = Assets::<Image>::default();
        let handles = [assets.add(Image::default()), assets.add(Image::default())];
        let pair = handles.each_ref().map(Handle::id);
        let mut gpu_images = RenderAssets::<GpuImage>::default();
        for index in 0..2 {
            gpu_images.insert(
                pair[index],
                GpuImage {
                    texture: textures[index].clone().into(),
                    texture_view: views[index].clone().into(),
                    sampler: sampler.clone().into(),
                    texture_descriptor: descriptor.clone(),
                    texture_view_descriptor: None,
                    had_data: true,
                },
            );
        }
        let mut bindings = BindingResources(vec![]);
        for (binding, id) in [1, 3].into_iter().zip(pair) {
            let image = gpu_images.get(id).unwrap();
            bindings.push((
                binding,
                OwnedBindingResource::TextureView(
                    TextureViewDimension::D2,
                    image.texture_view.clone(),
                ),
            ));
            bindings.push((
                binding + 1,
                OwnedBindingResource::Sampler(SamplerBindingType::Filtering, image.sampler.clone()),
            ));
        }
        assert!(sources_match(&bindings, pair, &gpu_images));
        let original = gpu_images.get(pair[0]).unwrap().clone();
        let mut changed = original.clone();
        changed.texture_view = textures[0].create_view(&default()).into();
        gpu_images.insert(pair[0], changed);
        assert!(!sources_match(&bindings, pair, &gpu_images));
        let mut changed = original.clone();
        changed.sampler = device.create_sampler(&default()).into();
        gpu_images.insert(pair[0], changed);
        assert!(!sources_match(&bindings, pair, &gpu_images));
        gpu_images.insert(pair[0], original);
        gpu_images.remove(pair[1]);
        assert!(!sources_match(&bindings, pair, &gpu_images));

        // Execute the production fragment against two ordinary source-over
        // reference draws. sRGB attachments retain the existing rounding step.
        let production = include_str!("../../assets/shaders/stage/crossfade.wgsl")
            .lines()
            .filter(|line| !line.starts_with("#import"))
            .collect::<Vec<_>>()
            .join("\n")
            .replace("#{MATERIAL_BIND_GROUP}", "0");
        let source = format!(
            r#"
struct VertexOutput {{ @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }}
@vertex fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {{
    let uv = array<vec2<f32>, 3>(vec2<f32>(0.,0.), vec2<f32>(2.,0.), vec2<f32>(0.,2.))[index];
    return VertexOutput(vec4<f32>(uv * vec2<f32>(2.,-2.) + vec2<f32>(-1.,1.),0.,1.), uv);
}}
{production}
@fragment fn reference_old(mesh: VertexOutput) -> @location(0) vec4<f32> {{
    let color = textureSample(previous, previous_sampler, mesh.uv);
    return vec4<f32>(color.rgb, color.a * weights.x);
}}
@fragment fn reference_current(mesh: VertexOutput) -> @location(0) vec4<f32> {{
    let color = textureSample(current, current_sampler, mesh.uv);
    return vec4<f32>(color.rgb, color.a * weights.y);
}}
"#
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("crossfade-reference"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let make_pipeline = |entry, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    compilation_options: default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: descriptor.format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: default(),
                depth_stencil: None,
                multisample: default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let combined = make_pipeline("fragment", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let old = make_pipeline("reference_old", wgpu::BlendState::ALPHA_BLENDING);
        let incoming = make_pipeline("reference_current", wgpu::BlendState::ALPHA_BLENDING);
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entries = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&views[0]),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&views[1]),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ];
        let make_bind_group = |pipeline: &wgpu::RenderPipeline, indices: &[u32]| {
            let entries = entries
                .iter()
                .filter(|entry| indices.contains(&entry.binding))
                .cloned()
                .collect::<Vec<_>>();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            })
        };
        let groups = [
            make_bind_group(&old, &[0, 1, 2]),
            make_bind_group(&incoming, &[0, 3, 4]),
            make_bind_group(&combined, &[0, 1, 2, 3, 4]),
        ];
        let target_descriptor = wgpu::TextureDescriptor {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            ..descriptor
        };
        let targets = std::array::from_fn::<_, 2, _>(|_| device.create_texture(&target_descriptor));
        let target_views = targets
            .each_ref()
            .map(|texture| texture.create_view(&default()));
        let mut max_difference = 0;
        for alpha in [0.3, 1.0] {
            for progress in [0.0_f32, 0.13, 0.5, 0.87, 1.0] {
                let weights = [1.0_f32, progress, 0., 0.]
                    .into_iter()
                    .flat_map(f32::to_ne_bytes)
                    .collect::<Vec<_>>();
                queue.write_buffer(&uniform, 0, &weights);
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 4096,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let mut encoder = device.create_command_encoder(&default());
                for (index, view) in target_views.iter().enumerate() {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.18,
                                    g: 0.31,
                                    b: 0.59,
                                    a: alpha,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..default()
                    });
                    if index == 0 {
                        pass.set_bind_group(0, &groups[0], &[]);
                        pass.set_pipeline(&old);
                        pass.draw(0..3, 0..1);
                        pass.set_bind_group(0, &groups[1], &[]);
                        pass.set_pipeline(&incoming);
                        pass.draw(0..3, 0..1);
                    } else {
                        pass.set_bind_group(0, &groups[2], &[]);
                        pass.set_pipeline(&combined);
                        pass.draw(0..3, 0..1);
                    }
                }
                for (index, texture) in targets.iter().enumerate() {
                    encoder.copy_texture_to_buffer(
                        texture.as_image_copy(),
                        wgpu::TexelCopyBufferInfo {
                            buffer: &readback,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: index as u64 * 2048,
                                bytes_per_row: Some(256),
                                rows_per_image: Some(8),
                            },
                        },
                        target_descriptor.size,
                    );
                }
                let submission = queue.submit([encoder.finish()]);
                let (sender, receiver) = std::sync::mpsc::channel();
                readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        sender.send(result).unwrap();
                    });
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(std::time::Duration::from_secs(10)),
                    })
                    .unwrap();
                receiver
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap()
                    .unwrap();
                let bytes = readback.slice(..).get_mapped_range();
                let difference = bytes[..2048]
                    .iter()
                    .zip(&bytes[2048..])
                    .map(|(reference, combined)| reference.abs_diff(*combined))
                    .max()
                    .unwrap();
                max_difference = max_difference.max(difference);
                assert!(
                    difference <= 2,
                    "progress {progress}, destination alpha {alpha}, max byte difference {difference}"
                );
            }
        }
        eprintln!(
            "crossfade GPU source-over comparison: maximum 8-bit difference {max_difference}/255"
        );
    }
}
