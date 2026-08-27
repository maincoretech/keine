use std::collections::{HashMap, HashSet};

use bevy::asset::{AssetPath, RenderAssetUsages, embedded_asset, embedded_path};
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::mesh::{
    Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology,
    VertexAttributeValues,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat, VertexFormat,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};
use keine_core::{DESIGN_HEIGHT, DESIGN_WIDTH, ParticleEffect};

use crate::runtime::platform::DesignViewport;
use crate::runtime::resources::GameState;

const MAX_PARTICLE_COUNT: usize = 256;
const FALLBACK_TEXTURE_SIZE: u32 = 32;
const PARTICLE_STEP_SECONDS: f32 = 1.0 / 60.0;
const MAX_PARTICLE_STEPS_PER_FRAME: usize = 8;
const ATTRIBUTE_PREVIOUS_POSITION: MeshVertexAttribute = MeshVertexAttribute::new(
    "ParticlePreviousPosition",
    988_540_918,
    VertexFormat::Float32x3,
);

pub(crate) struct ParticleMaterialPlugin;

impl Plugin for ParticleMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "../../assets/shaders/particle_material.wgsl");
        app.add_plugins(Material2dPlugin::<ParticleMaterial>::default());
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct ParticleMaterial {
    #[texture(0, visibility(fragment))]
    #[sampler(1, visibility(fragment))]
    texture: Handle<Image>,
}

impl Material2d for ParticleMaterial {
    fn vertex_shader() -> ShaderRef {
        particle_shader()
    }

    fn fragment_shader() -> ShaderRef {
        particle_shader()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(4),
            ATTRIBUTE_PREVIOUS_POSITION.at_shader_location(5),
        ])?];
        Ok(())
    }
}

fn particle_shader() -> ShaderRef {
    ShaderRef::Path(
        AssetPath::from_path_buf(embedded_path!(
            "../../assets/shaders/particle_material.wgsl"
        ))
        .with_source("embedded"),
    )
}

#[derive(SystemParam)]
pub(crate) struct ParticleAssets<'w> {
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<ParticleMaterial>>,
}

#[derive(Resource, Default)]
pub(crate) struct ParticleRuntime {
    effects: HashMap<String, ParticleEffect>,
    native_textures: HashMap<ParticleKind, Handle<Image>>,
}

/// Fixed-rate weather simulation clock.
///
/// A 120/144 Hz presentation no longer integrates and uploads the same small
/// particle mesh at the monitor refresh rate. Rendering remains uncapped and
/// frame-rate independent; only the ambient simulation uses a stable 60 Hz
/// cadence, catching up in bounded steps after a slow frame.
#[derive(Resource, Default)]
pub(crate) struct ParticleClock {
    accumulator: f32,
    elapsed: f32,
}

impl ParticleClock {
    fn advance(&mut self, delta_seconds: f32) -> ParticleFrame {
        let max_accumulator = PARTICLE_STEP_SECONDS * MAX_PARTICLE_STEPS_PER_FRAME as f32;
        self.accumulator = (self.accumulator + delta_seconds.max(0.0)).min(max_accumulator);
        let steps = (self.accumulator / PARTICLE_STEP_SECONDS).floor() as usize;
        if steps > 0 {
            let advanced = PARTICLE_STEP_SECONDS * steps as f32;
            self.accumulator -= advanced;
            self.elapsed += advanced;
        }
        ParticleFrame {
            steps,
            previous_elapsed: (self.elapsed - PARTICLE_STEP_SECONDS).max(0.0),
            current_elapsed: self.elapsed,
        }
    }

    fn synchronize(&mut self, elapsed: f32) {
        self.accumulator = elapsed.rem_euclid(PARTICLE_STEP_SECONDS);
        self.elapsed = elapsed - self.accumulator;
    }
}

#[derive(Debug, Clone, Copy)]
struct ParticleFrame {
    steps: usize,
    previous_elapsed: f32,
    current_elapsed: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ParticleKind {
    Snow,
    Rain,
    Firefly,
    Leaf,
    Ambient,
}

#[derive(Clone, Copy)]
struct Particle {
    position: Vec2,
    previous_position: Vec2,
    velocity: Vec2,
    size: Vec2,
    drift: f32,
    phase: f32,
    angular_velocity: f32,
    rotation: f32,
    previous_rotation: f32,
    base_alpha: f32,
    depth: f32,
    cycle: u32,
}

/// One ECS/render entity per emitter. Individual particles are packed into a
/// single dynamic mesh instead of paying transform, sprite extraction and
/// batching costs for every flake or rain streak.
#[derive(Component)]
pub(crate) struct ParticleBatch {
    effect_id: String,
    kind: ParticleKind,
    particles: Vec<Particle>,
    acceleration: Vec2,
    drag: f32,
    color: Color,
    mesh: Handle<Mesh>,
    material: Handle<ParticleMaterial>,
}

pub(crate) fn sync(
    state: Res<GameState>,
    mut runtime: ResMut<ParticleRuntime>,
    batches: Query<(Entity, &ParticleBatch)>,
    clock: Res<ParticleClock>,
    asset_server: Res<AssetServer>,
    mut assets: ParticleAssets,
    mut commands: Commands,
) {
    let unchanged = runtime.effects.len() == state.particle_effects.len()
        && state
            .particle_effects
            .iter()
            .all(|(id, active)| runtime.effects.get(id) == Some(&active.effect));
    if unchanged {
        return;
    }
    let desired = state
        .particle_effects
        .iter()
        .map(|(id, active)| (id.clone(), active.effect.clone()))
        .collect::<HashMap<_, _>>();

    let changed = runtime
        .effects
        .keys()
        .chain(desired.keys())
        .filter(|id| runtime.effects.get(*id) != desired.get(*id))
        .cloned()
        .collect::<HashSet<_>>();
    let mut stale_meshes = Vec::new();
    let mut stale_materials = Vec::new();
    for (entity, batch) in &batches {
        if changed.contains(&batch.effect_id) {
            stale_meshes.push(batch.mesh.id());
            stale_materials.push(batch.material.id());
            commands.entity(entity).despawn();
        }
    }
    for mesh in stale_meshes {
        assets.meshes.remove(mesh);
    }
    for material in stale_materials {
        assets.materials.remove(material);
    }

    for id in &changed {
        let Some(effect) = desired.get(id) else {
            continue;
        };
        let style = ParticleStyle::from_effect(effect);
        let texture = if let Some(path) = effect.texture.as_ref().filter(|path| !path.is_empty()) {
            asset_server.load::<Image>(path.clone())
        } else if let Some(texture) = runtime.native_textures.get(&style.kind) {
            texture.clone()
        } else {
            let texture = assets.images.add(native_particle_texture(style.kind));
            runtime.native_textures.insert(style.kind, texture.clone());
            texture
        };
        let count = if effect.count == 0 {
            style.count
        } else {
            usize::from(effect.count)
        }
        .clamp(1, MAX_PARTICLE_COUNT);

        let particles = (0..count)
            .map(|index| {
                let perspective = ParticlePerspective::new(style.kind, index);
                let depth = perspective.depth;
                let size = style.size * perspective.size;
                let speed = style.speed * perspective.speed;
                let horizontal = if style.kind == ParticleKind::Rain {
                    effect.wind.unwrap_or(style.wind) * (speed / style.speed)
                } else {
                    let horizontal =
                        effect.wind.unwrap_or(style.wind) + (random(index, 4) - 0.5) * style.spread;
                    if style.kind == ParticleKind::Snow {
                        horizontal * perspective.speed
                    } else {
                        horizontal
                    }
                };
                let position = Vec2::new(
                    random(index, 5) * (DESIGN_WIDTH + 240.0) - 120.0,
                    random(index, 6) * (DESIGN_HEIGHT + 160.0) - 40.0,
                );
                let base_alpha = style.alpha * perspective.alpha;
                let rotation = if style.kind == ParticleKind::Rain {
                    style.rotation
                } else {
                    style.rotation + random(index, 8) * std::f32::consts::TAU
                };
                Particle {
                    position,
                    previous_position: position,
                    velocity: Vec2::new(horizontal, -speed),
                    size: Vec2::new(size * style.aspect, size),
                    drift: style.drift * perspective.drift,
                    phase: random(index, 10) * std::f32::consts::TAU,
                    angular_velocity: style.angular_velocity
                        * (0.55 + random(index, 11) * 0.9)
                        * if random(index, 12) > 0.5 { 1.0 } else { -1.0 },
                    rotation,
                    previous_rotation: rotation,
                    base_alpha,
                    depth,
                    cycle: 0,
                }
            })
            .collect::<Vec<_>>();
        let mesh = assets.meshes.add(particle_mesh(
            &particles,
            style.kind,
            style.color,
            state
                .particle_effects
                .get(id)
                .map_or(1.0, |active| active.opacity()),
            clock.elapsed,
        ));
        let material = assets.materials.add(ParticleMaterial { texture });
        commands.spawn((
            Name::new(format!("particle-batch::{id}")),
            ParticleBatch {
                effect_id: id.clone(),
                kind: style.kind,
                particles,
                acceleration: Vec2::new(
                    style.acceleration_x,
                    -effect.gravity.unwrap_or(style.acceleration_y),
                ),
                drag: style.drag,
                color: style.color,
                mesh: mesh.clone(),
                material: material.clone(),
            },
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform::from_xyz(0.0, 0.0, 0.8),
            RenderLayers::layer(0),
        ));
    }
    runtime.effects = desired;
}

pub(crate) fn animate(
    time: Res<Time>,
    state: Res<GameState>,
    windows: Query<&Window>,
    mut clock: ResMut<ParticleClock>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut batches: Query<(&mut ParticleBatch, &mut Transform)>,
) {
    if batches.is_empty() {
        clock.synchronize(time.elapsed_secs());
        return;
    }
    let frame = clock.advance(time.delta_secs());
    let Ok(window) = windows.single() else {
        return;
    };
    let viewport = DesignViewport::from_window(window);
    for (mut batch, mut transform) in &mut batches {
        let Some(effect) = state.particle_effects.get(&batch.effect_id) else {
            continue;
        };
        let translation = viewport.content_center().extend(0.8);
        let scale = Vec3::splat(viewport.scale);
        if transform.translation != translation {
            transform.translation = translation;
        }
        if transform.scale != scale {
            transform.scale = scale;
        }
        if frame.steps == 0 {
            continue;
        }
        let kind = batch.kind;
        let acceleration = batch.acceleration;
        let drag_factor = (-batch.drag * PARTICLE_STEP_SECONDS).exp();
        let opacity = effect.opacity();
        let linear = batch.color.to_linear().to_f32_array();
        let mesh_handle = batch.mesh.clone();
        for particle in &mut batch.particles {
            for _ in 0..frame.steps {
                particle.previous_position = particle.position;
                particle.previous_rotation = particle.rotation;
                particle.velocity += acceleration * PARTICLE_STEP_SECONDS;
                particle.velocity *= drag_factor;
                particle.position += particle.velocity * PARTICLE_STEP_SECONDS;
                particle.rotation += particle.angular_velocity * PARTICLE_STEP_SECONDS;

                let margin = particle.size.max_element().max(24.0) * 2.0;
                if particle.position.y < -margin {
                    particle.cycle = particle.cycle.wrapping_add(1);
                    particle.position.y = DESIGN_HEIGHT + margin;
                    particle.position.x = respawn_x(particle);
                    particle.previous_position = particle.position;
                    particle.previous_rotation = particle.rotation;
                }
                if particle.position.x < -margin {
                    particle.position.x = DESIGN_WIDTH + margin;
                    particle.previous_position = particle.position;
                    particle.previous_rotation = particle.rotation;
                } else if particle.position.x > DESIGN_WIDTH + margin {
                    particle.position.x = -margin;
                    particle.previous_position = particle.position;
                    particle.previous_rotation = particle.rotation;
                }
            }
        }

        let Some(mut mesh) = meshes.get_mut(&mesh_handle) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(previous_positions)) =
            mesh.attribute_mut(ATTRIBUTE_PREVIOUS_POSITION)
        else {
            continue;
        };
        write_batch_positions(
            previous_positions,
            &batch.particles,
            kind,
            frame.previous_elapsed,
            true,
        );
        let Some(VertexAttributeValues::Float32x3(current_positions)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        write_batch_positions(
            current_positions,
            &batch.particles,
            kind,
            frame.current_elapsed,
            false,
        );
        let Some(VertexAttributeValues::Float32x4(colors)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
        else {
            continue;
        };
        write_particle_colors(
            colors,
            &batch.particles,
            kind,
            linear,
            opacity,
            frame.current_elapsed,
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct ParticlePerspective {
    depth: f32,
    size: f32,
    speed: f32,
    alpha: f32,
    drift: f32,
}

impl ParticlePerspective {
    fn new(kind: ParticleKind, index: usize) -> Self {
        if kind == ParticleKind::Rain {
            let selector = random(index, 1);
            let within_band = random(index, 13);
            let depth = if selector < 0.42 {
                0.24 + within_band * 0.20
            } else if selector < 0.80 {
                0.48 + within_band * 0.24
            } else {
                0.78 + within_band * 0.22
            };
            return Self {
                depth,
                // Rain uses a restrained perspective range so its parallel
                // direction remains the dominant visual characteristic.
                size: (0.72 + depth * 0.56) * (0.90 + random(index, 2) * 0.20),
                speed: (0.76 + depth * 0.42) * (0.94 + random(index, 3) * 0.12),
                alpha: (0.58 + depth * 0.42) * (0.88 + random(index, 7) * 0.12),
                drift: 1.0,
            };
        }

        if kind != ParticleKind::Snow {
            let depth = random(index, 1).mul_add(0.7, 0.3);
            return Self {
                depth,
                size: (0.64 + random(index, 2) * 0.72) * (0.55 + depth * 0.7),
                speed: 0.72 + random(index, 3) * 0.56,
                alpha: (0.72 + random(index, 7) * 0.28) * depth.sqrt(),
                drift: 0.55 + random(index, 9) * 0.9,
            };
        }

        // Deliberately separated depth bands read more clearly than a uniform
        // distribution: many distant flakes establish scale, while a smaller
        // foreground layer crosses the screen faster and larger.
        let selector = random(index, 1);
        let within_band = random(index, 13);
        let depth = if selector < 0.46 {
            0.16 + within_band * 0.22
        } else if selector < 0.82 {
            0.42 + within_band * 0.30
        } else {
            0.78 + within_band * 0.22
        };
        Self {
            depth,
            size: (0.20 + depth.powf(1.35) * 1.50) * (0.82 + random(index, 2) * 0.36),
            speed: (0.38 + depth * 1.10) * (0.88 + random(index, 3) * 0.24),
            alpha: (0.40 + depth * 0.60) * (0.82 + random(index, 7) * 0.18),
            drift: (0.50 + depth * 0.90) * (0.72 + random(index, 9) * 0.56),
        }
    }
}

fn particle_mesh(
    particles: &[Particle],
    kind: ParticleKind,
    color: Color,
    opacity: f32,
    elapsed: f32,
) -> Mesh {
    let count = particles.len();
    let mut uvs = Vec::with_capacity(count * 4);
    let mut indices = Vec::with_capacity(count * 6);
    for index in 0..count {
        uvs.extend_from_slice(&[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        let base = (index * 4) as u32;
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    let mut positions = vec![[0.0; 3]; count * 4];
    write_batch_positions(&mut positions, particles, kind, elapsed, false);
    let mut colors = vec![[1.0, 1.0, 1.0, 0.0]; count * 4];
    write_particle_colors(
        &mut colors,
        particles,
        kind,
        color.to_linear().to_f32_array(),
        opacity,
        elapsed,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone());
    mesh.insert_attribute(ATTRIBUTE_PREVIOUS_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn write_particle_colors(
    colors: &mut [[f32; 4]],
    particles: &[Particle],
    kind: ParticleKind,
    linear: [f32; 4],
    opacity: f32,
    elapsed: f32,
) {
    for (index, particle) in particles.iter().enumerate() {
        let pulse = if kind == ParticleKind::Firefly {
            0.7 + 0.3 * (elapsed * 2.1 + particle.phase).sin().abs()
        } else {
            1.0
        };
        let alpha = (particle.base_alpha * opacity * pulse).clamp(0.0, 1.0);
        colors[index * 4..index * 4 + 4].fill([linear[0], linear[1], linear[2], alpha]);
    }
}

fn write_batch_positions(
    positions: &mut [[f32; 3]],
    particles: &[Particle],
    kind: ParticleKind,
    elapsed: f32,
    previous: bool,
) {
    for (index, particle) in particles.iter().enumerate() {
        let center = if previous {
            particle.previous_position
        } else {
            particle.position
        } + particle_motion(kind, particle, elapsed);
        let rotation = if previous {
            particle.previous_rotation
        } else {
            particle.rotation
        };
        write_particle_quad(
            positions,
            index,
            center,
            particle.size,
            rotation,
            particle.depth,
        );
    }
}

fn particle_motion(kind: ParticleKind, particle: &Particle, elapsed: f32) -> Vec2 {
    match kind {
        ParticleKind::Snow => Vec2::new(
            (elapsed * (0.72 + particle.depth * 0.86) + particle.phase).sin() * particle.drift,
            0.0,
        ),
        ParticleKind::Leaf => Vec2::new(
            (elapsed * 1.15 + particle.phase).sin() * particle.drift,
            0.0,
        ),
        ParticleKind::Firefly => Vec2::new(
            (elapsed * 0.83 + particle.phase).sin() * particle.drift,
            (elapsed * 1.07 + particle.phase * 0.7).cos() * particle.drift * 0.45,
        ),
        ParticleKind::Rain | ParticleKind::Ambient => Vec2::ZERO,
    }
}

fn write_particle_quad(
    positions: &mut [[f32; 3]],
    particle_index: usize,
    center: Vec2,
    size: Vec2,
    rotation: f32,
    depth: f32,
) {
    let center = center - Vec2::new(DESIGN_WIDTH, DESIGN_HEIGHT) * 0.5;
    let half = size * 0.5;
    let (sin, cos) = rotation.sin_cos();
    for (corner_index, corner) in [
        Vec2::new(-half.x, -half.y),
        Vec2::new(half.x, -half.y),
        Vec2::new(half.x, half.y),
        Vec2::new(-half.x, half.y),
    ]
    .into_iter()
    .enumerate()
    {
        let rotated = Vec2::new(
            corner.x * cos - corner.y * sin,
            corner.x * sin + corner.y * cos,
        );
        let point = center + rotated;
        positions[particle_index * 4 + corner_index] = [point.x, point.y, depth * 0.001];
    }
}

#[derive(Clone, Copy)]
struct ParticleStyle {
    kind: ParticleKind,
    color: Color,
    count: usize,
    speed: f32,
    wind: f32,
    spread: f32,
    acceleration_x: f32,
    acceleration_y: f32,
    drag: f32,
    size: f32,
    aspect: f32,
    alpha: f32,
    drift: f32,
    rotation: f32,
    angular_velocity: f32,
}

impl ParticleStyle {
    fn from_effect(effect: &ParticleEffect) -> Self {
        match effect.preset.to_ascii_uppercase().as_str() {
            "LIGHT_SNOW" => Self::snow(64, 82.0, 8.0),
            "MODERATE_SNOW" => Self::snow(120, 118.0, 9.0),
            "HEAVY_SNOW" => Self::snow(192, 154.0, 10.0),
            "LIGHT_RAIN" => Self::rain(56, 660.0),
            "MODERATE_RAIN" => Self::rain(112, 820.0),
            "HEAVY_RAIN" => Self::rain(192, 980.0),
            "FIREFLY" => Self::firefly(),
            "FALLEN_LEAVES" => Self::leaves(),
            name if name.contains("SNOW") => Self::snow(96, 112.0, 9.0),
            name if name.contains("RAIN") => Self::rain(96, 760.0),
            name if name.contains("FIREFLY") || name.contains("LIGHT") => Self::firefly(),
            name if name.contains("LEAF") || name.contains("SAKURA") || name.contains("PETAL") => {
                Self::leaves()
            }
            _ => Self::ambient(),
        }
    }

    fn snow(count: usize, speed: f32, size: f32) -> Self {
        Self {
            kind: ParticleKind::Snow,
            color: Color::WHITE,
            count,
            speed,
            // Scale horizontal velocity with fall speed so every density
            // keeps the same clearly diagonal trajectory.
            wind: -speed * 0.34,
            spread: speed * 0.08,
            acceleration_x: 0.0,
            acceleration_y: 8.0,
            drag: 0.035,
            size,
            aspect: 1.0,
            alpha: 0.82,
            drift: 22.0,
            rotation: 0.0,
            angular_velocity: 0.45,
        }
    }

    fn rain(count: usize, speed: f32) -> Self {
        Self {
            kind: ParticleKind::Rain,
            color: Color::srgba(0.72, 0.84, 0.96, 1.0),
            count,
            speed,
            wind: -105.0,
            spread: 36.0,
            acceleration_x: 0.0,
            acceleration_y: 0.0,
            drag: 0.01,
            size: 64.0,
            aspect: 0.14,
            alpha: 0.72,
            drift: 0.0,
            rotation: -0.14,
            angular_velocity: 0.0,
        }
    }

    fn firefly() -> Self {
        Self {
            kind: ParticleKind::Firefly,
            color: Color::srgba(1.0, 0.86, 0.38, 1.0),
            count: 46,
            speed: -7.0,
            wind: 2.0,
            spread: 8.0,
            acceleration_x: 0.0,
            acceleration_y: 0.0,
            drag: 0.22,
            size: 20.0,
            aspect: 1.0,
            alpha: 0.72,
            drift: 30.0,
            rotation: 0.0,
            angular_velocity: 0.0,
        }
    }

    fn leaves() -> Self {
        Self {
            kind: ParticleKind::Leaf,
            color: Color::srgba(0.78, 0.48, 0.16, 1.0),
            count: 30,
            speed: 46.0,
            wind: -18.0,
            spread: 18.0,
            acceleration_x: -1.0,
            acceleration_y: 5.0,
            drag: 0.08,
            size: 34.0,
            aspect: 0.58,
            alpha: 0.88,
            drift: 20.0,
            rotation: 0.0,
            angular_velocity: 0.54,
        }
    }

    fn ambient() -> Self {
        Self {
            kind: ParticleKind::Ambient,
            color: Color::srgba(0.78, 0.88, 1.0, 1.0),
            count: 56,
            speed: 62.0,
            wind: 0.0,
            spread: 18.0,
            acceleration_x: 0.0,
            acceleration_y: 4.0,
            drag: 0.08,
            size: 9.0,
            aspect: 1.0,
            alpha: 0.55,
            drift: 0.0,
            rotation: 0.0,
            angular_velocity: 0.0,
        }
    }
}

fn soft_particle_texture() -> Image {
    let center = (FALLBACK_TEXTURE_SIZE as f32 - 1.0) * 0.5;
    let mut rgba = Vec::with_capacity((FALLBACK_TEXTURE_SIZE * FALLBACK_TEXTURE_SIZE * 4) as usize);
    for y in 0..FALLBACK_TEXTURE_SIZE {
        for x in 0..FALLBACK_TEXTURE_SIZE {
            let distance = Vec2::new(x as f32 - center, y as f32 - center).length() / center;
            let alpha = (1.0 - distance).clamp(0.0, 1.0);
            let alpha = alpha * alpha * (3.0 - 2.0 * alpha);
            rgba.extend_from_slice(&[255, 255, 255, (alpha * 255.0).round() as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: FALLBACK_TEXTURE_SIZE,
            height: FALLBACK_TEXTURE_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn native_particle_texture(kind: ParticleKind) -> Image {
    match kind {
        ParticleKind::Rain => procedural_texture(|point| {
            let across = (point.x * 5.5).abs();
            let along = (1.0 - point.y.abs()).clamp(0.0, 1.0);
            (1.0 - across).clamp(0.0, 1.0).powi(2) * along.sqrt()
        }),
        ParticleKind::Leaf => procedural_texture(|point| {
            let along = point.y.abs();
            let half_width = (1.0 - along).max(0.0).sqrt() * 0.68;
            let body = 1.0 - (point.x.abs() / half_width.max(0.001));
            body.clamp(0.0, 1.0).powf(0.7)
        }),
        ParticleKind::Firefly => procedural_texture(|point| {
            let distance = point.length();
            (1.0 - distance).clamp(0.0, 1.0).powf(1.6)
        }),
        ParticleKind::Snow | ParticleKind::Ambient => soft_particle_texture(),
    }
}

fn procedural_texture(alpha_at: impl Fn(Vec2) -> f32) -> Image {
    let center = (FALLBACK_TEXTURE_SIZE as f32 - 1.0) * 0.5;
    let mut rgba = Vec::with_capacity((FALLBACK_TEXTURE_SIZE * FALLBACK_TEXTURE_SIZE * 4) as usize);
    for y in 0..FALLBACK_TEXTURE_SIZE {
        for x in 0..FALLBACK_TEXTURE_SIZE {
            let point = Vec2::new(x as f32 - center, y as f32 - center) / center;
            let alpha = alpha_at(point).clamp(0.0, 1.0);
            rgba.extend_from_slice(&[255, 255, 255, (alpha * 255.0).round() as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: FALLBACK_TEXTURE_SIZE,
            height: FALLBACK_TEXTURE_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn respawn_x(particle: &Particle) -> f32 {
    let seed = particle
        .cycle
        .wrapping_mul(1_597_334_677)
        .wrapping_add(particle.phase.to_bits());
    hash(seed) * (DESIGN_WIDTH + 240.0) - 120.0
}

fn random(index: usize, salt: u32) -> f32 {
    hash((index as u32).wrapping_mul(31).wrapping_add(salt))
}

fn hash(value: u32) -> f32 {
    let value = value.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let value = ((value >> ((value >> 28) + 4)) ^ value).wrapping_mul(277_803_737);
    ((value >> 22) ^ value) as f32 / u32::MAX as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_clock_is_fixed_rate_and_catch_up_is_bounded() {
        let mut clock = ParticleClock::default();
        assert_eq!(clock.advance(PARTICLE_STEP_SECONDS * 0.5).steps, 0);
        let frame = clock.advance(PARTICLE_STEP_SECONDS * 0.5);
        assert_eq!(frame.steps, 1);
        assert_eq!(clock.advance(PARTICLE_STEP_SECONDS * 3.2).steps, 3);
        assert_eq!(clock.advance(10.0).steps, MAX_PARTICLE_STEPS_PER_FRAME);
    }

    #[test]
    fn presets_have_bounded_gal_friendly_density() {
        for preset in [
            "LIGHT_SNOW",
            "MODERATE_SNOW",
            "HEAVY_SNOW",
            "LIGHT_RAIN",
            "MODERATE_RAIN",
            "HEAVY_RAIN",
            "FIREFLY",
            "FALLEN_LEAVES",
        ] {
            let style = ParticleStyle::from_effect(&ParticleEffect::preset(preset));
            assert!((1..=MAX_PARTICLE_COUNT).contains(&style.count));
            assert!(style.size > 0.0);
            assert!(style.alpha > 0.0 && style.alpha <= 1.0);
        }
    }

    #[test]
    fn fallback_texture_has_soft_transparent_edges() {
        let image = soft_particle_texture();
        let data = image.data.as_ref().unwrap();
        assert_eq!(data[3], 0);
        let center =
            ((FALLBACK_TEXTURE_SIZE / 2 * FALLBACK_TEXTURE_SIZE + FALLBACK_TEXTURE_SIZE / 2) * 4
                + 3) as usize;
        assert!(data[center] > 240);
    }

    #[test]
    fn native_weather_textures_are_not_reused_snow_discs() {
        let rain = native_particle_texture(ParticleKind::Rain);
        let leaf = native_particle_texture(ParticleKind::Leaf);
        let rain = rain.data.as_ref().unwrap();
        let leaf = leaf.data.as_ref().unwrap();
        assert_ne!(rain, leaf);

        let style = ParticleStyle::from_effect(&ParticleEffect::preset("LIGHT_RAIN"));
        assert_eq!(style.kind, ParticleKind::Rain);
        assert_eq!(style.angular_velocity, 0.0);
    }

    #[test]
    fn snow_uses_distinct_perspective_layers() {
        let profiles = (0..MAX_PARTICLE_COUNT)
            .map(|index| ParticlePerspective::new(ParticleKind::Snow, index))
            .collect::<Vec<_>>();
        let far = profiles
            .iter()
            .filter(|profile| profile.depth < 0.4)
            .collect::<Vec<_>>();
        let has_middle = profiles
            .iter()
            .any(|profile| (0.4..0.76).contains(&profile.depth));
        let near = profiles
            .iter()
            .filter(|profile| profile.depth >= 0.76)
            .collect::<Vec<_>>();

        assert!(!far.is_empty() && has_middle && !near.is_empty());
        assert!(
            near.iter()
                .map(|profile| profile.size)
                .fold(f32::MAX, f32::min)
                > far.iter().map(|profile| profile.size).fold(0.0, f32::max)
        );
        assert!(
            near.iter()
                .map(|profile| profile.speed)
                .fold(f32::MAX, f32::min)
                > far.iter().map(|profile| profile.speed).fold(0.0, f32::max)
        );
    }

    #[test]
    fn snow_presets_are_fine_fast_and_diagonal() {
        let light = ParticleStyle::from_effect(&ParticleEffect::preset("LIGHT_SNOW"));
        let moderate = ParticleStyle::from_effect(&ParticleEffect::preset("MODERATE_SNOW"));
        let heavy = ParticleStyle::from_effect(&ParticleEffect::preset("HEAVY_SNOW"));

        assert!(light.size <= 8.0 && moderate.size <= 9.0 && heavy.size <= 10.0);
        assert!(light.speed >= 82.0 && moderate.speed >= 118.0 && heavy.speed >= 154.0);
        for style in [light, moderate, heavy] {
            assert!(style.wind < 0.0);
            assert!((style.wind / style.speed + 0.34).abs() < 0.001);
            assert!(style.spread <= style.speed * 0.08 + f32::EPSILON);
        }
    }

    #[test]
    fn rain_has_subtle_depth_without_changing_its_direction() {
        let profiles = (0..MAX_PARTICLE_COUNT)
            .map(|index| ParticlePerspective::new(ParticleKind::Rain, index))
            .collect::<Vec<_>>();
        let far = profiles
            .iter()
            .filter(|profile| profile.depth < 0.46)
            .collect::<Vec<_>>();
        let near = profiles
            .iter()
            .filter(|profile| profile.depth >= 0.76)
            .collect::<Vec<_>>();

        assert!(!far.is_empty() && !near.is_empty());
        let average = |values: &[&ParticlePerspective], read: fn(&ParticlePerspective) -> f32| {
            values.iter().map(|profile| read(profile)).sum::<f32>() / values.len() as f32
        };
        assert!(average(&near, |profile| profile.size) > average(&far, |profile| profile.size));
        assert!(average(&near, |profile| profile.speed) > average(&far, |profile| profile.speed));

        let style = ParticleStyle::rain(96, 760.0);
        for profile in profiles {
            let vertical = style.speed * profile.speed;
            let horizontal = style.wind * (vertical / style.speed);
            assert!((horizontal / vertical - style.wind / style.speed).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn emitter_mesh_batches_four_vertices_and_six_indices_per_particle() {
        let particle = Particle {
            position: Vec2::ZERO,
            previous_position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            size: Vec2::ONE,
            drift: 0.0,
            phase: 0.0,
            angular_velocity: 0.0,
            rotation: 0.0,
            previous_rotation: 0.0,
            base_alpha: 1.0,
            depth: 1.0,
            cycle: 0,
        };
        let particles = std::iter::repeat_with(|| Particle { ..particle })
            .take(192)
            .collect::<Vec<_>>();
        let mesh = particle_mesh(&particles, ParticleKind::Snow, Color::WHITE, 1.0, 0.0);
        assert_eq!(mesh.count_vertices(), 192 * 4);
        assert_eq!(mesh.indices().unwrap().len(), 192 * 6);
        assert!(mesh.attribute(ATTRIBUTE_PREVIOUS_POSITION).is_some());
    }

    #[test]
    fn particle_material_uses_global_time_without_a_cpu_uniform() {
        let shader = include_str!("../../assets/shaders/particle_material.wgsl");
        assert_eq!(shader.matches("var<uniform>").count(), 0);
        assert_eq!(shader.matches("@binding(").count(), 2);
        assert!(shader.contains("fract(globals.time * 60.0)"));
        assert!(shader.contains("@location(5) previous_position"));
        assert!(shader.contains("let local_position = mix("));
    }
}
