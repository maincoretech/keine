//! Versioned compiled-program codec for `.keine/compiled/program.bin`.
//!
//! V1 layout: a fixed 64-byte envelope header followed by a Postcard
//! metadata section and a Postcard payload section:
//!
//! ```text
//! [0..8)   magic "KEINEPG\0"
//! [8..12)  envelope_version u32 LE
//! [12..16) ir_schema_version u32 LE
//! [16..20) flags u32 LE (0 in V1)
//! [20..24) metadata_len u32 LE
//! [24..32) payload_len u64 LE
//! [32..36) metadata_crc32 u32 LE
//! [36..40) payload_crc32 u32 LE
//! [40..48) program_fingerprint u64 LE
//! [48..64) reserved, must be zero in V1
//! ```
//!
//! Hakutaku already authenticates its encrypted blocks; the envelope CRC32
//! additionally guards the logical file against wrong
//! read ranges, truncation and payload/metadata drift. Schema mismatches must
//! fail loudly instead of silently falling back to source scripts.

use std::collections::HashSet;
use std::fmt;

use keine_core::{Action, Program};
use serde::{Deserialize, Serialize};

use crate::{LoadedScene, ResourceRef, SceneRef};

pub const PROGRAM_MAGIC: [u8; 8] = *b"KEINEPG\0";
pub const ENVELOPE_VERSION: u32 = 1;
pub const IR_SCHEMA_VERSION: u32 = 1;
pub const FIXED_HEADER_LEN: usize = 64;

/// Upper bounds for the envelope. Values follow the v2 plan; they are
/// deliberately conservative hardening, not expected production sizes.
pub const MAX_METADATA_LEN: usize = 1 << 20;
pub const MAX_PAYLOAD_LEN: u64 = 512 << 20;
pub const MAX_SCENE_COUNT: usize = 1_000_000;
pub const MAX_ACTION_COUNT: u64 = 100_000_000;
pub const MAX_STRING_LEN: usize = 16 << 20;

/// Fixed envelope header. Kept private so the on-disk layout lives in one
/// place; schema evolution touches only this type and the version constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    envelope_version: u32,
    ir_schema_version: u32,
    flags: u32,
    metadata_len: u32,
    payload_len: u64,
    metadata_crc: u32,
    payload_crc: u32,
    fingerprint: u64,
}

impl Header {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&PROGRAM_MAGIC);
        out.extend_from_slice(&self.envelope_version.to_le_bytes());
        out.extend_from_slice(&self.ir_schema_version.to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
        out.extend_from_slice(&self.metadata_len.to_le_bytes());
        out.extend_from_slice(&self.payload_len.to_le_bytes());
        out.extend_from_slice(&self.metadata_crc.to_le_bytes());
        out.extend_from_slice(&self.payload_crc.to_le_bytes());
        out.extend_from_slice(&self.fingerprint.to_le_bytes());
        out.extend_from_slice(&[0u8; 16]);
    }

    fn read(bytes: &[u8]) -> Result<Self, CompiledError> {
        if bytes.len() < FIXED_HEADER_LEN {
            return Err(CompiledError::Truncated {
                expected: FIXED_HEADER_LEN,
                found: bytes.len(),
            });
        }
        if bytes[..PROGRAM_MAGIC.len()] != PROGRAM_MAGIC {
            return Err(CompiledError::BadMagic);
        }
        if bytes[48..64].iter().any(|byte| *byte != 0) {
            return Err(CompiledError::ReservedNonZero);
        }
        Ok(Self {
            envelope_version: read_u32(bytes, 8),
            ir_schema_version: read_u32(bytes, 12),
            flags: read_u32(bytes, 16),
            metadata_len: read_u32(bytes, 20),
            payload_len: read_u64(bytes, 24),
            metadata_crc: read_u32(bytes, 32),
            payload_crc: read_u32(bytes, 36),
            fingerprint: read_u64(bytes, 40),
        })
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("header length is validated before field reads"),
    )
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("header length is validated before field reads"),
    )
}

/// One compiled scene: typed actions plus the resource and sub-scene
/// references collected by the source adapter. Source spans and diagnostics
/// are intentionally excluded from V1; the debug symbol table is a later
/// schema version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledSceneV1 {
    pub name: String,
    pub actions: Vec<Action>,
    pub resources: Vec<ResourceRef>,
    pub sub_scenes: Vec<SceneRef>,
}

impl CompiledSceneV1 {
    pub fn from_loaded(scene: &LoadedScene) -> Self {
        Self {
            name: scene.name.clone(),
            actions: scene.actions.clone(),
            resources: scene.resources.clone(),
            sub_scenes: scene.sub_scenes.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CompiledProgramV1 {
    pub scenes: Vec<CompiledSceneV1>,
}

#[derive(Serialize)]
struct CompiledProgramRef<'a> {
    scenes: &'a [CompiledSceneV1],
}

impl CompiledProgramV1 {
    pub fn from_loaded(scenes: &[LoadedScene]) -> Self {
        Self {
            scenes: scenes.iter().map(CompiledSceneV1::from_loaded).collect(),
        }
    }
}

/// Build-time provenance. `created_unix` is deliberately absent so identical
/// sources produce byte-identical artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramMetadataV1 {
    pub compiler_version: String,
    pub engine_version: String,
    pub source_adapter: String,
    pub scene_count: u32,
    pub action_count: u64,
    pub source_manifest_hash: u64,
}

/// Input to [`encode`]. `fingerprint` is the identity of the source-built
/// `Program` (`Program::from_scenes(...).fingerprint()`) and is compared by
/// the runtime after reconstruction.
#[derive(Debug, Clone)]
pub struct EncodeInput {
    pub scenes: Vec<CompiledSceneV1>,
    pub metadata: ProgramMetadataV1,
    pub fingerprint: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedProgram {
    pub metadata: ProgramMetadataV1,
    pub scenes: Vec<CompiledSceneV1>,
    pub fingerprint: u64,
}

#[derive(Debug)]
pub enum CompiledError {
    BadMagic,
    UnsupportedEnvelope(u32),
    UnsupportedFlags(u32),
    UnsupportedSchema {
        expected: u32,
        found: u32,
    },
    Truncated {
        expected: usize,
        found: usize,
    },
    TrailingBytes {
        expected: usize,
        found: usize,
    },
    TrailingSectionBytes {
        section: &'static str,
        count: usize,
    },
    ReservedNonZero,
    MetadataTooLarge(usize),
    PayloadTooLarge(u64),
    MetadataCrcMismatch {
        expected: u32,
        actual: u32,
    },
    PayloadCrcMismatch {
        expected: u32,
        actual: u32,
    },
    TooManyScenes(usize),
    TooManyActions(u64),
    StringTooLong,
    DuplicateScene(String),
    MetadataCountMismatch {
        field: &'static str,
        expected: u64,
        actual: u64,
    },
    FingerprintMismatch {
        expected: u64,
        actual: u64,
    },
    Payload(postcard::Error),
    Metadata(postcard::Error),
}

impl fmt::Display for CompiledError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => write!(f, "compiled program: invalid magic"),
            Self::UnsupportedEnvelope(version) => {
                write!(
                    f,
                    "compiled program: unsupported envelope version {version}"
                )
            }
            Self::UnsupportedFlags(flags) => {
                write!(f, "compiled program: unsupported envelope flags {flags:#x}")
            }
            Self::UnsupportedSchema { expected, found } => write!(
                f,
                "compiled program schema {found} is unsupported; runtime requires schema {expected}"
            ),
            Self::Truncated { expected, found } => write!(
                f,
                "compiled program: truncated header ({found} bytes, expected at least {expected})"
            ),
            Self::TrailingBytes { expected, found } => write!(
                f,
                "compiled program: length mismatch ({found} bytes, expected {expected})"
            ),
            Self::TrailingSectionBytes { section, count } => write!(
                f,
                "compiled program: {section} contains {count} trailing byte(s)"
            ),
            Self::ReservedNonZero => {
                write!(f, "compiled program: reserved header bytes are not zero")
            }
            Self::MetadataTooLarge(len) => {
                write!(f, "compiled program: metadata exceeds {len} bytes")
            }
            Self::PayloadTooLarge(len) => {
                write!(f, "compiled program: payload exceeds {len} bytes")
            }
            Self::MetadataCrcMismatch { expected, actual } => write!(
                f,
                "compiled program: metadata CRC mismatch (expected {expected:08x}, got {actual:08x})"
            ),
            Self::PayloadCrcMismatch { expected, actual } => write!(
                f,
                "compiled program: payload CRC mismatch (expected {expected:08x}, got {actual:08x})"
            ),
            Self::TooManyScenes(count) => {
                write!(f, "compiled program: scene count {count} exceeds limit")
            }
            Self::TooManyActions(count) => {
                write!(f, "compiled program: action count {count} exceeds limit")
            }
            Self::StringTooLong => {
                write!(f, "compiled program: string exceeds {MAX_STRING_LEN} bytes")
            }
            Self::DuplicateScene(name) => {
                write!(f, "compiled program: duplicate scene {name:?}")
            }
            Self::MetadataCountMismatch {
                field,
                expected,
                actual,
            } => write!(
                f,
                "compiled program: metadata {field} mismatch (expected {expected}, got {actual})"
            ),
            Self::FingerprintMismatch { expected, actual } => write!(
                f,
                "compiled program: fingerprint mismatch (expected {expected:016x}, got {actual:016x})"
            ),
            Self::Payload(error) => write!(f, "compiled program: invalid payload: {error}"),
            Self::Metadata(error) => write!(f, "compiled program: invalid metadata: {error}"),
        }
    }
}

impl std::error::Error for CompiledError {}

/// Serialize scenes, metadata and fingerprint into a versioned program.bin.
pub fn encode(input: &EncodeInput) -> Result<Vec<u8>, CompiledError> {
    validate_semantics(&input.scenes, &input.metadata)?;
    validate_fingerprint(&input.scenes, input.fingerprint)?;
    let payload = postcard::to_stdvec(&CompiledProgramRef {
        scenes: &input.scenes,
    })
    .map_err(CompiledError::Payload)?;
    if payload.len() as u64 > MAX_PAYLOAD_LEN {
        return Err(CompiledError::PayloadTooLarge(payload.len() as u64));
    }
    let metadata = postcard::to_stdvec(&input.metadata).map_err(CompiledError::Metadata)?;
    if metadata.len() > MAX_METADATA_LEN {
        return Err(CompiledError::MetadataTooLarge(metadata.len()));
    }

    let header = Header {
        envelope_version: ENVELOPE_VERSION,
        ir_schema_version: IR_SCHEMA_VERSION,
        flags: 0,
        metadata_len: metadata.len() as u32,
        payload_len: payload.len() as u64,
        metadata_crc: crc32fast::hash(&metadata),
        payload_crc: crc32fast::hash(&payload),
        fingerprint: input.fingerprint,
    };
    let mut out = Vec::with_capacity(FIXED_HEADER_LEN + metadata.len() + payload.len());
    header.write(&mut out);
    out.extend_from_slice(&metadata);
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Decode and validate a program.bin produced by [`encode`].
pub fn decode(bytes: &[u8], expected_schema: u32) -> Result<DecodedProgram, CompiledError> {
    let header = Header::read(bytes)?;
    if header.envelope_version != ENVELOPE_VERSION {
        return Err(CompiledError::UnsupportedEnvelope(header.envelope_version));
    }
    if header.flags != 0 {
        return Err(CompiledError::UnsupportedFlags(header.flags));
    }
    if header.ir_schema_version != expected_schema {
        return Err(CompiledError::UnsupportedSchema {
            expected: expected_schema,
            found: header.ir_schema_version,
        });
    }
    if header.metadata_len as usize > MAX_METADATA_LEN {
        return Err(CompiledError::MetadataTooLarge(
            header.metadata_len as usize,
        ));
    }
    if header.payload_len > MAX_PAYLOAD_LEN {
        return Err(CompiledError::PayloadTooLarge(header.payload_len));
    }
    let expected = FIXED_HEADER_LEN
        .checked_add(header.metadata_len as usize)
        .and_then(|len| len.checked_add(header.payload_len as usize))
        .ok_or(CompiledError::PayloadTooLarge(header.payload_len))?;
    if bytes.len() != expected {
        return Err(CompiledError::TrailingBytes {
            expected,
            found: bytes.len(),
        });
    }

    let metadata_start = FIXED_HEADER_LEN;
    let metadata_end = metadata_start + header.metadata_len as usize;
    let metadata_bytes = &bytes[metadata_start..metadata_end];
    let payload_bytes = &bytes[metadata_end..];
    let actual = crc32fast::hash(metadata_bytes);
    if actual != header.metadata_crc {
        return Err(CompiledError::MetadataCrcMismatch {
            expected: header.metadata_crc,
            actual,
        });
    }
    let actual = crc32fast::hash(payload_bytes);
    if actual != header.payload_crc {
        return Err(CompiledError::PayloadCrcMismatch {
            expected: header.payload_crc,
            actual,
        });
    }

    let (metadata, remaining): (ProgramMetadataV1, _) =
        postcard::take_from_bytes(metadata_bytes).map_err(CompiledError::Metadata)?;
    if !remaining.is_empty() {
        return Err(CompiledError::TrailingSectionBytes {
            section: "metadata",
            count: remaining.len(),
        });
    }
    let (program, remaining): (CompiledProgramV1, _) =
        postcard::take_from_bytes(payload_bytes).map_err(CompiledError::Payload)?;
    if !remaining.is_empty() {
        return Err(CompiledError::TrailingSectionBytes {
            section: "payload",
            count: remaining.len(),
        });
    }
    validate_semantics(&program.scenes, &metadata)?;
    validate_fingerprint(&program.scenes, header.fingerprint)?;
    Ok(DecodedProgram {
        metadata,
        scenes: program.scenes,
        fingerprint: header.fingerprint,
    })
}

fn validate_fingerprint(scenes: &[CompiledSceneV1], expected: u64) -> Result<(), CompiledError> {
    let actual = Program::fingerprint_scenes(
        scenes
            .iter()
            .map(|scene| (scene.name.as_str(), scene.actions.as_slice())),
    );
    if actual != expected {
        return Err(CompiledError::FingerprintMismatch { expected, actual });
    }
    Ok(())
}

fn validate_semantics(
    scenes: &[CompiledSceneV1],
    metadata: &ProgramMetadataV1,
) -> Result<(), CompiledError> {
    if scenes.len() > MAX_SCENE_COUNT {
        return Err(CompiledError::TooManyScenes(scenes.len()));
    }
    let action_count = scenes.iter().try_fold(0u64, |total, scene| {
        total
            .checked_add(scene.actions.len() as u64)
            .ok_or(CompiledError::TooManyActions(total))
    })?;
    if action_count > MAX_ACTION_COUNT {
        return Err(CompiledError::TooManyActions(action_count));
    }
    if scenes.len() as u32 != metadata.scene_count {
        return Err(CompiledError::MetadataCountMismatch {
            field: "scene_count",
            expected: metadata.scene_count as u64,
            actual: scenes.len() as u64,
        });
    }
    if action_count != metadata.action_count {
        return Err(CompiledError::MetadataCountMismatch {
            field: "action_count",
            expected: metadata.action_count,
            actual: action_count,
        });
    }
    let mut names = HashSet::with_capacity(scenes.len());
    for scene in scenes {
        if scene.name.len() > MAX_STRING_LEN {
            return Err(CompiledError::StringTooLong);
        }
        if !names.insert(scene.name.as_str()) {
            return Err(CompiledError::DuplicateScene(scene.name.clone()));
        }
        for resource in &scene.resources {
            if resource.path.len() > MAX_STRING_LEN {
                return Err(CompiledError::StringTooLong);
            }
        }
        for sub_scene in &scene.sub_scenes {
            if sub_scene.scene.len() > MAX_STRING_LEN {
                return Err(CompiledError::StringTooLong);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use keine_core::Program;

    fn fixture_input() -> EncodeInput {
        let scenes = vec![CompiledSceneV1 {
            name: "start".to_string(),
            actions: vec![
                Action::Comment,
                Action::Set {
                    name: "score".to_string(),
                    expression: "1 + 1".to_string(),
                    global: false,
                },
            ],
            resources: vec![],
            sub_scenes: vec![],
        }];
        let program = Program::from_scenes(
            scenes
                .iter()
                .map(|scene| (scene.name.clone(), scene.actions.clone())),
        );
        EncodeInput {
            scenes,
            metadata: ProgramMetadataV1 {
                compiler_version: "0.8.1".to_string(),
                engine_version: "0.8.1".to_string(),
                source_adapter: "webgal".to_string(),
                scene_count: 1,
                action_count: 2,
                source_manifest_hash: 0,
            },
            fingerprint: program.fingerprint(),
        }
    }

    #[test]
    fn roundtrip_preserves_scenes_metadata_and_fingerprint() {
        let input = fixture_input();
        let bytes = encode(&input).unwrap();
        let decoded = decode(&bytes, IR_SCHEMA_VERSION).unwrap();
        assert_eq!(decoded.metadata, input.metadata);
        assert_eq!(decoded.scenes, input.scenes);
        assert_eq!(decoded.fingerprint, input.fingerprint);
    }

    #[test]
    fn encoding_is_reproducible() {
        let input = fixture_input();
        assert_eq!(encode(&input).unwrap(), encode(&input).unwrap());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = encode(&fixture_input()).unwrap();
        bytes[0] = b'X';
        assert!(matches!(
            decode(&bytes, IR_SCHEMA_VERSION),
            Err(CompiledError::BadMagic)
        ));
    }

    #[test]
    fn rejects_unsupported_envelope_and_schema() {
        let mut bytes = encode(&fixture_input()).unwrap();
        bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
        assert!(matches!(
            decode(&bytes, IR_SCHEMA_VERSION),
            Err(CompiledError::UnsupportedEnvelope(2))
        ));

        let mut bytes = encode(&fixture_input()).unwrap();
        bytes[12..16].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(
            decode(&bytes, IR_SCHEMA_VERSION),
            Err(CompiledError::UnsupportedSchema {
                expected: 1,
                found: 99
            })
        ));

        let mut bytes = encode(&fixture_input()).unwrap();
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        assert!(matches!(
            decode(&bytes, IR_SCHEMA_VERSION),
            Err(CompiledError::UnsupportedFlags(1))
        ));
    }

    #[test]
    fn rejects_truncation_trailing_bytes_and_reserved_nonzero() {
        let bytes = encode(&fixture_input()).unwrap();
        assert!(matches!(
            decode(&bytes[..20], IR_SCHEMA_VERSION),
            Err(CompiledError::Truncated { .. })
        ));

        let mut padded = bytes.clone();
        padded.push(0);
        assert!(matches!(
            decode(&padded, IR_SCHEMA_VERSION),
            Err(CompiledError::TrailingBytes { .. })
        ));

        let mut bytes = encode(&fixture_input()).unwrap();
        bytes[48] = 1;
        assert!(matches!(
            decode(&bytes, IR_SCHEMA_VERSION),
            Err(CompiledError::ReservedNonZero)
        ));
    }

    #[test]
    fn rejects_corrupted_crc() {
        let mut bytes = encode(&fixture_input()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        assert!(matches!(
            decode(&bytes, IR_SCHEMA_VERSION),
            Err(CompiledError::PayloadCrcMismatch { .. })
        ));

        let mut bytes = encode(&fixture_input()).unwrap();
        bytes[FIXED_HEADER_LEN] ^= 0xFF;
        assert!(matches!(
            decode(&bytes, IR_SCHEMA_VERSION),
            Err(CompiledError::MetadataCrcMismatch { .. })
        ));
    }

    #[test]
    fn rejects_trailing_section_data_and_fingerprint_drift() {
        let mut bytes = encode(&fixture_input()).unwrap();
        let metadata_len = read_u32(&bytes, 20) as usize;
        let extra_at = FIXED_HEADER_LEN + metadata_len;
        bytes.insert(extra_at, 0);
        bytes[20..24].copy_from_slice(&((metadata_len + 1) as u32).to_le_bytes());
        let crc = crc32fast::hash(&bytes[FIXED_HEADER_LEN..=extra_at]);
        bytes[32..36].copy_from_slice(&crc.to_le_bytes());
        assert!(matches!(
            decode(&bytes, IR_SCHEMA_VERSION),
            Err(CompiledError::TrailingSectionBytes {
                section: "metadata",
                count: 1
            })
        ));

        let mut bytes = encode(&fixture_input()).unwrap();
        bytes[40..48].copy_from_slice(&0u64.to_le_bytes());
        assert!(matches!(
            decode(&bytes, IR_SCHEMA_VERSION),
            Err(CompiledError::FingerprintMismatch { expected: 0, .. })
        ));
    }

    #[test]
    fn rejects_duplicate_scenes_and_count_mismatch() {
        let mut input = fixture_input();
        input.scenes.push(input.scenes[0].clone());
        input.metadata.scene_count = 2;
        input.metadata.action_count = 4;
        assert!(matches!(
            encode(&input),
            Err(CompiledError::DuplicateScene(name)) if name == "start"
        ));

        let mut input = fixture_input();
        input.metadata.action_count = 99;
        assert!(matches!(
            encode(&input),
            Err(CompiledError::MetadataCountMismatch {
                field: "action_count",
                ..
            })
        ));
    }

    #[test]
    fn golden_fixture_matches_expected_bytes() {
        let input = fixture_input();
        let bytes = encode(&input).unwrap();
        let expected = const_hex();
        assert_eq!(bytes, expected);
    }

    fn const_hex() -> Vec<u8> {
        const HEX: &str = "4b45494e45504700010000000100000000000000160000001900000000000000200f16e1d813f0f720c8f62ed624edd80000000000000000000000000000000005302e382e3105302e382e310677656267616c01020001057374617274021c0f0573636f72650531202b2031000000";
        (0..HEX.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&HEX[index..index + 2], 16).unwrap())
            .collect()
    }
}
