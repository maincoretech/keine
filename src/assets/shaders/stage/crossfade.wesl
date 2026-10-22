import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> weights: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var previous: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var previous_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var current: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var current_sampler: sampler;
@fragment fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let old = textureSample(previous, previous_sampler, mesh.uv);
    let incoming = textureSample(current, current_sampler, mesh.uv);
    let old_alpha = old.a * weights.x;
    let new_alpha = incoming.a * weights.y;
    let remaining = old_alpha * (1.0 - new_alpha);
    return vec4<f32>(incoming.rgb * new_alpha + old.rgb * remaining, new_alpha + remaining);
}
