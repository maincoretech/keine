//! Build-time application icon derivation, shared by publisher and Editor export.
//! Not enabled in the shipping runtime. PNG/WebP inputs stay allocation-bounded.
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::Path;

use image::{ImageEncoder, ImageFormat, ImageReader, RgbaImage, imageops};

pub const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const SIZES: &[u32] = &[
    16, 24, 32, 48, 64, 72, 96, 128, 144, 192, 256, 264, 512, 1024,
];

pub struct IconSet {
    png: BTreeMap<u32, Vec<u8>>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

impl IconSet {
    pub fn read(reader: impl Read) -> io::Result<Self> {
        let mut bytes = Vec::new();
        reader.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_SOURCE_BYTES {
            return Err(invalid("application icon exceeds 64 MiB"));
        }
        Self::decode(&bytes)
    }

    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() as u64 > MAX_SOURCE_BYTES {
            return Err(invalid("application icon exceeds 64 MiB"));
        }
        let rgba = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(4096);
            limits.max_image_height = Some(4096);
            limits.max_alloc = Some(64 * 1024 * 1024);
            reader.limits(limits);
            reader
                .decode()
                .map_err(|error| invalid(error.to_string()))?
                .to_rgba8()
        } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
            let mut square = true;
            let decoded = crate::decode_webp(bytes, |size| {
                square = size.width == size.height && size.width <= 4096;
                crate::ImageSize::new(size.width.min(1024), size.height.min(1024))
            })?;
            if !square {
                return Err(invalid(
                    "application icon must be square and at most 4096 px",
                ));
            }
            let size = decoded.size();
            RgbaImage::from_raw(size.width, size.height, decoded.into_pixels())
                .ok_or_else(|| invalid("invalid icon pixels"))?
        } else {
            return Err(invalid("application icon must be a PNG or WebP"));
        };
        if rgba.width() != rgba.height() || rgba.width() < 32 {
            return Err(invalid(
                "application icon must be square and at least 32 px",
            ));
        }
        // Filter premultiplied pixels. Hidden RGB in transparent source corners
        // must not bleed a colored fringe into small icons.
        let premultiplied = image::Rgba32FImage::from_fn(rgba.width(), rgba.height(), |x, y| {
            let p = rgba.get_pixel(x, y);
            let alpha = p[3] as f32 / 255.;
            image::Rgba([
                p[0] as f32 / 255. * alpha,
                p[1] as f32 / 255. * alpha,
                p[2] as f32 / 255. * alpha,
                alpha,
            ])
        });
        let mut png = BTreeMap::new();
        for &size in SIZES {
            let resized =
                imageops::resize(&premultiplied, size, size, imageops::FilterType::Lanczos3);
            let pixels = RgbaImage::from_fn(size, size, |x, y| {
                let p = resized.get_pixel(x, y);
                let alpha = p[3].clamp(0., 1.);
                let channel = |n: usize| {
                    if alpha > 0. {
                        (p[n] / alpha * 255.).round().clamp(0., 255.) as u8
                    } else {
                        0
                    }
                };
                image::Rgba([
                    channel(0),
                    channel(1),
                    channel(2),
                    (alpha * 255.).round() as u8,
                ])
            });
            let mut bytes = Vec::new();
            image::codecs::png::PngEncoder::new(&mut bytes)
                .write_image(pixels.as_raw(), size, size, image::ExtendedColorType::Rgba8)
                .map_err(|error| invalid(error.to_string()))?;
            png.insert(size, bytes);
        }
        Ok(Self { png })
    }

    pub fn png(&self, size: u32) -> &[u8] {
        &self.png[&size]
    }

    pub fn ico(&self) -> Vec<u8> {
        let sizes = [16, 24, 32, 48, 64, 128, 256];
        let mut result = vec![0, 0, 1, 0, 7, 0];
        let mut offset = 6 + 16 * sizes.len() as u32;
        for size in sizes {
            let png = self.png(size);
            result.extend_from_slice(&[size as u8, size as u8, 0, 0, 1, 0, 32, 0]);
            result.extend_from_slice(&(png.len() as u32).to_le_bytes());
            result.extend_from_slice(&offset.to_le_bytes());
            offset += png.len() as u32;
        }
        for size in sizes {
            result.extend_from_slice(self.png(size));
        }
        result
    }

    pub fn icns(&self) -> Vec<u8> {
        let mut result = b"icns\0\0\0\0".to_vec();
        for (kind, size) in [
            (b"icp4", 16),
            (b"icp5", 32),
            (b"icp6", 64),
            (b"ic07", 128),
            (b"ic08", 256),
            (b"ic09", 512),
            (b"ic10", 1024),
            (b"ic11", 32),
            (b"ic12", 64),
            (b"ic13", 256),
            (b"ic14", 512),
        ] {
            let png = self.png(size);
            result.extend_from_slice(kind);
            result.extend_from_slice(&((png.len() + 8) as u32).to_be_bytes());
            result.extend_from_slice(png);
        }
        let length = result.len() as u32;
        result[4..8].copy_from_slice(&length.to_be_bytes());
        result
    }

    pub fn write(&self, output: &Path) -> io::Result<()> {
        fs::create_dir_all(output)?;
        fs::write(output.join("keine.ico"), self.ico())?;
        fs::write(output.join("keine.icns"), self.icns())?;
        for size in [256, 512] {
            fs::write(output.join(format!("keine-{size}.png")), self.png(size))?;
        }
        let res = output.join("android");
        for (density, size) in [
            ("mdpi", 48),
            ("hdpi", 72),
            ("xhdpi", 96),
            ("xxhdpi", 144),
            ("xxxhdpi", 192),
        ] {
            let dir = res.join(format!("mipmap-{density}"));
            fs::create_dir_all(&dir)?;
            fs::write(dir.join("ic_launcher.png"), self.png(size))?;
        }
        for (path, data) in [
            ("drawable-nodpi/ic_launcher_logo.png", self.png(264)),
            ("mipmap-anydpi-v26/ic_launcher.xml", ADAPTIVE.as_bytes()),
            ("drawable/ic_launcher_foreground.xml", FOREGROUND.as_bytes()),
            ("values/colors.xml", COLORS.as_bytes()),
        ] {
            let path = res.join(path);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, data)?;
        }
        Ok(())
    }
}

const ADAPTIVE: &str = r#"<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android"><background android:drawable="@color/ic_launcher_background"/><foreground android:drawable="@drawable/ic_launcher_foreground"/></adaptive-icon>"#;
const FOREGROUND: &str = r#"<layer-list xmlns:android="http://schemas.android.com/apk/res/android"><item android:width="66dp" android:height="66dp" android:gravity="center"><bitmap android:src="@drawable/ic_launcher_logo" android:gravity="fill"/></item></layer-list>"#;
const COLORS: &str =
    r##"<resources><color name="ic_launcher_background">#20392F</color></resources>"##;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transparent_rgb_cannot_bleed_and_oversized_sources_are_rejected() {
        let rgba = RgbaImage::from_fn(32, 32, |x, y| {
            if (8..24).contains(&x) && (8..24).contains(&y) {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 0])
            }
        });
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(rgba.as_raw(), 32, 32, image::ExtendedColorType::Rgba8)
            .unwrap();
        let set = IconSet::decode(&png).unwrap();
        let tiny = image::load_from_memory(set.png(16)).unwrap().to_rgba8();
        assert!(tiny.pixels().any(|p| p[3] > 0 && p[3] < 255));
        assert!(tiny.pixels().all(|p| p[2] == 0));
        // A valid PNG exceeding the dimension limit is rejected, independently
        // of corrupt headers or checksum failures.
        let mut oversized = Vec::new();
        image::codecs::png::PngEncoder::new(&mut oversized)
            .write_image(&vec![0; 4097 * 4], 4097, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        assert!(IconSet::decode(&oversized).is_err());
        assert!(IconSet::read(io::repeat(0).take(MAX_SOURCE_BYTES + 1)).is_err());
    }
    #[test]
    fn png_and_webp_derive_identical_containers_and_reject_invalid_geometry() {
        let rgba = vec![255u8; 64 * 64 * 4];
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&rgba, 64, 64, image::ExtendedColorType::Rgba8)
            .unwrap();
        let webp = crate::encode_webp_rgba(&rgba, 64, 64, 100.).unwrap();
        let from_png = IconSet::decode(&png).unwrap();
        let from_webp = IconSet::decode(&webp).unwrap();
        assert_eq!(from_png.ico(), from_webp.ico());
        assert_eq!(from_png.icns(), from_webp.icns());
        assert_eq!(&from_png.ico()[..6], &[0, 0, 1, 0, 7, 0]);
        assert_eq!(&from_png.icns()[..4], b"icns");
        assert!(IconSet::decode(b"not an icon").is_err());
        let mut rectangle = Vec::new();
        image::codecs::png::PngEncoder::new(&mut rectangle)
            .write_image(
                &vec![0; 32 * 64 * 4],
                32,
                64,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        assert!(IconSet::decode(&rectangle).is_err());
    }
}
