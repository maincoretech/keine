//! A small, deterministic background tint, derived before CPU pixels are released.
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::images::{ImageDimensions, ImageRole, ImageRoleRegistry};
use crate::runtime::resources::{GameConfigResource, GameState};

// Linear sRGB/Rec. 709 luminance, not an average of gamma-encoded channels.
const LUMINANCE: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

#[derive(Clone, Copy)]
pub(crate) struct EnvironmentSample {
    luminance: f32,
    color: Vec3,
}

impl EnvironmentSample {
    fn from_color(color: Vec3) -> Self {
        Self {
            luminance: color.dot(LUMINANCE),
            color,
        }
    }

    fn blend(self, other: Self, progress: f32) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        Self {
            luminance: self.luminance + (other.luminance - self.luminance) * progress,
            color: self.color.lerp(other.color, progress),
        }
    }

    fn neutral() -> Self {
        Self::from_color(Vec3::ONE)
    }
}

pub(super) fn sample(image: &Image) -> Option<EnvironmentSample> {
    let srgb = match image.texture_descriptor.format {
        TextureFormat::Rgba8UnormSrgb => true,
        TextureFormat::Rgba8Unorm => false,
        _ => return None,
    };
    let pixels = image.data.as_ref()?;
    let size = image.size();
    if size.x == 0 || size.y == 0 || pixels.len() != size.x as usize * size.y as usize * 4 {
        return None;
    }
    let mut samples = Vec::with_capacity(256);
    let mut weight = 0.0;
    // At most 256 samples, independent of source dimensions. Transparent pixels
    // cannot introduce their hidden RGB into the environment.
    for y in 0..size.y.min(16) {
        for x in 0..size.x.min(16) {
            let sx = ((x as f32 + 0.5) * size.x as f32 / size.x.min(16) as f32) as u32;
            let sy = ((y as f32 + 0.5) * size.y as f32 / size.y.min(16) as f32) as u32;
            let offset = (sy as usize * size.x as usize + sx as usize) * 4;
            let rgba = &pixels[offset..offset + 4];
            let alpha = rgba[3] as f32 / 255.0;
            let mut rgb = Vec3::new(rgba[0] as f32, rgba[1] as f32, rgba[2] as f32) / 255.0;
            if srgb {
                let color = LinearRgba::from(Srgba::new(rgb.x, rgb.y, rgb.z, 1.0));
                rgb = Vec3::new(color.red, color.green, color.blue);
            }
            if alpha > 0.0 {
                samples.push((rgb.dot(LUMINANCE), rgb, alpha));
                weight += alpha;
            }
        }
    }
    if weight <= f32::EPSILON {
        return None;
    }
    samples.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let percentile = |fraction| {
        let mut cumulative = 0.0;
        samples
            .iter()
            .find_map(|(luminance, _, alpha)| {
                cumulative += alpha;
                (cumulative >= weight * fraction).then_some(*luminance)
            })
            .unwrap_or(samples.last().unwrap().0)
    };
    // Dark materials are not evidence of weak illumination. Use the upper
    // quartile for exposure and the middle-to-bright region for colour, while
    // excluding the brightest outliers (e.g. windows or emissive accents).
    let luminance = percentile(0.75);
    let low = percentile(0.5);
    let high = percentile(0.95);
    let mut color = Vec3::ZERO;
    let mut color_weight = 0.0;
    for (luminance, rgb, alpha) in samples {
        if luminance >= low && luminance <= high {
            color += rgb * alpha;
            color_weight += alpha;
        }
    }
    Some(EnvironmentSample {
        luminance,
        color: color / color_weight,
    })
}

pub(super) fn tint(environment: EnvironmentSample, strength: f32) -> Vec3 {
    let strength = strength.clamp(0.0, 1.0);
    // Establish exposure first; retain a readability floor in unlit scenes.
    // This is an artistic estimate from a finished image, not measured lux.
    // Compress exposure so already shaded portrait art keeps skin/highlights.
    let brightness = 1.0 + strength * (environment.luminance.sqrt().clamp(0.65, 1.0) - 1.0);
    // Divide out luminance before adapting colour. Desaturate toward neutral
    // along a constant-luminance line so colour does not change exposure again.
    let color_luminance = environment.color.dot(LUMINANCE);
    let deviation = if color_luminance > f32::EPSILON {
        environment.color / color_luminance - Vec3::ONE
    } else {
        Vec3::ZERO
    };
    let chroma_limit = (0.25 / deviation.abs().max_element().max(0.5)).min(0.5);
    (Vec3::ONE + deviation * (chroma_limit * strength)) * brightness
}

pub(super) fn environment(
    state: &GameState,
    config: &GameConfigResource,
    server: &AssetServer,
    roles: &ImageRoleRegistry,
    dimensions: &ImageDimensions,
) -> Vec3 {
    if !config.adapter.script.eq_ignore_ascii_case("keine")
        || config.layout.environment_light <= 0.0
    {
        return Vec3::ONE;
    }
    let color_for = |path: String, role| {
        dimensions.environment(&super::images::load(server, roles, path, role))
    };
    let current = state
        .bg
        .as_ref()
        .and_then(|image| color_for(config.bg_path(image), ImageRole::BACKGROUND));
    let mut color = current;
    if let Some(transition) = &state.bg_transition {
        let previous = transition
            .from
            .as_ref()
            .and_then(|image| color_for(config.bg_path(image), ImageRole::BACKGROUND));
        color = Some(previous.unwrap_or_else(EnvironmentSample::neutral).blend(
            current.unwrap_or_else(EnvironmentSample::neutral),
            transition.progress,
        ));
    }
    // Migrated multi-layer scenes can use sprites instead of a background.
    if color.is_none() {
        color = state
            .sprites
            .iter()
            .filter(|(id, sprite)| {
                id.starts_with("scene-layer:")
                    && sprite.transform.alpha > 0.001
                    && sprite.transition_progress > 0.001
            })
            .filter_map(|(id, sprite)| {
                color_for(config.figure_path(&sprite.image), ImageRole::FIGURE).map(|color| {
                    (
                        sprite.z_index,
                        id,
                        color,
                        sprite.transform.alpha * sprite.transition_progress,
                    )
                })
            })
            .min_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)))
            .map(|(_, _, color, alpha)| EnvironmentSample::neutral().blend(color, alpha));
    }
    tint(
        color.unwrap_or_else(EnvironmentSample::neutral),
        config.layout.environment_light,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension};

    #[test]
    fn sampling_ignores_transparent_pixels_and_matches_linear_tint() {
        let image = Image::new(
            Extent3d {
                width: 2,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![255, 0, 0, 0, 128, 128, 128, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD,
        );
        let color = sample(&image).unwrap();
        let linear = LinearRgba::from(Srgba::new(128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0));
        assert!((color.color - Vec3::splat(linear.red)).length() < 0.00001);
        assert!((color.luminance - linear.red).abs() < 0.00001);
        assert_eq!(tint(color, 0.0), Vec3::ONE);
        assert!(tint(EnvironmentSample::from_color(Vec3::ZERO), 0.2).min_element() >= 0.85);
    }

    #[test]
    fn exposure_and_colour_adaptation_are_independent() {
        let warm = EnvironmentSample::from_color(Vec3::new(0.7, 0.3, 0.1));
        let gray = EnvironmentSample::from_color(Vec3::splat(warm.luminance));
        let warm_tint = tint(warm, 0.2);
        let gray_tint = tint(gray, 0.2);
        assert!((warm_tint.dot(LUMINANCE) - gray_tint.dot(LUMINANCE)).abs() < 0.00001);
        assert!(warm_tint.x > warm_tint.y && warm_tint.y > warm_tint.z);
        assert_eq!(gray_tint.x, gray_tint.z);
        let saturated = tint(EnvironmentSample::from_color(Vec3::Z), 1.0);
        assert!(saturated.is_finite() && saturated.min_element() > 0.0);
    }

    #[test]
    fn dark_materials_do_not_dim_an_otherwise_bright_environment() {
        let mut image = Image::new_fill(
            Extent3d {
                width: 4,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[230, 220, 200, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD,
        );
        image.data.as_mut().unwrap()[..8].copy_from_slice(&[10, 10, 10, 255, 20, 20, 20, 255]);
        let environment = sample(&image).unwrap();
        assert!(environment.luminance > 0.7);
        let tint = tint(environment, 1.0);
        assert!(tint.dot(LUMINANCE) > 0.83);
        assert!(tint.x > tint.z);
    }
}
