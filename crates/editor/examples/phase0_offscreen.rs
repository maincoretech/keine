use std::collections::HashMap;
use std::time::{Duration, Instant};

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use keine_editor::frame_transport::{FrameMetadata, LatestFrameBuffer, PixelFormat};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const CAPTURE_COUNT: usize = 123;
const IN_FLIGHT_LIMIT: usize = 3;

#[derive(Resource)]
struct CaptureTarget(Handle<Image>);

#[derive(Resource, Default)]
struct CaptureBenchmark {
    requested: usize,
    in_flight: HashMap<Entity, Instant>,
    latencies: Vec<Duration>,
    completions: Vec<Instant>,
}

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .init_resource::<CaptureBenchmark>()
        .add_systems(Startup, setup)
        .add_systems(Update, request_capture)
        .run();
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let target = images.add(Image::new_target_texture(
        WIDTH,
        HEIGHT,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));

    spawn_camera(
        &mut commands,
        &target,
        0,
        0,
        ClearColorConfig::Custom(Color::srgb_u8(9, 12, 16)),
    );
    spawn_camera(&mut commands, &target, 1, 1, ClearColorConfig::None);
    spawn_camera(&mut commands, &target, 2, 2, ClearColorConfig::None);

    commands.spawn((
        Sprite::from_color(
            Color::srgb_u8(32, 70, 94),
            Vec2::new(WIDTH as f32, HEIGHT as f32),
        ),
        RenderLayers::layer(0),
    ));
    commands.spawn((
        Sprite::from_color(
            Color::srgb_u8(68, 138, 112),
            Vec2::new(WIDTH as f32 / 2.0, HEIGHT as f32 * 5.0 / 9.0),
        ),
        RenderLayers::layer(1),
    ));
    commands.spawn((
        Sprite::from_color(
            Color::srgb_u8(163, 228, 255),
            Vec2::new(WIDTH as f32 * 0.15, HEIGHT as f32 * 0.18),
        ),
        RenderLayers::layer(2),
    ));
    commands.insert_resource(CaptureTarget(target));
}

fn spawn_camera(
    commands: &mut Commands,
    target: &Handle<Image>,
    order: isize,
    layer: usize,
    clear_color: ClearColorConfig,
) {
    commands.spawn((
        Camera2d,
        Camera {
            order,
            clear_color,
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
        RenderLayers::layer(layer),
    ));
}

fn request_capture(
    mut commands: Commands,
    target: Res<CaptureTarget>,
    frames: Res<FrameCount>,
    mut benchmark: ResMut<CaptureBenchmark>,
) {
    if frames.0 < 8 || benchmark.requested >= CAPTURE_COUNT {
        return;
    }
    if benchmark.in_flight.len() < IN_FLIGHT_LIMIT {
        let entity = commands
            .spawn(Screenshot::image(target.0.clone()))
            .observe(validate_capture)
            .id();
        benchmark.in_flight.insert(entity, Instant::now());
        benchmark.requested += 1;
    }
}

fn validate_capture(
    capture: On<ScreenshotCaptured>,
    mut benchmark: ResMut<CaptureBenchmark>,
    mut exits: MessageWriter<AppExit>,
) {
    let requested_at = benchmark
        .in_flight
        .remove(&capture.entity)
        .expect("capture completion must follow a request");
    benchmark.latencies.push(requested_at.elapsed());
    benchmark.completions.push(Instant::now());

    if benchmark.latencies.len() == 1 {
        assert_pixel(&capture.image, 8, 8, [32, 70, 94]);
        assert_pixel(
            &capture.image,
            WIDTH / 2,
            HEIGHT / 2 - HEIGHT / 4,
            [68, 138, 112],
        );
        assert_pixel(&capture.image, WIDTH / 2, HEIGHT / 2, [163, 228, 255]);
    }

    if benchmark.latencies.len() < CAPTURE_COUNT {
        return;
    }

    report_gpu_readback(
        &benchmark.latencies[IN_FLIGHT_LIMIT..],
        &benchmark.completions,
    );
    measure_copy(960, 540, 180);
    measure_copy(1920, 1080, 120);
    println!("phase0 offscreen composition: PASS (scene + ui + dialog camera layers)");
    exits.write(AppExit::Success);
}

fn report_gpu_readback(samples: &[Duration], completions: &[Instant]) {
    let mut milliseconds = samples
        .iter()
        .map(|sample| sample.as_secs_f64() * 1000.0)
        .collect::<Vec<_>>();
    milliseconds.sort_by(f64::total_cmp);
    let median = milliseconds[milliseconds.len() / 2];
    let p95 = milliseconds[(milliseconds.len() * 95 / 100).min(milliseconds.len() - 1)];
    let completion_window = completions
        .last()
        .unwrap()
        .duration_since(completions[IN_FLIGHT_LIMIT]);
    let throughput =
        (completions.len() - IN_FLIGHT_LIMIT - 1) as f64 / completion_window.as_secs_f64();
    println!(
        "phase0 pipelined GPU render + readback {WIDTH}x{HEIGHT}: {} warm samples, median latency={median:.3} ms, p95={p95:.3} ms, throughput={throughput:.2} fps",
        milliseconds.len(),
    );
}

fn assert_pixel(image: &Image, x: u32, y: u32, expected: [u8; 3]) {
    let pixel = image.pixel_bytes(UVec3::new(x, y, 0)).unwrap();
    let actual = [pixel[0], pixel[1], pixel[2]];
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.abs_diff(expected) <= 2),
        "pixel ({x}, {y}) was {actual:?}, expected {expected:?}"
    );
}

fn measure_copy(width: u32, height: u32, iterations: u64) {
    let bytes = vec![0x7f; width as usize * height as usize * 4];
    let mut frames = LatestFrameBuffer::new(width, height).unwrap();
    let started = Instant::now();
    for frame_id in 0..iterations {
        frames
            .publish(
                FrameMetadata {
                    session_generation: 1,
                    document_revision: 1,
                    frame_id,
                    width,
                    height,
                    stride: width * 4,
                    pixel_format: PixelFormat::Rgba8Srgb,
                },
                &bytes,
            )
            .unwrap();
        std::hint::black_box(frames.latest().unwrap().bytes[0]);
    }
    let elapsed = started.elapsed();
    let gib = bytes.len() as f64 * iterations as f64 / 1024_f64.powi(3);
    println!(
        "phase0 frame copy {width}x{height}: {iterations} frames in {:.3} ms ({:.2} GiB/s), bounded={} MiB",
        elapsed.as_secs_f64() * 1000.0,
        gib / elapsed.as_secs_f64(),
        frames.allocated_capacity() / (1024 * 1024),
    );
}
