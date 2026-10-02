use super::*;

// Original decode/resample path, retained only for repeatable release comparison.
fn original(file: &Path) -> Result<image::RgbaImage, ImageCacheError> {
    let mut reader = image::ImageReader::open(file)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = image::DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    let mut pixels = image.thumbnail(WIDTH, HEIGHT).into_rgba8();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(pixels)
}

#[test]
#[ignore = "release performance measurement"]
fn decode_thumbnail() {
    let base = std::env::temp_dir().join(format!("keine-bench-thumbnail-{}", std::process::id()));
    let generated = base.with_extension("png");
    let source = std::env::var_os("KEINE_BENCH_IMAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            image::RgbaImage::from_fn(1920, 1080, |x, y| {
                image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255])
            })
            .save(&generated)
            .unwrap();
            generated.clone()
        });
    let pixels = image::open(&source).unwrap().into_rgba8();
    let lossless = base.with_extension("webp");
    pixels.save(&lossless).unwrap();
    let lossy = base.with_extension("lossy.webp");
    fs::write(
        &lossy,
        keine_media::encode_webp_rgba(pixels.as_raw(), pixels.width(), pixels.height(), 85.)
            .unwrap(),
    )
    .unwrap();
    println!(
        "thumbnail source={} dimensions={:?}",
        source.display(),
        pixels.dimensions()
    );
    for input in [&source, &lossless, &lossy] {
        let mut before = Vec::new();
        let mut after = Vec::new();
        for _ in 0..11 {
            let now = std::time::Instant::now();
            std::hint::black_box(original(input).unwrap());
            before.push(now.elapsed().as_secs_f64() * 1000.);
            let now = std::time::Instant::now();
            std::hint::black_box(decode(input).unwrap());
            after.push(now.elapsed().as_secs_f64() * 1000.);
        }
        before.sort_by(f64::total_cmp);
        after.sort_by(f64::total_cmp);
        println!(
            "thumbnail file={} before={:.3}ms after={:.3}ms",
            input.file_name().unwrap().to_string_lossy(),
            before[5],
            after[5]
        );
    }
    fs::remove_file(lossless).unwrap();
    fs::remove_file(lossy).unwrap();
    if generated == source {
        fs::remove_file(generated).unwrap();
    }
}
