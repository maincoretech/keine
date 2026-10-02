use super::*;

#[test]
#[ignore = "release performance measurement"]
fn browser_cache() {
    let index = Arc::new(AuthoringIndex {
        assets: (0..5000)
            .map(|i| AssetEntry {
                kind: AssetKind::Background,
                id: format!("asset{i:05}"),
                path: PathBuf::from(format!("assets/bg/{i}.webp")),
                tags: vec!["interior".into()],
                exists: true,
                reference_count: 0,
            })
            .collect(),
        ..Default::default()
    });
    let mut cache = Cache::default();
    let query = AssetQuery::default();
    cache.resolve(&index, &query, false, 3, false, None);
    let mut timings = Vec::new();
    for _ in 0..11 {
        let now = std::time::Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(cache.resolve(&index, &query, false, 3, false, None));
        }
        timings.push(now.elapsed().as_secs_f64());
    }
    timings.sort_by(f64::total_cmp);
    println!("browser assets=5000 cached preparation={:.6}ms", timings[5]);
}
