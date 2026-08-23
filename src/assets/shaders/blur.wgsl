// Adapted from bevy_blur_regions (MIT/Apache-2.0) — separable Gaussian blur with region masking.
// Uses interpolated UV from vertex shader for exact pass-through; binary inside for blur.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> u: BlurUniform;

struct BlurRect {
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
    coc: f32,
    _pad: vec3<f32>,
}
struct BlurUniform { count: u32, _pad: vec3<u32>, rects: array<BlurRect, 16> }

fn region_coc(pos: vec4<f32>, expand_y: bool) -> f32 {
    var coc = 0.0;
    for (var i = 0u; i < u.count; i += 1u) {
        let r = u.rects[i];
        let safe_coc = clamp(r.coc, 0.0, 48.0);
        let sigma = safe_coc * 0.25;
        let padding = select(0.0, ceil(sigma * 1.5), expand_y);
        if (pos.x >= r.min_x && pos.x <= r.max_x &&
            pos.y >= r.min_y - padding && pos.y <= r.max_y + padding) {
            coc = max(coc, safe_coc);
        }
    }
    return coc;
}

fn gaussian_blur(frag_coord: vec4<f32>, coc: f32, frag_offset: vec2<f32>) -> vec4<f32> {
    let sigma = max(coc * 0.25, 0.001);
    let support = i32(ceil(sigma * 1.5));
    let texel_size = 1.0 / vec2<f32>(textureDimensions(src));
    let uv = frag_coord.xy * texel_size;
    let offset = frag_offset * texel_size;
    let exp_factor = -1.0 / (2.0 * sigma * sigma);

    var sum = textureSampleLevel(src, smp, uv, 0.0);
    var weight_sum = 1.0;
    // Incremental Gaussian coefficients (GPU Gems 3, chapter 40): one exp for
    // the whole regularly spaced kernel, followed by cheap quotient updates.
    let first_weight = exp(exp_factor);
    let ratio_step = first_weight * first_weight;
    var sample_weight = first_weight;
    var ratio = first_weight * ratio_step;
    for (var i = 1; i <= support; i += 2) {
        let w0 = sample_weight;
        var w1 = 0.0;
        if (i + 1 <= support) {
            w1 = w0 * ratio;
        }
        let uv_offset = offset * (f32(i) + w1 / (w0 + w1));
        let weight = w0 + w1;
        sum += (textureSampleLevel(src, smp, uv + uv_offset, 0.0) +
                textureSampleLevel(src, smp, uv - uv_offset, 0.0)) * weight;
        weight_sum += weight * 2.0;
        let next_ratio = ratio * ratio_step;
        sample_weight = w1 * next_ratio;
        ratio = next_ratio * ratio_step;
    }
    return sum / weight_sum;
}

@fragment fn horizontal(
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
) -> @location(0) vec4<f32> {
    let coc = region_coc(pos, true);
    if (coc <= 0.0) { return textureSample(src, smp, uv); }
    return gaussian_blur(pos, coc, vec2<f32>(1.0, 0.0));
}

@fragment fn vertical(
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
) -> @location(0) vec4<f32> {
    let coc = region_coc(pos, false);
    // The scissor is the bounding box of every active region. Passing the
    // horizontal intermediate through here would leak its padded rows into
    // gaps between rectangles (most visibly above TITLE's continue preview).
    // Discard keeps the already-loaded original destination pixel untouched.
    if (coc <= 0.0) { discard; }
    return gaussian_blur(pos, coc, vec2<f32>(0.0, 1.0));
}
