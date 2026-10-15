//! Shared desktop/Android workload inventory and authored target resolution.
use crate::ui::performance::{BenchmarkCameras, BenchmarkTarget};
use keine_core::{Action, State};

pub(super) type BenchmarkWorkload = (&'static str, &'static str);
pub(super) type BenchmarkSection = (&'static str, &'static [BenchmarkWorkload]);

pub(super) const BASELINE_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] =
    &[("reference composition", "bench_baseline")];
pub(super) const ISOLATED_EFFECT_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    ("effect bloom", "bench_effect_bloom"),
    ("effect depth", "bench_effect_depth"),
    ("effect blur", "bench_effect_blur"),
    ("effect distortion", "bench_effect_distortion"),
    ("effect vignette", "bench_effect_vignette"),
    ("effect color", "bench_effect_color"),
    ("effect tone", "bench_effect_tone"),
    ("effect old_film", "bench_effect_old_film"),
    ("effect shock", "bench_effect_shock"),
    ("effect godray", "bench_effect_godray"),
    ("effect lut", "bench_effect_lut"),
    ("effect chromatic", "bench_effect_chromatic"),
    ("effect pixelate", "bench_effect_pixelate"),
    ("effect glitch", "bench_effect_glitch"),
    ("effect crt", "bench_effect_crt"),
    ("effect sharpen", "bench_effect_sharpen"),
    ("effect radial_blur", "bench_effect_radial_blur"),
    ("effect motion_blur", "bench_effect_motion_blur"),
    ("effect zoom_blur", "bench_effect_zoom_blur"),
    ("effect light_leak", "bench_effect_light_leak"),
    ("effect lens_flare", "bench_effect_lens_flare"),
    ("effect grain", "bench_effect_grain"),
    ("effect heat_haze", "bench_effect_heat_haze"),
    ("effect water", "bench_effect_water"),
    ("effect fog", "bench_effect_fog"),
    ("effect vhs", "bench_effect_vhs"),
    ("effect halftone", "bench_effect_halftone"),
    ("effect dither", "bench_effect_dither"),
    ("effect outline", "bench_effect_outline"),
    ("effect eyelid", "bench_effect_eyelid"),
    ("effect shatter", "bench_effect_shatter"),
    ("effect speed_lines", "bench_effect_speed_lines"),
];
pub(super) const PARTICLE_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    ("particle snow_80", "bench_particle_snow_80"),
    ("particle snow_256", "bench_particle_snow_256"),
    ("particle rain_256", "bench_particle_rain_256"),
    ("particle petals_128", "bench_particle_petals_128"),
    ("particle texture_256", "bench_particle_texture_256"),
    ("particle large_256", "bench_particle_large_256"),
    ("particle layers_768", "bench_particle_layers_768"),
];
pub(super) const MEDIA_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    ("audio_loop", "bench_audio_loop"),
    ("audio_crossfade", "bench_audio_crossfade"),
    ("video_fullscreen", "bench_video_fullscreen"),
    ("video_mixed", "bench_video_mixed"),
];
pub(super) const UI_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    ("text_layers", "bench_text_layers"),
    ("curtain", "bench_curtain"),
    ("sprite_filter", "bench_sprite_filter"),
    ("light_off", "bench_light_off"),
    ("choice", "bench_choice"),
    ("input", "bench_input"),
    ("settings", "bench_settings"),
    ("save panel", "bench_save"),
    ("load panel", "bench_load"),
    ("history", "bench_history"),
    ("gallery", "bench_gallery"),
];

pub(super) const DAILY_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    (
        "representative dialogue · runtime composition",
        "bench_representative_dialogue",
    ),
    (
        "representative portrait motion · runtime composition",
        "bench_representative_portrait_motion",
    ),
    (
        "representative scene transition · runtime composition",
        "bench_representative_scene_transition",
    ),
];
pub(super) const FEATURE_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    ("shared transforms", "bench_shared_transform_clock"),
    ("classic camera", "bench_classic_camera_properties"),
    ("optical effects", "bench_optical_effects"),
    ("blur family", "bench_blur_family"),
    ("atmosphere effects", "bench_atmosphere_effects"),
    ("retro and mask effects", "bench_retro_and_eyelid_mask"),
    ("timed event types", "bench_all_event_types"),
    ("playback controls", "bench_playback_options"),
];
pub(super) const CLASSIC_ATTRIBUTION_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] = &[
    (
        "classic sampling · depth blur and shock",
        "bench_classic_sampling",
    ),
    ("classic godray math", "bench_classic_godray"),
    ("classic film noise", "bench_classic_film_noise"),
    (
        "classic color and lens math",
        "bench_classic_color_and_lens",
    ),
];
pub(super) const STRESS_BENCHMARK_WORKLOADS: &[BenchmarkWorkload] =
    &[("stress composition", "bench_stress_composition")];
pub(super) const REPEATED_BENCHMARK_TARGETS: &[&str] = &[
    "bench_classic_camera_properties",
    "bench_optical_effects",
    "bench_blur_family",
    "bench_atmosphere_effects",
    "bench_retro_and_eyelid_mask",
    "bench_all_event_types",
    "bench_classic_sampling",
    "bench_classic_godray",
    "bench_classic_film_noise",
    "bench_classic_color_and_lens",
    "bench_stress_composition",
];
pub(super) const HOTSPOT_BENCHMARK_RUNS: usize = 3;
pub(super) const CAMERA_BENCHMARK_WORKLOADS: &[(&str, BenchmarkCameras)] = &[
    (
        "opening composition · scene + UI",
        BenchmarkCameras::SceneUi,
    ),
    (
        "opening composition · scene + dialog",
        BenchmarkCameras::SceneDialog,
    ),
    (
        "opening composition · scene only",
        BenchmarkCameras::SceneOnly,
    ),
];
pub(super) const PORTABLE_BENCHMARK_SECTIONS: &[BenchmarkSection] = &[
    (
        "control · same background, portraits and dialogue; no effects",
        BASELINE_BENCHMARK_WORKLOADS,
    ),
    (
        "isolated effects · one family at a time",
        ISOLATED_EFFECT_BENCHMARK_WORKLOADS,
    ),
    (
        "particles · density, texture, fill and emitter scaling",
        PARTICLE_BENCHMARK_WORKLOADS,
    ),
    (
        "media · Opus loop/crossfade and 1080p H.264 playback",
        MEDIA_BENCHMARK_WORKLOADS,
    ),
    (
        "UI and sprites · overlays, filter and environment light control",
        UI_BENCHMARK_WORKLOADS,
    ),
    (
        "daily workloads · representative player-facing actions",
        DAILY_BENCHMARK_WORKLOADS,
    ),
    (
        "feature coverage · authored property and event combinations",
        FEATURE_BENCHMARK_WORKLOADS,
    ),
    (
        "classic camera attribution · isolated shader cost groups",
        CLASSIC_ATTRIBUTION_BENCHMARK_WORKLOADS,
    ),
    (
        "stress workload · intentionally combined peak load",
        STRESS_BENCHMARK_WORKLOADS,
    ),
];

/// Android shares the desktop workload inventory; unsupported presentation/video
/// cases remain explicit skips rather than silently disappearing from coverage.
#[cfg(feature = "publisher")]
pub(crate) fn android_benchmark_plan() -> serde_json::Value {
    use serde_json::json;
    let mut samples = Vec::new();
    let mut add = |label: &str,
                   target: Option<&str>,
                   camera: &str,
                   mode: &str,
                   runs: usize,
                   kind: &str,
                   skip: Option<&str>,
                   required: bool| {
        let mut args = vec!["perf".to_owned(), "embedded".to_owned()];
        if kind == "startup" {
            args.extend(["--startup".into(), "--runs".into(), "1".into()]);
        } else if kind == "render" {
            args.extend([
                "--seconds".into(),
                "5".into(),
                "--mode".into(),
                mode.into(),
                "--raw".into(),
            ]);
            if let Some(target) = target {
                args.extend(["--timeline".into(), target.into()]);
            }
            if camera != "runtime" {
                args.extend(["--camera".into(), camera.into()]);
            }
        }
        samples.push(
            json!({"label":label,"target":target,"camera":camera,"mode":mode,
            "runs":runs,"kind":kind,"args":args,"skip":skip,"required":required}),
        );
    };
    add(
        "startup", None, "runtime", "runtime", 7, "startup", None, false,
    );
    add(
        "opening composition · runtime sleep/wake",
        None,
        "runtime",
        "runtime",
        1,
        "render",
        None,
        false,
    );
    add(
        "opening composition · runtime composition",
        None,
        "runtime",
        "continuous",
        1,
        "render",
        None,
        false,
    );
    for (label, cameras) in CAMERA_BENCHMARK_WORKLOADS {
        add(
            label,
            None,
            cameras.id(),
            "continuous",
            1,
            "render",
            None,
            false,
        );
    }
    for (_, workloads) in PORTABLE_BENCHMARK_SECTIONS {
        for (label, target) in *workloads {
            let skip = target
                .starts_with("bench_video_")
                .then_some("Android runtime does not support video");
            let android_target = if *target == "bench_stress_composition" {
                "bench_stress_android"
            } else {
                target
            };
            let label = if *target == "bench_stress_composition" {
                "stress composition · Android, without video"
            } else {
                label
            };
            add(
                label,
                Some(android_target),
                "runtime",
                "continuous",
                benchmark_workload_runs(target),
                "render",
                skip,
                true,
            );
        }
    }
    for (label, target, skip) in [
        (
            "720p particles",
            "bench_particle_snow_256",
            Some("Android uses the actual device surface; desktop window resizing is unsupported"),
        ),
        (
            "1080p particles",
            "bench_particle_snow_256",
            Some("Android uses the actual device surface; desktop window resizing is unsupported"),
        ),
        ("runtime dialogue", "bench_representative_dialogue", None),
        ("runtime particles", "bench_particle_snow_256", None),
        ("runtime audio", "bench_audio_loop", None),
        (
            "runtime video",
            "bench_video_fullscreen",
            Some("Android runtime does not support video"),
        ),
        (
            "fullscreen stress · Android, without video",
            "bench_stress_android",
            None,
        ),
    ] {
        add(
            label,
            Some(target),
            "runtime",
            if label.starts_with("runtime ") {
                "runtime"
            } else {
                "continuous"
            },
            1,
            "render",
            skip,
            true,
        );
    }
    add(
        "packaged APK I/O",
        None,
        "runtime",
        "runtime",
        1,
        "package",
        None,
        false,
    );
    json!({"schema":1,"application_id":"moe.maincore.keine.benchmark", "engine_version":env!("CARGO_PKG_VERSION"),
        "commit":env!("KEINE_BUILD_COMMIT"), "profile":"profiling", "samples":samples})
}

pub(super) fn portable_benchmark_render_runs() -> usize {
    2 + CAMERA_BENCHMARK_WORKLOADS.len()
        + PORTABLE_BENCHMARK_SECTIONS
            .iter()
            .flat_map(|(_, workloads)| *workloads)
            .map(|(_, target)| benchmark_workload_runs(target))
            .sum::<usize>()
        + 7
}

pub(super) fn benchmark_workload_runs(target: &str) -> usize {
    if REPEATED_BENCHMARK_TARGETS.contains(&target)
        || target == "bench_baseline"
        || target.starts_with("bench_particle_")
    {
        HOTSPOT_BENCHMARK_RUNS
    } else {
        1
    }
}

pub(crate) fn benchmark_timelines(state: &State) -> Vec<(String, usize, String)> {
    let mut timelines = state
        .program
        .scene_names()
        .flat_map(|scene| {
            state
                .program
                .scene(scene)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(move |(index, action)| match action {
                    Action::StageAnimation { animation } => {
                        Some((scene.to_owned(), index, animation.id.clone()))
                    }
                    _ => None,
                })
        })
        .collect::<Vec<_>>();
    timelines.sort();
    timelines
}

pub(crate) fn resolve_benchmark_target(
    state: &State,
    target: &BenchmarkTarget,
) -> Option<(String, usize)> {
    match target {
        BenchmarkTarget::Cursor(cursor) => state
            .program
            .scene_len(&state.current_scene)
            .filter(|len| *cursor < *len)
            .map(|_| (state.current_scene.clone(), *cursor)),
        BenchmarkTarget::SceneCursor(scene, cursor) => state
            .program
            .scene_len(scene)
            .filter(|len| *cursor < *len)
            .map(|_| (scene.clone(), *cursor)),
        BenchmarkTarget::Timeline(wanted) => {
            let mut matches = benchmark_timelines(state)
                .into_iter()
                .filter(|(_, _, timeline)| timeline == wanted)
                .map(|(scene, cursor, _)| (scene, cursor));
            let resolved = matches.next();
            if matches.next().is_some() {
                log::error!(
                    target: "keine::performance",
                    "benchmark timeline {wanted:?} is ambiguous across fragments",
                );
                None
            } else {
                resolved
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::bootstrap::{open_project, validate_project};
    use keine_core::Program;
    use keine_loader::{LoaderRegistry, load_scenes_with};
    use std::path::Path;

    #[test]
    fn portable_benchmark_keeps_daily_coverage_and_stress_groups_distinct() {
        let targets = PORTABLE_BENCHMARK_SECTIONS
            .iter()
            .flat_map(|(_, workloads)| workloads.iter().map(|(_, target)| *target))
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(DAILY_BENCHMARK_WORKLOADS.len(), 3);
        assert_eq!(FEATURE_BENCHMARK_WORKLOADS.len(), 8);
        assert_eq!(CLASSIC_ATTRIBUTION_BENCHMARK_WORKLOADS.len(), 4);
        assert_eq!(STRESS_BENCHMARK_WORKLOADS.len(), 1);
        assert_eq!(REPEATED_BENCHMARK_TARGETS.len(), 11);
        assert_eq!(HOTSPOT_BENCHMARK_RUNS, 3);
        assert_eq!(benchmark_workload_runs("bench_blur_family"), 3);
        assert_eq!(benchmark_workload_runs("bench_classic_godray"), 3);
        assert_eq!(benchmark_workload_runs("bench_shared_transform_clock"), 1);
        assert_eq!(targets.len(), 71);
        assert_eq!(portable_benchmark_render_runs(), 121);
        assert_eq!(CAMERA_BENCHMARK_WORKLOADS.len(), 3);
        assert!(
            CAMERA_BENCHMARK_WORKLOADS
                .iter()
                .all(|(_, cameras)| *cameras != BenchmarkCameras::Runtime)
        );
        assert_eq!(
            CAMERA_BENCHMARK_WORKLOADS
                .iter()
                .map(|(_, cameras)| cameras.id())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            CAMERA_BENCHMARK_WORKLOADS.len()
        );
    }

    #[test]
    fn portable_benchmark_fixture_resolves_every_required_workload() {
        let loader = LoaderRegistry::default();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-benchmark");
        let project = open_project(&root, &loader).unwrap();
        let languages = loader.languages(&project.config.adapter.script).unwrap();
        let report = validate_project(&project.config, &project.content, &languages).unwrap();
        assert_eq!(
            (report.errors, report.warnings),
            (0, 0),
            "{:?}",
            report.diagnostics
        );
        assert!(project.config.assets.luts.contains_key("cinematic"));
        assert!(project.content.contains_asset(Path::new(
            &project.config.bg_path(&project.config.title_background)
        )));
        let scenes = load_scenes_with(&project.content, &languages).unwrap();
        let mut state = State::new();
        state.install_program(Program::from_scenes(
            scenes.into_iter().map(|s| (s.name, s.actions)),
        ));
        let required = PORTABLE_BENCHMARK_SECTIONS
            .iter()
            .flat_map(|(_, w)| w.iter().map(|(_, t)| *t));
        for target in required {
            let (scene, cursor) =
                resolve_benchmark_target(&state, &BenchmarkTarget::Timeline(target.into()))
                    .unwrap_or_else(|| panic!("missing or duplicate target: {target}"));
            let mut preview = State::new();
            preview.program = state.program.clone();
            preview.program_fingerprint = state.program_fingerprint;
            preview.current_scene = scene.clone();
            preview.ended = false;
            assert!(
                crate::runtime::tick::seek_editor_state(&mut preview, &scene, cursor, cursor + 1),
                "{target}"
            );
            assert!(
                preview.stage_animation.is_some(),
                "{target} is not an active native workload"
            );
        }
        // Transparent calibration art must exercise sprites, not invisible placeholders.
        let image = keine_media::decode_webp(
            &std::fs::read(root.join("assets/portrait.webp")).unwrap(),
            |size| size,
        )
        .unwrap();
        assert_eq!(image.size(), keine_media::ImageSize::new(720, 1440));
        let pixels = image.into_pixels();
        assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
        assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 255));
    }

    #[test]
    fn benchmark_missing_scene_or_out_of_range_cursor_is_rejected() {
        let mut state = State::new();
        state.insert_scene("first".into(), vec![Action::Wait { seconds: 1.0 }]);
        state.current_scene = "first".into();
        assert_eq!(
            resolve_benchmark_target(&state, &BenchmarkTarget::SceneCursor("first".into(), 0)),
            Some(("first".into(), 0))
        );
        assert_eq!(
            resolve_benchmark_target(&state, &BenchmarkTarget::SceneCursor("missing".into(), 0)),
            None
        );
        assert_eq!(
            resolve_benchmark_target(&state, &BenchmarkTarget::Cursor(1)),
            None
        );
    }
}
