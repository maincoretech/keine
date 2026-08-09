#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import bevy_sprite::mesh2d_view_bindings::globals

struct StageMaterialUniform {
    tint: vec4<f32>,
    filter_data: vec4<f32>,
    transition_data: vec4<f32>,
    post_a: vec4<f32>,
    post_b: vec4<f32>,
    post_c: vec4<f32>,
    post_d: vec4<f32>,
    post_e: vec4<f32>,
    post_f: vec4<f32>,
    post_g: vec4<f32>,
    post_h: vec4<f32>,
    post_i: vec4<f32>,
    post_j: vec4<f32>,
    post_k: vec4<f32>,
    post_l: vec4<f32>,
    post_m: vec4<f32>,
    post_n: vec4<f32>,
    post_o: vec4<f32>,
    post_p: vec4<f32>,
    post_q: vec4<f32>,
    post_r: vec4<f32>,
    post_s: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: StageMaterialUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var lut_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var lut_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var color_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var color_sampler: sampler;

fn noise(point: vec2<f32>) -> f32 {
    return fract(sin(dot(point, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn distort_uv(uv: vec2<f32>) -> vec2<f32> {
    let dimensions = vec2<f32>(textureDimensions(color_texture));
    let short_side = min(dimensions.x, dimensions.y);
    let centered = (uv * dimensions - dimensions * 0.5) / short_side;
    let distorted = centered * (1.0 + material.post_a.x * dot(centered, centered));
    return (distorted * short_side + dimensions * 0.5) / dimensions;
}

fn animate_uv(source: vec2<f32>) -> vec2<f32> {
    let dimensions = vec2<f32>(textureDimensions(color_texture));
    var uv = source;
    if material.post_g.w > 1.01 {
        let pixels = max(vec2<f32>(1.0), dimensions / material.post_g.w);
        uv = (floor(uv * pixels) + vec2<f32>(0.5)) / pixels;
    }
    if material.post_h.x > 0.001 {
        let frame = floor(globals.time * 18.0);
        let row = floor(uv.y * 42.0);
        let glitch_active = step(0.82 - material.post_h.x * 0.35, noise(vec2<f32>(row, frame)));
        uv.x += (noise(vec2<f32>(row * 5.7, frame * 3.1)) - 0.5) * glitch_active * material.post_h.x * 0.12;
    }
    if material.post_l.z > 0.001 {
        let haze = sin((uv.y * material.post_m.x + globals.time * material.post_l.w) * 6.2831853)
            + sin((uv.y * material.post_m.x * 1.73 - globals.time * material.post_l.w * 0.71) * 6.2831853);
        uv.x += haze * material.post_l.z * 0.004;
    }
    if material.post_m.y > 0.001 {
        let centered = uv - material.post_n.xy;
        let radius = length(centered);
        let ripple = sin(radius * material.post_m.z * 6.2831853 - globals.time * material.post_m.w * 5.0);
        uv += centered / max(radius, 0.0001) * ripple * material.post_m.y * 0.008;
    }
    if material.post_o.y > 0.001 {
        let line = floor(uv.y * dimensions.y * 0.25);
        let frame = floor(globals.time * 24.0);
        let jitter = noise(vec2<f32>(line, frame)) - 0.5;
        uv.x += jitter * material.post_o.z * material.post_o.y * 0.025;
    }
    return uv;
}

fn sample_blurred(uv: vec2<f32>) -> vec4<f32> {
    let blur = max(0.0, material.filter_data.x + material.post_a.w);
    var color = textureSample(color_texture, color_sampler, uv);
    if blur > 0.25 {
        let dimensions = vec2<f32>(textureDimensions(color_texture));
        let step_uv = vec2<f32>(blur) / dimensions;
        color = color * 0.20
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>( 0.00, -0.65)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>( 0.54, -0.35)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>(-0.54,  0.35)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>( 0.25,  0.60)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>(-0.25, -0.60)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>( 0.72,  0.18)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>(-0.72, -0.18)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>( 0.05,  0.90)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>(-0.05, -0.90)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>( 1.15, -0.45)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>(-1.15,  0.45)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>( 0.48, -1.12)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>(-0.48,  1.12)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>( 0.95,  0.78)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>(-0.95, -0.78)) * 0.05
            + textureSample(color_texture, color_sampler, uv + step_uv * vec2<f32>(-0.82,  0.95)) * 0.05;
    }
    return color;
}

fn sample_camera_effects(uv: vec2<f32>) -> vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(color_texture));
    let texel = vec2<f32>(1.0) / dimensions;
    var color = sample_blurred(uv);

    if material.post_g.z > 0.001 {
        let split = texel.x * (1.0 + material.post_g.z * 18.0);
        color.r = sample_blurred(clamp(uv + vec2<f32>(split, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))).r;
        color.b = sample_blurred(clamp(uv - vec2<f32>(split, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))).b;
    }
    if material.post_i.z > 0.001 {
        let direction = vec2<f32>(cos(material.post_i.w), sin(material.post_i.w)) * material.post_i.z * 0.018;
        var motion = vec4<f32>(0.0);
        for (var index = 0; index < 5; index += 1) {
            let amount = (f32(index) - 2.0) * 0.5;
            motion += sample_blurred(clamp(uv + direction * amount, vec2<f32>(0.0), vec2<f32>(1.0)));
        }
        color = motion / 5.0;
    }
    let center_blur = max(material.post_h.w, material.post_j.x);
    if center_blur > 0.001 {
        let center = mix(material.post_i.xy, material.post_j.yz, step(material.post_h.w, material.post_j.x));
        let direction = uv - center;
        var radial = vec4<f32>(0.0);
        for (var index = 0; index < 6; index += 1) {
            let amount = f32(index) / 5.0 * center_blur * 0.055;
            radial += sample_blurred(clamp(uv - direction * amount, vec2<f32>(0.0), vec2<f32>(1.0)));
        }
        color = radial / 6.0;
    }
    if material.post_h.z > 0.001 {
        let neighbours = sample_blurred(clamp(uv + vec2<f32>(texel.x, 0.0), vec2<f32>(0.0), vec2<f32>(1.0)))
            + sample_blurred(clamp(uv - vec2<f32>(texel.x, 0.0), vec2<f32>(0.0), vec2<f32>(1.0)))
            + sample_blurred(clamp(uv + vec2<f32>(0.0, texel.y), vec2<f32>(0.0), vec2<f32>(1.0)))
            + sample_blurred(clamp(uv - vec2<f32>(0.0, texel.y), vec2<f32>(0.0), vec2<f32>(1.0)));
        color = vec4<f32>(color.rgb + (color.rgb * 4.0 - neighbours.rgb) * material.post_h.z * 0.22, color.a);
    }
    if material.post_g.y > 0.001 {
        let radius = texel * (2.0 + material.post_g.y * 5.0);
        let glow = (sample_blurred(clamp(uv + radius, vec2<f32>(0.0), vec2<f32>(1.0))).rgb
            + sample_blurred(clamp(uv - radius, vec2<f32>(0.0), vec2<f32>(1.0))).rgb
            + sample_blurred(clamp(uv + vec2<f32>(radius.x, -radius.y), vec2<f32>(0.0), vec2<f32>(1.0))).rgb
            + sample_blurred(clamp(uv + vec2<f32>(-radius.x, radius.y), vec2<f32>(0.0), vec2<f32>(1.0))).rgb) * 0.25;
        color = vec4<f32>(color.rgb + max(glow - vec3<f32>(0.55), vec3<f32>(0.0)) * material.post_g.y, color.a);
    }
    return color;
}

fn apply_basic_filter(color: vec4<f32>) -> vec4<f32> {
    let luminance = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    var rgb = mix(vec3<f32>(luminance), color.rgb, material.filter_data.w);
    rgb = (rgb - vec3<f32>(0.5)) * material.filter_data.z + vec3<f32>(0.5);
    rgb *= material.filter_data.y;
    return vec4<f32>(rgb, color.a);
}

fn lookup_lut(color: vec3<f32>) -> vec3<f32> {
    let blue_index = clamp(color.b, 0.0, 1.0) * 15.0;
    let low = floor(blue_index);
    let high = ceil(blue_index);
    let fraction = blue_index - low;
    let low_uv = vec2<f32>(
        (low * 16.0 + clamp(color.r, 0.0, 1.0) * 15.0 + 0.5) / 256.0,
        (clamp(color.g, 0.0, 1.0) * 15.0 + 0.5) / 16.0,
    );
    let high_uv = vec2<f32>(
        (high * 16.0 + clamp(color.r, 0.0, 1.0) * 15.0 + 0.5) / 256.0,
        low_uv.y,
    );
    return mix(
        textureSample(lut_texture, lut_sampler, low_uv).rgb,
        textureSample(lut_texture, lut_sampler, high_uv).rgb,
        fraction,
    );
}

fn godray(uv: vec2<f32>) -> f32 {
    if material.post_c.x <= 0.001 {
        return 0.0;
    }
    let angle = material.post_c.z;
    let direction = vec2<f32>(cos(angle), sin(angle));
    let center = vec2<f32>(material.post_d.w, material.post_e.x);
    let point = uv - center;
    let axis = select(normalize(point + vec2<f32>(0.0001)), direction, material.post_d.z > 0.5);
    let across = dot(point, vec2<f32>(-axis.y, axis.x));
    let along = max(0.0, dot(point, axis));
    let density = material.post_d.y;
    let phase = globals.time * material.post_c.w;
    var layers = 0.0;
    var frequency = 9.0 * density;
    var amplitude = 1.0;
    for (var octave = 0; octave < 3; octave += 1) {
        let wave = 0.5 + 0.5 * sin(across * frequency + phase * (1.0 + f32(octave) * 0.37));
        layers += pow(wave, 3.5) * amplitude;
        frequency *= 1.73;
        amplitude *= mix(0.28, 0.62, material.post_d.x);
    }
    let source_falloff = select(exp(-along * 1.25), 1.0, material.post_d.z > 0.5);
    let edge_fade = smoothstep(0.0, 0.12, uv.y) * smoothstep(0.0, 0.12, 1.0 - uv.y);
    return layers * source_falloff * edge_fade * material.post_c.x * 0.22;
}

fn apply_post(color: vec4<f32>, uv: vec2<f32>) -> vec4<f32> {
    var result = color;
    result = vec4<f32>(result.rgb * exp2(material.post_f.x) + vec3<f32>(material.post_f.y), result.a);
    let color_luma = dot(result.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    result = vec4<f32>(mix(vec3<f32>(color_luma), result.rgb, material.post_f.w), result.a);
    result = vec4<f32>((result.rgb - vec3<f32>(0.5)) * (1.0 + material.post_f.z) + vec3<f32>(0.5), result.a);
    result = vec4<f32>(result.rgb + vec3<f32>(material.post_g.x * 0.10, material.post_g.x * 0.025, -material.post_g.x * 0.10), result.a);
    let tone = u32(material.post_b.x + 0.5);
    let tone_strength = material.post_b.y;
    if tone == 1u && tone_strength > 0.001 {
        let grey = dot(result.rgb, vec3<f32>(0.299, 0.587, 0.114));
        result = vec4<f32>(mix(result.rgb, vec3<f32>(grey), tone_strength * 0.7), result.a);
    } else if tone == 2u && tone_strength > 0.001 {
        let sepia = vec3<f32>(
            dot(result.rgb, vec3<f32>(0.393, 0.769, 0.189)),
            dot(result.rgb, vec3<f32>(0.349, 0.686, 0.168)),
            dot(result.rgb, vec3<f32>(0.272, 0.534, 0.131)),
        );
        result = vec4<f32>(mix(result.rgb, sepia, tone_strength), result.a);
    }
    if material.post_c.y > 0.001 {
        result = vec4<f32>(mix(result.rgb, lookup_lut(result.rgb), material.post_c.y), result.a);
    }
    if material.post_b.z > 0.001 {
        let grain = noise(vec2<f32>(floor(uv.x * 960.0), floor(uv.y * 540.0) + floor(globals.time * 24.0)));
        let scratch = step(0.996 - material.post_b.z * 0.003, noise(vec2<f32>(floor(uv.x * 420.0), floor(globals.time * 12.0))));
        let grained = result.rgb * (1.0 + (grain - 0.5) * material.post_b.z * 0.4);
        result = vec4<f32>(mix(grained, vec3<f32>(0.82), scratch * material.post_b.z * 0.6), result.a);
    }
    if material.post_l.x > 0.001 {
        let grain_size = max(1.0, material.post_l.y);
        let grain = noise(
            floor(uv * vec2<f32>(960.0, 540.0) / grain_size)
                + vec2<f32>(floor(globals.time * 30.0)),
        );
        result = vec4<f32>(result.rgb + vec3<f32>((grain - 0.5) * material.post_l.x * 0.24), result.a);
    }
    if material.post_j.w > 0.001 {
        let direction = vec2<f32>(cos(material.post_k.x), sin(material.post_k.x));
        let band = exp(-pow(dot(uv - vec2<f32>(0.5), vec2<f32>(-direction.y, direction.x)) * 3.2, 2.0));
        let drift = 0.7 + 0.3 * sin(dot(uv, direction) * 7.0 + globals.time * 0.35);
        result = vec4<f32>(result.rgb + vec3<f32>(1.0, 0.32, 0.08) * band * drift * material.post_j.w * 0.38, result.a);
    }
    if material.post_k.y > 0.001 {
        let delta = uv - material.post_k.zw;
        let flare = exp(-length(delta) * 10.0) + exp(-length(uv - (vec2<f32>(1.0) - material.post_k.zw)) * 18.0) * 0.45;
        result = vec4<f32>(result.rgb + vec3<f32>(1.0, 0.78, 0.44) * flare * material.post_k.y, result.a);
    }
    if material.post_n.z > 0.001 {
        let fog_uv = uv * material.post_o.x;
        let fog = noise(floor(fog_uv * 5.0) + vec2<f32>(globals.time * material.post_n.w, 0.0)) * 0.45
            + noise(floor(fog_uv * 11.0) - vec2<f32>(0.0, globals.time * material.post_n.w * 0.7)) * 0.25;
        result = vec4<f32>(mix(result.rgb, vec3<f32>(0.72, 0.76, 0.80), fog * material.post_n.z), result.a);
    }
    if material.post_o.y > 0.001 {
        let scanline = 0.96 + 0.04 * sin(uv.y * 1080.0 * 3.14159265);
        let tape_noise = (noise(vec2<f32>(floor(uv.y * 540.0), floor(globals.time * 30.0))) - 0.5) * material.post_o.w;
        result = vec4<f32>(
            mix(result.rgb, result.rgb * scanline + vec3<f32>(tape_noise), material.post_o.y),
            result.a,
        );
    }
    if material.post_h.y > 0.001 {
        let centered = uv - vec2<f32>(0.5);
        let cell = fract(uv * vec2<f32>(640.0, 360.0)) - vec2<f32>(0.5);
        let mask = 0.72 + 0.28 * (1.0 - smoothstep(0.18, 0.52, length(cell)));
        let scanline = 0.92 + 0.08 * sin(uv.y * 720.0 * 3.14159265);
        let curved = 1.0 - dot(centered, centered) * 0.22;
        result = vec4<f32>(mix(result.rgb, result.rgb * mask * scanline * curved, material.post_h.y), result.a);
    }
    if material.post_p.x > 0.001 {
        let direction = vec2<f32>(cos(material.post_p.z), sin(material.post_p.z));
        let rotated = vec2<f32>(dot(uv, direction), dot(uv, vec2<f32>(-direction.y, direction.x)));
        let cell = fract(rotated * material.post_p.y * 12.0) - vec2<f32>(0.5);
        let dots = 1.0 - smoothstep(0.12, 0.55, length(cell));
        let luma = dot(result.rgb, vec3<f32>(0.299, 0.587, 0.114));
        result = vec4<f32>(mix(result.rgb, vec3<f32>(luma * (0.62 + dots * 0.38)), material.post_p.x), result.a);
    }
    if material.post_p.w > 0.001 {
        let levels = max(2.0, material.post_q.x);
        let threshold = (noise(floor(uv * vec2<f32>(1920.0, 1080.0))) - 0.5) / levels;
        let quantized = floor(clamp(result.rgb + threshold, vec3<f32>(0.0), vec3<f32>(1.0)) * levels) / levels;
        result = vec4<f32>(mix(result.rgb, quantized, material.post_p.w), result.a);
    }
    if material.post_q.y > 0.001 {
        let texel = vec2<f32>(material.post_q.z) / vec2<f32>(textureDimensions(color_texture));
        let left = textureSample(color_texture, color_sampler, clamp(uv - vec2<f32>(texel.x, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))).rgb;
        let right = textureSample(color_texture, color_sampler, clamp(uv + vec2<f32>(texel.x, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))).rgb;
        let up = textureSample(color_texture, color_sampler, clamp(uv - vec2<f32>(0.0, texel.y), vec2<f32>(0.0), vec2<f32>(1.0))).rgb;
        let down = textureSample(color_texture, color_sampler, clamp(uv + vec2<f32>(0.0, texel.y), vec2<f32>(0.0), vec2<f32>(1.0))).rgb;
        let edge = clamp(length(left - right) + length(up - down), 0.0, 1.0);
        result = vec4<f32>(mix(result.rgb, vec3<f32>(edge), material.post_q.y), result.a);
    }
    let ray = godray(uv);
    result = vec4<f32>(result.rgb + vec3<f32>(1.0, 0.88, 0.62) * ray, result.a);
    if material.post_a.y > 0.001 {
        let dimensions = vec2<f32>(textureDimensions(color_texture));
        let centered = (uv * dimensions - dimensions * 0.5) / min(dimensions.x, dimensions.y);
        let inner = smoothstep(material.post_a.z, max(0.0, material.post_a.z - 0.2), length(centered));
        result = vec4<f32>(result.rgb * mix(1.0 - material.post_a.y, 1.0, inner), result.a);
    }
    if material.post_q.w < 0.999 {
        let centered = uv - vec2<f32>(material.post_r.w, material.post_s.x);
        let horizontal = abs(centered.x) / max(material.post_r.x, 0.001);
        let opening = material.post_q.w * 0.5 - horizontal * horizontal * material.post_r.y * 0.5;
        let visible = 1.0 - smoothstep(opening - material.post_r.z, opening + material.post_r.z, abs(centered.y));
        result = vec4<f32>(result.rgb * visible, result.a);
    }
    return result;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
#ifdef STAGE_COMPLEX
    let kind = u32(material.transition_data.x + 0.5);
    let progress = clamp(material.transition_data.y, 0.0, 1.0);
    if kind == 1u && mesh.uv.x > progress {
        discard;
    }
    let effects = u32(material.transition_data.z + 0.5);
    let animation_progress = clamp(material.transition_data.w, 0.0, 1.0);
    var uv = animate_uv(distort_uv(mesh.uv));
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        discard;
    }
    let shockwave_in = (effects & 64u) != 0u;
    let shockwave_out = (effects & 128u) != 0u;
    if shockwave_in || shockwave_out {
        let centered = uv - vec2<f32>(0.5);
        let radius = length(centered);
        let direction = centered / max(radius, 0.0001);
        let wave_radius = select(
            (1.0 - animation_progress) * 0.72,
            animation_progress * 0.72,
            shockwave_out,
        );
        let ring = exp(-pow((radius - wave_radius) * 34.0, 2.0));
        let envelope = sin(animation_progress * 3.14159265);
        let polarity = select(-1.0, 1.0, shockwave_out);
        uv += direction * ring * envelope * polarity * 0.045;
    }
    if (effects & 8u) != 0u {
        let frame = floor(globals.time * 30.0);
        let row = floor(uv.y * 56.0);
        let band_seed = noise(vec2<f32>(row * 3.17, frame * 7.31));
        let band = step(0.62, band_seed);
        let shift_seed = noise(vec2<f32>(row * 11.9 + frame, frame * 19.7));
        let shift = (shift_seed - 0.5) * band * (0.08 + band_seed * 0.12);
        uv = vec2<f32>(fract(uv.x + shift), uv.y);
    }
    var color = apply_basic_filter(sample_camera_effects(uv)) * material.tint;
    if material.post_b.w > 0.001 {
        let direction = uv - vec2<f32>(0.5);
        var zoom = vec4<f32>(0.0);
        for (var index = 0; index < 6; index += 1) {
            let amount = f32(index) / 5.0 * material.post_b.w * 0.05;
            zoom += textureSample(color_texture, color_sampler, clamp(uv - direction * amount, vec2<f32>(0.0), vec2<f32>(1.0)));
        }
        zoom /= 6.0;
        let split = material.post_b.w * 0.01;
        let red = textureSample(color_texture, color_sampler, clamp(uv + vec2<f32>(split, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))).r;
        let blue = textureSample(color_texture, color_sampler, clamp(uv - vec2<f32>(split, 0.0), vec2<f32>(0.0), vec2<f32>(1.0))).b;
        color = vec4<f32>(mix(color.rgb, vec3<f32>(red, zoom.g, blue), material.post_b.w), color.a);
    }
    color = apply_post(color, uv);
    if kind == 2u {
        let fine = noise(floor(mesh.uv * vec2<f32>(960.0, 540.0)));
        let coarse = noise(floor(mesh.uv * vec2<f32>(480.0, 270.0)));
        let threshold = fine * 0.72 + coarse * 0.28;
        color = vec4<f32>(
            color.rgb,
            color.a * smoothstep(threshold - 0.045, threshold + 0.045, progress),
        );
    }
    if (effects & 1u) != 0u {
        let grey = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
        let frame = floor(globals.time * 20.0);
        let grain_cell = floor(uv * vec2<f32>(360.0, 203.0));
        let grain = noise(grain_cell + vec2<f32>(frame * 37.0, frame * 17.0));
        let flicker_seed = noise(vec2<f32>(frame * 5.3, 19.0));
        let flicker = 0.91 + flicker_seed * 0.17;
        let fine_line = step(
            0.992,
            noise(vec2<f32>(floor(uv.y * 320.0) * 13.0, frame * 3.7)),
        );
        let scratch = step(
            0.996,
            noise(vec2<f32>(floor(uv.x * 420.0) * 7.0, frame * 11.3)),
        );
        let flash = step(0.965, noise(vec2<f32>(frame * 2.1, 71.0))) * 0.07;
        let sepia = vec3<f32>(grey * 1.12, grey * 1.01, grey * 0.76)
            * (flicker + flash) * (0.82 + grain * 0.28);
        let scratched = mix(
            sepia,
            vec3<f32>(0.92, 0.86, 0.72),
            max(fine_line * 0.32, scratch * 0.24),
        );
        color = vec4<f32>(mix(color.rgb, scratched, 0.86), color.a);
    }
    if (effects & 2u) != 0u {
        let centered = uv - vec2<f32>(0.5);
        let lens = dot(centered, centered);
        let crt_uv = uv + centered * lens * 0.16;
        let cell = fract(crt_uv * vec2<f32>(58.0, 32.625)) - vec2<f32>(0.5);
        let dot_mask = 1.0 - smoothstep(0.16, 0.50, length(cell));
        let scanline = 0.91 + 0.09 * sin(crt_uv.y * 720.0 * 3.14159265);
        let center_lift = 1.12 - lens * 0.42;
        color = vec4<f32>(
            color.rgb * (0.62 + dot_mask * 0.38) * scanline * center_lift,
            color.a,
        );
    }
    if (effects & 4u) != 0u {
        let sweep = fract(globals.time * 0.18);
        let sheen = pow(max(0.0, 1.0 - abs(uv.x - sweep) * 6.0), 3.0);
        color = vec4<f32>(
            color.rgb + vec3<f32>(0.86, 0.93, 1.0) * sheen * 0.34,
            color.a,
        );
    }
    if (effects & 16u) != 0u {
        let offset = vec2<f32>(0.007 + 0.002 * sin(globals.time * 18.0), 0.0);
        let red = sample_blurred(clamp(uv + offset, vec2<f32>(0.0), vec2<f32>(1.0))).r;
        let blue = sample_blurred(clamp(uv - offset, vec2<f32>(0.0), vec2<f32>(1.0))).b;
        color = vec4<f32>(red, color.g, blue, color.a);
    }
    if (effects & 32u) != 0u {
        let source = -0.10 + fract(globals.time * 0.018) * 1.20;
        let ray = pow(max(0.0, 1.0 - abs(uv.x - source) * 2.2), 3.0)
            * (1.0 - uv.y)
            * (0.84 + 0.16 * sin(uv.y * 42.0 + globals.time * 0.7));
        color = vec4<f32>(
            color.rgb + vec3<f32>(1.0, 0.87, 0.61) * ray * 0.30,
            color.a,
        );
    }
#ifdef BLEND_MULTIPLY
    // Pixi/WebGAL's multiply is perceived in display space. Bevy samples the
    // sRGB texture into linear space, so feeding raw linear RGB to the fixed
    // blend factors makes the result visibly too dark. Restore perceptual RGB
    // before premultiplying while keeping the standard source-over coverage.
    let perceptual = pow(max(color.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    color = vec4<f32>(perceptual * color.a, color.a);
#endif
#ifdef BLEND_SCREEN
    color = vec4<f32>(color.rgb * color.a, color.a);
#endif
    return color;
#else
    var color = textureSample(color_texture, color_sampler, mesh.uv) * material.tint;
#ifdef BLEND_MULTIPLY
    let perceptual = pow(max(color.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    color = vec4<f32>(perceptual * color.a, color.a);
#endif
#ifdef BLEND_SCREEN
    color = vec4<f32>(color.rgb * color.a, color.a);
#endif
    return color;
#endif
}
