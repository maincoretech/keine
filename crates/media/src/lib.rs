//! Small, allocation-bounded native codec boundaries shared by the runtime
//! and in-process fuzz targets.

#![warn(unused_crate_dependencies)]

use std::io;

use libwebp_sys::{
    VP8StatusCode, WEBP_CSP_MODE, WebPDecode, WebPDecoderConfig, WebPEncodeRGBA, WebPFree,
    WebPFreeDecBuffer, WebPGetFeatures, WebPInitDecoderConfig, WebPRGBABuffer,
};

pub const MAX_WEBP_FILE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_WEBP_SOURCE_PIXELS: u64 = 64 * 1024 * 1024;
pub const MAX_WEBP_OUTPUT_PIXELS: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

impl ImageSize {
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    fn nonzero(self) -> Self {
        Self::new(self.width.max(1), self.height.max(1))
    }
}

#[derive(Debug)]
pub struct DecodedRgba {
    size: ImageSize,
    pixels: Vec<u8>,
}

impl DecodedRgba {
    #[must_use]
    pub const fn size(&self) -> ImageSize {
        self.size
    }

    #[must_use]
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }
}

/// Decode a WebP into tightly packed RGBA8 after applying a bounded target
/// size. The target callback runs only after the native header is validated.
pub fn decode_webp(
    bytes: &[u8],
    target: impl FnOnce(ImageSize) -> ImageSize,
) -> io::Result<DecodedRgba> {
    if bytes.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty WebP"));
    }
    if bytes.len() > MAX_WEBP_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("WebP exceeds the {MAX_WEBP_FILE_BYTES}-byte input limit"),
        ));
    }

    // SAFETY: `WebPInitDecoderConfig` initializes the complete C structure
    // before `assume_init`. Input and output buffers then remain alive for the
    // full native calls, and all sizes are checked before their pointers are
    // exposed to C.
    unsafe {
        let mut config = std::mem::MaybeUninit::<WebPDecoderConfig>::uninit();
        if !WebPInitDecoderConfig(config.as_mut_ptr()) {
            return Err(io::Error::other("libwebp ABI mismatch"));
        }
        let mut config = config.assume_init();
        let status = WebPGetFeatures(bytes.as_ptr(), bytes.len(), &mut config.input);
        if status != VP8StatusCode::VP8_STATUS_OK
            || config.input.width <= 0
            || config.input.height <= 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid WebP header ({status:?})"),
            ));
        }

        let original = ImageSize::new(config.input.width as u32, config.input.height as u32);
        check_pixel_budget("source", original, MAX_WEBP_SOURCE_PIXELS)?;
        let output = target(original).nonzero();
        check_pixel_budget("output", output, MAX_WEBP_OUTPUT_PIXELS)?;
        let stride = output
            .width
            .checked_mul(4)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "WebP row too wide"))?;
        let output_len = (stride as usize)
            .checked_mul(output.height as usize)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "WebP output too large"))?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(output_len).map_err(|error| {
            io::Error::other(format!("failed to reserve WebP output buffer: {error}"))
        })?;
        rgba.resize(output_len, 0);

        config.options.use_threads = 1;
        if output != original {
            config.options.use_scaling = 1;
            config.options.scaled_width = output.width as i32;
            config.options.scaled_height = output.height as i32;
        }
        config.output.colorspace = WEBP_CSP_MODE::MODE_RGBA;
        config.output.is_external_memory = 1;
        config.output.u.RGBA = WebPRGBABuffer {
            rgba: rgba.as_mut_ptr(),
            stride,
            size: rgba.len(),
        };

        let status = WebPDecode(bytes.as_ptr(), bytes.len(), &mut config);
        WebPFreeDecBuffer(&mut config.output);
        if status != VP8StatusCode::VP8_STATUS_OK {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("libwebp decode failed ({status:?})"),
            ));
        }

        Ok(DecodedRgba {
            size: output,
            pixels: rgba,
        })
    }
}

fn check_pixel_budget(label: &str, size: ImageSize, maximum: u64) -> io::Result<()> {
    let pixels = u64::from(size.width)
        .checked_mul(u64::from(size.height))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "WebP pixel count overflow"))?;
    if pixels > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "WebP {label} dimensions {}x{} exceed the {maximum}-pixel limit",
                size.width, size.height
            ),
        ));
    }
    Ok(())
}

/// Encode one tightly packed RGBA8 buffer as WebP.
pub fn encode_webp_rgba(rgba: &[u8], width: u32, height: u32, quality: f32) -> io::Result<Vec<u8>> {
    if width == 0 || height == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WebP dimensions must be non-zero",
        ));
    }
    let width_i32 = i32::try_from(width)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "WebP width is too large"))?;
    let height_i32 = i32::try_from(height)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "WebP height is too large"))?;
    let stride = width
        .checked_mul(4)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "WebP row too wide"))?;
    let expected = (stride as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "WebP image too large"))?;
    if rgba.len() != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WebP RGBA buffer has an invalid length",
        ));
    }
    if !quality.is_finite() || !(0.0..=100.0).contains(&quality) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WebP quality must be finite and between 0 and 100",
        ));
    }

    let mut encoded = std::ptr::null_mut();
    // SAFETY: `rgba` is validated as tightly packed RGBA8 and remains alive
    // for the call. libwebp owns `encoded` until it is copied and freed below.
    let len = unsafe {
        WebPEncodeRGBA(
            rgba.as_ptr(),
            width_i32,
            height_i32,
            stride,
            quality,
            &mut encoded,
        )
    };
    if len == 0 || encoded.is_null() {
        return Err(io::Error::other("libwebp encoding failed"));
    }
    // SAFETY: libwebp returned `len` initialized bytes at `encoded`.
    let bytes = unsafe { std::slice::from_raw_parts(encoded, len).to_vec() };
    // SAFETY: The allocation was returned by libwebp and has been copied.
    unsafe { WebPFree(encoded.cast()) };
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_oversized_input_without_entering_libwebp() {
        assert!(decode_webp(&[], |size| size).is_err());
        let oversized = vec![0; MAX_WEBP_FILE_BYTES + 1];
        assert!(decode_webp(&oversized, |size| size).is_err());
    }

    #[test]
    fn enforces_source_and_output_pixel_budgets() {
        assert!(
            check_pixel_budget(
                "source",
                ImageSize::new(8_192, 8_192),
                MAX_WEBP_SOURCE_PIXELS,
            )
            .is_ok()
        );
        assert!(
            check_pixel_budget(
                "source",
                ImageSize::new(8_193, 8_192),
                MAX_WEBP_SOURCE_PIXELS,
            )
            .is_err()
        );
        assert!(
            check_pixel_budget(
                "output",
                ImageSize::new(4_096, 4_096),
                MAX_WEBP_OUTPUT_PIXELS,
            )
            .is_ok()
        );
        assert!(
            check_pixel_budget(
                "output",
                ImageSize::new(4_097, 4_096),
                MAX_WEBP_OUTPUT_PIXELS,
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_invalid_encode_dimensions() {
        assert!(encode_webp_rgba(&[], 0, 1, 80.0).is_err());
        assert!(encode_webp_rgba(&[], 1, 0, 80.0).is_err());
        assert!(encode_webp_rgba(&[], u32::MAX, 1, 80.0).is_err());
        assert!(encode_webp_rgba(&[], 1, u32::MAX, 80.0).is_err());
    }

    #[test]
    fn round_trips_and_scales_rgba() {
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255, 64,
            64, 64, 255, 128, 128, 128, 255, 192, 192, 192, 255,
        ];
        let encoded = encode_webp_rgba(&rgba, 4, 2, 80.0).expect("encode");
        let decoded = decode_webp(&encoded, |_| ImageSize::new(2, 1)).expect("decode");
        assert_eq!(decoded.size(), ImageSize::new(2, 1));
        assert_eq!(decoded.into_pixels().len(), 8);
    }
}
