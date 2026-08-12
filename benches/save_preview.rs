//! Save-card WebP encoding cost and output size at the runtime thumbnail size.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use libwebp_sys::{WebPEncodeLosslessRGB, WebPEncodeRGBA, WebPFree};

const WIDTH: usize = 480;
const HEIGHT: usize = 270;
const QUALITY: f32 = 80.0;

fn representative_rgba() -> Vec<u8> {
    let mut pixels = Vec::with_capacity(WIDTH * HEIGHT * 4);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let grain = ((x * 17 + y * 31 + x * y) & 31) as u8;
            pixels.extend_from_slice(&[
                (x * 255 / WIDTH) as u8 ^ grain,
                (y * 255 / HEIGHT) as u8,
                ((x + y) * 255 / (WIDTH + HEIGHT)) as u8,
                255,
            ]);
        }
    }
    pixels
}

fn rgba_to_rgb(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|pixel| pixel[..3].iter().copied())
        .collect()
}

fn encode_lossless(rgb: &[u8]) -> Vec<u8> {
    let mut encoded = std::ptr::null_mut();
    // SAFETY: The benchmark owns a tightly packed 480x270 RGB8 buffer for the
    // duration of the call and releases libwebp's returned allocation.
    let len = unsafe {
        WebPEncodeLosslessRGB(
            rgb.as_ptr(),
            WIDTH as i32,
            HEIGHT as i32,
            (WIDTH * 3) as i32,
            &mut encoded,
        )
    };
    assert!(len > 0 && !encoded.is_null());
    // SAFETY: libwebp returned `len` initialized bytes at `encoded`.
    let output = unsafe { std::slice::from_raw_parts(encoded, len).to_vec() };
    // SAFETY: The allocation belongs to libwebp and has been copied.
    unsafe { WebPFree(encoded.cast()) };
    output
}

fn encode_lossy(rgba: &[u8]) -> Vec<u8> {
    let mut encoded = std::ptr::null_mut();
    // SAFETY: The benchmark owns a tightly packed 480x270 RGBA8 buffer for the
    // duration of the call and releases libwebp's returned allocation.
    let len = unsafe {
        WebPEncodeRGBA(
            rgba.as_ptr(),
            WIDTH as i32,
            HEIGHT as i32,
            (WIDTH * 4) as i32,
            QUALITY,
            &mut encoded,
        )
    };
    assert!(len > 0 && !encoded.is_null());
    // SAFETY: libwebp returned `len` initialized bytes at `encoded`.
    let output = unsafe { std::slice::from_raw_parts(encoded, len).to_vec() };
    // SAFETY: The allocation belongs to libwebp and has been copied.
    unsafe { WebPFree(encoded.cast()) };
    output
}

fn bench(c: &mut Criterion) {
    let rgba = representative_rgba();
    let rgb = rgba_to_rgb(&rgba);
    let lossless_size = encode_lossless(&rgb).len();
    let lossy_size = encode_lossy(&rgba).len();
    eprintln!("save preview bytes · lossless={lossless_size} · lossy-q80={lossy_size}");

    let mut group = c.benchmark_group("save_preview");
    group.bench_function("lossless_rgb_480x270", |b| {
        b.iter(|| black_box(encode_lossless(black_box(&rgb))))
    });
    group.bench_function("lossy_rgba_q80_480x270", |b| {
        b.iter(|| black_box(encode_lossy(black_box(&rgba))))
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
