//! Real-project adapter and source-compilation baseline for the local official
//! LetsGal Studio sample. The commercial sample is deliberately not committed.

use std::path::{Path, PathBuf};

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use keine_core::Program;
use keine_loader::{AdaptedProject, LoaderRegistry, load_scenes};

const EXPECTED_ACTIONS: u64 = 896;

fn project_root() -> PathBuf {
    let configured = std::env::var_os("KEINE_LETSGAL_PROJECT").map(PathBuf::from);
    let default = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects/letsgal");
    let root = configured.unwrap_or(default);
    root.canonicalize().unwrap_or_else(|error| {
        panic!(
            "LetsGal sample is unavailable at {} ({error}); copy the official Studio template to projects/letsgal or set KEINE_LETSGAL_PROJECT",
            root.display()
        )
    })
}

fn open_project(registry: &LoaderRegistry, root: &Path) -> AdaptedProject {
    registry
        .open_project(root)
        .expect("LetsGal adapter detection should succeed")
        .expect("the project should be recognized as LetsGal")
}

fn open_parse_and_fingerprint(registry: &LoaderRegistry, root: &Path) -> (u64, u64) {
    let project = open_project(registry, root);
    let scenes = load_scenes(&project.content).expect("LetsGal scenes should compile");
    let actions = scenes
        .iter()
        .map(|scene| scene.actions.len() as u64)
        .sum::<u64>();
    let fingerprint = Program::fingerprint_scenes(
        scenes
            .iter()
            .map(|scene| (scene.name.as_str(), scene.actions.as_slice())),
    );
    (fingerprint, actions)
}

fn bench(c: &mut Criterion) {
    let root = project_root();
    let registry = LoaderRegistry::default();
    let project = open_project(&registry, &root);
    let (_, action_count) = open_parse_and_fingerprint(&registry, &root);
    assert_eq!(action_count, EXPECTED_ACTIONS);

    let mut open = c.benchmark_group("letsgal_project/open");
    open.bench_function("adapter_and_manifest", |b| {
        b.iter(|| black_box(open_project(&registry, &root)));
    });
    open.finish();

    let mut parse = c.benchmark_group("letsgal_project/parse");
    parse.throughput(Throughput::Elements(EXPECTED_ACTIONS));
    parse.bench_function("scenes_896_actions", |b| {
        b.iter(|| black_box(load_scenes(&project.content).expect("compile scenes")));
    });
    parse.finish();

    let mut complete = c.benchmark_group("letsgal_project/complete");
    complete.throughput(Throughput::Elements(EXPECTED_ACTIONS));
    complete.bench_function("open_parse_fingerprint", |b| {
        b.iter(|| black_box(open_parse_and_fingerprint(&registry, &root)));
    });
    complete.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
