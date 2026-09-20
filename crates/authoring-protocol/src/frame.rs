use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use memmap2::{MmapMut, MmapOptions};
use serde::{Deserialize, Serialize};

const MAGIC: [u8; 8] = *b"KNEFRM01";
const FRAME_SCHEMA: u32 = 1;
const BYTES_PER_PIXEL: u32 = 4;
pub const FRAME_SLOT_COUNT: usize = 3;
const SLOT_FREE: u64 = 0;
const SLOT_WRITING: u64 = 1;
const STATE_BITS: u32 = 2;
const STATE_MASK: u64 = (1 << STATE_BITS) - 1;
const SLOT_READY: u64 = 2;
const SLOT_READING: u64 = 3;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PixelFormat {
    Rgba8Srgb,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrameTransportDescriptor {
    pub path: PathBuf,
    pub project_key: u64,
    pub session_generation: u64,
    pub max_width: u32,
    pub max_height: u32,
    pub pixel_format: PixelFormat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameMetadata {
    pub project_key: u64,
    pub session_generation: u64,
    pub document_revision: u64,
    pub frame_id: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixel_format: PixelFormat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedFrame {
    pub metadata: FrameMetadata,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameTransportStats {
    pub published: u64,
    pub overwritten: u64,
}

#[repr(C, align(64))]
struct SharedHeader {
    magic: [u8; 8],
    schema: u32,
    slot_count: u32,
    project_key: u64,
    session_generation: u64,
    max_width: u32,
    max_height: u32,
    slot_capacity: u64,
    slot_offsets: [u64; FRAME_SLOT_COUNT],
    published: AtomicU64,
    overwritten: AtomicU64,
}

#[repr(C, align(64))]
struct SharedSlot {
    state: AtomicU64,
    document_revision: u64,
    frame_id: u64,
    width: u32,
    height: u32,
    stride: u32,
    pixel_format: u32,
    data_len: u64,
}

struct PublishedFrame<'a> {
    document_revision: u64,
    frame_id: u64,
    width: u32,
    height: u32,
    stride: u32,
    bytes: &'a [u8],
}

pub struct SharedFrameConsumer {
    descriptor: FrameTransportDescriptor,
    map: MmapMut,
    file: File,
    last_frame_id: u64,
}

pub struct SharedFrameProducer {
    descriptor: FrameTransportDescriptor,
    map: MmapMut,
    next_slot: usize,
    next_frame_id: u64,
}

impl SharedFrameConsumer {
    pub fn create(
        path: PathBuf,
        project_key: u64,
        session_generation: u64,
        max_width: u32,
        max_height: u32,
    ) -> io::Result<Self> {
        validate_dimensions(max_width, max_height)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        let slot_capacity = frame_len(max_width, max_height)?;
        let slot_stride = align_to(
            std::mem::size_of::<SharedSlot>()
                .checked_add(slot_capacity)
                .ok_or_else(layout_overflow)?,
            64,
        )?;
        let header_len = align_to(std::mem::size_of::<SharedHeader>(), 64)?;
        let map_len = header_len
            .checked_add(
                slot_stride
                    .checked_mul(FRAME_SLOT_COUNT)
                    .ok_or_else(layout_overflow)?,
            )
            .ok_or_else(layout_overflow)?;
        file.set_len(u64::try_from(map_len).map_err(|_| layout_overflow())?)?;

        // SAFETY: This process created and exclusively owns `file`, fixed its
        // length above, and keeps the file alive for the complete mapping.
        let mut map = unsafe { MmapOptions::new().len(map_len).map_mut(&file)? };
        map.fill(0);
        let slot_offsets = std::array::from_fn(|index| {
            u64::try_from(header_len + slot_stride * index).expect("frame mapping fits u64")
        });
        let header = SharedHeader {
            magic: MAGIC,
            schema: FRAME_SCHEMA,
            slot_count: FRAME_SLOT_COUNT as u32,
            project_key,
            session_generation,
            max_width,
            max_height,
            slot_capacity: slot_capacity as u64,
            slot_offsets,
            published: AtomicU64::new(0),
            overwritten: AtomicU64::new(0),
        };
        // SAFETY: mmap bases are page-aligned, the header starts at offset 0,
        // and the mapping is exclusively initialized before it is shared.
        unsafe { map.as_mut_ptr().cast::<SharedHeader>().write(header) };
        for offset in slot_offsets {
            // SAFETY: Every offset is 64-byte aligned and reserves a complete
            // `SharedSlot` followed by `slot_capacity` bytes inside `map`.
            unsafe {
                map.as_mut_ptr()
                    .add(offset as usize)
                    .cast::<SharedSlot>()
                    .write(SharedSlot {
                        state: AtomicU64::new(SLOT_FREE),
                        document_revision: 0,
                        frame_id: 0,
                        width: 0,
                        height: 0,
                        stride: 0,
                        pixel_format: PixelFormat::Rgba8Srgb as u32,
                        data_len: 0,
                    });
            }
        }
        map.flush()?;
        drop(map);
        // SAFETY: The initialized file remains fixed-size while both peers map
        // it. The producer never truncates it and slot state prevents a writer
        // from touching bytes claimed by this consumer.
        let map = unsafe { MmapOptions::new().map_mut(&file)? };
        let descriptor = FrameTransportDescriptor {
            path,
            project_key,
            session_generation,
            max_width,
            max_height,
            pixel_format: PixelFormat::Rgba8Srgb,
        };
        validate_header(&map, &descriptor)?;
        Ok(Self {
            descriptor,
            map,
            file,
            last_frame_id: 0,
        })
    }

    pub fn descriptor(&self) -> &FrameTransportDescriptor {
        &self.descriptor
    }

    pub fn read_latest(&mut self, document_revision: u64) -> io::Result<Option<OwnedFrame>> {
        let header = header(&self.map);
        for _ in 0..FRAME_SLOT_COUNT * 2 {
            let published = header.published.load(Ordering::Acquire);
            let Some((frame_id, slot_index)) = decode_publication(published) else {
                return Ok(None);
            };
            if frame_id <= self.last_frame_id {
                return Ok(None);
            }
            let slot = slot(&self.map, header, slot_index)?;
            let ready = encode_state(frame_id, SLOT_READY)?;
            if slot
                .state
                .compare_exchange(
                    ready,
                    encode_state(frame_id, SLOT_READING)?,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                std::hint::spin_loop();
                continue;
            }

            let result = copy_claimed_frame(
                &self.map,
                header,
                slot,
                slot_index,
                self.descriptor.project_key,
                self.descriptor.session_generation,
            );
            slot.state.store(SLOT_FREE, Ordering::Release);
            let frame = result?;
            self.last_frame_id = frame.metadata.frame_id;
            if frame.metadata.document_revision != document_revision {
                continue;
            }
            return Ok(Some(frame));
        }
        Ok(None)
    }

    pub fn stats(&self) -> FrameTransportStats {
        let header = header(&self.map);
        FrameTransportStats {
            published: decode_publication(header.published.load(Ordering::Acquire))
                .map_or(0, |(frame, _)| frame),
            overwritten: header.overwritten.load(Ordering::Relaxed),
        }
    }
}

impl Drop for SharedFrameConsumer {
    fn drop(&mut self) {
        let path = self.descriptor.path.clone();
        let _ = self.file.sync_all();
        let _ = fs::remove_file(path);
    }
}

impl SharedFrameProducer {
    pub fn open(descriptor: FrameTransportDescriptor) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&descriptor.path)?;
        // SAFETY: The editor-created descriptor fixes the mapping length for
        // the session, and the editor keeps its owner file alive until Stop.
        let map = unsafe { MmapOptions::new().map_mut(&file)? };
        validate_header(&map, &descriptor)?;
        Ok(Self {
            descriptor,
            map,
            next_slot: 0,
            next_frame_id: 1,
        })
    }

    pub fn publish(
        &mut self,
        document_revision: u64,
        width: u32,
        height: u32,
        stride: u32,
        bytes: &[u8],
    ) -> io::Result<u64> {
        // SAFETY: The validated mapping starts with a stable, aligned header.
        // Keeping a raw pointer avoids treating the whole mmap as immutably
        // borrowed while this producer writes a claimed slot below.
        let header = unsafe { &*self.map.as_ptr().cast::<SharedHeader>() };
        validate_frame(header, width, height, stride, bytes.len())?;
        let frame_id = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.wrapping_add(1).max(1);

        for offset in 0..FRAME_SLOT_COUNT {
            let slot_index = (self.next_slot + offset) % FRAME_SLOT_COUNT;
            let slot_ref = slot(&self.map, header, slot_index)?;
            let current = slot_ref.state.load(Ordering::Acquire);
            if state_kind(current) == SLOT_READING || state_kind(current) == SLOT_WRITING {
                continue;
            }
            if slot_ref
                .state
                .compare_exchange(
                    current,
                    encode_state(frame_id, SLOT_WRITING)?,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                continue;
            }
            if state_kind(current) == SLOT_READY {
                header.overwritten.fetch_add(1, Ordering::Relaxed);
            }
            write_claimed_frame(
                &mut self.map,
                header,
                slot_index,
                PublishedFrame {
                    document_revision,
                    frame_id,
                    width,
                    height,
                    stride,
                    bytes,
                },
            )?;
            let slot_ref = slot(&self.map, header, slot_index)?;
            slot_ref
                .state
                .store(encode_state(frame_id, SLOT_READY)?, Ordering::Release);
            header
                .published
                .store(encode_publication(frame_id, slot_index)?, Ordering::Release);
            self.next_slot = (slot_index + 1) % FRAME_SLOT_COUNT;
            return Ok(frame_id);
        }

        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "every preview frame slot is currently claimed",
        ))
    }

    pub fn descriptor(&self) -> &FrameTransportDescriptor {
        &self.descriptor
    }
}

fn validate_dimensions(width: u32, height: u32) -> io::Result<()> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "preview frame dimensions must be within 1..=4096",
        ));
    }
    let _ = frame_len(width, height)?;
    Ok(())
}

fn frame_len(width: u32, height: u32) -> io::Result<usize> {
    usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(BYTES_PER_PIXEL as usize))
        .and_then(|stride| stride.checked_mul(height as usize))
        .ok_or_else(layout_overflow)
}

fn validate_header(map: &[u8], descriptor: &FrameTransportDescriptor) -> io::Result<()> {
    if map.len() < std::mem::size_of::<SharedHeader>() {
        return Err(invalid_mapping("preview frame mapping is truncated"));
    }
    let header = header(map);
    if header.magic != MAGIC
        || header.schema != FRAME_SCHEMA
        || header.slot_count as usize != FRAME_SLOT_COUNT
        || header.project_key != descriptor.project_key
        || header.session_generation != descriptor.session_generation
        || header.max_width != descriptor.max_width
        || header.max_height != descriptor.max_height
    {
        return Err(invalid_mapping("preview frame mapping identity mismatch"));
    }
    for index in 0..FRAME_SLOT_COUNT {
        let offset = usize::try_from(header.slot_offsets[index])
            .map_err(|_| invalid_mapping("preview slot offset overflow"))?;
        let end = offset
            .checked_add(std::mem::size_of::<SharedSlot>())
            .and_then(|end| end.checked_add(header.slot_capacity as usize))
            .ok_or_else(|| invalid_mapping("preview slot layout overflow"))?;
        if !offset.is_multiple_of(64) || end > map.len() {
            return Err(invalid_mapping("preview slot is outside the mapping"));
        }
    }
    Ok(())
}

fn validate_frame(
    header: &SharedHeader,
    width: u32,
    height: u32,
    stride: u32,
    data_len: usize,
) -> io::Result<()> {
    if width == 0
        || height == 0
        || width > header.max_width
        || height > header.max_height
        || stride < width.saturating_mul(BYTES_PER_PIXEL)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid preview frame dimensions or stride",
        ));
    }
    let expected = usize::try_from(stride)
        .ok()
        .and_then(|stride| stride.checked_mul(height as usize))
        .ok_or_else(layout_overflow)?;
    if expected != data_len || expected > header.slot_capacity as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "preview frame payload length does not match its layout",
        ));
    }
    Ok(())
}

fn write_claimed_frame(
    map: &mut MmapMut,
    header: &SharedHeader,
    slot_index: usize,
    frame: PublishedFrame<'_>,
) -> io::Result<()> {
    let offset = header.slot_offsets[slot_index] as usize;
    let slot_ptr = unsafe { map.as_mut_ptr().add(offset).cast::<SharedSlot>() };
    // SAFETY: The producer changed this slot from FREE/READY to WRITING. The
    // single consumer can only read a READY slot after a successful claim, so
    // metadata and payload are exclusively writable until the release store.
    unsafe {
        (*slot_ptr).document_revision = frame.document_revision;
        (*slot_ptr).frame_id = frame.frame_id;
        (*slot_ptr).width = frame.width;
        (*slot_ptr).height = frame.height;
        (*slot_ptr).stride = frame.stride;
        (*slot_ptr).pixel_format = PixelFormat::Rgba8Srgb as u32;
        (*slot_ptr).data_len = frame.bytes.len() as u64;
        std::ptr::copy_nonoverlapping(
            frame.bytes.as_ptr(),
            slot_ptr.add(1).cast::<u8>(),
            frame.bytes.len(),
        );
    }
    Ok(())
}

fn copy_claimed_frame(
    map: &[u8],
    header: &SharedHeader,
    slot: &SharedSlot,
    slot_index: usize,
    project_key: u64,
    session_generation: u64,
) -> io::Result<OwnedFrame> {
    let data_len = usize::try_from(slot.data_len)
        .map_err(|_| invalid_mapping("preview frame length overflow"))?;
    validate_frame(header, slot.width, slot.height, slot.stride, data_len)?;
    if slot.pixel_format != PixelFormat::Rgba8Srgb as u32 {
        return Err(invalid_mapping("unsupported preview pixel format"));
    }
    let offset = header.slot_offsets[slot_index] as usize + std::mem::size_of::<SharedSlot>();
    let end = offset
        .checked_add(data_len)
        .ok_or_else(|| invalid_mapping("preview frame payload overflow"))?;
    let bytes = map
        .get(offset..end)
        .ok_or_else(|| invalid_mapping("preview frame payload is truncated"))?
        .to_vec();
    Ok(OwnedFrame {
        metadata: FrameMetadata {
            project_key,
            session_generation,
            document_revision: slot.document_revision,
            frame_id: slot.frame_id,
            width: slot.width,
            height: slot.height,
            stride: slot.stride,
            pixel_format: PixelFormat::Rgba8Srgb,
        },
        bytes,
    })
}

fn header(map: &[u8]) -> &SharedHeader {
    // SAFETY: All maps are validated to contain an aligned initialized header
    // before this helper is used.
    unsafe { &*map.as_ptr().cast::<SharedHeader>() }
}

fn slot<'a>(map: &'a [u8], header: &SharedHeader, index: usize) -> io::Result<&'a SharedSlot> {
    let offset = *header
        .slot_offsets
        .get(index)
        .ok_or_else(|| invalid_mapping("preview slot index is invalid"))? as usize;
    let end = offset
        .checked_add(std::mem::size_of::<SharedSlot>())
        .ok_or_else(|| invalid_mapping("preview slot header overflow"))?;
    if !offset.is_multiple_of(64) || end > map.len() {
        return Err(invalid_mapping(
            "preview slot header is outside the mapping",
        ));
    }
    // SAFETY: Offset alignment and bounds were checked above and the slot was
    // initialized before the mapping descriptor became visible to the peer.
    Ok(unsafe { &*map.as_ptr().add(offset).cast::<SharedSlot>() })
}

fn encode_publication(frame_id: u64, slot: usize) -> io::Result<u64> {
    frame_id
        .checked_shl(2)
        .and_then(|value| value.checked_add(slot as u64 + 1))
        .ok_or_else(layout_overflow)
}

fn decode_publication(value: u64) -> Option<(u64, usize)> {
    let encoded_slot = value & STATE_MASK;
    if encoded_slot == 0 {
        return None;
    }
    Some((value >> 2, encoded_slot as usize - 1))
}

fn encode_state(frame_id: u64, state: u64) -> io::Result<u64> {
    frame_id
        .checked_shl(STATE_BITS)
        .and_then(|value| value.checked_add(state))
        .ok_or_else(layout_overflow)
}

const fn state_kind(state: u64) -> u64 {
    state & STATE_MASK
}

fn align_to(value: usize, alignment: usize) -> io::Result<usize> {
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or_else(layout_overflow)
}

fn layout_overflow() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "preview frame layout overflow")
}

fn invalid_mapping(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn remove_stale_mapping(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "keine-frame-{name}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ))
    }

    #[test]
    fn shared_triple_buffer_publishes_only_the_latest_complete_frame() {
        let path = path("latest");
        remove_stale_mapping(&path).unwrap();
        let mut consumer = SharedFrameConsumer::create(path, 17, 9, 4, 2).unwrap();
        let mut producer = SharedFrameProducer::open(consumer.descriptor().clone()).unwrap();
        for value in 1..=12 {
            producer.publish(4, 4, 2, 16, &[value; 32]).unwrap();
        }
        let frame = consumer.read_latest(4).unwrap().unwrap();
        assert_eq!(frame.metadata.frame_id, 12);
        assert_eq!(frame.bytes, [12; 32]);
        assert_eq!(consumer.stats().published, 12);
        assert!(consumer.stats().overwritten > 0);
    }

    #[test]
    fn stale_revision_is_consumed_but_not_returned() {
        let path = path("revision");
        remove_stale_mapping(&path).unwrap();
        let mut consumer = SharedFrameConsumer::create(path, 5, 6, 2, 2).unwrap();
        let mut producer = SharedFrameProducer::open(consumer.descriptor().clone()).unwrap();
        producer.publish(2, 2, 2, 8, &[1; 16]).unwrap();
        assert!(consumer.read_latest(3).unwrap().is_none());
        producer.publish(3, 2, 2, 8, &[2; 16]).unwrap();
        assert_eq!(consumer.read_latest(3).unwrap().unwrap().bytes, [2; 16]);
    }

    #[test]
    fn stale_session_cannot_open_an_existing_mapping() {
        let path = path("session");
        remove_stale_mapping(&path).unwrap();
        let consumer = SharedFrameConsumer::create(path, 5, 6, 2, 2).unwrap();
        let mut stale = consumer.descriptor().clone();
        stale.session_generation += 1;
        assert!(SharedFrameProducer::open(stale).is_err());
    }
}
