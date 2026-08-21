use std::io;

use bevy::asset::io::Reader;
use futures_lite::io::AsyncReadExt;

const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Read an asset into memory without reallocating its complete prefix for each
/// chunk. `try_reserve_exact` keeps the requested capacity inside the caller's
/// budget; the geometric target avoids the exact-size growth pattern that
/// would otherwise copy the buffer once per 64 KiB read.
pub(crate) async fn read_to_end(
    reader: &mut dyn Reader,
    maximum: usize,
    label: &str,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; READ_CHUNK_BYTES];
    loop {
        let read = AsyncReadExt::read(reader, &mut chunk).await?;
        if read == 0 {
            return Ok(bytes);
        }
        let new_len = bytes
            .len()
            .checked_add(read)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "asset size overflow"))?;
        if new_len > maximum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{label} exceeds the {maximum}-byte input limit"),
            ));
        }
        if bytes.capacity() < new_len {
            let target = next_capacity(bytes.capacity(), new_len, maximum);
            bytes
                .try_reserve_exact(target - bytes.len())
                .map_err(|error| {
                    io::Error::other(format!("failed to reserve {label} input: {error}"))
                })?;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

fn next_capacity(current: usize, required: usize, maximum: usize) -> usize {
    current
        .saturating_mul(2)
        .max(READ_CHUNK_BYTES)
        .min(maximum)
        .max(required)
}

#[cfg(test)]
mod tests {
    use bevy::asset::io::VecReader;

    use super::*;

    #[test]
    fn accepts_the_boundary_and_rejects_one_more_byte() {
        let accepted = futures_lite::future::block_on(read_to_end(
            &mut VecReader::new(vec![1; 4]),
            4,
            "fixture",
        ))
        .unwrap();
        assert_eq!(accepted, vec![1; 4]);

        let error = futures_lite::future::block_on(read_to_end(
            &mut VecReader::new(vec![1; 5]),
            4,
            "fixture",
        ))
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("4-byte input limit"));
    }

    #[test]
    fn capacity_growth_is_geometric_and_capped() {
        assert_eq!(next_capacity(0, 1, usize::MAX), READ_CHUNK_BYTES);
        assert_eq!(
            next_capacity(READ_CHUNK_BYTES, READ_CHUNK_BYTES + 1, usize::MAX),
            READ_CHUNK_BYTES * 2
        );
        assert_eq!(next_capacity(8, 9, 10), 10);
    }
}
