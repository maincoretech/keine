mod keine {
    use std::io::Read;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::{Context, Result, bail};
    use keine_core::State;
    use serde::{Deserialize, Serialize};

    use super::{SavedState, StoreAdapter, StoreMetadata, StoreStatus};

    const MAGIC: [u8; 8] = *b"KEINE\0\0\0";
    const VERSION: u32 = 9;
    const HEADER_SIZE: usize = 28;
    const MAX_METADATA_SIZE: usize = 64 * 1024;
    const MAX_STATE_SIZE: usize = 64 * 1024 * 1024;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct KeineStore;

    impl StoreAdapter for KeineStore {
        fn name(&self) -> &'static str {
            "keine"
        }

        fn extension(&self) -> &'static str {
            "sav"
        }

        fn encode(&self, state: &State) -> Result<Vec<u8>> {
            encode_at(
                state,
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            )
        }

        fn decode(&self, bytes: &[u8]) -> Result<SavedState> {
            let (header, metadata, state) = sections(bytes)?;
            if header.version != VERSION {
                bail!("unsupported save version {}", header.version);
            }
            if crc32fast::hash(metadata) != header.metadata_checksum {
                bail!("save metadata failed its integrity check");
            }
            if crc32fast::hash(state) != header.state_checksum {
                bail!("save state failed its integrity check");
            }
            let _metadata: SerializedMetadata =
                postcard::from_bytes(metadata).context("invalid save metadata")?;
            postcard::from_bytes(state)
                .map(SavedState::new)
                .context("failed to deserialize game state")
        }

        fn inspect(&self, reader: &mut dyn Read) -> Result<StoreStatus> {
            let prefix = inspection_prefix(reader)?;
            Ok(self.inspect_prefix(&prefix))
        }

        fn inspect_prefix(&self, prefix: &[u8]) -> StoreStatus {
            let Ok((header, metadata)) = metadata_section(prefix) else {
                return StoreStatus::Corrupt;
            };
            if header.version != VERSION {
                return StoreStatus::Unsupported(header.version);
            }
            if crc32fast::hash(metadata) != header.metadata_checksum {
                return StoreStatus::Corrupt;
            }
            match postcard::from_bytes::<SerializedMetadata>(metadata) {
                Ok(metadata) => StoreStatus::Ready(metadata.into()),
                Err(_) => StoreStatus::Corrupt,
            }
        }
    }

    fn encode_at(state: &State, saved_at_unix: u64) -> Result<Vec<u8>> {
        let metadata = postcard::to_stdvec(&metadata(state, saved_at_unix))
            .context("failed to serialize save metadata")?;
        let state = postcard::to_stdvec(state).context("failed to serialize game state")?;
        validate_lengths(metadata.len(), state.len())?;
        let mut output = Vec::with_capacity(HEADER_SIZE + metadata.len() + state.len());
        output.extend_from_slice(&encode_header(
            metadata.len(),
            state.len(),
            crc32fast::hash(&metadata),
            crc32fast::hash(&state),
        ));
        output.extend_from_slice(&metadata);
        output.extend_from_slice(&state);
        Ok(output)
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct SerializedMetadata {
        saved_at_unix: u64,
        program_fingerprint: u64,
        scene: String,
        cursor: usize,
        speaker: String,
        text: String,
    }

    impl From<SerializedMetadata> for StoreMetadata {
        fn from(value: SerializedMetadata) -> Self {
            Self {
                saved_at_unix: value.saved_at_unix,
                program_fingerprint: value.program_fingerprint,
                scene: value.scene,
                cursor: value.cursor,
                speaker: value.speaker,
                text: value.text,
            }
        }
    }

    fn metadata(state: &State, saved_at_unix: u64) -> SerializedMetadata {
        let (speaker, text) = state.dialogue.as_ref().map_or_else(
            || (String::new(), String::new()),
            |dialogue| (dialogue.speaker.clone(), dialogue.text.clone()),
        );
        SerializedMetadata {
            saved_at_unix,
            program_fingerprint: state.program_fingerprint,
            scene: state.current_scene.clone(),
            cursor: state.cursor,
            speaker,
            text,
        }
    }

    struct Header {
        version: u32,
        metadata_len: usize,
        state_len: usize,
        metadata_checksum: u32,
        state_checksum: u32,
    }

    fn encode_header(
        metadata_len: usize,
        state_len: usize,
        metadata_checksum: u32,
        state_checksum: u32,
    ) -> [u8; HEADER_SIZE] {
        let mut header = [0; HEADER_SIZE];
        header[..8].copy_from_slice(&MAGIC);
        header[8..12].copy_from_slice(&VERSION.to_le_bytes());
        header[12..16].copy_from_slice(&(metadata_len as u32).to_le_bytes());
        header[16..20].copy_from_slice(&(state_len as u32).to_le_bytes());
        header[20..24].copy_from_slice(&metadata_checksum.to_le_bytes());
        header[24..28].copy_from_slice(&state_checksum.to_le_bytes());
        header
    }

    fn inspection_prefix(reader: &mut dyn Read) -> Result<Vec<u8>> {
        let mut header = [0; HEADER_SIZE];
        reader
            .read_exact(&mut header)
            .context("incomplete save header")?;
        let parsed = parse_header(&header)?;
        if parsed.version != VERSION {
            return Ok(header.to_vec());
        }

        let mut prefix = Vec::with_capacity(HEADER_SIZE + parsed.metadata_len);
        prefix.extend_from_slice(&header);
        let mut metadata = vec![0; parsed.metadata_len];
        reader
            .read_exact(&mut metadata)
            .context("incomplete save metadata")?;
        prefix.extend_from_slice(&metadata);
        Ok(prefix)
    }

    fn parse_header(bytes: &[u8]) -> Result<Header> {
        let header: &[u8; HEADER_SIZE] = bytes
            .get(..HEADER_SIZE)
            .and_then(|bytes| bytes.try_into().ok())
            .context("incomplete save header")?;
        if header[..8] != MAGIC {
            bail!("invalid save signature");
        }
        let header = Header {
            version: u32::from_le_bytes(header[8..12].try_into().unwrap()),
            metadata_len: u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize,
            state_len: u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize,
            metadata_checksum: u32::from_le_bytes(header[20..24].try_into().unwrap()),
            state_checksum: u32::from_le_bytes(header[24..28].try_into().unwrap()),
        };
        validate_lengths(header.metadata_len, header.state_len)?;
        Ok(header)
    }

    fn metadata_section(bytes: &[u8]) -> Result<(Header, &[u8])> {
        let header = parse_header(bytes)?;
        if header.version != VERSION {
            if bytes.len() != HEADER_SIZE {
                bail!("unsupported save prefix contains trailing data");
            }
            return Ok((header, &[]));
        }
        let metadata_end = HEADER_SIZE + header.metadata_len;
        if metadata_end != bytes.len() {
            bail!("save metadata prefix length does not match header");
        }
        Ok((header, &bytes[HEADER_SIZE..metadata_end]))
    }

    fn sections(bytes: &[u8]) -> Result<(Header, &[u8], &[u8])> {
        let header = parse_header(bytes)?;
        let metadata_end = HEADER_SIZE + header.metadata_len;
        let state_end = metadata_end + header.state_len;
        if state_end != bytes.len() {
            bail!("save length does not match header");
        }
        Ok((
            header,
            &bytes[HEADER_SIZE..metadata_end],
            &bytes[metadata_end..state_end],
        ))
    }

    fn validate_lengths(metadata_len: usize, state_len: usize) -> Result<()> {
        if metadata_len == 0 || metadata_len > MAX_METADATA_SIZE {
            bail!("invalid save metadata length {metadata_len}");
        }
        if state_len == 0 || state_len > MAX_STATE_SIZE {
            bail!("invalid save state length {state_len}");
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Cursor;

        use keine_core::state::{Dialogue, DialogueKey, DialogueRetraction};
        use keine_core::{Action, EffectCue, EffectEvent, Program, SayOptions, Value};

        fn inspect(bytes: &[u8]) -> StoreStatus {
            KeineStore.inspect(&mut Cursor::new(bytes)).unwrap()
        }

        #[test]
        fn round_trips_and_inspects_state() {
            let mut state = State::new();
            state.current_scene = "demo".into();
            state.cursor = 42;
            let bytes = KeineStore.encode(&state).unwrap();
            assert_eq!(KeineStore.decode(&bytes).unwrap().snapshot(), &state);
            assert!(matches!(
                inspect(&bytes),
                StoreStatus::Ready(metadata) if metadata.scene == "demo"
            ));
        }

        #[test]
        fn save_size_is_independent_of_compiled_script_size() {
            let actions = (0..2_000)
                .map(|index| Action::Say {
                    speaker: "Archivist".into(),
                    text: format!("compiled line {index}"),
                    options: SayOptions::default(),
                })
                .collect();
            let mut state = State::new();
            state.install_program(Program::from_scenes([("main".into(), actions)]));
            state.current_scene = "main".into();

            let bytes = KeineStore.encode(&state).unwrap();
            let decoded = KeineStore.decode(&bytes).unwrap();

            assert!(
                bytes.len() < 1_024,
                "save unexpectedly contains program data"
            );
            assert!(decoded.snapshot().program.is_empty());
            assert_eq!(decoded.snapshot().current_scene, "main");
        }

        #[test]
        fn inspection_reads_exactly_the_header_and_metadata() {
            let bytes = KeineStore.encode(&State::new()).unwrap();
            let metadata_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
            let mut reader = Cursor::new(&bytes);

            assert!(matches!(
                KeineStore.inspect(&mut reader).unwrap(),
                StoreStatus::Ready(_)
            ));
            assert_eq!(reader.position() as usize, HEADER_SIZE + metadata_len);
            assert!((reader.position() as usize) < bytes.len());
        }

        #[test]
        fn inspect_and_decode_reject_corrupt_metadata() {
            let mut corrupt = KeineStore.encode(&State::new()).unwrap();
            corrupt[HEADER_SIZE] ^= 0xff;

            assert_eq!(inspect(&corrupt), StoreStatus::Corrupt);
            assert!(KeineStore.decode(&corrupt).is_err());
        }

        #[test]
        fn state_integrity_is_checked_only_when_the_state_is_loaded() {
            let bytes = KeineStore.encode(&State::new()).unwrap();
            let mut corrupt = bytes.clone();
            *corrupt.last_mut().unwrap() ^= 0xff;

            assert!(matches!(inspect(&corrupt), StoreStatus::Ready(_)));
            assert!(KeineStore.decode(&corrupt).is_err());
        }

        #[test]
        fn decoded_slot_restores_only_into_the_matching_current_program() {
            let program = Program::from_scenes([("main".into(), vec![Action::Comment])]);
            let mut saved = State::new();
            saved.install_program(program.clone());
            saved.current_scene = "main".into();
            saved.cursor = 1;
            let bytes = KeineStore.encode(&saved).unwrap();

            let mut matching = State::new();
            matching.install_program(program);
            KeineStore
                .decode(&bytes)
                .unwrap()
                .restore_into(&mut matching)
                .unwrap();
            assert_eq!(matching.current_scene, "main");
            assert_eq!(matching.cursor, 1);
            assert!(!matching.program.is_empty());

            let mut different = State::new();
            different.install_program(Program::from_scenes([(
                "changed".into(),
                vec![Action::Comment],
            )]));
            assert!(
                KeineStore
                    .decode(&bytes)
                    .unwrap()
                    .restore_into(&mut different)
                    .is_err()
            );
            assert!(different.program.contains_scene("changed"));
        }

        #[test]
        fn complex_state_round_trip_keeps_slot_fields_and_skips_external_domains() {
            let mut state = State::new();
            state.install_program(Program::from_scenes([(
                "main".into(),
                vec![Action::Comment; 3],
            )]));
            state.current_scene = "main".into();
            state.cursor = 2;
            state.dialogue = Some(Dialogue {
                speaker: "Mio".into(),
                text: "Tomorrow, together.".into(),
                markup: "[Tomorrow](bold), together.".into(),
                visible_chars: 7,
                pauses: Vec::new(),
                vocal: Some("mio.opus".into()),
                volume: 0.6,
                auto_advance: false,
            });
            state.vars.insert(
                "clues".into(),
                Value::Array(vec![Value::Bool(true), Value::Int(17)]),
            );
            state.wait_remaining = 0.25;
            state.wait_blocking = true;
            state.film_mode = true;
            state.dialogue_retraction = Some(DialogueRetraction {
                keep: "Tomorrow".into(),
                target_visible_chars: 8,
                fractional_chars: 0.375,
                awaiting_advance: false,
            });
            state.record_dialogue(1);

            state.global_vars.insert("route".into(), Value::Int(3));
            state.read_dialogues.insert(DialogueKey {
                scene: "main".into(),
                action_index: 1,
            });
            state
                .unlocked_cg
                .insert("ending.webp".into(), "Ending".into());
            state
                .unlocked_bgm
                .insert("theme.opus".into(), "Theme".into());
            state.effect_queue.push(EffectEvent::Play(EffectCue {
                id: None,
                file: "bell.opus".into(),
                volume: 0.4,
                fade_in: 0.0,
            }));

            let decoded = KeineStore
                .decode(&KeineStore.encode(&state).unwrap())
                .unwrap();
            let mut expected = state;
            expected.program = Default::default();
            expected.global_vars.clear();
            expected.read_dialogues.clear();
            expected.unlocked_cg.clear();
            expected.unlocked_bgm.clear();
            expected.effect_queue.clear();

            assert_eq!(decoded.snapshot(), &expected);
            assert_eq!(decoded.snapshot().backlog.len(), 1);
            assert_eq!(
                decoded.snapshot().backlog[0].snapshot.program_fingerprint,
                decoded.snapshot().program_fingerprint
            );
        }

        #[test]
        fn save_v9_golden_is_stable() {
            let mut state = State::new();
            state.install_program(Program::from_scenes([(
                "main".into(),
                vec![Action::Comment],
            )]));
            state.current_scene = "main".into();
            state.cursor = 1;
            let bytes = encode_at(&state, 1_700_000_000).unwrap();
            if std::env::var_os("KEINE_UPDATE_STORE_GOLDEN").is_some() {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/store-v9.sav");
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, &bytes).unwrap();
                return;
            }
            let expected = include_bytes!("../../tests/fixtures/store-v9.sav");

            assert_eq!(bytes.as_slice(), expected);
        }
    }
}

use std::io::Read;

use anyhow::Result;
use keine_core::{RestoreError, State};

pub use keine::KeineStore;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreMetadata {
    pub saved_at_unix: u64,
    pub program_fingerprint: u64,
    pub scene: String,
    pub cursor: usize,
    pub speaker: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreStatus {
    Ready(StoreMetadata),
    Corrupt,
    Unsupported(u32),
}

/// Decoded slot payload that is deliberately not a runnable game state.
///
/// Compiled Program data and profile-scoped fields are absent from save files;
/// callers must merge this payload into the currently loaded project through
/// [`SavedState::restore_into`] instead of replacing `State` directly.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedState(State);

impl SavedState {
    pub(crate) fn new(state: State) -> Self {
        Self(state)
    }

    /// Read-only access for metadata/preview projection. Execution must still
    /// use `restore_into` so Program and profile invariants are restored.
    pub fn snapshot(&self) -> &State {
        &self.0
    }

    pub fn restore_into(self, current: &mut State) -> Result<(), RestoreError> {
        current.restore_saved(self.0)
    }
}

/// Encodes and parses one persistent game-state format. Slot discovery and
/// atomic filesystem replacement remain the engine storage layer's job.
pub trait StoreAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn extension(&self) -> &'static str;
    fn encode(&self, state: &State) -> Result<Vec<u8>>;
    fn decode(&self, bytes: &[u8]) -> Result<SavedState>;

    /// Inspects a slot through an abstract reader so storage backends do not
    /// leak filesystem concerns into format adapters.
    ///
    /// The default keeps future adapters simple by reading the complete
    /// payload. Formats with a self-describing metadata prefix should override
    /// this method and stop reading before the state payload.
    fn inspect(&self, reader: &mut dyn Read) -> Result<StoreStatus> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(self.inspect_prefix(&bytes))
    }

    /// Parses the bytes collected by [`StoreAdapter::inspect`]. For adapters
    /// using the default reader implementation this is the complete payload;
    /// prefix-aware adapters may receive only their header and metadata.
    fn inspect_prefix(&self, prefix: &[u8]) -> StoreStatus;
}
