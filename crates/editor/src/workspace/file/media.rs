//! Import-time normalization. Sources stay read-only; only validated output is published.
use super::*;
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageEncoder};
use std::process::{Command, Stdio};

const MAX_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;

pub(super) fn output_extension(source: &Path, kind: AssetKind) -> io::Result<String> {
    let size = fs::metadata(source)?.len();
    if size == 0 || size > MAX_MEDIA_BYTES {
        return Err(invalid("Resource size is invalid"));
    }
    let extension = extension(source);
    if validate_resource(source, kind, &extension).is_ok() {
        return Ok(extension);
    }
    let supported = match kind {
        AssetKind::Background | AssetKind::Figure | AssetKind::Particle => {
            matches!(
                extension.as_str(),
                "webp" | "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff"
            )
        }
        AssetKind::Voice | AssetKind::Bgm | AssetKind::Effect => {
            matches!(
                extension.as_str(),
                "ogg" | "opus" | "oga" | "wav" | "mp3" | "flac" | "aac" | "m4a"
            )
        }
        AssetKind::Video => matches!(extension.as_str(), "mp4" | "m4v" | "mov" | "webm" | "mkv"),
    };
    if !supported {
        return Err(invalid(format!(
            "File format is incompatible with {}",
            kind.label()
        )));
    }
    Ok(match kind {
        AssetKind::Background | AssetKind::Figure | AssetKind::Particle => "webp",
        AssetKind::Voice | AssetKind::Bgm | AssetKind::Effect => "opus",
        AssetKind::Video => "mp4",
    }
    .into())
}

pub(super) fn import(source: &Path, destination: &Path, kind: AssetKind) -> io::Result<()> {
    let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temporary =
        destination.with_file_name(format!(".media-import-{}-{nonce}", std::process::id()));
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    let result = (|| {
        if validate_resource(source, kind, &extension(source)).is_ok() {
            io::copy(&mut File::open(source)?, &mut output)?;
        } else {
            match kind {
                AssetKind::Background | AssetKind::Figure | AssetKind::Particle => {
                    let (image, profile) = decode_image(source)?;
                    let mut writer = io::BufWriter::new(&mut output);
                    let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut writer);
                    if let Some(profile) = profile {
                        encoder
                            .set_icc_profile(profile)
                            .map_err(|error| invalid(error.to_string()))?;
                    }
                    encoder
                        .write_image(
                            image.as_bytes(),
                            image.width(),
                            image.height(),
                            image::ExtendedColorType::Rgba8,
                        )
                        .map_err(|error| invalid(format!("WebP encoding failed: {error}")))?;
                    use io::Write;
                    writer.flush()?;
                }
                _ => {
                    drop(output);
                    transcode(source, &temporary, kind)?;
                    output = OpenOptions::new().write(true).open(&temporary)?;
                }
            }
        }
        output.sync_all()?;
        drop(output);
        validate_resource(&temporary, kind, &extension(destination))?;
        // Publish without replacing a file created by another program during conversion.
        fs::hard_link(&temporary, destination)?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn decode_image(source: &Path) -> io::Result<(DynamicImage, Option<Vec<u8>>)> {
    let image_error = |error| invalid(format!("Invalid image: {error}"));
    let mut reader = ImageReader::open(source)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits.clone());
    // A static resource cannot preserve animation; never silently discard frames.
    match reader.format() {
        Some(ImageFormat::Gif) => {
            let mut decoder =
                image::codecs::gif::GifDecoder::new(BufReader::new(File::open(source)?))
                    .map_err(image_error)?;
            decoder.set_limits(limits).map_err(image_error)?;
            let frames = decoder
                .into_frames()
                .take(2)
                .collect::<Result<Vec<_>, _>>()
                .map_err(image_error)?;
            if frames.len() > 1 {
                return Err(invalid(
                    "Animated images require a sprite sequence or video",
                ));
            }
        }
        Some(ImageFormat::Png) => {
            let decoder = image::codecs::png::PngDecoder::new(BufReader::new(File::open(source)?))
                .map_err(image_error)?;
            if decoder.is_apng().map_err(image_error)? {
                return Err(invalid(
                    "Animated images require a sprite sequence or video",
                ));
            }
        }
        _ => {}
    }
    let mut decoder = reader.into_decoder().map_err(image_error)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(invalid("Image exceeds the production pixel budget"));
    }
    let orientation = decoder.orientation().map_err(image_error)?;
    let profile = decoder.icc_profile().map_err(image_error)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(image_error)?;
    image.apply_orientation(orientation);
    // WebP's lossless encoder accepts RGB8/RGBA8; alpha remains intact.
    Ok((DynamicImage::ImageRgba8(image.into_rgba8()), profile))
}

fn transcode(source: &Path, output: &Path, kind: AssetKind) -> io::Result<()> {
    let mut command = Command::new(ffmpeg_executable()?);
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-protocol_whitelist",
            "file",
            "-i",
        ])
        .arg(source.canonicalize()?)
        .args(["-map_metadata", "-1", "-map_chapters", "-1"]);
    if kind == AssetKind::Video {
        command.args([
            "-map",
            "0:v:0",
            "-map",
            "0:a:0?",
            "-sn",
            "-dn",
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ]);
    } else {
        command.args([
            "-map",
            "0:a:0",
            "-vn",
            "-sn",
            "-dn",
            "-c:a",
            "libopus",
            "-b:a",
            "192k",
            "-vbr",
            "on",
            "-application",
            "audio",
            "-ar",
            "48000",
            "-af",
            "aformat=channel_layouts=mono|stereo",
            "-f",
            "ogg",
        ]);
    }
    command
        .args(["-fs", &(MAX_MEDIA_BYTES + 1).to_string()])
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    if !command.status()?.success() {
        return Err(invalid(
            "FFmpeg conversion failed; check the input and libopus/libx264 support. Original file unchanged",
        ));
    }
    Ok(())
}

pub(super) fn ffmpeg_executable() -> io::Result<PathBuf> {
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        candidates.push(parent.join(name));
        candidates.push(parent.join("../Resources").join(name));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|path| path.join(name)));
    }
    #[cfg(target_os = "macos")]
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/ffmpeg"),
        PathBuf::from("/usr/local/bin/ffmpeg"),
    ]);
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Audio/video import needs FFmpeg on PATH or beside Editor; original file unchanged",
            )
        })
}
