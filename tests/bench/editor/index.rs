use super::*;

#[test]
#[ignore = "release performance measurement"]
fn index_and_filter() {
    let mut base = AuthoringIndex::default();
    for file in 0..10 {
        let path = PathBuf::from(format!("scripts/{file}.shou"));
        let source = format!(
            "scene scene{file} {{\n{}\n}}",
            "  \"对白 benchmark\",\n".repeat(500)
        );
        index_source(
            &path,
            &source,
            &mut base,
            &mut HashMap::new(),
            &HashSet::new(),
        );
    }
    base.assets = (0..5000)
        .map(|i| AssetEntry {
            kind: AssetKind::Background,
            id: format!("asset{i:05}"),
            path: PathBuf::from(format!("assets/bg/asset{i:05}.webp")),
            tags: vec!["interior".into()],
            exists: true,
            reference_count: 0,
        })
        .collect();
    for changed in [1, 10] {
        let sources = (0..changed)
            .map(|i| {
                (
                    PathBuf::from(format!("scripts/{i}.shou")),
                    format!(
                        "scene scene{i} {{\n{}\n}}",
                        "  \"对白 changed\",\n".repeat(500)
                    ),
                )
            })
            .collect();
        let mut timings = Vec::new();
        for _ in 0..11 {
            let now = std::time::Instant::now();
            std::hint::black_box(base.with_sources(&sources));
            timings.push(now.elapsed().as_secs_f64() * 1000.);
        }
        timings.sort_by(f64::total_cmp);
        println!(
            "index dialogues=5000 assets=5000 changed_files={changed} {:.3}ms",
            timings[5]
        );
    }
    for search in ["", "interior 123"] {
        let query = AssetQuery {
            search: search.into(),
            ..Default::default()
        };
        let mut timings = Vec::new();
        for _ in 0..11 {
            let now = std::time::Instant::now();
            for _ in 0..100 {
                std::hint::black_box(query.results(&base.assets));
            }
            timings.push(now.elapsed().as_secs_f64() * 10.);
        }
        timings.sort_by(f64::total_cmp);
        println!("filter assets=5000 search={search:?} {:.3}ms", timings[5]);
    }
}
