#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import bevy_sprite::mesh2d_view_bindings::globals

struct StageMaskUniform {
    shape_a: vec4<f32>,
    shape_b: vec4<f32>,
    shape_c: vec4<f32>,
    fill_a: vec4<f32>,
    fill_b: vec4<f32>,
    color: vec4<f32>,
    gradient_start: vec4<f32>,
    gradient_end: vec4<f32>,
    effect_a: vec4<f32>,
    effect_b: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: StageMaskUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var mask_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var mask_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var fill_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var fill_sampler: sampler;

fn noise(point: vec2<f32>) -> f32 {
    return fract(sin(dot(point, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn rotate_2d(point: vec2<f32>, angle: f32) -> vec2<f32> {
    let sine = sin(angle);
    let cosine = cos(angle);
    return vec2<f32>(point.x * cosine - point.y * sine, point.x * sine + point.y * cosine);
}

fn fitted_uv(source: vec2<f32>, fit: u32, texture_size: vec2<f32>) -> vec2<f32> {
    if fit == 0u {
        return source;
    }
    let box_size = max(material.shape_b.zw * 2.0, vec2<f32>(1.0));
    let box_aspect = box_size.x / box_size.y;
    let texture_aspect = texture_size.x / max(texture_size.y, 1.0);
    var uv = source;
    if fit == 1u {
        if texture_aspect > box_aspect {
            uv.x = (uv.x - 0.5) * box_aspect / texture_aspect + 0.5;
        } else {
            uv.y = (uv.y - 0.5) * texture_aspect / box_aspect + 0.5;
        }
    } else if texture_aspect > box_aspect {
        uv.y = (uv.y - 0.5) * texture_aspect / box_aspect + 0.5;
    } else {
        uv.x = (uv.x - 0.5) * box_aspect / texture_aspect + 0.5;
    }
    return uv;
}

fn shape_coverage(world_position: vec2<f32>) -> vec3<f32> {
    let local = rotate_2d(world_position - material.shape_b.xy, -material.shape_c.x);
    let half_size = max(material.shape_b.zw, vec2<f32>(0.5));
    let source_uv = local / (half_size * 2.0) + vec2<f32>(0.5);
    let shape = u32(material.shape_a.x + 0.5);
    var coverage = 0.0;
    if shape == 3u {
        var uv = vec2<f32>(source_uv.x, 1.0 - source_uv.y);
        uv = fitted_uv(uv, u32(material.shape_c.w + 0.5), vec2<f32>(textureDimensions(mask_texture)));
        if all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0)) {
            let sample = textureSample(mask_texture, mask_sampler, uv);
            let channel = select(sample.a, dot(sample.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)), material.shape_c.z > 0.5);
            let softness = max(fwidth(channel), material.shape_a.z / max(min(half_size.x, half_size.y), 1.0));
            coverage = smoothstep(0.5 - softness, 0.5 + softness, channel);
        }
    } else {
        var distance_from_shape = 0.0;
        if shape == 2u {
            distance_from_shape = (length(local / half_size) - 1.0) * min(half_size.x, half_size.y);
        } else {
            let radius = select(0.0, min(material.shape_c.y, min(half_size.x, half_size.y)), shape == 1u);
            let delta = abs(local) - (half_size - vec2<f32>(radius));
            distance_from_shape = length(max(delta, vec2<f32>(0.0))) + min(max(delta.x, delta.y), 0.0) - radius;
        }
        let softness = max(material.shape_a.z, fwidth(distance_from_shape));
        coverage = 1.0 - smoothstep(-softness, softness, distance_from_shape);
    }
    if material.shape_a.y > 0.5 {
        coverage = 1.0 - coverage;
    }
    return vec3<f32>(coverage, source_uv);
}

fn adjusted_color(source: vec3<f32>) -> vec3<f32> {
    let cosine = cos(material.effect_b.x);
    let sine = sin(material.effect_b.x);
    let weights = vec3<f32>(0.299, 0.587, 0.114);
    let rotated = source * cosine
        + cross(weights, source) * sine
        + weights * dot(weights, source) * (1.0 - cosine);
    let luminance = dot(rotated, weights);
    return mix(vec3<f32>(luminance), rotated, material.effect_b.y) * material.effect_b.z;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let shape = shape_coverage(mesh.world_position.xy);
    let coverage = shape.x * material.shape_a.w;
    if coverage <= 0.001 {
        discard;
    }
    let local_uv = shape.yz;
    let direction = vec2<f32>(cos(material.fill_a.y), sin(material.fill_a.y));
    let gradient = clamp(dot(local_uv - vec2<f32>(0.5), direction) + 0.5, 0.0, 1.0);
    let fill_mode = u32(material.fill_a.x + 0.5);
    var color = material.color;
    if fill_mode == 1u {
        color = mix(material.gradient_start, material.gradient_end, gradient);
    } else if fill_mode == 2u {
        var uv = (local_uv - vec2<f32>(0.5)) / max(material.fill_b.x, 0.01) + vec2<f32>(0.5);
        uv.y = 1.0 - uv.y;
        uv = fitted_uv(uv, u32(material.fill_a.z + 0.5), vec2<f32>(textureDimensions(fill_texture)));
        var texture_color = textureSample(fill_texture, fill_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
        if material.fill_b.z > 0.25 {
            let texel = vec2<f32>(material.fill_b.z) / vec2<f32>(textureDimensions(fill_texture));
            texture_color = texture_color * 0.4
                + textureSample(fill_texture, fill_sampler, clamp(uv + vec2<f32>(texel.x, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))) * 0.15
                + textureSample(fill_texture, fill_sampler, clamp(uv - vec2<f32>(texel.x, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))) * 0.15
                + textureSample(fill_texture, fill_sampler, clamp(uv + vec2<f32>(0.0, texel.y), vec2<f32>(0.0), vec2<f32>(1.0))) * 0.15
                + textureSample(fill_texture, fill_sampler, clamp(uv - vec2<f32>(0.0, texel.y), vec2<f32>(0.0), vec2<f32>(1.0))) * 0.15;
        }
        let blend = u32(material.fill_a.w + 0.5);
        var blended = texture_color.rgb;
        if blend == 1u {
            blended = color.rgb * texture_color.rgb;
        } else if blend == 2u {
            blended = vec3<f32>(1.0) - (vec3<f32>(1.0) - color.rgb) * (vec3<f32>(1.0) - texture_color.rgb);
        } else if blend == 3u {
            blended = min(vec3<f32>(1.0), color.rgb + texture_color.rgb);
        }
        color = vec4<f32>(mix(color.rgb, blended, material.fill_b.y), mix(color.a, texture_color.a, material.fill_b.y));
    }
    let centered = local_uv - vec2<f32>(0.5);
    let vignette = smoothstep(material.effect_a.y, 0.72, length(centered));
    color = vec4<f32>(color.rgb * (1.0 - vignette * material.effect_a.x), color.a);
    if material.effect_a.z > 0.001 {
        let cell = floor(mesh.world_position.xy / max(material.effect_a.w, 1.0));
        color = vec4<f32>(color.rgb + vec3<f32>((noise(cell + vec2<f32>(floor(globals.time * 30.0))) - 0.5) * material.effect_a.z), color.a);
    }
    color = vec4<f32>(clamp(adjusted_color(color.rgb), vec3<f32>(0.0), vec3<f32>(1.0)), color.a * coverage);
    return color;
}
