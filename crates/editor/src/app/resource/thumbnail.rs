//! Bounded browser thumbnails. Full-resolution images belong to Asset Preview.
use super::*;
use gpui_kit::{Asset, ImageCacheError, RenderImage};
use image::ImageDecoder as _;

const CAPACITY: usize = 64;
const WIDTH: u32 = 384;
const HEIGHT: u32 = 256;

#[derive(Clone, Hash, PartialEq, Eq)]
struct Source {
    owner: gpui_kit::EntityId,
    root: PathBuf,
    path: PathBuf,
    bytes: u64,
    modified: Option<std::time::SystemTime>,
}

struct Loader;

impl Asset for Loader {
    type Source = Source;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    // The returned worker future must not capture GPUI's non-Send App argument.
    #[allow(clippy::manual_async_fn)]
    fn load(source: Source, cx: &mut App) -> impl Future<Output = Self::Output> + Send + 'static {
        let background = cx.background_executor().clone();
        background.spawn_with_priority(gpui_kit::Priority::Low, async move {
            let _permit = decode_permit().await;
            let file = confined_existing_file(&source.root, &source.path)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Thumbnail unavailable"))?;
            let pixels = decode(&file)?;
            Ok(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
        })
    }
}

// Bound concurrent full-image decodes without blocking an executor thread while waiting.
static DECODING: std::sync::Mutex<(usize, Vec<std::task::Waker>)> =
    std::sync::Mutex::new((0, Vec::new()));
struct DecodePermit;

async fn decode_permit() -> DecodePermit {
    std::future::poll_fn(|cx| {
        let mut state = DECODING.lock().unwrap();
        if state.0 < 2 {
            state.0 += 1;
            std::task::Poll::Ready(DecodePermit)
        } else {
            if !state.1.iter().any(|waker| waker.will_wake(cx.waker())) {
                state.1.push(cx.waker().clone());
            }
            std::task::Poll::Pending
        }
    })
    .await
}

impl Drop for DecodePermit {
    fn drop(&mut self) {
        let waiting = {
            let mut state = DECODING.lock().unwrap();
            state.0 -= 1;
            std::mem::take(&mut state.1)
        };
        for waker in waiting {
            waker.wake();
        }
    }
}

fn decode(file: &Path) -> Result<image::RgbaImage, ImageCacheError> {
    let mut reader = image::ImageReader::open(file)?.with_guessed_format()?;
    let webp = if reader.format() == Some(image::ImageFormat::WebP) {
        let decoder =
            image::codecs::webp::WebPDecoder::new(std::io::BufReader::new(fs::File::open(file)?))?;
        !decoder.has_animation()
    } else {
        false
    };
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = if webp {
        use std::io::Read as _;
        // Bound the read even if the file grows after metadata inspection.
        let mut bytes = Vec::new();
        fs::File::open(file)?
            .take(keine_media::MAX_WEBP_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        let rotated = matches!(
            orientation,
            image::metadata::Orientation::Rotate90
                | image::metadata::Orientation::Rotate270
                | image::metadata::Orientation::Rotate90FlipH
                | image::metadata::Orientation::Rotate270FlipH
        );
        let decoded = keine_media::decode_webp(&bytes, |size| {
            let (width, height) = if rotated {
                (HEIGHT, WIDTH)
            } else {
                (WIDTH, HEIGHT)
            };
            let ratio = (width as f64 / size.width as f64)
                .min(height as f64 / size.height as f64)
                .min(1.);
            keine_media::ImageSize::new(
                (size.width as f64 * ratio).round().max(1.) as u32,
                (size.height as f64 * ratio).round().max(1.) as u32,
            )
        })?;
        let size = decoded.size();
        image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(size.width, size.height, decoded.into_pixels())
                .ok_or_else(|| io::Error::other("Invalid thumbnail buffer"))?,
        )
    } else {
        image::DynamicImage::from_decoder(decoder)?
    };
    image.apply_orientation(orientation);
    let mut pixels = if webp {
        image.into_rgba8()
    } else {
        image.thumbnail(WIDTH, HEIGHT).into_rgba8()
    };
    // GPUI expects BGRA, including transparent pixels.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(pixels)
}

#[derive(Default)]
pub(in crate::app) struct Thumbnails {
    recent: VecDeque<Source>,
}

impl Thumbnails {
    pub(in crate::app) fn new(cx: &mut App) -> Entity<Self> {
        let entity = cx.new(|_| Self::default());
        cx.observe_release(&entity, |cache, cx| {
            for source in cache.recent.drain(..) {
                if let Some(Ok(image)) = cx.fetch_asset::<Loader>(&source) {
                    cx.drop_image(image, None);
                }
                cx.remove_asset::<Loader>(&source);
            }
        })
        .detach();
        entity
    }

    fn touch(&mut self, source: &Source) -> Option<Source> {
        if let Some(position) = self.recent.iter().position(|key| key == source) {
            self.recent.remove(position);
        }
        self.recent.push_back(source.clone());
        (self.recent.len() > CAPACITY).then(|| self.recent.pop_front().unwrap())
    }

    pub(super) fn image(
        cache: &Entity<Self>,
        root: &Path,
        path: &Path,
        media: &file_ops::AssetMediaInfo,
    ) -> gpui_kit::Img {
        let source = Source {
            owner: cache.entity_id(),
            root: root.to_owned(),
            path: path.to_owned(),
            bytes: media.bytes,
            modified: media.modified,
        };
        let cache = cache.clone();
        img(move |window: &mut Window, cx: &mut App| {
            cache.update(cx, |cache, cx| {
                if let Some(old) = cache.touch(&source) {
                    if let Some(Ok(image)) = window.get_asset::<Loader>(&old, cx) {
                        cx.drop_image(image, Some(window));
                    }
                    cx.remove_asset::<Loader>(&old);
                }
                window.use_asset::<Loader>(&source, cx)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_queue_waits_without_blocking_and_releases_on_drop() {
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let mut first = Box::pin(decode_permit());
        let std::task::Poll::Ready(first) = first.as_mut().poll(&mut context) else {
            panic!("first")
        };
        let mut second = Box::pin(decode_permit());
        let std::task::Poll::Ready(second) = second.as_mut().poll(&mut context) else {
            panic!("second")
        };
        let mut waiting = Box::pin(decode_permit());
        assert!(waiting.as_mut().poll(&mut context).is_pending());
        drop(first);
        assert!(waiting.as_mut().poll(&mut context).is_ready());
        drop(second);
        assert_eq!(DECODING.lock().unwrap().0, 0);
    }

    #[test]
    fn scaled_webp_preserves_transparency_and_portrait_bounds() {
        let path =
            std::env::temp_dir().join(format!("keine-thumbnail-{}.webp", std::process::id()));
        let pixels = image::RgbaImage::from_pixel(1080, 1920, image::Rgba([210, 40, 10, 128]));
        pixels.save(&path).unwrap();
        let thumb = decode(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(thumb.dimensions(), (144, 256));
        let actual = thumb.get_pixel(50, 100).0;
        // Native rescaling can round premultiplied RGB by one byte.
        for (actual, expected) in actual[..3].iter().zip([10_u8, 40, 210]) {
            assert!(actual.abs_diff(expected) <= 1);
        }
        assert_eq!(actual[3], 128);
    }

    #[test]
    fn scaled_webp_applies_exif_orientation_before_browser_bounds() {
        use image::ImageEncoder as _;
        let path = std::env::temp_dir().join(format!(
            "keine-thumbnail-rotated-{}.webp",
            std::process::id()
        ));
        let pixels = image::RgbaImage::from_pixel(1920, 1080, image::Rgba([210, 40, 10, 128]));
        let mut encoder =
            image::codecs::webp::WebPEncoder::new_lossless(fs::File::create(&path).unwrap());
        // TIFF IFD0: orientation = 6 (90 degrees clockwise).
        encoder
            .set_exif_metadata(vec![
                73, 73, 42, 0, 8, 0, 0, 0, 1, 0, 18, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
            ])
            .unwrap();
        encoder
            .write_image(pixels.as_raw(), 1920, 1080, image::ExtendedColorType::Rgba8)
            .unwrap();
        let thumb = decode(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(thumb.dimensions(), (144, 256));
        let actual = thumb.get_pixel(50, 100).0;
        // Native rescaling can round premultiplied RGB by one byte.
        for (actual, expected) in actual[..3].iter().zip([10_u8, 40, 210]) {
            assert!(actual.abs_diff(expected) <= 1);
        }
        assert_eq!(actual[3], 128);
    }

    #[test]
    fn cache_is_bounded_and_file_revisions_do_not_reuse_pixels() {
        let mut cache = Thumbnails::default();
        let source = |i| Source {
            owner: gpui_kit::EntityId::from(1_u64),
            root: PathBuf::from("/project"),
            path: PathBuf::from(format!("{i}.png")),
            bytes: 10,
            modified: None,
        };
        for i in 0..CAPACITY {
            assert!(cache.touch(&source(i)).is_none());
        }
        assert!(cache.touch(&source(0)).is_none());
        assert_eq!(cache.touch(&source(CAPACITY)).unwrap().path, source(1).path);
        let mut revision = source(0);
        revision.modified = Some(std::time::SystemTime::UNIX_EPOCH);
        assert!(cache.touch(&revision).is_some());
        assert_eq!(cache.recent.len(), CAPACITY);
        assert_ne!(cache.recent.back().unwrap().modified, source(0).modified);
    }

    #[test]
    fn thumbnail_bounds_channels_and_alpha() {
        let path = std::env::temp_dir().join(format!("keine-thumbnail-{}.png", std::process::id()));
        let pixels = image::RgbaImage::from_pixel(1920, 1080, image::Rgba([210, 40, 10, 128]));
        pixels.save(&path).unwrap();
        let thumb = decode(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(thumb.dimensions(), (384, 216));
        assert_eq!(thumb.get_pixel(100, 100).0, [10, 40, 210, 128]);
        assert!(thumb.as_raw().len() <= (WIDTH * HEIGHT * 4) as usize);
    }
}

#[cfg(test)]
#[path = "../../../../../tests/bench/editor/thumbnail.rs"]
mod benchmark;
