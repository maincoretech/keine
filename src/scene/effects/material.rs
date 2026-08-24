use bevy::asset::{AssetPath, embedded_asset, embedded_path};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor,
    SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};
use keine_core::{BlendMode, CameraTargets, ColorToneMode, PostProcessEffect, VisualFilter};

pub(crate) struct StageMaterialPlugin;

impl Plugin for StageMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "../../assets/shaders/stage_material.wgsl");
        app.add_plugins(Material2dPlugin::<StageMaterial>::default())
            .add_systems(Startup, setup_quad);
    }
}

#[derive(Resource, Clone)]
pub(crate) struct StageQuad(pub(crate) Handle<Mesh>);

fn setup_quad(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(StageQuad(meshes.add(Rectangle::new(1.0, 1.0))));
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, PartialEq)]
#[bind_group_data(StageMaterialKey)]
pub(crate) struct StageMaterial {
    // Keep all scalar parameters in one uniform buffer. Separate bindings exceed
    // Metal's vertex-stage buffer limit once the complete post-process set is active.
    #[uniform(0)]
    pub(crate) tint: Vec4,
    #[uniform(0)]
    pub(crate) filter: Vec4,
    #[uniform(0)]
    pub(crate) transition: Vec4,
    #[uniform(0)]
    pub(crate) post_a: Vec4,
    #[uniform(0)]
    pub(crate) post_b: Vec4,
    #[uniform(0)]
    pub(crate) post_c: Vec4,
    #[uniform(0)]
    pub(crate) post_d: Vec4,
    #[uniform(0)]
    pub(crate) post_e: Vec4,
    #[uniform(0)]
    pub(crate) post_f: Vec4,
    #[uniform(0)]
    pub(crate) post_g: Vec4,
    #[uniform(0)]
    pub(crate) post_h: Vec4,
    #[uniform(0)]
    pub(crate) post_i: Vec4,
    #[uniform(0)]
    pub(crate) post_j: Vec4,
    #[uniform(0)]
    pub(crate) post_k: Vec4,
    #[uniform(0)]
    pub(crate) post_l: Vec4,
    #[uniform(0)]
    pub(crate) post_m: Vec4,
    #[uniform(0)]
    pub(crate) post_n: Vec4,
    #[uniform(0)]
    pub(crate) post_o: Vec4,
    #[uniform(0)]
    pub(crate) post_p: Vec4,
    #[uniform(0)]
    pub(crate) post_q: Vec4,
    #[uniform(0)]
    pub(crate) post_r: Vec4,
    #[uniform(0)]
    pub(crate) post_s: Vec4,
    #[texture(1, visibility(fragment))]
    #[sampler(2, visibility(fragment))]
    pub(crate) lut: Option<Handle<Image>>,
    #[texture(3, visibility(fragment))]
    #[sampler(4, visibility(fragment))]
    pub(crate) image: Handle<Image>,
    pub(crate) blend: BlendMode,
}

/// Reuses an entity's material handle and only marks the asset changed when
/// its GPU-visible data actually differs. Stage animation revisions may wake
/// both stage layers even when a composited camera effect was stripped from
/// their materials; writing the identical asset would still force Bevy to
/// prepare and upload it again.
pub(crate) fn upsert_stage_material(
    existing: Option<&MeshMaterial2d<StageMaterial>>,
    materials: &mut Assets<StageMaterial>,
    material: StageMaterial,
) -> Handle<StageMaterial> {
    let Some(existing) = existing else {
        return materials.add(material);
    };
    if materials
        .get(&existing.0)
        .is_some_and(|current| *current != material)
        && let Some(mut current) = materials.get_mut(&existing.0)
    {
        *current = material;
    }
    existing.0.clone()
}

impl StageMaterial {
    pub(crate) fn new(
        image: Handle<Image>,
        alpha: f32,
        filter: VisualFilter,
        blend: BlendMode,
        transition: Vec4,
        post: &PostProcessEffect,
        lut: Option<Handle<Image>>,
    ) -> Self {
        let color_tone = match post.color_tone {
            ColorToneMode::None => 0.0,
            ColorToneMode::Grayscale => 1.0,
            ColorToneMode::Sepia => 2.0,
        };
        Self {
            tint: Vec4::new(1.0, 1.0, 1.0, alpha.clamp(0.0, 1.0)),
            filter: Vec4::new(
                filter.blur.max(0.0),
                filter.brightness.clamp(0.0, 4.0),
                filter.contrast.clamp(0.0, 4.0),
                filter.saturation.clamp(0.0, 4.0),
            ),
            transition,
            post_a: Vec4::new(
                post.distortion_strength.clamp(-1.0, 1.0),
                post.vignette_intensity.clamp(0.0, 1.0),
                post.vignette_size.clamp(0.0, 1.0),
                post.blur_amount.clamp(0.0, 20.0),
            ),
            post_b: Vec4::new(
                color_tone,
                post.color_tone_intensity.clamp(0.0, 1.0),
                post.old_film_intensity.clamp(0.0, 1.0),
                post.shock_intensity.clamp(0.0, 1.0),
            ),
            post_c: Vec4::new(
                post.godray_intensity.clamp(0.0, 1.0),
                if lut.is_some() {
                    post.lut_intensity.clamp(0.0, 1.0)
                } else {
                    0.0
                },
                post.godray_angle.to_radians(),
                post.godray_speed.clamp(-3.0, 3.0),
            ),
            post_d: Vec4::new(
                post.godray_gain.clamp(0.0, 1.0),
                post.godray_lacunarity.clamp(1.0, 5.0),
                f32::from(post.godray_parallel),
                post.godray_center_x.clamp(0.0, 1.0),
            ),
            post_e: Vec4::new(post.godray_center_y.clamp(0.0, 1.0), 0.0, 0.0, 0.0),
            post_f: Vec4::new(
                post.color_exposure.clamp(-2.0, 2.0),
                post.color_brightness.clamp(-1.0, 1.0),
                post.color_contrast.clamp(-1.0, 1.0),
                post.color_saturation.clamp(0.0, 2.0),
            ),
            post_g: Vec4::new(
                post.color_temperature.clamp(-1.0, 1.0),
                post.bloom_intensity.clamp(0.0, 2.0),
                post.chromatic_aberration.clamp(0.0, 1.0),
                post.pixelate_size.clamp(1.0, 128.0),
            ),
            post_h: Vec4::new(
                post.glitch_intensity.clamp(0.0, 1.0),
                post.crt_intensity.clamp(0.0, 1.0),
                post.sharpen_strength.clamp(0.0, 2.0),
                post.radial_blur_strength.clamp(0.0, 1.0),
            ),
            post_i: Vec4::new(
                post.radial_blur_center_x.clamp(0.0, 1.0),
                post.radial_blur_center_y.clamp(0.0, 1.0),
                post.motion_blur_strength.clamp(0.0, 1.0),
                post.motion_blur_angle.to_radians(),
            ),
            post_j: Vec4::new(
                post.zoom_blur_strength.clamp(0.0, 1.0),
                post.zoom_blur_center_x.clamp(0.0, 1.0),
                post.zoom_blur_center_y.clamp(0.0, 1.0),
                post.light_leak_intensity.clamp(0.0, 1.0),
            ),
            post_k: Vec4::new(
                post.light_leak_angle.to_radians(),
                post.lens_flare_intensity.clamp(0.0, 1.0),
                post.lens_flare_center_x.clamp(0.0, 1.0),
                post.lens_flare_center_y.clamp(0.0, 1.0),
            ),
            post_l: Vec4::new(
                post.film_grain_intensity.clamp(0.0, 1.0),
                post.film_grain_size.clamp(0.25, 16.0),
                post.heat_haze_intensity.clamp(0.0, 1.0),
                post.heat_haze_speed.clamp(-8.0, 8.0),
            ),
            post_m: Vec4::new(
                post.heat_haze_scale.clamp(0.25, 32.0),
                post.water_ripple_intensity.clamp(0.0, 1.0),
                post.water_ripple_frequency.clamp(0.1, 64.0),
                post.water_ripple_speed.clamp(-8.0, 8.0),
            ),
            post_n: Vec4::new(
                post.water_ripple_center_x.clamp(0.0, 1.0),
                post.water_ripple_center_y.clamp(0.0, 1.0),
                post.fog_intensity.clamp(0.0, 1.0),
                post.fog_speed.clamp(-4.0, 4.0),
            ),
            post_o: Vec4::new(
                post.fog_scale.clamp(0.25, 32.0),
                post.vhs_intensity.clamp(0.0, 1.0),
                post.vhs_jitter.clamp(0.0, 1.0),
                post.vhs_noise.clamp(0.0, 1.0),
            ),
            post_p: Vec4::new(
                post.halftone_intensity.clamp(0.0, 1.0),
                post.halftone_scale.clamp(1.0, 64.0),
                post.halftone_angle.to_radians(),
                post.dither_intensity.clamp(0.0, 1.0),
            ),
            post_q: Vec4::new(
                post.dither_levels.clamp(2.0, 32.0),
                post.outline_intensity.clamp(0.0, 1.0),
                post.outline_thickness.clamp(0.25, 8.0),
                post.eyelid_openness.clamp(0.0, 1.0),
            ),
            post_r: Vec4::new(
                post.eyelid_width.clamp(0.1, 2.0),
                post.eyelid_curvature.clamp(0.0, 2.0),
                post.eyelid_softness.clamp(0.001, 0.25),
                post.eyelid_center_x.clamp(0.0, 1.0),
            ),
            post_s: Vec4::new(post.eyelid_center_y.clamp(0.0, 1.0), 0.0, 0.0, 0.0),
            lut,
            image,
            blend,
        }
    }
}

pub(crate) fn effective_post_process(
    effect: &PostProcessEffect,
    targets: CameraTargets,
    group: &str,
    distance: Option<f32>,
) -> PostProcessEffect {
    let targeted = if group == "scene" {
        targets.scene()
    } else {
        targets.characters()
    };
    if !targeted {
        return PostProcessEffect::default();
    }
    let mut effect = effect.clone();
    if let (Some(distance), Some(focal_distance)) = (distance, effect.focal_distance) {
        effect.blur_amount = (effect.blur_amount
            + (distance - focal_distance).abs() * effect.blur_strength.max(0.0) * 6.0)
            .min(20.0);
    }
    crate::render::camera_blur::strip_composited_effects(&mut effect, targets);
    effect
}

/// Returns a LUT only when the authored effect can visibly use it. Some source
/// formats keep a preset name while leaving intensity empty;
/// loading that inactive preset would produce a false missing-asset error.
pub(crate) fn active_lut_preset(effect: &PostProcessEffect) -> Option<&str> {
    effect
        .lut_preset
        .as_deref()
        .filter(|_| effect.lut_intensity > 0.001)
}

pub(crate) fn animation_uniform(
    films: keine_core::FilmEffects,
    animation: Option<&keine_core::state::PresetAnimation>,
) -> Vec4 {
    use keine_core::AnimationPreset;
    const SHOCKWAVE_IN: u8 = 1 << 6;
    const SHOCKWAVE_OUT: u8 = 1 << 7;

    let mut effects = films.bits();
    let progress = animation.map_or(0.0, |animation| {
        effects |= match animation.preset {
            AnimationPreset::ShockwaveIn => SHOCKWAVE_IN,
            AnimationPreset::ShockwaveOut => SHOCKWAVE_OUT,
            _ => 0,
        };
        (animation.elapsed / animation.duration).clamp(0.0, 1.0)
    });
    Vec4::new(0.0, 0.0, f32::from(effects), progress)
}

#[cfg(test)]
mod animation_tests {
    use keine_core::state::PresetAnimation;
    use keine_core::{AnimationPreset, FilmEffects, SpriteTransform};

    use super::animation_uniform;

    #[test]
    fn film_bits_compose_and_shockwave_keeps_progress() {
        let mut films = FilmEffects::default();
        assert!(films.apply(&AnimationPreset::OldFilm));
        assert!(films.apply(&AnimationPreset::RgbFilm));
        let animation = PresetAnimation {
            preset: AnimationPreset::ShockwaveOut,
            base: SpriteTransform::default(),
            elapsed: 0.5,
            duration: 1.0,
            blocking: true,
            remove_on_finish: false,
        };
        let uniform = animation_uniform(films, Some(&animation));
        assert_eq!(
            uniform.z as u8,
            FilmEffects::OLD_FILM | FilmEffects::RGB_FILM | 128
        );
        assert_eq!(uniform.w, 0.5);
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct StageMaterialKey(u8);

// Keep a bounded ladder instead of specializing every effect combination.
// Optical is the superset used when blur and outline must coexist.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageShaderClass {
    Plain,
    Basic,
    Blur,
    Outline,
    Optical,
}

impl From<&StageMaterial> for StageMaterialKey {
    fn from(material: &StageMaterial) -> Self {
        let blend = match material.blend {
            BlendMode::Alpha => 0,
            BlendMode::Add => 1,
            BlendMode::Multiply => 2,
            BlendMode::Screen => 3,
        };
        Self(blend | (material.shader_class() as u8) << 2)
    }
}

impl StageMaterial {
    fn shader_class(&self) -> StageShaderClass {
        const ACTIVE: f32 = 0.001;
        const RGB_FILM: u8 = 1 << 4;
        let blur = self.filter.x > ACTIVE
            || self.post_a.w > ACTIVE
            || self.post_b.w > ACTIVE
            || (self.transition.z.round() as u8 & RGB_FILM) != 0;
        let outline = self.post_q.y > ACTIVE;
        let optical = self.post_g.y > ACTIVE
            || self.post_g.z > ACTIVE
            || self.post_h.z > ACTIVE
            || self.post_h.w > ACTIVE
            || self.post_i.z > ACTIVE
            || self.post_j.x > ACTIVE;
        if optical || (blur && outline) {
            return StageShaderClass::Optical;
        }
        if blur {
            return StageShaderClass::Blur;
        }
        if outline {
            return StageShaderClass::Outline;
        }
        let basic = (self.filter.y - 1.0).abs() > ACTIVE
            || (self.filter.z - 1.0).abs() > ACTIVE
            || (self.filter.w - 1.0).abs() > ACTIVE
            || self.transition.x.abs() > ACTIVE
            || self.transition.z.abs() > ACTIVE
            || self.post_a.x.abs() > ACTIVE
            || self.post_a.y > ACTIVE
            || self.post_b.y > ACTIVE
            || self.post_b.z > ACTIVE
            || self.post_c.x > ACTIVE
            || self.post_c.y > ACTIVE
            || self.post_f.x.abs() > ACTIVE
            || self.post_f.y.abs() > ACTIVE
            || self.post_f.z.abs() > ACTIVE
            || (self.post_f.w - 1.0).abs() > ACTIVE
            || self.post_g.x.abs() > ACTIVE
            || self.post_g.w > 1.01
            || self.post_h.x > ACTIVE
            || self.post_h.y > ACTIVE
            || self.post_j.w > ACTIVE
            || self.post_k.y > ACTIVE
            || self.post_l.x > ACTIVE
            || self.post_l.z > ACTIVE
            || self.post_m.y > ACTIVE
            || self.post_n.z > ACTIVE
            || self.post_o.y > ACTIVE
            || self.post_p.x > ACTIVE
            || self.post_p.w > ACTIVE
            || self.post_q.w < 0.999;
        if basic {
            StageShaderClass::Basic
        } else {
            StageShaderClass::Plain
        }
    }
}

impl Material2d for StageMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path(
            AssetPath::from_path_buf(embedded_path!("../../assets/shaders/stage_material.wgsl"))
                .with_source("embedded"),
        )
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let blend_key = key.bind_group_data.0 & 0b11;
        let blend = match blend_key {
            1 => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::SrcAlpha,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent::OVER,
            },
            2 => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::Dst,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent::OVER,
            },
            3 => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrc,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent::OVER,
            },
            _ => BlendState::ALPHA_BLENDING,
        };
        if let Some(fragment) = descriptor.fragment.as_mut() {
            let shader_class = key.bind_group_data.0 >> 2;
            if shader_class != StageShaderClass::Plain as u8 {
                fragment.shader_defs.push("STAGE_COMPLEX".into());
            }
            match shader_class {
                class if class == StageShaderClass::Blur as u8 => {
                    fragment.shader_defs.push("STAGE_BLUR".into());
                }
                class if class == StageShaderClass::Outline as u8 => {
                    fragment.shader_defs.push("STAGE_OUTLINE".into());
                }
                class if class == StageShaderClass::Optical as u8 => {
                    fragment.shader_defs.push("STAGE_BLUR".into());
                    fragment.shader_defs.push("STAGE_OUTLINE".into());
                    fragment.shader_defs.push("STAGE_OPTICAL".into());
                }
                _ => {}
            }
            match blend_key {
                2 => fragment.shader_defs.push("BLEND_MULTIPLY".into()),
                3 => fragment.shader_defs.push("BLEND_SCREEN".into()),
                _ => {}
            }
            if let Some(target) = fragment.targets.first_mut().and_then(Option::as_mut) {
                target.blend = Some(blend);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_material_keeps_a_compact_bind_group() {
        let shader = include_str!("../../assets/shaders/stage_material.wgsl");

        assert_eq!(shader.matches("var<uniform>").count(), 1);
        assert_eq!(shader.matches("@binding(").count(), 5);
        for binding in 0..=4 {
            assert!(shader.contains(&format!("@binding({binding})")));
        }
    }

    #[test]
    fn lut_without_visible_intensity_does_not_request_an_asset() {
        let mut effect = PostProcessEffect {
            lut_preset: Some("warm".into()),
            ..default()
        };
        assert_eq!(active_lut_preset(&effect), None);

        effect.lut_intensity = 0.5;
        assert_eq!(active_lut_preset(&effect), Some("warm"));
    }

    #[test]
    fn classic_color_parameters_follow_letsgal_filter_limits() {
        let effect = PostProcessEffect {
            color_exposure: 9.0,
            color_brightness: -9.0,
            color_contrast: 9.0,
            color_saturation: 9.0,
            ..default()
        };
        let material = StageMaterial::new(
            Handle::default(),
            1.0,
            VisualFilter::default(),
            BlendMode::Alpha,
            Vec4::ZERO,
            &effect,
            None,
        );

        assert_eq!(material.post_f, Vec4::new(2.0, -1.0, 1.0, 2.0));
    }

    #[test]
    fn plain_material_uses_the_lightweight_pipeline_variant() {
        let plain = StageMaterial::new(
            Handle::default(),
            1.0,
            VisualFilter::default(),
            BlendMode::Alpha,
            Vec4::ZERO,
            &PostProcessEffect::default(),
            None,
        );
        assert_eq!(StageMaterialKey::from(&plain).0, 0);

        let mut filtered = plain.clone();
        filtered.filter.y = 1.2;
        assert_eq!(StageMaterialKey::from(&filtered).0, 0b100);

        let blur_variants: [fn(&mut StageMaterial); 4] = [
            |material| material.filter.x = 1.0,
            |material| material.post_a.w = 1.0,
            |material| material.post_b.w = 0.5,
            |material| material.transition.z = 16.0,
        ];
        for activate in blur_variants {
            let mut blur = plain.clone();
            activate(&mut blur);
            assert_eq!(StageMaterialKey::from(&blur).0, 0b1000);
        }

        let mut outline = plain.clone();
        outline.post_q.y = 0.5;
        assert_eq!(StageMaterialKey::from(&outline).0, 0b1100);

        let optical_variants: [fn(&mut StageMaterial); 6] = [
            |material| material.post_g.y = 0.5,
            |material| material.post_g.z = 0.5,
            |material| material.post_h.z = 0.5,
            |material| material.post_h.w = 0.5,
            |material| material.post_i.z = 0.5,
            |material| material.post_j.x = 0.5,
        ];
        for activate in optical_variants {
            let mut optical = plain.clone();
            activate(&mut optical);
            assert_eq!(StageMaterialKey::from(&optical).0, 0b10000);
        }

        let mut combined = outline;
        combined.filter.x = 1.0;
        assert_eq!(StageMaterialKey::from(&combined).0, 0b10000);

        let mut screen = plain;
        screen.blend = BlendMode::Screen;
        assert_eq!(StageMaterialKey::from(&screen).0, 0b11);
    }

    #[test]
    fn global_optical_effects_do_not_promote_every_layer_to_optical() {
        let source = PostProcessEffect {
            radial_blur_strength: 0.3,
            motion_blur_strength: 0.4,
            zoom_blur_strength: 0.5,
            chromatic_aberration: 0.6,
            sharpen_strength: 0.7,
            bloom_intensity: 0.8,
            ..default()
        };
        let composed_layer =
            effective_post_process(&source, CameraTargets::ALL, "characters", None);
        let selective_layer =
            effective_post_process(&source, CameraTargets::CHARACTERS, "characters", None);
        let material = |effect: &PostProcessEffect| {
            StageMaterial::new(
                Handle::default(),
                1.0,
                VisualFilter::default(),
                BlendMode::Alpha,
                Vec4::ZERO,
                effect,
                None,
            )
        };

        assert_eq!(StageMaterialKey::from(&material(&composed_layer)).0, 0);
        assert_eq!(
            StageMaterialKey::from(&material(&selective_layer)).0,
            0b10000
        );
    }
}
