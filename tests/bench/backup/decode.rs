//! Backup-envelope decode cost for the owned V2 representation and the
//! in-place borrowed representation used by import.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use serde::{Deserialize, Serialize};

const PAYLOAD_BYTES: usize = 32 * 1024 * 1024;

#[derive(Serialize)]
struct EncodedBundle {
    version: u32,
    files: Vec<EncodedFile>,
}

#[derive(Serialize)]
struct EncodedFile {
    name: String,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
struct OwnedBundle {
    version: u32,
    files: Vec<OwnedFile>,
}

#[derive(Deserialize)]
struct OwnedFile {
    name: String,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
struct BorrowedBundle<'a> {
    version: u32,
    #[serde(borrow)]
    files: Vec<BorrowedFile<'a>>,
}

#[derive(Deserialize)]
struct BorrowedFile<'a> {
    #[serde(borrow)]
    name: &'a str,
    #[serde(borrow)]
    bytes: &'a [u8],
}

fn encoded_backup() -> Vec<u8> {
    postcard::to_stdvec(&EncodedBundle {
        version: 2,
        files: vec![EncodedFile {
            name: "slot_1.keine".to_owned(),
            bytes: vec![0x5a; PAYLOAD_BYTES],
        }],
    })
    .expect("representative backup must encode")
}

fn bench(c: &mut Criterion) {
    let encoded = encoded_backup();
    let mut group = c.benchmark_group("backup_decode");
    group.sample_size(20);
    group.throughput(Throughput::Bytes(PAYLOAD_BYTES as u64));
    group.bench_with_input(
        BenchmarkId::new("owned_v2", PAYLOAD_BYTES),
        &encoded,
        |b, bytes| {
            b.iter(|| {
                let decoded: OwnedBundle = postcard::from_bytes(black_box(bytes)).unwrap();
                black_box((
                    decoded.version,
                    decoded.files[0].name.len(),
                    decoded.files[0].bytes.len(),
                ))
            });
        },
    );
    group.bench_with_input(
        BenchmarkId::new("borrowed_v2", PAYLOAD_BYTES),
        &encoded,
        |b, bytes| {
            b.iter(|| {
                let decoded: BorrowedBundle<'_> = postcard::from_bytes(black_box(bytes)).unwrap();
                black_box((
                    decoded.version,
                    decoded.files[0].name.len(),
                    decoded.files[0].bytes.len(),
                ))
            });
        },
    );
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
