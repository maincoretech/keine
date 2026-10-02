#import bevy_sprite::{
    mesh2d_functions as mesh_functions,
    mesh2d_view_bindings::view,
}
#import bevy_sprite::mesh2d_view_bindings::globals

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif
#ifdef SRGB_OUTPUT
#import bevy_render::color_operations::linear_to_srgb
#endif
#ifdef OKLAB_OUTPUT
#import bevy_render::color_operations::linear_rgb_to_oklab
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var particle_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var particle_sampler: sampler;

struct Particle {
    previous: vec4<f32>,
    current: vec4<f32>,
    size_depth: vec4<f32>,
    color: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> particles: array<Particle>;

fn particle_point(center_rotation: vec4<f32>, corner: vec2<f32>) -> vec2<f32> {
    return center_rotation.xy + vec2<f32>(
        corner.x * center_rotation.z - corner.y * center_rotation.w,
        corner.x * center_rotation.w + corner.y * center_rotation.z,
    );
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let interpolation = fract(globals.time * 60.0);
    let particle = particles[u32(vertex.position.x)];
    let corner = vertex.position.yz * particle.size_depth.xy;
    // Interpolate the expanded corners, preserving the previous mesh path's
    // motion and rotation interpolation instead of interpolating angles.
    let local_position = vec3<f32>(mix(
        particle_point(particle.previous, corner),
        particle_point(particle.current, corner),
        interpolation,
    ), particle.size_depth.z);
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let world_position = mesh_functions::mesh2d_position_local_to_world(
        world_from_local,
        vec4<f32>(local_position, 1.0),
    );
    out.position = mesh_functions::mesh2d_position_world_to_clip(world_position);
    out.uv = vertex.uv;
    out.color = particle.color;
    return out;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    var output_color = mesh.color * textureSample(particle_texture, particle_sampler, mesh.uv);
#ifdef TONEMAP_IN_SHADER
    output_color = tonemapping::tone_mapping(output_color, view.color_grading);
#endif
#ifdef SRGB_OUTPUT
    output_color = vec4(linear_to_srgb(output_color.rgb), output_color.a);
#endif
#ifdef OKLAB_OUTPUT
    output_color = vec4(linear_rgb_to_oklab(output_color.rgb), output_color.a);
#endif
    return output_color;
}
