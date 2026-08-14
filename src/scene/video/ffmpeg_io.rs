use std::ffi::{CString, c_int, c_void};
use std::io::{Read, Seek, SeekFrom};
use std::mem::ManuallyDrop;
use std::ptr;
use std::sync::Arc;

use ffmpeg_next as ffmpeg;

use super::shared::PreparedSource;

const IO_BUFFER_BYTES: usize = 64 * 1024;

struct ReaderState {
    stream: keine_loader::ContentFile,
    length: u64,
}

struct CustomIo {
    context: *mut ffmpeg::ffi::AVIOContext,
    reader: *mut ReaderState,
}

/// FFmpeg input that keeps filesystem fast paths native while adapting
/// archive entries through AVIO's existing random-access contract.
pub(super) struct MediaInput {
    input: ManuallyDrop<ffmpeg::format::context::Input>,
    custom: Option<CustomIo>,
}

// Each instance owns its AVFormatContext, AVIOContext, and cursor. FFmpeg uses
// them only from the decoder thread that owns this value.
unsafe impl Send for MediaInput {}

impl MediaInput {
    pub(super) fn open(source: &Arc<PreparedSource>) -> Result<Self, ffmpeg::Error> {
        if let Some(path) = source.physical_path() {
            return Ok(Self {
                input: ManuallyDrop::new(ffmpeg::format::input(path)?),
                custom: None,
            });
        }
        Self::open_custom(source)
    }

    fn open_custom(source: &Arc<PreparedSource>) -> Result<Self, ffmpeg::Error> {
        let stream = source.open().map_err(|_| io_error())?;
        let reader = Box::into_raw(Box::new(ReaderState {
            stream,
            length: source.len(),
        }));
        let buffer = unsafe { ffmpeg::ffi::av_malloc(IO_BUFFER_BYTES) }.cast::<u8>();
        if buffer.is_null() {
            unsafe { drop(Box::from_raw(reader)) };
            return Err(ffmpeg::Error::Bug);
        }
        let io = unsafe {
            ffmpeg::ffi::avio_alloc_context(
                buffer,
                IO_BUFFER_BYTES as c_int,
                0,
                reader.cast(),
                Some(read_packet),
                None,
                Some(seek),
            )
        };
        if io.is_null() {
            unsafe {
                ffmpeg::ffi::av_free(buffer.cast());
                drop(Box::from_raw(reader));
            }
            return Err(ffmpeg::Error::Bug);
        }

        let mut format = unsafe { ffmpeg::ffi::avformat_alloc_context() };
        if format.is_null() {
            unsafe { free_custom_io(io, reader) };
            return Err(ffmpeg::Error::Bug);
        }
        unsafe {
            (*format).pb = io;
            (*format).flags |= ffmpeg::ffi::AVFMT_FLAG_CUSTOM_IO;
        }
        let hint = CString::new(format!(
            "keine-media.{}",
            source.extension().unwrap_or("bin")
        ))
        .expect("static media name must not contain NUL");
        let open_result = unsafe {
            ffmpeg::ffi::avformat_open_input(
                &mut format,
                hint.as_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if open_result < 0 {
            if !format.is_null() {
                unsafe { ffmpeg::ffi::avformat_free_context(format) };
            }
            unsafe { free_custom_io(io, reader) };
            return Err(ffmpeg::Error::from(open_result));
        }
        let info_result =
            unsafe { ffmpeg::ffi::avformat_find_stream_info(format, ptr::null_mut()) };
        if info_result < 0 {
            unsafe {
                ffmpeg::ffi::avformat_close_input(&mut format);
                free_custom_io(io, reader);
            }
            return Err(ffmpeg::Error::from(info_result));
        }
        Ok(Self {
            input: ManuallyDrop::new(unsafe { ffmpeg::format::context::Input::wrap(format) }),
            custom: Some(CustomIo {
                context: io,
                reader,
            }),
        })
    }

    /// Seek using the selected stream's time base. Constraining the upper
    /// bound to the target makes FFmpeg choose the closest decodable point at
    /// or before it, so the decoder can discard the short preroll precisely.
    pub(super) fn seek_stream_before(
        &mut self,
        stream_index: usize,
        timestamp: i64,
    ) -> Result<(), ffmpeg::Error> {
        let stream_index = c_int::try_from(stream_index).map_err(|_| ffmpeg::Error::InvalidData)?;
        // SAFETY: `self.input` owns a live AVFormatContext for this call, the
        // stream index came from that same context, and avformat_seek_file does
        // not retain any of the scalar arguments.
        let result = unsafe {
            ffmpeg::ffi::avformat_seek_file(
                self.input.as_mut_ptr(),
                stream_index,
                i64::MIN,
                timestamp,
                timestamp,
                0,
            )
        };
        if result >= 0 {
            Ok(())
        } else {
            Err(ffmpeg::Error::from(result))
        }
    }
}

impl std::ops::Deref for MediaInput {
    type Target = ffmpeg::format::context::Input;

    fn deref(&self) -> &Self::Target {
        &self.input
    }
}

impl std::ops::DerefMut for MediaInput {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.input
    }
}

impl Drop for MediaInput {
    fn drop(&mut self) {
        unsafe { ManuallyDrop::drop(&mut self.input) };
        if let Some(custom) = self.custom.take() {
            unsafe { free_custom_io(custom.context, custom.reader) };
        }
    }
}

unsafe fn free_custom_io(mut context: *mut ffmpeg::ffi::AVIOContext, reader: *mut ReaderState) {
    if !context.is_null() {
        unsafe {
            ffmpeg::ffi::av_freep((&raw mut (*context).buffer).cast());
            ffmpeg::ffi::avio_context_free(&mut context);
        }
    }
    if !reader.is_null() {
        unsafe { drop(Box::from_raw(reader)) };
    }
}

unsafe extern "C" fn read_packet(opaque: *mut c_void, output: *mut u8, output_len: c_int) -> c_int {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if opaque.is_null() || output.is_null() || output_len <= 0 {
            return Err(io_error());
        }
        let reader = unsafe { &mut *opaque.cast::<ReaderState>() };
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_len as usize) };
        match reader.stream.read(output) {
            Ok(0) => Ok(ffmpeg::ffi::AVERROR_EOF),
            Ok(read) => Ok(read as c_int),
            Err(_) => Err(io_error()),
        }
    }));
    result
        .unwrap_or(Err(ffmpeg::Error::External))
        .unwrap_or_else(Into::into)
}

unsafe extern "C" fn seek(opaque: *mut c_void, offset: i64, whence: c_int) -> i64 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if opaque.is_null() {
            return Err(io_error());
        }
        let reader = unsafe { &mut *opaque.cast::<ReaderState>() };
        if whence & ffmpeg::ffi::AVSEEK_SIZE != 0 {
            return i64::try_from(reader.length).map_err(|_| io_error());
        }
        let whence = whence & !ffmpeg::ffi::AVSEEK_FORCE;
        let position = match whence {
            ffmpeg::ffi::SEEK_SET if offset >= 0 => SeekFrom::Start(offset as u64),
            ffmpeg::ffi::SEEK_CUR => SeekFrom::Current(offset),
            ffmpeg::ffi::SEEK_END => SeekFrom::End(offset),
            _ => return Err(io_error()),
        };
        reader
            .stream
            .seek(position)
            .and_then(|position| i64::try_from(position).map_err(std::io::Error::other))
            .map_err(|_| io_error())
    }));
    result
        .unwrap_or(Err(ffmpeg::Error::External))
        .unwrap_or_else(|error| i64::from(c_int::from(error)))
}

fn io_error() -> ffmpeg::Error {
    ffmpeg::Error::Other {
        errno: ffmpeg::error::EIO,
    }
}

pub(super) struct VideoDecoder {
    input: MediaInput,
    decoder: ffmpeg::decoder::Video,
    stream_index: usize,
    time_base: ffmpeg::Rational,
    duration: f32,
    draining: bool,
}

impl VideoDecoder {
    pub(super) fn open(source: &Arc<PreparedSource>) -> Result<Self, ffmpeg::Error> {
        let input = MediaInput::open(source)?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let stream_index = stream.index();
        let time_base = stream.time_base();
        let duration = if stream.duration() > 0 {
            stream.duration() as f64 * f64::from(time_base.numerator())
                / f64::from(time_base.denominator())
        } else {
            input.duration().max(0) as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE)
        } as f32;
        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = context.decoder().video()?;
        Ok(Self {
            input,
            decoder,
            stream_index,
            time_base,
            duration,
            draining: false,
        })
    }

    pub(super) const fn time_base(&self) -> ffmpeg::Rational {
        self.time_base
    }

    pub(super) const fn duration(&self) -> f32 {
        self.duration
    }

    pub(super) fn has_audio(&self) -> bool {
        self.input
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .is_some()
    }

    pub(super) fn decode_raw(&mut self) -> Result<Option<ffmpeg::frame::Video>, ffmpeg::Error> {
        loop {
            let mut frame = ffmpeg::frame::Video::empty();
            match self.decoder.receive_frame(&mut frame) {
                Ok(()) => return Ok(Some(frame)),
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {}
                Err(error) => return Err(error),
            }
            if self.draining {
                return Ok(None);
            }
            let packet = self
                .input
                .packets()
                .find(|(stream, _)| stream.index() == self.stream_index)
                .map(|(_, packet)| packet);
            if let Some(packet) = packet {
                self.decoder.send_packet(&packet)?;
            } else {
                self.decoder.send_eof()?;
                self.draining = true;
            }
        }
    }

    pub(super) fn seek_to_start(&mut self) -> Result<(), ffmpeg::Error> {
        self.input.seek(i64::MIN, ..)?;
        self.decoder.flush();
        self.draining = false;
        Ok(())
    }
}
