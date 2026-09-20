const BYTES_PER_PIXEL: usize = 4;
const SLOT_COUNT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba8Srgb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameMetadata {
    pub session_generation: u64,
    pub document_revision: u64,
    pub frame_id: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixel_format: PixelFormat,
}

impl FrameMetadata {
    fn byte_len(self) -> Result<usize, FrameError> {
        let minimum_stride = self
            .width
            .checked_mul(BYTES_PER_PIXEL as u32)
            .ok_or(FrameError::DimensionsOverflow)?;
        if self.width == 0 || self.height == 0 || self.stride < minimum_stride {
            return Err(FrameError::InvalidLayout);
        }
        usize::try_from(self.stride)
            .ok()
            .and_then(|stride| stride.checked_mul(self.height as usize))
            .ok_or(FrameError::DimensionsOverflow)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    DimensionsOverflow,
    InvalidLayout,
    ExceedsCapacity,
    PayloadLength,
}

#[derive(Default)]
struct FrameSlot {
    metadata: Option<FrameMetadata>,
    bytes: Vec<u8>,
}

pub struct FrameView<'a> {
    pub metadata: FrameMetadata,
    pub bytes: &'a [u8],
}

/// Phase 0 in-process benchmark model retained by the offscreen example.
/// Production Preview uses `keine_authoring::SharedFrameConsumer` instead.
pub struct LatestFrameBuffer {
    slots: [FrameSlot; SLOT_COUNT],
    capacity: usize,
    next_slot: usize,
    latest_slot: Option<usize>,
}

impl LatestFrameBuffer {
    pub fn new(max_width: u32, max_height: u32) -> Result<Self, FrameError> {
        let capacity = FrameMetadata {
            session_generation: 0,
            document_revision: 0,
            frame_id: 0,
            width: max_width,
            height: max_height,
            stride: max_width
                .checked_mul(BYTES_PER_PIXEL as u32)
                .ok_or(FrameError::DimensionsOverflow)?,
            pixel_format: PixelFormat::Rgba8Srgb,
        }
        .byte_len()?;
        Ok(Self {
            slots: std::array::from_fn(|_| FrameSlot {
                metadata: None,
                bytes: Vec::with_capacity(capacity),
            }),
            capacity,
            next_slot: 0,
            latest_slot: None,
        })
    }

    pub fn publish(&mut self, metadata: FrameMetadata, bytes: &[u8]) -> Result<(), FrameError> {
        let byte_len = metadata.byte_len()?;
        if byte_len > self.capacity {
            return Err(FrameError::ExceedsCapacity);
        }
        if bytes.len() != byte_len {
            return Err(FrameError::PayloadLength);
        }
        let slot_index = self.next_slot;
        let slot = &mut self.slots[slot_index];
        slot.bytes.clear();
        slot.bytes.extend_from_slice(bytes);
        slot.metadata = Some(metadata);
        self.latest_slot = Some(slot_index);
        self.next_slot = (slot_index + 1) % SLOT_COUNT;
        Ok(())
    }

    pub fn latest(&self) -> Option<FrameView<'_>> {
        let slot = &self.slots[self.latest_slot?];
        Some(FrameView {
            metadata: slot.metadata?,
            bytes: &slot.bytes,
        })
    }

    pub const fn allocated_capacity(&self) -> usize {
        self.capacity * SLOT_COUNT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(frame_id: u64, width: u32, height: u32) -> FrameMetadata {
        FrameMetadata {
            session_generation: 4,
            document_revision: 8,
            frame_id,
            width,
            height,
            stride: width * BYTES_PER_PIXEL as u32,
            pixel_format: PixelFormat::Rgba8Srgb,
        }
    }

    #[test]
    fn latest_frame_wins_without_unbounded_growth() {
        let mut frames = LatestFrameBuffer::new(4, 2).unwrap();
        let capacity = frames.allocated_capacity();
        for frame_id in 0..20 {
            frames
                .publish(metadata(frame_id, 4, 2), &[frame_id as u8; 32])
                .unwrap();
        }
        let latest = frames.latest().unwrap();
        assert_eq!(latest.metadata.frame_id, 19);
        assert_eq!(latest.bytes, [19; 32]);
        assert_eq!(frames.allocated_capacity(), capacity);
    }

    #[test]
    fn rejects_invalid_or_oversized_frames() {
        let mut frames = LatestFrameBuffer::new(4, 2).unwrap();
        assert_eq!(
            frames.publish(metadata(1, 4, 2), &[0; 31]),
            Err(FrameError::PayloadLength)
        );
        assert_eq!(
            frames.publish(metadata(2, 8, 2), &[0; 64]),
            Err(FrameError::ExceedsCapacity)
        );
        let mut invalid = metadata(3, 4, 2);
        invalid.stride = 4;
        assert_eq!(
            frames.publish(invalid, &[0; 8]),
            Err(FrameError::InvalidLayout)
        );
    }
}
