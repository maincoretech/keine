//! Native EYS parsing and Program construction, plus the historical WebGAL
//! and compiled `program.bin` baselines. Optional real corpus stays read-only.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod metrics;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use keine_core::config::AssetSourceConfig;
use keine_core::{Program, State, step};
use keine_loader::{
    CompiledSceneV1, DiagnosticLevel, EncodeInput, IR_SCHEMA_VERSION, ProgramMetadataV1, decode,
    encode, load_project, load_scenes, parse_native_document, parse_native_scenes,
};
use std::hint::black_box;

const ACTION_COUNT: usize = 100_000;

fn fixture() -> &'static Path {
    static FIXTURE: OnceLock<PathBuf> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("keine-bench-{}", std::process::id()));
        let script = dir.join("scripts/bench.txt");
        if !script.exists() {
            std::fs::create_dir_all(dir.join("scripts")).unwrap();
            std::fs::create_dir_all(dir.join("assets")).unwrap();
            let mut content = String::with_capacity(ACTION_COUNT * 14);
            for i in 0..ACTION_COUNT {
                content.push_str("comment:bench-");
                content.push_str(&i.to_string());
                content.push_str(";\n");
            }
            std::fs::write(&script, content).unwrap();
        }
        dir
    })
}

fn load_and_build() -> u64 {
    build_program().fingerprint()
}

fn build_program() -> Program {
    let project = load_project(
        fixture(),
        &[AssetSourceConfig {
            path: ".".to_string(),
            format: "fs".to_string(),
        }],
    )
    .expect("load project");
    let scenes = load_scenes(&project).expect("load scenes");
    let actions = scenes
        .iter()
        .map(|scene| (scene.name.clone(), scene.actions.clone()))
        .collect::<Vec<_>>();
    Program::from_scenes(actions)
}

fn prepared_program() -> &'static Program {
    static PROGRAM: OnceLock<Program> = OnceLock::new();
    PROGRAM.get_or_init(build_program)
}

fn apply_prebuilt_program(program: Program) -> (u64, usize) {
    let mut state = State::new();
    state.install_program(program);
    state.current_scene = "bench".to_owned();
    state.ended = false;
    let _ = step::step(&mut state);
    (state.program_fingerprint, state.cursor)
}

fn compiled_fixture() -> &'static [u8] {
    static COMPILED: OnceLock<Vec<u8>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        let project = load_project(
            fixture(),
            &[AssetSourceConfig {
                path: ".".to_string(),
                format: "fs".to_string(),
            }],
        )
        .expect("load project");
        let scenes = load_scenes(&project).expect("load scenes");
        let program = Program::from_scenes(
            scenes
                .iter()
                .map(|scene| (scene.name.clone(), scene.actions.clone())),
        );
        encode(&EncodeInput {
            scenes: scenes.iter().map(CompiledSceneV1::from_loaded).collect(),
            metadata: ProgramMetadataV1 {
                compiler_version: "benchmark".to_string(),
                engine_version: "benchmark".to_string(),
                source_adapter: "webgal".to_string(),
                scene_count: scenes.len() as u32,
                action_count: ACTION_COUNT as u64,
                source_manifest_hash: 0,
            },
            fingerprint: program.fingerprint(),
        })
        .expect("encode compiled fixture")
    })
}

fn decode_take_and_build() -> u64 {
    let decoded = decode(compiled_fixture(), IR_SCHEMA_VERSION).expect("decode compiled fixture");
    Program::from_scenes(
        decoded
            .scenes
            .into_iter()
            .map(|scene| (scene.name, scene.actions)),
    )
    .fingerprint()
}

/// Models the former packaged startup path: keep the decoded artifact alive
/// while cloning its complete action tree into the runtime Program.
fn decode_clone_and_build() -> u64 {
    let decoded = decode(compiled_fixture(), IR_SCHEMA_VERSION).expect("decode compiled fixture");
    Program::from_scenes(
        decoded
            .scenes
            .iter()
            .map(|scene| (scene.name.clone(), scene.actions.clone())),
    )
    .fingerprint()
}

fn bench(c: &mut Criterion) {
    if metrics::run() {
        return;
    }
    bench_native(c);
    let mut group = c.benchmark_group("program_load");
    group.throughput(Throughput::Elements(ACTION_COUNT as u64));
    group.bench_function(format!("parse_and_build_{ACTION_COUNT}"), |b| {
        b.iter(|| black_box(load_and_build()));
    });
    group.bench_function(format!("decode_clone_and_build_{ACTION_COUNT}"), |b| {
        b.iter(|| black_box(decode_clone_and_build()));
    });
    group.bench_function(format!("decode_take_and_build_{ACTION_COUNT}"), |b| {
        b.iter(|| black_box(decode_take_and_build()));
    });
    group.bench_function(format!("frame_apply_prebuilt_{ACTION_COUNT}"), |b| {
        b.iter_batched(
            || prepared_program().clone(),
            |program| black_box(apply_prebuilt_program(program)),
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

fn bench_native(c: &mut Criterion) {
    let mut text = String::from("scene bench {\n");
    let mut mixed = String::from("scene bench {\nlet counter = 0");
    for index in 0..ACTION_COUNT {
        if index != 0 {
            text.push_str(",\n");
        }
        text.push_str(&format!("\"Line {index}: 你好，世界。\""));
        if index % 2 == 0 {
            mixed.push_str(",\ncounter = (counter + 1) * 2 % 1000");
        } else {
            mixed.push_str(",\n\"Line ${counter}: 你好，世界。\"");
        }
    }
    text.push_str("\n}");
    mixed.push_str("\n}");
    let mut sources = vec![("text_100000", text), ("mixed_100001", mixed)];
    if let Some(corpus) = corpus() {
        sources.push(("corpus", corpus));
    }
    let mut group = c.benchmark_group("native_parse");
    for (name, source) in &sources {
        let scenes = parse_native_scenes(source);
        assert!(
            scenes
                .iter()
                .flat_map(|scene| &scene.report.diagnostics)
                .all(|diagnostic| diagnostic.level != DiagnosticLevel::Error),
            "{name}: invalid fixture"
        );
        let count: usize = scenes.iter().map(|scene| scene.report.actions.len()).sum();
        eprintln!(
            "NATIVE_INPUT {name}: {} bytes, {} scenes, {count} lowered actions",
            source.len(),
            scenes.len()
        );
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_function(*name, |b| {
            b.iter(|| black_box(parse_native_scenes(black_box(source))))
        });
    }
    group.finish();
    let mut group = c.benchmark_group("native_build");
    group.throughput(Throughput::Elements((ACTION_COUNT + 1) as u64));
    group.bench_function("mixed_100001", |b| {
        b.iter(|| {
            let scenes = parse_native_scenes(black_box(&sources[1].1));
            black_box(Program::from_scenes(scenes.into_iter().map(|scene| {
                (scene.name.expect("native scene name"), scene.report.actions)
            })))
        })
    });
    group.finish();
    if let Some((_, corpus)) = sources.iter().find(|(name, _)| *name == "corpus") {
        let mut group = c.benchmark_group("native_document");
        group.throughput(Throughput::Bytes(corpus.len() as u64));
        group.bench_function("corpus", |b| {
            b.iter(|| black_box(parse_native_document(black_box(corpus))))
        });
        group.finish();
    }
}

fn corpus() -> Option<String> {
    let directory = std::env::var_os("KEINE_NATIVE_SOURCE_DIR")?;
    let mut paths = std::fs::read_dir(directory)
        .expect("native source directory")
        .map(|entry| entry.expect("source entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "shou")
        })
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty(), "native corpus must contain .shou files");
    Some(
        paths
            .iter()
            .map(|path| std::fs::read_to_string(path).expect("native source"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

criterion_group!(benches, bench);
criterion_main!(benches);
