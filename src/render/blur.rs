// Lightweight region-based Gaussian blur post-processing for Bevy 0.19.
use crate::runtime::resources::GameState;
use crate::scene::sprites::SpriteNode;
use crate::ui::control_bar::{
    AutoHideTiming, BlurSource, BlurStrength, HideContentBg, UiBlurSource,
};
use crate::ui::dialog::DialogRequest;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{Core2d, Core2dSystems, FullscreenShader},
    render::{
        RenderApp, RenderStartup,
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_resource::{binding_types::*, *},
        renderer::{RenderAdapter, RenderContext, RenderDevice, ViewQuery},
        view::ViewTarget,
    },
};
use std::borrow::Cow;

// Keep CPU scissor padding aligned with the shader clamp. Larger values used
// to enlarge the processed area without producing a stronger visual result.
const MAX_BLUR_STRENGTH: f32 = 48.0;
// Give every UI surface a small, consistent lift without changing blur values
// authored by scripts for backgrounds and sprites.
const UI_BLUR_STRENGTH_SCALE: f32 = 1.125;

// ── BlurCamera ──
#[derive(Component, Clone, ExtractComponent, ShaderType)]
pub struct BlurCamera {
    pub count: u32,
    pub _pad: UVec3,
    pub rects: [BlurRect; 16],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub(crate) struct BlurRect {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
    pub coc: f32,
    pub _pad: Vec3,
}

/// The layer-0 camera applies regional blur behind the regular UI.
#[derive(Component, Clone, ExtractComponent)]
pub struct SceneBlurCamera;

/// The layer-1 camera applies full-screen blur behind a modal dialog.
#[derive(Component, Clone, ExtractComponent)]
pub struct UiBlurCamera;

/// The layer-2 camera renders modal UI after all backdrop processing.
#[derive(Component)]
pub struct DialogCamera;

impl Default for BlurCamera {
    fn default() -> Self {
        Self {
            count: 0,
            _pad: UVec3::ZERO,
            rects: [BlurRect::default(); 16],
        }
    }
}

// ── Plugin ──
pub struct BlurPlugin;
impl Plugin for BlurPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "src", "../assets/shaders/blur.wgsl");
        app.add_plugins(ExtractComponentPlugin::<BlurCamera>::default());
        app.add_plugins(UniformComponentPlugin::<BlurCamera>::default());
        app.add_plugins(ExtractComponentPlugin::<SceneBlurCamera>::default());
        app.add_plugins(ExtractComponentPlugin::<UiBlurCamera>::default());
    }
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let shader_handle: Handle<Shader> =
            load_embedded_asset!(render_app.world_mut(), "../assets/shaders/blur.wgsl");
        render_app.insert_resource(BlurShader(shader_handle));
        render_app.add_systems(
            Core2d,
            do_scene_blur.in_set(Core2dSystems::EarlyPostProcess),
        );
        render_app.add_systems(
            Core2d,
            do_ui_blur
                .after(bevy::ui_render::render_pass::ui_pass)
                .before(bevy::core_pipeline::upscaling::upscaling),
        );
        render_app.add_systems(RenderStartup, setup_blur_pipeline.ambiguous_with_all());
    }
}

// ── Pipeline resource ──
#[derive(Resource)]
struct BlurShader(Handle<Shader>);
#[derive(Resource)]
struct BlurPipeline {
    layout: BindGroupLayout,
    layout_desc: BindGroupLayoutDescriptor,
    sampler: Sampler,
    shader: Handle<Shader>,
    vertex: VertexState,
}

struct BlurIntermediate {
    _texture: Texture,
    view: TextureView,
    size: Extent3d,
    format: TextureFormat,
}

#[derive(Resource, Default)]
struct BlurIntermediatePool(Option<BlurIntermediate>);

struct BlurRunResources<'a> {
    pipeline_cache: &'a PipelineCache,
    pipeline: &'a BlurPipeline,
    adapter: &'a RenderAdapter,
    pipelines: &'a mut SpecializedRenderPipelines<BlurPipeline>,
    uniforms: &'a ComponentUniforms<BlurCamera>,
}

#[derive(SystemParam)]
struct BlurRenderResources<'w> {
    pipeline_cache: Res<'w, PipelineCache>,
    pipeline: Res<'w, BlurPipeline>,
    adapter: Res<'w, RenderAdapter>,
    pipelines: ResMut<'w, SpecializedRenderPipelines<BlurPipeline>>,
    uniforms: Res<'w, ComponentUniforms<BlurCamera>>,
    intermediate: ResMut<'w, BlurIntermediatePool>,
}

struct BlurPassTargets<'a> {
    source: &'a TextureView,
    destination: &'a TextureView,
    scissor: ScissorRect,
}

#[derive(Clone, Copy)]
struct ScissorRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct BlurPipelineKey {
    format: TextureFormat,
    vertical: bool,
}

impl SpecializedRenderPipeline for BlurPipeline {
    type Key = BlurPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some(Cow::Borrowed(if key.vertical {
                "blur_vertical"
            } else {
                "blur_horizontal"
            })),
            layout: vec![self.layout_desc.clone()],
            immediate_size: 0,
            vertex: self.vertex.clone(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                shader_defs: vec![],
                entry_point: Some(Cow::Borrowed(if key.vertical {
                    "vertical"
                } else {
                    "horizontal"
                })),
                targets: vec![Some(ColorTargetState {
                    format: key.format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            zero_initialize_workgroup_memory: false,
        }
    }
}

fn setup_blur_pipeline(
    device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    shader: Res<BlurShader>,
    mut commands: Commands,
) {
    let entries = &BindGroupLayoutEntries::sequential(
        ShaderStages::FRAGMENT,
        (
            texture_2d(TextureSampleType::Float { filterable: true }),
            sampler(SamplerBindingType::Filtering),
            uniform_buffer::<BlurCamera>(true),
        ),
    );
    let layout_desc = BindGroupLayoutDescriptor::new("blur_layout", entries);
    let layout = device.create_bind_group_layout("blur_layout", entries);
    let sampler = device.create_sampler(&SamplerDescriptor {
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(BlurPipeline {
        layout,
        layout_desc,
        sampler,
        shader: shader.0.clone(),
        vertex: fullscreen_shader.to_vertex_state(),
    });
    commands.insert_resource(SpecializedRenderPipelines::<BlurPipeline>::default());
    commands.insert_resource(BlurIntermediatePool::default());
}

// ── Render function ──

fn do_scene_blur(
    view: ViewQuery<(
        &ViewTarget,
        &BlurCamera,
        &DynamicUniformIndex<BlurCamera>,
        Option<&SceneBlurCamera>,
    )>,
    mut resources: BlurRenderResources,
    mut ctx: RenderContext,
) {
    let (vt, bc, index, marker) = view.into_inner();
    if marker.is_none() {
        return;
    }
    run_blur(
        vt,
        bc,
        index,
        BlurRunResources {
            pipeline_cache: &resources.pipeline_cache,
            pipeline: &resources.pipeline,
            adapter: &resources.adapter,
            pipelines: &mut resources.pipelines,
            uniforms: &resources.uniforms,
        },
        &mut resources.intermediate.0,
        &mut ctx,
    );
}

fn do_ui_blur(
    view: ViewQuery<(
        &ViewTarget,
        &BlurCamera,
        &DynamicUniformIndex<BlurCamera>,
        Option<&UiBlurCamera>,
    )>,
    mut resources: BlurRenderResources,
    mut ctx: RenderContext,
) {
    let (vt, bc, index, marker) = view.into_inner();
    if marker.is_none() {
        return;
    }
    run_blur(
        vt,
        bc,
        index,
        BlurRunResources {
            pipeline_cache: &resources.pipeline_cache,
            pipeline: &resources.pipeline,
            adapter: &resources.adapter,
            pipelines: &mut resources.pipelines,
            uniforms: &resources.uniforms,
        },
        &mut resources.intermediate.0,
        &mut ctx,
    );
}

fn run_blur(
    vt: &ViewTarget,
    bc: &BlurCamera,
    uniform_index: &DynamicUniformIndex<BlurCamera>,
    mut resources: BlurRunResources,
    intermediate: &mut Option<BlurIntermediate>,
    ctx: &mut RenderContext,
) {
    // Avoid allocating/encoding either regional pass when no blur is requested.
    if bc.count == 0 {
        return;
    }

    let Some(uniform_binding) = resources.uniforms.uniforms().binding() else {
        return;
    };
    let format = vt.main_texture_format();
    let features = resources.adapter.get_texture_format_features(format);
    if !features
        .allowed_usages
        .contains(TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT)
        || !features
            .flags
            .contains(TextureFormatFeatureFlags::FILTERABLE)
    {
        // A GAL can safely degrade to an unblurred backdrop on an unusual
        // off-screen target instead of failing the entire render pipeline.
        return;
    }
    let horizontal = resources.pipelines.specialize(
        resources.pipeline_cache,
        resources.pipeline,
        BlurPipelineKey {
            format,
            vertical: false,
        },
    );
    let vertical = resources.pipelines.specialize(
        resources.pipeline_cache,
        resources.pipeline,
        BlurPipelineKey {
            format,
            vertical: true,
        },
    );
    let size = vt.main_texture().size();
    let recreate_intermediate = intermediate
        .as_ref()
        .is_none_or(|texture| texture.size != size || texture.format != format);
    if recreate_intermediate {
        let texture = ctx.render_device().create_texture(&TextureDescriptor {
            label: Some("blur_intermediate"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        *intermediate = Some(BlurIntermediate {
            _texture: texture,
            view,
            size,
            format,
        });
    }
    let intermediate_view = &intermediate.as_ref().expect("created above").view;
    let original = vt.main_texture_view().clone();
    let horizontal_scissor = blur_scissor(bc, size, true);
    let vertical_scissor = blur_scissor(bc, size, false);
    run_blur_pass(
        BlurPassTargets {
            source: &original,
            destination: intermediate_view,
            scissor: horizontal_scissor,
        },
        horizontal,
        uniform_index,
        &uniform_binding,
        &mut resources,
        ctx,
    );
    run_blur_pass(
        BlurPassTargets {
            source: intermediate_view,
            destination: vt.main_texture_view(),
            scissor: vertical_scissor,
        },
        vertical,
        uniform_index,
        &uniform_binding,
        &mut resources,
        ctx,
    );
}

fn run_blur_pass(
    targets: BlurPassTargets,
    pipeline_id: CachedRenderPipelineId,
    uniform_index: &DynamicUniformIndex<BlurCamera>,
    uniform_binding: &BindingResource<'_>,
    resources: &mut BlurRunResources,
    ctx: &mut RenderContext,
) {
    let Some(pipeline) = resources.pipeline_cache.get_render_pipeline(pipeline_id) else {
        return;
    };
    // ViewTarget textures can change after resize or render recovery. Creating
    // these two small bind groups per active blur avoids caching against wgpu
    // implementation details and follows Bevy's post-process pattern.
    let bind_group = ctx.render_device().create_bind_group(
        "blur_bg",
        &resources.pipeline.layout,
        &BindGroupEntries::sequential((
            targets.source,
            &resources.pipeline.sampler,
            uniform_binding.clone(),
        )),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("blur"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: targets.destination,
            resolve_target: None,
            // Regional passes must preserve every pixel outside the scissor.
            // `Operations::default()` clears the attachment, which would turn
            // the untouched part of the camera target black.
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[uniform_index.index()]);
    pass.set_scissor_rect(
        targets.scissor.x,
        targets.scissor.y,
        targets.scissor.width,
        targets.scissor.height,
    );
    pass.draw(0..3, 0..1);
}

fn blur_scissor(camera: &BlurCamera, size: Extent3d, horizontal: bool) -> ScissorRect {
    let mut min = Vec2::splat(f32::MAX);
    let mut max = Vec2::splat(f32::MIN);
    for region in camera.rects.iter().take(camera.count as usize) {
        let padding = if horizontal {
            (region.coc * 0.25 * 1.5).ceil()
        } else {
            0.0
        };
        min.x = min.x.min(region.min_x);
        min.y = min.y.min(region.min_y - padding);
        max.x = max.x.max(region.max_x);
        max.y = max.y.max(region.max_y + padding);
    }
    // The horizontal intermediate needs outward padding for the vertical
    // kernel. The final pass must round inward: outward rounding would copy a
    // one-pixel row of horizontally blurred intermediate data beyond a
    // fractional UI boundary.
    let (min_x, min_y, max_x, max_y) = if horizontal {
        (min.x.floor(), min.y.floor(), max.x.ceil(), max.y.ceil())
    } else {
        (min.x.ceil(), min.y.ceil(), max.x.floor(), max.y.floor())
    };
    let x = min_x.clamp(0.0, size.width as f32) as u32;
    let y = min_y.clamp(0.0, size.height as f32) as u32;
    let max_x = max_x.clamp(x as f32, size.width as f32) as u32;
    let max_y = max_y.clamp(y as f32, size.height as f32) as u32;
    ScissorRect {
        x,
        y,
        width: max_x - x,
        height: max_y - y,
    }
}

// ── Main-world system ──
/// Reads UI node positions from ComputedNode + UiGlobalTransform (Bevy 0.19 UI layout).
/// BlurSource markers tell the system which nodes need blur behind them.
/// No more manual coordinate sync — just tag a node and blur follows.
type SceneBlurQuery<'w, 's> =
    Query<'w, 's, &'static mut BlurCamera, (With<SceneBlurCamera>, Without<UiBlurCamera>)>;
type UiBlurQuery<'w, 's> =
    Query<'w, 's, &'static mut BlurCamera, (With<UiBlurCamera>, Without<SceneBlurCamera>)>;
type BlurNodeQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static ComputedNode,
        &'static UiGlobalTransform,
        Option<&'static BlurStrength>,
        Option<&'static HideContentBg>,
        &'static InheritedVisibility,
    ),
    With<BlurSource>,
>;

#[derive(Default)]
pub(crate) struct BlurRegionScratch {
    scene: Vec<BlurRect>,
    ui: Vec<BlurRect>,
}

#[derive(SystemParam)]
pub struct BlurBehavior<'w> {
    dialog: Option<Res<'w, DialogRequest>>,
    timing: Res<'w, AutoHideTiming>,
    state: Res<'w, GameState>,
    textbox_fade: Res<'w, crate::ui::textbox::TextboxOverlayFade>,
    initial_textbox_fade: Res<'w, crate::ui::textbox::InitialTextboxFade>,
}

#[derive(SystemParam)]
pub struct BlurSources<'w, 's> {
    blur_nodes: BlurNodeQuery<'w, 's>,
    ui_blur_nodes: Query<
        'w,
        's,
        (
            &'static ComputedNode,
            &'static UiGlobalTransform,
            Option<&'static BlurStrength>,
            &'static InheritedVisibility,
        ),
        With<UiBlurSource>,
    >,
    world_sprites: Query<'w, 's, (&'static SpriteNode, &'static Sprite, &'static Transform)>,
}

pub fn update_blur_regions(
    window_query: Query<&Window>,
    mut scene_blur_query: SceneBlurQuery,
    mut ui_blur_query: UiBlurQuery,
    behavior: BlurBehavior,
    sources: BlurSources,
    mut scratch: Local<BlurRegionScratch>,
) {
    let Ok(w) = window_query.single() else { return };
    let sf = w.scale_factor();
    let sh = w.height() * sf;
    let design_viewport = crate::runtime::platform::DesignViewport::from_window(w);
    let ui_origin = design_viewport.offset * sf;
    let blur_scale = design_viewport.scale * sf;

    // Camera 1 runs after the regular UI has been composited, so a full-screen
    // pass here blurs the scene, textbox, and control bar together. Camera 2
    // then draws the dialog itself without post-processing.
    if behavior.dialog.is_some() {
        if let Ok(mut bc) = scene_blur_query.single_mut()
            && bc.count != 0
        {
            bc.count = 0;
        }
        if let Ok(mut bc) = ui_blur_query.single_mut() {
            let min = ui_origin;
            let max = min
                + Vec2::new(keine_core::DESIGN_WIDTH, keine_core::DESIGN_HEIGHT)
                    * design_viewport.scale
                    * sf;
            let region = BlurRect {
                min_x: min.x,
                max_x: max.x,
                min_y: min.y,
                max_y: max.y,
                coc: clamp_ui_blur(crate::ui::FULLSCREEN_BLUR_STRENGTH, blur_scale),
                _pad: Vec3::ZERO,
            };
            if bc.count != 1 || bc.rects[0] != region {
                bc.rects[0] = region;
                bc.count = 1;
            }
        }
        return;
    }

    if let Ok(mut bc) = ui_blur_query.single_mut() {
        scratch.ui.clear();
        for (node, transform, strength, visibility) in &sources.ui_blur_nodes {
            let strength = strength.map_or(30.0, |strength| strength.0);
            let size = node.size();
            if !visibility.get() || strength <= f32::EPSILON || size.x <= 0.0 || size.y <= 0.0 {
                continue;
            }
            let position = transform.translation + ui_origin;
            let half = size * 0.5;
            scratch.ui.push(BlurRect {
                min_x: position.x - half.x,
                max_x: position.x + half.x,
                min_y: position.y - half.y,
                max_y: position.y + half.y,
                coc: clamp_ui_blur(strength, blur_scale),
                _pad: Vec3::ZERO,
            });
        }
        if prepare_regions(&bc, &mut scratch.ui) {
            write_prepared_regions(&mut bc, &mut scratch.ui);
        } else {
            scratch.ui.clear();
        }
    }

    let Ok(mut bc) = scene_blur_query.single_mut() else {
        return;
    };

    scratch.scene.clear();
    if behavior.state.bg_transform.blur > 0.0 {
        scratch.scene.push(BlurRect {
            min_x: 0.0,
            max_x: w.width() * sf,
            min_y: 0.0,
            max_y: sh,
            coc: clamp_blur(behavior.state.bg_transform.blur * blur_scale),
            _pad: Vec3::ZERO,
        });
    }
    for (node, sprite, transform) in &sources.world_sprites {
        let Some(effect) = behavior
            .state
            .sprites
            .get(&node.0)
            .map(|sprite| sprite.transform)
        else {
            continue;
        };
        if effect.blur <= 0.0 {
            continue;
        }
        let Some(size) = sprite.custom_size else {
            continue;
        };
        let center =
            (transform.translation.truncate() + Vec2::new(w.width(), w.height()) * 0.5) * sf;
        let half = size.abs() * sf * 0.5;
        scratch.scene.push(BlurRect {
            min_x: center.x - half.x,
            max_x: center.x + half.x,
            min_y: center.y - half.y,
            max_y: center.y + half.y,
            coc: clamp_blur(effect.blur * blur_scale),
            _pad: Vec3::ZERO,
        });
    }
    for (node, transform, strength, hide_content, visibility) in &sources.blur_nodes {
        let alpha = blur_node_alpha(
            hide_content.is_some(),
            behavior.textbox_fade.alpha,
            behavior.timing.hide_alpha,
            behavior.initial_textbox_fade.alpha,
        );
        if alpha < 0.01 {
            continue;
        }
        let strength = strength.map_or(30.0, |strength| strength.0) * alpha;
        let size = node.size(); // physical pixels
        if !visibility.get() || strength <= f32::EPSILON || size.x <= 0.0 || size.y <= 0.0 {
            continue;
        }
        let pos = transform.translation + ui_origin;
        let half = size * 0.5;
        scratch.scene.push(BlurRect {
            min_x: pos.x - half.x,
            max_x: pos.x + half.x,
            min_y: pos.y - half.y,
            max_y: pos.y + half.y,
            coc: clamp_ui_blur(strength, blur_scale),
            _pad: Vec3::ZERO,
        });
    }
    if prepare_regions(&bc, &mut scratch.scene) {
        write_prepared_regions(&mut bc, &mut scratch.scene);
    } else {
        scratch.scene.clear();
    }
}

fn blur_node_alpha(
    follows_textbox_content: bool,
    overlay_alpha: f32,
    hide_alpha: f32,
    initial_alpha: f32,
) -> f32 {
    if follows_textbox_content {
        overlay_alpha * hide_alpha * initial_alpha
    } else {
        overlay_alpha
    }
}

fn clamp_blur(strength: f32) -> f32 {
    strength.clamp(0.0, MAX_BLUR_STRENGTH)
}

fn clamp_ui_blur(strength: f32, viewport_scale: f32) -> f32 {
    clamp_blur(strength * viewport_scale * UI_BLUR_STRENGTH_SCALE)
}

#[cfg(test)]
fn write_regions(camera: &mut BlurCamera, regions: &mut Vec<BlurRect>) {
    merge_regions(regions, camera.rects.len());
    write_prepared_regions(camera, regions);
}

fn write_prepared_regions(camera: &mut BlurCamera, regions: &mut Vec<BlurRect>) {
    camera.count = regions.len() as u32;
    for (target, region) in camera.rects.iter_mut().zip(regions.drain(..)) {
        *target = region;
    }
}

fn prepare_regions(camera: &BlurCamera, regions: &mut Vec<BlurRect>) -> bool {
    merge_regions(regions, camera.rects.len());
    camera.count as usize != regions.len() || camera.rects[..regions.len()] != regions[..]
}

fn merge_regions(regions: &mut Vec<BlurRect>, limit: usize) {
    let mut index = 0;
    while index < regions.len() {
        let mut other = index + 1;
        while other < regions.len() {
            if (regions[index].coc - regions[other].coc).abs() < 0.01
                && regions[index].touches(regions[other])
            {
                regions[index] = regions[index].union(regions[other]);
                regions.swap_remove(other);
            } else {
                other += 1;
            }
        }
        index += 1;
    }

    // Preserve every requested area when pathological UI creates more regions
    // than the uniform can hold. Merge the pair with the least added coverage;
    // this may blur a small gap, but never silently loses a node.
    while regions.len() > limit {
        let mut best = (0, 1, f32::MAX);
        for left in 0..regions.len() {
            for right in left + 1..regions.len() {
                let union = regions[left].union(regions[right]);
                let added = union.area() - regions[left].area() - regions[right].area();
                if added < best.2 {
                    best = (left, right, added);
                }
            }
        }
        let merged = regions[best.0].union(regions[best.1]);
        regions[best.0] = merged;
        regions.swap_remove(best.1);
    }
}

impl BlurRect {
    fn touches(self, other: Self) -> bool {
        self.min_x <= other.max_x + 1.0
            && self.max_x + 1.0 >= other.min_x
            && self.min_y <= other.max_y + 1.0
            && self.max_y + 1.0 >= other.min_y
    }

    fn union(self, other: Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            max_x: self.max_x.max(other.max_x),
            min_y: self.min_y.min(other.min_y),
            max_y: self.max_y.max(other.max_y),
            coc: self.coc.max(other.coc),
            _pad: Vec3::ZERO,
        }
    }

    fn area(self) -> f32 {
        (self.max_x - self.min_x).max(0.0) * (self.max_y - self.min_y).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(min_x: f32, max_x: f32, min_y: f32, max_y: f32, coc: f32) -> BlurRect {
        BlurRect {
            min_x,
            max_x,
            min_y,
            max_y,
            coc,
            _pad: Vec3::ZERO,
        }
    }

    #[test]
    fn textbox_blur_follows_initial_and_hide_fades() {
        assert_eq!(blur_node_alpha(true, 1.0, 1.0, 0.0), 0.0);
        assert_eq!(blur_node_alpha(true, 1.0, 1.0, 0.5), 0.5);
        assert_eq!(blur_node_alpha(true, 0.8, 0.5, 0.5), 0.2);
    }

    #[test]
    fn non_textbox_blur_ignores_textbox_specific_fades() {
        assert_eq!(blur_node_alpha(false, 1.0, 0.0, 0.0), 1.0);
        assert_eq!(blur_node_alpha(false, 0.4, 0.0, 0.0), 0.4);
    }

    #[test]
    fn ui_blur_receives_a_small_global_lift_and_stays_clamped() {
        assert_eq!(clamp_ui_blur(36.0, 1.0), 40.5);
        assert_eq!(clamp_ui_blur(48.0, 1.0), MAX_BLUR_STRENGTH);
    }

    #[test]
    fn merges_touching_regions_with_equal_strength() {
        let mut regions = vec![
            region(0.0, 100.0, 0.0, 100.0, 30.0),
            region(100.0, 180.0, 20.0, 80.0, 30.0),
            region(20.0, 40.0, 20.0, 40.0, 12.0),
        ];
        merge_regions(&mut regions, 16);

        assert_eq!(regions.len(), 2);
        assert!(
            regions.iter().any(|region| {
                region.coc == 30.0 && region.min_x == 0.0 && region.max_x == 180.0
            })
        );
        assert!(regions.iter().any(|region| region.coc == 12.0));
    }

    #[test]
    fn stable_regions_do_not_rewrite_the_camera_uniform() {
        let mut camera = BlurCamera::default();
        let mut initial = vec![region(10.0, 100.0, 20.0, 80.0, 30.0)];
        write_regions(&mut camera, &mut initial);
        let snapshot = camera.clone();

        let mut same = vec![region(10.0, 100.0, 20.0, 80.0, 30.0)];
        assert!(!prepare_regions(&camera, &mut same));

        assert_eq!(camera.count, snapshot.count);
        assert_eq!(camera.rects, snapshot.rects);
    }

    #[test]
    fn capacity_merging_never_drops_requested_coverage() {
        let mut regions = (0..20)
            .map(|index| {
                let x = index as f32 * 20.0;
                region(x, x + 10.0, 0.0, 10.0, index as f32 + 1.0)
            })
            .collect::<Vec<_>>();
        merge_regions(&mut regions, 16);

        assert_eq!(regions.len(), 16);
        for index in 0..20 {
            let x = index as f32 * 20.0 + 5.0;
            assert!(
                regions
                    .iter()
                    .any(|region| x >= region.min_x && x <= region.max_x),
                "region {index} was lost"
            );
        }
    }

    #[test]
    fn horizontal_scissor_includes_kernel_padding() {
        let mut camera = BlurCamera::default();
        let mut regions = vec![region(10.0, 100.0, 20.0, 80.0, 40.0)];
        write_regions(&mut camera, &mut regions);
        let size = Extent3d {
            width: 200,
            height: 100,
            depth_or_array_layers: 1,
        };

        let horizontal = blur_scissor(&camera, size, true);
        let vertical = blur_scissor(&camera, size, false);
        assert_eq!((horizontal.y, horizontal.height), (5, 90));
        assert_eq!((vertical.y, vertical.height), (20, 60));
    }

    #[test]
    fn final_scissor_does_not_escape_fractional_region() {
        let mut camera = BlurCamera::default();
        let mut regions = vec![region(10.25, 100.75, 20.25, 80.75, 40.0)];
        write_regions(&mut camera, &mut regions);
        let size = Extent3d {
            width: 200,
            height: 100,
            depth_or_array_layers: 1,
        };

        let vertical = blur_scissor(&camera, size, false);
        assert_eq!((vertical.x, vertical.y), (11, 21));
        assert_eq!((vertical.width, vertical.height), (89, 59));
    }
}
