//! Algorithmic A/Bs for low-frequency runtime hotspots found by static audit.

use std::collections::HashMap;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

const BGM_COUNT: usize = 4_096;
const SPRITE_COUNT: usize = 256;

fn bgm_fixture() -> HashMap<String, String> {
    (0..BGM_COUNT)
        .map(|index| {
            (
                format!("track-{index:04}.opus"),
                format!("Display {:04}", BGM_COUNT - index),
            )
        })
        .collect()
}

fn legacy_idle_bgm_order(tracks: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut ordered = tracks
        .iter()
        .map(|(file, name)| (file.clone(), name.clone()))
        .collect::<Vec<_>>();
    ordered.sort_unstable_by(|left, right| left.1.cmp(&right.1));
    ordered
}

fn indexed_sprite_fixture() -> (Vec<String>, HashMap<String, usize>) {
    let ids = (0..SPRITE_COUNT)
        .map(|index| format!("sprite-{index:03}"))
        .collect::<Vec<_>>();
    let index = ids
        .iter()
        .cloned()
        .enumerate()
        .map(|(entity, id)| (id, entity))
        .collect();
    (ids, index)
}

fn legacy_sprite_lookup(ids: &[String]) -> usize {
    ids.iter()
        .map(|wanted| {
            ids.iter()
                .position(|candidate| candidate == wanted)
                .unwrap()
        })
        .sum()
}

fn indexed_sprite_lookup(ids: &[String], index: &HashMap<String, usize>) -> usize {
    ids.iter().map(|id| index[id]).sum()
}

#[derive(Default)]
struct TextLengthCache {
    text: String,
    characters: usize,
}

impl TextLengthCache {
    fn count(&mut self, text: &str) -> usize {
        if self.text != text {
            self.text.clear();
            self.text.push_str(text);
            self.characters = text.chars().count();
        }
        self.characters
    }
}

fn bench(c: &mut Criterion) {
    let tracks = bgm_fixture();
    let mut bgm = c.benchmark_group("runtime_hotspots/idle_bgm");
    bgm.bench_function("legacy_copy_and_sort", |b| {
        b.iter(|| black_box(legacy_idle_bgm_order(black_box(&tracks))))
    });
    bgm.bench_function("lazy_no_action", |b| b.iter(|| black_box(())));
    bgm.finish();

    let (ids, index) = indexed_sprite_fixture();
    let mut sprites = c.benchmark_group("runtime_hotspots/sprite_lookup_256");
    sprites.bench_function("legacy_nested_scan", |b| {
        b.iter(|| black_box(legacy_sprite_lookup(black_box(&ids))))
    });
    sprites.bench_function("persistent_index", |b| {
        b.iter(|| black_box(indexed_sprite_lookup(black_box(&ids), black_box(&index))))
    });
    sprites.finish();

    let text = "我🙂a".repeat(1_024);
    let mut cache = TextLengthCache::default();
    assert_eq!(cache.count(&text), text.chars().count());
    let mut dialogue = c.benchmark_group("runtime_hotspots/dialogue_chars_3072");
    dialogue.bench_function("legacy_decode_every_frame", |b| {
        b.iter(|| black_box(black_box(&text).chars().count()))
    });
    dialogue.bench_function("content_cache_hit", |b| {
        b.iter(|| black_box(cache.count(black_box(&text))))
    });
    dialogue.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
