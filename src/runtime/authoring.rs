use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use bevy::prelude::App;
use keine_authoring::{
    Capability, ClientCommand, ClientMessage, ErrorCode, FrameTransportDescriptor, LifecycleState,
    MAX_DOCUMENT_BYTES, PROTOCOL_VERSION, ServerMessage, ServerResponse, SharedFrameProducer,
    read_message, write_message,
};
use keine_loader::LoaderRegistry;

use super::bootstrap::{
    OpenedProject, build_authoring_preview_app, open_project, validate_project,
};

const ACTIVE_FRAME_INTERVAL: Duration = Duration::from_micros(16_667);
const IDLE_INTERVAL: Duration = Duration::from_millis(250);
static OVERLAY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct PendingSnapshot {
    path: PathBuf,
    revision: u64,
    total_bytes: usize,
    bytes: Vec<u8>,
}

struct PreviewOverlay {
    root: PathBuf,
}

impl PreviewOverlay {
    fn create() -> io::Result<Self> {
        let sequence = OVERLAY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("keine-authoring-{}-{sequence}", std::process::id()));
        fs::create_dir_all(root.join("scripts"))?;
        Ok(Self { root })
    }

    fn write_script(&self, relative_path: &Path, contents: &[u8]) -> io::Result<()> {
        let target = self.root.join(relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, contents)
    }
}

impl Drop for PreviewOverlay {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Session {
    project: Option<OpenedProject>,
    project_path: Option<PathBuf>,
    lifecycle: LifecycleState,
    runtime: Option<App>,
    transport: Option<FrameTransportDescriptor>,
    document_revision: u64,
    pending_snapshot: Option<PendingSnapshot>,
    documents: HashMap<PathBuf, Vec<u8>>,
    overlay: PreviewOverlay,
}

impl Session {
    fn new() -> io::Result<Self> {
        Ok(Self {
            project: None,
            project_path: None,
            lifecycle: LifecycleState::Stopped,
            runtime: None,
            transport: None,
            document_revision: 0,
            pending_snapshot: None,
            documents: HashMap::new(),
            overlay: PreviewOverlay::create()?,
        })
    }

    fn stop(&mut self) {
        self.runtime = None;
        self.transport = None;
        self.lifecycle = LifecycleState::Stopped;
    }

    fn rebuild_runtime(&mut self, loader: &LoaderRegistry) -> Result<()> {
        let project_path = self.project_path.as_deref().context("no project is open")?;
        let transport = self
            .transport
            .clone()
            .context("preview transport is not configured")?;
        let producer = SharedFrameProducer::open(transport)?;
        let paused = self.lifecycle == LifecycleState::Paused;
        let mut runtime = build_authoring_preview_app(
            project_path,
            Some(&self.overlay.root),
            loader,
            super::preview::AuthoringPreviewConfig {
                producer,
                document_revision: self.document_revision,
            },
        )?;
        runtime.finish();
        runtime.cleanup();
        super::preview::set_paused(&mut runtime, paused);
        runtime.update();
        self.runtime = Some(runtime);
        Ok(())
    }

    fn update_runtime(&mut self) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.update();
        }
    }

    fn next_interval(&self) -> Duration {
        match (&self.runtime, self.lifecycle) {
            (Some(runtime), LifecycleState::Running)
                if super::preview::wants_continuous_updates(runtime) =>
            {
                ACTIVE_FRAME_INTERVAL
            }
            _ => IDLE_INTERVAL,
        }
    }
}

pub(crate) fn run(endpoint: &str, token: &str, loader: LoaderRegistry) -> Result<()> {
    let endpoint: SocketAddr = endpoint
        .parse()
        .with_context(|| format!("invalid authoring endpoint {endpoint:?}"))?;
    if !endpoint.ip().is_loopback() {
        anyhow::bail!("authoring endpoint must use a loopback address");
    }
    let mut stream = TcpStream::connect_timeout(&endpoint, Duration::from_secs(5))
        .context("failed to connect to the editor authoring endpoint")?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);

    let hello: ClientMessage = read_message(&mut reader)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing authoring hello"))?;
    let (protocol_version, presented_token) = match &hello.command {
        ClientCommand::Hello {
            protocol_version,
            token,
            ..
        } => (*protocol_version, token.as_str()),
        _ => {
            send_error(
                &mut stream,
                &hello,
                ErrorCode::InvalidRequest,
                "the first command must be hello",
            )?;
            anyhow::bail!("the first authoring command was not hello");
        }
    };
    if presented_token != token {
        send_error(
            &mut stream,
            &hello,
            ErrorCode::Authentication,
            "authoring token rejected",
        )?;
        anyhow::bail!("authoring token rejected");
    }
    if protocol_version != PROTOCOL_VERSION {
        send_error(
            &mut stream,
            &hello,
            ErrorCode::ProtocolMismatch,
            &format!(
                "editor protocol {protocol_version} is incompatible with engine protocol {PROTOCOL_VERSION}"
            ),
        )?;
        anyhow::bail!("authoring protocol mismatch");
    }
    send(
        &mut stream,
        &hello,
        ServerResponse::Hello {
            protocol_version: PROTOCOL_VERSION,
            engine_version: env!("CARGO_PKG_VERSION").into(),
            build_id: option_env!("KEINE_BUILD_ID")
                .unwrap_or("development")
                .into(),
            project_formats: vec!["native".into(), "letsgal".into(), "webgal".into()],
            capabilities: vec![
                Capability::NativeProject,
                Capability::LetsGalProject,
                Capability::WebGalProject,
                Capability::Validate,
                Capability::RawFramePreview,
                Capability::SourceSnapshots,
                Capability::RuntimeInput,
                Capability::SourceCursor,
                Capability::Lifecycle,
            ],
        },
    )?;

    let generation = hello.generation;
    stream.set_read_timeout(None)?;
    let messages = spawn_reader(reader);
    let mut session = Session::new()?;
    let mut last_update = Instant::now();
    loop {
        let timeout = session
            .next_interval()
            .saturating_sub(last_update.elapsed());
        match messages.recv_timeout(timeout) {
            Ok(Ok(Some(message))) => {
                if message.generation != generation {
                    send_error(
                        &mut stream,
                        &message,
                        ErrorCode::InvalidRequest,
                        "stale authoring session generation",
                    )?;
                    continue;
                }
                let shutdown = matches!(message.command, ClientCommand::Shutdown);
                let response = handle(&loader, &mut session, generation, &message.command);
                match response {
                    Ok(response) => send(&mut stream, &message, response)?,
                    Err((code, message_text)) => {
                        send_error(&mut stream, &message, code, &message_text)?
                    }
                }
                if shutdown {
                    break;
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(error)) => return Err(error.into()),
            Err(RecvTimeoutError::Timeout) => {
                session.update_runtime();
                last_update = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    session.stop();
    Ok(())
}

fn spawn_reader(mut reader: BufReader<TcpStream>) -> Receiver<io::Result<Option<ClientMessage>>> {
    let (sender, receiver) = mpsc::sync_channel(8);
    thread::spawn(move || {
        loop {
            let message = read_message::<ClientMessage>(&mut reader);
            let finished = !matches!(message, Ok(Some(_)));
            if sender.send(message).is_err() || finished {
                break;
            }
        }
    });
    receiver
}

fn handle(
    loader: &LoaderRegistry,
    session: &mut Session,
    generation: u64,
    command: &ClientCommand,
) -> Result<ServerResponse, (ErrorCode, String)> {
    match command {
        ClientCommand::Hello { .. } => Err((
            ErrorCode::InvalidRequest,
            "hello may only be sent once".into(),
        )),
        ClientCommand::OpenProject { path } => match open_project(path, loader) {
            Ok(project) => {
                session.stop();
                session.documents.clear();
                let root = project.root.clone();
                let title = project.config.title.clone();
                session.project_path = Some(root.clone());
                session.project = Some(project);
                session.lifecycle = LifecycleState::Ready;
                Ok(ServerResponse::ProjectOpened { root, title })
            }
            Err(error) => Err((ErrorCode::ProjectOpenFailed, format!("{error:#}"))),
        },
        ClientCommand::CloseProject => {
            session.stop();
            session.project = None;
            session.project_path = None;
            session.pending_snapshot = None;
            session.documents.clear();
            session.lifecycle = LifecycleState::Stopped;
            Ok(ServerResponse::ProjectClosed)
        }
        ClientCommand::Validate => {
            let Some(project) = session.project.as_ref() else {
                return Err((ErrorCode::ProjectNotOpen, "no project is open".into()));
            };
            let languages = loader
                .languages(&project.config.adapter.script)
                .map_err(|error| (ErrorCode::ValidationFailed, format!("{error:#}")))?;
            validate_project(&project.config, &project.content, &languages)
                .map(ServerResponse::Validation)
                .map_err(|error| (ErrorCode::ValidationFailed, format!("{error:#}")))
        }
        ClientCommand::BeginDocumentSnapshot {
            path,
            revision,
            total_bytes,
        } => begin_snapshot(session, path, *revision, *total_bytes),
        ClientCommand::AppendDocumentSnapshot {
            revision,
            offset,
            bytes,
        } => append_snapshot(session, *revision, *offset, bytes),
        ClientCommand::CommitDocumentSnapshot { revision } => {
            commit_snapshot(loader, session, *revision)
        }
        ClientCommand::ApplyDocumentPatch {
            path,
            base_revision,
            revision,
            start,
            end,
            replacement,
        } => apply_patch(
            loader,
            session,
            path,
            *base_revision,
            *revision,
            *start,
            *end,
            replacement,
        ),
        ClientCommand::StartPreview {
            transport,
            document_revision,
        } => {
            if session.project.is_none() {
                return Err((ErrorCode::ProjectNotOpen, "no project is open".into()));
            }
            if transport.session_generation != generation
                || transport.max_width != keine_core::DESIGN_WIDTH as u32
                || transport.max_height != keine_core::DESIGN_HEIGHT as u32
            {
                return Err((
                    ErrorCode::InvalidRequest,
                    "preview transport identity or dimensions are invalid".into(),
                ));
            }
            session.stop();
            session.transport = Some(transport.clone());
            session.document_revision = *document_revision;
            session.lifecycle = LifecycleState::Running;
            session.rebuild_runtime(loader).map_err(internal_error)?;
            Ok(ServerResponse::Lifecycle {
                state: session.lifecycle,
            })
        }
        ClientCommand::Pause => {
            if session.lifecycle != LifecycleState::Running || session.runtime.is_none() {
                return Err((
                    ErrorCode::InvalidRequest,
                    "pause requires a running preview".into(),
                ));
            }
            session.lifecycle = LifecycleState::Paused;
            if let Some(runtime) = session.runtime.as_mut() {
                super::preview::set_paused(runtime, true);
                runtime.update();
            }
            Ok(ServerResponse::Lifecycle {
                state: session.lifecycle,
            })
        }
        ClientCommand::Resume => {
            if session.lifecycle != LifecycleState::Paused || session.runtime.is_none() {
                return Err((
                    ErrorCode::InvalidRequest,
                    "resume requires a paused preview".into(),
                ));
            }
            session.lifecycle = LifecycleState::Running;
            if let Some(runtime) = session.runtime.as_mut() {
                super::preview::set_paused(runtime, false);
                runtime.update();
            }
            Ok(ServerResponse::Lifecycle {
                state: session.lifecycle,
            })
        }
        ClientCommand::Stop => {
            session.stop();
            Ok(ServerResponse::Lifecycle {
                state: session.lifecycle,
            })
        }
        ClientCommand::Input {
            document_revision,
            event,
        } => {
            require_revision(session, *document_revision)?;
            let Some(runtime) = session.runtime.as_mut() else {
                return Err((ErrorCode::InvalidRequest, "preview is not running".into()));
            };
            if session.lifecycle != LifecycleState::Running {
                return Err((ErrorCode::InvalidRequest, "preview is paused".into()));
            }
            super::preview::queue_input(runtime, *event);
            Ok(ServerResponse::InputAccepted {
                document_revision: *document_revision,
            })
        }
        ClientCommand::SetExecutionCursor {
            document_revision,
            path,
            line,
            column,
        } => {
            require_revision(session, *document_revision)?;
            let Some(runtime) = session.runtime.as_mut() else {
                return Err((ErrorCode::InvalidRequest, "preview is not running".into()));
            };
            if !super::preview::seek_source(runtime, path, *line) {
                return Err((
                    ErrorCode::InvalidRequest,
                    "source position does not resolve to an executable action".into(),
                ));
            }
            Ok(ServerResponse::SourceLocation {
                document_revision: *document_revision,
                path: path.clone(),
                line: *line,
                column: *column,
            })
        }
        ClientCommand::Ping => Ok(ServerResponse::Pong),
        ClientCommand::Shutdown => Ok(ServerResponse::Bye),
    }
}

fn begin_snapshot(
    session: &mut Session,
    path: &Path,
    revision: u64,
    total_bytes: u32,
) -> Result<ServerResponse, (ErrorCode, String)> {
    if session.project.is_none() {
        return Err((ErrorCode::ProjectNotOpen, "no project is open".into()));
    }
    let relative = confined_script_path(path).ok_or_else(|| {
        (
            ErrorCode::InvalidRequest,
            "preview snapshots must target a confined scripts/*.shou path".into(),
        )
    })?;
    let total_bytes = total_bytes as usize;
    if total_bytes > MAX_DOCUMENT_BYTES {
        return Err((
            ErrorCode::InvalidRequest,
            "preview source exceeds the 1 MiB document limit".into(),
        ));
    }
    session.pending_snapshot = Some(PendingSnapshot {
        path: relative,
        revision,
        total_bytes,
        bytes: Vec::with_capacity(total_bytes),
    });
    Ok(ServerResponse::SnapshotApplied {
        document_revision: revision,
    })
}

fn append_snapshot(
    session: &mut Session,
    revision: u64,
    offset: u32,
    bytes: &[u8],
) -> Result<ServerResponse, (ErrorCode, String)> {
    let Some(pending) = session.pending_snapshot.as_mut() else {
        return Err((
            ErrorCode::InvalidRequest,
            "no document snapshot is in progress".into(),
        ));
    };
    if pending.revision != revision || pending.bytes.len() != offset as usize {
        return Err((
            ErrorCode::InvalidRequest,
            "snapshot revision or chunk offset does not match".into(),
        ));
    }
    if pending.bytes.len().saturating_add(bytes.len()) > pending.total_bytes {
        return Err((
            ErrorCode::InvalidRequest,
            "snapshot chunks exceed the declared document length".into(),
        ));
    }
    pending.bytes.extend_from_slice(bytes);
    Ok(ServerResponse::SnapshotApplied {
        document_revision: revision,
    })
}

fn commit_snapshot(
    loader: &LoaderRegistry,
    session: &mut Session,
    revision: u64,
) -> Result<ServerResponse, (ErrorCode, String)> {
    let pending = session.pending_snapshot.take().ok_or_else(|| {
        (
            ErrorCode::InvalidRequest,
            "no document snapshot is in progress".into(),
        )
    })?;
    if pending.revision != revision || pending.bytes.len() != pending.total_bytes {
        return Err((
            ErrorCode::InvalidRequest,
            "snapshot is incomplete or belongs to another revision".into(),
        ));
    }
    if revision <= session.document_revision {
        return Err((
            ErrorCode::StaleRevision,
            "snapshot revision must advance the authoring document".into(),
        ));
    }
    std::str::from_utf8(&pending.bytes).map_err(|_| {
        (
            ErrorCode::InvalidRequest,
            "preview source snapshot must be UTF-8".into(),
        )
    })?;
    session
        .overlay
        .write_script(&pending.path, &pending.bytes)
        .map_err(internal_error)?;
    session
        .documents
        .insert(pending.path.clone(), pending.bytes);
    session.document_revision = revision;
    if session.runtime.is_some() {
        session.rebuild_runtime(loader).map_err(internal_error)?;
    }
    Ok(ServerResponse::SnapshotApplied {
        document_revision: revision,
    })
}

#[allow(clippy::too_many_arguments)]
fn apply_patch(
    loader: &LoaderRegistry,
    session: &mut Session,
    path: &Path,
    base_revision: u64,
    revision: u64,
    start: u32,
    end: u32,
    replacement: &[u8],
) -> Result<ServerResponse, (ErrorCode, String)> {
    require_revision(session, base_revision)?;
    if revision <= base_revision {
        return Err((
            ErrorCode::StaleRevision,
            "patch revision must advance the authoring document".into(),
        ));
    }
    let path = confined_script_path(path).ok_or_else(|| {
        (
            ErrorCode::InvalidRequest,
            "preview patches must target a confined scripts/*.shou path".into(),
        )
    })?;
    let document = session.documents.get_mut(&path).ok_or_else(|| {
        (
            ErrorCode::InvalidRequest,
            "a full snapshot is required before applying a document patch".into(),
        )
    })?;
    let range = start as usize..end as usize;
    let document_text = std::str::from_utf8(document).map_err(|_| {
        (
            ErrorCode::Internal,
            "stored preview snapshot is not valid UTF-8".into(),
        )
    })?;
    if range.start > range.end
        || range.end > document.len()
        || !document_text.is_char_boundary(range.start)
        || !document_text.is_char_boundary(range.end)
        || std::str::from_utf8(replacement).is_err()
    {
        return Err((
            ErrorCode::InvalidRequest,
            "preview patch range or UTF-8 replacement is invalid".into(),
        ));
    }
    let new_len = document
        .len()
        .saturating_sub(range.len())
        .saturating_add(replacement.len());
    if new_len > MAX_DOCUMENT_BYTES {
        return Err((
            ErrorCode::InvalidRequest,
            "patched preview source exceeds the 1 MiB document limit".into(),
        ));
    }
    document.splice(range, replacement.iter().copied());
    session
        .overlay
        .write_script(&path, document)
        .map_err(internal_error)?;
    session.document_revision = revision;
    if session.runtime.is_some() {
        session.rebuild_runtime(loader).map_err(internal_error)?;
    }
    Ok(ServerResponse::SnapshotApplied {
        document_revision: revision,
    })
}

fn confined_script_path(path: &Path) -> Option<PathBuf> {
    if path.is_absolute()
        || path.extension().and_then(|extension| extension.to_str()) != Some("shou")
    {
        return None;
    }
    let mut components = path.components();
    if components.next() != Some(Component::Normal("scripts".as_ref())) {
        return None;
    }
    if components.any(|component| !matches!(component, Component::Normal(_))) {
        return None;
    }
    Some(path.to_owned())
}

fn require_revision(session: &Session, document_revision: u64) -> Result<(), (ErrorCode, String)> {
    if document_revision != session.document_revision {
        return Err((
            ErrorCode::StaleRevision,
            format!(
                "preview is at document revision {}, not {document_revision}",
                session.document_revision
            ),
        ));
    }
    Ok(())
}

fn internal_error(error: impl std::fmt::Display) -> (ErrorCode, String) {
    (ErrorCode::Internal, error.to_string())
}

fn send(
    stream: &mut TcpStream,
    request: &ClientMessage,
    response: ServerResponse,
) -> io::Result<()> {
    write_message(
        stream,
        &ServerMessage {
            generation: request.generation,
            request_id: request.request_id,
            response,
        },
    )
}

fn send_error(
    stream: &mut TcpStream,
    request: &ClientMessage,
    code: ErrorCode,
    message: &str,
) -> io::Result<()> {
    send(
        stream,
        request,
        ServerResponse::Error {
            code,
            message: message.into(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_snapshot_paths_are_confined_to_native_scripts() {
        assert_eq!(
            confined_script_path(Path::new("scripts/main.shou")),
            Some(PathBuf::from("scripts/main.shou"))
        );
        assert!(confined_script_path(Path::new("main.shou")).is_none());
        assert!(confined_script_path(Path::new("scripts/../project.yaml")).is_none());
        assert!(confined_script_path(Path::new("scripts/main.txt")).is_none());
    }
}
