use std::path::Path;
use std::sync::OnceLock;

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use hakutaku_pack::{Identity, PackOptions, pack_directory};
use keine_loader::{ContentBackend, ContentMount, HakutakuArchive, OpenPolicy};

const DIRECTORY_COUNT: usize = 64;
const FILES_PER_DIRECTORY: usize = 128;

struct LookupFixture {
    archive: HakutakuArchive,
    filesystem: ContentMount,
}

fn fixture() -> &'static LookupFixture {
    static FIXTURE: OnceLock<LookupFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let root =
            std::env::temp_dir().join(format!("keine-content-lookup-bench-{}", std::process::id()));
        let input = root.join("input");
        let release = root.join("release");
        for directory in 0..DIRECTORY_COUNT {
            let path = input.join(format!("group-{directory:03}"));
            std::fs::create_dir_all(&path).unwrap();
            for file in 0..FILES_PER_DIRECTORY {
                std::fs::write(path.join(format!("file-{file:03}.bin")), [file as u8]).unwrap();
            }
        }
        let identity = Identity::generate().unwrap();
        pack_directory(&PackOptions::new(&input, &release), &identity).unwrap();
        let archive = HakutakuArchive::open_with_keys(
            &release.join("game.haku"),
            identity.root_key(),
            identity.public_key(),
            OpenPolicy::TrustFirstRelease,
        )
        .unwrap();
        let filesystem = ContentMount::new(ContentBackend::FileSystem(input), "").unwrap();
        LookupFixture {
            archive,
            filesystem,
        }
    })
}

fn bench(c: &mut Criterion) {
    let fixture = fixture();
    let path = Path::new("group-031");
    let file = Path::new("group-031/file-063.bin");
    let mut group = c.benchmark_group("content_lookup");
    group.sample_size(30);
    group.throughput(Throughput::Elements(
        (DIRECTORY_COUNT * FILES_PER_DIRECTORY) as u64,
    ));
    group.bench_function("hakutaku_read_directory_8192", |b| {
        b.iter(|| black_box(fixture.archive.read_directory(black_box(path))));
    });
    group.throughput(Throughput::Elements(1));
    group.bench_function("hakutaku_layer_contains", |b| {
        b.iter(|| black_box(fixture.archive.contains_file(black_box(file))));
    });
    group.bench_function("filesystem_layer_open", |b| {
        b.iter(|| black_box(fixture.filesystem.open_file(black_box(file)).unwrap()));
    });
    group.bench_function("filesystem_layer_contains", |b| {
        b.iter(|| black_box(fixture.filesystem.contains_file(black_box(file))));
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
