#![warn(unused_crate_dependencies)]

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientMessage {
    pub generation: u64,
    pub request_id: u64,
    pub command: ClientCommand,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClientCommand {
    Hello {
        protocol_version: u32,
        token: String,
        editor_version: String,
    },
    OpenProject {
        path: PathBuf,
    },
    CloseProject,
    Validate,
    Start,
    Pause,
    Resume,
    Stop,
    Ping,
    Shutdown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerMessage {
    pub generation: u64,
    pub request_id: u64,
    pub response: ServerResponse,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServerResponse {
    Hello {
        protocol_version: u32,
        engine_version: String,
        build_id: String,
        project_formats: Vec<String>,
        capabilities: Vec<Capability>,
    },
    ProjectOpened {
        root: PathBuf,
        title: String,
    },
    ProjectClosed,
    Validation(ValidationReport),
    Lifecycle {
        state: LifecycleState,
    },
    Pong,
    Bye,
    Error {
        code: ErrorCode,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    NativeProject,
    LetsGalProject,
    WebGalProject,
    Validate,
    NoFramePreview,
    Lifecycle,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Ready,
    Running,
    Paused,
    Stopped,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Authentication,
    ProtocolMismatch,
    InvalidRequest,
    ProjectNotOpen,
    ProjectOpenFailed,
    ValidationFailed,
    Internal,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationReport {
    pub title: String,
    pub scenes: usize,
    pub actions: usize,
    pub sources: usize,
    pub warnings: usize,
    pub errors: usize,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Warning,
    Error,
}

pub fn write_message(writer: &mut impl Write, message: &impl Serialize) -> io::Result<()> {
    let bytes = postcard::to_stdvec(message).map_err(io::Error::other)?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "authoring message exceeds the 256 KiB limit",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

pub fn read_message<T: DeserializeOwned>(reader: &mut impl BufRead) -> io::Result<Option<T>> {
    if reader.fill_buf()?.is_empty() {
        return Ok(None);
    }
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "authoring message length is outside the bounded envelope",
        ));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    postcard::from_bytes(&bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::*;

    #[test]
    fn message_round_trip_is_length_delimited_and_versioned() {
        let message = ClientMessage {
            generation: 7,
            request_id: 9,
            command: ClientCommand::Hello {
                protocol_version: PROTOCOL_VERSION,
                token: "nonce".into(),
                editor_version: "0.9.1".into(),
            },
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &message).unwrap();
        assert_eq!(
            u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize,
            bytes.len() - 4
        );
        let decoded = read_message(&mut BufReader::new(bytes.as_slice()))
            .unwrap()
            .unwrap();
        assert_eq!(message, decoded);
    }

    #[test]
    fn oversized_or_truncated_messages_fail_closed() {
        let oversized = ((MAX_MESSAGE_BYTES + 1) as u32).to_be_bytes();
        assert!(read_message::<ClientMessage>(&mut BufReader::new(oversized.as_slice())).is_err());
        assert!(
            read_message::<ClientMessage>(&mut BufReader::new([0, 0, 0, 8, 1].as_slice())).is_err()
        );
    }
}
