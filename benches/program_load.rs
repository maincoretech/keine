//! Script parse + Program construction throughput for a synthetic 100k-action
//! WebGAL directory project. Baseline for the compiled `program.bin` path.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use keine_core::Program;
use keine_core::config::AssetSourceConfig;
use keine_loader::{load_project, load_scenes};

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
    let program = Program::from_scenes(actions);
    program.fingerprint()
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("program_load");
    group.throughput(Throughput::Elements(ACTION_COUNT as u64));
    group.bench_function(format!("parse_and_build_{ACTION_COUNT}"), |b| {
        b.iter(|| black_box(load_and_build()));
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
