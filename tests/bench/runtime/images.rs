//! Opt-in measurement through the production overlay reader and WebP decoder.
use super::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Instant;

use bevy::asset::io::AssetSourceId;
use hakutaku_pack::{Identity, PackOptions, pack_directory};
use keine_loader::{ContentBackend, ContentMount, HakutakuArchive};

fn files(root: &Path, result: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            files(&entry.path(), result);
        } else if kind.is_file() && is_webp(&entry.path().to_string_lossy()) {
            result.push(entry.path());
        }
    }
}

fn role(path: &Path) -> ImageRole {
    if path.components().any(|part| {
        matches!(
            part.as_os_str().to_str(),
            Some("figures" | "characters" | "portraits")
        )
    }) {
        ImageRole::FIGURE
    } else {
        ImageRole::BACKGROUND
    }
}

#[test]
#[ignore = "set KEINE_IMAGE_BENCH to a read-only WebP asset directory"]
fn mounted_image_reads() {
    let root = PathBuf::from(std::env::var_os("KEINE_IMAGE_BENCH").expect("image directory"));
    let mut paths = Vec::new();
    files(&root, &mut paths);
    paths.sort_by_cached_key(|path| (std::fs::metadata(path).unwrap().len(), path.clone()));
    assert!(!paths.is_empty());
    let count = paths.len().min(40);
    let selected: Vec<_> = (0..count)
        .map(|index| &paths[index * (paths.len() - 1) / (count.max(2) - 1)])
        .collect();
    let fixture = tempfile::tempdir().unwrap();
    let input = fixture.path().join("input");
    let release = fixture.path().join("release");
    std::fs::create_dir(&input).unwrap();
    let mut logical = 0;
    for (index, path) in selected.iter().enumerate() {
        logical += std::fs::copy(path, input.join(format!("image-{index:04}.webp"))).unwrap();
    }
    let identity = Identity::generate().unwrap();
    pack_directory(&PackOptions::new(&input, &release), &identity).unwrap();
    let keys = identity.runtime_key_material().unwrap();
    println!(
        "image_fixture available={} selected={count} bytes={logical}",
        paths.len()
    );
    let mut expected = HashMap::new();
    for backend in ["filesystem", "hakutaku"] {
        for pass in 0..5 {
            let backend_value = if backend == "filesystem" {
                ContentBackend::FileSystem(input.clone())
            } else {
                ContentBackend::Hakutaku(
                    HakutakuArchive::open_with_keys(
                        &release.join("game.haku"),
                        keys.root_key(),
                        keys.public_key,
                        hakutaku_core::OpenPolicy::TrustFirstRelease,
                    )
                    .unwrap(),
                )
            };
            let mount = ContentMount::new(backend_value, "").unwrap();
            let source = crate::runtime::asset_reader::overlay_source(vec![mount]).build(
                AssetSourceId::Default,
                false,
                false,
            );
            let mut totals = [(0.0, 0.0, 0.0, 0_usize, 0_usize); 2];
            for (index, path) in selected.iter().enumerate() {
                let image_role = role(path);
                let group = usize::from(image_role.figure);
                let started = Instant::now();
                let bytes = futures_lite::future::block_on(async {
                    let logical_path = format!("image-{index:04}.webp");
                    let mut reader = source
                        .reader()
                        .read(Path::new(&logical_path))
                        .await
                        .unwrap();
                    read_webp_input(&mut *reader).await.unwrap()
                });
                let read_ms = started.elapsed().as_secs_f64() * 1000.0;
                let started = Instant::now();
                // Same target selection, libwebp and Image::new ownership transfer as NativeWebpLoader.
                let image =
                    decode_webp(&bytes, |original| target_size(image_role, original, 1080.0))
                        .unwrap();
                let decode_ms = started.elapsed().as_secs_f64() * 1000.0;
                let started = Instant::now();
                std::hint::black_box(crate::scene::lighting::sample(&image));
                let prepare_ms = started.elapsed().as_secs_f64() * 1000.0;
                let pixels = image.data.as_ref().unwrap();
                let mut hash = DefaultHasher::new();
                pixels.hash(&mut hash);
                let signature = (image.size(), hash.finish());
                if let Some(previous) = expected.insert(index, signature) {
                    assert_eq!(signature, previous);
                }
                println!(
                    "image_item backend={backend} pass={pass} index={index} role={} read_ms={read_ms:.6} decode_ms={decode_ms:.6} prepare_ms={prepare_ms:.6} encoded={} rgba={} width={} height={} hash={}",
                    if group == 1 { "figure" } else { "background" },
                    bytes.len(),
                    pixels.len(),
                    image.width(),
                    image.height(),
                    signature.1
                );
                totals[group].0 += read_ms;
                totals[group].1 += decode_ms;
                totals[group].2 += prepare_ms;
                totals[group].3 += pixels.len();
                totals[group].4 += 1;
            }
            for (group, (read, decode, prepare, rgba, files)) in totals.into_iter().enumerate() {
                if files > 0 {
                    println!(
                        "image_batch backend={backend} pass={pass} role={} files={files} read_ms={read:.6} decode_ms={decode:.6} prepare_ms={prepare:.6} rgba={rgba}",
                        if group == 1 { "figure" } else { "background" }
                    );
                }
            }
        }
    }
}
