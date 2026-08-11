//! Kēne's checked-in acceptance project through filesystem and Hakutaku mounts.
//!
//! The fixture is packed once. Timed reads use the same `ContentMount` and
//! `ContentFile` contract as Bevy and native video, so the benchmark cannot
//! grow a container-specific fast path.

use criterion::{BatchSize, Criterion, Throughput, black_box, criterion_group, criterion_main};
use hakutaku_pack::{Identity, PackOptions, pack_directory};
use keine_loader::{ContentBackend, ContentMount, HakutakuArchive};
use std::fs;
use std::path::{Path, PathBuf};

const PROJECT_INPUTS: &[&str] = &[
    ".keine/compiled/program.bin",
    "assets",
    "chapters",
    "characters.json",
    "project.json",
    "scenes.json",
];
const FIXTURE_FILES: usize = 12;
const FIXTURE_BYTES: u64 = 1_126_318;
const FIXTURE_CRC32: u32 = 0x90e8_8d07;

struct Fixture {
    root: PathBuf,
    input: PathBuf,
    release: PathBuf,
    root_key: [u8; 32],
    public_key: [u8; 32],
    files: Vec<PathBuf>,
    bytes: u64,
}

impl Fixture {
    fn new() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let project = workspace.join("projects/test-project");
        let root = std::env::temp_dir().join(format!(
            "keine-content-io-{}-{}",
            env!("CARGO_PKG_VERSION"),
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove stale benchmark fixture");
        }
        let input = root.join("input");
        let release = root.join("release");
        fs::create_dir_all(&input).expect("create benchmark input");
        for relative in PROJECT_INPUTS {
            copy_tree(&project.join(relative), &input.join(relative));
        }

        let mut files = Vec::new();
        collect_files(&input, &input, &mut files);
        files.sort();
        let bytes = files
            .iter()
            .map(|path| {
                fs::metadata(input.join(path))
                    .expect("benchmark metadata")
                    .len()
            })
            .sum();
        let fingerprint = fixture_fingerprint(&input, &files);
        assert_eq!(files.len(), FIXTURE_FILES, "rename the benchmark fixture");
        assert_eq!(bytes, FIXTURE_BYTES, "rename the benchmark fixture");
        assert_eq!(fingerprint, FIXTURE_CRC32, "rename the benchmark fixture");

        let identity = Identity::generate().expect("generate benchmark identity");
        pack_directory(&PackOptions::new(&input, &release), &identity)
            .expect("pack benchmark fixture");
        Self {
            root,
            input,
            release,
            root_key: identity.root_key(),
            public_key: identity.public_key(),
            files,
            bytes,
        }
    }

    fn archive(&self) -> HakutakuArchive {
        HakutakuArchive::open_with_keys(
            &self.release.join("game.haku"),
            self.root_key,
            self.public_key,
        )
        .expect("open benchmark package")
    }

    fn filesystem_mount(&self) -> ContentMount {
        ContentMount::new(
            ContentBackend::FileSystem(self.input.clone()),
            PathBuf::new(),
        )
        .expect("mount benchmark directory")
    }

    fn hakutaku_mount(&self, archive: HakutakuArchive) -> ContentMount {
        ContentMount::new(ContentBackend::Hakutaku(archive), PathBuf::new())
            .expect("mount benchmark package")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture_fingerprint(root: &Path, files: &[PathBuf]) -> u32 {
    let mut fingerprint = crc32fast::Hasher::new();
    for path in files {
        let bytes = fs::read(root.join(path)).expect("read benchmark fingerprint input");
        let canonical_path = path
            .iter()
            .map(|component| component.to_str().expect("benchmark path is UTF-8"))
            .collect::<Vec<_>>()
            .join("/");
        fingerprint.update(&(canonical_path.len() as u64).to_le_bytes());
        fingerprint.update(canonical_path.as_bytes());
        fingerprint.update(&(bytes.len() as u64).to_le_bytes());
        fingerprint.update(&bytes);
    }
    fingerprint.finalize()
}

fn copy_tree(source: &Path, destination: &Path) {
    if source.is_dir() {
        fs::create_dir_all(destination).expect("create benchmark directory");
        let mut entries = fs::read_dir(source)
            .expect("read benchmark source directory")
            .collect::<std::io::Result<Vec<_>>>()
            .expect("collect benchmark source directory");
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            copy_tree(&entry.path(), &destination.join(entry.file_name()));
        }
    } else {
        fs::create_dir_all(destination.parent().expect("benchmark file has parent"))
            .expect("create benchmark file parent");
        fs::copy(source, destination).expect("copy benchmark file");
    }
}

fn collect_files(root: &Path, directory: &Path, output: &mut Vec<PathBuf>) {
    let mut entries = fs::read_dir(directory)
        .expect("read benchmark fixture")
        .collect::<std::io::Result<Vec<_>>>()
        .expect("collect benchmark fixture");
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, output);
        } else {
            output.push(
                path.strip_prefix(root)
                    .expect("fixture file remains below root")
                    .to_owned(),
            );
        }
    }
}

fn read_mix(mount: &ContentMount, files: &[PathBuf]) -> usize {
    files
        .iter()
        .map(|path| {
            let mut file = mount.open_file(path).expect("open benchmark asset");
            let mut bytes = Vec::with_capacity(
                usize::try_from(file.len().expect("benchmark asset length"))
                    .expect("benchmark asset fits memory"),
            );
            file.read_remaining_into(&mut bytes)
                .expect("read benchmark asset");
            black_box(bytes.as_slice());
            bytes.len()
        })
        .sum()
}

fn bench(c: &mut Criterion) {
    let fixture = Fixture::new();
    let mut group = c.benchmark_group("content_io/test_project_v1");

    group.bench_function("open_and_index_hakutaku", |b| {
        b.iter(|| {
            let archive = fixture.archive();
            black_box(archive.contains_file(&fixture.files[0]));
        });
    });

    group.throughput(Throughput::Bytes(fixture.bytes));
    let filesystem = fixture.filesystem_mount();
    group.bench_function("filesystem_mix", |b| {
        b.iter(|| black_box(read_mix(&filesystem, &fixture.files)));
    });

    group.bench_function("hakutaku_fresh_runtime_cache_mix", |b| {
        b.iter_batched(
            || fixture.archive(),
            |archive| {
                let mount = fixture.hakutaku_mount(archive);
                black_box(read_mix(&mount, &fixture.files));
            },
            BatchSize::PerIteration,
        );
    });

    let hakutaku = fixture.hakutaku_mount(fixture.archive());
    group.bench_function("hakutaku_warm_runtime_cache_mix", |b| {
        b.iter(|| black_box(read_mix(&hakutaku, &fixture.files)));
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
