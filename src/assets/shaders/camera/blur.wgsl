@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> effects: CameraEffectsUniform;

struct CameraEffectsUniform {
    // radial strength, radial center xy, motion strength
    params_a: vec4<f32>,
    // motion angle, zoom strength, zoom center xy
    params_b: vec4<f32>,
    // chromatic aberration, sharpen, bloom, unused
    params_c: vec4<f32>,
}

fn sample_clamped(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(source, source_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
}

@fragment
fn fragment(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<f32>(1.0) / vec2<f32>(textureDimensions(source));
    let center_strength = max(effects.params_a.x, effects.params_b.y);
    var color = vec4<f32>(0.0);
    if center_strength > 0.001 {
        // Preserve StageMaterial's existing precedence: zoom wins ties and the
        // stronger center blur suppresses motion blur for this frame.
        let center = mix(effects.params_a.yz, effects.params_b.zw, step(effects.params_a.x, effects.params_b.y));
        let direction = uv - center;
        for (var index = 0; index < 6; index += 1) {
            let amount = f32(index) / 5.0 * center_strength * 0.055;
            color += sample_clamped(uv - direction * amount);
        }
        color /= 6.0;
    } else if effects.params_a.w > 0.001 {
        let direction = vec2<f32>(cos(effects.params_b.x), sin(effects.params_b.x)) * effects.params_a.w * 0.018;
        for (var index = 0; index < 5; index += 1) {
            let amount = (f32(index) - 2.0) * 0.5;
            color += sample_clamped(uv + direction * amount);
        }
        color /= 5.0;
    } else {
        color = sample_clamped(uv);
        if effects.params_c.x > 0.001 {
            let split = texel.x * (1.0 + effects.params_c.x * 18.0);
            color.r = sample_clamped(uv + vec2<f32>(split, 0.0)).r;
            color.b = sample_clamped(uv - vec2<f32>(split, 0.0)).b;
        }
    }
    if effects.params_c.y > 0.001 {
        let neighbours = sample_clamped(uv + vec2<f32>(texel.x, 0.0))
            + sample_clamped(uv - vec2<f32>(texel.x, 0.0))
            + sample_clamped(uv + vec2<f32>(0.0, texel.y))
            + sample_clamped(uv - vec2<f32>(0.0, texel.y));
        color = vec4<f32>(
            color.rgb + (color.rgb * 4.0 - neighbours.rgb) * effects.params_c.y * 0.22,
            color.a,
        );
    }
    if effects.params_c.z > 0.001 {
        let radius = texel * (2.0 + effects.params_c.z * 5.0);
        let glow = (
            sample_clamped(uv + radius).rgb
            + sample_clamped(uv - radius).rgb
            + sample_clamped(uv + vec2<f32>(radius.x, -radius.y)).rgb
            + sample_clamped(uv + vec2<f32>(-radius.x, radius.y)).rgb
        ) * 0.25;
        color = vec4<f32>(
            color.rgb + max(glow - vec3<f32>(0.55), vec3<f32>(0.0)) * effects.params_c.z,
            color.a,
        );
    }
    return color;
}
